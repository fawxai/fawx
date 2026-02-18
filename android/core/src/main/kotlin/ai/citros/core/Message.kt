package ai.citros.core

import kotlinx.serialization.Serializable
import kotlinx.serialization.Transient
import kotlinx.serialization.json.*

private val json = Json { ignoreUnknownKeys = true }

/**
 * A message in a conversation.
 * 
 * Supports regular text messages (user/assistant), tool result messages, and
 * assistant messages with tool calls. All message types can be serialized and
 * reconstructed correctly after deserialization.
 */
@Serializable
data class Message(
    val role: String,  // "user", "assistant", or "tool" (for OpenAI)
    val content: String,
    val timestamp: Long = System.currentTimeMillis(),
    // Tool-specific fields (persisted for reconstruction after deserialization)
    val toolCallId: String? = null,  // For tool result messages
    val toolCallsJson: String? = null,  // For assistant messages with tool calls (JSON array)
    val isError: Boolean = false,  // For tool result messages: propagated to API as is_error
    /** Tool name for tool result messages (e.g., "tap", "web_search"). Used for category-aware context trimming. */
    val toolName: String? = null,
    /** True if this is a mid-loop steer message sent while the agent was executing tools.
     *  Only applicable to role="user" messages. Persisted across serialization. */
    val isSteer: Boolean = false,
    @Transient
    private val _contentBlocks: List<Map<String, Any>>? = null  // Internal storage for content blocks
) {
    /**
     * Get the content blocks for Anthropic format.
     * 
     * If the message was deserialized from disk (where @Transient fields are lost),
     * this property will reconstruct the content blocks from persisted fields.
     * This ensures tool conversations can be persisted and reloaded correctly.
     */
    val contentBlocks: List<Map<String, Any>>?
        get() = _contentBlocks ?: when {
            // Tool result message: reconstruct from toolCallId and content
            role == "tool" && toolCallId != null -> listOf(
                buildMap {
                    put("type", "tool_result")
                    put("tool_use_id", toolCallId)
                    put("content", content)
                    if (isError) put("is_error", true)
                }
            )
            // Assistant message with tool calls: reconstruct from toolCallsJson
            role == "assistant" && toolCallsJson != null -> reconstructAssistantBlocks()
            else -> null
        }
    
    /**
     * Reconstruct content blocks for assistant message with tool calls.
     */
    private fun reconstructAssistantBlocks(): List<Map<String, Any>> {
        val blocks = mutableListOf<Map<String, Any>>()
        
        // Add text block if content is not just tool names
        val textContent = content.takeIf { 
            !it.startsWith("[Tools:") && it.isNotEmpty() 
        }?.substringBefore(" [Tools:")
        
        if (!textContent.isNullOrEmpty()) {
            blocks.add(mapOf("type" to "text", "text" to textContent))
        }
        
        // Parse tool calls from JSON
        try {
            val toolCallsArray = json.parseToJsonElement(toolCallsJson!!).jsonArray
            for (element in toolCallsArray) {
                val obj = element.jsonObject
                blocks.add(
                    mapOf(
                        "type" to "tool_use",
                        "id" to (obj["id"]?.jsonPrimitive?.content ?: ""),
                        "name" to (obj["name"]?.jsonPrimitive?.content ?: ""),
                        "input" to parseJsonToMap(obj["input"] ?: json.parseToJsonElement("{}"))
                    )
                )
            }
        } catch (e: Exception) {
            // If parsing fails, return just the text block if any
        }
        
        return blocks
    }
    
    private fun parseJsonToMap(element: JsonElement): Map<String, Any> {
        return when (element) {
            is JsonObject -> JsonUtils.parseJsonObjectToMap(element)
            else -> emptyMap()
        }
    }
    
    companion object {
        /** Standard message roles for API compatibility. */
        const val ROLE_USER = "user"
        const val ROLE_ASSISTANT = "assistant"
        const val ROLE_TOOL = "tool"

        /** Recursively convert Any to JsonElement for serialization. */
        private fun anyToJsonElement(value: Any?): JsonElement = when (value) {
            null -> JsonNull
            is String -> JsonPrimitive(value)
            is Number -> JsonPrimitive(value)
            is Boolean -> JsonPrimitive(value)
            is Map<*, *> -> buildJsonObject {
                value.forEach { (k, v) -> put(k.toString(), anyToJsonElement(v)) }
            }
            is List<*> -> buildJsonArray {
                value.forEach { add(anyToJsonElement(it)) }
            }
            is JsonElement -> value
            else -> JsonPrimitive(value.toString())
        }

        /**
         * Create a tool result message for conversation history.
         * 
         * This method creates a message in the appropriate format based on
         * provider requirements. ClaudeClient will serialize this correctly
         * for either Anthropic or OpenAI format.
         * 
         * @param toolCallId The tool call ID this result corresponds to
         * @param content The result content (JSON string or plain text)
         */
        fun toolResult(toolCallId: String, content: String, toolName: String? = null, isError: Boolean = false): Message {
            return Message(
                role = "tool",
                content = content,
                toolCallId = toolCallId,
                isError = isError,
                toolName = toolName,
                // For Anthropic format, we store structured content blocks
                _contentBlocks = listOf(
                    buildMap {
                        put("type", "tool_result")
                        put("tool_use_id", toolCallId)
                        put("content", content)
                        if (isError) put("is_error", true)
                    }
                )
            )
        }
        
        /**
         * Create an assistant message with tool calls for conversation history.
         * 
         * This is used to preserve tool calls in the conversation when Claude
         * requests to use tools. The content blocks will be serialized correctly
         * for the API provider.
         * 
         * @param text Optional text response from the assistant
         * @param toolCalls List of tool calls the assistant wants to execute
         */
        fun assistantWithTools(text: String?, toolCalls: List<ToolCall>): Message {
            val blocks = mutableListOf<Map<String, Any>>()
            
            // Add text block if present (must be non-empty per Anthropic API)
            if (!text.isNullOrEmpty()) {
                blocks.add(mapOf("type" to "text", "text" to text))
            }
            
            // Add tool use blocks
            toolCalls.forEach { toolCall ->
                blocks.add(
                    mapOf(
                        "type" to "tool_use",
                        "id" to toolCall.id,
                        "name" to toolCall.name,
                        "input" to toolCall.input
                    )
                )
            }
            
            // For content, join text and tool call names
            val contentStr = buildString {
                if (!text.isNullOrBlank()) append(text)
                if (toolCalls.isNotEmpty()) {
                    if (isNotEmpty()) append(" ")
                    append("[Tools: ${toolCalls.joinToString { it.name }}]")
                }
            }
            
            // Serialize tool calls to JSON for persistence
            val toolCallsJsonStr = if (toolCalls.isNotEmpty()) {
                buildJsonArray {
                    toolCalls.forEach { tc ->
                        addJsonObject {
                            put("id", tc.id)
                            put("name", tc.name)
                            putJsonObject("input") {
                                tc.input.forEach { (k, v) -> put(k, JsonUtils.anyToJsonElement(v)) }
                            }
                        }
                    }
                }.toString()
            } else null
            
            return Message(
                role = "assistant",
                content = contentStr,
                toolCallsJson = toolCallsJsonStr,
                _contentBlocks = blocks
            )
        }
    }
}

