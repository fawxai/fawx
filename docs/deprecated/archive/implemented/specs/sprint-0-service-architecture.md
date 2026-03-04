# Sprint 0: Service Architecture

*Move the brain out of the activity. Make it survive the real world.*

**Status:** Spec
**Prerequisite:** None
**Depends on:** Nothing — this is the foundation
**Estimated PRs:** 3-4
**Interim architecture:** This spec defines a Kotlin foreground service as the brain host. Per SPEC.md §3.5.2, the long-term architecture is a Rust daemon (`fawx-daemon`) with the Kotlin app as a sensor/display adapter. This foreground service is the pragmatic Horizon 1 implementation that will be replaced by the Rust daemon in Horizon 3. The service interface (`AgentState`, `TaskStateManager`) is designed to be portable — the Rust daemon can expose the same contracts over Unix socket IPC.

---

## Problem

The agent loop runs in `ChatViewModel`, which is scoped to `ChatActivity`'s lifecycle. When Android destroys the activity — memory pressure, user switches apps, screen off timeout, system rotation — the coroutine scope cancels and the loop dies mid-execution.

This is the single biggest reliability ceiling in Fawx. No amount of loop intelligence fixes a process that gets killed during step 5 of a 10-step task.

**Current architecture:**
```
ChatActivity (owns lifecycle)
  └── ChatViewModel (scoped to activity)
       └── AgentExecutor (loop runs here)
            ├── ToolRunner
            ├── ScreenManager
            └── ContextManager

AccessibilityService (independent lifecycle — survives)
OverlayService (independent lifecycle — survives)
```

The irony: the agent's eyes (AccessibilityService) and mouth (OverlayService) survive activity death. Only the brain dies.

---

## Solution

### Architecture

```
AgentService (foreground service, persistent notification)
  ├── AgentExecutor (loop lives here)
  ├── TaskStateManager (durable state, survives process death)
  ├── WakeLockManager (partial wake lock during active execution)
  └── ServiceBinder (exposes state to UI)

ChatActivity (pure UI, binds to AgentService)
  └── ChatViewModel (observes service state, dispatches user input)

OverlayService (pure UI, binds to AgentService)

AccessibilityService (phone control, unchanged)
```

**Key change:** `AgentExecutor` moves from `ChatViewModel` to `AgentService`. The activity becomes a display adapter that binds to the service and observes its state. The service owns the loop lifecycle.

### What Survives What

| Event | Activity | Service | Loop |
|-------|----------|---------|------|
| User switches apps | ❌ May be destroyed | ✅ Running | ✅ Continues |
| Screen off | ❌ May be destroyed | ✅ Running (wake lock) | ✅ Continues |
| Memory pressure | ❌ First to die | ✅ Protected (foreground) | ✅ Continues |
| Force-stop | ❌ | ❌ | ❌ (nothing survives) |
| Reboot | ❌ | ❌ | 🔄 Resume from durable state |
| Process death (rare for FG service) | ❌ | ❌ | 🔄 Resume from durable state |

---

## Design

### 1. AgentService — Foreground Service

