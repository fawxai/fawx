package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class AgentExecutorPolicyAuditTest {

    @Test
    fun `allow decisions are audited when opt in enabled`() = runTest {
        val delegate = FakeToolExecutionDelegate()
        val listener = FakeLoopProgressListener()
        val events = mutableListOf<PolicyAuditEvent>()
        val logger = object : PolicyAuditLogger {
            override fun emit(event: PolicyAuditEvent): Result<Unit> {
                events += event
                return Result.success(Unit)
            }
        }

        val executor = AgentExecutor(
            delegate = delegate,
            progressListener = listener,
            actionPolicy = object : ActionPolicy {
                override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                    return PolicyEvaluation(PolicyDecision.Allow, reasonCode = PolicyReasonCode.ALLOW_EGRESS_ALLOWLISTED)
                }
            },
            policyAuditLogger = logger,
            auditAllowDecisions = true
        )

        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "think", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        assertEquals(1, events.size)
        assertEquals(PolicyAuditDecision.ALLOW, events.first().decision)
        assertEquals(PolicyReasonCode.ALLOW_EGRESS_ALLOWLISTED, events.first().reasonCode)
    }

    @Test
    fun `audit emission failure blocks allow-path execution fail closed`() = runTest {
        var executeCount = 0
        val delegate = FakeToolExecutionDelegate().also { d ->
            d.onExecute = { _, _ ->
                executeCount++
                ToolResult("should not execute")
            }
        }
        val logger = object : PolicyAuditLogger {
            override fun emit(event: PolicyAuditEvent): Result<Unit> = Result.failure(IllegalStateException("disk full"))
        }

        val executor = AgentExecutor(
            delegate = delegate,
            progressListener = FakeLoopProgressListener(),
            actionPolicy = object : ActionPolicy {
                override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                    return PolicyEvaluation(PolicyDecision.Allow, reasonCode = PolicyReasonCode.ALLOW_DEFAULT)
                }
            },
            policyAuditLogger = logger,
            auditAllowDecisions = true
        )

        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "tap", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        assertEquals(0, executeCount)
        assertTrue(delegate.toolResults.any { it.second.contains("Policy audit write failed") })
    }

    @Test
    fun `policy evaluation exception is denied fail closed`() = runTest {
        var executeCount = 0
        val delegate = FakeToolExecutionDelegate().also { d ->
            d.onExecute = { _, _ ->
                executeCount++
                ToolResult("should not execute")
            }
        }

        val executor = AgentExecutor(
            delegate = delegate,
            progressListener = FakeLoopProgressListener(),
            actionPolicy = object : ActionPolicy {
                override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                    throw IllegalStateException("boom")
                }
            }
        )

        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "tap", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        assertEquals(0, executeCount)
        assertTrue(delegate.toolResults.any { it.second.contains("Policy evaluation failed; action blocked") })
    }
}
