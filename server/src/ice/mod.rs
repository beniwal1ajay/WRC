use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use anyhow::{bail, Context, Result};
use rand_core::{OsRng, RngCore};
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

use crate::protocol::IceCandidate;
use crate::stun::{
    create_binding_request, create_binding_response, parse_binding_response,
    query_stun_server, BINDING_REQUEST, DEFAULT_STUN_SERVERS,
};

pub const CANDIDATE_TYPE_HOST: u8 = 1;
pub const CANDIDATE_TYPE_SRFLX: u8 = 2;
pub const CANDIDATE_TYPE_RELAY: u8 = 3;

pub const PROTOCOL_UDP: u8 = 1;
pub const PROTOCOL_TCP: u8 = 2;

/// Calculate RFC 8445 ICE candidate priority:
/// priority = (type_preference << 24) | (local_preference << 8) | (256 - component_id)
pub fn calculate_candidate_priority(candidate_type: u8, local_pref: u16, component_id: u8) -> u32 {
    let type_pref: u32 = match candidate_type {
        CANDIDATE_TYPE_HOST => 126,
        CANDIDATE_TYPE_SRFLX => 100,
        CANDIDATE_TYPE_RELAY => 0,
        _ => 50,
    };
    (type_pref << 24) | ((local_pref as u32) << 8) | (256 - component_id as u32)
}

/// Calculate RFC 8445 candidate pair priority:
/// pair_priority = (2^32 * min(G, D)) + (2 * max(G, D)) + (1 if G > D else 0)
/// where G is controlling agent priority and D is controlled agent priority.
pub fn calculate_pair_priority(controlling_prio: u32, controlled_prio: u32) -> u64 {
    let g = controlling_prio as u64;
    let d = controlled_prio as u64;
    let min_gd = g.min(d);
    let max_gd = g.max(d);
    let s = if g > d { 1 } else { 0 };
    ((1u64 << 32) * min_gd) + (2 * max_gd) + s
}

/// Represents an ICE candidate pair formed for connectivity checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePair {
    pub local: IceCandidate,
    pub remote: IceCandidate,
    pub priority: u64,
}

/// ICE agent managing local and remote candidates, pair formation, and connectivity checks.
pub struct IceAgent {
    pub socket: Arc<UdpSocket>,
    pub local_candidates: Vec<IceCandidate>,
    pub remote_candidates: Vec<IceCandidate>,
    pub is_controlling: bool,
    pub turn_allocation: Option<crate::turn::TurnAllocation>,
}

impl IceAgent {
    /// Create a new ICE agent wrapping an existing UDP socket.
    pub fn new(socket: Arc<UdpSocket>, is_controlling: bool) -> Self {
        Self {
            socket,
            local_candidates: Vec::new(),
            remote_candidates: Vec::new(),
            is_controlling,
            turn_allocation: None,
        }
    }

    /// Bind a UDP socket to the given address (e.g. "0.0.0.0:0") and create an ICE agent.
    pub async fn bind(addr: &str, is_controlling: bool) -> Result<Self> {
        let socket = UdpSocket::bind(addr).await
            .context(format!("Failed to bind UDP socket on {}", addr))?;
        Ok(Self::new(Arc::new(socket), is_controlling))
    }

