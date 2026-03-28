# Loop V2: Observe, Don't Declare

**Status:** Draft  
**Date:** 2026-02-28  
**Supersedes:** emit_intent-based loop (loop_engine.rs current)  
**Related issues:** #931 (emit_intent failures), #924 (verify loop), #930 (abort/steer), #933-936 (test failures)

---

## 1. Design Principles

### Observe, Don't Declare
The agent acts naturally (text responses, native tool calls). The kernel observes what happened and derives intent, friction, and learning signals after the fact. No custom wrapper tools. No forced schema declarations.

### Signals as a Side Channel
Every loop step emits structured signals. Signals flow to three consumers: TUI display (developer visibility), agent context (self-correction), and persistent storage (cross-session learning). Signals never block the main loop.

### Human-in-the-Loop
The user can interact during execution: abort, steer, or queue input. User input during the loop is a first-class signal that can redirect the next iteration.

### Recursive by Observation
The loop still recurses via Continue — but recursion is driven by observed signals (tool failure, partial results, user steering), not by comparing declared intent against actual outcomes.

---

## 1b. Doctrine Mapping

Loop V2 must preserve all invariants from DOCTRINE.md. This section maps each to
its enforcement point in the new architecture.

| Doctrine Invariant | Enforcement Point in Loop V2 |
|--------------------|------------------------------|
| **Single human owner** | Unchanged — identity context in PERCEIVE |
| **Kernel immutability** | Loop steps, gates, signal emission are kernel code. Model cannot modify them. Tools live in loadable layer via `ToolExecutor` trait. |
| **Tool sandbox** | DECIDE step: path jail, command allowlist, size limits applied per-call before execution. `ToolExecutor` enforces at execution time. Dual enforcement. |
| **N+2 nesting depth** | `run_cycle` tracks recursion depth. Max depth enforced in CONTINUE. Signal emitted when approaching limit. |
| **Typed subagent roles** | When Loop V2 is used for subagent execution (Implementer/Reviewer/Fixer), the role's permission set constrains which tools are available in PERCEIVE and which gates apply in DECIDE. Role permissions are loadable config, not model-controlled. |
| **No self-escalation** | The model cannot add tools, modify gates, or change its own role. Tool definitions are assembled by the kernel in PERCEIVE from loadable config. |
| **Outbound-only (except messaging)** | Network access gated in DECIDE — only approved outbound calls. No inbound listeners. |
| **Memory write protection** | Write operations (write_file, run_command with side effects) gated in DECIDE. Destructive action detection as pre-execution check. |
| **Untrusted input boundaries** | Signals injected into agent context (PERCEIVE) are kernel-generated, not user/model content. Subagent output, external content, and user-triggered signals are tagged as untrusted and cannot become system-level instructions. |
| **LLM provider channel security** | Unchanged — CompletionRequest construction in REASON. |

### Thinking Trace Privacy

Thinking traces may contain sensitive reasoning about user data. Controls:
- **Dev mode (default during build phase):** full traces captured, stored locally, shown in `/debug`
- **Prod mode:** traces are ephemeral (memory only during cycle), not persisted to disk
- **Never:** traces are never sent to external services, analytics, or cross-session storage without explicit opt-in
- Traces are redacted from any signal export or log shipping
- `/debug` command shows traces for the current session only — no cross-session trace access

---

## 2. Loop Architecture

