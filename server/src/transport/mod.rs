pub mod lan;

use std::net::SocketAddr;
use anyhow::Result;
use tokio::io::{AsyncRead, AsyncWrite};

/// Identifies the network transport mechanism currently in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransportType {
    /// Local Area Network (direct TLS 1.3 over TCP).
    Lan,
    /// Direct Peer-to-Peer over Internet (via STUN / ICE / QUIC).
    DirectP2p,
    /// Secure Encrypted Relay (when direct P2P fails due to symmetric NAT).
    Relay,
}

impl std::fmt::Display for TransportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lan => write!(f, "LAN"),
            Self::DirectP2p => write!(f, "P2P"),
            Self::Relay => write!(f, "Relay"),
        }
    }
}

/// Abstract bidirectional byte stream for remote desktop communication.
pub trait TransportStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {
    /// Returns the type of transport.
    fn transport_type(&self) -> TransportType;

    /// Returns the remote peer address if available.
    fn peer_addr(&self) -> Option<SocketAddr>;
}

/// Abstract listener for accepting incoming client transport streams.
pub trait TransportListener: Send + Sync {
    /// Returns the local bound address.
    fn local_addr(&self) -> Result<SocketAddr>;

    /// Returns the transport type provided by this listener.
    fn transport_type(&self) -> TransportType;
}

#[cfg(test)]
mod tests {
    use super::*;
    use lan::LanTransportListener;

    #[test]
    fn test_transport_type_formatting() {
        assert_eq!(TransportType::Lan.to_string(), "LAN");
        assert_eq!(TransportType::DirectP2p.to_string(), "P2P");
        assert_eq!(TransportType::Relay.to_string(), "Relay");
    }

    #[tokio::test]
    async fn test_lan_transport_listener_bind() {
        let listener = LanTransportListener::bind(0).await.expect("Failed to bind LanTransportListener");
        let addr = listener.local_addr().expect("Failed to get local address");
        assert!(addr.port() > 0, "Bound port must be greater than 0");
        assert_eq!(listener.transport_type(), TransportType::Lan);
    }
}

