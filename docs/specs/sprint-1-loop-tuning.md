# Sprint 1: Loop Tuning

*Make first executions reliable before teaching the agent to remember them.*

**Status:** Spec
**Prerequisite:** Sprint 0 (Service Architecture)
**Depends on:** AgentExecutor running in AgentService with durable task state
**Estimated PRs:** 3-4
**Note on `ScreenFingerprint`:** Sprint 1 uses a minimal `ScreenFingerprint` type (structural hash + package name) for failure detection. Sprint 2 defines the full `ScreenFingerprint` data class with fuzzy matching, activity tracking, and class signatures. During implementation, Sprint 1 should define the minimal type; Sprint 2 extends it. Both share `structuralHash` as the common field.

---

## Problem

The agent loop has a ~50-70% success rate on complex multi-step tasks. When it works, it's impressive. When it doesn't, it fails in predictable ways:

1. **No decomposition.** "Book a flight to Tokyo" requires: open browser → navigate to flights → search → filter → select → fill details → confirm. The agent tries to do this as one flat loop, losing context as the conversation grows. By step 12, early observations are compacted away and the agent repeats steps or loses track of its progress.

2. **No recovery from failed actions.** A tap doesn't register → the screen doesn't change → the agent tries the same tap again → fails again → gets stuck. There's stuck detection (screen hash repetition), but it fires *after 3 failed attempts*, wasting 3 steps. And when it fires, the "try something different" message is vague — the agent doesn't have concrete fallback strategies.

3. **No regression testing.** We have unit tests for individual components but no automated way to verify that "send a text to Mom" still works after a refactor. End-to-end verification is manual: Joe runs it on the Pixel and watches.

These three problems compound: unreliable first executions → unreliable playbook recordings (Sprint 2) → unreliable replays → broken moat. The loop must be good before we teach it to remember.

---

## Solution

### 1. Subtask Decomposition

Enable the model to decompose complex goals into isolated sub-loops. This is a direct implementation of `agentic-loop-v2.md §6`.

### 2. Deterministic Recovery

Replace the vague "try something different" stuck detection with concrete, strategy-aware recovery behaviors.

### 3. Regression Harness

Automated task execution + verification framework for on-device testing.

---

## Design

### 1. Subtask Decomposition

#### 1.1 Tool Definition

```kotlin
val SUBTASK_TOOL = Tool(
    name = "subtask",
    description = """Decompose a complex goal into a focused sub-task.
        Use when a task has distinct phases that benefit from isolated context
        (e.g., "find info" then "compose message with that info").
        The sub-task runs in its own context and returns a structured result.
        For simple linear tasks (< 5 steps), just use regular tools directly.""",
    inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "goal" to mapOf(
                "type" to "string",
                "description" to "Clear description of what the sub-task should accomplish"
            ),
            "success_criteria" to mapOf(
                "type" to "string",
                "description" to "How to determine if the sub-task succeeded"
            ),
            "max_steps" to mapOf(
                "type" to "integer",
                "description" to "Maximum tool steps for this sub-task (default: 10)",
                "default" to 10
            )
        ),
        "required" to listOf("goal", "success_criteria")
    )
)
```

#### 1.2 Execution Model

When the orchestrator calls `subtask`:

```kotlin
/**
 * Execute a subtask in an isolated context.
 *
 * The subtask gets:
 * - Fresh conversation history (context isolation)
 * - The goal as the user message
 * - Success criteria in the system prompt
 * - Same model config as parent (inherits model floor)
 * - Shared ScreenReader (same physical screen)
 * - Its own step counter, capped at max_steps
 * - Parent's cancellation token (user cancel kills everything)
 *
 * Returns structured result to the parent.
 */
suspend fun executeSubtask(
    goal: String,
    successCriteria: String,
    maxSteps: Int,
    parentCancellationToken: CancellationToken,
    depth: Int
): SubtaskResult
```