```
User Input
    │
    ▼
┌─────────────────────────────────────────────────────┐
│  1. PERCEIVE                                        │
│                                                     │
│  Assemble context window:                           │
│    • System prompt (simple: "You are Fawx...")      │
│    • Conversation history (20-turn buffer)          │
│    • Tool definitions (real tools, native format)   │
│    • Previous turn signals (for self-correction)    │
│    • Budget snapshot                                │
│    • Pending user steering input (if any)           │
│                                                     │
│  Retroactive feedback signal:                        │
│    Analyze current user input against previous turn: │
│    rephrasing? steering? acceptance? Emit            │
│    SignalKind::UserFeedback for the PREVIOUS turn.   │
│                                                     │
│  Signals emitted:                                   │
│    context_window_tokens, history_depth,             │
│    tools_available, steering_injected,               │
│    user_input (length, complexity, references),      │
│    user_feedback (retroactive, for previous turn)    │
└────────────────────┬────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────┐
│  2. REASON  (LLM call)                              │
│                                                     │
│  Model receives real tools directly.                │
│  Responds naturally:                                │
│    • Text only → answering the user                 │
│    • Tool calls → needs to act                      │
│    • Both → answer + act (multi-tool supported)     │
│                                                     │
│  Thinking trace captured (Claude extended thinking, │
│  o-series reasoning tokens) as first-class signal.  │
│                                                     │
│  ──── CANCELLATION CHECK ────                       │
│  Between streaming chunks, check for:               │
│    • User abort → cancel stream immediately         │
│    • User steer → let response finish, inject       │
│      steer at next iteration (don't waste tokens    │
│      from abandoned partial responses)              │
│                                                     │
│  Signals emitted:                                   │
│    model, thinking_trace, response_type,             │
│    tools_called[], latency_ms, tokens_in,            │
│    tokens_out, stop_reason                           │
└────────────────────┬────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────┐
│  3. DECIDE  (kernel, no LLM)                        │
│                                                     │
│  Parse CompletionResponse:                          │
│    text only       → Decision::Respond              │
│    tool call(s)    → Decision::UseTools             │
│    empty           → Decision::Respond(fallback)    │
│                                                     │
│  PRE-EXECUTION GATES (before any tool runs):        │
│    ☐ Budget check (tokens, calls, cost, wall time)  │
│    ☐ Sandbox validation (path jail, size limits)    │
│    ☐ Command allowlist / blocklist                  │
│    ☐ Destructive action detection                   │
│    ☐ Rate limiting                                  │
│                                                     │
│  Each tool call individually approved or blocked.   │
│  Blocked calls produce error tool results.          │
│                                                     │
│  INTENT-PLAUSIBILITY CHECK (lightweight, no LLM):   │
│    Does the set of tools called make sense given     │
│    the user's message? Heuristic examples:           │
│    • User said "read/search" but model called write  │
│    • User said "list" but model called run_command   │
│    If implausible: emit Friction signal, do NOT      │
│    block (model may be correct). Log for analysis.   │
│                                                     │
│  Signals emitted:                                   │
│    decision_type, gates_applied[],                   │
│    approved_calls[], blocked_calls[],                │
│    budget_consumed, plausibility_check               │
│                                                     │
│  Decision-kind signals for gate boundary cases:     │
│    When an approved call is close to a gate          │
│    boundary (e.g., matched 3 of 5 destructive-      │
│    action heuristics but still passed), emit a       │
│    SignalKind::Decision with the gate scores.        │
│    This surfaces "almost blocked" calls for          │
│    analysis without blocking them.                   │
└────────────────────┬────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────┐
│  4. ACT                                             │
│                                                     │
│  For Decision::Respond:                             │
│    Pass text through directly. No synthesis needed. │
│                                                     │
│  For Decision::UseTools:                            │
│    Execute approved calls via ToolExecutor.          │
│    SEQUENTIAL execution in model-returned order.     │
│    No parallel execution (future optimization       │
│    requires safety analysis).                        │
│    Collect results: success/failure + output.        │
│    Respect command timeouts.                         │
│                                                     │
│  ──── CANCELLATION CHECK ────                       │
│  Abort: checked BETWEEN tool executions AND          │
│    DURING individual tool execution via              │
│    CancellationToken on subprocess/future.           │
│  Steer: checked BETWEEN tool executions only         │
│    (current tool completes, steer applies after).    │
│                                                     │
│  Signals emitted:                                   │
│    tool_results[{name, success, latency_ms,          │
│    output_bytes}], failures[], timeouts[]            │
└────────────────────┬────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────┐
│  5. SYNTHESIZE  (LLM call, conditional)             │
│                                                     │
│  Conditional execution:                              │
│    • Text-only response → SKIP (pass through)       │
│    • Tool calls only → RUN synthesis                 │
│    • Text + tool calls → SKIP if model text is      │
│      substantial (>50 chars). The model already      │
│      provided its answer alongside the tool calls.   │
│      Synthesizing would produce redundant output.    │
│                                                     │
│  Model receives:                                    │
│    • Original user question                         │
│    • Tool results (success + output)                │
│    • Synthesis instruction                          │
│    • Failed tool error messages (if any)            │
│                                                     │
│  Generates natural response using tool output.      │
│                                                     │
│  ──── CANCELLATION CHECK ────                       │
│                                                     │
│  Signals emitted:                                   │
│    synthesis_ran, latency_ms, tokens_out             │
└────────────────────┬────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────┐
│  6. VERIFY  (kernel, no LLM)                        │
│                                                     │
│  Observational checks only:                         │
│    • All tools succeeded? (boolean)                 │
│    • Response non-empty? (boolean)                  │
│    • Response references tool output? (heuristic)   │
│    • Any tool timeouts? (boolean)                   │
│    • Response is not a generic fallback? (check      │
│      against known fallback phrases like "I wasn't   │
│      able to process that")                          │
│                                                     │
│  Friction signal detection:                         │
│    • Tool failures (which tool, what error)         │
│    • Empty search results                           │
│    • Sandbox rejections (path jail, size limit)     │
│    • Repeated similar tool calls across iterations  │
│    • Budget approaching limits                      │
│                                                     │
│  NO declared-vs-actual comparison.                  │
│  NO expected outcome matching.                      │
│                                                     │
│  Signals emitted:                                   │
│    all_tools_succeeded, response_quality,            │
│    friction_signals[], quality_assessment            │
└────────────────────┬────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────┐
│  7. CONTINUE                                        │
│                                                     │
│  Decision tree (in precedence order):                │
│    User sent abort → Cancel immediately             │
│    Budget exhausted → BudgetExhausted               │
│    Safety limit reached → Error                     │
│    User sent steer → Continue with new input        │
│    All tools succeeded + response ok → Complete     │
│    Partial failure (some tools ok, some failed) →   │
│      Continue with ALL results (success + failure)  │
│      as context. Let model decide next action.      │
│      Do NOT retry all — successful calls may have   │
│      had side effects.                              │
│    All tools failed + retryable → Retry (max 1)     │
│    Tool blocked by gate → Complete with explanation  │
│                                                     │
│  Retry taxonomy (what counts as retryable):         │
│    ✅ Timeout → retryable                            │
│    ✅ Transient network error → retryable            │
│    ❌ Permission/sandbox block → not retryable       │
│    ❌ File not found → not retryable                 │
│    ❌ Parse/validation error → not retryable         │
│    ❌ Deterministic user error → not retryable       │
│                                                     │
│  On Continue: loop back to Perceive with:           │
│    • Tool results as context (success AND failure)  │
│    • Friction signals from Verify                   │
│    • User steering input (if any)                   │
│                                                     │
│  Signals emitted:                                   │
│    continuation_decision, total_iterations,          │
│    total_latency_ms, total_tokens                    │
└─────────────────────────────────────────────────────┘
```

