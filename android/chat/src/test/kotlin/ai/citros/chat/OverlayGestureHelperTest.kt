package ai.citros.chat

import kotlin.test.assertEquals
import org.junit.Test

class OverlayGestureHelperTest {

    private val screenHeight = 2400

    @Test
    fun `fast upward swipe snaps panel to top`() {
        val result = OverlayGestureHelper.classifyPanelGesture(
            velocityY = -1200f,
            releaseY = 1200f,
            screenHeight = screenHeight
        )
        assertEquals(PanelGestureAction.SNAP_TO_TOP, result)
    }

    @Test
    fun `fast downward swipe minimizes panel to search bar`() {
        val result = OverlayGestureHelper.classifyPanelGesture(
            velocityY = 1200f,
            releaseY = 1200f,
            screenHeight = screenHeight
        )
        assertEquals(PanelGestureAction.MINIMIZE_TO_SEARCH_BAR, result)
    }

    @Test
    fun `release in top quarter snaps panel to top`() {
        val result = OverlayGestureHelper.classifyPanelGesture(
            velocityY = 0f,
            releaseY = 400f,
            screenHeight = screenHeight
        )
        assertEquals(PanelGestureAction.SNAP_TO_TOP, result)
    }

    @Test
    fun `slow middle release snaps panel to bottom`() {
        val result = OverlayGestureHelper.classifyPanelGesture(
            velocityY = 0f,
            releaseY = 1800f,
            screenHeight = screenHeight
        )
        assertEquals(PanelGestureAction.SNAP_TO_BOTTOM, result)
    }

    @Test
    fun `velocity thresholds still gate swipe decisions`() {
        val down = OverlayGestureHelper.classifyPanelGesture(
            velocityY = OverlayGestureHelper.FLING_VELOCITY_THRESHOLD_PX_PER_SEC + 1f,
            releaseY = 1800f,
            screenHeight = screenHeight
        )
        assertEquals(PanelGestureAction.MINIMIZE_TO_SEARCH_BAR, down)

        val up = OverlayGestureHelper.classifyPanelGesture(
            velocityY = -(OverlayGestureHelper.FLING_VELOCITY_THRESHOLD_PX_PER_SEC + 1f),
            releaseY = 1800f,
            screenHeight = screenHeight
        )
        assertEquals(PanelGestureAction.SNAP_TO_TOP, up)
    }

    @Test
    fun `downward swipe dismisses keyboard instead of minimizing when IME is visible`() {
        val result = OverlayGestureHelper.classifyPanelReleaseAction(
            velocityY = 1200f,
            releaseY = 1600f,
            screenHeight = screenHeight,
            isImeVisible = true
        )

        assertEquals(PanelReleaseAction.DISMISS_KEYBOARD, result)
    }

    @Test
    fun `downward swipe still minimizes when IME is hidden`() {
        val result = OverlayGestureHelper.classifyPanelReleaseAction(
            velocityY = 1200f,
            releaseY = 1600f,
            screenHeight = screenHeight,
            isImeVisible = false
        )

        assertEquals(
            PanelReleaseAction.PERFORM_GESTURE(PanelGestureAction.MINIMIZE_TO_SEARCH_BAR),
            result
        )
    }

    @Test
    fun `release that would snap to top stays snap-to-top when IME is visible`() {
        val result = OverlayGestureHelper.classifyPanelReleaseAction(
            velocityY = 0f,
            releaseY = 300f,
            screenHeight = screenHeight,
            isImeVisible = true
        )

        assertEquals(
            PanelReleaseAction.PERFORM_GESTURE(PanelGestureAction.SNAP_TO_TOP),
            result
        )
    }

    @Test
    fun `upward swipe still snaps panel to top when IME is visible`() {
        val result = OverlayGestureHelper.classifyPanelReleaseAction(
            velocityY = -1200f,
            releaseY = 1600f,
            screenHeight = screenHeight,
            isImeVisible = true
        )

        assertEquals(
            PanelReleaseAction.PERFORM_GESTURE(PanelGestureAction.SNAP_TO_TOP),
            result
        )
    }
}
