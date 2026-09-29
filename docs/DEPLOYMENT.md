# WARD — Production Deployment Guide

This guide covers building, installing, running, and managing WARD (Wayland Android Remote Desktop) across your Ubuntu host and Android devices for local LAN and remote WAN access.

---

## 1. Quick Start (Automated Deployment)

### Host Server (Ubuntu 24.04 Wayland)
To compile the release binary, install it to `~/.local/bin`, and configure the systemd user service:

```bash
# 1. Install dependencies (if not already installed)
bash scripts/install-deps.sh

# 2. Build and install server & systemd service
bash scripts/install-server.sh
```

### Android Client
To build the APK and automatically install it onto a connected phone via ADB:

```bash
bash scripts/deploy-android.sh
```

---

## 2. Server Deployment Details

### System Requirements
- **OS:** Ubuntu 24.04 LTS (or modern Linux distribution with GNOME Wayland)
- **Session:** Active Wayland user session (`echo $XDG_SESSION_TYPE` should output `wayland`)
- **Services:** `pipewire`, `wireplumber`, and `xdg-desktop-portal-gnome`

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
Because WARD captures your live Wayland desktop and injects input via D-Bus and PipeWire, it runs as a **systemd user service** under your user account (not root).

The unit file is installed at `~/.config/systemd/user/ward.service`:

```ini
[Unit]
Description=WARD - Wayland Android Remote Desktop Server
Documentation=https://github.com/beniwal1ajay/WRC
After=graphical-session.target pipewire.service
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/ward-server stream
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
| **Start Service** | `systemctl --user start ward.service` |
| **Stop Service** | `systemctl --user stop ward.service` |
| **Check Status** | `systemctl --user status ward.service` |
| **Restart Service** | `systemctl --user restart ward.service` |
| **Enable on Login** | `systemctl --user enable ward.service` |
| **Disable Autostart**| `systemctl --user disable ward.service` |
| **Live Logs** | `journalctl --user -u ward.service -f` |

---

## 3. Firewall & Network Setup

### Local Area Network (LAN)
If `ufw` (Uncomplicated Firewall) is active on your Ubuntu host, allow inbound connections on WARD's default port (`9740/tcp`):

```bash
sudo ufw allow 9740/tcp comment 'WARD Remote Desktop'
sudo ufw reload
```

### Remote Access via Mesh VPN (Recommended)
For secure remote access across cellular networks (LTE/5G) without opening public router ports or port forwarding:

1. **Install Tailscale on Ubuntu:**
   ```bash
   curl -fsSL https://tailscale.com/install.sh | sh
   sudo tailscale up
   ```
2. **Install Tailscale on Android:**
   - Install Tailscale from Google Play Store or F-Droid.
   - Sign in with the same account.
3. **Connect directly:**
   - Note your Ubuntu host's Tailscale IP (`100.x.y.z`).
   - In the WARD Android app, enter the `100.x.y.z` address.
   - Traffic connects directly with zero relay overhead over encrypted WireGuard tunnels.

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

## 5. First-Time Pairing & Client Management

1. **Start WARD Server:**
   ```bash
   systemctl --user start ward.service
   # or run directly in a terminal:
   ward-server stream
   ```
2. **First Connection:**
   - Open WARD on Android and connect to your Ubuntu IP address (e.g. `192.168.1.100` or `100.x.y.z`).
   - The host will generate a secure 6-digit numeric pairing PIN.
   - A native desktop notification will pop up on your Ubuntu desktop displaying the PIN:
     ```
     WARD Remote Desktop: Pairing PIN for Pixel 8 Pro
     Enter PIN on Android: 849201
     ```
   - Enter the PIN on your phone.
3. **Automatic Fast Token Reconnection:**
   - Once paired, WARD stores a cryptographically secure 256-bit token on both host (`~/.config/ward/authorized_clients.json`) and Android (`EncryptedSharedPreferences`).
   - All subsequent connections bypass PIN verification and resume instantly.

### CLI Management

Inspect server identity, public keys, and all authorized client devices:
```bash
ward-server status
```

List all paired devices:
```bash
ward-server devices
```

Revoke access for a specific device:
```bash
ward-server unpair <DEVICE_ID>
```

---

## 6. Uninstallation

To cleanly remove WARD from the host:

```bash
bash scripts/uninstall-server.sh
```

To also delete all persisted keys and paired device authorizations:
```bash
rm -rf ~/.config/ward
```
