#!/bin/bash
# WARD Server — Production Installation Script
# Installs ward-server binary and configures systemd user service.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
SERVER_DIR="${ROOT_DIR}/server"
SYSTEMD_DIR="${ROOT_DIR}/systemd"
BIN_INSTALL_DIR="${HOME}/.local/bin"
SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"

echo "=================================================="
echo "   WARD Server — Production Installer            "
echo "=================================================="
echo ""

# 1. Check Wayland and desktop session
if [ "${XDG_SESSION_TYPE:-}" != "wayland" ]; then
    echo "⚠️ Warning: Current session type is '${XDG_SESSION_TYPE:-unknown}', expected 'wayland'."
    echo "   Screen capture requires a Wayland desktop session."
fi

# 2. Build release binary
echo "==> Step 1: Compiling optimized release binary..."
cd "${SERVER_DIR}"
cargo build --release

# 3. Install binary to ~/.local/bin
echo ""
echo "==> Step 2: Installing binary to ${BIN_INSTALL_DIR}..."
mkdir -p "${BIN_INSTALL_DIR}"
install -m 0755 "${SERVER_DIR}/target/release/ward-server" "${BIN_INSTALL_DIR}/ward-server"
echo "✅ Installed: ${BIN_INSTALL_DIR}/ward-server"

# Ensure ~/.local/bin is in PATH
if [[ ":$PATH:" != *":${BIN_INSTALL_DIR}:"* ]]; then
    echo "⚠️ Note: ${BIN_INSTALL_DIR} is not in your current PATH."
    echo "   Consider adding 'export PATH=\"\$HOME/.local/bin:\$PATH\"' to your ~/.bashrc or ~/.zshrc."
fi

# 4. Install systemd user unit
echo ""
echo "==> Step 3: Installing systemd user service..."
mkdir -p "${SYSTEMD_USER_DIR}"
cp "${SYSTEMD_DIR}/ward.service" "${SYSTEMD_USER_DIR}/ward.service"
echo "✅ Installed: ${SYSTEMD_USER_DIR}/ward.service"

# Reload systemd user daemon
echo ""
echo "==> Step 4: Reloading systemd user daemon..."
systemctl --user daemon-reload

# 5. Enable and start service
echo ""
echo "==> Step 5: Enabling WARD service to start on desktop login..."
systemctl --user enable ward.service

echo ""
echo "=================================================="
echo "🎉 WARD Server successfully installed!"
echo "=================================================="
echo ""
echo "Useful Commands:"
echo "  • Start service now:     systemctl --user start ward.service"
echo "  • Check service status:  systemctl --user status ward.service"
echo "  • View live server logs: journalctl --user -u ward.service -f"
echo "  • Inspect identity:      ward-server status"
echo "  • Stop service:          systemctl --user stop ward.service"
echo "  • Uninstall:             bash scripts/uninstall-server.sh"
echo ""
