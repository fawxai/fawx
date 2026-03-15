# #930 — Streaming Output + Async Input

**Status:** Implementation Spec  
**Date:** 2026-03-02  
**Related:** loop-v2-spec.md §4, #931 (emit_intent removal), #924 (verify loop)  
**Crate scope:** `fx-cli`, `fx-kernel`, `fx-core`

---

## 1. Problem Statement

### Current Architecture

The TUI main loop (`TuiApp::run`) follows a blocking sequential pattern:

```
User types input → readline blocks until Enter
    → handle_message() called
        → run_with_thinking_spinner wraps the entire loop engine cycle
            → LoopEngine::run_cycle blocks until complete
        → display_response() renders the final result
    → readline blocks again
```

Three problems arise from this design:

**1. No streaming output to the terminal during the main loop cycle.**

`run_with_thinking_spinner` (tui.rs:330-340) wraps the *entire* `run_cycle` call
in a spinner future. While the LLM response is being generated, the user sees
"⠋ thinking..." — not the actual tokens arriving. The `RouterLoopLlmProvider`
does have streaming support via `generate_streaming` (tui.rs:2141), but it's only
used for the *synthesis* fallback path inside `LoopEngine::generate_tool_summary`
(loop_engine.rs). The primary reasoning path goes through `LoopEngine::reason()`
→ `llm.complete()`, which buffers the entire response before returning. The
`consume_stream_silent` function (tui.rs:2455) explicitly suppresses stream output
for the `generate` path ("internal reasoning output is not leaked to the terminal").

Even `generate_streaming` writes directly to `io::stdout()` (tui.rs:2161), which
conflicts with the spinner writing to `io::stderr()` — both fight for terminal
cursor position.

**2. No input accepted during execution (beyond Ctrl+C).**

While `run_cycle` is running, `readline` is not active — the main task is blocked
inside `run_with_thinking_spinner`. The `LoopInputChannel` infrastructure exists
in `fx-kernel` (input.rs) and the engine checks `check_user_input()` between steps,
but the TUI **never creates a channel or wires it up**. The `parse_bare_command`
function (tui.rs:678) is annotated `#[allow(dead_code)]` with the comment "Wired
into readline handoff in a follow-up." The `set_input_channel` method on LoopEngine
is never called by TuiApp.

The only interruption mechanism is `CancellationToken` via `Ctrl+C` signal handler
(tui.rs:779-787), which works but doesn't support steering commands like
`stop`, `wait`, `pause`, `/status`.

**3. Response rendering happens only after the full cycle completes.**

`handle_message` (tui.rs:1081-1113) collects the full `LoopResult`, renders it with
`render_loop_result`, and only then calls `display_response`. For multi-tool turns
with synthesis, this means the user waits for: reasoning LLM call + tool execution
+ synthesis LLM call — seeing only the spinner the entire time. Token-by-token
display would provide immediate feedback that something is happening and let the
user steer early.

### Why This Matters

- **User experience:** Users of agentic CLI tools (Claude Code, Codex CLI) expect
  streaming output. A spinner with no content for 10+ seconds feels broken.
- **Steering:** The loop-v2 spec (§4) specifies that bare-word commands during
  execution are first-class. Without async input, the user can't `stop` a runaway
  multi-tool loop or `wait` to inspect intermediate results.
- **Responsiveness:** The user should be able to steer mid-execution without waiting
  for the current cycle to finish. Bare-word `status` / `/status` should provide a
  lightweight runtime snapshot during execution (via `StatusQuery`); richer status
  UI formatting remains follow-up work.

---

## 2. Files to Change

| File | Lines (approx, as of `84504f10`) | Change Summary |
|------|-----------------------------------|----------------|
| `engine/crates/fx-cli/src/tui.rs` | 304-340 (spinner), 678-686 (command parse), 1081-1113 (`handle_message`), 2091-2184 (`RouterLoopLlmProvider`), 2455-2488 (stream consumers) | Async input task, streamed output writer, `handle_message` restructured, spinner replaced with stream renderer |
| `engine/crates/fx-kernel/src/loop_engine.rs` | 33-67 (kernel `LlmProvider` trait), 303-315 (`set_event_bus` + `set_input_channel`), 379-418 (`run_cycle`), 658-678 (`reason`), 1317-1503 (`act_with_tools` + tool continuation/synthesis) | Add `complete_stream` to the **kernel** `LlmProvider` trait (defined here), reuse existing `LoopEngine` `event_bus` plumbing for streaming publication, stream reason/tool-continuation deltas through EventBus, define streaming cancellation/error semantics, and handle `StatusQuery` side-band snapshots |
| `engine/crates/fx-kernel/src/perceive.rs` | 11-20 (`ProcessedPerception`) | Add `steer_context: Option<String>` and propagate it from snapshot processing so reasoning prompt assembly can consume steer via `ProcessedPerception` |
| `engine/crates/fx-kernel/src/input.rs` | 15-25 (`LoopCommand`), 37-60 (channel send/recv) | Add `Steer(String)` + `StatusQuery` variants to `LoopCommand` and update command-priority handling |
| `engine/crates/fx-kernel/src/types.rs` | 13-29 (`PerceptionSnapshot`) | Add `steer_context: Option<String>` to `PerceptionSnapshot` for steer injection into reasoning prompts |
| `engine/crates/fx-core/src/event.rs` | 24-69 (`EventBus`) | No structural changes needed (already supports broadcast) |
| `engine/crates/fx-core/src/message.rs` | 9-67 (`InternalMessage` enum) | Add `StreamPhase` enum plus `StreamDelta`, `StreamingStarted`, `StreamingFinished` variants to `InternalMessage` |

