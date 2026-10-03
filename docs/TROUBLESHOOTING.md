# WARD — Troubleshooting Guide

---

## 1. Quick Diagnostic Commands

### Check Wayland Session
```bash
echo "Session Type: $XDG_SESSION_TYPE"       # Expected: wayland
echo "Desktop:      $XDG_CURRENT_DESKTOP"   # Expected: ubuntu:GNOME
echo "Display:      $WAYLAND_DISPLAY"       # Expected: wayland-0
```

### Check GNOME & Mutter Version
```bash
gnome-shell --version
# Expected: GNOME Shell 46.x or 50.x
```

### Check PipeWire Services
```bash
systemctl --user status pipewire
systemctl --user status pipewire-pulse
systemctl --user status wireplumber
# All three should be: active (running)
```

### Check WARD Host Daemon
```bash
# Check service status
systemctl --user status ward-host.service

# View live streaming logs
journalctl --user -u ward-host.service -f

# Inspect host configuration and endpoints
ward-server status
```

### Check Network Port Accessibility
```bash
# Verify port 9740 is bound and listening
ss -tlnp | grep 9740
# Expected: LISTEN 0 128 0.0.0.0:9740
```

---

## 2. Common Issues & Solutions

### Issue: "Authentication failed: invalid host PIN (attempt X/5)"
- **Cause:** The PIN entered on the Android device does not match the host's configured permanent PIN.
- **Solution:** Verify or reset your permanent PIN on the Ubuntu host:
  ```bash
  ward-server set-pin 123456
  ```
  Note: After 5 consecutive failed attempts, the host enforces a 60-second administrative lockout to prevent brute-force attacks. Wait 60 seconds before retrying.

### Issue: "Session rejected: Host temporarily disabled"
- **Cause:** WARD was administratively disabled via `ward-server disable`.
- **Solution:** Re-enable remote access on the host:
  ```bash
  ward-server enable
  ```

### Issue: Screen Sharing Dialog Appeared in Older Versions
- **Cause:** Older versions invoked `xdg-desktop-portal` directly, prompting the user with *"Share your screen?"*.
- **Solution:** WARD's unattended host architecture connects directly to GNOME Mutter's native ScreenCast API (`org.gnome.Mutter.ScreenCast`). Ensure `ward-host.service` is running rather than old scripts:
  ```bash
  systemctl --user restart ward-host.service
  ```

### Issue: Mouse Cursor Not Visible on Android
- **Cause:** ScreenCast stream configured with `Metadata` or `Hidden` cursor mode rather than `Embedded`.
- **Solution:** WARD v1.0 sets `MetaScreenCastCursorMode = 1` (`Embedded`), causing the compositor to burn the cursor directly into the video stream framebuffers. If running an older build, update to the `vpn-stable` release.

### Issue: Android Client Can't Connect (Timeout or Connection Refused)
1. **Firewall:** If `ufw` is active on Ubuntu, allow inbound TCP on port 9740:
   ```bash
   sudo ufw allow 9740/tcp
   sudo ufw reload
   ```
2. **Network Route:** Ensure both devices are on the same Wi-Fi network, or both are connected to the same Tailscale / WireGuard VPN mesh.
3. **Verify IP:** Run `ward-server status` to verify the IP address of your host.

### Issue: Video Lag or Dropped Frames
1. **Network Congestion:** WARD includes an adaptive rate controller that dynamically scales encoding quality based on network throughput.
2. **Hardware Acceleration:** Ensure your CPU/GPU supports hardware-accelerated video decoding on Android and fast encoding on Ubuntu.
3. **TCP NoDelay:** WARD enables `TCP_NODELAY` on both client and host sockets to minimize latency.

### Issue: `XDG_SESSION_TYPE` is not `wayland`
- **Cause:** You are logged into an X11 / Xorg desktop session.
- **Solution:** Log out of Ubuntu, click the gear icon on the login screen, select **"Ubuntu on Wayland"**, and log back in. WARD is designed specifically for Wayland and does not use X11.

---

## 3. Log Levels & Verbose Debugging

Set the log level via the `RUST_LOG` environment variable when running diagnostics:

```bash
# Debugging
RUST_LOG=debug ward-server host

# Trace PipeWire frame delivery and D-Bus calls
RUST_LOG=ward_server::portal=trace,ward_server::capture=trace ward-server host
```

---

## 4. Service Recovery & Clean Restart

If an unexpected state occurs, restart the desktop media stack in sequence:

```bash
# 1. Restart PipeWire & WirePlumber
systemctl --user restart pipewire wireplumber

# 2. Restart WARD Host Daemon
systemctl --user restart ward-host.service

# 3. Verify status
systemctl --user status ward-host.service
```
