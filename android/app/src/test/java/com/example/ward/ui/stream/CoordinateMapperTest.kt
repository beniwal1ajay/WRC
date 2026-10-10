package com.example.ward.ui.stream

import org.junit.Assert.*
import org.junit.Test

class CoordinateMapperTest {

    @Test
    fun test1920x1080OnPortraitDeviceCornersAndCenter() {
        val viewWidth = 1080f
        val viewHeight = 2400f
        val remoteWidth = 1920
        val remoteHeight = 1080

        val viewport = CoordinateMapper.getRemoteViewport(
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(viewport)
        viewport!!

        // Expected letterbox: video width 1080, height 1080 / (16/9) = 607.5
        assertEquals(1080f, viewport.width, 0.01f)
        assertEquals(607.5f, viewport.height, 0.01f)
        val expectedTop = (2400f - 607.5f) / 2f
        assertEquals(0f, viewport.left, 0.01f)
        assertEquals(expectedTop, viewport.top, 0.01f)

        // Top-left
        val topLeft = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.left,
            touchY = viewport.top,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(topLeft)
        assertEquals(0f, topLeft!!.x, 0.5f)
        assertEquals(0f, topLeft.y, 0.5f)

        // Top-right
        val topRight = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.right,
            touchY = viewport.top,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(topRight)
        assertEquals(1920f, topRight!!.x, 0.5f)
        assertEquals(0f, topRight.y, 0.5f)

        // Bottom-left
        val bottomLeft = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.left,
            touchY = viewport.bottom,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(bottomLeft)
        assertEquals(0f, bottomLeft!!.x, 0.5f)
        assertEquals(1080f, bottomLeft.y, 0.5f)

        // Bottom-right
        val bottomRight = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.right,
            touchY = viewport.bottom,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(bottomRight)
        assertEquals(1920f, bottomRight!!.x, 0.5f)
        assertEquals(1080f, bottomRight.y, 0.5f)

        // Center
        val center = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.centerX,
            touchY = viewport.centerY,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(center)
        assertEquals(960f, center!!.x, 0.5f)
        assertEquals(540f, center.y, 0.5f)
    }

    @Test
    fun test1920x1080OnLandscapeDeviceCornersAndCenter() {
        val viewWidth = 2400f
        val viewHeight = 1080f
        val remoteWidth = 1920
        val remoteHeight = 1080

        val viewport = CoordinateMapper.getRemoteViewport(
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(viewport)
        viewport!!

        // Expected pillarbox: video height 1080, width 1080 * (16/9) = 1920
        assertEquals(1920f, viewport.width, 0.01f)
        assertEquals(1080f, viewport.height, 0.01f)
        val expectedLeft = (2400f - 1920f) / 2f
        assertEquals(expectedLeft, viewport.left, 0.01f)
        assertEquals(0f, viewport.top, 0.01f)

        // Top-left
        val topLeft = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.left,
            touchY = viewport.top,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(topLeft)
        assertEquals(0f, topLeft!!.x, 0.5f)
        assertEquals(0f, topLeft.y, 0.5f)

        // Top-right
        val topRight = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.right,
            touchY = viewport.top,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(topRight)
        assertEquals(1920f, topRight!!.x, 0.5f)
        assertEquals(0f, topRight.y, 0.5f)

        // Bottom-left
        val bottomLeft = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.left,
            touchY = viewport.bottom,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(bottomLeft)
        assertEquals(0f, bottomLeft!!.x, 0.5f)
        assertEquals(1080f, bottomLeft.y, 0.5f)

        // Bottom-right
        val bottomRight = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.right,
            touchY = viewport.bottom,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(bottomRight)
        assertEquals(1920f, bottomRight!!.x, 0.5f)
        assertEquals(1080f, bottomRight.y, 0.5f)

        // Center
        val center = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.centerX,
            touchY = viewport.centerY,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNotNull(center)
        assertEquals(960f, center!!.x, 0.5f)
        assertEquals(540f, center.y, 0.5f)
    }

    @Test
    fun testTouchesOutsideViewportAreRejected() {
        val viewWidth = 1080f
        val viewHeight = 2400f
        val remoteWidth = 1920
        val remoteHeight = 1080

        // In portrait, top and bottom letterbox bars are outside the remote viewport:
        // Top black bar touch
        val topLetterboxTouch = CoordinateMapper.mapTouchToRemote(
            touchX = 540f,
            touchY = 200f,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNull("Touch in top letterbox bar must be rejected", topLetterboxTouch)

        // Bottom black bar touch
        val bottomLetterboxTouch = CoordinateMapper.mapTouchToRemote(
            touchX = 540f,
            touchY = 2200f,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNull("Touch in bottom letterbox bar must be rejected", bottomLetterboxTouch)

        // In landscape, side pillarbox bars are outside the remote viewport:
        val landscapeViewWidth = 2400f
        val landscapeViewHeight = 1080f

        val leftPillarboxTouch = CoordinateMapper.mapTouchToRemote(
            touchX = 50f,
            touchY = 540f,
            viewWidth = landscapeViewWidth,
            viewHeight = landscapeViewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNull("Touch in left pillarbox bar must be rejected", leftPillarboxTouch)

        val rightPillarboxTouch = CoordinateMapper.mapTouchToRemote(
            touchX = 2350f,
            touchY = 540f,
            viewWidth = landscapeViewWidth,
            viewHeight = landscapeViewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight
        )
        assertNull("Touch in right pillarbox bar must be rejected", rightPillarboxTouch)
    }

    @Test
    fun testSystemInsetsHandling() {
        val viewWidth = 1080f
        val viewHeight = 2400f
        val remoteWidth = 1920
        val remoteHeight = 1080
        val insetsTop = 80f
        val insetsBottom = 120f

        val viewport = CoordinateMapper.getRemoteViewport(
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight,
            insetsTop = insetsTop,
            insetsBottom = insetsBottom
        )
        assertNotNull(viewport)
        viewport!!

        // Touch inside status bar area must be rejected
        val statusBarTouch = CoordinateMapper.mapTouchToRemote(
            touchX = 540f,
            touchY = 40f,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight,
            insetsTop = insetsTop,
            insetsBottom = insetsBottom
        )
        assertNull("Touch in status bar insets must be rejected", statusBarTouch)

        // Touch inside navigation bar area must be rejected
        val navBarTouch = CoordinateMapper.mapTouchToRemote(
            touchX = 540f,
            touchY = 2350f,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight,
            insetsTop = insetsTop,
            insetsBottom = insetsBottom
        )
        assertNull("Touch in navigation bar insets must be rejected", navBarTouch)

        // Viewport center must map to remote center
        val center = CoordinateMapper.mapTouchToRemote(
            touchX = viewport.centerX,
            touchY = viewport.centerY,
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight,
            insetsTop = insetsTop,
            insetsBottom = insetsBottom
        )
        assertNotNull(center)
        assertEquals(960f, center!!.x, 0.5f)
        assertEquals(540f, center.y, 0.5f)
    }

    @Test
    fun testDifferentRemoteResolutions() {
        // 720p stream
        val p720 = CoordinateMapper.mapTouchToRemote(
            touchX = 640f,
            touchY = 360f,
            viewWidth = 1280f,
            viewHeight = 720f,
            remoteWidth = 1280,
            remoteHeight = 720
        )
        assertNotNull(p720)
        assertEquals(640f, p720!!.x, 0.5f)
        assertEquals(360f, p720.y, 0.5f)

        // 16:10 aspect ratio desktop (1920x1200)
        val viewport1610 = CoordinateMapper.getRemoteViewport(
            viewWidth = 1920f,
            viewHeight = 1080f,
            remoteWidth = 1920,
            remoteHeight = 1200
        )
        assertNotNull(viewport1610)
        // Container 1920x1080 is wider than 1920/1200 (1.6) -> pillarbox on sides
        assertEquals(1080f, viewport1610!!.height, 0.01f)
        assertEquals(1080f * (1920f / 1200f), viewport1610.width, 0.01f)

        // 4:3 aspect ratio desktop (1024x768)
        val p43 = CoordinateMapper.mapTouchToRemote(
            touchX = 512f,
            touchY = 384f,
            viewWidth = 1024f,
            viewHeight = 768f,
            remoteWidth = 1024,
            remoteHeight = 768
        )
        assertNotNull(p43)
        assertEquals(512f, p43!!.x, 0.5f)
        assertEquals(384f, p43.y, 0.5f)
    }

    @Test
    fun testInvalidDimensionsHandledGracefully() {
        assertNull(CoordinateMapper.getRemoteViewport(0f, 1080f, 1920, 1080))
        assertNull(CoordinateMapper.getRemoteViewport(1920f, 0f, 1920, 1080))
        assertNull(CoordinateMapper.getRemoteViewport(1920f, 1080f, 0, 1080))
        assertNull(CoordinateMapper.getRemoteViewport(1920f, 1080f, 1920, 0))
        assertNull(CoordinateMapper.mapTouchToRemote(100f, 100f, -10f, 1080f, 1920, 1080))
    }

    @Test
    fun testMeasuredViewportMapping() {
        val measured = ViewportRect(left = 0f, top = 896f, right = 1080f, bottom = 1504f)
        val pTop = CoordinateMapper.mapTouchToRemoteWithViewport(
            touchX = 540f,
            touchY = 896f,
            viewport = measured,
            remoteWidth = 1920,
            remoteHeight = 1080
        )
        assertNotNull(pTop)
        assertEquals(960f, pTop!!.x, 0.5f)
        assertEquals(0f, pTop.y, 0.5f)

        val pBottom = CoordinateMapper.mapTouchToRemoteWithViewport(
            touchX = 540f,
            touchY = 1504f,
            viewport = measured,
            remoteWidth = 1920,
            remoteHeight = 1080
        )
        assertNotNull(pBottom)
        assertEquals(960f, pBottom!!.x, 0.5f)
        assertEquals(1080f, pBottom.y, 0.5f)
    }
}
