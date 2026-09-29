use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use anyhow::{Context, Result, bail};
use rand_core::{OsRng, RngCore};
use tokio::net::UdpSocket;
use tracing::{info, warn};

pub const STUN_MAGIC_COOKIE: u32 = 0x2112_A442;
pub const BINDING_REQUEST: u16 = 0x0001;
pub const BINDING_RESPONSE: u16 = 0x0101;

pub const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
pub const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;

pub const DEFAULT_STUN_SERVERS: &[&str] = &[
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun2.l.google.com:19302",
];

/// Result of STUN reflexive public address resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StunReflexiveAddress {
    pub public_ip: IpAddr,
    pub public_port: u16,
    pub local_port: u16,
    pub stun_server: String,
}

/// Characterized NAT mapping behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    /// Full / Restricted Cone NAT: same public port mapped across all destinations (Hole punchable).
    Cone,
    /// Symmetric NAT: different public port allocated per destination (Requires TURN relay).
    Symmetric,
    /// Direct public IP / no NAT translation detected.
    OpenInternet,
}

/// Create a 20-byte RFC 5389 STUN Binding Request packet.
pub fn create_binding_request(transaction_id: &[u8; 12]) -> [u8; 20] {
    let mut buf = [0u8; 20];
    buf[0..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    buf[2..4].copy_from_slice(&0u16.to_be_bytes()); // length = 0 (no attributes)
    buf[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
    buf[8..20].copy_from_slice(transaction_id);
    buf
}

/// Parse an RFC 5389 / RFC 8489 STUN Binding Response and extract public mapped address.
pub fn parse_binding_response(data: &[u8], expected_tid: &[u8; 12]) -> Result<SocketAddr> {
    if data.len() < 20 {
        bail!("STUN response too short: {} bytes (expected >= 20)", data.len());
    }

    let msg_type = u16::from_be_bytes([data[0], data[1]]);
    if msg_type != BINDING_RESPONSE {
        bail!("Expected STUN Binding Response (0x0101), got 0x{:04x}", msg_type);
    }

    let magic_cookie = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if magic_cookie != STUN_MAGIC_COOKIE {
        bail!("Invalid STUN magic cookie: 0x{:08x}", magic_cookie);
    }

    let tid = &data[8..20];
    if tid != expected_tid {
        bail!("STUN transaction ID mismatch");
    }

    let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    let mut offset = 20;
    let end = (20 + msg_len).min(data.len());

    let mut mapped_addr: Option<SocketAddr> = None;

    while offset + 4 <= end {
        let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;

        if offset + attr_len > data.len() {
            break;
        }

        let attr_val = &data[offset..offset + attr_len];
        // Advance offset with 4-byte padding alignment
        offset += (attr_len + 3) & !3;

        if attr_type == ATTR_XOR_MAPPED_ADDRESS && attr_val.len() >= 8 {
            let family = attr_val[1];
            let x_port = u16::from_be_bytes([attr_val[2], attr_val[3]]);
            let port = x_port ^ (STUN_MAGIC_COOKIE >> 16) as u16;

            if family == 0x01 && attr_val.len() >= 8 {
                // IPv4
                let x_ip = u32::from_be_bytes([attr_val[4], attr_val[5], attr_val[6], attr_val[7]]);
                let ip_u32 = x_ip ^ STUN_MAGIC_COOKIE;
                let ip = IpAddr::V4(Ipv4Addr::from(ip_u32));
                return Ok(SocketAddr::new(ip, port));
            } else if family == 0x02 && attr_val.len() >= 20 {
                // IPv6
                let mut xor_mask = [0u8; 16];
                xor_mask[0..4].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
                xor_mask[4..16].copy_from_slice(expected_tid);

                let mut ip_bytes = [0u8; 16];
                for i in 0..16 {
                    ip_bytes[i] = attr_val[4 + i] ^ xor_mask[i];
                }
                let ip = IpAddr::V6(Ipv6Addr::from(ip_bytes));
                return Ok(SocketAddr::new(ip, port));
            }
        } else if attr_type == ATTR_MAPPED_ADDRESS && attr_val.len() >= 8 && mapped_addr.is_none() {
            let family = attr_val[1];
            let port = u16::from_be_bytes([attr_val[2], attr_val[3]]);
            if family == 0x01 && attr_val.len() >= 8 {
                let ip = IpAddr::V4(Ipv4Addr::new(attr_val[4], attr_val[5], attr_val[6], attr_val[7]));
                mapped_addr = Some(SocketAddr::new(ip, port));
            }
        }
    }

    mapped_addr.context("No MAPPED-ADDRESS or XOR-MAPPED-ADDRESS found in STUN response")
}

/// Query a specific STUN server to discover the public reflexive IP and port for a socket.
pub async fn query_stun_server(socket: &UdpSocket, stun_server: &str, timeout_ms: u64) -> Result<SocketAddr> {
    let mut tid = [0u8; 12];
    OsRng.fill_bytes(&mut tid);

    let request = create_binding_request(&tid);
    socket.send_to(&request, stun_server).await
        .context(format!("Failed to send STUN request to {}", stun_server))?;

    let mut buf = [0u8; 1024];
    let (len, _peer) = tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        socket.recv_from(&mut buf),
    )
    .await
    .context(format!("Timed out waiting for STUN response from {}", stun_server))?
    .context("Failed to receive STUN response")?;

    parse_binding_response(&buf[..len], &tid)
}

/// Query public STUN servers with fallback to discover external reflexive address.
pub async fn resolve_public_address() -> Result<StunReflexiveAddress> {
    let socket = UdpSocket::bind("0.0.0.0:0").await
        .context("Failed to bind UDP socket for STUN discovery")?;
    let local_port = socket.local_addr()?.port();

    for &server in DEFAULT_STUN_SERVERS {
        match query_stun_server(&socket, server, 2000).await {
            Ok(mapped) => {
                info!(
                    "STUN discovery succeeded via {}: public endpoint is {}:{}",
                    server, mapped.ip(), mapped.port()
                );
                return Ok(StunReflexiveAddress {
                    public_ip: mapped.ip(),
                    public_port: mapped.port(),
                    local_port,
                    stun_server: server.to_string(),
                });
            }
            Err(e) => {
                warn!("STUN server {} failed: {}. Trying fallback...", server, e);
            }
        }
    }

    bail!("All STUN servers failed to resolve public reflexive address")
}

/// Determine NAT mapping behavior (Cone vs Symmetric) by querying two distinct STUN servers.
pub async fn detect_nat_type() -> Result<NatType> {
    let socket = UdpSocket::bind("0.0.0.0:0").await
        .context("Failed to bind UDP socket for NAT characterization")?;
    let local_port = socket.local_addr()?.port();

    let res1 = query_stun_server(&socket, DEFAULT_STUN_SERVERS[0], 2500).await?;
    let res2 = query_stun_server(&socket, DEFAULT_STUN_SERVERS[1], 2500).await?;

    if res1.port() == local_port && res1.ip().is_loopback() {
        return Ok(NatType::OpenInternet);
    }

    if res1.port() == res2.port() && res1.ip() == res2.ip() {
        info!("NAT Type: Cone NAT (Identical port mapping across destinations: {})", res1.port());
        Ok(NatType::Cone)
    } else {
        warn!(
            "NAT Type: Symmetric NAT (Port changed from {} to {}). Direct P2P hole punch may require TURN relay.",
            res1.port(), res2.port()
        );
        Ok(NatType::Symmetric)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_binding_request_format() {
        let tid = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let req = create_binding_request(&tid);

        assert_eq!(req.len(), 20);
        assert_eq!(&req[0..2], &0x0001u16.to_be_bytes()); // Binding Request
        assert_eq!(&req[2..4], &0x0000u16.to_be_bytes()); // Length 0
        assert_eq!(&req[4..8], &STUN_MAGIC_COOKIE.to_be_bytes());
        assert_eq!(&req[8..20], &tid);
    }

    #[test]
    fn test_parse_xor_mapped_ipv4() {
        let tid = [0xAA; 12];
        let mut resp = Vec::new();

        // Header: Type 0x0101, Length 12, Magic Cookie, TID
        resp.extend_from_slice(&0x0101u16.to_be_bytes());
        resp.extend_from_slice(&12u16.to_be_bytes());
        resp.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        resp.extend_from_slice(&tid);

        // Attribute: XOR-MAPPED-ADDRESS (0x0020), Len 8
        resp.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
        resp.extend_from_slice(&8u16.to_be_bytes());

        // Value: Reserved 0x00, Family 0x01 (IPv4)
        resp.push(0x00);
        resp.push(0x01);

        // Target: Port 54321, IP 198.51.100.42
        let port: u16 = 54321;
        let x_port = port ^ (STUN_MAGIC_COOKIE >> 16) as u16;
        resp.extend_from_slice(&x_port.to_be_bytes());

        let ip_u32 = u32::from_be_bytes([198, 51, 100, 42]);
        let x_ip = ip_u32 ^ STUN_MAGIC_COOKIE;
        resp.extend_from_slice(&x_ip.to_be_bytes());

        let parsed = parse_binding_response(&resp, &tid).expect("Failed to parse XOR-MAPPED-ADDRESS");
        assert_eq!(parsed.port(), 54321);
        assert_eq!(parsed.ip(), IpAddr::V4(Ipv4Addr::new(198, 51, 100, 42)));
    }
}
