# Steer UI Task Spec (H1 PR 3.5)

*For: Jarvis*
*Author: Clawdio*
*Date: 2026-02-16*
*Branch: `ui/steer-ui` from `feat/android-mvp`*
*PR target: `feat/android-mvp`*

---

## Context

Steer lets users send messages **while the agent is executing tools**. The infrastructure is fully built:
- `AgentExecutor` accepts a `steerMessageSource` lambda that drains queued messages at each tool boundary
- `SteerCheck` (a `BoundaryCheck`) fires when pending steer messages exist
- `CheckResult.Steer(userMessages)` skips remaining tool calls in the batch and injects user messages as first-class conversation turns
- `ToolExecutionDelegate.addSteerMessage(text)` adds messages to the API conversation
- `ChatViewModel` already has `queuedMessage`, `isLoading`, `addSteerMessage()`, and `setQueuedMessage()`

**What\'s missing is the UI wiring.** The `AgentExecutor` is currently created in `ChatViewModel.sendMessage()` without a `steerMessageSource` (defaults to empty). The `MessageInput` composable disables when `isLoading` is true. There\'s no visual feedback for queued/steered messages.

---

## Requirements

### 1. Wire steer queue to AgentExecutor

**File:** `ChatViewModel.kt` (shared file — keep changes minimal)

Add a `ConcurrentLinkedQueue<String>` for steer messages:

```kotlin
import java.util.concurrent.ConcurrentLinkedQueue

// Add near other state fields (around line 144)
private val steerQueue = ConcurrentLinkedQueue<String>()
```

Pass it as `steerMessageSource` when creating the executor (~line 580):

```kotlin
val executor = AgentExecutor(
    delegate = this@ChatViewModel,
    progressListener = this@ChatViewModel,
    maxToolSteps = MAX_TOOL_STEPS,
    steerMessageSource = {
        // Drain semantics: take all pending and clear
        val messages = mutableListOf<String>()
        while (true) {
            messages.add(steerQueue.poll() ?: break)
        }
        messages
    }
)
```

Clear the queue when a new `sendMessage()` starts (in the existing reset block around line 531):

```kotlin
steerQueue.clear()
```

### 2. Input behavior during tool loop

**File:** `ChatActivity.kt` — `MessageInput` composable

Currently: `enabled = !viewModel.isLoading.value` (input disabled during tool execution)

**New behavior:**
- Input field stays **enabled** during tool execution (so user can type)
- Send button changes behavior based on state:
  - **Not loading:** Send button sends normally (calls `viewModel.sendMessage(text)`)
  - **Loading (tool loop active):** Send button **steers** (calls `viewModel.steerMessage(text)`)
- Send button visual changes:
  - **Not loading + has text:** Filled send icon (current behavior)
  - **Loading + has text:** Different icon or color to indicate "steer" mode (suggestion: use `Icons.Filled.NorthEast` or change button color to a warning/accent tone)
  - **Loading + no text:** Show a stop/cancel button instead (`Icons.Filled.Stop`)

Add to `ChatViewModel.kt`:

```kotlin
/** Queue a steer message for mid-loop injection. */
fun steerMessage(text: String) {
    if (!isLoading.value) {
        // Not in a tool loop — treat as normal send
        sendMessage(text)
        return
    }
    // Add to steer queue for AgentExecutor to drain at next boundary
    steerQueue.offer(text)
    // Also add to visible messages so user sees their own message
    messages.add(Message(role = "user", content = text))
}
```

Update `MessageInput` signature and call site:

```kotlin
@Composable
internal fun MessageInput(
    onSend: (String) -> Unit,
    onSteer: (String) -> Unit,  // NEW
    onCancel: () -> Unit,       // NEW
    isLoading: Boolean,
    flavor: FawxFlavor = FawxFlavor.TANGERINE,
    placeholder: String = "Message Fawx..."
)
```

Call site (~line 1253):

```kotlin
MessageInput(
    onSend = { viewModel.sendMessage(it) },
    onSteer = { viewModel.steerMessage(it) },
    onCancel = { viewModel.cancelToolExecution() },
    isLoading = viewModel.isLoading.value,
    flavor = selectedFlavor,
    placeholder = if (viewModel.isLoading.value) {
        "Redirect the agent..."
    } else if (viewModel.accessibilityEnabled.value) {
        "Ask me to do something..."
    } else {
        "Message Fawx..."
    }
)
```

### 3. "Delivering..." feedback

**File:** `ChatActivity.kt`

When a steer message is sent, briefly show a subtle indicator below the message:

- Option A: Add a small text label below steer messages ("Redirecting...") that disappears when `isLoading` goes false
- Option B: Use the existing `LoadingIndicator` but change its text/animation when steers are pending

Keep it simple — even just changing the loading indicator text to "Redirecting..." when `steerQueue` is non-empty would work.

Expose queue state in ViewModel:

```kotlin
val hasQueuedSteer = mutableStateOf(false)

// Update in steerMessage():
fun steerMessage(text: String) {
    // ... existing code ...
    steerQueue.offer(text)
    hasQueuedSteer.value = true
    // ...
}

// Reset when steerQueue is drained (add to steerMessageSource lambda):
steerMessageSource = {
    val messages = mutableListOf<String>()
    while (true) {
        messages.add(steerQueue.poll() ?: break)
    }
    if (messages.isNotEmpty()) {
        // Reset UI indicator on main thread
        viewModelScope.launch { hasQueuedSteer.value = false }
    }
    messages
}
```

