package ai.citros.chat

import org.junit.Test
import kotlin.test.assertEquals

class OverlaySurfaceModeTest {

    // ========== toPrefValue ==========

    @Test
    fun `PANEL serializes to panel`() {
        assertEquals("panel", OverlaySurfaceMode.PANEL.toPrefValue())
    }

    @Test
    fun `SEARCH_BAR serializes to search_bar`() {
        assertEquals("search_bar", OverlaySurfaceMode.SEARCH_BAR.toPrefValue())
    }

    @Test
    fun `FULL_APP serializes to full_app`() {
        assertEquals("full_app", OverlaySurfaceMode.FULL_APP.toPrefValue())
    }

    // ========== fromPrefValue ==========

    @Test
    fun `fromPrefValue parses panel`() {
        assertEquals(OverlaySurfaceMode.PANEL, OverlaySurfaceMode.fromPrefValue("panel"))
    }

    @Test
    fun `fromPrefValue parses search_bar`() {
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlaySurfaceMode.fromPrefValue("search_bar"))
    }

    @Test
    fun `fromPrefValue parses legacy mini_chat`() {
        assertEquals(OverlaySurfaceMode.PANEL, OverlaySurfaceMode.fromPrefValue("mini_chat"))
    }

    @Test
    fun `fromPrefValue parses legacy bubble`() {
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlaySurfaceMode.fromPrefValue("bubble"))
    }

    @Test
    fun `fromPrefValue parses full_app`() {
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlaySurfaceMode.fromPrefValue("full_app"))
    }

    @Test
    fun `fromPrefValue parses dynamic_island`() {
        assertEquals(OverlaySurfaceMode.DYNAMIC_ISLAND, OverlaySurfaceMode.fromPrefValue("dynamic_island"))
    }

    @Test
    fun `fromPrefValue defaults to SEARCH_BAR for null`() {
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlaySurfaceMode.fromPrefValue(null))
    }

    @Test
    fun `fromPrefValue defaults to SEARCH_BAR for unknown string`() {
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlaySurfaceMode.fromPrefValue("unknown"))
    }

    // ========== Round-trip ==========

    @Test
    fun `round-trip through toPrefValue and fromPrefValue`() {
        OverlaySurfaceMode.entries.forEach { mode ->
            assertEquals(mode, OverlaySurfaceMode.fromPrefValue(mode.toPrefValue()),
                "Round-trip failed for $mode")
        }
    }
}
