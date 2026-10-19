//! Native GNOME Mutter ScreenCast and RemoteDesktop Backend.
//!
//! Provides direct unattended screen capture and input injection via Mutter's
//! session D-Bus interfaces (org.gnome.Mutter.ScreenCast & org.gnome.Mutter.RemoteDesktop).
//!
//! This backend is used for unattended background host operation in GNOME Wayland sessions,
//! operating without interactive authorization prompts at connection time.

use std::collections::HashMap;
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use serde::Deserialize;
use tracing::info;
use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Type, Value};

use super::remote_desktop::ButtonState;

#[derive(Deserialize, Type, Debug)]
struct MonitorSpec(String, String, String, String);

#[derive(Deserialize, Type, Debug)]
struct MonitorMode(
    String,
    i32,
    i32,
    f64,
    f64,
    Vec<f64>,
    HashMap<String, OwnedValue>,
);

#[derive(Deserialize, Type, Debug)]
struct MonitorInfo(
    MonitorSpec,
    Vec<MonitorMode>,
    HashMap<String, OwnedValue>,
);

#[derive(Deserialize, Type, Debug)]
struct LogicalMonitor(
    i32,
    i32,
    f64,
    u32,
    bool,
    Vec<MonitorSpec>,
    HashMap<String, OwnedValue>,
);

#[derive(Deserialize, Type, Debug)]
struct DisplayConfigState(
    u32,
    Vec<MonitorInfo>,
    Vec<LogicalMonitor>,
    HashMap<String, OwnedValue>,
);

#[derive(Debug, Clone)]
pub struct DisplayMonitor {
    pub id: u8,
    pub connector: String,
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub is_primary: bool,
}

pub async fn query_connected_monitors(connection: &Connection) -> Result<Vec<DisplayMonitor>> {
    let dc_proxy = zbus::Proxy::new(
        connection,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )
    .await
    .context("Failed to connect to Mutter DisplayConfig")?;

    let state: DisplayConfigState = dc_proxy
        .call("GetCurrentState", &())
        .await
        .context("Mutter DisplayConfig GetCurrentState failed")?;

    let primary_connector = state.2.iter()
        .find(|lm| lm.4)
        .and_then(|lm| lm.5.first())
        .map(|s| s.0.clone());

    let mut monitors = Vec::new();
    for (idx, m) in state.1.iter().enumerate() {
        let connector = (m.0).0.clone();
        let name = m.2.get("display-name")
            .and_then(|v| v.downcast_ref::<zbus::zvariant::Str>().ok().map(|s| s.to_string()))
            .unwrap_or_else(|| connector.clone());

        let current_mode = m.1.iter().find(|mode| {
            mode.6.get("is-current")
                .and_then(|v| v.downcast_ref::<bool>().ok())
                .unwrap_or(false)
        }).or_else(|| m.1.first());

        let (width, height) = current_mode.map(|cm| (cm.1 as u16, cm.2 as u16)).unwrap_or((1920, 1080));
        let is_primary = primary_connector.as_ref() == Some(&connector);

        monitors.push(DisplayMonitor {
            id: idx as u8,
            connector,
            name,
            width,
            height,
            is_primary,
        });
    }

    // Sort so primary monitor is index 0
    monitors.sort_by(|a, b| b.is_primary.cmp(&a.is_primary));
    for (idx, m) in monitors.iter_mut().enumerate() {
        m.id = idx as u8;
    }

    Ok(monitors)
}

#[derive(Debug, Clone)]
pub struct MutterStreamInfo {
    pub display_id: u8,
    pub connector: String,
    pub name: String,
    pub stream_path: OwnedObjectPath,
    pub node_id: u32,
    pub width: u16,
    pub height: u16,
    pub is_primary: bool,
}

/// Open a direct UNIX connection to the PipeWire daemon socket in the user's runtime directory.
pub fn open_pipewire_socket() -> Result<OwnedFd> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let uid = unsafe { libc::getuid() };
        format!("/run/user/{}", uid)
    });

    let socket_path = PathBuf::from(runtime_dir).join("pipewire-0");
    if !socket_path.exists() {
        bail!("PipeWire socket does not exist at {}", socket_path.display());
    }

    let stream = UnixStream::connect(&socket_path)
        .context(format!("Failed to connect to PipeWire socket at {}", socket_path.display()))?;

    let raw = stream.into_raw_fd();
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

