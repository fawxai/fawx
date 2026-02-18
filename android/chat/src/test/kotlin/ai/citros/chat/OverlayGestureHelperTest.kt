package ai.citros.chat

import org.junit.Test
import kotlin.test.assertEquals

class OverlayGestureHelperTest {

    private val screenWidth = 1080
    private val screenHeight = 2400
    private val bubbleSize = 56 * 3 // 56dp at 3x density = 168px
    private val margin = 16 * 3 // 48px

    // ========== Mini-Chat Gesture Classification ==========

    @Test
    fun `fast upward swipe snaps mini-chat to top`() {
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = -1200f, // strong upward
            releaseY = 1200f,   // middle of screen
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.SNAP_TO_TOP, result)
    }

    @Test
    fun `fast downward swipe minimizes to bubble`() {
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = 1200f, // strong downward
            releaseY = 1200f,
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.MINIMIZE_TO_BUBBLE, result)
    }

    @Test
    fun `release in top quarter snaps to top`() {
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = 0f,    // no velocity (slow drag)
            releaseY = 400f,   // top quarter of 2400px screen
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.SNAP_TO_TOP, result)
    }

    @Test
    fun `release in bottom half snaps to bottom`() {
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = 0f,
            releaseY = 1800f,  // bottom half
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.SNAP_TO_BOTTOM, result)
    }

    @Test
    fun `slow upward drag in middle snaps to bottom`() {
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = -200f,  // slow upward (below threshold)
            releaseY = 1200f,   // middle
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.SNAP_TO_BOTTOM, result)
    }

    @Test
    fun `velocity at exact threshold minimizes to bubble`() {
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = OverlayGestureHelper.FLING_VELOCITY_THRESHOLD_PX_PER_SEC + 1f,
            releaseY = 1200f,
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.MINIMIZE_TO_BUBBLE, result)
    }

    @Test
    fun `velocity just below threshold does not fling`() {
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = OverlayGestureHelper.FLING_VELOCITY_THRESHOLD_PX_PER_SEC - 1f,
            releaseY = 1800f,
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.SNAP_TO_BOTTOM, result)
    }

    @Test
    fun `negative velocity at exact threshold snaps to top`() {
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = -(OverlayGestureHelper.FLING_VELOCITY_THRESHOLD_PX_PER_SEC + 1f),
            releaseY = 1800f,
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.SNAP_TO_TOP, result)
    }

    @Test
    fun `negative velocity just below threshold does not snap to top`() {
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = -(OverlayGestureHelper.FLING_VELOCITY_THRESHOLD_PX_PER_SEC - 1f),
            releaseY = 1800f, // bottom half, so should snap to bottom
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.SNAP_TO_BOTTOM, result)
    }

    @Test
    fun `upward velocity takes priority over position`() {
        // Even though released in bottom half, fast upward swipe → snap to top
        val result = OverlayGestureHelper.classifyMiniChatGesture(
            velocityY = -1000f,
            releaseY = 2000f,  // very bottom
            screenHeight = screenHeight
        )
        assertEquals(MiniChatGestureAction.SNAP_TO_TOP, result)
    }

    // ========== Bubble Corner Snap ==========

    @Test
    fun `bubble released top-left snaps to top-left corner`() {
        val (x, y) = OverlayGestureHelper.snapBubbleToCorner(
            releaseX = 100, releaseY = 100,
            screenWidth = screenWidth, screenHeight = screenHeight,
            bubbleSizePx = bubbleSize, marginPx = margin
        )
        assertEquals(margin, x)
        assertEquals(margin, y)
    }

    @Test
    fun `bubble released top-right snaps to top-right corner`() {
        val (x, y) = OverlayGestureHelper.snapBubbleToCorner(
            releaseX = 900, releaseY = 100,
            screenWidth = screenWidth, screenHeight = screenHeight,
            bubbleSizePx = bubbleSize, marginPx = margin
        )
        assertEquals(screenWidth - bubbleSize - margin, x)
        assertEquals(margin, y)
    }

    @Test
    fun `bubble released bottom-left snaps to bottom-left corner`() {
        val (x, y) = OverlayGestureHelper.snapBubbleToCorner(
            releaseX = 100, releaseY = 2000,
            screenWidth = screenWidth, screenHeight = screenHeight,
            bubbleSizePx = bubbleSize, marginPx = margin
        )
        assertEquals(margin, x)
        assertEquals(screenHeight - bubbleSize - margin, y)
    }

    @Test
    fun `bubble released bottom-right snaps to bottom-right corner`() {
        val (x, y) = OverlayGestureHelper.snapBubbleToCorner(
            releaseX = 900, releaseY = 2000,
            screenWidth = screenWidth, screenHeight = screenHeight,
            bubbleSizePx = bubbleSize, marginPx = margin
        )
        assertEquals(screenWidth - bubbleSize - margin, x)
        assertEquals(screenHeight - bubbleSize - margin, y)
    }

    @Test
    fun `odd bubble size snaps correctly without rounding errors`() {
        val oddBubbleSize = 171 // odd px — center offset is 85.5, truncated to 85
        val (x, y) = OverlayGestureHelper.snapBubbleToCorner(
            releaseX = 100, releaseY = 100,
            screenWidth = screenWidth, screenHeight = screenHeight,
            bubbleSizePx = oddBubbleSize, marginPx = margin
        )
        assertEquals(margin, x)
        assertEquals(margin, y)

        // Verify bottom-right with odd size
        val (x2, y2) = OverlayGestureHelper.snapBubbleToCorner(
            releaseX = 900, releaseY = 2000,
            screenWidth = screenWidth, screenHeight = screenHeight,
            bubbleSizePx = oddBubbleSize, marginPx = margin
        )
        assertEquals(screenWidth - oddBubbleSize - margin, x2)
        assertEquals(screenHeight - oddBubbleSize - margin, y2)
    }

    @Test
    fun `bubble at exact center snaps to bottom-right`() {
        // center = (540, 1200), which is right-half, bottom-half
        val (x, y) = OverlayGestureHelper.snapBubbleToCorner(
            releaseX = 540 - bubbleSize / 2,
            releaseY = 1200 - bubbleSize / 2,
            screenWidth = screenWidth, screenHeight = screenHeight,
            bubbleSizePx = bubbleSize, marginPx = margin
        )
        assertEquals(screenWidth - bubbleSize - margin, x)
        assertEquals(screenHeight - bubbleSize - margin, y)
    }

    // ========== Bubble Gesture Classification ==========

    @Test
    fun `fast upward swipe on bubble expands to mini-chat`() {
        val result = OverlayGestureHelper.classifyBubbleGesture(velocityY = -1000f)
        assertEquals(BubbleGestureAction.EXPAND_TO_MINI_CHAT, result)
    }

    @Test
    fun `slow drag on bubble snaps to corner`() {
        val result = OverlayGestureHelper.classifyBubbleGesture(velocityY = -200f)
        assertEquals(BubbleGestureAction.SNAP_TO_CORNER, result)
    }

    @Test
    fun `downward swipe on bubble snaps to corner`() {
        val result = OverlayGestureHelper.classifyBubbleGesture(velocityY = 1000f)
        assertEquals(BubbleGestureAction.SNAP_TO_CORNER, result)
    }

    @Test
    fun `zero velocity on bubble snaps to corner`() {
        val result = OverlayGestureHelper.classifyBubbleGesture(velocityY = 0f)
        assertEquals(BubbleGestureAction.SNAP_TO_CORNER, result)
    }
}
