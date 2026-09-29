use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use anyhow::{bail, Context, Result};
use md5::{Digest, Md5};
use rand_core::{OsRng, RngCore};
use tokio::net::UdpSocket;
use tracing::{debug, info};

use crate::stun::{
    ATTR_MAPPED_ADDRESS, ATTR_XOR_MAPPED_ADDRESS, STUN_MAGIC_COOKIE,
};

// TURN Message Types (RFC 5766 / RFC 8656)
pub const ALLOCATE_REQUEST: u16 = 0x0003;
pub const ALLOCATE_RESPONSE: u16 = 0x0103;
pub const ALLOCATE_ERROR_RESPONSE: u16 = 0x0113;
pub const REFRESH_REQUEST: u16 = 0x0004;
pub const REFRESH_RESPONSE: u16 = 0x0104;
pub const CREATE_PERMISSION_REQUEST: u16 = 0x0008;
pub const CREATE_PERMISSION_RESPONSE: u16 = 0x0108;
pub const CHANNEL_BIND_REQUEST: u16 = 0x0009;
pub const CHANNEL_BIND_RESPONSE: u16 = 0x0109;
pub const SEND_INDICATION: u16 = 0x0016;
pub const DATA_INDICATION: u16 = 0x0117;

// TURN Attribute Types
pub const ATTR_USERNAME: u16 = 0x0006;
pub const ATTR_MESSAGE_INTEGRITY: u16 = 0x0008;
pub const ATTR_ERROR_CODE: u16 = 0x0009;
pub const ATTR_LIFETIME: u16 = 0x000D;
pub const ATTR_XOR_PEER_ADDRESS: u16 = 0x0012;
pub const ATTR_DATA: u16 = 0x0013;
pub const ATTR_REALM: u16 = 0x0014;
pub const ATTR_NONCE: u16 = 0x0015;
pub const ATTR_XOR_RELAYED_ADDRESS: u16 = 0x0016;
pub const ATTR_REQUESTED_TRANSPORT: u16 = 0x0019;
pub const ATTR_DONT_FRAGMENT: u16 = 0x001A;

/// Configuration for connecting to a TURN relay server.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TurnConfig {
    pub server: String,
    pub username: String,
    pub password: String,
    pub realm: Option<String>,
}

/// Represents an active allocated relay session on a TURN server.
#[derive(Debug, Clone)]
pub struct TurnAllocation {
    pub relayed_addr: SocketAddr,
    pub mapped_addr: Option<SocketAddr>,
    pub lifetime_secs: u32,
    pub realm: String,
    pub nonce: String,
    pub key: [u8; 16],
    pub turn_server: SocketAddr,
}

/// Generate 16-byte key for RFC 5389 / RFC 5766 Long-Term Credential Mechanism:
/// key = MD5(username + ":" + realm + ":" + password)
pub fn generate_long_term_key(username: &str, realm: &str, password: &str) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(format!("{}:{}:{}", username, realm, password).as_bytes());
    hasher.finalize().into()
}

/// Append a STUN / TURN attribute with 4-byte padding alignment.
pub fn append_attribute(buf: &mut Vec<u8>, attr_type: u16, val: &[u8]) {
    buf.extend_from_slice(&attr_type.to_be_bytes());
    buf.extend_from_slice(&(val.len() as u16).to_be_bytes());
    buf.extend_from_slice(val);
    let padding = (4 - (val.len() % 4)) % 4;
    for _ in 0..padding {
        buf.push(0);
    }
}

/// Encode a SocketAddr into a STUN XOR-Address attribute value (used for XOR-PEER-ADDRESS, etc.)
pub fn encode_xor_address(addr: SocketAddr, tid: &[u8; 12]) -> Vec<u8> {
    let mut val = Vec::with_capacity(20);
    val.push(0); // reserved
    match addr {
        SocketAddr::V4(v4) => {
            val.push(1); // IPv4
            let x_port = v4.port() ^ (STUN_MAGIC_COOKIE >> 16) as u16;
            val.extend_from_slice(&x_port.to_be_bytes());
            let x_ip = u32::from(*v4.ip()) ^ STUN_MAGIC_COOKIE;
            val.extend_from_slice(&x_ip.to_be_bytes());
        }
        SocketAddr::V6(v6) => {
            val.push(2); // IPv6
            let x_port = v6.port() ^ (STUN_MAGIC_COOKIE >> 16) as u16;
            val.extend_from_slice(&x_port.to_be_bytes());
            let mut mask = [0u8; 16];
            mask[0..4].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
            mask[4..16].copy_from_slice(tid);
            let octets = v6.ip().octets();
            for i in 0..16 {
                val.push(octets[i] ^ mask[i]);
            }
        }
    }
    val
}

