package ai.citros.core

import android.util.Log
import kotlinx.serialization.json.*

/**
 * Shared implementation for OpenAI-compatible provider APIs.
 *
 * Handles both OpenAI and OpenRouter, which use identical Chat Completions
 * API protocols. The only differences are the base URL and API key header,
 * both of which are configured via [ProviderConfig].
 *
 * This eliminates ~258 lines of duplication between the former
 * `OpenAiClientImpl` and `OpenRouterClientImpl`.
 *
 * See: https://github.com/abbudjoe/citros/issues/495
 */
internal class OpenAiCompatibleClientImpl(
    config: ProviderConfig,
    systemPrompt: String = DEFAULT_SYSTEM_PROMPT,
    maxTokens: Int = 4096,
    maxAttempts: Int = 4
) : BaseProviderClient(config, systemPrompt, maxTokens, maxAttempts) {

    init {
        require(config.provider == Provider.OPENAI || config.provider == Provider.OPENROUTER) {
            "OpenAiCompatibleClientImpl requires OPENAI or OPENROUTER config, got ${config.provider}"
        }
    }

    override suspend fun chat(conversation: Conversation): Result<String> {
        return executeTextChat(conversation).map { it.text!! }
    }

    override suspend fun chatWithUsage(conversation: Conversation): Result<Pair<String, TokenUsage?>> {
        return executeTextChat(conversation).map { response -> response.text!! to response.usage }
    }

    /**
     * Stream a chat response using OpenAI-compatible SSE streaming format.
     *
     * OpenAI SSE events:
     * - `data: {"choices":[{"delta":{"content":"token"}}]}` → text token
     * - `data: [DONE]` → stream complete
     *
     * Works for both OpenAI and OpenRouter (identical streaming protocol).
     */
    override suspend fun chatStreaming(
        conversation: Conversation,
        onDelta: (String) -> Unit
    ): Result<String> {
        return executeStreamingRequest(
            requestBody = buildChatRequest(conversation),
            parseSSEDelta = ::parseOpenAiSSEDelta,
            isDone = ::isOpenAiStreamDone,
            onDelta = onDelta
        )
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
     * Build request body for OpenAI-compatible Chat Completions API.
     * Format: system as first message with role "system", then user/assistant messages.
     */
    override fun buildChatRequest(conversation: Conversation): JsonObject {
        return buildJsonObject {
            put("model", config.chatModelId)
            put("max_tokens", maxTokens)
            putJsonArray("messages") {
                addJsonObject {
                    put("role", "system")
                    put("content", systemPrompt)
                }
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
     * Build request body for OpenAI-compatible Chat Completions API with tool support.
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

            if (tools.isNotEmpty()) putJsonArray("tools") {
                tools.forEach { tool ->
                    addJsonObject {
                        put("type", "function")
                        putJsonObject("function") {
                            put("name", tool.name)
                            put("description", tool.description)
                            putJsonObject("parameters") {
                                tool.inputSchema.forEach { (key, value) ->
                                    put(key, anyToJsonElement(value))
                                }
                            }
                        }
                    }
                }
            }

            putJsonArray("messages") {
                addJsonObject {
                    put("role", "system")
                    put("content", systemPrompt)
                }

                messages.forEach { msg ->
                    addJsonObject {
                        put("role", msg.role)

                        when {
                            msg.role == "assistant" && msg.toolCallsJson != null -> {
                                if (msg.content.isNullOrBlank() || msg.content.startsWith("[Tools:")) {
                                    put("content", JsonNull)
                                } else {
                                    put("content", msg.content)
                                }
                                val toolCallsArray = json.parseToJsonElement(msg.toolCallsJson).jsonArray
                                putJsonArray("tool_calls") {
                                    toolCallsArray.forEach { tc ->
                                        val tcObj = tc.jsonObject
                                        addJsonObject {
                                            put("id", tcObj["id"]?.jsonPrimitive?.content ?: "")
                                            put("type", "function")
                                            putJsonObject("function") {
                                                put("name", tcObj["name"]?.jsonPrimitive?.content ?: "")
                                                put("arguments", (tcObj["input"] ?: JsonNull).toString())
                                            }
                                        }
                                    }
                                }
                            }
                            msg.role == "tool" && msg.toolCallId != null -> {
                                put("content", msg.content ?: "")
                                put("tool_call_id", msg.toolCallId)
                            }
                            else -> {
                                put("content", msg.content ?: "")
                            }
                        }
                    }
                }
            }
        }
    }

    /**
     * Parse OpenAI-compatible response format.
     * Response: { "choices": [{"message": {"role": "assistant", "content": "..."}}] }
     */
    override fun parseChatResponse(jsonResponse: JsonObject): String? {
        val choicesArray = jsonResponse["choices"]?.jsonArray
        if (choicesArray == null || choicesArray.isEmpty()) {
            return null
        }

        val firstChoice = choicesArray[0].jsonObject
        val message = firstChoice["message"]?.jsonObject ?: return null
        val content = message["content"] ?: return null
        if (content is JsonNull) return null
        return content.jsonPrimitive.content
    }

    /**
     * Parse OpenAI-compatible response with tool use support.
     * Response: { "choices": [{"message": {"role": "assistant", "content": "...", "tool_calls": [...]}, "finish_reason": "..."}] }
     */
    internal override fun parseToolResponse(jsonResponse: JsonObject): ChatResponse {
        val choicesArray = jsonResponse["choices"]?.jsonArray
        if (choicesArray == null || choicesArray.isEmpty()) {
            return ChatResponse(null, emptyList(), null)
        }

        val firstChoice = choicesArray[0].jsonObject
        val message = firstChoice["message"]?.jsonObject
            ?: return ChatResponse(null, emptyList(), null)
        val finishReason = firstChoice["finish_reason"]?.jsonPrimitive?.content

        val content = message["content"]
        val text = when (content) {
            null, is JsonNull -> null
            else -> content.jsonPrimitive.content
        }
        val toolCallsArray = message["tool_calls"]?.jsonArray
        val toolCalls = mutableListOf<ToolCall>()

        toolCallsArray?.forEach { toolCallElement ->
            try {
                val toolCallObj = toolCallElement.jsonObject
                val id = toolCallObj["id"]?.jsonPrimitive?.content ?: return@forEach
                val functionObj = toolCallObj["function"]?.jsonObject ?: return@forEach
                val name = functionObj["name"]?.jsonPrimitive?.content ?: return@forEach
                val argumentsStr = functionObj["arguments"]?.jsonPrimitive?.content ?: return@forEach

                val argumentsJson = try {
                    json.parseToJsonElement(argumentsStr).jsonObject
                } catch (e: Exception) {
                    return@forEach
                }

                val inputMap = parseJsonObjectToMap(argumentsJson)

                toolCalls.add(ToolCall(id, name, inputMap))
            } catch (e: Exception) {
                // Skip malformed tool call entries
            }
        }

        return ChatResponse(text, toolCalls, finishReason, parseUsage(jsonResponse))
    }

    /**
     * Parse an OpenAI-compatible SSE data line and extract text delta.
     *
     * Returns the content fragment from `choices[0].delta.content`,
     * or null for non-content events (role-only deltas, empty deltas).
     */
    internal fun parseOpenAiSSEDelta(data: String): String? {
        return try {
            val jsonObj = json.parseToJsonElement(data).jsonObject
            jsonObj["choices"]?.jsonArray?.firstOrNull()?.jsonObject
                ?.get("delta")?.jsonObject
                ?.get("content")?.let { element ->
                    if (element is kotlinx.serialization.json.JsonNull) null
                    else element.jsonPrimitive.content
                }
        } catch (e: Exception) {
            Log.w("CitrosAPI", "OpenAI SSE parse error: ${e.message}, data=${data.take(80)}")
            null
        }
    }

    /**
     * Check if an OpenAI-compatible SSE data line signals stream completion.
     */
    internal fun isOpenAiStreamDone(data: String): Boolean = data == "[DONE]"

    /**
     * OpenAI-compatible vision uses image_url with data URI.
     */
    override fun buildVisionRequest(base64Image: String, prompt: String, maxTokens: Int): JsonObject {
        return buildJsonObject {
            put("model", config.chatModelId)
            put("max_tokens", maxTokens)
            putJsonArray("messages") {
                addJsonObject {
                    put("role", "user")
                    putJsonArray("content") {
                        addJsonObject {
                            put("type", "image_url")
                            putJsonObject("image_url") {
                                put("url", "data:image/png;base64,$base64Image")
                            }
                        }
                        addJsonObject {
                            put("type", "text")
                            put("text", prompt)
                        }
                    }
                }
            }
        }
    }
}
