//! XDG Desktop Portal ScreenCast interface.
//!
//! Implements the org.freedesktop.portal.ScreenCast D-Bus interface
//! to capture the Wayland desktop via PipeWire.

use std::collections::HashMap;
use std::os::fd::OwnedFd;

use anyhow::{Context, Result, bail};
use tracing::{debug, info, warn};
use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::session::{
    PortalResponse, generate_request_token, generate_session_token,
    request_path, wait_for_response,
};

/// Source types for screen capture.
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum SourceType {
    Monitor = 1,
    Window = 2,
    Virtual = 4,
}

/// Cursor modes for screen capture.
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum CursorMode {
    Hidden = 1,
    Embedded = 2,
    Metadata = 4,
}

/// Information about a captured stream source.
#[derive(Debug, Clone)]
pub struct CaptureStream {
    pub pipewire_node_id: u32,
    pub position: Option<(i32, i32)>,
    pub size: Option<(i32, i32)>,
    pub source_type: Option<u32>,
}

/// Helper to create a portal proxy for the given interface.
async fn portal_proxy<'a>(
    connection: &'a Connection,
    interface: &str,
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

/// Manages a ScreenCast portal session.
pub struct ScreenCastPortal {
    connection: Connection,
    session_handle: Option<OwnedObjectPath>,
    streams: Vec<CaptureStream>,
}

