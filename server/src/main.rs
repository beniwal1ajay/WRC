//! WARD Server — Wayland Android Remote Desktop
//!
//! Milestone 1: Proof of Wayland screen capture.
//!
//! This program:
//! 1. Connects to the XDG ScreenCast portal via D-Bus.
//! 2. Requests permission to capture the monitor.
//! 3. Receives a PipeWire stream.
//! 4. Captures frames from the stream.
//! 5. Saves captured frames as PNG files for verification.

mod portal;
mod capture;

use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use portal::remote_desktop::{ButtonState, RemoteDesktopPortal, buttons, device_types, keys};
use portal::screencast::{CursorMode, ScreenCastPortal, SourceType};
use capture::pipewire_capture::{CaptureStats, CapturedFrame, FrameFormat, run_pipewire_capture};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .with_target(true)
        .with_thread_ids(true)
        .init();

    info!("WARD Server starting");
    info!("Session type: {}", std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()));
    info!("Desktop: {}", std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into()));

    // Verify we're on Wayland
    let session_type = std::env::var("XDG_SESSION_TYPE")
        .unwrap_or_default();
    if session_type != "wayland" {
        warn!(
            "XDG_SESSION_TYPE is '{}', expected 'wayland'. Screen capture and input injection may not work correctly.",
            session_type
        );
    }

    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("input");

    match mode {
        "capture" | "--capture" => {
            info!("Mode: Milestone 1 — Screen Capture Proof");
            match run_screen_capture_poc().await {
                Ok(()) => {
                    info!("Screen capture proof of concept completed successfully!");
                    Ok(())
                }
                Err(e) => {
                    error!("Screen capture failed: {:#}", e);
                    Err(e)
                }
            }
        }
        "input" | "--input" | _ => {
            info!("Mode: Milestone 2 — Remote Input Proof");
            match run_remote_input_poc().await {
                Ok(()) => {
                    info!("Remote input proof of concept completed successfully!");
                    Ok(())
                }
                Err(e) => {
                    error!("Remote input failed: {:#}", e);
                    Err(e)
                }
            }
        }
    }
}