### Files NOT changed (important boundaries)

- **`fx-llm` crate:** Already supports streaming primitives (`CompletionStream`, `StreamChunk`). No trait/interface changes in `fx-llm` for this PR; `complete_stream` is added only to the kernel `LlmProvider` trait in `loop_engine.rs`.
- **`fx-config`:** No new config keys in this PR. Streaming is always-on.
- **Signal infrastructure:** `SignalCollector`, `Signal`, `LoopStep`, `SignalKind` are untouched. Signals continue to accumulate normally.

---

## 3. API Design

### 3.1 New InternalMessage Variants

```rust
// engine/crates/fx-core/src/message.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamPhase {
    Reason,
    Synthesize,
}

enum InternalMessage {
    // ... existing variants ...

    /// A streaming text delta from the LLM (for real-time TUI rendering).
    StreamDelta {
        /// The incremental text chunk.
        delta: String,
        /// Which phase produced this delta.
        phase: StreamPhase,
    },

    /// LLM streaming has started for a phase.
    StreamingStarted {
        phase: StreamPhase,
    },

    /// LLM streaming has finished for a phase.
    StreamingFinished {
        phase: StreamPhase,
    },
}
```

### 3.2 Extended LoopCommand

```rust
// engine/crates/fx-kernel/src/input.rs

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopCommand {
    Stop,
    Abort,
    Wait,
    Resume,
    /// Request an in-flight loop status snapshot without interrupting execution.
    StatusQuery,
    /// User-typed text during execution that isn't a control word.
    /// Injected as steering context for the next iteration.
    Steer(String),
}
```

Note: `LoopCommand` currently derives `Copy`. Adding `Steer(String)` removes `Copy`.
The only place that matters is `LoopInputSender::send` which takes `LoopCommand` by
value (already correct for owned types). `check_user_input` in loop_engine.rs returns
`Option<LoopCommand>` (also fine). `matches!` patterns on `LoopCommand::Stop | LoopCommand::Abort`
remain unchanged — `Steer` and `StatusQuery` are handled separately.

#### StatusQuery semantics

- `parse_bare_command` maps `status`, `st`, and `/status` to `LoopCommand::StatusQuery`.
- When `StatusQuery` is dequeued, the engine snapshots `LoopStatus` and publishes a
  human-readable status line via existing `InternalMessage::SystemStatus`.
- Status line template (single line):
  `status: iter={iteration_count}/{max_iterations} llm={llm_calls_used} tools={tool_invocations_used} tokens={tokens_used} cost_cents={cost_cents_used} remaining(llm={remaining.llm_calls},tools={remaining.tool_invocations},tokens={remaining.tokens},cost_cents={remaining.cost_cents})`.
- `StatusQuery` is side-band only: it does **not** alter continuation state, budget
  decisions, or loop control flow.

### 3.3 TUI Async Input Task

```rust
// engine/crates/fx-cli/src/tui.rs (new function)

/// Spawn a background task that reads raw terminal input during loop execution
/// and routes it through the LoopInputSender.
///
/// Returns a JoinHandle that resolves when the loop completes (signaled via
/// the stop channel). Any buffered bytes that were not completed into an Enter-
/// terminated command are discarded on shutdown (`rustyline` has no input
/// injection API for replay).
fn spawn_input_reader(
    sender: LoopInputSender,
    stop_signal: oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<()>;
```

The input reader:
1. Enables crossterm raw mode and temporarily disables the process-level Ctrl+C
   signal handler while raw mode is active
2. Polls `crossterm::event::poll` with a short timeout (50ms)
3. On key event: accumulates into a line buffer
4. On Enter: parses via `parse_bare_command` (including `status`/`st`/`/status`).
   If recognized → sends through `sender`; if unrecognized and non-empty → sends
   `LoopCommand::Steer(text)`.
