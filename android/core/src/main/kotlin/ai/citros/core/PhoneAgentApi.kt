package ai.citros.core

/**
 * Phone control agent using structured tool use.
 * Works with both API keys and setup tokens.
 * 
 * Supports dual-model configuration:
 * - chatClient: For user-facing conversations (typically Sonnet)
 * - actionClient: For action loop iterations (typically Haiku for speed/cost)
 * 
 * @param chatClient Client for initial user messages (e.g., Sonnet for planning)
 * @param actionClient Client for action loop follow-ups (e.g., Haiku for speed)
 */
import android.util.Log
import java.util.concurrent.CopyOnWriteArrayList

open class PhoneAgentApi(
    private val chatClient: ProviderClient,
    private val actionClient: ProviderClient,
    private val agentFileManager: AgentFileManager? = null,
    private val memoryProvider: MemoryProvider? = null,
    private val promptBuilder: AgentPromptBuilder? = agentFileManager?.let { AgentPromptBuilder(it) },
    private val contextManager: ContextManager = ContextManager(),
    private val trimmingPolicy: TrimmingPolicy = TrimmingPolicy(),
    private val contextCompactor: ContextCompactor = ContextCompactor(trimmingPolicy),
    val verifier: ActionVerifier = ActionVerifier(actionClient, VerificationMode.NEVER),
    actionModelId: String? = null,
    /** Base URL for SearXNG search instance (e.g., "http://100.93.251.101:8888"). Null to disable. */
    private val searchBaseUrl: String? = null,
    /** Brave Search API key for fallback search. Null to disable Brave. */
    private val braveApiKey: String? = null,
    /** TinyFish Web Agent API key for browser automation. Null to disable. */
    private val tinyFishApiKey: String? = null,
    /** TinyFish endpoint override for testing. Null uses production default. */
    private val tinyFishEndpoint: String? = null,
    /** Citros app token for authenticating to Citros API endpoints. */
    private val citrosAppToken: String? = null,
    /** Compatibility mode for tactical domain-specific web guardrails. */
    private val domainGuardrailMode: PhoneAgentPrompts.DomainGuardrailMode =
        PhoneAgentPrompts.DomainGuardrailMode.GENERIC
) {
    /** Shared search client — reuses OkHttpClient connection pools across calls. */
    private val searchClient by lazy {
        WebSearchClient(
            citrosAppToken = citrosAppToken,
            searxngBaseUrl = searchBaseUrl,
            braveApiKey = braveApiKey,
            domainGuardrailMode = domainGuardrailMode
        )
    }

    /** Shared fetch client — reuses OkHttpClient connection pool across calls. */
    private val fetchClient by lazy { WebFetchClient() }

    /** Shared TinyFish client for web browser automation. Only initialized when API key is set. */
    private val tinyFishClient by lazy {
        tinyFishApiKey?.let { TinyFishClient(apiKey = it, endpoint = tinyFishEndpoint ?: TinyFishClient.DEFAULT_ENDPOINT) }
    }

    init {
        // Validate that the action model meets the minimum capability floor.
        // The action loop processes untrusted screen content — weak models are a security risk.
        if (actionModelId != null && !ModelConfig.isModelAboveFloor(actionClient.provider, actionModelId)) {
            throw IllegalArgumentException(
                "Action model \"$actionModelId\" is below the minimum capability floor. " +
                    ModelConfig.MODEL_FLOOR_DESCRIPTION
            )
        }
    }

    /**
     * Get the tool list for a given model tier.
     *
     * API tools (web_search, web_fetch) are only available to STANDARD tier and above.
     * SMALL models are excluded from API tools due to prompt injection risk with
     * untrusted web content.
     *
     * @param modelId The model ID to check tier for (null = use action client model)
     * @return Combined list of phone tools + applicable API tools
     */
    fun getToolsForModel(modelId: String? = null): List<Tool> {
        val tier = modelId?.let { ModelClassifier.classify(it) } ?: ModelTier.STANDARD
        val tools = PhoneTools.getToolsForCategories(ToolCategory.entries.toSet(), tier)
        return if (tinyFishApiKey != null) tools else tools.filter { it.name != "web_browse" }
    }

    private val messages: MutableList<Message> = CopyOnWriteArrayList()

    /** Expose message count for testing. */
    @get:androidx.annotation.VisibleForTesting
    internal val messageCount: Int get() = messages.size

    /** Current tool step counter, used for context compaction. Set by ChatViewModel. */
    @Volatile
    var currentToolStep: Int = 0

    /**
     * Seed the API conversation history from UI messages when the agent's
     * in-memory history is empty (e.g. after process recreation).
     *
     * Android may kill the process while the user is in another app (during a
     * tool loop). When they return, [ChatViewModel.messages] persists (UI state)
     * but [PhoneAgentApi.messages] is empty. Without seeding, the model loses
     * all context from prior turns (#612).
     *
     * Only text content is preserved — tool_use/tool_result blocks are lost.
     * This is intentional: the Anthropic API requires matching tool_use/tool_result
     * pairs, and reconstructing those from UI messages is not reliable. Text-only
     * history is sufficient for conversational continuity.
     */
    @Synchronized
    fun seedConversationHistory(uiMessages: List<Message>) {
        if (messages.isNotEmpty()) return  // Already has history, skip
        if (uiMessages.isEmpty()) return

        // Convert UI messages to simple text-only messages for API context.
        // Drop tool/system messages, keep user and assistant text.
        val seeded = uiMessages.mapNotNull { msg ->
            when {
                msg.role == Message.ROLE_USER -> Message(role = "user", content = msg.content)
                msg.role == Message.ROLE_ASSISTANT && msg.content.isNotBlank() -> {
                    // Strip tool metadata suffix (e.g. "[Tools: tap, type]")
                    val textOnly = msg.content
                        .replace(Regex("""\s*\[Tools:.*?]"""), "")
                        .trim()
                    if (textOnly.isNotEmpty()) Message(role = "assistant", content = textOnly) else null
                }
                else -> null
            }
        }

        // Deduplicate consecutive same-role messages that can arise when
        // steer messages (mid-loop user turns) are adjacent to regular user turns
        // after tool-role messages are stripped. The Anthropic API rejects
        // consecutive same-role messages.
        val deduped = mutableListOf<Message>()
        for (msg in seeded) {
            if (deduped.isEmpty() || deduped.last().role != msg.role) {
                deduped.add(msg)
            }
            // else: skip consecutive same-role message
        }

        if (deduped.isNotEmpty()) {
            messages.addAll(deduped)
            Log.d(TAG, "seedConversationHistory: seeded ${'$'}{deduped.size} messages from UI (${'$'}{seeded.size - deduped.size} consecutive dupes removed)")
        }
    }

    /**
     * Optional callback for real-time tool progress updates.
     * Called from IO dispatcher during long-running tool operations (e.g., web_browse).
     * Set by ChatViewModel to update UI status text.
     */
    var onToolProgress: ((String) -> Unit)? = null

    /**
     * Override for phone control availability check. When null, uses [ScreenReader.isAttached()].
     * Set to `true` in tests to simulate phone control being available.
     */
    @androidx.annotation.VisibleForTesting
    @Volatile
    var phoneControlOverride: Boolean? = null

    companion object {
        private const val TAG = "CitrosAgent"
        private const val CLIPBOARD_NOT_ATTACHED =
            "Clipboard not available (accessibility service not attached)"
        private const val NOTIFICATION_NOT_ATTACHED =
            "Notification listener not attached. Enable notification access in Settings → Apps → Special access → Notification access."

        /**
         * Tools that mutate the UI. After these execute, fresh screen state should be
         * appended to the tool result so the model sees the consequence of its action.
         * Non-mutating tools (think, remember, file ops, etc.) return result only.
         *
         * See spec: docs/agentic-loop-v2.md §3.4
         */
        val UI_MUTATING_TOOLS = setOf(
            "tap", "tap_text", "long_press",
            "type_text",
            "swipe", "scroll",
            "press_back", "press_home",
            "open_app", "open_notifications"
        )

        /** Keywords that indicate the user wants a phone control action, not casual chat. */
        private val ACTION_HINTS = setOf(
            "open", "tap", "click", "press", "type", "send", "swipe", "scroll",
            "go home", "launch", "find", "search", "turn on", "turn off",
            "take", "screenshot", "set", "timer", "check", "show", "call",
            "read", "write", "capture", "navigate", "enable", "disable",
            "go to", "go back", "install", "uninstall", "download", "share",
            "copy", "paste", "delete", "remove", "close", "switch",
            // Context words that imply phone interaction even in questions
            "calendar", "email", "notification", "alarm", "weather",
            "message", "photo", "camera", "wifi", "bluetooth", "brightness"
        )

        /** Strict Android package-name validation for learn tool input. */
        private val ANDROID_PACKAGE_PATTERN =
            Regex("^[a-zA-Z][a-zA-Z0-9_]*(\\.[a-zA-Z][a-zA-Z0-9_]*)+$")

        /** Regex patterns for tool-like artifacts that should be stripped from chat responses. */
        private val TOOL_ARTIFACT_PATTERNS = listOf(
            Regex("""<tool_use>.*?</tool_use>""", RegexOption.DOT_MATCHES_ALL),
            Regex("""<tool_call>.*?</tool_call>""", RegexOption.DOT_MATCHES_ALL),
            Regex("""<function_call>.*?</function_call>""", RegexOption.DOT_MATCHES_ALL),
            // JSON tool objects with known tool names (handles one level of nesting)
            Regex("""\{\s*"name"\s*:\s*"(tap|type_text|swipe|open_app|press_back|press_home|read_screen|screenshot|scroll|long_press|tap_text|open_notifications|wait|think|web_browse|web_search|web_fetch|request_tools)"[^{}]*(\{[^{}]*\}[^{}]*)?\}""")
        )

        /**
         * Strip hallucinated tool-use artifacts from chat-mode responses.
         * Defense-in-depth: even when tools are not provided, some models may
         * hallucinate XML or JSON tool syntax in plain text.
         */
        fun stripToolArtifacts(text: String): String {
            // Early exit: skip regex if no possible markers present
            if ('<' !in text && '{' !in text) return text
            var result = text
            for (pattern in TOOL_ARTIFACT_PATTERNS) {
                result = pattern.replace(result, "")
            }
            return result.trim()
        }
    }

    private val conversationalPhrases = setOf(
        "hi", "hello", "hey", "yo", "sup", "what's up", "whats up", "how are you",
        "how's it going", "hows it going", "what're you doing", "what are you doing",
        "good morning", "good afternoon", "good evening", "thanks", "thank you"
    )
    
    /**
     * Single-model constructor (uses same client for both chat and actions).
     */
    constructor(providerClient: ProviderClient) : this(providerClient, providerClient, null, null, null, ContextManager(), TrimmingPolicy(), ContextCompactor(), ActionVerifier(providerClient, VerificationMode.NEVER), null)

    // Note: currentToolStep is set directly by ChatViewModel (single source of truth).
    // This avoids dual counters drifting out of sync.
    
    /**
     * Send a message and get a response with potential tool calls.
     * 
     * @param userMessage The user's message
     * @param screenContent Current screen state (will be included in the message)
     * @param isActionLoop Whether this is an action loop follow-up (uses actionClient)
     * @return ChatResponse with text and/or tool calls
     */
    /**
     * @param onTextDelta Optional streaming callback for chat-mode responses.
     *   When provided and the message is conversational (no tools), tokens are
     *   emitted via SSE streaming. The callback is invoked on the IO thread;
     *   callers should ensure thread-safe UI updates. Ignored for tool-mode requests.
     */
    suspend fun sendMessage(
        userMessage: String,
        screenContent: ScreenContent?,
        isActionLoop: Boolean = false,
        onTextDelta: ((String) -> Unit)? = null
    ): ChatResponse {
        // When phone control is not available, always use chat mode without tools (#390).
        // This prevents the model from hallucinating XML tool calls in plain text.
        val phoneControlAvailable = phoneControlOverride ?: ScreenReader.isAttached()
        val useChatMode = !phoneControlAvailable ||
            (!isActionLoop && isLikelyConversationalMessage(userMessage))
        Log.d(TAG, "sendMessage: phoneControl=$phoneControlAvailable, chatMode=$useChatMode, isActionLoop=$isActionLoop, msg='${userMessage.take(60)}'")
        if (useChatMode) {
            // When phone control is unavailable, prepend a system note so the
            // model knows not to attempt phone actions.
            val chatMessage = if (!phoneControlAvailable) {
                "[Phone control is not available. The accessibility service is not enabled. " +
                    "Respond conversationally. If the user asks you to do something on their phone, " +
                    "let them know they need to enable the accessibility service in Settings.]\n\n$userMessage"
            } else {
                userMessage
            }
            val conversation = Conversation(messages.toMutableList()).apply {
                addUser(chatMessage)
            }
            val chatResult = if (onTextDelta != null) {
                chatClient.chatStreaming(conversation, onTextDelta)
            } else {
                chatClient.chat(conversation)
            }
            return chatResult.fold(
                onSuccess = { rawText ->
                    // Strip any hallucinated tool artifacts from chat-mode responses
                    val text = stripToolArtifacts(rawText)
                    messages.add(Message(role = "user", content = userMessage))
                    messages.add(Message(role = "assistant", content = text))
                    ChatResponse(text = text, toolCalls = emptyList(), stopReason = "end_turn")
                },
                onFailure = { error ->
                    ChatResponse(
                        text = "Error: ${error.message}",
                        toolCalls = emptyList(),
                        stopReason = "error"
                    )
                }
            )
        }

        // Build message with screen context
        val fullMessage = buildString {
            if (screenContent != null && phoneControlAvailable) {
                appendLine("CURRENT SCREEN:")
                appendLine(screenContent.toPromptText())
                appendLine()
            }
            append(userMessage)
        }

        messages.add(Message(role = "user", content = fullMessage))

        // Get response from appropriate client
        val client = if (isActionLoop) actionClient else chatClient
        val modelName = client.modelId
        val systemPrompt = if (isActionLoop) {
            promptBuilder?.trimmed(
                phoneControlAvailable = phoneControlAvailable,
                modelName = modelName,
                domainGuardrailMode = domainGuardrailMode
            )
                ?: PhoneAgentPrompts.buildActionPrompt(
                    phoneControlAvailable = phoneControlAvailable,
                    modelName = modelName,
                    domainGuardrailMode = domainGuardrailMode
                )
        } else {
            promptBuilder?.full(
                phoneControlAvailable = phoneControlAvailable,
                modelName = modelName,
                domainGuardrailMode = domainGuardrailMode
            )
                ?: PhoneAgentPrompts.buildSystemPrompt(
                    phoneControlAvailable = phoneControlAvailable,
                    modelName = modelName,
                    domainGuardrailMode = domainGuardrailMode
                )
        }
        
        // Use compacted messages for action loop to manage context window.
        // Two-stage compaction:
        //   1. ContextCompactor strips SCREEN dumps from old tool results (cheap, regex-based)
        //   2. ContextManager summarizes remaining old messages (step-threshold based)
        val messagesForModel = if (isActionLoop) {
            val (screenStripped, compactorMetrics) = contextCompactor.compactWithMetrics(messages)
            val (compacted, managerMetrics) = contextManager.compactWithMetrics(screenStripped, currentToolStep)
            if (compactorMetrics != null) Log.d(TAG, "sendMessage compaction stage1: $compactorMetrics")
            if (managerMetrics != null) Log.d(TAG, "sendMessage compaction stage2: $managerMetrics")
            compacted
        } else {
            messages.toList()
        }
        
        val result = client.chatWithTools(messagesForModel, systemPrompt = systemPrompt, tools = getToolsForModel())

        return result.fold(
            onSuccess = { response ->
                // Add assistant response to conversation history
                if (response.toolCalls.isNotEmpty()) {
                    messages.add(Message.assistantWithTools(response.text, response.toolCalls))
                } else if (response.text != null) {
                    messages.add(Message(role = "assistant", content = response.text))
                }
                response
            },
            onFailure = { error ->
                ChatResponse(
                    text = "Error: ${error.message}",
                    toolCalls = emptyList(),
                    stopReason = "error"
                )
            }
        )
    }
    
    /**
     * Send an ephemeral prompt using existing conversation as context,
     * without persisting the user message in conversation history.
     *
     * Used for system-generated prompts (e.g., final explanation after max_steps)
     * that should not pollute the persistent message history for future API calls.
     *
     * Only the assistant's reply is appended to the persistent messages list.
     *
     * @param prompt The ephemeral user prompt (not persisted)
     * @return The assistant's text response, or null on failure
     */
    open suspend fun sendEphemeral(prompt: String): String? {
        val ephemeralMessages = messages.toMutableList().apply {
            add(Message(role = "user", content = prompt))
        }
        val phoneControlAvailable = phoneControlOverride ?: ScreenReader.isAttached()
        val modelName = chatClient.modelId
        val systemPrompt = promptBuilder?.full(
            phoneControlAvailable = phoneControlAvailable,
            modelName = modelName,
            domainGuardrailMode = domainGuardrailMode
        )
            ?: PhoneAgentPrompts.buildSystemPrompt(
                phoneControlAvailable = phoneControlAvailable,
                modelName = modelName,
                domainGuardrailMode = domainGuardrailMode
            )
        val result = chatClient.chatWithTools(
            ephemeralMessages,
            systemPrompt = systemPrompt,
            tools = emptyList()
        )
        return result.fold(
            onSuccess = { response ->
                response.text?.also { text ->
                    messages.add(Message(role = "assistant", content = text))
                }
            },
            onFailure = { error ->
                Log.w(TAG, "sendEphemeral failed: ${error.message}")
                null
            }
        )
    }

    /**
     * Continue the conversation after tool results have been added.
     *
     * Unlike [sendMessage], this does NOT inject a new user message. The model
     * sees its own tool_use → tool_result flow and decides what to do next
     * (more tool calls, or end_turn with text).
     *
     * This replaces the v1 pattern of injecting synthetic "[Step X/20]" user
     * messages which polluted context and confused turn-taking.
     *
     * @return ChatResponse with text and/or tool calls
     * See docs/agentic-loop-v2.md §3.3
     */
    open suspend fun continueAfterTools(): ChatResponse {
        val phoneControlAvailable = phoneControlOverride ?: ScreenReader.isAttached()
        val systemPrompt = promptBuilder?.trimmed(
            phoneControlAvailable = phoneControlAvailable,
            modelName = actionClient.modelId,
            domainGuardrailMode = domainGuardrailMode
        )
            ?: PhoneAgentPrompts.buildActionPrompt(
                phoneControlAvailable = phoneControlAvailable,
                modelName = actionClient.modelId,
                domainGuardrailMode = domainGuardrailMode
            )

        // Two-stage compaction: strip old SCREEN dumps first, then summarize old messages
        val (screenStripped, compactorMetrics) = contextCompactor.compactWithMetrics(messages)
        val (messagesForModel, managerMetrics) = contextManager.compactWithMetrics(screenStripped, currentToolStep)
        if (compactorMetrics != null) Log.d(TAG, "continueAfterTools compaction stage1: $compactorMetrics")
        if (managerMetrics != null) Log.d(TAG, "continueAfterTools compaction stage2: $managerMetrics")
        Log.d(TAG, "continueAfterTools: step=$currentToolStep, rawMessages=${messages.size}, compacted=${messagesForModel.size}")

        val result = actionClient.chatWithTools(
            messagesForModel,
            systemPrompt = systemPrompt,
            tools = getToolsForModel()
        )

        return result.fold(
            onSuccess = { response ->
                if (response.toolCalls.isNotEmpty()) {
                    messages.add(Message.assistantWithTools(response.text, response.toolCalls))
                } else if (response.text != null) {
                    messages.add(Message(role = "assistant", content = response.text))
                }
                response
            },
            onFailure = { error ->
                ChatResponse(
                    text = "Error: ${error.message}",
                    toolCalls = emptyList(),
                    stopReason = "error"
                )
            }
        )
    }

    /**
     * Build a structured tool result string with optional screen state.
     *
     * For UI-mutating tools, the screen state is appended so the model
     * observes the consequence of its action without needing an explicit
     * read_screen call.
     *
     * @param actionSummary Human-readable summary of what happened (e.g., "Tapped element 5")
     * @param screenContent Fresh screen state to append (null for non-mutating tools)
     * @return Formatted tool result string
     * See docs/agentic-loop-v2.md §5.2
     */
    open fun formatToolResult(actionSummary: String, screenContent: ScreenContent? = null): String {
        if (screenContent == null) return actionSummary
        return "$actionSummary\n\nSCREEN:\n${screenContent.toPromptText()}"
    }

    /**
     * Execute a tool call with optional post-action verification.
     *
     * Runs the tool, then (if the [verifier] says so) captures a screenshot
     * and asks the action model whether the action succeeded. If verification
     * fails, appends the failure description to the result so the agent can
     * decide to retry.
     *
     * @param toolCall The tool call to execute
     * @param screenContent Current screen state (used for tap_text lookup)
     * @return Result text, potentially augmented with verification info
     */
    suspend fun executeToolCallWithVerification(
        toolCall: ToolCall,
        screenContent: ScreenContent?
    ): ToolResult {
        val result = executeToolCall(toolCall, screenContent)

        if (!verifier.shouldVerify(toolCall.name, result.text)) {
            return result
        }

        val verification = verifier.verify(toolCall.name, result.text)

        return when {
            verification.error != null -> {
                // Verification system itself failed — don't block the loop
                ToolResult("${result.text}\n[Verification skipped: ${verification.error}]", result.isError)
            }
            verification.verified -> {
                ToolResult("${result.text}\n[Verified: ${verification.description}]", result.isError)
            }
            else -> {
                ToolResult("${result.text}\n[Verification FAILED: ${verification.description}]", result.isError)
            }
        }
    }

    /**
     * Execute a tool call and return the result text.
     * 
     * @param toolCall The tool call to execute
     * @param screenContent Current screen state (used for tap_text lookup)
     * @return Result description
     */
    suspend fun executeToolCall(toolCall: ToolCall, screenContent: ScreenContent?): ToolResult {
        Log.d(TAG, "executeToolCall: name=${toolCall.name}, input=${toolCall.input.toString().take(100)}")
        val startMs = System.currentTimeMillis()
        return try {
            val result = when (toolCall.name) {
                "tap" -> {
                    val elementId = (toolCall.input["element_id"] as? Number)?.toInt()
                        ?: throw IllegalArgumentException("tap requires element_id (integer)")
                    
                    if (ScreenReader.clickElement(elementId)) {
                        "Tapped element $elementId"
                    } else {
                        "Failed: tap: could not tap element $elementId"
                    }
                }
                
                "tap_text" -> {
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("tap_text requires non-empty text (string)")
                    
                    // Find element containing the text
                    val element = screenContent?.elements?.find { 
                        it.text?.contains(text, ignoreCase = true) == true ||
                        it.contentDescription?.contains(text, ignoreCase = true) == true
                    }
                    
                    if (element != null && ScreenReader.clickElement(element.id)) {
                        "Tapped \"$text\""
                    } else {
                        "Failed: tap_text: no element matching \"$text\""
                    }
                }
                
                "type_text" -> {
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("type_text requires non-empty text (string)")
                    
                    if (ScreenReader.typeText(text)) {
                        "Typed \"$text\""
                    } else {
                        "Failed: type_text: no text field focused"
                    }
                }
                
                "swipe" -> {
                    val direction = toolCall.input["direction"] as? String
                        ?: throw IllegalArgumentException("swipe requires direction (up/down/left/right)")
                    
                    val (screenW, screenH) = ScreenReader.getDisplaySize()
                    val centerX = screenW / 2
                    val centerY = screenH / 2
                    // "scroll down" = see content below = swipe UP (finger moves up)
                    // "scroll up" = see content above = swipe DOWN (finger moves down)
                    val (startX, startY, endX, endY) = when (direction) {
                        "down" -> listOf(centerX, screenH * 3 / 4, centerX, screenH / 4)
                        "up" -> listOf(centerX, screenH / 4, centerX, screenH * 3 / 4)
                        "left" -> listOf(screenW * 4 / 5, centerY, screenW / 5, centerY)
                        "right" -> listOf(screenW / 5, centerY, screenW * 4 / 5, centerY)
                        else -> throw IllegalArgumentException("Invalid direction \"$direction\" - use up/down/left/right")
                    }
                    
                    if (ScreenReader.swipe(startX, startY, endX, endY, durationMs = 500)) {
                        "Swiped $direction"
                    } else {
                        "Failed: swipe: gesture not dispatched"
                    }
                }
                
                "scroll" -> {
                    val direction = toolCall.input["direction"] as? String
                        ?: throw IllegalArgumentException("scroll requires direction (up/down)")
                    
                    // Note: scroll is currently implemented as a vertical swipe gesture.
                    // A future implementation could use AccessibilityNodeInfo.ACTION_SCROLL_FORWARD/BACKWARD
                    // for scrolling within specific scrollable containers.
                    val (screenW, screenH) = ScreenReader.getDisplaySize()
                    val centerX = screenW / 2
                    // "scroll down" = see content below = swipe UP (finger moves up)
                    // "scroll up" = see content above = swipe DOWN (finger moves down)
                    val (startX, startY, endX, endY) = when (direction) {
                        "down" -> listOf(centerX, screenH * 3 / 4, centerX, screenH / 4)
                        "up" -> listOf(centerX, screenH / 4, centerX, screenH * 3 / 4)
                        else -> throw IllegalArgumentException("Invalid direction \"$direction\" - use up/down")
                    }
                    
                    if (ScreenReader.swipe(startX, startY, endX, endY, durationMs = 500)) {
                        "Scrolled $direction"
                    } else {
                        "Failed: scroll: gesture not dispatched"
                    }
                }
                
                "press_back" -> {
                    if (ScreenReader.pressBack()) {
                        "Pressed back"
                    } else {
                        "Failed: press_back: gesture not dispatched"
                    }
                }
                
                "press_home" -> {
                    if (ScreenReader.pressHome()) {
                        "Pressed home"
                    } else {
                        "Failed: press_home: gesture not dispatched"
                    }
                }
                
                "open_app" -> {
                    val appName = toolCall.input["app_name"] as? String
                        ?: throw IllegalArgumentException("open_app requires app_name (string)")
                    
                    if (ScreenReader.launchApp(appName)) {
                        "Opened $appName"
                    } else {
                        ScreenReader.pressHome()
                        "Failed: open_app: $appName not found — returned to home"
                    }
                }
                
                "open_notifications" -> {
                    if (ScreenReader.openNotifications()) {
                        "Opened notifications"
                    } else {
                        "Failed: open_notifications: could not expand status bar"
                    }
                }
                
                "think" -> {
                    val thought = (toolCall.input["thought"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("think requires non-empty thought (string)")
                    "Thought: $thought"
                }

                "wait" -> {
                    val seconds = ((toolCall.input["seconds"] as? Number)?.toInt() ?: 2).coerceIn(1, 5)
                    kotlinx.coroutines.delay(seconds * 1000L)
                    if (ScreenReader.isAttached()) {
                        val content = ScreenReader.getScreenContent()
                        "Waited ${seconds}s. Screen:\n${content.toPromptText()}"
                    } else {
                        "Waited ${seconds}s"
                    }
                }

                "long_press" -> {
                    val elementId = (toolCall.input["element_id"] as? Number)?.toInt()
                        ?: throw IllegalArgumentException("long_press requires element_id (integer)")
                    
                    if (ScreenReader.longPressElement(elementId)) {
                        "Long-pressed element $elementId"
                    } else {
                        "Failed: long_press: could not long-press element $elementId"
                    }
                }

                "copy" -> {
                    if (!ClipboardHelper.isAttached()) {
                        CLIPBOARD_NOT_ATTACHED
                    } else {
                        val text = ClipboardHelper.read()
                        if (text != null) {
                            "Clipboard content: $text"
                        } else {
                            "Clipboard is empty or access denied (Android 13+ may restrict clipboard reading)"
                        }
                    }
                }

                "set_clipboard" -> {
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("set_clipboard requires non-empty text (string)")
                    if (!ClipboardHelper.isAttached()) {
                        CLIPBOARD_NOT_ATTACHED
                    } else if (ClipboardHelper.write(text)) {
                        "Copied to clipboard (${text.length} chars): \"${text.take(50)}${if (text.length > 50) "…" else ""}\""
                    } else {
                        "Failed: set_clipboard: clipboard write denied"
                    }
                }

                "paste" -> {
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("paste requires non-empty text (string)")
                    if (!ClipboardHelper.isAttached()) {
                        CLIPBOARD_NOT_ATTACHED
                    } else if (ClipboardHelper.writeAndPaste(text)) {
                        "Pasted (${text.length} chars): \"${text.take(50)}${if (text.length > 50) "…" else ""}\""
                    } else {
                        "Failed: paste: no focused input field or clipboard write failed"
                    }
                }

                "read_notifications" -> {
                    if (!NotificationHelper.isAttached()) {
                        NOTIFICATION_NOT_ATTACHED
                    } else {
                        try {
                            val includeOngoing = toolCall.input["include_ongoing"] as? Boolean ?: false
                            val notifications = NotificationHelper.getActiveNotifications(includeOngoing)
                            NotificationHelper.formatForPrompt(notifications)
                        } catch (e: NotificationAccessDeniedException) {
                            e.message ?: "Notification access denied"
                        }
                    }
                }

                "tap_notification" -> {
                    val key = requireValidNotificationKey(toolCall, "tap_notification")
                    if (!NotificationHelper.isAttached()) {
                        NOTIFICATION_NOT_ATTACHED
                    } else if (NotificationHelper.tapNotification(key)) {
                        "Opened notification"
                    } else {
                        "Failed: tap_notification: notification may have been dismissed or has no content intent"
                    }
                }

                "dismiss_notification" -> {
                    val key = requireValidNotificationKey(toolCall, "dismiss_notification")
                    if (!NotificationHelper.isAttached()) {
                        NOTIFICATION_NOT_ATTACHED
                    } else if (NotificationHelper.dismissNotification(key)) {
                        "Dismissed notification"
                    } else {
                        "Failed: dismiss_notification: notification may be ongoing or already dismissed"
                    }
                }

                "reply_notification" -> {
                    val key = requireValidNotificationKey(toolCall, "reply_notification")
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("reply_notification requires non-empty text (string)")
                    if (!NotificationHelper.isAttached()) {
                        NOTIFICATION_NOT_ATTACHED
                    } else if (NotificationHelper.replyToNotification(key, text)) {
                        "Replied to notification"
                    } else {
                        "Failed: reply_notification: notification may not support inline reply or was dismissed"
                    }
                }

                "read_screen" -> {
                    if (ScreenReader.isAttached()) {
                        val content = ScreenReader.getScreenContent()
                        "Screen refreshed:\n${content.toPromptText()}"
                    } else {
                        "Accessibility service not attached"
                    }
                }

                "screenshot" -> {
                    if (!ScreenReader.isAttached()) {
                        "Accessibility service not attached"
                    } else {
                        val base64 = ScreenReader.takeScreenshot()
                            ?: return ToolResult("Failed: screenshot: requires Android 11+", isError = true)
                        val prompt = (toolCall.input["prompt"] as? String)?.takeIf { it.isNotBlank() }
                            ?: PhoneAgentPrompts.DEFAULT_VISION_PROMPT
                        // Use chat model (vision-capable) for screenshot description
                        val result = chatClient.describeImage(base64, prompt)
                        result.fold(
                            onSuccess = { description -> "Screenshot description:\n$description" },
                            onFailure = { error -> "Screenshot captured but vision failed: ${error.message}" }
                        )
                    }
                }

                "read_file" -> fileToolResult("read_file") {
                    val manager = agentFileManager ?: throw IllegalStateException("Agent file manager not configured")
                    val path = (toolCall.input["path"] as? String)?.takeIf { it.isNotBlank() }
                        ?: throw IllegalArgumentException("read_file requires path (string)")
                    val content = manager.readFile(path)
                    fileToolSuccess(toolName = "read_file", path = path, content = content)
                }

                "write_file" -> fileToolResult("write_file") {
                    val manager = agentFileManager ?: throw IllegalStateException("Agent file manager not configured")
                    val path = (toolCall.input["path"] as? String)?.takeIf { it.isNotBlank() }
                        ?: throw IllegalArgumentException("write_file requires path (string)")
                    val content = toolCall.input["content"] as? String
                        ?: throw IllegalArgumentException("write_file requires content (string)")
                    manager.writeFile(path, content)
                    fileToolSuccess(toolName = "write_file", path = path)
                }

                "list_files" -> fileToolResult("list_files") {
                    val manager = agentFileManager ?: throw IllegalStateException("Agent file manager not configured")
                    val path = toolCall.input["path"] as? String
                    val files = manager.listFiles(path)
                    fileToolSuccess(toolName = "list_files", path = path ?: ".", files = files)
                }

                "learn" -> fileToolResult("learn") {
                    val manager = agentFileManager ?: throw IllegalStateException("Agent file manager not configured")
                    val appPackage = (toolCall.input["app_package"] as? String)?.trim()?.takeIf { it.isNotBlank() }
                        ?: throw IllegalArgumentException("learn requires app_package (string)")
                    if (!ANDROID_PACKAGE_PATTERN.matches(appPackage)) {
                        throw IllegalArgumentException(
                            "app_package must be a valid Android package name (e.g., com.example.app)"
                        )
                    }
                    val pattern = (toolCall.input["pattern"] as? String)?.takeIf { it.isNotBlank() }
                        ?: throw IllegalArgumentException("learn requires pattern (string)")
                    val category = (toolCall.input["category"] as? String)
                        ?.trim()
                        ?.lowercase()
                        ?.takeIf { it.isNotBlank() }
                        ?: "navigation"
                    if (category !in listOf("navigation", "failure", "strategy")) {
                        throw IllegalArgumentException("category must be navigation, failure, or strategy")
                    }
                    manager.writeKnowledge(appPackage, pattern, category)
                    "Learned pattern for $appPackage [$category]: $pattern"
                }

                // Memory tools use runBlocking because executeToolCall() is synchronous
                // and is called from within an existing coroutine context (the tool loop in
                // ChatViewModel.sendMessage). The MemoryProvider suspends on Dispatchers.IO
                // internally, so the main thread is not blocked.
                "remember" -> memoryToolResult("remember") {
                    val provider = memoryProvider ?: throw IllegalStateException("Memory provider not configured")
                    val content = (toolCall.input["content"] as? String)?.takeIf { it.isNotBlank() }
                        ?: throw IllegalArgumentException("remember requires content (string)")
                    val tags = (toolCall.input["tags"] as? String)
                        ?.split(',')
                        ?.map { it.trim() }
                        ?.filter { it.isNotEmpty() }
                        .orEmpty()
                    val id = kotlinx.coroutines.runBlocking {
                        provider.store(content, MemoryMetadata(tags = tags))
                    }
                    memoryToolSuccess(toolName = "remember", id = id, content = content, tags = tags)
                }

                "recall" -> memoryToolResult("recall") {
                    val provider = memoryProvider ?: throw IllegalStateException("Memory provider not configured")
                    val query = (toolCall.input["query"] as? String)?.takeIf { it.isNotBlank() }
                        ?: throw IllegalArgumentException("recall requires query (string)")
                    val limit = (toolCall.input["limit"] as? Number)?.toInt() ?: 5
                    val results = kotlinx.coroutines.runBlocking { provider.search(query = query, limit = limit) }
                    memoryToolSuccess(toolName = "recall", results = results)
                }

                "list_memories" -> memoryToolResult("list_memories") {
                    val provider = memoryProvider ?: throw IllegalStateException("Memory provider not configured")
                    val limit = (toolCall.input["limit"] as? Number)?.toInt() ?: 10
                    val results = kotlinx.coroutines.runBlocking {
                        provider.list(MemoryFilter(limit = limit))
                    }
                    memoryToolSuccess(toolName = "list_memories", results = results)
                }

                "web_search" -> {
                    val query = toolCall.input["query"]?.toString()
                        ?: return ToolResult("Missing required parameter: query", isError = true)
                    val count = (toolCall.input["count"] as? Number)?.toInt() ?: 3
                    searchClient.search(query, count)
                }

                "web_fetch" -> {
                    val url = toolCall.input["url"]?.toString()
                        ?: return ToolResult("Missing required parameter: url", isError = true)
                    val maxChars = (toolCall.input["max_chars"] as? Number)?.toInt() ?: 5000
                    fetchClient.fetch(url, maxChars)
                }

                "web_browse" -> {
                    val client = tinyFishClient
                        ?: return ToolResult("Web browse not available: TinyFish API key not configured", isError = true)
                    val url = toolCall.input["url"]?.toString()
                        ?: return ToolResult("Missing required parameter: url", isError = true)
                    val goal = toolCall.input["goal"]?.toString()
                        ?: return ToolResult("Missing required parameter: goal", isError = true)
                    val stealth = toolCall.input["stealth"] as? Boolean ?: false
                    client.browse(url = url, goal = goal, stealth = stealth, onProgress = onToolProgress)
                }

                "request_tools" -> {
                    val rawCategories = toolCall.input["categories"] as? List<*>
                        ?: return ToolResult(
                            "Missing required parameter: categories (array of strings)",
                            isError = true
                        )
                    val validCategories = ToolCategory.entries
                        .filter { it != ToolCategory.CORE }
                        .map { it.name.lowercase() }
                        .sorted()

                    if (rawCategories.isEmpty()) {
                        return ToolResult(
                            "categories must contain at least one category. Available: ${validCategories.joinToString(", ")}",
                            isError = true
                        )
                    }

                    val requested = linkedSetOf<ToolCategory>()
                    val invalidCategories = mutableListOf<String>()
                    rawCategories.forEach { item ->
                        val raw = (item as? String)?.trim()
                        if (raw.isNullOrEmpty()) {
                            invalidCategories += item?.toString() ?: "null"
                            return@forEach
                        }
                        val category = try {
                            ToolCategory.valueOf(raw.uppercase())
                        } catch (_: IllegalArgumentException) {
                            null
                        }
                        if (category == null || category == ToolCategory.CORE) {
                            invalidCategories += raw
                        } else {
                            requested += category
                        }
                    }

                    if (invalidCategories.isNotEmpty()) {
                        return ToolResult(
                            "Invalid categories: ${invalidCategories.joinToString(", ")}. Available: ${validCategories.joinToString(", ")}",
                            isError = true
                        )
                    }

                    if (requested.isEmpty()) {
                        return ToolResult(
                            "No valid categories requested. Available: ${validCategories.joinToString(", ")}",
                            isError = true
                        )
                    }

                    val tools = PhoneTools.getToolsForCategories(requested, currentExecutionModelTier())
                        .filter { tinyFishApiKey != null || it.name != "web_browse" }
                        .sortedBy { it.name }

                    val text = buildString {
                        append("Requested categories: ")
                        append(requested.joinToString(", ") { it.name.lowercase() })
                        append('\n')
                        append("Available tools:\n")
                        tools.forEach { tool ->
                            append("- ")
                            append(tool.name)
                            append(": ")
                            append(tool.description)
                            append('\n')
                        }
                    }.trimEnd()

                    ToolResult(text)
                }

                else -> {
                    "Failed: unknown tool \"${toolCall.name}\""
                }
            }
            val elapsedMs = System.currentTimeMillis() - startMs
            if (result is ToolResult) {
                Log.d(TAG, "executeToolCall: name=${toolCall.name} done in ${elapsedMs}ms, result='${result.text.take(120)}'")
                result
            } else {
                val text = result as String
                Log.d(TAG, "executeToolCall: name=${toolCall.name} done in ${elapsedMs}ms, result='${text.take(120)}'")
                ToolResult(text)
            }
        } catch (e: Exception) {
            val elapsedMs = System.currentTimeMillis() - startMs
            Log.e(TAG, "executeToolCall: name=${toolCall.name} FAILED in ${elapsedMs}ms: ${e.message}")
            ToolResult("Failed: ${toolCall.name}: ${e.message?.take(100)}", isError = true)
        }
    }
    
    private inline fun fileToolResult(toolName: String, block: () -> String): ToolResult {
        return try {
            ToolResult(block())
        } catch (e: SecurityException) {
            ToolResult(fileToolError(toolName, "Access denied: ${e.message}"), isError = true)
        } catch (e: IllegalArgumentException) {
            ToolResult(fileToolError(toolName, "Invalid input: ${e.message}"), isError = true)
        } catch (e: IllegalStateException) {
            ToolResult(fileToolError(toolName, "Tool not configured: ${e.message}"), isError = true)
        } catch (e: Exception) {
            ToolResult(fileToolError(toolName, e.message ?: "Unknown error"), isError = true)
        }
    }

    private fun currentExecutionModelTier(): ModelTier =
        actionClient.modelId?.let { ModelClassifier.classify(it) } ?: ModelTier.STANDARD

    // JSON encoding helpers — short aliases for readability
    private fun encStr(value: String): String =
        kotlinx.serialization.json.Json.encodeToString(kotlinx.serialization.serializer<String>(), value)
    private fun encStrList(value: List<String>): String =
        kotlinx.serialization.json.Json.encodeToString(
            kotlinx.serialization.builtins.ListSerializer(kotlinx.serialization.serializer<String>()), value)

    private fun fileToolSuccess(
        toolName: String,
        path: String? = null,
        content: String? = null,
        files: List<String>? = null
    ): String {
        val fields = mutableListOf<String>()
        fields += "\"ok\":true"
        fields += "\"tool\":${encStr(toolName)}"
        path?.let { fields += "\"path\":${encStr(it)}" }
        content?.let { fields += "\"content\":${encStr(it)}" }
        files?.let { fields += "\"files\":${encStrList(it)}" }
        return "{${fields.joinToString(",")}}"
    }

    private fun fileToolError(toolName: String, message: String): String =
        "{\"ok\":false,\"tool\":${encStr(toolName)},\"error\":${encStr(message)}}"

    private inline fun memoryToolResult(toolName: String, block: () -> String): ToolResult {
        return try {
            ToolResult(block())
        } catch (e: IllegalArgumentException) {
            ToolResult(memoryToolError(toolName, "Invalid input: ${e.message}"), isError = true)
        } catch (e: IllegalStateException) {
            ToolResult(memoryToolError(toolName, "Tool not configured: ${e.message}"), isError = true)
        } catch (e: Exception) {
            ToolResult(memoryToolError(toolName, e.message ?: "Unknown error"), isError = true)
        }
    }

    /**
     * Validate and return the notification key from a tool call.
     *
     * Android notification keys follow the format:
     * `<id>|<package.name>|<tag>|<user>|<postTime>`
     * e.g. `0|com.example.app|123|null|1000`
     *
     * This method enforces:
     * - at least 3 pipe-separated segments
     * - the second segment is a valid-looking Java package name (2+ dot-separated non-empty parts)
     * - no leading/trailing whitespace (rejects rather than trims to surface LLM errors)
     */
    private fun requireValidNotificationKey(toolCall: ToolCall, toolName: String): String {
        val key = toolCall.input["notification_key"] as? String
            ?: throw IllegalArgumentException("$toolName requires notification_key (string)")

        if (key.trim() != key || key.isEmpty()) {
            throw IllegalArgumentException(
                "$toolName requires non-empty notification_key without whitespace"
            )
        }

        val parts = key.split('|')

        if (parts.size < 3) {
            throw IllegalArgumentException(
                "$toolName requires valid notification_key format " +
                    "(expected Android key like 0|com.example.app|123|null|1000)"
            )
        }

        val packageName = parts[1]
        val packageSegments = packageName.split('.')

        if (packageSegments.size < 2 || packageSegments.any { it.isEmpty() }) {
            throw IllegalArgumentException(
                "$toolName requires valid notification_key format " +
                    "(expected Android key like 0|com.example.app|123|null|1000)"
            )
        }

        return key
    }

    private fun memoryToolSuccess(
        toolName: String,
        id: String? = null,
        content: String? = null,
        tags: List<String>? = null,
        results: List<MemoryResult>? = null
    ): String {
        val fields = mutableListOf<String>()
        fields += "\"ok\":true"
        fields += "\"tool\":${encStr(toolName)}"
        id?.let { fields += "\"id\":${encStr(it)}" }
        content?.let { fields += "\"content\":${encStr(it)}" }
        tags?.let { fields += "\"tags\":${encStrList(it)}" }
        results?.let { fields += "\"results\":${encodeMemoryResults(it)}" }
        return "{${fields.joinToString(",")}}"
    }

    private fun encodeMemoryResults(results: List<MemoryResult>): String {
        val rows = results.map { result ->
            val source = result.source?.let { encStr(it) } ?: "null"
            "{\"id\":${encStr(result.id)},\"content\":${encStr(result.content)},\"tags\":${encStrList(result.tags)},\"source\":$source,\"createdAt\":${result.createdAt}}"
        }
        return "[${rows.joinToString(",")}]"
    }

    private fun memoryToolError(toolName: String, message: String): String =
        "{\"ok\":false,\"tool\":${encStr(toolName)},\"error\":${encStr(message)}}"

    /**
     * Determine if a message is conversational (no tool use needed).
     *
     * Priority order (per v2 spec §9.3):
     * 1. Known conversational phrases → chat mode
     * 2. Contains action hint → tool mode (even if ends with `?`)
     * 3. Ends with `?` and no action hints → chat mode
     * 4. Short message (≤3 words, no special chars) → chat mode
     * 5. Default → tool mode
     */
    @androidx.annotation.VisibleForTesting
    internal fun isLikelyConversationalMessage(userMessage: String): Boolean {
        val normalized = userMessage.trim().lowercase()
        if (normalized.isEmpty()) return false

        // 1. Known conversational phrases → chat mode
        if (normalized in conversationalPhrases) return true

        // 2. Contains action hint → tool mode (even if ends with ?)
        // Use word-boundary matching to avoid substring false positives
        // (e.g., "calendaring" shouldn't match "calendar")
        val words = normalized.split(Regex("[\\s,.!?;:]+")).filter { it.isNotEmpty() }
        val hasActionHint = ACTION_HINTS.any { hint ->
            if (hint.contains(' ')) {
                // Multi-word hints (e.g., "go home", "turn on") — use substring match
                normalized.contains(hint)
            } else {
                // Single-word hints — match as whole word
                words.any { it == hint }
            }
        }
        if (hasActionHint) return false

        // 3. Ends with ? and no action hints → chat mode
        if (normalized.endsWith("?")) return true

        // 4. Short message without special chars → chat mode
        val wordCount = normalized.split(Regex("\\s+")).size
        return wordCount <= 3 && normalized.all { it.isLetterOrDigit() || it.isWhitespace() || it == '\'' }
    }

    /**
     * Add a tool result to the conversation.
     * 
     * @param toolCallId The tool call ID this result corresponds to
     * @param result The result text
     */
    open fun addToolResult(toolCallId: String, result: String, toolName: String? = null, isError: Boolean = false) {
        messages.add(Message.toolResult(toolCallId, result, toolName, isError))
    }

    /**
     * Add a user steer message to conversation history.
     */
    open fun addSteerMessage(text: String) {
        messages.add(Message(role = Message.ROLE_USER, content = text))
    }

    /**
     * Clear the conversation history.
     *
     * Thread-safe: synchronized to prevent races with [seedConversationHistory].
     * Note: if an API call is in-flight, it already has a snapshot of messages
     * (via `toList()` / `toMutableList()`), so clearing won't corrupt it.
     */
    @Synchronized
    fun clearConversation() {
        messages.clear()
        currentToolStep = 0
    }
    
    // ========== Legacy compatibility methods ==========
    
    /**
     * Legacy compatibility: process a message and return an AgentResponse.
     * This is kept for backward compatibility but internally uses the new tool system.
     * 
     * WARNING: Only returns the first tool call if multiple are present in the response.
     * Use sendMessage() to get all tool calls.
     * 
     * The action loop is now expected to be handled by the caller using sendMessage(),
     * executeToolCall(), and addToolResult() directly.
     */
    @Deprecated("Use sendMessage() and handle tool calls explicitly. WARNING: Only returns first tool call if multiple are present.", ReplaceWith("sendMessage(userMessage, screenContent, isActionLoop)"))
    suspend fun process(
        userMessage: String,
        screenContent: ScreenContent?,
        isActionLoop: Boolean = false
    ): AgentResponse {
        val response = sendMessage(userMessage, screenContent, isActionLoop)
        
        // Convert ChatResponse to AgentResponse (legacy format)
        // If there are tool calls, return the first one as a PhoneAction
        val action = if (response.toolCalls.isNotEmpty()) {
            toolCallToPhoneAction(response.toolCalls[0])
        } else {
            null
        }
        
        return AgentResponse(text = response.text, action = action)
    }
    
    /**
     * Legacy compatibility: execute a PhoneAction.
     */
    @Deprecated("Use executeToolCall() instead")
    suspend fun executeAction(action: PhoneAction, screenContent: ScreenContent?): String {
        val toolCall = phoneActionToToolCall(action)
        return executeToolCall(toolCall, screenContent).text
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
            else -> null
        }
    }
    
    private fun phoneActionToToolCall(action: PhoneAction): ToolCall {
        return when (action) {
            is PhoneAction.Click -> ToolCall("dummy", "tap", mapOf("element_id" to action.elementId))
            is PhoneAction.ClickText -> ToolCall("dummy", "tap_text", mapOf("text" to action.text))
            is PhoneAction.Type -> ToolCall("dummy", "type_text", mapOf("text" to action.text))
            is PhoneAction.Swipe -> ToolCall("dummy", "swipe", mapOf("direction" to action.direction))
            PhoneAction.Back -> ToolCall("dummy", "press_back", emptyMap())
            PhoneAction.Home -> ToolCall("dummy", "press_home", emptyMap())
            is PhoneAction.OpenApp -> ToolCall("dummy", "open_app", mapOf("app_name" to action.name))
            PhoneAction.OpenNotifications -> ToolCall("dummy", "open_notifications", emptyMap())
        }
    }
}
