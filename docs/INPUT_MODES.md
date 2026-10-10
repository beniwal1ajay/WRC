# WARD Dual Input Mode: Cursor + Touch

## Overview

WARD Remote Desktop features dual input mode support on Android, allowing seamless switching during an active remote session on LAN and VPN:

1. 🖱 **Cursor Mode (Default)**: Relative trackpad touchpad interaction model. Finger swipes move the remote mouse cursor relatively, preserving the existing precision desktop workflow.
2. 👆 **Touch Mode**: Direct-touch interaction model (similar to Chrome Remote Desktop). Touches on the Android display map directly to the corresponding coordinates on the remote Ubuntu desktop.

Switching between modes occurs instantly during an active remote session without reconnecting, restarting video streaming, or disturbing authentication.

---

## Architecture

Both modes operate within the Android client and feed into the unified **Input Mapper**, which produces standard WARD protocol messages (`MouseMove`, `MouseButton`, `MouseScroll`) dispatched to GNOME Mutter RemoteDesktop over Wayland.

```
                    Android Screen Touch
                              │
                    ┌─────────┴─────────┐
                    │                   │
               Cursor Mode          Touch Mode
           (Relative Trackpad)   (Direct Coordinates)
                    │                   │
                    └─────────┬─────────┘
                              ↓
                      Coordinate Mapper
                      & Cursor Tracker
                              ↓
                     Unified Input Mapper
                              ↓
                     Existing WARD Input
                 (MouseMove, Button, Scroll)
                              ↓
                   GNOME Mutter RemoteDesktop
                              ↓
                           Wayland
```

### Components

1. **`InputMode.kt`**:
   - `enum class InputMode`: `CURSOR`, `TOUCH`.
   - Defaults to `CURSOR`.
2. **`CoordinateMapper.kt`**:
   - Transforms Android display coordinates `(touchX, touchY)` into remote desktop coordinates `(remoteX, remoteY)`.
   - Accounts dynamically for:
     - Remote desktop resolution (e.g. 1920×1080, 1280×720, 2560×1440, 16:10, 4:3)
     - Android display resolution & screen aspect ratio
     - Letterboxing (portrait padding) and pillarboxing (landscape padding)
     - System UI boundaries (Android status bar, navigation bar, cutouts via `WindowInsetsCompat`)
     - Immersive / fullscreen boundaries
   - Any touch falling outside the actual remote viewport is strictly rejected and never dispatched to Ubuntu.
3. **`CursorTracker.kt`**:
   - Maintains virtual pointer position state shared across both modes.
   - Synchronizes position during relative motion and direct touches.
   - Ensures switching between Cursor Mode and Touch Mode is perfectly seamless.
4. **`CursorInputHandler.kt`**:
   - Preserves 100% of the existing working Cursor Mode touchpad logic.
5. **`TouchInputHandler.kt`**:
   - Direct-touch gesture interpreter for single tap, double tap, long press, drag, two-finger scroll, and two-finger tap.
6. **`WardInputMapper.kt`**:
   - Coordinates the active mode and dispatches input events through `WardInputListener`.

---

## Gestures Reference

| Gesture | Cursor Mode (🖱 Trackpad) | Touch Mode (👆 Direct Touch) |
| :--- | :--- | :--- |
| **Single Finger Move** | Relative cursor movement (~100Hz paced) | Updates drag / tracking if active |
| **Single Tap** | Left click at current cursor location | Moves cursor to touch point and left-clicks |
| **Double Tap** | Double left click at cursor location | Moves cursor to touch point and double-clicks |
| **Long Press (>450ms)** | Enters drag mode | Right click (context menu) at touch point |
| **One-Finger Drag** | Mouse drag after hold | Warps cursor to touch, mouse button down, drags window/selection |
| **Two-Finger Vertical Drag** | Vertical mouse wheel scroll | Vertical mouse wheel scroll |
| **Two-Finger Horizontal Drag**| Horizontal mouse wheel scroll | Horizontal mouse wheel scroll |
| **Two-Finger Tap (<250ms)** | Right click at cursor position | Right click at midpoint touch coordinate |
| **Three-Finger Swipe Up** | Super / Windows key trigger | Super / Windows key trigger |
| **Three-Finger Tap (<250ms)** | Middle mouse click | Middle mouse click |
| **Physical Mouse** | Native pass-through | Native pass-through |

---

## Coordinate Transformation

