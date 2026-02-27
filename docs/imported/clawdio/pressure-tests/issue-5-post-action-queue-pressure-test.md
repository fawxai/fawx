# Issue 5: Post-Action Queue & "Anything Else?" — Pressure Test

*Author: Clawdio*
*Date: 2026-02-18*
*Related: Issue #5 (message queueing not wired in full-screen chat)*

---

## Feature Summary

After the agent completes an action task (tool loop with `toolSteps > 0`), two behaviors should fire:

1. **Queue drain** — If a queued message exists, dispatch it automatically as a new turn
2. **Check-in** — If the queue is empty after the last turn resolves, inject a system message prompting the agent to ask "is that all?" / "anything else?"

Only applies when the agent **ran tools** (action tasks). Pure conversational turns don't trigger check-in.

---

## 1. Reference Implementation: OpenClaw

### How OpenClaw handles messages during an active run

OpenClaw has 5 queue modes, configured per-session:

| Mode | Behavior |
|------|----------|
| `steer` | Inject message into active run mid-stream (cancel pending tool calls at next boundary), ALSO enqueue as followup if steer fails |
| `followup` | Buffer message, dispatch as new turn after current run completes |
| `collect` | Buffer ALL messages, coalesce into single turn with header `[Queued messages while agent was busy]` |
| `steer-backlog` | Try steer first, also enqueue as followup (both mechanisms) |
| `interrupt` | Abort current run entirely, start new run with the new message |

Default mode: `steer` (aliased from `queue`/`queued`).

### OpenClaw's steer mechanism (source-level)

```
User message arrives while run is active
  ↓
queueEmbeddedPiMessage(sessionId, text)
  ↓ Gets active run handle from ACTIVE_EMBEDDED_RUNS map
  ↓ Checks handle.isStreaming() and !handle.isCompacting()
  ↓ Calls handle.queueMessage(text)
  ↓ handle.queueMessage = async (text) => activeSession.steer(text)
  ↓
activeSession.steer(text)
  ↓ [Anthropic SDK level — injects user message into streaming conversation]
```

Key detail: OpenClaw's steer operates at the **API streaming level**, not at tool boundaries. The Anthropic SDK's `activeSession.steer()` interrupts the current streaming response and injects a user turn. Fawx's `SteerCheck` at tool boundaries is a coarser-grained but functionally equivalent approach.

### OpenClaw's followup queue (source-level)

```
Run completes
  ↓
finalizeWithFollowup(value, queueKey, runFollowupTurn)
  ↓
scheduleFollowupDrain(key, runFollowup)
  ↓ async loop: while (queue.items.length > 0 || queue.droppedCount > 0)
  ↓   waitForQueueDebounce(queue)  // configurable debounce
  ↓   mode=collect: coalesce all items into single prompt
  ↓   mode=followup: dispatch one at a time
  ↓   runFollowup(item) → starts new run with queued prompt
  ↓ loop ends when queue empty
  ↓ if (queue.items.length === 0 && queue.droppedCount === 0)
  ↓   FOLLOWUP_QUEUES.delete(key)  // cleanup
```

### What OpenClaw does NOT do

- **No "anything else?" prompt** — OpenClaw has no built-in check-in after task completion. The agent just finishes and waits for the next user message.
- **No tool-loop-specific queueing** — OpenClaw's queue is at the session/run level, not tied to whether tools were used.
- **No post-completion system message injection** — The followup queue drains mechanically; there's no "the agent should ask a follow-up question" logic.

---

## 2. How Fawx Currently Works

### Three send paths (current state)

| Surface | During tool loop | After tool loop |
|---------|-----------------|-----------------|
| **Full-screen chat** | `steerMessage()` → mid-loop injection via `SteerCheck` | Nothing — no queue mechanism |
| **OverlayService** (old) | `OverlayAction.QueueMessage` → `setQueuedMessage()` — queued for AFTER loop only, no mid-loop visibility | `queuedMessage.value` dispatched via `sendMessage()` |
| **OverlayPortedScreen** (new) | `steerMessage()` → mid-loop injection ✅ | Clears `queuedMessage` on submit |

### Post-loop dispatch (ChatViewModel lines 857-864)

```kotlin
// Dispatch queued message only if loop was not cancelled
if (!toolLoopCancelled.get()) {
    val pending = queuedMessage.value?.takeIf { it.isNotBlank() }
    if (pending != null) {
        queuedMessage.value = null
        sendMessage(pending)
    }
}
```

