# H1 PR 3: Steer — Mid-Loop Message Injection

## What

When a user sends a message while the agent is executing a tool loop, inject it at the next tool boundary so the agent can adjust behavior mid-task. ("No, I meant the Calendar app" / "Also CC alice@")

Currently, messages during a tool loop go to `queuedMessage` and are dispatched *after* the loop finishes. Steer injects them *during* the loop.

## Pressure Test vs OpenClaw (2026-02-16)

Dug through the actual OpenClaw source (`pi-coding-agent`, `pi-agent-core`, gateway reply pipeline). Three critical findings that changed the design:

### 1. Steer messages are USER messages, not tool result appendages

**OpenClaw**: `agent.steer()` injects a proper `{role: "user", content: [...]}` message into the conversation at the tool boundary. The model sees a first-class user turn.

**Our original design**: Used `CheckResult.Inject` to append text to the tool result string. The model would see `"Tool result: opened Gmail... ⚡ The user says: no, Calendar"` — mixed into tool output.

**Why it matters**: Models weigh user messages much more heavily than incidental text in tool results. For a phone agent, where steer = "stop doing the wrong thing RIGHT NOW", the signal needs to be unambiguous.

**Fix**: New `CheckResult.Steer(messages)` variant. AgentExecutor handles it specially — adds steer messages via delegate as user messages before the next API call.

### 2. Steer cancels remaining tool calls in the batch

**OpenClaw**: When steer fires, remaining tool calls in the current batch are **skipped**. If the model requested `[tap_text("Gmail"), type_text("hello"), tap("Send")]` and steer fires after the first tap, the type and send never execute.

**Our original design**: `Inject` doesn't break out of the inner for-each loop. All queued tool calls would still execute, potentially taking the phone further from where the user wants.

**Why it matters for phone agent**: If the user says "wrong app" but we still execute 2 more taps, the phone is now 2 screens deeper into the wrong app. The redirect becomes much harder.

**Fix**: `Steer` result breaks the inner for-each loop immediately. Only the current tool call's result is committed. Then steer messages are added and the loop continues with a fresh API call.

### 3. Steering mode: all vs one-at-a-time

**OpenClaw**: Configurable `steeringMode: "all" | "one-at-a-time"` controls whether multiple queued steer messages are delivered as one batch or one per turn.

**Our design**: All messages drain at once. For a single-user phone agent this is fine — unlikely to have multiple concurrent steers. Noted for future.

## Revised Design

### New: `CheckResult.Steer`

```kotlin
sealed class CheckResult {
    object Continue : CheckResult()
    data class Inject(val message: String) : CheckResult()
    data class Stop(val reason: String) : CheckResult()
    data class Steer(val userMessages: List<String>) : CheckResult()  // NEW
}
```

`Steer` semantics:
- **Skip remaining tool calls** in the current batch
- **Add user messages** to conversation via delegate (first-class user turns)
- **Continue the loop** — don't exit, get a fresh API response incorporating the redirect

Priority in `evaluateBoundaryChecks`:
- `Stop` still short-circuits everything
- `Steer` takes priority over `Inject` (user intent > system warnings)
- `Inject` still concatenates (stuck warnings can coexist within a tool call, but steer fires at the batch level)

### New: `SteerCheck` boundary check

```kotlin
class SteerCheck : BoundaryCheck {
    override fun check(state: LoopState): CheckResult {
        if (state.pendingSteerMessages.isEmpty()) return CheckResult.Continue
        return CheckResult.Steer(state.pendingSteerMessages)
    }
}
```

Stateless — reads from `LoopState`, which is an immutable snapshot.

### Modified: `LoopState`

Add one field:

```kotlin
data class LoopState(
    val step: Int,
    val maxSteps: Int,
    val lastToolName: String,
    val lastScreenHash: Int?,
    val isCancelled: Boolean,
    val pendingSteerMessages: List<String> = emptyList()  // NEW
)
```

Default `emptyList()` keeps all existing callers + tests unchanged.

### Modified: `AgentExecutor`

