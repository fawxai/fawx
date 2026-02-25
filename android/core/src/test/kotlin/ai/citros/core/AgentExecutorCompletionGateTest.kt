package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

class AgentExecutorCompletionGateTest {

    @Test
    fun `final loop text is gated to NOT_COMPLETED when artifacts are missing`() = runTest {
        val delegate = FakeToolExecutionDelegate().apply {
            executeResult = ToolResult("Opened Gmail compose")
        }
        val listener = FakeLoopProgressListener()
        val executor = AgentExecutor(delegate, listener)

        val initial = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "open_app", mapOf("app_name" to "Gmail"))),
            stopReason = "tool_use"
        )

        val result = executor.run(initial, null, { false }) {
            ChatResponse(text = "All done, email sent.", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        assertTrue(result.text!!.startsWith("NOT_COMPLETED:"))
        assertTrue(result.text!!.contains("email_sent"))
    }
}