```kotlin
package ai.fawx.chat

/**
 * Foreground service that owns the agent execution loop.
 * Survives activity destruction, screen off, and app switching.
 *
 * Lifecycle:
 * - Started when user sends first message (or app opens with pending task)
 * - Runs as foreground service with persistent notification
 * - Stops after idle timeout (configurable, default 5 minutes of no activity)
 * - Can be manually stopped by user via notification action
 */
class AgentService : Service() {

    companion object {
        const val NOTIFICATION_ID = 1001
        const val CHANNEL_ID = "fawx_agent"
        const val IDLE_TIMEOUT_MS = 5 * 60 * 1000L  // 5 minutes

        // Actions
        const val ACTION_START_TASK = "ai.fawx.action.START_TASK"
        const val ACTION_STEER = "ai.fawx.action.STEER"
        const val ACTION_CANCEL = "ai.fawx.action.CANCEL"
        const val ACTION_STOP = "ai.fawx.action.STOP"
    }

    // --- Core Components ---
    private lateinit var agentExecutor: AgentExecutor
    private lateinit var taskStateManager: TaskStateManager
    private lateinit var wakeLockManager: WakeLockManager

    /**
     * Restart strategy: START_STICKY.
     *
     * If the system kills the service, it will be recreated with a null intent.
     * On recreation, we check TaskStateManager for pending durable state and
     * resume if found. START_REDELIVER_INTENT is not needed because we don't
     * rely on the intent payload for recovery — durable state is the source
     * of truth.
     */
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START_TASK -> handleStartTask(intent)
            ACTION_STEER -> handleSteer(intent)
            ACTION_CANCEL -> handleCancel(intent)
            ACTION_STOP -> stopSelf()
            null -> {
                // System restart after kill — check for durable state
                serviceScope.launch { attemptRecoveryFromCheckpoint() }
            }
        }
        return START_STICKY
    }

    // --- Observable State (UI binds to these) ---
    private val _agentState = MutableStateFlow<AgentState>(AgentState.Idle)
    val agentState: StateFlow<AgentState> = _agentState.asStateFlow()

    private val _conversationMessages = MutableStateFlow<List<Message>>(emptyList())
    val conversationMessages: StateFlow<List<Message>> = _conversationMessages.asStateFlow()

    private val _currentTask = MutableStateFlow<TaskInfo?>(null)
    val currentTask: StateFlow<TaskInfo?> = _currentTask.asStateFlow()
}
```

### Concurrent Task Policy

**Single-task execution model.** AgentService executes one task at a time. When a new message arrives during an active task:

| Scenario | Behavior |
|----------|----------|
| Active task executing | New message is treated as a **steer** — injected into the active task's conversation at the next boundary check. The agent sees it as user guidance mid-task ("actually send it to Dad instead"). |
| Active task waiting for confirmation | New message replaces the confirmation — acts as user input for the pending decision. |
| Active task in final response (Complete) | New message starts a new task. |
| Idle (no active task) | New message starts a new task. |

This matches the current `ChatViewModel` behavior (steer on message during execution) and avoids the complexity of a task queue. A queue may be added in a future sprint if proactive behaviors (H3.4) require multiple pending tasks.

### 2. AgentState — Observable Lifecycle

```kotlin
package ai.fawx.core

/**
 * Observable state of the agent, exposed by AgentService.
 * UI binds to this to show appropriate feedback.
 */
sealed class AgentState {
    /** No active task. Service may be running but idle. */
    object Idle : AgentState()

    /** Processing a user message (initial API call). */
    data class Thinking(val taskId: String) : AgentState()

    /** Executing a tool call. */
    data class Executing(
        val taskId: String,
        val toolName: String,
        val stepIndex: Int,
        val totalSteps: Int
    ) : AgentState()

    /** Waiting for user confirmation (policy gate). */
    data class WaitingConfirmation(
        val taskId: String,
        val requestId: String,
        val toolName: String,
        val reason: String
    ) : AgentState()

    /** Task completed, displaying result. */
    data class Complete(val taskId: String, val result: String) : AgentState()

    /** Task failed. */
    data class Failed(val taskId: String, val error: String) : AgentState()

    /** Resuming from durable state after process restart. */
    data class Resuming(val taskId: String) : AgentState()
}
```

### 3. TaskStateManager — Durable Persistence

