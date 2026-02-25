package ai.citros.core

import android.graphics.Rect
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class SideEffectCompletionGuardTest {

    @Test
    fun `guardFinalText leaves non-claims unchanged`() {
        val guard = SideEffectCompletionGuard()
        val text = "Here are the latest inbox messages."

        assertEquals(text, guard.guardFinalText(text))
    }

    @Test
    fun `recordExecution allows email claim after ui mutating evidence`() {
        val guard = SideEffectCompletionGuard()

        guard.recordExecution(
            toolCall = ToolCall("t1", "tap_text", mapOf("text" to "Send")),
            actionResult = ToolResult("Tapped \"Send\""),
            screenBefore = screen("com.google.android.gm"),
            screenAfter = screen("com.google.android.gm"),
            isUiMutatingTool = true
        )

        val finalText = "I sent the email to Sam."
        assertEquals(finalText, guard.guardFinalText(finalText))
    }

    @Test
    fun `recordExecution ignores passive non-mutating outputs as side-effect evidence`() {
        val guard = SideEffectCompletionGuard()

        guard.recordExecution(
            toolCall = ToolCall("t1", "read_screen", emptyMap()),
            actionResult = ToolResult("SCREEN: Gmail Sent folder (42)."),
            screenBefore = screen("com.google.android.gm"),
            screenAfter = screen("com.google.android.gm"),
            isUiMutatingTool = false
        )

        val rewritten = guard.guardFinalText("I sent the email to Sam.").orEmpty()
        assertTrue(rewritten.startsWith("NOT_COMPLETED:"))
        assertTrue(rewritten.contains("email sending"))
    }

    @Test
    fun `booking and scheduling claims are allowed with direct evidence`() {
        val guard = SideEffectCompletionGuard()

        guard.recordExecution(
            toolCall = ToolCall("t1", "tap_text", mapOf("text" to "Book now")),
            actionResult = ToolResult("Tapped \"Book now\""),
            screenBefore = null,
            screenAfter = null,
            isUiMutatingTool = true
        )
        guard.recordExecution(
            toolCall = ToolCall("t2", "tap_text", mapOf("text" to "Schedule")),
            actionResult = ToolResult("Tapped \"Schedule\""),
            screenBefore = null,
            screenAfter = null,
            isUiMutatingTool = true
        )

        val finalText = "I booked the flight and scheduled the meeting."
        assertEquals(finalText, guard.guardFinalText(finalText))
    }

    @Test
    fun `mixed multi-claim text rewrites only unsupported claim and preserves safe sentence`() {
        val guard = SideEffectCompletionGuard()

        guard.recordExecution(
            toolCall = ToolCall("t1", "tap_text", mapOf("text" to "Send")),
            actionResult = ToolResult("Tapped \"Send\""),
            screenBefore = null,
            screenAfter = null,
            isUiMutatingTool = true
        )

        val rewritten = guard.guardFinalText(
            "I sent the email to Sam. I booked the flight to Denver."
        ).orEmpty()

        assertTrue(rewritten.startsWith("NOT_COMPLETED:"))
        assertTrue(rewritten.contains("booking"))
        assertFalse(rewritten.contains("email sending"))
        assertTrue(rewritten.contains("I sent the email to Sam."))
        assertFalse(rewritten.contains("I booked the flight to Denver."))
    }

    @Test
    fun `negated side-effect language does not trigger claim detection`() {
        val guard = SideEffectCompletionGuard()
        val text = "I have not sent the email yet, and the booking is not confirmed."

        assertEquals(text, guard.guardFinalText(text))
    }

    @Test
    fun `unknown package containing mail substring is not treated as email app`() {
        val guard = SideEffectCompletionGuard()

        guard.recordExecution(
            toolCall = ToolCall("t1", "submit_form", emptyMap()),
            actionResult = ToolResult("Queued"),
            screenBefore = screen("com.example.mailroom"),
            screenAfter = screen("com.example.mailroom"),
            isUiMutatingTool = true
        )

        val rewritten = guard.guardFinalText("I sent the email to Sam.").orEmpty()
        assertTrue(rewritten.startsWith("NOT_COMPLETED:"))
    }

    @Test
    fun `known email package allowlist can support conservative fallback evidence`() {
        val guard = SideEffectCompletionGuard()

        guard.recordExecution(
            toolCall = ToolCall("t1", "submit_form", emptyMap()),
            actionResult = ToolResult("Queued"),
            screenBefore = screen("com.ninefolders.hd3"),
            screenAfter = screen("com.ninefolders.hd3"),
            isUiMutatingTool = true
        )

        val finalText = "I sent the email to Sam."
        assertEquals(finalText, guard.guardFinalText(finalText))
    }

    @Test
    fun `rewrite preserves safe diagnostics and strips unsupported success sentence`() {
        val guard = SideEffectCompletionGuard()

        val rewritten = guard.guardFinalText(
            "I tried to send the email but the compose window crashed. I sent the email to Sam."
        ).orEmpty()

        assertTrue(rewritten.startsWith("NOT_COMPLETED:"))
        assertTrue(rewritten.contains("compose window crashed"))
        assertFalse(rewritten.contains("I sent the email to Sam."))
    }

    @Test
    fun `guard emits telemetry callback when rewrite occurs`() {
        val events = mutableListOf<SideEffectCompletionGuard.GuardFireTelemetry>()
        val guard = SideEffectCompletionGuard(onGuardTriggered = { events += it })

        val rewritten = guard.guardFinalText("I sent the email to Sam.").orEmpty()

        assertTrue(rewritten.startsWith("NOT_COMPLETED:"))
        assertEquals(1, events.size)
        val event = events.single()
        assertTrue(event.missingClaims.contains("EMAIL_SEND"))
        assertTrue(event.evidencedClaims.isEmpty())
        assertTrue(event.originalTextPreview.contains("I sent the email to Sam."))
    }

    private fun screen(packageName: String?): ScreenContent =
        ScreenContent(
            elements = listOf(
                ScreenElement(
                    id = 1,
                    text = "Send",
                    contentDescription = null,
                    className = "android.widget.Button",
                    isClickable = true,
                    isEditable = false,
                    bounds = Rect(0, 0, 100, 50)
                )
            ),
            packageName = packageName
        )
}
