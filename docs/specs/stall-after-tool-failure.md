# ⚠️ SUPERSEDED — See `loop-simplification.md`

This spec proposed modifying `verify()` to handle tool failure stalling. It has been superseded by `loop-simplification.md` which removes `verify()` entirely. Kept as reference for the analysis that led to the redesign.

---

# Spec: Fix Agent Stalling After Tool Failures (SUPERSEDED)

## Problem

When tools fail (e.g., `write_file` with malformed JSON, permission denied, file not found), the agent stalls: it produces a text response explaining the failure ("I'm having trouble writing the file...") instead of retrying with a different approach. The loop then treats this text as a successful completion and terminates.

### Root Cause

The verification step in `should_continue()` uses a simple heuristic:
- If `response_text` is non-empty → `outcome_satisfactory = true` → loop terminates
- If `response_text` is empty AND tools failed → discrepancy → loop retries

So when the model responds to tool failures with explanatory text (a natural behavior), the loop exits successfully. The model's failure explanation becomes the final output.

### What Should Happen

The model should retry with alternative approaches (different tool, different arguments, different strategy) before giving up. The loop should recognize that a response about tool failure is not task completion.

## Solution: Tool-Failure Awareness in Verify Step

### Option A: Partial Success Detection (Recommended)

Don't treat a response as satisfactory when there were tool failures in the same action, even if the response is non-empty. The model should explicitly signal "I cannot complete this task" rather than the loop inferring completion from any text output.

Changes to `verify()`:
```rust
// Current: tool failure with no response → discrepancy
if has_tool_failure && !has_response {
    discrepancies.push("tool calls failed and produced no response".to_string());
}

// New: tool failure with response → still a discrepancy (but lower severity)
// The model responded, but it was in the context of failed tools.
// Give it one more chance to try a different approach.
if has_tool_failure && has_response && !self.retry_after_tool_failure_spent {
    discrepancies.push("tool calls failed; response may be premature".to_string());
    self.retry_after_tool_failure_spent = true; // Only retry once
}
```

This gives the model one retry opportunity after tool failures, even when it produced text. The `retry_after_tool_failure_spent` flag prevents infinite loops — on the second failure, the response is accepted.

### Option B: Distinguish "I failed" from "Here's your answer"

Use the response content to detect failure explanations vs. genuine answers. Fragile (relies on text heuristics) but more precise.

### Option C: Budget pressure via system message

When tools fail, inject a system message in the next reasoning request: "Your previous tool calls failed. Try a different approach before responding with text." This is advisory (same weakness as the nudge) but zero-risk.

## Recommendation

**Option A** — it's deterministic, bounded (one retry), and doesn't rely on heuristics or advisory messages. Combined with the `content_ref` spec (which reduces write_file failures in the first place), this covers the stalling behavior.

## Implementation

### Changes

1. **`engine/crates/fx-kernel/src/loop_engine.rs`**:
   - Add `retry_after_tool_failure_spent: bool` field to `LoopEngine`
   - Initialize to `false` in builder and `prepare_cycle()`
   - In `verify()`: when `has_tool_failure && has_response && !self.retry_after_tool_failure_spent`, add discrepancy and set flag
   - This causes `should_continue()` to return `Continuation::Continue(...)` instead of `Complete`

2. **Continuation message** for this case:
   ```
   "Tool calls failed but a response was generated. Try a different approach to complete the task — use alternative tools or arguments instead of explaining the failure."
   ```

### Tests

1. `tool_failure_with_response_triggers_retry` — tool fails, model responds with text, verify finds discrepancy, loop continues
2. `tool_failure_retry_only_once` — second failure with text response → accepted (no infinite loop)
3. `tool_success_with_response_completes_normally` — no tool failures → behavior unchanged
4. `tool_failure_retry_flag_resets_on_cycle_start` — `prepare_cycle()` clears the flag

### Edge Cases

- **Mixed results (some succeed, some fail)**: If any tool failed and the response looks like an apology, retry. If most tools succeeded and the response addresses the user's question, accept.
- **All tools blocked by retry policy**: The model can't retry the same calls, so it should try different tools. The continuation message guides this.
- **Budget exhaustion during retry**: Normal budget checks still apply; retry doesn't bypass budget limits.

## Interaction with Other Specs

- **TerminationConfig (PR #1577)**: Tool-only turn counting is orthogonal. This spec handles the case where the model DOES produce text (just the wrong kind).
- **Content-ref (write-file-content-ref.md)**: Reduces the frequency of write_file failures, making this retry less necessary. But other tools can still fail.
- **Smart tool retry (PR #1569)**: Per-call retry limits prevent retrying the same broken call. This spec handles the case where the model should try a DIFFERENT approach, not the same call.
