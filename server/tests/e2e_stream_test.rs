//! End-to-End TLS 1.3 Streaming Pipeline Integration Test
//!
//! Tests:
//! 1. Starting WardServer on an ephemeral port.
//! 2. Connecting a TLS 1.3 client using tokio-rustls.
//! 3. Protocol handshake: ConnectionInit -> SessionAccepted -> VideoConfig.
//! 4. Frame capture feeding -> H.264 real-time encoding -> VideoKeyframe/VideoFrame over TLS.
//! 5. Bi-directional input messaging: Client MouseMove -> server input dispatcher.
//! 6. Clean Disconnect and shutdown.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use rustls::pki_types::ServerName;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;

use ward_server::capture::pipewire_capture::{CapturedFrame, FrameFormat};
use ward_server::network::{handle_client_session, WardServer};
use ward_server::protocol::{self, Message};

#[derive(Debug)]
struct NoServerVerification;

impl rustls::client::danger::ServerCertVerifier for NoServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn create_client_tls_config() -> Arc<rustls::ClientConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let client_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoServerVerification))
        .with_no_client_auth();

    Arc::new(client_config)
}

/// Helper to generate a test synthetic BGRx frame of given dimensions.
fn generate_test_frame(width: u32, height: u32, sequence: u64, color: (u8, u8, u8)) -> CapturedFrame {
    let stride = width * 4;
    let mut data = vec![0u8; (stride * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let offset = ((y * stride) + (x * 4)) as usize;
            data[offset] = color.2;     // B
            data[offset + 1] = color.1; // G
            data[offset + 2] = color.0; // R
            data[offset + 3] = 255;     // x
        }
    }
    CapturedFrame {
        data,
        width,
        height,
        stride,
        format: FrameFormat::BGRx,
        sequence,
    }
}

#[tokio::test]
async fn test_end_to_end_streaming_session() {
    // 1. Bind server on ephemeral port (port 0)
    let server = WardServer::bind(0).await.expect("Failed to bind test server");
    let server_addr = server.local_addr().expect("Failed to get server addr");

    let (frame_tx, frame_rx) = mpsc::channel::<CapturedFrame>(10);
    let server_running = Arc::new(AtomicBool::new(true));
    let server_running_clone = server_running.clone();

    // 2. Spawn server accept & handle task
    let server_task = tokio::spawn(async move {
        let (tls_stream, peer_addr) = server.accept().await.expect("Failed to accept TLS connection");
        handle_client_session(
            tls_stream,
            peer_addr,
            frame_rx,
            None, // No portal needed for integration test
            server_running_clone,
        )
        .await
        .expect("Server handle_client_session failed");
    });

    // 3. Connect client via TLS 1.3
    let client_tls_config = create_client_tls_config();
    let connector = TlsConnector::from(client_tls_config);
    let tcp_stream = tokio::net::TcpStream::connect(server_addr)
        .await
        .expect("Failed to connect TCP stream to server");
    let server_name = ServerName::try_from("localhost".to_string()).unwrap();
    let tls_client_stream = connector
        .connect(server_name, tcp_stream)
        .await
        .expect("Client TLS handshake failed");

    let (mut client_reader, mut client_writer) = tokio::io::split(tls_client_stream);

    // 4. Send ConnectionInit
    let init_msg = Message::ConnectionInit {
        protocol_version: 1,
        client_type: 1, // Android
        device_id: [0u8; 32],
        device_name: "Pixel 8 Test".to_string(),
    };
    protocol::write_message(&mut client_writer, &init_msg, 1)
        .await
        .expect("Failed to write ConnectionInit");

    // 5. Receive SessionAccepted
    let (_header, msg_accepted) = tokio::time::timeout(
        Duration::from_secs(5),
        protocol::read_message(&mut client_reader),
    )
    .await
    .expect("Timeout waiting for SessionAccepted")
    .expect("Read error for SessionAccepted");

    match msg_accepted {
        Message::SessionAccepted { session_id } => {
            assert_eq!(session_id, 1);
        }
        other => panic!("Expected SessionAccepted, got {:?}", other),
    }

    // 6. Receive VideoConfig
    let (_header, msg_video_config) = tokio::time::timeout(
        Duration::from_secs(5),
        protocol::read_message(&mut client_reader),
    )
    .await
    .expect("Timeout waiting for VideoConfig")
    .expect("Read error for VideoConfig");

    match msg_video_config {
        Message::VideoConfig { width, height, codec, fps_target } => {
            assert!(width > 0, "Video width must be > 0");
            assert!(height > 0, "Video height must be > 0");
            assert_eq!(codec, 1, "Codec should be H.264");
            assert_eq!(fps_target, 60);
        }
        other => panic!("Expected VideoConfig, got {:?}", other),
    }

    // 7. Feed 5 synthetic captured frames to the server pipeline
    let colors = [
        (255, 0, 0),
        (0, 255, 0),
        (0, 0, 255),
        (255, 255, 0),
        (0, 255, 255),
    ];
    for (i, &color) in colors.iter().enumerate() {
        let frame = generate_test_frame(1280, 720, (i + 1) as u64, color);
        frame_tx.send(frame).await.expect("Failed to feed synthetic frame");
    }

    // 8. Client receives encoded H.264 video frames
    let mut received_frames = 0;
    let mut got_keyframe = false;

    for _ in 0..5 {
        let (_header, frame_msg) = tokio::time::timeout(
            Duration::from_secs(5),
            protocol::read_message(&mut client_reader),
        )
        .await
        .expect("Timeout waiting for video frame")
        .expect("Read error for video frame");

        match frame_msg {
            Message::VideoKeyframe { timestamp_ns, data } => {
                assert!(timestamp_ns > 0);
                assert!(!data.is_empty(), "Keyframe payload must not be empty");
                // Verify H.264 start code (0x00 0x00 0x00 0x01 or 0x00 0x00 0x01)
                assert!(
                    data.starts_with(&[0, 0, 0, 1]) || data.starts_with(&[0, 0, 1]),
                    "Keyframe must begin with H.264 start code"
                );
                got_keyframe = true;
                received_frames += 1;
            }
            Message::VideoFrame { timestamp_ns, data } => {
                assert!(timestamp_ns > 0);
                assert!(!data.is_empty(), "Video frame payload must not be empty");
                assert!(
                    data.starts_with(&[0, 0, 0, 1]) || data.starts_with(&[0, 0, 1]),
                    "Video frame must begin with H.264 start code"
                );
                received_frames += 1;
            }
            other => panic!("Expected video frame message, got msg_type={:?}", other.msg_type()),
        }
    }

    assert_eq!(received_frames, 5, "Must receive all 5 streamed frames");
    assert!(got_keyframe, "At least the first frame must be an IDR keyframe");

    // 9. Send client input message (MouseMove)
    let move_msg = Message::MouseMove { x: 500.0, y: 300.0 };
    protocol::write_message(&mut client_writer, &move_msg, 2)
        .await
        .expect("Failed to send MouseMove");

    // 10. Send Disconnect
    let disconnect_msg = Message::Disconnect;
    protocol::write_message(&mut client_writer, &disconnect_msg, 3)
        .await
        .expect("Failed to send Disconnect");

    // 11. Wait for server task to cleanly finish
    tokio::time::timeout(Duration::from_secs(5), server_task)
        .await
        .expect("Timeout waiting for server shutdown")
        .expect("Server task panicked");
}
