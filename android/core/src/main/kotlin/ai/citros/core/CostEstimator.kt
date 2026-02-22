package ai.citros.core

import android.util.Log
import java.time.LocalDate
import java.time.temporal.ChronoUnit
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.double
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Estimates API cost from token usage.
 * Pricing is USD per 1M tokens.
 */
object CostEstimator {
    private const val TAG = "CitrosBudget"
    private const val MAX_PRICING_AGE_DAYS = 90L
    private const val PRICING_CATALOG_RESOURCE = "cost-pricing-catalog.json"
    private val providerPrefixRegex =
        Regex("^(anthropic|openai|openrouter|google|meta|mistral|xai)[-:._/]+")
    private val fallbackPricingLastUpdated: LocalDate = LocalDate.of(2026, 2, 21)
    @Volatile
    private var stalePricingWarningLogged = false
    @Volatile
    private var nowProvider: () -> LocalDate = { LocalDate.now() }

    data class ModelPricing(
        val inputPer1M: Double,
        val outputPer1M: Double,
        val cacheReadPer1M: Double = inputPer1M * 0.1,
        val cacheWritePer1M: Double = inputPer1M * 1.25
    )

    private data class PricingCatalog(
        val lastUpdated: LocalDate,
        val pricing: Map<String, ModelPricing>
    )

    private val fallbackPricing = mapOf(
        // Anthropic families + explicit versions
        "claude-opus-4" to ModelPricing(15.0, 75.0),
        "claude-sonnet-4" to ModelPricing(3.0, 15.0),
        "claude-haiku-4" to ModelPricing(0.80, 4.0),
        "claude-opus-4-6" to ModelPricing(15.0, 75.0),
        "claude-sonnet-4-5" to ModelPricing(3.0, 15.0),
        "claude-haiku-4-5" to ModelPricing(0.80, 4.0),
        // OpenAI
        "gpt-4o" to ModelPricing(2.50, 10.0),
        "gpt-4o-mini" to ModelPricing(0.15, 0.60),
        "gpt-4" to ModelPricing(30.0, 60.0),
        // Fallback
        "default" to ModelPricing(3.0, 15.0)
    )
    private val catalog: PricingCatalog by lazy { loadPricingCatalog() }
    private val pricing: Map<String, ModelPricing> get() = catalog.pricing
    private val pricingLastUpdated: LocalDate get() = catalog.lastUpdated

    fun estimate(usage: TokenUsage, modelId: String?): Double {
        warnIfPricingIsStale()
        val modelPricing = findPricing(modelId)
        return (usage.inputTokens * modelPricing.inputPer1M / 1_000_000.0) +
            (usage.outputTokens * modelPricing.outputPer1M / 1_000_000.0) +
            (usage.cacheReadTokens * modelPricing.cacheReadPer1M / 1_000_000.0) +
            (usage.cacheWriteTokens * modelPricing.cacheWritePer1M / 1_000_000.0)
    }

    fun estimateTask(accumulator: TaskTokenAccumulator, modelId: String?): Double {
        warnIfPricingIsStale()
        val modelPricing = findPricing(modelId)
        return (accumulator.totalInputTokens * modelPricing.inputPer1M / 1_000_000.0) +
            (accumulator.totalOutputTokens * modelPricing.outputPer1M / 1_000_000.0) +
            (accumulator.totalCacheReadTokens * modelPricing.cacheReadPer1M / 1_000_000.0) +
            (accumulator.totalCacheWriteTokens * modelPricing.cacheWritePer1M / 1_000_000.0)
    }

    internal fun findPricing(modelId: String?): ModelPricing {
        val fallback = pricing.getValue("default")
        if (modelId == null) return fallback

        for (candidate in normalizeCandidates(modelId)) {
            pricing[candidate]?.let { return it }

            val noDate = candidate.replace(Regex("-\\d{8,}$"), "")
            pricing[noDate]?.let { return it }

            var prefix = noDate
            while (prefix.contains('-')) {
                prefix = prefix.substringBeforeLast('-')
                pricing[prefix]?.let { return it }
            }
        }

        return fallback
    }

