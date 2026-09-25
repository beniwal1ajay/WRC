# WARD — Architecture Research & Environment Discovery

**Date:** 2026-09-24
**Status:** Complete — Milestone 0

---

## 1. System Environment

| Component               | Value                                              |
|-------------------------|----------------------------------------------------|
| **OS**                  | Ubuntu 24.04.5 LTS (Noble Numbat)                  |
| **Kernel**              | 7.0.0-31-generic x86_64                            |
| **Session Type**        | `wayland` ✅                                       |
| **Desktop**             | `ubuntu:GNOME`                                     |
| **GNOME Shell**         | 46.0                                               |
| **Mutter**              | libmutter-14 46.2                                  |
| **Wayland Library**     | libwayland 1.22.0                                  |
| **Display**             | eDP-1, 1920×1080 @ 60 Hz (built-in)                |
| **CPU**                 | Intel Core i5-8350U @ 1.70 GHz, 4C/8T              |
| **GPU**                 | Intel UHD Graphics 620 (integrated)                |
| **RAM**                 | 16 GB                                              |
| **Machine**             | HP EliteBook 830 G5                                |

**Conclusion:** Standard Ubuntu 24.04 GNOME Wayland session on Intel hardware. Exactly the target environment.

---

## 2. PipeWire

| Component               | Value                               |
|--------------------------|-------------------------------------|
| **PipeWire version**     | 1.0.5                              |
| **Service status**       | ✅ Active (running)                 |
| **PipeWire-Pulse**       | ✅ Active (running)                 |
| **WirePlumber**          | ✅ Active (running)                 |

**Conclusion:** PipeWire is fully operational. ScreenCast portal frames will be delivered via PipeWire streams. No setup needed.

---

## 3. XDG Desktop Portal

| Component                        | Value                                     |
|----------------------------------|-------------------------------------------|
| **xdg-desktop-portal**           | 1.18.4 ✅                                 |
| **xdg-desktop-portal-gnome**     | 46.2 ✅                                   |
| **Portal service status**        | Active (running)                          |
| **Portal GNOME backend status**  | Active (running)                          |

### 3.1 ScreenCast Portal (org.freedesktop.portal.ScreenCast)

| Property / Method       | Available | Notes                                           |
|-------------------------|-----------|-------------------------------------------------|
| `CreateSession`         | ✅        | Creates a ScreenCast session                    |
| `SelectSources`         | ✅        | Select monitors/windows                         |
| `Start`                 | ✅        | Begin the screencast, yields PipeWire stream    |
| `OpenPipeWireRemote`    | ✅        | Get PipeWire fd for the stream                  |
| `AvailableSourceTypes`  | `7`       | Monitor (1) + Window (2) + Virtual (4) = all    |
| `AvailableCursorModes`  | `7`       | Hidden (1) + Embedded (2) + Metadata (4) = all  |
| `version`               | `5`       | Latest portal version                           |

**Conclusion:** Full ScreenCast portal support. Can capture monitors, windows, or virtual displays. Cursor can be embedded in the stream or provided as metadata. Version 5 is current.

### 3.2 RemoteDesktop Portal (org.freedesktop.portal.RemoteDesktop)