```kotlin
package ai.fawx.core

/**
 * Persists task state to survive process death.
 *
 * Uses Proto DataStore for atomic writes and type safety.
 * State is checkpointed after each tool execution step.
 *
 * Recovery contract:
 * - On service restart, check for pending task state
 * - If found and task is resumable: resume from last checkpoint
 * - If found but task is stale (>30 min old): discard and notify user
 * - If not found: start fresh (normal case)
 */
interface TaskStateManager {
    /** Save current task state (called after each tool step). */
    suspend fun checkpoint(state: TaskState)

    /**
     * Load pending task state, if any. Null = no pending work.
     *
     * Stale threshold: tasks older than STALE_THRESHOLD_MS (default 30 min)
     * are discarded rather than resumed. Rationale: screen state has almost
     * certainly changed after 30 minutes (notifications, app updates, user
     * interaction), making the checkpoint's screen context unreliable.
     *
     * This is configurable via user settings because some tasks (email
     * composition, multi-app workflows) may be worth resuming after longer
     * gaps. The UI shows "Resume interrupted task?" when a non-stale
     * checkpoint is found, giving the user the final decision.
     */
    suspend fun loadPending(staleThresholdMs: Long = STALE_THRESHOLD_MS): TaskState?

    companion object {
        const val STALE_THRESHOLD_MS = 30 * 60 * 1000L  // 30 minutes, user-configurable
    }

    /** Clear task state (called on task completion or discard). */
    suspend fun clear()
}

/**
 * Serializable task state — everything needed to resume a task.
 */
data class TaskState(
    val taskId: String,
    val userMessage: String,
    val conversationHistory: List<SerializedMessage>,
    val currentStep: Int,
    val maxSteps: Int,
    val startedAtMs: Long,
    val lastCheckpointMs: Long,
    val pendingToolCalls: List<SerializedToolCall>,
    val status: TaskStatus,
    val subtaskInProgress: Boolean = false,
    val subtaskGoal: String? = null,
    val subtaskDepth: Int = 0
)

enum class TaskStatus {
    /** Task is actively executing. */
    ACTIVE,
    /** Task was interrupted (process death during execution). */
    INTERRUPTED,
    /** Task completed successfully. */
    COMPLETED,
    /** Task failed. */
    FAILED
}
```

### 4. WakeLockManager — Power Management

```kotlin
package ai.fawx.chat

/**
 * Manages partial wake lock during active agent execution.
 *
 * Wake lock is acquired when the agent starts executing tool calls
 * and released when:
 * - Task completes or fails
 * - Idle timeout fires (no tool execution for IDLE_RELEASE_MS)
 * - Service stops
 *
 * PARTIAL_WAKE_LOCK keeps CPU running but allows screen to turn off.
 * This is appropriate for tool execution (taps, API calls) but not
 * for idle waiting.
 */
class WakeLockManager(context: Context) {

    companion object {
        const val TAG = "fawx:agent-execution"
        const val MAX_WAKE_LOCK_MS = 10 * 60 * 1000L  // 10 minutes max
        const val IDLE_RELEASE_MS = 30_000L             // Release after 30s idle
    }

    private val powerManager = context.getSystemService(PowerManager::class.java)
    private var wakeLock: PowerManager.WakeLock? = null

    fun acquire() {
        if (wakeLock?.isHeld == true) return
        wakeLock = powerManager.newWakeLock(
            PowerManager.PARTIAL_WAKE_LOCK,
            TAG
        ).apply {
            acquire(MAX_WAKE_LOCK_MS)
        }
    }

    fun release() {
        wakeLock?.let {
            if (it.isHeld) it.release()
        }
        wakeLock = null
    }
}
```

### 5. Service Binding — UI Connection

```kotlin
package ai.fawx.chat

/**
 * Binder that exposes AgentService state to ChatActivity.
 *
 * Design: Activity binds on start, unbinds on stop. The service
 * continues running independently. If the activity is destroyed
 * and recreated (rotation, back navigation), it rebinds and
 * catches up via StateFlow collection.
 */
inner class AgentBinder : Binder() {
    fun getService(): AgentService = this@AgentService
}

// In ChatActivity:
private var agentService: AgentService? = null
private val serviceConnection = object : ServiceConnection {
    override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
        agentService = (binder as AgentService.AgentBinder).getService()
        // Collect StateFlows in lifecycleScope
        lifecycleScope.launch {
            agentService!!.agentState.collect { state ->
                viewModel.onAgentStateChanged(state)
            }
        }
        lifecycleScope.launch {
            agentService!!.conversationMessages.collect { messages ->
                viewModel.onMessagesChanged(messages)
            }
        }
    }

    override fun onServiceDisconnected(name: ComponentName?) {
        agentService = null
    }
}

override fun onStart() {
    super.onStart()
    // BIND_AUTO_CREATE is intentionally NOT used here.
    // The service is only created via startForegroundService() when a task
    // is dispatched. This avoids showing a persistent notification when the
    // user just opens the app to chat without requesting phone actions.
    // If the service is already running (task in progress), bindService
    // connects to it. If not, the bind silently fails and the activity
    // operates in standalone chat mode.
    Intent(this, AgentService::class.java).also { intent ->
        bindService(intent, serviceConnection, 0)  // No BIND_AUTO_CREATE
    }
}

override fun onStop() {
    super.onStop()
    unbindService(serviceConnection)
}
```

