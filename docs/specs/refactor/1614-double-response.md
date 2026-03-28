# Spec: #1614 — Fix Double Response (Text + Tool Call in Same Turn)

## Status
Foundation laid. `ActionNextStep` provides typed continuation signals. But the actual mixed-response handling in `act_with_tools` is unchanged.

## Goal
When the model emits both text content AND tool calls in a single response, the loop must continue executing tools and not treat the text as a final response.

## Current State (codex/provider-owned-loop-refactor branch)

File: `engine/crates/fx-kernel/src/loop_engine.rs` (24,098 lines)

### The bug path

In `act_with_tools` (line 5175), tool rounds produce `ToolRoundOutcome::Response(response)`. The handler at the bottom of the function (around line 5340):

```rust
ToolRoundOutcome::Response(response) => {
    if !response.tool_calls.is_empty() {
        // Has tool calls → continue to next round
        // (text in the response is NOT explicitly captured)
        state.current_calls = capped;
        continue;
    }

    // No tool calls → finalize as response
    let response = self.continue_truncated_response(...).await?;
    return Ok(self.finalize_tool_response(...));
}
```

**When the model returns text + tool calls:**
- `!response.tool_calls.is_empty()` is true → continues
- The text content from the response lives in `response.content` (content blocks)
- It gets included in `continuation_messages` via `append_tool_round_messages` in the *next* round
- But the text is appended as the *assistant's prior message*, not preserved as partial output

**The problem:** On the final round, when the model returns text-only after prior mixed responses, `finalize_tool_response` extracts text from that final response only. Any text emitted in earlier mixed-content rounds is lost from the user-visible output (though it's in the continuation messages).

**The user experience:** Model does 5 searches, returns a summary + one more tool call, loop continues, then model returns a final brief response. The user sees only the brief final response, not the earlier summary.

### Where the text goes

`CompletionResponse.content` is a `Vec<ContentBlock>`. Content blocks include `ContentBlock::Text { text }` and `ContentBlock::ToolUse { ... }`. When there are both:
- `extract_response_text()` extracts text blocks
- `response.tool_calls` extracts tool use blocks
- Both are present simultaneously

### The `ActionNextStep` opportunity

The outer `run_cycle` loop already handles `ActionNextStep`. The fix should surface partial text through `ActionContinuation`:

```rust
ActionNextStep::Continue(ActionContinuation {
    partial_response: Some("summary text from mixed response"),
    ...
})
```

The `ActionContinuation` struct already has `partial_response: Option<String>`.

## Fix

### In `act_with_tools`, when continuing after a mixed response:

Before `continue;`, extract and accumulate the text:
```rust
ToolRoundOutcome::Response(response) => {
    if !response.tool_calls.is_empty() {
        // Capture text from mixed-content response
        let mixed_text = extract_response_text(&response);
        if !mixed_text.trim().is_empty() {
            state.accumulated_text.push(mixed_text);
        }
        // ... existing tool call handling ...
        state.current_calls = capped;
        continue;
    }
    // ... existing finalize path ...
}
```

### In `finalize_tool_response` and `synthesize_tool_fallback`:

Prepend accumulated text to the final response:
```rust
let mut full_text = state.accumulated_text.join("\n\n");
if !final_response_text.is_empty() {
    full_text.push_str("\n\n");
    full_text.push_str(&final_response_text);
}
```

### Add to `ToolRoundState`:
```rust
struct ToolRoundState {
    // ... existing fields ...
    accumulated_text: Vec<String>,  // text from mixed-content responses
}
```

## Deliverables

1. Add `accumulated_text: Vec<String>` to `ToolRoundState`
2. In `act_with_tools`, capture text from mixed-content responses before continuing
3. In `finalize_tool_response` and `synthesize_tool_fallback`, merge accumulated text with final response
4. Preserve existing behavior for text-only and tool-only responses (no regression)
5. Add tests:
   - Mixed text+tool response → text is preserved in final output
   - Multiple rounds of mixed responses → all text accumulated in order
   - Tool-only responses → no change
   - Text-only responses → no change
6. Also capture the accumulated text in `ActionContinuation.partial_response` when returning `ActionNextStep::Continue`

## Files to modify
- `engine/crates/fx-kernel/src/loop_engine.rs` (ToolRoundState, act_with_tools, finalize paths)

## Not in scope
- Changes to streaming event emission (text from mixed responses may already stream to user via the streaming callback; this fix ensures the final `ActionResult` captures it)
- Changes to how `continuation_messages` are built
- Changes to `run_cycle` or `ActionNextStep` handling (those already work correctly)

## Depends on
- #1644 should be complete first so that `ActionContinuation.partial_response` is the stable mechanism