| Property / Method                  | Available | Notes                                    |
|------------------------------------|-----------|------------------------------------------|
| `CreateSession`                    | ✅        | Creates a RemoteDesktop session          |
| `SelectDevices`                    | ✅        | Select keyboard/pointer/touchscreen      |
| `Start`                            | ✅        | Begin session                            |
| `ConnectToEIS`                     | ✅        | Get EIS fd for libei integration         |
| `NotifyPointerMotion`              | ✅        | Relative mouse movement                  |
| `NotifyPointerMotionAbsolute`      | ✅        | Absolute mouse movement (stream-relative)|
| `NotifyPointerButton`              | ✅        | Mouse button press/release               |
| `NotifyPointerAxis`                | ✅        | Continuous scroll (touchpad-like)        |
| `NotifyPointerAxisDiscrete`        | ✅        | Discrete scroll (mouse wheel)            |
| `NotifyKeyboardKeycode`            | ✅        | Evdev keycode input                      |
| `NotifyKeyboardKeysym`             | ✅        | X keysym input                           |
| `NotifyTouchDown`                  | ✅        | Touch start                              |
| `NotifyTouchMotion`                | ✅        | Touch drag                               |
| `NotifyTouchUp`                    | ✅        | Touch end                                |
| `AvailableDeviceTypes`             | `7`       | Keyboard (1) + Pointer (2) + Touch (4)   |
| `version`                          | `2`       |                                          |

**Conclusion:** Full RemoteDesktop portal support. All required input methods (mouse, keyboard, scroll, touch) are available via D-Bus. EIS connection is also available for lower-level libei integration.

### 3.3 Combined ScreenCast + RemoteDesktop Session

The ScreenCast and RemoteDesktop portals can share a single session. This is the intended architecture:

1. Create a RemoteDesktop session.
2. Call `SelectDevices` to request keyboard + pointer.
3. Call `SelectSources` (ScreenCast) on the same session to request monitor capture.
4. Call `Start` — user is prompted once for both screen sharing and remote control.
5. Receive PipeWire stream for screen capture.
6. Use `NotifyPointer*` / `NotifyKeyboard*` for input injection.

This is **exactly** the architecture WARD needs.

---

## 4. GNOME Remote Desktop (gnome-remote-desktop)

| Property                 | Value                                              |
|--------------------------|----------------------------------------------------|
| **Package**              | gnome-remote-desktop 46.3                          |
| **Service status**       | ❌ Inactive (disabled)                             |
| **RDP enabled**          | `false` (via gsettings)                            |
| **Default port**         | 3389                                               |
| **Screen share mode**    | `mirror-primary`                                   |
| **View-only**            | `false` (full control available)                   |
| **TLS cert path**        | `~/.local/share/gnome-remote-desktop/certificates/`|
| **CLI tool**             | `grdctl` available                                 |

### 4.1 Mutter RemoteDesktop Interface

| Item                     | Value     |
|--------------------------|-----------|
| `CreateSession`          | ✅        |
| `SupportedDeviceTypes`   | `7` (all) |
| `Version`                | `1`       |

### 4.2 GNOME Remote Desktop Assessment (Phase A)

**Can GNOME Remote Desktop + RDP satisfy WARD's requirements?**

| Requirement                      | GRD/RDP Status | Notes                                                |
|----------------------------------|----------------|------------------------------------------------------|
| Screen streaming                 | ✅             | RDP mirrors primary display                          |
| Mouse control                    | ✅             | Full pointer support                                 |
| Keyboard input                   | ✅             | Full keyboard support                                |
| Clipboard sync                   | ✅             | RDP supports clipboard channel                       |
| Custom Android client            | ⚠️ Partial    | Would need an RDP client library for Android         |
| Pairing/device identity          | ❌             | RDP uses username/password, not device pairing       |
| Custom protocol extensions       | ❌             | RDP is a fixed protocol                              |
| Latency/FPS stats on client      | ⚠️ Limited    | Standard RDP clients show some stats                 |
| Connection quality display       | ⚠️ Limited    | Limited in standard RDP                              |
| Device revocation                | ❌             | No device identity model                             |
| Custom authentication model      | ❌             | Locked to RDP auth                                   |
| Custom touch gesture mapping     | ❌             | Depends on RDP client capability                     |

**Phase A Decision:**

GNOME Remote Desktop provides the underlying infrastructure (portals, PipeWire, input injection) that WARD will use. However, the GRD *service itself* (as an RDP server) does not satisfy WARD's requirements because:

