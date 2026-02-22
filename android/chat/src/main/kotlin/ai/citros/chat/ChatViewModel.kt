package ai.citros.chat

import androidx.annotation.VisibleForTesting
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import ai.citros.core.*
import ai.citros.core.VoiceManager
import android.util.Log
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean

/** Reason the conversation was auto-cleared by lifecycle checks. */
enum class LifecycleClearReason {
    IDLE_TIMEOUT,
    DAILY_RESET
}

class ChatViewModel : ViewModel(), ToolExecutionDelegate, LoopProgressListener {
    
    companion object {
        private const val TAG = "CitrosLoop"
        private const val MAX_TTS_CHARS = 4000

        /** Maximum number of tool execution steps before forcing loop exit.
         *  Simple tasks (open app, send email) take 1-10 steps; complex tasks
         *  (flight search, multi-form web navigation) need 15-20+. Stuck detection
         *  (screen hash tracking + wait detection) is the real guard against loops;
         *  this limit is a hard ceiling for genuinely long flows. */
        private const val MAX_TOOL_STEPS = 25

        /** Number of identical consecutive screen hashes that trigger a stuck warning. */
        private const val STUCK_SCREEN_THRESHOLD = 3

        /** Number of consecutive wait calls with no screen change that trigger a warning. */
        private const val STUCK_WAIT_THRESHOLD = 2
        
        /** Delay after tap actions to allow UI transitions to complete */
        private const val DELAY_AFTER_TAP = 800L
        
        /** Default delay for other actions */
        private const val DELAY_DEFAULT = 500L

        /** How long to wait for the accessibility service to reattach before aborting. */
        private const val ACCESSIBILITY_WAIT_MS_DEFAULT = 5000L

        /**
         * Map a tool name to a user-friendly status label using [OutputClassifier.categoryOf].
         *
         * - MECHANICAL tools → generic "Interacting..." (individual taps/swipes are noise)
         * - PROMINENT tools → specific action, e.g. "Opening Gmail..."
         * - RESEARCH tools  → "Searching the web..." / "Fetching page..."
         * - REASONING tools → "Thinking..."
         * - OTHER           → tool name as-is with "Running ..."
         */
        internal fun toolStatusLabel(toolName: String): String {
            return when (OutputClassifier.categoryOf(toolName)) {
                OutputToolCategory.MECHANICAL -> "Interacting..."
                OutputToolCategory.PROMINENT -> when (toolName) {
                    "open_app" -> "Opening app..."
                    "open_notifications" -> "Opening notifications..."
                    "screenshot" -> "Taking screenshot..."
                    "subtask" -> "Running subtask..."
                    else -> "Running $toolName..."
                }
                OutputToolCategory.RESEARCH -> when (toolName) {
                    "web_search" -> "Searching the web..."
                    "web_fetch" -> "Fetching page..."
                    "web_browse" -> "Browsing the web..."
                    else -> "Researching..."
                }
                OutputToolCategory.REASONING -> "Thinking..."
                OutputToolCategory.OTHER -> "Running $toolName..."
            }
        }
    }

    /** Mutable state for stuck detection, reset per tool loop. */
    @androidx.annotation.VisibleForTesting
    internal data class StuckDetectionState(
        val recentScreenHashes: MutableList<Int> = mutableListOf(),
        var consecutiveWaits: Int = 0,
        var uniqueScreens: Int = 0
    )

    /**
     * Check for stuck conditions and return a warning to inject into the tool result,
     * or null if the agent is making progress.
     *
     * Two detection modes:
     * 1. Screen hash repetition — all hashes in rolling window are identical
     * 2. Consecutive waits with no screen change — waiting isn't helping
     *
     * Screen-stuck warning takes precedence over wait warning (more actionable).
     */
    @Deprecated("Use StuckDetector in :core instead. Kept for backward compatibility with existing tests.")
    @VisibleForTesting
    internal fun checkStuck(
        state: StuckDetectionState,
        toolName: String,
        screenContent: ScreenContent?
    ): String? {
        var warning: String? = null

        // Track screen hashes
        if (screenContent != null) {
            val hash = screenContent.hashCode()
            // Count as new only if different from the last screen seen
            val isNew = state.recentScreenHashes.lastOrNull() != hash
            if (isNew) state.uniqueScreens++
            state.recentScreenHashes.add(hash)
            // Keep rolling window
            if (state.recentScreenHashes.size > STUCK_SCREEN_THRESHOLD) {
                state.recentScreenHashes.removeAt(0)
            }
            // All hashes in the rolling window are identical → stuck
            if (state.recentScreenHashes.size >= STUCK_SCREEN_THRESHOLD &&
                state.recentScreenHashes.distinct().size == 1) {
                warning = "\n\n⚠️ STUCK: The screen has not changed in $STUCK_SCREEN_THRESHOLD actions " +
                    "(only ${state.uniqueScreens} unique screen${if (state.uniqueScreens == 1) "" else "s"} seen). " +
                    "Try a different approach (scroll, tap a different element, press back) " +
                    "or tell the user what's blocking you."
                Log.w(TAG, "stuckDetection: screen unchanged for $STUCK_SCREEN_THRESHOLD steps")
            }
        }

        // Track consecutive waits. Screen-stuck warning takes precedence
        // (more actionable), so skip wait warning if already set.
        if (toolName == "wait") {
            state.consecutiveWaits++
            val screenIsStuck = state.recentScreenHashes.size >= 2 &&
                state.recentScreenHashes.distinct().size == 1
            if (state.consecutiveWaits >= STUCK_WAIT_THRESHOLD && screenIsStuck && warning == null) {
                warning = "\n\n⚠️ Waiting more won't help — the screen hasn't changed. " +
                    "Take a different action."
                Log.w(TAG, "stuckDetection: ${state.consecutiveWaits} consecutive waits")
            }
        } else {
            state.consecutiveWaits = 0
        }

        return warning
    }

    private var cloudApiClient: ProviderClient? = null
    private var localLLMClient: LocalLLMClient? = null
    private var phoneAgentApi: PhoneAgentApi? = null
    private var systemPrompt: String = PhoneAgentPrompts.buildSystemPrompt()
    private var phoneAgentLocal: PhoneAgentLocal? = null
    private var lastWalletProvider: Provider? = null
    private var lastWalletChatModelId: String? = null
    private var lastWalletActionModelId: String? = null

    /** Base URL for SearXNG or other search provider. Set via [setSearchConfig]. */
    private var searchBaseUrl: String? = null

    /** Brave Search API key for fallback search. Set via [setSearchConfig]. */
    private var braveApiKey: String? = null

    /** TinyFish Web Agent API key for browser automation. Set via [setSearchConfig]. */
    private var tinyFishApiKey: String? = null

    /** Citros app token for authenticating to Citros API endpoints. Set via [setSearchConfig]. */
    private var citrosAppToken: String? = null

    /** Agent file manager for knowledge file tools (learn, read_file, etc.). */
    private var agentFileManager: AgentFileManager? = null

    /**
     * Update search provider configuration.
     *
     * Call from Activity after reading SharedPreferences. These values are used
     * when [configureWithWallet] builds the [PhoneAgentApi] backend.
     */
    fun setSearchConfig(searxngUrl: String? = null, braveKey: String? = null, tinyFishKey: String? = null) {
        searchBaseUrl = searxngUrl
        braveApiKey = braveKey
        tinyFishApiKey = tinyFishKey
    }

