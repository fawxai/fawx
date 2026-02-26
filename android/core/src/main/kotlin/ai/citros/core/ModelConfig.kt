package ai.citros.core

/**
 * Centralized model configuration for API providers.
 *
 * Anthropic direct API requires date-stamped model IDs (e.g. claude-sonnet-4-5-20250929).
 * The `-latest` aliases do NOT work on the raw Messages API — they return 404.
 *
 * OpenRouter uses semantic versioning format (anthropic/claude-sonnet-4.5)
 * which automatically tracks the latest patch version.
 */
object ModelConfig {
    // Anthropic direct API model IDs (date-stamped, required by Messages API)
    /**
     * High-capability model for user-facing chat, planning, and complex reasoning.
     * Must use date-stamped ID — `-latest` aliases return 404 on raw Anthropic API.
     */
    const val CHAT_MODEL = "claude-sonnet-4-5-20250929"

    /**
     * Model for action loop iterations. Must meet the model floor (Sonnet-tier minimum).
     * Must use date-stamped ID — `-latest` aliases return 404 on raw Anthropic API.
     */
    const val ACTION_MODEL = "claude-sonnet-4-5-20250929"

    /**
     * Default model for single-client configurations.
     */
    const val DEFAULT_MODEL = CHAT_MODEL

    // OpenRouter API model IDs
    /**
     * OpenRouter ID for Claude Sonnet 4.5 (chat model).
     * OpenRouter's semantic versioning automatically tracks the latest patch version.
     */
    const val OPENROUTER_CHAT_MODEL = "anthropic/claude-sonnet-4.5"

    /**
     * OpenRouter action model (Sonnet-tier minimum per model floor policy).
     * OpenRouter's semantic versioning automatically tracks the latest patch version.
     */
    const val OPENROUTER_ACTION_MODEL = "anthropic/claude-sonnet-4.5"

    // OpenAI API model IDs
    /**
     * OpenAI high-capability model for chat.
     */
    const val OPENAI_CHAT_MODEL = "gpt-4o"

    /**
     * OpenAI action model (GPT-4o tier minimum per model floor policy).
     */
    const val OPENAI_ACTION_MODEL = "gpt-4o"

    /**
     * Get available chat models for a provider.
     *
     * Returns model IDs in the provider's native format.
     *
     * @param provider The API provider
     * @return List of available chat model IDs
     */
    /**
     * Curated chat model list per provider (#456).
     * Only models that work well and are currently available.
     * Dynamic discovery deferred to #391.
     *
     * Models ordered by capability tier: highest first (e.g. Opus → Sonnet → Haiku).
     */
    fun chatModelsForProvider(provider: Provider): List<String> = when (provider) {
        Provider.ANTHROPIC -> listOf(
            "claude-opus-4-6",
            "claude-sonnet-4-5-20250929",
            "claude-haiku-3-5-20241022"
        )
        Provider.OPENROUTER -> listOf(
            "anthropic/claude-sonnet-4.5",
            "anthropic/claude-opus-4.5",
            "google/gemini-2.5-pro-preview",
            "openai/gpt-4o",
            "deepseek/deepseek-r1",
            "meta-llama/llama-4-maverick"
        )
        Provider.OPENAI -> listOf(
            "gpt-4o",
            "gpt-4o-mini"
        )
    }

    /**
     * Get available action models for a provider.
     *
     * Returns model IDs in the provider's native format.
     * Action models are typically faster/cheaper variants suitable for the action loop.
     *
     * @param provider The API provider
     * @return List of available action model IDs
     */
    /**
     * Curated action model list per provider (#456).
     * Action models must meet the Sonnet-tier floor (no Haiku/mini).
     *
     * Models ordered by capability tier: highest first (e.g. Opus → Sonnet).
     */
    fun actionModelsForProvider(provider: Provider): List<String> = when (provider) {
        Provider.ANTHROPIC -> listOf(
            "claude-opus-4-6",
            "claude-sonnet-4-5-20250929"
        )
        Provider.OPENROUTER -> listOf(
            "anthropic/claude-sonnet-4.5",
            "anthropic/claude-opus-4.5"
        )
        Provider.OPENAI -> listOf(
            "gpt-4o"
        )
    }

    /**
     * Get the default chat model for a provider.
     *
     * @param provider The API provider
     * @return The default chat model ID for this provider
     */
    fun defaultChatModel(provider: Provider): String = when (provider) {
        Provider.ANTHROPIC -> CHAT_MODEL
        Provider.OPENROUTER -> OPENROUTER_CHAT_MODEL
        Provider.OPENAI -> OPENAI_CHAT_MODEL
    }

    /**
     * Get the default action model for a provider.
     *
     * @param provider The API provider
     * @return The default action model ID for this provider
     */
    fun defaultActionModel(provider: Provider): String = when (provider) {
        Provider.ANTHROPIC -> ACTION_MODEL
        Provider.OPENROUTER -> OPENROUTER_ACTION_MODEL
        Provider.OPENAI -> OPENAI_ACTION_MODEL
    }

    /**
     * All known model IDs across all providers, for validation.
     * This is the hardcoded fallback catalog; prefer [runtimeKnownModels] for
     * live+cached provider-aware validation.
     */
    fun allKnownModels(provider: Provider): Set<String> =
        (chatModelsForProvider(provider) + actionModelsForProvider(provider)).toSet()

