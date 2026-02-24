package ai.citros.core

/**
 * Owns the tool execution loop lifecycle: step counting, cancellation,
 * stuck detection, steer message injection, and structured result reporting.
 *
 * Lives in :core — no Android UI, no ScreenReader, no ViewModel references.
 * All side effects are delegated through [ToolExecutionDelegate] and
 * [LoopProgressListener].
 *
 * Loop exit conditions and mid-loop behaviors are formalized as [BoundaryCheck]s
 * evaluated after each tool call. The default checks handle cancellation, step
 * limits, stuck detection, and user steer message injection.
 *
 * **Steer** enables mid-loop message injection: when the user sends a message
 * during a tool loop, it is delivered at the next tool boundary as a first-class
 * user message. Two steer checkpoints exist:
 * 1. **Pre-batch** — after API returns but before any tool executes (zero wasted actions)
 * 2. **Post-tool** — after each tool executes (one wasted action max)
 *
 * **Accessibility gating** is handled via [AccessibilityGateCheck] when included
 * in the boundary checks list. Use [defaultBoundaryChecksWithAccessibility] for
 * the full set including accessibility gating.
 *
 * See docs/agentic-loop-v2.md §3.2 and docs/specs/citros-architecture-roadmap.md §1.2
 */
class AgentExecutor(
    private val delegate: ToolExecutionDelegate,
    private val progressListener: LoopProgressListener,
    private val boundaryChecks: List<BoundaryCheck> = defaultBoundaryChecks(),
    private val maxToolSteps: Int = DEFAULT_MAX_TOOL_STEPS,
    /**
     * Lambda that atomically drains pending user steer messages at each boundary.
     *
     * Called at two checkpoints per step:
     * 1. Pre-batch — after API returns, before any tool executes
     * 2. Post-tool — after each tool in the batch, via [SteerCheck] in [LoopState]
     *
     * The lambda should return all pending messages and clear its internal buffer
     * (drain semantics). Defaults to `{ emptyList() }` for executors without steer.
     */
    private val steerMessageSource: () -> List<String> = { emptyList() },
    /**
     * Lambda that returns the next user interruption event, or null if none pending.
     *
     * Used by [UserInterruptionCheck] to detect app switches, user touches, and
     * external interrupts. The lambda should drain the event atomically (return
     * once, then null on subsequent calls).
     *
     * Also used to populate [LoopState.pendingInterruption] at each boundary.
     *
     * Defaults to `{ null }` for executors without interruption detection.
     */
    private val interruptionSource: () -> InterruptionEvent? = { null },
    /**
     * Optional hook that runs before each LLM call inside the tool loop.
     *
     * This is the injection point for context management: trimming old messages,
     * summarizing tool results, injecting external context, or pruning the
     * conversation to fit within token limits.
     *
     * Called before every `continueAfterTools()` invocation (both post-steer and
     * post-tool-batch). The hook operates on the conversation through the
     * [ToolExecutionDelegate] — callers modify the message list in-place.
     * Typical implementations will call delegate methods to modify the message
     * list, such as removing old messages or summarizing tool results. The
     * delegate interface must provide appropriate mutation methods for the hook
     * to be useful (e.g., for H2 context trimming).
     *
     * If the hook throws an exception, the loop will fail and the exception
     * will propagate to the caller. Implementations should handle errors
     * internally if they need graceful degradation.
     *
     * Defaults to `null` (no transformation). Required for H2 context trimming.
     */
    private val transformContext: (suspend () -> Unit)? = null,
    /** Deterministic recovery strategy manager (advisory guidance injection). */
    private val recoveryManager: RecoveryManager = RecoveryManager(),
    /**
     * Optional hook invoked after each tool boundary with checkpoint metadata.
     * Used by service architecture to persist durable recovery state.
     */
    private val checkpointCallback: (suspend (LoopCheckpoint) -> Unit)? = null
) {
    /**
     * Per-tool consecutive failure counters for error severity escalation only.
     *
     * Distinct from `consecutiveFailures` in [run], which is a single global streak used by
     * recovery detection across sequential tool calls (including calls in one model response).
     */
    private val failureCounts = mutableMapOf<String, Int>()

    companion object {
        /** Maximum number of tool execution steps before forcing loop exit. */
        const val DEFAULT_MAX_TOOL_STEPS = 25

        /** Maximum length for error messages in tool results and loop results. */
        const val ERROR_MESSAGE_MAX_LENGTH = 100

        /**
         * Default boundary checks (without accessibility gating).
         *
         * Callers with accessibility should use [defaultBoundaryChecksWithAccessibility]
         * for the full set, or prepend [AccessibilityGateCheck] manually:
         * ```
         * listOf(CancellationCheck(), AccessibilityGateCheck(...)) + defaultBoundaryChecks()
         * ```
         *
         * Evaluation order:
         * 1. [CancellationCheck] — highest priority, exits immediately
         * 2. [StepLimitCheck] — hard ceiling on loop iterations
         * 3. [StuckDetectionCheck] — injects warning when screen is unchanged
         * 4. [SteerCheck] — injects user messages (last: stop should short-circuit first)
         */
        fun defaultBoundaryChecks(): List<BoundaryCheck> = listOf(
            CancellationCheck(),
            StepLimitCheck(),
            StuckDetectionCheck.withDefaults(),
            ActionVerificationCheck(),
            UserInterruptionCheck(),
            SteerCheck()
        )

        /**
         * Default boundary checks including accessibility gating.
         *
         * Check order:
         * 1. [CancellationCheck] — user cancel always wins
         * 2. [AccessibilityGateCheck] — gate on service availability
         * 3. [StepLimitCheck] — hard ceiling
         * 4. [StuckDetectionCheck] — injects warning when screen is unchanged
         * 5. [SteerCheck] — injects user messages (last: stop should short-circuit first)
         */
        fun defaultBoundaryChecksWithAccessibility(
            isAvailable: () -> Boolean,
            waitForReconnect: suspend (Long) -> Boolean,
            onReconnected: suspend () -> Unit,
            onLost: () -> Unit,
            baseTimeoutMs: Long = AccessibilityGateCheck.DEFAULT_BASE_TIMEOUT_MS,
            maxRetries: Int = AccessibilityGateCheck.DEFAULT_MAX_RETRIES
        ): List<BoundaryCheck> = listOf(
            CancellationCheck(),
            AccessibilityGateCheck(isAvailable, waitForReconnect, onReconnected, onLost, baseTimeoutMs, maxRetries),
            StepLimitCheck(),
            StuckDetectionCheck.withDefaults(),
            ActionVerificationCheck(),
            UserInterruptionCheck(),
            SteerCheck()
        )
    }

    /**
     * Run the tool execution loop starting from an initial [ChatResponse].
     *
     * The loop processes tool calls from the model, executes them via [delegate],
     * reports progress via [progressListener], evaluates [boundaryChecks] after
     * each tool call, and continues until the model stops requesting tools or
     * a boundary check returns [CheckResult.Stop].
     *
     * @param initialResponse The first response from the model (may contain tool calls)
     * @param initialScreenContent Current screen state at loop start
     * @param isCancelled Lambda checked at boundary evaluations and between steps
     * @param continueAfterTools Lambda to get the next model response after tool results
     * @return Structured [LoopResult] describing what happened
     */
    suspend fun run(
        initialResponse: ChatResponse,
        initialScreenContent: ScreenContent?,
        isCancelled: () -> Boolean,
        continueAfterTools: suspend () -> ChatResponse
    ): LoopResult {
        failureCounts.clear()
        var response: ChatResponse? = initialResponse
        var screenContent = initialScreenContent
        var toolSteps = 0
        // Recovery-level streak across tool calls in this run (global, not per tool name).
        // Distinct from `failureCounts`, which tracks per-tool retries for error severity escalation.
        var consecutiveFailures = 0

        // If no tool calls in initial response, return immediately
        if (response == null || response.toolCalls.isEmpty()) {
            return LoopResult.Completed(
                text = response?.text,
                steps = 0,
                exitReason = "no_tools"
            )
        }

        while (response != null && response.toolCalls.isNotEmpty()) {
            // ======= PRE-BATCH STEER CHECK =======
            // Catches steers that arrived DURING the API call (model thinking).
            // This is the thinking→acting boundary — model decided what to do,
            // but nothing has executed yet. Steer here = zero wasted actions.
            val earlySteer = steerMessageSource()
            if (earlySteer.isNotEmpty()) {
                // Always deliver steer messages to conversation history, even if
                // the user cancels immediately after. Users expect sent messages
                // to appear in the chat — dropping them silently creates confusion.
                for (msg in earlySteer) delegate.addSteerMessage(msg)

                // Check cancellation AFTER delivering messages — user cancel
                // prevents further tool execution, but the steer is preserved.
                if (isCancelled()) {
                    return LoopResult.Completed(
                        text = null,
                        steps = toolSteps,
                        exitReason = "cancelled"
                    )
                }

                // Transform context before re-prompting after steer
                transformContext?.invoke()

                response = try {
                    continueAfterTools()
                } catch (e: Exception) {
                    // API error after steer delivery — exit explicitly rather than
                    // continuing with an error response that the model never sees.
                    return LoopResult.Completed(
                        text = "Error: ${e.message?.take(ERROR_MESSAGE_MAX_LENGTH)}",
                        steps = toolSteps,
                        exitReason = "api_error_after_steer"
                    )
                }
                // No tools executed yet, so don't increment toolSteps.
                // The step counter tracks tool execution steps, not loop iterations.
                continue
            }

            // Top-of-loop cancellation guard — catches cancellation that arrived
            // during the API call when no steer messages were pending.
            if (isCancelled()) {
                return LoopResult.Completed(
                    text = null,
                    steps = toolSteps,
                    exitReason = "cancelled"
                )
            }

            toolSteps++
            delegate.onStepStarted(toolSteps, maxToolSteps)

            var steered = false
            for ((toolIndex, toolCall) in response.toolCalls.withIndex()) {
                val preActionScreen = screenContent
                val preActionHash = preActionScreen?.hashCode()
                progressListener.onToolStarted(toolCall.name, toolIndex, response.toolCalls.size)

                // Execute the tool
                val actionResult = try {
                    delegate.executeToolCall(toolCall, screenContent)
                } catch (e: Exception) {
                    ToolResult("Error: ${e.message?.take(ERROR_MESSAGE_MAX_LENGTH)}", isError = true)
                }

                // Track consecutive failures per tool for severity escalation
                val retryContext = if (actionResult.isError) {
                    val count = failureCounts.merge(toolCall.name, 1, Int::plus)!!
                    RetryContext(consecutiveFailures = count)
                } else {
                    failureCounts.remove(toolCall.name)
                    null
                }

                // Compute effective severity once (avoid duplicate classifyError calls)
                val effectiveSeverity = if (actionResult.isError) {
                    actionResult.severity ?: OutputClassifier.classifyError(toolCall.name, actionResult.text, retryContext)
                } else null

                // Classify and report to UI
                val visibility = OutputClassifier.classify(
                    toolCall.name, actionResult.text, actionResult.isError,
                    severity = effectiveSeverity,
                    retryContext = retryContext
                )

                // Notify listener of errors before reporting the result
                if (actionResult.isError && effectiveSeverity != null) {
                    progressListener.onToolError(toolCall.name, actionResult.text, effectiveSeverity)
                }

                val effectiveVisibility = OutputClassifier.applyVerbosity(
                    visibility, delegate.outputVerbosity(), effectiveSeverity
                )
                progressListener.onToolResult(toolCall.name, actionResult.text, effectiveVisibility, actionResult.isError)

                // Settle delay
                delegate.settleDelay(toolCall.name, actionResult.text)

                // For UI-mutating tools, refresh screen and format result
                val baseToolResult = if (delegate.isUiMutatingTool(toolCall.name)) {
                    screenContent = delegate.refreshScreenAfterTool(toolCall.name, actionResult.text)
                    delegate.formatToolResult(actionResult.text, screenContent)
                } else {
                    actionResult.text
                }

                val failure = detectFailure(
                    toolCall = toolCall,
                    result = actionResult,
                    screenBefore = preActionScreen.toFingerprint(),
                    screenAfter = screenContent.toFingerprint(),
                    // Shared across sequential tool calls (including calls in the same model batch).
                    // A success resets this streak, so recovery escalation reflects the current run's
                    // contiguous failure streak rather than lifetime failures.
                    consecutiveFailures = consecutiveFailures
                )

                val recoveryGuidance = failure?.let { recoveryManager.evaluateFailure(it) }
                if (failure != null) consecutiveFailures++ else consecutiveFailures = 0

                val toolResult = if (recoveryGuidance != null) {
                    baseToolResult + recoveryGuidance
                } else {
                    baseToolResult
                }

                // === POST-TOOL BOUNDARY CHECK POINT ===
                // Drain steer messages at each tool boundary so SteerCheck can see them.
                val steerMessages = steerMessageSource()
                val loopState = LoopState(
                    step = toolSteps,
                    maxSteps = maxToolSteps,
                    lastToolName = toolCall.name,
                    lastScreenHash = screenContent?.hashCode(),
                    isCancelled = isCancelled(),
                    pendingSteerMessages = steerMessages,
                    lastToolWasUiMutating = delegate.isUiMutatingTool(toolCall.name),
                    preActionScreenHash = preActionHash,
                    pendingInterruption = interruptionSource()
                )
                val checkResult = evaluateBoundaryChecks(loopState)

                var pendingForCheckpoint = response.toolCalls.drop(toolIndex + 1)
                var stopReason: String? = null
                var shouldBreakToolBatch = false

                when (checkResult) {
                    is CheckResult.Steer -> {
                        // Commit this tool's result as-is
                        delegate.addToolResult(toolCall.id, toolResult, toolCall.name, actionResult.isError)
                        // Provide explicit skip results for remaining tool calls.
                        // The API contract requires every tool_use block to have a
                        // corresponding tool_result. Without this, the next API call
                        // after a mid-batch steer would fail with a schema error.
                        val currentIndex = response.toolCalls.indexOf(toolCall)
                        for (i in (currentIndex + 1) until response.toolCalls.size) {
                            val skipped = response.toolCalls[i]
                            delegate.addToolResult(skipped.id, "Skipped: user sent a new message.", isError = false, toolName = skipped.name)
                        }
                        for (msg in checkResult.userMessages) {
                            delegate.addSteerMessage(msg)
                        }
                        pendingForCheckpoint = emptyList()
                        steered = true
                        shouldBreakToolBatch = true
                    }
                    is CheckResult.Stop -> {
                        delegate.addToolResult(toolCall.id, toolResult, toolCall.name, actionResult.isError)
                        pendingForCheckpoint = emptyList()
                        stopReason = checkResult.reason
                    }
                    is CheckResult.Inject -> {
                        delegate.addToolResult(toolCall.id, toolResult + checkResult.message, toolCall.name, actionResult.isError)
                    }
                    CheckResult.Continue -> {
                        // Continue intentionally relies on the shared checkpoint path below,
                        // so every branch emits exactly one checkpoint callback per tool boundary.
                        delegate.addToolResult(toolCall.id, toolResult, toolCall.name, actionResult.isError)
                    }
                }

                checkpointCallback?.invoke(
                    LoopCheckpoint(
                        step = toolSteps,
                        maxSteps = maxToolSteps,
                        lastToolName = toolCall.name,
                        pendingToolCalls = pendingForCheckpoint
                    )
                )

                if (stopReason != null) {
                    return LoopResult.Completed(
                        text = null,
                        steps = toolSteps,
                        exitReason = stopReason
                    )
                }

                if (shouldBreakToolBatch) {
                    break // Skip remaining tool calls in this batch
                }
            }

            // Between-step cancellation guard — catches cancellation that
            // occurred after the last tool call's boundary check but before
            // the continueAfterTools call.
            if (!steered && isCancelled()) {
                return LoopResult.Completed(
                    text = null,
                    steps = toolSteps,
                    exitReason = "cancelled"
                )
            }

            // Transform context before next LLM call (e.g. trim old messages)
            transformContext?.invoke()

            // Get next response from model
            response = try {
                continueAfterTools()
            } catch (e: Exception) {
                ChatResponse(
                    text = "Error: ${e.message?.take(ERROR_MESSAGE_MAX_LENGTH)}",
                    toolCalls = emptyList(),
                    stopReason = "error"
                )
            }
        }

        val finalText = response?.text
        val exitReason = if (finalText != null) "end_turn" else "no_response"

        return LoopResult.Completed(
            text = finalText,
            steps = toolSteps,
            exitReason = exitReason
        )
    }

    /**
     * Evaluate all boundary checks against the current state.
     *
     * Priority: [CheckResult.Stop] > [CheckResult.Steer] > [CheckResult.Inject].
     * [Stop] short-circuits — remaining checks are skipped.
     * [Steer] takes priority over [Inject] (user intent > system warnings).
     * Multiple [Inject] results are concatenated.
     * [CheckResult.Continue] is returned when all checks pass or the list is empty.
     */
    private suspend fun evaluateBoundaryChecks(state: LoopState): CheckResult {
        val injections = mutableListOf<String>()
        var steer: CheckResult.Steer? = null
        for (check in boundaryChecks) {
            when (val result = check.check(state)) {
                is CheckResult.Stop -> return result
                is CheckResult.Steer -> steer = result
                is CheckResult.Inject -> injections.add(result.message)
                CheckResult.Continue -> { /* no-op */ }
            }
        }
        // Steer takes priority over Inject — user intent overrides system warnings
        if (steer != null) return steer
        return if (injections.isEmpty()) CheckResult.Continue
        else CheckResult.Inject(injections.joinToString(""))
    }
}

