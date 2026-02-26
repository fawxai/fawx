package ai.citros.core

/**
 * Accumulates token usage across all API calls in a single task.
 */
class TaskTokenAccumulator {
    private var totalInputTokensCounter: Long = 0L
    private var totalOutputTokensCounter: Long = 0L
    private var totalCacheReadTokensCounter: Long = 0L
    private var totalCacheWriteTokensCounter: Long = 0L
    private var callCountCounter: Int = 0

    @Synchronized
    fun record(usage: TokenUsage) {
        totalInputTokensCounter += usage.inputTokens.toLong()
        totalOutputTokensCounter += usage.outputTokens.toLong()
        totalCacheReadTokensCounter += usage.cacheReadTokens.toLong()
        totalCacheWriteTokensCounter += usage.cacheWriteTokens.toLong()
        callCountCounter += 1
    }

    val totalTokens: Long
        @Synchronized get() =
            totalInputTokensCounter + totalOutputTokensCounter + totalCacheReadTokensCounter + totalCacheWriteTokensCounter

    val totalInputTokens: Long
        @Synchronized get() = totalInputTokensCounter

    val totalOutputTokens: Long
        @Synchronized get() = totalOutputTokensCounter

    val totalCacheReadTokens: Long
        @Synchronized get() = totalCacheReadTokensCounter

    val totalCacheWriteTokens: Long
        @Synchronized get() = totalCacheWriteTokensCounter

    val callCount: Int
        @Synchronized get() = callCountCounter

    /**
     * Returns a compact aggregate usage snapshot.
     * One aggregated row avoids unbounded per-call memory growth in long tasks.
     */
    @Synchronized
    fun snapshot(): List<TokenUsage> {
        if (callCountCounter == 0) return emptyList()
        return listOf(
            TokenUsage(
                inputTokens = totalInputTokensCounter.toIntSaturated(),
                outputTokens = totalOutputTokensCounter.toIntSaturated(),
                cacheReadTokens = totalCacheReadTokensCounter.toIntSaturated(),
                cacheWriteTokens = totalCacheWriteTokensCounter.toIntSaturated()
            )
        )
    }

    @Synchronized
    fun reset() {
        totalInputTokensCounter = 0L
        totalOutputTokensCounter = 0L
        totalCacheReadTokensCounter = 0L
        totalCacheWriteTokensCounter = 0L
        callCountCounter = 0
    }

    private fun Long.toIntSaturated(): Int = coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
}
