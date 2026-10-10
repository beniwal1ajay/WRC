package com.example.ward.ui.stream.input

/**
 * Tracks the virtual position of the remote desktop cursor to allow
 * accurate coordinate translation and seamless transitions between
 * relative Cursor Mode and direct Touch Mode.
 */
class CursorTracker {
    var virtualX: Float = 0f
        private set
    var virtualY: Float = 0f
        private set

    var isInitialized: Boolean = false
        private set

    fun ensureInitialized(remoteWidth: Int, remoteHeight: Int) {
        if (!isInitialized && remoteWidth > 0 && remoteHeight > 0) {
            virtualX = remoteWidth / 2f
            virtualY = remoteHeight / 2f
            isInitialized = true
        }
    }

    fun updateRelative(dx: Float, dy: Float, remoteWidth: Int, remoteHeight: Int) {
        ensureInitialized(remoteWidth, remoteHeight)
        if (remoteWidth > 0 && remoteHeight > 0) {
            virtualX = (virtualX + dx).coerceIn(0f, remoteWidth.toFloat())
            virtualY = (virtualY + dy).coerceIn(0f, remoteHeight.toFloat())
        }
    }

    fun setAbsolute(x: Float, y: Float, remoteWidth: Int, remoteHeight: Int) {
        ensureInitialized(remoteWidth, remoteHeight)
        if (remoteWidth > 0 && remoteHeight > 0) {
            virtualX = x.coerceIn(0f, remoteWidth.toFloat())
            virtualY = y.coerceIn(0f, remoteHeight.toFloat())
        }
    }

    fun reset() {
        isInitialized = false
        virtualX = 0f
        virtualY = 0f
    }
}
