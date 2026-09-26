//! Portal session management.
//!
//! Both ScreenCast and RemoteDesktop portals share a session concept.
//! A session is created, configured, started, and eventually closed.
//! The portal communicates results via D-Bus signals on Request objects.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

/// Response codes from the portal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalResponse {
    /// User granted permission.
    Success,
    /// User cancelled the dialog.
    Cancelled,
    /// An error occurred.
    Other(u32),
}

impl From<u32> for PortalResponse {
    fn from(code: u32) -> Self {
        match code {
            0 => PortalResponse::Success,
            1 => PortalResponse::Cancelled,
            other => PortalResponse::Other(other),
        }
    }
}

/// Generates a unique request token for portal requests.
pub fn generate_request_token() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ward{id}")
}

/// Generates a unique session token.
pub fn generate_session_token() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ward_session{id}")
}

/// Wait for a portal Response signal on the given request path.
///
/// Portal methods return an object path for the Request.
/// The actual result arrives as a `Response` signal on that path.
pub async fn wait_for_response(
    connection: &Connection,
    request_path: &str,
) -> Result<(PortalResponse, HashMap<String, OwnedValue>)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));

    debug!("Waiting for portal response on: {}", request_path);

    // Create proxy for the request object using zbus v5 API
    let proxy = zbus::Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        request_path,
        "org.freedesktop.portal.Request",
    )
    .await
    .context("Failed to create request proxy")?;

    // Listen for the Response signal
    let mut stream = proxy
        .receive_signal("Response")
        .await
        .context("Failed to subscribe to Response signal")?;

    // Spawn a task to wait for the signal
    let tx_for_task = tx.clone();
    tokio::spawn(async move {
        use futures_util::StreamExt;
        if let Some(signal) = stream.next().await {
            match signal.body().deserialize::<(u32, HashMap<String, OwnedValue>)>() {
                Ok((response_code, results)) => {
                    if let Some(sender) = tx_for_task.lock().unwrap().take() {
                        let _ = sender.send((PortalResponse::from(response_code), results));
                    }
                }
                Err(e) => {
                    warn!("Failed to deserialize portal response: {}", e);
                    if let Some(sender) = tx_for_task.lock().unwrap().take() {
                        let _ = sender.send((
                            PortalResponse::Other(99),
                            HashMap::new(),
                        ));
                    }
                }
            }
        }
    });

    // Wait for the response with a timeout
    let result = tokio::time::timeout(std::time::Duration::from_secs(120), rx)
        .await
        .context("Portal response timed out (120s)")?
        .context("Response channel closed")?;

    info!("Portal response received: {:?}", result.0);
    Ok(result)
}

/// Compute the expected request path for a given sender and token.
pub fn request_path(sender: &str, token: &str) -> String {
    let sender_part = sender
        .trim_start_matches(':')
        .replace('.', "_");
    format!("/org/freedesktop/portal/desktop/request/{sender_part}/{token}")
}

/// A prepared listener for a portal Response signal.
/// Create with [`subscribe_portal_response`] BEFORE making the D-Bus call
/// to avoid missing the signal due to a race condition.
pub struct ResponseListener {
    rx: tokio::sync::oneshot::Receiver<Result<(PortalResponse, HashMap<String, OwnedValue>)>>,
}

impl ResponseListener {
    /// Wait for the portal response with a timeout.
    pub async fn wait(self) -> Result<(PortalResponse, HashMap<String, OwnedValue>)> {
        let result = tokio::time::timeout(std::time::Duration::from_secs(120), self.rx)
            .await
            .context("Portal response timed out (120s)")?
            .context("Response channel closed")?;

        let (response, results) = result?;
        info!("Portal response received: {:?}", response);
        Ok((response, results))
    }
}

/// Subscribe to a portal Response signal **before** making the D-Bus call.
///
/// The XDG Desktop Portal emits a `Response` signal on a Request object
/// path. If we call the portal method first and then subscribe, the signal
/// may arrive before the subscription is active — causing a permanent hang.
///
/// This function sets up the listener first, so no signal can be missed.
///
/// # Usage
/// ```ignore
/// let listener = subscribe_portal_response(&connection, &request_path).await?;
/// proxy.call("CreateSession", &args).await?;
/// let (response, results) = listener.wait().await?;
/// ```
pub async fn subscribe_portal_response(
    connection: &Connection,
    request_path: &str,
) -> Result<ResponseListener> {
    debug!("Subscribing to portal response on: {}", request_path);

    // Create proxy for the Request object and subscribe to the signal.
    let proxy = zbus::Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        request_path,
        "org.freedesktop.portal.Request",
    )
    .await
    .context("Failed to create request proxy")?;

    let mut stream = proxy
        .receive_signal("Response")
        .await
        .context("Failed to subscribe to Response signal")?;

    // The signal stream is internally `'static` (it holds its own message
    // channel from the D-Bus connection), so it can be moved into a task.
    let (tx, rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        use futures_util::StreamExt;
        if let Some(signal) = stream.next().await {
            match signal.body().deserialize::<(u32, HashMap<String, OwnedValue>)>() {
                Ok((response_code, results)) => {
                    let _ = tx.send(Ok((PortalResponse::from(response_code), results)));
                }
                Err(e) => {
                    warn!("Failed to deserialize portal response: {}", e);
                    let _ = tx.send(Err(
                        anyhow::anyhow!("Failed to deserialize portal response: {}", e)
                    ));
                }
            }
        } else {
            let _ = tx.send(Err(
                anyhow::anyhow!("Response signal stream ended unexpectedly")
            ));
        }
    });

    Ok(ResponseListener { rx })
}
