package ai.citros.chat

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Binder
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import ai.citros.core.AgentExecutor
import ai.citros.core.AgentState
import ai.citros.core.ErrorSeverity
import ai.citros.core.LoopProgressListener
import ai.citros.core.LoopResult
import ai.citros.core.Message
import ai.citros.core.InterruptionEvent
import ai.citros.core.OutputClassifier
import ai.citros.core.OutputVisibility
import ai.citros.core.PhoneAgentApi
import ai.citros.core.ScreenContent
import ai.citros.core.ScreenReader
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.util.UUID

/**
 * Foreground service that owns the agent execution lifecycle.
 *
 * Survives activity destruction, screen off, and app switching.
 * The brain lives here; ChatActivity and OverlayService are pure UI
 * that bind to this service to observe state and dispatch user input.
 *
 * Lifecycle:
 * - Started via [startForegroundService] when user sends a task message
 * - Runs as foreground service with persistent notification
 * - Stops after [IDLE_TIMEOUT_MS] of no activity (configurable)
 * - Can be manually stopped by user via notification action
 *
 * See docs/specs/sprint-0-service-architecture.md
 */
/**
 * Callback interface for UI progress events from the service.
 * Implemented by ChatViewModel to receive execution progress.
 * When null (activity dead), the service continues headlessly.
 */
interface ServiceProgressCallback {
    fun onAssistantMessage(text: String)
    fun onToolStatus(status: String?)
    fun onExecutionComplete(steps: Int)
    fun onExecutionError(error: String, steps: Int)
}

class AgentService : Service() {

    companion object {
        private const val TAG = "AgentService"
        const val NOTIFICATION_ID = 1001
        const val CHANNEL_ID = "citros_agent"
        const val CHANNEL_NAME = "Citros Agent"

        /** Idle timeout before service self-stops. Default 5 minutes. */
        const val IDLE_TIMEOUT_MS = 5 * 60 * 1000L

        // Intent actions
        const val ACTION_START_TASK = "ai.citros.action.START_TASK"
        const val ACTION_STEER = "ai.citros.action.STEER"
        const val ACTION_CANCEL = "ai.citros.action.CANCEL"
        const val ACTION_STOP = "ai.citros.action.STOP"

        // Intent extras
        const val EXTRA_MESSAGE = "message"
        const val EXTRA_TASK_ID = "task_id"

        /**
         * Create an intent to start a new task.
         */
        fun startTaskIntent(context: Context, message: String): Intent {
            return Intent(context, AgentService::class.java).apply {
                action = ACTION_START_TASK
                putExtra(EXTRA_MESSAGE, message)
                putExtra(EXTRA_TASK_ID, UUID.randomUUID().toString())
            }
        }

        /**
         * Create an intent to inject a steer message into the active task.
         */
        fun steerIntent(context: Context, message: String): Intent {
            return Intent(context, AgentService::class.java).apply {
                action = ACTION_STEER
                putExtra(EXTRA_MESSAGE, message)
            }
        }

        /**
         * Create an intent to cancel the active task.
         */
        fun cancelIntent(context: Context): Intent {
            return Intent(context, AgentService::class.java).apply {
                action = ACTION_CANCEL
            }
        }

        /**
         * Create an intent to stop the service entirely.
         *
         * NOTE: Callers should dispatch this via [Context.startService], NOT
         * [Context.startForegroundService], to avoid the startForeground race
         * condition. The service is already foreground from the initial start.
         */
        fun stopIntent(context: Context): Intent {
            return Intent(context, AgentService::class.java).apply {
                action = ACTION_STOP
            }
        }
    }

    // --- Service scope (survives activity lifecycle) ---
    private val serviceJob = SupervisorJob()

    /**
     * Coroutine scope for service-owned work (idle timeout, future task execution).
     * Dispatcher is injectable for testing (B3 fix: TestDispatcher support).
     */
    @androidx.annotation.VisibleForTesting
    internal var dispatcher: kotlinx.coroutines.CoroutineDispatcher = Dispatchers.Main
    private val serviceScope: CoroutineScope
        get() = CoroutineScope(dispatcher + serviceJob)

    /** Active task job — cancelled by handleCancel() and handleStop(). */
    @androidx.annotation.VisibleForTesting
    internal var currentTaskJob: kotlinx.coroutines.Job? = null

    // --- Execution dependencies (set by ChatViewModel via binder) ---

