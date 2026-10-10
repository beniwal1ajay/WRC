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
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use tracing::info;
use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, Value};

use super::remote_desktop::ButtonState;

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
        })
    }

    pub fn set_stream_path(&self, stream: String) {
        if let Ok(mut lock) = self.stream_path.write() {
            *lock = Some(stream);
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
        let _: () = proxy.call("NotifyPointerMotionAbsolute", &(stream.as_str(), x, y)).await?;
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

        let mut record_props: HashMap<&str, Value<'_>> = HashMap::new();
        // Mutter ScreenCast cursor-mode enum: 0 = Hidden, 1 = Embedded, 2 = Metadata
        // Setting to 1 (Embedded) instructs Mutter to render and burn the cursor directly into the screen capture framebuffer
        record_props.insert("cursor-mode", Value::from(1u32));
        let stream_path: OwnedObjectPath = sc_session_proxy
            .call("RecordMonitor", &("", record_props))
            .await
            .context("Mutter ScreenCast RecordMonitor failed")?;

        info!("Created Mutter ScreenCast stream with embedded cursor (cursor-mode=1): {}", stream_path);

        let stream_proxy = zbus::Proxy::new(
            &connection,
            "org.gnome.Mutter.ScreenCast",
            &stream_path,
            "org.gnome.Mutter.ScreenCast.Stream",
        )
        .await
        .context("Failed to create Mutter ScreenCast.Stream proxy")?;

        let mut signal_stream = stream_proxy
            .receive_signal("PipeWireStreamAdded")
            .await
            .context("Failed to subscribe to PipeWireStreamAdded signal")?;

        // Start RemoteDesktop (this automatically starts the linked ScreenCast session)
        remote_desktop.start().await?;
        remote_desktop.set_stream_path(stream_path.to_string());

        info!("Mutter ScreenCast session started; awaiting PipeWire node ID...");

        let signal_msg = tokio::time::timeout(Duration::from_secs(5), signal_stream.next())
            .await
            .context("Timeout waiting for Mutter PipeWireStreamAdded signal")?
            .context("Stream ended before PipeWireStreamAdded received")?;

        let (node_id,): (u32,) = signal_msg
            .body()
            .deserialize()
            .context("Failed to deserialize PipeWireStreamAdded node_id")?;

        info!("Mutter ScreenCast stream active with PipeWire node {}", node_id);

        let screencast = MutterScreenCast {
            connection: connection.clone(),
            session_path: sc_session_path,
            stream_path,
            node_id,
        };

        Ok(Self {
            screencast,
            remote_desktop,
        })
    }

    /// Stop both Mutter sessions cleanly.
    pub async fn stop(&self) -> Result<()> {
        let _ = self.screencast.stop().await;
        let _ = self.remote_desktop.stop().await;
        Ok(())
    }
}