5. On Ctrl+C key event (`KeyModifiers::CONTROL + 'c'`): sends `LoopCommand::Abort`
   directly (in raw mode this is a key event, not a signal)
6. On stop signal: disables raw mode, re-enables the signal handler, and discards
   any unconsumed buffered bytes

### 3.4 TUI Stream Renderer

```rust
// engine/crates/fx-cli/src/tui.rs (new struct)
use fx_core::EventBus as MessageBus;
use std::io::Write;

/// Subscribes to the EventBus and renders streaming deltas to the terminal.
/// Replaces the thinking spinner for phases that produce streaming output.
struct StreamRenderer {
    /// EventBus subscription receiver.
    receiver: broadcast::Receiver<InternalMessage>,
    /// Output target for rendered deltas (stdout in production, test buffer in unit tests).
    writer: Box<dyn Write + Send>,
    /// Whether we're currently in a streaming phase (suppress spinner).
    active: bool,
}

impl StreamRenderer {
    /// Production constructor: writes to stdout.
    fn new(bus: &MessageBus) -> Self;

    /// Test constructor: caller injects writer.
    fn with_writer(bus: &MessageBus, writer: Box<dyn Write + Send>) -> Self;

    /// Run the renderer loop. Exits when the stop signal fires.
    async fn run(mut self, stop_signal: oneshot::Receiver<()>);
}
```

The renderer:
1. On `StreamingStarted { phase: StreamPhase::Reason | StreamPhase::Synthesize }` →
   clear spinner line, print `"assistant › "` header, set `active = true`
2. On `StreamDelta` → write delta text to the configured writer (stdout by default), flush
3. On `StreamingFinished` → print newline, set `active = false`
4. While `!active` → show thinking spinner (existing behavior)
5. Spinner fallback timeout is a named constant: `STREAM_SPINNER_FALLBACK_MS` (default `500`)

### 3.5 LoopEngine: Streaming Reason Path

The current `reason()` method calls `llm.complete()` which buffers the full response.
Instead, it should use `complete_stream` through the **kernel** `LlmProvider` trait
in `fx-kernel/src/loop_engine.rs` (not an `fx-llm` trait change) and publish deltas
through the EventBus.

**Implementation note (codebase-verified):** `LoopEngine` already exposes
`event_bus: Option<fx_core::EventBus>` and `set_event_bus(&mut self, bus: fx_core::EventBus)`
(currently around `loop_engine.rs:160` and `loop_engine.rs:303-304`). This spec
does **not** add a new field/setter; it reuses that existing infrastructure and
extends usage by publishing `StreamingStarted` / `StreamDelta` /
`StreamingFinished` during streaming reason/tool-continuation paths. The TUI's
`StreamRenderer` subscribes to these events.

**EventBus type clarification:** this uses `fx_core::EventBus` (Tokio
`broadcast`-based, async pub/sub), not the `fx-kernel` synchronous observer/signal
pattern (`SignalCollector` and loop `Signal` events). `fx_core::EventBus` is used
for streaming UI deltas because it supports async, decoupled shell subscribers
without coupling kernel logic to terminal rendering.

EventBus disambiguation (required): this spec uses `fx_core::EventBus` (or
`use fx_core::EventBus as MessageBus`) for streaming/message publication.
`fx-kernel/src/event_bus.rs` defines a different observer bus type; do not mix them.

**Kernel trait naming and migration clarification:**

- The trait in `fx-kernel/src/loop_engine.rs` is consistently named **`LlmProvider`**
  and this spec uses that name consistently.
- `fx-llm` also defines an `LlmProvider`, but that is a lower-level provider trait;
  the kernel trait is a boundary adapter used by `LoopEngine`.
- `RouterLoopLlmProvider` remains the adapter between kernel `LlmProvider` and
  `fx-llm`'s router/provider stack.
- `complete_stream` is the canonical path for loop execution (`reason()` and tool
  continuation).
- Existing callback-style `generate_streaming` remains short-term for compatibility
  with legacy callers, but no new kernel path should depend on it; follow-up cleanup
  deprecates/removes it once all callers are migrated.
- `fx_llm::ProviderError` is a re-export alias of `fx_llm::LlmError`
  (`pub use ... LlmError as ProviderError`); this spec uses the `ProviderError`
  name for interface clarity.

The kernel trait already uses `#[async_trait]` in `loop_engine.rs`; add
`complete_stream` under that existing annotation and keep the same pattern in all
implementations/mocks.

**New method on the kernel `LlmProvider` trait (`loop_engine.rs`, must use `#[async_trait]`):**

```rust
use async_trait::async_trait;

#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    // ... existing methods ...

    /// Complete with streaming, returning chunks through a stream.
    /// Default implementation: call complete() and wrap result as a single chunk.
    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderError> {
        let response = self.complete(request).await?;
        Ok(Box::pin(futures::stream::iter(vec![Ok(response_to_chunk(response))])))
    }
}
```