    /**
     * PhoneAgentApi reference set by ChatViewModel when dispatching a task.
     * Survives activity death because it's a network client with no Activity dependency.
     */
    @Volatile
    private var phoneAgentApi: PhoneAgentApi? = null

    /**
     * Steer message source. Returns pending steer messages drained from the queue.
     * Set by ChatViewModel when bound. Null when unbound (headless mode — no steers).
     */
    @Volatile
    var steerMessageSource: (() -> List<String>)? = null

    /**
     * Cancellation check. Returns true if the user wants to cancel.
     * Falls back to service-level cancel flag when ViewModel is unbound.
     */
    @Volatile
    private var externalCancelCheck: (() -> Boolean)? = null
    private val serviceCancelFlag = java.util.concurrent.atomic.AtomicBoolean(false)

    /**
     * Interruption source for detecting app interruptions during tool loop.
     * Returns a pending InterruptionEvent or null. Set by ChatViewModel.
     */
    @Volatile
    var interruptionSource: (() -> InterruptionEvent?)? = null

    /**
     * UI callback for progress events. Set by ChatViewModel when bound.
     * Null when unbound (headless mode — progress goes to StateFlow only).
     */
    @Volatile
    var progressCallback: ServiceProgressCallback? = null

    /** Max tool steps per task. */
    @androidx.annotation.VisibleForTesting
    internal var maxToolSteps: Int = 25

    // --- Observable state ---
    private val _agentState = MutableStateFlow<AgentState>(AgentState.Idle)
    val agentState: StateFlow<AgentState> = _agentState.asStateFlow()

    // TODO(PR 2): Populated by AgentExecutor integration. ChatViewModel
    // currently owns message state; PR 2 migrates it here.
    private val _conversationMessages = MutableStateFlow<List<Message>>(emptyList())
    val conversationMessages: StateFlow<List<Message>> = _conversationMessages.asStateFlow()

    // --- Foreground state ---
    private var isForeground = false

    // --- Idle timeout ---
    @androidx.annotation.VisibleForTesting
    internal var idleTimeoutJob: kotlinx.coroutines.Job? = null

    // --- Binder ---
    private val binder = AgentBinder()

    inner class AgentBinder : Binder() {
        fun getService(): AgentService = this@AgentService

        /**
         * Configure execution dependencies. Called by ChatViewModel when binding.
         * These references survive activity death (PhoneAgentApi is a network client,
         * steer/cancel sources gracefully degrade to null when ViewModel unbinds).
         */
        fun configureExecution(
            api: PhoneAgentApi,
            steerSource: (() -> List<String>)? = null,
            cancelCheck: (() -> Boolean)? = null,
            interruptSource: (() -> InterruptionEvent?)? = null,
            progress: ServiceProgressCallback? = null
        ) {
            phoneAgentApi = api
            steerMessageSource = steerSource
            externalCancelCheck = cancelCheck
            interruptionSource = interruptSource
            progressCallback = progress
        }

        /**
         * Clear ViewModel-dependent references on unbind.
         * PhoneAgentApi is kept (survives activity death).
         * Callbacks are cleared (they reference ViewModel/Activity).
         */
        fun clearCallbacks() {
            steerMessageSource = null
            externalCancelCheck = null
            interruptionSource = null
            progressCallback = null
        }
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        Log.d(TAG, "onCreate: AgentService created")
    }

    /**
     * START_STICKY: if the system kills the service, it will be recreated
     * with a null intent. On recreation, we check for durable state and
     * resume if found. START_REDELIVER_INTENT is not needed because we
     * don't rely on the intent payload for recovery — durable state is
     * the source of truth.
     */
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // B1 fix: only call startForeground once, and skip it entirely for STOP.
        // startForeground must be called within 5s of startForegroundService() to
        // avoid ForegroundServiceDidNotStartInTimeException — but STOP intents
        // should use regular startService() (see stopIntent() companion method).
        if (!isForeground && intent?.action != ACTION_STOP) {
            startForeground(NOTIFICATION_ID, buildNotification(_agentState.value))
            isForeground = true
        }

        when (intent?.action) {
            ACTION_START_TASK -> handleStartTask(intent)
            ACTION_STEER -> handleSteer(intent)
            ACTION_CANCEL -> handleCancel()
            ACTION_STOP -> handleStop()
            null -> {
                // System restart after kill — attempt recovery from durable state.
                // TaskStateManager integration comes in Sprint 0 PR 3.
                if (!isForeground) {
                    startForeground(NOTIFICATION_ID, buildNotification(_agentState.value))
                    isForeground = true
                }
                Log.i(TAG, "onStartCommand: null intent (system restart), checking for durable state")
                handleSystemRestart()
            }
        }

