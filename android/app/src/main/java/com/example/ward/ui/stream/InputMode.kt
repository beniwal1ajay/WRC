package com.example.ward.ui.stream

/**
 * WARD Dual Input Modes:
 * - CURSOR: Relative trackpad mode (default). Moving finger moves remote cursor relatively.
 * - TOUCH: Direct touch mode (Chrome Remote Desktop-like). Touching the screen maps
 *   directly to the corresponding coordinates on the remote Ubuntu desktop.
 */
enum class InputMode(
    val displayName: String,
    val icon: String,
    val shortLabel: String
) {
    CURSOR("Cursor Mode", "🖱", "Cursor"),
    TOUCH("Touch Mode", "👆", "Touch")
}
