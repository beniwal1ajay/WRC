//! WARD Remote Desktop Session Management.
//!
//! Manages active client streaming sessions independently of the underlying transport.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::capture::pipewire_capture::CapturedFrame;
use crate::encoder::{EncoderSettings, VideoEncoder};
use crate::portal::remote_desktop::{ButtonState, RemoteDesktopPortal, buttons};
use crate::protocol::{self, Message};
use crate::transport::TransportStream;

/// Run a streaming remote desktop session over any abstract TransportStream.
pub async fn run_session(
    stream: Box<dyn TransportStream>,
    mut frame_rx: mpsc::Receiver<CapturedFrame>,
    portal: Option<Arc<tokio::sync::Mutex<RemoteDesktopPortal>>>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let transport_type = stream.transport_type();
    let peer_addr = stream.peer_addr().unwrap_or_else(|| "0.0.0.0:0".parse().unwrap());
    info!("Active remote desktop session started via {} with {}", transport_type, peer_addr);

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

    let (client_device_id_bytes, client_device_name) = match init_msg {
        Message::ConnectionInit { protocol_version, device_id, device_name, .. } => {
            info!(
                "Client connected: '{}' (protocol v{}) via {}",
                device_name, protocol_version, transport_type
            );
            (device_id, device_name)
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
    };

    // 2. Client Authentication & Pairing Verification
    let is_zero_device_id = client_device_id_bytes.iter().all(|&b| b == 0);
    if !is_zero_device_id {
        let device_id_hex: String = client_device_id_bytes.iter().map(|b| format!("{:02x}", b)).collect();
        if let Ok(mut auth_mgr) = crate::auth::AuthManager::load_default() {
            if auth_mgr.is_authorized(&device_id_hex) {
                // Client is already paired: verify auth token
                let auth_req = Message::AuthRequest {
                    auth_method: crate::auth::AUTH_METHOD_TOKEN,
                    challenge: b"WARD_TOKEN_CHALLENGE".to_vec(),
                };
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                protocol::write_message(&mut writer, &auth_req, seq).await
                    .context("Failed to send AuthRequest for token")?;

                let (_h, auth_resp) = tokio::time::timeout(
                    Duration::from_secs(10),
                    protocol::read_message(&mut reader),
                )
                .await
                .context("Timed out waiting for AuthResponse")?
                .context("Failed to read AuthResponse")?;

                let token_valid = match auth_resp {
                    Message::AuthResponse { auth_method, response } if auth_method == crate::auth::AUTH_METHOD_TOKEN => {
                        let token_str = String::from_utf8_lossy(&response);
                        auth_mgr.verify_token(&device_id_hex, token_str.trim())
                    }
                    _ => false,
                };

                if !token_valid {
                    warn!("Client '{}' failed token authentication", client_device_name);
                    let rejected = Message::SessionRejected {
                        reason_code: 0x0002,
                        reason_message: "Invalid authentication token".to_string(),
                    };
                    let seq = sequence.fetch_add(1, Ordering::Relaxed);
                    let _ = protocol::write_message(&mut writer, &rejected, seq).await;
                    return Ok(());
                }
                info!("Client '{}' successfully authenticated via saved token", client_device_name);
            } else {
                // Client is not yet paired: generate 6-digit PIN and challenge client
                let (pin, _token) = auth_mgr.generate_pairing_pin(&device_id_hex, &client_device_name);
                info!("============================================================");
                info!("🔑 NEW PAIRING REQUEST FROM: '{}'", client_device_name);
                info!("🔑 DEVICE ID: {}", device_id_hex);
                info!("🔑 PAIRING PIN: [{}]", pin);
                info!("============================================================");

                // Spawn non-blocking desktop notification
                let dev_name = client_device_name.clone();
                let pin_clone = pin.clone();
                std::thread::spawn(move || {
                    let _ = std::process::Command::new("notify-send")
                        .arg("-u")
                        .arg("critical")
                        .arg("-a")
                        .arg("WARD Remote Desktop")
                        .arg(format!("Pairing PIN for {}", dev_name))
                        .arg(format!("Enter PIN on Android: {}", pin_clone))
                        .status();
                });

                let auth_req = Message::AuthRequest {
                    auth_method: crate::auth::AUTH_METHOD_PIN,
                    challenge: b"WARD_PIN_CHALLENGE".to_vec(),
                };
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                protocol::write_message(&mut writer, &auth_req, seq).await
                    .context("Failed to send AuthRequest for PIN")?;

                let (_h, auth_resp) = tokio::time::timeout(
                    Duration::from_secs(60),
                    protocol::read_message(&mut reader),
                )
                .await
                .context("Timed out waiting for AuthResponse PIN")?
                .context("Failed to read AuthResponse PIN")?;

                let pin_res = match auth_resp {
                    Message::AuthResponse { auth_method, response } if auth_method == crate::auth::AUTH_METHOD_PIN => {
                        let pin_str = String::from_utf8_lossy(&response);
                        auth_mgr.verify_pairing_pin(&device_id_hex, &client_device_name, pin_str.trim())
                    }
                    _ => Err(anyhow::anyhow!("Unexpected response to PIN auth request")),
                };

                match pin_res {
                    Ok(_assigned_token) => {
                        info!("Client '{}' successfully paired with PIN!", client_device_name);
                    }
                    Err(e) => {
                        warn!("Client '{}' failed PIN verification: {}", client_device_name, e);
                        let rejected = Message::SessionRejected {
                            reason_code: 0x0001,
                            reason_message: format!("Pairing failed: {}", e),
                        };
                        let seq = sequence.fetch_add(1, Ordering::Relaxed);
                        let _ = protocol::write_message(&mut writer, &rejected, seq).await;
                        return Ok(());
                    }
                }
            }
        }
    }

    // 2. Accept session with persistent Host Identity
    let host_identity = crate::identity::HostIdentity::load_or_create(None)
        .unwrap_or_else(|_| crate::identity::HostIdentity {
            device_id: "0".repeat(64),
            device_name: crate::identity::DEFAULT_DEVICE_NAME.to_string(),
            public_key_hex: "0".repeat(64),
            secret_key_bytes: [0u8; 32],
        });

    let accepted = Message::SessionAccepted {
        session_id: 1,
        server_device_id: host_identity.public_key_bytes(),
        server_device_name: host_identity.device_name.clone(),
    };
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
    let (input_tx, mut input_rx) = mpsc::channel::<Message>(512);
    let (telemetry_tx, mut telemetry_rx) = mpsc::channel::<(u16, u16)>(64);
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<Message>(64);
    let outbound_tx_reader = outbound_tx.clone();
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
                    Message::Stats { fps, latency_ms, .. } => {
                        debug!("Client stats: {} fps, latency {}ms", fps, latency_ms);
                        let _ = telemetry_tx.try_send((latency_ms, fps));
                    }
                    Message::Ping { timestamp_ns } => {
                        debug!("Received Heartbeat Ping from client ({}ns)", timestamp_ns);
                        let _ = outbound_tx_reader.try_send(Message::Pong { timestamp_ns });
                    }
                    Message::Pong { timestamp_ns } => {
                        let now_ns = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64;
                        let rtt_ms = if now_ns > timestamp_ns {
                            ((now_ns - timestamp_ns) / 1_000_000) as u16
                        } else {
                            0
                        };
                        debug!("Received Heartbeat Pong from client, RTT: {}ms", rtt_ms);
                        let _ = telemetry_tx.try_send((rtt_ms, 0));
                    }
                    Message::Disconnect => {
                        info!("Client sent disconnect request");
                        running_reader.store(false, Ordering::Relaxed);
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
                    running_reader.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }
    });

    // Spawn input dispatcher task (injects events via RemoteDesktop portal)
    let portal_clone = portal.clone();
    let running_input = running.clone();
    let input_dispatcher = tokio::spawn(async move {
        let mut pressed_keys: std::collections::HashSet<u32> = std::collections::HashSet::new();

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
                                pressed_keys.insert(keycode);
                                if (keycode & 0x8000_0000) != 0 || keycode > 0xFFF {
                                    let keysym = (keycode & 0x7FFF_FFFF) as i32;
                                    let _ = p.notify_keyboard_keysym(keysym, ButtonState::Pressed).await;
                                } else {
                                    let _ = p.notify_keyboard_keycode(keycode as i32, ButtonState::Pressed).await;
                                }
                            }
                            Message::KeyUp { keycode } => {
                                pressed_keys.remove(&keycode);
                                if (keycode & 0x8000_0000) != 0 || keycode > 0xFFF {
                                    let keysym = (keycode & 0x7FFF_FFFF) as i32;
                                    let _ = p.notify_keyboard_keysym(keysym, ButtonState::Released).await;
                                } else {
                                    let _ = p.notify_keyboard_keycode(keycode as i32, ButtonState::Released).await;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                None => break,
            }
        }

        // Safety cleanup: release any keys still marked as pressed
        if let Some(ref p_lock) = portal_clone {
            let p = p_lock.lock().await;
            for keycode in pressed_keys {
                if (keycode & 0x8000_0000) != 0 || keycode > 0xFFF {
                    let keysym = (keycode & 0x7FFF_FFFF) as i32;
                    let _ = p.notify_keyboard_keysym(keysym, ButtonState::Released).await;
                } else {
                    let _ = p.notify_keyboard_keycode(keycode as i32, ButtonState::Released).await;
                }
            }
        }
    });

    // 4. Video Streaming Loop: pipe captured frames -> encoder -> transport writer
    let initial_tier = match transport_type {
        crate::transport::TransportType::Lan => crate::transport::selector::ConnectionTier::LanDirect,
        crate::transport::TransportType::Vpn => crate::transport::selector::ConnectionTier::VpnDirect,
        crate::transport::TransportType::DirectP2p => crate::transport::selector::ConnectionTier::P2pIce,
        crate::transport::TransportType::Relay => crate::transport::selector::ConnectionTier::TurnRelay,
    };
    let mut rate_controller = crate::encoder::adaptive::AdaptiveVideoController::new(initial_tier);

    info!("Streaming video to client via {} (adaptive profile: {} fps, target {} kbps)...",
        transport_type, rate_controller.current_fps, rate_controller.current_bitrate_bps / 1000);
    let mut frames_sent: u64 = 0;

    while running.load(Ordering::Relaxed) {
        tokio::select! {
            biased;

            outbound_opt = outbound_rx.recv() => {
                match outbound_opt {
                    Some(msg) => {
                        let seq = sequence.fetch_add(1, Ordering::Relaxed);
                        if let Err(e) = protocol::write_message(&mut writer, &msg, seq).await {
                            warn!("Failed to write outbound message to client: {:#}", e);
                            break;
                        }
                    }
                    None => {
                        // Channel closed
                    }
                }
            }

            telemetry_opt = telemetry_rx.recv() => {
                match telemetry_opt {
                    Some((latency_ms, client_fps)) => {
                        rate_controller.on_network_feedback(latency_ms, client_fps);
                    }
                    None => {
                        break;
                    }
                }
            }

            frame_opt = frame_rx.recv() => {
                match frame_opt {
                    Some(frame) => {
                        let is_keyframe_req = keyframe_requested.swap(false, Ordering::Relaxed);
                        if is_keyframe_req {
                            encoder.force_keyframe();
                            rate_controller.on_loss_event();
                        }

                        if !is_keyframe_req && !rate_controller.should_encode_frame() {
                            continue; // Pace frame transmission according to dynamic FPS target
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

    info!("Session ended via {}. Total frames streamed: {}", transport_type, frames_sent);
    running.store(false, Ordering::Relaxed);
    let _ = input_dispatcher.await;
    let _ = reader_handle.await;

    Ok(())
}