/// Milestone 2: Proof of Remote Input injection via combined RemoteDesktop + ScreenCast portal.
async fn run_remote_input_poc() -> Result<()> {
    info!("=== Milestone 2: Remote Input Proof of Concept ===");

    // Step 1: Create a RemoteDesktop session
    info!("Step 1: Creating RemoteDesktop portal session...");
    let mut portal = RemoteDesktopPortal::new().await?;
    portal.create_session().await?;

    // Step 2: Select devices (Keyboard + Pointer)
    info!("Step 2: Selecting input devices (Keyboard + Pointer)...");
    portal.select_devices(device_types::KEYBOARD | device_types::POINTER).await?;

    // Step 3: Select sources (Monitor, combined session)
    info!("Step 3: Selecting capture sources for combined session...");
    portal.select_sources(SourceType::Monitor, CursorMode::Embedded).await?;

    // Step 4: Start combined session
    info!("Step 4: Starting combined RemoteDesktop + ScreenCast session...");
    info!(">>> A permission dialog may appear on your screen. Please click 'Share' / 'Allow'. <<<");
    let streams = portal.start("").await?;

    info!("RemoteDesktop session active! Stream count: {}", streams.len());
    for (i, stream) in streams.iter().enumerate() {
        info!(
            "  Stream {}: PipeWire node={}, size={:?}",
            i, stream.pipewire_node_id, stream.size
        );
    }

    info!("Starting input injection sequence in 2 seconds...");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Test 1: Relative Pointer Motion (Smooth Square)
    info!("--- Test 1: Relative pointer motion (smooth square) ---");
    // Move Right (+100px in 20 steps of 5px)
    for _ in 0..20 {
        portal.notify_pointer_motion(5.0, 0.0).await?;
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    }
    // Move Down (+100px in 20 steps of 5px)
    for _ in 0..20 {
        portal.notify_pointer_motion(0.0, 5.0).await?;
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    }
    // Move Left (-100px in 20 steps of 5px)
    for _ in 0..20 {
        portal.notify_pointer_motion(-5.0, 0.0).await?;
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    }
    // Move Up (-100px in 20 steps of 5px)
    for _ in 0..20 {
        portal.notify_pointer_motion(0.0, -5.0).await?;
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    }
    info!("✅ Test 1 passed: Relative pointer motion completed");

    // Test 2: Absolute Pointer Motion
    if let Some(stream) = streams.first() {
        let node_id = stream.pipewire_node_id;
        let (target_x, target_y) = if let Some((w, h)) = stream.size {
            (w as f64 / 2.0, h as f64 / 2.0)
        } else {
            (960.0, 540.0)
        };
        info!(
            "--- Test 2: Absolute pointer motion to stream center ({}, {}) on node {} ---",
            target_x, target_y, node_id
        );
        portal.notify_pointer_motion_absolute(node_id, target_x, target_y).await?;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        info!("✅ Test 2 passed: Absolute pointer motion completed");
    }

    // Test 3: Mouse Buttons
    info!("--- Test 3: Mouse button clicks ---");
    portal.click_pointer_button(buttons::BTN_LEFT).await?;
    info!("  Left click sent");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    info!("✅ Test 3 passed: Mouse clicks completed");

    // Test 4: Mouse Scrolling
    info!("--- Test 4: Pointer scrolling ---");
    portal.notify_pointer_axis_discrete(0, -1).await?; // scroll up
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    portal.notify_pointer_axis_discrete(0, 1).await?;  // scroll down
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    info!("✅ Test 4 passed: Mouse scroll completed");

    // Test 5: Keyboard Keycodes & Modifier Keys
    info!("--- Test 5: Keyboard keycodes (Shift modifier + keycode) ---");
    // Press Shift
    portal.notify_keyboard_keycode(keys::KEY_LEFT_SHIFT, ButtonState::Pressed).await?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    // Press & Release 'A'
    portal.type_keyboard_keycode(keys::KEY_A).await?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    // Release Shift
    portal.notify_keyboard_keycode(keys::KEY_LEFT_SHIFT, ButtonState::Released).await?;
    info!("✅ Test 5 passed: Keyboard keycode + modifier completed");

    // Test 6: Keyboard Keysym
    info!("--- Test 6: Keyboard keysym injection ---");
    // Keysym for 'w' (0x77), 'a' (0x61), 'r' (0x72), 'd' (0x64)
    for keysym in [0x77, 0x61, 0x72, 0x64] {
        portal.notify_keyboard_keysym(keysym, ButtonState::Pressed).await?;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        portal.notify_keyboard_keysym(keysym, ButtonState::Released).await?;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    info!("✅ Test 6 passed: Keyboard keysym injection completed");

    // Step 7: Clean up
    info!("Step 7: Closing RemoteDesktop portal session...");
    portal.close().await?;
    info!("✅ RemoteDesktop session closed cleanly");
    info!("==================================================");
    info!("🎉 Milestone 2: Remote Input Proof of Concept SUCCEEDED!");
    info!("==================================================");

    Ok(())
}


async fn run_screen_capture_poc() -> Result<()> {
    // Step 1: Connect to the ScreenCast portal
    info!("Step 1: Creating ScreenCast portal session...");
    let mut portal = ScreenCastPortal::new().await?;
    portal.create_session().await?;

    // Step 2: Select the monitor to capture
    info!("Step 2: Selecting capture source (monitor, embedded cursor)...");
    info!(">>> A permission dialog should appear on your screen. Please grant access. <<<");
    portal.select_sources(
        SourceType::Monitor,
        CursorMode::Embedded,
        false, // single monitor
    ).await?;

    // Step 3: Start the session (triggers permission dialog)
    info!("Step 3: Starting ScreenCast session...");
    let streams = portal.start("").await?;

    if streams.is_empty() {
        anyhow::bail!("No capture streams returned by portal");
    }

    let stream = &streams[0];
    info!(
        "Capture stream ready: PipeWire node={}, size={:?}",
        stream.pipewire_node_id, stream.size
    );

    // Step 4: Open PipeWire remote
    info!("Step 4: Opening PipeWire remote fd...");
    let pw_fd = portal.open_pipewire_remote().await?;
    info!("PipeWire fd obtained: {}", pw_fd.as_raw_fd());

    // Step 5: Start PipeWire capture
    info!("Step 5: Starting PipeWire capture loop...");

    let running = Arc::new(AtomicBool::new(true));
    let stats = Arc::new(CaptureStats::default());

    // Create a bounded channel for frames
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<CapturedFrame>(4);

    let capture_handle = run_pipewire_capture(
        pw_fd,
        stream.pipewire_node_id,
        frame_tx,
        running.clone(),
        stats.clone(),
    )?;

    // Step 6: Receive and save frames
    info!("Step 6: Waiting for frames...");
    info!("Will capture 10 frames and save them as PNG files.");

    let output_dir = std::path::Path::new("captured_frames");
    std::fs::create_dir_all(output_dir)
        .context("Failed to create output directory")?;

    let mut frames_saved = 0;
    let max_frames = 10;
    let timeout = std::time::Duration::from_secs(30);
    let start_time = std::time::Instant::now();

    while frames_saved < max_frames {
        if start_time.elapsed() > timeout {
            warn!("Timeout waiting for frames after {} seconds", timeout.as_secs());
            break;
        }

        match frame_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(frame) => {
                info!(
                    "Frame {}: {}x{} stride={} format={:?} size={} bytes",
                    frame.sequence,
                    frame.width,
                    frame.height,
                    frame.stride,
                    frame.format,
                    frame.data.len()
                );

                // Save the first few frames as PNG
                if frames_saved < 3 {
                    match save_frame_as_png(&frame, output_dir, frames_saved) {
                        Ok(path) => info!("Saved frame as: {}", path),
                        Err(e) => warn!("Failed to save frame: {}", e),
                    }
                }

                frames_saved += 1;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                info!(
                    "Waiting for frames... (received={}, dropped={})",
                    stats.frames_received.load(Ordering::Relaxed),
                    stats.frames_dropped.load(Ordering::Relaxed)
                );
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                warn!("Frame channel disconnected");
                break;
            }
        }
    }

    // Step 7: Clean up
    info!("Step 7: Cleaning up...");
    running.store(false, Ordering::Relaxed);

    info!("\n=== Capture Statistics ===");
    info!("Frames received: {}", stats.frames_received.load(Ordering::Relaxed));
    info!("Frames dropped:  {}", stats.frames_dropped.load(Ordering::Relaxed));
    info!("Resolution:      {}x{}",
        stats.width.load(Ordering::Relaxed),
        stats.height.load(Ordering::Relaxed)
    );
    info!("Frames saved:    {}", frames_saved);
    info!("========================\n");

    // Close the portal session
    portal.close().await?;
    info!("Portal session closed");

    // Wait for the capture thread to finish (with timeout)
    info!("Waiting for capture thread to finish...");
    // Don't block forever on the join
    std::thread::spawn(move || {
        let _ = capture_handle.join();
    });

    if frames_saved > 0 {
        info!("✅ Screen capture proof of concept SUCCEEDED!");
        info!("Captured {} frames from the Wayland desktop.", frames_saved);
        info!("Check the 'captured_frames/' directory for saved PNGs.");
    } else {
        warn!("⚠️ No frames were captured. This may indicate:");
        warn!("  - PipeWire format negotiation needs adjustment");
        warn!("  - DMA-BUF frames need DMA-BUF handling support");
        warn!("  - Check logs with RUST_LOG=debug for more info");
    }

    Ok(())
}

