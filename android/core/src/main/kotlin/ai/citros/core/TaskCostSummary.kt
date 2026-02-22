package ai.citros.core

/** Summary of estimated token and cost usage for a single task. */
data class TaskCostSummary(
    val totalTokens: Long,
    val inputTokens: Long,
    val outputTokens: Long,
    val apiCalls: Int,
    val estimatedCostUsd: Double
) {
    companion object {
        val EMPTY = TaskCostSummary(
            totalTokens = 0L,
            inputTokens = 0L,
            outputTokens = 0L,
            apiCalls = 0,
            estimatedCostUsd = 0.0
        )
    }
}
