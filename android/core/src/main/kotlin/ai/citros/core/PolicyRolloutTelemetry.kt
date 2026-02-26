package ai.citros.core

import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/** Phase-1 rollout telemetry counters for policy visibility and rollback gates. */
class PolicyRolloutTelemetry {
    // TODO: Add reset() if telemetry instance is ever shared across executor runs
    private val reasonCounters = ConcurrentHashMap<String, AtomicLong>()
    private val confirmationTotal = AtomicLong(0L)
    private val confirmationTimeouts = AtomicLong(0L)
    private val requiredAuditAttempts = AtomicLong(0L)
    private val requiredAuditFailures = AtomicLong(0L)
    private val nonFinancialEvaluationCount = AtomicLong(0L)
    private val financialSubmitDenyCount = AtomicLong(0L)

    @Synchronized
    fun recordEvaluation(evaluation: PolicyEvaluation) {
        val reason = evaluation.reasonCode ?: when (val d = evaluation.decision) {
            is PolicyDecision.Allow -> PolicyReasonCode.ALLOW_DEFAULT
            is PolicyDecision.Confirm -> d.reasonCode
            is PolicyDecision.Deny -> d.reasonCode
            is PolicyDecision.RateLimited -> d.reasonCode
        }
        reasonCounters.computeIfAbsent(reason) { AtomicLong(0L) }.incrementAndGet()

        when (reason) {
            PolicyReasonCode.DENY_FINANCIAL_SUBMIT -> financialSubmitDenyCount.incrementAndGet()
            // Exclude degraded financial denies from the denominator so
            // `financialSubmitDenyRateVsNonFinancial` compares strict
            // financial-submit denies against non-financial evaluations.
            PolicyReasonCode.DENY_DEGRADED_FINANCIAL_SUBMIT -> Unit
            else -> nonFinancialEvaluationCount.incrementAndGet()
        }
    }

    @Synchronized
    fun recordConfirmationOutcome(outcome: PolicyConfirmOutcome) {
        confirmationTotal.incrementAndGet()
        if (outcome == PolicyConfirmOutcome.TIMEOUT) confirmationTimeouts.incrementAndGet()
    }

    @Synchronized
    fun recordRequiredAuditEmission(success: Boolean) {
        requiredAuditAttempts.incrementAndGet()
        if (!success) requiredAuditFailures.incrementAndGet()
    }

    /**
     * Returns a consistent point-in-time view across all counters.
     *
     * Reads are synchronized with all `record*` writes so callers cannot observe
     * mixed-state snapshots from concurrent updates.
     */
    @Synchronized
    fun snapshot(): Snapshot {
        val reasons = reasonCounters.mapValues { it.value.get() }
        val confirmations = confirmationTotal.get()
        val auditAttempts = requiredAuditAttempts.get()
        val nonFinancial = nonFinancialEvaluationCount.get()
        return Snapshot(
            reasonCodeCounts = reasons,
            denyFinancialSubmit = reasons[PolicyReasonCode.DENY_FINANCIAL_SUBMIT] ?: 0L,
            denyDegradedFinancialSubmit = reasons[PolicyReasonCode.DENY_DEGRADED_FINANCIAL_SUBMIT] ?: 0L,
            confirmMissingAppIdentifier = reasons[PolicyReasonCode.CONFIRM_MISSING_APP_TARGET] ?: 0L,
            denyEgressUnapproved = reasons[PolicyReasonCode.DENY_EGRESS_UNAPPROVED] ?: 0L,
            confirmationPrompts = confirmations,
            confirmationTimeouts = confirmationTimeouts.get(),
            requiredAuditAttempts = auditAttempts,
            requiredAuditFailures = requiredAuditFailures.get(),
            nonFinancialEvaluations = nonFinancial,
            financialSubmitDenies = financialSubmitDenyCount.get()
        )
    }

    data class Snapshot(
        val reasonCodeCounts: Map<String, Long>,
        val denyFinancialSubmit: Long,
        val denyDegradedFinancialSubmit: Long,
        val confirmMissingAppIdentifier: Long,
        val denyEgressUnapproved: Long,
        val confirmationPrompts: Long,
        val confirmationTimeouts: Long,
        val requiredAuditAttempts: Long,
        val requiredAuditFailures: Long,
        val nonFinancialEvaluations: Long,
        val financialSubmitDenies: Long
    ) {
        val confirmationTimeoutRate: Double
            get() = if (confirmationPrompts == 0L) 0.0 else confirmationTimeouts.toDouble() / confirmationPrompts

        val auditFailureRate: Double
            get() = if (requiredAuditAttempts == 0L) 0.0 else requiredAuditFailures.toDouble() / requiredAuditAttempts

        /**
         * Ratio of strict financial-submit denies to non-financial evaluations.
         *
         * This is not a true false-positive rate because the denominator excludes
         * financial-submit denies and degraded-financial denies.
         * Use for rollout directional monitoring only.
         */
        val financialSubmitDenyRateVsNonFinancial: Double
            get() = if (nonFinancialEvaluations == 0L) 0.0 else financialSubmitDenies.toDouble() / nonFinancialEvaluations
    }
}
