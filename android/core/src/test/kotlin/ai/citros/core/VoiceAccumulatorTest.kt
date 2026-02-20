package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Tests for [VoiceAccumulator] — the accumulation algorithm extracted
 * from `MessageInput.beginListening` (#637).
 *
 * Tests the exact production logic: prefix preservation, multi-segment
 * concatenation, partial display states, auto-send, and error handling.
 */
class VoiceAccumulatorTest {

    // ── Single segment ─────────────────────────────────────────────────

    @Test
    fun `single Final produces text`() {
        val acc = VoiceAccumulator(prefix = "")
        val display = acc.onEvent(SpeechEvent.Final("hello"))
        assertEquals("hello", display)
        assertEquals("hello", acc.accumulatedText)
    }

    // ── Multi-segment accumulation ─────────────────────────────────────

    @Test
    fun `multiple Finals accumulate with spaces`() {
        val acc = VoiceAccumulator(prefix = "")
        acc.onEvent(SpeechEvent.Final("hello"))
        val display = acc.onEvent(SpeechEvent.Final("world"))
        assertEquals("hello world", display)
        assertEquals("hello world", acc.accumulatedText)
    }

    @Test
    fun `three segments accumulate correctly`() {
        val acc = VoiceAccumulator(prefix = "")
        acc.onEvent(SpeechEvent.Final("the quick"))
        acc.onEvent(SpeechEvent.Final("brown fox"))
        val display = acc.onEvent(SpeechEvent.Final("jumps"))
        assertEquals("the quick brown fox jumps", display)
    }

    // ── Prefix preservation ────────────────────────────────────────────

    @Test
    fun `existing text is preserved as prefix`() {
        val acc = VoiceAccumulator(prefix = "draft:")
        val display = acc.onEvent(SpeechEvent.Final("hello"))
        assertEquals("draft: hello", display)
    }

    @Test
    fun `prefix plus multi-segment accumulation`() {
        val acc = VoiceAccumulator(prefix = "draft:")
        acc.onEvent(SpeechEvent.Final("hello"))
        val display = acc.onEvent(SpeechEvent.Final("world"))
        assertEquals("draft: hello world", display)
    }

    @Test
    fun `blank prefix does not add extra space`() {
        val acc = VoiceAccumulator(prefix = "")
        val display = acc.onEvent(SpeechEvent.Final("hello"))
        assertEquals("hello", display)
    }

    @Test
    fun `whitespace-only prefix treated as blank`() {
        val acc = VoiceAccumulator(prefix = "   ")
        val display = acc.onEvent(SpeechEvent.Final("hello"))
        // isNotBlank() returns false for whitespace-only, so no prefix prepended
        assertEquals("hello", display)
    }

    // ── Partial display states ─────────────────────────────────────────

    @Test
    fun `Partial with no accumulated text shows Listening`() {
        val acc = VoiceAccumulator(prefix = "")
        val display = acc.onEvent(SpeechEvent.Partial("Listening..."))
        assertEquals("Listening...", display)
    }

    @Test
    fun `Partial after Final shows accumulated with ellipsis`() {
        val acc = VoiceAccumulator(prefix = "")
        acc.onEvent(SpeechEvent.Final("hello"))
        val display = acc.onEvent(SpeechEvent.Partial("Listening..."))
        assertEquals("hello...", display)
    }

    @Test
    fun `Partial with prefix and no accumulated text`() {
        val acc = VoiceAccumulator(prefix = "note:")
        val display = acc.onEvent(SpeechEvent.Partial("Listening..."))
        assertEquals("note: Listening...", display)
    }

    @Test
    fun `Partial with prefix and accumulated text`() {
        val acc = VoiceAccumulator(prefix = "note:")
        acc.onEvent(SpeechEvent.Final("hello"))
        val display = acc.onEvent(SpeechEvent.Partial("Listening..."))
        assertEquals("note: hello...", display)
    }

    @Test
    fun `Final after Partial replaces indicator with real text`() {
        val acc = VoiceAccumulator(prefix = "")
        acc.onEvent(SpeechEvent.Partial("Listening..."))
        val display = acc.onEvent(SpeechEvent.Final("hello"))
        assertEquals("hello", display)
    }

    // ── Auto-send behavior ─────────────────────────────────────────────

    @Test
    fun `finish with auto-send returns sendable text and clears display`() {
        val acc = VoiceAccumulator(prefix = "")
        acc.onEvent(SpeechEvent.Final("hello"))
        acc.onEvent(SpeechEvent.Final("world"))
        val result = acc.finish(autoSend = true)
        assertEquals("hello world", result.autoSendText)
        assertEquals("", result.displayText)
    }

    @Test
    fun `finish with auto-send includes prefix in final text`() {
        val acc = VoiceAccumulator(prefix = "draft:")
        acc.onEvent(SpeechEvent.Final("hello"))
        val result = acc.finish(autoSend = true)
        assertEquals("draft: hello", result.autoSendText)
        assertEquals("", result.displayText)
    }

    @Test
    fun `finish with auto-send skipped when accumulated is blank`() {
        val acc = VoiceAccumulator(prefix = "draft:")
        acc.onEvent(SpeechEvent.Partial("Listening..."))
        val result = acc.finish(autoSend = true)
        assertNull(result.autoSendText)
        // Prefix preserved when nothing accumulated
        assertEquals("draft:", result.displayText)
    }

    @Test
    fun `finish without auto-send leaves text in field`() {
        val acc = VoiceAccumulator(prefix = "")
        acc.onEvent(SpeechEvent.Final("hello"))
        val result = acc.finish(autoSend = false)
        assertNull(result.autoSendText)
        assertEquals("hello", result.displayText)
    }

    @Test
    fun `finish without auto-send preserves prefix plus accumulated`() {
        val acc = VoiceAccumulator(prefix = "note:")
        acc.onEvent(SpeechEvent.Final("hello"))
        val result = acc.finish(autoSend = false)
        assertNull(result.autoSendText)
        assertEquals("note: hello", result.displayText)
    }

    // ── Error handling ─────────────────────────────────────────────────

    @Test
    fun `Error event sets hasError flag`() {
        val acc = VoiceAccumulator(prefix = "")
        assertFalse(acc.hasError)
        val display = acc.onEvent(SpeechEvent.Error(SpeechError.Timeout("timed out")))
        assertTrue(acc.hasError)
        assertNull(display) // Error returns null — callers handle UI
    }

    @Test
    fun `Error after accumulated text preserves accumulated`() {
        val acc = VoiceAccumulator(prefix = "")
        acc.onEvent(SpeechEvent.Final("hello"))
        acc.onEvent(SpeechEvent.Error(SpeechError.EngineError("boom")))
        assertTrue(acc.hasError)
        assertEquals("hello", acc.accumulatedText)
        // Can still finish and get the text
        val result = acc.finish(autoSend = false)
        assertEquals("hello", result.displayText)
    }

    // ── Edge cases ─────────────────────────────────────────────────────

    @Test
    fun `empty Final text is trimmed away`() {
        val acc = VoiceAccumulator(prefix = "")
        acc.onEvent(SpeechEvent.Final("hello"))
        acc.onEvent(SpeechEvent.Final("  "))  // whitespace-only segment
        val display = acc.onEvent(SpeechEvent.Final("world"))
        assertEquals("hello world", display)
    }

    @Test
    fun `no events leaves prefix in finish result`() {
        val acc = VoiceAccumulator(prefix = "existing text")
        val result = acc.finish(autoSend = false)
        assertEquals("existing text", result.displayText)
        assertNull(result.autoSendText)
    }

    @Test
    fun `no events with empty prefix finishes with empty display`() {
        val acc = VoiceAccumulator(prefix = "")
        val result = acc.finish(autoSend = false)
        assertEquals("", result.displayText)
        assertNull(result.autoSendText)
    }

    @Test
    fun `no events with auto-send does not fire`() {
        val acc = VoiceAccumulator(prefix = "")
        val result = acc.finish(autoSend = true)
        assertNull(result.autoSendText)
    }
}
