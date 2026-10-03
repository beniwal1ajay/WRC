//! Verification test for Mutter native ScreenCast and RemoteDesktop with PipeWire frame capture.

use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use zbus::Connection;
use zbus::zvariant::OwnedObjectPath;

use ward_server::capture::pipewire_capture::{run_pipewire_capture, CaptureStats, CapturedFrame};

pub fn open_pipewire_socket() -> Result<OwnedFd> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    let socket_path = PathBuf::from(runtime_dir).join("pipewire-0");
    let stream = UnixStream::connect(&socket_path)
        .context(format!("Failed to connect to PipeWire socket at {}", socket_path.display()))?;
    let raw = stream.into_raw_fd();
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

#[tokio::test]
async fn test_mutter_screencast_pipewire_frame_capture() -> Result<()> {
    // 1. Connect to session D-Bus
    let connection = Connection::session().await?;
    println!("Connected to session bus");

    // 2. Create Mutter ScreenCast session
    let sc_proxy = zbus::Proxy::new(
        &connection,
        "org.gnome.Mutter.ScreenCast",
        "/org/gnome/Mutter/ScreenCast",
        "org.gnome.Mutter.ScreenCast",
    ).await?;

    let empty_props: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> = std::collections::HashMap::new();
    let session_path: OwnedObjectPath = sc_proxy.call("CreateSession", &(empty_props,)).await?;
    println!("Created Mutter ScreenCast session: {}", session_path);

    let session_proxy = zbus::Proxy::new(
        &connection,
        "org.gnome.Mutter.ScreenCast",
        &session_path,
        "org.gnome.Mutter.ScreenCast.Session",
    ).await?;

    let empty_record_props: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> = std::collections::HashMap::new();
    let stream_path: OwnedObjectPath = session_proxy.call("RecordMonitor", &("", empty_record_props)).await?;
    println!("Created Mutter ScreenCast stream: {}", stream_path);

    // 3. Set up signal listener for PipeWireStreamAdded
    let stream_proxy = zbus::Proxy::new(
        &connection,
        "org.gnome.Mutter.ScreenCast",
        &stream_path,
        "org.gnome.Mutter.ScreenCast.Stream",
    ).await?;

    let mut signal_stream = stream_proxy.receive_signal("PipeWireStreamAdded").await?;

    // 4. Start ScreenCast session
    let _: () = session_proxy.call("Start", &()).await?;
    println!("Started Mutter ScreenCast session");

    // 5. Await node_id signal
    let signal_msg = tokio::time::timeout(Duration::from_secs(3), signal_stream.next())
        .await
        .context("Timeout waiting for PipeWireStreamAdded")?
        .context("Stream ended before PipeWireStreamAdded")?;

    let (node_id,): (u32,) = signal_msg.body().deserialize()?;
    println!("Received PipeWireStreamAdded: node_id = {}", node_id);
    assert!(node_id > 0, "Node ID should be valid");

    // 6. Connect to PipeWire via user socket and capture frames
    let pw_fd = open_pipewire_socket()?;
    println!("Connected to PipeWire socket fd: {:?}", pw_fd);

    let (pw_tx, pw_rx) = std::sync::mpsc::sync_channel::<CapturedFrame>(8);
    let running = Arc::new(AtomicBool::new(true));
    let stats = Arc::new(CaptureStats::default());

    let capture_handle = run_pipewire_capture(
        pw_fd,
        node_id,
        pw_tx,
        running.clone(),
        stats.clone(),
    )?;

    println!("Awaiting captured frames from PipeWire...");
    let mut frames_received = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);

    while std::time::Instant::now() < deadline && frames_received < 5 {
        if let Ok(frame) = pw_rx.recv_timeout(Duration::from_millis(500)) {
            frames_received += 1;
            println!(
                "Received Frame #{}: {}x{}, stride={}, format={:?}, data_len={}",
                frame.sequence, frame.width, frame.height, frame.stride, frame.format, frame.data.len()
            );
        }
    }

    running.store(false, Ordering::Relaxed);
    let _ = capture_handle.join();

    let _: () = session_proxy.call("Stop", &()).await?;
    println!("Stopped session. Total frames received: {}", frames_received);

    assert!(frames_received > 0, "Should have received at least 1 real frame from PipeWire");
    Ok(())
}
