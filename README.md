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

## Quick Start

### 1. Install Dependencies

```bash
sudo apt install build-essential pkg-config libpipewire-0.3-dev \
    libspa-0.2-dev libdbus-1-dev libssl-dev libclang-dev clang libva-dev
```

### 2. Build the Server

```bash
cd ward/server
cargo build --release
```

### 3. Run

```bash
./target/release/ward-server
```

A screen sharing permission dialog will appear — grant access.

### 4. Connect from Android

Install the WARD Android app, scan for devices on the LAN, and connect.

## Project Structure

```
ward/
├── docs/           # Documentation
├── server/         # Rust server
├── android/        # Android client (Kotlin)
├── protocol/       # Protocol specification
├── scripts/        # Install/build scripts
├── systemd/        # User service files
└── .github/        # CI configuration
```

## Documentation

- [Architecture Research](docs/ARCHITECTURE_RESEARCH.md)
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