`response_to_chunk` maps `CompletionResponse` into one terminal `StreamChunk`: concatenate `ContentBlock::Text` values into `delta_content`, convert each `tool_calls` entry into a completed `ToolUseDelta` (`id`/`name`, JSON `arguments_delta`, `arguments_done = true`), copy `usage`, and copy `stop_reason` unchanged (including `None`).
Implement `response_to_chunk` as a **private free function** in
`fx-kernel/src/loop_engine.rs` (module scope, near the `LlmProvider` default impl),
not as a `LoopEngine` method.

The `reason()` method changes from:
```rust
let response = llm.complete(request).await?;
```
to:
```rust
let mut stream = llm.complete_stream(request).await?;
self.publish_streaming_started(StreamPhase::Reason);
let response = self
    .consume_stream_with_events(&mut stream, StreamPhase::Reason)
    .await?;
self.publish_streaming_finished(StreamPhase::Reason);
```

`consume_stream_with_events` signature (explicit):

```rust
async fn consume_stream_with_events(
    &self,
    stream: &mut CompletionStream,
    phase: StreamPhase,
) -> Result<CompletionResponse, ProviderError>;
```

Behavioral contract for `consume_stream_with_events`:
- Collect chunks, publish deltas through EventBus, and assemble the final
  `CompletionResponse`.
- Accumulate `tool_use_deltas` across chunks into complete tool calls.
- On cancellation, set `stop_reason = "cancelled"`, discard partial tool calls, and
  return an incomplete marker response.
- On provider error mid-stream (after one or more deltas):
  - always publish `StreamingFinished { phase }` from a `finally`-style path so the
    renderer exits streaming mode cleanly,
  - preserve accumulated text as best-effort partial context for diagnostics,
  - return `ProviderError` so callers surface a recoverable loop error.

Unicode note: streamed deltas are `String` chunks, so each chunk is valid UTF-8
(codepoint-aligned by type). No byte-level UTF-8 reassembly is required in V1;
grapheme clusters may span chunks and are rendered via normal append behavior.


### 3.6 Steer Context Propagation (PerceptionSnapshot → ProcessedPerception)

```rust
// engine/crates/fx-kernel/src/types.rs
pub struct PerceptionSnapshot {
    // ... existing fields ...
    pub steer_context: Option<String>,
}

// engine/crates/fx-kernel/src/perceive.rs
pub struct ProcessedPerception {
    // ... existing fields ...
    pub steer_context: Option<String>,
}
```

Steer injection rules:
- `LoopEngine` stores pending steer text as `Option<String>`.
- If multiple `Steer(...)` commands arrive before the next `perceive()` call,
  **last received steer wins**; earlier steers in that same perceive window are
  discarded.
- On `perceive()`, the winning steer (if any) is copied into
  `PerceptionSnapshot::steer_context` and then cleared from pending state.
- In the same `perceive()` pipeline, when constructing `ProcessedPerception`, copy
  `snapshot.steer_context.clone()` into `ProcessedPerception::steer_context`.
- `reasoning_user_prompt(perception: &ProcessedPerception)` reads
  `perception.steer_context` and appends a dedicated section when present, e.g.
  `"User steer (latest): <text>"`, so steer text is visible to the model in the
  next reasoning pass.

Design note: keeping steer on both snapshot and processed forms in V1 makes the
perceive→reason data path explicit while preserving snapshot-level state for
loop bookkeeping/serialization.


---

## 4. Implementation Plan

### Step 1: Extend InternalMessage (fx-core)

- Add `StreamDelta`, `StreamingStarted`, `StreamingFinished` variants to `InternalMessage`
- Add serde roundtrip tests for new variants
- **No other crate changes.** This is a pure additive change.

### Step 2: Extend LoopCommand with Steer + StatusQuery (fx-kernel)

- Add `Steer(String)` and `StatusQuery` to `LoopCommand`
- Remove `Copy` derive (becomes `Clone` only)
- Update `check_user_input` priority logic:
  - `Abort` still wins over everything
  - `Stop` wins over `Wait`/`Resume`/`StatusQuery`/`Steer`
  - `Wait`/`Resume` outrank `StatusQuery` and `Steer`
  - `StatusQuery` outranks `Steer` (status requests are side-band; steer is lowest)
- On `StatusQuery`, emit runtime snapshot text through `InternalMessage::SystemStatus`
  and continue loop execution unchanged
- Use a single-line status format template:
  `status: iter={iteration_count}/{max_iterations} llm={llm_calls_used} tools={tool_invocations_used} tokens={tokens_used} cost_cents={cost_cents_used} remaining(llm={remaining.llm_calls},tools={remaining.tool_invocations},tokens={remaining.tokens},cost_cents={remaining.cost_cents})`