    /**
     * Runtime-resolved chat model IDs for a provider.
     *
     * Uses cached live catalog when present; otherwise falls back to static
     * curated defaults.
     */
    fun runtimeChatModels(provider: Provider): List<String> {
        val cached = ModelCatalog.getCachedModels(provider)
            ?.map { it.id }
            ?.distinct()
            .orEmpty()
        if (cached.isEmpty()) return chatModelsForProvider(provider)

        // Keep curated models first (stable UX), then append additional runtime models.
        val curated = chatModelsForProvider(provider)
        val runtimeOrdered = (curated + cached).distinct()
        val cap = if (provider == Provider.OPENROUTER) 24 else 12
        return runtimeOrdered.take(cap)
    }

    /**
     * Runtime-resolved action model IDs for a provider.
     *
     * Uses cached live catalog when present and enforces the model floor. If no
     * cached options remain, falls back to curated static action models.
     */
    fun runtimeActionModels(provider: Provider): List<String> {
        val cached = ModelCatalog.getCachedModels(provider)
            ?.filter { isModelAboveFloor(provider, it.id) }
            ?.map { it.id }
            ?.distinct()
            .orEmpty()
        return if (cached.isNotEmpty()) cached else actionModelsForProvider(provider)
    }

    /** Runtime-known model IDs for validation (chat + action). */
    fun runtimeKnownModels(provider: Provider): Set<String> =
        (runtimeChatModels(provider) + runtimeActionModels(provider)).toSet()

    /**
     * Validate a model ID against a catalog for a provider.
     *
     * @param provider The API provider
     * @param modelId The model ID to validate
     * @param knownModels Allowed model IDs for this provider. Defaults to static known models.
     * @return A validation result with suggestion if invalid
     */
    fun validateModel(
        provider: Provider,
        modelId: String,
        knownModels: Set<String> = allKnownModels(provider)
    ): ModelValidation {
        if (modelId.isBlank()) {
            return ModelValidation(valid = false, message = "Model ID cannot be blank.")
        }
        val known = if (knownModels.isNotEmpty()) knownModels else allKnownModels(provider)
        if (modelId in known) return ModelValidation(valid = true)

        val suggestion = known.minByOrNull { levenshtein(it, modelId) }
        return ModelValidation(
            valid = false,
            message = "Unknown model \"$modelId\" for ${provider.name}. " +
                "Known models: ${known.joinToString(", ")}." +
                if (suggestion != null) " Did you mean \"$suggestion\"?" else ""
        )
    }

    /**
     * Levenshtein edit distance between two strings.
     * Used for typo detection — finds the closest known model ID to suggest.
     * Returns the minimum number of single-character insertions, deletions,
     * or substitutions needed to transform [a] into [b].
     * Uses dynamic programming: dp[i][j] = edit distance between a[0..i) and b[0..j).
     */
    private fun levenshtein(a: String, b: String): Int {
        val dp = Array(a.length + 1) { IntArray(b.length + 1) }
        for (i in 0..a.length) dp[i][0] = i   // deleting all chars from a
        for (j in 0..b.length) dp[0][j] = j   // inserting all chars of b
        for (i in 1..a.length) {
            for (j in 1..b.length) {
                dp[i][j] = minOf(
                    dp[i - 1][j] + 1,         // delete from a
                    dp[i][j - 1] + 1,         // insert into a
                    dp[i - 1][j - 1] + if (a[i - 1] == b[j - 1]) 0 else 1  // substitute
                )
            }
        }
        return dp[a.length][b.length]
    }

    /**
     * Check whether a model meets the minimum capability floor for the action loop.
     *
     * The action loop processes untrusted screen content, so only Sonnet-tier
     * (STANDARD) and above models are permitted. Delegates to [ModelClassifier]
     * for pattern-based tier classification — no hardcoded blocklist.
     *
     * @param provider The API provider (kept for API compatibility; classification is provider-agnostic)
     * @param modelId The model ID to check
     * @return true if the model is at or above the minimum capability floor
     */
    @Suppress("UNUSED_PARAMETER")
    fun isModelAboveFloor(provider: Provider, modelId: String): Boolean {
        return ModelClassifier.isAboveFloor(modelId)
    }

    /** Human-readable description of allowed action models (for error messages). */
    const val MODEL_FLOOR_DESCRIPTION =
        "Action model must be Sonnet-tier or above (e.g. Claude Sonnet, Claude Opus, GPT-4o, o1). " +
        "Haiku variants and GPT-4o-mini are not permitted for the action loop."

    data class ModelValidation(
        val valid: Boolean,
        val message: String? = null
    )

    /**
     * Get the action model for a given chat model selection.
     *
     * Always returns the provider's default action model (Sonnet-tier minimum)
     * regardless of [chatModelId], because the action loop processes untrusted
     * screen content and requires a minimum capability floor.
     *
     * @param provider The API provider
     * @param chatModelId The selected chat model (unused — action model is always the provider default)
     * @return The default action model ID for this provider (always above floor)
     */
    @Deprecated(
        message = "Use defaultActionModel(provider) directly. The chatModelId parameter is unused " +
            "because the model floor policy requires a fixed Sonnet-tier minimum for the action loop.",
        replaceWith = ReplaceWith("defaultActionModel(provider)")
    )
    @Suppress("UNUSED_PARAMETER")
    fun actionModelForChat(provider: Provider, chatModelId: String): String {
        return defaultActionModel(provider)
    }
}
