//! WARD TLS 1.3 Network Server & Streaming Pipeline.
//!
//! Handles:
//! - TLS 1.3 encrypted TCP connection on port 9740
//! - Client handshake (ConnectionInit -> SessionAccepted)
//! - Real-time streaming of H.264 video frames
//! - Receiving and dispatching remote input messages (Mouse/Keyboard)

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, info, warn};

use crate::capture::pipewire_capture::CapturedFrame;
use crate::encoder::{EncoderSettings, VideoEncoder};
use crate::portal::remote_desktop::{ButtonState, RemoteDesktopPortal, buttons};
use crate::protocol::{self, Message};

/// Default TCP port for the WARD server.
pub const DEFAULT_PORT: u16 = 9740;

/// Create a TLS 1.3 server configuration with a self-signed X.509 certificate.
pub fn create_tls_config() -> Result<Arc<rustls::ServerConfig>> {
    // Ensure ring crypto provider is installed
    let _ = rustls::crypto::ring::default_provider().install_default();

    let subject_alt_names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "ward-server".to_string(),
    ];

    let certified_key = generate_simple_self_signed(subject_alt_names)
        .context("Failed to generate self-signed TLS certificate")?;

    let cert_der = CertificateDer::from(certified_key.cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified_key.signing_key.serialize_der(),
    ));

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .context("Failed to build rustls ServerConfig")?;

    Ok(Arc::new(server_config))
}