---

## 3. Signal Architecture

### Signal Struct

```rust
/// A structured observation emitted by a loop step.
struct Signal {
    /// Which loop step produced this signal.
    step: LoopStep,
    /// Signal category.
    kind: SignalKind,
    /// Human-readable description.
    message: String,
    /// Structured metadata (step-specific).
    metadata: serde_json::Value,
    /// Timestamp.
    timestamp_ms: u64,
}

enum LoopStep {
    Perceive,
    Reason,
    Decide,
    Act,
    Synthesize,
    Verify,
    Continue,
}

enum SignalKind {
    /// Normal operational signal.
    Trace,
    /// Model's internal reasoning (thinking tokens).
    Thinking,
    /// Something that might indicate a problem.
    Friction,
    /// Something that went well.
    Success,
    /// A gate blocked an action.
    Blocked,
    /// Performance/efficiency observation.
    Performance,
    /// User interaction during loop (steer/stop/wait).
    UserIntervention,
    /// User input characteristics (length, complexity, rephrasing detection).
    UserInput,
    /// Retroactive user feedback signal — computed at the START of the next
    /// turn by observing what the user did after our response:
    ///   - Moved to new topic → implicit positive
    ///   - Rephrased same question → friction (implicit negative)
    ///   - Said "no" / steered → explicit negative
    ///   - Said "thanks" / "perfect" → explicit positive
    /// This is a delayed signal — it belongs to the PREVIOUS turn but is
    /// emitted during the current turn's Perceive step.
    UserFeedback,
    /// Kernel decision signal (gate results, continuation decisions).
    Decision,
}
```

