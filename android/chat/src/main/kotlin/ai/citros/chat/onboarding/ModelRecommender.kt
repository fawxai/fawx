package ai.citros.chat.onboarding

import ai.citros.core.Provider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Recommends the best model for a given provider and API key.
 *
 * Priority order: Sonnet 4.5 > Sonnet 4 > Haiku > first available.
 * For Anthropic: uses a hardcoded model list (no network call needed).
 * For OpenRouter: queries `/api/v1/models` to discover available models.
 * Falls back to hardcoded defaults on network failure.
 */
class ModelRecommender(
    private val modelFetcher: ModelFetcher = DefaultModelFetcher()
) {

    suspend fun recommend(provider: Provider, apiKey: String): ModelRecommendation {
        val available = try {
            modelFetcher.fetchAvailableModels(provider, apiKey)
        } catch (_: Exception) {
            fallbackModels(provider)
        }

        if (available.isEmpty()) {
            val fallback = fallbackModels(provider)
            return pickBest(fallback)
        }

        return pickBest(available)
    }

    private fun pickBest(models: List<AvailableModel>): ModelRecommendation {
        val sorted = models.sortedBy { model ->
            val index = MODEL_PREFERENCE_ORDER.indexOf(model.id)
            if (index == -1) Int.MAX_VALUE else index
        }
        val best = sorted.firstOrNull()
            ?: throw IllegalStateException("No models available for provider")
        return ModelRecommendation(
            model = best.id,
            reason = MODEL_REASONS[best.id] ?: "Available model for your provider",
            alternatives = sorted.drop(1).map { it.id }
        )
    }

    companion object {
        /** Preference order — lower index = higher preference. */
        internal val MODEL_PREFERENCE_ORDER = listOf(
            "claude-sonnet-4-5-latest",
            "claude-sonnet-4-latest",
            "claude-haiku-4-5-latest",
            "anthropic/claude-sonnet-4-5-latest",
            "anthropic/claude-sonnet-4-latest",
            "anthropic/claude-haiku-4-5-latest"
        )

        private val MODEL_REASONS = mapOf(
            "claude-sonnet-4-5-latest" to "Best balance of quality and speed for phone control",
            "claude-sonnet-4-latest" to "Fast and accurate for phone control tasks",
            "claude-haiku-4-5-latest" to "Fastest responses, good for simple tasks",
            "anthropic/claude-sonnet-4-5-latest" to "Best balance of quality and speed for phone control",
            "anthropic/claude-sonnet-4-latest" to "Fast and accurate for phone control tasks",
            "anthropic/claude-haiku-4-5-latest" to "Fastest responses, good for simple tasks"
        )

        internal val ANTHROPIC_MODELS = listOf(
            AvailableModel("claude-sonnet-4-5-latest", "Claude Sonnet 4.5"),
            AvailableModel("claude-sonnet-4-latest", "Claude Sonnet 4"),
            AvailableModel("claude-haiku-4-5-latest", "Claude Haiku 4.5")
        )

        internal val OPENROUTER_FALLBACK_MODELS = listOf(
            AvailableModel("anthropic/claude-sonnet-4-5-latest", "Claude Sonnet 4.5"),
            AvailableModel("anthropic/claude-sonnet-4-latest", "Claude Sonnet 4"),
            AvailableModel("anthropic/claude-haiku-4-5-latest", "Claude Haiku 4.5")
        )

        internal val OPENAI_FALLBACK_MODELS = listOf(
            AvailableModel("gpt-4o", "GPT-4o"),
            AvailableModel("gpt-4o-mini", "GPT-4o Mini")
        )

        internal fun fallbackModels(provider: Provider): List<AvailableModel> = when (provider) {
            Provider.ANTHROPIC -> ANTHROPIC_MODELS
            Provider.OPENROUTER -> OPENROUTER_FALLBACK_MODELS
            Provider.OPENAI -> OPENAI_FALLBACK_MODELS
        }
    }
}

data class ModelRecommendation(
    val model: String,
    val reason: String,
    val alternatives: List<String>
)

data class AvailableModel(
    val id: String,
    val displayName: String
) {
    val displayId: String
        get() = id.substringAfterLast("/")
}

/**
 * Fetches available models from a provider's API.
 * Extracted as an interface for testability.
 */
interface ModelFetcher {
    suspend fun fetchAvailableModels(provider: Provider, apiKey: String): List<AvailableModel>
}

/**
 * Default implementation that queries real APIs.
 * Anthropic: returns hardcoded list (no model listing endpoint).
 * OpenRouter: queries /api/v1/models.
 * OpenAI: returns hardcoded list.
 */
class DefaultModelFetcher : ModelFetcher {
    override suspend fun fetchAvailableModels(
        provider: Provider,
        apiKey: String
    ): List<AvailableModel> = when (provider) {
        Provider.ANTHROPIC -> ModelRecommender.ANTHROPIC_MODELS
        Provider.OPENROUTER -> fetchOpenRouterModels(apiKey)
        Provider.OPENAI -> ModelRecommender.OPENAI_FALLBACK_MODELS
    }

    private suspend fun fetchOpenRouterModels(apiKey: String): List<AvailableModel> =
        withContext(Dispatchers.IO) {
            val url = URL("https://openrouter.ai/api/v1/models")
            // Keep HttpURLConnection here to avoid introducing a new dependency just for a
            // single lightweight GET in onboarding. OkHttp can replace this if/when added.
            val conn = url.openConnection() as HttpURLConnection
            try {
                conn.requestMethod = "GET"
                conn.setRequestProperty("Authorization", "Bearer $apiKey")
                conn.connectTimeout = 10_000
                conn.readTimeout = 10_000

                if (conn.responseCode != 200) {
                    return@withContext ModelRecommender.OPENROUTER_FALLBACK_MODELS
                }

                val body = conn.inputStream.bufferedReader().readText()
                parseOpenRouterModels(body)
            } finally {
                conn.disconnect()
            }
        }

    internal companion object {
        /**
         * Parse OpenRouter /api/v1/models JSON response.
         * Extracts Claude models from the "data" array.
         * Format: { "data": [{ "id": "anthropic/claude-...", "name": "..." }, ...] }
         */
        fun parseOpenRouterModels(json: String): List<AvailableModel> {
            return runCatching {
                val data = JSONObject(json).optJSONArray("data") ?: return emptyList()
                buildList {
                    for (i in 0 until data.length()) {
                        val model = data.optJSONObject(i) ?: continue
                        val id = model.optString("id", "")
                        if (!id.contains("claude", ignoreCase = true)) continue
                        val name = model.optString("name", id)
                        add(AvailableModel(id, name))
                    }
                }
            }.getOrDefault(emptyList())
        }
    }
}