This fires at tool loop completion in the `finally` block. It works, but:
- **Full-screen never populates `queuedMessage`** — steered messages go through `steerQueue`, not `queuedMessage`
- **OverlayService sets `queuedMessage` but doesn't steer** — the user's message is invisible to the agent during the loop
- **No "anything else?" behavior exists anywhere**

---

## 3. Gap Analysis

### Same as OpenClaw

| Feature | OpenClaw | Fawx |
|---------|----------|--------|
| Mid-run message injection | `activeSession.steer()` (API-level) | `SteerCheck` at tool boundaries |
| Message buffering during run | `enqueueFollowupRun()` | `queuedMessage` in ChatViewModel |
| Post-run queue drain | `scheduleFollowupDrain()` loop | `finally` block dispatches `queuedMessage` |
| Cancel active run | `abortEmbeddedPiRun()` | `cancelToolExecution()` |

### Different from OpenClaw (intentional)

| Feature | OpenClaw | Fawx | Why different |
|---------|----------|--------|---------------|
| Queue granularity | 5 modes, per-session config | Single mode (steer + post-loop queue) | Fawx has one user, one device — complexity not needed |
| Multi-message coalescing | `collect` mode batches all queued | Single `queuedMessage` (one at a time) | Phone UX is simpler — user sends one follow-up, not bursts |
| Debounce | Configurable per-channel | None | No multi-channel routing needed |
| Drop policy | `old`/`new`/`summarize` | Last write wins (new message overwrites `queuedMessage`) | Acceptable for single-user phone agent |

### Gaps to close

| Gap | Severity | Description |
|-----|----------|-------------|
| **Full-screen has no post-loop queue** | Medium | Steered messages are consumed mid-loop. If the user wants "do X next" (after completion), there's no mechanism in full-screen. |
| **OverlayService doesn't steer** | Medium | Messages from the old overlay are invisible to the agent during the loop. Inconsistent with OverlayPortedScreen. |
| **No "anything else?" check-in** | New feature | Neither OpenClaw nor Fawx has this. This is a Fawx-specific enhancement — makes sense for a phone agent that executes multi-step physical tasks. |
| **Queue only holds one message** | Low | If user sends multiple messages during a loop, only the last one survives in `queuedMessage`. Acceptable for v1. |

---

## 4. Design: Post-Action Check-In

### Approach: System message injection

After a tool loop completes with `toolSteps > 0`:

1. Check `queuedMessage` — if non-null, dispatch it (existing behavior)
2. After the dispatched turn completes (or if no queued message):
   - If the original task involved tool execution (`toolSteps > 0`)
   - And the queue is now empty
   - Inject a system message into the conversation: `[Task completed. The user may have follow-up requests. Briefly confirm completion and ask if they need anything else.]`
   - This message is a **system role message**, not visible in the UI
   - The agent sees it and naturally asks "anything else?" in its own voice

### Why system message over prompt modification

| Approach | Pros | Cons |
|----------|------|------|
| **System message (chosen)** | Only fires when relevant, doesn't bloat base prompt, stateless | Adds one message to context per task completion |
| **Prompt rule** | Always available, no injection needed | Fires even for non-action turns, wastes prompt space, harder to condition on `toolSteps > 0` |
| **Client-side UI prompt** | Zero token cost | Breaks agent voice/personality, feels mechanical |

### Implementation sketch

In `ChatViewModel.kt`, after the tool loop `finally` block:

```kotlin
// After tool loop completes
finally {
    // ... existing cleanup ...
    
    if (toolSteps > 0) {
        val pending = queuedMessage.value?.takeIf { it.isNotBlank() }
        if (pending != null && !toolLoopCancelled.get()) {
            // Queued follow-up exists and loop wasn't cancelled — dispatch it
            queuedMessage.value = null
            sendMessage(pending)
        } else {
            // No queued message (or cancelled) — prompt check-in
            val exitReason = when {
                toolLoopCancelled.get() -> ToolLoopExit.CANCELLED
                loopResult?.exitReason == "max_steps" -> ToolLoopExit.MAX_STEPS
                loopResult?.exitReason == "accessibility_lost" -> ToolLoopExit.ACCESSIBILITY_LOST
                else -> ToolLoopExit.COMPLETED
            }
            injectPostActionCheckIn(exitReason)
        }
    }
}
```

Exit reason enum and check-in method:

