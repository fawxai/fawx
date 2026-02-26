package ai.citros.core

import ai.citros.test.Flaky
import ai.citros.test.FlakyTestRule
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import org.junit.Rule
import org.junit.Test
import kotlin.math.ceil
import kotlin.test.assertTrue

class ActionPolicyConcurrencyBenchmark {

    @Rule
    @JvmField
    val flakyTestRule = FlakyTestRule()

    @Flaky("#780")
    @Test
    fun `default action policy evaluate p99 stays under 5ms at 100-way concurrency`() = runTest {
        val policy = DefaultActionPolicy()
        val context = PolicyContext(foregroundApp = "com.example.app")
        val latenciesNs = mutableListOf<Long>()
        val lock = Any()

        coroutineScope {
            repeat(100) { index ->
                launch {
                    val start = System.nanoTime()
                    policy.evaluate(ToolCall("tc-$index", "tap", mapOf("element_id" to index)), context)
                    val elapsed = System.nanoTime() - start
                    synchronized(lock) {
                        latenciesNs += elapsed
                    }
                }
            }
        }

        val sorted = latenciesNs.sorted()
        val p50 = percentile(sorted, 0.50)
        val p95 = percentile(sorted, 0.95)
        val p99 = percentile(sorted, 0.99)

        // Local baseline in CI-like runs is typically sub-millisecond; keep strict p99 budget.
        assertTrue(
            p99 < 5_000_000,
            "ActionPolicy.evaluate latency too high (samples=${sorted.size}): p50=${p50 / 1_000_000.0}ms, p95=${p95 / 1_000_000.0}ms, p99=${p99 / 1_000_000.0}ms"
        )
    }

    private fun percentile(sorted: List<Long>, quantile: Double): Long {
        if (sorted.isEmpty()) return 0L
        val idx = ceil(sorted.size * quantile).toInt().coerceAtLeast(1) - 1
        return sorted[idx]
    }
}