    /**
     * Set the agent file manager for knowledge tools (learn, read_file, write_file, list_files).
     *
     * Call from Activity after creating [AgentFileManager.fromContext]. Must be set
     * before [configureWithWallet] so backends are built with file tool support.
     */
    fun setAgentFileManager(manager: AgentFileManager) {
        agentFileManager = manager
    }

    /**
     * Update keys delivered from the Citros key delivery endpoint.
     * Only updates non-null values — does not overwrite user-provided Settings.
     *
     * If API backends are already configured, rebuild them so runtime key updates
     * (app token / TinyFish key) are applied to active PhoneAgentApi instances.
     */
    fun updateCitrosKeys(appToken: String? = null, tinyFishKey: String? = null) {
        val appTokenChanged = appToken != null && appToken != citrosAppToken
        val tinyFishKeyChanged = tinyFishKey != null && tinyFishKey != tinyFishApiKey
        if (!appTokenChanged && !tinyFishKeyChanged) return

        if (appTokenChanged) citrosAppToken = appToken
        if (tinyFishKeyChanged) tinyFishApiKey = tinyFishKey

        rebuildApiBackendsForRuntimeKeyUpdate()
    }

    private enum class Mode { API, LOCAL }
    private var mode: Mode = Mode.API

    private data class ApiBackend(
        val provider: Provider,
        val chatClient: ProviderClient,
        val actionClient: ProviderClient,
        val agent: PhoneAgentApi
    )

    private val apiBackends = mutableListOf<ApiBackend>()
    private val apiBackendConfigs = mutableListOf<ProviderConfig>()
    private var activeApiBackendIndex: Int = -1

    val messages = mutableStateListOf<Message>()
    val isLoading = mutableStateOf(false)
    val error = mutableStateOf<String?>(null)

    /**
     * Monotonically increasing counter bumped each time a streaming message's
     * content is updated in-place. Compose observers can key on this to
     * auto-scroll during streaming even though [messages].size stays constant.
     * @see #618
     */
    val streamingContentVersion = mutableIntStateOf(0)

    /** User-facing status text updated as tools execute (e.g. "Opening Gmail...", "Searching the web..."). */
    val currentToolStatus = mutableStateOf<String?>(null)
    val isConfigured = mutableStateOf(false)
    val needsAuth = mutableStateOf(false)
    val accessibilityEnabled = mutableStateOf(false)
    val queuedMessage = mutableStateOf<String?>(null)
    val unreadCount = mutableIntStateOf(0)

    // ── Voice I/O state ──
    private val _voiceManager = MutableStateFlow<VoiceManager?>(null)
    val voiceManager: StateFlow<VoiceManager?> = _voiceManager.asStateFlow()

    private val _voiceReady = MutableStateFlow(false)
    /** True when VoiceManager is initialized and ready for use. */
    val voiceReady: StateFlow<Boolean> = _voiceReady.asStateFlow()

    /**
     * Set the VoiceManager after model extraction and provider initialization.
     * Called from ChatActivity once voice providers are ready.
     */
    fun setVoiceManager(vm: VoiceManager) {
        _voiceManager.value = vm
        _voiceReady.value = true
    }

    override fun onCleared() {
        super.onCleared()
        releaseVoiceManager()
    }

    /**
     * Release the VoiceManager. Called automatically via [onCleared].
     */
    fun releaseVoiceManager() {
        _voiceManager.value?.release()
        _voiceManager.value = null
        _voiceReady.value = false
    }

    /**
     * Speak the given text via TTS if auto-speak is enabled.
     * Fire-and-forget — does not block the calling coroutine.
     */
    private fun speakIfEnabled(text: String) {
        val vm = _voiceManager.value ?: return
        if (!vm.autoSpeakResponses.value) return
        if (text.isBlank()) return
        viewModelScope.launch {
            try {
                val speakText = if (text.length > MAX_TTS_CHARS) {
                    Log.w(TAG, "TTS text truncated from ${text.length} to $MAX_TTS_CHARS chars")
                    text.take(MAX_TTS_CHARS)
                } else text
                vm.activeTts.value.speak(speakText)
            } catch (e: Exception) {
                Log.w(TAG, "Auto-speak failed: ${e.message}")
            }
        }
    }

    /** User preference for tool output verbosity. Default: NORMAL (hide mechanical actions). */
    var outputVerbosity: OutputVerbosity = OutputVerbosity.NORMAL

    /** On-device memory provider for remember/recall/list_memories tools. */
    private var memoryProvider: MemoryProvider? = null

    /** Device sensor provider for runtime prompt context injection. */
    private var sensorProvider: SensorProvider? = null

    private val toolLoopCancelled = AtomicBoolean(false)
    private var lastUserMessage: String? = null

    /**
     * Stashed task message from when accessibility was unavailable (#447).
     * Set when the user sends a task but accessibility is off; consumed on
     * the next message after the user re-enables accessibility.
     */
    @VisibleForTesting
    internal var pendingTaskMessage: String? = null

    /**
     * Tracks the last abnormal tool loop exit reason (#603).
     * Consumed on the next [sendMessage] call to prepend context.
     * Matches OpenClaw's abortedLastRun reactive-hint pattern.
     */
    @VisibleForTesting
    internal var lastExitReason: ToolLoopExit? = null

    /** Queue for mid-loop steer messages from the user. Thread-safe. */
    @VisibleForTesting
    internal val steerQueue = ConcurrentLinkedQueue<String>()

    /** Observable flag: true when steer messages are queued but not yet consumed. */
    val hasQueuedSteer = mutableStateOf(false)

    /** Epoch millis of last user/agent activity. Updated on send and tool result. */
    var lastActivityTimestamp: Long = 0L

    /** Accessibility reattachment timeout. Override in tests for speed. */
    @VisibleForTesting
    internal var accessibilityWaitMs: Long = ACCESSIBILITY_WAIT_MS_DEFAULT