```kotlin
data class SubtaskResult(
    val status: SubtaskStatus,
    val result: String,
    val stepsUsed: Int,
    val summary: String
)

enum class SubtaskStatus {
    SUCCESS,
    FAILED,
    PARTIAL,     // Made progress but didn't fully achieve goal
    CANCELLED,   // User cancelled
    TIMEOUT      // Hit wall-clock or step limit
}
```

#### 1.3 Depth and Resource Limits

- **Maximum recursion depth:** 3 levels (orchestrator → subtask → sub-subtask)
- **Step accounting:** Parent's step counter increments by 1 per subtask call (not by internal steps). This prevents subtasks from consuming the parent's budget.
- **Wall-clock timeout:** 60 seconds per subtask by default, configurable via `max_time_seconds` parameter on the subtask tool (range: 10-300). The model can request more time for known-slow operations (web loading, multi-screen navigation). Whichever fires first — step limit or timeout — wins.
- **Cancellation propagation:** Shared token. User cancel kills all levels.
- **Depth enforcement:** `executeSubtask()` checks `depth >= MAX_DEPTH` and returns FAILED immediately if exceeded.

#### 1.4 Context Isolation

The subtask gets a fresh `PhoneAgentApi` instance with empty conversation history. This is the key design choice — it prevents context pollution from the parent's long conversation.

The subtask's system prompt includes:
```
You are completing a specific sub-task as part of a larger goal.

SUB-TASK GOAL: {goal}
SUCCESS CRITERIA: {successCriteria}

Complete this focused task efficiently. When done, provide a clear result
that the orchestrating agent can use to continue the larger workflow.
```

#### 1.5 Subtask System Prompt Section

Add to the orchestrator's system prompt (when `subtask` tool is available):

```
TASK DECOMPOSITION:
For complex tasks with distinct phases, use the `subtask` tool to break them
into focused steps. Each subtask runs in isolated context — use it when:
- The task has 2+ distinct phases (find info → use info)
- The total task would exceed 8-10 steps as a flat loop
- Context from early steps would be lost by the time you need it

Do NOT decompose simple tasks (< 5 steps). Just use regular tools directly.

Example: "Send Sarah the weather forecast"
→ subtask 1: "Check the weather forecast for today" (success: current temperature and conditions)
→ subtask 2: "Send a text to Sarah with the forecast" (success: message sent)
```

---

### 2. Deterministic Recovery

#### 2.1 Recovery Strategy Interface

```kotlin
package ai.citros.core

/**
 * A concrete recovery strategy that can be attempted when an action fails.
 *
 * Unlike the current stuck detection (which injects a vague "try something
 * different" message after 3 failures), recovery strategies are specific
 * and actionable.
 */
interface RecoveryStrategy {
    /** Human-readable name for logging. */
    val name: String

    /**
     * Check if this strategy applies to the current failure.
     *
     * @param failure Description of what went wrong
     * @return true if this strategy might help
     */
    fun appliesTo(failure: ActionFailure): Boolean

    /**
     * Generate the recovery action(s) to try.
     *
     * @param failure The failure context
     * @return A list of tool calls to attempt, or null if recovery isn't possible
     */
    fun recover(failure: ActionFailure): List<RecoveryAction>?
}

/**
 * Structured description of an action failure.
 */
data class ActionFailure(
    val toolCall: ToolCall,
    val result: ToolResult,
    val screenBefore: ScreenFingerprint?,
    val screenAfter: ScreenFingerprint?,
    val consecutiveFailures: Int,
    val foregroundApp: String?,
    val failureType: FailureType
)

enum class FailureType {
    /** Action executed but screen didn't change (tap didn't register). */
    NO_EFFECT,
    /** Target element not found on screen. */
    TARGET_NOT_FOUND,
    /** Unexpected screen appeared (dialog, error, different app). */
    UNEXPECTED_STATE,
    /** Action returned an error. */
    TOOL_ERROR,
    /** Screen changed but not in the expected way. */
    WRONG_OUTCOME
}

/**
 * A concrete recovery action to attempt.
 */
data class RecoveryAction(
    val description: String,
    val toolName: String,
    val toolInput: Map<String, Any>
)
```