        return START_STICKY
    }

    override fun onDestroy() {
        super.onDestroy()
        serviceScope.cancel()
        Log.d(TAG, "onDestroy: AgentService destroyed")
    }

    // --- Intent handlers ---

    private fun handleStartTask(intent: Intent) {
        val message = intent.getStringExtra(EXTRA_MESSAGE) ?: return
        val taskId = intent.getStringExtra(EXTRA_TASK_ID) ?: UUID.randomUUID().toString()

        val currentState = _agentState.value
        if (currentState.isActive()) {
            // Task already in progress — treat as steer (concurrent task policy)
            Log.d(TAG, "handleStartTask: active task, treating as steer: '${message.take(60)}'")
            handleSteerMessage(message)
            return
        }

        Log.i(TAG, "handleStartTask: starting task $taskId: '${message.take(60)}'")
        cancelIdleTimeout()
        serviceCancelFlag.set(false)
        _agentState.value = AgentState.Thinking(taskId)
        updateNotification()

        val api = phoneAgentApi
        if (api == null) {
            Log.e(TAG, "handleStartTask: no PhoneAgentApi registered — cannot execute")
            failTask(taskId, "Agent not configured")
            return
        }

        currentTaskJob = serviceScope.launch {
            executeTask(taskId, message, api)
        }
    }

    private fun handleSteer(intent: Intent) {
        val message = intent.getStringExtra(EXTRA_MESSAGE) ?: return
        handleSteerMessage(message)
    }

    private fun handleSteerMessage(message: String) {
        val currentState = _agentState.value
        if (!currentState.isActive()) {
            Log.w(TAG, "handleSteer: no active task, ignoring steer")
            return
        }
        Log.d(TAG, "handleSteer: injecting steer message: '${message.take(60)}'")
        // Steer queue integration comes in PR 2.
    }

    private fun handleCancel() {
        val currentState = _agentState.value
        if (!currentState.isActive()) {
            Log.w(TAG, "handleCancel: no active task to cancel")
            return
        }
        Log.i(TAG, "handleCancel: cancelling active task")
        // Signal cancellation through both mechanisms:
        // 1. serviceCancelFlag — checked by AgentExecutor's isCancelled lambda
        // 2. coroutine job cancellation — propagates CancellationException
        serviceCancelFlag.set(true)
        currentTaskJob?.cancel()
        currentTaskJob = null
        _agentState.value = AgentState.Idle
        updateNotification()
        startIdleTimeout()
    }

    private fun handleStop() {
        Log.i(TAG, "handleStop: stopping service")
        currentTaskJob?.cancel()
        currentTaskJob = null
        cancelIdleTimeout()
        _agentState.value = AgentState.Idle
        isForeground = false
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun handleSystemRestart() {
        // Durable state recovery (TaskStateManager) comes in PR 3.
        // For now, just go idle and start the timeout.
        _agentState.value = AgentState.Idle
        startIdleTimeout()
    }

    // --- Task execution ---

    /**
     * Run the full agent task lifecycle: initial API call → tool loop → result.
     *
     * This is the core of the service architecture migration. The executor
     * runs in serviceScope (not viewModelScope), surviving activity death.
     */
    private suspend fun executeTask(taskId: String, message: String, api: PhoneAgentApi) {
        var toolSteps = 0
        var monitoringStarted = false
        try {
            val screenContent = try {
                if (ScreenReader.isAttached()) ScreenReader.getScreenContent() else null
            } catch (_: Exception) { null }

            appendConversation("user", message)
            api.seedConversationHistory(_conversationMessages.value.dropLast(1))

            val response = sendMessageWithFallback(api, message, screenContent)
            if (response == null) {
                failTask(taskId, "No response from API")
                return
            }

            if (response.toolCalls.isEmpty()) {
                val text = response.text ?: ""
                appendConversation("assistant", text)
                safeProgressCallback { it.onAssistantMessage(text) }
                completeTask(taskId, text)
                return
            }

            try {
                ScreenReader.toolLoopOverlayHideHook?.invoke()
            } catch (e: Exception) {
                Log.w(TAG, "executeTask: overlay hide hook failed: ${e.message}")
            }

            val delegate = ServiceToolDelegate(api)
            val serviceProgressListener = object : LoopProgressListener {
                override fun onToolStarted(toolName: String, toolIndex: Int, batchSize: Int) {
                    _agentState.value = AgentState.Executing(taskId, toolName, toolIndex, batchSize)
                    updateNotification()
                    safeProgressCallback { it.onToolStatus(toolName) }
                }

                override fun onToolResult(toolName: String, result: String, visibility: OutputVisibility, isError: Boolean) {
                    safeProgressCallback { it.onToolStatus(null) }
                    val displayText = OutputClassifier.formatForDisplay(toolName, result, visibility)
                    if (displayText != null) {
                        appendConversation("assistant", displayText)
                        safeProgressCallback { it.onAssistantMessage(displayText) }
                    }
                }

                override fun onToolError(toolName: String, errorText: String, severity: ErrorSeverity) {
                    if (severity == ErrorSeverity.PERSISTENT) {
                        safeProgressCallback { it.onToolStatus("⚠️ $errorText") }
                    }
                }

                override fun onAccessibilityLost() {
                    val warning = "⚠️ Phone control disconnected. Please re-enable it in Settings → Accessibility."
                    appendConversation("assistant", warning)
                    safeProgressCallback { it.onAssistantMessage(warning) }
                    serviceCancelFlag.set(true)
                }
            }

            val executor = AgentExecutor(
                delegate = delegate,
                progressListener = serviceProgressListener,
                maxToolSteps = maxToolSteps,
                steerMessageSource = { steerMessageSource?.invoke() ?: emptyList() },
                interruptionSource = { interruptionSource?.invoke() }
            )

            val isCancelled = {
                serviceCancelFlag.get() || (externalCancelCheck?.invoke() ?: false)
            }

            InterruptionDetector.startMonitoring(screenContent?.packageName)
            monitoringStarted = true

            val result = executor.run(
                initialResponse = response,
                initialScreenContent = screenContent,
                isCancelled = isCancelled,
                continueAfterTools = { api.continueAfterTools() }
            )

            toolSteps = when (result) {
                is LoopResult.Completed -> {
                    var finalText = result.text
                    val explanationPrompt = explanationPromptForExit(result.exitReason)
                    if (finalText == null && explanationPrompt != null) {
                        finalText = api.sendEphemeral(explanationPrompt)
                    }
                    if (finalText != null) {
                        appendConversation("assistant", finalText)
                        safeProgressCallback { it.onAssistantMessage(finalText) }
                    }
                    safeProgressCallback { it.onExecutionComplete(result.steps) }
                    completeTask(taskId, finalText ?: "Task completed")
                    result.steps
                }
                is LoopResult.Error -> {
                    safeProgressCallback { it.onExecutionError(result.message, result.steps) }
                    failTask(taskId, result.message)
                    result.steps
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            Log.i(TAG, "executeTask: cancelled")
            _agentState.value = AgentState.Idle
            updateNotification()
        } catch (e: Exception) {
            Log.e(TAG, "executeTask: crashed: ${e.message}", e)
            safeProgressCallback { it.onExecutionError("Crashed: ${e.message?.take(120)}", toolSteps) }
            failTask(taskId, "Crashed: ${e.message?.take(120)}")
        } finally {
            if (monitoringStarted) {
                InterruptionDetector.stopMonitoring()
            }
            try {
                ScreenReader.toolLoopOverlayRestoreHook?.invoke()
            } catch (e: Exception) {
                Log.w(TAG, "executeTask: overlay restore hook failed: ${e.message}")
            }
        }
    }

    private suspend fun sendMessageWithFallback(
        api: PhoneAgentApi,
        message: String,
        screenContent: ScreenContent?
    ) = try {
        api.sendMessage(message, screenContent, isActionLoop = false)
    } catch (e: Exception) {
        Log.w(TAG, "sendMessageWithFallback: first attempt failed (${e.message}), retrying once")
        api.sendMessage(message, screenContent, isActionLoop = false)
    }

    private fun safeProgressCallback(block: (ServiceProgressCallback) -> Unit) {
        try {
            progressCallback?.let(block)
        } catch (e: Exception) {
            Log.w(TAG, "progress callback failed: ${e.message}")
        }
    }

    private fun appendConversation(role: String, content: String) {
        if (content.isBlank()) return
        _conversationMessages.value = _conversationMessages.value + Message(role = role, content = content)
    }

    private fun explanationPromptForExit(exitReason: String): String? = when (exitReason) {
        "max_steps" -> "[System: You hit the step limit and couldn't finish the task. Summarize what you accomplished so far and ask the user how they'd like to proceed.]"
        "accessibility_lost" -> "[System: You lost connection to the phone's accessibility service during the task. Let the user know what happened and ask if they'd like to retry or do something else.]"
        else -> null
    }

    // --- Task lifecycle helpers ---

    /**
     * Transition to a terminal state and start idle timeout.
     * Called when task finishes (success or failure).
     */
    internal fun completeTask(taskId: String, result: String) {
        _agentState.value = AgentState.Complete(taskId, result)
        updateNotification()
        startIdleTimeout()
    }

    /**
     * Transition to failed state and start idle timeout.
     */
    internal fun failTask(taskId: String, error: String) {
        _agentState.value = AgentState.Failed(taskId, error)
        updateNotification()
        startIdleTimeout()
    }

    /**
     * Update the agent state. Used by AgentExecutor to report progress.
     */
    fun updateState(state: AgentState) {
        _agentState.value = state
        updateNotification()
    }

    // --- Idle timeout ---

    private fun startIdleTimeout() {
        cancelIdleTimeout()
        idleTimeoutJob = serviceScope.launch {
            delay(IDLE_TIMEOUT_MS)
            if (!_agentState.value.isActive()) {
                Log.i(TAG, "Idle timeout reached, stopping service")
                handleStop()
            }
        }
    }

    private fun cancelIdleTimeout() {
        idleTimeoutJob?.cancel()
        idleTimeoutJob = null
    }

    // --- Notification ---

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            CHANNEL_NAME,
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = "Citros agent activity status"
            setShowBadge(false)
        }
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(channel)
    }

    private fun buildNotification(state: AgentState): Notification {
        // NB5: Use action+package instead of direct class reference so
        // AgentService doesn't hard-depend on ChatActivity's class name.
        val contentIntent = PendingIntent.getActivity(
            this, 0,
            Intent(Intent.ACTION_MAIN).apply {
                setPackage(packageName)
                addCategory(Intent.CATEGORY_LAUNCHER)
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val builder = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_dialog_info)  // TODO: custom Citros notification icon
            .setContentIntent(contentIntent)
            .setOngoing(true)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)

        when (state) {
            is AgentState.Idle -> {
                builder.setContentTitle("Citros")
                    .setContentText("Ready")
                    .setPriority(NotificationCompat.PRIORITY_LOW)
                    .addAction(buildStopAction())
            }
            is AgentState.Thinking -> {
                builder.setContentTitle("Citros")
                    .setContentText("Thinking...")
                    .setPriority(NotificationCompat.PRIORITY_DEFAULT)
                    .addAction(buildCancelAction())
            }
            is AgentState.Executing -> {
                builder.setContentTitle("Working...")
                    .setContentText(state.toolName)
                    .setPriority(NotificationCompat.PRIORITY_DEFAULT)
                val total = state.totalSteps
                if (total != null) {
                    builder.setProgress(total, state.stepIndex, false)
                } else {
                    builder.setProgress(0, 0, true)  // Indeterminate when total unknown
                }
                builder.addAction(buildCancelAction())
            }
            is AgentState.WaitingForInput -> {
                builder.setContentTitle("Needs your input")
                    .setContentText(state.reason)
                    .setPriority(NotificationCompat.PRIORITY_HIGH)
            }
            is AgentState.Complete -> {
                builder.setContentTitle("Done")
                    .setContentText(state.result.take(100))
                    .setPriority(NotificationCompat.PRIORITY_LOW)
                    .setOngoing(false)
                    .setTimeoutAfter(10_000)
            }
            is AgentState.Failed -> {
                builder.setContentTitle("Error")
                    .setContentText(state.error.take(100))
                    .setPriority(NotificationCompat.PRIORITY_DEFAULT)
                    .setOngoing(false)
            }
            is AgentState.Resuming -> {
                builder.setContentTitle("Citros")
                    .setContentText("Resuming task...")
                    .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            }
        }

        return builder.build()
    }

    private fun updateNotification() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, buildNotification(_agentState.value))
    }

    private fun buildCancelAction(): NotificationCompat.Action {
        val intent = PendingIntent.getService(
            this, 1,
            cancelIntent(this),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Action.Builder(0, "Cancel", intent).build()
    }

    private fun buildStopAction(): NotificationCompat.Action {
        val intent = PendingIntent.getService(
            this, 2,
            stopIntent(this),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Action.Builder(0, "Stop", intent).build()
    }
}
