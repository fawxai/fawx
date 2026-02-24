package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class PolicyRolloutTelemetryTest {

    @Test
    fun `required phase1 reason-code counters are visible in snapshot`() {
        val telemetry = PolicyRolloutTelemetry()
        telemetry.recordEvaluation(PolicyEvaluation(PolicyDecision.Deny(PolicyReasonCode.DENY_FINANCIAL_SUBMIT, "no")))
        telemetry.recordEvaluation(PolicyEvaluation(PolicyDecision.Deny(PolicyReasonCode.DENY_DEGRADED_FINANCIAL_SUBMIT, "no")))
        telemetry.recordEvaluation(PolicyEvaluation(PolicyDecision.Confirm(PolicyReasonCode.CONFIRM_MISSING_APP_TARGET, "why")))
        telemetry.recordEvaluation(PolicyEvaluation(PolicyDecision.Deny(PolicyReasonCode.DENY_EGRESS_UNAPPROVED, "no")))

        val snapshot = telemetry.snapshot()
        assertEquals(1L, snapshot.denyFinancialSubmit)
        assertEquals(1L, snapshot.denyDegradedFinancialSubmit)
        assertEquals(1L, snapshot.confirmMissingAppIdentifier)
        assertEquals(1L, snapshot.denyEgressUnapproved)
    }

    @Test
    fun `agent executor keeps decision reason parity in telemetry snapshot`() = runTest {
        val decisions = listOf(
            PolicyDecision.Confirm(PolicyReasonCode.CONFIRM_MISSING_APP_TARGET, "missing"),
            PolicyDecision.Deny(PolicyReasonCode.DENY_EGRESS_UNAPPROVED, "blocked"),
            PolicyDecision.RateLimited(PolicyReasonCode.RATE_LIMIT_GLOBAL, "slow", 0)
        )
        var idx = 0
        val executor = AgentExecutor(
            delegate = FakeToolExecutionDelegate(),
            progressListener = FakeLoopProgressListener(),
            actionPolicy = object : ActionPolicy {
                override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                    return PolicyEvaluation(decisions[idx++])
                }
            }
        )

        val response = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "tap", emptyMap()),
                ToolCall("t2", "web_fetch", mapOf("url" to "https://nope.example")),
                ToolCall("t3", "think", emptyMap())
            ),
            stopReason = "tool_use"
        )
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        val snapshot = executor.policyRolloutTelemetrySnapshot()
        assertEquals(1L, snapshot.reasonCodeCounts[PolicyReasonCode.CONFIRM_MISSING_APP_TARGET])
        assertEquals(1L, snapshot.reasonCodeCounts[PolicyReasonCode.DENY_EGRESS_UNAPPROVED])
        assertEquals(1L, snapshot.reasonCodeCounts[PolicyReasonCode.RATE_LIMIT_GLOBAL])
    }

    @Test
    fun `audit fail-closed path increments audit failure telemetry deterministically`() = runTest {
        val logger = object : PolicyAuditLogger {
            override fun emit(event: PolicyAuditEvent): Result<Unit> = Result.failure(IllegalStateException("fail"))
        }
        val executor = AgentExecutor(
            delegate = FakeToolExecutionDelegate(),
            progressListener = FakeLoopProgressListener(),
            actionPolicy = object : ActionPolicy {
                override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation {
                    return PolicyEvaluation(PolicyDecision.Deny(PolicyReasonCode.DENY_PHASE1_TOOL, "blocked"))
                }
            },
            policyAuditLogger = logger
        )

        val response = ChatResponse(text = null, toolCalls = listOf(ToolCall("t1", "root_shell", emptyMap())), stopReason = "tool_use")
        executor.run(response, null, { false }) { ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn") }

        val snapshot = executor.policyRolloutTelemetrySnapshot()
        assertEquals(1L, snapshot.requiredAuditAttempts)
        assertEquals(1L, snapshot.requiredAuditFailures)
        assertEquals(1.0, snapshot.auditFailureRate)
    }

    @Test
    fun `financial submit deny rate vs non-financial excludes degraded financial denies from denominator`() {
        val telemetry = PolicyRolloutTelemetry()

        telemetry.recordEvaluation(PolicyEvaluation(PolicyDecision.Deny(PolicyReasonCode.DENY_FINANCIAL_SUBMIT, "no")))
        telemetry.recordEvaluation(PolicyEvaluation(PolicyDecision.Deny(PolicyReasonCode.DENY_DEGRADED_FINANCIAL_SUBMIT, "degraded")))
        telemetry.recordEvaluation(PolicyEvaluation(PolicyDecision.Allow, reasonCode = PolicyReasonCode.ALLOW_DEFAULT))
        telemetry.recordEvaluation(PolicyEvaluation(PolicyDecision.Deny(PolicyReasonCode.DENY_EGRESS_UNAPPROVED, "no")))

        val snapshot = telemetry.snapshot()
        assertEquals(1L, snapshot.financialSubmitDenies)
        assertEquals(2L, snapshot.nonFinancialEvaluations)
        assertEquals(0.5, snapshot.financialSubmitDenyRateVsNonFinancial)
    }
}