@Serializable
data class Conversation(
    val messages: MutableList<Message> = mutableListOf()
) {
    fun addUser(content: String) {
        messages.add(Message(role = "user", content = content))
    }
    
    fun addAssistant(content: String) {
        messages.add(Message(role = "assistant", content = content))
    }
    
    fun addToolResult(toolCallId: String, content: String, toolName: String? = null, isError: Boolean = false) {
        messages.add(Message.toolResult(toolCallId, content, toolName, isError))
    }
    
    /**
     * Convert conversation to API message format for **chat mode** (no tools).
     *
     * This method is used when the model is called without tool definitions
     * (conversational responses). For tool-use conversations, provider clients
     * serialize [Message] objects directly via [Message.contentBlocks].
     *
     * **Turn-aware trimming**: When the conversation exceeds [maxMessages], trimming
     * finds the nearest user-message boundary so we never cut mid-turn. This
     * prevents orphaned tool results or context-free assistant replies.
     *
     * **Tool message filtering**: Tool result messages (`role="tool"`) and assistant
     * messages with tool calls are converted to text-only format since the chat
     * API doesn't accept tool-specific roles or content blocks.
     *
     * @param maxMessages Maximum number of recent user/assistant messages to include.
     *                    Default is 20 (~10 conversation turns).
     */
    fun toApiMessages(maxMessages: Int = 20): List<Map<String, String>> {
        // Create snapshot to avoid ConcurrentModificationException if messages
        // are added from another coroutine during processing.
        val snapshot = messages.toList()

        // Stage 1: Convert to chat-safe messages.
        // - Tool result messages (role="tool") are dropped — they're API machinery
        //   that's meaningless without tool definitions.
        // - Assistant messages with tool calls are converted to text-only, keeping
        //   any text content the model produced alongside the tool calls.
        val chatMessages = snapshot.mapNotNull { msg ->
            when {
                msg.role == Message.ROLE_TOOL -> null  // Drop tool results entirely
                msg.role == Message.ROLE_ASSISTANT && msg.toolCallsJson != null -> {
                    // Extract text portion from "text [Tools: tap, type]" format
                    // Strip tool metadata suffix: whitespace + "[Tools: ...]"
                    val textOnly = msg.content
                        .replace(Regex("""\s*\[Tools:.*?]"""), "")
                        .trim()
                    if (textOnly.isNotEmpty()) {
                        msg.copy(content = textOnly)
                    } else {
                        null  // Drop pure tool-call messages with no text
                    }
                }
                else -> msg
            }
        }

        // Stage 2: Turn-aware trimming.
        // Find the nearest user-message boundary to avoid starting mid-conversation.
        val trimmed = if (chatMessages.size > maxMessages) {
            val rawStart = chatMessages.size - maxMessages
            // Walk forward from rawStart to find a user message (safe turn boundary)
            var safeStart = rawStart
            while (safeStart < chatMessages.size && chatMessages[safeStart].role != Message.ROLE_USER) {
                safeStart++
            }
            if (safeStart < chatMessages.size) {
                // Found a user message boundary after rawStart
                chatMessages.subList(safeStart, chatMessages.size)
            } else {
                // No user message found after rawStart — walk backward to find one.
                // This handles conversations with many trailing assistant messages.
                var backStart = rawStart - 1
                while (backStart >= 0 && chatMessages[backStart].role != Message.ROLE_USER) {
                    backStart--
                }
                if (backStart >= 0) {
                    chatMessages.subList(backStart, chatMessages.size)
                } else {
                    // No user messages at all — return whatever we have
                    chatMessages
                }
            }
        } else {
            chatMessages
        }

        // Stage 3: Ensure first message has role="user" (API requirement).
        val messagesToSend = trimmed.dropWhile { it.role != Message.ROLE_USER }

        return messagesToSend.map { mapOf("role" to it.role, "content" to it.content) }
    }
}