### 6. Notification — Foreground Service Requirement

```kotlin
/**
 * Persistent notification required for foreground service.
 * Shows current agent state and provides quick actions.
 *
 * States:
 * - Idle: "Fawx is ready" (minimal, low priority)
 * - Executing: "Working on: {task summary}" with cancel action
 * - Waiting: "Needs your approval" with approve/deny actions
 * - Complete: "Finished: {result summary}" (auto-dismiss after 10s)
 */
private fun buildNotification(state: AgentState): Notification {
    val builder = NotificationCompat.Builder(this, CHANNEL_ID)
        .setSmallIcon(R.drawable.ic_fawx_notification)
        .setOngoing(true)
        .setForegroundServiceBehavior(FOREGROUND_SERVICE_IMMEDIATE)

    when (state) {
        is AgentState.Idle -> {
            builder.setContentTitle("Fawx")
                .setContentText("Ready")
                .setPriority(NotificationCompat.PRIORITY_LOW)
                .addAction(R.drawable.ic_stop, "Stop", stopPendingIntent())
        }
        is AgentState.Executing -> {
            builder.setContentTitle("Working...")
                .setContentText(state.toolName)
                .setPriority(NotificationCompat.PRIORITY_DEFAULT)
                .setProgress(state.totalSteps, state.stepIndex, false)
                .addAction(R.drawable.ic_cancel, "Cancel", cancelPendingIntent(state.taskId))
        }
        is AgentState.WaitingConfirmation -> {
            builder.setContentTitle("Needs approval")
                .setContentText(state.reason)
                .setPriority(NotificationCompat.PRIORITY_HIGH)
                .addAction(R.drawable.ic_check, "Allow", approvePendingIntent(state.requestId))
                .addAction(R.drawable.ic_close, "Deny", denyPendingIntent(state.requestId))
        }
        is AgentState.Complete -> {
            builder.setContentTitle("Done")
                .setContentText(state.result.take(100))
                .setPriority(NotificationCompat.PRIORITY_LOW)
                .setTimeoutAfter(10_000)
        }
        else -> {
            builder.setContentTitle("Fawx")
                .setContentText("Active")
        }
    }

    return builder.build()
}
```

### 7. Foreground Service Type (Android 14+)

```xml
<!-- AndroidManifest.xml -->
<uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
<uses-permission android:name="android.permission.FOREGROUND_SERVICE_SPECIAL_USE" />
<uses-permission android:name="android.permission.WAKE_LOCK" />

<service
    android:name=".AgentService"
    android:foregroundServiceType="specialUse"
    android:exported="false">
    <property
        android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE"
        android:value="AI agent executing user-requested phone automation tasks" />
</service>
```

