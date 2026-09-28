# WARD — Development Roadmap

**Last Updated:** 2026-09-24

---

## Milestone Overview

| #  | Milestone                    | Status       | Target                              |
|----|------------------------------|--------------|-------------------------------------|
| 0  | Environment Discovery        | ✅ Complete  | System capabilities documented      |
| 1  | Wayland Screen Capture       | ✅ Complete  | Capture frames via ScreenCast+PW    |
| 2  | Remote Input                 | ✅ Complete  | Mouse/keyboard via RemoteDesktop    |
| 3  | LAN Video Streaming          | ✅ Complete  | Android views Ubuntu desktop        |
| 4  | LAN Remote Control           | ✅ Complete  | Android controls Ubuntu desktop     |
| 5  | Clipboard Sync               | 🔄 Active   | Bidirectional text clipboard        |
| 6  | Authentication & Pairing     | ⬜ Pending   | Device identity and encryption      |
| 7  | Reconnect & Resilience       | ⬜ Pending   | Survive disconnects, sleep/wake     |
| 8  | Performance Optimization     | ⬜ Pending   | 720p/30→1080p/30→1080p/60           |
| 9  | Packaging                    | ⬜ Pending   | systemd, install/uninstall scripts  |
| 10 | Security Audit               | ⬜ Pending   | Comprehensive security review       |
| 11 | Documentation                | ⬜ Pending   | Full user and developer docs        |

---

## Milestone 0: Environment Discovery ✅

**Completed:** 2026-09-24

**Deliverables:**
- [ARCHITECTURE_RESEARCH.md](./ARCHITECTURE_RESEARCH.md)

**Key Findings:**
- Ubuntu 24.04.5, GNOME 46, Wayland, PipeWire 1.0.5
- ScreenCast portal v5 — full monitor/window/virtual capture
- RemoteDesktop portal v2 — full mouse/keyboard/touch input injection
- VA-API H.264 hardware encoding available (Intel UHD 620)
- mDNS/DNS-SD available via Avahi
- All portals active and running

**Architecture Decision:** Custom Rust server using XDG Desktop Portals directly, not wrapping GNOME Remote Desktop's RDP.

---

## Milestone 1: Wayland Screen Capture ✅

**Completed:** 2026-09-28

**Goal:** Prove that a Rust program can capture the Wayland desktop.

**Tasks:**
1. ✅ Install Rust toolchain and dependencies.
2. ✅ Create Rust project with D-Bus and PipeWire bindings.
3. ✅ Connect to ScreenCast portal via D-Bus (`subscribe_portal_response` race-free signals).
4. ✅ Request monitor capture.
5. ✅ Handle user permission dialog (persisted across sessions).
6. ✅ Receive PipeWire stream via remote fd.
7. ✅ Extract frames (SPA EnumFormat pod negotiates raw BGRx/BGRA SHM buffers).
8. ✅ Save captured frames as PNG/PPM to verify correctness (1920x1080 RGBA PNGs verified).

**Acceptance:** Captured image matches actual desktop content. (Achieved: 10 frames captured, 3 saved).

**Dependencies:** `zbus`, `pipewire-rs`, `libpipewire-0.3-dev`

---

## Milestone 2: Remote Input ✅

**Completed:** 2026-09-28

**Goal:** Prove mouse/keyboard input injection via RemoteDesktop portal.

**Tasks:**
1. ✅ Create a combined RemoteDesktop + ScreenCast session.
2. ✅ Move mouse to relative coordinates (smooth square pattern).
3. ✅ Move mouse to absolute coordinates (stream-relative target).
4. ✅ Perform left click (press and release with evdev codes).
5. ✅ Perform vertical discrete scroll (up & down).
6. ✅ Send keyboard keycodes with modifier keys (Shift + KEY_A).
7. ✅ Send keyboard keysyms (w, a, r, d sequence).

**Acceptance:** Input injection verified live on the Wayland desktop.

---

## Milestone 3: LAN Video Streaming ✅

**Completed:** 2026-09-28

**Goal:** Stream the desktop to an Android phone over LAN.

