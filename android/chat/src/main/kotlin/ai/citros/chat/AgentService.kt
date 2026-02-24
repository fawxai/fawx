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
import ai.citros.core.AgentState
import ai.citros.core.Message
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
        _agentState.value = AgentState.Thinking(taskId)
        updateNotification()

        // B4: Establish task job skeleton for cancellation support.
        // AgentExecutor integration comes in PR 2 — this job will own
        // the executor.run() coroutine. For now, it's a placeholder that
        // enables cancel tests to verify job cancellation.
        currentTaskJob = serviceScope.launch {
            // PR 2: AgentExecutor.run() goes here
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
        // B4: cancel the active task's coroutine job. AgentExecutor checks
        // isCancelled() which will return true once the job is cancelled.
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

    // --- Task lifecycle helpers ---

    /**
     * Transition to a terminal state and start idle timeout.
     * Called by AgentExecutor callbacks when a task finishes.
     */
    fun completeTask(taskId: String, result: String) {
        _agentState.value = AgentState.Complete(taskId, result)
        updateNotification()
        startIdleTimeout()
    }

    /**
     * Transition to failed state and start idle timeout.
     */
    fun failTask(taskId: String, error: String) {
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
