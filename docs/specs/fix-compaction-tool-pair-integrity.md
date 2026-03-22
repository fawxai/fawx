# Fix: Compaction Tool Pair Integrity

## Problem

`emergency_compact` drops all middle messages between the system prefix and the
preserved tail without checking tool pair integrity. When an assistant message
containing `ToolUse { id: "X" }` falls in the evicted middle but its matching
`ToolResult { tool_use_id: "X" }` is in the preserved tail, the result is an
orphaned `tool_result`.

**OpenAI path:** `validate_tool_message_sequence` in `openai_responses.rs`
catches the orphan and returns a hard error — tool continuation stops entirely.

**Anthropic path:** No equivalent validation exists. The malformed request goes
to the API, which either rejects it with a 400 or produces a degenerate/empty
response (the "incomplete turn" symptom).

### Why sliding compaction doesn't have this bug

`sliding_compaction_result` uses `paired_removal_offsets` which transitively
finds all partner messages sharing tool IDs and evicts them atomically. If a
partner is protected, the entire group is skipped.

### Why `prune_tool_blocks` doesn't have this bug

It converts both `ToolUse` and `ToolResult` blocks to `Text` blocks, destroying
the structural pair on both sides. No orphaned `ToolResult` block exists after
pruning.

## Root Cause

`emergency_compact` in `conversation_compactor.rs` at line ~486:
```rust
pub fn emergency_compact(messages: &[Message], preserve_recent_turns: usize) -> CompactionResult {
    let bounds = zone_bounds(messages, preserve_recent_turns);
    let evicted_indices: Vec<usize> = (bounds.prefix_end..bounds.tail_start).collect();
    // ^^^ Drops ALL middle messages regardless of tool pairs
```

It does not call `debug_assert_tool_pair_integrity` on its output.

## Fix (3 parts)

### Part 1: Make `emergency_compact` tool-pair-aware

Expand the preserved tail to include any message that is the partner of a
message already in the tail. Specifically:

1. Compute `bounds` as today (system prefix + recent turns tail).
2. Collect all tool IDs referenced in the tail (`ids_referenced_in_tail`).
3. Walk the middle zone: any message whose tool IDs overlap with the tail's
   referenced IDs is "protected" — it must be kept.
4. Transitively expand: if a newly-protected message references additional tool
   IDs, check if those IDs have partners in the middle that also need
   protection.
5. Assemble the result: prefix + protected middle messages (in original order) +
   emergency marker + tail.

The function `protected_middle_indices` already exists and does exactly this for
sliding compaction. Reuse it in `emergency_compact`.

**Implementation:**

```rust
pub fn emergency_compact(messages: &[Message], preserve_recent_turns: usize) -> CompactionResult {
    let bounds = zone_bounds(messages, preserve_recent_turns);
    let protected_middle = protected_middle_indices(messages, &bounds);
    
    let evicted_indices: Vec<usize> = (bounds.prefix_end..bounds.tail_start)
        .filter(|i| !protected_middle.contains(i))
        .collect();
    let compacted_count = evicted_indices.len();
    
    if compacted_count == 0 {
        return CompactionResult {
            messages: messages.to_vec(),
            compacted_count: 0,
            estimated_tokens: ConversationBudget::estimate_tokens(messages),
            used_summarization: false,
            evicted_indices,
        };
    }

    let mut result_messages = Vec::new();
    result_messages.extend_from_slice(&messages[..bounds.prefix_end]);
    
    // Insert protected middle messages in original order
    for i in bounds.prefix_end..bounds.tail_start {
        if protected_middle.contains(&i) {
            result_messages.push(messages[i].clone());
        }
    }
    
    if compacted_count > 0 {
        result_messages.push(emergency_compaction_marker_message(compacted_count));
    }
    
    result_messages.extend_from_slice(&messages[bounds.tail_start..]);
    
    debug_assert_tool_pair_integrity(&result_messages);
    
    CompactionResult {
        estimated_tokens: ConversationBudget::estimate_tokens(&result_messages),
        messages: result_messages,
        compacted_count,
        used_summarization: false,
        evicted_indices,
    }
}
```

