//! Unattended Host Architecture End-to-End Integration Test
//!
//! Validates the full unattended host lifecycle:
//! 1. Host identity and configuration persistence.
//! 2. Permanent PIN verification (PBKDF2-HMAC-SHA256, 600,000 iterations).
//! 3. Wrong PIN rejection.
//! 4. First-time pairing with permanent PIN returning auth token.
//! 5. Subsequent fast re-authentication using saved token (zero PIN interaction).
//! 6. Remote desktop streaming session and input dispatch.
//! 7. Host disable switch enforcement.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;

use ward_server::auth::{AuthManager, HostConfig, AUTH_METHOD_PIN, AUTH_METHOD_TOKEN};
use ward_server::capture::pipewire_capture::{CapturedFrame, FrameFormat};
use ward_server::identity::HostIdentity;
use ward_server::network::{WardServer, handle_client_session_with_sink};
use ward_server::protocol::{self, Message, MessageType};

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

fn generate_test_frame(width: u32, height: u32) -> CapturedFrame {
    CapturedFrame {
        data: vec![0x7F; (width * height * 4) as usize],
        width,
        height,
        stride: width * 4,
        format: FrameFormat::BGRx,
        sequence: 1,
    }
}

#[tokio::test]
async fn test_unattended_host_full_lifecycle() {
    println!("============================================================");
    println!("  WARD UNATTENDED HOST ARCHITECTURE INTEGRATION TEST        ");
    println!("============================================================");

    // 1. Setup Isolated Temporary Config & Host Identity & Permanent PIN
    let temp_config_dir = tempfile::tempdir().expect("Failed to create temp config dir");
    let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp_config_dir.path());
    }

    let identity = HostIdentity::load_or_create(None).expect("Failed to load host identity");
    assert!(!identity.device_id.is_empty());
    println!("1. Host Identity verified: '{}' (ID: {}...)", identity.device_name, &identity.device_id[..16]);

    let permanent_pin = "842195";
    let _ = HostConfig::create_or_update(permanent_pin).expect("Failed to set permanent PIN");
    let mut auth_mgr = AuthManager::load_default().expect("Failed to load AuthManager");
    assert!(auth_mgr.host_config().map(|c| c.has_pin()).unwrap_or(false));
    println!("2. Permanent PIN configured with PBKDF2-HMAC-SHA256 verifier");

    // 2. Start WARD Server on ephemeral port
    let server = WardServer::bind(0).await.expect("Failed to bind server");
    let local_addr = server.local_addr().expect("Failed to get local address");
    let port = local_addr.port();
    let server_running = Arc::new(AtomicBool::new(true));
    let server_running_clone = server_running.clone();
    let active_client_tx: Arc<std::sync::Mutex<Option<mpsc::Sender<CapturedFrame>>>> =
        Arc::new(std::sync::Mutex::new(None));
    let act_clone = active_client_tx.clone();

    // Spawn server accept loop
    tokio::spawn(async move {
        while server_running_clone.load(Ordering::Relaxed) {
            if let Ok((tls_stream, peer_addr)) = server.accept().await {
                let s_running = Arc::new(AtomicBool::new(true));
                let (frame_tx, frame_rx) = mpsc::channel(4);
                let _ = frame_tx.send(generate_test_frame(1280, 720)).await;
                *act_clone.lock().unwrap() = Some(frame_tx);

                let act_inner = act_clone.clone();
                tokio::spawn(async move {
                    let _ = handle_client_session_with_sink(
                        tls_stream,
                        peer_addr,
                        frame_rx,
                        None,
                        s_running,
                    ).await;
                    *act_inner.lock().unwrap() = None;
                });
            }
        }
    });

    let client_device_id = [0x55u8; 32];
    let client_dev_hex: String = client_device_id.iter().map(|b| format!("{:02x}", b)).collect();
    let client_name = "Pixel 8 Pro (Unattended Test)";

    // Ensure client starts in unpaired state for test
    let _ = auth_mgr.remove_authorized_client(&client_dev_hex);

    let tls_config = create_client_tls_config();
    let connector = TlsConnector::from(tls_config.clone());
    let server_name = ServerName::try_from("localhost").unwrap().to_owned();

    // -------------------------------------------------------------------------
    // TEST 3: Wrong PIN Rejection
    // -------------------------------------------------------------------------
    {
        let tcp = TcpStream::connect(format!("127.0.0.1:{}", port)).await.unwrap();
        let mut client_stream = connector.connect(server_name.clone(), tcp).await.unwrap();

        let init = Message::ConnectionInit {
            protocol_version: 1,
            client_type: 1,
            device_id: client_device_id,
            device_name: client_name.to_string(),
        };
        protocol::write_message(&mut client_stream, &init, 1).await.unwrap();

        let (_h, auth_req) = protocol::read_message(&mut client_stream).await.unwrap();
        match auth_req {
            Message::AuthRequest { auth_method, .. } => assert_eq!(auth_method, AUTH_METHOD_PIN),
            other => panic!("Expected AuthRequest, got {:?}", other),
        }

        // Send WRONG PIN
        let bad_resp = Message::AuthResponse {
            auth_method: AUTH_METHOD_PIN,
            response: b"000000".to_vec(),
        };
        protocol::write_message(&mut client_stream, &bad_resp, 2).await.unwrap();

        let (_h, rej) = protocol::read_message(&mut client_stream).await.unwrap();
        match rej {
            Message::SessionRejected { reason_code, .. } => {
                assert_eq!(reason_code, 0x0001);
                println!("3. Wrong PIN correctly rejected (reason_code=0x0001) ✅");
            }
            other => panic!("Expected SessionRejected, got {:?}", other),
        }
    }

    // -------------------------------------------------------------------------
    // TEST 4: First-Time Pairing with Permanent PIN -> Receive Auth Token
    // -------------------------------------------------------------------------
    let saved_token: String;
    {
        let tcp = TcpStream::connect(format!("127.0.0.1:{}", port)).await.unwrap();
        let mut client_stream = connector.connect(server_name.clone(), tcp).await.unwrap();

        let init = Message::ConnectionInit {
            protocol_version: 1,
            client_type: 1,
            device_id: client_device_id,
            device_name: client_name.to_string(),
        };
        protocol::write_message(&mut client_stream, &init, 1).await.unwrap();

        let (_h, auth_req) = protocol::read_message(&mut client_stream).await.unwrap();
        match auth_req {
            Message::AuthRequest { auth_method, .. } => assert_eq!(auth_method, AUTH_METHOD_PIN),
            other => panic!("Expected AuthRequest, got {:?}", other),
        }

        // Send CORRECT permanent PIN
        let good_resp = Message::AuthResponse {
            auth_method: AUTH_METHOD_PIN,
            response: permanent_pin.as_bytes().to_vec(),
        };
        protocol::write_message(&mut client_stream, &good_resp, 2).await.unwrap();

        let (_h, accepted) = protocol::read_message(&mut client_stream).await.unwrap();
        match accepted {
            Message::SessionAccepted { session_id, server_device_name, auth_token, .. } => {
                assert_eq!(session_id, 1);
                assert_eq!(server_device_name, identity.device_name);
                let tok = auth_token.expect("Auth token must be returned upon first-time PIN pairing");
                assert!(!tok.is_empty());
                saved_token = tok;
                println!("4. Permanent PIN verified successfully! SessionAccepted received with persistent token ✅");
            }
            other => panic!("Expected SessionAccepted, got {:?}", other),
        }

        // Read initial VIDEO_CONFIG
        let (_h, v_config) = protocol::read_message(&mut client_stream).await.unwrap();
        match v_config {
            Message::VideoConfig { width, height, fps_target, .. } => {
                assert!(width > 0 && height > 0);
                assert_eq!(fps_target, 45);
                println!("5. Video stream established: {}x{} @ {}fps ✅", width, height, fps_target);
            }
            other => panic!("Expected VideoConfig, got {:?}", other),
        }

        // Test remote input event
        let mouse_event = Message::MouseMove { x: 100.0, y: 200.0 };
        protocol::write_message(&mut client_stream, &mouse_event, 3).await.unwrap();

        // Disconnect first session
        let disc = Message::Disconnect;
        let _ = protocol::write_message(&mut client_stream, &disc, 4).await;
    }

    // -------------------------------------------------------------------------
    // TEST 5: Subsequent Re-Authentication via Saved Token (Zero PIN Interaction)
    // -------------------------------------------------------------------------
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let tcp = TcpStream::connect(format!("127.0.0.1:{}", port)).await.unwrap();
        let mut client_stream = connector.connect(server_name.clone(), tcp).await.unwrap();

        let init = Message::ConnectionInit {
            protocol_version: 1,
            client_type: 1,
            device_id: client_device_id,
            device_name: client_name.to_string(),
        };
        protocol::write_message(&mut client_stream, &init, 1).await.unwrap();

        // Server challenges with TOKEN auth because client is already paired!
        let (_h, auth_req) = protocol::read_message(&mut client_stream).await.unwrap();
        match auth_req {
            Message::AuthRequest { auth_method, .. } => {
                assert_eq!(auth_method, AUTH_METHOD_TOKEN);
                println!("6. Server challenged paired client with TOKEN auth (zero PIN required) ✅");
            }
            other => panic!("Expected AuthRequest for token, got {:?}", other),
        }

        // Respond with saved token
        let token_resp = Message::AuthResponse {
            auth_method: AUTH_METHOD_TOKEN,
            response: saved_token.into_bytes(),
        };
        protocol::write_message(&mut client_stream, &token_resp, 2).await.unwrap();

        let (_h, accepted) = protocol::read_message(&mut client_stream).await.unwrap();
        assert_eq!(accepted.msg_type(), MessageType::SessionAccepted);
        println!("7. Reconnection authenticated instantly via saved token! ✅");
    }

    // -------------------------------------------------------------------------
    // TEST 6: Host Disable Switch (Administrative Lockout)
    // -------------------------------------------------------------------------
    {
        let mut config = HostConfig::load_default().unwrap();
        config.host_enabled = false;
        config.save_default().unwrap();
        println!("8. WARD Host administrative disable switch toggled: host_enabled=false");

        let tcp = TcpStream::connect(format!("127.0.0.1:{}", port)).await.unwrap();
        let mut client_stream = connector.connect(server_name.clone(), tcp).await.unwrap();

        let init = Message::ConnectionInit {
            protocol_version: 1,
            client_type: 1,
            device_id: client_device_id,
            device_name: client_name.to_string(),
        };
        protocol::write_message(&mut client_stream, &init, 1).await.unwrap();

        let (_h, rej) = protocol::read_message(&mut client_stream).await.unwrap();
        match rej {
            Message::SessionRejected { reason_code, .. } => {
                assert_eq!(reason_code, 0x0003); // Host disabled
                println!("9. Disabled host strictly rejected incoming connection (reason_code=0x0003) ✅");
            }
            other => panic!("Expected SessionRejected for disabled host, got {:?}", other),
        }

        // Re-enable host
        let mut config = HostConfig::load_default().unwrap();
        config.host_enabled = true;
        config.save_default().unwrap();
        println!("10. Host re-enabled successfully ✅");
    }

    server_running.store(false, Ordering::Relaxed);
    unsafe {
        if let Some(prev) = prev_xdg {
            std::env::set_var("XDG_CONFIG_HOME", prev);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }
    println!("============================================================");
    println!("  ALL UNATTENDED HOST ARCHITECTURE TESTS PASSED!            ");
    println!("============================================================");
}
