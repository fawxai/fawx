package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals

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
}
