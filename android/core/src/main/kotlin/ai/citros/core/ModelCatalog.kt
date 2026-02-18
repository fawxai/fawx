package ai.citros.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import okhttp3.Request

/**
 * Dynamic model catalog: fetches available models from provider APIs,
 * classifies by capability tier, and caches with TTL.
 *
 * Replaces hardcoded model lists with live data from:
 * - Anthropic: `GET /v1/models`
 * - OpenAI: `GET /v1/models`
 * - OpenRouter: `GET /api/v1/models`
 *
 * Falls back to hardcoded defaults if fetch fails (offline, API down).
 *
 * Usage:
 * ```
 * val models = ModelCatalog.getModels(providerConfig)
 * val actionModels = models.filter { it.tier != ModelTier.SMALL }
 * ```
 *
 * Thread-safe: uses synchronized blocks for cache access.
 */
object ModelCatalog {

    /**
     * A model fetched from a provider API with tier classification.
     *
     * @param id Provider-native model ID (e.g. "claude-sonnet-4-5-20250929")
     * @param displayName Human-readable name if available
     * @param tier Capability tier as classified by [ModelClassifier]
     * @param provider Which provider this model belongs to
     */
    data class CachedModel(
        val id: String,
        val displayName: String?,
        val tier: ModelTier,
        val provider: Provider
    )

    /** Cache entry with fetch timestamp. */
    private data class CacheEntry(
        val models: List<CachedModel>,
        val fetchedAt: Long = System.currentTimeMillis()
    )

    /** Cache TTL: 24 hours. */
    private const val CACHE_TTL_MS = 24 * 60 * 60 * 1000L

    /** Per-provider cache. */
    private val cache = mutableMapOf<Provider, CacheEntry>()

    /** JSON parser. */
    private val json = Json { ignoreUnknownKeys = true }

    /**
     * Get models for a provider, fetching from API if cache is missing or expired.
     *
     * @param config Provider configuration (supplies auth headers and provider type)
     * @param forceRefresh Bypass cache TTL and fetch fresh data
     * @return List of available models with tier classification
     */
    suspend fun getModels(
        config: ProviderConfig,
        forceRefresh: Boolean = false
    ): List<CachedModel> {
        val cached = synchronized(cache) { cache[config.provider] }

        if (!forceRefresh && cached != null && !isExpired(cached)) {
            return cached.models
        }

        return try {
            val fetched = fetchModels(config)
            synchronized(cache) {
                cache[config.provider] = CacheEntry(fetched)
            }
            fetched
        } catch (_: Exception) {
            // Fallback: expired cache if available, else hardcoded defaults
            cached?.models ?: hardcodedFallback(config.provider)
        }
    }

    /**
     * Get cached models without making a network call.
     *
     * @return Cached models, or null if no cache exists for this provider
     */
    fun getCachedModels(provider: Provider): List<CachedModel>? {
        return synchronized(cache) { cache[provider]?.models }
    }

    /**
     * Get only models that meet the action loop floor requirement.
     *
     * @param config Provider configuration
     * @param forceRefresh Bypass cache TTL
     * @return Models with tier >= STANDARD
     */
    suspend fun getActionModels(
        config: ProviderConfig,
        forceRefresh: Boolean = false
    ): List<CachedModel> {
        return getModels(config, forceRefresh).filter { it.tier != ModelTier.SMALL }
    }

    /**
     * Fetch models from the provider's model listing API.
     */
    private suspend fun fetchModels(config: ProviderConfig): List<CachedModel> =
        withContext(Dispatchers.IO) {
            val url = modelsEndpoint(config.provider)
            val requestBuilder = Request.Builder()
                .url(url)
                .get()

            // Add provider auth headers
            config.headers.forEach { (key, value) ->
                requestBuilder.addHeader(key, value)
            }

            val response = BaseProviderClient.sharedClient
                .newCall(requestBuilder.build())
                .execute()

            val body = response.body?.string()
                ?: throw RuntimeException("Empty response from $url")

            if (!response.isSuccessful) {
                throw RuntimeException("Model fetch failed: HTTP ${response.code}")
            }

            parseModels(body, config.provider)
        }

