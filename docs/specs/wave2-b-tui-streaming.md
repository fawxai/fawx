# Wave 2 PR B — TUI Streaming Output

## Goal
Wire the kernel's streaming completion path (from PR #1091) to the TUI so tokens render
incrementally as they arrive. This is the visible half of streaming — PR A added the plumbing,
this PR makes it visible.

## Target files
- **EDIT**: `engine/crates/fx-cli/src/tui.rs` — StreamRenderer, streaming display logic
- **EDIT**: `engine/crates/fx-cli/Cargo.toml` — may need `tokio-stream` or `futures-util` if not already present
- **POSSIBLY EDIT**: `engine/crates/fx-kernel/src/loop_engine.rs` — only if the streaming event flow needs adjustment for TUI consumption

## Current state (post PR #1091)
The kernel already:
- Has `complete_stream()` on the `LlmProvider` trait
- Publishes `StreamingStarted(phase)` and `StreamingFinished(phase)` events via EventBus
- Assembles tool calls from stream deltas via `StreamToolCallAssembler`
- Has `StreamPhase::Reason` and `StreamPhase::Synthesize`
- Handles `steer_context` in `PerceptionSnapshot`

The TUI currently:
- Calls the kernel's agentic loop which uses `complete_stream()` internally
- Does NOT render tokens as they arrive — it waits for the full response
- EventBus events are published but nothing in the TUI subscribes to them

## Design

### EventBus subscription
The TUI needs to subscribe to EventBus events to receive streaming chunks:
1. Subscribe to `StreamingStarted` → show streaming indicator, prepare output area
2. Subscribe to stream chunk events (text deltas) → render each token immediately
3. Subscribe to `StreamingFinished` → finalize the output, show iteration summary

**IMPORTANT**: Check how EventBus events are currently structured in `fx-kernel/src/types.rs` and 
`fx-kernel/src/loop_engine.rs`. The kernel may need a new event variant like `StreamTextDelta(String)` 
that carries the actual text chunk for the TUI to render. If this event doesn't exist yet, add it.

### StreamRenderer component
```rust
struct StreamRenderer {
    // NOTE: `cycle_active` was originally spec'd here but omitted from the
    // implementation. A fresh `StreamRenderer` is created per cycle in
    // `streaming_display_loop`, so a separate `cycle_active` flag is
    // unnecessary — the struct's lifetime *is* the cycle lifetime.
    /// Current phase (Reason or Synthesize)  
    current_phase: Option<StreamPhase>,
    /// Whether we've printed the prefix for this cycle
    prefix_printed: bool,
    /// Accumulated token count for this phase
    token_count: usize,
}
```

Key behaviors:
- Print `assistant ›` prefix once per user cycle (not per phase, not per event)
- Print each text delta to stdout immediately (`print!` + `flush`, no newline until done)
- On `StreamingFinished`, print a newline and iteration summary
- Handle multi-phase streaming: Reason phase → (possible tool calls) → Synthesize phase
  - Second phase continues on same line if no tool output intervened
  - If tool output was printed between phases, new prefix for synthesize phase

### stdout vs status
- Streamed text tokens → `stdout` (direct `print!` + flush for immediate rendering)
- Status lines (iteration count, tool summaries) → `stderr` or dedicated status area
- Never mix status output into the streaming text flow

### Interrupt handling
- Ctrl+C during streaming should set the CancellationToken
- The kernel's stream consumption loop checks cancellation
- TUI should show `[interrupted]` marker when stream is cancelled mid-output

## Tests
1. `stream_renderer_prints_prefix_once_per_cycle` — multiple text deltas, prefix only appears once
2. `stream_renderer_handles_multi_phase` — Reason → tool → Synthesize, correct prefixing
3. `stream_renderer_flush_on_each_delta` — verify immediate output behavior
4. `stream_interrupted_shows_marker` — cancellation during stream → `[interrupted]`
5. `stream_renderer_resets_between_turns` — two user turns, each gets fresh state

## Constraints
- No functions > 40 lines. StreamRenderer methods are focused.
- No `.unwrap()` outside tests.
- `cargo fmt --all` before commit.
- Run `cargo test -p fx-cli` and `cargo test -p fx-kernel` to verify.
- Do NOT refactor existing tui.rs rendering — only add streaming path alongside existing batch path.
- The batch (non-streaming) path must continue to work for providers that don't support streaming.
