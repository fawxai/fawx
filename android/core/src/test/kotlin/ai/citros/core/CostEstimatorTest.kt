package ai.citros.core

import org.junit.Test
import java.time.LocalDate
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class CostEstimatorTest {

    @Test
    fun `known model uses expected pricing`() {
        val usage = TokenUsage(inputTokens = 1_000_000, outputTokens = 1_000_000)

        val cost = CostEstimator.estimate(usage, "claude-sonnet-4")

        assertEquals(18.0, cost, 0.000001)
    }

    @Test
    fun `prefix match strips date suffix`() {
        val base = CostEstimator.findPricing("claude-sonnet-4-5")
        val dated = CostEstimator.findPricing("claude-sonnet-4-5-20250514")

        assertEquals(base, dated)
    }

    @Test
    fun `prefix match strips trailing model segment progressively`() {
        val family = CostEstimator.findPricing("claude-opus-4")
        val variant = CostEstimator.findPricing("claude-opus-4-6")

        assertEquals(family, variant)
    }

    @Test
    fun `openrouter prefixed dotted model ids normalize to known pricing`() {
        val expected = CostEstimator.findPricing("claude-sonnet-4-5")

        val openRouterDotted = CostEstimator.findPricing("anthropic/claude-sonnet-4.5")
        val openRouterDottedDated = CostEstimator.findPricing("anthropic/claude-sonnet-4.5-20250929")

        assertEquals(expected, openRouterDotted)
        assertEquals(expected, openRouterDottedDated)
    }

    @Test
    fun `provider prefixed model ids normalize to known pricing`() {
        val expected = CostEstimator.findPricing("gpt-4o-mini")
        val prefixed = CostEstimator.findPricing("openai/gpt-4o-mini")

        assertEquals(expected, prefixed)
    }

    @Test
    fun `normalization variants resolve consistently and preserve cost monotonicity`() {
        val variantGroups = mapOf(
            "claude-sonnet-4-5" to listOf(
                "claude-sonnet-4.5",
                "anthropic/claude-sonnet-4.5",
                "openrouter/anthropic/claude-sonnet-4.5-20250929",
                "anthropic.claude-sonnet-4.5-20250929"
            ),
            "gpt-4o-mini" to listOf(
                "openai/gpt-4o-mini",
                "openrouter/openai/gpt-4o-mini",
                "openai.gpt-4o-mini"
            )
        )
        val lowUsage = TokenUsage(inputTokens = 10, outputTokens = 5)
        val highUsage = TokenUsage(inputTokens = 20, outputTokens = 10)

        variantGroups.forEach { (canonicalModel, variants) ->
            val canonicalPricing = CostEstimator.findPricing(canonicalModel)
            val canonicalLowCost = CostEstimator.estimate(lowUsage, canonicalModel)
            val canonicalHighCost = CostEstimator.estimate(highUsage, canonicalModel)
            assertTrue(canonicalHighCost > canonicalLowCost, "baseline monotonicity should hold for $canonicalModel")

            variants.forEach { variant ->
                assertEquals(canonicalPricing, CostEstimator.findPricing(variant), "variant should map to canonical pricing: $variant")
                val variantLowCost = CostEstimator.estimate(lowUsage, variant)
                val variantHighCost = CostEstimator.estimate(highUsage, variant)
                assertEquals(canonicalLowCost, variantLowCost, 0.0000000001, "variant low-cost should match canonical: $variant")
                assertEquals(canonicalHighCost, variantHighCost, 0.0000000001, "variant high-cost should match canonical: $variant")
                assertTrue(variantHighCost > variantLowCost, "cost monotonicity should hold for variant: $variant")
            }
        }
    }

    @Test
    fun `unknown model falls back to default pricing`() {
        val default = CostEstimator.findPricing(null)
        val unknown = CostEstimator.findPricing("unknown-model-v3")

        assertEquals(default, unknown)
    }

    @Test
    fun `null model falls back to default pricing`() {
        val usage = TokenUsage(inputTokens = 1_000_000, outputTokens = 1_000_000)

        val cost = CostEstimator.estimate(usage, null)

        assertEquals(18.0, cost, 0.000001)
    }

    @Test
    fun `cache tokens are included in estimate`() {
        val usage = TokenUsage(
            inputTokens = 1_000_000,
            outputTokens = 0,
            cacheReadTokens = 1_000_000,
            cacheWriteTokens = 1_000_000
        )

        val cost = CostEstimator.estimate(usage, "claude-sonnet-4")

        // input 3.0 + cache read 0.3 + cache write 3.75 = 7.05
        assertEquals(7.05, cost, 0.000001)
    }

    @Test
    fun `zero tokens estimate to zero cost`() {
        val usage = TokenUsage(inputTokens = 0, outputTokens = 0)

        val cost = CostEstimator.estimate(usage, "claude-sonnet-4")

        assertEquals(0.0, cost, 0.0)
    }

    @Test
    fun `task estimate sums all recorded calls`() {
        val accumulator = TaskTokenAccumulator().apply {
            record(TokenUsage(inputTokens = 1_000_000, outputTokens = 0))
            record(TokenUsage(inputTokens = 0, outputTokens = 1_000_000))
        }

        val total = CostEstimator.estimateTask(accumulator, "claude-sonnet-4")

        assertEquals(18.0, total, 0.000001)
    }

    @Test
    fun `pricing catalog staleness check is deterministic`() {
        assertFalse(CostEstimator.isPricingCatalogStale(LocalDate.of(2026, 2, 22)))
        assertTrue(CostEstimator.isPricingCatalogStale(LocalDate.of(2026, 6, 1)))
    }

    @Test
    fun `clock can be injected for deterministic staleness path`() {
        CostEstimator.setNowProviderForTests { LocalDate.of(2026, 6, 1) }
        try {
            val usage = TokenUsage(inputTokens = 1, outputTokens = 1)
            val cost = CostEstimator.estimate(usage, "claude-sonnet-4")
            assertTrue(cost > 0.0)
        } finally {
            CostEstimator.resetNowProviderForTests()
        }
    }

    @Test
    fun `estimateTask also executes staleness path deterministically`() {
        CostEstimator.setNowProviderForTests { LocalDate.of(2026, 6, 1) }
        try {
            val accumulator = TaskTokenAccumulator().apply {
                record(TokenUsage(inputTokens = 1, outputTokens = 1))
            }
            val cost = CostEstimator.estimateTask(accumulator, "claude-sonnet-4")
            assertTrue(cost > 0.0)
        } finally {
            CostEstimator.resetNowProviderForTests()
        }
    }
}
