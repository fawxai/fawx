package ai.citros.core

/** Typed outcome of a budget transition. */
sealed interface BudgetDecision {
    /** Spend accepted and recorded. */
    object Allowed : BudgetDecision

    /** Spend was recorded, but one or more limits are now exceeded. */
    data class OverLimit(
        val message: String,
        val code: BudgetErrorCode
    ) : BudgetDecision

    /**
     * Usage metadata was missing, so fallback cost was applied.
     * [overLimitMessage]/[overLimitCode] are non-null when fallback pushed spending over budget.
     */
    data class MissingUsageMetadata(
        val message: String,
        val fallbackCostUsd: Double,
        val overLimitMessage: String? = null,
        val overLimitCode: BudgetErrorCode? = null
    ) : BudgetDecision
}