#### 2.2 Built-in Recovery Strategies

```kotlin
/**
 * When a tap doesn't register (NO_EFFECT), try alternative targeting:
 * 1. If original was coordinate tap → try tap_text with nearby text
 * 2. If original was tap_text → try coordinate tap on the element's location
 * 3. If both fail → scroll slightly and retry
 */
class TapRecoveryStrategy : RecoveryStrategy {
    override val name = "tap_recovery"

    override fun appliesTo(failure: ActionFailure): Boolean {
        return failure.failureType == FailureType.NO_EFFECT &&
            failure.toolCall.name in setOf("tap", "tap_text") &&
            failure.consecutiveFailures <= 3
    }

    override fun recover(failure: ActionFailure): List<RecoveryAction> {
        val actions = mutableListOf<RecoveryAction>()

        when (failure.toolCall.name) {
            "tap" -> {
                // Coordinate tap failed → try text-based tap
                val nearbyText = findNearbyText(failure)
                if (nearbyText != null) {
                    actions.add(RecoveryAction(
                        description = "Tap failed, trying text-based tap on '$nearbyText'",
                        toolName = "tap_text",
                        toolInput = mapOf("text" to nearbyText)
                    ))
                }
            }
            "tap_text" -> {
                // Text tap failed → try scroll then retry
                actions.add(RecoveryAction(
                    description = "Text not found, scrolling to find it",
                    toolName = "scroll",
                    toolInput = mapOf("direction" to "down")
                ))
            }
        }

        return actions.ifEmpty { null }
    }
}

/**
 * When an unexpected dialog appears (UNEXPECTED_STATE), try to dismiss it:
 * 1. press_back to close dialog
 * 2. Re-read screen
 * 3. If still wrong → tap common dismiss buttons ("OK", "Cancel", "Dismiss", "Not now")
 */
class DialogRecoveryStrategy : RecoveryStrategy {
    override val name = "dialog_recovery"

    override fun appliesTo(failure: ActionFailure): Boolean {
        return failure.failureType == FailureType.UNEXPECTED_STATE
    }

    override fun recover(failure: ActionFailure): List<RecoveryAction> {
        return listOf(
            RecoveryAction(
                description = "Unexpected dialog — pressing back to dismiss",
                toolName = "press_back",
                toolInput = emptyMap()
            )
        )
    }
}

/**
 * When the agent ends up in the wrong app (app crash, system dialog, etc.),
 * reset to a known state:
 * 1. press_home
 * 2. re-open the target app
 */
class AppResetRecoveryStrategy : RecoveryStrategy {
    override val name = "app_reset_recovery"

    override fun appliesTo(failure: ActionFailure): Boolean {
        return failure.failureType == FailureType.UNEXPECTED_STATE &&
            failure.foregroundApp != null &&
            failure.consecutiveFailures >= 2
    }

    override fun recover(failure: ActionFailure): List<RecoveryAction> {
        return listOf(
            RecoveryAction(
                description = "Wrong app state — resetting to home",
                toolName = "press_home",
                toolInput = emptyMap()
            )
        )
    }
}

/**
 * Graceful cancellation: when the agent is hopelessly stuck (5+ consecutive
 * failures), cleanly exit to home screen and report to user.
 */
class GracefulCancelStrategy : RecoveryStrategy {
    override val name = "graceful_cancel"

    override fun appliesTo(failure: ActionFailure): Boolean {
        return failure.consecutiveFailures >= 5
    }

    override fun recover(failure: ActionFailure): List<RecoveryAction> {
        return listOf(
            RecoveryAction(
                description = "Stuck after ${failure.consecutiveFailures} failures — returning to home",
                toolName = "press_home",
                toolInput = emptyMap()
            )
        )
    }
}
```

#### 2.3 RecoveryManager — Orchestration

