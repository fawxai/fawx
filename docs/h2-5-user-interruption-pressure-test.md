# H2.5 User Interruption Detection — Pressure Test

> **Date:** 2026-02-21
> **Author:** Clawdio
> **Reference implementation:** OpenClaw steer system (pi-embedded-runner, reply module)
> **Citros baseline:** BoundaryCheck + SteerCheck (H1.2)

## 1. How OpenClaw Handles Mid-Task User Input

### Architecture

OpenClaw has a **queue mode system** with 5 modes for handling inbound messages while the agent is executing tools:

| Mode | Behavior |
|------|----------|
| `steer` | Inject message directly into active session via `activeSession.steer(text)`. Message becomes a user turn in conversation history. Model sees it on next API call. |
| `followup` | Queue message as a followup. Delivered after current run completes. |
| `collect` | Accumulate messages. Delivered as batch after run completes. |
| `steer-backlog` | Steer + queue any overflow for later delivery. |
| `interrupt` | Similar to followup but with different semantics for sub-agent announces. |

### Steer Flow (source: pi-embedded-CHb5giY2.js)

1. **Message arrives** while agent is streaming/executing tools
2. `queueEmbeddedPiMessage(sessionId, text)` checks:
   - Is there an active run for this session? (`ACTIVE_EMBEDDED_RUNS.get(sessionId)`)
   - Is the run currently streaming? (`handle.isStreaming()`)
   - Is the run compacting? (`handle.isCompacting()`) — if so, reject (can't steer during compaction)
3. If all checks pass: `handle.queueMessage(text)` → calls `activeSession.steer(text)`
4. The session's `steer()` method injects the text as a user message into conversation history
5. On the next API call, the model sees the user's message and can change course

### Key Design Decisions in OpenClaw

- **Steer is the default mode** — messages are injected immediately, not queued
- **Compaction blocks steer** — can't inject during context compaction (state is being rewritten)
- **No abort on steer** — the model gets to see the steer message and decide what to do. OpenClaw does NOT cancel the current tool execution or API call
- **Rate limiting on subagent steer** — prevents rapid-fire steer flooding
- **Steer restarts suppress announces** — when a session is steered, pending sub-agent announcements are suppressed to avoid confusion

## 2. How Citros Currently Handles Mid-Task User Input

### Architecture (H1.2 — BoundaryCheck + SteerCheck)

Citros already has a steer system built during H1:

1. **User types message** in chat UI during active tool loop
2. `ChatViewModel` adds message to a pending queue
3. `AgentExecutor` calls `steerMessageSource()` at every tool boundary
4. `SteerCheck` sees pending messages → returns `CheckResult.Steer(messages)`
5. Messages are injected as user turns via `ToolExecutionDelegate.addSteerMessage()`
6. Remaining tool calls in current batch are **skipped**
7. Loop continues with fresh API call — model sees user's steer message

### Early Steer (during API call)

Citros also handles messages that arrive DURING the API call (lines 162-202 of AgentExecutor.kt):
- After each API call returns, `steerMessageSource()` is drained
- If messages arrived during the call, they're delivered before tool execution begins
- The model gets a fresh API call with the steer messages visible

### What Citros Does NOT Have (the H2.5 gap)

The current steer system only handles **explicit text messages** typed by the user. It does NOT detect:

1. **User touching the screen** — the user taps/swipes while the agent is executing tools
2. **User switching apps** — the user presses Home or switches to another app
3. **Screen changes without agent action** — an incoming call, notification, or system dialog appears

These are all forms of **implicit interruption** — the user is taking control without typing a message.

## 3. Gap Analysis: Citros vs OpenClaw

| Aspect | OpenClaw | Citros (current) | Citros (H2.5 target) |
|--------|----------|-------------------|----------------------|
| Explicit text steer | \u2705 steer mode | \u2705 SteerCheck | \u2705 (already done) |
| Touch/gesture interrupt | N/A (no physical screen) | \u274c Not detected | \u2705 Detect + pause |
| App switch detection | N/A (no foreground concept) | \u274c Not detected | \u2705 Detect + pause |
| External interruption | N/A (no incoming calls) | \u274c Not detected | \u2705 Detect + pause |
| Steer during compaction | \u274c Blocked | N/A | Should also block |
| Resume after pause | N/A | N/A | \u2705 Resume protocol |
| Rate limiting | \u2705 On subagents | \u274c | \u2705 Debounce rapid events |

**Key insight:** OpenClaw's steer is entirely text-based because it operates over messaging channels. Citros needs to handle the same problem PLUS physical interaction detection — a harder problem with no direct reference implementation.

## 4. Design Proposal

### 4.1 Architecture: New BoundaryCheck — `UserInterruptionCheck`

Following the existing pattern, user interruption detection is a new `BoundaryCheck` that runs at tool boundaries.

```kotlin
class UserInterruptionCheck(
    private val interruptionSource: () -> InterruptionEvent?
) : BoundaryCheck {
    override suspend fun check(state: LoopState): CheckResult {
        val event = interruptionSource() ?: return CheckResult.Continue
        return when (event) {
            is InterruptionEvent.UserTouch -> CheckResult.Steer(
                listOf("[SYSTEM: User touched the screen during task execution. " +
                    "Pause and ask if they want you to continue or cancel.]")
            )
            is InterruptionEvent.AppSwitch -> CheckResult.Steer(
                listOf("[SYSTEM: User switched to ${event.newApp}. " +
                    "Pause and ask if they want you to continue or cancel.]")
            )
            is InterruptionEvent.ExternalInterrupt -> CheckResult.Steer(
                listOf("[SYSTEM: ${event.description}. " +
                    "Pause and ask if they want you to continue or cancel.]")
            )
        }
    }
}

sealed class InterruptionEvent {
    data class UserTouch(val x: Int, val y: Int) : InterruptionEvent()
    data class AppSwitch(val previousApp: String, val newApp: String) : InterruptionEvent()
    data class ExternalInterrupt(val description: String) : InterruptionEvent()
}
```

### 4.2 Detection Sources (Accessibility Service)

**User Touch Detection:**
- The accessibility service already receives `AccessibilityEvent` callbacks
- Currently `eventTypes = 0` (ghost input fix, PR #662) — we receive NO events
- For H2.5: selectively enable `TYPE_TOUCH_EXPLORATION_GESTURE_START` or `TYPE_VIEW_CLICKED` ONLY during agent execution
- Distinguish agent-injected events from user events: agent actions go through `AccessibilityService.performAction()` or `GestureDescription` — set a flag before each agent action, clear it after
- If an event arrives without the agent flag, it's a user touch

**App Switch Detection:**
- `TYPE_WINDOW_STATE_CHANGED` events fire when foreground app changes
- Track the "expected" foreground app (set when agent calls `open_app` or navigates)
- If foreground changes without a corresponding agent action → user switched apps

**External Interruption:**
- Incoming calls: `TYPE_WINDOW_STATE_CHANGED` with phone/dialer package
- System dialogs: `TYPE_WINDOW_STATE_CHANGED` with `android` system package
- These are app switches but with specific packages → classify as external

### 4.3 Debouncing

Rapid accessibility events are common (scroll generates dozens). Debounce strategy:
- **Cooldown period:** After detecting one interruption, ignore events for 500ms
- **Agent action window:** After each agent action, suppress events for 200ms (settle time)
- **First event wins:** Once an interruption is queued, further events are dropped until the model responds

### 4.4 Integration with Existing Steer

User interruption and text steer coexist:
- `SteerCheck` handles explicit text messages (existing)
- `UserInterruptionCheck` handles implicit touch/app-switch (new)
- Both produce `CheckResult.Steer` — the evaluation pipeline handles them identically
- If both fire simultaneously, messages are concatenated (user typed AND touched)

### 4.5 LoopState Extension

```kotlin
data class LoopState(
    // ... existing fields ...
    val pendingInterruption: InterruptionEvent? = null  // new
)
```

Backward compatible — defaults to null.

### 4.6 Resume Protocol

When the model receives an interruption steer:
1. It pauses and asks: "I was working on [task]. Want me to continue or cancel?"
2. User responds with text → processed as normal user message
3. If "continue" → model resumes from where it left off (full history preserved)
4. If "cancel" → model acknowledges, loop ends naturally (model returns text, no more tool calls)

No special resume mechanism needed — conversation history IS the state. The model has full context of what it was doing.

## 5. Pressure Test Results

### Critical (must fix before implementation)

1. **Ghost input regression risk** — PR #662 set `eventTypes = 0` to fix phantom typing. Re-enabling events must be surgical: ONLY during active tool loops, ONLY specific event types. A regression here would be severe.
   - **Mitigation:** Dynamic event type toggling via `ServiceInfo.eventTypes` update when loop starts/stops. Test with `eventTypes = TYPE_WINDOW_STATE_CHANGED` only (not `TYPES_ALL_MASK`).

2. **Agent event vs user event disambiguation** — This is the hardest problem. Android accessibility doesn't natively distinguish between programmatic and user-initiated events.
   - **Mitigation:** Flag-based approach (set flag before agent action, clear after). Timing-based fallback (events within 200ms of agent action are agent events). Both have edge cases — document known gaps.

### Deferred (file as issues)

3. **Steer during API call** — User touches screen while model is thinking (API call in flight). Current early-steer mechanism handles text messages here but not touch events. Touch events during API calls should be buffered and checked when the call returns.
   - **Issue:** File as sub-task of H2.5.

4. **Multi-touch disambiguation** — If the user accidentally touches the screen while holding the phone, we don't want false interruptions. Consider: interruptions only from deliberate taps (not edge touches or accidental contacts).
   - **Issue:** File as future enhancement. Start with all touches = interruption, refine based on real usage.

### Intentional Divergences from OpenClaw

5. **Steer injection vs loop pause** — OpenClaw injects steer text and lets the model decide. Citros does the same architecturally (uses `CheckResult.Steer`), but the *content* of the steer message asks the model to pause. This is an intentional UX decision: physical device control is higher stakes than text chat, so we default to "ask before continuing" rather than "silently incorporate."

6. **No compaction blocking** — OpenClaw blocks steer during compaction. Citros doesn't have mid-loop compaction (compaction happens between turns in `transformContext`). Not applicable.

## 6. Implementation Plan

### Phase 1: App Switch Detection (simplest, highest value)
1. Add `InterruptionEvent` sealed class
2. Add `UserInterruptionCheck` boundary check
3. Track foreground app in accessibility service, detect unexpected changes
4. Wire into `AgentExecutor` via `LoopState.pendingInterruption`
5. Tests: app switch detected, agent-initiated switch ignored, debouncing works

### Phase 2: User Touch Detection
1. Dynamic `eventTypes` toggling (enable during loop, disable after)
2. Agent action flagging to distinguish agent vs user events
3. Debounce rapid touch events
4. Tests: user tap detected, agent tap ignored, settle time suppression

### Phase 3: External Interruption
1. Classify specific packages (phone, system dialogs) as external interruptions
2. Different steer message for external vs user-initiated interruptions
3. Tests: incoming call pauses loop, system dialog pauses loop

---

*Sources: OpenClaw dist (pi-embedded-CHb5giY2.js, reply-B4B0jUCM.js, subagent-registry-DOZpiiys.js), Citros BoundaryCheck.kt, AgentExecutor.kt*