    /**
     * After open_app or press_home, poll until the screen package changes from Citros.
     * Returns the new [ScreenContent] once the target app has focus, or the current
     * screen content if timeout expires (degrades to current behavior).
     *
     * @param currentPackage The package to wait to leave (typically Citros's own package)
     * @param timeoutMs Maximum time to poll
     * @param pollIntervalMs Interval between polls
     */
    private suspend fun pollForPackageChangeImpl(
        currentPackage: String?,
        timeoutMs: Long = 3000L,
        pollIntervalMs: Long = 300L
    ): ScreenContent? {
        if (currentPackage == null) return null
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            delay(pollIntervalMs)
            val content = try {
                if (ScreenReader.isAttached()) ScreenReader.getScreenContent() else null
            } catch (_: Exception) { null }
            if (content != null && content.packageName != currentPackage) {
                Log.d(TAG, "pollForPackageChange: switched to ${content.packageName} (was $currentPackage)")
                return content
            }
        }
        Log.w(TAG, "pollForPackageChange: timed out after ${timeoutMs}ms, still on $currentPackage")
        return try {
            if (ScreenReader.isAttached()) ScreenReader.getScreenContent() else null
        } catch (_: Exception) { null }
    }

    /**
     * Check if the screen reader is available. Delegates to [ScreenReader.isAttached]
     * in production. Override in tests to simulate detachment scenarios.
     */
    @VisibleForTesting
    internal var screenReaderAvailableOverride: Boolean? = null


    /**
     * Configure with wallet manager.
     *
     * Retrieves the active configuration from the wallet and sets up API backends.
     * If no active key is configured, sets needsAuth to true.
     *
     * @param walletManager The wallet manager containing credentials and configuration
     */
    fun configureWithWallet(walletManager: WalletManager) {
        val config = walletManager.activeConfig()
        if (config == null) {
            needsAuth.value = true
            isConfigured.value = false
            return
        }

        val backend = buildWalletBackend(config)

        apiBackends.clear()
        apiBackendConfigs.clear()
        apiBackends.add(backend)
        apiBackendConfigs.add(config)
        activateApiBackend(0)

        lastWalletProvider = config.provider
        lastWalletChatModelId = config.chatModelId
        lastWalletActionModelId = config.actionModelId

        mode = Mode.API
        isConfigured.value = true
        needsAuth.value = false
    }

    /**
     * Lightweight update for model-only wallet changes.
     * Rebuilds only the active backend clients when provider/key is unchanged.
     * Falls back to a full wallet configure when needed.
     */
    fun updateModelsFromWallet(walletManager: WalletManager) {
        val config = walletManager.activeConfig()
        val activeIndex = activeApiBackendIndex
        val activeBackend = apiBackends.getOrNull(activeIndex)

        if (config == null || mode != Mode.API || activeBackend == null || activeBackend.provider != config.provider) {
            configureWithWallet(walletManager)
            return
        }

        // Check both chat AND action model IDs (#609)
        val modelsUnchanged =
            lastWalletProvider == config.provider &&
                lastWalletChatModelId == config.chatModelId &&
                lastWalletActionModelId == config.actionModelId
        if (modelsUnchanged) {
            return
        }

        // Model switch creates a new PhoneAgentApi with empty messages.
        // Don't transfer raw history — it may contain provider-specific tool
        // formats or blow the new model's context window. Instead, rely on
        // seedConversationHistory() called in ChatViewModel.sendMessage() which re-seeds text-only
        // conversational context from UI messages on the next turn (#609).
        val updatedBackend = buildWalletBackend(config)
        apiBackends[activeIndex] = updatedBackend
        activateApiBackend(activeIndex)

        lastWalletProvider = config.provider
        lastWalletChatModelId = config.chatModelId
        lastWalletActionModelId = config.actionModelId

        mode = Mode.API
        isConfigured.value = true
        needsAuth.value = false
    }

    /**
     * Configure with any supported cloud credential.
     *
     * Provider is resolved from explicit auth hints or token detection.
     * If provider cannot be confidently detected, configuration fails fast.
     */
    fun configureWithToken(
        token: String,
        preferredProvider: Provider? = null,
        authKind: CloudAuthKind? = null
    ) {
        val providerOrder = determineProviderOrder(token, preferredProvider, authKind)
        val backendWithConfigs = providerOrder
            .distinct()
            .mapNotNull { provider ->
                runCatching {
                    val config = when (provider) {
                        Provider.ANTHROPIC -> ProviderConfig.anthropic(token)
                        Provider.OPENROUTER -> ProviderConfig.openRouter(token)
                        Provider.OPENAI -> ProviderConfig.openAi(token)
                    }
                    config to buildWalletBackend(config)
                }.getOrNull()
            }

        val builtBackends = backendWithConfigs.map { it.second }

        if (builtBackends.isEmpty()) {
            error.value = "Could not detect provider. Please select your provider explicitly."
            isConfigured.value = false
            return
        }

        apiBackends.clear()
        apiBackendConfigs.clear()
        apiBackends.addAll(builtBackends)
        apiBackendConfigs.addAll(backendWithConfigs.map { it.first })
        activateApiBackend(0)

        lastWalletProvider = null
        lastWalletChatModelId = null
        lastWalletActionModelId = null

        mode = Mode.API
        isConfigured.value = true
        needsAuth.value = false
    }

    /**
     * Configure the chat client with a local LLM server (Ollama or llama.cpp).
     *
     * @param baseUrl The base URL of the local LLM server
     * @param model The model name to use (default: qwen2.5:3b)
     */
    fun configureWithLocalLLM(baseUrl: String, model: String = "qwen2.5:3b") {
        localLLMClient = LocalLLMClient(
            baseUrl = baseUrl,
            model = model,
            systemPrompt = PhoneAgentLocal.LOCAL_SYSTEM_PROMPT
        )
        phoneAgentLocal = PhoneAgentLocal(localLLMClient!!)
        apiBackends.clear()
        apiBackendConfigs.clear()
        activeApiBackendIndex = -1
        lastWalletProvider = null
        lastWalletChatModelId = null
        lastWalletActionModelId = null
        mode = Mode.LOCAL
        isConfigured.value = true
        needsAuth.value = false
    }

    private fun determineProviderOrder(
        token: String,
        preferredProvider: Provider?,
        authKind: CloudAuthKind?
    ): List<Provider> {
        val authHintProvider = when (authKind) {
            CloudAuthKind.ANTHROPIC_CREDENTIAL -> Provider.ANTHROPIC
            CloudAuthKind.OPENAI_API_KEY, CloudAuthKind.OPENAI_CODEX_OAUTH, CloudAuthKind.OPENAI_DEVICE_CODE -> Provider.OPENAI
            CloudAuthKind.OPENROUTER_API_KEY -> Provider.OPENROUTER
            null -> null
        }

        val detected = ProviderConfig.detectProvider(token, authHintProvider ?: preferredProvider)
        return when {
            detected != null -> listOf(detected)
            authHintProvider != null -> listOf(authHintProvider)
            else -> emptyList()  // Fail fast - don't guess and leak credentials
        }
    }

    private fun buildWalletBackend(config: ProviderConfig): ApiBackend {
        val chatConfig = config
        // Action model must meet the security floor (Sonnet-tier minimum) because
        // the action loop processes untrusted screen content. Always uses the
        // provider's default action model regardless of chat model selection.
        // Note: config.actionModelId from the wallet is intentionally ignored here.
        // lastWalletActionModelId tracking in updateModelsFromWallet() is
        // forward-compatible for when user-selectable action models are enabled.
        val actionModelId = ModelConfig.defaultActionModel(config.provider)
        val actionConfig = config.copy(chatModelId = actionModelId)

        val chatClient = createProviderClient(chatConfig)
        val actionClient = createProviderClient(actionConfig)

        return ApiBackend(
            provider = config.provider,
            chatClient = chatClient,
            actionClient = actionClient,
            agent = PhoneAgentApi(
                chatClient = chatClient,
                actionClient = actionClient,
                actionModelId = actionModelId,
                memoryProvider = memoryProvider,
                sensorProvider = sensorProvider,
                agentFileManager = agentFileManager,
                searchBaseUrl = searchBaseUrl,
                braveApiKey = braveApiKey,
                tinyFishApiKey = tinyFishApiKey,
                citrosAppToken = citrosAppToken
            )
        )
    }

    private fun buildApiBackend(provider: Provider, token: String): ApiBackend {
        val baseConfig = when (provider) {
            Provider.ANTHROPIC -> ProviderConfig.anthropic(token)
            Provider.OPENROUTER -> ProviderConfig.openRouter(token)
            Provider.OPENAI -> ProviderConfig.openAi(token)
        }

        return buildWalletBackend(baseConfig)
    }

    private fun activateApiBackend(index: Int) {
        val backend = apiBackends.getOrNull(index) ?: return
        activeApiBackendIndex = index
        cloudApiClient = backend.chatClient
        phoneAgentApi = backend.agent
        // Wire real-time progress updates for long-running tools (e.g., web_browse)
        backend.agent.onToolProgress = { status -> currentToolStatus.value = OutputClassifier.formatStatus(status) }
    }

    /**
     * Rebuild configured API backends so runtime-delivered keys take effect.
     *
     * This handles startup races where key delivery completes after wallet
     * configuration has already built PhoneAgentApi instances (#656).
     */
    private fun rebuildApiBackendsForRuntimeKeyUpdate() {
        if (mode != Mode.API) return
        if (apiBackends.isEmpty() || apiBackendConfigs.isEmpty()) return
        if (apiBackendConfigs.size != apiBackends.size) {
            Log.w(
                TAG,
                "Skipping runtime key rebind: backend/config count mismatch (${apiBackends.size}/${apiBackendConfigs.size})"
            )
            return
        }

        // Rebuild from stored provider configs; keys are now read from updated
        // ViewModel fields when PhoneAgentApi/WebSearchClient are constructed.
        val rebuiltBackends = apiBackendConfigs.map { buildWalletBackend(it) }
        apiBackends.clear()
        apiBackends.addAll(rebuiltBackends)

        activateApiBackend(activeApiBackendIndex.coerceIn(0, apiBackends.lastIndex))
    }

    @VisibleForTesting
    internal data class TestApiBackend(
        val provider: Provider,
        val chatClient: ProviderClient,
        val actionClient: ProviderClient,
        val agent: PhoneAgentApi
    )

    /**
     * Build a test backend without using reflection in unit tests.
     */
    @VisibleForTesting
    internal fun createTestBackend(
        provider: Provider,
        chatClient: ProviderClient,
        actionClient: ProviderClient = chatClient,
        memoryProvider: MemoryProvider? = null,
        agent: PhoneAgentApi = PhoneAgentApi(
            chatClient = chatClient,
            actionClient = actionClient,
            memoryProvider = memoryProvider
        ).also {
            // In tests, ScreenReader is not attached. Override to true so tool
            // mode works without a real accessibility service.
            it.phoneControlOverride = true
        }
    ): TestApiBackend = TestApiBackend(provider, chatClient, actionClient, agent)

    /**
     * Configure API mode with injected test backends.
     */
    @VisibleForTesting
    internal fun configureForTesting(backends: List<TestApiBackend>, startIndex: Int = 0) {
        apiBackends.clear()
        apiBackendConfigs.clear()
        apiBackends.addAll(
            backends.map { backend ->
                ApiBackend(
                    provider = backend.provider,
                    chatClient = backend.chatClient,
                    actionClient = backend.actionClient,
                    agent = backend.agent
                )
            }
        )

        activeApiBackendIndex = -1
        if (backends.isNotEmpty()) {
            activateApiBackend(startIndex.coerceIn(0, backends.lastIndex))
        }

        lastWalletProvider = null
        lastWalletChatModelId = null
        lastWalletActionModelId = null

        mode = Mode.API
        isConfigured.value = true
        needsAuth.value = false
    }

    /**
     * Configure local LLM mode with injected test agent.
     */
    @VisibleForTesting
    internal fun configureWithLocalLLMForTesting(agent: PhoneAgentLocal) {
        phoneAgentLocal = agent
        apiBackends.clear()
        activeApiBackendIndex = -1
        lastWalletProvider = null
        lastWalletChatModelId = null
        lastWalletActionModelId = null

        mode = Mode.LOCAL
        isConfigured.value = true
        needsAuth.value = false
    }

    private fun createProviderClient(config: ProviderConfig): ProviderClient {
        return when (config.provider) {
            Provider.ANTHROPIC -> AnthropicClient(
                config = config,
                systemPrompt = systemPrompt
            )
            Provider.OPENAI -> OpenAiClient(
                config = config,
                systemPrompt = systemPrompt
            )
            Provider.OPENROUTER -> OpenRouterClient(
                config = config,
                systemPrompt = systemPrompt
            )
        }
    }

    /**
     * Try to fail over to another API backend.
     * Simple linear walk: try the next backend, stop when list is exhausted.
     * No round-robin - won't retry a failed backend.
     *
     * @return True if failover succeeded (another backend was activated), false if no more backends available
     */
    private fun tryFailoverApiBackend(): Boolean {
        if (mode != Mode.API) return false
        val nextIndex = activeApiBackendIndex + 1
        if (nextIndex >= apiBackends.size) return false
        activateApiBackend(nextIndex)
        return true
    }
    
    /**
     * Update the accessibility service status.
     * 
     * @param enabled True if CitrosAccessibilityService is enabled, false otherwise
     */
    fun updateAccessibilityStatus(enabled: Boolean) {
        accessibilityEnabled.value = enabled
    }
    
    fun setSystemPrompt(prompt: String) {
        val normalizedPrompt = prompt.ifBlank { PhoneAgentPrompts.buildSystemPrompt() }
        if (systemPrompt == normalizedPrompt) return

        systemPrompt = normalizedPrompt

        if (mode != Mode.API || apiBackends.isEmpty() || apiBackendConfigs.size != apiBackends.size) {
            return
        }

        val activeIndex = activeApiBackendIndex.coerceAtLeast(0)
        val rebuilt = apiBackendConfigs.map { config -> buildWalletBackend(config) }
        apiBackends.clear()
        apiBackends.addAll(rebuilt)
        activateApiBackend(activeIndex.coerceAtMost(apiBackends.lastIndex))
    }

    /**
     * Set the on-device memory provider. If backends are already configured,
     * they are rebuilt so the agent can use remember/recall/list_memories tools.
     *
     * Synchronized on [apiBackends] to prevent concurrent modification during
     * backend rebuild (e.g. if called while [sendMessage] is running).
     */
    fun setMemoryProvider(provider: MemoryProvider) {
        synchronized(apiBackends) {
            if (memoryProvider === provider) return
            memoryProvider = provider

            // Rebuild backends so PhoneAgentApi picks up the provider
            if (mode != Mode.API || apiBackends.isEmpty() || apiBackendConfigs.size != apiBackends.size) {
                return
            }
            val activeIndex = activeApiBackendIndex.coerceAtLeast(0)
            val rebuilt = apiBackendConfigs.map { config -> buildWalletBackend(config) }
            apiBackends.clear()
            apiBackends.addAll(rebuilt)
            activateApiBackend(activeIndex.coerceAtMost(apiBackends.lastIndex))
        }
    }

    /**
     * Set the device sensor provider. If API backends are already configured,
     * rebuild them so [PhoneAgentApi] receives the new provider.
     */
    fun setSensorProvider(provider: SensorProvider?) {
        synchronized(apiBackends) {
            if (sensorProvider === provider) return
            sensorProvider = provider

            if (mode != Mode.API || apiBackends.isEmpty() || apiBackendConfigs.size != apiBackends.size) {
                return
            }
            val activeIndex = activeApiBackendIndex.coerceAtLeast(0)
            val rebuilt = apiBackendConfigs.map { config -> buildWalletBackend(config) }
            apiBackends.clear()
            apiBackends.addAll(rebuilt)
            activateApiBackend(activeIndex.coerceAtMost(apiBackends.lastIndex))
        }
    }

    /**
     * Request authentication/configuration from the user.
     * Sets needsAuth to true, triggering the auth UI.
     */
    fun requestAuth() {
        needsAuth.value = true
    }

    /**
     * Send a message to the LLM and handle the response.
     * Processes both chat and action responses, with automatic failover for API backends.
     *
     * For API mode, delegates the tool execution loop to [AgentExecutor].
     * For local LLM mode, runs a legacy loop (local doesn't support continueAfterTools).
     * 
     * @param content The user message text to send
     */
    fun sendMessage(content: String) {
        // UI shows the original user text (not the effective content with prepended context).
        // The agent may receive a different string via effectiveContent (#447) — this is
        // intentional: "Original request: ..." prefix is internal context, not user-visible.
        messages.add(Message(role = "user", content = content))
        lastUserMessage = content
        lastActivityTimestamp = System.currentTimeMillis()
        toolLoopCancelled.set(false)
        steerQueue.clear()
        hasQueuedSteer.value = false
        error.value = null
        isLoading.value = true

        // Prepend context if last tool loop exited abnormally (#603)
        // Consumed on main thread before coroutine launch; written from main-dispatched coroutine. No race.
        val exitHint = consumeExitReasonHint()

        viewModelScope.launch {
            var toolSteps = 0
            try {
                var screenContent = try {
                    if (ScreenReader.isAttached()) ScreenReader.getScreenContent() else null
                } catch (_: Exception) { null }

                // Seed API conversation history from UI messages if agent's
                // in-memory history was lost (process recreation) (#612).
                phoneAgentApi?.seedConversationHistory(messages.dropLast(1))

                Log.d(TAG, "sendMessage: mode=$mode, content='${content.take(60)}', screenAttached=${ScreenReader.isAttached()}, package=${screenContent?.packageName}")

                // #447: Stash/replay task context across accessibility enable prompt.
                // When accessibility is OFF, stash the user's task message.
                // When accessibility is ON and a pending task exists, prepend it as context.
                val accessibilityAvailable = ScreenReader.isAttached()
                setPendingTaskIfAccessibilityUnavailable(content, accessibilityAvailable)
                val baseContent = if (exitHint != null) "$exitHint\n\n$content" else content
                val effectiveContent = if (accessibilityAvailable) {
                    consumePendingTaskContext(baseContent)
                } else {
                    baseContent
                }

                // Initial user message uses chat model (Sonnet)
                // Pre-add an empty assistant message for streaming updates.
                // If the response is conversational (no tool calls), the message
                // gets populated token-by-token via the onTextDelta callback.
                // If tool calls are returned, we remove this placeholder.
                val streamingMsgIndex = messages.size
                messages.add(Message(role = "assistant", content = ""))
                val streamingText = StringBuilder()

                val mainHandler = android.os.Handler(android.os.Looper.getMainLooper())
                val onDelta: (String) -> Unit = { delta ->
                    streamingText.append(delta)
                    // Post to main thread — streaming callback runs on IO dispatcher
                    // but Compose state writes must happen on the main thread.
                    mainHandler.post {
                        messages[streamingMsgIndex] = Message(
                            role = "assistant",
                            content = streamingText.toString()
                        )
                        // Runs on Dispatchers.Main via mainHandler.post — safe for
                        // Compose mutableIntStateOf writes without additional wrapping.
                        streamingContentVersion.intValue++
                    }
                }

                var response = sendMessageWithFallback(effectiveContent, screenContent, isActionLoop = false, onDelta)
                Log.d(TAG, "initialResponse: stopReason=${response?.stopReason}, toolCalls=${response?.toolCalls?.map { it.name }}, text=${response?.text?.take(80)}")

                if (response == null) {
                    messages.removeAt(streamingMsgIndex)
                    messages.add(Message(role = "assistant", content = "Not configured"))
                    isLoading.value = false
                    streamingContentVersion.intValue = 0
                    return@launch
                }

                // If no tool calls, display text and we're done
                if (response.toolCalls.isEmpty()) {
                    val text = response.text
                    if (text != null) {
                        // Update the streaming placeholder with the final text.
                        // If streaming was active, this may be identical to what's
                        // already there (idempotent). If streaming wasn't used
                        // (e.g. local mode), this populates the placeholder.
                        messages[streamingMsgIndex] = Message(
                            role = "assistant",
                            content = text
                        )
                        speakIfEnabled(text)
                    } else {
                        // No text at all — remove the empty placeholder
                        messages.removeAt(streamingMsgIndex)
                    }
                    return@launch
                }

                // Tool calls returned — remove the streaming placeholder.
                // The tool loop handles its own message display.
                messages.removeAt(streamingMsgIndex)

                // Hide overlay during tool loop so it doesn't block gestures on
                // the target app (e.g. FABs covered by the bubble) (#457).
                try {
                    ScreenReader.toolLoopOverlayHideHook?.invoke()
                } catch (e: kotlin.coroutines.cancellation.CancellationException) {
                    throw e
                } catch (e: Exception) {
                    Log.w(TAG, "toolLoop: overlay hide hook failed: ${e.message}")
                }

                // Delegate the tool execution loop to AgentExecutor.
                // ChatViewModel implements ToolExecutionDelegate and LoopProgressListener.
                val agent = phoneAgentApi
                if (agent == null) {
                    // Local mode fallback — run legacy loop
                    toolSteps = runLocalModeLoop(response, screenContent)
                } else {
                    val executor = AgentExecutor(
                        delegate = this@ChatViewModel,
                        progressListener = this@ChatViewModel,
                        maxToolSteps = MAX_TOOL_STEPS,
                        steerMessageSource = { drainSteerQueue() },
                        interruptionSource = { InterruptionDetector.drain() }
                    )
                    InterruptionDetector.startMonitoring(screenContent?.packageName)
                    val result = executor.run(
                        initialResponse = response,
                        initialScreenContent = screenContent,
                        isCancelled = { toolLoopCancelled.get() },
                        continueAfterTools = { agent.continueAfterTools() }
                    )

                    toolSteps = when (result) {
                        is LoopResult.Completed -> {
                            val finalText = result.text
                            if (finalText != null) {
                                messages.add(Message(role = "assistant", content = finalText))
                                speakIfEnabled(finalText)
                            }

                            // Determine exit type and handle post-loop behavior (#603)
                            val exit = when {
                                toolLoopCancelled.get() -> ToolLoopExit.CANCELLED
                                result.exitReason == "max_steps" -> ToolLoopExit.MAX_STEPS
                                result.exitReason == "accessibility_lost" -> ToolLoopExit.ACCESSIBILITY_LOST
                                else -> ToolLoopExit.END_TURN
                            }
                            // Final API call for exits that need model explanation
                            exit.systemPrompt?.let { prompt ->
                                requestFinalExplanation(prompt)
                            }
                            // Reactive flag for cancelled — hint prepended on next user message
                            if (exit == ToolLoopExit.CANCELLED) {
                                lastExitReason = ToolLoopExit.CANCELLED
                            }

                            result.steps
                        }
                        is LoopResult.Error -> {
                            messages.add(Message(role = "assistant", content = "💥 Error: ${result.message}"))
                            result.steps
                        }
                    }
                }
            } catch (e: kotlin.coroutines.cancellation.CancellationException) {
                throw e
            } catch (e: Exception) {
                messages.add(Message(role = "assistant", content = "💥 Crashed: ${e.message?.take(120)}"))
            } finally {
                // Stop interruption monitoring
                InterruptionDetector.stopMonitoring()
                // Restore overlay visibility after tool loop ends (#457)
                try {
                    ScreenReader.toolLoopOverlayRestoreHook?.invoke()
                } catch (e: Exception) {
                    Log.w(TAG, "toolLoop: overlay restore hook failed: ${e.message}")
                }
                isLoading.value = false
                streamingContentVersion.intValue = 0
                currentToolStatus.value = null
                // Increment unread once per completed execution run (not per tool result)
                if (toolSteps > 0) {
                    unreadCount.intValue += 1
                }
                // Clear pending task context on successful completion (#447).
                // If the tool loop ran to completion, the task was processed —
                // no need to replay the original request on the next message.
                if (!toolLoopCancelled.get()) {
                    pendingTaskMessage = null
                }
                // Dispatch queued message only if loop was not cancelled.
                // Clear before sending to prevent duplicate dispatch (#561):
                // if sendMessage re-enters before the clear, the same message
                // could be dispatched twice.
                if (!toolLoopCancelled.get()) {
                    val pending = queuedMessage.value?.takeIf { it.isNotBlank() }
                    queuedMessage.value = null
                    if (pending != null) {
                        sendMessage(pending)
                    }
                }
            }
        }
    }

    /**
     * Legacy loop for local LLM mode which doesn't support continueAfterTools().
     * Returns the number of tool steps executed.
     */
    private suspend fun runLocalModeLoop(
        initialResponse: ChatResponse,
        initialScreenContent: ScreenContent?
    ): Int {
        var response: ChatResponse? = initialResponse
        var screenContent = initialScreenContent
        var toolSteps = 0
        val stuckState = StuckDetectionState()

        while (response != null && response.toolCalls.isNotEmpty() && toolSteps < MAX_TOOL_STEPS) {
            if (toolLoopCancelled.get()) break
            toolSteps++
            for (toolCall in response.toolCalls) {
                if (toolLoopCancelled.get()) break
                val actionResult = try {
                    phoneAgentLocal?.executeToolCall(toolCall, screenContent)
                        ?: ToolResult("Not configured", isError = true)
                } catch (e: Exception) {
                    ToolResult("Error: ${e.message?.take(100)}", isError = true)
                }
                val visibility = OutputClassifier.classify(toolCall.name, actionResult.text, actionResult.isError)
                val effectiveVisibility = OutputClassifier.applyVerbosity(visibility, outputVerbosity)
                val displayText = OutputClassifier.formatForDisplay(toolCall.name, actionResult.text, effectiveVisibility)
                if (displayText != null) {
                    messages.add(Message(role = "assistant", content = displayText))
                }
                delay(DELAY_DEFAULT)
                if (toolCall.name in PhoneAgentApi.UI_MUTATING_TOOLS) {
                    screenContent = try {
                        if (ScreenReader.isAttached()) ScreenReader.getScreenContent() else null
                    } catch (_: Exception) { null }
                }
                @Suppress("DEPRECATION") val stuckWarning = checkStuck(stuckState, toolCall.name, screenContent) // TODO: migrate local mode to AgentExecutor + core StuckDetector
                val finalResult = if (stuckWarning != null) actionResult.text + stuckWarning else actionResult.text
                phoneAgentLocal?.addToolResult(toolCall.id, finalResult)
            }
            if (toolSteps >= MAX_TOOL_STEPS || toolLoopCancelled.get()) break
            response = try {
                sendMessageWithFallback("continue", screenContent, isActionLoop = true)
            } catch (e: Exception) {
                ChatResponse(text = "Error: ${e.message?.take(80)}", toolCalls = emptyList(), stopReason = "error")
            }
        }

        val finalText = response?.text
        if (finalText != null) {
            messages.add(Message(role = "assistant", content = finalText))
            speakIfEnabled(finalText)
        }

        // Post-loop exit handling for local mode (#603)
        val localExit = when {
            toolLoopCancelled.get() -> ToolLoopExit.CANCELLED
            // If the model produced final text on the last step, it wrapped up naturally —
            // treat as END_TURN (no need for requestFinalExplanation). Asymmetric with
            // API mode where AgentExecutor reports max_steps regardless of final text,
            // but intentional: local mode can detect natural completion.
            toolSteps >= MAX_TOOL_STEPS && finalText == null -> ToolLoopExit.MAX_STEPS
            else -> ToolLoopExit.END_TURN
        }
        localExit.systemPrompt?.let { prompt ->
            requestFinalExplanation(prompt)
        }
        if (localExit == ToolLoopExit.CANCELLED) {
            lastExitReason = ToolLoopExit.CANCELLED
        }

        return toolSteps
    }

    // ========== ToolExecutionDelegate implementation ==========

    override suspend fun executeToolCall(toolCall: ToolCall, screenContent: ScreenContent?): ToolResult {
        val isUiMutating = isUiMutatingTool(toolCall.name)
        if (isUiMutating) InterruptionDetector.markAgentAction()
        try {
            return phoneAgentApi?.executeToolCall(toolCall, screenContent)
                ?: phoneAgentLocal?.executeToolCall(toolCall, screenContent)
                ?: ToolResult("Not configured", isError = true)
        } finally {
            if (isUiMutating) InterruptionDetector.clearAgentAction()
        }
    }

    override suspend fun refreshScreen(): ScreenContent? {
        return try {
            if (ScreenReader.isAttached()) ScreenReader.getScreenContent() else null
        } catch (_: Exception) { null }
    }

    override suspend fun refreshScreenAfterTool(toolName: String, actionResult: String): ScreenContent? {
        val usesSmartPoll = toolName == "open_app" || toolName == "press_home"
        val screen = if (usesSmartPoll && !actionResult.startsWith("Failed")) {
            val selfPackage = BuildConfig.APPLICATION_ID
            pollForPackageChangeImpl(selfPackage)
        } else {
            refreshScreen()
        }
        // Update expected package so InterruptionDetector knows where the agent is
        screen?.packageName?.let { InterruptionDetector.setExpectedPackage(it) }
        return screen
    }

    override suspend fun settleDelay(toolName: String, actionResult: String) {
        val usesSmartPoll = toolName == "open_app" || toolName == "press_home"
        if (usesSmartPoll) return  // Smart poll tools don't use fixed delays
        val waitMs = when (toolName) {
            "think" -> 0L
            "wait" -> 0L  // wait tool already sleeps internally
            "tap", "tap_text", "long_press" -> DELAY_AFTER_TAP
            else -> DELAY_DEFAULT
        }
        if (waitMs > 0) delay(waitMs)
    }

    override fun formatToolResult(actionSummary: String, screenContent: ScreenContent?): String {
        return phoneAgentApi?.formatToolResult(actionSummary, screenContent) ?: actionSummary
    }

    override fun isUiMutatingTool(toolName: String): Boolean {
        return toolName in PhoneAgentApi.UI_MUTATING_TOOLS
    }

    override fun isScreenReaderAvailable(): Boolean =
        screenReaderAvailableOverride ?: phoneAgentApi?.phoneControlOverride ?: ScreenReader.isAttached()

    override suspend fun waitForAccessibility(timeoutMs: Long): Boolean {
        return if (screenReaderAvailableOverride != null) {
            false // Test override: don't actually wait
        } else {
            ScreenReader.waitForAttachment(timeoutMs = timeoutMs)
        }
    }

    override fun accessibilityWaitMs(): Long = accessibilityWaitMs

    override fun outputVerbosity(): OutputVerbosity = outputVerbosity

    override fun addToolResult(toolCallId: String, result: String, toolName: String?, isError: Boolean) {
        phoneAgentApi?.addToolResult(toolCallId, result, toolName, isError)
        phoneAgentLocal?.addToolResult(toolCallId, result, toolName, isError)
    }

    override fun addSteerMessage(text: String) {
        // Add steer as a first-class user message in conversation history
        phoneAgentApi?.addSteerMessage(text)
        phoneAgentLocal?.addSteerMessage(text)
    }

    override fun onStepStarted(step: Int, maxSteps: Int) {
        phoneAgentApi?.let { it.currentToolStep = step }
    }

    // ========== LoopProgressListener implementation ==========

    override fun onToolStarted(toolName: String, toolIndex: Int, batchSize: Int) {
        currentToolStatus.value = toolStatusLabel(toolName)
    }

    override fun onToolResult(toolName: String, result: String, visibility: OutputVisibility, isError: Boolean) {
        // Clear tool status when result arrives, unless a persistent error is showing.
        // Persistent warnings (⚠️ prefix) stick until a subsequent success or explicit dismissal.
        val currentStatus = currentToolStatus.value
        if (currentStatus == null || !currentStatus.startsWith("⚠️")) {
            currentToolStatus.value = null
        } else if (!isError) {
            // Successful result clears even persistent status
            currentToolStatus.value = null
        }
        lastActivityTimestamp = System.currentTimeMillis()
        val displayText = OutputClassifier.formatForDisplay(toolName, result, visibility)
        if (displayText != null) {
            messages.add(Message(role = "assistant", content = displayText))
        }
    }

    /** Job for auto-clearing transient error status. Cancelled when a new status is set. */
    private var transientClearJob: kotlinx.coroutines.Job? = null

    override fun onToolError(toolName: String, errorText: String, severity: ErrorSeverity) {
        when (severity) {
            ErrorSeverity.TRANSIENT -> {
                // Cancel any previous transient clear to avoid clearing this one
                transientClearJob?.cancel()
                // Brief "Retrying..." status that auto-clears
                currentToolStatus.value = "Retrying..."
                // Auto-clear after 2 seconds if still showing "Retrying..."
                transientClearJob = viewModelScope.launch {
                    delay(2000)
                    if (currentToolStatus.value == "Retrying...") {
                        currentToolStatus.value = null
                    }
                }
            }
            ErrorSeverity.PERSISTENT -> {
                // Sticky warning status
                currentToolStatus.value = "⚠️ ${OutputClassifier.formatStatus(errorText)}"
            }
            ErrorSeverity.EXPLORATORY,
            ErrorSeverity.INFORMATIONAL -> {
                // No status bar update for these
            }
        }
    }

    override fun onAccessibilityLost() {
        messages.add(Message(
            role = "assistant",
            content = "⚠️ Phone control disconnected. Please re-enable it in Settings → Accessibility."
        ))
        toolLoopCancelled.set(true)
    }

    /** Request cancellation of the active tool execution loop. Thread-safe. */
    fun cancelToolExecution() {
        Log.d(TAG, "cancelToolExecution: setting toolLoopCancelled=true (was=${toolLoopCancelled.get()}, isLoading=${isLoading.value})")
        toolLoopCancelled.set(true)
    }

    /**
     * Resume execution after a cancellation.
     * Uses the queued message if one is pending, otherwise re-sends the last user message.
     * No-op if a tool loop is already running.
     */
    fun resumeExecution() {
        if (isLoading.value) return
        val queued = queuedMessage.value?.takeIf { it.isNotBlank() }
        val content = queued ?: lastUserMessage ?: return
        queuedMessage.value = null
        sendMessage(content)
    }

    fun setQueuedMessage(text: String) {
        queuedMessage.value = text.takeIf { it.isNotBlank() }
    }

    /**
     * Stash the user's task message when accessibility is unavailable (#447).
     * Called from [sendMessage] when the no-accessibility chat-mode path fires.
     * The stashed message is replayed as context on the next [consumePendingTaskContext].
     */
    fun setPendingTaskIfAccessibilityUnavailable(message: String, accessibilityAvailable: Boolean) {
        if (!accessibilityAvailable) {
            pendingTaskMessage = message
        }
    }

    /**
     * If a pending task message was stashed (from accessibility-unavailable path),
     * prepend it as context to the new user message and clear the stash (#447).
     * Returns the original message unchanged if no pending task exists.
     * Always clears the stash on read to prevent accidental re-use.
     */
    fun consumePendingTaskContext(newMessage: String): String {
        val pending = pendingTaskMessage
        pendingTaskMessage = null // Clear regardless — one-shot replay
        return if (pending != null) {
            "Original request: $pending\nUser follow-up: $newMessage"
        } else {
            newMessage
        }
    }

    /**
     * Drain all pending steer messages from [steerQueue] atomically.
     * Called by the [AgentExecutor] at each tool boundary via [steerMessageSource].
     * Resets [hasQueuedSteer] synchronously to avoid race conditions.
     */
    @VisibleForTesting
    internal fun drainSteerQueue(): List<String> {
        val drained = mutableListOf<String>()
        while (true) {
            drained.add(steerQueue.poll() ?: break)
        }
        if (drained.isNotEmpty()) {
            hasQueuedSteer.value = false
        }
        return drained
    }

    /**
     * Why the tool loop ended. Determines post-loop behavior (#603).
     */
    @VisibleForTesting
    internal enum class ToolLoopExit(val systemPrompt: String?) {
        /** Model chose end_turn. Already has final text. No extra action. */
        END_TURN(null),
        /** User cancelled. Handled reactively via [lastExitReason] flag. */
        CANCELLED(null),
        /** Hit step limit. Model should summarize partial progress. */
        MAX_STEPS(
            "[System: You hit the step limit and couldn't finish the task. " +
                "Summarize what you accomplished so far and ask the user how they'd like to proceed.]"
        ),
        /** Lost accessibility service. Model should explain. */
        ACCESSIBILITY_LOST(
            "[System: You lost connection to the phone's accessibility service during the task. " +
                "Let the user know what happened and ask if they'd like to retry or do something else.]"
        )
    }

    /**
     * Give the model one final API call to explain an abnormal exit in its own voice (#603).
     * Replaces hardcoded "Hit step limit" strings with natural model responses.
     */
    private suspend fun requestFinalExplanation(systemPrompt: String) {
        try {
            // Use sendEphemeral for API mode to avoid polluting conversation history
            // with the system prompt. Only the assistant's reply is persisted.
            val text = when (mode) {
                Mode.API -> phoneAgentApi?.sendEphemeral(systemPrompt)
                Mode.LOCAL -> {
                    val response = sendMessageWithAgent(
                        message = systemPrompt,
                        screenContent = null,
                        isActionLoop = false
                    )
                    response?.text
                }
            }
            text?.let {
                messages.add(Message(role = "assistant", content = it))
                speakIfEnabled(it)
            }
        } catch (e: Exception) {
            Log.w(TAG, "requestFinalExplanation failed: ${e.message}")
        }
    }

    /**
     * Consume the [lastExitReason] flag and return a hint to prepend, or null (#603).
     * Called once per [sendMessage]; the flag is cleared after consumption.
     */
    @VisibleForTesting
    internal fun consumeExitReasonHint(): String? {
        val reason = lastExitReason ?: return null
        lastExitReason = null
        return when (reason) {
            ToolLoopExit.CANCELLED -> "Note: The previous task was stopped by the user. Resume carefully or ask for clarification."
            else -> null
        }
    }

    /**
     * Queue a steer message for mid-loop injection.
     * If the tool loop is not running, falls back to a normal [sendMessage].
     *
     * During a tool loop, the message is added to [steerQueue] and will be
     * picked up by [AgentExecutor] at the next tool boundary. The message
     * also appears immediately in the UI message list with [Message.isSteer] = true.
     */
    fun steerMessage(text: String) {
        if (!isLoading.value) {
            sendMessage(text)
            return
        }
        steerQueue.offer(text)
        hasQueuedSteer.value = true
        messages.add(Message(role = "user", content = text, isSteer = true))
    }

    fun resetUnreadCount() {
        unreadCount.intValue = 0
    }

    @VisibleForTesting
    internal fun isToolLoopCancelledForTesting(): Boolean = toolLoopCancelled.get()

    private suspend fun sendMessageWithAgent(
        message: String,
        screenContent: ScreenContent?,
        isActionLoop: Boolean = false,
        onTextDelta: ((String) -> Unit)? = null
    ): ChatResponse? {
        Log.d(TAG, "sendMessageWithAgent: mode=$mode, isActionLoop=$isActionLoop, screen=${screenContent?.packageName}")
        return when (mode) {
            Mode.LOCAL -> {
                // Local agent still uses old process() method for now
                val agentResponse = phoneAgentLocal?.process(message, screenContent)
                // Convert AgentResponse to ChatResponse
                agentResponse?.let {
                    val action = it.action  // Capture in local variable for smart cast
                    val toolCalls = if (action != null) {
                        // Convert PhoneAction to a ToolCall for uniform handling
                        listOf(phoneActionToToolCall(action))
                    } else {
                        emptyList()
                    }
                    ChatResponse(
                        text = it.text,
                        toolCalls = toolCalls,
                        stopReason = if (toolCalls.isEmpty()) "end_turn" else "tool_use"
                    )
                }
            }
            Mode.API -> phoneAgentApi?.sendMessage(message, screenContent, isActionLoop, onTextDelta)
        }
    }

    private suspend fun sendMessageWithFallback(
        message: String,
        screenContent: ScreenContent?,
        isActionLoop: Boolean = false,
        onTextDelta: ((String) -> Unit)? = null
    ): ChatResponse? {
        while (true) {
            try {
                return sendMessageWithAgent(message, screenContent, isActionLoop, onTextDelta)
            } catch (e: ProviderException) {
                if (e.isAuthFailure && tryFailoverApiBackend()) {
                    val providerName = apiBackends.getOrNull(activeApiBackendIndex)?.provider?.name ?: "fallback"
                    messages.add(Message(role = "assistant", content = "🔁 Authentication failed, switching to $providerName and retrying..."))
                    continue
                }
                return ChatResponse(text = "Error: ${e.message?.take(120)}", toolCalls = emptyList(), stopReason = "error")
            } catch (e: Exception) {
                return ChatResponse(text = "Error: ${e.message?.take(120)}", toolCalls = emptyList(), stopReason = "error")
            }
        }
    }

    private fun phoneActionToToolCall(action: PhoneAction): ToolCall {
        return when (action) {
            is PhoneAction.Click -> ToolCall("local_${System.nanoTime()}", "tap", mapOf("element_id" to action.elementId))
            is PhoneAction.ClickText -> ToolCall("local_${System.nanoTime()}", "tap_text", mapOf("text" to action.text))
            is PhoneAction.Type -> ToolCall("local_${System.nanoTime()}", "type_text", mapOf("text" to action.text))
            is PhoneAction.Swipe -> ToolCall("local_${System.nanoTime()}", "swipe", mapOf("direction" to action.direction))
            PhoneAction.Back -> ToolCall("local_${System.nanoTime()}", "press_back", emptyMap())
            PhoneAction.Home -> ToolCall("local_${System.nanoTime()}", "press_home", emptyMap())
            is PhoneAction.OpenApp -> ToolCall("local_${System.nanoTime()}", "open_app", mapOf("app_name" to action.name))
            PhoneAction.OpenNotifications -> ToolCall("local_${System.nanoTime()}", "open_notifications", emptyMap())
        }
    }

    /**
     * Clear any displayed error message.
     */
    fun clearError() {
        error.value = null
    }

    /**
     * Check if conversation should be auto-cleared due to idle timeout or daily reset.
     * Called from [ChatActivity.onResume].
     *
     * @param timeoutMs configured idle timeout (ms), or [ConversationLifecycle.TIMEOUT_NEVER]
     * @param lastConversationDate stored date string of last conversation ("YYYY-MM-DD"), or null
     * @param nowMs current time in epoch ms (injectable for testing)
     * @return a [LifecycleClearReason] if cleared, or null if no action taken
     */
    fun checkLifecycleAndClear(
        timeoutMs: Long,
        lastConversationDate: String?,
        nowMs: Long = System.currentTimeMillis()
    ): LifecycleClearReason? {
        if (messages.isEmpty()) return null

        val today = ConversationLifecycle.todayDateString(nowMs)

        // Daily reset takes priority (new day = fresh start)
        if (ConversationLifecycle.shouldClearForNewDay(lastConversationDate, today)) {
            clearConversation()
            messages.add(Message(role = "assistant", content = "New day, fresh start \uD83C\uDF05"))
            return LifecycleClearReason.DAILY_RESET
        }

        // Idle timeout
        if (ConversationLifecycle.shouldClearForIdleTimeout(lastActivityTimestamp, nowMs, timeoutMs)) {
            clearConversation()
            messages.add(Message(role = "assistant", content = "Session cleared after inactivity"))
            return LifecycleClearReason.IDLE_TIMEOUT
        }

        return null
    }

    /**
     * Clear the conversation history for all agents.
     * Removes all messages from the UI and clears agent conversation state.
     */
    fun clearConversation() {
        messages.clear()
        queuedMessage.value = null
        unreadCount.intValue = 0
        streamingContentVersion.intValue = 0
        toolLoopCancelled.set(false)
        steerQueue.clear()
        hasQueuedSteer.value = false
        isLoading.value = false
        currentToolStatus.value = null
        error.value = null
        lastUserMessage = null
        pendingTaskMessage = null
        lastExitReason = null
        lastActivityTimestamp = 0L
        apiBackends.forEach { it.agent.clearConversation() }
        phoneAgentLocal?.clearConversation()
        // Overlay state is synced by ChatActivity's LaunchedEffect when messages.size
        // changes. No direct OverlayController mutation here (#463: unidirectional flow).
    }

    /**
     * Sign out and clear all authentication state.
     * Clears conversation history, resets all clients, and marks as not configured.
     */
    fun signOut() {
        apiBackends.forEach { it.agent.clearConversation() }
        cloudApiClient = null
        localLLMClient = null
        phoneAgentApi = null
        phoneAgentLocal = null
        apiBackends.clear()
        apiBackendConfigs.clear()
        activeApiBackendIndex = -1
        lastWalletProvider = null
        lastWalletChatModelId = null
        lastWalletActionModelId = null
        isConfigured.value = false
        needsAuth.value = false
        isLoading.value = false
        currentToolStatus.value = null
        error.value = null
        queuedMessage.value = null
        unreadCount.intValue = 0
        toolLoopCancelled.set(false)
        steerQueue.clear()
        hasQueuedSteer.value = false
        lastUserMessage = null
        messages.clear()
    }
}
