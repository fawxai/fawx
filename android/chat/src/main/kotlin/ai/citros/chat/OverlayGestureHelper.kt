package ai.citros.chat

/**
 * Pure-Kotlin gesture classification for overlay drag/swipe/fling (#408).
 *
 * All methods are stateless and deterministic — easy to unit test.
 * OverlayService calls these from its touch handler to decide what
 * action to take on ACTION_UP.
 */
object OverlayGestureHelper {

    /** Minimum fling velocity (px/sec) to trigger a swipe gesture. */
    const val FLING_VELOCITY_THRESHOLD_PX_PER_SEC = 800f

    /** Duration (ms) for snap animations (corner snap, top/bottom dock). */
    const val SNAP_ANIMATION_DURATION_MS = 250L

    /** Frame interval for animation loop (~60fps). */
    const val ANIMATION_FRAME_INTERVAL_MS = 16L

    /** Decelerate interpolator factor — higher = sharper deceleration. */
    const val DECELERATE_FACTOR = 2f

    /**
     * If the mini-chat overlay is released in the top quarter of the screen,
     * snap to top gravity.
     */
    const val SNAP_TO_TOP_FRACTION = 0.25f

    /**
     * Classify the gesture that occurred on a MINI_CHAT overlay drag release.
     *
     * @param velocityY Vertical velocity in px/sec (negative = upward)
     * @param releaseY The Y coordinate where the finger was lifted (absolute screen px)
     * @param screenHeight Total screen height in px
     * @return The gesture action to take
     */
    fun classifyMiniChatGesture(
        velocityY: Float,
        releaseY: Float,
        screenHeight: Int
    ): MiniChatGestureAction {
        // Fast upward swipe → snap to top
        if (velocityY < -FLING_VELOCITY_THRESHOLD_PX_PER_SEC) {
            return MiniChatGestureAction.SNAP_TO_TOP
        }
        // Fast downward swipe → minimize to bubble
        if (velocityY > FLING_VELOCITY_THRESHOLD_PX_PER_SEC) {
            return MiniChatGestureAction.MINIMIZE_TO_BUBBLE
        }
        // Released in top quarter → snap to top
        if (releaseY < screenHeight * SNAP_TO_TOP_FRACTION) {
            return MiniChatGestureAction.SNAP_TO_TOP
        }
        // Default: snap back to bottom
        return MiniChatGestureAction.SNAP_TO_BOTTOM
    }

    /**
     * Calculate the nearest screen corner for a bubble fling release.
     *
     * @param releaseX X coordinate of release point
     * @param releaseY Y coordinate of release point
     * @param screenWidth Screen width in px
     * @param screenHeight Screen height in px
     * @param bubbleSizePx Bubble diameter in px
     * @param marginPx Margin from screen edge in px
     * @return Pair of (x, y) coordinates for the target corner position
     */
    fun snapBubbleToCorner(
        releaseX: Int,
        releaseY: Int,
        screenWidth: Int,
        screenHeight: Int,
        bubbleSizePx: Int,
        marginPx: Int,
        bottomMarginPx: Int = marginPx
    ): Pair<Int, Int> {
        val centerX = releaseX + bubbleSizePx / 2
        val centerY = releaseY + bubbleSizePx / 2

        val isLeft = centerX < screenWidth / 2
        val isTop = centerY < screenHeight / 2

        val x = if (isLeft) marginPx else screenWidth - bubbleSizePx - marginPx
        // Bottom corners use a larger margin to stay above the nav bar / gesture area
        val y = if (isTop) marginPx else screenHeight - bubbleSizePx - bottomMarginPx

        return Pair(x, y)
    }

    /**
     * Classify a bubble drag release gesture.
     *
     * @param velocityY Vertical velocity in px/sec (negative = upward)
     * @return The gesture action to take
     */
    fun classifyBubbleGesture(velocityY: Float): BubbleGestureAction {
        // Fast upward swipe → expand to mini-chat
        if (velocityY < -FLING_VELOCITY_THRESHOLD_PX_PER_SEC) {
            return BubbleGestureAction.EXPAND_TO_MINI_CHAT
        }
        // Default: snap to nearest corner
        return BubbleGestureAction.SNAP_TO_CORNER
    }
}

/** Actions resulting from a mini-chat drag gesture classification. */
enum class MiniChatGestureAction {
    /** Dock the overlay at the top of the screen. */
    SNAP_TO_TOP,
    /** Return the overlay to its default bottom position. */
    SNAP_TO_BOTTOM,
    /** Minimize the overlay to bubble mode. */
    MINIMIZE_TO_BUBBLE
}

/** Actions resulting from a bubble drag gesture classification. */
enum class BubbleGestureAction {
    /** Snap the bubble to the nearest screen corner. */
    SNAP_TO_CORNER,
    /** Expand the bubble into mini-chat mode. */
    EXPAND_TO_MINI_CHAT
}
