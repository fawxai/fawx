package ai.citros.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.runBlocking
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class BudgetGuardTest {

    @Test
    fun `disabled budget always allows spending`() {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = false, dailyLimitUsd = 1.0, monthlyLimitUsd = 1.0))
        val guard = BudgetGuard(store)

        val error = guard.trySpend(10.0)

        assertNull(error)
        assertEquals(10_000_000L, store.getDailySpentMicrodollars())
        assertEquals(10_000_000L, store.getMonthlySpentMicrodollars())
    }

    @Test
    fun `daily limit records overage and reports exceeded`() {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 1.0, monthlyLimitUsd = 0.0),
            dailySpentMicrodollars = 900_000L,
            monthlySpentMicrodollars = 900_000L
        )
        val guard = BudgetGuard(store)

        val error = guard.trySpend(0.11)

        assertNotNull(error)
        assertTrue(error.contains("Daily budget"))
        assertEquals(1_010_000L, store.getDailySpentMicrodollars())
    }

    @Test
    fun `monthly limit records overage and reports exceeded`() {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.0, monthlyLimitUsd = 2.0),
            dailySpentMicrodollars = 1_900_000L,
            monthlySpentMicrodollars = 1_900_000L
        )
        val guard = BudgetGuard(store)

        val error = guard.trySpend(0.11)

        assertNotNull(error)
        assertTrue(error.contains("Monthly budget"))
        assertEquals(2_010_000L, store.getMonthlySpentMicrodollars())
    }

    @Test
    fun `over-limit spend is persisted so zero-amount precheck cannot bypass`() {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 1.0, monthlyLimitUsd = 0.0),
            dailySpentMicrodollars = 950_000L,
            monthlySpentMicrodollars = 950_000L
        )
        val guard = BudgetGuard(store)

        val firstError = guard.trySpend(0.10)
        val precheckError = guard.trySpend(0.0)

        assertNotNull(firstError)
        assertTrue(firstError.contains("Daily budget"))
        assertEquals(1_050_000L, store.getDailySpentMicrodollars())
        assertNotNull(precheckError)
        assertTrue(precheckError.contains("Daily budget"))
    }

    @Test
    fun `per-task limit blocks when reached`() {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, perTaskLimitUsd = 0.05)
        )
        val guard = BudgetGuard(store)

        val error = guard.checkTaskLimit(0.05)

        assertNotNull(error)
        assertTrue(error.contains("Per-task budget"))
    }

    @Test
    fun `reset methods clear totals and pending carry`() {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true),
            dailySpentMicrodollars = 123L,
            monthlySpentMicrodollars = 456L,
            pendingNanodollars = 900L
        )

        store.resetDaily()
        store.resetMonthly()

        assertEquals(0L, store.getDailySpentMicrodollars())
        assertEquals(0L, store.getMonthlySpentMicrodollars())
        assertEquals(0L, store.getPendingNanodollars())
    }

    @Test
    fun `daily reset clears fractional carry so prior-day nanos do not bill next day`() {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = true))
        val guard = BudgetGuard(store)

        // 0.6 microdollars accumulates as pending nanos but should not bill yet.
        assertNull(guard.trySpend(0.0000006))
        assertEquals(0L, store.getDailySpentMicrodollars())
        assertEquals(600L, store.getPendingNanodollars())

        store.resetDaily()

        // After reset, prior-day pending nanos must not spill into next day billing.
        assertNull(guard.trySpend(0.0000005))
        assertEquals(0L, store.getDailySpentMicrodollars())
        assertEquals(500L, store.getPendingNanodollars())
    }

    @Test
    fun `monthly reset clears fractional carry so prior-month nanos do not bill next month`() {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = true))
        val guard = BudgetGuard(store)

        assertNull(guard.trySpend(0.0000006))
        assertEquals(0L, store.getMonthlySpentMicrodollars())
        assertEquals(600L, store.getPendingNanodollars())

        store.resetMonthly()

        assertNull(guard.trySpend(0.0000005))
        assertEquals(0L, store.getMonthlySpentMicrodollars())
        assertEquals(500L, store.getPendingNanodollars())
    }

    @Test
    fun `reset and spend race does not leak prior-period fractional carry`() = runBlocking {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = true))
        val guard = BudgetGuard(store)

        repeat(300) {
            // Seed pending carry below one microdollar.
            assertNull(guard.trySpend(0.0000006))
            assertEquals(0L, store.getDailySpentMicrodollars())

            val race = listOf(
                async(Dispatchers.Default) { guard.trySpend(0.0000005) },
                async(Dispatchers.Default) { store.resetDaily() }
            )
            race.awaitAll()

            // Regardless of ordering, no full microdollar should be billed in the new period.
            assertEquals(0L, store.getDailySpentMicrodollars())
            assertTrue(store.getPendingNanodollars() in setOf(0L, 500L))
            store.resetDaily()
            store.resetMonthly()
        }
    }

    @Test
    fun `monthly reset and spend race does not leak prior-period fractional carry`() = runBlocking {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = true))
        val guard = BudgetGuard(store)

        repeat(300) {
            assertNull(guard.trySpend(0.0000006))
            assertEquals(0L, store.getMonthlySpentMicrodollars())

            val race = listOf(
                async(Dispatchers.Default) { guard.trySpend(0.0000005) },
                async(Dispatchers.Default) { store.resetMonthly() }
            )
            race.awaitAll()

            assertEquals(0L, store.getMonthlySpentMicrodollars())
            assertTrue(store.getPendingNanodollars() in setOf(0L, 500L))
            store.resetDaily()
            store.resetMonthly()
        }
    }

    @Test
    fun `daily and monthly reset interleaving preserves no billed carry across rollover`() = runBlocking {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = true))
        val guard = BudgetGuard(store)

        repeat(300) {
            assertNull(guard.trySpend(0.0000006))

            val race = listOf(
                async(Dispatchers.Default) { guard.trySpend(0.0000005) },
                async(Dispatchers.Default) { store.resetDaily() },
                async(Dispatchers.Default) { store.resetMonthly() }
            )
            race.awaitAll()

            assertEquals(0L, store.getDailySpentMicrodollars())
            assertEquals(0L, store.getMonthlySpentMicrodollars())
            assertTrue(store.getPendingNanodollars() in setOf(0L, 500L))
            store.resetDaily()
            store.resetMonthly()
        }
    }

    @Test
    fun `trySpend is atomic under contention`() = runBlocking {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 1.0, monthlyLimitUsd = 1.0)
        )
        val guard = BudgetGuard(store)

        val attempts = (1..10).map {
            async(Dispatchers.Default) {
                guard.trySpend(0.2)
            }
        }.awaitAll()

        val successes = attempts.count { it == null }
        val failures = attempts.count { it != null }

        assertEquals(5, successes)
        assertEquals(5, failures)
        assertEquals(2_000_000L, store.getDailySpentMicrodollars())
        assertEquals(2_000_000L, store.getMonthlySpentMicrodollars())
    }

    @Test
    fun `trySpend rejects negative spend amounts`() {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = true))
        val guard = BudgetGuard(store)

        assertFailsWith<IllegalArgumentException> {
            guard.trySpend(-0.01)
        }
        assertEquals(0L, store.getDailySpentMicrodollars())
        assertEquals(0L, store.getMonthlySpentMicrodollars())
    }

    @Test
    fun `trySpend rejects non-finite spend amounts`() {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = true))
        val guard = BudgetGuard(store)

        assertFailsWith<IllegalArgumentException> {
            guard.trySpend(Double.POSITIVE_INFINITY)
        }
        assertFailsWith<IllegalArgumentException> {
            guard.trySpend(Double.NaN)
        }
        assertEquals(0L, store.getDailySpentMicrodollars())
        assertEquals(0L, store.getMonthlySpentMicrodollars())
    }

    @Test
    fun `multiple guards sharing one store remain atomic under contention`() = runBlocking {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 1.0, monthlyLimitUsd = 1.0)
        )
        val guards = listOf(BudgetGuard(store), BudgetGuard(store), BudgetGuard(store))

        val attempts = (1..12).map { idx ->
            async(Dispatchers.Default) {
                guards[idx % guards.size].trySpend(0.2)
            }
        }.awaitAll()

        val successes = attempts.count { it == null }
        val failures = attempts.count { it != null }

        assertEquals(5, successes)
        assertEquals(7, failures)
        assertEquals(2_400_000L, store.getDailySpentMicrodollars())
        assertEquals(2_400_000L, store.getMonthlySpentMicrodollars())
    }

    @Test
    fun `typed budget decision reports missing usage metadata`() {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 1.0, monthlyLimitUsd = 1.0)
        )
        val guard = BudgetGuard(store)

        val decision = guard.recordFallbackSpendForMissingUsage(0.000001)

        assertTrue(decision is BudgetDecision.MissingUsageMetadata)
        assertNull(decision.overLimitCode)
        assertEquals(1L, store.getDailySpentMicrodollars())
    }

    @Test
    fun `typed budget decisions include explicit limit codes`() {
        val dailyStore = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 1.0, monthlyLimitUsd = 0.0),
            dailySpentMicrodollars = 950_000L,
            monthlySpentMicrodollars = 950_000L
        )
        val dailyDecision = BudgetGuard(dailyStore).trySpendDecision(0.10)
        assertTrue(dailyDecision is BudgetDecision.OverLimit)
        assertEquals(BudgetErrorCode.DAILY_LIMIT, (dailyDecision as BudgetDecision.OverLimit).code)

        val monthlyStore = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.0, monthlyLimitUsd = 1.0),
            dailySpentMicrodollars = 950_000L,
            monthlySpentMicrodollars = 950_000L
        )
        val monthlyDecision = BudgetGuard(monthlyStore).trySpendDecision(0.10)
        assertTrue(monthlyDecision is BudgetDecision.OverLimit)
        assertEquals(BudgetErrorCode.MONTHLY_LIMIT, (monthlyDecision as BudgetDecision.OverLimit).code)

        val perTaskStore = InMemoryBudgetStore(config = BudgetConfig(enabled = true, perTaskLimitUsd = 0.05))
        val perTaskViolation = BudgetGuard(perTaskStore).checkTaskLimitDecision(0.05)
        assertNotNull(perTaskViolation)
        assertEquals(BudgetErrorCode.PER_TASK_LIMIT, perTaskViolation.code)
    }

    @Test
    fun `fallback decision preserves over-limit code`() {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.000001, monthlyLimitUsd = 10.0),
            dailySpentMicrodollars = 1L,
            monthlySpentMicrodollars = 1L
        )
        val guard = BudgetGuard(store)

        val decision = guard.recordFallbackSpendForMissingUsage(0.000001)

        assertNotNull(decision.overLimitMessage)
        assertEquals(BudgetErrorCode.DAILY_LIMIT, decision.overLimitCode)
    }

    @Test
    fun `fallback spend rejects non-finite amounts`() {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = true))
        val guard = BudgetGuard(store)

        assertFailsWith<IllegalArgumentException> {
            guard.recordFallbackSpendForMissingUsage(Double.POSITIVE_INFINITY)
        }
        assertFailsWith<IllegalArgumentException> {
            guard.recordFallbackSpendForMissingUsage(Double.NaN)
        }
        assertEquals(0L, store.getDailySpentMicrodollars())
        assertEquals(0L, store.getMonthlySpentMicrodollars())
    }

    @Test
    fun `sub-micro spends accumulate without rounding loss`() {
        val store = InMemoryBudgetStore(config = BudgetConfig(enabled = true))
        val guard = BudgetGuard(store)

        repeat(10) {
            val error = guard.trySpend(0.0000004)
            assertNull(error)
        }

        assertEquals(4L, store.getDailySpentMicrodollars())
        assertEquals(4L, store.getMonthlySpentMicrodollars())
    }

    @Test
    fun `emits structured telemetry on per-task cap`() {
        val events = mutableListOf<BudgetTelemetryEvent>()
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, perTaskLimitUsd = 0.05)
        )
        val guard = BudgetGuard(store) { event -> events += event }

        val error = guard.checkTaskLimit(0.05)

        assertNotNull(error)
        assertTrue(events.any { it.type == BudgetTelemetryEvent.Type.PER_TASK_LIMIT_REACHED })
    }

    @Test
    fun `telemetry callbacks are dispatched outside store lock`() {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 10.0, monthlyLimitUsd = 10.0, perTaskLimitUsd = 0.01)
        )
        val callbackHeldStates = mutableListOf<Boolean>()
        val guard = BudgetGuard(store) { _ ->
            callbackHeldStates += Thread.holdsLock(store)
        }

        guard.trySpend(0.005)
        guard.checkTaskLimit(0.01)

        assertEquals(2, callbackHeldStates.size)
        assertFalse(callbackHeldStates[0], "spend telemetry callback must run after unlock")
        assertFalse(callbackHeldStates[1], "per-task telemetry callback must run after unlock")
    }

    @Test
    fun `precheck reports over-limit without side effects`() {
        val events = mutableListOf<BudgetTelemetryEvent>()
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 1.0, monthlyLimitUsd = 10.0),
            dailySpentMicrodollars = 1_100_000L,
            monthlySpentMicrodollars = 1_100_000L
        )
        val guard = BudgetGuard(store) { event -> events += event }

        val precheck = guard.checkWouldExceedBudgetWithoutSpending()

        assertNotNull(precheck)
        assertTrue(precheck.contains("Daily budget"))
        assertEquals(1_100_000L, store.getDailySpentMicrodollars())
        assertEquals(1_100_000L, store.getMonthlySpentMicrodollars())
        assertEquals(1, events.size)
        assertEquals(BudgetTelemetryEvent.Type.PRECALL_DAILY_LIMIT_BLOCKED, events.first().type)
    }

    @Test
    fun `precheck blocks immediately when daily spend equals limit`() {
        val events = mutableListOf<BudgetTelemetryEvent>()
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 1.0, monthlyLimitUsd = 10.0),
            dailySpentMicrodollars = 1_000_000L,
            monthlySpentMicrodollars = 1_000_000L
        )
        val guard = BudgetGuard(store) { event -> events += event }

        val precheck = guard.checkWouldExceedBudgetWithoutSpending()

        assertNotNull(precheck)
        assertTrue(precheck.contains("Daily budget"))
        assertEquals(1_000_000L, store.getDailySpentMicrodollars())
        assertEquals(1_000_000L, store.getMonthlySpentMicrodollars())
        assertEquals(1, events.size)
        assertEquals(BudgetTelemetryEvent.Type.PRECALL_DAILY_LIMIT_BLOCKED, events.first().type)
    }

    @Test
    fun `precheck blocks immediately when monthly spend equals limit`() {
        val events = mutableListOf<BudgetTelemetryEvent>()
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 10.0, monthlyLimitUsd = 1.0),
            dailySpentMicrodollars = 1_000_000L,
            monthlySpentMicrodollars = 1_000_000L
        )
        val guard = BudgetGuard(store) { event -> events += event }

        val precheck = guard.checkWouldExceedBudgetWithoutSpending()

        assertNotNull(precheck)
        assertTrue(precheck.contains("Monthly budget"))
        assertEquals(1_000_000L, store.getDailySpentMicrodollars())
        assertEquals(1_000_000L, store.getMonthlySpentMicrodollars())
        assertEquals(1, events.size)
        assertEquals(BudgetTelemetryEvent.Type.PRECALL_MONTHLY_LIMIT_BLOCKED, events.first().type)
    }

    @Test
    fun `over-limit spend message uses exceeded wording after spend is recorded`() {
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 1.0, monthlyLimitUsd = 10.0),
            dailySpentMicrodollars = 950_000L,
            monthlySpentMicrodollars = 950_000L
        )
        val guard = BudgetGuard(store)

        val error = guard.trySpend(0.10)

        assertNotNull(error)
        assertTrue(error.contains("exceeded"))
        assertFalse(error.contains("would be exceeded"))
    }

    @Test
    fun `missing usage fallback emits a single telemetry event per fallback decision`() {
        val events = mutableListOf<BudgetTelemetryEvent>()
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.000001, monthlyLimitUsd = 10.0),
            dailySpentMicrodollars = 1L,
            monthlySpentMicrodollars = 1L
        )
        val guard = BudgetGuard(store) { event -> events += event }

        val decision = guard.recordFallbackSpendForMissingUsage(0.000001)

        assertNotNull(decision.overLimitMessage)
        assertEquals(1, events.size, "fallback should emit only one telemetry event")
        assertEquals(BudgetTelemetryEvent.Type.MISSING_USAGE_FALLBACK, events.first().type)
    }

    @Test
    fun `missing usage fallback over-limit emits fallback telemetry`() {
        val events = mutableListOf<BudgetTelemetryEvent>()
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.000001, monthlyLimitUsd = 10.0),
            dailySpentMicrodollars = 1L,
            monthlySpentMicrodollars = 1L
        )
        val guard = BudgetGuard(store) { event -> events += event }

        val decision = guard.recordFallbackSpendForMissingUsage(0.000001)

        assertNotNull(decision.overLimitMessage)
        val fallbackEvents = events.filter { it.type == BudgetTelemetryEvent.Type.MISSING_USAGE_FALLBACK }
        assertEquals(1, fallbackEvents.size)
        assertNotNull(fallbackEvents.first().message)
        assertTrue(
            fallbackEvents.first().message!!.contains("Daily budget"),
            "fallback telemetry should include over-limit message context"
        )
    }

    @Test
    fun `missing usage fallback telemetry payload is deterministic when over-limit`() {
        val events = mutableListOf<BudgetTelemetryEvent>()
        val store = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.000001, monthlyLimitUsd = 10.0),
            dailySpentMicrodollars = 1L,
            monthlySpentMicrodollars = 1L
        )
        val guard = BudgetGuard(store) { event -> events += event }

        val decision = guard.recordFallbackSpendForMissingUsage(0.000001)

        assertNotNull(decision.overLimitMessage)
        val fallbackEvent = events.single { it.type == BudgetTelemetryEvent.Type.MISSING_USAGE_FALLBACK }
        assertEquals(0.000001, fallbackEvent.amountUsd, 0.0)
        assertEquals(1_000L, fallbackEvent.amountNanodollars)
        assertEquals(0.000002, fallbackEvent.dailySpentUsd, 0.0)
        assertEquals(2L, fallbackEvent.dailySpentMicrodollars)
        assertEquals(0.000002, fallbackEvent.monthlySpentUsd, 0.0)
        assertEquals(2L, fallbackEvent.monthlySpentMicrodollars)
        assertEquals(
            "Daily budget of \$0.000001 exceeded (\$0.000002 used). Reset at midnight. Usage metadata missing; applied conservative fallback estimate of \$0.000001.",
            fallbackEvent.message
        )
    }
}