    /**
     * Models list API endpoint for each provider.
     */
    private fun modelsEndpoint(provider: Provider): String = when (provider) {
        Provider.ANTHROPIC -> "https://api.anthropic.com/v1/models"
        Provider.OPENAI -> "https://api.openai.com/v1/models"
        Provider.OPENROUTER -> "https://openrouter.ai/api/v1/models"
    }

    /**
     * Parse the JSON response from a provider's models endpoint.
     *
     * All three providers use the `{ "data": [{ "id": "...", ... }] }` format.
     * Filters to chat-capable models only (excludes embeddings, TTS, image gen, etc.).
     */
    internal fun parseModels(jsonBody: String, provider: Provider): List<CachedModel> {
        val root = json.parseToJsonElement(jsonBody).jsonObject
        val data = root["data"]?.jsonArray ?: return emptyList()

        return data.mapNotNull { element ->
            val obj = element.jsonObject
            val id = obj["id"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null

            // Filter to chat-capable models
            if (!isChatCapableModel(id, provider)) return@mapNotNull null

            val displayName = obj["display_name"]?.jsonPrimitive?.contentOrNull
                ?: obj["name"]?.jsonPrimitive?.contentOrNull

            CachedModel(
                id = id,
                displayName = displayName,
                tier = ModelClassifier.classify(id),
                provider = provider
            )
        }.sortedWith(compareBy({ it.tier.ordinal }, { it.id }))
    }

    /**
     * Filter to only text-generation / chat-capable models.
     *
     * Provider APIs return all model types (embeddings, TTS, image gen, moderation, etc.).
     * We only want models that support the Messages/Chat Completions API.
     */
    internal fun isChatCapableModel(modelId: String, provider: Provider): Boolean {
        val id = modelId.lowercase()
        return when (provider) {
            Provider.ANTHROPIC -> id.startsWith("claude-")

            Provider.OPENAI -> {
                // Include GPT, o1, o3 series
                // Exclude embeddings, TTS, DALL-E, Whisper, moderation, etc.
                (id.startsWith("gpt-") || id.startsWith("o1") || id.startsWith("o3")) &&
                    !id.contains("embedding") &&
                    !id.contains("instruct") &&
                    !id.contains("realtime") &&
                    !id.contains("audio") &&
                    !id.contains("search") &&
                    !id.contains("tts") &&
                    !id.contains("whisper") &&
                    !id.contains("moderation") &&
                    !id.contains("dall")
            }

            Provider.OPENROUTER -> {
                // OpenRouter aggregates many providers — accept known chat model families
                // The tier classifier handles safety; this just filters non-chat models
                id.contains("claude") ||
                    id.contains("gpt-") ||
                    id.contains("/o1") || id.contains("/o3") ||
                    id.contains("gemini") ||
                    id.contains("llama") ||
                    id.contains("mistral") ||
                    id.contains("command")
                // Note: this is intentionally broad. OpenRouter model diversity is a feature.
                // TODO: Use OpenRouter's response metadata for capability-based filtering
            }
        }
    }

    private fun isExpired(entry: CacheEntry): Boolean {
        return System.currentTimeMillis() - entry.fetchedAt > CACHE_TTL_MS
    }

    /**
     * Hardcoded fallback when API fetch fails.
     * Uses the static lists in [ModelConfig] as last resort.
     */
    private fun hardcodedFallback(provider: Provider): List<CachedModel> {
        return ModelConfig.chatModelsForProvider(provider).map { id ->
            CachedModel(
                id = id,
                displayName = null,
                tier = ModelClassifier.classify(id),
                provider = provider
            )
        }
    }

    /**
     * Clear all caches. Exposed for testing.
     */
    internal fun clearCache() {
        synchronized(cache) { cache.clear() }
    }
}