### 4. Steer message styling

**File:** `ChatActivity.kt` — `MessageBubble` / `PortedMessageBubble`

Steer messages are added as `Message(role = "user", ...)` — they look identical to normal user messages. Consider:

- Adding a small "redirected" label or icon on steer messages
- Slightly different background tint

To distinguish steers, add a field to `Message`:

```kotlin
// In Message.kt (coordinate with Clawdio since this is a shared file)
val isSteer: Boolean = false
```

Then in `steerMessage()`:

```kotlin
messages.add(Message(role = "user", content = text, isSteer = true))
```

**⚠️ IMPORTANT:** Adding a field to `Message.kt` is a shared file change. Ping Clawdio before modifying. The field must default to `false` and not break serialization.

---

## What NOT to Do

- **Don\'t modify `AgentExecutor.kt`** — the steer infrastructure is complete. You only need to pass the `steerMessageSource` lambda.
- **Don\'t modify `BoundaryCheck.kt`** — `SteerCheck` already works.
- **Don\'t modify `PhoneAgentApi.kt`** — `addSteerMessage()` is implemented.
- **Don\'t change the steer priority logic** (Stop > Steer > Inject) — that\'s architectural.
- **Don\'t add a queue-vs-steer toggle yet** — we may not need it. The current behavior (send during tool loop = steer, send while idle = normal) is intuitive. If user testing reveals confusion, we\'ll add a toggle later.

## Queue vs Steer: Simplified Decision

The original plan had a UI toggle for "queue" (wait until agent finishes) vs "steer" (interrupt now). After building the infrastructure, the simpler approach is:

- **During tool loop:** Send = steer (interrupt). This is what users intuitively expect.
- **Unconsumed steers** are already handled — `ChatViewModel.sendMessage()` dispatches them post-loop via `queuedMessage`.
- **If agent is in API call** (not tool loop): Message goes to `steerQueue`, gets picked up at the pre-batch checkpoint before any tools execute. Zero wasted actions.

No toggle needed for v1. Add later if users ask.

---

## Test Plan

### Unit Tests (ChatViewModelTest.kt)
1. `steerMessage adds to steerQueue when loading`
2. `steerMessage falls back to sendMessage when not loading`
3. `steerQueue is cleared on new sendMessage`
4. `steerMessageSource drains queue atomically`
5. `hasQueuedSteer updates correctly`
6. `steer messages appear in messages list with isSteer=true`

### Manual Tests
1. Start a multi-step tool task (e.g., "open Settings and find Bluetooth")
2. While tools are executing, type a new message and send
3. Verify: message appears in chat, agent redirects within 1-2 tool steps
4. Verify: loading indicator changes to "Redirecting..." briefly
5. Verify: send button shows steer icon during tool loop
6. Verify: pressing stop (no text) cancels tool loop
7. Verify: after agent finishes, input returns to normal send mode

---

## File Summary

| File | Changes | Ownership |
|------|---------|-----------|
| `ChatViewModel.kt` | Add `steerQueue`, `steerMessage()`, `hasQueuedSteer`, wire `steerMessageSource` | **Shared** — minimal changes |
| `ChatActivity.kt` | Update `MessageInput`, add steer indicators, update call site | **Jarvis owns** |
| `Message.kt` | Add `isSteer: Boolean = false` field | **Shared** — coordinate with Clawdio |
| `ChatViewModelTest.kt` | Add 6 steer UI tests | **Shared** |

---

## PR Checklist
- [ ] Branch: `ui/steer-ui` from latest `feat/android-mvp`
- [ ] All existing tests pass (don\'t break anything)
- [ ] New tests for steer UI behavior
- [ ] `@claude review this PR` on the PR
- [ ] Address ALL review items
- [ ] All CI green before tagging `@abbudjoe ready for merge`

---

## Architecture Reference

If you need to understand how steer flows through the system:

```
User types during tool loop
    ↓
MessageInput.onSteer(text)
    ↓
ChatViewModel.steerMessage(text)
    ↓ adds to steerQueue + messages list
    ↓
AgentExecutor (running on coroutine)
    ↓ at each tool boundary, calls steerMessageSource()
    ↓ steerMessageSource drains steerQueue
    ↓ populates LoopState.pendingSteerMessages
    ↓
SteerCheck.check(state)
    ↓ returns CheckResult.Steer(userMessages)
    ↓
AgentExecutor handles Steer:
    ↓ skips remaining tool calls in batch
    ↓ calls delegate.addSteerMessage(msg) for each
    ↓ calls continueAfterTools() for fresh API response
    ↓ loop continues with new direction
```

Pre-batch checkpoint (before ANY tools execute):
```
API returns with tool calls
    ↓
AgentExecutor.run() checks steerMessageSource() FIRST
    ↓ if steer messages exist: inject + skip ALL tools + re-call API
    ↓ if empty: proceed to execute tools normally
```