1. **No device pairing model** — RDP uses username/password authentication.
2. **No custom Android UI** — We'd be limited to existing RDP clients.
3. **No custom protocol** — Cannot add WARD-specific features (device identity, stats, etc.).
4. **No device revocation** — RDP has no concept of trusted device identity.

**Architecture Decision:** WARD will **not** use gnome-remote-desktop as its server. Instead, WARD will use the **same underlying APIs** that GRD uses:

- **XDG Desktop Portal ScreenCast** → PipeWire → Screen capture
- **XDG Desktop Portal RemoteDesktop** → Input injection
- **Custom Rust server** with WARD protocol
- **Custom Kotlin/Compose Android client**

This gives us full control while remaining Wayland-native and using the proper GNOME APIs.

---

## 5. libei / EIS (Emulated Input)

| Component          | Version | Installed |
|--------------------|---------|-----------|
| **libei1**         | 1.2.1   | ✅        |
| **libeis1**        | 1.2.1   | ✅        |
| **pkg-config**     | —       | ❌ (dev packages not installed) |

**What is libei/EIS?**

- **EIS** (Emulated Input Server) runs inside the Wayland compositor (Mutter).
- **libei** is the client library that connects to EIS for input injection.
- The RemoteDesktop portal's `ConnectToEIS` method provides an EIS socket fd.
- This is the preferred modern mechanism for input injection under Wayland.

**Assessment:** For the MVP, the RemoteDesktop portal's D-Bus `Notify*` methods are sufficient for all input injection. They internally use EIS. Direct libei usage would be an optimization for reduced latency, but is not needed initially.

**Decision:** MVP will use D-Bus `Notify*` methods. Direct libei integration deferred to performance optimization phase.

---

## 6. Video Encoding

### 6.1 Hardware Acceleration (VA-API)

| Component                 | Available | Notes                                    |
|---------------------------|-----------|------------------------------------------|
| **Intel iHD driver**      | ✅        | `/usr/lib/x86_64-linux-gnu/dri/iHD_drv_video.so` |
| **i965 driver**           | ✅        | `/usr/lib/x86_64-linux-gnu/dri/i965_drv_video.so` |
| **intel-media-va-driver** | ✅        | 24.1.0                                   |
| **vainfo**                | ❌        | Not installed (can be installed)          |

Intel UHD 620 (Kaby Lake Refresh / Coffee Lake) supports:
- H.264 hardware encode and decode
- VP8 decode
- VP9 decode (some profiles)
- HEVC decode (some profiles)

**The Intel iHD driver should provide H.264 hardware encoding** via VA-API. This is ideal for WARD's use case.

### 6.2 GStreamer

| Component                 | Version | Notes                                    |
|---------------------------|---------|------------------------------------------|
| **GStreamer**              | 1.24.2  | Full installation                        |
| **vaapih264enc**          | ✅      | VA-API H.264 hardware encoder            |
| **vaapih264dec**          | ✅      | VA-API H.264 hardware decoder            |
| **x264enc**               | ✅      | Software H.264 encoder (fallback)        |

### 6.3 FFmpeg

FFmpeg is **not installed** on the system. However, the Rust server will use direct VA-API or GStreamer bindings rather than FFmpeg CLI.

### 6.4 Encoding Strategy

| Priority | Encoder         | Type     | Expected Performance                        |
|----------|-----------------|----------|---------------------------------------------|
| 1        | VA-API H.264    | Hardware | 1080p/60fps easily, very low CPU usage      |
| 2        | x264 (software) | Software | 720p/30fps feasible on i5-8350U             |
| 3        | OpenH264        | Software | Fallback, lower quality                     |

**Decision:** The Rust server will use VA-API H.264 hardware encoding via the `iHD` driver as the primary encoder, with software x264 as fallback. GStreamer pipeline or direct VA-API bindings will be evaluated during Milestone 1.

---

## 7. Networking

