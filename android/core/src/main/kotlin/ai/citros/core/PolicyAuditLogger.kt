package ai.citros.core

enum class PolicyAuditDecision { ALLOW, CONFIRM, DENY, RATE_LIMITED }
enum class PolicyConfirmOutcome { APPROVED, DENIED, TIMEOUT, NA }

data class PolicyAuditEvent(
    val eventId: String,
    val tsUtc: String,
    val taskId: String,
    val toolCallId: String,
    val toolName: String,
    val decision: PolicyAuditDecision,
    val reasonCode: String,
    val reasonText: String?,
    val foregroundApp: String?,
    val appIdentifier: String?,
    val endpointHost: String?,
    val firstUseObserved: Boolean,
    val overrideApplied: Boolean,
    val confirmOutcome: PolicyConfirmOutcome,
    val confirmationRequestId: String?
)

interface PolicyAuditLogger {
    fun emit(event: PolicyAuditEvent): Result<Unit>
}

object NoopPolicyAuditLogger : PolicyAuditLogger {
    override fun emit(event: PolicyAuditEvent): Result<Unit> = Result.success(Unit)
}
