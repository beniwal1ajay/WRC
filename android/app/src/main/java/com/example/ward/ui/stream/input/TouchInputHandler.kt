package com.example.ward.ui.stream.input

import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import com.example.ward.ui.stream.CoordinateMapper
import com.example.ward.ui.stream.RemotePoint
import com.example.ward.ui.stream.ViewportRect

/**
 * Handles TOUCH MODE input:
 * Provides a Chrome Remote Desktop-like direct-touch experience.
 *
 * Touch coordinates map directly to remote desktop coordinates via CoordinateMapper.
 */
class TouchInputHandler(
    private val listener: WardInputListener,
    private val cursorTracker: CursorTracker
) {
    // 1-finger tracking
    private var downX = 0f
    private var downY = 0f
    private var downTime = 0L
    private var downRemotePoint: RemotePoint? = null
    private var lastDragRemotePoint: RemotePoint? = null
    private var isDragging = false
    private var isLongPressTriggered = false
    private var isDoubleTapHandled = false
    private var isOutsideViewport = false

    // Double tap detection
    private var lastTapTime = 0L
    private var lastTapX = 0f
    private var lastTapY = 0f

    // Motion throttling (~100Hz max to avoid D-Bus saturation)
    private var accumulatedDx = 0f
    private var accumulatedDy = 0f
    private var lastMoveDispatchTime = 0L

    // 2-finger scroll & tap tracking
    private var twoFingerDownTime = 0L
    private var twoFingerDownX1 = 0f
    private var twoFingerDownY1 = 0f
    private var twoFingerDownX2 = 0f
    private var twoFingerDownY2 = 0f
    private var lastTwoFingerX1 = 0f
    private var lastTwoFingerY1 = 0f
    private var lastTwoFingerX2 = 0f
    private var lastTwoFingerY2 = 0f

    // Scroll accumulation (lowered to ~25-30% sensitivity)
    private var accumulatedScrollY = 0f
    private var accumulatedScrollX = 0f
    private val scrollStepY = 36f
    private val scrollStepX = 44f

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

    private fun resolveRemotePoint(
        touchX: Float,
        touchY: Float,
        viewWidth: Float,
        viewHeight: Float,
        remoteWidth: Int,
        remoteHeight: Int
    ): RemotePoint? {
        val viewport = CoordinateMapper.getCenteredViewport(
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        ) ?: return null

        return CoordinateMapper.mapTouchToRemoteWithViewport(
            touchX = touchX,
            touchY = touchY,
            viewport = viewport,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
    }

    fun onTouchEvent(
        view: View,
        event: MotionEvent,
        viewWidth: Float,
        viewHeight: Float,
        remoteWidth: Int,
        remoteHeight: Int,
        insetsLeft: Float = 0f,
        insetsTop: Float = 0f,
        insetsRight: Float = 0f,
        insetsBottom: Float = 0f
    ): Boolean {
        val pointerCount = event.pointerCount

        // 1. Physical mouse button & move events
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

        // 2. Direct touch gesture interpretation
        return when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                val rawX = event.x
                val rawY = event.y

                val mapped = resolveRemotePoint(
                    touchX = rawX,
                    touchY = rawY,
                    viewWidth = viewWidth,
                    viewHeight = viewHeight,
                    remoteWidth = remoteWidth,
                    remoteHeight = remoteHeight
                )

                if (mapped == null) {
                    // Touch is outside remote desktop viewport
                    isOutsideViewport = true
                    return true
                }

                isOutsideViewport = false
                downX = rawX
                downY = rawY
                downTime = System.currentTimeMillis()
                downRemotePoint = mapped
                lastDragRemotePoint = mapped
                isDragging = false
                isLongPressTriggered = false
                accumulatedDx = 0f
                accumulatedDy = 0f
                lastMoveDispatchTime = downTime
                view.performClick()

                // Check for Double Tap (< 300ms and < 35px from previous tap)
                val timeSinceLast = downTime - lastTapTime
                val distFromLast = Math.hypot((rawX - lastTapX).toDouble(), (rawY - lastTapY).toDouble())
                if (timeSinceLast in 30..300 && distFromLast < 35.0) {
                    isDoubleTapHandled = true
                    lastTapTime = 0L // Reset

                    // Absolute cursor positioning to tapped remote coordinate
                    listener.sendMouseMoveAbsolute(mapped.x, mapped.y)
                    cursorTracker.setAbsolute(mapped.x, mapped.y, remoteWidth, remoteHeight)

                    // Dispatch Double Click
                    listener.sendMouseButton(1, 1)
                    listener.sendMouseButton(1, 0)
                    listener.sendMouseButton(1, 1)
                    listener.sendMouseButton(1, 0)
                } else {
                    isDoubleTapHandled = false
                }
                return true
            }

            MotionEvent.ACTION_POINTER_DOWN -> {
                if (pointerCount == 2) {
                    twoFingerDownTime = System.currentTimeMillis()
                    twoFingerDownX1 = event.getX(0)
                    twoFingerDownY1 = event.getY(0)
                    twoFingerDownX2 = event.getX(1)
                    twoFingerDownY2 = event.getY(1)
                    lastTwoFingerX1 = twoFingerDownX1
                    lastTwoFingerY1 = twoFingerDownY1
                    lastTwoFingerX2 = twoFingerDownX2
                    lastTwoFingerY2 = twoFingerDownY2
                    accumulatedScrollY = 0f
                    accumulatedScrollX = 0f
                }
                return true
            }

            MotionEvent.ACTION_MOVE -> {
                if (isOutsideViewport) return true
                val now = System.currentTimeMillis()

                if (pointerCount == 1) {
                    if (isDoubleTapHandled) return true

                    val rawX = event.x
                    val rawY = event.y
                    val distFromDown = Math.hypot((rawX - downX).toDouble(), (rawY - downY).toDouble())

                    // Long press check (> 450ms and finger stayed still < 20px)
                    if (!isDragging && !isLongPressTriggered) {
                        if (now - downTime >= 450L && distFromDown < 20.0) {
                            isLongPressTriggered = true
                            downRemotePoint?.let { pt ->
                                listener.sendMouseMoveAbsolute(pt.x, pt.y)
                                cursorTracker.setAbsolute(pt.x, pt.y, remoteWidth, remoteHeight)
                                // Dispatch Right Click
                                listener.sendMouseButton(2, 1)
                                listener.sendMouseButton(2, 0)
                            }
                            return true
                        }
                    }

                    // Drag detection (finger moved > 22px without long press)
                    if (!isDragging && !isLongPressTriggered && distFromDown >= 22.0) {
                        isDragging = true
                        downRemotePoint?.let { pt ->
                            listener.sendMouseMoveAbsolute(pt.x, pt.y)
                            cursorTracker.setAbsolute(pt.x, pt.y, remoteWidth, remoteHeight)
                            // Mouse down to begin dragging selection/window
                            listener.sendMouseButton(1, 1)
                        }
                    }

                    // Active Dragging: stream motion deltas
                    if (isDragging) {
                        val currentRemote = resolveRemotePoint(
                            touchX = rawX,
                            touchY = rawY,
                            viewWidth = viewWidth,
                            viewHeight = viewHeight,
                            remoteWidth = remoteWidth,
                            remoteHeight = remoteHeight
                        )

                        if (currentRemote != null && lastDragRemotePoint != null) {
                            lastDragRemotePoint = currentRemote

                            // Throttled ~100Hz dispatch
                            if (now - lastMoveDispatchTime >= 10L) {
                                listener.sendMouseMoveAbsolute(currentRemote.x, currentRemote.y)
                                cursorTracker.setAbsolute(currentRemote.x, currentRemote.y, remoteWidth, remoteHeight)
                                lastMoveDispatchTime = now
                            }
                        }
                    }
                } else if (pointerCount == 2) {
                    // Two-finger scroll with accumulator (decreased sensitivity to ~25-30%)
                    val currentY1 = event.getY(0)
                    val currentY2 = event.getY(1)
                    val currentX1 = event.getX(0)
                    val currentX2 = event.getX(1)

                    val dy = ((currentY1 - lastTwoFingerY1) + (currentY2 - lastTwoFingerY2)) / 2f
                    val dx = ((currentX1 - lastTwoFingerX1) + (currentX2 - lastTwoFingerX2)) / 2f

                    accumulatedScrollY += dy
                    accumulatedScrollX += dx

                    while (Math.abs(accumulatedScrollY) >= scrollStepY) {
                        val stepY = if (accumulatedScrollY > 0) 1.0f else -1.0f
                        listener.sendMouseScroll(0f, stepY)
                        accumulatedScrollY -= stepY * scrollStepY
                    }

                    while (Math.abs(accumulatedScrollX) >= scrollStepX) {
                        val stepX = if (accumulatedScrollX > 0) 1.0f else -1.0f
                        listener.sendMouseScroll(stepX, 0f)
                        accumulatedScrollX -= stepX * scrollStepX
                    }

                    lastTwoFingerX1 = currentX1
                    lastTwoFingerY1 = currentY1
                    lastTwoFingerX2 = currentX2
                    lastTwoFingerY2 = currentY2
                }
                return true
            }

            MotionEvent.ACTION_POINTER_UP -> {
                if (pointerCount == 2) {
                    val duration = System.currentTimeMillis() - twoFingerDownTime
                    val dist1 = Math.hypot((event.getX(0) - twoFingerDownX1).toDouble(), (event.getY(0) - twoFingerDownY1).toDouble())
                    val dist2 = Math.hypot((event.getX(1) - twoFingerDownX2).toDouble(), (event.getY(1) - twoFingerDownY2).toDouble())

                    // Two-finger tap (< 250ms and < 25px motion) -> Right click
                    if (duration < 250 && dist1 < 25.0 && dist2 < 25.0) {
                        val midX = (event.getX(0) + event.getX(1)) / 2f
                        val midY = (event.getY(0) + event.getY(1)) / 2f

                        val mappedMid = resolveRemotePoint(
                            touchX = midX,
                            touchY = midY,
                            viewWidth = viewWidth,
                            viewHeight = viewHeight,
                            remoteWidth = remoteWidth,
                            remoteHeight = remoteHeight
                        )

                        if (mappedMid != null) {
                            listener.sendMouseMoveAbsolute(mappedMid.x, mappedMid.y)
                            cursorTracker.setAbsolute(mappedMid.x, mappedMid.y, remoteWidth, remoteHeight)
                            listener.sendMouseButton(2, 1)
                            listener.sendMouseButton(2, 0)
                        }
                    }
                    accumulatedScrollY = 0f
                    accumulatedScrollX = 0f
                }
                return true
            }

            MotionEvent.ACTION_UP -> {
                if (isOutsideViewport) {
                    isOutsideViewport = false
                    return true
                }

                if (isDragging) {
                    lastDragRemotePoint?.let { pt ->
                        listener.sendMouseMoveAbsolute(pt.x, pt.y)
                        cursorTracker.setAbsolute(pt.x, pt.y, remoteWidth, remoteHeight)
                    }
                    // Release mouse drag
                    listener.sendMouseButton(1, 0)
                    isDragging = false
                } else if (!isDoubleTapHandled && !isLongPressTriggered) {
                    val rawX = event.x
                    val rawY = event.y
                    val duration = System.currentTimeMillis() - downTime
                    val dist = Math.hypot((rawX - downX).toDouble(), (rawY - downY).toDouble())

                    // Single Tap (< 250ms and < 22px motion)
                    if (duration < 250L && dist < 22.0) {
                        downRemotePoint?.let { pt ->
                            // Absolute cursor positioning to tapped coordinate
                            listener.sendMouseMoveAbsolute(pt.x, pt.y)
                            cursorTracker.setAbsolute(pt.x, pt.y, remoteWidth, remoteHeight)

                            // Single left click
                            listener.sendMouseButton(1, 1)
                            listener.sendMouseButton(1, 0)

                            lastTapTime = System.currentTimeMillis()
                            lastTapX = rawX
                            lastTapY = rawY
                        }
                    }
                }

                isDoubleTapHandled = false
                isLongPressTriggered = false
                accumulatedScrollY = 0f
                accumulatedScrollX = 0f
                return true
            }

            MotionEvent.ACTION_CANCEL -> {
                if (isDragging) {
                    listener.sendMouseButton(1, 0)
                    isDragging = false
                }
                isDoubleTapHandled = false
                isLongPressTriggered = false
                accumulatedScrollY = 0f
                accumulatedScrollX = 0f
                return true
            }

            else -> false
        }
    }
}
