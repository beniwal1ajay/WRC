#!/bin/bash
# WARD — Install development dependencies
# Run this script with: bash scripts/install-deps.sh

set -euo pipefail

echo "=== WARD Development Dependencies Installer ==="
echo ""
echo "This will install the following packages:"
echo "  - build-essential (gcc, make, etc.)"
echo "  - pkg-config"
echo "  - libpipewire-0.3-dev (PipeWire development headers)"
echo "  - libspa-0.2-dev (SPA plugin development headers)"
echo "  - libdbus-1-dev (D-Bus development headers)"
echo "  - libssl-dev (OpenSSL development headers)"
echo "  - libclang-dev (Clang development headers for bindgen)"
echo "  - clang (C/C++ compiler for bindgen)"
echo "  - vainfo (VA-API info tool)"
echo "  - libva-dev (VA-API development headers)"
echo "  - libgstreamer1.0-dev (GStreamer development headers)"
echo "  - libgstreamer-plugins-base1.0-dev (GStreamer plugins dev)"
echo ""

sudo apt-get update
sudo apt-get install -y \
    build-essential \
    pkg-config \
    libpipewire-0.3-dev \
    libspa-0.2-dev \
    libdbus-1-dev \
    libssl-dev \
    libclang-dev \
    clang \
    vainfo \
    libva-dev \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev

echo ""
echo "=== Verifying installation ==="

echo -n "pkg-config: "
pkg-config --version

echo -n "libpipewire-0.3: "
pkg-config --modversion libpipewire-0.3 2>/dev/null || echo "NOT FOUND"

echo -n "dbus-1: "
pkg-config --modversion dbus-1 2>/dev/null || echo "NOT FOUND"

echo -n "openssl: "
pkg-config --modversion openssl 2>/dev/null || echo "NOT FOUND"

echo -n "libva: "
pkg-config --modversion libva 2>/dev/null || echo "NOT FOUND"

echo ""
echo "=== VA-API capabilities ==="
vainfo 2>&1 | head -20 || echo "vainfo failed (may need reboot)"

echo ""
echo "=== Dependencies installed successfully! ==="
echo "You can now build the server: cd server && cargo build"
