//! XDG Desktop Portal RemoteDesktop interface.
//!
//! Implements org.freedesktop.portal.RemoteDesktop for input injection.
//! Supports combined RemoteDesktop + ScreenCast sessions.

use std::collections::HashMap;
use std::os::fd::{FromRawFd, OwnedFd};

use anyhow::{Context, Result, bail};
use tracing::info;
use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, Value};

use super::screencast::{CaptureStream, CursorMode, SourceType, parse_streams};
use super::session::{
    PortalResponse, generate_request_token, generate_session_token,
    request_path, subscribe_portal_response,
};

/// Button state for mouse/keyboard events.
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum ButtonState {
    Released = 0,
    Pressed = 1,
}

/// Helper to create a portal proxy.
async fn portal_proxy<'a>(
    connection: &'a Connection,
    interface: &'a str,
) -> Result<zbus::Proxy<'a>> {
    zbus::Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        interface,
    )
    .await
    .context("Failed to create portal proxy")
}

/// Manages a RemoteDesktop portal session (optionally combined with ScreenCast).
pub struct RemoteDesktopPortal {
    connection: Connection,
    session_handle: Option<OwnedObjectPath>,
    streams: Vec<CaptureStream>,
}

#[allow(dead_code)]
impl RemoteDesktopPortal {
    pub async fn new() -> Result<Self> {
        let connection = Connection::session()
            .await
            .context("Failed to connect to session D-Bus")?;
        info!("Connected to session D-Bus for RemoteDesktop portal");
        Ok(Self {
            connection,
            session_handle: None,
            streams: Vec::new(),
        })
    }

    pub async fn with_connection(connection: Connection) -> Result<Self> {
        Ok(Self {
            connection,
            session_handle: None,
            streams: Vec::new(),
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn session_handle(&self) -> Option<&OwnedObjectPath> {
        self.session_handle.as_ref()
    }

    /// Step 1: Create a RemoteDesktop session.
    pub async fn create_session(&mut self) -> Result<()> {
        let token = generate_request_token();
        let session_token = generate_session_token();

        let sender = self.connection.unique_name()
            .context("No unique bus name")?
            .to_string();
        let expected_path = request_path(&sender, &token);

        let listener = subscribe_portal_response(&self.connection, &expected_path).await?;

        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));
        options.insert("session_handle_token", Value::from(session_token.as_str()));

        let _request_path: OwnedObjectPath = proxy
            .call("CreateSession", &(options,))
            .await
            .context("CreateSession D-Bus call failed")?;

        let (response, results) = listener.wait().await?;

        match response {
            PortalResponse::Success => {
                let session_handle = results
                    .get("session_handle")
                    .context("No session_handle in response")?;
                let session_str: String = session_handle
                    .try_clone().context("Clone failed")?
                    .try_into().context("session_handle not a string")?;
                self.session_handle = Some(OwnedObjectPath::try_from(session_str.clone())?);
                info!("RemoteDesktop session created: {}", session_str);
                Ok(())
            }
            PortalResponse::Cancelled => bail!("User cancelled"),
            PortalResponse::Other(code) => bail!("Failed with code {}", code),
        }
    }

    /// Step 2a: Select input devices.
    pub async fn select_devices(&self, device_types: u32) -> Result<()> {
        let session = self.session_handle.as_ref().context("No session")?;

        let token = generate_request_token();
        let sender = self.connection.unique_name().context("No unique bus name")?.to_string();
        let expected_path = request_path(&sender, &token);

        let listener = subscribe_portal_response(&self.connection, &expected_path).await?;

        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));
        options.insert("types", Value::from(device_types));

        let _: OwnedObjectPath = proxy
            .call("SelectDevices", &(session, options))
            .await
            .context("SelectDevices failed")?;

        let (response, _) = listener.wait().await?;