impl ScreenCastPortal {
    pub async fn new() -> Result<Self> {
        let connection = Connection::session()
            .await
            .context("Failed to connect to session D-Bus")?;
        info!("Connected to session D-Bus for ScreenCast portal");
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

    /// Step 1: Create a portal session.
    pub async fn create_session(&mut self) -> Result<()> {
        let token = generate_request_token();
        let session_token = generate_session_token();

        let sender = self.connection.unique_name()
            .context("No unique bus name")?
            .to_string();
        let expected_path = request_path(&sender, &token);

        debug!("Creating ScreenCast session (token={}, session_token={})", token, session_token);

        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.ScreenCast").await?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));
        options.insert("session_handle_token", Value::from(session_token.as_str()));

        let _request_path: OwnedObjectPath = proxy
            .call("CreateSession", &(options,))
            .await
            .context("CreateSession D-Bus call failed")?;

        let (response, results) = wait_for_response(&self.connection, &expected_path).await?;

        match response {
            PortalResponse::Success => {
                let session_handle = results
                    .get("session_handle")
                    .context("No session_handle in response")?;
                let session_str: String = session_handle
                    .try_clone()
                    .context("Failed to clone session handle")?
                    .try_into()
                    .context("session_handle is not a string")?;
                self.session_handle = Some(
                    OwnedObjectPath::try_from(session_str.clone())
                        .context("Invalid session handle path")?
                );
                info!("ScreenCast session created: {}", session_str);
                Ok(())
            }
            PortalResponse::Cancelled => bail!("User cancelled session creation"),
            PortalResponse::Other(code) => bail!("Session creation failed with code {}", code),
        }
    }

    /// Step 2: Select sources (which monitors/windows to capture).
    pub async fn select_sources(
        &self,
        source_type: SourceType,
        cursor_mode: CursorMode,
        allow_multiple: bool,
    ) -> Result<()> {
        let session = self.session_handle.as_ref()
            .context("No session — call create_session first")?;

        let token = generate_request_token();
        let sender = self.connection.unique_name()
            .context("No unique bus name")?
            .to_string();
        let expected_path = request_path(&sender, &token);

        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.ScreenCast").await?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));
        options.insert("types", Value::from(source_type as u32));
        options.insert("cursor_mode", Value::from(cursor_mode as u32));
        options.insert("multiple", Value::from(allow_multiple));
        options.insert("persist_mode", Value::from(2u32));

        let _request_path: OwnedObjectPath = proxy
            .call("SelectSources", &(session, options))
            .await
            .context("SelectSources D-Bus call failed")?;

        let (response, _results) = wait_for_response(&self.connection, &expected_path).await?;

        match response {
            PortalResponse::Success => {
                info!("Sources selected successfully");
                Ok(())
            }
            PortalResponse::Cancelled => bail!("User cancelled source selection"),
            PortalResponse::Other(code) => bail!("Source selection failed with code {}", code),
        }
    }

    /// Step 3: Start the screencast session.
    pub async fn start(&mut self, parent_window: &str) -> Result<Vec<CaptureStream>> {
        let session = self.session_handle.as_ref()
            .context("No session — call create_session first")?;

        let token = generate_request_token();
        let sender = self.connection.unique_name()
            .context("No unique bus name")?
            .to_string();
        let expected_path = request_path(&sender, &token);

        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.ScreenCast").await?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));

        let _request_path: OwnedObjectPath = proxy
            .call("Start", &(session, parent_window, options))
            .await
            .context("Start D-Bus call failed")?;

        let (response, results) = wait_for_response(&self.connection, &expected_path).await?;

        match response {
            PortalResponse::Success => {
                let streams = parse_streams(&results)?;
                info!("ScreenCast started with {} stream(s)", streams.len());
                for (i, stream) in streams.iter().enumerate() {
                    info!(
                        "  Stream {}: PipeWire node={}, size={:?}",
                        i, stream.pipewire_node_id, stream.size
                    );
                }
                self.streams = streams.clone();
                Ok(streams)
            }
            PortalResponse::Cancelled => bail!("User cancelled screen sharing"),
            PortalResponse::Other(code) => bail!("Start failed with code {}", code),
        }
    }

    /// Step 4: Open a PipeWire remote fd for accessing the stream.
    pub async fn open_pipewire_remote(&self) -> Result<OwnedFd> {
        let session = self.session_handle.as_ref()
            .context("No session")?;

        let proxy = portal_proxy(&self.connection, "org.freedesktop.portal.ScreenCast").await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();

        let fd: OwnedFd = proxy
            .call("OpenPipeWireRemote", &(session, options))
            .await
            .context("OpenPipeWireRemote D-Bus call failed")?;

        info!("PipeWire remote fd obtained");
        Ok(fd)
    }

    pub fn streams(&self) -> &[CaptureStream] {
        &self.streams
    }

    pub async fn close(&mut self) -> Result<()> {
        if let Some(session) = self.session_handle.take() {
            debug!("Closing ScreenCast session: {}", session.as_str());
            let proxy = zbus::Proxy::new(
                &self.connection,
                "org.freedesktop.portal.Desktop",
                session.as_str(),
                "org.freedesktop.portal.Session",
            )
            .await?;
            let _ = proxy.call_method("Close", &()).await;
            info!("ScreenCast session closed");
        }
        Ok(())
    }
}

impl Drop for ScreenCastPortal {
    fn drop(&mut self) {
        if self.session_handle.is_some() {
            warn!("ScreenCast session was not explicitly closed");
        }
    }
}

/// Parse the streams array from the portal response.
pub fn parse_streams(
    results: &HashMap<String, OwnedValue>,
) -> Result<Vec<CaptureStream>> {
    let streams_value = results
        .get("streams")
        .context("No 'streams' in Start response")?;

    // streams is a(ua{sv}) — array of (uint32, dict)
    let streams_array: Vec<(u32, HashMap<String, OwnedValue>)> = streams_value
        .try_clone()
        .context("Failed to clone streams value")?
        .try_into()
        .context("Failed to parse streams as a(ua{sv})")?;

    let mut result = Vec::new();
    for (node_id, props) in streams_array {
        let mut stream = CaptureStream {
            pipewire_node_id: node_id,
            position: None,
            size: None,
            source_type: None,
        };

        if let Some(size_val) = props.get("size") {
            if let Ok(size_tuple) = TryInto::<(i32, i32)>::try_into(
                size_val.try_clone().unwrap_or(size_val.clone()),
            ) {
                stream.size = Some(size_tuple);
            }
        }

        if let Some(pos_val) = props.get("position") {
            if let Ok(pos_tuple) = TryInto::<(i32, i32)>::try_into(
                pos_val.try_clone().unwrap_or(pos_val.clone()),
            ) {
                stream.position = Some(pos_tuple);
            }
        }

        if let Some(type_val) = props.get("source_type") {
            if let Ok(source_type) = TryInto::<u32>::try_into(
                type_val.try_clone().unwrap_or(type_val.clone()),
            ) {
                stream.source_type = Some(source_type);
            }
        }

        result.push(stream);
    }

    Ok(result)
}