```kotlin
private enum class ToolLoopExit {
    COMPLETED,
    CANCELLED,
    MAX_STEPS,
    ACCESSIBILITY_LOST
}

private val CHECK_IN_MESSAGES = mapOf(
    ToolLoopExit.COMPLETED to
        "[System: Task completed. Briefly confirm what you did and ask if the user needs anything else. Keep it natural and concise.]",
    ToolLoopExit.CANCELLED to
        "[System: The user stopped the current task. Briefly acknowledge you stopped and ask what they'd like you to do instead.]",
    ToolLoopExit.MAX_STEPS to
        "[System: Task couldn't be completed within the step limit. Explain what you accomplished so far and ask how the user wants to proceed.]",
    ToolLoopExit.ACCESSIBILITY_LOST to
        "[System: Lost connection to the accessibility service during the task. Let the user know and ask if they want to retry or do something else.]"
)

private fun injectPostActionCheckIn(exitReason: ToolLoopExit) {
    // Skip if user already typed something new
    if (steerQueue.isNotEmpty() || queuedMessage.value?.isNotBlank() == true) return
    
    val prompt = CHECK_IN_MESSAGES[exitReason] ?: return
    val checkInMessage = Message(
        role = "system",
        content = prompt,
        isSystem = true  // don't render in UI
    )
    messages.add(checkInMessage)
    
    viewModelScope.launch {
        val response = sendMessageWithAgent(
            message = prompt,
            screenContent = null,
            isActionLoop = false
        )
        response?.text?.let { text ->
            messages.add(Message(role = "assistant", content = text))
            speakIfEnabled(text)
        }
    }
}
```

### Edge cases

| Case | Behavior |
|------|----------|
| Tool loop cancelled by user | Check-in with "what should I do instead?" prompt |
| Tool loop hit max_steps | Check-in with "couldn't finish, how to proceed?" prompt |
| Tool loop completed successfully | Check-in with "anything else?" prompt |
| Tool loop completed with 0 steps | No check-in (pure conversation, no action attempted) |
| Queued message dispatched → that turn also has tools | Chain: tools complete → check queue again → if empty, check-in |
| User sends new message before check-in fires | New message takes priority, check-in skipped |
| Local mode (no API) | Same behavior — `sendMessageWithAgent` handles both modes |

### Check-in message variants

The system message content varies based on how the tool loop ended:

| Exit reason | System message |
|-------------|---------------|
| **Successful completion** (`toolSteps > 0`, not cancelled) | `[System: Task completed. Briefly confirm what you did and ask if the user needs anything else. Keep it natural and concise.]` |
| **User cancelled** (`toolLoopCancelled = true`) | `[System: The user stopped the current task. Briefly acknowledge you stopped and ask what they'd like you to do instead.]` |
| **Step limit reached** (`exit_reason = max_steps`) | `[System: Task couldn't be completed within the step limit. Explain what you accomplished so far and ask how the user wants to proceed.]` |
| **Accessibility lost** (`exit_reason = accessibility_lost`) | `[System: Lost connection to the accessibility service during the task. Let the user know and ask if they want to retry or do something else.]` |

All four variants trigger a natural agent response in the agent's own voice. The only case with NO check-in is `toolSteps == 0` (pure conversational turn, no action was attempted).

### Queue chaining

The queued message → dispatch → check-in flow should be:

```
Tool loop ends (toolSteps > 0)
  ↓
How did it end?
  ├─ CANCELLED → inject check-in("what should I do instead?")
  ├─ MAX_STEPS → inject check-in("couldn't finish, how to proceed?")
  ├─ ACCESSIBILITY_LOST → inject check-in("lost connection, retry?")
  └─ COMPLETED →
      queuedMessage exists?
      ├─ YES → sendMessage(queued) → new tool loop starts
      │         ↓ (that loop completes)
      │         queuedMessage exists?
      │         ├─ YES → repeat
      │         └─ NO → inject check-in("anything else?")
      └─ NO → inject check-in("anything else?")
```

This naturally chains: if the user queued "also check bluetooth" during the first task, it runs. When bluetooth check completes, if nothing else is queued, the agent asks "anything else?"

---

## 5. Design: Fix OverlayService Inconsistency

### Problem

`OverlayService.kt` dispatches `OverlayAction.QueueMessage` which only calls `setQueuedMessage()` — no mid-loop steering.

`OverlayPortedScreen.kt` correctly routes to `steerMessage()` during loading.

### Fix

In `ChatActivity.kt`, the mediator for `OverlayAction.QueueMessage`:

```kotlin
// CURRENT (line 330-332):
is OverlayAction.QueueMessage -> {
    sharedChatViewModel.setQueuedMessage(action.text)
}

// FIXED:
is OverlayAction.QueueMessage -> {
    if (sharedChatViewModel.isLoading.value) {
        sharedChatViewModel.steerMessage(action.text)
    } else {
        sharedChatViewModel.sendMessage(action.text)
    }
}
```

