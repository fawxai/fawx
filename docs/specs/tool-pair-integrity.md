# Spec: Tool Pair Integrity in Compaction and Provider Serialization

## Problem Statement

Two related bugs share a root cause: the compaction system can break tool call/result pairs, and the OpenAI Responses provider validates against this but the test data doesn't include proper pairs.

### Symptom 1: CI Test Failures (2 tests)
- `openai_responses::tests::test_build_request_body_maps_tool_result_string_content`
- `openai_responses::tests::test_build_request_body_maps_tool_result_error_prefix`

Both construct a `CompletionRequest` with only a `Tool` role message (containing `ToolResult`) but no preceding `Assistant` message with the matching `ToolUse`. The `validate_tool_message_sequence()` validator (introduced in PR #1531) correctly rejects these as invalid.

### Symptom 2: GPT-5.4 Decompose Regression
When GPT-5.4 is the active model, Fawx outputs raw decomposition JSON as text instead of executing tool calls. Hypothesis: after compaction removes assistant messages containing `ToolUse` blocks while preserving subsequent `Tool` messages with `ToolResult` blocks, the OpenAI provider's validator rejects the entire request (or the request reaches the API with orphaned function_call_outputs, which OpenAI rejects). This forces the model into a degraded state where tool calling doesn't work.

### Symptom 3: "What's your question?" responses
Long sessions that hit compaction may strip tool history in ways that confuse the model about conversation state.

## Root Cause Analysis

### The Validator (correct behavior)
`validate_tool_message_sequence()` in `openai_responses.rs:763` tracks `seen_tool_calls` by scanning assistant messages for `ToolUse` blocks, then verifies every `ToolResult` references a previously seen `ToolUse` ID. This is correct — the OpenAI Responses API requires every `function_call_output` to have a preceding `function_call`.

### The Gap: `protected_middle_indices()` doesn't protect tool pairs
In `conversation_compactor.rs:158`, messages in the middle zone are protected only if:
1. They are system-like messages
2. Their tool IDs are **unresolved** (tool_use without any tool_result anywhere)
3. Their tool IDs are **referenced in the tail** (recent messages)

**Missing case:** If both the assistant (tool_use) and tool (tool_result) messages are in the middle zone, and neither is referenced in the tail, both are independently removable. The sliding compactor removes oldest-first, so it can remove the assistant message (index N) while keeping the tool message (index N+1), creating an orphaned tool_result.

### Affected Compaction Strategies

| Strategy | Risk | Explanation |
|----------|------|-------------|
| `SlidingWindowCompactor` | **HIGH** | Removes oldest-first from removable middle. Can split pairs. |
| `SummarizingCompactor` | **LOW** | Removes all summarizable (non-protected) middle at once, replacing with summary. If both messages are summarizable, both get removed. But if one is protected and the other isn't, it can split. |
| `emergency_compact` | **NONE** | Removes the entire middle zone. No splitting possible. |
| `prune_tool_blocks` | **NONE** | Replaces block content in-place with text summaries. Messages stay; roles preserved. No orphaning. |

### Why Anthropic isn't affected
The Anthropic provider (`anthropic.rs`) does NOT have `validate_tool_message_sequence()`. It sends whatever messages it receives. Anthropic's API is more tolerant of orphaned tool results (it ignores them or handles them gracefully). The OpenAI Responses API is stricter.

## Fix Scope

### Part 1: Protect tool pairs during compaction (root cause fix)

Modify `protected_middle_indices()` in `conversation_compactor.rs` to also protect messages whose tool IDs are referenced by OTHER messages in the middle zone.

Algorithm:
1. Build a set of all tool IDs present in middle-zone messages
2. For each middle-zone message, if any of its tool IDs appear in another middle-zone message, protect both
3. This ensures tool_use and tool_result stay together — either both are evicted or neither is

More precisely: build a map from tool_id → set of message indices. Any tool_id that appears in more than one message index means those messages are a pair. Protect all of them UNLESS all messages in the pair are eligible for removal (then remove all together).

The cleaner approach: instead of protecting pairs, ensure the compactor evicts pairs atomically. If it removes an assistant message with tool_use IDs, it must also remove any tool messages referencing those IDs (and vice versa). This keeps the "remove oldest first" logic but ensures pairs are never split.

**Recommended approach: atomic pair eviction.** When marking a message for removal in `remove_oldest_middle_until_target()`, also mark its pair partner(s). This is simpler than protecting everything — it naturally handles chains of tool calls.

### Part 2: Fix the test data (symptom fix)

Update the two failing tests to include a preceding assistant message with the matching `ToolUse` block:

```rust
messages: vec![
    Message {
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_1".to_string(),
            provider_id: None,
            name: "some_tool".to_string(),
            input: serde_json::json!({}),
        }],
    },
    Message {
        role: MessageRole::Tool,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "call_1".to_string(),
            content: Value::String("tool output".to_string()),
        }],
    },
],
```

This makes the tests properly represent real-world message sequences. The assertions about the output should still hold — they just verify the tool result mapping, and the assistant message maps to a function_call in the output.

### Part 3: Audit compaction output paths

Every path that produces a `CompactionResult` with modified messages must guarantee that if a `ToolResult` with ID X exists in the output, a preceding `ToolUse` with ID X also exists. Add a debug assertion or validation step at the end of each compaction strategy's `compact()` method.

Suggested: add `debug_assert_tool_pair_integrity(messages)` call at the end of `SlidingWindowCompactor::compact()` and `SummarizingCompactor::compact()`. In release builds this is zero-cost. In debug/test builds it catches regressions.

### Part 4: Regression tests

1. **Compactor test:** Construct a message sequence where assistant(tool_use:X) is older than tool(tool_result:X), both in middle zone, not referenced in tail. Verify compaction preserves both or removes both — never splits.

2. **Validator test:** Verify `validate_tool_message_sequence` accepts valid pairs and rejects orphaned results (already exists: `validate_responses_input_sequence_rejects_orphan_function_call_output`). Add a test for the Message-level validator too.

3. **End-to-end test:** Run a full compaction cycle (prune → slide → summarize) on a conversation with tool pairs in the middle zone. Verify the output passes `validate_tool_message_sequence()`.

## Files to Modify

| File | Change |
|------|--------|
| `engine/crates/fx-kernel/src/conversation_compactor.rs` | Atomic pair eviction in `remove_oldest_middle_until_target()`, debug assertion in compact outputs |
| `engine/crates/fx-llm/src/openai_responses.rs` | Fix test data (2 tests), add Message-level validator test |
| `engine/crates/fx-kernel/src/conversation_compactor.rs` (tests) | Regression tests for pair integrity |

## Non-goals

- Changing the validator behavior (it's correct)
- Adding validation to the Anthropic provider (not needed — API tolerates it)
- Modifying emergency_compact or prune_tool_blocks (already safe)

## Risk Assessment

- **Low risk:** The fix adds protection, doesn't change existing removal logic
- **Backward compatible:** Existing conversations are unaffected
- **Performance:** Negligible — one extra pass through middle-zone messages to build pair map
