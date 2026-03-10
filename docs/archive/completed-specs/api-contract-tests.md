# API Contract Tests — Specification

**Status:** Draft  
**Scope:** fx-llm crate  
**Motivation:** Three thinking-related bugs (#1226, #1227, #1228) all passed unit tests but failed against Anthropic's real API. Each violated an API contract not encoded in our types.

---

## Problem

Unit tests verify our logic. They don't verify that the requests we build are valid according to the provider's API, or that we can deserialize every response shape the provider sends. The TUI smoke test catches these — but late, manually, and expensively.

### Bugs this would have caught

| PR | Bug | Layer |
|----|-----|-------|
| #1226 | Temperature sent with thinking enabled | Request validation |
| #1227 | max_tokens < thinking.budget_tokens | Request validation |
| #1228 | `thinking` content block unknown to serde | Response fixture |

---

## Layer 1: Request Validation

### What
A `validate_anthropic_request()` function that encodes known Anthropic API constraints. Called automatically in `build_request_body()` during debug/test builds. Panics on violation with a clear message.

### Constraints to encode

```rust
/// Validates an Anthropic request body against known API constraints.
/// Panics in debug/test builds on violation.
#[cfg(debug_assertions)]
fn validate_request(body: &AnthropicRequestBody) {
    // 1. Temperature must be None when thinking is enabled
    //    (Anthropic requires temperature=1 or omitted)
    
    // 2. max_tokens must be > thinking.budget_tokens
    
    // 3. model must be non-empty
    
    // 4. messages must be non-empty
    
    // 5. thinking.budget_tokens must be > 0 if thinking is enabled
    
    // 6. thinking.budget_tokens must be <= MAX_THINKING_BUDGET
    
    // 7. max_tokens must be > 0
}
```

### Integration point

```rust
fn build_request_body(&self, request: &CompletionRequest, stream: bool) -> Result<...> {
    // ... existing code ...
    let body = AnthropicRequestBody { ... };
    
    #[cfg(debug_assertions)]
    validate_request(&body);
    
    Ok(body)
}
```

### Why debug-only
- Zero runtime cost in release builds
- Every `cargo test` run automatically validates every request we construct
- Catches constraint violations at the test that builds the bad request, not 3 layers later

### Tests
- `validate_request_rejects_temperature_with_thinking` — expects panic
- `validate_request_rejects_low_max_tokens` — expects panic  
- `validate_request_accepts_valid_thinking_request` — no panic
- `validate_request_accepts_valid_non_thinking_request` — no panic

---

## Layer 2: Response Fixture Tests

### What
Real Anthropic API responses saved as JSON fixtures. Deserialized in tests to verify our types handle every response shape.

### Fixture directory

```
engine/crates/fx-llm/tests/
├── fixtures/
│   ├── anthropic/
│   │   ├── response_text.json            # Simple text response
│   │   ├── response_thinking.json        # Thinking + text response
│   │   ├── response_tool_call.json       # Tool use response
│   │   ├── response_multi_tool.json      # Multiple tool calls
│   │   ├── response_thinking_tool.json   # Thinking + tool use
│   │   ├── stream_text.sse              # Streaming text (raw SSE)
│   │   ├── stream_thinking.sse          # Streaming with thinking blocks
│   │   ├── stream_tool_call.sse         # Streaming tool call
│   │   ├── stream_multi_tool.sse        # Streaming multiple tools
│   │   └── error_invalid_request.json   # 400 error response
```

### How to capture fixtures
1. Add a `#[cfg(test)] fn capture_response()` helper that logs raw response bytes
2. Run once manually with a real API key
3. Sanitize (remove any PII from prompts/responses) and commit fixtures
4. OR: hand-craft fixtures based on Anthropic's documentation examples

### Fixture test pattern

```rust
#[test]
fn deserialize_thinking_response_fixture() {
    let fixture = include_str!("fixtures/anthropic/response_thinking.json");
    let body: AnthropicResponseBody = serde_json::from_str(fixture)
        .expect("fixture must deserialize — if this fails, our types don't match Anthropic's schema");
    
    // Verify structure
    assert!(body.content.iter().any(|b| matches!(b, AnthropicContentBlock::Thinking { .. })));
    assert!(body.content.iter().any(|b| matches!(b, AnthropicContentBlock::Text { .. })));
}

#[test]
fn parse_thinking_stream_fixture() {
    let fixture = include_str!("fixtures/anthropic/stream_thinking.sse");
    let chunks = AnthropicProvider::parse_sse_payload(fixture)
        .expect("fixture must parse");
    
    // Verify thinking blocks produce no output
    let text_chunks: Vec<_> = chunks.iter()
        .filter(|c| c.delta_content.is_some())
        .collect();
    assert!(!text_chunks.is_empty(), "must have text output");
}
```

### What each fixture covers

| Fixture | Verifies |
|---------|----------|
| `response_text.json` | Basic deserialization, text extraction |
| `response_thinking.json` | Thinking block skipped, text preserved |
| `response_tool_call.json` | Tool name, id, arguments parsed |
| `response_multi_tool.json` | Multiple tool calls in one response |
| `response_thinking_tool.json` | Thinking + tool call coexistence |
| `stream_text.sse` | SSE parsing, text delta assembly |
| `stream_thinking.sse` | Thinking deltas skipped, text deltas preserved |
| `stream_tool_call.sse` | Streaming tool call argument assembly |
| `stream_multi_tool.sse` | Multiple streaming tool calls with correct index tracking |
| `error_invalid_request.json` | Error response handling |

---

## Scope & Constraints

### In scope
- `AnthropicProvider` request building and response parsing
- Request validation function (debug-only)
- Response fixture tests (JSON + SSE)
- All tests in fx-llm crate

### Out of scope (follow-up)
- **Layer 3 (live contract tests)** — `#[ignore]` tests against real API. TUI smoke test covers this for now.
- **OpenAI provider fixtures** — same pattern, separate PR
- **fawx-test harness integration** — behavioral scenarios that run the binary
- **CI fixture refresh** — automated re-capture when API versions change

### YAGNI boundary
- No mock HTTP server framework — fixtures are simpler and sufficient
- No fixture generation tooling — hand-craft or one-time capture
- No provider-version tracking — update fixtures when a test breaks

---

## Implementation Plan

### PR 1: Request validation (~100 lines)
1. Add `validate_request()` with `#[cfg(debug_assertions)]`
2. Call from `build_request_body()`
3. Add 4 validation tests (2 rejection, 2 acceptance)
4. Verify all existing tests still pass (they should — our requests are now correct post-#1226/#1227)

### PR 2: Response fixtures (~200 lines + fixtures)
1. Create `tests/fixtures/anthropic/` directory
2. Hand-craft 5 non-streaming fixtures from Anthropic docs
3. Hand-craft 4 streaming SSE fixtures
4. Add fixture deserialization tests
5. Add fixture-to-parsed-response tests (verify full pipeline)

### Sizing
- **PR 1:** ~100 lines, ~1 hour. Single file change.
- **PR 2:** ~200 lines code + ~500 lines fixtures, ~2 hours. New test file + fixtures directory.
- **Total:** ~2 PRs, both simple, no architectural changes.

---

## Success Criteria

After this lands:
1. Any future request constraint violation is caught by `cargo test` (not TUI smoke test)
2. Any future response shape change is caught by fixture deserialization (not runtime crash)
3. Zero false positives — validation only fires on actual API violations
4. Zero runtime cost — all validation is `#[cfg(debug_assertions)]` only
