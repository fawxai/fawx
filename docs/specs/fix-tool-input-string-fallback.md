# Fix: Tool Input String Fallback Causes API Rejection

## Problem

When tool call arguments fail to parse as JSON, the fallback stores a `Value::String` in `ContentBlock::ToolUse { input }`. On the next turn, the conversation history replays this string value to the provider, which expects a JSON object (`"input": {}`). Anthropic rejects with:

```
messages.35.content.0.tool_use.input: Input should be a valid dictionary
```

## Root Cause

Two call sites:

1. `engine/crates/fx-llm/src/streaming.rs:325`:
```rust
serde_json::from_str(normalized).unwrap_or_else(|_| Value::String(arguments.to_string()))
```

2. `engine/crates/fx-llm/src/openai.rs:698`:
```rust
serde_json::from_str(normalized).unwrap_or_else(|_| Value::String(value.to_string()))
```

Both use `Value::String` as a fallback, which silently passes through until the next conversation turn where the provider API rejects it.

## Fix

Replace the fallback with `Value::Object` wrapping:

```rust
serde_json::from_str(normalized).unwrap_or_else(|e| {
    tracing::warn!("tool arguments JSON parse failed: {e}, wrapping as __fawx_raw_args");
    serde_json::json!({ "__fawx_raw_args": arguments })
})
```

This ensures:
- The value is always a JSON object (satisfies provider APIs)
- The original string content is preserved for debugging
- A warning is logged so the issue is visible
- Downstream tool execution can detect `__fawx_raw_args` and handle gracefully

## Files

- `engine/crates/fx-llm/src/streaming.rs` (line ~325)
- `engine/crates/fx-llm/src/openai.rs` (line ~698)
- `engine/crates/fx-llm/src/openai_responses.rs` (check for same pattern)

## Tests

- Test that malformed JSON arguments produce `Value::Object` not `Value::String`
- Test that `__fawx_raw_args` key contains the original string
- Test roundtrip: malformed args → store → replay to Anthropic → no rejection