/// Remote Desktop input controller using Mutter's session D-Bus API.
pub struct MutterRemoteDesktop {
    connection: Connection,
    session_path: OwnedObjectPath,
    pub session_id: String,
    stream_path: std::sync::RwLock<Option<String>>,
    active_dims: std::sync::RwLock<(u32, u32)>,
    stream_dims: std::sync::RwLock<(u32, u32)>,
}

impl MutterRemoteDesktop {
    pub async fn new(connection: &Connection) -> Result<Self> {
        let rd_proxy = zbus::Proxy::new(
            connection,
            "org.gnome.Mutter.RemoteDesktop",
            "/org/gnome/Mutter/RemoteDesktop",
            "org.gnome.Mutter.RemoteDesktop",
        )
        .await
        .context("Failed to create org.gnome.Mutter.RemoteDesktop proxy")?;

        let session_path: OwnedObjectPath = rd_proxy
            .call("CreateSession", &())
            .await
            .context("Mutter RemoteDesktop CreateSession failed")?;

        info!("Created Mutter RemoteDesktop session: {}", session_path);

        let session_proxy = zbus::Proxy::new(
            connection,
            "org.gnome.Mutter.RemoteDesktop",
            &session_path,
            "org.gnome.Mutter.RemoteDesktop.Session",
        )
        .await
        .context("Failed to create Mutter RemoteDesktop.Session proxy")?;

        let session_id: String = session_proxy
            .get_property("SessionId")
            .await
            .context("Failed to get Mutter RemoteDesktop SessionId property")?;

        info!("Mutter RemoteDesktop session initialized (SessionId: {})", session_id);

        Ok(Self {
            connection: connection.clone(),
            session_path,
            session_id,
            stream_path: std::sync::RwLock::new(None),
            active_dims: std::sync::RwLock::new((1920, 1080)),
            stream_dims: std::sync::RwLock::new((1920, 1080)),
        })
    }

    pub fn set_stream_path(&self, stream: String) {
        if let Ok(mut lock) = self.stream_path.write() {
            *lock = Some(stream);
        }
    }

    pub fn set_active_monitor_dims(&self, width: u32, height: u32) {
        if let Ok(mut lock) = self.active_dims.write() {
            *lock = (width, height);
        }
    }

    pub fn set_stream_dims(&self, width: u32, height: u32) {
        if let Ok(mut lock) = self.stream_dims.write() {
            *lock = (width, height);
        }
    }

    pub async fn start(&self) -> Result<()> {
        let proxy = self.session_proxy().await?;
        let _: () = proxy
            .call("Start", &())
            .await
            .context("Mutter RemoteDesktop Start failed")?;
        info!("Mutter RemoteDesktop session started");
        Ok(())
    }

    async fn session_proxy(&self) -> Result<zbus::Proxy<'_>> {
        zbus::Proxy::new(
            &self.connection,
            "org.gnome.Mutter.RemoteDesktop",
            &self.session_path,
            "org.gnome.Mutter.RemoteDesktop.Session",
        )
        .await
        .context("Failed to get Mutter RemoteDesktop session proxy")
    }

    pub async fn notify_pointer_motion(&self, dx: f64, dy: f64) -> Result<()> {
        let proxy = self.session_proxy().await?;
        let _: () = proxy.call("NotifyPointerMotionRelative", &(dx, dy)).await?;
        Ok(())
    }

    pub async fn notify_pointer_motion_absolute(&self, x: f64, y: f64) -> Result<()> {
        let proxy = self.session_proxy().await?;
        let stream = self.stream_path.read().unwrap().clone().unwrap_or_default();

        let (mon_w, mon_h) = *self.active_dims.read().unwrap();
        let (str_w, str_h) = *self.stream_dims.read().unwrap();

        let (target_x, target_y) = if str_w > 0 && str_h > 0 && mon_w > 0 && mon_h > 0 {
            let sx = (x / str_w as f64) * mon_w as f64;
            let sy = (y / str_h as f64) * mon_h as f64;
            (sx.clamp(0.0, (mon_w.saturating_sub(1)) as f64), sy.clamp(0.0, (mon_h.saturating_sub(1)) as f64))
        } else {
            (x, y)
        };

        let _: () = proxy.call("NotifyPointerMotionAbsolute", &(stream.as_str(), target_x, target_y)).await?;
        Ok(())
    }

    pub async fn notify_pointer_button(&self, button: i32, state: ButtonState) -> Result<()> {
        let proxy = self.session_proxy().await?;
        let pressed = matches!(state, ButtonState::Pressed);
        let _: () = proxy.call("NotifyPointerButton", &(button, pressed)).await?;
        Ok(())
    }

    pub async fn notify_pointer_axis(&self, dx: f64, dy: f64) -> Result<()> {
        let proxy = self.session_proxy().await?;
        let flags: u32 = 0;
        let _: () = proxy.call("NotifyPointerAxis", &(dx, dy, flags)).await?;
        Ok(())
    }

    pub async fn notify_pointer_axis_discrete(&self, axis: u32, steps: i32) -> Result<()> {
        let proxy = self.session_proxy().await?;
        let _: () = proxy.call("NotifyPointerAxisDiscrete", &(axis, steps)).await?;
        Ok(())
    }

    pub async fn notify_keyboard_keycode(&self, keycode: i32, state: ButtonState) -> Result<()> {
        let proxy = self.session_proxy().await?;
        let pressed = matches!(state, ButtonState::Pressed);
        let _: () = proxy.call("NotifyKeyboardKeycode", &(keycode as u32, pressed)).await?;
        Ok(())
    }

    pub async fn notify_keyboard_keysym(&self, keysym: i32, state: ButtonState) -> Result<()> {
        let proxy = self.session_proxy().await?;
        let pressed = matches!(state, ButtonState::Pressed);
        let _: () = proxy.call("NotifyKeyboardKeysym", &(keysym as u32, pressed)).await?;
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        if let Ok(proxy) = self.session_proxy().await {
            let _: () = proxy.call("Stop", &()).await?;
            info!("Mutter RemoteDesktop session stopped cleanly");
        }
        Ok(())
    }
}


