package ai.citros.chat

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ChatActivityOverlayBehaviorTest {

    @Test
    fun `deriveBackgroundSurfaceMode returns dynamic island when loading and tool status is null`() {
        val mode = deriveBackgroundSurfaceMode(
            isToolExecutionActive = false,
            isModelBusy = true,
            idleSurfaceMode = OverlaySurfaceMode.SEARCH_BAR
        )

        assertEquals(OverlaySurfaceMode.DYNAMIC_ISLAND, mode)
    }

    @Test
    fun `deriveBackgroundSurfaceMode returns idle surface when not loading and no tool active`() {
        val mode = deriveBackgroundSurfaceMode(
            isToolExecutionActive = false,
            isModelBusy = false,
            idleSurfaceMode = OverlaySurfaceMode.PANEL
        )

        assertEquals(OverlaySurfaceMode.PANEL, mode)
    }

    @Test
    fun `shouldPopOverlayRoute returns true when overlay service is null`() {
        assertTrue(shouldPopOverlayRoute(overlayServiceInstance = null))
    }

    @Test
    fun `shouldPopOverlayRoute returns false when overlay service is alive`() {
        val overlayService = OverlayService()

        assertFalse(shouldPopOverlayRoute(overlayServiceInstance = overlayService))
    }
}