/// Run the WARD streaming server session for a single connected client.
pub async fn handle_client_session(
    stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    peer_addr: SocketAddr,
    mut frame_rx: mpsc::Receiver<CapturedFrame>,
    portal: Option<Arc<tokio::sync::Mutex<RemoteDesktopPortal>>>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    info!("TLS 1.3 handshake completed with {}", peer_addr);

    let (mut reader, mut writer) = tokio::io::split(stream);
    let sequence = Arc::new(AtomicU32::new(1));

    // 1. Initial Handshake: Read CONNECTION_INIT
    let (_header, init_msg) = tokio::time::timeout(
        Duration::from_secs(10),
        protocol::read_message(&mut reader),
    )
    .await
    .context("Timed out waiting for CONNECTION_INIT")?
    .context("Failed to read CONNECTION_INIT")?;

    match init_msg {
        Message::ConnectionInit { protocol_version, device_name, .. } => {
            info!(
                "Client connected: '{}' (protocol v{})",
                device_name, protocol_version
            );
        }
        other => {
            warn!("Expected ConnectionInit, received {:?}", other.msg_type());
            let err = Message::Error {
                code: 0x0004,
                message: "Expected ConnectionInit message".to_string(),
            };
            let seq = sequence.fetch_add(1, Ordering::Relaxed);
            let _ = protocol::write_message(&mut writer, &err, seq).await;
            return Ok(());
        }
    }

    // 2. Accept session
    let accepted = Message::SessionAccepted { session_id: 1 };
    let seq = sequence.fetch_add(1, Ordering::Relaxed);
    protocol::write_message(&mut writer, &accepted, seq).await
        .context("Failed to send SessionAccepted")?;

    // 3. Initialize H.264 Encoder
    let mut encoder = VideoEncoder::new(EncoderSettings::default())
        .context("Failed to initialize video encoder")?;

    // Send initial VIDEO_CONFIG
    let video_config = Message::VideoConfig {
        width: encoder.width() as u16,
        height: encoder.height() as u16,
        codec: 1, // H.264
        fps_target: 60,
    };
    let seq = sequence.fetch_add(1, Ordering::Relaxed);
    protocol::write_message(&mut writer, &video_config, seq).await
        .context("Failed to send VideoConfig")?;

    info!("Video stream configured: {}x{} @ 60fps (H.264)", encoder.width(), encoder.height());

    // Channel for client input events -> input dispatcher
    let (input_tx, mut input_rx) = mpsc::channel::<Message>(32);
    let keyframe_requested = Arc::new(AtomicBool::new(true)); // force keyframe on start
    let kf_req_clone = keyframe_requested.clone();
    let running_reader = running.clone();

    // Spawn reader task for client input messages
    let reader_handle = tokio::spawn(async move {
        while running_reader.load(Ordering::Relaxed) {
            match protocol::read_message(&mut reader).await {
                Ok((_header, msg)) => match msg {
                    Message::KeyframeRequest => {
                        debug!("Client requested keyframe");
                        kf_req_clone.store(true, Ordering::Relaxed);
                    }
                    Message::Disconnect => {
                        info!("Client sent disconnect request");
                        break;
                    }
                    input_event => {
                        if input_tx.send(input_event).await.is_err() {
                            break;
                        }
                    }
                },
                Err(e) => {
                    debug!("Client read error or disconnected: {:#}", e);
                    break;
                }
            }
        }
    });

    // Spawn input dispatcher task (injects events via RemoteDesktop portal)
    let portal_clone = portal.clone();
    let running_input = running.clone();
    let input_dispatcher = tokio::spawn(async move {
        while running_input.load(Ordering::Relaxed) {
            match input_rx.recv().await {
                Some(msg) => {
                    if let Some(ref p_lock) = portal_clone {
                        let p = p_lock.lock().await;
                        match msg {
                            Message::MouseMove { x, y } => {
                                let _ = p.notify_pointer_motion(x as f64, y as f64).await;
                            }
                            Message::MouseButton { button, state } => {
                                let btn_code = match button {
                                    1 => buttons::BTN_LEFT,
                                    2 => buttons::BTN_RIGHT,
                                    3 => buttons::BTN_MIDDLE,
                                    _ => buttons::BTN_LEFT,
                                };
                                let btn_state = if state == 1 { ButtonState::Pressed } else { ButtonState::Released };
                                let _ = p.notify_pointer_button(btn_code, btn_state).await;
                            }
                            Message::MouseScroll { dy, .. } => {
                                let steps = if dy > 0.0 { -1 } else { 1 };
                                let _ = p.notify_pointer_axis_discrete(0, steps).await;
                            }
                            Message::KeyDown { keycode } => {
                                let _ = p.notify_keyboard_keycode(keycode as i32, ButtonState::Pressed).await;
                            }
                            Message::KeyUp { keycode } => {
                                let _ = p.notify_keyboard_keycode(keycode as i32, ButtonState::Released).await;
                            }
                            _ => {}
                        }
                    }
                }
                None => break,
            }
        }
    });

    // 4. Video Streaming Loop: pipe captured frames -> encoder -> TLS socket
    info!("Streaming video to client...");
    let mut frames_sent: u64 = 0;

    while running.load(Ordering::Relaxed) {
        tokio::select! {
            frame_opt = frame_rx.recv() => {
                match frame_opt {
                    Some(frame) => {
                        if keyframe_requested.swap(false, Ordering::Relaxed) {
                            encoder.force_keyframe();
                        }

                        let now_ns = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64;

                        match encoder.encode_bgrx(
                            &frame.data,
                            frame.width as usize,
                            frame.height as usize,
                            frame.stride as usize,
                            now_ns,
                        ) {
                            Ok(packet) => {
                                let msg = if packet.is_keyframe {
                                    Message::VideoKeyframe {
                                        timestamp_ns: packet.timestamp_ns,
                                        data: packet.data,
                                    }
                                } else {
                                    Message::VideoFrame {
                                        timestamp_ns: packet.timestamp_ns,
                                        data: packet.data,
                                    }
                                };

                                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                                if let Err(e) = protocol::write_message(&mut writer, &msg, seq).await {
                                    warn!("Failed to send video frame to client: {:#}", e);
                                    break;
                                }

                                frames_sent += 1;
                                if frames_sent % 60 == 0 {
                                    debug!("Streamed {} frames to client", frames_sent);
                                }
                            }
                            Err(e) => {
                                warn!("Encode error: {:#}", e);
                            }
                        }
                    }
                    None => {
                        info!("Capture channel closed, stopping stream");
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if reader_handle.is_finished() {
                    info!("Client disconnected");
                    break;
                }
            }
        }
    }

    info!("Session ended. Total frames streamed: {}", frames_sent);
    running.store(false, Ordering::Relaxed);
    let _ = input_dispatcher.await;
    let _ = reader_handle.await;

    Ok(())
}

/// Standalone TLS Server Listener.
pub struct WardServer {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    #[allow(dead_code)]
    running: Arc<AtomicBool>,
}

impl WardServer {
    pub async fn bind(port: u16) -> Result<Self> {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = TcpListener::bind(addr).await
            .context(format!("Failed to bind TCP listener to {}", addr))?;

        let tls_config = create_tls_config()
            .context("Failed to configure TLS")?;
        let acceptor = TlsAcceptor::from(tls_config);

        info!("WARD Server listening on {} (TLS 1.3)", addr);

        Ok(Self {
            listener,
            acceptor,
            running: Arc::new(AtomicBool::new(true)),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.listener.local_addr().context("Failed to get local address")
    }

    pub async fn accept(&self) -> Result<(tokio_rustls::server::TlsStream<tokio::net::TcpStream>, SocketAddr)> {
        let (tcp_stream, peer_addr) = self.listener.accept().await
            .context("Failed to accept TCP connection")?;

        let tls_stream = self.acceptor.accept(tcp_stream).await
            .context(format!("TLS handshake failed with {}", peer_addr))?;

        Ok((tls_stream, peer_addr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_config_generation() {
        let config = create_tls_config();
        assert!(config.is_ok(), "TLS config generation should succeed: {:?}", config.err());
    }

    #[tokio::test]
    async fn test_server_bind() {
        let server = WardServer::bind(0).await.expect("Failed to bind to ephemeral port");
        let addr = server.local_addr().expect("Failed to get local addr");
        assert!(addr.port() > 0, "Server should be bound to a valid port");
    }
}

