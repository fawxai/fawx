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
        return executeTextChat(conversation).map { it.text!! }
    }

    override suspend fun chatWithUsage(conversation: Conversation): Result<Pair<String, TokenUsage?>> {
        return executeTextChat(conversation).map { response -> response.text!! to response.usage }
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
        val sanitizedChatMessages = sanitizeChatMessagesForApi(conversation.toApiMessages())

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
                sanitizedChatMessages.forEach { msg ->
                    addJsonObject {
                        put("role", msg.getValue("role"))
                        put("content", msg.getValue("content"))
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
     * Sanitize messages before sending to the API.
     *
     * Enforces Anthropic tool-call invariants in both directions:
     * - every tool_result must reference a tool_use in the immediately
     *   preceding assistant message
     * - every assistant tool_use must have a corresponding subsequent tool_result
     *
     * Invalid tool state is degraded to plain text context to prevent API
     * rejection while preserving conversational continuity.
     *
     * Also ensures the first message is role="user" (not a tool message).
     *
     * @param messages Raw message list
     * @return Sanitized message list safe for the Anthropic Messages API
     */
    internal fun sanitizeMessages(messages: List<Message>): List<Message> {
        if (messages.isEmpty()) return messages

        // First pass: if an assistant tool batch is incomplete (missing tool_result
        // for one or more tool_use IDs), degrade that assistant message to plain
        // text so the unmatched tool_use blocks are never sent to Anthropic.
        val toolUseSanitized = sanitizeIncompleteAssistantToolUse(messages)

        val result = mutableListOf<Message>()

        // Drop leading tool messages (they have no preceding assistant)
        var startIdx = 0
        while (startIdx < toolUseSanitized.size && toolUseSanitized[startIdx].role == Message.ROLE_TOOL) {
            Log.w("CitrosAPI", "sanitizeMessages: dropping leading tool message at index $startIdx (toolCallId=${toolUseSanitized[startIdx].toolCallId})")
            // Convert to plain user text so context isn't lost
            result.add(Message(role = Message.ROLE_USER, content = "[tool result] ${toolUseSanitized[startIdx].content.take(200)}"))
            startIdx++
        }

        // Process remaining messages, checking tool_result/tool_use pairing
        var lastAssistantToolIds: Set<String> = emptySet()
        for (i in startIdx until toolUseSanitized.size) {
            val msg = toolUseSanitized[i]
            when (msg.role) {
                Message.ROLE_ASSISTANT -> {
                    // Extract tool_use IDs from this assistant message
                    lastAssistantToolIds = msg.contentBlocks
                        ?.filter { it["type"] == "tool_use" }
                        ?.mapNotNull { it["id"] as? String }
                        ?.toSet()
                        ?: emptySet()
                    result.add(msg)
                }
                Message.ROLE_TOOL -> {
                    val toolCallId = msg.toolCallId
                    if (toolCallId != null && toolCallId in lastAssistantToolIds) {
                        result.add(msg)
                    } else {
                        // Orphaned tool_result: convert to plain text
                        Log.w("CitrosAPI", "sanitizeMessages: orphaned tool_result at index $i " +
                            "(toolCallId=$toolCallId, expected one of $lastAssistantToolIds). Converting to text.")
                        result.add(Message(role = Message.ROLE_USER, content = "[tool result] ${msg.content.take(200)}"))
                    }
                }
                else -> {
                    lastAssistantToolIds = emptySet()
                    result.add(msg)
                }
            }
        }

        // Merge consecutive same-role user messages that may have been created
        val merged = mutableListOf<Message>()
        for (msg in result) {
            if (merged.isNotEmpty() && merged.last().role == msg.role && msg.role == Message.ROLE_USER && msg.contentBlocks == null && merged.last().contentBlocks == null) {
                val prev = merged.removeAt(merged.lastIndex)
                merged.add(Message(role = Message.ROLE_USER, content = prev.content + "\n" + msg.content))
            } else {
                merged.add(msg)
            }
        }

        return merged
    }

    /**
     * Sanitize assistant tool-use messages when a tool batch is partially completed.
     *
     * Assumption (documented): Anthropic pairing is evaluated against immediately
     * adjacent ROLE_TOOL messages that follow the assistant tool batch. Any
     * non-tool gap ends that batch; later tool results are treated as unrelated.
     *
     * Behavior:
     * - complete batch: keep unchanged
     * - partial batch: preserve only matched tool_use blocks; degrade only unmatched
     *   tool_use blocks by rebuilding assistant message with surviving tool calls
     * - no matched results: fully degrade assistant message to plain text
     */
    private fun sanitizeIncompleteAssistantToolUse(messages: List<Message>): List<Message> {
        val sanitized = messages.toMutableList()

        for (index in sanitized.indices) {
            val message = sanitized[index]
            if (message.role != Message.ROLE_ASSISTANT) continue

            val toolUseBlocks = message.contentBlocks
                ?.filter { it["type"] == "tool_use" }
                .orEmpty()
            val expectedToolIds = toolUseBlocks
                .mapNotNull { it["id"] as? String }
                .toSet()

            if (expectedToolIds.isEmpty()) continue

            val observedToolResultIds = mutableSetOf<String>()
            var cursor = index + 1
            while (cursor < sanitized.size && sanitized[cursor].role == Message.ROLE_TOOL) {
                sanitized[cursor].toolCallId?.let { observedToolResultIds.add(it) }
                cursor++
            }

            if (observedToolResultIds.containsAll(expectedToolIds)) continue

            val preservedIds = expectedToolIds.intersect(observedToolResultIds)
            val missingIds = expectedToolIds - observedToolResultIds
            val fallbackText = message.content
                .substringBefore(Message.TOOL_CALLS_MARKER)
                .ifBlank { "Previous tool request was interrupted." }

            if (preservedIds.isEmpty()) {
                Log.w(
                    "CitrosAPI",
                    "sanitizeMessages: assistant tool_use without tool_result at index $index " +
                        "(missing=$missingIds, preservedTextLen=0, fallbackTextLen=${fallbackText.length}). " +
                        "Converting assistant tool batch to text."
                )
                sanitized[index] = Message(
                    role = Message.ROLE_ASSISTANT,
                    content = fallbackText,
                    timestamp = message.timestamp
                )
                continue
            }

            val preservedToolCalls = toolUseBlocks.mapNotNull { block ->
                val id = block["id"] as? String ?: return@mapNotNull null
                if (id !in preservedIds) return@mapNotNull null
                val name = block["name"] as? String ?: return@mapNotNull null
                @Suppress("UNCHECKED_CAST")
                val input = (block["input"] as? Map<String, Any>).orEmpty()
                ToolCall(id = id, name = name, input = input)
            }

            val rebuilt = Message.assistantWithTools(
                text = fallbackText.takeIf { it.isNotBlank() },
                toolCalls = preservedToolCalls
            )
                // Keep original timestamp while preserving assistantWithTools-built _contentBlocks.
                .copy(timestamp = message.timestamp)

            Log.w(
                "CitrosAPI",
                "sanitizeMessages: partial assistant tool_use at index $index " +
                    "(preserved=$preservedIds, missing=$missingIds, preservedTextLen=${rebuilt.content.length}, fallbackTextLen=${fallbackText.length}). " +
                    "Dropping unmatched tool_use blocks."
            )

            sanitized[index] = rebuilt
        }

        return sanitized
    }

    /**
     * Anthropic rejects assistant text that ends with trailing whitespace.
     *
     * We intentionally use [trimEnd] (not [trim]) so we preserve leading
     * whitespace/indentation (for example Markdown/code formatting) while
     * removing the API-invalid suffix.
     */
    private fun sanitizeAssistantTextForApi(text: String): String = text.trimEnd()

    /**
     * Return sanitized assistant text or null when trimming would produce empty content.
     *
     * Anthropic rejects empty assistant text payloads, so callers should drop
     * assistant messages/blocks that become empty after sanitization.
     */
    private fun sanitizeAssistantTextForApiOrNull(text: String): String? =
        sanitizeAssistantTextForApi(text).takeIf { it.isNotEmpty() }

    /**
     * Apply Anthropic assistant-text sanitization to chat-mode API messages.
     *
     * This keeps sanitization centralized for both [buildChatRequest] and
     * [buildToolRequest] and drops assistant messages that would serialize as
     * empty strings.
     */
    private fun sanitizeChatMessagesForApi(messages: List<Map<String, String>>): List<Map<String, String>> {
        return messages.mapNotNull { message ->
            if (message["role"] != Message.ROLE_ASSISTANT) {
                return@mapNotNull message
            }

            val rawContent = message["content"] ?: return@mapNotNull null
            val sanitizedContent = sanitizeAssistantTextForApiOrNull(rawContent)
                ?: return@mapNotNull null

            message + ("content" to sanitizedContent)
        }
    }

    private fun sanitizeAssistantContentBlocksForApi(blocks: List<Map<String, Any>>): List<Map<String, Any>> {
        return blocks.mapNotNull { block ->
            if (block["type"] != "text") {
                return@mapNotNull block
            }

            val rawText = block["text"] as? String ?: return@mapNotNull block
            val sanitizedText = sanitizeAssistantTextForApiOrNull(rawText)
                ?: return@mapNotNull null

            block + ("text" to sanitizedText)
        }
    }

    internal override fun buildToolRequest(
        messages: List<Message>,
        systemPrompt: String,
        tools: List<Tool>,
        maxTokens: Int
    ): JsonObject {
        val sanitizedMessages = sanitizeMessages(messages)
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
                while (i < sanitizedMessages.size) {
                    val msg = sanitizedMessages[i]
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
                                    while (i + 1 < sanitizedMessages.size && sanitizedMessages[i + 1].role == "tool") {
                                        i++
                                        sanitizedMessages[i].contentBlocks?.forEach { block ->
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
                            val sanitizedBlocks = sanitizeAssistantContentBlocksForApi(blocks)
                            if (sanitizedBlocks.isEmpty()) {
                                Log.w(
                                    "CitrosAPI",
                                    "buildToolRequest: dropping assistant content-block message with empty text after sanitization"
                                )
                            } else {
                                addJsonObject {
                                    put("role", "assistant")
                                    putJsonArray("content") {
                                        sanitizedBlocks.forEach { block ->
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
                        // Regular text message
                        else -> {
                            val content = if (msg.role == Message.ROLE_ASSISTANT) {
                                sanitizeAssistantTextForApiOrNull(msg.content)
                            } else {
                                msg.content
                            }

                            if (content == null) {
                                Log.w(
                                    "CitrosAPI",
                                    "buildToolRequest: dropping assistant text message with empty content after sanitization"
                                )
                            } else {
                                addJsonObject {
                                    put("role", msg.role)
                                    put("content", content)
                                }
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