**Why `specialUse`:** Android 14 requires declaring a foreground service type. The agent doesn't fit neatly into predefined categories (media, location, health). `specialUse` is the catch-all for legitimate foreground work that doesn't match a specific type. The description string explains the use case for Google Play review (relevant when/if we submit, but we're sideload-only for now).

---

## Migration Plan

### Phase 1: Extract (PR 1)

Move `AgentExecutor` creation and ownership from `ChatViewModel` to `AgentService`. The `ChatViewModel` becomes a thin proxy:

```kotlin
// Before (ChatViewModel owns everything):
class ChatViewModel : ViewModel() {
    private val executor = AgentExecutor(...)
    fun sendMessage(text: String) {
        viewModelScope.launch { executor.run(text) }
    }
}

// After (ChatViewModel delegates to service):
class ChatViewModel : ViewModel() {
    private var service: AgentService? = null

    fun sendMessage(text: String) {
        service?.startTask(text) ?: error("AgentService not bound")
    }

    fun onAgentStateChanged(state: AgentState) {
        // Update UI state from service observations
    }
}
```

**Files changed:**
- `AgentService.kt` — NEW
- `ChatViewModel.kt` — Remove executor ownership, add service binding
- `ChatActivity.kt` — Add service connection lifecycle
- `AndroidManifest.xml` — Register service + permissions

**Test strategy:**
- Unit: `AgentService` starts/stops correctly, state transitions are valid
- Unit: `ChatViewModel` proxies to service correctly
- Integration: Full message → execution → result flow through service

### Phase 2: Persist (PR 2)

Add `TaskStateManager` with Proto DataStore backend. Checkpoint after each tool step.

**Files changed:**
- `TaskStateManager.kt` — NEW (interface)
- `ProtoTaskStateManager.kt` — NEW (DataStore implementation)
- `TaskState.proto` — NEW (protobuf schema)
- `AgentService.kt` — Wire checkpointing into tool loop
- `AgentExecutor.kt` — Add checkpoint callback hook

**Test strategy:**
- Unit: Serialize/deserialize TaskState round-trip
- Unit: Checkpoint → kill → loadPending returns correct state
- Unit: Stale state (>30 min) is discarded
- Integration: Simulate process death mid-task, verify recovery

### Phase 3: Power (PR 3)

Add `WakeLockManager`. Acquire during active execution, release on idle.

**Files changed:**
- `WakeLockManager.kt` — NEW
- `AgentService.kt` — Wire wake lock acquire/release

**Test strategy:**
- Unit: Wake lock acquired on task start, released on completion
- Unit: Max duration enforced
- Unit: Idle timeout releases wake lock

### Phase 4: Polish (PR 4, optional)

- Notification updates reflecting agent state
- Battery optimization exemption prompt in onboarding
- Service restart on boot (optional, needs `RECEIVE_BOOT_COMPLETED`)

---

## Android Lifecycle Considerations

### Battery Optimization

Android's battery optimization can still affect foreground services on some devices. The onboarding flow should prompt the user to exempt Fawx from battery optimization:

```kotlin
if (!powerManager.isIgnoringBatteryOptimizations(packageName)) {
    // Show dialog explaining why, then:
    startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
        data = Uri.parse("package:$packageName")
    })
}
```

On Pixel devices (our primary target), foreground services are well-respected and this is mostly a precaution. On Samsung/Xiaomi/Huawei, it's critical.

### Doze Mode

Android Doze restricts network access and defers jobs when the device is idle. Foreground services are exempt from most Doze restrictions, but network requests may still be deferred in deep Doze. Since the agent loop primarily uses local accessibility actions (not network), this is low risk. Cloud API calls (LLM inference) may experience brief delays.

### Service Lifecycle Edge Cases

| Scenario | Behavior |
|----------|----------|
| User opens app while task is running | Activity binds to service, catches up via StateFlow |
| User rotates device during task | Activity destroyed/recreated, rebinds, task continues |
| User force-stops app | Everything dies. On next open, check for durable state |
| System kills process (extreme memory pressure) | Rare for FG service. On restart, resume from checkpoint |
| User clears app data | Durable state is cleared. Clean start. |

---

## Test Matrix (Implementation PR Gate)

| Layer | Test ID | Scenario | Expected |
|-------|---------|----------|----------|
| Unit | S1 | Service starts as foreground with notification | Service running, notification visible |
| Unit | S2 | Service stops after idle timeout | Service stopped, wake lock released |
| Unit | S3 | Task dispatched to service via intent | AgentExecutor.run() called, state = Executing |
| Unit | S4 | State transitions: Idle → Thinking → Executing → Complete | Each state emitted in order via StateFlow |
| Unit | S5 | Cancel intent stops active task | AgentExecutor cancellation fires, state = Idle |
| Unit | S6 | Steer intent injects message at boundary | Existing steer mechanism works through service |
| Unit | S7 | TaskState serialization round-trip | Proto serialize → deserialize is identical |
| Unit | S8 | Checkpoint written after each tool step | DataStore contains current step data |
| Unit | S9 | Load pending task after simulated process death | TaskState recovered with correct step/history |
| Unit | S10 | Stale task (>30 min) discarded on load | loadPending returns null, state cleared |
| Unit | S11 | Wake lock acquired on task start | PowerManager.WakeLock.isHeld == true |
| Unit | S12 | Wake lock released on task completion | PowerManager.WakeLock.isHeld == false |
| Unit | S13 | Wake lock max duration enforced | Released after MAX_WAKE_LOCK_MS even if task continues |
| Integration | S14 | Activity destroyed mid-task, recreated, rebinds | New activity shows current task state |
| Integration | S15 | Full message flow through service | User sends message → service runs executor → result displayed |
| Integration | S16 | Service survives activity onStop/onDestroy | Task continues executing after activity destroyed |
| Integration | S17 | Confirmation flow through service notification | WaitingConfirmation state → notification actions work |
| Integration | S18 | Multiple tasks queued | Second task waits for first to complete |

---

## Blindspots

1. **AccessibilityService coordination.** The service needs to reference the AccessibilityService for screen reading and touch injection. Currently `ChatViewModel` accesses it via a static reference. The service needs the same access pattern or a better one (shared singleton, dependency injection).

2. **Overlay coordination.** The OverlayService currently receives updates from ChatViewModel. It needs to observe AgentService state instead. This is a wiring change, not an architecture change.

3. **Voice input routing.** Voice input currently goes through `ChatActivity` → `ChatViewModel`. It needs to route through to `AgentService`. The VoiceAccumulator lives in the activity (it needs the mic), but the transcribed text goes to the service.

4. **Testing foreground services.** Android's testing framework for services is more limited than for ViewModels. Robolectric supports basic service lifecycle but wake locks and notifications need careful mocking.

5. **Proto DataStore vs Room.** We chose Proto DataStore for task state because it's simpler (single object, atomic writes, no schema migrations). Room is already used for memory. If task state becomes more complex (multiple concurrent tasks, task history), Room might be better. Start simple.

6. **Service restart guarantees.** Even with `START_STICKY`, Android doesn't guarantee immediate service restart after process death. There may be a gap of seconds to minutes before the system restarts the service. Durable state bridges this gap, but the user might see a brief interruption.

7. **Android 15+ foreground service type restrictions.** Android 15 introduced stricter review of `specialUse` foreground services. Even though we're sideload-only, the justification string matters if we ever submit to Play Store. The `PROPERTY_SPECIAL_USE_FGS_SUBTYPE` value must accurately describe the use case. Monitor Android platform changes for foreground service type deprecations.

8. **Accessibility service reconnection after service restart.** If `AgentService` is killed and restarted via `START_STICKY`, the `AccessibilityService` may still be running but the communication channel (static reference) is stale. On service creation, re-establish the reference by querying `AccessibilityService.getInstance()` or using a `ServiceConnection`-style handshake. Test this explicitly.

9. **Non-Pixel OEM battery killers.** Samsung "Deep Sleep," Xiaomi "Battery Saver," and Huawei EMUI can kill foreground services regardless of wake locks. Our primary target is Pixel (stock Android), but if Fawx expands to other devices, OEM-specific workarounds (e.g., `dontkillmyapp.com` guidance) will be needed.

10. **Rollback strategy.** The migration from `ChatViewModel` → `AgentService` is a major architectural change. Rollback plan: keep `AgentExecutor` constructable from both `ChatViewModel` and `AgentService` behind a feature flag (`USE_SERVICE_ARCHITECTURE`). During development, the flag defaults to `true` on debug builds and can be toggled. If the service architecture causes regressions, flipping the flag reverts to the `ChatViewModel` path without code changes. Remove the flag and legacy path once the service architecture is stable (1-2 releases).

---

*Next: Sprint 1 (Loop Tuning) builds on this foundation.*