/// Save a captured frame as a PNG file.
fn save_frame_as_png(
    frame: &CapturedFrame,
    output_dir: &std::path::Path,
    index: usize,
) -> Result<String> {
    let path = output_dir.join(format!("frame_{:04}.png", index));

    // Convert from the capture format to RGBA for PNG
    let rgba_data = convert_to_rgba(frame)?;

    let img = image::RgbaImage::from_raw(frame.width, frame.height, rgba_data)
        .context("Failed to create image from frame data")?;

    img.save(&path)
        .context("Failed to save PNG")?;

    Ok(path.to_string_lossy().into_owned())
}

/// Convert captured frame data to RGBA format.
fn convert_to_rgba(frame: &CapturedFrame) -> Result<Vec<u8>> {
    let pixel_count = (frame.width * frame.height) as usize;
    let mut rgba = Vec::with_capacity(pixel_count * 4);

    match frame.format {
        FrameFormat::BGRx | FrameFormat::BGRA => {
            // Convert BGRx/BGRA → RGBA
            for y in 0..frame.height {
                let row_start = (y * frame.stride) as usize;
                for x in 0..frame.width {
                    let offset = row_start + (x as usize) * 4;
                    if offset + 3 < frame.data.len() {
                        rgba.push(frame.data[offset + 2]); // R (from B position)
                        rgba.push(frame.data[offset + 1]); // G
                        rgba.push(frame.data[offset]);     // B (from R position)
                        rgba.push(255);                     // A
                    }
                }
            }
        }
        FrameFormat::RGBx | FrameFormat::RGBA => {
            // RGBx/RGBA → RGBA (just set alpha to 255 for RGBx)
            for y in 0..frame.height {
                let row_start = (y * frame.stride) as usize;
                for x in 0..frame.width {
                    let offset = row_start + (x as usize) * 4;
                    if offset + 3 < frame.data.len() {
                        rgba.push(frame.data[offset]);     // R
                        rgba.push(frame.data[offset + 1]); // G
                        rgba.push(frame.data[offset + 2]); // B
                        rgba.push(255);                     // A
                    }
                }
            }
        }
        FrameFormat::RGB => {
            // RGB → RGBA
            for y in 0..frame.height {
                let row_start = (y * frame.stride) as usize;
                for x in 0..frame.width {
                    let offset = row_start + (x as usize) * 3;
                    if offset + 2 < frame.data.len() {
                        rgba.push(frame.data[offset]);     // R
                        rgba.push(frame.data[offset + 1]); // G
                        rgba.push(frame.data[offset + 2]); // B
                        rgba.push(255);                     // A
                    }
                }
            }
        }
        FrameFormat::Unknown(fmt) => {
            warn!("Unknown pixel format: {}. Attempting BGRx interpretation.", fmt);
            // Best effort: assume BGRx
            for y in 0..frame.height {
                let row_start = (y * frame.stride) as usize;
                for x in 0..frame.width {
                    let offset = row_start + (x as usize) * 4;
                    if offset + 3 < frame.data.len() {
                        rgba.push(frame.data[offset + 2]);
                        rgba.push(frame.data[offset + 1]);
                        rgba.push(frame.data[offset]);
                        rgba.push(255);
                    }
                }
            }
        }
    }

    Ok(rgba)
}
