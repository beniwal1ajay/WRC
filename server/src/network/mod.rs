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

    let mut subject_alt_names = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
        "ward-server".to_string(),
    ];
    for (_name, ip) in list_network_interfaces() {
        let ip_str = ip.to_string();
        if !subject_alt_names.contains(&ip_str) {
            subject_alt_names.push(ip_str);
        }
    }

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

        info!("============================================================");
        info!("  [WARD NETWORK] Server active on port {}", port);
        info!("  Available connection endpoints:");
        for (iface, ip) in list_network_interfaces() {
            let tag = if iface.starts_with("tailscale") {
                "Tailscale VPN"
            } else if iface.starts_with("wg") {
                "WireGuard VPN"
            } else if iface.starts_with("tun") {
                "VPN Tunnel"
            } else if iface == "lo" {
                "Localhost"
            } else {
                "LAN / Wi-Fi"
            };
            info!("    - {:15}: {}:{} ({})", tag, ip, port, iface);
        }
        info!("============================================================");

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

        tracing::info!("[DIAG] TCP accepted from: {}", peer_addr);

        tracing::info!("[DIAG] TLS handshake BEGIN for: {}", peer_addr);
        tracing::info!("[DIAG] TLS handshake AWAIT for: {}", peer_addr);
        let handshake_start = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(10);
        match tokio::time::timeout(timeout_duration, self.acceptor.accept(tcp_stream)).await {
            Ok(Ok(tls_stream)) => {
                let elapsed = handshake_start.elapsed();
                tracing::info!("[DIAG] TLS handshake RETURNED for: {} (elapsed: {:?})", peer_addr, elapsed);
                let (_, server_conn) = tls_stream.get_ref();
                let protocol = server_conn.protocol_version().map(|v| format!("{:?}", v)).unwrap_or_else(|| "unknown".into());
                let cipher = server_conn.negotiated_cipher_suite().map(|c| format!("{:?}", c.suite())).unwrap_or_else(|| "unknown".into());
                let sni = server_conn.server_name().unwrap_or("none");
                let alpn = server_conn.alpn_protocol().map(|p| String::from_utf8_lossy(p).into_owned()).unwrap_or_else(|| "none".into());
                tracing::info!(
                    "[DIAG] TLS handshake SUCCESS for: {} (protocol={}, cipher={}, sni={}, alpn={}, elapsed={:?})",
                    peer_addr, protocol, cipher, sni, alpn, elapsed
                );
                Ok((tls_stream, peer_addr))
            }
            Ok(Err(e)) => {
                let elapsed = handshake_start.elapsed();
                tracing::warn!("[DIAG] TLS handshake RETURNED for: {} (elapsed: {:?})", peer_addr, elapsed);
                tracing::warn!(
                    "[DIAG] TLS handshake ERROR for: {}: kind={:?}, error={:#}",
                    peer_addr, e.kind(), e
                );
                Err(anyhow::anyhow!("TLS handshake failed with {}: {}", peer_addr, e))
            }
            Err(_) => {
                let elapsed = handshake_start.elapsed();
                tracing::warn!(
                    "[DIAG] TLS handshake TIMEOUT for: peer={}, elapsed={:?}, last known state=waiting for client TLS handshake completion",
                    peer_addr, elapsed
                );
                Err(anyhow::anyhow!("TLS handshake TIMEOUT with {} after {:?}", peer_addr, elapsed))
            }
        }
    }
}

/// Query the host's active network interfaces (LAN, VPNs like Tailscale/WireGuard, loopback).
pub fn list_network_interfaces() -> Vec<(String, std::net::IpAddr)> {
    let mut interfaces = Vec::new();
    #[cfg(unix)]
    unsafe {
        let mut ifaddrs_ptr = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifaddrs_ptr) == 0 && !ifaddrs_ptr.is_null() {
            let mut curr = ifaddrs_ptr;
            while !curr.is_null() {
                let ifa = *curr;
                if !ifa.ifa_addr.is_null() && (*ifa.ifa_addr).sa_family as i32 == libc::AF_INET {
                    let sa = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    let ip_u32 = u32::from_be(sa.sin_addr.s_addr);
                    let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::from(ip_u32));
                    let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
                    interfaces.push((name, ip));
                }
                curr = ifa.ifa_next;
            }
            libc::freeifaddrs(ifaddrs_ptr);
        }
    }
    interfaces
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

