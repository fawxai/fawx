package ai.citros.core

import android.util.Log
import kotlinx.serialization.json.*

/**
 * Anthropic provider client implementation.
 *
 * Handles Anthropic-specific Messages API protocol with prompt caching support.
 * System prompts and tool definitions are cached for 5 minutes (TTL refreshed on hit).
 * 
 * Caching impact:
 * - First request: +25% token cost (cache write)
 * - Subsequent requests within TTL: -90% token cost (cache read)
 */
class AnthropicClient : BaseProviderClient {

    /**
     * Legacy constructor for backward compatibility.
     */
    constructor(
        apiKey: String,
        model: String = ModelConfig.DEFAULT_MODEL,
        systemPrompt: String = DEFAULT_SYSTEM_PROMPT,
        maxTokens: Int = 4096,
        maxAttempts: Int = 4,
        baseUrl: String = "https://api.anthropic.com/v1/messages"
    ) : super(
        config = ProviderConfig(
            provider = Provider.ANTHROPIC,
            baseUrl = baseUrl,
            chatModelId = model,
            actionModelId = model,
            headers = mapOf(
                "x-api-key" to apiKey,
                "anthropic-version" to ProviderConfig.ANTHROPIC_API_VERSION,
                "anthropic-beta" to ProviderConfig.ANTHROPIC_PROMPT_CACHING_BETA
            )
        ),
        systemPrompt = systemPrompt,
        maxTokens = maxTokens,
        maxAttempts = maxAttempts
    )

    /**
     * Construct with provider configuration.
     */
    constructor(
        config: ProviderConfig,
        systemPrompt: String = DEFAULT_SYSTEM_PROMPT,
        maxTokens: Int = 4096,
        maxAttempts: Int = 4
    ) : super(config, systemPrompt, maxTokens, maxAttempts) {
        require(config.provider == Provider.ANTHROPIC) {
            "AnthropicClient requires Provider.ANTHROPIC config, got ${config.provider}"
        }
        require(config.headers["anthropic-beta"] == ProviderConfig.ANTHROPIC_PROMPT_CACHING_BETA) {
            "AnthropicClient requires anthropic-beta: ${ProviderConfig.ANTHROPIC_PROMPT_CACHING_BETA} header for prompt caching"
        }
    }

    override suspend fun chat(conversation: Conversation): Result<String> {
        return executeRequest(
            requestBody = buildChatRequest(conversation),
            parseResponse = { jsonResponse ->
                val text = parseChatResponse(jsonResponse)
                    ?: throw ProviderException(
                        provider = provider,
                        statusCode = null,
                        message = "API returned no text content in response",
                        isAuthFailure = false
                    )
                ChatResponse(text, emptyList(), null, parseUsage(jsonResponse))
            }
        ).map { it.text!! }
    }

    /**
     * Stream a chat response using Anthropic's SSE streaming format.
     *
     * Anthropic SSE events:
     * - `content_block_delta` with `delta.type == "text_delta"` → text token
     * - `message_stop` → stream complete
     *
     * Also sends `anthropic-beta` header for prompt caching support during streaming.
     */
    override suspend fun chatStreaming(
        conversation: Conversation,
        onDelta: (String) -> Unit
    ): Result<String> {
        return executeStreamingRequest(
            requestBody = buildChatRequest(conversation),
            parseSSEDelta = ::parseAnthropicSSEDelta,
            isDone = ::isAnthropicStreamDone,
            onDelta = onDelta
        ).map { rawText ->
            // Strip tool artifacts from chat-mode streaming responses (same as non-streaming)
            PhoneAgentApi.stripToolArtifacts(rawText)
        }
    }

    override suspend fun chatWithTools(
        messages: List<Message>,
        systemPrompt: String?,
        tools: List<Tool>,
        tokenLimit: Int?
    ): Result<ChatResponse> {
        val effectiveSystemPrompt = systemPrompt ?: this.systemPrompt
        val effectiveTokenLimit = tokenLimit ?: this.maxTokens

        return executeRequest(
            requestBody = buildToolRequest(messages, effectiveSystemPrompt, tools, effectiveTokenLimit),
            parseResponse = { jsonResponse -> parseToolResponse(jsonResponse) }
        )
    }

