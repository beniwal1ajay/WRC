package com.example.ward.ui.stream

/**
 * Bounds of the active remote desktop viewport on the Android screen.
 */
data class ViewportRect(
    val left: Float,
    val top: Float,
    val right: Float,
    val bottom: Float
) {
    val width: Float get() = (right - left).coerceAtLeast(0f)
    val height: Float get() = (bottom - top).coerceAtLeast(0f)
    val centerX: Float get() = left + width / 2f
    val centerY: Float get() = top + height / 2f

    fun contains(x: Float, y: Float, tolerance: Float = 0.5f): Boolean {
        return x in (left - tolerance)..(right + tolerance) &&
               y in (top - tolerance)..(bottom + tolerance)
    }
}

/**
 * 2D coordinates on the remote Ubuntu desktop.
 */
data class RemotePoint(
    val x: Float,
    val y: Float
)



/**
 * Transformation layer that maps Android screen touch coordinates to remote desktop coordinates.
 *
 * Accounts for:
 * - Dynamic remote desktop resolution (negotiated stream dimensions e.g. 1920x1080, 1280x720)
 * - Android display resolution & screen density
 * - Aspect ratio preservation (letterboxing in portrait, pillarboxing in wide landscape)
 * - Android system insets & immersive mode
 * - Measured onGloballyPositioned layout coordinates from Compose
 */
object CoordinateMapper {

    /**
     * Calculates the viewport centered in the container view without inset offset bias.
     */
    fun getCenteredViewport(
        viewWidth: Float,
        viewHeight: Float,
        remoteWidth: Int,
        remoteHeight: Int
    ): ViewportRect? {
        if (viewWidth <= 0f || viewHeight <= 0f || remoteWidth <= 0 || remoteHeight <= 0) {
            return null
        }

        val remoteAspect = remoteWidth.toFloat() / remoteHeight.toFloat()
        val viewAspect = viewWidth / viewHeight

        val (viewportWidth, viewportHeight) = if (viewAspect > remoteAspect) {
            val vh = viewHeight
            val vw = vh * remoteAspect
            Pair(vw, vh)
        } else {
            val vw = viewWidth
            val vh = vw / remoteAspect
            Pair(vw, vh)
        }

        val left = (viewWidth - viewportWidth) / 2f
        val top = (viewHeight - viewportHeight) / 2f
        val right = left + viewportWidth
        val bottom = top + viewportHeight

        return ViewportRect(left, top, right, bottom)
    }

    /**
     * Calculates the remote desktop viewport rectangle within the Android view accounting for insets.
     */
    fun getRemoteViewport(
        viewWidth: Float,
        viewHeight: Float,
        remoteWidth: Int,
        remoteHeight: Int,
        insetsLeft: Float = 0f,
        insetsTop: Float = 0f,
        insetsRight: Float = 0f,
        insetsBottom: Float = 0f
    ): ViewportRect? {
        if (viewWidth <= 0f || viewHeight <= 0f || remoteWidth <= 0 || remoteHeight <= 0) {
            return null
        }

        if (insetsLeft == 0f && insetsTop == 0f && insetsRight == 0f && insetsBottom == 0f) {
            return getCenteredViewport(viewWidth, viewHeight, remoteWidth, remoteHeight)
        }

        val usableWidth = (viewWidth - insetsLeft - insetsRight).coerceAtLeast(0f)
        val usableHeight = (viewHeight - insetsTop - insetsBottom).coerceAtLeast(0f)
        if (usableWidth <= 0f || usableHeight <= 0f) {
            return null
        }

        val remoteAspect = remoteWidth.toFloat() / remoteHeight.toFloat()
        val usableAspect = usableWidth / usableHeight

        val (viewportWidth, viewportHeight) = if (usableAspect > remoteAspect) {
            val vh = usableHeight
            val vw = vh * remoteAspect
            Pair(vw, vh)
        } else {
            val vw = usableWidth
            val vh = vw / remoteAspect
            Pair(vw, vh)
        }

        val left = insetsLeft + (usableWidth - viewportWidth) / 2f
        val top = insetsTop + (usableHeight - viewportHeight) / 2f
        val right = left + viewportWidth
        val bottom = top + viewportHeight

        return ViewportRect(left, top, right, bottom)
    }

    /**
     * Transforms an Android touch coordinate to remote desktop coordinates
     * using an exact measured viewport rectangle (e.g. from Compose onGloballyPositioned).
     */
    fun mapTouchToRemoteWithViewport(
        touchX: Float,
        touchY: Float,
        viewport: ViewportRect,
        remoteWidth: Int,
        remoteHeight: Int
    ): RemotePoint? {
        if (viewport.width <= 0f || viewport.height <= 0f || remoteWidth <= 0 || remoteHeight <= 0) {
            return null
        }

        if (!viewport.contains(touchX, touchY)) {
            return null
        }

        val normalizedX = ((touchX - viewport.left) / viewport.width).coerceIn(0f, 1f)
        val normalizedY = ((touchY - viewport.top) / viewport.height).coerceIn(0f, 1f)

        val remoteX = (normalizedX * remoteWidth.toFloat()).coerceIn(0f, remoteWidth.toFloat())
        val remoteY = (normalizedY * remoteHeight.toFloat()).coerceIn(0f, remoteHeight.toFloat())

        return RemotePoint(remoteX, remoteY)
    }

    /**
     * Transforms an Android touch coordinate to remote desktop coordinates.
     */
    fun mapTouchToRemote(
        touchX: Float,
        touchY: Float,
        viewWidth: Float,
        viewHeight: Float,
        remoteWidth: Int,
        remoteHeight: Int,
        insetsLeft: Float = 0f,
        insetsTop: Float = 0f,
        insetsRight: Float = 0f,
        insetsBottom: Float = 0f
    ): RemotePoint? {
        val viewport = getRemoteViewport(
            viewWidth = viewWidth,
            viewHeight = viewHeight,
            remoteWidth = remoteWidth,
            remoteHeight = remoteHeight,
            insetsLeft = insetsLeft,
            insetsTop = insetsTop,
            insetsRight = insetsRight,
            insetsBottom = insetsBottom
        ) ?: return null

        return mapTouchToRemoteWithViewport(touchX, touchY, viewport, remoteWidth, remoteHeight)
    }
}