New constructor parameter:

```kotlin
class AgentExecutor(
    private val delegate: ToolExecutionDelegate,
    private val progressListener: LoopProgressListener,
    private val boundaryChecks: List<BoundaryCheck> = defaultBoundaryChecks(),
    private val maxToolSteps: Int = DEFAULT_MAX_TOOL_STEPS,
    private val steerMessageSource: () -> List<String> = { emptyList() }  // NEW
)
```

Updated loop logic — **two steer checkpoints** ensure steer works between thinking AND acting:

```kotlin
while (response != null && response.toolCalls.isNotEmpty()) {
    toolSteps++
    delegate.onStepStarted(toolSteps, maxToolSteps)

    // ======= PRE-BATCH STEER CHECK =======
    // Catches steers that arrived DURING the API call (model thinking).
    // This is the thinking→acting boundary — model decided what to do,
    // but nothing has executed yet. Steer here = zero wasted actions.
    val earlySteer = steerMessageSource()
    if (earlySteer.isNotEmpty()) {
        for (msg in earlySteer) delegate.addSteerMessage(msg)
        response = continueAfterTools()
        continue  // restart with model's new plan
    }

    for (toolCall in response.toolCalls) {
        // ... execute tool, format result ...

        // ======= POST-TOOL STEER CHECK (boundary checkpoint) =======
        // Catches steers that arrived DURING tool execution.
        // This is the acting→acting boundary — one tool finished,
        // next one hasn't started. Steer here = skip remaining batch.
        val steerMessages = steerMessageSource()
        val loopState = LoopState(
            step = toolSteps,
            maxSteps = maxToolSteps,
            lastToolName = toolCall.name,
            lastScreenHash = screenContent?.hashCode(),
            isCancelled = isCancelled(),
            pendingSteerMessages = steerMessages
        )
        val checkResult = evaluateBoundaryChecks(loopState)

        when (checkResult) {
            is CheckResult.Inject -> {
                delegate.addToolResult(toolCall.id, toolResult + checkResult.message)
            }
            is CheckResult.Steer -> {
                // Commit this tool's result as-is
                delegate.addToolResult(toolCall.id, toolResult)
                // Add steer messages as user messages
                for (msg in checkResult.userMessages) {
                    delegate.addSteerMessage(msg)
                }
                // BREAK — skip remaining tool calls in this batch
                break
            }
            is CheckResult.Stop -> {
                delegate.addToolResult(toolCall.id, toolResult)
                return LoopResult.Completed(null, toolSteps, checkResult.reason)
            }
            CheckResult.Continue -> {
                delegate.addToolResult(toolCall.id, toolResult)
            }
        }
    }

    // ... continueAfterTools() ...
}
```

**Two steer checkpoints, two boundaries:**

| Checkpoint | When | What it catches | Wasted actions |
|---|---|---|---|
| Pre-batch | After API returns, before any tool executes | Steers sent during model thinking | **Zero** |
| Post-tool (boundary check) | After each tool executes | Steers sent during tool execution | **One** (current tool) |

Updated `evaluateBoundaryChecks`:

```kotlin
private fun evaluateBoundaryChecks(state: LoopState): CheckResult {
    val injections = mutableListOf<String>()
    var steer: CheckResult.Steer? = null

    for (check in boundaryChecks) {
        when (val result = check.check(state)) {
            is CheckResult.Stop -> return result  // immediate exit
            is CheckResult.Steer -> steer = result  // steer takes priority over inject
            is CheckResult.Inject -> injections.add(result.message)
            CheckResult.Continue -> { /* no-op */ }
        }
    }

    // Steer > Inject (user intent overrides system warnings)
    if (steer != null) return steer
    return if (injections.isEmpty()) CheckResult.Continue
    else CheckResult.Inject(injections.joinToString(""))
}
```

Default boundary checks:

```kotlin
fun defaultBoundaryChecks(): List<BoundaryCheck> = listOf(
    CancellationCheck(),
    StepLimitCheck(),
    StuckDetectionCheck.withDefaults(),
    SteerCheck()  // NEW — after stuck detection
)
```

