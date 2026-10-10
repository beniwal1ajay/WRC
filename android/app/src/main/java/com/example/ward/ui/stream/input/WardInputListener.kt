package com.example.ward.ui.stream.input

/**
 * Common sink interface for remote desktop input events.
 * Dispatches to WardSessionManager.
 */
interface WardInputListener {
    fun sendMouseMove(dx: Float, dy: Float)
    fun sendMouseMoveAbsolute(x: Float, y: Float)
    fun sendMouseButton(button: Int, state: Int)
    fun sendMouseScroll(dx: Float, dy: Float)
    fun sendKeysym(keysym: Int)
}