    /// Return the local socket address.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr().context("Failed to get local UDP address")
    }

    /// Gather local candidates:
    /// 1. Host candidates from local network interfaces.
    /// 2. Server reflexive (srflx) candidate via public STUN query on the same socket.
    /// 3. Relay candidate via TURN allocation (if configured).
    pub async fn gather_candidates(
        &mut self,
        stun_server: Option<&str>,
        turn_config: Option<&crate::turn::TurnConfig>,
    ) -> Result<Vec<IceCandidate>> {
        let mut candidates = Vec::new();
        let local_port = self.local_addr()?.port();

        // 1. Host candidates
        let interfaces = crate::network::list_network_interfaces();
        let mut host_count = 0u16;
        for (_ifname, ip) in &interfaces {
            // Prefer non-loopback interfaces, but include loopback if no other interfaces exist
            let local_pref = if ip.is_loopback() { 1000 } else { 65535 - host_count * 100 };
            host_count += 1;

            let priority = calculate_candidate_priority(CANDIDATE_TYPE_HOST, local_pref, 1);
            candidates.push(IceCandidate {
                candidate_type: CANDIDATE_TYPE_HOST,
                protocol: PROTOCOL_UDP,
                ip: ip.to_string(),
                port: local_port,
                priority,
            });
        }

        // If no interfaces found via list_network_interfaces, add fallback 0.0.0.0 or 127.0.0.1
        if candidates.is_empty() {
            let priority = calculate_candidate_priority(CANDIDATE_TYPE_HOST, 65535, 1);
            candidates.push(IceCandidate {
                candidate_type: CANDIDATE_TYPE_HOST,
                protocol: PROTOCOL_UDP,
                ip: "127.0.0.1".to_string(),
                port: local_port,
                priority,
            });
        }

        // 2. Server Reflexive (srflx) candidate via STUN
        let stun_target = stun_server.unwrap_or(DEFAULT_STUN_SERVERS[0]);
        match query_stun_server(&self.socket, stun_target, 2000).await {
            Ok(mapped_addr) => {
                info!(
                    "ICE gathered srflx candidate: {}:{} (via {})",
                    mapped_addr.ip(), mapped_addr.port(), stun_target
                );
                let priority = calculate_candidate_priority(CANDIDATE_TYPE_SRFLX, 65535, 1);
                candidates.push(IceCandidate {
                    candidate_type: CANDIDATE_TYPE_SRFLX,
                    protocol: PROTOCOL_UDP,
                    ip: mapped_addr.ip().to_string(),
                    port: mapped_addr.port(),
                    priority,
                });
            }
            Err(e) => {
                warn!("ICE srflx candidate gathering failed via {}: {}", stun_target, e);
            }
        }

        // 3. Relay candidate via TURN
        if let Some(config) = turn_config {
            let turn_client = crate::turn::TurnClient::new(self.socket.clone());
            match turn_client.allocate(config).await {
                Ok(allocation) => {
                    info!("ICE gathered Relay candidate: {}", allocation.relayed_addr);
                    let priority = calculate_candidate_priority(CANDIDATE_TYPE_RELAY, 65535, 1);
                    candidates.push(IceCandidate {
                        candidate_type: CANDIDATE_TYPE_RELAY,
                        protocol: PROTOCOL_UDP,
                        ip: allocation.relayed_addr.ip().to_string(),
                        port: allocation.relayed_addr.port(),
                        priority,
                    });
                    self.turn_allocation = Some(allocation);
                }
                Err(e) => {
                    warn!("ICE relay candidate gathering failed via {}: {}", config.server, e);
                }
            }
        }

        self.local_candidates = candidates.clone();
        Ok(candidates)
    }

    /// Add or update remote peer candidates received via protocol signalling.
    pub fn set_remote_candidates(&mut self, candidates: Vec<IceCandidate>) {
        self.remote_candidates = candidates;
    }

    /// Form candidate pairs from local and remote candidates and sort by RFC 8445 priority (descending).
    pub fn form_candidate_pairs(&self) -> Vec<CandidatePair> {
        let mut pairs = Vec::new();

        for local in &self.local_candidates {
            for remote in &self.remote_candidates {
                // Must match protocol
                if local.protocol != remote.protocol {
                    continue;
                }

                let priority = if self.is_controlling {
                    calculate_pair_priority(local.priority, remote.priority)
                } else {
                    calculate_pair_priority(remote.priority, local.priority)
                };

                pairs.push(CandidatePair {
                    local: local.clone(),
                    remote: remote.clone(),
                    priority,
                });
            }
        }

        // Sort descending by pair priority
        pairs.sort_by(|a, b| b.priority.cmp(&a.priority));
        pairs
    }

    /// Handle an incoming UDP packet. If it is a STUN Binding Request, send a STUN Binding Response.
    /// Returns Ok(true) if the packet was handled as a STUN request.
    pub async fn handle_incoming_stun_packet(&self, data: &[u8], src: SocketAddr) -> Result<bool> {
        if data.len() >= 20 {
            let msg_type = u16::from_be_bytes([data[0], data[1]]);
            if msg_type == BINDING_REQUEST {
                let mut tid = [0u8; 12];
                tid.copy_from_slice(&data[8..20]);
                let resp = create_binding_response(&tid, src);
                let _ = self.socket.send_to(&resp, src).await;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Check connectivity for a candidate pair by sending a STUN binding request and awaiting response.
    pub async fn check_pair_connectivity(&self, pair: &CandidatePair, timeout_ms: u64) -> Result<()> {
        let target_addr: SocketAddr = format!("{}:{}", pair.remote.ip, pair.remote.port)
            .parse()
            .context(format!("Invalid remote candidate address {}:{}", pair.remote.ip, pair.remote.port))?;

        let mut tid = [0u8; 12];
        OsRng.fill_bytes(&mut tid);
        let request = create_binding_request(&tid);

        // Send probe
        self.socket.send_to(&request, target_addr).await
            .context(format!("Failed to send STUN probe to {}", target_addr))?;

        // Wait for response or incoming requests
        let start = std::time::Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        let mut buf = [0u8; 1024];

        while start.elapsed() < timeout {
            let remaining = timeout.saturating_sub(start.elapsed());
            match tokio::time::timeout(remaining, self.socket.recv_from(&mut buf)).await {
                Ok(Ok((len, peer))) => {
                    // Check if peer sent a binding request
                    if self.handle_incoming_stun_packet(&buf[..len], peer).await.unwrap_or(false) {
                        continue;
                    }

                    // Check if this is the binding response for our transaction ID
                    if let Ok(mapped) = parse_binding_response(&buf[..len], &tid) {
                        debug!("ICE connectivity verified for pair: remote={}, mapped={}", target_addr, mapped);
                        return Ok(());
                    }
                }
                Ok(Err(e)) => {
                    bail!("UDP recv error during ICE check: {}", e);
                }
                Err(_) => {
                    break;
                }
            }
        }

        bail!("ICE connectivity check timed out for pair {}:{}", pair.remote.ip, pair.remote.port)
    }

    /// Perform ICE connectivity checks across all candidate pairs in priority order.
    /// Returns the first nominated candidate pair that succeeds.
    pub async fn perform_ice_checks(&self, timeout_per_pair_ms: u64) -> Result<CandidatePair> {
        let pairs = self.form_candidate_pairs();
        if pairs.is_empty() {
            bail!("No candidate pairs available for ICE connectivity checks");
        }

        info!("Starting ICE connectivity checks across {} candidate pairs...", pairs.len());

        for (idx, pair) in pairs.iter().enumerate() {
            info!(
                "Testing candidate pair #{}: local={}:{} ({}) <-> remote={}:{} ({}) [prio={}]",
                idx + 1,
                pair.local.ip, pair.local.port, pair.local.candidate_type,
                pair.remote.ip, pair.remote.port, pair.remote.candidate_type,
                pair.priority
            );

            match self.check_pair_connectivity(pair, timeout_per_pair_ms).await {
                Ok(()) => {
                    info!(
                        "ICE candidate pair #{} nominated successfully! P2P path verified.",
                        idx + 1
                    );
                    return Ok(pair.clone());
                }
                Err(e) => {
                    debug!("ICE candidate pair #{} check failed: {}. Trying next pair...", idx + 1, e);
                }
            }
        }

        bail!("All ICE candidate pairs failed connectivity checks")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candidate_priority_ordering() {
        let host_prio = calculate_candidate_priority(CANDIDATE_TYPE_HOST, 65535, 1);
        let srflx_prio = calculate_candidate_priority(CANDIDATE_TYPE_SRFLX, 65535, 1);
        let relay_prio = calculate_candidate_priority(CANDIDATE_TYPE_RELAY, 65535, 1);

        // Host priority must be higher than srflx, which is higher than relay
        assert!(host_prio > srflx_prio);
        assert!(srflx_prio > relay_prio);
    }

    #[tokio::test]
    async fn test_candidate_pair_priority_and_sorting() {
        let local_host = IceCandidate {
            candidate_type: CANDIDATE_TYPE_HOST,
            protocol: PROTOCOL_UDP,
            ip: "192.168.1.10".to_string(),
            port: 5000,
            priority: calculate_candidate_priority(CANDIDATE_TYPE_HOST, 65535, 1),
        };
        let local_srflx = IceCandidate {
            candidate_type: CANDIDATE_TYPE_SRFLX,
            protocol: PROTOCOL_UDP,
            ip: "203.0.113.1".to_string(),
            port: 5000,
            priority: calculate_candidate_priority(CANDIDATE_TYPE_SRFLX, 65535, 1),
        };

        let remote_host = IceCandidate {
            candidate_type: CANDIDATE_TYPE_HOST,
            protocol: PROTOCOL_UDP,
            ip: "192.168.1.20".to_string(),
            port: 6000,
            priority: calculate_candidate_priority(CANDIDATE_TYPE_HOST, 65535, 1),
        };
        let remote_srflx = IceCandidate {
            candidate_type: CANDIDATE_TYPE_SRFLX,
            protocol: PROTOCOL_UDP,
            ip: "203.0.113.2".to_string(),
            port: 6000,
            priority: calculate_candidate_priority(CANDIDATE_TYPE_SRFLX, 65535, 1),
        };

        let std_sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        std_sock.set_nonblocking(true).unwrap();
        let tokio_sock = tokio::net::UdpSocket::from_std(std_sock).unwrap();
        let mut agent = IceAgent::new(Arc::new(tokio_sock), true);

        agent.local_candidates = vec![local_host.clone(), local_srflx.clone()];
        agent.remote_candidates = vec![remote_host.clone(), remote_srflx.clone()];

        let pairs = agent.form_candidate_pairs();
        assert_eq!(pairs.len(), 4);

        // First pair must be Host <-> Host since both have highest individual priorities
        assert_eq!(pairs[0].local.candidate_type, CANDIDATE_TYPE_HOST);
        assert_eq!(pairs[0].remote.candidate_type, CANDIDATE_TYPE_HOST);
        assert!(pairs[0].priority >= pairs[1].priority);
    }

    #[tokio::test]
    async fn test_ice_connectivity_check_direct() {
        let agent1 = IceAgent::bind("127.0.0.1:0", true).await.unwrap();
        let agent2 = IceAgent::bind("127.0.0.1:0", false).await.unwrap();

        let addr1 = agent1.local_addr().unwrap();
        let addr2 = agent2.local_addr().unwrap();

        let cand1 = IceCandidate {
            candidate_type: CANDIDATE_TYPE_HOST,
            protocol: PROTOCOL_UDP,
            ip: addr1.ip().to_string(),
            port: addr1.port(),
            priority: calculate_candidate_priority(CANDIDATE_TYPE_HOST, 65535, 1),
        };

        let cand2 = IceCandidate {
            candidate_type: CANDIDATE_TYPE_HOST,
            protocol: PROTOCOL_UDP,
            ip: addr2.ip().to_string(),
            port: addr2.port(),
            priority: calculate_candidate_priority(CANDIDATE_TYPE_HOST, 65535, 1),
        };

        let pair1 = CandidatePair {
            local: cand1.clone(),
            remote: cand2.clone(),
            priority: calculate_pair_priority(cand1.priority, cand2.priority),
        };

        // Spawn responder on agent2
        let socket2 = agent2.socket.clone();
        let handle = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let (_len, src) = socket2.recv_from(&mut buf).await.unwrap();
            let mut tid = [0u8; 12];
            tid.copy_from_slice(&buf[8..20]);
            let resp = create_binding_response(&tid, src);
            socket2.send_to(&resp, src).await.unwrap();
        });

        // Run check from agent1
        let result = agent1.check_pair_connectivity(&pair1, 2000).await;
        assert!(result.is_ok(), "ICE connectivity check failed: {:?}", result.err());

        handle.await.unwrap();
    }
}
