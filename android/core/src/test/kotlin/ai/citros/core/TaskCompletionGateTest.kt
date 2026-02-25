package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class TaskCompletionGateTest {

    @Test
    fun `full artifact pass keeps completed response`() {
        val gate = TaskCompletionGate()
        gate.recordExecution(
            toolName = "tap",
            toolInput = mapOf("target" to "Send email"),
            resultText = "Email sent successfully from Gmail",
            isError = false,
            isUiMutatingTool = true
        )

        assertEquals("Done — email sent.", gate.guardFinalText("Done — email sent."))
    }

    @Test
    fun `partial artifact fail returns deterministic NOT_COMPLETED with missing list`() {
        val gate = TaskCompletionGate()
        gate.recordExecution(
            toolName = "open_app",
            toolInput = mapOf("app_name" to "Gmail"),
            resultText = "Opened Gmail compose",
            isError = false,
            isUiMutatingTool = true
        )

        assertEquals(
            "NOT_COMPLETED: Missing required artifacts: email_sent.",
            gate.guardFinalText("Great, email sent.")
        )
    }

    @Test
    fun `mixed side effects fail when only subset artifacts are present`() {
        val gate = TaskCompletionGate()
        gate.recordExecution(
            toolName = "tap",
            toolInput = mapOf("target" to "Send"),
            resultText = "Email sent successfully",
            isError = false,
            isUiMutatingTool = true
        )

        val guarded = gate.guardFinalText("I sent the email and booking confirmed.")
        assertTrue(guarded!!.startsWith("NOT_COMPLETED:"))
        assertTrue(guarded.contains("booking_confirmed"))
    }

    @Test
    fun `generic scheduled wording does not trigger calendar contract`() {
        val gate = TaskCompletionGate()
        assertEquals(
            "Your reminder is scheduled with the provider.",
            gate.guardFinalText("Your reminder is scheduled with the provider.")
        )
    }

    @Test
    fun `passive booking text without ui action does not satisfy booking artifact`() {
        val gate = TaskCompletionGate()
        gate.recordExecution(
            toolName = "read_screen",
            toolInput = emptyMap(),
            resultText = "It says booking confirmed",
            isError = false,
            isUiMutatingTool = false
        )

        val guarded = gate.guardFinalText("Booking confirmed.")
        assertTrue(guarded!!.startsWith("NOT_COMPLETED:"))
        assertTrue(guarded.contains("booking_confirmed"))
    }
}