/**
 * Minimal persisted checkpoint metadata emitted by [AgentExecutor] after each tool boundary.
 */
data class LoopCheckpoint(
    val step: Int,
    val maxSteps: Int,
    val lastToolName: String,
    val pendingToolCalls: List<ToolCall>
)

/**
 * Structured result from [AgentExecutor.run].
 */
sealed class LoopResult {
    abstract val steps: Int

    /**
     * Loop completed (possibly with tools, possibly just text).
     *
     * @param text Final text response from the model (null if loop was cancelled or hit limits)
     * @param steps Number of tool execution steps completed
     * @param exitReason Why the loop ended. One of:
     *   - `"no_tools"` — initial response had no tool calls (pure text reply)
     *   - `"end_turn"` — model returned text with no more tool calls (normal completion)
     *   - `"max_steps"` — hit the [AgentExecutor.maxToolSteps] limit
     *   - `"cancelled"` — [isCancelled] returned true (user pressed Stop)
     *   - `"accessibility_lost"` — accessibility service detached and couldn't reconnect
     *   - `"no_response"` — loop ended without text or tool calls (shouldn't happen normally)
     *   - Custom reasons from custom [BoundaryCheck] implementations
     */
    data class Completed(
        val text: String?,
        override val steps: Int,
        val exitReason: String
    ) : LoopResult()

