package com.example.ward.ui.stream.input

import android.view.MotionEvent
import android.view.View
import com.example.ward.ui.stream.InputMode

/**
 * Unified Input Mapper coordinating CURSOR MODE and TOUCH MODE.
 */
class WardInputMapper(
    listener: WardInputListener,
    val cursorTracker: CursorTracker = CursorTracker()
) {
    var mode: InputMode = InputMode.CURSOR
        private set

    private val cursorHandler = CursorInputHandler(listener, cursorTracker)
    val touchHandler = TouchInputHandler(listener, cursorTracker)

    fun setMode(newMode: InputMode) {
        mode = newMode
    }

    fun onGenericMotion(event: MotionEvent): Boolean {
        return when (mode) {
            InputMode.CURSOR -> cursorHandler.onGenericMotion(event)
            InputMode.TOUCH -> touchHandler.onGenericMotion(event)
        }
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
        return when (mode) {
            InputMode.CURSOR -> cursorHandler.onTouchEvent(
                view = view,
                event = event,
                remoteWidth = remoteWidth,
                remoteHeight = remoteHeight
            )
            InputMode.TOUCH -> touchHandler.onTouchEvent(
                view = view,
                event = event,
                viewWidth = viewWidth,
                viewHeight = viewHeight,
                remoteWidth = remoteWidth,
                remoteHeight = remoteHeight,
                insetsLeft = insetsLeft,
                insetsTop = insetsTop,
                insetsRight = insetsRight,
                insetsBottom = insetsBottom
            )
        }
    }
}