- Add `PerceptionSnapshot::steer_context: Option<String>` in `fx-kernel/src/types.rs`
- Add `ProcessedPerception::steer_context: Option<String>` in `fx-kernel/src/perceive.rs`
- Store pending steer in `LoopEngine` state; inject into `steer_context` at the
  next `perceive()` call
- In `LoopEngine::perceive()`, propagate `snapshot.steer_context` into
  `ProcessedPerception::steer_context` so the reason-step prompt path can consume it
- Update `reasoning_user_prompt` to read steer from `ProcessedPerception`
- Deduplicate steers per perceive window: last received steer wins, earlier steers
  in that window are discarded
- Add unit tests for priority ordering, status query side-band behavior, and steer deduplication

### Step 3: Add `complete_stream` to the kernel `LlmProvider` trait (`loop_engine.rs`)

- Use the actual kernel trait name consistently: `LlmProvider`.
- Match the current codebase pattern in examples/signatures: use `#[async_trait]`
  on kernel `LlmProvider` and its implementations (including mocks), and add
  `complete_stream` under that existing trait.
- Add `complete_stream` to kernel `LlmProvider` with a default implementation that wraps `complete()`.
- Add a private free function `response_to_chunk(...) -> StreamChunk` in
  `loop_engine.rs` (module scope, near the trait default implementation).
- Reuse existing `LoopEngine` `event_bus: Option<fx_core::EventBus>` and
  `set_event_bus(&mut self, bus: fx_core::EventBus)`; no new field/setter is
  introduced in this spec.
- Disambiguate EventBus types in this file via fully qualified type or alias,
  e.g. `use fx_core::EventBus as MessageBus`, so it cannot be confused with
  `crate::event_bus::EventBus`.
- Keep `fx-llm` traits unchanged in this PR (separate layer, separate trait).
- Clarify method coexistence/migration on kernel trait:
  - `complete_stream` is canonical for loop execution paths.
  - `generate_streaming` remains a compatibility adapter (delegates to streamed chunks + callback).
  - New logic should not use `generate_streaming`; cleanup/deprecation follows migration.
- Implement `complete_stream` on `RouterLoopLlmProvider` in `tui.rs` (delegates to `self.router.complete_stream()`).
- Add helper `LoopEngine::consume_stream_with_events(&self, stream: &mut CompletionStream, phase: StreamPhase) -> Result<CompletionResponse, ProviderError>` that:
  - Iterates over `CompletionStream`.
  - Publishes `StreamDelta` events through `event_bus` (if present).
  - Assembles text content blocks and tool-call deltas into a `CompletionResponse`.
  - Performs tool-call delta accumulation in kernel code unless an existing helper is confirmed reusable.
  - Checks cancellation between chunks; on cancel sets `stop_reason = "cancelled"`, discards partial tool calls, and returns an incomplete response.
  - Handles provider errors mid-stream by finalizing streaming state (`StreamingFinished`) and returning `ProviderError`.
  - Treats chunk text as valid UTF-8 `String` data (append-only; no byte-fragment decoder needed).
- Refactor `reason()` to use `complete_stream` path.
- Refactor tool continuation (`request_tool_continuation`) to use `complete_stream`.
- **Tests:** Mock `LlmProvider` multi-chunk streams; verify events, tool-call assembly, cancellation behavior, provider error behavior, and Unicode chunk handling.

### Step 4: Build StreamRenderer in TUI (fx-cli)

- New `StreamRenderer` struct that subscribes to EventBus
- `StreamRenderer` accepts an injectable `writer: Box<dyn Write + Send>` for testability
  (default `new()` uses stdout, tests use `with_writer(...)`)
- Replaces `run_with_thinking_spinner` for the message handling path
- On `StreamingStarted(StreamPhase::Reason)`: clear spinner, print assistant header
- On `StreamDelta`: write delta to injected writer, flush
- On `StreamingFinished(StreamPhase::Reason)`: no-op (wait for next phase or completion)
- On `StreamingStarted(StreamPhase::Synthesize)`: same behavior (continue streaming)
- Between streaming phases: show spinner (tool execution phase)
- **Tests:** StreamRenderer with a mock EventBus + in-memory writer verifies output sequence

### Step 5: Wire Async Input in TUI (fx-cli)

