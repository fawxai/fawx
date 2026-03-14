# Spec: Error Surfacing

**Status**: Ready for implementation  
**Crates touched**: `fx-core`, `fx-kernel`, `fx-cli`  
**Estimated scope**: ~180 lines production code + ~100 lines tests

---

## Problem

Async failures are silently swallowed. When a provider times out, a tool
fails in the background, a channel error occurs, or compaction fails — the
error goes to `tracing::warn/error` but never reaches the user's chat
surface (TUI, Telegram, HTTP SSE). The user has no way to know something
broke unless they're watching server logs.

For Fawx to be a daily driver, the user needs to see when things go wrong.

## Solution

Add a `StreamEvent::Error` variant to the existing streaming callback
system. Errors that are user-visible (not internal retries or infra noise)
get emitted through the same `StreamCallback` that already delivers text
deltas, tool calls, and phase changes to every surface.

---

## Why StreamCallback, not EventBus?

The codebase has two event systems:

1. **`EventBus`** (`fx-core::EventBus`) — broadcast channel with
   `InternalMessage` variants. However, it's not actively consumed by any
   surface (TUI, Telegram, HTTP). It's wired into the `LoopEngine` but no
   subscriber reads from it in the CLI layer.

2. **`StreamCallback`** (`fx-kernel::streaming::StreamCallback`) — an
   `Arc<dyn Fn(StreamEvent) + Send + Sync>` passed to `LoopEngine::run()`.
   This IS actively consumed: the TUI uses it for rendering, HTTP SSE uses
   it for streaming responses, headless mode uses it for JSON output. Every
   surface already handles `StreamEvent`.

Using `StreamCallback` means errors automatically reach every surface with
zero additional wiring per channel. The EventBus is the wrong abstraction
for this — it would require adding subscribers in every surface.

---

## Design

### 1. Add `Error` variant to `StreamEvent` (`fx-kernel/src/streaming.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamEvent {
    // ... existing variants ...

    /// An error occurred that the user should see.
    ///
    /// Not all errors are surfaced — only those that are user-visible
    /// and actionable. Internal retries, fallbacks, and infra noise
    /// stay in tracing logs.
    Error {
        /// Machine-readable error category.
        category: ErrorCategory,
        /// Human-readable error message for the user.
        message: String,
        /// Whether the error is recoverable (engine continues)
        /// or fatal (engine stops).
        recoverable: bool,
    },
}

/// Categories of user-visible errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// LLM provider error (rate limit, auth, timeout, API error).
    Provider,
    /// Tool execution failed and was not retried.
    ToolExecution,
    /// Channel/surface error (send failure, parse error).
    Channel,
    /// Compaction or memory operation failed.
    Memory,
    /// Configuration or system error.
    System,
}
```

### 2. Add helper method to `LoopEngine` (`fx-kernel/src/loop_engine.rs`)

```rust
impl LoopEngine {
    /// Emit a user-visible error through the stream callback.
    fn emit_error(
        stream: &CycleStream<'_>,
        category: ErrorCategory,
        message: impl Into<String>,
        recoverable: bool,
    ) {
        if let Some(callback) = &stream.callback {
            callback(StreamEvent::Error {
                category,
                message: message.into(),
                recoverable,
            });
        }
    }
}
```

### 3. Surface errors at key failure points

Replace silent `tracing::warn/error` calls with `tracing` + `emit_error`
at these specific locations:

#### A. Provider errors (loop_engine.rs — `reason` and `synthesize` methods)

When `LlmProvider::complete()` or streaming fails, the error currently
propagates as `LoopError`. Before returning the error, emit it:

```rust
// In the reason step, when the LLM call fails:
Err(error) => {
    Self::emit_error(
        &stream,
        ErrorCategory::Provider,
        format!("LLM call failed: {error}"),
        false, // fatal — engine stops
    );
    Err(LoopError::from(error))
}
```

The LoopEngine already handles provider errors by returning `LoopError`.
The change is: ALSO emit the error to the stream before returning, so the
user sees it on their surface.

Find the error paths in:
- `reason()` method (line ~1363+): LLM completion failure
- `synthesize()` method: LLM completion failure  
- `execute_tool_round()`: when tool continuation LLM call fails

#### B. Tool execution errors (loop_engine.rs — `execute_tools` in act step)

When a tool fails and the engine does NOT retry (retry budget exhausted
or error is not retryable), emit:

```rust
Self::emit_error(
    &stream,
    ErrorCategory::ToolExecution,
    format!("Tool '{}' failed: {}", tool_name, error),
    true, // recoverable — engine continues with error result
);
```

Find in: `execute_tool_round()` method, where tool errors are collected.
Note: tool errors that the model sees and handles are already surfaced
(the model gets the error in the tool result). Only emit for errors that
the user should ALSO see (repeated failures, unexpected crashes).

#### C. Compaction memory flush failure (loop_engine.rs — `compact_if_needed`)

Already has a `tracing::warn!`. Add error emission:

```rust
if let Err(err) = flush.flush(&evicted, scope.as_str()).await {
    tracing::warn!(...);  // keep existing
    Self::emit_error(
        &stream,  // NOTE: compact_if_needed doesn't have stream access
        ErrorCategory::Memory,
        format!("Memory flush failed during compaction: {err}"),
        true,
    );
}
```

**Problem**: `compact_if_needed` doesn't have access to `CycleStream`.
Two options:
1. Pass `CycleStream` through — invasive, changes many signatures.
2. Use the `EventBus` for memory errors only — already available on LoopEngine.
3. Store a `StreamCallback` on the engine for out-of-band errors.

**Recommended**: Option 3. Add an optional `error_callback: Option<StreamCallback>`
to `LoopEngine` (set via builder). Used for errors that occur outside the
main cycle (compaction, background operations). This keeps `compact_if_needed`
clean while still surfacing errors.

```rust
// In LoopEngine struct:
error_callback: Option<StreamCallback>,

