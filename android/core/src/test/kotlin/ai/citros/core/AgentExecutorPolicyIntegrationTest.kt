package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertTrue

class AgentExecutorPolicyIntegrationTest {
    @Test
    fun `policy evaluate exception fails secure and skips execution`() = runTest {
        val delegate = FakeToolExecutionDelegate()
        val listener = FakeLoopProgressListener()
        val policy = object : ActionPolicy {
            override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                throw IllegalStateException("boom")
            }
        }
        val executor = AgentExecutor(delegate, listener, actionPolicy = policy)
        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "tap", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        assertTrue(delegate.toolResults.any { it.second.contains("User denied") || it.second.contains("timed out") })
    }
}