### Part 2: Add post-compaction invariant check in `compact_if_needed`

After all tiers run and before returning, validate tool pair integrity on the
final result. This is a safety net — if any tier (current or future) produces
orphans, the check catches it at the source rather than at send time.

In `loop_engine.rs`, method `compact_if_needed`, add after the tier cascade and
before `ensure_within_hard_limit`:

```rust
// Validate tool pair integrity after compaction
debug_assert_tool_pair_integrity(current.as_ref());
```

Import `debug_assert_tool_pair_integrity` from `conversation_compactor`.
Make it `pub(crate)` if not already public.

### Part 3: Add Anthropic adapter tool pair validation

In `anthropic.rs`, add the same validation that `openai_responses.rs` already
has. Before building the request body in the `complete` method, validate tool
message sequence.

Reuse or extract the validation logic. Options:
- **Option A (preferred):** Move `validate_tool_message_sequence` to
  `fx_llm/src/types.rs` (or a new `fx_llm/src/validation.rs`) as a shared
  function. Both adapters call it.
- **Option B:** Duplicate the function in `anthropic.rs`. Less DRY but
  smaller diff.

Use Option A — extract to a shared module.

## Files to Change

1. `engine/crates/fx-kernel/src/conversation_compactor.rs`
   - Rewrite `emergency_compact` to use `protected_middle_indices`
   - Add `debug_assert_tool_pair_integrity` call on output
   - Make `debug_assert_tool_pair_integrity` pub(crate)

2. `engine/crates/fx-kernel/src/loop_engine.rs`
   - Add post-compaction `debug_assert_tool_pair_integrity` in `compact_if_needed`
   - Import the function

3. `engine/crates/fx-llm/src/validation.rs` (NEW)
   - Extract `validate_tool_message_sequence` as shared function

4. `engine/crates/fx-llm/src/openai_responses.rs`
   - Import from shared validation module instead of local function

5. `engine/crates/fx-llm/src/anthropic.rs`
   - Add `validate_tool_message_sequence` call in `complete` method
   - Import from shared validation module

## Tests Required

### conversation_compactor.rs tests

1. `emergency_compact_preserves_tool_pairs_across_boundary`
   - Setup: [user, tool_use("X"), tool_result("X"), user, assistant] with
     preserve_recent_turns=2
   - Assert: both tool_use and tool_result are present in output OR both are
     absent (atomic eviction)

2. `emergency_compact_keeps_tool_use_when_result_in_tail`
   - Setup: [user, tool_use("X"), user, assistant, tool_result("X"), user] with
     preserve_recent_turns=2 (tool_result in tail)
   - Assert: tool_use("X") is preserved (not evicted) because its partner is in
     the tail

3. `emergency_compact_evicts_complete_pairs_in_middle`
   - Setup: [user, tool_use("old"), tool_result("old"), user(big), assistant(big),
     user, assistant] with preserve_recent_turns=2
   - Assert: both tool_use("old") and tool_result("old") are evicted (pair is
     fully in middle, no tail references)

4. `emergency_compact_handles_multi_tool_chain`
   - Setup: multiple tool pairs, some in middle, one spanning middle/tail
   - Assert: spanning pair preserved, fully-middle pairs evicted

### anthropic.rs tests

5. `anthropic_rejects_orphaned_tool_result`
   - Build a CompletionRequest with orphaned tool_result
   - Assert: returns LlmError, does not send to API

### loop_engine.rs tests

6. `compact_if_needed_emergency_tier_preserves_tool_pairs`
   - Messages that trigger emergency compaction with tool pairs spanning zones
   - Assert: output has valid tool pair integrity

## Validation

After implementation:
```
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Both must pass with zero errors/warnings.

## Branch

`fix/compaction-tool-pair-integrity` from `origin/dev` (`93c227bf`)
PR targets `dev`.
