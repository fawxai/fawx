# Kernel Yield Primitive — Unified Park/Wake for the Agentic Loop

**Status:** DRAFT
**Phase:** 5.5 / 6
**Author:** Clawdio + Joe (2026-03-15)

---

## Problem

The Fawx kernel has three distinct patterns that all need the same thing: "the loop has nothing to do until an external event occurs."

1. **Fleet workers** — parked between tasks, polling on a timer
2. **Permission prompts** — loop paused while awaiting user approval
3. **Human interrupt** (#1440) — agent mid-tool-chain, human message arrives, needs to pause-respond-resume

Each is solved with a bespoke mechanism:
- Fleet: `tokio::time::sleep` poll loop in `serve_fleet.rs`
- Permissions: `tokio::sync::oneshot` + `tokio::time::timeout` in `PermissionGateExecutor`
- Interrupt: Not yet implemented (filed as #1440)

This leads to duplicated patterns, inconsistent cancellation behavior, and no unified way to reason about "what is the loop waiting for?"

---

## Proposal: `LoopCommand::Yield`

Add a first-class yield primitive to the loop engine:

```rust
pub enum LoopCommand {
    // ... existing variants ...
    
    /// Park the loop until one of the wake conditions fires.
    /// The loop releases its LLM context and enters a low-resource state.
    /// On wake, the loop resumes from where it left off.
    Yield {
        wake_on: Vec<WakeCondition>,
        /// Maximum time to stay parked before auto-waking.
        timeout: Option<Duration>,
    },
}

pub enum WakeCondition {
    /// Wake when a message arrives on the input channel.
    UserMessage,
    /// Wake when a specific oneshot channel resolves.
    Channel(oneshot::Receiver<WakePayload>),
    /// Wake when a timer fires.
    Timer(Duration),
    /// Wake when the cancellation token is triggered.
    Cancellation,
    /// Wake when a fleet task is dispatched to this worker.
    TaskDispatch,
    /// Wake when a permission prompt is resolved.
    PermissionResolved(String), // prompt ID
}

pub struct WakePayload {
    pub reason: WakeReason,
    pub data: Option<serde_json::Value>,
}

pub enum WakeReason {
    UserMessage,
    PermissionResolved,
    TaskDispatched,
    TimerFired,
    Cancelled,
    Timeout,
}
```

---

## How It Works

### Park
1. Loop receives `LoopCommand::Yield` (from agent decision, tool executor, or external signal)
2. Loop saves current state (conversation history, tool context if mid-chain)
3. Loop enters `tokio::select!` on all wake conditions
4. Resource release: drops any held LLM streaming state, reduces memory footprint

### Wake
1. One of the conditions fires
2. Loop restores state
3. Injects a `WakePayload` as context for the next iteration
4. Resumes execution

### For fleet workers:
```rust
// Instead of polling:
loop {
    let task = poll_for_task().await;
    if task.is_none() {
        tokio::time::sleep(heartbeat_interval).await;
        continue;
    }
    execute(task).await;
}

// With yield:
loop {
    yield(wake_on: [TaskDispatch, Timer(heartbeat_interval)]);
    match wake_reason {
        TaskDispatched(task) => execute(task).await,
        TimerFired => send_heartbeat().await,
    }
}
```

### For permission prompts:
```rust
// Instead of oneshot + timeout inside executor:
let receiver = prompt_state.register(id, tool);
match tokio::time::timeout(300s, receiver).await { ... }

// With yield:
emit_permission_prompt(prompt);
yield(wake_on: [
    PermissionResolved(prompt_id),
    Timer(300s),
    Cancellation,
]);
match wake_reason {
    PermissionResolved => execute_tool(),
    TimerFired | Cancelled => deny(),
}
```

### For human interrupt:
```rust
// During a long tool chain, between tool rounds:
if has_pending_human_message() {
    save_tool_chain_state();
    yield(wake_on: [UserMessage]);
    // Process human message
    restore_tool_chain_state();
    continue;
}
```

---

## Implementation Plan

### Phase 1: Core primitive
- Add `LoopCommand::Yield` and `WakeCondition` to fx-kernel
- Implement `select!`-based wait in the loop engine
- State save/restore for mid-cycle parking
- Tests with mock wake conditions

### Phase 2: Refactor existing patterns
- Migrate `PermissionGateExecutor` to use yield instead of inline oneshot
- Migrate fleet worker to use yield instead of sleep loop
- Both should be transparent to callers

### Phase 3: Human interrupt
- Wire `LoopCommand::Yield` into the tool round loop
- Between tool rounds, check for human messages
- On human message: yield, process, resume

---

## Design Decisions

### Why in the kernel?
Yield is a scheduling primitive, not application logic. It needs access to:
- The loop's cancellation token
- The input channel
- The tool execution state (for save/restore)
- The event bus (for wake signals)

None of these are accessible from the loadable layer.

### Why not just use `tokio::select!` directly?
You can — and we do today. But each usage reinvents:
- Timeout handling
- Cancellation threading
- State management around the await point
- Logging/observability of what the loop is waiting for

A unified primitive gives us one place to add observability, one place to handle cancellation correctly, and one pattern for future features.

### What about multi-condition wakes?
The `Vec<WakeCondition>` supports multiple conditions. `tokio::select!` naturally handles this — the first condition that fires wins. The others are dropped.

### Resource release
When parked, the loop should:
- Not hold any LLM streaming connections
- Not hold any tool executor locks
- Minimize memory footprint
- Continue sending heartbeats (for fleet workers)

This means yield should happen at clean boundaries (between iterations, between tool rounds), not mid-stream.

---

## Open Questions

1. **Granularity:** Can yield happen mid-tool-round, or only between rounds? Mid-round requires saving partial tool results, which is complex.

2. **Observability:** Should parked state be visible via `/v1/status`? Yes — "loop parked, waiting for: [permission prompt, timer]" would be valuable for debugging.

3. **Nested yields:** Can a tool executor yield (e.g., permission prompt) while the loop is already in a yielded state? Probably not — yields should be mutually exclusive.

4. **Persistence:** Should yield state survive server restart? For fleet workers, yes (they re-park on startup). For permission prompts, no (they expire). This suggests yield state is not persisted — the wake conditions are re-established on restart.

---

## Relationship to OpenClaw's `sessions_yield`

This is directly inspired by OpenClaw's `sessions_yield` tool, which Clawdio uses for event-driven subagent orchestration. The pattern is identical:
- "I have nothing to do until an external event"
- "Release my resources"
- "Wake me when something happens"

The difference is that OpenClaw's yield operates at the session level (between turns), while Fawx's yield would operate at the loop level (within a turn, between iterations or tool rounds).

---

*This spec is a starting point for discussion. The implementation complexity is moderate — the core `select!` mechanism is straightforward, but state save/restore for mid-cycle parking requires careful design around the loop engine's ownership model.*