- Create `LoopInputChannel` in `TuiApp::handle_message` before calling `run_cycle`
- Pass `LoopInputSender` to `spawn_input_reader`
- Pass `LoopInputChannel` to `loop_engine.set_input_channel()`
- `spawn_input_reader` uses crossterm raw mode + event polling:
  - Accumulates keystrokes into a line buffer
  - Keystrokes are consumed **without inline echo** while a cycle is running (V1 decision)
  - On Enter: `parse_bare_command` → send recognized LoopCommand (`stop`, `abort`,
    `wait`, `resume`, `status`/`/status`), or `Steer(text)` for unrecognized input;
    echo the submitted command once after Enter for operator visibility
  - On Ctrl+C key event: send `Abort` directly from the reader
  - Backspace: edit buffer
  - Temporarily disable the existing signal-based Ctrl+C handler while raw mode is
    active, then re-enable it on teardown
- On cycle completion: stop input reader and discard any unconsumed buffered bytes
- Remove `#[allow(dead_code)]` from `parse_bare_command`
- **Tests:** Integration test with mock LLM that delays; verify Abort command stops cycle

### Step 6: Restructure handle_message (fx-cli)

Refactor `TuiApp::handle_message` from:
```rust
let loop_result = run_with_thinking_spinner(async { ... }).await?;
// ... render and display
```
to:
```rust
use fx_core::EventBus as MessageBus;

// 1. Set up EventBus on loop_engine
// Sized for ~640ms burst at ~100 stream events/sec before lag/drop.
const STREAM_EVENT_BUS_CAPACITY: usize = 64;
const STREAM_SPINNER_FALLBACK_MS: u64 = 500;
let bus = MessageBus::new(STREAM_EVENT_BUS_CAPACITY);
self.loop_engine.set_event_bus(bus.clone());

// 2. Set up input channel
let (sender, channel) = loop_input_channel();
self.loop_engine.set_input_channel(channel);

// 3. Spawn stream renderer (replaces spinner)
let (stop_render_tx, stop_render_rx) = oneshot::channel();
let renderer = StreamRenderer::new(&bus); // uses STREAM_SPINNER_FALLBACK_MS internally
let render_handle = tokio::spawn(renderer.run(stop_render_rx));

// 4. Spawn async input reader
let (stop_input_tx, stop_input_rx) = oneshot::channel();
let input_handle = tokio::spawn(spawn_input_reader(sender, stop_input_rx));

// 5. Run the cycle
let loop_result = self.loop_engine.run_cycle(snapshot, &llm).await
    .map_err(|e| TuiError::Loop(e.reason))?;

// 6. Tear down
let _ = stop_render_tx.send(());
let _ = stop_input_tx.send(());
let _ = render_handle.await;
let _ = input_handle.await; // buffered bytes are intentionally discarded

// 7. Post-process result (existing logic)
```

---

## 5. Test Plan

### Unit Tests

| Test | Location | What It Verifies |
|------|----------|------------------|
| `steer_command_priority_below_abort` | `fx-kernel/src/input.rs` | Abort wins over queued Steer |
| `status_query_priority_below_stop_above_steer` | `fx-kernel/src/input.rs` | Stop outranks StatusQuery; StatusQuery outranks Steer |
| `status_query_emits_system_status_message` | `fx-kernel/src/loop_engine.rs` | `StatusQuery` publishes `InternalMessage::SystemStatus` and does not alter loop continuation |
| `steer_stored_and_injected_in_perceive` | `fx-kernel/src/loop_engine.rs` | Winning steer text appears in `PerceptionSnapshot::steer_context` and is propagated to `ProcessedPerception::steer_context` on the next perceive |
| `steer_dedup_last_wins_within_window` | `fx-kernel/src/loop_engine.rs` | Multiple steers before one perceive are deduped; only the last survives |
| `consume_stream_with_events_publishes_deltas` | `fx-kernel/src/loop_engine.rs` | Each stream chunk → `StreamDelta` event on bus |
| `consume_stream_with_events_accumulates_tool_deltas` | `fx-kernel/src/loop_engine.rs` | Tool call deltas are assembled into complete tool calls |
| `consume_stream_cancellation_mid_stream` | `fx-kernel/src/loop_engine.rs` | Cancellation sets `stop_reason = "cancelled"`, drops partial tool calls, returns incomplete response |
| `consume_stream_provider_error_mid_stream_finishes_phase` | `fx-kernel/src/loop_engine.rs` | Mid-stream provider error still emits `StreamingFinished` and returns `ProviderError` |
| `consume_stream_with_events_handles_unicode_chunks` | `fx-kernel/src/loop_engine.rs` | Unicode text assembled correctly from chunked `String` deltas |
| `stream_renderer_clears_spinner_on_start` | `fx-cli/src/tui.rs` | `StreamingStarted` clears spinner line |
| `stream_renderer_writes_deltas_to_writer` | `fx-cli/src/tui.rs` | `StreamDelta` events written to injected writer |
| `parse_bare_command_status_aliases` | `fx-cli/src/tui.rs` | `status`, `st`, `/status` map to `LoopCommand::StatusQuery` |
| `parse_bare_command_unrecognized_returns_none` | `fx-cli/src/tui.rs` | Unrecognized text returns `None` (caller wraps as Steer) |
| `loop_command_steer_not_copy` | `fx-kernel/src/input.rs` | `LoopCommand::Steer` compiles without Copy |
| `internal_message_stream_delta_serde` | `fx-core/src/message.rs` | Serde roundtrip for new variants |

