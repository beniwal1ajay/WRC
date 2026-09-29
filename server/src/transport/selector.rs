use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use anyhow::{bail, Result};
use tracing::{debug, info};

use super::TransportType;

/// Connection tier for automatic network selection, ordered by performance and cost preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum ConnectionTier {
    /// 1. Direct Local Area Network (lowest latency, highest throughput, unmetered).
    LanDirect = 1,
    /// 2. Direct VPN Tunnel (Tailscale, WireGuard, direct encrypted IP).
    VpnDirect = 2,
    /// 3. Direct Peer-to-Peer over NAT via ICE / STUN hole punching.
    P2pIce = 3,
    /// 4. Relayed connection via TURN server (guaranteed fallback when hole-punching fails).
    TurnRelay = 4,
}

impl ConnectionTier {
    pub fn preference_score(&self) -> u32 {
        match self {
            Self::LanDirect => 1000,
            Self::VpnDirect => 800,
            Self::P2pIce => 600,
            Self::TurnRelay => 200,
        }
    }

    pub fn to_transport_type(&self) -> TransportType {
        match self {
            Self::LanDirect => TransportType::Lan,
            Self::VpnDirect => TransportType::Vpn,
            Self::P2pIce => TransportType::DirectP2p,
            Self::TurnRelay => TransportType::Relay,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::LanDirect => "Direct LAN (TLS)",
            Self::VpnDirect => "VPN Tunnel",
            Self::P2pIce => "Direct P2P (ICE)",
            Self::TurnRelay => "TURN Relay",
        }
    }
}

/// An endpoint candidate evaluated by the connection selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateEndpoint {
    pub tier: ConnectionTier,
    pub addr: SocketAddr,
    pub label: String,
}

/// Result of probing an endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub endpoint: CandidateEndpoint,
    pub rtt_ms: u64,
    pub success: bool,
}

/// Helper to classify an IP address into its probable ConnectionTier.
pub fn classify_endpoint_ip(ip: &IpAddr, ifname_hint: Option<&str>) -> ConnectionTier {
    if let Some(ifname) = ifname_hint {
        if ifname.starts_with("tailscale") || ifname.starts_with("wg") || ifname.starts_with("tun") {
            return ConnectionTier::VpnDirect;
        }
    }

    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            // Tailscale CGNAT subnet (100.64.0.0/10: 100.64.0.0 - 100.127.255.255)
            if octets[0] == 100 && (octets[1] >= 64 && octets[1] <= 127) {
                return ConnectionTier::VpnDirect;
            }
            // Standard RFC 1918 Private LAN
            if octets[0] == 10
                || (octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31))
                || (octets[0] == 192 && octets[1] == 168)
                || octets[0] == 127
            {
                return ConnectionTier::LanDirect;
            }
            // Public IPv4
            ConnectionTier::P2pIce
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                ConnectionTier::LanDirect
            } else {
                ConnectionTier::P2pIce
            }
        }
    }
}

/// Automatic connection selector that ranks and tests candidates.
pub struct ConnectionSelector;

impl ConnectionSelector {
    /// Sorts candidates by their connection tier preference score (highest first).
    pub fn rank_candidates(candidates: &mut [CandidateEndpoint]) {
        candidates.sort_by(|a, b| {
            b.tier.preference_score().cmp(&a.tier.preference_score())
        });
    }

