# Phase 2a: Streaming Responses (Token-by-Token Output)

*Migrated from issue #1279. Originally Phase 2+, reclassified to Phase 2a — essential for production UX.*

## Summary

Stream tokens from the LLM to the user in real-time instead of waiting for complete responses. Currently Fawx returns nothing until the full loop cycle finishes — multi-tool chains mean 10-30+ seconds of dead air.

## Current State

- `LoopEngine::run_cycle()` returns a complete `LoopResult` — no intermediate output
- `fx-llm` already supports streaming internally (Anthropic and OpenAI providers use streaming for token counting)
- `fawx-tui` already has `BackendEvent::TextDelta` variant — unused
- `fawx_backend.rs` has SSE parsing scaffolding
- HTTP server returns complete JSON responses from `/message`
- Telegram channel has partial streaming enabled (via OpenClaw, not Fawx)

## Design

### Layer 1: Engine Streaming (fx-kernel)

Add a callback/channel mechanism to `LoopEngine`:

```rust
pub enum StreamEvent {
    /// Token from the LLM during reasoning/synthesis
    TextDelta { text: String },
    /// Tool call being constructed (name known, args streaming)
    ToolCallStart { id: String, name: String },
    /// Tool call arguments streaming
    ToolCallDelta { id: String, args_delta: String },
    /// Tool call complete, execution starting
    ToolCallComplete { id: String, name: String, arguments: String },
    /// Tool execution result
    ToolResult { id: String, output: String, is_error: bool },
    /// Loop cycle phase change
    PhaseChange { phase: String },  // "perceive", "reason", "act", "synthesize"
    /// Final response complete
    Done { response: String },
}

pub type StreamCallback = Arc<dyn Fn(StreamEvent) + Send + Sync>;
```

Integrate into `run_cycle()`:
1. **Reason phase**: Forward `TextDelta` events from the LLM provider's stream
2. **Act phase**: Emit `ToolCallStart`, `ToolCallDelta`, `ToolCallComplete`, `ToolResult`
3. **Synthesize phase**: Forward `TextDelta` from synthesis response
4. **Between phases**: Emit `PhaseChange`

### Layer 2: Provider Streaming (fx-llm)

The providers already stream internally — they just consume the stream and return the final result. Refactor to expose the stream:

```rust
// Current (simplified)
pub trait LlmProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
}

// New: add streaming variant
pub trait LlmProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
    async fn stream(&self, request: CompletionRequest, callback: StreamCallback) -> Result<CompletionResponse>;
}
```

Default `stream()` implementation calls `complete()` and emits a single `TextDelta` with the full response (graceful fallback for providers that don't support streaming).

### Layer 3: HTTP Server (SSE)

New endpoint: `GET /stream` or modify `POST /message` to support streaming via `Accept: text/event-stream`:

```
POST /message
Authorization: Bearer <token>
Accept: text/event-stream
Content-Type: application/json

{"message": "Write a fibonacci function"}
```

Response (SSE):
```
event: phase
data: {"phase": "reason"}

event: text_delta
data: {"text": "I'll write"}

event: text_delta
data: {"text": " a fibonacci"}

event: tool_call
data: {"id": "tc_1", "name": "write_file", "status": "start"}

event: tool_result
data: {"id": "tc_1", "output": "Written to fib.rs", "is_error": false}

event: text_delta
data: {"text": "I've created"}

event: done
data: {"response": "I'll write a fibonacci...I've created fib.rs with..."}
```

Non-streaming clients continue to use `Accept: application/json` and get the current behavior.

### Layer 4: TUI Integration

**HTTP mode (`fawx-tui`):**
- `HttpBackend` sends `Accept: text/event-stream`
- Parses SSE events → maps to `BackendEvent::TextDelta`
- TUI renders incrementally (already has the plumbing)

**Embedded mode (`fawx-tui --embedded`):**
- `EmbeddedBackend` passes `StreamCallback` to engine
- Callback maps `StreamEvent` → `BackendEvent` directly (no HTTP/SSE overhead)
- This is the fast path — in-process, zero serialization

### Layer 5: Channel Integration

**Telegram channel:**
- Buffer `TextDelta` events
- Emit partial updates on configurable interval (e.g., every 500ms or every sentence)
- Matches OpenClaw's `streaming: "partial"` behavior
- Final `Done` event sends the complete message (edits the partial)

**Future channels:** Same pattern — buffer + interval + final

## Configuration

```toml
[streaming]
# Enable streaming (default: true)
enabled = true

# Minimum interval between partial updates for channels (ms)
# Prevents rate limiting on Telegram etc.
channel_interval_ms = 500

# Buffer text deltas and emit at word boundaries (reduces partial-word flicker)
word_boundary = true
```

## Implementation Phases

This is too large for a single PR. Break into 3:

### PR 1: fx-llm Provider Streaming (~200 lines)
- Add `stream()` method to `LlmProvider` trait with default impl
- Implement for Anthropic provider (already has SSE parsing)
- Implement for OpenAI provider (already has SSE parsing)
- Tests: stream callback receives tokens, final response matches non-streaming

### PR 2: Engine + HTTP Streaming (~300 lines)
- Add `StreamEvent` and `StreamCallback` to fx-kernel
- Wire into `LoopEngine::run_cycle()` — forward provider stream events
- Add SSE response support to HTTP server
- Tests: SSE endpoint returns events, non-streaming endpoint unchanged

### PR 3: TUI + Channel Streaming (~200 lines)
- Wire `HttpBackend` SSE parsing
- Wire `EmbeddedBackend` callback
- Add channel buffering (Telegram partial updates)
- Tests: TUI renders incrementally, channel respects interval

## Dependencies

None new. SSE is plain HTTP — no crate needed. Providers already parse SSE internally.

## Risks

- **Provider differences**: Anthropic and OpenAI stream differently (content_block_delta vs choices[0].delta). Abstraction layer must normalize.
- **Error handling during stream**: If the stream errors mid-response, need graceful recovery (show what we have + error indicator).
- **Tool calls during streaming**: Some providers stream tool call arguments character-by-character — need to buffer and emit at meaningful boundaries.
- **Backpressure**: If the TUI/channel can't keep up, buffer with bounded size and drop oldest deltas.

## Size Estimate

~700 lines total across 3 PRs + ~400 lines of tests.

## References

- OpenClaw streaming modes (partial, block, progress)
- Anthropic SSE format: `event: content_block_delta`, `data: {"delta": {"text": "..."}}`
- OpenAI SSE format: `data: {"choices": [{"delta": {"content": "..."}}]}`
- Issue #1279
