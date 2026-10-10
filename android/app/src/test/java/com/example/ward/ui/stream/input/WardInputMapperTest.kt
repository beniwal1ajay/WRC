package com.example.ward.ui.stream.input

import com.example.ward.ui.stream.InputMode
import org.junit.Assert.*
import org.junit.Test

class WardInputMapperTest {

    private class TestInputListener : WardInputListener {
        val mouseMoves = mutableListOf<Pair<Float, Float>>()
        val mouseMovesAbsolute = mutableListOf<Pair<Float, Float>>()
        val mouseButtons = mutableListOf<Pair<Int, Int>>()
        val mouseScrolls = mutableListOf<Pair<Float, Float>>()
        val keysyms = mutableListOf<Int>()

        override fun sendMouseMove(dx: Float, dy: Float) {
            mouseMoves.add(Pair(dx, dy))
        }

        override fun sendMouseMoveAbsolute(x: Float, y: Float) {
            mouseMovesAbsolute.add(Pair(x, y))
        }

        override fun sendMouseButton(button: Int, state: Int) {
            mouseButtons.add(Pair(button, state))
        }

        override fun sendMouseScroll(dx: Float, dy: Float) {
            mouseScrolls.add(Pair(dx, dy))
        }

        override fun sendKeysym(keysym: Int) {
            keysyms.add(keysym)
        }
    }

    @Test
    fun testCursorTrackerStateManagement() {
        val tracker = CursorTracker()
        assertFalse(tracker.isInitialized)

        // Initial setup at remote center
        tracker.ensureInitialized(1920, 1080)
        assertTrue(tracker.isInitialized)
        assertEquals(960f, tracker.virtualX, 0.01f)
        assertEquals(540f, tracker.virtualY, 0.01f)

        // Relative motion update
        tracker.updateRelative(100f, -40f, 1920, 1080)
        assertEquals(1060f, tracker.virtualX, 0.01f)
        assertEquals(500f, tracker.virtualY, 0.01f)

        // Clamping to screen edges
        tracker.updateRelative(5000f, 5000f, 1920, 1080)
        assertEquals(1920f, tracker.virtualX, 0.01f)
        assertEquals(1080f, tracker.virtualY, 0.01f)

        tracker.updateRelative(-5000f, -5000f, 1920, 1080)
        assertEquals(0f, tracker.virtualX, 0.01f)
        assertEquals(0f, tracker.virtualY, 0.01f)

        // Direct absolute positioning
        tracker.setAbsolute(500f, 400f, 1920, 1080)
        assertEquals(500f, tracker.virtualX, 0.01f)
        assertEquals(400f, tracker.virtualY, 0.01f)
    }

    @Test
    fun testModeSwitchingMaintainsSessionAndCursorState() {
        val listener = TestInputListener()
        val mapper = WardInputMapper(listener)

        // Default mode must be CURSOR
        assertEquals(InputMode.CURSOR, mapper.mode)

        // Cursor mode moves tracker
        mapper.cursorTracker.ensureInitialized(1920, 1080)
        mapper.cursorTracker.updateRelative(50f, 50f, 1920, 1080)
        val posAfterCursor = Pair(mapper.cursorTracker.virtualX, mapper.cursorTracker.virtualY)
        assertEquals(1010f, posAfterCursor.first, 0.01f)
        assertEquals(590f, posAfterCursor.second, 0.01f)

        // Switch to TOUCH mode
        mapper.setMode(InputMode.TOUCH)
        assertEquals(InputMode.TOUCH, mapper.mode)

        // Verify cursor tracker position is preserved
        assertEquals(posAfterCursor.first, mapper.cursorTracker.virtualX, 0.01f)
        assertEquals(posAfterCursor.second, mapper.cursorTracker.virtualY, 0.01f)

        // In Touch mode, touch sets absolute coordinate
        mapper.cursorTracker.setAbsolute(300f, 200f, 1920, 1080)
        assertEquals(300f, mapper.cursorTracker.virtualX, 0.01f)
        assertEquals(200f, mapper.cursorTracker.virtualY, 0.01f)

        // Switch back to CURSOR mode
        mapper.setMode(InputMode.CURSOR)
        assertEquals(InputMode.CURSOR, mapper.mode)

        // Cursor mode starts from touch position
        assertEquals(300f, mapper.cursorTracker.virtualX, 0.01f)
        assertEquals(200f, mapper.cursorTracker.virtualY, 0.01f)
    }

    @Test
    fun testInputModeProperties() {
        assertEquals("Cursor Mode", InputMode.CURSOR.displayName)
        assertEquals("🖱", InputMode.CURSOR.icon)
        assertEquals("Touch Mode", InputMode.TOUCH.displayName)
        assertEquals("👆", InputMode.TOUCH.icon)
    }
}