/// Parse an XOR address attribute from raw attribute bytes.
pub fn parse_xor_address(attr_val: &[u8], tid: &[u8; 12]) -> Result<SocketAddr> {
    if attr_val.len() < 8 {
        bail!("XOR-Address attribute too short: {} bytes", attr_val.len());
    }
    let family = attr_val[1];
    let x_port = u16::from_be_bytes([attr_val[2], attr_val[3]]);
    let port = x_port ^ (STUN_MAGIC_COOKIE >> 16) as u16;

    if family == 0x01 && attr_val.len() >= 8 {
        let x_ip = u32::from_be_bytes([attr_val[4], attr_val[5], attr_val[6], attr_val[7]]);
        let ip = std::net::Ipv4Addr::from(x_ip ^ STUN_MAGIC_COOKIE);
        Ok(SocketAddr::new(std::net::IpAddr::V4(ip), port))
    } else if family == 0x02 && attr_val.len() >= 20 {
        let mut mask = [0u8; 16];
        mask[0..4].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        mask[4..16].copy_from_slice(tid);

        let mut ip_bytes = [0u8; 16];
        for i in 0..16 {
            ip_bytes[i] = attr_val[4 + i] ^ mask[i];
        }
        let ip = std::net::Ipv6Addr::from(ip_bytes);
        Ok(SocketAddr::new(std::net::IpAddr::V6(ip), port))
    } else {
        bail!("Unsupported address family: 0x{:02x}", family);
    }
}

/// Calculate and append MESSAGE-INTEGRITY attribute according to RFC 5389 Section 15.4.
pub fn add_message_integrity(msg: &mut Vec<u8>, key: &[u8; 16]) {
    // Current payload len without MI
    let current_payload = msg.len() - 20;
    // Length including the 24-byte MESSAGE-INTEGRITY attribute (4 byte header + 20 byte HMAC-SHA1)
    let total_payload = (current_payload + 24) as u16;
    msg[2..4].copy_from_slice(&total_payload.to_be_bytes());

    let hmac_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key);
    let signature = ring::hmac::sign(&hmac_key, msg);

    append_attribute(msg, ATTR_MESSAGE_INTEGRITY, signature.as_ref());
}

/// Verify MESSAGE-INTEGRITY attribute of an incoming STUN message.
pub fn verify_message_integrity(data: &[u8], key: &[u8; 16]) -> Result<bool> {
    if data.len() < 24 {
        return Ok(false);
    }

    let mut offset = 20;
    let end = data.len();

    while offset + 4 <= end {
        let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        let attr_end = offset + 4 + attr_len;

        if attr_type == ATTR_MESSAGE_INTEGRITY && attr_len == 20 && attr_end <= end {
            let received_mi = &data[offset + 4..attr_end];

            // Reconstruct the message up to MESSAGE-INTEGRITY
            let mut sub_msg = data[..offset].to_vec();
            let sub_payload_len = (offset - 20 + 24) as u16;
            sub_msg[2..4].copy_from_slice(&sub_payload_len.to_be_bytes());

            let hmac_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key);
            let computed = ring::hmac::sign(&hmac_key, &sub_msg);

            return Ok(subtle::ConstantTimeEq::ct_eq(computed.as_ref(), received_mi).into());
        }

        offset = (attr_end + 3) & !3;
    }

    Ok(false)
}

/// Create a TURN SendIndication packet (RFC 5766 Section 10).
pub fn create_send_indication(peer_addr: SocketAddr, data: &[u8]) -> Vec<u8> {
    let mut tid = [0u8; 12];
    OsRng.fill_bytes(&mut tid);

    let mut buf = Vec::with_capacity(64 + data.len());
    // Header: Type = 0x0016 (SendIndication), Len = 0 (placeholder), Magic Cookie, TID
    buf.extend_from_slice(&SEND_INDICATION.to_be_bytes());
    buf.extend_from_slice(&0u16.to_be_bytes());
    buf.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
    buf.extend_from_slice(&tid);

    // XOR-PEER-ADDRESS
    let peer_val = encode_xor_address(peer_addr, &tid);
    append_attribute(&mut buf, ATTR_XOR_PEER_ADDRESS, &peer_val);

    // DATA
    append_attribute(&mut buf, ATTR_DATA, data);

    // Update length
    let payload_len = (buf.len() - 20) as u16;
    buf[2..4].copy_from_slice(&payload_len.to_be_bytes());
    buf
}

