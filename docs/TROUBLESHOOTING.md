# WARD — Troubleshooting

---

## Diagnostic Commands

### Check Wayland Session

```bash
echo $XDG_SESSION_TYPE
# Expected: wayland

echo $XDG_CURRENT_DESKTOP
# Expected: ubuntu:GNOME

echo $WAYLAND_DISPLAY
# Expected: wayland-0
```

### Check GNOME Version

```bash
gnome-shell --version
# Expected: GNOME Shell 46.x
```

### Check PipeWire

```bash
systemctl --user status pipewire
# Expected: active (running)

systemctl --user status pipewire-pulse
# Expected: active (running)

systemctl --user status wireplumber
# Expected: active (running)

pipewire --version
# Expected: Compiled with libpipewire 1.x.x
```

### Check XDG Desktop Portal

```bash
systemctl --user status xdg-desktop-portal
# Expected: active (running)

systemctl --user status xdg-desktop-portal-gnome
# Expected: active (running)
```

### Check Portal Interfaces

```bash
# ScreenCast portal available?
busctl --user introspect org.freedesktop.portal.Desktop \
    /org/freedesktop/portal/desktop \
    org.freedesktop.portal.ScreenCast

# RemoteDesktop portal available?
busctl --user introspect org.freedesktop.portal.Desktop \
    /org/freedesktop/portal/desktop \
    org.freedesktop.portal.RemoteDesktop
```

### Check GNOME Remote Desktop

```bash
systemctl --user status gnome-remote-desktop
# Note: WARD does not require this service to be running

dpkg -l gnome-remote-desktop
```

### Check VA-API (Hardware Video Encoding)

```bash
vainfo
# Look for: H264 encode

# If vainfo is not installed:
sudo apt install vainfo
```

### Check Network

```bash
ip addr show
# Look for your LAN IP

# Test if WARD port is accessible:
ss -tlnp | grep 9740
```

### Check WARD Service

```bash
systemctl --user status ward
journalctl --user -u ward -f
```

---

## Common Issues

### "Portal response timed out"

**Cause:** The screen sharing permission dialog wasn't answered within 120 seconds.

**Fix:** When WARD starts, look for the GNOME permission dialog asking to share your screen. Click "Share" (or select the monitor and click "Share").

### "No capture streams returned"

**Cause:** The portal didn't provide any PipeWire streams.

**Fix:**
1. Make sure `xdg-desktop-portal-gnome` is running.
2. Restart the portal: `systemctl --user restart xdg-desktop-portal xdg-desktop-portal-gnome`
3. Try again.

### "Failed to connect to PipeWire"

**Cause:** PipeWire service not running or portal fd invalid.

**Fix:**
1. Check PipeWire: `systemctl --user status pipewire`
2. Restart if needed: `systemctl --user restart pipewire wireplumber`
3. Try again.

### "No data pointer (likely DMA-BUF)"

**Cause:** PipeWire is sending DMA-BUF (GPU memory) frames instead of SHM (shared memory).

**Context:** This is common and actually preferred for performance. WARD will add DMA-BUF support.

**Workaround:** Set `PIPEWIRE_DEBUG=3` environment variable for more info.

### "XDG_SESSION_TYPE is not wayland"

**Cause:** You're running under X11 or another session type.

**Fix:**
1. Log out and select "Ubuntu on Wayland" at the login screen.
2. Do NOT switch to X11 as a fix for WARD. WARD is designed for Wayland.

### "D-Bus connection failed"

**Cause:** The D-Bus session bus is not available.

**Fix:**
1. Make sure `DBUS_SESSION_BUS_ADDRESS` is set: `echo $DBUS_SESSION_BUS_ADDRESS`
2. If running via SSH, you may need to set this manually or use `dbus-run-session`.

### Android Client Can't Connect

1. Ensure both devices are on the same LAN/Wi-Fi.
2. Check the server IP: `ip addr show wlp1s0` (or your interface).
3. Check WARD is listening: `ss -tlnp | grep 9740`.
4. Check firewall: `sudo ufw status`. If active, allow port 9740: `sudo ufw allow 9740/tcp`.
5. Try pinging the server from another device on the LAN.

---

## Log Levels

Set the log level via the `RUST_LOG` environment variable:

```bash
# Normal operation
RUST_LOG=info ward-server

# Debugging
RUST_LOG=debug ward-server

# Verbose debugging (includes PipeWire frame details)
RUST_LOG=trace ward-server

# Specific module debugging
RUST_LOG=ward_server::portal=debug,ward_server::capture=trace ward-server
```

---

## Restarting Services

If things are broken, try restarting in this order:

```bash
# 1. PipeWire
systemctl --user restart pipewire wireplumber

# 2. XDG Desktop Portal
systemctl --user restart xdg-desktop-portal xdg-desktop-portal-gnome

# 3. WARD
systemctl --user restart ward

# 4. If all else fails, log out and back in
```

Do **NOT** restart GDM or switch to X11 as a troubleshooting step.
