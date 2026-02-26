package ai.citros.test

import ai.citros.core.ChatResponse
import ai.citros.core.Conversation
import ai.citros.core.Message
import ai.citros.core.Provider
import ai.citros.core.ProviderClient
import ai.citros.core.ProviderException
import ai.citros.core.TokenUsage
import ai.citros.core.Tool

class ScriptedProviderClient(
    override val provider: Provider,
    private val chatResponses: ArrayDeque<String> = ArrayDeque<String>(),
    private val chatWithUsageResponses: ArrayDeque<Pair<String, TokenUsage?>> = ArrayDeque<Pair<String, TokenUsage?>>(),
    private val streamingResponses: ArrayDeque<List<String>> = ArrayDeque<List<String>>(),
    private val toolResponses: ArrayDeque<ChatResponse> = ArrayDeque<ChatResponse>(),
    private val visionResponses: ArrayDeque<String> = ArrayDeque<String>(),
    override val modelId: String? = null,
    private val toolResponseResults: ArrayDeque<Result<ChatResponse>> = ArrayDeque<Result<ChatResponse>>()
) : ProviderClient {
    var chatCalls = 0
    var chatWithUsageCalls = 0
    var chatStreamingCalls = 0
    var chatWithToolsCalls = 0
    var describeImageCalls = 0

    /** Last messages list passed to chatWithTools, for verifying conversation flow. */
    var lastMessages: List<Message>? = null

    /** Last system prompt passed to chatWithTools. */
    var lastSystemPrompt: String? = null

    /** Last tool list passed to chatWithTools. */
    var lastTools: List<Tool>? = null

    override suspend fun chat(conversation: Conversation): Result<String> {
        chatCalls++
        return Result.success(chatResponses.removeFirst())
    }

    override suspend fun chatWithUsage(conversation: Conversation): Result<Pair<String, TokenUsage?>> {
        chatWithUsageCalls++
        return if (chatWithUsageResponses.isNotEmpty()) {
            Result.success(chatWithUsageResponses.removeFirst())
        } else {
            Result.success(chat(conversation).getOrThrow() to null)
        }
    }

    override suspend fun chatStreaming(
        conversation: Conversation,
        onDelta: (String) -> Unit
    ): Result<String> {
        chatStreamingCalls++
        val chunks = if (streamingResponses.isNotEmpty()) {
            streamingResponses.removeFirst()
        } else {
            listOf(chatResponses.removeFirst())
        }
        chunks.forEach(onDelta)
        return Result.success(chunks.joinToString(""))
    }

    override suspend fun chatWithTools(
        messages: List<Message>,
        systemPrompt: String?,
        tools: List<Tool>,
        tokenLimit: Int?
    ): Result<ChatResponse> {
        chatWithToolsCalls++
        lastMessages = messages.toList()
        lastSystemPrompt = systemPrompt
        lastTools = tools.toList()
        return if (toolResponseResults.isNotEmpty()) {
            toolResponseResults.removeFirst()
        } else {
            Result.success(toolResponses.removeFirst())
        }
    }

    override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
        describeImageCalls++
        return if (visionResponses.isNotEmpty()) {
            Result.success(visionResponses.removeFirst())
        } else {
            Result.failure(ProviderException(provider, null, "No vision response", false))
        }
    }
}
