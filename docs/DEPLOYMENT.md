# WARD — Production Deployment Guide

This guide covers building, installing, running, and managing WARD (Wayland Android Remote Desktop) across your Ubuntu host and Android devices for local LAN and remote VPN access.

---

## 1. Quick Start (Automated Deployment)

### Host Server (Ubuntu Wayland)
To compile the release binary, install it to `~/.local/bin`, and configure the systemd user service:

```bash
# 1. Install dependencies
bash scripts/install-deps.sh

# 2. Build release binary, install ward-host.service, and start it
bash scripts/install-server.sh

# 3. Perform one-time setup and set your permanent host PIN
ward-server setup
```

### Android Client
To build the APK and automatically install it onto a connected phone via ADB:

```bash
bash scripts/deploy-android.sh
```

---

## 2. Server Deployment Details

### System Requirements
- **OS:** Ubuntu 24.04 LTS or 26.04 LTS with GNOME Wayland
- **Session:** Active Wayland user session (`echo $XDG_SESSION_TYPE` should output `wayland`)
- **Services:** `pipewire`, `wireplumber`, and `mutter` (GNOME Wayland Compositor)
- **Privileges:** Standard user account (no root required)

### Binary Installation
Compiling the optimized release binary manually:

```bash
cd server
cargo build --release
install -m 0755 target/release/ward-server ~/.local/bin/ward-server
```

Ensure `~/.local/bin` is in your environment PATH:
```bash
export PATH="$HOME/.local/bin:$PATH"
```

### Systemd User Service Configuration
Because WARD captures your live Wayland desktop and injects input via GNOME Mutter's D-Bus interfaces and PipeWire, it runs as a **systemd user service** under your user account (`ward-host.service`).

The unit file is installed at `~/.config/systemd/user/ward-host.service`:

```ini
[Unit]
Description=WARD Host - Unattended Wayland Remote Desktop Server
Documentation=https://github.com/beniwal1ajay/WRC
After=graphical-session.target pipewire.service
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/ward-server host
Restart=on-failure
RestartSec=3s
Environment=RUST_BACKTRACE=1
PassEnvironment=WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS

[Install]
WantedBy=graphical-session.target
```

#### Service Management Commands

| Action | Command |
|---|---|
| **Start Service** | `systemctl --user start ward-host.service` |
| **Stop Service** | `systemctl --user stop ward-host.service` |
| **Check Status** | `systemctl --user status ward-host.service` |
| **Restart Service** | `systemctl --user restart ward-host.service` |
| **Enable on Login** | `systemctl --user enable ward-host.service` |
| **Disable Autostart**| `systemctl --user disable ward-host.service` |
| **Live Logs** | `journalctl --user -u ward-host.service -f` |

---

## 3. Firewall & Network Setup

### Local Area Network (LAN)
If `ufw` (Uncomplicated Firewall) is active on your Ubuntu host, allow inbound connections on WARD's default port (`9740/tcp`):

```bash
sudo ufw allow 9740/tcp comment 'WARD Remote Desktop'
sudo ufw reload
```

### Remote Access via Mesh VPN (Recommended)
For secure remote access across cellular networks (LTE/5G) without opening public router ports or configuring port forwarding:

1. **Install Tailscale on Ubuntu:**
   ```bash
   curl -fsSL https://tailscale.com/install.sh | sh
   sudo tailscale up
   ```
2. **Install Tailscale on Android:**
   - Install Tailscale from Google Play Store or F-Droid.
   - Sign in with the same account.
3. **Connect directly:**
   - Note your Ubuntu host's Tailscale IP (`100.x.y.z`) via `tailscale ip -4`.
   - In the WARD Android app, enter the `100.x.y.z` address.
   - Traffic connects directly peer-to-peer with zero relay overhead over encrypted WireGuard tunnels.

---

## 4. Android Client Deployment

### Building the APK
The Android client is built using Gradle wrapper:

```bash
cd android
./gradlew assembleDebug
```
The resulting APK is located at:
`android/app/build/outputs/apk/debug/app-debug.apk`

### Installing via USB ADB
1. Enable **Developer Options** and **USB Debugging** on your Android device.
2. Plug the device in via USB and verify with `adb devices`.
3. Run:
   ```bash
   bash scripts/deploy-android.sh
   ```

### Installing via Wireless ADB
1. On Android 11+: Go to **Settings > System > Developer options > Wireless debugging**.
2. Pair device: `adb pair <IP>:<PAIRING_PORT>`
3. Connect: `adb connect <IP>:<CONNECT_PORT>`
4. Run: `bash scripts/deploy-android.sh`

---

## 5. Host Setup & Authentication

### 1. One-Time Setup
Run the setup wizard on Ubuntu:
```bash
ward-server setup
```
This command:
- Tests the native GNOME Mutter ScreenCast and RemoteDesktop D-Bus interfaces.
- Generates the host's cryptographic Ed25519 identity keypair (`~/.config/ward/host_identity.json`).
- Hashes and stores your permanent PIN using PBKDF2-HMAC-SHA256 with 600,000 iterations (`~/.config/ward/host_config.json`).
- Ensures the systemd user service `ward-host.service` is enabled and active.

### 2. First Connection from Android
1. Open WARD on Android.
2. Under "My Computers", select your Ubuntu PC (or enter your LAN or Tailscale IP).
3. Tap **Connect**.
4. Enter your permanent 6-digit PIN when prompted.
5. Upon successful PIN verification, the server issues a cryptographically secure 256-bit token saved on both host (`~/.config/ward/authorized_clients.json`) and Android client (`EncryptedSharedPreferences`).

### 3. Subsequent Reconnections
All future connections challenge the client with token authentication. The Android client presents its token and connects **instantly with zero interaction required on either Ubuntu or Android**.

### 4. Changing PIN & Invalidation
To change your host PIN:
```bash
ward-server set-pin 654321
```
Changing the PIN automatically:
- Updates the PBKDF2 verifier.
- Revokes all existing client tokens (requires all clients to re-authenticate with the new PIN).
- Automatically restarts `ward-host.service` to terminate any currently active sessions.

### 5. CLI Management Commands

Inspect host status, identity, active endpoints, and authorized devices:
```bash
ward-server status
```

List all authorized client devices:
```bash
ward-server devices
```

Revoke access for a specific device:
```bash
ward-server unpair <DEVICE_ID>
```

Revoke access for all devices:
```bash
ward-server unpair --all
```

Temporarily disable all remote connections (emergency kill-switch):
```bash
ward-server disable
```

Re-enable remote connections:
```bash
ward-server enable
```

---

## 6. Uninstallation

To cleanly remove WARD from the host:

```bash
bash scripts/uninstall-server.sh
```

To also delete all persisted keys and host configuration:
```bash
rm -rf ~/.config/ward
```
