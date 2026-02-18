package ai.citros.core

/**
 * OpenAI provider client.
 *
 * Delegates to [OpenAiCompatibleClientImpl] using OpenAI provider config.
 */
class OpenAiClient private constructor(
    private val delegate: OpenAiCompatibleClientImpl
) : ProviderClient by delegate {

    constructor(
        apiKey: String,
        systemPrompt: String = BaseProviderClient.DEFAULT_SYSTEM_PROMPT,
        maxTokens: Int = 4096,
        maxAttempts: Int = 4
    ) : this(
        OpenAiCompatibleClientImpl(
            config = ProviderConfig.openAi(apiKey),
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
            require(config.provider == Provider.OPENAI) {
                "OpenAiClient requires Provider.OPENAI config, got ${config.provider}"
            }
            return OpenAiCompatibleClientImpl(config, systemPrompt, maxTokens, maxAttempts)
        }
    }
}