| Component         | Status   | Notes                                         |
|-------------------|----------|-----------------------------------------------|
| **Wi-Fi**         | ✅ UP    | `wlp1s0` (DORMANT = connected but idle)       |
| **Ethernet**      | DOWN     | `enp0s31f6` (no cable)                        |
| **Docker**        | Present  | `docker0` (not relevant)                      |
| **VMware**        | Present  | `vmnet1`, `vmnet8` (not relevant)             |
| **Avahi (mDNS)**  | ✅ Active| mDNS/DNS-SD discovery available               |
| **OpenSSL**       | 3.0.13   | TLS 1.3 support ✅                            |

**Decision:**

- **Transport:** TLS 1.3 over TCP for the MVP. Simple, well-understood, and sufficient for LAN.
- **Discovery:** mDNS/DNS-SD via Avahi. The server will advertise `_ward._tcp.local`.
- **Binding:** Server binds to `0.0.0.0` on a configurable port (default: `9740`), or optionally to a specific interface.

**Why TLS 1.3 over TCP and not QUIC?**

1. TLS 1.3 over TCP is simpler to implement correctly.
2. TCP provides reliable ordered delivery which simplifies protocol handling.
3. For LAN, TCP latency is negligible.
4. QUIC provides benefits mainly for unreliable networks and connection migration — not critical for LAN MVP.
5. QUIC can be added later as an optimization for Internet access (Milestone post-MVP).

---

## 8. Development Tools

| Tool              | Status        | Notes                                    |
|-------------------|---------------|------------------------------------------|
| **Rust**          | ❌ Not installed | Must install via rustup                |
| **Cargo**         | ❌ Not installed | Comes with Rust                        |
| **Java**          | ✅ JDK 24.0.2 | Java SE Runtime                          |
| **Android SDK**   | ❌ ANDROID_HOME not set | May need Android Studio or CLI SDK |
| **Gradle**        | ❌ Not installed | Will use Gradle wrapper in project     |
| **OpenSSL**       | ✅ 3.0.13     |                                          |

**Required installations before development:**

1. **Rust toolchain** via `rustup`
2. **System dev packages** for building: `libpipewire-0.3-dev`, `libei-dev`, `libssl-dev`, `libdbus-1-dev`, `pkg-config`, `build-essential`
3. **Android SDK/Studio** for the Android client
4. **vainfo** for verifying VA-API capabilities

---

## 9. Security Infrastructure

| Component             | Available | Notes                                     |
|-----------------------|-----------|-------------------------------------------|
| **OpenSSL 3.0**       | ✅        | TLS 1.3 support                          |
| **D-Bus session bus** | ✅        | `/run/user/1000/bus`                      |
| **Linux keyring**     | ✅        | GNOME Keyring for secret storage          |
| **File permissions**  | ✅        | Standard POSIX                            |

**Pairing & Authentication Design:**

1. Each device generates an Ed25519 keypair.
2. First pairing uses a 6-digit code displayed on the server.
3. Devices exchange public keys over TLS during pairing.
4. Subsequent connections use mutual TLS with client certificates.
5. Server stores trusted device public keys in `~/.config/ward/devices/`.
6. Private key stored with `0600` permissions.
7. Android uses Android Keystore for key storage.

---

## 10. Architecture Decision Summary

