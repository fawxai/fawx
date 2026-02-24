package ai.citros.core

/** Result of evaluating a tool call against the action policy. */
sealed class PolicyDecision {
    object Allow : PolicyDecision()

    data class Confirm(
        val reasonCode: String,
        val reason: String
    ) : PolicyDecision()

    data class Deny(
        val reasonCode: String,
        val reason: String
    ) : PolicyDecision()

    data class RateLimited(
        val reasonCode: String,
        val reason: String,
        val cooldownMs: Long = 5_000L
    ) : PolicyDecision()
}

/** Canonical reason codes for policy decisions and tests. */
object PolicyReasonCode {
    fun defaultConfirmForTool(toolName: String): String = "confirm.default.$toolName"

    const val CONFIRM_UNKNOWN_TOOL = "confirm.unknown_tool"
    const val CONFIRM_SENSITIVE_APP = "confirm.sensitive_app_interaction"
    const val CONFIRM_DEGRADED_SENSITIVE = "confirm.degraded_context_sensitive"
    const val CONFIRM_FIRST_USE_APP = "confirm.first_use_app"
    const val CONFIRM_MISSING_APP_TARGET = "confirm.missing_app_identifier"
    const val CONFIRM_POLICY_EVAL_EXCEPTION = "confirm.policy_eval_exception"
    const val CONFIRM_USER_OVERRIDE = "confirm.user_override"

    const val DENY_PHASE1_TOOL = "deny.phase1.tool"
    const val DENY_EGRESS_UNAPPROVED = "deny.egress.unapproved"
    const val DENY_FINANCIAL_SUBMIT = "deny.financial_submit"
    const val DENY_DEGRADED_FINANCIAL_SUBMIT = "deny.degraded_context_financial_submit"
    const val DENY_USER_OVERRIDE = "deny.user_override"

    const val RATE_LIMIT_GLOBAL = "rate_limit.global_attempts"
    const val RATE_LIMIT_MESSAGES = "rate_limit.messaging_attempts"
}