// In LoopEngineBuilder:
pub fn error_callback(mut self, cb: StreamCallback) -> Self {
    self.error_callback = Some(cb);
    self
}

// In compact_if_needed:
if let Err(err) = flush.flush(&evicted, scope.as_str()).await {
    tracing::warn!(...);
    if let Some(cb) = &self.error_callback {
        cb(StreamEvent::Error {
            category: ErrorCategory::Memory,
            message: format!("Memory flush failed: {err}"),
            recoverable: true,
        });
    }
}
```

### 4. Handle `StreamEvent::Error` in surfaces (`fx-cli`)

#### A. HTTP SSE (http_serve.rs)

The SSE handler already matches on `StreamEvent` variants. Add:

```rust
StreamEvent::Error { category, message, recoverable } => {
    // Send as SSE event with type "error"
    yield Ok(Event::default()
        .event("error")
        .json_data(serde_json::json!({
            "category": category,
            "message": message,
            "recoverable": recoverable,
        }))
        .unwrap_or_else(|_| Event::default()));
}
```

#### B. Headless JSON mode (headless.rs)

Already matches `StreamEvent` for JSON output:

```rust
StreamEvent::Error { message, recoverable, .. } => {
    let prefix = if recoverable { "warning" } else { "error" };
    eprintln!("[{prefix}] {message}");
}
```

#### C. Telegram channel

The Telegram channel receives the final response, not individual stream
events. For now, errors in the stream will be visible in TUI and HTTP but
not Telegram. This is acceptable for Phase 2 — Telegram error forwarding
can be a follow-up when we add the native GUI.

---

## What NOT to surface

These errors stay in `tracing` only — they are NOT emitted to the user:

- **Compaction cooldown skip** — internal bookkeeping, not actionable
- **Summarization fallback to sliding window** — automatic recovery, no user action needed
- **Tool call with malformed JSON args** — handled by dropping the call, model adapts
- **SSE slow client disconnect** — client-side issue
- **Tailscale IP detection failure** — startup-only, not runtime
- **Config manager lock poisoned** — internal state, already recovered
- **Credential store fallback** — automatic, not actionable
- **WASM skill load failure** — startup-only

---

## Testing

### Unit tests in `fx-kernel` (streaming.rs)

1. `error_event_serializes_correctly` — Verify `StreamEvent::Error` round-trips
   through serde_json.
2. `error_category_serializes_as_snake_case` — Verify `ErrorCategory::ToolExecution`
   serializes as `"tool_execution"`.

### Unit tests in `fx-kernel` (loop_engine.rs)

3. `provider_error_emits_stream_error` — Mock LLM that returns error, verify
   `StreamEvent::Error` received by callback with `category: Provider`.
4. `error_callback_receives_memory_errors` — Set error_callback, trigger
   compaction with failing flush, verify callback receives `ErrorCategory::Memory`.

### Unit tests in `fx-cli` (headless.rs or http_serve.rs)

5. `sse_error_event_has_correct_format` — Verify SSE output includes
   `event: error` with JSON data containing category/message/recoverable.

---

## File changes summary

| File | Change |
|------|--------|
| `fx-kernel/src/streaming.rs` | Add `Error` variant to `StreamEvent`, add `ErrorCategory` enum |
| `fx-kernel/src/loop_engine.rs` | Add `error_callback` field + builder, `emit_error` helper, emit errors at provider/tool/memory failure points |
| `fx-cli/src/http_serve.rs` | Handle `StreamEvent::Error` in SSE stream |
| `fx-cli/src/headless.rs` | Handle `StreamEvent::Error` in JSON output |

---

## Scope control

This spec intentionally limits the surface area:

- **Only 3-4 error emission points** in the loop engine (provider, tool, memory).
  We do NOT audit all 40+ `tracing::warn` calls. Most are startup-only or
  have automatic recovery. We surface the ones that matter at runtime.
- **No Telegram changes** — the channel doesn't consume StreamEvents individually.
  This is a follow-up.
- **No new error types** — we reuse existing error flows and add emission
  alongside them.
- **No error aggregation or rate limiting** — if the same error fires 10 times,
  the user sees it 10 times. Dedup is a follow-up.
