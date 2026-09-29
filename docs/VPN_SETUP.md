# WARD Remote Access via VPN (Tailscale & WireGuard)

This guide explains how to connect your Android device to your Ubuntu PC behind a home NAT or firewall without router port forwarding or public IP exposure, as part of **Phase 4 (VPN Remote Access)**.

---

## 1. Architecture Overview

```
+-----------------------------+           Encrypted WireGuard / Tailscale Tunnel
|  Android Phone (Remote)     | <=======================================================>
|  - Mobile 4G / 5G / Wi-Fi   |                                                         
|  - Tailscale IP: 100.64.0.2 |                                                         
+-----------------------------+                                                         
                                                                                        
+---------------------------------------------------------------------------------------+
|  Ubuntu Host PC (Behind NAT)                                                          |
|  - Tailscale IP: 100.64.0.1                                                           |
|  - Interface: tailscale0                                                              |
|  - WARD Server: 0.0.0.0:9740 (TLS 1.3 + Ed25519 Mutual Auth)                          |
+---------------------------------------------------------------------------------------+
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

### Step 3: Start WARD Server on Ubuntu
```bash
cargo run --release -- stream
```
The server will automatically detect and print the Tailscale endpoint:
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
2. Tap **▼ Edit Address / Port**.
3. Set the Host to your Ubuntu Tailscale IP (e.g. `100.82.14.99`) and port `9740`.
4. Tap **Connect to AJAY-UBUNTU**.
5. When connecting for the first time, enter the 6-digit PIN displayed on your Ubuntu terminal.
6. Once paired, future connections connect instantly via saved token without re-entering the PIN!

---

## 3. Option B: Self-Hosted WireGuard

If you run your own WireGuard server or point-to-point tunnel:

1. Ensure the WireGuard tunnel interface is active on Ubuntu:
   ```bash
   sudo wg-quick up wg0
   ip addr show wg0
   # Example: 10.10.0.1
   ```
2. Start `ward-server`. The console will display:
   ```
   - WireGuard VPN  : 10.10.0.1:9740 (wg0)
   ```
3. Connect your Android device via the WireGuard Android app, then connect WARD to `10.10.0.1:9740`.

---

## 4. Network Optimizations for WAN / VPN

1. **TCP Keepalive**: `LanTlsTransport` enables `socket.keepAlive = true` to prevent cellular carriers and stateful NAT routers from dropping idle sessions.
2. **TCP NoDelay**: Disables Nagle's algorithm (`socket.tcpNoDelay = true`) ensuring low-latency cursor tracking and keyboard response over WAN routes.
3. **Adaptive Video**: In subsequent phases (Phase 9), WARD's video encoder dynamically tunes bitrates to handle fluctuating mobile bandwidth.