This aligns the OverlayService path with OverlayPortedScreen and full-screen behavior.

**Note:** The `queuedMessage` field is still useful for `resumeExecution()` and for the OverlayPortedScreen's draft sync. Don't remove it — just don't use `setQueuedMessage()` as the submit path.

---

## 6. Design: Full-Screen Post-Loop Queue

### Problem

Full-screen sends go through `steerMessage()` during loading. Steered messages are consumed mid-loop at the next tool boundary. There's no way to say "do this AFTER you finish" — everything is "do this NOW."

### Assessment

For v1, this is **acceptable**. The steer mechanism is what users expect when they type during an active task — they want the agent to adjust immediately. The "do this after" use case is niche on mobile (users don't batch-plan phone tasks).

The `queuedMessage` mechanism exists and works for the OverlayService path. If we need full-screen post-loop queueing later, we could add a long-press on the send button to "queue instead of steer."

### Decision

**No change for v1.** Full-screen steers mid-loop. Post-loop check-in handles the "anything else?" prompt regardless. If the user steered mid-loop, the check-in still fires after everything resolves (since `toolSteps > 0`).

---

## 7. Summary of Changes

| Component | Change | Priority |
|-----------|--------|----------|
| `ChatViewModel.kt` | Add `injectPostActionCheckIn()` — system message after tool loop with `toolSteps > 0` and empty queue | P0 |
| `ChatViewModel.kt` | Queue chaining — after dispatched queue turn completes, re-check queue, then check-in | P0 |
| `ChatActivity.kt` | Fix `OverlayAction.QueueMessage` mediator to route through `steerMessage()` during loading | P0 |
| `Message.kt` | Add `isSystem: Boolean = false` field for check-in messages (don't render in UI) | P1 |
| Tests | Post-action check-in tests, queue chaining tests, overlay steer fix tests | P0 |

### What we're NOT building

- Multi-message coalescing (OpenClaw's `collect` mode) — overkill for single-user phone agent
- Configurable queue modes — one behavior fits all for Fawx v1
- Debounce/cap/drop policies — no multi-channel routing needed
- Full-screen "queue instead of steer" toggle — steer is the right default for phone UX

---

## 8. Pressure Test Results

### ✅ Aligned with OpenClaw

- **Steer at boundaries** — functionally equivalent to OpenClaw's `activeSession.steer()`, just at a coarser granularity (tool boundary vs API stream level). Both achieve the same outcome: user message redirects the agent.
- **Post-loop queue drain** — same pattern as `scheduleFollowupDrain()`. Mechanically identical: run completes → check queue → dispatch next.
- **Single-session concurrency=1** — both systems process one message at a time per session.

### ✅ Intentional divergences (documented)

- **No multi-mode queue** — Fawx doesn't need `collect`/`interrupt` modes. Single user, single device, one message at a time.
- **Single-message queue** — `queuedMessage` holds one string. OpenClaw's `FOLLOWUP_QUEUES` holds arrays. Acceptable for phone UX where users don't send bursts during tool loops.

### 🆕 Fawx-specific enhancement

- **Post-action check-in** — OpenClaw doesn't have this. It's a UX enhancement specific to Fawx's phone-agent context where the agent performs physical multi-step tasks. The system message approach is lightweight and preserves agent personality.

### ⚠️ Potential concern: check-in token cost

Each check-in adds ~1 API call (system message → agent response). For a session with many short tasks, this could add up. Mitigation: only fire check-in when `toolSteps > 0`, and consider a cooldown (e.g., don't check in if the last check-in was < 2 minutes ago).

### ⚠️ Potential concern: check-in interrupting user flow

If the user is already typing their next message when the check-in fires, they'll see an "anything else?" message followed by their own message. This is slightly awkward but not harmful — the agent will simply process their next message normally.

Mitigation: `injectPostActionCheckIn()` checks `steerQueue` and `queuedMessage` before firing. If the user already typed something, skip check-in and let their message take priority.

### ⚠️ Potential concern: cancelled loop check-in feels pushy

When the user stops a task, they might just want silence — not a "what should I do instead?" The counter-argument: on a phone agent, if you stop the agent you almost always want it to do something different. Silence after cancel = dead end.

Mitigation: the system message says "ask what they'd like you to do instead" not "insist on doing something." The agent should be concise — one line acknowledging the stop + a light prompt, not a paragraph.
