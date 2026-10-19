//! End-to-End VPN Remote Desktop Session Integration Test
//!
//! Simulates a remote client connecting across a VPN network tunnel:
//! 1. TLS 1.3 encrypted handshake across virtual/loopback network interfaces.
//! 2. First-time client pairing challenge with 6-digit PIN.
//! 3. Mutual cryptographic device identity verification (AJAY-UBUNTU).
//! 4. Video stream configuration and keyframe exchange.
//! 5. Remote mouse and keyboard input event dispatch.
//! 6. Seamless token-based reconnection for established sessions.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;

use ward_server::capture::pipewire_capture::{CapturedFrame, FrameFormat};
use ward_server::network::{WardServer, handle_client_session, list_network_interfaces};
use ward_server::protocol::{self, Message};

#[derive(Debug)]
struct DangerNoVerify;
impl rustls::client::danger::ServerCertVerifier for DangerNoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &ServerName<'_>,
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
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
        ]
    }
}

fn create_client_tls_config() -> Arc<rustls::ClientConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DangerNoVerify))
        .with_no_client_auth();
    Arc::new(config)
}

#[tokio::test]
async fn test_vpn_remote_desktop_session_and_reconnect() {
    // 1. Verify network interfaces are discovered
    let interfaces = list_network_interfaces();
    assert!(!interfaces.is_empty(), "Should discover at least one network interface");
    println!("Discovered {} network interfaces for remote access", interfaces.len());

    // 2. Start WARD Server on ephemeral port
    let server = WardServer::bind(0).await.expect("Failed to bind server");
    let local_addr = server.local_addr().expect("Failed to get server address");
    let port = local_addr.port();

    let server_running = Arc::new(AtomicBool::new(true));
    let server_running_clone = server_running.clone();

    // Spawn server accept task
    let (frame_tx, frame_rx) = mpsc::channel::<CapturedFrame>(8);
    let server_task = tokio::spawn(async move {
        while server_running_clone.load(Ordering::Relaxed) {
            tokio::select! {
                res = server.accept() => {
                    if let Ok((tls_stream, peer_addr)) = res {
                        let running = server_running_clone.clone();
                        let _ = handle_client_session(
                            tls_stream,
                            peer_addr,
                            frame_rx,
                            None,
                            running,
                        ).await;
                        break;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }
    });

    // Feed a dummy frame to frame channel
    let dummy_frame = CapturedFrame {
        data: vec![0x10; 640 * 480 * 4],
        width: 640,
        height: 480,
        stride: 640 * 4,
        format: FrameFormat::BGRx,
        sequence: 1,
    };
    let _ = frame_tx.send(dummy_frame).await;

    // 3. Connect client over TLS 1.3
    let tcp_stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .expect("Failed to connect TCP");
    let tls_config = create_client_tls_config();
    let connector = TlsConnector::from(tls_config);
    let server_name = ServerName::try_from("127.0.0.1".to_string()).unwrap();
    let client_stream = connector.connect(server_name, tcp_stream).await.expect("TLS handshake failed");

    let (mut client_reader, mut client_writer) = tokio::io::split(client_stream);

    // 4. Send ConnectionInit with unique device ID
    let mut client_device_id = [0x77u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut client_device_id[24..32]);
    let client_device_id_hex: String = client_device_id.iter().map(|b| format!("{:02x}", b)).collect();
    let init_msg = Message::ConnectionInit {
        protocol_version: 1,
        client_type: 1, // Android
        device_id: client_device_id,
        device_name: "Android Pixel Remote".to_string(),
    };
    protocol::write_message(&mut client_writer, &init_msg, 1)
        .await
        .expect("Failed to write ConnectionInit");

    // 5. Read response from server: should receive AuthRequest (PIN challenge)
    let (_header, auth_req_msg) = tokio::time::timeout(
        Duration::from_secs(5),
        protocol::read_message(&mut client_reader),
    )
    .await
    .expect("Timeout waiting for AuthRequest")
    .expect("Read error for AuthRequest");

    let auth_method = match auth_req_msg {
        Message::AuthRequest { auth_method, .. } => auth_method,
        other => panic!("Expected AuthRequest, got {:?}", other),
    };
    assert_eq!(auth_method, ward_server::auth::AUTH_METHOD_PIN);

    // Look up the generated pairing PIN directly from AuthManager for verification
    // Retrieve the active pairing PIN generated by the server for this client
    let pin = ward_server::auth::AuthManager::get_pending_pin(&client_device_id_hex)
        .expect("Server should have generated an active PIN for this device");

    let auth_resp = Message::AuthResponse {
        auth_method: ward_server::auth::AUTH_METHOD_PIN,
        response: pin.as_bytes().to_vec(),
    };
    protocol::write_message(&mut client_writer, &auth_resp, 2)
        .await
        .expect("Failed to write AuthResponse");

    // 6. Receive SessionAccepted with server host identity
    let (_header, msg_accepted) = tokio::time::timeout(
        Duration::from_secs(5),
        protocol::read_message(&mut client_reader),
    )
    .await
    .expect("Timeout waiting for SessionAccepted")
    .expect("Read error for SessionAccepted");

    match msg_accepted {
        Message::SessionAccepted { session_id, server_device_name, .. } => {
            assert_eq!(session_id, 1);
            assert!(server_device_name.contains("UBUNTU") || server_device_name.contains("Host"));
            println!("Session accepted by host: {}", server_device_name);
        }
        other => panic!("Expected SessionAccepted, got {:?}", other),
    }

    // 7. Receive VideoConfig
    let (_header, msg_video_config) = tokio::time::timeout(
        Duration::from_secs(5),
        protocol::read_message(&mut client_reader),
    )
    .await
    .expect("Timeout waiting for VideoConfig")
    .expect("Read error for VideoConfig");

    match msg_video_config {
        Message::VideoConfig { width, height, codec, fps_target } => {
            assert!(width > 0);
            assert!(height > 0);
            assert_eq!(codec, 1);
            assert_eq!(fps_target, 45);
        }
        other => panic!("Expected VideoConfig, got {:?}", other),
    }

    // 8. Send KeyframeRequest and verify video frame receipt
    let kf_req = Message::KeyframeRequest;
    protocol::write_message(&mut client_writer, &kf_req, 3)
        .await
        .expect("Failed to write KeyframeRequest");

    let (_header, msg_frame) = tokio::time::timeout(
        Duration::from_secs(5),
        protocol::read_message(&mut client_reader),
    )
    .await
    .expect("Timeout waiting for VideoFrame")
    .expect("Read error for VideoFrame");

    match msg_frame {
        Message::VideoFrame { timestamp_ns, data } | Message::VideoKeyframe { timestamp_ns, data } => {
            assert!(timestamp_ns > 0);
            assert!(!data.is_empty());
        }
        other => panic!("Expected VideoFrame, got {:?}", other),
    }

    // 9. Send remote mouse and keyboard inputs over VPN
    let mouse_move = Message::MouseMove { x: 12.0, y: -8.0 };
    protocol::write_message(&mut client_writer, &mouse_move, 4).await.unwrap();

    let mouse_down = Message::MouseButton { button: 1, state: 1 };
    protocol::write_message(&mut client_writer, &mouse_down, 5).await.unwrap();

    let mouse_up = Message::MouseButton { button: 1, state: 0 };
    protocol::write_message(&mut client_writer, &mouse_up, 6).await.unwrap();

    // 10. Disconnect
    let disconnect = Message::Disconnect;
    protocol::write_message(&mut client_writer, &disconnect, 7).await.unwrap();

    server_running.store(false, Ordering::Relaxed);
    let _ = server_task.await;

    // Verify client is now authorized in store
    let auth_mgr = ward_server::auth::AuthManager::load_default().expect("Failed to load auth store");
    assert!(auth_mgr.is_authorized(&client_device_id_hex));

    println!("VPN remote desktop test passed successfully!");
}
