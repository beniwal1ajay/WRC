use std::time::{Duration, Instant};
use tracing::{debug, info};
use crate::transport::selector::ConnectionTier;

/// Video quality profile dynamically chosen based on connection tier and network health.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoProfile {
    pub bitrate_bps: u32,
    pub max_fps: u32,
    pub keyframe_interval_secs: u32,
}

impl VideoProfile {
    pub fn for_tier(tier: ConnectionTier) -> Self {
        match tier {
            ConnectionTier::LanDirect => Self {
                bitrate_bps: 8_000_000, // 8 Mbps
                max_fps: 60,
                keyframe_interval_secs: 10,
            },
            ConnectionTier::VpnDirect => Self {
                bitrate_bps: 4_500_000, // 4.5 Mbps
                max_fps: 60,
                keyframe_interval_secs: 5,
            },
            ConnectionTier::P2pIce => Self {
                bitrate_bps: 2_500_000, // 2.5 Mbps
                max_fps: 45,
                keyframe_interval_secs: 3,
            },
            ConnectionTier::TurnRelay => Self {
                bitrate_bps: 1_200_000, // 1.2 Mbps
                max_fps: 30,
                keyframe_interval_secs: 2,
            },
        }
    }
}

/// Adaptive video rate controller adjusting bitrate, framerate, and keyframe generation in real time.
#[derive(Debug, Clone)]
pub struct AdaptiveVideoController {
    pub tier: ConnectionTier,
    pub current_bitrate_bps: u32,
    pub min_bitrate_bps: u32,
    pub max_bitrate_bps: u32,
    pub current_fps: u32,
    pub min_fps: u32,
    pub max_fps: u32,
    last_adaptation: Instant,
    last_frame_sent: Instant,
    frame_interval: Duration,
    pub smoothed_rtt_ms: f32,
}

impl AdaptiveVideoController {
    pub fn new(tier: ConnectionTier) -> Self {
        let profile = VideoProfile::for_tier(tier);
        let min_bitrate = match tier {
            ConnectionTier::LanDirect => 2_000_000,
            ConnectionTier::VpnDirect => 1_000_000,
            ConnectionTier::P2pIce => 500_000,
            ConnectionTier::TurnRelay => 400_000,
        };
        let max_bitrate = match tier {
            ConnectionTier::LanDirect => 15_000_000,
            ConnectionTier::VpnDirect => 8_000_000,
            ConnectionTier::P2pIce => 5_000_000,
            ConnectionTier::TurnRelay => 2_000_000,
        };

        let frame_interval = Duration::from_micros(1_000_000 / profile.max_fps as u64);

        let min_fps = match tier {
            ConnectionTier::LanDirect => 60,
            ConnectionTier::VpnDirect => 45,
            ConnectionTier::P2pIce => 30,
            ConnectionTier::TurnRelay => 15,
        };

        Self {
            tier,
            current_bitrate_bps: profile.bitrate_bps,
            min_bitrate_bps: min_bitrate,
            max_bitrate_bps: max_bitrate,
            current_fps: profile.max_fps,
            min_fps,
            max_fps: profile.max_fps,
            last_adaptation: Instant::now(),
            last_frame_sent: Instant::now(),
            frame_interval,
            smoothed_rtt_ms: 20.0,
        }
    }

    /// Update network telemetry metrics received from client (RTT, latency, reported FPS).
    pub fn on_network_feedback(&mut self, latency_ms: u16, _client_fps: u16) {
        let rtt = latency_ms as f32;
        // EWMA smoothing
        self.smoothed_rtt_ms = 0.8 * self.smoothed_rtt_ms + 0.2 * rtt;

        let now = Instant::now();
        if now.duration_since(self.last_adaptation) < Duration::from_millis(500) {
            return;
        }

        // On LAN / VPN, maintain 60 FPS video smoothness without artificial throttling
        if self.tier == ConnectionTier::LanDirect || self.tier == ConnectionTier::VpnDirect {
            if self.smoothed_rtt_ms > 250.0 {
                let new_bitrate = ((self.current_bitrate_bps as f32) * 0.85) as u32;
                self.current_bitrate_bps = new_bitrate.max(self.min_bitrate_bps);
                self.last_adaptation = now;
            } else if self.smoothed_rtt_ms < 60.0 && self.current_bitrate_bps < self.max_bitrate_bps {
                self.current_bitrate_bps = (self.current_bitrate_bps + 500_000).min(self.max_bitrate_bps);
                self.last_adaptation = now;
            }
            return;
        }

        if self.smoothed_rtt_ms > 250.0 {
            // Severe congestion on WAN/Relay: decrease bitrate by 25% and step down FPS
            let new_bitrate = ((self.current_bitrate_bps as f32) * 0.75) as u32;
            self.current_bitrate_bps = new_bitrate.max(self.min_bitrate_bps);
            self.current_fps = (self.current_fps.saturating_sub(10)).max(self.min_fps);
            self.update_frame_interval();
            self.last_adaptation = now;
            info!(
                "AdaptiveVideo: Congestion detected (RTT {:.1}ms)! Decreased bitrate to {} kbps, FPS to {}",
                self.smoothed_rtt_ms, self.current_bitrate_bps / 1000, self.current_fps
            );
        } else if self.smoothed_rtt_ms > 120.0 {
            // Mild congestion: decrease bitrate by 15%
            let new_bitrate = ((self.current_bitrate_bps as f32) * 0.85) as u32;
            self.current_bitrate_bps = new_bitrate.max(self.min_bitrate_bps);
            self.last_adaptation = now;
            debug!(
                "AdaptiveVideo: Mild latency (RTT {:.1}ms), adjusted bitrate to {} kbps",
                self.smoothed_rtt_ms, self.current_bitrate_bps / 1000
            );
        } else if self.smoothed_rtt_ms < 50.0 && self.current_bitrate_bps < self.max_bitrate_bps {
            // Clear network: additive increase (+300 kbps)
            self.current_bitrate_bps = (self.current_bitrate_bps + 300_000).min(self.max_bitrate_bps);
            if self.current_fps < self.max_fps {
                self.current_fps = (self.current_fps + 5).min(self.max_fps);
                self.update_frame_interval();
            }
            self.last_adaptation = now;
            debug!(
                "AdaptiveVideo: Low RTT ({:.1}ms), ramped up bitrate to {} kbps, FPS to {}",
                self.smoothed_rtt_ms, self.current_bitrate_bps / 1000, self.current_fps
            );
        }
    }