### Signal Collector

```rust
/// Accumulates signals for a single loop cycle.
struct SignalCollector {
    /// All signals from this cycle, in order.
    signals: Vec<Signal>,
    /// Thinking trace (concatenated from Reason step).
    thinking_trace: Option<String>,
    /// Friction signals only (for quick access).
    friction: Vec<Signal>,
    /// Hard cap on signals per cycle (prevents runaway accumulation).
    max_signals: usize, // default: 200
}

impl SignalCollector {
    /// Emit a signal. Drops silently if at capacity (signals never block).
    fn emit(&mut self, signal: Signal);

    /// Drain signals by kind (for consumers that only care about friction, etc.)
    fn drain_by_kind(&mut self, kind: SignalKind) -> Vec<Signal>;

    /// Iterator over all signals (read-only, for shell queries).
    fn iter(&self) -> impl Iterator<Item = &Signal>;

    /// Condensed summary for TUI display (max 5 lines).
    fn summary(&self) -> String;

    /// Full debug dump (all signals, raw).
    fn debug_dump(&self) -> String;
}
```

### Signal Transport (Kernel → Shell)

Signals flow through the existing shell-engine boundary, NOT through direct struct
access. The `LoopStatus` struct (already returned by `run_cycle`) is extended with
a `signals` field:

```rust
struct LoopStatus {
    // ... existing fields ...
    /// Signals from the last cycle. Shell reads these to populate /signals, /debug.
    signals: Vec<Signal>,
    /// Condensed signal summary for after-response display.
    signal_summary: String,
}
```

This preserves DOCTRINE.md's shell-engine boundary: shells query signals through
the return value, never by reaching into kernel internals. Future UISpec trait
can formalize this as `fn signals(&self) -> &[Signal]`.


### Signal Consumers

```
Signal Collector
       │
       ├──→ TUI Display
       │      • After-response summary (condensed)
       │      • /signals command (full last-turn trace)
       │      • /debug command (all signals, raw)
       │      • Real-time spinner (thinking trace excerpts)
       │
       ├──→ Agent Context (self-correction)
       │      • Previous turn friction → injected into Perceive
       │      • "Last tool call to write_file failed: path
       │        outside working directory"
       │      • Enables model to self-correct without retry loop
       │
       ├──→ Structured Log (file)
       │      • JSONL format, one line per signal
       │      • Rotated daily, retained for analysis
       │      • Dev mode: verbose. Prod mode: friction only.
       │
### Signal Policies

**Buffering/drop:** SignalCollector has a hard cap (200 signals/cycle). If exceeded,
oldest `Trace` and `Performance` signals are dropped first. `Friction`, `Blocked`,
`UserIntervention`, and `Decision` signals are preferentially retained — they are
only dropped as a last resort when the buffer is full of exclusively high-priority
signals (in which case newest high-priority signals are dropped). In practice this
cap is generous; a 10-iteration cycle with 5 signals/step produces ~50 signals.
Signals never block the main loop — `emit()` is fire-and-forget.

**Context injection cap:** When injecting previous-turn signals into PERCEIVE for
self-correction, max 200 tokens of signal context. Only friction signals from the
immediately previous turn. No cross-session signal injection. This prevents signals
from becoming a context-window bloat vector.

**Structured log retention:** Dev mode logs all signals as JSONL, rotated daily,
7-day retention. Prod mode logs friction signals only, 30-day retention. Thinking
traces are NEVER written to persistent logs (privacy).

       └──→ Future: Async LLM Judge
              • Factory-style cross-session analysis
              • Pattern clustering, facet evolution
              • Self-filing issues on friction thresholds
```

### Thinking Trace Capture

Models that produce thinking traces:
- **Claude 4.6 (adaptive thinking):** `thinking` content blocks, interleaved between tool calls
- **Claude 4.5 (extended thinking):** `thinking` content blocks, single block before response
- **gpt-5.3-codex:** reasoning traces at configured effort level
- **o-series (OpenAI):** reasoning tokens (not always exposed to API consumers)
- **Other models:** Not available — signal is simply absent

