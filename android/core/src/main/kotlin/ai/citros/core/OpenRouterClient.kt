package ai.citros.core

/**
 * OpenRouter provider client.
 *
 * Delegates to [OpenAiCompatibleClientImpl] — OpenRouter uses the same
 * Chat Completions API protocol as OpenAI. The only differences are
 * the base URL and API key header, configured via [ProviderConfig].
 */
class OpenRouterClient private constructor(
    private val delegate: OpenAiCompatibleClientImpl
) : ProviderClient by delegate {

    constructor(
        apiKey: String,
        systemPrompt: String = BaseProviderClient.DEFAULT_SYSTEM_PROMPT,
        maxTokens: Int = 4096,
        maxAttempts: Int = 4
    ) : this(
        OpenAiCompatibleClientImpl(
            config = ProviderConfig.openRouter(apiKey),
            systemPrompt = systemPrompt,
            maxTokens = maxTokens,
            maxAttempts = maxAttempts
        )
    )

    constructor(
        config: ProviderConfig,
        systemPrompt: String = BaseProviderClient.DEFAULT_SYSTEM_PROMPT,
        maxTokens: Int = 4096,
        maxAttempts: Int = 4
    ) : this(
        validateAndBuild(config, systemPrompt, maxTokens, maxAttempts)
    )

    companion object {
        private fun validateAndBuild(
            config: ProviderConfig,
            systemPrompt: String,
            maxTokens: Int,
            maxAttempts: Int
        ): OpenAiCompatibleClientImpl {
            require(config.provider == Provider.OPENROUTER) {
                "OpenRouterClient requires Provider.OPENROUTER config, got ${config.provider}"
            }
            return OpenAiCompatibleClientImpl(config, systemPrompt, maxTokens, maxAttempts)
        }
    }
}