    /// Triggered on explicit packet loss or keyframe requests.
    pub fn on_loss_event(&mut self) {
        let new_bitrate = ((self.current_bitrate_bps as f32) * 0.80) as u32;
        self.current_bitrate_bps = new_bitrate.max(self.min_bitrate_bps);
        self.last_adaptation = Instant::now();
        info!(
            "AdaptiveVideo: Packet loss event! Backed off bitrate to {} kbps",
            self.current_bitrate_bps / 1000
        );
    }

    /// Check if enough time has passed to encode and send the next frame (FPS pacing).
    pub fn should_encode_frame(&mut self) -> bool {
        let now = Instant::now();
        // If operating at full target frame rate (not throttled by congestion), do not drop frames
        if self.current_fps >= self.max_fps {
            self.last_frame_sent = now;
            return true;
        }

        if now.duration_since(self.last_frame_sent) >= self.frame_interval {
            self.last_frame_sent = now;
            true
        } else {
            false
        }
    }

    fn update_frame_interval(&mut self) {
        let fps = self.current_fps.max(1) as u64;
        self.frame_interval = Duration::from_micros(1_000_000 / fps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_profile_per_tier() {
        let lan_profile = VideoProfile::for_tier(ConnectionTier::LanDirect);
        let vpn_profile = VideoProfile::for_tier(ConnectionTier::VpnDirect);
        let ice_profile = VideoProfile::for_tier(ConnectionTier::P2pIce);
        let relay_profile = VideoProfile::for_tier(ConnectionTier::TurnRelay);

        assert!(lan_profile.bitrate_bps > vpn_profile.bitrate_bps);
        assert!(vpn_profile.bitrate_bps > ice_profile.bitrate_bps);
        assert!(ice_profile.bitrate_bps > relay_profile.bitrate_bps);

        assert_eq!(lan_profile.max_fps, 60);
        assert_eq!(relay_profile.max_fps, 30);
    }

    #[test]
    fn test_adaptive_controller_backoff_on_severe_latency() {
        let mut controller = AdaptiveVideoController::new(ConnectionTier::LanDirect);
        let initial_bitrate = controller.current_bitrate_bps;
        let initial_fps = controller.current_fps;

        // Simulate high RTT (250ms)
        controller.last_adaptation = Instant::now() - Duration::from_secs(1);
        controller.on_network_feedback(250, 15);

        assert!(controller.current_bitrate_bps < initial_bitrate);
        assert!(controller.current_fps <= initial_fps);
    }

    #[test]
    fn test_adaptive_controller_ramp_up() {
        let mut controller = AdaptiveVideoController::new(ConnectionTier::P2pIce);
        let initial_bitrate = controller.current_bitrate_bps;

        // Simulate clean network with low RTT (15ms)
        controller.smoothed_rtt_ms = 15.0;
        controller.last_adaptation = Instant::now() - Duration::from_secs(1);
        controller.on_network_feedback(15, 45);

        assert!(controller.current_bitrate_bps > initial_bitrate);
    }

    #[test]
    fn test_loss_event_triggers_multiplicative_decrease() {
        let mut controller = AdaptiveVideoController::new(ConnectionTier::VpnDirect);
        let initial = controller.current_bitrate_bps;

        controller.on_loss_event();
        assert_eq!(controller.current_bitrate_bps, (initial as f32 * 0.8) as u32);
    }
}
