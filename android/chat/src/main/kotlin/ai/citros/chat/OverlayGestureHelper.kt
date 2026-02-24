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

    /**
     * If the panel overlay is released in the top quarter of the screen,
     * snap to top gravity.
     */
    const val SNAP_TO_TOP_FRACTION = 0.25f

    /**
     * Classify the gesture that occurred on a PANEL overlay drag release.
     *
     * @param velocityY Vertical velocity in px/sec (negative = upward)
     * @param releaseY The Y coordinate where the finger was lifted (absolute screen px)
     * @param screenHeight Total screen height in px
     * @return The gesture action to take
     */
    fun classifyPanelGesture(
        velocityY: Float,
        releaseY: Float,
        screenHeight: Int
    ): PanelGestureAction {
        // Fast upward swipe → snap to top
        if (velocityY < -FLING_VELOCITY_THRESHOLD_PX_PER_SEC) {
            return PanelGestureAction.SNAP_TO_TOP
        }
        // Fast downward swipe → minimize to search bar
        if (velocityY > FLING_VELOCITY_THRESHOLD_PX_PER_SEC) {
            return PanelGestureAction.MINIMIZE_TO_SEARCH_BAR
        }
        // Released in top quarter → snap to top
        if (releaseY < screenHeight * SNAP_TO_TOP_FRACTION) {
            return PanelGestureAction.SNAP_TO_TOP
        }
        // Default: snap back to bottom
        return PanelGestureAction.SNAP_TO_BOTTOM
    }

    /**
     * Resolve final panel release behavior when IME visibility should be considered.
     *
     * When keyboard is visible, a fast downward swipe should dismiss keyboard first
     * instead of minimizing overlay to search bar.
     */
    fun classifyPanelReleaseAction(
        velocityY: Float,
        releaseY: Float,
        screenHeight: Int,
        isImeVisible: Boolean
    ): PanelReleaseAction {
        val baseAction = classifyPanelGesture(
            velocityY = velocityY,
            releaseY = releaseY,
            screenHeight = screenHeight
        )
        return if (isImeVisible && baseAction == PanelGestureAction.MINIMIZE_TO_SEARCH_BAR) {
            PanelReleaseAction.DISMISS_KEYBOARD
        } else {
            PanelReleaseAction.PERFORM_GESTURE(baseAction)
        }
    }
}

sealed interface PanelReleaseAction {
    data class PERFORM_GESTURE(val action: PanelGestureAction) : PanelReleaseAction
    data object DISMISS_KEYBOARD : PanelReleaseAction
}

/** Actions resulting from a panel drag gesture classification. */
enum class PanelGestureAction {
    /** Dock the overlay at the top of the screen. */
    SNAP_TO_TOP,
    /** Return the overlay to its default bottom position. */
    SNAP_TO_BOTTOM,
    /** Minimize the overlay to docked search bar mode. */
    MINIMIZE_TO_SEARCH_BAR
}
