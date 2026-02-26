package ai.citros.core

import kotlinx.serialization.json.*

/**
 * Phone control agent using local LLM (Ollama, llama.cpp, etc.)
 * Identical logic to PhoneAgent but uses LocalLLMClient.
 */
class PhoneAgentLocal(
    private val llmClient: LocalLLMClient,
    private val clickElement: (Int) -> ScreenReader.ElementActionResult = { ScreenReader.clickElementDetailed(it) }
) {
    private val json = Json { ignoreUnknownKeys = true }
    
    companion object {
        val LOCAL_SYSTEM_PROMPT = """You control an Android phone. You receive the screen content and a user request.

IMPORTANT: If the user is asking a QUESTION or CHATTING, reply with normal text. Do NOT output JSON.

If the user wants you to DO something on the phone, reply with exactly ONE JSON action:
{"action": "home"}
{"action": "back"}
{"action": "open_app", "name": "YouTube"}
{"action": "click", "element": 5}
{"action": "click_text", "text": "Settings"}
{"action": "type", "text": "hello"}
{"action": "swipe", "direction": "up"}

Rules:
- Questions like "what can you do?" or "how are you?" = reply with TEXT, not JSON
- Phone tasks like "open YouTube" or "go home" = reply with ONE JSON action
- Only output the JSON object, nothing else, when doing an action""".trimIndent()
    }
    
    suspend fun process(userMessage: String, screenContent: ScreenContent?): AgentResponse {
        val fullMessage = buildString {
            if (screenContent != null && ScreenReader.isAttached()) {
                appendLine("CURRENT SCREEN:")
                appendLine(screenContent.toPromptText())
                appendLine()
            }
            append("USER: $userMessage")
        }
        
        val result = llmClient.chat(fullMessage)
        
        return result.fold(
            onSuccess = { response -> parseResponse(response) },
            onFailure = { error ->
                AgentResponse(text = "Error: ${error.message}", action = null)
            }
        )
    }
    
    private fun parseResponse(response: String): AgentResponse {
        val trimmed = response.trim()
        
        // Try to extract JSON action — small models sometimes wrap in markdown
        val jsonStr = extractJson(trimmed)
        
        if (jsonStr != null) {
            try {
                val jsonObj = json.parseToJsonElement(jsonStr).jsonObject
                val actionType = jsonObj["action"]?.jsonPrimitive?.content
                
                if (actionType != null) {
                    val action = when (actionType) {
                        "click" -> {
                            val elementId = jsonObj["element"]?.jsonPrimitive?.int
                            if (elementId != null) PhoneAction.Click(elementId) else null
                        }
                        "click_text" -> {
                            val text = jsonObj["text"]?.jsonPrimitive?.content
                            if (text != null) PhoneAction.ClickText(text) else null
                        }
                        "type" -> {
                            val text = jsonObj["text"]?.jsonPrimitive?.content
                            if (text != null) PhoneAction.Type(text) else null
                        }
                        "swipe" -> {
                            val direction = jsonObj["direction"]?.jsonPrimitive?.content
                            if (direction != null) PhoneAction.Swipe(direction) else null
                        }
                        "back" -> PhoneAction.Back
                        "home" -> PhoneAction.Home
                        "open_app" -> {
                            val name = jsonObj["name"]?.jsonPrimitive?.content
                            if (name != null) PhoneAction.OpenApp(name) else null
                        }
                        "open_notifications" -> PhoneAction.OpenNotifications
                        "none" -> null
                        else -> null
                    }
                    
                    if (action != null) {
                        return AgentResponse(text = null, action = action)
                    }
                }
            } catch (_: Exception) {}
        }
        
        // Fallback: lenient regex (handles partial JSON from small models)
        val actionPattern = Regex("""action["\s]*:\s*["\s]*(\w+)""")
        val actionMatch = actionPattern.find(trimmed)
        
        if (actionMatch != null) {
            val actionType = actionMatch.groupValues[1]
            val directionPattern = Regex("""direction["\s]*:\s*["\s]*(\w+)""")
            val textPattern = Regex("""text["\s]*:\s*"([^"]+)"""")
            val elementPattern = Regex("""element["\s]*:\s*(\d+)""")
            val namePattern = Regex("""name["\s]*:\s*"([^"]+)"""")
            
            val action = when (actionType) {
                "click" -> elementPattern.find(trimmed)?.groupValues?.get(1)?.toIntOrNull()?.let { PhoneAction.Click(it) }
                "click_text" -> textPattern.find(trimmed)?.groupValues?.get(1)?.let { PhoneAction.ClickText(it) }
                "type" -> textPattern.find(trimmed)?.groupValues?.get(1)?.let { PhoneAction.Type(it) }
                "swipe" -> directionPattern.find(trimmed)?.groupValues?.get(1)?.let { PhoneAction.Swipe(it) }
                "back" -> PhoneAction.Back
                "home" -> PhoneAction.Home
                "open_app" -> namePattern.find(trimmed)?.groupValues?.get(1)?.let { PhoneAction.OpenApp(it) }
                "open_notifications" -> PhoneAction.OpenNotifications
                "none" -> null
                else -> null
            }
            
            if (action != null) {
                return AgentResponse(text = null, action = action)
            }
        }
        
        return AgentResponse(text = trimmed, action = null)
    }
    
    /**
     * Extract JSON object from text — handles markdown code blocks and surrounding text.
     */
    private fun extractJson(text: String): String? {
        // Try markdown code block first: ```json {...} ```
        val codeBlock = Regex("""```(?:json)?\s*(\{[^`]*\})\s*```""", RegexOption.DOT_MATCHES_ALL)
            .find(text)?.groupValues?.get(1)
        if (codeBlock != null) return codeBlock.trim()
        
        // Try bare JSON object
        val jsonObj = Regex("""\{[^{}]*"action"[^{}]*\}""").find(text)?.value
        if (jsonObj != null) return jsonObj
        
        // Try if the whole thing is JSON
        val trimmed = text.trim()
        if (trimmed.startsWith("{") && trimmed.endsWith("}")) return trimmed
        
        return null
    }
    
    fun executeAction(action: PhoneAction, screenContent: ScreenContent?): String {
        return when (action) {
            is PhoneAction.Click -> {
                when (clickElement(action.elementId)) {
                    is ScreenReader.ElementActionResult.Success -> {
                    "Clicked element ${action.elementId}"
                    }
                    is ScreenReader.ElementActionResult.PrivacyBlocked -> {
                        "Failed: click: blocked by privacy mode for ${PrivacyRedaction.APP_PLACEHOLDER}"
                    }
                    ScreenReader.ElementActionResult.ServiceUnavailable ->
                        "Failed: click: accessibility service unavailable"
                    ScreenReader.ElementActionResult.GestureDispatchFailed ->
                        "Failed: click: gesture dispatch failed"
                    ScreenReader.ElementActionResult.ElementNotFound ->
                        "Failed to click element ${action.elementId}"
                }
            }
            is PhoneAction.ClickText -> {
                if (screenContent?.privacyMode == true) {
                    "Failed: click_text: blocked by privacy mode for ${PrivacyRedaction.APP_PLACEHOLDER}"
                } else {
                    val element = screenContent?.elements?.find {
                        it.text?.contains(action.text, ignoreCase = true) == true ||
                        it.contentDescription?.contains(action.text, ignoreCase = true) == true
                    }
                    when {
                        element == null -> "Could not find element with text \"${action.text}\""
                        else -> when (clickElement(element.id)) {
                        is ScreenReader.ElementActionResult.Success -> "Clicked \"${action.text}\""
                        is ScreenReader.ElementActionResult.PrivacyBlocked ->
                            "Failed: click_text: blocked by privacy mode for ${PrivacyRedaction.APP_PLACEHOLDER}"
                        ScreenReader.ElementActionResult.ServiceUnavailable ->
                            "Failed: click_text: accessibility service unavailable"
                        ScreenReader.ElementActionResult.GestureDispatchFailed ->
                            "Failed: click_text: gesture dispatch failed"
                        ScreenReader.ElementActionResult.ElementNotFound ->
                            "Could not find element with text \"${action.text}\""
                    }
                }
                }
            }
            is PhoneAction.Type -> {
                if (ScreenReader.typeText(action.text)) "Typed \"${action.text}\""
                else "Failed to type - no text field focused"
            }
            is PhoneAction.Swipe -> {
                val (screenW, screenH) = ScreenReader.getDisplaySize()
                val centerX = screenW / 2
                val centerY = screenH / 2
                val (startX, startY, endX, endY) = when (action.direction) {
                    "up" -> listOf(centerX, screenH * 3 / 4, centerX, screenH / 4)
                    "down" -> listOf(centerX, screenH / 4, centerX, screenH * 3 / 4)
                    "left" -> listOf(screenW * 4 / 5, centerY, screenW / 5, centerY)
                    "right" -> listOf(screenW / 5, centerY, screenW * 4 / 5, centerY)
                    else -> listOf(centerX, screenH * 3 / 4, centerX, screenH / 4)
                }
                if (ScreenReader.swipe(startX, startY, endX, endY, durationMs = 500)) {
                    "Swiped ${action.direction}"
                } else {
                    "Failed to swipe"
                }
            }
            PhoneAction.Back -> if (ScreenReader.pressBack()) "Pressed back" else "Failed to press back"
            PhoneAction.Home -> if (ScreenReader.pressHome()) "Pressed home" else "Failed to press home"
            is PhoneAction.OpenApp -> {
                if (ScreenReader.launchApp(action.name)) "Opened ${action.name}"
                else {
                    ScreenReader.pressHome()
                    "Couldn't find ${action.name} — went home"
                }
            }
            PhoneAction.OpenNotifications -> {
                if (ScreenReader.openNotifications()) "Opened notifications"
                else "Failed to open notifications"
            }
        }
    }
    
    fun clearConversation() {
        llmClient.clearConversation()
    }
    
    // ========== Compatibility methods for new tool-based ChatViewModel ==========
    
    /**
     * Execute a tool call (compatibility wrapper for executeAction).
     * Local LLM still uses JSON parsing, so this converts ToolCall → PhoneAction.
     */
    fun executeToolCall(toolCall: ToolCall, screenContent: ScreenContent?): ToolResult {
        val action = toolCallToPhoneAction(toolCall)
            ?: return ToolResult("Error: unsupported tool ${toolCall.name}", isError = true)
        val result = executeAction(action, screenContent)
        val isError = result.startsWith("Failed") || result.startsWith("Could not") ||
            (result.startsWith("{") && runCatching {
                json.parseToJsonElement(result)
                    .jsonObject["ok"]?.jsonPrimitive?.boolean == false
            }.getOrDefault(false))
        return ToolResult(result, isError)
    }
    
    /**
     * Add a tool result to the conversation (no-op for local agent).
     * Local LLM doesn't use structured tool results in conversation history.
     */
    fun addToolResult(toolCallId: String, result: String, toolName: String? = null, isError: Boolean = false) {
        // No-op: local LLM doesn't need tool results in history
    }

    /**
     * Add a user steer message to conversation history (no-op for local agent).
     * Local LLM doesn't use structured conversation history.
     */
    fun addSteerMessage(text: String) {
        // No-op: local LLM doesn't need steer messages in history
    }
    
    private fun toolCallToPhoneAction(toolCall: ToolCall): PhoneAction? {
        return when (toolCall.name) {
            "tap" -> {
                val id = (toolCall.input["element_id"] as? Number)?.toInt() ?: return null
                PhoneAction.Click(id)
            }
            "tap_text" -> {
                val text = toolCall.input["text"] as? String ?: return null
                PhoneAction.ClickText(text)
            }
            "type_text" -> {
                val text = toolCall.input["text"] as? String ?: return null
                PhoneAction.Type(text)
            }
            "swipe", "scroll" -> {
                val dir = toolCall.input["direction"] as? String ?: return null
                PhoneAction.Swipe(dir)
            }
            "press_back" -> PhoneAction.Back
            "press_home" -> PhoneAction.Home
            "open_app" -> {
                val name = toolCall.input["app_name"] as? String ?: return null
                PhoneAction.OpenApp(name)
            }
            "open_notifications" -> PhoneAction.OpenNotifications
            "read_screen" -> null  // No-op for local
            else -> null
        }
    }
}
