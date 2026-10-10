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
use crate::portal::InputSink;
use crate::portal::remote_desktop::{ButtonState, RemoteDesktopPortal, buttons};
use crate::protocol::{self, Message};
use crate::transport::TransportStream;

/// Run a streaming remote desktop session over any abstract TransportStream with an InputSink.
pub async fn run_session(
    stream: Box<dyn TransportStream>,
    mut frame_rx: mpsc::Receiver<CapturedFrame>,
    input_sink: Option<InputSink>,
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

    let mut assigned_token = None;

    // 2. Client Authentication & Pairing Verification
    let is_zero_device_id = client_device_id_bytes.iter().all(|&b| b == 0);
    if !is_zero_device_id {
        let device_id_hex: String = client_device_id_bytes.iter().map(|b| format!("{:02x}", b)).collect();
        if let Ok(mut auth_mgr) = crate::auth::AuthManager::load_default() {
            // Check if host is enabled
            if let Some(config) = auth_mgr.host_config() {
                if !config.host_enabled {
                    warn!("[WARD AUTH] Host is disabled by administrator. Rejecting client '{}'", client_device_name);
                    let rejected = Message::SessionRejected {
                        reason_code: 0x0003,
                        reason_message: "WARD Host is disabled by administrator".to_string(),
                    };
                    let seq = sequence.fetch_add(1, Ordering::Relaxed);
                    let _ = protocol::write_message(&mut writer, &rejected, seq).await;
                    return Ok(());
                }
            }

            let mut authenticated = false;

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

                if token_valid {
                    info!("Client '{}' successfully authenticated via saved token", client_device_name);
                    authenticated = true;
                } else {
                    warn!("[WARD AUTH] Client '{}' failed token authentication; removing stale token and challenging for permanent PIN", client_device_name);
                    let _ = auth_mgr.remove_authorized_client(&device_id_hex);
                }
            }

            if !authenticated {
                // Client is not yet paired (or token was invalid): challenge client with permanent PIN (or test pairing PIN)
                let is_test_pending = crate::auth::AuthManager::get_pending_pin(&device_id_hex).is_some();
                if !is_test_pending {
                    let _ = auth_mgr.generate_pairing_pin(&device_id_hex, &client_device_name);
                }

                info!("[WARD AUTH] Client '{}' ({}) requested connection - challenging for PIN", client_device_name, device_id_hex);

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

                let provided_pin = match auth_resp {
                    Message::AuthResponse { auth_method, response } if auth_method == crate::auth::AUTH_METHOD_PIN => {
                        String::from_utf8_lossy(&response).trim().to_string()
                    }
                    _ => {
                        warn!("Client failed to provide PIN response");
                        let rejected = Message::SessionRejected {
                            reason_code: 0x0001,
                            reason_message: "Expected AuthResponse with permanent PIN".to_string(),
                        };
                        let seq = sequence.fetch_add(1, Ordering::Relaxed);
                        let _ = protocol::write_message(&mut writer, &rejected, seq).await;
                        return Ok(());
                    }
                };

                assigned_token = match auth_mgr.verify_permanent_pin(&device_id_hex, &client_device_name, &provided_pin) {
                    Ok(token) => {
                        info!("[WARD AUTH] Client '{}' ({}) successfully authenticated with permanent PIN", client_device_name, device_id_hex);
                        Some(token)
                    }
                    Err(e) => {
                        warn!("[WARD AUTH] Client '{}' failed permanent PIN verification: {:#}", client_device_name, e);
                        let rejected = Message::SessionRejected {
                            reason_code: 0x0001,
                            reason_message: format!("Authentication failed: {}", e),
                        };
                        let seq = sequence.fetch_add(1, Ordering::Relaxed);
                        let _ = protocol::write_message(&mut writer, &rejected, seq).await;
                        return Ok(());
                    }
                };
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
        auth_token: assigned_token,
    };
    let seq = sequence.fetch_add(1, Ordering::Relaxed);
    protocol::write_message(&mut writer, &accepted, seq).await
        .context("Failed to send SessionAccepted")?;

    // 3. Initialize Adaptive Rate Controller and H.264 Encoder
    let initial_tier = match transport_type {
        crate::transport::TransportType::Lan => crate::transport::selector::ConnectionTier::LanDirect,
        crate::transport::TransportType::Vpn => crate::transport::selector::ConnectionTier::VpnDirect,
        crate::transport::TransportType::DirectP2p => crate::transport::selector::ConnectionTier::P2pIce,
        crate::transport::TransportType::Relay => crate::transport::selector::ConnectionTier::TurnRelay,
    };
    let mut rate_controller = crate::encoder::adaptive::AdaptiveVideoController::new(initial_tier);

    // Default to Native 1080p (1920x1080) for sharp, clear desktop text rendering.
    // 720p can be requested via client VideoConfig message or WARD_STREAM_RES=720p
    let is_720p = matches!(
        std::env::var("WARD_STREAM_RES").as_deref(),
        Ok("720p")
    );
    let (stream_w, stream_h, initial_bitrate) = if is_720p {
        (1280, 720, 8_000_000)
    } else {
        (1920, 1080, 12_000_000)
    };
    rate_controller.current_bitrate_bps = initial_bitrate;

    let mut encoder = VideoEncoder::new(EncoderSettings {
        width: stream_w,
        height: stream_h,
        bitrate_bps: rate_controller.current_bitrate_bps,
        max_fps: rate_controller.current_fps as f32,
    }).context("Failed to initialize video encoder")?;

    // Send initial VIDEO_CONFIG to client
    let video_config = Message::VideoConfig {
        width: encoder.width() as u16,
        height: encoder.height() as u16,
        codec: 1, // H.264
        fps_target: rate_controller.current_fps as u16,
    };
    let seq = sequence.fetch_add(1, Ordering::Relaxed);
    protocol::write_message(&mut writer, &video_config, seq).await
        .context("Failed to send VideoConfig")?;

    info!(
        "Video stream configured: {}x{} @ {}fps (H.264, profile: {}, initial bitrate {} kbps)",
        encoder.width(), encoder.height(), rate_controller.current_fps,
        if is_720p { "Performance 720p" } else { "Native 1080p (Full HD)" },
        rate_controller.current_bitrate_bps / 1000
    );

    // Channel for client input events -> input dispatcher
    let (input_tx, mut input_rx) = mpsc::channel::<Message>(512);
    let (telemetry_tx, mut telemetry_rx) = mpsc::channel::<(u16, u16)>(64);
    let (reconfig_tx, mut reconfig_rx) = mpsc::channel::<(u16, u16, u16)>(16);
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
                    Message::VideoConfig { width, height, fps_target, .. } => {
                        info!("Client requested video stream change: {}x{} @ {}fps", width, height, fps_target);
                        let _ = reconfig_tx.try_send((width, height, fps_target));
                    }
                    Message::Ping { timestamp_ns } => {
                        let pong = Message::Pong { timestamp_ns };
                        let _ = outbound_tx_reader.try_send(pong);
                    }
                    Message::Pong { timestamp_ns } => {
                        let now_ns = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64;
                        let rtt_ms = if now_ns >= timestamp_ns {
                            ((now_ns - timestamp_ns) / 1_000_000) as u16
                        } else {
                            0
                        };
                        debug!("Received Pong from client, RTT: {}ms", rtt_ms);
                        let _ = telemetry_tx.try_send((rtt_ms, 0));
                    }
                    Message::Stats { fps, latency_ms, .. } => {
                        let _ = telemetry_tx.try_send((latency_ms, fps));
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

    // Spawn input dispatcher task (injects events via InputSink)
    let sink_clone = input_sink.clone();
    let running_input = running.clone();
    let input_dispatcher = tokio::spawn(async move {
        let mut pressed_keys: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut pending_msg: Option<Message> = None;

        while running_input.load(Ordering::Relaxed) {
            let msg = if let Some(m) = pending_msg.take() {
                m
            } else {
                match input_rx.recv().await {
                    Some(m) => m,
                    None => break,
                }
            };

            if let Some(ref sink) = sink_clone {
                match msg {
                    Message::MouseMove { mut x, mut y } => {
                        // Coalesce consecutive MouseMove messages to prevent D-Bus message queue saturation
                        while let Ok(next) = input_rx.try_recv() {
                            match next {
                                Message::MouseMove { x: nx, y: ny } => {
                                    x += nx;
                                    y += ny;
                                }
                                other => {
                                    pending_msg = Some(other);
                                    break;
                                }
                            }
                        }
                        let _ = sink.notify_pointer_motion(x as f64, y as f64).await;
                    }
                    Message::MouseMoveAbsolute { mut x, mut y } => {
                        // Coalesce consecutive MouseMoveAbsolute messages taking the latest target coordinate
                        while let Ok(next) = input_rx.try_recv() {
                            match next {
                                Message::MouseMoveAbsolute { x: nx, y: ny } => {
                                    x = nx;
                                    y = ny;
                                }
                                other => {
                                    pending_msg = Some(other);
                                    break;
                                }
                            }
                        }
                        let _ = sink.notify_pointer_motion_absolute(x as f64, y as f64).await;
                    }
                    Message::MouseButton { button, state } => {
                        let btn_code = match button {
                            1 => buttons::BTN_LEFT,
                            2 => buttons::BTN_RIGHT,
                            3 => buttons::BTN_MIDDLE,
                            _ => buttons::BTN_LEFT,
                        };
                        let btn_state = if state == 1 { ButtonState::Pressed } else { ButtonState::Released };
                        let _ = sink.notify_pointer_button(btn_code, btn_state).await;
                    }
                    Message::MouseScroll { dx, dy } => {
                        if dy.abs() > 0.001 {
                            // Axis 0 = Vertical scroll
                            let steps = if dy > 0.0 { -1 } else { 1 };
                            let _ = sink.notify_pointer_axis_discrete(0, steps).await;
                        }
                        if dx.abs() > 0.001 {
                            // Axis 1 = Horizontal scroll
                            let steps = if dx > 0.0 { 1 } else { -1 };
                            let _ = sink.notify_pointer_axis_discrete(1, steps).await;
                        }
                    }
                            Message::KeyDown { keycode } => {
                                pressed_keys.insert(keycode);
                                if (keycode & 0x8000_0000) != 0 || keycode > 0xFFF {
                                    let keysym = (keycode & 0x7FFF_FFFF) as i32;
                                    let _ = sink.notify_keyboard_keysym(keysym, ButtonState::Pressed).await;
                                } else {
                                    let _ = sink.notify_keyboard_keycode(keycode as i32, ButtonState::Pressed).await;
                                }
                            }
                            Message::KeyUp { keycode } => {
                                pressed_keys.remove(&keycode);
                                if (keycode & 0x8000_0000) != 0 || keycode > 0xFFF {
                                    let keysym = (keycode & 0x7FFF_FFFF) as i32;
                                    let _ = sink.notify_keyboard_keysym(keysym, ButtonState::Released).await;
                                } else {
                                    let _ = sink.notify_keyboard_keycode(keycode as i32, ButtonState::Released).await;
                                }
                            }
                            _ => {}
                        }
                    }
                }

        // Safety cleanup: release any keys still marked as pressed
        if let Some(ref sink) = sink_clone {
            for keycode in pressed_keys {
                if (keycode & 0x8000_0000) != 0 || keycode > 0xFFF {
                    let keysym = (keycode & 0x7FFF_FFFF) as i32;
                    let _ = sink.notify_keyboard_keysym(keysym, ButtonState::Released).await;
                } else {
                    let _ = sink.notify_keyboard_keycode(keycode as i32, ButtonState::Released).await;
                }
            }
        }
    });

    // 4. Video Streaming Loop: pipe captured frames -> encoder -> transport writer
    let mut frames_sent: u64 = 0;
    let mut perf_last_report = std::time::Instant::now();
    let mut perf_frames_count: u32 = 0;
    let mut perf_capture_wait_us: u64 = 0;
    let mut perf_encode_us: u64 = 0;
    let mut perf_write_us: u64 = 0;
    let mut last_frame_instant = std::time::Instant::now();

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

            reconfig_opt = reconfig_rx.recv() => {
                if let Some((target_w, target_h, target_fps)) = reconfig_opt {
                    let w = if target_w > 0 { target_w as usize } else { encoder.width() };
                    let h = if target_h > 0 { target_h as usize } else { encoder.height() };
                    let fps = if target_fps > 0 { target_fps as u32 } else { rate_controller.current_fps };

                    info!("Reconfiguring video stream to {}x{} @ {} FPS upon client request", w, h, fps);
                    rate_controller.set_target_fps(fps);
                    if w >= 1920 {
                        rate_controller.current_bitrate_bps = std::cmp::max(rate_controller.current_bitrate_bps, 12_000_000);
                    } else if w <= 1280 {
                        rate_controller.current_bitrate_bps = std::cmp::min(rate_controller.current_bitrate_bps, 8_000_000);
                    }

                    let new_settings = EncoderSettings {
                        width: w,
                        height: h,
                        bitrate_bps: rate_controller.current_bitrate_bps,
                        max_fps: fps as f32,
                    };
                    match VideoEncoder::new(new_settings) {
                        Ok(new_enc) => {
                            encoder = new_enc;
                            encoder.force_keyframe();

                            // Send updated VideoConfig confirmation back to client
                            let conf = Message::VideoConfig {
                                width: w as u16,
                                height: h as u16,
                                codec: 1,
                                fps_target: fps as u16,
                            };
                            let seq = sequence.fetch_add(1, Ordering::Relaxed);
                            if let Err(e) = protocol::write_message(&mut writer, &conf, seq).await {
                                warn!("Failed to send reconfigured VideoConfig to client: {:#}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to reconfigure video encoder to {}x{}: {:#}", w, h, e);
                        }
                    }
                }
            }

            telemetry_opt = telemetry_rx.recv() => {
                match telemetry_opt {
                    Some((latency_ms, client_fps)) => {
                        rate_controller.on_network_feedback(latency_ms, client_fps);
                        encoder.set_bitrate(rate_controller.current_bitrate_bps);
                    }
                    None => {
                        break;
                    }
                }
            }

            frame_opt = frame_rx.recv() => {
                match frame_opt {
                    Some(frame) => {
                        let wait_dur = last_frame_instant.elapsed();
                        perf_capture_wait_us += wait_dur.as_micros() as u64;
                        last_frame_instant = std::time::Instant::now();

                        let is_keyframe_req = keyframe_requested.swap(false, Ordering::Relaxed);
                        if is_keyframe_req {
                            encoder.force_keyframe();
                        }

                        if !is_keyframe_req && !rate_controller.should_encode_frame() {
                            continue; // Pace frame transmission according to dynamic FPS target
                        }

                        let now_ns = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64;

                        let t_enc = std::time::Instant::now();
                        let enc_res = encoder.encode_bgrx(
                            &frame.data,
                            frame.width as usize,
                            frame.height as usize,
                            frame.stride as usize,
                            now_ns,
                        );
                        perf_encode_us += t_enc.elapsed().as_micros() as u64;

                        match enc_res {
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
                                let t_write = std::time::Instant::now();
                                let write_res = protocol::write_message(&mut writer, &msg, seq).await;
                                perf_write_us += t_write.elapsed().as_micros() as u64;

                                if let Err(e) = write_res {
                                    warn!("Failed to send video frame to client: {:#}", e);
                                    break;
                                }

                                frames_sent += 1;
                                perf_frames_count += 1;

                                if perf_last_report.elapsed().as_secs() >= 5 && perf_frames_count > 0 {
                                    let elapsed = perf_last_report.elapsed().as_secs_f64();
                                    let fps = perf_frames_count as f64 / elapsed;
                                    let avg_capture = perf_capture_wait_us as f64 / (perf_frames_count as f64 * 1000.0);
                                    let avg_encode = perf_encode_us as f64 / (perf_frames_count as f64 * 1000.0);
                                    let avg_write = perf_write_us as f64 / (perf_frames_count as f64 * 1000.0);
                                    info!(
                                        "[PERF METRICS] Video: {:.1} FPS | Latency: capture_wait={:.2}ms, encode={:.2}ms, tcp_write={:.2}ms | CPU Max: {:.0} FPS",
                                        fps, avg_capture, avg_encode, avg_write,
                                        if avg_encode > 0.0 { 1000.0 / avg_encode } else { 999.0 }
                                    );
                                    perf_last_report = std::time::Instant::now();
                                    perf_frames_count = 0;
                                    perf_capture_wait_us = 0;
                                    perf_encode_us = 0;
                                    perf_write_us = 0;
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

/// Backwards-compatible wrapper for running a session with RemoteDesktopPortal.
pub async fn run_session_with_portal(
    stream: Box<dyn TransportStream>,
    frame_rx: mpsc::Receiver<CapturedFrame>,
    portal: Option<Arc<tokio::sync::Mutex<RemoteDesktopPortal>>>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let input_sink = portal.map(InputSink::Portal);
    run_session(stream, frame_rx, input_sink, running).await
}
