package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class AgentExecutorPolicyEnforcementTest {

    @Test
    fun `confirm approved executes tool`() = runTest {
        var executeCount = 0
        val delegate = object : FakeToolExecutionDelegate() {
            override suspend fun requestUserConfirmation(
                toolCall: ToolCall,
                requestId: String,
                reason: String,
                timeoutMs: Long,
                reasonCode: String?
            ): Boolean = true
        }
        delegate.onExecute = { _, _ ->
            executeCount++
            ToolResult("ok")
        }

        val executor = AgentExecutor(
            delegate = delegate,
            progressListener = FakeLoopProgressListener(),
            actionPolicy = object : ActionPolicy {
                override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                    return PolicyEvaluation(PolicyDecision.Confirm("confirm.test", "needs approval"))
                }
            }
        )

        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "tap", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        assertEquals(1, executeCount)
    }

    @Test
    fun `confirm denied skips tool execution`() = runTest {
        var executeCount = 0
        val delegate = object : FakeToolExecutionDelegate() {
            override suspend fun requestUserConfirmation(
                toolCall: ToolCall,
                requestId: String,
                reason: String,
                timeoutMs: Long,
                reasonCode: String?
            ): Boolean = false
        }
        delegate.onExecute = { _, _ ->
            executeCount++
            ToolResult("ok")
        }

        val executor = AgentExecutor(
            delegate = delegate,
            progressListener = FakeLoopProgressListener(),
            actionPolicy = object : ActionPolicy {
                override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                    return PolicyEvaluation(PolicyDecision.Confirm("confirm.test", "needs approval"))
                }
            }
        )

        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "tap", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        assertEquals(0, executeCount)
        assertTrue(delegate.toolResults.any { it.second.contains("User denied") })
    }

    @Test
    fun `rate limited skips execution and records reason`() = runTest {
        var executeCount = 0
        val delegate = FakeToolExecutionDelegate()
        delegate.onExecute = { _, _ ->
            executeCount++
            ToolResult("ok")
        }

        val executor = AgentExecutor(
            delegate = delegate,
            progressListener = FakeLoopProgressListener(),
            actionPolicy = object : ActionPolicy {
                override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                    return PolicyEvaluation(PolicyDecision.RateLimited("rate.test", "slow down", cooldownMs = 0))
                }
            }
        )

        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "tap", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        assertEquals(0, executeCount)
        assertTrue(delegate.toolResults.any { it.second.contains("slow down") })
    }
}