```kotlin
package ai.citros.core

/**
 * Manages recovery strategy selection and execution.
 *
 * Plugged into AgentExecutor's tool loop after per-action verification.
 * When a tool execution results in a detectable failure, RecoveryManager
 * selects the appropriate strategy and generates recovery actions.
 *
 * Recovery actions are injected as model context (like steer messages),
 * not executed automatically. The model sees "Recovery: tap failed, trying
 * text-based tap on 'Send'" and decides whether to follow the suggestion
 * or try something else.
 */
class RecoveryManager(
    private val strategies: List<RecoveryStrategy> = listOf(
        TapRecoveryStrategy(),
        DialogRecoveryStrategy(),
        AppResetRecoveryStrategy(),
        GracefulCancelStrategy()
    )
) {
    /**
     * Evaluate a failure and return recovery guidance for the model.
     *
     * @return A string to inject into the tool result, or null if no recovery applies
     */
    fun evaluateFailure(failure: ActionFailure): String? {
        val applicable = strategies.filter { it.appliesTo(failure) }
        if (applicable.isEmpty()) return null

        val strategy = applicable.first()
        val actions = strategy.recover(failure) ?: return null

        return buildString {
            appendLine()
            appendLine("⚠️ RECOVERY (${strategy.name}):")
            for (action in actions) {
                appendLine("  → ${action.description}")
                appendLine("    Suggested: ${action.toolName}(${action.toolInput})")
            }
            appendLine("Follow the suggestion above, or try a different approach.")
        }
    }
}
```

#### 2.4 Integration with AgentExecutor

```kotlin
// In AgentExecutor tool loop, after tool execution and screen refresh:

val screenAfter = ScreenReader.getScreenContent()
val failure = detectFailure(toolCall, result, screenBefore, screenAfter, consecutiveFailures)

if (failure != null) {
    val recoveryGuidance = recoveryManager.evaluateFailure(failure)
    if (recoveryGuidance != null) {
        // Append recovery guidance to the tool result.
        // NOTE: Recovery guidance is ADVISORY — the model decides whether to
        // follow the suggestion or try a different approach. This is intentional:
        // the model stays in control of strategy, recovery just provides informed
        // suggestions based on failure patterns.
        val enrichedResult = result.text + recoveryGuidance
        delegate.addToolResult(toolCall.id, enrichedResult, toolCall.name, result.isError)
    }
    consecutiveFailures++
} else {
    consecutiveFailures = 0
}
```

#### 2.5 Failure Detection

```kotlin
/**
 * Detect whether a tool execution actually failed, even if the tool
 * returned "success."
 *
 * This is the per-action verification upgrade from the roadmap (H2.4),
 * now integrated with recovery strategies.
 */
/**
 * Pure function — all state passed in as parameters.
 * consecutiveFailures is tracked by the caller (AgentExecutor)
 * and passed in, not accessed from outer scope.
 */
fun detectFailure(
    toolCall: ToolCall,
    result: ToolResult,
    screenBefore: ScreenFingerprint?,
    screenAfter: ScreenFingerprint?,
    consecutiveFailures: Int,
    foregroundPackage: String? = screenAfter?.packageName
): ActionFailure? {
    // Explicit error from tool
    if (result.isError) {
        return ActionFailure(
            toolCall = toolCall,
            result = result,
            screenBefore = screenBefore,
            screenAfter = screenAfter,
            consecutiveFailures = consecutiveFailures + 1,
            foregroundApp = screenAfter?.packageName,
            failureType = FailureType.TOOL_ERROR
        )
    }

    // UI-mutating tool but screen didn't change
    if (toolCall.name in UI_MUTATING_TOOLS &&
        screenBefore != null && screenAfter != null &&
        screenBefore.structuralHash == screenAfter.structuralHash) {
        return ActionFailure(
            toolCall = toolCall,
            result = result,
            screenBefore = screenBefore,
            screenAfter = screenAfter,
            consecutiveFailures = consecutiveFailures + 1,
            foregroundApp = screenAfter.packageName,
            failureType = FailureType.NO_EFFECT
        )
    }

    // App changed unexpectedly (e.g., crashed to home, system dialog appeared)
    if (screenBefore?.packageName != null && screenAfter?.packageName != null &&
        screenBefore.packageName != screenAfter.packageName &&
        toolCall.name !in setOf("open_app", "press_home", "press_back")) {
        return ActionFailure(
            toolCall = toolCall,
            result = result,
            screenBefore = screenBefore,
            screenAfter = screenAfter,
            consecutiveFailures = consecutiveFailures + 1,
            foregroundApp = screenAfter.packageName,
            failureType = FailureType.UNEXPECTED_STATE
        )
    }

    return null  // No failure detected
}
```

