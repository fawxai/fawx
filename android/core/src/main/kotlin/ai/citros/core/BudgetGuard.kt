package ai.citros.core

import android.util.Log
import java.util.Locale
import kotlin.math.roundToLong

/** Enforces user-configurable spending limits. */
class BudgetGuard(
    private val store: BudgetStore,
    private val onTelemetry: ((BudgetTelemetryEvent) -> Unit)? = null
) {
    companion object {
        private const val TAG = "CitrosBudget"
        private const val MICRODOLLARS_PER_USD = 1_000_000L
        private const val NANODOLLARS_PER_USD = 1_000_000_000L
        private const val NANODOLLARS_PER_MICRODOLLAR = 1_000L
    }

    /**
     * Atomically records spend, then reports whether limits were exceeded.
     * Returns null when within limits, or an error message when blocked.
     */
    fun trySpend(amountUsd: Double): String? {
        return when (val decision = trySpendDecision(amountUsd)) {
            BudgetDecision.Allowed -> null
            is BudgetDecision.OverLimit -> decision.message
            is BudgetDecision.MissingUsageMetadata -> decision.overLimitMessage
        }
    }

    /**
     * Atomically records spend, then reports typed budget state.
     */
    fun trySpendDecision(amountUsd: Double): BudgetDecision {
        return trySpendDecisionInternal(amountUsd, emitTelemetry = true)
    }

    private fun trySpendDecisionInternal(
        amountUsd: Double,
        emitTelemetry: Boolean
    ): BudgetDecision {
        require(amountUsd >= 0.0 && amountUsd.isFinite()) { "amountUsd must be finite and non-negative" }

        val result: Pair<BudgetDecision, BudgetTelemetryEvent?> = synchronized(store) {
            val config = store.getConfig()
            val amountMicrodollars = usdToMicrodollarsWithCarry(amountUsd)

            if (!config.enabled) {
                store.addSpendMicrodollars(amountMicrodollars)
                val updatedDailySpent = store.getDailySpentMicrodollars()
                val updatedMonthlySpent = store.getMonthlySpentMicrodollars()
                Log.d(TAG, "Budget disabled; recorded spend usd=${formatUsd(amountUsd)}")
                return@synchronized Pair(
                    BudgetDecision.Allowed,
                    buildTelemetryEvent(
                    type = BudgetTelemetryEvent.Type.ALLOWED,
                    amountUsd = amountUsd,
                    dailySpentMicrodollars = updatedDailySpent,
                    monthlySpentMicrodollars = updatedMonthlySpent
                )
                )
            }

            val dailySpent = store.getDailySpentMicrodollars()
            val monthlySpent = store.getMonthlySpentMicrodollars()
            store.addSpendMicrodollars(amountMicrodollars)
            val updatedDailySpent = dailySpent + amountMicrodollars
            val updatedMonthlySpent = monthlySpent + amountMicrodollars

            val dailyLimit = usdToMicrodollars(config.dailyLimitUsd)
            if (dailyLimit > 0 && updatedDailySpent > dailyLimit) {
                val message = "Daily budget of \$${formatUsdForBudgetMessage(config.dailyLimitUsd)} exceeded (\$${formatUsdForBudgetMessage(microdollarsToUsd(updatedDailySpent))} used). Reset at midnight."
                Log.w(TAG, "Budget exceeded (daily): $message")
                return@synchronized Pair(
                    BudgetDecision.OverLimit(message, BudgetErrorCode.DAILY_LIMIT),
                    buildTelemetryEvent(
                    type = BudgetTelemetryEvent.Type.DAILY_OVER_LIMIT,
                    amountUsd = amountUsd,
                    dailySpentMicrodollars = updatedDailySpent,
                    monthlySpentMicrodollars = updatedMonthlySpent,
                    message = message
                )
                )
            }

            val monthlyLimit = usdToMicrodollars(config.monthlyLimitUsd)
            if (monthlyLimit > 0 && updatedMonthlySpent > monthlyLimit) {
                val message = "Monthly budget of \$${formatUsdForBudgetMessage(config.monthlyLimitUsd)} exceeded (\$${formatUsdForBudgetMessage(microdollarsToUsd(updatedMonthlySpent))} used). Reset on the 1st."
                Log.w(TAG, "Budget exceeded (monthly): $message")
                return@synchronized Pair(
                    BudgetDecision.OverLimit(message, BudgetErrorCode.MONTHLY_LIMIT),
                    buildTelemetryEvent(
                    type = BudgetTelemetryEvent.Type.MONTHLY_OVER_LIMIT,
                    amountUsd = amountUsd,
                    dailySpentMicrodollars = updatedDailySpent,
                    monthlySpentMicrodollars = updatedMonthlySpent,
                    message = message
                )
                )
            }

            Log.d(
                TAG,
                "Recorded spend usd=${formatUsd(amountUsd)} dailyUsd=${formatUsd(microdollarsToUsd(updatedDailySpent))} monthlyUsd=${formatUsd(microdollarsToUsd(updatedMonthlySpent))}"
            )
            Pair(
                BudgetDecision.Allowed,
                buildTelemetryEvent(
                    type = BudgetTelemetryEvent.Type.ALLOWED,
                    amountUsd = amountUsd,
                    dailySpentMicrodollars = updatedDailySpent,
                    monthlySpentMicrodollars = updatedMonthlySpent
                )
            )
        }
        if (emitTelemetry) {
            result.second?.let { event -> onTelemetry?.invoke(event) }
        }
        return result.first
    }

    /**
     * Records a conservative fallback spend when provider usage metadata is missing.
     */
    fun recordFallbackSpendForMissingUsage(fallbackCostUsd: Double): BudgetDecision.MissingUsageMetadata {
        require(fallbackCostUsd >= 0.0 && fallbackCostUsd.isFinite()) {
            "fallbackCostUsd must be finite and non-negative"
        }

        val fallbackDisplay = formatUsd(fallbackCostUsd, 6)
        val baseMessage =
            "Usage metadata missing; applied conservative fallback estimate of \$$fallbackDisplay."
        val spendDecision = trySpendDecisionInternal(fallbackCostUsd, emitTelemetry = false)
        val fallbackResult = when (spendDecision) {
            BudgetDecision.Allowed -> {
                Log.w(TAG, baseMessage)
                BudgetDecision.MissingUsageMetadata(
                    message = baseMessage,
                    fallbackCostUsd = fallbackCostUsd
                )
            }
            is BudgetDecision.OverLimit -> {
                val combined = "${spendDecision.message} $baseMessage"
                Log.w(TAG, combined)
                BudgetDecision.MissingUsageMetadata(
                    message = combined,
                    fallbackCostUsd = fallbackCostUsd,
                    overLimitMessage = spendDecision.message,
                    overLimitCode = spendDecision.code
                )
            }
            is BudgetDecision.MissingUsageMetadata -> spendDecision
        }
        val telemetrySnapshot = synchronized(store) {
            buildTelemetryEvent(
                type = BudgetTelemetryEvent.Type.MISSING_USAGE_FALLBACK,
                amountUsd = fallbackCostUsd,
                dailySpentMicrodollars = store.getDailySpentMicrodollars(),
                monthlySpentMicrodollars = store.getMonthlySpentMicrodollars(),
                message = fallbackResult.message
            )
        }
        onTelemetry?.invoke(telemetrySnapshot)
        return fallbackResult
    }

    /**
     * Checks cumulative task spending against per-task cap.
     * Returns null when allowed, or an error message when blocked.
     */
    fun checkTaskLimit(taskSpentSoFar: Double): String? {
        return checkTaskLimitDecision(taskSpentSoFar)?.message
    }

    /**
     * Checks cumulative task spending against per-task cap.
     * Returns structured error when blocked.
     */
    fun checkTaskLimitDecision(taskSpentSoFar: Double): BudgetLimitViolation? {
        return checkTaskLimitDecisionMicrodollars(usdToMicrodollars(taskSpentSoFar))
    }

    /**
     * Checks cumulative task spending against per-task cap using integer microdollars.
     * Returns null when allowed, or an error message when blocked.
     */
    fun checkTaskLimitMicrodollars(taskSpentSoFarMicrodollars: Long): String? {
        return checkTaskLimitDecisionMicrodollars(taskSpentSoFarMicrodollars)?.message
    }

    /**
     * Checks cumulative task spending against per-task cap using integer microdollars.
     * Returns structured error when blocked.
     */
    fun checkTaskLimitDecisionMicrodollars(taskSpentSoFarMicrodollars: Long): BudgetLimitViolation? {
        require(taskSpentSoFarMicrodollars >= 0L) { "taskSpentSoFarMicrodollars must be non-negative" }
        val result: Pair<BudgetLimitViolation?, BudgetTelemetryEvent?> = synchronized(store) {
            val config = store.getConfig()
            if (!config.enabled || config.perTaskLimitUsd <= 0.0) return@synchronized Pair(null, null)

            val perTaskLimitMicrodollars = usdToMicrodollars(config.perTaskLimitUsd)
            if (perTaskLimitMicrodollars > 0 && taskSpentSoFarMicrodollars >= perTaskLimitMicrodollars) {
                val message = "Per-task budget of \$${formatUsdForBudgetMessage(config.perTaskLimitUsd)} reached (\$${formatUsdForBudgetMessage(microdollarsToUsd(taskSpentSoFarMicrodollars))} used)."
                Log.w(TAG, "Per-task budget exceeded: $message")
                return@synchronized Pair(
                    BudgetLimitViolation(message = message, code = BudgetErrorCode.PER_TASK_LIMIT),
                    buildTelemetryEvent(
                        type = BudgetTelemetryEvent.Type.PER_TASK_LIMIT_REACHED,
                        amountUsd = 0.0,
                        dailySpentMicrodollars = store.getDailySpentMicrodollars(),
                        monthlySpentMicrodollars = store.getMonthlySpentMicrodollars(),
                        message = message
                    )
                )
            }
            Pair(null, null)
        }
        result.second?.let { event -> onTelemetry?.invoke(event) }
        return result.first
    }

    /**
     * Read-only budget precheck used for pre-call gating.
     * Returns null when within limits, or an over-limit error message.
     */
    fun checkWouldExceedBudgetWithoutSpending(): String? {
        return checkWouldExceedBudgetWithoutSpendingDecision()?.message
    }

    /**
     * Read-only budget precheck used for pre-call gating.
     * Returns structured error when blocked.
     */
    fun checkWouldExceedBudgetWithoutSpendingDecision(): BudgetLimitViolation? {
        val result: Pair<BudgetLimitViolation?, BudgetTelemetryEvent?> = synchronized(store) {
            val config = store.getConfig()
            if (!config.enabled) return@synchronized Pair(null, null)

            val dailySpent = store.getDailySpentMicrodollars()
            val dailyLimit = usdToMicrodollars(config.dailyLimitUsd)
            if (dailyLimit > 0 && dailySpent >= dailyLimit) {
                val message = "Daily budget of \$${formatUsdForBudgetMessage(config.dailyLimitUsd)} would be exceeded (\$${formatUsdForBudgetMessage(microdollarsToUsd(dailySpent))} used). Reset at midnight."
                return@synchronized Pair(
                    BudgetLimitViolation(message = message, code = BudgetErrorCode.DAILY_LIMIT),
                    buildTelemetryEvent(
                        type = BudgetTelemetryEvent.Type.PRECALL_DAILY_LIMIT_BLOCKED,
                        amountUsd = 0.0,
                        dailySpentMicrodollars = dailySpent,
                        monthlySpentMicrodollars = store.getMonthlySpentMicrodollars(),
                        message = message
                    )
                )
            }

            val monthlySpent = store.getMonthlySpentMicrodollars()
            val monthlyLimit = usdToMicrodollars(config.monthlyLimitUsd)
            if (monthlyLimit > 0 && monthlySpent >= monthlyLimit) {
                val message = "Monthly budget of \$${formatUsdForBudgetMessage(config.monthlyLimitUsd)} would be exceeded (\$${formatUsdForBudgetMessage(microdollarsToUsd(monthlySpent))} used). Reset on the 1st."
                return@synchronized Pair(
                    BudgetLimitViolation(message = message, code = BudgetErrorCode.MONTHLY_LIMIT),
                    buildTelemetryEvent(
                        type = BudgetTelemetryEvent.Type.PRECALL_MONTHLY_LIMIT_BLOCKED,
                        amountUsd = 0.0,
                        dailySpentMicrodollars = store.getDailySpentMicrodollars(),
                        monthlySpentMicrodollars = monthlySpent,
                        message = message
                    )
                )
            }
            Pair(null, null)
        }
        result.second?.let { event -> onTelemetry?.invoke(event) }
        return result.first
    }

    private fun usdToMicrodollarsWithCarry(usd: Double): Long {
        val priorPendingNanodollars = store.getPendingNanodollars()
        val amountNanodollars = usdToNanodollars(usd)
        val totalNanodollars = priorPendingNanodollars + amountNanodollars
        val spendMicrodollars = totalNanodollars / NANODOLLARS_PER_MICRODOLLAR
        val pendingNanodollars = totalNanodollars % NANODOLLARS_PER_MICRODOLLAR
        store.setPendingNanodollars(pendingNanodollars)
        return spendMicrodollars
    }

    private fun usdToNanodollars(usd: Double): Long = (usd * NANODOLLARS_PER_USD).roundToLong()

    private fun usdToMicrodollars(usd: Double): Long = (usd * MICRODOLLARS_PER_USD).roundToLong()

    private fun microdollarsToUsd(microdollars: Long): Double = microdollars / MICRODOLLARS_PER_USD.toDouble()

    private fun buildTelemetryEvent(
        type: BudgetTelemetryEvent.Type,
        amountUsd: Double,
        dailySpentMicrodollars: Long,
        monthlySpentMicrodollars: Long,
        message: String? = null
    ): BudgetTelemetryEvent =
        BudgetTelemetryEvent(
            type = type,
            amountUsd = amountUsd,
            amountNanodollars = usdToNanodollars(amountUsd),
            dailySpentUsd = microdollarsToUsd(dailySpentMicrodollars),
            dailySpentMicrodollars = dailySpentMicrodollars,
            monthlySpentUsd = microdollarsToUsd(monthlySpentMicrodollars),
            monthlySpentMicrodollars = monthlySpentMicrodollars,
            message = message
        )

    private fun formatUsdForBudgetMessage(value: Double): String {
        val abs = kotlin.math.abs(value)
        val decimals = when {
            abs >= 0.01 -> 2
            abs >= 0.0001 -> 4
            else -> 6
        }
        return formatUsd(value, decimals)
    }

    private fun formatUsd(value: Double, decimals: Int = 2): String =
        String.format(Locale.US, "%1$.${decimals}f", value)
}

data class BudgetLimitViolation(
    val message: String,
    val code: BudgetErrorCode
)
