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
import java.security.MessageDigest
import java.nio.charset.StandardCharsets
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.roundToLong
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject

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
    /** TinyFish client factory override for testing. */
    private val tinyFishClientFactory: (apiKey: String, endpoint: String) -> TinyFishBrowserClient = { apiKey, endpoint ->
        TinyFishClient(apiKey = apiKey, endpoint = endpoint)
    },
    /** Citros app token for authenticating to Citros API endpoints. */
    private val citrosAppToken: String? = null,
    /** Sensor provider for device context injection. Null to disable. */
    private val sensorProvider: SensorProvider? = null,
    /** ScreenReader attachment check, injectable for unit tests. */
    private val isScreenReaderAttached: () -> Boolean = { ScreenReader.isAttached() },
    /** Screen content provider, injectable for unit tests. */
    private val getScreenContent: () -> ScreenContent = { ScreenReader.getScreenContent() },
    /** Screenshot provider, injectable for unit tests. */
    private val takeScreenshot: suspend () -> ScreenshotResult = { ScreenReader.takeScreenshot() },
    /** Element tap provider, injectable for unit tests. */
    private val clickElement: (Int) -> ScreenReader.ElementActionResult = { ScreenReader.clickElementDetailed(it) },
    /** Element long-press provider, injectable for unit tests. */
    private val longPressElement: (Int) -> ScreenReader.ElementActionResult = { ScreenReader.longPressElementDetailed(it) },
    /** Optional spending guard for API usage. */
    private val budgetGuard: BudgetGuard? = null
) {
    private val taskSensorSnapshotLock = Any()
    @Volatile
    private var taskSensorSnapshot: SensorContext? = null
    @Volatile
    private var taskSensorSnapshotInitialized: Boolean = false
    private val taskSensorSnapshotCaptureMutex = Mutex()
    private val taskSensorSnapshotEpoch = AtomicLong(0L)
    private val sensorSnapshotFailureCount = AtomicInteger(0)
    private val sensorSnapshotTimeoutCount = AtomicInteger(0)
    private val sensorSnapshotCount = AtomicInteger(0)
    private val sensorSnapshotLatencyTotalMsCounter = AtomicLong(0L)
    private val sensorSnapshotLatencyMaxMsCounter = AtomicLong(0L)

    private enum class SnapshotOutcome { SUCCESS, TIMEOUT, FAILURE }

    private fun recordSensorSnapshotMetrics(durationMs: Long, outcome: SnapshotOutcome) {
        sensorSnapshotCount.incrementAndGet()
        sensorSnapshotLatencyTotalMsCounter.addAndGet(durationMs)
        sensorSnapshotLatencyMaxMsCounter.updateAndGet { current -> maxOf(current, durationMs) }
        Log.d(TAG, "sensor snapshot outcome=$outcome duration_ms=$durationMs")
    }

    /** Gather sensor context, never throws. */
    private suspend fun getSensorSnapshot(): SensorContext? {
        val startedNs = System.nanoTime()
        return try {
            val snapshot = withTimeout(SENSOR_SNAPSHOT_TIMEOUT_MS) {
                sensorProvider?.snapshot()
            }
            val elapsedMs = (System.nanoTime() - startedNs) / 1_000_000
            recordSensorSnapshotMetrics(elapsedMs, SnapshotOutcome.SUCCESS)
            snapshot
        } catch (_: TimeoutCancellationException) {
            val timeouts = sensorSnapshotTimeoutCount.incrementAndGet()
            Log.w(TAG, "sensor snapshot timed out (count=$timeouts)")
            val elapsedMs = (System.nanoTime() - startedNs) / 1_000_000
            recordSensorSnapshotMetrics(elapsedMs, SnapshotOutcome.TIMEOUT)
            null
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            val failures = sensorSnapshotFailureCount.incrementAndGet()
            Log.w(TAG, "sensor snapshot failed (count=$failures): ${e.javaClass.simpleName}")
            val elapsedMs = (System.nanoTime() - startedNs) / 1_000_000
            recordSensorSnapshotMetrics(elapsedMs, SnapshotOutcome.FAILURE)
            null  // Defense-in-depth: provider contract says no-throw, but be safe
        }
    }

    /**
     * Return the cached task snapshot, refreshing only when a new task starts.
     *
     * New task boundary: non-action `sendMessage` call.
     * Continuations (`continueAfterTools`) and action-loop turns reuse the same snapshot.
     */
    private suspend fun getTaskSensorSnapshot(startNewTask: Boolean): SensorContext? {
        val captureEpoch = synchronized(taskSensorSnapshotLock) {
            if (startNewTask) {
                taskSensorSnapshotEpoch.incrementAndGet()
                taskSensorSnapshot = null
                taskSensorSnapshotInitialized = false
            }
            if (taskSensorSnapshotInitialized) {
                return taskSensorSnapshot
            }
            taskSensorSnapshotEpoch.get()
        }

        val captured = taskSensorSnapshotCaptureMutex.withLock {
            synchronized(taskSensorSnapshotLock) {
                if (taskSensorSnapshotInitialized && taskSensorSnapshotEpoch.get() == captureEpoch) {
                    return@withLock taskSensorSnapshot
                }
            }
            getSensorSnapshot()
        }

        return synchronized(taskSensorSnapshotLock) {
            if (!taskSensorSnapshotInitialized && taskSensorSnapshotEpoch.get() == captureEpoch) {
                taskSensorSnapshot = captured
                taskSensorSnapshotInitialized = true
            }
            taskSensorSnapshot
        }
    }

    @get:androidx.annotation.VisibleForTesting
    internal val sensorSnapshotFailureTotal: Int
        get() = sensorSnapshotFailureCount.get()

    @get:androidx.annotation.VisibleForTesting
    internal val sensorSnapshotTimeoutTotal: Int
        get() = sensorSnapshotTimeoutCount.get()

    @get:androidx.annotation.VisibleForTesting
    internal val sensorSnapshotTotal: Int
        get() = sensorSnapshotCount.get()

    @get:androidx.annotation.VisibleForTesting
    internal val sensorSnapshotLatencyTotalMs: Long
        get() = sensorSnapshotLatencyTotalMsCounter.get()

    @get:androidx.annotation.VisibleForTesting
    internal val sensorSnapshotLatencyMaxMs: Long
        get() = sensorSnapshotLatencyMaxMsCounter.get()

    @get:androidx.annotation.VisibleForTesting
    internal val cachedTaskSensorSnapshot: SensorContext?
        get() = taskSensorSnapshot

    /** Shared search client — reuses OkHttpClient connection pools across calls. */
    private val searchClient by lazy {
        WebSearchClient(citrosAppToken = citrosAppToken, searxngBaseUrl = searchBaseUrl, braveApiKey = braveApiKey)
    }

    /** Shared fetch client — reuses OkHttpClient connection pool across calls. */
    private val fetchClient by lazy { WebFetchClient() }

    /** Shared TinyFish client for web browser automation. Only initialized when API key is set. */
    private val tinyFishClient by lazy {
        tinyFishApiKey?.let { tinyFishClientFactory(it, tinyFishEndpoint ?: TinyFishClient.DEFAULT_ENDPOINT) }
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

    /**
     * Get tools for a model with tool grouping policy applied.
     * When [FeatureFlags.toolGroupingV1Enabled] is true, uses [ToolGroupingPolicy]
     * to dynamically select categories. When false, falls back to legacy all-category behavior.
     *
     * @return Pair of tool list and optional [ResolvedToolPlan] (null when grouping is off)
     */
    internal fun getToolsForModelWithGrouping(
        modelId: String? = null,
        messageText: String = "",
        userSettings: UserToolCategorySettings = UserToolCategorySettings.allEnabled()
    ): Pair<List<Tool>, ResolvedToolPlan?> {
        if (!FeatureFlags.toolGroupingV1Enabled) {
            return getToolsForModel(modelId) to null
        }

        val tier = modelId?.let { ModelClassifier.classify(it) } ?: ModelTier.STANDARD
        val capabilities = ToolGroupingPolicy.Capabilities(
            accessibilityAttached = phoneControlOverride ?: isScreenReaderAttached(),
            hasTinyFishKey = tinyFishApiKey != null
        )

        val plan = ToolGroupingPolicy.resolve(
            messageText = messageText,
            modelTier = tier,
            capabilities = capabilities,
            userSettings = userSettings.snapshot()
        )

        // Filter from a single tool list using policy-selected tool names.
        val allTools = PhoneTools.getToolsForCategories(ToolCategory.entries.toSet(), tier)
        val tools = allTools.filter { it.name in plan.toolNames }

        return tools to plan
    }

    private val messages: MutableList<Message> = CopyOnWriteArrayList()
    private val requestFlowMutex = Mutex()
    private val clearRequested = AtomicBoolean(false)
    private val deferredSeedMessages = AtomicReference<DeferredSeedRequest?>(null)
    private val deferredSeedBootstrapEligibleInActiveFlow = AtomicBoolean(false)
    private val taskTokenAccumulator = TaskTokenAccumulator()
    @Volatile
    private var taskEstimatedCostNanodollars: Long = 0L
    @Volatile
    private var taskEstimatedCostMicrodollars: Long = 0L
    @Volatile
    var lastTaskCostSummary: TaskCostSummary = TaskCostSummary.EMPTY
        private set

    /** Expose message count for testing. */
    @get:androidx.annotation.VisibleForTesting
    internal val messageCount: Int get() = messages.size

    data class DeferredSeedSignal(
        val action: String,
        val reason: String,
        val seedMessageCount: Int,
        val liveMessageCount: Int
    )

    @androidx.annotation.VisibleForTesting
    @Volatile
    internal var onDeferredSeedSignal: ((DeferredSeedSignal) -> Unit)? = null

    @androidx.annotation.VisibleForTesting
    @Volatile
    internal var onBeforeDeferredMaintenanceInActiveFlow: (() -> Unit)? = null

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
    fun seedConversationHistory(uiMessages: List<Message>) {
        if (!requestFlowMutex.tryLock()) {
            val liveHistoryWasBootstrapEmptyAtDeferral = deferredSeedBootstrapEligibleInActiveFlow.get()
            val deferred = DeferredSeedRequest(
                uiMessages = uiMessages.toList(),
                liveHistoryWasEmptyAtDeferral = liveHistoryWasBootstrapEmptyAtDeferral
            )
            deferredSeedMessages.set(deferred)
            Log.d(TAG, "seedConversationHistory: deferred while request flow is active")
            return
        }
        try {
            applySeedConversationHistoryUnlocked(uiMessages, allowPrependToExisting = false)
        } finally {
            requestFlowMutex.unlock()
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
        private const val NANODOLLARS_PER_USD = 1_000_000_000L
        private const val NANODOLLARS_PER_MICRODOLLAR = 1_000L
        private const val FALLBACK_PROVIDER_FRAMING_CHARS = 256
        private const val FALLBACK_INPUT_CONSERVATISM_FACTOR = 1.25
        private const val FALLBACK_OUTPUT_CONSERVATISM_FACTOR = 1.10
        // Failsafe cap for full snapshot capture. Individual sensors should enforce
        // tighter per-field budgets to preserve partial context when one field is slow.
        internal const val SENSOR_SNAPSHOT_TIMEOUT_MS = 60L
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
            "go home", "launch", "find", "search", "fetch", "turn on", "turn off",
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

        private val EXPLICIT_WEB_FETCH_URL_REGEX =
            Regex("""https?://[^\s\]\)>]+""", RegexOption.IGNORE_CASE)

        private val EXPLICIT_WEB_FETCH_HINTS = setOf(
            "fetch", "summarize", "summarise", "extract", "read"
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
    ): ChatResponse = withRequestFlowLock {
        if (!isActionLoop) {
            resetTaskCostTracking()
        }

        // When phone control is not available, always use chat mode without tools (#390).
        // This prevents the model from hallucinating XML tool calls in plain text.
        val phoneControlAvailable = phoneControlOverride ?: ScreenReader.isAttached()
        val forceToolModeForResume =
            !isActionLoop &&
                isResumeIntent(userMessage) &&
                hasRecentInterruptionPauseContext()
        val useChatMode = !phoneControlAvailable ||
            (!isActionLoop && isLikelyConversationalMessage(userMessage) && !forceToolModeForResume)
        Log.d(
            TAG,
            "sendMessage: phoneControl=$phoneControlAvailable, chatMode=$useChatMode, " +
                "isActionLoop=$isActionLoop, forceResumeToolMode=$forceToolModeForResume, msg='${userMessage.take(60)}'"
        )
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
            checkBudgetBeforeApiCall()
            val chatResult: Result<Pair<String, TokenUsage?>> = if (onTextDelta != null) {
                chatClient.chatStreaming(conversation, onTextDelta).map { text ->
                    text to null
                }
            } else {
                chatClient.chatWithUsage(conversation)
            }
            return chatResult.fold(
                onSuccess = { (rawText, usage) ->
                    // Strip any hallucinated tool artifacts from chat-mode responses
                    val text = stripToolArtifacts(rawText)
                    recordUsageOutcomeAndCheckBudgets(
                        usage = usage,
                        modelId = chatClient.modelId,
                        promptChars = approximateConversationChars(conversation.messages),
                        responseChars = max(rawText.length, text.length),
                        systemPromptChars = 0,
                        toolSchemaChars = 0
                    )
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

        checkBudgetBeforeApiCall()
        val pendingUserMessage = Message(role = "user", content = fullMessage)
        val messagesWithPendingUser = messages.toMutableList().apply { add(pendingUserMessage) }

        // Get response from appropriate client
        val client = if (isActionLoop) actionClient else chatClient
        val modelName = client.modelId
        val sensorSnapshot = getTaskSensorSnapshot(startNewTask = !isActionLoop)
        val systemPrompt = if (isActionLoop) {
            promptBuilder?.trimmed(
                phoneControlAvailable = phoneControlAvailable,
                modelName = modelName,
                sensorContext = sensorSnapshot
            )
                ?: PhoneAgentPrompts.buildActionPrompt(
                    phoneControlAvailable = phoneControlAvailable,
                    modelName = modelName,
                    sensorContext = sensorSnapshot
                )
        } else {
            buildChatSystemPrompt(
                phoneControlAvailable = phoneControlAvailable,
                modelName = modelName,
                sensorContext = sensorSnapshot
            )
        }
        
        // Use compacted messages for action loop to manage context window.
        // Two-stage compaction:
        //   1. ContextCompactor strips SCREEN dumps from old tool results (cheap, regex-based)
        //   2. ContextManager summarizes remaining old messages (step-threshold based)
        val messagesForModel = if (isActionLoop) {
            val (screenStripped, compactorMetrics) = contextCompactor.compactWithMetrics(messagesWithPendingUser)
            val (compacted, managerMetrics) = contextManager.compactWithMetrics(screenStripped, currentToolStep)
            if (compactorMetrics != null) Log.d(TAG, "sendMessage compaction stage1: $compactorMetrics")
            if (managerMetrics != null) Log.d(TAG, "sendMessage compaction stage2: $managerMetrics")
            compacted
        } else {
            messagesWithPendingUser.toList()
        }

        val toolsForModel = getToolsForModel(client.modelId)
        val result = client.chatWithTools(messagesForModel, systemPrompt = systemPrompt, tools = toolsForModel)

        return result.fold(
            onSuccess = { rawResponse ->
                recordUsageOutcomeAndCheckBudgets(
                    usage = rawResponse.usage,
                    modelId = client.modelId,
                    promptChars = approximateMessageChars(messagesForModel),
                    responseChars = approximateResponseChars(rawResponse),
                    systemPromptChars = systemPrompt.length,
                    toolSchemaChars = approximateToolSchemaChars(toolsForModel)
                )
                val response = enforceExplicitWebFetchIntent(
                    userMessage = userMessage,
                    response = rawResponse,
                    isActionLoop = isActionLoop,
                    modelId = client.modelId
                )
                messages.add(pendingUserMessage)
                appendAssistantResponse(response)
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
        return withRequestFlowLock {
        val ephemeralMessages = messages.toMutableList().apply {
            add(Message(role = "user", content = prompt))
        }
        val phoneControlAvailable = phoneControlOverride ?: ScreenReader.isAttached()
        val modelName = chatClient.modelId
        val ephemeralSensors = getTaskSensorSnapshot(startNewTask = false)
        val systemPrompt = buildChatSystemPrompt(
            phoneControlAvailable = phoneControlAvailable,
            modelName = modelName,
            sensorContext = ephemeralSensors
        )
        checkBudgetBeforeApiCall()
        val result = chatClient.chatWithTools(
            ephemeralMessages,
            systemPrompt = systemPrompt,
            tools = emptyList()
        )
        return result.fold(
            onSuccess = { response ->
                recordUsageOutcomeAndCheckBudgets(
                    usage = response.usage,
                    modelId = chatClient.modelId,
                    promptChars = approximateMessageChars(ephemeralMessages),
                    responseChars = approximateResponseChars(response),
                    systemPromptChars = systemPrompt.length,
                    toolSchemaChars = 0
                )
                response.text?.also { text ->
                    messages.add(Message(role = "assistant", content = text))
                }
                response.text
            },
            onFailure = { error ->
                Log.w(TAG, "sendEphemeral failed: ${error.message}")
                null
            }
        )
        }
    }

    private fun buildChatSystemPrompt(
        phoneControlAvailable: Boolean,
        modelName: String?,
        sensorContext: SensorContext?
    ): String {
        return promptBuilder?.full(
            phoneControlAvailable = phoneControlAvailable,
            modelName = modelName,
            sensorContext = sensorContext
        ) ?: PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = phoneControlAvailable,
            modelName = modelName,
            sensorContext = sensorContext
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
        return withRequestFlowLock {
        val phoneControlAvailable = phoneControlOverride ?: ScreenReader.isAttached()
        val sensorSnapshot = getTaskSensorSnapshot(startNewTask = false)
        val systemPrompt = promptBuilder?.trimmed(
            phoneControlAvailable = phoneControlAvailable,
            modelName = actionClient.modelId,
            sensorContext = sensorSnapshot
        )
            ?: PhoneAgentPrompts.buildActionPrompt(
                phoneControlAvailable = phoneControlAvailable,
                modelName = actionClient.modelId,
                sensorContext = sensorSnapshot
            )

        // Two-stage compaction: strip old SCREEN dumps first, then summarize old messages
        val (screenStripped, compactorMetrics) = contextCompactor.compactWithMetrics(messages)
        val (messagesForModel, managerMetrics) = contextManager.compactWithMetrics(screenStripped, currentToolStep)
        if (compactorMetrics != null) Log.d(TAG, "continueAfterTools compaction stage1: $compactorMetrics")
        if (managerMetrics != null) Log.d(TAG, "continueAfterTools compaction stage2: $managerMetrics")
        Log.d(TAG, "continueAfterTools: step=$currentToolStep, rawMessages=${messages.size}, compacted=${messagesForModel.size}")

        checkBudgetBeforeApiCall()
        val toolsForModel = getToolsForModel(actionClient.modelId)
        val result = actionClient.chatWithTools(
            messagesForModel,
            systemPrompt = systemPrompt,
            tools = toolsForModel
        )

        return result.fold(
            onSuccess = { response ->
                recordUsageOutcomeAndCheckBudgets(
                    usage = response.usage,
                    modelId = actionClient.modelId,
                    promptChars = approximateMessageChars(messagesForModel),
                    responseChars = approximateResponseChars(response),
                    systemPromptChars = systemPrompt.length,
                    toolSchemaChars = approximateToolSchemaChars(toolsForModel)
                )
                appendAssistantResponse(response)
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
                result.copy(text = "${result.text}\n[Verification skipped: ${verification.error}]")
            }
            verification.verified -> {
                result.copy(text = "${result.text}\n[Verified: ${verification.description}]")
            }
            else -> {
                result.copy(text = "${result.text}\n[Verification FAILED: ${verification.description}]")
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

                    when (clickElement(elementId)) {
                        is ScreenReader.ElementActionResult.Success -> ToolResult("Tapped element $elementId")
                        is ScreenReader.ElementActionResult.PrivacyBlocked ->
                            privacyBlockedResult("tap")
                        ScreenReader.ElementActionResult.ServiceUnavailable ->
                            executionFailedResult("Failed: tap: accessibility service unavailable")
                        ScreenReader.ElementActionResult.GestureDispatchFailed ->
                            executionFailedResult("Failed: tap: gesture dispatch failed")
                        ScreenReader.ElementActionResult.ElementNotFound ->
                            executionFailedResult("Failed: tap: element $elementId not found")
                    }
                }

                "tap_text" -> {
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("tap_text requires non-empty text (string)")

                    if (screenContent?.privacyMode == true) {
                        privacyBlockedResult("tap_text")
                    } else {
                        // Find element containing the text
                        val element = screenContent?.elements?.find {
                            it.text?.contains(text, ignoreCase = true) == true ||
                            it.contentDescription?.contains(text, ignoreCase = true) == true
                        }

                        when {
                            element == null -> executionFailedResult("Failed: tap_text: no element matching \"$text\"")
                            else -> when (clickElement(element.id)) {
                            is ScreenReader.ElementActionResult.Success -> ToolResult("Tapped \"$text\"")
                            is ScreenReader.ElementActionResult.PrivacyBlocked ->
                                privacyBlockedResult("tap_text")
                            ScreenReader.ElementActionResult.ServiceUnavailable ->
                                executionFailedResult("Failed: tap_text: accessibility service unavailable")
                            ScreenReader.ElementActionResult.GestureDispatchFailed ->
                                executionFailedResult("Failed: tap_text: gesture dispatch failed")
                            ScreenReader.ElementActionResult.ElementNotFound ->
                                executionFailedResult("Failed: tap_text: no element matching \"$text\"")
                        }
                    }
                    }
                }

                "type_text" -> {
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("type_text requires non-empty text (string)")

                    if (!ScreenReader.typeText(text)) {
                        executionFailedResult("Failed: type_text: no text field focused")
                    } else {
                        val mapsHandled = runCatching { executeMapsSuggestionStrategyIfApplicable(text) }
                            .getOrElse { false }
                        if (mapsHandled) {
                            ToolResult("Typed and submitted Maps search \"$text\"")
                        } else {
                            ToolResult("Typed \"$text\"")
                        }
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
                    
                    if (ScreenReader.swipe(startX, startY, endX, endY, durationMs = 500)) ToolResult("Swiped $direction")
                    else executionFailedResult("Failed: swipe: gesture not dispatched")
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
                    
                    if (ScreenReader.swipe(startX, startY, endX, endY, durationMs = 500)) ToolResult("Scrolled $direction")
                    else executionFailedResult("Failed: scroll: gesture not dispatched")
                }

                "press_back" -> {
                    if (ScreenReader.pressBack()) ToolResult("Pressed back")
                    else executionFailedResult("Failed: press_back: gesture not dispatched")
                }

                "press_home" -> {
                    if (ScreenReader.pressHome()) ToolResult("Pressed home")
                    else executionFailedResult("Failed: press_home: gesture not dispatched")
                }

                "open_app" -> {
                    val appName = toolCall.input["app_name"] as? String
                        ?: throw IllegalArgumentException("open_app requires app_name (string)")

                    if (ScreenReader.launchApp(appName)) {
                        ToolResult("Opened $appName")
                    } else {
                        ScreenReader.pressHome()
                        executionFailedResult("Failed: open_app: $appName not found — returned to home")
                    }
                }

                "open_notifications" -> {
                    if (ScreenReader.openNotifications()) ToolResult("Opened notifications")
                    else executionFailedResult("Failed: open_notifications: could not expand status bar")
                }

                "think" -> {
                    val thought = (toolCall.input["thought"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("think requires non-empty thought (string)")
                    ToolResult("Thought: $thought")
                }

                "wait" -> {
                    val seconds = ((toolCall.input["seconds"] as? Number)?.toInt() ?: 2).coerceIn(1, 5)
                    kotlinx.coroutines.delay(seconds * 1000L)
                    if (ScreenReader.isAttached()) {
                        val content = ScreenReader.getScreenContent()
                        ToolResult("Waited ${seconds}s. Screen:\n${content.toPromptText()}")
                    } else {
                        ToolResult("Waited ${seconds}s")
                    }
                }

                "long_press" -> {
                    val elementId = (toolCall.input["element_id"] as? Number)?.toInt()
                        ?: throw IllegalArgumentException("long_press requires element_id (integer)")

                    when (longPressElement(elementId)) {
                        is ScreenReader.ElementActionResult.Success -> ToolResult("Long-pressed element $elementId")
                        is ScreenReader.ElementActionResult.PrivacyBlocked ->
                            privacyBlockedResult("long_press")
                        ScreenReader.ElementActionResult.ServiceUnavailable ->
                            executionFailedResult("Failed: long_press: accessibility service unavailable")
                        ScreenReader.ElementActionResult.GestureDispatchFailed ->
                            executionFailedResult("Failed: long_press: gesture dispatch failed")
                        ScreenReader.ElementActionResult.ElementNotFound ->
                            executionFailedResult("Failed: long_press: element $elementId not found")
                    }
                }

                "copy" -> {
                    if (!ClipboardHelper.isAttached()) {
                        ToolResult(
                            CLIPBOARD_NOT_ATTACHED,
                            isError = true,
                            errorCode = ToolErrorCode.SERVICE_UNAVAILABLE
                        )
                    } else {
                        val text = ClipboardHelper.read()
                        if (text != null) {
                            ToolResult("Clipboard content: $text")
                        } else {
                            ToolResult("Clipboard is empty or access denied (Android 13+ may restrict clipboard reading)")
                        }
                    }
                }

                "set_clipboard" -> {
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("set_clipboard requires non-empty text (string)")
                    if (!ClipboardHelper.isAttached()) {
                        ToolResult(
                            CLIPBOARD_NOT_ATTACHED,
                            isError = true,
                            errorCode = ToolErrorCode.SERVICE_UNAVAILABLE
                        )
                    } else if (ClipboardHelper.write(text)) {
                        ToolResult("Copied to clipboard (${text.length} chars): \"${text.take(50)}${if (text.length > 50) "…" else ""}\"")
                    } else {
                        executionFailedResult("Failed: set_clipboard: clipboard write denied")
                    }
                }

                "paste" -> {
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("paste requires non-empty text (string)")
                    if (!ClipboardHelper.isAttached()) {
                        ToolResult(
                            CLIPBOARD_NOT_ATTACHED,
                            isError = true,
                            errorCode = ToolErrorCode.SERVICE_UNAVAILABLE
                        )
                    } else if (ClipboardHelper.writeAndPaste(text)) {
                        ToolResult("Pasted (${text.length} chars): \"${text.take(50)}${if (text.length > 50) "…" else ""}\"")
                    } else {
                        executionFailedResult("Failed: paste: no focused input field or clipboard write failed")
                    }
                }

                "read_notifications" -> {
                    if (!NotificationHelper.isAttached()) {
                        ToolResult(
                            NOTIFICATION_NOT_ATTACHED,
                            isError = true,
                            errorCode = ToolErrorCode.SERVICE_UNAVAILABLE
                        )
                    } else {
                        try {
                            val includeOngoing = toolCall.input["include_ongoing"] as? Boolean ?: false
                            val notifications = NotificationHelper.getActiveNotifications(includeOngoing)
                            ToolResult(NotificationHelper.formatForPrompt(notifications))
                        } catch (e: NotificationAccessDeniedException) {
                            ToolResult(
                                e.message ?: "Notification access denied",
                                isError = true,
                                errorCode = ToolErrorCode.ACCESS_DENIED
                            )
                        }
                    }
                }

                "tap_notification" -> {
                    val key = requireValidNotificationKey(toolCall, "tap_notification")
                    if (!NotificationHelper.isAttached()) {
                        ToolResult(
                            NOTIFICATION_NOT_ATTACHED,
                            isError = true,
                            errorCode = ToolErrorCode.SERVICE_UNAVAILABLE
                        )
                    } else if (NotificationHelper.tapNotification(key)) {
                        ToolResult("Opened notification")
                    } else {
                        executionFailedResult("Failed: tap_notification: notification may have been dismissed or has no content intent")
                    }
                }

                "dismiss_notification" -> {
                    val key = requireValidNotificationKey(toolCall, "dismiss_notification")
                    if (!NotificationHelper.isAttached()) {
                        ToolResult(
                            NOTIFICATION_NOT_ATTACHED,
                            isError = true,
                            errorCode = ToolErrorCode.SERVICE_UNAVAILABLE
                        )
                    } else if (NotificationHelper.dismissNotification(key)) {
                        ToolResult("Dismissed notification")
                    } else {
                        executionFailedResult("Failed: dismiss_notification: notification may be ongoing or already dismissed")
                    }
                }

                "reply_notification" -> {
                    val key = requireValidNotificationKey(toolCall, "reply_notification")
                    val text = (toolCall.input["text"] as? String)?.takeIf { it.isNotEmpty() }
                        ?: throw IllegalArgumentException("reply_notification requires non-empty text (string)")
                    if (!NotificationHelper.isAttached()) {
                        ToolResult(
                            NOTIFICATION_NOT_ATTACHED,
                            isError = true,
                            errorCode = ToolErrorCode.SERVICE_UNAVAILABLE
                        )
                    } else if (NotificationHelper.replyToNotification(key, text)) {
                        ToolResult("Replied to notification")
                    } else {
                        executionFailedResult("Failed: reply_notification: notification may not support inline reply or was dismissed")
                    }
                }

                "read_screen" -> {
                    if (isScreenReaderAttached()) {
                        val content = getScreenContent()
                        if (content.privacyMode) {
                            ToolResult(
                                "Screen refreshed:\n${content.toToolResult()}",
                                isError = true,
                                errorCode = ToolErrorCode.PRIVACY_BLOCKED
                            )
                        } else {
                            ToolResult("Screen refreshed:\n${content.toToolResult()}")
                        }
                    } else {
                        ToolResult(
                            "Accessibility service not attached",
                            isError = true,
                            errorCode = ToolErrorCode.SERVICE_UNAVAILABLE
                        )
                    }
                }

                "screenshot" -> {
                    if (!isScreenReaderAttached()) {
                        ToolResult(
                            "Accessibility service not attached",
                            isError = true,
                            errorCode = ToolErrorCode.SERVICE_UNAVAILABLE
                        )
                    } else {
                        val screenshot = takeScreenshot()
                        val base64 = when (screenshot) {
                            is ScreenshotResult.Success -> screenshot.base64
                            is ScreenshotResult.PrivacyBlocked -> return privacyBlockedResult("screenshot")
                            is ScreenshotResult.Failed -> return executionFailedResult(
                                "Failed: screenshot: ${screenshot.reason ?: "requires Android 11+"}"
                            )
                        }
                        val prompt = (toolCall.input["prompt"] as? String)?.takeIf { it.isNotBlank() }
                            ?: PhoneAgentPrompts.DEFAULT_VISION_PROMPT
                        // Use chat model (vision-capable) for screenshot description
                        val result = chatClient.describeImage(base64, prompt)
                        result.fold(
                            onSuccess = { description -> ToolResult("Screenshot description:\n$description") },
                            onFailure = { error -> executionFailedResult("Screenshot captured but vision failed: ${error.message}") }
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
                        ?: return invalidInputResult("Missing required parameter: query")
                    val count = (toolCall.input["count"] as? Number)?.toInt() ?: 3
                    searchClient.search(query, count)
                }

                "web_fetch" -> {
                    val url = toolCall.input["url"]?.toString()
                        ?: return invalidInputResult("Missing required parameter: url")
                    val maxChars = (toolCall.input["max_chars"] as? Number)?.toInt() ?: 5000
                    fetchClient.fetch(url, maxChars)
                }

                "web_browse" -> {
                    val client = tinyFishClient
                        ?: return ToolResult(
                            "Web browse not available: TinyFish API key not configured",
                            isError = true,
                            errorCode = ToolErrorCode.NOT_CONFIGURED
                        )
                    val url = toolCall.input["url"]?.toString()
                        ?: return invalidInputResult("Missing required parameter: url")
                    val goal = toolCall.input["goal"]?.toString()
                        ?: return invalidInputResult("Missing required parameter: goal")
                    val stealth = toolCall.input["stealth"] as? Boolean ?: false
                    client.browse(url = url, goal = goal, stealth = stealth, onProgress = onToolProgress)
                }

                "request_tools" -> {
                    val rawCategories = toolCall.input["categories"] as? List<*>
                        ?: return invalidInputResult("Missing required parameter: categories (array of strings)")
                    val validCategories = ToolCategory.entries
                        .filter { it != ToolCategory.CORE }
                        .map { it.name.lowercase() }
                        .sorted()

                    if (rawCategories.isEmpty()) {
                        return invalidInputResult(
                            "categories must contain at least one category. Available: ${validCategories.joinToString(", ")}",
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
                        return invalidInputResult(
                            "Invalid categories: ${invalidCategories.joinToString(", ")}. Available: ${validCategories.joinToString(", ")}",
                        )
                    }

                    if (requested.isEmpty()) {
                        return invalidInputResult(
                            "No valid categories requested. Available: ${validCategories.joinToString(", ")}",
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
                    ToolResult(
                        "Failed: unknown tool \"${toolCall.name}\"",
                        isError = true,
                        errorCode = ToolErrorCode.TOOL_NOT_FOUND
                    )
                }
            }
            val elapsedMs = System.currentTimeMillis() - startMs
            Log.d(TAG, "executeToolCall: name=${toolCall.name} done in ${elapsedMs}ms, result='${result.text.take(120)}'")
            result
        } catch (e: IllegalArgumentException) {
            val elapsedMs = System.currentTimeMillis() - startMs
            Log.e(TAG, "executeToolCall: name=${toolCall.name} INVALID_INPUT in ${elapsedMs}ms: ${e.message}")
            ToolResult(
                "Failed: ${toolCall.name}: ${e.message?.take(100)}",
                isError = true,
                errorCode = ToolErrorCode.INVALID_INPUT
            )
        } catch (e: NotificationAccessDeniedException) {
            val elapsedMs = System.currentTimeMillis() - startMs
            Log.e(TAG, "executeToolCall: name=${toolCall.name} ACCESS_DENIED in ${elapsedMs}ms: ${e.message}")
            ToolResult(
                e.message ?: "Notification access denied",
                isError = true,
                errorCode = ToolErrorCode.ACCESS_DENIED
            )
        } catch (e: Exception) {
            val elapsedMs = System.currentTimeMillis() - startMs
            Log.e(TAG, "executeToolCall: name=${toolCall.name} FAILED in ${elapsedMs}ms: ${e.message}")
            ToolResult(
                "Failed: ${toolCall.name}: ${e.message?.take(100)}",
                isError = true,
                errorCode = ToolErrorCode.EXECUTION_FAILED
            )
        }
    }

    /**
     * Runs Maps-specific suggestion handling after `type_text` only when Google Maps is foreground.
     *
     * This avoids generic Enter/tap behavior that can dismiss Maps suggestions and miss the intended
     * destination selection flow.
     */
    internal fun executeMapsSuggestionStrategyIfApplicable(query: String): Boolean {
        val service = ScreenReader.getService() ?: return false
        val screen = getScreenContent()
        if (screen.privacyMode) return false
        if (screen.packageName != MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE) return false

        val root = ScreenReader.findAppWindowRoot(service) ?: return false
        return try {
            val searchField = root.findFocus(android.view.accessibility.AccessibilityNodeInfo.FOCUS_INPUT)
                ?: return false
            try {
                val result = MapsSuggestionStrategy(
                    resources = service.resources,
                    readScreen = getScreenContent,
                    tapAt = { x, y -> ScreenReader.clickAt(x, y) },
                    pressEnterKey = { MapsSuggestionStrategy.defaultEnterKeyPress() }
                ).handleMapsSuggestion(searchField, query)
                result.success
            } finally {
                searchField.recycle()
            }
        } finally {
            root.recycle()
        }
    }

    private fun executionFailedResult(text: String): ToolResult =
        ToolResult(text, isError = true, errorCode = ToolErrorCode.EXECUTION_FAILED)

    private fun invalidInputResult(text: String): ToolResult =
        ToolResult(text, isError = true, errorCode = ToolErrorCode.INVALID_INPUT)

    private fun privacyBlockedResult(toolName: String): ToolResult =
        ToolResult(privacyBlockedByMode(toolName), isError = true, errorCode = ToolErrorCode.PRIVACY_BLOCKED)

    private inline fun fileToolResult(toolName: String, block: () -> String): ToolResult {
        return try {
            ToolResult(block())
        } catch (e: SecurityException) {
            ToolResult(
                fileToolError(toolName, "Access denied: ${e.message}"),
                isError = true,
                errorCode = ToolErrorCode.ACCESS_DENIED
            )
        } catch (e: IllegalArgumentException) {
            ToolResult(
                fileToolError(toolName, "Invalid input: ${e.message}"),
                isError = true,
                errorCode = ToolErrorCode.INVALID_INPUT
            )
        } catch (e: IllegalStateException) {
            ToolResult(
                fileToolError(toolName, "Tool not configured: ${e.message}"),
                isError = true,
                errorCode = ToolErrorCode.NOT_CONFIGURED
            )
        } catch (e: Exception) {
            ToolResult(
                fileToolError(toolName, e.message ?: "Unknown error"),
                isError = true,
                errorCode = ToolErrorCode.EXECUTION_FAILED
            )
        }
    }

    private fun privacyBlockedByMode(toolName: String): String =
        "Failed: $toolName: blocked by privacy mode for ${PrivacyRedaction.APP_PLACEHOLDER}"

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
            ToolResult(
                memoryToolError(toolName, "Invalid input: ${e.message}"),
                isError = true,
                errorCode = ToolErrorCode.INVALID_INPUT
            )
        } catch (e: IllegalStateException) {
            ToolResult(
                memoryToolError(toolName, "Tool not configured: ${e.message}"),
                isError = true,
                errorCode = ToolErrorCode.NOT_CONFIGURED
            )
        } catch (e: Exception) {
            ToolResult(
                memoryToolError(toolName, e.message ?: "Unknown error"),
                isError = true,
                errorCode = ToolErrorCode.EXECUTION_FAILED
            )
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
     * Determine whether the user is asking to continue/resume a paused task.
     *
     * Keeps matching intentionally narrow: short "continue/resume/proceed" variants.
     */
    private fun isResumeIntent(userMessage: String): Boolean {
        val normalized = userMessage
            .trim()
            .lowercase()
            .replace(Regex("[^a-z0-9\\s]"), "")
            .replace(Regex("\\s+"), " ")
        return normalized in setOf(
            "continue",
            "continue please",
            "resume",
            "resume please",
            "proceed",
            "proceed please",
            "go on",
            "keep going"
        )
    }

    /**
     * True when recent conversation history contains an interruption pause marker.
     *
     * This marker is injected by UserInterruptionCheck and signals that a short
     * follow-up like "continue" should re-enter actionable mode.
     */
    private fun hasRecentInterruptionPauseContext(windowSize: Int = 10): Boolean {
        val recent = messages.takeLast(windowSize)
        val markerIndex = recent.indexOfLast { msg ->
            msg.role == Message.ROLE_USER && msg.content.contains(INTERRUPTION_RESUME_MARKER)
        }
        if (markerIndex < 0) return false

        val trailing = recent.drop(markerIndex + 1)
        if (trailing.none { it.role == Message.ROLE_ASSISTANT }) return false

        // Resume window closes once any subsequent user turn occurs
        // (e.g. user canceled, asked something else, or moved on).
        if (trailing.any { it.role == Message.ROLE_USER }) return false

        return true
    }

    @androidx.annotation.VisibleForTesting
    internal fun extractExplicitWebFetchUrl(userMessage: String): String? {
        val normalized = userMessage.trim().lowercase()
        if (normalized.isEmpty()) return null
        if (!EXPLICIT_WEB_FETCH_HINTS.any { normalized.contains(it) }) return null

        val urlMatch = EXPLICIT_WEB_FETCH_URL_REGEX.find(userMessage) ?: return null
        val rawUrl = urlMatch.value

        val preContext = normalized.substring(0, urlMatch.range.first)
            .takeLast(120)
            .trim()
        val postContext = normalized.substring(urlMatch.range.last + 1)
            .take(120)
            .trim()

        val hasExplicitVerbBeforeUrl =
            preContext.matches(Regex(""".*\b(fetch|summari[sz]e|extract)\b.*""")) ||
                preContext.matches(Regex(""".*\bread\b(?:\s+(?:this|that|the))?\s+(url|link|page|article|website|webpage|site)\b.*"""))

        val hasExplicitVerbAfterUrl =
            postContext.matches(Regex("""^(?:,|\.|;|:|\)|\]|\s)*(and\s+)?(fetch|summari[sz]e|extract)\b.*""")) ||
                postContext.matches(Regex("""^(?:,|\.|;|:|\)|\]|\s)*(and\s+)?read\s+(it|this|that|the\s+(url|link|page|article|website|webpage|site))\b.*"""))

        if (!hasExplicitVerbBeforeUrl && !hasExplicitVerbAfterUrl) return null

        return rawUrl.trimEnd('.', ',', ';', ':', ')', ']', '>')
    }

    @androidx.annotation.VisibleForTesting
    internal fun enforceExplicitWebFetchIntent(
        userMessage: String,
        response: ChatResponse,
        isActionLoop: Boolean,
        modelId: String?
    ): ChatResponse {
        if (isActionLoop) return response
        val requestedUrl = extractExplicitWebFetchUrl(userMessage) ?: return response
        if (response.toolCalls.any { it.name == "web_fetch" }) return response

        val tools = getToolsForModel(modelId)
        if (tools.none { it.name == "web_fetch" }) return response

        val noActionableToolSelection =
            response.toolCalls.isEmpty() || response.toolCalls.all { it.name == "think" }
        if (!noActionableToolSelection) return response

        val forcedToolCall = ToolCall(
            id = buildForcedWebFetchToolCallId(requestedUrl),
            name = "web_fetch",
            input = mapOf("url" to requestedUrl)
        )
        val mergedToolCalls = if (response.toolCalls.isEmpty()) {
            listOf(forcedToolCall)
        } else {
            response.toolCalls + forcedToolCall
        }

        Log.d(
            TAG,
            "enforceExplicitWebFetchIntent: injecting web_fetch for explicit fetch URL " +
                "(url=$requestedUrl, originalStopReason=${response.stopReason}, originalTools=${response.toolCalls.map { it.name }})"
        )

        return response.copy(
            text = response.text?.takeIf { it.isNotBlank() } ?: "Fetching requested URL.",
            toolCalls = mergedToolCalls,
            stopReason = "tool_use"
        )
    }

    private fun buildForcedWebFetchToolCallId(requestedUrl: String): String {
        val digest = MessageDigest.getInstance("SHA-256")
            .digest(requestedUrl.toByteArray(StandardCharsets.UTF_8))
        val suffix = digest.joinToString("") { "%02x".format(it) }.take(24)
        return "forced_web_fetch_$suffix"
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
     * Thread-safe and non-blocking for callers:
     * - If no request is active, clears immediately.
     * - If a request is active, defers clear until lock release.
     */
    fun clearConversation() {
        clearRequested.set(true)
        if (!requestFlowMutex.tryLock()) {
            return
        }
        try {
            applyDeferredClearIfRequested()
        } finally {
            requestFlowMutex.unlock()
        }
    }

    private suspend inline fun <T> withRequestFlowLock(block: suspend () -> T): T {
        return requestFlowMutex.withLock {
            deferredSeedBootstrapEligibleInActiveFlow.set(messages.isEmpty())
            try {
                block()
            } finally {
                onBeforeDeferredMaintenanceInActiveFlow?.invoke()
                applyDeferredMaintenance()
                deferredSeedBootstrapEligibleInActiveFlow.set(false)
            }
        }
    }

    private fun applyDeferredMaintenance() {
        if (applyDeferredClearIfRequested()) {
            deferredSeedMessages.set(null)
            return
        }
        applyDeferredSeedIfRequested()
    }

    private fun applyDeferredClearIfRequested(): Boolean {
        if (clearRequested.getAndSet(false)) {
            clearConversationUnlocked()
            return true
        }
        return false
    }

    private fun applyDeferredSeedIfRequested() {
        val pending = deferredSeedMessages.getAndSet(null) ?: return
        // Invariant: deferred prepend is only valid for bootstrap restore when the live
        // in-memory history was empty at deferral time. Otherwise the seed may be stale
        // and must never be prepended to an already-populated conversation.
        if (!pending.liveHistoryWasEmptyAtDeferral) {
            emitDeferredSeedSignal(
                action = DeferredSeedAction.SKIPPED,
                reason = DeferredSeedReason.NON_EMPTY_LIVE_HISTORY_AT_DEFERRAL,
                seedMessageCount = pending.uiMessages.size,
                liveMessageCount = messages.size
            )
            return
        }
        val applied = applySeedConversationHistoryUnlocked(
            pending.uiMessages,
            allowPrependToExisting = true
        )
        emitDeferredSeedSignal(
            action = if (applied) DeferredSeedAction.APPLIED else DeferredSeedAction.SKIPPED,
            reason = if (applied) {
                DeferredSeedReason.BOOTSTRAP_EMPTY_HISTORY
            } else {
                DeferredSeedReason.EMPTY_AFTER_NORMALIZATION
            },
            seedMessageCount = pending.uiMessages.size,
            liveMessageCount = messages.size
        )
    }

    private fun clearConversationUnlocked() {
        messages.clear()
        deferredSeedMessages.set(null)
        currentToolStep = 0
        taskSensorSnapshotEpoch.incrementAndGet()
        synchronized(taskSensorSnapshotLock) {
            taskSensorSnapshot = null
            taskSensorSnapshotInitialized = false
        }
        resetTaskCostTracking()
    }

    @Synchronized
    private fun resetTaskCostTracking() {
        taskTokenAccumulator.reset()
        taskEstimatedCostNanodollars = 0L
        taskEstimatedCostMicrodollars = 0L
        lastTaskCostSummary = TaskCostSummary.EMPTY
    }

    private fun checkBudgetBeforeApiCall() {
        budgetGuard?.checkTaskLimitDecisionMicrodollars(taskEstimatedCostMicrodollars)?.let { error ->
            throw BudgetExceededException(error.message, error.code)
        }
        budgetGuard?.checkWouldExceedBudgetWithoutSpendingDecision()?.let { error ->
            throw BudgetExceededException(error.message, error.code)
        }
    }

    @Synchronized
    private fun recordUsageAndCheckBudgets(usage: TokenUsage, modelId: String?) {
        val callCost = CostEstimator.estimate(usage, modelId)
        recordEstimatedCost(usage, callCost)

        when (val decision = budgetGuard?.trySpendDecision(callCost)) {
            null, BudgetDecision.Allowed -> Unit
            is BudgetDecision.OverLimit -> throw BudgetExceededException(decision.message, decision.code)
            is BudgetDecision.MissingUsageMetadata -> {
                decision.overLimitMessage?.let { throw BudgetExceededException(it, decision.overLimitCode) }
            }
        }
        budgetGuard?.checkTaskLimitDecisionMicrodollars(taskEstimatedCostMicrodollars)?.let { error ->
            throw BudgetExceededException(error.message, error.code)
        }
    }

    private fun recordUsageOutcomeAndCheckBudgets(
        usage: TokenUsage?,
        modelId: String?,
        promptChars: Int,
        responseChars: Int,
        systemPromptChars: Int,
        toolSchemaChars: Int
    ) {
        if (usage != null) {
            recordUsageAndCheckBudgets(usage, modelId)
            return
        }

        val fallbackUsage = estimateFallbackUsage(
            promptChars = promptChars,
            responseChars = responseChars,
            systemPromptChars = systemPromptChars,
            toolSchemaChars = toolSchemaChars
        )
        val fallbackCost = max(CostEstimator.estimate(fallbackUsage, modelId), 0.000001)
        recordEstimatedCost(fallbackUsage, fallbackCost)

        val fallbackDecision = budgetGuard?.recordFallbackSpendForMissingUsage(fallbackCost)
        if (fallbackDecision?.overLimitMessage != null) {
            throw BudgetExceededException(
                fallbackDecision.overLimitMessage,
                fallbackDecision.overLimitCode ?: BudgetErrorCode.MISSING_USAGE_FALLBACK
            )
        }
        budgetGuard?.checkTaskLimitDecisionMicrodollars(taskEstimatedCostMicrodollars)?.let { error ->
            throw BudgetExceededException(error.message, error.code)
        }
    }

    private fun estimateFallbackUsage(
        promptChars: Int,
        responseChars: Int,
        systemPromptChars: Int,
        toolSchemaChars: Int
    ): TokenUsage {
        val estimatedInputChars =
            promptChars + systemPromptChars + toolSchemaChars + FALLBACK_PROVIDER_FRAMING_CHARS
        val estimatedOutputChars = responseChars + FALLBACK_PROVIDER_FRAMING_CHARS
        val estimatedInputTokens = max(
            1,
            ceil((estimatedInputChars * FALLBACK_INPUT_CONSERVATISM_FACTOR) / 4.0).toInt()
        )
        val estimatedOutputTokens = max(
            1,
            ceil((estimatedOutputChars * FALLBACK_OUTPUT_CONSERVATISM_FACTOR) / 4.0).toInt()
        )
        return TokenUsage(inputTokens = estimatedInputTokens, outputTokens = estimatedOutputTokens)
    }

    private fun approximateConversationChars(messages: List<Message>): Int = approximateMessageChars(messages)

    private fun approximateMessageChars(messages: List<Message>): Int {
        return messages.sumOf { msg ->
            msg.content.length + msg.role.length + (msg.toolCallsJson?.length ?: 0) + (msg.toolCallId?.length ?: 0)
        }
    }

    private fun approximateResponseChars(response: ChatResponse): Int {
        val toolCallChars = response.toolCalls.sumOf { call ->
            call.id.length + call.name.length + stableSerializedLength(call.input)
        }
        return (response.text?.length ?: 0) + toolCallChars
    }

    private fun approximateToolSchemaChars(tools: List<Tool>): Int {
        if (tools.isEmpty()) return 0
        return tools.sumOf { tool ->
            tool.name.length +
                tool.description.length +
                stableSerializedLength(tool.inputSchema)
        }
    }

    private fun applySeedConversationHistoryUnlocked(
        uiMessages: List<Message>,
        allowPrependToExisting: Boolean
    ): Boolean {
        if (uiMessages.isEmpty()) return false
        if (messages.isNotEmpty() && !allowPrependToExisting) return false

        val seeded = uiMessages.mapNotNull { msg ->
            when {
                msg.role == Message.ROLE_USER -> Message(role = "user", content = msg.content)
                msg.role == Message.ROLE_ASSISTANT && msg.content.isNotBlank() -> {
                    val textOnly = msg.content
                        .replace(Regex("""\s*\[Tools:.*?]"""), "")
                        .trim()
                    if (textOnly.isNotEmpty()) Message(role = "assistant", content = textOnly) else null
                }
                else -> null
            }
        }

        val dedupedSeed = dedupeConsecutiveRoles(seeded)
        if (dedupedSeed.isEmpty()) return false

        val merged = if (messages.isEmpty()) {
            dedupedSeed
        } else {
            if (!allowPrependToExisting) return false
            mergeSeedWithLiveMessages(dedupedSeed, messages.toList())
        }

        messages.clear()
        messages.addAll(merged)
        Log.d(
            TAG,
            "seedConversationHistory: applied ${merged.size} messages from UI (${seeded.size - dedupedSeed.size} consecutive dupes removed)"
        )
        return true
    }

    private data class DeferredSeedRequest(
        val uiMessages: List<Message>,
        val liveHistoryWasEmptyAtDeferral: Boolean
    )

    private enum class DeferredSeedAction(val value: String) {
        APPLIED("applied"),
        SKIPPED("skipped")
    }

    private enum class DeferredSeedReason(val value: String) {
        BOOTSTRAP_EMPTY_HISTORY("bootstrap_empty_history"),
        EMPTY_AFTER_NORMALIZATION("empty_after_normalization"),
        NON_EMPTY_LIVE_HISTORY_AT_DEFERRAL("non_empty_live_history_at_deferral")
    }

    private fun emitDeferredSeedSignal(
        action: DeferredSeedAction,
        reason: DeferredSeedReason,
        seedMessageCount: Int,
        liveMessageCount: Int
    ) {
        Log.d(
            TAG,
            "deferred_seed action=${action.value} reason=${reason.value} seedCount=$seedMessageCount liveCount=$liveMessageCount"
        )
        onDeferredSeedSignal?.invoke(
            DeferredSeedSignal(
                action = action.value,
                reason = reason.value,
                seedMessageCount = seedMessageCount,
                liveMessageCount = liveMessageCount
            )
        )
    }

    private fun dedupeConsecutiveRoles(source: List<Message>): List<Message> {
        if (source.isEmpty()) return emptyList()
        val deduped = mutableListOf<Message>()
        source.forEach { msg ->
            if (deduped.isEmpty() || deduped.last().role != msg.role) {
                deduped += msg
            }
        }
        return deduped
    }

    private fun mergeSeedWithLiveMessages(seed: List<Message>, live: List<Message>): List<Message> {
        if (seed.isEmpty()) return live
        if (live.isEmpty()) return seed
        return if (seed.last().role == live.first().role) {
            seed.dropLast(1) + live
        } else {
            seed + live
        }
    }

    private fun stableSerializedLength(value: Any?): Int {
        val json = JsonUtils.anyToJsonElement(value)
        return canonicalizeJsonObjectOrder(json).toString().length
    }

    private fun canonicalizeJsonObjectOrder(element: JsonElement): JsonElement {
        return when (element) {
            is JsonObject -> buildJsonObject {
                element.entries
                    .sortedBy { it.key }
                    .forEach { (key, value) -> put(key, canonicalizeJsonObjectOrder(value)) }
            }
            is JsonArray -> buildJsonArray {
                element.forEach { add(canonicalizeJsonObjectOrder(it)) }
            }
            else -> element
        }
    }

    @Synchronized
    private fun recordEstimatedCost(usage: TokenUsage, estimatedCostUsd: Double) {
        taskTokenAccumulator.record(usage)
        taskEstimatedCostNanodollars += usdToNanodollars(estimatedCostUsd)
        taskEstimatedCostMicrodollars = taskEstimatedCostNanodollars / NANODOLLARS_PER_MICRODOLLAR
        lastTaskCostSummary = TaskCostSummary(
            totalTokens = taskTokenAccumulator.totalTokens,
            inputTokens = taskTokenAccumulator.totalInputTokens,
            outputTokens = taskTokenAccumulator.totalOutputTokens,
            apiCalls = taskTokenAccumulator.callCount,
            estimatedCostUsd = taskEstimatedCostNanodollars / NANODOLLARS_PER_USD.toDouble()
        )
    }

    private fun usdToNanodollars(usd: Double): Long = (usd * NANODOLLARS_PER_USD).roundToLong()

    private fun appendAssistantResponse(response: ChatResponse) {
        if (response.toolCalls.isNotEmpty()) {
            messages.add(Message.assistantWithTools(response.text, response.toolCalls))
        } else if (response.text != null) {
            messages.add(Message(role = "assistant", content = response.text))
        }
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