---

### 3. Regression Harness (Skeleton)

#### 3.1 Design

An automated framework for running predefined tasks and verifying outcomes. This is the skeleton — full coverage is built up over time.

```kotlin
package ai.citros.test

/**
 * A regression test case for end-to-end agent task execution.
 *
 * Each test case defines:
 * - A user message (the task)
 * - Pre-conditions (expected starting screen state)
 * - Success criteria (how to verify the task worked)
 * - Maximum allowed steps
 * - Maximum wall-clock time
 */
data class RegressionTask(
    val id: String,
    val name: String,
    val userMessage: String,
    val preconditions: List<Precondition> = emptyList(),
    val successCriteria: List<SuccessCriterion>,
    val maxSteps: Int = 15,
    val maxTimeMs: Long = 60_000,
    val tags: Set<String> = emptySet()  // e.g., "messaging", "navigation", "search"
)

sealed class Precondition {
    /** Device must be on home screen. */
    object HomeScreen : Precondition()
    /** Specific app must be in foreground. */
    data class AppInForeground(val packageName: String) : Precondition()
}

sealed class SuccessCriterion {
    /** Agent completed within step limit. */
    object CompletedWithinSteps : SuccessCriterion()
    /** Specific app is in foreground after task. */
    data class AppInForeground(val packageName: String) : SuccessCriterion()
    /** Screen contains specific text. */
    data class ScreenContainsText(val text: String) : SuccessCriterion()
    /** Agent's final response contains specific text. */
    data class ResponseContains(val text: String) : SuccessCriterion()
    /** Agent used fewer than N steps. */
    data class StepsLessThan(val maxSteps: Int) : SuccessCriterion()
}
```

#### 3.2 Initial Test Suite

```kotlin
val REGRESSION_SUITE = listOf(
    RegressionTask(
        id = "nav-001",
        name = "Open Settings",
        userMessage = "Open Settings",
        preconditions = listOf(Precondition.HomeScreen),
        successCriteria = listOf(
            SuccessCriterion.AppInForeground("com.android.settings"),
            SuccessCriterion.StepsLessThan(3)
        ),
        tags = setOf("navigation", "simple")
    ),
    RegressionTask(
        id = "nav-002",
        name = "Open Gmail",
        userMessage = "Open Gmail",
        preconditions = listOf(Precondition.HomeScreen),
        successCriteria = listOf(
            SuccessCriterion.AppInForeground("com.google.android.gm"),
            SuccessCriterion.StepsLessThan(3)
        ),
        tags = setOf("navigation", "simple")
    ),
    RegressionTask(
        id = "info-001",
        name = "Weather query (conversational)",
        userMessage = "What's the weather like?",
        successCriteria = listOf(
            SuccessCriterion.CompletedWithinSteps,
            SuccessCriterion.ResponseContains("temperature")  // loose check
        ),
        tags = setOf("information", "conversational")
    ),
    RegressionTask(
        id = "multi-001",
        name = "Set a timer",
        userMessage = "Set a timer for 5 minutes",
        preconditions = listOf(Precondition.HomeScreen),
        successCriteria = listOf(
            SuccessCriterion.CompletedWithinSteps,
            SuccessCriterion.StepsLessThan(8)
        ),
        maxSteps = 12,
        tags = setOf("utility", "multi-step")
    ),
    RegressionTask(
        id = "msg-001",
        name = "Open Messages compose",
        userMessage = "Open Messages and start a new message",
        preconditions = listOf(Precondition.HomeScreen),
        successCriteria = listOf(
            SuccessCriterion.AppInForeground("com.google.android.apps.messaging"),
            SuccessCriterion.StepsLessThan(6)
        ),
        tags = setOf("messaging", "multi-step")
    )
)
```

