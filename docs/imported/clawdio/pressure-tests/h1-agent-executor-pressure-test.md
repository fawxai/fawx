# Retroactive Pressure Test: AgentExecutor Loop vs OpenClaw Agent Loop

*Pressure test for #478 — Tier 1 retroactive audit*
*Citros: `AgentExecutor` (PR #476) | OpenClaw: `pi-agent-core` Agent + `agent-loop.ts` + `AgentSession`*

---

## 1. OpenClaw's Architecture (Source-Level)

OpenClaw's agent loop is split across three layers:

### Layer 1: `agent-loop.ts` (pi-agent-core) — The Pure Loop

The core loop is ~418 lines, framework-agnostic, with zero side effects beyond event emission.

**Entry points:**
- `agentLoop(prompts, context, config, signal, streamFn)` — new turn with user message(s)
- `agentLoopContinue(context, config, signal)` — resume from existing context (retries)

**Structure — dual nested loops:**

```
runLoop():
  OUTER while(true):                        ← follow-up message loop
    INNER while(hasMoreToolCalls || pendingMessages):  ← tool execution loop
      1. Inject pending messages (steer/follow-up) into context
      2. Stream assistant response (LLM call)
      3. If error/aborted → emit turn_end + agent_end → return
      4. Extract tool calls from response
      5. If has tool calls → executeToolCalls()
         - For each tool call:
           a. Validate args against schema
           b. Execute tool
           c. Emit tool_execution_start/end events
           d. After EACH tool: check getSteeringMessages()
           e. If steering found → skip remaining tools, break
         - Push all tool results to context
      6. Emit turn_end
      7. Check for more steering messages
    END INNER
    
    Check getFollowUpMessages()
    If follow-ups → set as pending, continue OUTER
    Else → break
  END OUTER
  
  Emit agent_end
```

**Key design decisions in the loop:**

1. **Event-driven, not return-value-driven**: The loop communicates via `EventStream<AgentEvent>`, not return values. Events include `agent_start`, `turn_start`, `message_start/update/end`, `tool_execution_start/update/end`, `turn_end`, `agent_end`. This enables streaming UI updates.

2. **Steering checks AFTER each individual tool call**: Inside `executeToolCalls()`, after each tool executes, `getSteeringMessages()` is polled. If messages exist, remaining tools in the batch are **skipped** with `"Skipped due to queued user message."` result.

3. **Two message queues with different semantics**:
   - `getSteeringMessages()` — checked after each tool call AND at end of inner loop. Interrupts tool execution.
   - `getFollowUpMessages()` — checked only when agent would otherwise stop (outer loop). Never interrupts tools.

4. **Context is mutated in-place**: `currentContext.messages.push(...)` — the context object is the source of truth, mutated as messages and tool results arrive.

5. **No step counter, no max steps**: The core loop has NO built-in step limit. It runs until the model stops requesting tools, an error occurs, or abort signal fires.

6. **No stuck detection in the loop**: Stuck detection, compaction, and guardrails live above this layer.

7. **`transformContext` runs before EVERY LLM call**: An optional hook that can prune/inject messages. This is where context trimming happens.

8. **`convertToLlm` runs after `transformContext`**: Converts `AgentMessage[]` (which can include custom types) to `Message[]` (LLM-compatible). Custom messages are filtered out.

### Layer 2: `Agent` class (pi-agent-core) — State Management

Wraps the loop with state management, queue handling, and lifecycle:

- **State**: `AgentState` holds systemPrompt, model, thinkingLevel, tools, messages, isStreaming, pendingToolCalls, error
- **Queues**: `steeringQueue` and `followUpQueue` with configurable drain modes (`"all"` or `"one-at-a-time"`)
- **`prompt()`**: Main entry — creates user message, calls `_runLoop()`
- **`steer()`**: Queues a steering message (delivered at next tool boundary)
- **`followUp()`**: Queues a follow-up message (delivered after agent finishes)
- **`abort()`**: Fires AbortController signal
- **`waitForIdle()`**: Returns a promise that resolves when the loop finishes
- **Error handling**: Catches errors from the loop, creates an error `AssistantMessage`, appends to state

### Layer 3: `AgentSession` (pi-coding-agent) — Application Layer

The OpenClaw-specific wrapper that adds:

- **System prompt building** via `buildSystemPrompt()` (modular sections)
- **Extension/plugin system**: `ExtensionRunner` wraps tools with before/after hooks
- **Compaction**: Auto-compaction on context overflow or threshold
- **Auto-retry**: Retries on transient errors (with configurable attempts)
- **Session management**: Save, load, switch, branch sessions
- **Slash commands**: `/model`, `/compact`, `/new`, etc.
- **Skill block parsing**: Lazy skill content injection

---

## 2. Citros's Architecture

### `AgentExecutor` — Single class, ~220 lines

Citros combines elements from all three OpenClaw layers into one class:

**Structure:**

```
run(initialResponse, screenContent, isCancelled, continueAfterTools):
  if no tools → return Completed("no_tools")
  
  while (response has tool calls):
    1. PRE-BATCH STEER: drain steerMessageSource()
       - If messages: deliver as user messages, check cancel, get new response, continue
    2. TOP-OF-LOOP CANCELLATION GUARD
    3. toolSteps++, delegate.onStepStarted()
    
    for (toolCall in response.toolCalls):
      a. [INLINE] Accessibility check for phone tools
      b. Execute tool via delegate
      c. Classify output visibility
      d. Settle delay
      e. Refresh screen for UI-mutating tools
      f. POST-TOOL BOUNDARY CHECK: drain steer, build LoopState, evaluateBoundaryChecks()
         - Stop → return
         - Steer → commit result, inject messages, break batch
         - Inject → append to tool result
         - Continue → commit result
    
    4. Between-step cancellation guard
    5. continueAfterTools() → next response
  
  return Completed(finalText, steps, exitReason)
```

---

## 3. Comparison

### 3.1 Loop Structure

| Aspect | OpenClaw | Citros | Assessment |
|--------|----------|--------|------------|
| Entry point | `agentLoop()` with user messages → builds first response internally | `run()` with **pre-built** initial response | **Gap**: Citros skips the "intake" phase. The LLM call that produces the initial response happens outside AgentExecutor, in ChatViewModel. This means the executor doesn't control the first API call or its error handling. |
| Outer loop | Explicit follow-up message loop | None — follow-up handled outside | **Intentional**: Single-user phone agent doesn't need the outer follow-up loop. ChatViewModel handles sequencing. |
| Inner loop | `while(hasMoreToolCalls \|\| pendingMessages)` | `while(response has tool calls)` + pre-batch steer check | **Equivalent**: Both continue while tools are requested. Citros's pre-batch steer effectively handles the `pendingMessages` case. |
| Step counting | None in core loop | Built-in `toolSteps` counter + `StepLimitCheck` | **Citros is more restrictive**: Phone agent needs hard limits (tools are real-world actions). OpenClaw delegates this to higher layers. |
| Tool execution | Sequential with per-tool steer check | Sequential with per-tool boundary check | **Equivalent pattern** |
| Batch skip on steer | Remaining tools get `"Skipped due to queued user message."` | Remaining tools simply not executed (break) | **Gap**: OpenClaw provides skip results for every tool call in the batch, maintaining API contract (every tool_use gets a tool_result). Citros breaks the loop — skipped tool calls have no result. **Need to verify**: Does the Anthropic API require results for all tool calls in a batch? If yes, this is a bug. |

### 3.2 Event System vs Return Values

| Aspect | OpenClaw | Citros |
|--------|----------|--------|
| Communication | `EventStream<AgentEvent>` with 12 event types | `LoopResult` return value + `LoopProgressListener` callbacks |
| Streaming | Events emitted during LLM streaming (`message_update`) | No streaming within executor — streaming happens in ChatViewModel |
| Granularity | Per-token streaming events, per-tool events, turn boundaries | Tool result events only |

**Assessment**: OpenClaw's event stream is designed for a CLI/web UI that needs real-time updates. Citros's simpler callback model is appropriate for a phone UI where the overlay shows minimal status. **Not a gap** — different requirements.

### 3.3 Message & Context Handling

| Aspect | OpenClaw | Citros |
|--------|----------|--------|
| Context mutation | In-place mutation of `currentContext.messages` | Delegate methods (`addToolResult`, `addSteerMessage`) |
| Message types | `AgentMessage` union (user, assistant, toolResult, custom) | Tool results as strings, steer as separate user messages |
| `transformContext` | Runs before every LLM call — pruning/injection point | No equivalent in executor — context management is external |
| `convertToLlm` | Filters non-LLM messages before API call | No equivalent — all messages assumed LLM-compatible |

**Assessment**: OpenClaw's `transformContext` hook is powerful — it's where context window management happens automatically. Citros has no equivalent inside the executor loop. Context trimming will need to be added for H1.4 (Smart Context Trimming). **Gap — deferred to H1.4**: The executor should have a pre-LLM-call hook for context transformation.

### 3.4 Steer Implementation

| Aspect | OpenClaw | Citros |
|--------|----------|--------|
| Steer delivery | After each tool execution via `getSteeringMessages()` | Two checkpoints: pre-batch + post-tool via `SteerCheck` |
| Steer as user messages | Yes — steering messages are `AgentMessage` with `role: "user"` | Yes — `addSteerMessage()` adds as user turn |
| Batch cancellation | Remaining tools get explicit skip results | Remaining tools silently dropped (break) |
| Steer modes | `"all"` or `"one-at-a-time"` (configurable drain) | Always drains all pending messages |
| Follow-up queue | Separate `getFollowUpMessages()` checked at outer loop boundary | No follow-up queue in executor |

**Assessment**: Pre-batch steer (Citros) is a good addition that OpenClaw doesn't have — it catches steers that arrive during the API call, before ANY tools execute (zero wasted actions). The missing skip results for batch cancellation is the only concrete gap.

### 3.5 Error Handling

| Aspect | OpenClaw | Citros |
|--------|----------|--------|
| Tool execution errors | Caught, result becomes error content (`isError: true`) | Caught, error message truncated to 100 chars |
| API call errors | Caught at stream level, `stopReason: "error"` | Pre-batch steer: explicit `api_error_after_steer`. End-of-loop: error becomes a `ChatResponse` with error text |
| Abort/cancel | `AbortController` signal propagated to LLM call | `isCancelled()` lambda checked at boundaries |
| Auto-retry | In `AgentSession` layer — configurable attempts with backoff | None |

**Assessment**: OpenClaw's auto-retry is valuable for transient errors (rate limits, 503s). Citros has no retry mechanism — errors surface to the user immediately. **Gap — deferred to H3**: Auto-retry should be added when model failover ships.

### 3.6 Lifecycle Phases

OpenClaw's full lifecycle (across all three layers):

```
1. INTAKE      — message arrives, queue routing (steer/followup/new prompt)
2. CONTEXT     — transformContext() prunes/injects, convertToLlm() filters
3. INFERENCE   — LLM call with streaming events
4. TOOL EXEC   — per-tool with steer checks between each
5. STREAMING   — real-time events to UI throughout
6. PERSISTENCE — session save after agent_end
7. COMPACTION  — auto-compact if context threshold exceeded
```

Citros's lifecycle:

```
1. (External)  — ChatViewModel receives message, builds prompt, makes first API call
2. TOOL EXEC   — AgentExecutor.run() handles tool loop with boundary checks
3. (External)  — ChatViewModel handles response display and state updates
```

**Assessment**: Citros's executor handles only phase 4 of OpenClaw's 7-phase lifecycle. Phases 1-3 and 5-7 live in ChatViewModel. This is fine for now — the executor boundary is at the right place. But as features grow (context trimming, compaction, retry), more logic will need to move into or adjacent to the executor.

---

## 4. Gaps Found

### Critical (must address before shipping more features)

1. **Skipped tool calls have no result on steer**
   - When steer fires mid-batch, Citros `break`s the for loop — remaining tool calls get no result
   - OpenClaw explicitly generates skip results: `"Skipped due to queued user message."`
   - **Risk**: Some LLM providers may reject the next request if tool_use blocks lack corresponding tool_result blocks
   - **Fix**: Add skip results for remaining tool calls after steer break
   - **Severity**: Could cause API errors on multi-tool batches with steer

### Deferred (file as issues)

2. **No `transformContext` hook in the executor** (H1.4)
   - Context trimming needs a pre-LLM-call injection point
   - Currently impossible without modifying the executor
   - File as prerequisite for H1.4 Smart Context Trimming

3. **No auto-retry on transient errors** (H3)
   - Rate limits, 503s, network errors all surface immediately to the user
   - OpenClaw has configurable retry with backoff in AgentSession
   - File as prerequisite for H3.1 Model Failover Chain

4. **First API call happens outside the executor** (architectural observation)
   - `AgentExecutor.run()` takes a pre-built `initialResponse`
   - This means the executor can't control retry, context transform, or error handling for the first call
   - Not critical now but limits future features (retry, streaming control)
   - Consider for a future executor API evolution

### Intentional Divergences (document and keep)

5. **No outer follow-up loop** — single-user phone agent; ChatViewModel sequences turns
6. **Return values instead of event stream** — phone UI doesn't need per-token streaming from executor; ChatViewModel handles display
7. **Built-in step limits** — phone tools are real-world actions (taps, swipes); hard limits prevent runaway behavior. OpenClaw delegates this to higher layers
8. **Built-in stuck detection** — same rationale as step limits; phone-specific concern baked into boundary checks
9. **Accessibility gating** — phone-specific; no OpenClaw equivalent

---

## 5. Recommendations

### Immediate (PR 5 or follow-up)

**Add skip results for steered-away tool calls:**
```kotlin
is CheckResult.Steer -> {
    delegate.addToolResult(toolCall.id, toolResult)
    // Skip remaining tool calls with explicit result
    for (i in (response.toolCalls.indexOf(toolCall) + 1) until response.toolCalls.size) {
        val skipped = response.toolCalls[i]
        delegate.addToolResult(skipped.id, "Skipped: user sent a new message.")
    }
    for (msg in checkResult.userMessages) {
        delegate.addSteerMessage(msg)
    }
    steered = true
    break
}
```

### H1.4 (Context Trimming)

**Add a `transformContext` equivalent:**
Either a lambda on AgentExecutor or a new delegate method called before `continueAfterTools()`.

### H3 (Failover)

**Add auto-retry with configurable backoff** around the `continueAfterTools()` call, similar to OpenClaw's `AgentSession._handleAutoRetry()`.

---

*Pressure test completed 2026-02-16*
*Reference: pi-agent-core `agent-loop.ts` (418 lines), `agent.ts` (560 lines), `types.ts` (195 lines); pi-coding-agent `agent-session.ts` (2865 lines) — all extracted from sourcemaps*
*Citros: `AgentExecutor.kt` (~220 lines), `BoundaryCheck.kt` (~170 lines)*
