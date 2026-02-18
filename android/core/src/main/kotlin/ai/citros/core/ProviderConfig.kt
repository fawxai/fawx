package ai.citros.core

/**
 * Supported API providers for LLM models.
 */
enum class Provider {
    /** Anthropic's direct API (api.anthropic.com). */
    ANTHROPIC,
    /** OpenRouter proxy API (openrouter.ai). */
    OPENROUTER,
    /** OpenAI direct API (api.openai.com). */
    OPENAI
}

/**
 * Configuration for an API provider.
 * 
 * @param provider The provider type
 * @param baseUrl API base URL
 * @param chatModelId Model ID for high-capability chat
 * @param actionModelId Model ID for fast action loop iterations
 * @param headers HTTP headers required by the provider
 */
data class ProviderConfig(
    val provider: Provider,
    val baseUrl: String,
    val chatModelId: String,
    val actionModelId: String,
    val headers: Map<String, String>
) {
    /**
     * Validate that configured model IDs are in the known catalog.
     * Returns a list of warnings for unknown models (empty if all valid).
     * Unknown models are allowed (user may have access to newer models)
     * but callers should surface warnings.
     *
     * Call sites:
     * - `ChatViewModel.updateModelsFromWallet()` — validates after wallet model selection
     * - `ChatActivity` startup — validates initial config
     * - Warnings should be logged via `Log.w` and optionally shown as a toast/snackbar
     *   so users know their model ID may be stale or mistyped.
     */
    fun validateModels(): List<String> {
        val warnings = mutableListOf<String>()
        val chatResult = ModelConfig.validateModel(provider, chatModelId)
        if (!chatResult.valid) {
            warnings += "Chat model: ${chatResult.message}"
        }
        val actionResult = ModelConfig.validateModel(provider, actionModelId)
        if (!actionResult.valid) {
            warnings += "Action model: ${actionResult.message}"
        }
        return warnings
    }

    companion object {
        /**
         * Anthropic API version used in the anthropic-version header.
         * Pinned to stable version to avoid breaking changes.
         */
        const val ANTHROPIC_API_VERSION = "2023-06-01"
        
        /**
         * Anthropic beta feature header for prompt caching.
         * Enables ~90% cost reduction on cached content (system prompt + tools).
         */
        internal const val ANTHROPIC_PROMPT_CACHING_BETA = "prompt-caching-2024-07-31"
        
        /**
         * Create configuration for Anthropic's direct API.
         * 
         * @param apiKey Anthropic API key (sk-ant-api03-*) or setup token (sk-ant-oat01-*)
         */
        fun anthropic(apiKey: String) = ProviderConfig(
            provider = Provider.ANTHROPIC,
            baseUrl = "https://api.anthropic.com/v1/messages",
            chatModelId = ModelConfig.CHAT_MODEL,
            actionModelId = ModelConfig.ACTION_MODEL,
            headers = mapOf(
                "x-api-key" to apiKey,
                "anthropic-version" to ANTHROPIC_API_VERSION,
                "anthropic-beta" to ANTHROPIC_PROMPT_CACHING_BETA
            )
        )
        
        /**
         * Create configuration for OpenRouter proxy API.
         * 
         * @param apiKey OpenRouter API key (sk-or-*)
         */
        fun openRouter(apiKey: String) = ProviderConfig(
            provider = Provider.OPENROUTER,
            baseUrl = "https://openrouter.ai/api/v1/chat/completions",
            chatModelId = ModelConfig.OPENROUTER_CHAT_MODEL,
            actionModelId = ModelConfig.OPENROUTER_ACTION_MODEL,
            headers = mapOf(
                "Authorization" to "Bearer $apiKey",
                "HTTP-Referer" to "https://citros.ai",
                "X-Title" to "Citros"
            )
        )
        
        /**
         * Create configuration for OpenAI direct API.
         * 
         * Works with both standard API keys and OAuth subscription tokens.
         * 
         * @param apiKey OpenAI API key (sk-*) or OAuth token
         */
        fun openAi(apiKey: String) = ProviderConfig(
            provider = Provider.OPENAI,
            baseUrl = "https://api.openai.com/v1/chat/completions",
            chatModelId = ModelConfig.OPENAI_CHAT_MODEL,
            actionModelId = ModelConfig.OPENAI_ACTION_MODEL,
            headers = mapOf(
                "Authorization" to "Bearer $apiKey"
            )
        )

        /**
         * Auto-detect provider from credential format, with optional explicit override.
         *
         * @param credential API key or OAuth token
         * @param preferredProvider Explicit provider selection from UI/settings. If set,
         * this takes precedence over auto-detection.
         * @return The detected provider, or null if unrecognized and no preference provided
         */
        fun detectProvider(
            credential: String,
            preferredProvider: Provider? = null
        ): Provider? {
            preferredProvider?.let { return it }

            val token = credential.trim()
            if (token.isEmpty()) return null

            return when {
                token.startsWith("sk-ant-api") || token.startsWith("sk-ant-oat") -> Provider.ANTHROPIC
                token.startsWith("sk-or-") -> Provider.OPENROUTER
                // OpenAI keys start with sk- (but not sk-ant- or sk-or-)
                token.startsWith("sk-") -> Provider.OPENAI
                // OpenAI OAuth tokens are often JWT-like or session-prefixed.
                isLikelyOpenAiOauthToken(token) -> Provider.OPENAI
                else -> null
            }
        }

        /**
         * Heuristic detection for OpenAI OAuth/subscription tokens.
         *
         * Checks for known OpenAI-specific token prefixes (sess-, oauth_, oa-).
         * Generic JWT detection is intentionally excluded to avoid misclassifying
         * tokens from other providers (Google, Firebase, Auth0, etc.).
         *
         * When `preferredProvider` is set in `detectProvider()`, it takes precedence over this
         * heuristic, allowing users to explicitly select OpenAI for OAuth tokens that don't
         * match these prefixes.
         */
        fun isLikelyOpenAiOauthToken(token: String): Boolean {
            val trimmed = token.trim()
            if (trimmed.isEmpty()) return false

            return trimmed.startsWith("sess-") || 
                   trimmed.startsWith("oauth_") || 
                   trimmed.startsWith("oa-")
        }
    }
}
