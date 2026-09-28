//! WARD TLS 1.3 Network Server & Streaming Pipeline.
//!
//! Handles:
//! - TLS 1.3 encrypted TCP connection on port 9740
//! - Client handshake (ConnectionInit -> SessionAccepted)
//! - Real-time streaming of H.264 video frames
//! - Receiving and dispatching remote input messages (Mouse/Keyboard)

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tracing::info;

use crate::capture::pipewire_capture::CapturedFrame;
use crate::portal::remote_desktop::RemoteDesktopPortal;

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
    frame_rx: mpsc::Receiver<CapturedFrame>,
    portal: Option<Arc<tokio::sync::Mutex<RemoteDesktopPortal>>>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let transport_stream = Box::new(crate::transport::lan::LanTransportStream::new(stream, peer_addr));
    crate::session::run_session(transport_stream, frame_rx, portal, running).await
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