    /** Loop failed with an unrecoverable error. */
    data class Error(
        val message: String,
        override val steps: Int
    ) : LoopResult()
}

/**
 * Delegate interface for tool execution side effects.
 * ChatViewModel implements this, bridging AgentExecutor to ScreenReader and PhoneAgentApi.
 */
interface ToolExecutionDelegate {
    /** Execute a tool call and return a typed result. */
    suspend fun executeToolCall(toolCall: ToolCall, screenContent: ScreenContent?): ToolResult

    /** Refresh the current screen content. */
    suspend fun refreshScreen(): ScreenContent?

    /**
     * Refresh screen after a tool execution, with smart polling for tools
     * like open_app/press_home that cause package changes.
     */
    suspend fun refreshScreenAfterTool(toolName: String, actionResult: String): ScreenContent?

    /** Wait for UI to settle after a tool action. */
    suspend fun settleDelay(toolName: String, actionResult: String)

    /** Format a tool result with optional screen context appended. */
    fun formatToolResult(actionSummary: String, screenContent: ScreenContent?): String

    /** Whether this tool mutates the UI (needs screen refresh after). */
    fun isUiMutatingTool(toolName: String): Boolean

    /** Whether the screen reader / accessibility service is currently available. */
    fun isScreenReaderAvailable(): Boolean

