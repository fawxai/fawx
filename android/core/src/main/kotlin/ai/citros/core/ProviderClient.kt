package ai.citros.core

/**
 * Provider-agnostic client contract for cloud LLM providers.
 *
 * Implementations handle provider-specific HTTP formats while exposing a
 * common API to the rest of the app.
 */
interface ProviderClient {
    val provider: Provider

    /** The model ID used for chat requests (e.g. "claude-sonnet-4-5-20250929"). */
    val modelId: String?
        get() = null

    suspend fun chat(conversation: Conversation): Result<String>

    /**
     * Non-streaming chat that can return usage metadata when supported.
     *
     * Default implementation delegates to [chat] and returns null usage for
     * providers that do not expose usage via this path.
     */
    suspend fun chatWithUsage(conversation: Conversation): Result<Pair<String, TokenUsage?>> =
        chat(conversation).map { it to null }

    suspend fun chatWithTools(
        messages: List<Message>,
        systemPrompt: String? = null,
        tools: List<Tool> = PhoneTools.ALL,
        tokenLimit: Int? = null
    ): Result<ChatResponse>

    /**
     * Describe an image using the provider's vision model.
     *
     * @param base64Image Base64-encoded PNG image data
     * @param prompt Text prompt to guide the description (defaults to [PhoneAgentPrompts.DEFAULT_VISION_PROMPT])
     * @param maxTokens Maximum tokens for the response (defaults to [DEFAULT_VISION_MAX_TOKENS] = 1024)
     * @return Text description of the image, or failure
     */
    suspend fun describeImage(
        base64Image: String,
        prompt: String = PhoneAgentPrompts.DEFAULT_VISION_PROMPT,
        maxTokens: Int = DEFAULT_VISION_MAX_TOKENS
    ): Result<String>

    /**
     * Stream a chat response, calling [onDelta] for each text token as it arrives.
     *
     * Returns the complete assembled text on success, same as [chat].
     * Default implementation falls back to non-streaming [chat] and emits
     * the full text as a single delta.
     *
     * @param conversation The conversation to send
     * @param onDelta Called with each text fragment as it arrives from the SSE stream
     * @return Complete response text on success, or failure
     */
    suspend fun chatStreaming(
        conversation: Conversation,
        onDelta: (String) -> Unit
    ): Result<String> = chat(conversation).also { result ->
        result.onSuccess { onDelta(it) }
    }

    companion object {
        /** Default max_tokens for vision requests. */
        const val DEFAULT_VISION_MAX_TOKENS = 1024
    }
}