#### 3.3 Runner

```kotlin
/**
 * Executes regression tasks and collects results.
 *
 * Designed to run on a real device (not emulator) via instrumentation test
 * or a standalone test activity. The runner:
 * 1. Ensures preconditions (navigate to home screen, etc.)
 * 2. Sends the user message to AgentService
 * 3. Waits for task completion
 * 4. Evaluates success criteria against actual outcome
 * 5. Records results (pass/fail, steps used, time taken, failure reason)
 */
class RegressionRunner(
    private val agentService: AgentService,
    private val screenReader: ScreenReader
) {
    suspend fun run(task: RegressionTask): RegressionResult {
        // 1. Enforce preconditions
        for (precondition in task.preconditions) {
            enforcePrecondition(precondition)
        }

        // 2. Execute task
        val startTime = System.currentTimeMillis()
        agentService.startTask(task.userMessage)

        // 3. Wait for completion (with timeout)
        val finalState = withTimeoutOrNull(task.maxTimeMs) {
            agentService.agentState.first { it is AgentState.Complete || it is AgentState.Failed }
        }

        val elapsed = System.currentTimeMillis() - startTime

        // 4. Evaluate criteria
        val screen = screenReader.getScreenContent()
        val criteriaResults = task.successCriteria.map { criterion ->
            evaluateCriterion(criterion, finalState, screen, task)
        }

        return RegressionResult(
            taskId = task.id,
            taskName = task.name,
            passed = criteriaResults.all { it.passed },
            criteriaResults = criteriaResults,
            stepsUsed = /* extract from finalState */,
            elapsedMs = elapsed,
            finalState = finalState
        )
    }
}
```

---

## Integration Points

### With Sprint 0 (Service Architecture)

- Subtask execution runs within `AgentService`'s coroutine scope, not the activity
- Recovery actions use the service's `AgentExecutor` infrastructure
- Regression harness talks to `AgentService` directly
- Durable task state includes subtask context (parent task ID, depth level)

### With Sprint 2 (Action Playbooks)

- Successful task executions (including subtask results) are candidates for playbook recording
- Recovery strategies inform playbook branching — if a recovery was needed, the playbook stores the successful path, not the failed one
- Subtask boundaries are natural playbook segmentation points

### With Existing Code

- `BoundaryCheck` interface — recovery integrates as a new check type (pre-next-step, not post-execution)
- `StuckDetector` — partially replaced by `detectFailure()` + `RecoveryManager`, but kept as a last-resort fallback
- `OutputClassifier` — subtask results classified as SHOW (user sees sub-task progress)
- `ContextCompactor` — subtask isolation reduces compaction pressure on the parent loop

---

## Test Matrix (Implementation PR Gate)