### Integration Tests

| Test | What It Verifies |
|------|------------------|
| `streaming_reason_produces_visible_output` | Full cycle with streaming mock → verify stdout contains incremental text (not just final) |
| `abort_during_streaming_reason_returns_incomplete_cancelled` | Start streaming mock with delay, send Abort via input channel → caller receives interrupted/incomplete result with `stop_reason = "cancelled"` |
| `status_query_during_execution_emits_runtime_snapshot` | Send `status` while loop is active → EventBus emits a status line and cycle continues |
| `provider_error_mid_stream_exits_renderer_mode_cleanly` | Inject provider failure after partial deltas → verify renderer receives `StreamingFinished` and loop surfaces recoverable error |
| `steer_during_tool_execution_redirects_next_iteration` | Send `Steer("try a different approach")` during tool exec → verify next iteration prompt includes only the latest steer via `ProcessedPerception::steer_context` |

### Manual Verification

- Start fawx TUI, send a message, verify tokens stream to terminal in real-time
- During streaming: type `stop` + Enter → verify cycle stops gracefully
- During execution: type `status` (or `/status`) + Enter → verify a runtime status snapshot prints and cycle continues
- During tool execution: type free-form steer text + Enter → verify next iteration prompt includes `User steer (latest)`
- While a cycle is running, type random characters → verify no inline echo; on Enter, only the submitted command line is echoed once
- Verify Ctrl+C aborts via raw-mode key handling while signal handler is temporarily disabled
- Simulate provider drop mid-stream (e.g., revoke network/token) → verify stream line closes cleanly and user sees recoverable error with partial context
- Verify readline history is not corrupted by mid-execution input

---

## 6. Edge Cases and Risks

### Race Conditions

**Risk:** Input reader and stream renderer both write to stdout simultaneously.  
**Mitigation:** V1 explicitly defers inline input display. While a cycle is active,
keystrokes are consumed without echo; only the final submitted command is echoed
once on Enter. This avoids cursor-fighting between renderer and reader.

**Risk:** `EventBus` publish returns 0 receivers if renderer hasn't subscribed yet.  
**Mitigation:** `EventBus::publish` already handles this gracefully (returns `Ok(0)`).
Create the bus and subscribe *before* spawning the cycle task.

### Terminal State

**Risk:** Raw mode not properly restored on panic or unexpected exit.  
**Mitigation:** Use the existing `RawModeGuard` (tui.rs) RAII pattern in the input
reader. Wrap the spawn in a guard that disables raw mode on drop. Also add a
`panic::set_hook` that disables raw mode (defense in depth).

**Risk:** Ctrl+C may be handled twice (signal + key event) or not at all.  
**Mitigation:** In raw mode, Ctrl+C is treated as a key event, not a signal. Disable
(or ignore) the existing signal-based Ctrl+C handler while raw mode is active.
`spawn_input_reader` handles Ctrl+C directly by sending `LoopCommand::Abort`, then
re-enable the signal handler when raw mode exits.

### LoopCommand Value Semantics (Steer + StatusQuery)

**Risk:** Removing `Copy` from `LoopCommand` breaks code that implicitly copies commands.  
**Mitigation:** Audit all uses. Currently: `check_user_input` returns `Option<LoopCommand>`,
`consume_stop_or_abort_command` uses `matches!`, `LoopInputSender::send` takes by value.
None of these require `Copy`. `StatusQuery` is unit-like, `Steer` is owned text; both
are cheap to clone when needed and clearer than stringly control paths.

### Spinner Fallback

**Risk:** If EventBus is not set (e.g., test paths), no streaming events → no visible output.  
**Mitigation:** `StreamRenderer` falls back to spinner behavior when no `StreamingStarted`
event arrives within `STREAM_SPINNER_FALLBACK_MS` (default `500`). Tests that don't use
EventBus continue to see spinner.

### Partial CompletionResponse Assembly

**Risk:** Assembling `CompletionResponse` from streamed chunks may miss tool_use deltas
or produce malformed responses.  
**Mitigation:** `fx-llm::StreamChunk` carries `tool_use_deltas` and `stop_reason`,
but does not guarantee cross-chunk tool-call assembly. Kernel code in
`consume_stream_with_events` must accumulate:
- `delta_content` → concatenate into `ContentBlock::Text`
- `tool_use_deltas` → accumulate JSON argument fragments, assemble into `ToolCall` objects
- `usage` → take the last non-None usage (providers send final usage on last chunk)

