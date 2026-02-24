package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class RecoveryScaffoldTest {

    @Test
    fun `detectFailure returns tool error when result is error`() {
        val failure = detectFailure(
            toolCall = ToolCall("t1", "tap", emptyMap()),
            result = ToolResult("Failed", isError = true),
            screenBefore = null,
            screenAfter = null,
            consecutiveFailures = 0
        )

        assertNotNull(failure)
        assertEquals(FailureType.TOOL_ERROR, failure.failureType)
    }

    @Test
    fun `detectFailure returns no effect when ui tool leaves same screen hash`() {
        val before = ScreenFingerprint(structuralHash = 111, packageName = "com.app")
        val after = ScreenFingerprint(structuralHash = 111, packageName = "com.app")

        val failure = detectFailure(
            toolCall = ToolCall("t2", "tap", emptyMap()),
            result = ToolResult("ok", isError = false),
            screenBefore = before,
            screenAfter = after,
            consecutiveFailures = 1
        )

        assertNotNull(failure)
        assertEquals(FailureType.NO_EFFECT, failure.failureType)
    }

    @Test
    fun `detectFailure does not mark non ui tool as no effect when hash unchanged`() {
        val before = ScreenFingerprint(structuralHash = 111, packageName = "com.app")
        val after = ScreenFingerprint(structuralHash = 111, packageName = "com.app")

        val failure = detectFailure(
            toolCall = ToolCall("t2b", "read_screen", emptyMap()),
            result = ToolResult("ok", isError = false),
            screenBefore = before,
            screenAfter = after,
            consecutiveFailures = 1
        )

        assertNull(failure)
    }

    @Test
    fun `detectFailure returns unexpected state when app changes unexpectedly`() {
        val before = ScreenFingerprint(structuralHash = 111, packageName = "com.a")
        val after = ScreenFingerprint(structuralHash = 222, packageName = "com.b")

        val failure = detectFailure(
            toolCall = ToolCall("t3", "tap_text", mapOf("text" to "Send")),
            result = ToolResult("ok", isError = false),
            screenBefore = before,
            screenAfter = after,
            consecutiveFailures = 0
        )

        assertNotNull(failure)
        assertEquals(FailureType.UNEXPECTED_STATE, failure.failureType)
    }

    @Test
    fun `detectFailure returns null when no failure pattern applies`() {
        val before = ScreenFingerprint(structuralHash = 111, packageName = "com.a")
        val after = ScreenFingerprint(structuralHash = 222, packageName = "com.a")

        val failure = detectFailure(
            toolCall = ToolCall("t4", "tap", emptyMap()),
            result = ToolResult("ok", isError = false),
            screenBefore = before,
            screenAfter = after,
            consecutiveFailures = 0
        )

        assertNull(failure)
    }

    @Test
    fun `recovery manager emits guidance from first applicable strategy`() {
        val failure = ActionFailure(
            toolCall = ToolCall("t5", "tap_text", mapOf("text" to "Send")),
            result = ToolResult("ok", isError = false),
            screenBefore = ScreenFingerprint(1, "com.a"),
            screenAfter = ScreenFingerprint(1, "com.a"),
            consecutiveFailures = 2,
            foregroundApp = "com.a",
            failureType = FailureType.NO_EFFECT
        )

        val guidance = RecoveryManager().evaluateFailure(failure)
        assertNotNull(guidance)
        assertTrue(guidance.contains("RECOVERY (tap_recovery)"))
        assertTrue(guidance.contains("scroll") || guidance.contains("tap_text"))
    }

    @Test
    fun `recovery manager chooses app reset for repeated unexpected state`() {
        val failure = ActionFailure(
            toolCall = ToolCall("t6", "tap", emptyMap()),
            result = ToolResult("ok", isError = false),
            screenBefore = ScreenFingerprint(1, "com.a"),
            screenAfter = ScreenFingerprint(2, "com.b"),
            consecutiveFailures = 2,
            foregroundApp = "com.b",
            failureType = FailureType.UNEXPECTED_STATE
        )

        val guidance = RecoveryManager().evaluateFailure(failure)
        assertNotNull(guidance)
        assertTrue(guidance.contains("RECOVERY (app_reset_recovery)"))
        assertTrue(guidance.contains("press_home"))
    }

    @Test
    fun `recovery manager returns null when no strategy applies`() {
        val failure = ActionFailure(
            toolCall = ToolCall("t7", "read_screen", emptyMap()),
            result = ToolResult("ok", isError = false),
            screenBefore = ScreenFingerprint(1, "com.a"),
            screenAfter = ScreenFingerprint(2, "com.a"),
            consecutiveFailures = 1,
            foregroundApp = "com.a",
            failureType = FailureType.WRONG_OUTCOME
        )

        assertNull(RecoveryManager().evaluateFailure(failure))
    }

    @Test
    fun `recovery manager chooses graceful cancel at five failures`() {
        val failure = ActionFailure(
            toolCall = ToolCall("t8", "read_screen", emptyMap()),
            result = ToolResult("ok", isError = false),
            screenBefore = ScreenFingerprint(1, "com.a"),
            screenAfter = ScreenFingerprint(2, "com.a"),
            consecutiveFailures = 5,
            foregroundApp = "com.a",
            failureType = FailureType.WRONG_OUTCOME
        )

        val guidance = RecoveryManager().evaluateFailure(failure)
        assertNotNull(guidance)
        assertTrue(guidance.contains("RECOVERY (graceful_cancel)"))
        assertTrue(guidance.contains("press_home"))
    }
}
