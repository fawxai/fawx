package ai.citros.core

/** Structured budget telemetry emitted by [BudgetGuard]. */
data class BudgetTelemetryEvent(
    val type: Type,
    val amountUsd: Double,
    val amountNanodollars: Long,
    val dailySpentUsd: Double,
    val dailySpentMicrodollars: Long,
    val monthlySpentUsd: Double,
    val monthlySpentMicrodollars: Long,
    val message: String? = null
) {
    enum class Type {
        ALLOWED,
        DAILY_OVER_LIMIT,
        MONTHLY_OVER_LIMIT,
        PER_TASK_LIMIT_REACHED,
        MISSING_USAGE_FALLBACK,
        PRECALL_DAILY_LIMIT_BLOCKED,
        PRECALL_MONTHLY_LIMIT_BLOCKED
    }
}