Given:
- Touch receiving View dimensions: $W_{\text{view}}$, $H_{\text{view}}$
- Remote desktop dimensions: $W_{\text{remote}}$, $H_{\text{remote}}$
- Remote desktop aspect ratio: $A_{\text{remote}} = W_{\text{remote}} / H_{\text{remote}}$
- View aspect ratio: $A_{\text{view}} = W_{\text{view}} / H_{\text{view}}$

The viewport is centered geometrically within the container view matching the exact SurfaceView placement:
- If $A_{\text{view}} > A_{\text{remote}}$ (Pillarboxing / Landscape):
  $$H_{\text{vp}} = H_{\text{view}}, \quad W_{\text{vp}} = H_{\text{view}} \times A_{\text{remote}}$$
  $$\text{Left} = \frac{W_{\text{view}} - W_{\text{vp}}}{2}, \quad \text{Top} = 0$$
- If $A_{\text{view}} \le A_{\text{remote}}$ (Letterboxing / Portrait):
  $$W_{\text{vp}} = W_{\text{view}}, \quad H_{\text{vp}} = W_{\text{view}} / A_{\text{remote}}$$
  $$\text{Left} = 0, \quad \text{Top} = \frac{H_{\text{view}} - H_{\text{vp}}}{2}$$

Touch mapping:
- If touch coordinate $(X_{\text{touch}}, Y_{\text{touch}})$ is outside $[\text{Left}, \text{Left} + W_{\text{vp}}] \times [\text{Top}, \text{Top} + H_{\text{vp}}]$, the touch is rejected (`null`).
- Otherwise:
  $$X_{\text{remote}} = \left(\frac{X_{\text{touch}} - \text{Left}}{W_{\text{vp}}}\right) \times W_{\text{remote}}$$
  $$Y_{\text{remote}} = \left(\frac{Y_{\text{touch}} - \text{Top}}{H_{\text{vp}}}\right) \times H_{\text{remote}}$$

### Scroll Sensitivity Calibration
- Replaced instantaneous 2px motion scroll triggers with an accumulator model.
- Accumulation step thresholds: $36\text{px}$ (vertical) and $44\text{px}$ (horizontal).
- Sensitivity reduced to ~25–30% of original rate to prevent row jumping and provide smooth, controlled scrolling.

---

## Test Verification Checklist

### Cursor Mode (Baseline)
- [x] Relative cursor movement with finger drag
- [x] Left click (tap < 220ms)
- [x] Double click
- [x] Right click (two-finger tap)
- [x] Drag selection (hold > 350ms and drag)
- [x] Vertical and horizontal scroll (two-finger swipe)
- [x] Existing functionality 100% preserved

### Touch Mode (Direct)
- [x] Single tap -> Left click at exact touch position
- [x] Double tap (< 300ms) -> Double left click at exact touch position
- [x] Long press (> 450ms) -> Right click at exact touch position
- [x] One-finger drag -> Mouse drag (button down + delta motion + button up)
- [x] Two-finger vertical drag -> Scroll up / down
- [x] Two-finger horizontal drag -> Scroll left / right
- [x] Two-finger tap -> Right click at touch midpoint
- [x] Pacing and throttling to ~100Hz max to prevent socket and D-Bus saturation

### Coordinate Mapping
- [x] Android top-left corner -> Remote top-left (0, 0)
- [x] Android top-right corner -> Remote top-right (W_remote, 0)
- [x] Android bottom-left corner -> Remote bottom-left (0, H_remote)
- [x] Android bottom-right corner -> Remote bottom-right (W_remote, H_remote)
- [x] Center -> Remote center (W_remote / 2, H_remote / 2)
- [x] Aspect-ratio mismatch handling (letterboxing & pillarboxing)
- [x] 1920×1080 native remote desktop
- [x] Dynamic resolutions: 1280×720, 2560×1440, 1920×1200 (16:10), 1024×768 (4:3)
- [x] System UI insets rejection (touches in status bar or navigation bar areas do not reach Ubuntu)

### UI & Mode Switching
- [x] WARD starts in Cursor Mode by default
- [x] Mode toggle in Landscape left control bar: `[ 🖱 ]` / `[ 👆 ]`
- [x] Mode toggle in Portrait top control bar: `[ 🖱 Cursor ]` / `[ 👆 Touch ]`
- [x] Floating Mode toggle button on screen: `[ 🖱 ]` / `[ 👆 ]`
- [x] Popover selector card: Option 1 Cursor Mode, Option 2 Touch Mode
- [x] Animated confirmation pill badge on mode switch
- [x] Instant switching during active remote session without reconnection
- [x] No video streaming restart
- [x] No authentication interruption
- [x] Seamless orientation rotation (Portrait ↔ Landscape) with state preservation
