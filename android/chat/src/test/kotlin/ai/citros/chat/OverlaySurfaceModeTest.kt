package ai.citros.chat

import org.junit.Test
import kotlin.test.assertEquals

class OverlaySurfaceModeTest {

    // ========== toPrefValue ==========

    @Test
    fun `MINI_CHAT serializes to mini_chat`() {
        assertEquals("mini_chat", OverlaySurfaceMode.MINI_CHAT.toPrefValue())
    }

    @Test
    fun `BUBBLE serializes to bubble`() {
        assertEquals("bubble", OverlaySurfaceMode.BUBBLE.toPrefValue())
    }

    @Test
    fun `FULL_APP serializes to full_app`() {
        assertEquals("full_app", OverlaySurfaceMode.FULL_APP.toPrefValue())
    }

    // ========== fromPrefValue ==========

    @Test
    fun `fromPrefValue parses mini_chat`() {
        assertEquals(OverlaySurfaceMode.MINI_CHAT, OverlaySurfaceMode.fromPrefValue("mini_chat"))
    }

    @Test
    fun `fromPrefValue parses bubble`() {
        assertEquals(OverlaySurfaceMode.BUBBLE, OverlaySurfaceMode.fromPrefValue("bubble"))
    }

    @Test
    fun `fromPrefValue parses full_app`() {
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlaySurfaceMode.fromPrefValue("full_app"))
    }

    @Test
    fun `fromPrefValue defaults to MINI_CHAT for null`() {
        assertEquals(OverlaySurfaceMode.MINI_CHAT, OverlaySurfaceMode.fromPrefValue(null))
    }

    @Test
    fun `fromPrefValue defaults to MINI_CHAT for unknown string`() {
        assertEquals(OverlaySurfaceMode.MINI_CHAT, OverlaySurfaceMode.fromPrefValue("unknown"))
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