/// Parse a TURN DataIndication packet (RFC 5766 Section 10.4).
pub fn parse_data_indication(data: &[u8]) -> Result<(SocketAddr, Vec<u8>)> {
    if data.len() < 20 {
        bail!("DataIndication packet too short: {} bytes", data.len());
    }

    let msg_type = u16::from_be_bytes([data[0], data[1]]);
    if msg_type != DATA_INDICATION {
        bail!("Expected DataIndication (0x0117), got 0x{:04x}", msg_type);
    }

    let mut tid = [0u8; 12];
    tid.copy_from_slice(&data[8..20]);

    let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    let limit = (20 + msg_len).min(data.len());
    let mut offset = 20;

    let mut peer_addr: Option<SocketAddr> = None;
    let mut payload: Option<Vec<u8>> = None;

    while offset + 4 <= limit {
        let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;

        if offset + attr_len > limit {
            break;
        }

        let attr_val = &data[offset..offset + attr_len];
        offset = (offset + attr_len + 3) & !3;

        match attr_type {
            ATTR_XOR_PEER_ADDRESS => {
                peer_addr = parse_xor_address(attr_val, &tid).ok();
            }
            ATTR_DATA => {
                payload = Some(attr_val.to_vec());
            }
            _ => {}
        }
    }

    let addr = peer_addr.context("Missing XOR-PEER-ADDRESS in DataIndication")?;
    let body = payload.context("Missing DATA in DataIndication")?;
    Ok((addr, body))
}

/// TURN client managing relay allocations and permissions.
pub struct TurnClient {
    pub socket: Arc<UdpSocket>,
}

impl TurnClient {
    pub fn new(socket: Arc<UdpSocket>) -> Self {
        Self { socket }
    }

    /// Allocate a relay address on the TURN server (RFC 5766 Section 6).
    pub async fn allocate(&self, config: &TurnConfig) -> Result<TurnAllocation> {
        let server_addr: SocketAddr = tokio::net::lookup_host(&config.server)
            .await
            .context(format!("Failed to resolve TURN server {}", config.server))?
            .next()
            .context(format!("No IP address found for TURN server {}", config.server))?;

        // 1. Initial unauthenticated Allocate request to elicit 401 with REALM and NONCE
        let mut tid1 = [0u8; 12];
        OsRng.fill_bytes(&mut tid1);

        let mut req1 = Vec::with_capacity(32);
        req1.extend_from_slice(&ALLOCATE_REQUEST.to_be_bytes());
        req1.extend_from_slice(&0u16.to_be_bytes());
        req1.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        req1.extend_from_slice(&tid1);

        // REQUESTED-TRANSPORT: Protocol 17 (UDP)
        append_attribute(&mut req1, ATTR_REQUESTED_TRANSPORT, &[17, 0, 0, 0]);
        let len1 = (req1.len() - 20) as u16;
        req1[2..4].copy_from_slice(&len1.to_be_bytes());

        self.socket.send_to(&req1, server_addr).await
            .context(format!("Failed to send initial Allocate request to {}", server_addr))?;

        let mut buf = [0u8; 2048];
        let (len, _) = tokio::time::timeout(Duration::from_millis(3000), self.socket.recv_from(&mut buf))
            .await
            .context("Timed out waiting for TURN 401 response")?
            .context("Failed to receive from TURN server")?;

        let resp_data = &buf[..len];
        if resp_data.len() < 20 {
            bail!("TURN response too short");
        }

        let resp_type = u16::from_be_bytes([resp_data[0], resp_data[1]]);
        let (realm, nonce) = if resp_type == ALLOCATE_ERROR_RESPONSE {
            self.extract_realm_nonce(resp_data)?
        } else if resp_type == ALLOCATE_RESPONSE {
            // Already allocated without auth (e.g. mock server)
            let (relayed, mapped, lifetime) = self.extract_allocation_info(resp_data, &tid1)?;
            return Ok(TurnAllocation {
                relayed_addr: relayed,
                mapped_addr: mapped,
                lifetime_secs: lifetime,
                realm: String::new(),
                nonce: String::new(),
                key: [0u8; 16],
                turn_server: server_addr,
            });
        } else {
            bail!("Unexpected response type from TURN server: 0x{:04x}", resp_type);
        };

        // 2. Authenticated Allocate request with USERNAME, REALM, NONCE, LIFETIME, and MESSAGE-INTEGRITY
        let key = generate_long_term_key(&config.username, &realm, &config.password);

        let mut tid2 = [0u8; 12];
        OsRng.fill_bytes(&mut tid2);

        let mut req2 = Vec::with_capacity(128);
        req2.extend_from_slice(&ALLOCATE_REQUEST.to_be_bytes());
        req2.extend_from_slice(&0u16.to_be_bytes());
        req2.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        req2.extend_from_slice(&tid2);

        // Attributes
        append_attribute(&mut req2, ATTR_REQUESTED_TRANSPORT, &[17, 0, 0, 0]);
        append_attribute(&mut req2, ATTR_LIFETIME, &600u32.to_be_bytes()); // 10 minutes
        append_attribute(&mut req2, ATTR_USERNAME, config.username.as_bytes());
        append_attribute(&mut req2, ATTR_REALM, realm.as_bytes());
        append_attribute(&mut req2, ATTR_NONCE, nonce.as_bytes());

        // Append MESSAGE-INTEGRITY
        add_message_integrity(&mut req2, &key);

        self.socket.send_to(&req2, server_addr).await
            .context("Failed to send authenticated Allocate request")?;

        let (len2, _) = tokio::time::timeout(Duration::from_millis(3000), self.socket.recv_from(&mut buf))
            .await
            .context("Timed out waiting for authenticated TURN response")?
            .context("Failed to receive authenticated TURN response")?;

        let resp2 = &buf[..len2];
        let resp2_type = u16::from_be_bytes([resp2[0], resp2[1]]);
        if resp2_type != ALLOCATE_RESPONSE {
            bail!("TURN allocation failed with response type 0x{:04x}", resp2_type);
        }

        let (relayed_addr, mapped_addr, lifetime_secs) = self.extract_allocation_info(resp2, &tid2)?;
        info!(
            "TURN allocation successful on {}: relayed endpoint is {}",
            server_addr, relayed_addr
        );

        Ok(TurnAllocation {
            relayed_addr,
            mapped_addr,
            lifetime_secs,
            realm,
            nonce,
            key,
            turn_server: server_addr,
        })
    }

