# WARD — Client Stability, Background Lifecycle & Locked-Screen Analysis

This document details the stability, lifecycle persistence, and Wayland session behavior of WARD across Linux (GNOME Wayland) and Android clients.

---

## 1. Executive Summary & Test Results Matrix

| Test Case | Status | Root Cause & Implementation Details |
| :--- | :---: | :--- |
| **Ubuntu Lock Support** | **FAIL (By Design in GNOME)** | GNOME Mutter compositor terminates user-level ScreenCast & RemoteDesktop sessions on screen lock to protect credentials. See Section 2. |
| **Remote Connection While Ubuntu Locked** | **FAIL (By Design in GNOME)** | Mutter rejects `CreateSession` / `RecordMonitor` over user D-Bus while the session shield is active. |
| **Screen Capture While Ubuntu Locked** | **FAIL (By Design in GNOME)** | PipeWire stream drops (`Streaming -> Paused / Error`) as Mutter ceases frame delivery. |
| **Remote Input While Ubuntu Locked** | **FAIL (By Design in GNOME)** | Mutter destroys the RemoteDesktop session object; synthetic input rejected. |
| **Android Home Button** | **PASS** | Session decoupled into `WardSessionManager` and `WardRemoteSessionService`. Network socket stays active. |
| **Android App Switching** | **PASS** | Switching apps maintains TCP/TLS connection and ping heartbeats in background service. |
| **Android Background Session** | **PASS** | Foreground service with persistent notification keeps WARD running. Frame decoding paused to save battery. |
| **Return to WARD** | **PASS** | `attachSurface()` binds new Surface, sends `requestKeyframe()`, and resumes video instantly without reconnecting. |
| **Phone Display Stays Awake** | **PASS** | Managed via `FLAG_KEEP_SCREEN_ON` on the active window during streaming. |
| **Screen Timeout Restored on Disconnect** | **PASS** | `FLAG_KEEP_SCREEN_ON` cleared on disconnect and automatically suspended when app is backgrounded. |
| **Landscape Mode** | **PASS** | Display fills available width/height with proper aspect-ratio letterboxing/pillarboxing. |
| **Portrait Mode** | **PASS** | Video displayed at top, lower area acts as multi-gesture trackpad without UI distortion. |
| **Gesture Navigation** | **PASS** | Touch events starting inside system bar insets (`WindowInsetsCompat`) are rejected and yielded to Android OS. |
| **3-Button Navigation** | **PASS** | Bottom navigation bar zone is protected; no unintended clicks sent to host PC. |
| **Mouse Coordinate Mapping** | **PASS** | Clamped strictly to active video rendering bounds (`computeVideoBounds`). |
| **Touch / System UI Boundary** | **PASS** | Coordinates outside video area in landscape or in system insets never leak to the remote host. |
| **Rotation While Connected** | **PASS** | Configuration changes survive; Surface destruction/recreation seamlessly swaps decoder output surface. |
| **Temporary Network Interruption** | **PASS** | Exponential backoff auto-reconnect engine with jitter automatically recovers dropped links. |
| **Reconnection** | **PASS** | Reconnection re-evaluates dynamic network candidate endpoints across LAN and Tailscale VPN. |

---

## 2. Ubuntu Locked-Screen Support: Deep Dive & Architectural Limitations

### The Objective
Allow an authenticated Android client to connect to an Ubuntu PC whose local screen is locked (`Super+L`), stream the lock screen, and permit typing the user password remotely to unlock the machine.

### Findings on GNOME Wayland (Mutter)
WARD runs as a user daemon (`ward-host.service`) inside the logged-in Wayland desktop session, communicating with GNOME via:
- `org.gnome.Mutter.RemoteDesktop`
- `org.gnome.Mutter.ScreenCast`
- PipeWire Media Server

#### Exact Sequence Observed When Ubuntu Locks:
1. **Screen Lock Engaged**: User presses `Super+L` or GNOME idle timer expires. GNOME Shell activates the Screen Shield (`org.gnome.ScreenSaver`).
2. **Mutter Session Teardown**: GNOME Shell immediately closes all active user `ScreenCast.Session` and `RemoteDesktop.Session` D-Bus objects, emitting the `Closed` signal.
3. **PipeWire Stream Interruption**: PipeWire stream transitions from `Streaming` to `Paused / Error ("no more input formats")`. Frame capture halts.
4. **Input Rejection**: D-Bus calls to `NotifyPointerMotion` and `NotifyKeyboardKeycode` fail with `UnknownObject` because Mutter deleted the session object.
5. **Reconnection While Locked**: When Android connects while locked, WARD calls `org.gnome.Mutter.ScreenCast.CreateSession`. Mutter denies the call or fails `RecordMonitor` with `GDBus.Error:org.gnome.Mutter.ScreenCast.Error.Denied` or `Failed: Screen is locked`.

### Root Cause
This is **intentional security enforcement** in GNOME Wayland:
> Wayland compositors prohibit user-space processes from capturing the screen or injecting synthetic keystrokes while the session is locked. This prevents background spyware from logging lock-screen passwords or capturing sensitive lock-screen notifications.

### Why Insecure Workarounds Must NOT Be Used:
- **Disabling screen lock (`lock-enabled false`)**: Exposes the physical machine to anyone with physical access.
- **Using virtual X11 / Xwayland bridges**: Violates Wayland security and does not work for native Wayland compositing.
- **Automatically unlocking via PAM script**: Weakens system authentication and compromises user credentials.

