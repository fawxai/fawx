package ai.citros.core

/**
 * Persistence interface for budget state.
 * All monetary values are stored as microdollars for precision.
 *
 * Implementations must make state mutations linearizable on a single monitor so spend updates
 * and resets cannot interleave in a way that resurrects pre-reset fractional carry.
 */
interface BudgetStore {
    fun getConfig(): BudgetConfig
    fun getDailySpentMicrodollars(): Long
    fun getMonthlySpentMicrodollars(): Long
    fun getPendingNanodollars(): Long
    fun addSpendMicrodollars(amountMicrodollars: Long)
    fun setPendingNanodollars(pendingNanodollars: Long)
    /**
     * Resets daily totals and clears fractional carry so pre-reset spend cannot leak into new day.
     */
    fun resetDaily()

    /**
     * Resets monthly totals and clears fractional carry so pre-reset spend cannot leak into new month.
     */
    fun resetMonthly()
}
