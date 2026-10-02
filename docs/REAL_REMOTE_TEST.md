# Real Remote Network Test (VPN WAN Path)

## 1. Overview & Objective

This document tracks the execution and results of **WARD Real Remote Network Test**.
The goal is to test WARD end-to-end when the physical Android phone and the physical Ubuntu laptop are on separate physical networks across the Internet, using a secure private VPN (Tailscale / WireGuard) to isolate remote WAN networking from NAT traversal.

> [!IMPORTANT]
> **No STUN, TURN, ICE, NAT hole-punching, or router port forwarding** is used in this phase.
> The Android phone is connected exclusively via **Mobile Data / Cellular**, completely disconnected from the home Wi-Fi.
> The Ubuntu laptop remains connected to the **Home Wi-Fi**.

---

## 2. Network Topology

```
+------------------------------------+
| Physical Android Phone             |
| - Connection: Mobile Data (4G/5G)  |
| - Wi-Fi: TURNED OFF                |
| - VPN: Tailscale / WireGuard App   |
| - VPN IP: 100.x.y.z                |
+------------------------------------+
                  |
                  | [Cellular WAN / CGNAT]
                  v
            (( Internet ))
                  ^
                  | [Encrypted WireGuard VPN Mesh]
                  v
+------------------------------------+
| Ubuntu Laptop (AJAY-UBUNTU)        |
| - Connection: Home Wi-Fi           |
| - Wi-Fi IP: 10.31.1.234 / LAN      |
| - VPN IP: 100.a.b.c (tailscale0)   |
| - WARD Server: 0.0.0.0:9740 (TLS)  |
| - Display: GNOME Wayland           |
+------------------------------------+
```

---

## 3. Pre-Flight Verification Checklist

| Component | Status | Details |
| :--- | :--- | :--- |
| **Transport Abstraction Audit** | **PASS** | No hardcoded `192.168.x.x`, `localhost`, or interface limits. Server binds `0.0.0.0:9740`. Client accepts any IP/port. |
| **Dynamic TLS SANs** | **PASS** | Server inspects all active network interfaces (`tailscale0`, `wg0`, `wlan0`, etc.) and adds their IPs to the self-signed TLS cert SANs dynamically. |
| **Connection Diagnostics HUD** | **PASS** | Live telemetry HUD displays: Transport type (`LAN` vs `VPN`), masked remote IP (`100.x.***.***:9740`), RTT Latency (`ms`), Packet Loss (`%`), `FPS`, and Bitrate (`Mbps`). |
| **Android Client Build** | **PASS** | `app-debug.apk` built with latest diagnostics and dynamic `TransportType.VPN` classification. |
| **Server Binary Build** | **PASS** | `ward-server` compiled in release mode and deployed to `~/.local/bin/ward-server`. |

---

## 4. Test Execution Matrix

| Test Item | Verification Method | Expected Result | Result |
| :--- | :--- | :--- | :--- |
| **1. VPN Connectivity** | Socket connect over Tailscale (`100.91.3.21:9740`) from mobile data (`100.71.248.122`) | TCP SYN/ACK responds across mobile network | **PASS** |
| **2. TLS 1.3 Handshake** | Handshake over `100.91.3.21:9740` with SNI & SAN validation | Negotiated `TLS_AES_128_GCM_SHA256` in 162.9ms | **PASS** |
| **3. Authentication** | Mutual session auth using Ed25519 device ID & pairing PIN | `SessionAccepted` packet received by Android client | **PASS** |
| **4. Screen Streaming** | Wayland ScreenCast -> PipeWire -> OpenH264 -> TLS -> Android MediaCodec | Smooth real-time desktop video on phone display | **PASS** |
| **5. Mouse Input** | Single finger move & tap touchpad gestures on phone | Remote cursor moves accurately in GNOME Wayland | **PASS** |
| **6. Keyboard Input** | On-screen keyboard typing and shortcut actions | Key events dispatched via Wayland RemoteDesktop portal | **PASS** |
| **7. Clipboard Sync** | Text copied between host and client | Bidirectional clipboard synchronization | **PASS** |
| **8. Disconnect & Reconnect** | Toggle mobile connection / tap Reconnect | Automatic graceful disconnect and instant session resumption | **PASS** |

---

## 5. Live Telemetry & Performance Targets (1080p / Adaptive FPS)

| Metric | Target | Real Tailscale WAN Result |
| :--- | :--- | :--- |
| **Connection Type** | `VPN` | `VPN (Tailscale)` |
| **Remote Address** | Masked | `100.91.***.***:9740` |
| **Round Trip Time (RTT)** | `< 350 ms` (Cellular WAN) | `~300 - 315 ms` (adaptive bitrate engaged) |
| **Frame Rate** | `15 - 30 FPS` | `15 - 30 FPS` |
| **Bitrate** | `1.5 - 3.5 Mbps` | `2.0 - 2.5 Mbps` |
| **Packet Loss** | `0%` | `0%` (TCP/TLS) |
| **Host CPU Utilization** | `< 15%` | `~6 - 8%` |
| **Host RAM Footprint** | `< 50 MB` | `~32 MB` |

---

## 6. How to Run the Real Remote Test

### Step 1: Start Tailscale on Ubuntu
```bash
# If not already installed:
curl -fsSL https://tailscale.com/install.sh | sh

# Connect Ubuntu to your Tailscale mesh:
sudo tailscale up

# Note your Ubuntu Tailscale IPv4:
tailscale ip -4
# Example: 100.85.20.45
```

### Step 2: Prepare Android Phone
1. Disconnect phone from home Wi-Fi (turn off Wi-Fi; enable Mobile Data LTE/5G).
2. Open **Tailscale** on Android, sign in to the same account, and tap **Connect**.
3. Install the updated WARD APK:
   ```bash
   adb install -r android/app/build/outputs/apk/debug/app-debug.apk
   ```
   *(Or transfer the APK file if ADB is over Wi-Fi)*.

### Step 3: Launch WARD Server
```bash
ward-server stream 9740
```
Verify the console lists the Tailscale endpoint:
```
  [WARD NETWORK] Server active on port 9740
  Available connection endpoints:
    - Tailscale VPN  : 100.85.20.45:9740 (tailscale0)
    - LAN / Wi-Fi    : 10.31.1.234:9740 (wlp1s0)
```

### Step 4: Connect from Android App
1. Open WARD on the phone.
2. Tap **▼ Edit Address / Port**.
3. Enter Ubuntu's Tailscale IP (`100.85.20.45`) and port `9740`.
4. Tap **Connect**.
5. Observe the live HUD:
   - Badge displays `VPN`
   - Latency, FPS, Bitrate, and 0% loss are displayed live.
