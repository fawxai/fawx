package ai.citros.core

interface ActionPolicy {
    fun evaluate(toolCall: ToolCall, context: PolicyContext): PolicyEvaluation
}

data class PolicyEvaluation(
    val decision: PolicyDecision,
    val firstUseObserved: Boolean = false
)

data class PolicyContext(
    val foregroundApp: String? = null,
    val appIdentifier: String? = null,
    val screenContentSummary: String? = null,
    val targetNodeHints: List<String> = emptyList(),
    val recentActionCount: Int = 0,
    val taskElapsedMs: Long = 0L
)