### New: `ToolExecutionDelegate.addSteerMessage()`

```kotlin
interface ToolExecutionDelegate {
    // ... existing methods ...

    /** Add a user steer message to conversation history. */
    fun addSteerMessage(text: String)
}
```

ChatViewModel implements this by adding a `Message(role = "user", content = text)` to the API conversation (if not already visible in chat UI, add there too).

### Modified: `ChatViewModel`

```kotlin
val queuedMessage = mutableStateOf<String?>(null)     // KEEP — for queue-after-loop
private val steerMessages = ConcurrentLinkedQueue<String>()  // NEW — for mid-loop injection

/** Atomically drain all pending steer messages. */
private fun drainSteerMessages(): List<String> {
    val messages = mutableListOf<String>()
    while (true) {
        val msg = steerMessages.poll() ?: break
        messages.add(msg)
    }
    return messages
}

fun sendMessage(content: String) {
    if (isLoading.value) {
        // Tool loop is active — steer instead of starting a new turn
        steerMessages.add(content)
        messages.add(Message(role = "user", content = content))  // Show in chat UI
        return
    }
    // ... existing flow
}

// ToolExecutionDelegate implementation:
override fun addSteerMessage(text: String) {
    // Add to API conversation history (PhoneAgentApi)
    phoneAgentApi?.addUserMessage(text)
}
```

Wire the source when constructing AgentExecutor:

```kotlin
val executor = AgentExecutor(
    delegate = this@ChatViewModel,
    progressListener = this@ChatViewModel,
    maxToolSteps = MAX_TOOL_STEPS,
    steerMessageSource = ::drainSteerMessages
)
```

### What about `queuedMessage`?

Keep it — it's for the Queue button (hold message for after loop). Steer is the *default* during a tool loop (user hits Send). Queue is explicit (user hits Queue button). Different intents, different mechanisms.

## Check Order & Priority

```
1. CancellationCheck  → Stop("cancelled")      — highest priority, immediate exit
2. StepLimitCheck     → Stop("max_steps")       — hard ceiling, immediate exit
3. StuckDetectionCheck → Inject("⚠️ STUCK...")  — appended to tool result
4. SteerCheck         → Steer(messages)         — breaks batch, adds user messages

Resolution:
- Stop short-circuits (remaining checks skipped)
- Steer beats Inject (user intent > system warning)
- Multiple Injects concatenate
```

## Files Changed

| File | Change |
|------|--------|
| `BoundaryCheck.kt` | Add `CheckResult.Steer`, `pendingSteerMessages` to `LoopState`, add `SteerCheck` |
| `AgentExecutor.kt` | Add `steerMessageSource` param, handle `Steer` in loop (break + user messages) |
| `ToolExecutionDelegate` | Add `addSteerMessage(text: String)` method |
| `ChatViewModel.kt` | Add `steerMessages` queue, `drainSteerMessages()`, steer-on-send, implement `addSteerMessage` |
| `BoundaryCheckTest.kt` | Tests for `SteerCheck`, `Steer` priority over `Inject` |
| `AgentExecutorTest.kt` | Test steer skips remaining tool calls, steer messages delivered as user messages |

## Bugs Found in Pressure Test (2026-02-16)

### Bug 1: Pre-batch `continueAfterTools()` has no error handling

The pre-batch steer calls `continueAfterTools()` without try/catch:
```kotlin
// BAD — API failure crashes the loop
response = continueAfterTools()
continue
```

Fix: wrap in the same try/catch as the main continuation:
```kotlin
response = try {
    continueAfterTools()
} catch (e: Exception) {
    ChatResponse(
        text = "Error: ${e.message?.take(80)}",
        toolCalls = emptyList(),
        stopReason = "error"
    )
}
continue
```

### Bug 2: Step counter increments on steer-only iterations

`toolSteps++` is at the top of the while loop. When pre-batch steer fires, no tools execute but a step is consumed. Three steers = 3 of 25 steps burned with zero work.

