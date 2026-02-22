package ai.citros.core

/** Structured budget failure categories for UI and telemetry consumers. */
enum class BudgetErrorCode {
    DAILY_LIMIT,
    MONTHLY_LIMIT,
    PER_TASK_LIMIT,
    MISSING_USAGE_FALLBACK
}
