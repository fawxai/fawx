package ai.citros.core

/**
 * Metrics from a single context compaction pass.
 * Used for logging and future telemetry integration.
 */
data class CompactionMetrics(
    /** Which compaction stage produced these metrics. */
    val stage: String,
    /** Number of messages before compaction. */
    val inputMessages: Int,
    /** Number of messages after compaction (same count, but some trimmed/summarized). */
    val outputMessages: Int,
    /** Number of messages that were trimmed or summarized. */
    val messagesCompacted: Int,
    /** Estimated tokens before compaction (chars / 3). */
    val estimatedTokensBefore: Int,
    /** Estimated tokens after compaction (chars / 3). */
    val estimatedTokensAfter: Int
) {
    /** Estimated tokens saved by this compaction pass. Clamped to non-negative. */
    val tokensSaved: Int get() = maxOf(0, estimatedTokensBefore - estimatedTokensAfter)

    /** Whether compaction actually changed anything. */
    val didCompact: Boolean get() = messagesCompacted > 0
}