| Decision                      | Choice                                            | Rationale                              |
|-------------------------------|---------------------------------------------------|----------------------------------------|
| **Screen capture**            | XDG ScreenCast Portal + PipeWire                  | Wayland-native, GNOME-supported        |
| **Input injection**           | XDG RemoteDesktop Portal (D-Bus Notify methods)   | Wayland-native, all input types        |
| **Video encoding**            | VA-API H.264 (hw), x264 (sw fallback)             | Hardware available, low CPU usage      |
| **Server language**           | Rust                                              | Memory safety, performance             |
| **Android language**          | Kotlin + Jetpack Compose                          | Modern Android stack                   |
| **Transport**                 | TLS 1.3 over TCP                                  | Simple, reliable for LAN               |
| **Discovery**                 | mDNS/DNS-SD via Avahi                             | Standard, already available            |
| **Authentication**            | Ed25519 keypairs + pairing code                   | Public-key device identity             |
| **Use GRD service?**          | No — use same underlying APIs directly            | Need custom protocol and auth          |
| **Clipboard**                 | Custom protocol messages over TLS                 | UTF-8 text, bidirectional, dedup       |
| **Android min API**           | API 26 (Android 8.0)                              | 99%+ device coverage, TLS 1.3 support |
| **Default port**              | 9740                                              | Unassigned IANA port                   |

---

## 11. Risk Assessment

| Risk                                      | Likelihood | Impact | Mitigation                                    |
|-------------------------------------------|------------|--------|-----------------------------------------------|
| Portal permission dialog every launch     | Medium     | High   | Investigate `Restore` token (portal v4+)      |
| VA-API encoding issues on this hardware   | Low        | Medium | x264 software fallback available              |
| PipeWire buffer format incompatibility    | Low        | Medium | Handle multiple DMA-BUF/SHM formats           |
| D-Bus input injection latency             | Medium     | Medium | Can switch to direct libei if needed           |
| Android MediaCodec H.264 decode issues    | Low        | Low    | Software decode fallback                       |
| Clipboard loop                            | Medium     | Low    | Content hash deduplication                     |
| Session invalidation on sleep/wake        | High       | Medium | Reconnection with new portal session           |

---

## 12. Component Architecture

```
Android Phone                          Ubuntu Laptop
┌─────────────────────┐                ┌──────────────────────────────────────┐
│                     │                │                                      │
│  WARD Android App   │                │  WARD Server (Rust)                  │
│                     │                │                                      │
│  ┌───────────────┐  │   TLS 1.3     │  ┌──────────┐   ┌─────────────────┐ │
│  │ Touch/Gesture │──┼───────────────►│  │ Protocol │──►│ RemoteDesktop   │ │
│  │ Handler       │  │   over TCP    │  │ Handler  │   │ Portal (D-Bus)  │ │
│  └───────────────┘  │               │  └──────────┘   └─────────────────┘ │
│                     │               │                                      │
│  ┌───────────────┐  │               │  ┌──────────┐   ┌─────────────────┐ │
│  │ H.264 Decoder │◄─┼───────────────┤  │ H.264    │◄──│ ScreenCast      │ │
│  │ (MediaCodec)  │  │  Video stream │  │ Encoder  │   │ Portal + PW     │ │
│  └───────────────┘  │               │  │ (VA-API) │   │                 │ │
│                     │               │  └──────────┘   └─────────────────┘ │
│  ┌───────────────┐  │               │                                      │
│  │ Surface View  │  │               │  ┌──────────┐                       │
│  │ (Render)      │  │               │  │ Clipboard│                       │
│  └───────────────┘  │               │  │ Sync     │                       │
│                     │               │  └──────────┘                       │
│  ┌───────────────┐  │               │                                      │
│  │ Clipboard     │  │               │  ┌──────────┐                       │
│  │ Manager       │  │               │  │ mDNS     │                       │
│  └───────────────┘  │               │  │ (Avahi)  │                       │
└─────────────────────┘                └──────────────────────────────────────┘
```

---

## 13. Next Steps

1. ✅ Environment discovery complete (this document).
2. **Install Rust toolchain and dev dependencies.**
3. **Begin Milestone 1:** Proof of Wayland screen capture.
   - Create Rust project with PipeWire and D-Bus bindings.
   - Connect to ScreenCast portal.
   - Receive PipeWire frames.
   - Save frames to verify capture works.
4. **Begin Milestone 2:** Proof of remote input.
   - Connect to RemoteDesktop portal.
   - Move mouse programmatically.
   - Send keyboard events.