        match response {
            PortalResponse::Success => { info!("Devices selected"); Ok(()) }
            PortalResponse::Cancelled => bail!("User cancelled"),
            PortalResponse::Other(code) => bail!("Device selection failed: {}", code),
        }
    }

    /// Step 2b: Select screen capture sources (for combined session).
    pub async fn select_sources(
        &self,
        source_type: SourceType,
        cursor_mode: CursorMode,
    ) -> Result<()> {
        let session = self.session_handle.as_ref().context("No session")?;

        let token = generate_request_token();
        let sender = self.connection.unique_name().context("No unique bus name")?.to_string();
        let expected_path = request_path(&sender, &token);

        let listener = subscribe_portal_response(&self.connection, &expected_path).await?;

        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.ScreenCast").await?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));
        options.insert("types", Value::from(source_type as u32));
        options.insert("cursor_mode", Value::from(cursor_mode as u32));
        options.insert("multiple", Value::from(false));

        let _: OwnedObjectPath = proxy
            .call("SelectSources", &(session, options))
            .await
            .context("SelectSources failed")?;

        let (response, _) = listener.wait().await?;

        match response {
            PortalResponse::Success => { info!("Sources selected on RemoteDesktop session"); Ok(()) }
            PortalResponse::Cancelled => bail!("User cancelled"),
            PortalResponse::Other(code) => bail!("Source selection failed: {}", code),
        }
    }

    /// Step 3: Start the session.
    pub async fn start(&mut self, parent_window: &str) -> Result<Vec<CaptureStream>> {
        let session = self.session_handle.as_ref().context("No session")?;

        let token = generate_request_token();
        let sender = self.connection.unique_name().context("No unique bus name")?.to_string();
        let expected_path = request_path(&sender, &token);

        let listener = subscribe_portal_response(&self.connection, &expected_path).await?;

        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));

        let _: OwnedObjectPath = proxy
            .call("Start", &(session, parent_window, options))
            .await
            .context("Start failed")?;

        let (response, results) = listener.wait().await?;

        match response {
            PortalResponse::Success => {
                if results.contains_key("streams") {
                    let streams = parse_streams(&results)?;
                    info!("RemoteDesktop+ScreenCast started: {} stream(s)", streams.len());
                    self.streams = streams.clone();
                    Ok(streams)
                } else {
                    info!("RemoteDesktop started (no screen capture)");
                    Ok(Vec::new())
                }
            }
            PortalResponse::Cancelled => bail!("User cancelled"),
            PortalResponse::Other(code) => bail!("Start failed: {}", code),
        }
    }

    /// Open PipeWire remote fd.
    ///
    /// OpenPipeWireRemote returns a unix fd via SCM_RIGHTS. We must use
    /// call_method and extract the fd from the reply message's fd list.
    pub async fn open_pipewire_remote(&self) -> Result<OwnedFd> {
        let session = self.session_handle.as_ref().context("No session")?;
        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.ScreenCast").await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();

        let reply = proxy
            .call_method("OpenPipeWireRemote", &(session, options))
            .await
            .context("OpenPipeWireRemote D-Bus call failed")?;

        let body = reply.body();
        let fd: zbus::zvariant::Fd<'_> = body.deserialize()
            .context("Failed to deserialize PipeWire fd from reply")?;

        use std::os::fd::AsRawFd;
        let raw_fd = fd.as_raw_fd();
        let owned = unsafe { OwnedFd::from_raw_fd(libc::dup(raw_fd)) };

        info!("PipeWire remote fd obtained for RemoteDesktop session (raw={})", raw_fd);
        Ok(owned)
    }

    // ─── Input injection methods ──────────────────────────────────────

    pub async fn notify_pointer_motion(&self, dx: f64, dy: f64) -> Result<()> {
        let session = self.session_handle.as_ref().context("No session")?;
        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call_method("NotifyPointerMotion", &(session, options, dx, dy)).await?;
        Ok(())
    }

    pub async fn notify_pointer_motion_absolute(
        &self, stream_node_id: u32, x: f64, y: f64,
    ) -> Result<()> {
        let session = self.session_handle.as_ref().context("No session")?;
        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call_method("NotifyPointerMotionAbsolute", &(session, options, stream_node_id, x, y)).await?;
        Ok(())
    }

    /// Button codes follow Linux evdev: BTN_LEFT=272, BTN_RIGHT=273, BTN_MIDDLE=274
    pub async fn notify_pointer_button(&self, button: i32, state: ButtonState) -> Result<()> {
        let session = self.session_handle.as_ref().context("No session")?;
        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call_method("NotifyPointerButton", &(session, options, button, state as u32)).await?;
        Ok(())
    }

    /// axis: 0=vertical, 1=horizontal. steps: positive=down/right, negative=up/left
    pub async fn notify_pointer_axis_discrete(&self, axis: u32, steps: i32) -> Result<()> {
        let session = self.session_handle.as_ref().context("No session")?;
        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call_method("NotifyPointerAxisDiscrete", &(session, options, axis, steps)).await?;
        Ok(())
    }

    pub async fn notify_pointer_axis(&self, dx: f64, dy: f64) -> Result<()> {
        let session = self.session_handle.as_ref().context("No session")?;
        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call_method("NotifyPointerAxis", &(session, options, dx, dy)).await?;
        Ok(())
    }

    pub async fn notify_keyboard_keycode(&self, keycode: i32, state: ButtonState) -> Result<()> {
        let session = self.session_handle.as_ref().context("No session")?;
        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call_method("NotifyKeyboardKeycode", &(session, options, keycode, state as u32)).await?;
        Ok(())
    }

    pub async fn notify_keyboard_keysym(&self, keysym: i32, state: ButtonState) -> Result<()> {
        let session = self.session_handle.as_ref().context("No session")?;
        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.RemoteDesktop").await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        proxy.call_method("NotifyKeyboardKeysym", &(session, options, keysym, state as u32)).await?;
        Ok(())
    }

    /// Click a pointer button (press and release with short delay).
    pub async fn click_pointer_button(&self, button: i32) -> Result<()> {
        self.notify_pointer_button(button, ButtonState::Pressed).await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        self.notify_pointer_button(button, ButtonState::Released).await?;
        Ok(())
    }

    /// Type a keycode (press and release with short delay).
    pub async fn type_keyboard_keycode(&self, keycode: i32) -> Result<()> {
        self.notify_keyboard_keycode(keycode, ButtonState::Pressed).await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        self.notify_keyboard_keycode(keycode, ButtonState::Released).await?;
        Ok(())
    }

    pub fn streams(&self) -> &[CaptureStream] {
        &self.streams
    }

    pub async fn close(&mut self) -> Result<()> {
        if let Some(session) = self.session_handle.take() {
            let proxy = zbus::Proxy::new(
                &self.connection,
                "org.freedesktop.portal.Desktop",
                session.as_str(),
                "org.freedesktop.portal.Session",
            ).await?;
            let _ = proxy.call_method("Close", &()).await;
            info!("RemoteDesktop session closed");
        }
        Ok(())
    }
}