Fix: increment AFTER the pre-batch check:
```kotlin
while (response != null && response.toolCalls.isNotEmpty()) {
    // Pre-batch steer check (before step increment)
    val earlySteer = steerMessageSource()
    if (earlySteer.isNotEmpty()) { ... continue }

    toolSteps++  // Only increment when tools will actually execute
    delegate.onStepStarted(toolSteps, maxToolSteps)
    // ...
}
```

## Edge Case Decisions

| Case | Behavior | Rationale |
|------|----------|-----------|
| Unconsumed steers after loop exits | Dispatch as new `sendMessage()` turn | Dropping silently is worse — user thinks they communicated |
| Steer message format | Raw user text, no prefix | Model should treat it as a normal user message (OpenClaw does the same) |
| Steer + Cancel race | Cancel wins, steer goes to post-loop | User explicitly cancelled — steer is stale |
| Steer at step limit | Stop wins, steer goes to post-loop | Loop was ending anyway |
| Empty steer text | Filter in `sendMessage()`: `if (content.isNotBlank())` | Defensive — prevent injecting empty user messages |
| Rapid pre-batch steers | Each triggers an API call, rate-limited by latency | Not a loop risk; documenting is sufficient |

## Post-Loop Steer Drain

In ChatViewModel `finally` block, after existing `queuedMessage` drain:
```kotlin
val remainingSteer = drainSteerMessages()
if (remainingSteer.isNotEmpty() && !toolLoopCancelled.get()) {
    sendMessage(remainingSteer.joinToString("\n"))
}
```

## Test Plan

| Test | What it verifies |
|------|-----------------|
| Pre-batch steer: no tools execute | Steer at loop start → `addSteerMessage` called, no `executeToolCall` |
| Pre-batch steer: step counter not incremented | Steer fires → `toolSteps` unchanged |
| Pre-batch steer: API error handled | `continueAfterTools()` throws → graceful error response |
| Post-tool steer: remaining tools skipped | Batch of 3 tools, steer after 1st → tools 2+3 never execute |
| Post-tool steer: user messages added | `addSteerMessage` called with correct text |
| Steer + Cancel: cancel wins | Both present → `Stop("cancelled")`, steer unconsumed |
| Steer + Stop: stop wins | At max_steps with steer → `Stop("max_steps")`, steer unconsumed |
| Multiple steers drain atomically | 3 messages in queue → all 3 in one `Steer` result |
| Empty steer queue: no-op | No messages → `SteerCheck` returns `Continue` |
| Steer priority over Inject | Both Steer and Inject present → Steer wins |
| Unconsumed steer: post-loop dispatch | Loop ends without consuming → dispatched as new turn |

## What This Does NOT Include (deferred to PR 3.5)

- **Multiple steer coalescing** — if user sends 3 messages rapidly, all 3 get injected. Fine for now.
- **Model floor** — that's PR 4
- **Tool gating** — that's PR 5

---

## PR 3.5: Steer UI Enhancements

Separate PR after PR 3 merges. Pure UI/UX work — no core logic changes.

### Send button state during tool loop

When `isLoading` is true, the send button changes appearance to indicate steer mode:
- Different icon (e.g. arrow curving into the loop, or a lightning bolt ⚡)
- Subtle color shift or label change
- Communicates "this will redirect the agent" vs "this starts a new conversation"

### "Delivering..." feedback

After sending a steer message, brief inline indicator:
- Ephemeral chip/toast below the message: "Will be delivered at next step"
- Disappears once the boundary check consumes it
- Gives confidence the message didn't vanish into a void

### Queue vs Steer toggle

Explicit UX for choosing behavior during a tool loop:
- **Send (default)** → steer (inject mid-loop)
- **Long-press Send** or **Queue button** → hold for after loop finishes
- Clear visual distinction between the two paths

### Steer message styling in chat

Steered messages could look slightly different in the chat history:
- Subtle ⚡ badge or different bubble accent
- Distinguishes "I said this mid-task" from regular conversation turns
- Helps user understand the conversation flow when scrolling back
