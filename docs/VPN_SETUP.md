# WARD Remote Access via VPN (Tailscale & WireGuard)

This guide explains how to connect your Android device to your Ubuntu PC behind home NATs, firewalls, or cellular carriers without router port forwarding or public IP exposure, as part of **Phase 4 (VPN Remote Access)**.

---

## 1. Architecture Overview

```
┌─────────────────────────────┐                 Encrypted WireGuard / Tailscale Tunnel
│  Android Phone (Remote)     │ <======================================================>
│  - Mobile 4G / 5G / Wi-Fi   │
│  - Tailscale IP: 100.64.0.2 │
└─────────────────────────────┘
                                                              │
                                                              ▼
                               ┌────────────────────────────────────────────────────────┐
                               │  Ubuntu Host PC (Behind NAT)                           │
                               │  - Tailscale IP: 100.64.0.1                            │
                               │  - Interface: tailscale0                               │
                               │  - WARD Host Service: ward-host.service                │
                               │  - Listening on 0.0.0.0:9740 (TLS 1.3)                 │
                               │  - Direct GNOME Mutter ScreenCast (Embedded Cursor)    │
                               │  - 60 FPS Zero-Copy Pipeline                           │
                               └────────────────────────────────────────────────────────┘
```

Because `ward-server` binds to `0.0.0.0:9740`, it automatically listens on all local, virtual, and VPN network interfaces (`eth0`, `wlan0`, `tailscale0`, `wg0`).

---

## 2. Option A: Tailscale Setup (Recommended — Zero Port Forwarding)

Tailscale provides an encrypted WireGuard mesh network connecting your devices across NATs with zero firewall configuration.

### Step 1: Install Tailscale on Ubuntu
```bash
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up
```
Check your Ubuntu Tailscale IP address:
```bash
tailscale ip -4
# Example output: 100.82.14.99
```

### Step 2: Install Tailscale on Android
1. Install **Tailscale** from Google Play Store or F-Droid.
2. Sign in with the same account as your Ubuntu PC.
3. Turn on the VPN toggle in the Tailscale app.

### Step 3: WARD Host Service (Unattended)
The WARD host service runs automatically in the background as a systemd user daemon:
```bash
systemctl --user status ward-host.service
```
To verify network endpoints and host identity:
```bash
ward-server status
```
The output displays the active endpoints:
```
============================================================
  [WARD NETWORK] Server active on port 9740
  Available connection endpoints:
    - Tailscale VPN  : 100.82.14.99:9740 (tailscale0)
    - LAN / Wi-Fi    : 192.168.1.15:9740 (wlan0)
    - Localhost      : 127.0.0.1:9740 (lo)
============================================================
```

### Step 4: Connect from Android App
1. Open the **WARD** app on Android.
2. Under **"My Computers"**, select your host PC (or tap **▼ Edit Address / Port** and enter your Ubuntu Tailscale IP, e.g. `100.82.14.99`).
3. Tap **Connect**.
4. When connecting for the first time, enter your permanent 6-digit PIN.
5. Once paired, all future connections connect instantly via saved token without re-entering the PIN!
6. Screen streams at 60 FPS with embedded hardware cursor and full remote input.

---

## 3. Option B: Self-Hosted WireGuard

If you run your own WireGuard server or point-to-point tunnel:

1. Ensure the WireGuard tunnel interface is active on Ubuntu:
   ```bash
   sudo wg-quick up wg0
   ip addr show wg0
   # Example: 10.10.0.1
   ```
2. Verify endpoints:
   ```bash
   ward-server status
   # Displays: - WireGuard VPN: 10.10.0.1:9740 (wg0)
   ```
3. Connect your Android device via the WireGuard Android app, then connect WARD to `10.10.0.1:9740`.

---

## 4. Network & Streaming Optimizations for VPN / WAN

1. **TCP Keepalive**: Enables `socket.keepAlive = true` to prevent cellular carriers and stateful NAT firewalls from dropping idle sessions.
2. **TCP NoDelay**: Disables Nagle's algorithm (`socket.tcpNoDelay = true`) ensuring sub-millisecond cursor tracking and keyboard response over VPN routes.
3. **Adaptive Rate Controller**: Automatically adjusts video encoding bitrate based on network throughput and frame delivery latency.
4. **Embedded Hardware Cursor**: Mutter burns the mouse cursor directly into the video stream, eliminating cursor drift and out-of-sync overlay artifacts.
5. **Zero-Copy Encoding**: `BorrowedYuv` format eliminates per-frame heap allocations on the host for consistent 60 FPS streaming.
