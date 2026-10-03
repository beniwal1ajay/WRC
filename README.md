# WARD — Wayland Android Remote Desktop

A secure, high-performance, personal remote desktop system for seamlessly controlling an Ubuntu GNOME Wayland desktop from an Android phone over LAN or Mesh VPN (Tailscale / WireGuard).

WARD provides a true **Unattended Host** experience modeled after Chrome Remote Desktop's "Remote Access":
- **Zero Interactive Dialogs:** Connects directly via GNOME Mutter's native ScreenCast and RemoteDesktop D-Bus interfaces. No recurring *"Share your screen?"* modal popups.
- **Permanent Host PIN:** Protected by PBKDF2-HMAC-SHA256 (600,000 rounds) with constant-time verification, automatic brute-force lockout, and fast 256-bit token reconnection.
- **Embedded Hardware Cursor:** Mouse cursor is composited directly into the video stream by the Wayland compositor.
- **Ultra Low Latency 60 FPS Streaming:** Zero-copy YUV conversion (`BorrowedYuv`), OpenH264 encoding with adaptive rate control, and `TCP_NODELAY`.
- **Pure Wayland Native:** 100% Wayland-native (no X11, no Xwayland hacks, no root/sudo privileges at runtime).
- **Background Daemon:** Runs as a `systemd --user` service (`ward-host.service`) surviving reboots and terminal closure.

---

```
┌─────────────────────────────────┐                 Encrypted TLS 1.3 Tunnel
│   Android Client (Phone/Tablet) │ <======================================================>
│   - Jetpack Compose UI          │              (Local Wi-Fi / LAN or Tailscale VPN)
│   - "My Computers" card view    │
│   - Hardware H.264 MediaCodec   │
└─────────────────────────────────┘
                                                              │
                                                              ▼
                                   ┌────────────────────────────────────────────────────────┐
                                   │  Ubuntu Host PC (Unattended Daemon)                    │
                                   │  - ward-host.service (systemd --user)                  │
                                   │  - TLS 1.3 + Ed25519 Mutual Authentication             │
                                   │  - Direct Mutter ScreenCast & RemoteDesktop D-Bus      │
                                   │  - PipeWire 60 FPS Zero-Copy Video Pipeline            │
                                   │  - OpenH264 Adaptive Rate Controller                   │
                                   └────────────────────────────────────────────────────────┘
```

---

## Key Features

- **True Unattended Access:** Configure once, connect anytime. The host runs continuously in the background as a systemd user service.
- **Embedded Mouse Cursor:** The compositor burns the desktop mouse cursor directly into video frames (`MetaScreenCastCursorMode::Embedded`). Remote pointer movement and clicks are immediately visible on your phone.
- **Fluid 60 FPS Video:** High-throughput, sub-frame latency H.264 encoding with zero-copy memory transfers and adaptive network rate control.
- **Comprehensive Remote Control:**
  - **Touch Gestures:** 1-finger pointer tracking, tap to left-click, 2-finger tap/press for right-click, 2-finger scroll, long-press drag & drop, 3-finger swipe for GNOME Activities/Super.
  - **Keyboard Input:** Full software keyboard support with IME integration, modifier keys (Ctrl, Alt, Super, Shift), and common shortcuts (Ctrl+C, Ctrl+V, Alt+Tab).
- **Hardened Cryptographic Security:**
  - TLS 1.3 encryption across all network communication.
  - Ed25519 cryptographic host identity keypair.
  - Permanent PIN hashed with PBKDF2-HMAC-SHA256 (600,000 iterations) with OS-random salt.
  - Constant-time verification to prevent timing side-channel attacks.
  - Brute-force protection: 5 consecutive failed attempts trigger an automatic 60-second administrative lockout.
  - Fast resumption: Authenticated devices receive a cryptographically random 256-bit token stored in Android `EncryptedSharedPreferences`. Subsequent connections resume instantly without re-prompting for the PIN.
  - Immediate token invalidation: Changing the host PIN instantly revokes all existing device tokens and disconnects active sessions.
- **Zero Port Forwarding (Mesh VPN):** Binds to all network interfaces (`0.0.0.0:9740`), enabling instant out-of-the-box remote access over Tailscale or WireGuard from cellular networks (4G/5G).

---

## Architecture Overview

| Component | Technology / Implementation |
|---|---|
| **Compositor Backend** | Native GNOME Mutter D-Bus (`org.gnome.Mutter.ScreenCast` & `RemoteDesktop`) |
| **Media Pipeline** | PipeWire SHM buffers (DMA-BUF ready) |
| **Video Encoder** | OpenH264 (Cisco) with zero-copy `BorrowedYuv` and row-pair YUV420p converter |
| **Framerate & Quality** | 1080p @ 60 FPS target with adaptive congestion control |
| **Network Transport** | TLS 1.3 over TCP (`tokio-rustls`, `rustls`) with `TCP_NODELAY` and keepalives |
| **Host Service** | systemd user service (`ward-host.service`) |
| **Client UI** | Kotlin, Jetpack Compose, Material Design 3 |
| **Client Decoder** | Android Hardware `MediaCodec` SurfaceView |

---

## Requirements

### Host (Ubuntu PC)
- **OS:** Ubuntu 24.04 LTS or 26.04 LTS
- **Desktop:** GNOME Shell (Wayland session active: `echo $XDG_SESSION_TYPE` → `wayland`)
- **Audio/Video:** PipeWire & WirePlumber (default in modern Ubuntu)
- **Build Tools:** Rust toolchain (`cargo`, `rustc` 2024 edition)

