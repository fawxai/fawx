package ai.citros.core

/** Configuration for budget limits. */
data class BudgetConfig(
    val enabled: Boolean = false,
    val dailyLimitUsd: Double = 0.0,
    val monthlyLimitUsd: Double = 0.0,
    val perTaskLimitUsd: Double = 0.0
)
