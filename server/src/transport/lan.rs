//! LAN Transport Implementation (TLS 1.3 over TCP).

use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use anyhow::{Context as _, Result};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use super::{TransportListener, TransportStream, TransportType};
use crate::network::create_tls_config;

/// LAN bidirectional stream wrapped in TLS 1.3 encryption.
pub struct LanTransportStream {
    inner: TlsStream<TcpStream>,
    peer_addr: SocketAddr,
}

impl LanTransportStream {
    pub fn new(inner: TlsStream<TcpStream>, peer_addr: SocketAddr) -> Self {
        Self { inner, peer_addr }
    }

    pub fn peer_address(&self) -> SocketAddr {
        self.peer_addr
    }
}

impl AsyncRead for LanTransportStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for LanTransportStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl TransportStream for LanTransportStream {
    fn transport_type(&self) -> TransportType {
        TransportType::Lan
    }

    fn peer_addr(&self) -> Option<SocketAddr> {
        Some(self.peer_addr)
    }
}

/// LAN Listener accepting incoming TCP connections and performing TLS 1.3 handshakes.
pub struct LanTransportListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl LanTransportListener {
    pub async fn bind(port: u16) -> Result<Self> {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = TcpListener::bind(addr).await
            .context(format!("Failed to bind LAN TCP listener to {}", addr))?;

        let tls_config = create_tls_config()
            .context("Failed to configure TLS for LAN listener")?;
        let acceptor = TlsAcceptor::from(tls_config);

        Ok(Self { listener, acceptor })
    }

    pub async fn accept(&self) -> Result<LanTransportStream> {
        let (tcp_stream, peer_addr) = self.listener.accept().await
            .context("Failed to accept TCP connection on LAN")?;

        let tls_stream = self.acceptor.accept(tcp_stream).await
            .context(format!("TLS handshake failed with LAN peer {}", peer_addr))?;

        Ok(LanTransportStream::new(tls_stream, peer_addr))
    }
}

impl TransportListener for LanTransportListener {
    fn local_addr(&self) -> Result<SocketAddr> {
        self.listener.local_addr().context("Failed to get local address")
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Lan
    }
}
