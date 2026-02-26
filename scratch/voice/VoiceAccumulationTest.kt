package ai.citros.chat

import ai.citros.core.SpeechError
import ai.citros.core.SpeechEvent
import ai.citros.core.SpeechToTextProvider
import ai.citros.core.VoiceAccumulator
import android.content.Context
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Tests for [VoiceAccumulator] — the voice accumulation logic introduced in #637.
 *
 * These tests exercise the real [VoiceAccumulator] class used by
 * [MessageInput.beginListening], eliminating algorithm drift between
 * production code and tests.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class VoiceAccumulationTest {

    // ── Helper to run a full accumulation session ──────────────────────

    private suspend fun runSession(
        prefix: String,
        events: List<SpeechEvent>,
        autoSend: Boolean = false,
    ): SessionResult {
        val accumulator = VoiceAccumulator(prefix = prefix)
        var displayText = prefix

        val stt = scriptedProvider(events)
        stt.startListening().collect { event ->
            val display = accumulator.onEvent(event)
            if (display != null) displayText = display
        }

        val result = accumulator.finish(autoSend = autoSend)
        return SessionResult(
            displayText = if (result.autoSendText != null) result.displayText else displayText,
            autoSendText = result.autoSendText,
            hasError = accumulator.hasError,
        )
    }

    private data class SessionResult(
        val displayText: String,
        val autoSendText: String?,
        val hasError: Boolean,
    )

    // ── Single segment ─────────────────────────────────────────────────

    @Test
    fun `single Final produces text`() = runTest {
        val result = runSession(prefix = "", events = listOf(SpeechEvent.Final("hello")))
        assertEquals("hello", result.displayText)
    }

    // ── Multi-segment accumulation ─────────────────────────────────────

    @Test
    fun `multiple Finals accumulate with spaces`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(SpeechEvent.Final("hello"), SpeechEvent.Final("world")),
        )
        assertEquals("hello world", result.displayText)
    }

    @Test
    fun `three segments accumulate correctly`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(
                SpeechEvent.Final("the quick"),
                SpeechEvent.Final("brown fox"),
                SpeechEvent.Final("jumps"),
            ),
        )
        assertEquals("the quick brown fox jumps", result.displayText)
    }

    // ── Prefix preservation ────────────────────────────────────────────

    @Test
    fun `existing text is preserved as prefix`() = runTest {
        val result = runSession(
            prefix = "draft:",
            events = listOf(SpeechEvent.Final("hello")),
        )
        assertEquals("draft: hello", result.displayText)
    }

    @Test
    fun `prefix plus multi-segment accumulation`() = runTest {
        val result = runSession(
            prefix = "draft:",
            events = listOf(SpeechEvent.Final("hello"), SpeechEvent.Final("world")),
        )
        assertEquals("draft: hello world", result.displayText)
    }

    @Test
    fun `blank prefix does not add extra space`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(SpeechEvent.Final("hello")),
        )
        assertEquals("hello", result.displayText)
    }

    @Test
    fun `whitespace-only prefix treated as blank`() = runTest {
        val result = runSession(
            prefix = "   ",
            events = listOf(SpeechEvent.Final("hello")),
        )
        assertEquals("hello", result.displayText)
    }

    // ── Partial display states ─────────────────────────────────────────

    @Test
    fun `Partial with no accumulated text shows Listening`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(SpeechEvent.Partial("Listening...")),
        )
        assertEquals("Listening...", result.displayText)
    }

    @Test
    fun `Partial after Final shows accumulated with ellipsis`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(
                SpeechEvent.Final("hello"),
                SpeechEvent.Partial("Listening..."),
            ),
        )
        assertEquals("hello...", result.displayText)
    }

    @Test
    fun `Partial with prefix and no accumulated text`() = runTest {
        val result = runSession(
            prefix = "note:",
            events = listOf(SpeechEvent.Partial("Listening...")),
        )
        assertEquals("note: Listening...", result.displayText)
    }

    @Test
    fun `Partial with prefix and accumulated text`() = runTest {
        val result = runSession(
            prefix = "note:",
            events = listOf(
                SpeechEvent.Final("hello"),
                SpeechEvent.Partial("Listening..."),
            ),
        )
        assertEquals("note: hello...", result.displayText)
    }

    @Test
    fun `Final after Partial replaces indicator with real text`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(
                SpeechEvent.Partial("Listening..."),
                SpeechEvent.Final("hello"),
            ),
        )
        assertEquals("hello", result.displayText)
    }

    // ── Auto-send behavior ─────────────────────────────────────────────

    @Test
    fun `auto-send fires on natural completion with accumulated text`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(SpeechEvent.Final("hello"), SpeechEvent.Final("world")),
            autoSend = true,
        )
        assertEquals("hello world", result.autoSendText)
        assertEquals("", result.displayText)
    }

    @Test
    fun `auto-send includes prefix in final text`() = runTest {
        val result = runSession(
            prefix = "draft:",
            events = listOf(SpeechEvent.Final("hello")),
            autoSend = true,
        )
        assertEquals("draft: hello", result.autoSendText)
        assertEquals("", result.displayText)
    }

    @Test
    fun `auto-send skipped when accumulated is blank`() = runTest {
        val result = runSession(
            prefix = "draft:",
            events = listOf(SpeechEvent.Partial("Listening...")),
            autoSend = true,
        )
        assertNull(result.autoSendText)
    }

    @Test
    fun `auto-send disabled leaves text in field`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(SpeechEvent.Final("hello")),
            autoSend = false,
        )
        assertNull(result.autoSendText)
        assertEquals("hello", result.displayText)
    }

    // ── Error handling ─────────────────────────────────────────────────

    @Test
    fun `Error mid-accumulation preserves accumulated text`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(
                SpeechEvent.Final("hello"),
                SpeechEvent.Final("world"),
                SpeechEvent.Error(SpeechError.Timeout("timed out")),
            ),
        )
        // Error doesn't clear accumulated text — it stays in the field
        assertTrue(result.hasError)
        assertEquals("hello world", result.displayText)
    }

    @Test
    fun `Error mid-accumulation with auto-send still sends`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(
                SpeechEvent.Final("hello"),
                SpeechEvent.Error(SpeechError.EngineError("mic failed")),
            ),
            autoSend = true,
        )
        // Auto-send fires with whatever was accumulated before the error
        assertTrue(result.hasError)
        assertEquals("hello", result.autoSendText)
        assertEquals("", result.displayText)
    }

    @Test
    fun `Error before any Finals leaves prefix unchanged`() = runTest {
        val result = runSession(
            prefix = "draft:",
            events = listOf(SpeechEvent.Error(SpeechError.PermissionDenied("denied"))),
        )
        assertTrue(result.hasError)
        assertNull(result.autoSendText)
        assertEquals("draft:", result.displayText)
    }

    @Test
    fun `Error with no prefix and no Finals leaves empty text`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(SpeechEvent.Error(SpeechError.Unavailable("not init"))),
        )
        assertTrue(result.hasError)
        assertEquals("", result.displayText)
    }

    // ── Edge cases ─────────────────────────────────────────────────────

    @Test
    fun `empty Final text is trimmed away`() = runTest {
        val result = runSession(
            prefix = "",
            events = listOf(
                SpeechEvent.Final("hello"),
                SpeechEvent.Final("  "),
                SpeechEvent.Final("world"),
            ),
        )
        assertEquals("hello world", result.displayText)
    }

    @Test
    fun `no events leaves prefix unchanged`() = runTest {
        val result = runSession(prefix = "existing text", events = emptyList())
        assertEquals("existing text", result.displayText)
    }

    // ── VoiceAccumulator unit tests (direct) ───────────────────────────

    @Test
    fun `onEvent returns null for Error`() {
        val acc = VoiceAccumulator(prefix = "")
        val display = acc.onEvent(SpeechEvent.Error(SpeechError.Timeout("t")))
        assertNull(display)
        assertTrue(acc.hasError)
    }

    @Test
    fun `accumulatedText tracks raw segments without prefix`() {
        val acc = VoiceAccumulator(prefix = "note:")
        acc.onEvent(SpeechEvent.Final("hello"))
        acc.onEvent(SpeechEvent.Final("world"))
        assertEquals("hello world", acc.accumulatedText)
    }

    @Test
    fun `finish with no accumulated returns prefix as display`() {
        val acc = VoiceAccumulator(prefix = "draft:")
        val result = acc.finish(autoSend = false)
        assertEquals("draft:", result.displayText)
        assertNull(result.autoSendText)
    }

    @Test
    fun `hasError is false when no errors received`() {
        val acc = VoiceAccumulator(prefix = "")
        acc.onEvent(SpeechEvent.Final("hello"))
        assertFalse(acc.hasError)
    }

    // ── Helper ──────────────────────────────────────────────────────────

    private fun scriptedProvider(events: List<SpeechEvent>): SpeechToTextProvider {
        return object : SpeechToTextProvider {
            override val providerId = "test-scripted"
            override val displayName = "Scripted"
            override val requiresNetwork = false
            override val isAvailable = true
            override suspend fun initialize(context: Context) {}
            override fun startListening(): Flow<SpeechEvent> = flow {
                for (event in events) emit(event)
            }
            override fun stopListening() {}
            override fun cancel() {}
            override fun release() {}
        }
    }
}