    /// Probe a single TCP candidate endpoint to measure round-trip latency.
    pub async fn probe_tcp_endpoint(endpoint: &CandidateEndpoint, timeout: Duration) -> ProbeResult {
        let start = Instant::now();
        match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(endpoint.addr)).await {
            Ok(Ok(_stream)) => {
                let rtt_ms = start.elapsed().as_millis() as u64;
                debug!("Probe succeeded for {}: {}ms", endpoint.label, rtt_ms);
                ProbeResult {
                    endpoint: endpoint.clone(),
                    rtt_ms,
                    success: true,
                }
            }
            Ok(Err(_e)) => {
                debug!("Probe failed for {}: connect error", endpoint.label);
                ProbeResult {
                    endpoint: endpoint.clone(),
                    rtt_ms: u64::MAX,
                    success: false,
                }
            }
            Err(_) => {
                debug!("Probe timed out for {}", endpoint.label);
                ProbeResult {
                    endpoint: endpoint.clone(),
                    rtt_ms: u64::MAX,
                    success: false,
                }
            }
        }
    }

    /// Select the best reachable connection candidate according to the multi-tier hierarchy:
    /// Tier 1 (LAN Direct) -> Tier 2 (VPN Direct) -> Tier 3 (P2P ICE) -> Tier 4 (TURN Relay).
    ///
    /// Evaluates candidates with prioritized parallel probing, giving higher-priority tiers
    /// the first opportunity to win.
    pub async fn select_best_endpoint(
        candidates: Vec<CandidateEndpoint>,
        probe_timeout: Duration,
    ) -> Result<ProbeResult> {
        if candidates.is_empty() {
            bail!("No connection candidates provided to selector");
        }

        let mut ranked = candidates;
        Self::rank_candidates(&mut ranked);

        info!("ConnectionSelector evaluating {} candidate endpoints in priority order...", ranked.len());

        // Group by tier
        let mut tiers: Vec<Vec<CandidateEndpoint>> = Vec::new();
        for candidate in ranked {
            if let Some(group) = tiers.last_mut() {
                if group[0].tier == candidate.tier {
                    group.push(candidate);
                    continue;
                }
            }
            tiers.push(vec![candidate]);
        }

        // Test tier by tier (highest priority tier tested first with fast parallel probing within the tier)
        for tier_group in tiers {
            let tier_name = tier_group[0].tier.display_name();
            debug!("Probing tier {} ({} candidate(s))...", tier_name, tier_group.len());

            let mut probe_futs = Vec::new();
            for ep in &tier_group {
                probe_futs.push(Self::probe_tcp_endpoint(ep, probe_timeout));
            }

            let results = futures_util::future::join_all(probe_futs).await;
            let mut successful: Vec<ProbeResult> = results.into_iter().filter(|r| r.success).collect();

            if !successful.is_empty() {
                // If multiple in same tier succeeded, pick the lowest RTT
                successful.sort_by_key(|r| r.rtt_ms);
                let winner = successful.remove(0);
                info!(
                    "ConnectionSelector selected tier '{}' via {} (RTT: {}ms)",
                    winner.endpoint.tier.display_name(),
                    winner.endpoint.addr,
                    winner.rtt_ms
                );
                return Ok(winner);
            }
        }

        bail!("All connection candidate tiers failed reachable connectivity checks")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_tier_priority() {
        assert!(ConnectionTier::LanDirect.preference_score() > ConnectionTier::VpnDirect.preference_score());
        assert!(ConnectionTier::VpnDirect.preference_score() > ConnectionTier::P2pIce.preference_score());
        assert!(ConnectionTier::P2pIce.preference_score() > ConnectionTier::TurnRelay.preference_score());
    }

    #[test]
    fn test_ip_classification() {
        let lan_ip: IpAddr = "192.168.1.50".parse().unwrap();
        assert_eq!(classify_endpoint_ip(&lan_ip, None), ConnectionTier::LanDirect);

        let vpn_tailscale: IpAddr = "100.85.12.34".parse().unwrap();
        assert_eq!(classify_endpoint_ip(&vpn_tailscale, None), ConnectionTier::VpnDirect);

        let vpn_wireguard: IpAddr = "10.0.0.1".parse().unwrap();
        assert_eq!(classify_endpoint_ip(&vpn_wireguard, Some("wg0")), ConnectionTier::VpnDirect);

        let public_ip: IpAddr = "203.0.113.88".parse().unwrap();
        assert_eq!(classify_endpoint_ip(&public_ip, None), ConnectionTier::P2pIce);
    }

    #[test]
    fn test_candidate_ranking() {
        let mut candidates = vec![
            CandidateEndpoint {
                tier: ConnectionTier::TurnRelay,
                addr: "1.2.3.4:3478".parse().unwrap(),
                label: "Relay".to_string(),
            },
            CandidateEndpoint {
                tier: ConnectionTier::LanDirect,
                addr: "192.168.1.100:51820".parse().unwrap(),
                label: "LAN".to_string(),
            },
            CandidateEndpoint {
                tier: ConnectionTier::VpnDirect,
                addr: "100.64.1.2:51820".parse().unwrap(),
                label: "VPN".to_string(),
            },
        ];

        ConnectionSelector::rank_candidates(&mut candidates);
        assert_eq!(candidates[0].tier, ConnectionTier::LanDirect);
        assert_eq!(candidates[1].tier, ConnectionTier::VpnDirect);
        assert_eq!(candidates[2].tier, ConnectionTier::TurnRelay);
    }

    #[tokio::test]
    async fn test_selector_picks_highest_priority_working_tier() {
        // Start a TCP listener simulating LAN
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let lan_addr = listener.local_addr().unwrap();

        let dead_vpn_addr: SocketAddr = "127.0.0.1:1".parse().unwrap(); // Port 1 unreachable

        let candidates = vec![
            CandidateEndpoint {
                tier: ConnectionTier::VpnDirect,
                addr: dead_vpn_addr,
                label: "Dead VPN".to_string(),
            },
            CandidateEndpoint {
                tier: ConnectionTier::LanDirect,
                addr: lan_addr,
                label: "Working LAN".to_string(),
            },
        ];

        let selected = ConnectionSelector::select_best_endpoint(candidates, Duration::from_millis(500))
            .await
            .expect("Selector must succeed on working LAN");

        assert_eq!(selected.endpoint.tier, ConnectionTier::LanDirect);
        assert_eq!(selected.endpoint.addr, lan_addr);
        assert!(selected.success);
    }

    #[tokio::test]
    async fn test_selector_fallback_when_higher_tier_unreachable() {
        // Higher tier (LAN) is down, lower tier (Relay) is up
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = listener.local_addr().unwrap();

        let dead_lan_addr: SocketAddr = "127.0.0.1:1".parse().unwrap();

        let candidates = vec![
            CandidateEndpoint {
                tier: ConnectionTier::LanDirect,
                addr: dead_lan_addr,
                label: "Dead LAN".to_string(),
            },
            CandidateEndpoint {
                tier: ConnectionTier::TurnRelay,
                addr: relay_addr,
                label: "Working Relay".to_string(),
            },
        ];

        let selected = ConnectionSelector::select_best_endpoint(candidates, Duration::from_millis(500))
            .await
            .expect("Selector must fallback to Relay");

        assert_eq!(selected.endpoint.tier, ConnectionTier::TurnRelay);
        assert_eq!(selected.endpoint.addr, relay_addr);
    }
}
