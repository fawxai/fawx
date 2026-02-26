package ai.citros.core

internal class InMemoryBudgetStore(
    private var config: BudgetConfig,
    private var dailySpentMicrodollars: Long = 0L,
    private var monthlySpentMicrodollars: Long = 0L,
    private var pendingNanodollars: Long = 0L
) : BudgetStore {

    override fun getConfig(): BudgetConfig = config

    override fun getDailySpentMicrodollars(): Long = dailySpentMicrodollars

    override fun getMonthlySpentMicrodollars(): Long = monthlySpentMicrodollars

    override fun getPendingNanodollars(): Long = pendingNanodollars

    @Synchronized
    override fun addSpendMicrodollars(amountMicrodollars: Long) {
        dailySpentMicrodollars += amountMicrodollars
        monthlySpentMicrodollars += amountMicrodollars
    }

    @Synchronized
    override fun setPendingNanodollars(pendingNanodollars: Long) {
        this.pendingNanodollars = pendingNanodollars
    }

    @Synchronized
    override fun resetDaily() {
        dailySpentMicrodollars = 0L
        pendingNanodollars = 0L
    }

    @Synchronized
    override fun resetMonthly() {
        monthlySpentMicrodollars = 0L
        pendingNanodollars = 0L
    }
}