### Client (Android)
- Android 8.0+ (API level 26 or higher)
- Hardware H.264 video decoding support (standard on modern Android devices)

---

## Quick Start Guide

### Step 1: Install WARD Host Server on Ubuntu

Clone the repository and run the automated installer:

```bash
# 1. Install prerequisites
bash scripts/install-deps.sh

# 2. Build release binary and install systemd user service
bash scripts/install-server.sh
```

### Step 2: One-Time Host Setup & PIN Configuration

Run the guided setup wizard to verify your Wayland environment, generate your cryptographic identity, and set your permanent host PIN:

```bash
ward-server setup
```

Or set your PIN directly:
```bash
ward-server set-pin 123456
```

Verify that the host background service is active:
```bash
systemctl --user status ward-host.service
```

### Step 3: Install Android Client

Build and install the client APK onto your connected phone via ADB:

```bash
bash scripts/deploy-android.sh
```

Alternatively, copy the APK directly to your phone from:
`android/app/build/outputs/apk/debug/app-debug.apk`

### Step 4: Connect from Android

1. Open the **WARD** app on Android.
2. In the **"My Computers"** section, tap your PC's card (or enter your LAN or Tailscale IP address).
3. On first connection, enter your permanent 6-digit PIN.
4. Your Ubuntu desktop will stream smoothly at 60 FPS with full remote touch and keyboard control.
5. All future connections will reconnect automatically with zero prompts!

---

## Remote Access via VPN (Tailscale / WireGuard)

To access your Ubuntu desktop remotely over cellular data (4G/5G) without opening firewall ports or port forwarding:

1. **Install Tailscale on Ubuntu:**
   ```bash
   curl -fsSL https://tailscale.com/install.sh | sh
   sudo tailscale up
   ```
2. **Install Tailscale on Android:**
   - Install **Tailscale** from Google Play Store or F-Droid and log into the same account.
3. **Connect via Tailscale IP:**
   - Note your Ubuntu PC's Tailscale IP (`100.x.y.z`) via `tailscale ip -4`.
   - Enter this IP into the WARD Android app.
   - Traffic connects directly peer-to-peer over an end-to-end encrypted WireGuard tunnel.

See [VPN Setup Guide](docs/VPN_SETUP.md) for detailed instructions.

---

## Host Management Commands

`ward-server` includes a complete CLI for managing your unattended host:

| Command | Description |
|---|---|
| `ward-server setup` | Run interactive environment check, identity setup, and set permanent PIN |
| `ward-server set-pin <PIN>` | Change permanent PIN (immediately revokes all active device sessions) |
| `ward-server status` | Show host identity, PIN status, active endpoints, and paired devices |
| `ward-server devices` | List all authorized client devices and their pairing timestamps |
| `ward-server unpair <DEVICE_ID>` | Revoke access for a specific device |
| `ward-server unpair --all` | Revoke all paired client devices |
| `ward-server disable` | Temporarily disable remote connections (kill-switch) |
| `ward-server enable` | Re-enable remote connections |
| `ward-server host` | Run the unattended host server daemon in foreground (used by systemd) |

### Systemd Service Management

| Action | Command |
|---|---|
| **Check service status** | `systemctl --user status ward-host.service` |
| **View live logs** | `journalctl --user -u ward-host.service -f` |
| **Restart service** | `systemctl --user restart ward-host.service` |
| **Stop service** | `systemctl --user stop ward-host.service` |
| **Start service** | `systemctl --user start ward-host.service` |

---

## Repository Structure

```
ward/
├── android/            # Android client (Kotlin, Jetpack Compose, MediaCodec)
├── docs/               # Technical architecture & deployment documentation
│   ├── UNATTENDED_HOST.md    # Unattended host architecture & test matrix
│   ├── VPN_SETUP.md          # Tailscale & WireGuard remote setup guide
│   ├── DEPLOYMENT.md         # Production deployment guide
│   ├── ARCHITECTURE_RESEARCH.md
│   ├── SECURITY.md           # Security model & threat analysis
│   └── TROUBLESHOOTING.md    # Diagnostics & issue resolution
├── protocol/           # Binary protocol specification (Rust & Kotlin)
├── scripts/            # Build, deploy, and service installation scripts
├── server/             # Rust server (Mutter D-Bus, PipeWire, OpenH264, TLS)
└── systemd/            # systemd user service unit files
```

---

## Stable Release Branches

- **`vpn-stable` (Branch & Tag `v1.0-vpn-stable`):**
  The frozen, production-tested release containing the complete Unattended Host architecture, native GNOME Mutter backend, embedded cursor, 60 FPS streaming pipeline, and LAN/VPN remote access. **No experimental WAN or breaking changes are committed to this branch.**

---

## Security Model Summary

- **Transport:** TLS 1.3 with AES-256-GCM / ChaCha20-Poly1305.
- **Host Authentication:** Ed25519 cryptographic keypairs.
- **Client Authentication:** PBKDF2-HMAC-SHA256 (600,000 iterations) permanent PIN verification with 256-bit session tokens.
- **Brute-Force Protection:** 5 failed attempts trigger an automatic 60-second lockout per device.
- **Zero Root:** Operates entirely within user privilege boundaries using standard Linux D-Bus and PipeWire permissions.

For full cryptographic analysis, see [SECURITY.md](docs/SECURITY.md) and [UNATTENDED_HOST.md](docs/UNATTENDED_HOST.md).

---

## License

MIT License. See [LICENSE](LICENSE) for details.
