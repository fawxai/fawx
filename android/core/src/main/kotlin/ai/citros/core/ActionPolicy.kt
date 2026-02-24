package ai.citros.core

interface ActionPolicy {
    fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation
}

data class PolicyEvaluation(
    val decision: PolicyDecision,
    val firstUseObserved: Boolean = false,
    val reasonCode: String? = null
)

data class PolicyContext(
    val foregroundApp: String? = null,
    val appIdentifier: String? = null,
    val screenContentSummary: String? = null,
    val targetNodeHints: List<String> = emptyList(),
    val recentActionCount: Int = 0,
    val taskElapsedMs: Long = 0L
)

object PermissiveActionPolicy : ActionPolicy {
    override fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation =
        PolicyEvaluation(PolicyDecision.Allow, reasonCode = PolicyReasonCode.ALLOW_PERMISSIVE_BYPASS)
}