The thinking trace is:
1. Captured during streaming (chunk by chunk, including interleaved blocks)
2. Displayed in the TUI spinner in real-time (already happening — those "thinking..." messages)
3. Stored as a `SignalKind::Thinking` signal (one per thinking block, preserving interleave order)
4. Available via `/signals` and `/debug` after the turn
5. NOT sent back to the model (it's the model's own output — no echo)

With Claude 4.6 adaptive + interleaved thinking, we get thinking traces BETWEEN tool
calls. This means the signal trace shows the model's reasoning about each tool result
before deciding what to do next — invaluable for debugging multi-tool turns.

---

## 4. User Interaction During Loop

### Commands

During loop execution (spinner phase), bare words are commands — no slash needed.
At the prompt (not executing), these are treated as normal user input.

| Input during execution | Effect |
|------------------------|--------|
| `Ctrl+C` / `stop` / `abort` | Cancel current cycle. Return partial response if available. Emit `UserIntervention` signal. |
| `no` | Cancel + implicit negative feedback signal for the current approach. |
| `wait` / `pause` | Pause after current iteration. Resume with Enter or steer with new input. |
| Any other text | Steer input — injected as context for the next iteration. Current iteration completes, then Continue uses the steering input instead of sub-goal. |

Slash variants (`/stop`, `/steer`, `/wait`) also work for consistency, but bare words
are the primary interface during execution. Reducing friction when the user is frustrated
(watching a runaway loop) is the design priority.

### Implementation

```rust
/// Channel for user input during loop execution.
struct LoopInputChannel {
    /// Receiver for user commands/input during execution.
    receiver: tokio::sync::mpsc::Receiver<LoopInput>,
}

enum LoopInput {
    /// User wants to cancel ("stop", "abort", Ctrl+C).
    Abort,
    /// User wants to cancel with negative feedback ("no").
    AbortNegative,
    /// User wants to redirect the next iteration (any other text).
    Steer(String),
    /// User wants to pause ("wait", "pause").
    Wait,
}
```

Note: no `Queued` variant. During execution, ALL user input is interpreted as a
loop control signal (abort, steer, or wait). If the user types a full message
that isn't a control word, it becomes a steer signal — which is the right behavior.
After the loop completes, normal readline input resumes.

The TUI spawns input reading on a separate task. During loop execution, keystrokes are routed to the `LoopInputChannel` instead of the main readline. The loop engine checks the channel at cancellation points (marked in the architecture diagram with `── CANCELLATION CHECK ──`).

### Cancellation Semantics by Step

