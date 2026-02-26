package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertTrue

class AgentExecutorPolicyWiringTest {

    @Test
    fun `deny decision skips tool execution behaviorally`() = runTest {
        val delegate = FakeToolExecutionDelegate()
        val listener = FakeLoopProgressListener()
        val policy = object : ActionPolicy {
            override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                return PolicyEvaluation(PolicyDecision.Deny("deny.test", "blocked"))
            }
        }

        val executor = AgentExecutor(delegate, listener, actionPolicy = policy)
        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "tap", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        assertTrue(delegate.toolResults.any { it.second.contains("Action blocked by policy") })
    }
}
