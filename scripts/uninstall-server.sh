#!/bin/bash
# WARD Server — Uninstallation Script
# Stops, disables, and removes ward.service and the ward-server binary.

set -euo pipefail

SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"
BIN_INSTALL_DIR="${HOME}/.local/bin"

echo "=================================================="
echo "   WARD Server — Uninstaller                     "
echo "=================================================="
echo ""

echo "==> Stopping and disabling WARD user services..."
systemctl --user stop ward-host.service 2>/dev/null || true
systemctl --user disable ward-host.service 2>/dev/null || true
systemctl --user stop ward.service 2>/dev/null || true
systemctl --user disable ward.service 2>/dev/null || true

if [ -f "${SYSTEMD_USER_DIR}/ward-host.service" ]; then
    echo "==> Removing ${SYSTEMD_USER_DIR}/ward-host.service..."
    rm -f "${SYSTEMD_USER_DIR}/ward-host.service"
fi

if [ -f "${SYSTEMD_USER_DIR}/ward.service" ]; then
    echo "==> Removing ${SYSTEMD_USER_DIR}/ward.service..."
    rm -f "${SYSTEMD_USER_DIR}/ward.service"
fi
systemctl --user daemon-reload

if [ -f "${BIN_INSTALL_DIR}/ward-server" ]; then
    echo "==> Removing binary ${BIN_INSTALL_DIR}/ward-server..."
    rm -f "${BIN_INSTALL_DIR}/ward-server"
fi

echo ""
echo "✅ WARD Server has been completely uninstalled."
echo "Note: Configuration & paired client keys in ~/.config/ward were preserved."
echo "      To remove them permanently: rm -rf ~/.config/ward"
echo ""