    /** Wait for the accessibility service to reattach. */
    suspend fun waitForAccessibility(timeoutMs: Long): Boolean

    /** Timeout for accessibility reattachment waiting. */
    fun accessibilityWaitMs(): Long

    /** Current user output verbosity preference. */
    fun outputVerbosity(): OutputVerbosity

    /** Add a tool result to the agent's conversation history. */
    fun addToolResult(toolCallId: String, result: String, toolName: String? = null, isError: Boolean = false)

    /**
     * Add a user steer message to conversation history as a first-class user turn.
     *
     * Called when a [SteerCheck] fires or a pre-batch steer is detected.
     * The message should be added as `role = "user"` in the API conversation,
     * not appended to a tool result. Models weigh user messages significantly
     * more than incidental text in tool output.
     */
    fun addSteerMessage(text: String)

    /** Called when a new tool step starts (for syncing step counters). */
    fun onStepStarted(step: Int, maxSteps: Int)
}

/**
 * Listener for loop progress events, used to update the UI.
 */
interface LoopProgressListener {
    /**
     * A tool is about to execute.
     *
     * @param toolName  the tool name (e.g. "open_app", "tap", "web_search")
     * @param toolIndex 0-based index within the current batch of tool calls
     * @param batchSize total number of tool calls in this batch
     */
    fun onToolStarted(toolName: String, toolIndex: Int, batchSize: Int)

    /**
     * A tool has produced a result. Visibility indicates how to display it.
     *
     * When [isError] is false, implementations may use this as a signal to clear
     * any persistent error status previously set by [onToolError] — a successful
     * result indicates the error condition has resolved.
     */
    fun onToolResult(toolName: String, result: String, visibility: OutputVisibility, isError: Boolean = false)

    /**
     * A tool execution resulted in an error.
     *
     * Called after error severity classification, before the tool result
     * is reported via [onToolResult]. Used for error-specific UI updates
     * like transient status indicators.
     *
     * Default no-op for backward compatibility.
     *
     * @param toolName The tool that failed
     * @param errorText The error message
     * @param severity Classified severity of this error
     */
    fun onToolError(toolName: String, errorText: String, severity: ErrorSeverity) {}

    /** The accessibility service was lost mid-loop. */
    fun onAccessibilityLost()
}