**Tasks:**
1. ✅ Implement H.264 encoding (`openh264`, BGRx->YUV420p converter, dynamic IDR keyframe forcing).
2. ✅ Define WARD protocol binary format (12-byte header, typed payload, async reader/writer in Rust & Kotlin).
3. ✅ Implement TLS 1.3 TCP server (`tokio-rustls`, `rcgen` self-signed certs, port 9740).
4. ✅ Stream encoded frames to connected client (dynamic client bridging, e2e integration test verified).
5. ✅ Create Android project (Kotlin, Jetpack Compose, Material 3, Gradle wrapper).
6. ✅ Implement TLS client (`WardClient` with TLS 1.3 SSLSocket, handshake, async input dispatcher).
7. ✅ Implement H.264 decoding (`VideoDecoder` with hardware-accelerated `MediaCodec`).
8. ✅ Display decoded frames in SurfaceView (`StreamScreen` with live stats overlay & touch controls).

**Acceptance:** Android client connects via TLS 1.3, decodes H.264 video on SurfaceView, and sends mouse/touch events.

---

## Milestone 4: LAN Remote Control ✅

**Completed:** 2026-09-28

**Goal:** Android can control the Ubuntu desktop.

**Tasks:**
1. ✅ Implement touch gesture recognition on Android (1-finger pointer, tap click, drag, 2-finger scroll/right click, 3-finger middle click/Super swipe).
2. ✅ Map gestures → protocol input messages (MouseMove, MouseButton, MouseScroll, KeyDown, KeyUp).
3. ✅ Send input messages from Android → server over TLS with sequential FIFO queue and sticky key prevention.
4. ✅ Server translates messages → RemoteDesktop portal D-Bus calls with keycode/keysym handling and held key cleanup.
5. ✅ Adaptive landscape dock, dedicated thumb scroll signs (`▲` / `▼`), and auto-contracting controls.

**Acceptance:** Can move mouse, click, scroll, and type from Android with high responsiveness and zero sticky keys.

---

## Milestone 5: Clipboard Sync

**Goal:** Bidirectional text clipboard.

**Tasks:**
1. Monitor Ubuntu clipboard (Wayland clipboard protocol via portal or wl-clipboard).
2. Send clipboard text to Android via protocol message.
3. Monitor Android clipboard.
4. Send clipboard text to Ubuntu.
5. Implement deduplication (hash-based).

**Acceptance:** Copy on Ubuntu → paste on Android, and vice versa. No infinite loops.

---

## Milestone 6: Authentication & Pairing

**Goal:** Secure device identity.

**Tasks:**
1. Generate Ed25519 keypair on server.
2. Generate Ed25519 keypair on Android (Android Keystore).
3. Implement pairing flow (6-digit code).
4. Exchange public keys during pairing.
5. Store trusted devices on server.
6. Implement mutual TLS with client certificates.
7. Implement device revocation.

**Acceptance:** Unpaired device is rejected. Paired device connects. Revoked device is rejected.

---

## Milestone 7: Reconnect & Resilience

**Goal:** Survive real-world conditions.

**Tasks:**
1. Detect network disconnect.
2. Implement automatic reconnect with backoff.
3. Handle laptop sleep/wake (portal session invalidation).
4. Handle monitor disconnect/reconnect.
5. Clean session teardown.

**Acceptance:** Temporarily disconnecting Wi-Fi and reconnecting restores the session.

---

## Milestone 8: Performance Optimization

**Goal:** Achieve stable performance targets.

**Tasks:**
1. Implement FPS/latency/bitrate measurement.
2. Profile CPU usage.
3. Optimize encode pipeline.
4. Implement adaptive quality.
5. Target: 720p/30fps → 1080p/30fps → 1080p/60fps.

**Acceptance:** Measured (not claimed) performance meets targets.

---

## Milestone 9: Packaging

**Goal:** Reproducible installation.

**Tasks:**
1. Create `install.sh` / `uninstall.sh`.
2. Create systemd user service.
3. Create `ward` CLI.
4. Desktop notification integration.

---

## Milestone 10: Security Audit

**Goal:** Verify security model.

**Tasks:** See §21 of project spec.

---

## Milestone 11: Documentation

**Goal:** Complete user and developer documentation.

**Tasks:** README, all docs/ files, troubleshooting.