| Layer | Test ID | Scenario | Expected |
|-------|---------|----------|----------|
| Unit | L1 | Subtask tool definition schema | Valid tool schema, required fields present |
| Unit | L2 | Subtask execution with success | SubtaskResult.SUCCESS returned with result text |
| Unit | L3 | Subtask execution exceeding max_steps | SubtaskResult.TIMEOUT with steps_used = max_steps |
| Unit | L4 | Subtask execution exceeding wall-clock timeout | SubtaskResult.TIMEOUT |
| Unit | L5 | Subtask depth limit (depth 3 → immediate FAILED) | SubtaskResult.FAILED without executing |
| Unit | L6 | Cancellation propagation parent → subtask | Subtask stops, returns CANCELLED |
| Unit | L7 | Subtask context isolation (fresh history) | Subtask conversation starts empty |
| Unit | L8 | Parent step counter increments by 1 per subtask | Parent at step N before, N+1 after subtask |
| Unit | L9 | Failure detection: NO_EFFECT (screen unchanged after tap) | ActionFailure with type NO_EFFECT |
| Unit | L10 | Failure detection: UNEXPECTED_STATE (app changed) | ActionFailure with type UNEXPECTED_STATE |
| Unit | L11 | Failure detection: no failure (screen changed normally) | null returned |
| Unit | L12 | TapRecoveryStrategy: coordinate tap failed → suggests text tap | RecoveryAction with tap_text |
| Unit | L13 | DialogRecoveryStrategy: unexpected dialog → suggests press_back | RecoveryAction with press_back |
| Unit | L14 | GracefulCancelStrategy: 5+ failures → suggests press_home | RecoveryAction with press_home |
| Unit | L15 | RecoveryManager: no strategy applies → null | No recovery guidance |
| Unit | L16 | RecoveryManager: multiple strategies apply → first wins | First applicable strategy selected |
| Unit | L17 | Recovery guidance appended to tool result | Tool result contains ⚠️ RECOVERY section |
| Integration | L18 | Full subtask flow: parent → subtask → result → parent continues | End-to-end orchestration works |
| Integration | L19 | Nested subtask: parent → subtask → sub-subtask → result chain | Two-level nesting works |
| Integration | L20 | Recovery integration: tap fails → recovery suggestion → model retries | Model sees recovery guidance |
| Regression | L21 | "Open Settings" passes | nav-001 criteria met |
| Regression | L22 | "Open Gmail" passes | nav-002 criteria met |
| Regression | L23 | Regression runner produces structured report | JSON report with pass/fail per task |

---

## Blindspots

1. **Subtask model costs.** Each subtask is a new API conversation. A 3-subtask decomposition means 3x the API calls for context setup. This is the tradeoff for context isolation. Mitigation: the model decides when decomposition is worth it — simple tasks stay flat.

2. **Screen state between parent and subtask.** The subtask shares the physical screen but not the parent's screen history. If the subtask leaves the phone in an unexpected state (wrong app, dialog open), the parent has to handle it. Mitigation: subtask result includes final screen state summary; parent checks screen state after subtask returns.

3. **Recovery strategy ordering.** Currently first-match wins. If strategies have overlapping applicability, the order matters. This is fragile — consider priority scoring instead of ordered lists.

4. **False positive failure detection.** Some UI-mutating tools legitimately produce no screen change (e.g., `type_text` in a field that doesn't update the accessibility tree until submit, `wait` that legitimately waits). The `NO_EFFECT` detector needs an exclusion list for tools/contexts where no change is expected.

5. **Regression tests require real devices.** The harness can't run in CI without a connected device or emulator. For now it's a manual test suite that Joe (or the agent itself) runs on the Pixel. Automated CI integration is a future goal.

6. **Subtask + durable state interaction.** If the process dies during a subtask, the parent's `TaskState` (Sprint 0) includes `subtaskInProgress: Boolean` and `subtaskGoal: String?` fields. On recovery, the parent **retries the subtask step from scratch** — subtask state is NOT persisted independently. This is a normative decision, not a recommendation. Rationale: subtask context is isolated and lightweight (fresh conversation), so replaying from scratch is cheap. Persisting subtask state independently adds complexity (nested checkpoints) with minimal benefit since subtasks are designed to complete quickly (60s timeout).

7. **Playbook recording during subtask execution (Sprint 2 interaction).** When subtasks create new `PhoneAgentApi` instances, each instance should get its own `ExecutionRecorder` (Sprint 2). Subtask-level recordings produce subtask-level playbooks — the natural granularity for replay. The parent task does NOT get a monolithic playbook; it orchestrates subtask playbooks. This wiring must be addressed during Sprint 2 implementation.

8. **Recovery guidance is advisory, not imperative.** The `RecoveryManager` generates suggestions appended to tool results. The model may ignore them. This is intentional — the model stays in control of strategy. Recovery success rate depends on prompt quality and model compliance. If models consistently ignore recovery suggestions, a future iteration could make certain recovery actions automatic (e.g., always press_home after 5+ failures).

---

*Next: Sprint 2 (Action Playbooks) builds on reliable first executions.*