/// Screen capture controller using Mutter's ScreenCast session D-Bus API.
pub struct MutterScreenCast {
    connection: Connection,
    session_path: OwnedObjectPath,
    pub stream_path: OwnedObjectPath,
    pub node_id: u32,
}

impl MutterScreenCast {
    pub async fn stop(&self) -> Result<()> {
        let session_proxy = zbus::Proxy::new(
            &self.connection,
            "org.gnome.Mutter.ScreenCast",
            &self.session_path,
            "org.gnome.Mutter.ScreenCast.Session",
        )
        .await?;

        let _: () = session_proxy.call("Stop", &()).await?;
        info!("Mutter ScreenCast session stopped cleanly");
        Ok(())
    }
}

/// Unified Mutter Session managing both ScreenCast and RemoteDesktop without dialog prompts.
pub struct MutterSession {
    pub screencast: MutterScreenCast,
    pub remote_desktop: Arc<MutterRemoteDesktop>,
    pub streams: Vec<MutterStreamInfo>,
    pub active_id: Arc<AtomicU8>,
}

impl MutterSession {
    /// Initialize a new Mutter ScreenCast + RemoteDesktop session linked together.
    pub async fn new() -> Result<Self> {
        let connection = Connection::session()
            .await
            .context("Failed to connect to session D-Bus for Mutter backend")?;

        let remote_desktop = Arc::new(MutterRemoteDesktop::new(&connection).await?);

        let sc_proxy = zbus::Proxy::new(
            &connection,
            "org.gnome.Mutter.ScreenCast",
            "/org/gnome/Mutter/ScreenCast",
            "org.gnome.Mutter.ScreenCast",
        )
        .await
        .context("Failed to create org.gnome.Mutter.ScreenCast proxy")?;

        let mut sc_props: HashMap<&str, Value<'_>> = HashMap::new();
        sc_props.insert("remote-desktop-session-id", Value::from(remote_desktop.session_id.as_str()));
        let sc_session_path: OwnedObjectPath = sc_proxy
            .call("CreateSession", &(sc_props,))
            .await
            .context("Mutter ScreenCast CreateSession failed")?;

        info!("Created Mutter ScreenCast session linked to RemoteDesktop: {}", sc_session_path);

        let sc_session_proxy = zbus::Proxy::new(
            &connection,
            "org.gnome.Mutter.ScreenCast",
            &sc_session_path,
            "org.gnome.Mutter.ScreenCast.Session",
        )
        .await
        .context("Failed to create Mutter ScreenCast.Session proxy")?;

        // Detect all connected monitors
        let detected = query_connected_monitors(&connection).await.unwrap_or_default();
        let monitors = if detected.is_empty() {
            vec![DisplayMonitor {
                id: 0,
                connector: "".to_string(),
                name: "Default Display".to_string(),
                width: 1920,
                height: 1080,
                is_primary: true,
            }]
        } else {
            detected
        };

        info!("Detected {} monitor(s) for Mutter screencast:", monitors.len());
        for m in &monitors {
            info!("  [Display {}] {} (connector: {}, {}x{}, primary: {})", m.id, m.name, m.connector, m.width, m.height, m.is_primary);
        }

        let mut signal_streams = Vec::new();
        for m in &monitors {
            let mut record_props: HashMap<&str, Value<'_>> = HashMap::new();
            record_props.insert("cursor-mode", Value::from(1u32));
            let stream_path: OwnedObjectPath = sc_session_proxy
                .call("RecordMonitor", &(m.connector.as_str(), record_props))
                .await
                .context(format!("Mutter ScreenCast RecordMonitor failed for connector '{}'", m.connector))?;

            info!("Created Mutter ScreenCast stream for Display {} ({}): {}", m.id, m.name, stream_path);

            let stream_proxy = zbus::Proxy::new(
                &connection,
                "org.gnome.Mutter.ScreenCast",
                &stream_path,
                "org.gnome.Mutter.ScreenCast.Stream",
            )
            .await
            .context("Failed to create Mutter ScreenCast.Stream proxy")?;

            let signal_stream = stream_proxy
                .receive_signal("PipeWireStreamAdded")
                .await
                .context("Failed to subscribe to PipeWireStreamAdded signal")?;

            signal_streams.push((m.clone(), stream_path, signal_stream));
        }

        // Start RemoteDesktop (this activates all linked ScreenCast streams)
        remote_desktop.start().await?;

        // Collect PipeWire node IDs for all monitors
        let mut streams = Vec::new();
        for (m, stream_path, mut signal_stream) in signal_streams {
            let signal_msg = tokio::time::timeout(Duration::from_secs(5), signal_stream.next())
                .await
                .context(format!("Timeout waiting for PipeWireStreamAdded on Display {}", m.id))?
                .context("Stream ended before PipeWireStreamAdded received")?;

            let (node_id,): (u32,) = signal_msg
                .body()
                .deserialize()
                .context("Failed to deserialize PipeWireStreamAdded node_id")?;

            info!("Mutter ScreenCast stream for Display {} ({}) active with PipeWire node {}", m.id, m.name, node_id);

            streams.push(MutterStreamInfo {
                display_id: m.id,
                connector: m.connector,
                name: m.name,
                stream_path,
                node_id,
                width: m.width,
                height: m.height,
                is_primary: m.is_primary,
            });
        }

        // Default active stream is the primary display
        let active_idx = streams.iter().position(|s| s.is_primary).unwrap_or(0);
        let active_stream = streams[active_idx].clone();
        remote_desktop.set_stream_path(active_stream.stream_path.to_string());
        remote_desktop.set_active_monitor_dims(active_stream.width as u32, active_stream.height as u32);
        let active_id = Arc::new(AtomicU8::new(active_stream.display_id));

        let screencast = MutterScreenCast {
            connection: connection.clone(),
            session_path: sc_session_path,
            stream_path: active_stream.stream_path,
            node_id: active_stream.node_id,
        };

        Ok(Self {
            screencast,
            remote_desktop,
            streams,
            active_id,
        })
    }