    /// Create permissions for peer IP on the allocated TURN relay (RFC 5766 Section 9).
    pub async fn create_permission(
        &self,
        allocation: &TurnAllocation,
        peer_addr: SocketAddr,
        username: &str,
    ) -> Result<()> {
        let mut tid = [0u8; 12];
        OsRng.fill_bytes(&mut tid);

        let mut req = Vec::with_capacity(128);
        req.extend_from_slice(&CREATE_PERMISSION_REQUEST.to_be_bytes());
        req.extend_from_slice(&0u16.to_be_bytes());
        req.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        req.extend_from_slice(&tid);

        let peer_val = encode_xor_address(peer_addr, &tid);
        append_attribute(&mut req, ATTR_XOR_PEER_ADDRESS, &peer_val);
        append_attribute(&mut req, ATTR_USERNAME, username.as_bytes());
        append_attribute(&mut req, ATTR_REALM, allocation.realm.as_bytes());
        append_attribute(&mut req, ATTR_NONCE, allocation.nonce.as_bytes());

        add_message_integrity(&mut req, &allocation.key);

        self.socket.send_to(&req, allocation.turn_server).await
            .context("Failed to send CreatePermission request")?;

        let mut buf = [0u8; 1024];
        let (_len, _) = tokio::time::timeout(Duration::from_millis(2000), self.socket.recv_from(&mut buf))
            .await
            .context("Timed out waiting for CreatePermission response")?
            .context("Failed to receive CreatePermission response")?;

        let resp_type = u16::from_be_bytes([buf[0], buf[1]]);
        if resp_type != CREATE_PERMISSION_RESPONSE {
            bail!("CreatePermission failed with response code 0x{:04x}", resp_type);
        }

        debug!("TURN permission created successfully for peer {}", peer_addr);
        Ok(())
    }

    fn extract_realm_nonce(&self, data: &[u8]) -> Result<(String, String)> {
        let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;
        let limit = (20 + msg_len).min(data.len());
        let mut offset = 20;

        let mut realm: Option<String> = None;
        let mut nonce: Option<String> = None;

        while offset + 4 <= limit {
            let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            offset += 4;
            if offset + attr_len > limit {
                break;
            }
            let val = &data[offset..offset + attr_len];
            offset = (offset + attr_len + 3) & !3;

            if attr_type == ATTR_REALM {
                realm = Some(String::from_utf8_lossy(val).into_owned());
            } else if attr_type == ATTR_NONCE {
                nonce = Some(String::from_utf8_lossy(val).into_owned());
            }
        }

        let r = realm.context("Missing REALM in TURN 401 error response")?;
        let n = nonce.context("Missing NONCE in TURN 401 error response")?;
        Ok((r, n))
    }

