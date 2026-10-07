#!/bin/bash
# WARD Android Client — Build & Deployment Script
# Builds the debug APK and optionally installs it to connected devices via ADB.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
ANDROID_DIR="${ROOT_DIR}/android"
APK_PATH="${ANDROID_DIR}/app/build/outputs/apk/debug/app-debug.apk"

echo "=================================================="
echo "   WARD Android Client — Build & Deploy          "
echo "=================================================="
echo ""

echo "==> Step 1: Building Android APK..."
cd "${ANDROID_DIR}"
./gradlew assembleDebug

if [ ! -f "${APK_PATH}" ]; then
    echo "❌ Error: APK not found at ${APK_PATH}"
    exit 1
fi

VERSIONED_APK="${ANDROID_DIR}/app/build/outputs/apk/debug/ward-v1.2.3.apk"
cp "${APK_PATH}" "${VERSIONED_APK}"

echo ""
echo "✅ Build successful!"
echo "   APK Location: ${APK_PATH}"
echo "   Versioned:    ${VERSIONED_APK}"
echo "   APK Size:     $(du -h "${APK_PATH}" | cut -f1)"
echo ""

# Check for ADB devices
if command -v adb >/dev/null 2>&1; then
    echo "==> Step 2: Checking for connected ADB devices..."
    DEVICES=$(adb devices | grep -v "List of devices" | grep "device$" | awk '{print $1}' || true)
    
    if [ -n "${DEVICES}" ]; then
        echo "Found connected device(s):"
        echo "${DEVICES}"
        for dev in ${DEVICES}; do
            echo ""
            echo "==> Installing to device: ${dev}..."
            adb -s "${dev}" install -r "${APK_PATH}"
            echo "✅ Installed successfully on ${dev}!"
            echo "   Launching WARD on ${dev}..."
            adb -s "${dev}" shell monkey -p dev.ward.client -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1 || true
        done
    else
        echo "ℹ️ No Android device currently connected via ADB."
        echo ""
        echo "Options to install on your phone:"
        echo "  1. Connect your phone via USB with USB Debugging enabled, then re-run:"
        echo "     bash scripts/deploy-android.sh"
        echo ""
        echo "  2. Connect via Wireless ADB (Developer options -> Wireless debugging):"
        echo "     adb connect <PHONE_IP>:<PORT>"
        echo "     bash scripts/deploy-android.sh"
        echo ""
        echo "  3. Transfer the APK file directly to your phone:"
        echo "     ${APK_PATH}"
    fi
else
    echo "ℹ️ ADB not found. Transfer the APK directly to your phone: ${APK_PATH}"
fi

echo ""
echo "=================================================="