| Step | Abort behavior | Steer behavior |
|------|---------------|----------------|
| **REASON** (streaming) | Cancel the stream. Count tokens from chunks received so far. Return partial thinking trace. | Let the current response finish (option A — avoids wasting tokens from abandoned partial responses). Inject steer in next iteration. |
| **DECIDE** | Immediate. No partial state. | Immediate. Overrides continuation. |
| **ACT** (tool execution) | Cancel current subprocess via `CancellationToken`. Return results for completed tools + "cancelled" for the interrupted one. | Wait for current tool to complete. Inject steer for next iteration. |
| **SYNTHESIZE** (streaming) | Cancel the stream. Return raw tool results directly (they're already available). Do NOT attempt to use partial synthesis text. | Let synthesis finish. Steer applies to next turn, not this one. |
| **VERIFY/CONTINUE** | Immediate. | Immediate override of continuation decision. |

### Event Precedence

When multiple inputs arrive during a single iteration:

```
Abort > AbortNegative > Wait > Steer
```

First-write-wins: the first control input received is authoritative. Subsequent
inputs in the same iteration are discarded (with a `UserIntervention` signal
noting the discard). Rationale: if the user hit Ctrl+C and then typed "try
something else", the Ctrl+C takes priority.

### Readline Handoff

When the loop completes and control returns to the readline prompt:
1. `LoopInputChannel` is drained
2. Any remaining queued keystrokes are replayed into the readline buffer
3. No keystrokes are dropped during the transition
4. The input task switches from routing to `LoopInputChannel` back to the
   main readline receiver

### ToolExecutor Cancellation Contract

```rust
trait ToolExecutor: Send + Sync + Debug {
    /// Execute tool calls with optional cancellation.
    fn execute(
        &self,
        calls: &[ToolCall],
        cancel: Option<CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError>;
}
```

For `run_command`: the subprocess is killed via the token. Returns partial stdout
captured so far + a `ToolResult { success: false, output: "cancelled by user" }`.

For other tools (read_file, write_file, etc.): execution is fast enough that
cancellation between tools is sufficient. No per-tool cancellation needed.

---

## 5. Model-Specific Thinking Configuration

### Thinking Modes by Provider

| Model | Thinking type | Configuration | Effort levels |
|-------|--------------|---------------|---------------|
| Claude Opus 4.6 (Anthropic) | **Adaptive** (recommended) | `thinking: { type: "adaptive" }` + `output_config: { effort: "<level>" }` | `max`, `high` (default), `medium`, `low` |
| Claude Sonnet 4.6 (Anthropic) | **Adaptive** (recommended) | `thinking: { type: "adaptive" }` + `output_config: { effort: "<level>" }` | `high` (default), `medium`, `low` |
| Claude Opus 4.5 / Sonnet 4.5 | **Extended** (legacy) | `thinking: { type: "enabled", budget_tokens: N }` | N/A (budget-based) |
| Claude via OpenRouter | Adaptive or Extended | Same as direct, passed through | Same |
| gpt-5.3-codex (OpenAI) | **Codex thinking** | `reasoning: { effort: "<level>" }` | `off`, `low`, `medium`, `high`, `xhigh` |
| o4-mini / o-series (OpenAI) | **Reasoning tokens** | `reasoning_effort: "<level>"` | `low`, `medium`, `high` |
| GPT-5.x (OpenAI, non-codex) | None | N/A | N/A |
| Other models | None | N/A | N/A |

### Adaptive Thinking (Claude 4.6 models)

Adaptive thinking is the recommended mode for Opus 4.6 and Sonnet 4.6. Key properties:
- **Dynamic allocation**: Claude determines when and how much to think per request
- **Interleaved thinking**: Claude can think between tool calls (critical for agentic loops)
- **Effort-guided**: the `effort` parameter is soft guidance, not a hard budget
- `thinking.type: "enabled"` with `budget_tokens` is **deprecated** on 4.6 models

The interleaved thinking property is especially valuable for Loop V2: the model can
reason about tool results between tool calls within a single turn, enabling better
multi-tool planning without additional loop iterations.

### TUI Commands

| Command | Effect |
|---------|--------|
| `/debug` | Toggle debug mode — show full signal trace after every response |
| `/debug last` | Show full signal trace for the last turn only |
| `/signals` | Show condensed signal summary for the last turn |
| `/thinking on` | Enable thinking (adaptive for 4.6, extended for older, codex for gpt-5.3) |
| `/thinking off` | Disable thinking |
| `/thinking <level>` | Set effort level: `max`, `high`, `medium`, `low`, `xhigh` (model-dependent) |

### Error Display

When things go wrong, the user should see clear, actionable error messages — not
generic fallbacks. The kernel knows exactly what happened via signals; the TUI
must surface this:

| Failure | User-facing message |
|---------|-------------------|
| Tool blocked by sandbox | `✗ write_file blocked: path '/tmp/foo' is outside working directory` |
| Tool execution failed | `✗ read_file failed: file not found 'doesnotexist.rs'` |
| Command timeout | `✗ run_command timed out after 30s (partial output available)` |
| Search returned empty | `No matches found for 'XYZZY' in the codebase.` |
| Budget exhausted | `✗ Budget limit reached (248000/250000 tokens used)` |
| Model returned empty | `✗ Model returned empty response. Try rephrasing.` |
| User cancelled | `✗ Cancelled by user` |

These are synthesized from signals in the TUI layer, not from the model. The
model never generates error messages about its own infrastructure — the shell does.

---

## 6. What Gets Removed

The following code in `loop_engine.rs` is replaced by this spec:

- `emit_intent` tool definition and all related functions
- `parse_emit_intent_call`, `wrap_direct_tool_call_as_intent`
- `parse_tool_call_intent`, `parse_tool_action`, `parse_tool_use_action`
- `ReasonedIntent` as a required model output (may be retained as internal type)
- `IntendedAction` enum as a model-facing schema
- `expected_outcome_mismatch` and declared-outcome verification
- `REASONING_SYSTEM_PROMPT` (replaced with simpler prompt)
- `build_reasoning_system_prompt`, `available_tools_instructions`
- `emit_intent_tool_definition`, `format_tool_instruction_line`
- `reasoning_user_prompt` (simplified)

Estimated net change: **~300 lines removed** (emit_intent infra) **+ ~1000 lines added** (signals, interaction, cancellation). Net growth ~700 lines, but the new code replaces brittle intent-parsing with robust observable infrastructure.

---

## 7. What Gets Preserved

- `LoopEngine` struct and `run_cycle` entry point
- `BudgetTracker` and budget enforcement
- `ContextCompactor` and context management
- `ToolExecutor` trait and `FawxToolExecutor`
- `ToolResult`, `ToolExecutorError`, `TokenUsage`
- `Decision` enum (Respond, UseTools, Clarify, Defer)
- `Continuation` enum (Complete, Continue, NeedsInput)
- `LoopResult` enum
- `LoopStatus` and status reporting
- Conversation history management
- All tool implementations (read_file, write_file, etc.)
- Synthesis step and synthesis instruction

---

## 8. Migration Path

### Phase 1: Direct Tool Calling (unblocks everything)
- Expose real tools alongside emit_intent in `build_reasoning_request`
- `wrap_direct_tool_call_as_intent` becomes the primary path
- emit_intent still accepted but not required
- Update system prompt to not require emit_intent
- Estimated effort: small PR, **~50 lines prod code + ~150 lines test changes**
  (15+ tests assert emit_intent-specific behavior and need updating)

### Phase 2: Remove emit_intent
- Remove emit_intent tool definition entirely
- Simplify REASONING_SYSTEM_PROMPT
- Rework DECIDE to accept CompletionResponse directly (not ReasonedIntent)
  — this is the hidden dependency: `decide_from_intent` needs reshaping
- Remove all intent parsing code
- Estimated effort: medium PR, **~200 lines removed + ~100 lines DECIDE rework**

### Phase 3: Signal Infrastructure
- Add Signal struct, SignalCollector, SignalKind enum
- Emit signals from each loop step (7 steps × 3-5 signals each)
- Wire signals through LoopStatus return value (shell-engine boundary)
- Add /signals (condensed), /debug (raw) commands to TUI
- Capture thinking trace from streaming
- Estimated effort: medium-large PR (~500 lines added)
  (struct definitions ~50, emission points ~250 across 7 functions, TUI commands ~100, tests ~100)

### Phase 4: User Interaction (highest risk — terminal I/O integration)
- Implement LoopInputChannel with tokio mpsc
- Bare word command parsing during execution
- CancellationToken integration with ToolExecutor
  **Note:** this is a breaking change to the `ToolExecutor` trait — `execute()` gains
  an `Option<CancellationToken>` parameter. All implementations (`FawxToolExecutor`,
  test mocks) must be updated. Coordinate with any in-flight PRs touching ToolExecutor.
- Ctrl+C signal handler (SIGINT → abort, not process kill)
- Readline ↔ LoopInputChannel handoff (race-prone)
- Estimated effort: large PR, **~500 lines added**
  (channel ~50, command parsing ~80, cancellation ~100, signal handler ~50, TUI integration ~120, tests ~100)

### Phase 5: Async Learning (future)
- Persistent signal storage
- Cross-session pattern analysis
- Factory-style LLM judge
- Self-filing issues on friction thresholds

---

## 9. References

- [Factory Signals](https://factory.ai/news/factory-signals) — inspiration for observational learning
- DOCTRINE.md — kernel immutability, tool execution, security posture
- ENGINEERING.md — code quality standards, testing requirements
- Issue #931 — emit_intent failure analysis
- Issue #930 — abort/steer/queue requirements
- Issue #924 — verify loop bug (symptom of declared-vs-actual)