    /**
     * Build request body for Anthropic Messages API.
     * Format: system as top-level field, messages array with user/assistant roles.
     */
    override fun buildChatRequest(conversation: Conversation): JsonObject {
        return buildJsonObject {
            put("model", config.chatModelId)
            put("max_tokens", maxTokens)
            putJsonArray("system") {
                addJsonObject {
                    put("type", "text")
                    put("text", systemPrompt)
                    addCacheControl()
                }
            }
            putJsonArray("messages") {
                conversation.toApiMessages().forEach { msg ->
                    addJsonObject {
                        put("role", msg["role"])
                        put("content", msg["content"])
                    }
                }
            }
        }
    }

    /**
     * Add cache_control block for Anthropic prompt caching.
     * Marks content for ephemeral caching (5 min TTL, -90% cost on cache reads).
     */
    private fun JsonObjectBuilder.addCacheControl() {
        putJsonObject("cache_control") {
            put("type", "ephemeral")
        }
    }

    /**
     * Build request body for Anthropic Messages API with tool support.
     */
    internal override fun buildToolRequest(
        messages: List<Message>,
        systemPrompt: String,
        tools: List<Tool>,
        maxTokens: Int
    ): JsonObject {
        return buildJsonObject {
            put("model", config.chatModelId)
            put("max_tokens", maxTokens)
            putJsonArray("system") {
                addJsonObject {
                    put("type", "text")
                    put("text", systemPrompt)
                    addCacheControl()
                }
            }

            // Add tools array (omitted for tool-free calls like sendEphemeral)
            if (tools.isNotEmpty()) putJsonArray("tools") {
                tools.forEachIndexed { index, tool ->
                    addJsonObject {
                        put("name", tool.name)
                        put("description", tool.description)
                        putJsonObject("input_schema") {
                            tool.inputSchema.forEach { (key, value) ->
                                put(key, anyToJsonElement(value))
                            }
                        }
                        // Cache the last tool (Anthropic caches everything up to and including the marked block)
                        if (index == tools.lastIndex) {
                            addCacheControl()
                        }
                    }
                }
            }

            // Add messages array.
            // Anthropic requires: (1) alternating user/assistant roles,
            // (2) all tool_results for a batch in ONE user message.
            // Our internal model stores each tool result as a separate Message,
            // so we must merge consecutive tool messages into one user message.
            //
            // Note: contentBlocks is non-null only for tool result messages (role="tool")
            // and assistant messages with tool calls. Regular user/assistant text messages
            // always have contentBlocks=null and fall through to the text branch.
            putJsonArray("messages") {
                var i = 0
                while (i < messages.size) {
                    val msg = messages[i]
                    val blocks = msg.contentBlocks
                    when {
                        // Tool result message(s): merge consecutive tool messages
                        // into a single role="user" message with all tool_result blocks.
                        msg.role == "tool" && blocks != null -> {
                            addJsonObject {
                                put("role", "user")
                                putJsonArray("content") {
                                    // Emit blocks from this message
                                    blocks.forEach { block ->
                                        addJsonObject {
                                            block.forEach { (key, value) ->
                                                put(key, anyToJsonElement(value))
                                            }
                                        }
                                    }
                                    // Merge any immediately following tool messages
                                    while (i + 1 < messages.size && messages[i + 1].role == "tool") {
                                        i++
                                        messages[i].contentBlocks?.forEach { block ->
                                            addJsonObject {
                                                block.forEach { (key, value) ->
                                                    put(key, anyToJsonElement(value))
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Assistant message with tool calls: role="assistant" with content blocks
                        msg.role == "assistant" && blocks != null -> {
                            addJsonObject {
                                put("role", "assistant")
                                putJsonArray("content") {
                                    blocks.forEach { block ->
                                        addJsonObject {
                                            block.forEach { (key, value) ->
                                                put(key, anyToJsonElement(value))
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Regular text message
                        else -> {
                            addJsonObject {
                                put("role", msg.role)
                                put("content", msg.content)
                            }
                        }
                    }
                    i++
                }
            }
        }
    }

    /**
     * Parse Anthropic response format.
     * Response: { "content": [{"type": "text", "text": "..."}], "role": "assistant" }
     */
    override fun parseChatResponse(jsonResponse: JsonObject): String? {
        val contentArray = jsonResponse["content"]?.jsonArray
        if (contentArray == null || contentArray.isEmpty()) {
            return null
        }

        // Find the first text content block. The API may return other types
        // (e.g. tool_use) which we skip for now.
        return contentArray
            .mapNotNull { block ->
                val obj = block.jsonObject
                if (obj["type"]?.jsonPrimitive?.content == "text") {
                    obj["text"]?.jsonPrimitive?.content
                } else null
            }
            .firstOrNull()
    }

    /**
     * Parse Anthropic response with tool use support.
     * Response: { "content": [{"type": "text", "text": "..."}, {"type": "tool_use", "id": "...", "name": "...", "input": {...}}], "stop_reason": "..." }
     */
    internal override fun parseToolResponse(jsonResponse: JsonObject): ChatResponse {
        val contentArray = jsonResponse["content"]?.jsonArray
            ?: return ChatResponse(null, emptyList(), null)
        val stopReason = jsonResponse["stop_reason"]?.jsonPrimitive?.content

        var text: String? = null
        val toolCalls = mutableListOf<ToolCall>()

        contentArray.forEach { block ->
            val obj = block.jsonObject
            when (obj["type"]?.jsonPrimitive?.content) {
                "text" -> {
                    text = obj["text"]?.jsonPrimitive?.content
                }
                "tool_use" -> {
                    val id = obj["id"]?.jsonPrimitive?.content ?: return@forEach
                    val name = obj["name"]?.jsonPrimitive?.content ?: return@forEach
                    val inputObj = obj["input"]?.jsonObject ?: return@forEach

                    // Convert JsonObject to Map<String, Any>, preserving JSON types
                    val inputMap = parseJsonObjectToMap(inputObj)

                    toolCalls.add(ToolCall(id, name, inputMap))
                }
            }
        }

        return ChatResponse(text, toolCalls, stopReason, parseUsage(jsonResponse))
    }

    /**
     * Parse an Anthropic SSE data line and extract text delta.
     *
     * Returns the text fragment for `content_block_delta` events with
     * `text_delta` type, null for all other event types.
     */
    internal fun parseAnthropicSSEDelta(data: String): String? {
        return try {
            val jsonObj = json.parseToJsonElement(data).jsonObject
            if (jsonObj["type"]?.jsonPrimitive?.content == "content_block_delta") {
                val delta = jsonObj["delta"]?.jsonObject ?: return null
                if (delta["type"]?.jsonPrimitive?.content == "text_delta") {
                    delta["text"]?.jsonPrimitive?.content
                } else null
            } else null
        } catch (e: Exception) {
            Log.w("CitrosAPI", "Anthropic SSE parse error: ${e.message}, data=${data.take(80)}")
            null
        }
    }

    /**
     * Check if an Anthropic SSE data line signals stream completion.
     */
    internal fun isAnthropicStreamDone(data: String): Boolean {
        return try {
            val jsonObj = json.parseToJsonElement(data).jsonObject
            jsonObj["type"]?.jsonPrimitive?.content == "message_stop"
        } catch (_: Exception) { false }
    }

    /** Token type classification for UI display purposes. */
    enum class TokenType {
        API_KEY,      // sk-ant-api03-*  (pay-per-token)
        SETUP_TOKEN,  // sk-ant-oat01-*  (Claude subscription via setup-token)
        UNKNOWN
    }

    companion object {
        /** Identify whether a credential is an API key, setup token, or unknown. */
        fun identifyTokenType(token: String): TokenType {
            return when {
                token.startsWith("sk-ant-api") -> TokenType.API_KEY
                token.startsWith("sk-ant-oat") -> TokenType.SETUP_TOKEN
                else -> TokenType.UNKNOWN
            }
        }
    }
}