/// Device types bitmask for SelectDevices.
#[allow(dead_code)]
pub mod device_types {
    pub const KEYBOARD: u32 = 1;
    pub const POINTER: u32 = 2;
    pub const TOUCHSCREEN: u32 = 4;
    pub const ALL: u32 = 7;
}

/// Common evdev button codes.
#[allow(dead_code)]
pub mod buttons {
    pub const BTN_LEFT: i32 = 272;
    pub const BTN_RIGHT: i32 = 273;
    pub const BTN_MIDDLE: i32 = 274;
}

/// Common evdev key codes.
#[allow(dead_code)]
pub mod keys {
    pub const KEY_ESC: i32 = 1;
    pub const KEY_ENTER: i32 = 28;
    pub const KEY_TAB: i32 = 15;
    pub const KEY_BACKSPACE: i32 = 14;
    pub const KEY_SPACE: i32 = 57;
    pub const KEY_LEFT_CTRL: i32 = 29;
    pub const KEY_LEFT_SHIFT: i32 = 42;
    pub const KEY_LEFT_ALT: i32 = 56;
    pub const KEY_LEFT_META: i32 = 125;
    pub const KEY_A: i32 = 30;
    pub const KEY_C: i32 = 46;
    pub const KEY_V: i32 = 47;
    pub const KEY_X: i32 = 45;
    pub const KEY_Z: i32 = 44;
    pub const KEY_DELETE: i32 = 111;
    pub const KEY_UP: i32 = 103;
    pub const KEY_DOWN: i32 = 108;
    pub const KEY_LEFT: i32 = 105;
    pub const KEY_RIGHT: i32 = 106;
}