On cancellation, force `stop_reason = "cancelled"`, discard partially assembled tool
calls, and return an incomplete response to the caller.

Implementation detail to verify during coding: confirm whether any provider-specific
helper already performs safe tool-delta assembly. If none exists, keep the kernel
assembler as the single source of truth.

### Mid-stream Provider Failure

**Risk:** Provider returns an error after streaming has already emitted visible deltas.  
**Mitigation:** `consume_stream_with_events` must finalize phase state in all paths
(success, cancellation, provider error). On provider error, publish
`StreamingFinished { phase }`, preserve best-effort partial text for diagnostics, and
propagate `ProviderError` so caller surfaces a recoverable failure to the user.

### Unicode Chunk Boundaries

**Risk:** Chunk boundaries split human-visible characters awkwardly.  
**Mitigation:** `StreamChunk` text is `String`, so each chunk is valid UTF-8. The
consumer appends chunks directly (no partial-byte decoder). Grapheme clusters may
span chunks, which is acceptable in V1 and validated via Unicode assembly tests.

### EventBus Capacity

**Risk:** Fast-streaming models fill the broadcast channel faster than renderer consumes.  
**Mitigation:** Use named constant `STREAM_EVENT_BUS_CAPACITY` (currently `64`) in
`handle_message`. That yields ~640ms of burst headroom at ~100 events/sec
(100 tokens/sec ≈ 100 delta events/sec). The renderer processes events in a tight
loop with no async I/O (just stdout writes). If the buffer fills,
`broadcast::Receiver` returns `RecvError::Lagged` — renderer skips missed deltas
(acceptable: final response text is still assembled correctly in the engine).

Future optimization: batch multiple tiny deltas before publishing/rendering to reduce
EventBus pressure and terminal flush frequency.


### Known V1 Limitations

- **Terminal resize:** active streaming + async input mode does not reflow on
  SIGWINCH in V1. Resizes may cause temporary visual artifacts until the next
  full redraw.
- **Non-TTY output:** when stdout is not a TTY (pipe/file/CI capture), disable
  live renderer writes and buffer stream text; emit it as a complete block when
  the phase finishes.


---

## 7. Estimated Complexity

| Area | Lines Added | Lines Modified | Files |
|------|-------------|----------------|-------|
| `fx-core/src/message.rs` | ~25 | 0 | 1 |
| `fx-kernel/src/input.rs` | ~15 | ~10 | 1 |
| `fx-kernel/src/types.rs` | ~5 | ~2 | 1 |
| `fx-kernel/src/loop_engine.rs` | ~130 | ~45 | 1 |
| `fx-cli/src/tui.rs` | ~250 | ~60 | 1 |
| Tests (all crates) | ~320 | ~20 | 4-5 |

**Total:** ~745 lines added, ~137 lines modified across 5 files (+ test files).

**Complexity tier:** Standard (single-PR feature). The changes touch multiple crates
but each change is self-contained. The biggest risk is the `CompletionResponse` assembly
from stream chunks — this deserves careful testing.

**Default PR split plan (recommended):**
- **PR 1 (default first):** Steps 1-3 (kernel-side streaming + InternalMessage + LoopCommand extension + StatusQuery semantics) — ~300 lines
- **PR 2 (default second):** Steps 4-6 (TUI-side input reader + stream renderer + wiring) — ~400 lines

Single-PR execution is fallback-only when scheduling constraints require it; split-by-default keeps review scope smaller and validates kernel streaming behavior before terminal wiring lands.

---

## Appendix: Architecture Diagram (After)

```
User types input → readline blocks until Enter
    → handle_message() called
        → EventBus created, StreamRenderer spawned (subscribes)
        → LoopInputChannel created, input reader spawned
        → LoopEngine::run_cycle runs on main task
            → reason():
                → llm.complete_stream() returns CompletionStream
                → engine iterates chunks:
                    → publishes StreamDelta(phase) → EventBus → StreamRenderer → stdout
                    → checks cancellation token + input channel between chunks
                    → on cancel: stop_reason="cancelled", drop partial tool calls
                    → assembles full CompletionResponse (or interrupted incomplete)
            → decide(): unchanged (pure kernel logic)
            → act():
                → tool execution (input reader checks for stop/status/steer between
                  tools; last steer in each perceive window wins)
                → tool continuation streaming: same delta publishing
            → verify/continue: unchanged
        → Cycle returns LoopResult
        → Stop renderer + input reader
        → display_response() renders metadata (iteration count, tokens, wall time)
    → readline resumes
```

Key difference from current: The user sees LLM tokens streaming to the terminal
during both the reasoning and synthesis phases, and can type commands during any
phase to steer or abort the cycle.