    pub async fn switch_display(&self, display_id: u8) -> Result<MutterStreamInfo> {
        let stream = self.streams.iter().find(|s| s.display_id == display_id)
            .cloned()
            .ok_or_else(|| anyhow!("Display ID {} not found", display_id))?;

        self.remote_desktop.set_stream_path(stream.stream_path.to_string());
        self.remote_desktop.set_active_monitor_dims(stream.width as u32, stream.height as u32);
        // Reposition cursor to center of the activated display
        let _ = self.remote_desktop.notify_pointer_motion_absolute(
            (stream.width / 2) as f64,
            (stream.height / 2) as f64,
        ).await;

        self.active_id.store(display_id, Ordering::Relaxed);
        info!("Mutter display switched to #{} ({}) [stream={}, node={}, size={}x{}]", display_id, stream.name, stream.stream_path, stream.node_id, stream.width, stream.height);
        Ok(stream)
    }

    pub fn get_display_list(&self) -> Vec<crate::protocol::DisplayInfo> {
        self.streams.iter().map(|s| crate::protocol::DisplayInfo {
            id: s.display_id,
            is_primary: s.is_primary,
            width: s.width,
            height: s.height,
            name: s.name.clone(),
        }).collect()
    }

    /// Stop both Mutter sessions cleanly.
    pub async fn stop(&self) -> Result<()> {
        let _ = self.screencast.stop().await;
        let _ = self.remote_desktop.stop().await;
        Ok(())
    }
}
