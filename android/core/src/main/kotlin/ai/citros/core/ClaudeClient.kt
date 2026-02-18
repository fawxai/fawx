package ai.citros.core

/**
 * Legacy ClaudeClient for backward compatibility.
 *
 * This class is now an alias for [AnthropicClient]. New code should use
 * [AnthropicClient] directly or the [ProviderClient] interface.
 *
 * @see AnthropicClient
 */
@Deprecated(
    message = "Use AnthropicClient instead. ClaudeClient is maintained for backward compatibility only.",
    replaceWith = ReplaceWith("AnthropicClient", "ai.citros.core.AnthropicClient"),
    level = DeprecationLevel.WARNING
)
class ClaudeClient : ProviderClient {
    private val delegate: AnthropicClient

    /**
     * Legacy constructor for backward compatibility.
     */
    constructor(
        apiKey: String = "",
        model: String = ModelConfig.DEFAULT_MODEL,
        systemPrompt: String = DEFAULT_SYSTEM_PROMPT,
        maxTokens: Int = 4096,
        maxAttempts: Int = 4,
        baseUrl: String = "https://api.anthropic.com/v1/messages",
        providerConfig: ProviderConfig? = null
    ) {
        delegate = if (providerConfig != null) {
            // If providerConfig is provided, use it (may be OpenAI/OpenRouter for legacy multi-provider support)
            // This path is DEPRECATED - use provider-specific clients instead
            when (providerConfig.provider) {
                Provider.ANTHROPIC -> AnthropicClient(
                    config = providerConfig,
                    systemPrompt = systemPrompt,
                    maxTokens = maxTokens,
                    maxAttempts = maxAttempts
                )
                // For OpenAI/OpenRouter, we can't delegate to AnthropicClient
                // This is the legacy behavior that violates SRP - warn but maintain compatibility
                else -> throw UnsupportedOperationException(
                    "ClaudeClient with OpenAI/OpenRouter config is deprecated. Use OpenAiClient or OpenRouterClient instead."
                )
            }
        } else {
            // Legacy Anthropic-only path
            AnthropicClient(
                apiKey = apiKey,
                model = model,
                systemPrompt = systemPrompt,
                maxTokens = maxTokens,
                maxAttempts = maxAttempts,
                baseUrl = baseUrl
            )
        }
    }

    /**
     * Construct with provider configuration.
     */
    constructor(
        config: ProviderConfig,
        systemPrompt: String = DEFAULT_SYSTEM_PROMPT,
        maxTokens: Int = 4096,
        maxAttempts: Int = 4
    ) {
        delegate = when (config.provider) {
            Provider.ANTHROPIC -> AnthropicClient(
                config = config,
                systemPrompt = systemPrompt,
                maxTokens = maxTokens,
                maxAttempts = maxAttempts
            )
            else -> throw UnsupportedOperationException(
                "ClaudeClient with ${config.provider} config is deprecated. Use OpenAiClient or OpenRouterClient instead."
            )
        }
    }

    override val provider: Provider get() = delegate.provider

    override suspend fun chat(conversation: Conversation): Result<String> {
        return delegate.chat(conversation)
    }

    override suspend fun chatWithTools(
        messages: List<Message>,
        systemPrompt: String?,
        tools: List<Tool>,
        tokenLimit: Int?
    ): Result<ChatResponse> {
        return delegate.chatWithTools(messages, systemPrompt, tools, tokenLimit)
    }

    override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
        return delegate.describeImage(base64Image, prompt, maxTokens)
    }

    /** Token type classification for UI display purposes. */
    enum class TokenType {
        API_KEY,      // sk-ant-api03-*  (pay-per-token)
        SETUP_TOKEN,  // sk-ant-oat01-*  (Claude subscription via setup-token)
        UNKNOWN
    }

    companion object {
        const val DEFAULT_SYSTEM_PROMPT = BaseProviderClient.DEFAULT_SYSTEM_PROMPT

        /** Identify whether a credential is an API key, setup token, or unknown. */
        fun identifyTokenType(token: String): TokenType {
            val anthropicType = AnthropicClient.identifyTokenType(token)
            return when (anthropicType) {
                AnthropicClient.TokenType.API_KEY -> TokenType.API_KEY
                AnthropicClient.TokenType.SETUP_TOKEN -> TokenType.SETUP_TOKEN
                AnthropicClient.TokenType.UNKNOWN -> TokenType.UNKNOWN
            }
        }
    }
}
