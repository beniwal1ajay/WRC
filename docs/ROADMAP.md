# WARD — Development Roadmap

**Last Updated:** 2026-10-03  
**Current Milestone:** Milestone 12 Complete (Unattended Host & VPN Stable Release)

---

## Milestone Overview

| #  | Milestone                    | Status       | Target                              |
|----|------------------------------|--------------|-------------------------------------|
| 0  | Environment Discovery        | ✅ Complete  | System capabilities documented      |
| 1  | Wayland Screen Capture       | ✅ Complete  | Capture frames via ScreenCast+PW    |
| 2  | Remote Input                 | ✅ Complete  | Mouse/keyboard via RemoteDesktop    |
| 3  | LAN Video Streaming          | ✅ Complete  | Android views Ubuntu desktop        |
| 4  | LAN Remote Control           | ✅ Complete  | Android controls Ubuntu desktop     |
| 5  | Clipboard Sync               | 🔄 Future    | Bidirectional text clipboard        |
| 6  | Authentication & Pairing     | ✅ Complete  | Ed25519 identity, PBKDF2 PIN auth   |
| 7  | Reconnect & Resilience       | ✅ Complete  | Automatic token re-auth, backoff    |
| 8  | Performance Optimization     | ✅ Complete  | 1080p @ 60 FPS zero-copy pipeline   |
| 9  | Packaging                    | ✅ Complete  | systemd user service, installers    |
| 10 | Security Audit               | ✅ Complete  | Cryptographic audit, rate limiting  |
| 11 | Documentation                | ✅ Complete  | Comprehensive guides & specifications|
| 12 | Unattended Host Architecture | ✅ Complete  | Direct Mutter D-Bus, embedded cursor|
| 13 | VPN Remote Access (Phase 4)  | ✅ Complete  | Tailscale & WireGuard mesh support  |
| 14 | WAN Direct P2P (Phase 5)     | ⏸️ Paused    | WebRTC, STUN/TURN, ICE (Future)     |

---

## Milestone Details

### Milestone 0: Environment Discovery ✅
- **Delivered:** Ubuntu 24.04/26.04 Wayland compositor and portal capabilities documented.
- **Result:** Native Mutter ScreenCast and RemoteDesktop D-Bus APIs validated.

### Milestone 1: Wayland Screen Capture ✅
- **Delivered:** PipeWire SHM buffer negotiation and frame capture with zero X11 dependencies.

### Milestone 2: Remote Input ✅
- **Delivered:** Absolute and relative pointer motion, button clicks, vertical scroll, and keyboard input injection.

### Milestone 3: LAN Video Streaming ✅
- **Delivered:** OpenH264 encoding, custom binary streaming protocol over TLS 1.3, Android MediaCodec hardware decoding.

### Milestone 4: LAN Remote Control ✅
- **Delivered:** Touch gestures on Android (tap to click, 2-finger scroll/right click, drag & drop) mapped to desktop input.

### Milestone 6: Authentication & Pairing ✅
- **Delivered:** Ed25519 host keypairs, PBKDF2-HMAC-SHA256 (600,000 rounds) permanent PIN verification, 5-attempt brute-force lockout, 256-bit token issuance and fast resumption.

### Milestone 7: Reconnect & Resilience ✅
- **Delivered:** Client token reconnection with instant session resumption and automatic cleanup on network interruptions.

### Milestone 8: Performance Optimization ✅
- **Delivered:** 60 FPS streaming pipeline, zero-copy `BorrowedYuv` format, OpenH264 adaptive rate control, row-pair YUV conversion, and `TCP_NODELAY`.

### Milestone 9: Packaging & Deployment ✅
- **Delivered:** `ward-host.service` systemd user unit, automated `install-server.sh`, `uninstall-server.sh`, and `deploy-android.sh`.

### Milestone 10: Security Model ✅
- **Delivered:** Constant-time PIN verification, POSIX 0600 file security, journald sanitization, and administrative disable switch.

### Milestone 11: Documentation ✅
- **Delivered:** Updated README, DEPLOYMENT, VPN_SETUP, UNATTENDED_HOST, and TROUBLESHOOTING guides.

### Milestone 12: Unattended Host Architecture ✅
- **Delivered:** Zero-prompt background operation via native GNOME Mutter ScreenCast D-Bus backend; embedded hardware cursor (`MetaScreenCastCursorMode::Embedded`).

### Milestone 13: VPN Remote Access ✅
- **Delivered:** Seamless remote access over Tailscale and WireGuard mesh VPNs with zero router port forwarding. Frozen in `vpn-stable` branch.
