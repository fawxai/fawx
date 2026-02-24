package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class AgentExecutorPolicyIntegrationTest {
    @Test
    fun `policy evaluate exception fails secure and skips execution`() = runTest {
        val delegate = FakeToolExecutionDelegate()
        var executions = 0
        delegate.onExecute = { _, _ ->
            executions++
            ToolResult("should-not-run")
        }
        val listener = FakeLoopProgressListener()
        val policy = object : ActionPolicy {
            override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                throw IllegalStateException("boom")
            }
        }
        val executor = AgentExecutor(delegate, listener, actionPolicy = policy)
        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "tap", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        assertEquals(0, executions)
        assertTrue(delegate.toolResults.any { it.second.contains("Action blocked by policy") && it.second.contains("fail-closed") })
    }
}