### Supported Upstream Path for Unattended Locked Access
To achieve true lock-screen and login-screen remote access under GNOME Wayland:
1. **GNOME Remote Login (`gnome-remote-desktop-system.service`)**:
   Introduced in GNOME 44+, GNOME provides a **system-level daemon** running inside GDM's display manager session rather than the user's desktop session. GDM exposes a dedicated headless RDP session that prompts for system credentials and logs the user in.
2. **Community Extension ("Allow Locked Remote Desktop")**:
   Some users install this GNOME Shell extension, which monkey-patches GNOME Shell's `screenShield` event handlers to keep user remote desktop sessions alive during lock. However, this requires explicit user installation and can break across GNOME version updates.

**WARD Decision**: WARD respects Ubuntu's native security boundaries. If Ubuntu locks, WARD cleanly ends the active stream and awaits unlock. WARD's daemon remains running (`ward-host.service`), ready to accept connections the instant the session is unlocked.

---

## 3. Android Background Architecture

### Problem
Previously, connection state, `WardClient`, and `VideoDecoder` were held directly inside the Compose Activity (`WardRemoteApp`). When the user pressed Home, switched apps, or rotated the phone:
- Compose disposed the UI (`DisposableEffect(Unit)`).
- The network socket was forcefully disconnected.
- Returning to WARD required starting a new session from scratch.

### Architectural Solution
We separated the UI layer from the networking and decoding layer:

```
┌────────────────────────────────────────────────────────┐
│                   Android OS Lifecycle                 │
└────────────────────────────────────────────────────────┘
                           │
             ┌─────────────┴─────────────┐
             ▼                           ▼
┌─────────────────────────┐ ┌─────────────────────────┐
│     WARD Activity       │ │ WardRemoteSessionService │
│  (UI / Compose Screen)  │ │   (Foreground Service)   │
└─────────────────────────┘ └─────────────────────────┘
             │                           │
             │      Observes State       │ Keeps Process Alive
             ├───────────────────────────┤ Persistent Notification
             │                           │
             ▼                           ▼
┌────────────────────────────────────────────────────────┐
│                   WardSessionManager                   │
│   - Connection StateFlow                               │
│   - WardClient (TCP / TLS / P2P socket engine)         │
│   - VideoDecoder (dynamic Surface attachment)          │
│   - Auto-reconnect & Ping Telemetry Heartbeats         │
└────────────────────────────────────────────────────────┘
```

### Key Components

1. **`WardRemoteSessionService`**:
   - Registered as an Android Foreground Service with `foregroundServiceType="connectedDevice"`.
   - Posts an ongoing notification: `"WARD Remote Session Active — Connected to [Host]"`.
   - Includes a quick **"Disconnect"** action button in the notification tray.
   - Prevents the Android system from killing the background process during app switching.

2. **`WardSessionManager`**:
   - Retains the active `WardClient` and `VideoDecoder` instances across Activity destruction.
   - Exposes reactive `StateFlow` streams (`connectionState`, `fps`, `streamWidth`, `streamHeight`, `sessionStats`).

3. **Background Battery & CPU Conservation**:
   - When the Activity is in the background (`detachSurface()`), incoming H.264 P-frames are **dropped before MediaCodec decoding**.
   - Network heartbeats (Ping/Pong) continue flowing to keep the connection alive without burning CPU/GPU rendering invisible frames.
   - Upon return (`attachSurface()`), WARD immediately calls `requestKeyframe()`. GNOME delivers a fresh IDR frame, and the screen restores instantly.

---

## 4. Screen Awake & Power Management

- **Active Session**: While `connectionState is ConnectionState.Connected`, `MainActivity` applies `WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON` to the window.
- **Backgrounding**: When the user switches away, Android automatically suspends the window flags. The phone follows normal system screen timeout rules.
- **Disconnection**: When disconnected, `FLAG_KEEP_SCREEN_ON` is immediately cleared.
- **Zero Special Permissions**: No battery-draining wake locks (`WAKE_LOCK`) or dangerous permissions are required.

---

## 5. Landscape, Immersive Mode & Coordinate Boundaries

### Immersive Sticky Mode
During active streaming, WARD engages immersive sticky mode via `WindowInsetsControllerCompat`:
- Hides status bars and navigation bars (`WindowInsetsCompat.Type.systemBars()`).
- Sets behavior to `BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE`.
- Maximizes visible screen area.

### System UI Insets Protection
When system bars are transiently swiped open (e.g., gesture navigation pill or 3-button bar):
- `ViewCompat.getRootWindowInsets(v)` extracts the active insets.
- Any touch event starting within the navigation or status bar bounds returns `false` and is yielded directly to Android OS.
- Android system gestures never send spurious clicks or drag events to the remote PC.

### Precise Display Bounds Mapping
In landscape orientation, phone screens (typically 20:9 or 21:9) pillarbox standard 16:9 remote desktop streams.
- `computeVideoBounds` calculates the exact rectangle occupied by the video surface (`[videoLeft, videoTop, videoRight, videoBottom]`).
- Any touch starting in the black letterbox/pillarbox margins returns `false`.
- Physical mouse events (`InputDevice.SOURCE_MOUSE`) are captured via `setOnGenericMotionListener` and mapped within valid display bounds.

---

## 6. Build & Deployment Verification

- **Server Release**: Built with `cargo build --release`, installed to `~/.local/bin/ward-server`, running under `ward-host.service`.
- **Android APK**: Compiled with Gradle (`./gradlew assembleDebug`), verified clean, stored at:
  [`android/app/build/outputs/apk/debug/ward-v1.1.apk`](file:///home/ajay/Desktop/WRC/ward/android/app/build/outputs/apk/debug/ward-v1.1.apk).
