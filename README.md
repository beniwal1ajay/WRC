# WARD — Wayland Android Remote Desktop

A secure, Wayland-native personal remote desktop system for controlling an Ubuntu GNOME desktop from an Android phone.

---

## What is WARD?

WARD lets you view and control your Ubuntu laptop's desktop from your Android phone over your local network. It's designed to work natively with Wayland — no X11 required.

```
Android Phone ──── encrypted connection ──── Ubuntu Laptop
                                              │
                                              ├── PipeWire / ScreenCast
                                              ├── RemoteDesktop / EIS
                                              │
                                              └── Existing GNOME Wayland Session
```

## Features

- **View your desktop** — Live screen streaming from Ubuntu to Android
- **Mouse control** — Tap, click, right-click, double-click, drag, scroll
- **Keyboard input** — Full keyboard with shortcuts (Ctrl+C, Alt+Tab, etc.)
- **Clipboard sync** — Copy/paste text between Ubuntu and Android
- **Secure** — TLS 1.3 encryption, Ed25519 device authentication, pairing codes
- **Wayland-native** — Uses XDG Desktop Portal APIs, no X11 hacks
- **User-level** — Runs as a user service, no root required for normal operation

## Architecture

| Component          | Technology                                 |
|--------------------|--------------------------------------------|
| Screen capture     | XDG ScreenCast Portal + PipeWire           |
| Input injection    | XDG RemoteDesktop Portal                   |
| Video encoding     | VA-API H.264 (hw) / x264 (sw fallback)    |
| Transport          | TLS 1.3 over TCP                           |
| Discovery          | mDNS/DNS-SD (Avahi)                        |
| Server             | Rust                                       |
| Android client     | Kotlin + Jetpack Compose                   |

## Requirements

### Server (Ubuntu)

- Ubuntu 24.04+ with GNOME and Wayland
- PipeWire (default in Ubuntu 24.04)
- XDG Desktop Portal with GNOME backend
- Rust toolchain (for building)

### Client (Android)

- Android 8.0+ (API 26)

## Quick Start & Deployment

### 1. Install & Run Server (Ubuntu Wayland)

```bash
# Automated installer (builds release binary and installs systemd user service)
bash scripts/install-server.sh

# Start the background service
systemctl --user start ward.service

# Or run directly in a terminal
ward-server stream
```

### 2. Deploy Android Client

```bash
# Builds APK and installs to connected phone via ADB
bash scripts/deploy-android.sh
```

### 3. Pair & Connect

Open the WARD app on Android and enter your Ubuntu host's IP (LAN or Tailscale VPN). A native desktop notification will pop up on your Ubuntu desktop displaying your 6-digit pairing PIN. Once paired, all future connections resume instantly.

## Project Structure

```
ward/
├── docs/           # Documentation & guides
├── server/         # Rust server (Wayland, PipeWire, TLS 1.3, STUN/TURN)
├── android/        # Android client (Kotlin + Jetpack Compose)
├── protocol/       # Binary protocol specification
├── scripts/        # Deployment & installation scripts
├── systemd/        # Systemd user service unit
└── .github/        # CI configuration
```

## Documentation

- [Production Deployment Guide](docs/DEPLOYMENT.md)
- [Architecture Research](docs/ARCHITECTURE_RESEARCH.md)
- [WAN Remote Architecture](docs/WAN_ARCHITECTURE.md)
- [VPN Remote Setup Guide](docs/VPN_SETUP.md)
- [Protocol Specification](protocol/PROTOCOL.md)
- [Security Model](docs/SECURITY.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)
- [Roadmap](docs/ROADMAP.md)

## Security

See [SECURITY.md](docs/SECURITY.md) for the full security model.

**Key points:**
- All communication is TLS 1.3 encrypted
- Device authentication via Ed25519 keypairs
- First-time pairing with visual code verification
- No data is ever transmitted in plaintext
- No passwords or secrets are logged

## License

MIT

## Status

🚧 **Under active development** — MVP in progress
