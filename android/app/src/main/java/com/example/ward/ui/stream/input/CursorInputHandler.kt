package com.example.ward.ui.stream.input

import android.view.InputDevice
import android.view.MotionEvent
import android.view.View

private const val KEYSYM_SUPER = 0xFFEB

/**
 * Handles CURSOR MODE input:
 * Operates as a full-screen multi-gesture touchpad.
 * Preserves 100% of the existing working relative cursor behavior.
 */
class CursorInputHandler(
    private val listener: WardInputListener,
    private val cursorTracker: CursorTracker
) {
    // Touch tracking variables
    private var lastX1 = 0f
    private var lastY1 = 0f
    private var lastX2 = 0f
    private var lastY2 = 0f
    private var downX = 0f
    private var downY = 0f
    private var downTime = 0L
    private var pointerCount = 0
    private var isDragging = false
    private var threeFingerSwipeStartY = 0f
    private var accumulatedDx = 0f
    private var accumulatedDy = 0f
    private var lastMoveDispatchTime = 0L
    private var accumulatedScrollY = 0f
    private var accumulatedScrollX = 0f

    fun onGenericMotion(event: MotionEvent): Boolean {
        if (event.isFromSource(InputDevice.SOURCE_MOUSE) || event.isFromSource(InputDevice.SOURCE_MOUSE_RELATIVE)) {
            when (event.action) {
                MotionEvent.ACTION_SCROLL -> {
                    val vScroll = event.getAxisValue(MotionEvent.AXIS_VSCROLL)
                    val hScroll = event.getAxisValue(MotionEvent.AXIS_HSCROLL)
                    if (Math.abs(vScroll) > 0.01f) {
                        val stepY = if (vScroll > 0) -1.0f else 1.0f
                        listener.sendMouseScroll(0f, stepY)
                    }
                    if (Math.abs(hScroll) > 0.01f) {
                        val stepX = if (hScroll > 0) 1.0f else -1.0f
                        listener.sendMouseScroll(stepX, 0f)
                    }
                    return true
                }
                MotionEvent.ACTION_HOVER_MOVE -> {
                    val rx = event.getAxisValue(MotionEvent.AXIS_RELATIVE_X)
                    val ry = event.getAxisValue(MotionEvent.AXIS_RELATIVE_Y)
                    if (rx != 0f || ry != 0f) {
                        listener.sendMouseMove(rx, ry)
                    }
                    return true
                }
            }
        }
        return false
    }

    fun onTouchEvent(
        view: View,
        event: MotionEvent,
        remoteWidth: Int,
        remoteHeight: Int
    ): Boolean {
        pointerCount = event.pointerCount

        // Physical mouse button events
        if (event.isFromSource(InputDevice.SOURCE_MOUSE)) {
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    val buttonState = event.buttonState
                    if ((buttonState and MotionEvent.BUTTON_PRIMARY) != 0) {
                        listener.sendMouseButton(1, 1)
                    }
                    if ((buttonState and MotionEvent.BUTTON_SECONDARY) != 0) {
                        listener.sendMouseButton(2, 1)
                    }
                    if ((buttonState and MotionEvent.BUTTON_TERTIARY) != 0) {
                        listener.sendMouseButton(3, 1)
                    }
                    return true
                }
                MotionEvent.ACTION_UP -> {
                    listener.sendMouseButton(1, 0)
                    listener.sendMouseButton(2, 0)
                    listener.sendMouseButton(3, 0)
                    return true
                }
                MotionEvent.ACTION_MOVE -> {
                    val rx = event.getAxisValue(MotionEvent.AXIS_RELATIVE_X)
                    val ry = event.getAxisValue(MotionEvent.AXIS_RELATIVE_Y)
                    if (rx != 0f || ry != 0f) {
                        listener.sendMouseMove(rx, ry)
                        cursorTracker.updateRelative(rx, ry, remoteWidth, remoteHeight)
                    }
                    return true
                }
            }
        }

        // Virtual touchpad gesture handling
        return when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                downX = event.x
                downY = event.y
                lastX1 = event.x
                lastY1 = event.y
                accumulatedDx = 0f
                accumulatedDy = 0f
                lastMoveDispatchTime = System.currentTimeMillis()
                downTime = System.currentTimeMillis()
                isDragging = false
                view.performClick()
                true
            }

            MotionEvent.ACTION_POINTER_DOWN -> {
                if (pointerCount == 2) {
                    lastX1 = event.getX(0)
                    lastY1 = event.getY(0)
                    lastX2 = event.getX(1)
                    lastY2 = event.getY(1)
                    accumulatedScrollY = 0f
                    accumulatedScrollX = 0f
                } else if (pointerCount == 3) {
                    threeFingerSwipeStartY = event.getY(0)
                }
                true
            }

            MotionEvent.ACTION_MOVE -> {
                val now = System.currentTimeMillis()

                if (pointerCount == 1) {
                    val dx = event.x - lastX1
                    val dy = event.y - lastY1
                    lastX1 = event.x
                    lastY1 = event.y

                    accumulatedDx += dx
                    accumulatedDy += dy

                    val totalDist = Math.hypot((event.x - downX).toDouble(), (event.y - downY).toDouble())

                    // Enter drag mode if held down for > 350ms
                    if (!isDragging && now - downTime > 350 && totalDist < 25) {
                        isDragging = true
                        listener.sendMouseButton(1, 1)
                    }

                    // Pace pointer updates to ~100Hz max to avoid D-Bus saturation
                    if (now - lastMoveDispatchTime >= 10L) {
                        if (accumulatedDx != 0f || accumulatedDy != 0f) {
                            listener.sendMouseMove(accumulatedDx, accumulatedDy)
                            cursorTracker.updateRelative(accumulatedDx, accumulatedDy, remoteWidth, remoteHeight)
                            accumulatedDx = 0f
                            accumulatedDy = 0f
                        }
                        lastMoveDispatchTime = now
                    }
                } else if (pointerCount == 2) {
                    // 2-finger scroll
                    val currentY1 = event.getY(0)
                    val currentY2 = event.getY(1)
                    val currentX1 = event.getX(0)
                    val currentX2 = event.getX(1)
                    val dy = ((currentY1 - lastY1) + (currentY2 - lastY2)) / 2f
                    val dx = ((currentX1 - lastX1) + (currentX2 - lastX2)) / 2f

                    accumulatedScrollY += dy
                    accumulatedScrollX += dx

                    val scrollStepY = 36f
                    while (Math.abs(accumulatedScrollY) >= scrollStepY) {
                        val stepY = if (accumulatedScrollY > 0) 1.0f else -1.0f
                        listener.sendMouseScroll(0f, stepY)
                        accumulatedScrollY -= stepY * scrollStepY
                    }

                    val scrollStepX = 44f
                    while (Math.abs(accumulatedScrollX) >= scrollStepX) {
                        val stepX = if (accumulatedScrollX > 0) 1.0f else -1.0f
                        listener.sendMouseScroll(stepX, 0f)
                        accumulatedScrollX -= stepX * scrollStepX
                    }

                    lastX1 = currentX1
                    lastY1 = currentY1
                    lastX2 = currentX2
                    lastY2 = currentY2
                } else if (pointerCount == 3) {
                    // 3-finger swipe up for Super / Windows key
                    val currentY = event.getY(0)
                    if (threeFingerSwipeStartY - currentY > 120f) {
                        listener.sendKeysym(KEYSYM_SUPER)
                        threeFingerSwipeStartY = currentY
                    }
                }
                true
            }

            MotionEvent.ACTION_POINTER_UP -> {
                val duration = System.currentTimeMillis() - downTime
                if (pointerCount == 2 && duration < 250) {
                    listener.sendMouseButton(2, 1)
                    listener.sendMouseButton(2, 0)
                } else if (pointerCount == 3 && duration < 250) {
                    listener.sendMouseButton(3, 1)
                    listener.sendMouseButton(3, 0)
                }
                true
            }

            MotionEvent.ACTION_UP -> {
                if (accumulatedDx != 0f || accumulatedDy != 0f) {
                    listener.sendMouseMove(accumulatedDx, accumulatedDy)
                    cursorTracker.updateRelative(accumulatedDx, accumulatedDy, remoteWidth, remoteHeight)
                    accumulatedDx = 0f
                    accumulatedDy = 0f
                }

                val duration = System.currentTimeMillis() - downTime
                val dist = Math.hypot((event.x - downX).toDouble(), (event.y - downY).toDouble())

                if (isDragging) {
                    listener.sendMouseButton(1, 0)
                    isDragging = false
                } else if (duration < 220 && dist < 20) {
                    listener.sendMouseButton(1, 1)
                    listener.sendMouseButton(1, 0)
                }
                true
            }

            else -> false
        }
    }
}