    fn extract_allocation_info(
        &self,
        data: &[u8],
        tid: &[u8; 12],
    ) -> Result<(SocketAddr, Option<SocketAddr>, u32)> {
        let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;
        let limit = (20 + msg_len).min(data.len());
        let mut offset = 20;

        let mut relayed_addr: Option<SocketAddr> = None;
        let mut mapped_addr: Option<SocketAddr> = None;
        let mut lifetime_secs: u32 = 600;

        while offset + 4 <= limit {
            let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            offset += 4;
            if offset + attr_len > limit {
                break;
            }
            let val = &data[offset..offset + attr_len];
            offset = (offset + attr_len + 3) & !3;

            match attr_type {
                ATTR_XOR_RELAYED_ADDRESS => {
                    relayed_addr = parse_xor_address(val, tid).ok();
                }
                ATTR_XOR_MAPPED_ADDRESS | ATTR_MAPPED_ADDRESS => {
                    mapped_addr = parse_xor_address(val, tid).ok();
                }
                ATTR_LIFETIME => {
                    if val.len() >= 4 {
                        lifetime_secs = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                    }
                }
                _ => {}
            }
        }

        let relayed = relayed_addr.context("Missing XOR-RELAYED-ADDRESS in Allocate response")?;
        Ok((relayed, mapped_addr, lifetime_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_long_term_key_generation() {
        let key = generate_long_term_key("user", "example.com", "pass");
        assert_eq!(key.len(), 16);
        // Deterministic check
        let key2 = generate_long_term_key("user", "example.com", "pass");
        assert_eq!(key, key2);
    }

    #[test]
    fn test_xor_address_encoding_and_parsing() {
        let tid = [0x77; 12];
        let original: SocketAddr = "198.51.100.25:54321".parse().unwrap();
        let encoded = encode_xor_address(original, &tid);
        let parsed = parse_xor_address(&encoded, &tid).unwrap();
        assert_eq!(original, parsed);

        let v6_original: SocketAddr = "[2001:db8::dead:beef]:49152".parse().unwrap();
        let v6_encoded = encode_xor_address(v6_original, &tid);
        let v6_parsed = parse_xor_address(&v6_encoded, &tid).unwrap();
        assert_eq!(v6_original, v6_parsed);
    }

    #[test]
    fn test_message_integrity_signature_and_verification() {
        let key = generate_long_term_key("testuser", "ward.relay", "secret123");
        let mut msg = Vec::new();
        msg.extend_from_slice(&ALLOCATE_REQUEST.to_be_bytes());
        msg.extend_from_slice(&0u16.to_be_bytes()); // length
        msg.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        msg.extend_from_slice(&[0x11; 12]); // tid

        append_attribute(&mut msg, ATTR_USERNAME, b"testuser");
        append_attribute(&mut msg, ATTR_REALM, b"ward.relay");

        add_message_integrity(&mut msg, &key);

        let valid = verify_message_integrity(&msg, &key).unwrap();
        assert!(valid, "MESSAGE-INTEGRITY must verify with matching key");

        let bad_key = generate_long_term_key("testuser", "ward.relay", "wrong_pass");
        let invalid = verify_message_integrity(&msg, &bad_key).unwrap();
        assert!(!invalid, "MESSAGE-INTEGRITY must fail with wrong key");
    }

    #[test]
    fn test_send_and_data_indication_roundtrip() {
        let peer: SocketAddr = "203.0.113.10:60000".parse().unwrap();
        let payload = b"WARD video frame packet over TURN relay";

        // Create SendIndication
        let send_pkt = create_send_indication(peer, payload);
        assert_eq!(u16::from_be_bytes([send_pkt[0], send_pkt[1]]), SEND_INDICATION);

        // Turn server relays this as DataIndication (same payload and peer)
        let mut data_pkt = send_pkt.clone();
        data_pkt[0..2].copy_from_slice(&DATA_INDICATION.to_be_bytes());

        let (extracted_peer, extracted_payload) = parse_data_indication(&data_pkt).unwrap();
        assert_eq!(extracted_peer, peer);
        assert_eq!(extracted_payload, payload);
    }
}