    private fun normalizeCandidates(modelId: String): List<String> {
        val candidates = linkedSetOf<String>()

        fun addCandidate(raw: String) {
            val trimmed = raw.trim().lowercase()
            if (trimmed.isNotEmpty()) {
                candidates += trimmed
            }
        }

        addCandidate(modelId)

        var tail = modelId.trim().lowercase()
        while (true) {
            val nextTail = tail.substringAfterLast('/').substringAfterLast(':')
            if (nextTail == tail) break
            addCandidate(nextTail)
            tail = nextTail
        }

        val existing = candidates.toList()
        for (candidate in existing) {
            val canonical = candidate
                .replace('.', '-')
                .replace('_', '-')
                .replace(Regex("-+"), "-")
                .trim('-')
            addCandidate(canonical)

            var withoutProvider = canonical
            while (true) {
                val stripped = withoutProvider.replaceFirst(providerPrefixRegex, "")
                if (stripped == withoutProvider) break
                withoutProvider = stripped
                addCandidate(withoutProvider)
            }
        }

        return candidates.toList()
    }

    internal fun isPricingCatalogStale(now: LocalDate): Boolean {
        val ageDays = ChronoUnit.DAYS.between(pricingLastUpdated, now)
        return ageDays > MAX_PRICING_AGE_DAYS
    }

    private fun warnIfPricingIsStale() {
        if (stalePricingWarningLogged) return
        if (!isPricingCatalogStale(nowProvider())) return
        stalePricingWarningLogged = true
        Log.w(
            TAG,
            "CostEstimator pricing catalog may be stale (last updated $pricingLastUpdated). Refresh model pricing data."
        )
    }

    internal fun setNowProviderForTests(nowProvider: () -> LocalDate) {
        this.nowProvider = nowProvider
        stalePricingWarningLogged = false
    }

    internal fun resetNowProviderForTests() {
        nowProvider = { LocalDate.now() }
        stalePricingWarningLogged = false
    }

    private fun loadPricingCatalog(): PricingCatalog {
        return try {
            val text = CostEstimator::class.java.classLoader
                ?.getResourceAsStream(PRICING_CATALOG_RESOURCE)
                ?.bufferedReader()
                ?.use { it.readText() }
                ?: return PricingCatalog(fallbackPricingLastUpdated, fallbackPricing)
            val parsed = Json.parseToJsonElement(text).jsonObject
            val lastUpdated = parsed["last_updated"]
                ?.jsonPrimitive
                ?.content
                ?.let(LocalDate::parse)
                ?: fallbackPricingLastUpdated
            val models = parsed["models"]?.jsonObject ?: return PricingCatalog(lastUpdated, fallbackPricing)
            val loadedPricing = buildMap {
                models.forEach { (name, value) ->
                    val model = value.jsonObject
                    val input = model["input_per_1m"]?.jsonPrimitive?.double
                    val output = model["output_per_1m"]?.jsonPrimitive?.double
                    if (input != null && output != null) {
                        val cacheRead = model["cache_read_per_1m"]?.jsonPrimitive?.double ?: (input * 0.1)
                        val cacheWrite = model["cache_write_per_1m"]?.jsonPrimitive?.double ?: (input * 1.25)
                        put(
                            name.lowercase(),
                            ModelPricing(
                                inputPer1M = input,
                                outputPer1M = output,
                                cacheReadPer1M = cacheRead,
                                cacheWritePer1M = cacheWrite
                            )
                        )
                    }
                }
            }
            if ("default" !in loadedPricing) {
                PricingCatalog(lastUpdated, fallbackPricing)
            } else {
                PricingCatalog(lastUpdated, loadedPricing)
            }
        } catch (e: Exception) {
            Log.w(TAG, "Failed to load pricing catalog resource; using fallback map: ${e.message}")
            PricingCatalog(fallbackPricingLastUpdated, fallbackPricing)
        }
    }
}
