# Agentic Loop Hardening — Spec

**Status:** Draft
**Date:** 2026-03-25
**Author:** Clawdio (architectural analysis), Joe (requirements from incident)
**Triggered by:** Mac Mini smoke test session where Fawx ran 11+ tool calls over 7 minutes without delivering the requested output, then confabulated its own session history.

---

## Problem Statement

During a real session, the user asked Fawx to "review the pdf and build turboquant.py." Fawx:

1. Analyzed the PDF correctly, presented a plan, and received explicit approval ("Yes, proceed").
2. Then ran 11+ tool calls over 7 minutes (read_file, search_text, run_command) without ever producing the requested output — kept "exploring" instead of building.
3. When asked "Did you get stuck?", confabulated an entirely different session history (claimed the conversation started empty with `<<HUMAN_CONVERSATION_START>>`).
4. Interleaved brief text ("Now I have a full picture...", "Now let me look at...") between tool batches, which reset the termination counter and prevented the nudge/strip mechanism from firing.
5. A tool call returned an error (1 tool, 1 failed) but Fawx continued past it silently — the user was never told.
6. Context usage was 9% — compaction never fired, so the original hypothesis (compaction context loss) was wrong.

---

## Root Cause Analysis

### Bug 1: Tool-only turn counter resets on interstitial text

**Location:** `loop_engine.rs` → `update_tool_only_turns()`

```rust
fn update_tool_only_turns(&mut self, action: &ActionResult) {
    if !action.tool_results.is_empty() && action.response_text.trim().is_empty() {
        self.consecutive_tool_only_turns += 1;
    } else if !action.response_text.trim().is_empty() {
        self.consecutive_tool_only_turns = 0;  // ← RESETS even if tools were also called
    }
}
```

**Problem:** When the model emits text + tool calls in the same turn (e.g., "Now let me look at the MLX training script..." + `read_file`), the counter resets to 0. The model learns to emit throwaway interstitial text to avoid the nudge/strip mechanism. The TerminationConfig's nudge_at_6/strip_at_9 never fires because the counter never reaches 6.

**Fix:** The counter should track _consecutive turns that include tool calls_ (regardless of whether text is also present), not _turns that are tool-only_. The reset condition should be: a turn that produces text with NO tool results. A turn that produces both text and tool results should still increment the counter.

### Bug 2: Nudge/strip only fires at perceive-time, not within tool continuation loop

**Location:** `loop_engine.rs` → `perceive()` (nudge injection), `reason()` (tool strip)

The current architecture is single-pass: `perceive → reason → decide → act`. The `act_with_tools` function runs the tool continuation loop (up to `max_iterations=10` rounds), but there is **no nudge/strip check inside this loop**. The nudge is only injected during `perceive` and tools are only stripped during `reason`, both of which execute once per cycle.

This means: within a single user turn, the model can chain 10 rounds of tool calls with no termination enforcement beyond the budget ceiling. The nudge_at_6 threshold is meaningless when 10 tool rounds happen inside a single act phase.

**Fix:** The tool continuation loop in `act_with_tools` must enforce its own progress check. After N tool rounds without the model producing a final text response (i.e., every round produces more tool calls), inject the nudge directive into the continuation messages. After N+M more rounds, strip tools from the continuation request, forcing a text response. This mirrors the cycle-level nudge/strip but operates within the tool continuation loop.

### Bug 3: Interstitial text in tool continuation constitutes "double response" (#1614)

**Location:** `act_with_tools` → `execute_tool_round` → `request_tool_continuation`

When the model returns text + tool_calls in a tool continuation response, the text gets included in the `response_text` of the eventual `ActionResult`. If the model does this on round 3 of 7, that interstitial text (e.g., "Now let me look at...") becomes visible to the user as a response, followed by a second actual response when the tool chain completes.

This is the root cause of the double-response pattern filed as #1614. The model emits partial "thinking aloud" text alongside tool calls.

**Fix:** During tool continuation rounds, if the response contains both text and tool calls, the text should be treated as internal reasoning (not streamed to the user as a response). Only the final response (the one with no tool calls) should be delivered. The interstitial text can optionally be preserved in the continuation context (so the model maintains coherence) but should not be streamed.

### Bug 4: Tool errors silently swallowed in continuation chain

**Location:** `act_with_tools` → `execute_tool_round`

When a tool call fails (returns `success: false`), the error is recorded in `all_tool_results` and sent back to the model as a tool result. However:
1. There is no requirement that the model acknowledges or reports the error to the user.
2. The `tool_synthesis_prompt` does include an error relay instruction, but that only fires during the fallback synthesis path (when the loop exhausts `max_iterations`). During normal continuation, the model sees the error but may choose to ignore it and call more tools.
3. There is no streaming event that surfaces tool failures to the user in real-time.

**Fix:** Two-part:
- **Streaming:** Emit a `StreamEvent::ToolError` (or similar) when a tool returns `success: false`, so the TUI/GUI can display the error immediately.
- **Continuation prompt:** When errors occur in a tool round, append a directive to the continuation messages: "One or more tools returned errors. Report these errors to the user before continuing with additional tool calls." This doesn't block execution but ensures the model prioritizes error communication.

### Bug 5: Model confabulation after long tool chains

This is not a code bug but a product-level concern. After 11+ tool calls and large amounts of tool output in context, the model lost track of its own conversation history and confabulated a different starting point. This suggests:
1. The conversation history in the continuation messages may be getting too large.
2. The model's attention to early messages degrades as the continuation context grows.

**Mitigation (not a fix — this is an LLM limitation):**
- Cap tool result sizes more aggressively within the continuation loop (the existing `max_tool_result_bytes` is 16KB per result — consider lowering for continuation rounds).
- Periodically inject a "task reminder" into continuation messages: "Your task: {original_user_message}. Stay focused on delivering this output." This helps anchor the model.
- The compaction summarize-before-slide fix (#1609) will help when continuation context triggers compaction, but the underlying issue is attention degradation, not compaction.

---

## Proposed Changes

### Change 1: Fix tool-turn counter logic

**File:** `engine/crates/fx-kernel/src/loop_engine.rs`

Rename `consecutive_tool_only_turns` → `consecutive_tool_turns` (semantic rename).

New logic:
```rust
fn update_tool_turns(&mut self, action: &ActionResult) {
    if !action.tool_results.is_empty() {
        // Any turn with tool results increments, even if text was also emitted.
        self.consecutive_tool_turns += 1;
    } else {
        // Only a pure text turn (no tools) resets the counter.
        self.consecutive_tool_turns = 0;
    }
}
```

### Change 2: Tool continuation loop progress enforcement

**File:** `engine/crates/fx-kernel/src/loop_engine.rs`

Add a `tool_round_counter` inside `act_with_tools` (local to the function, not engine state). After `tool_round_nudge_threshold` rounds (configurable, default 4), inject a progress directive into continuation messages. After `tool_round_strip_threshold` additional rounds (configurable, default 2), strip tools from the continuation request.

```rust
// Inside act_with_tools loop:
let nudge_threshold = self.budget.config().termination.tool_round_nudge_threshold();
let strip_threshold = nudge_threshold + self.budget.config().termination.tool_round_strip_after_nudge();

if round >= strip_threshold {
    // Force text-only response
    continuation_request.tools = vec![];
}
if round >= nudge_threshold {
    // Inject progress nudge
    state.continuation_messages.push(Message::system(TOOL_ROUND_PROGRESS_NUDGE.to_string()));
}
```

New `TerminationConfig` fields:
```rust
/// Tool rounds within act_with_tools before injecting a progress nudge.
/// 0 disables. Default: 4.
pub tool_round_nudge_after: u16,

/// Additional tool rounds after nudge before stripping tools.
/// Default: 2.
pub tool_round_strip_after_nudge: u16,
```

### Change 3: Suppress interstitial text during tool continuation

**File:** `engine/crates/fx-kernel/src/loop_engine.rs`

In `act_with_tools`, when a tool continuation response contains both text and tool_calls:
1. Do NOT stream the text to the user (don't emit `StreamEvent::TextDelta` for it).
2. Preserve the text in continuation_messages for model coherence.
3. Only stream/return the text from the final response (no tool calls).

Implementation: the `request_tool_continuation` path already streams text deltas via the callback. To suppress interstitial text, wrap the callback during non-final rounds to buffer text deltas instead of forwarding them. If the response turns out to be final (no tool calls), flush the buffer. If it's not final (more tool calls), discard the buffer.

### Change 4: Surface tool errors in streaming + continuation directive

**Files:**
- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-kernel/src/streaming.rs`

Add `StreamEvent::ToolError { tool_name: String, error: String }` variant.

In `execute_tool_round`, after executing tool calls, check for failures. If any:
1. Emit `StreamEvent::ToolError` for each failed tool.
2. Append a system message to continuation_messages: "Tool '{name}' failed with: {error}. Report this error to the user."

### Change 5: Task anchor in continuation context

**File:** `engine/crates/fx-kernel/src/loop_engine.rs`

After every N tool rounds (e.g., every 3), inject a system message into continuation_messages:
```
"Reminder: The user's request was: '{original_user_message}'. Stay focused on delivering this output. If you have enough information, produce the result now."
```

This is the `user_message` from `ProcessedPerception`, which is already available. Pass it through to `act_with_tools` so it can inject it periodically.

---

## TerminationConfig Changes (Summary)

Add two new fields to `TerminationConfig`:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `tool_round_nudge_after` | `u16` | 4 | Tool rounds in continuation loop before progress nudge |
| `tool_round_strip_after_nudge` | `u16` | 2 | Additional rounds after nudge before tool stripping |

Existing fields unchanged:
- `nudge_after_tool_turns` (6) — cross-cycle nudge (still useful for multi-cycle scenarios)
- `strip_tools_after_nudge` (3) — cross-cycle strip
- `synthesize_on_exhaustion` (true) — budget exhaustion behavior

---

## Testing Strategy

### Unit tests (per change):

1. **Counter fix:** Turn with tools+text increments counter; turn with text-only resets.
2. **Continuation nudge:** After N rounds, continuation messages include progress directive.
3. **Continuation strip:** After N+M rounds, continuation request has empty tools.
4. **Interstitial suppression:** Text+tools response does not emit TextDelta stream events for the text; final text-only response does.
5. **Error surfacing:** Failed tool emits ToolError stream event and error directive in continuation messages.
6. **Task anchor:** After every 3 rounds, continuation messages include task reminder.

### Integration test (TUI smoke):

Scenario that triggers long tool chains (e.g., multi-file analysis task). Verify:
- Model gets nudged after N rounds.
- Model gets tools stripped after N+M rounds and produces text response.
- No interstitial text visible in user-facing output.
- Tool errors shown in TUI.
- Budget and wall-time enforced.

---

## Implementation Order

1. **Change 1** (counter fix) — smallest, most impactful. Unblocks existing TerminationConfig.
2. **Change 2** (continuation loop enforcement) — the real fix for runaway tool chains.
3. **Change 4** (error surfacing) — quick win for user experience.
4. **Change 3** (interstitial suppression) — solves #1614 double response.
5. **Change 5** (task anchor) — mitigation for confabulation, lowest priority.

---

## Non-Goals

- Changing the single-pass architecture to multi-cycle. The current architecture is correct; the tool continuation loop within `act_with_tools` is the right place for tool chaining. The issue is that this loop lacks the guardrails that exist at the cycle level.
- Solving model confabulation in general. That's an LLM limitation. We can mitigate with anchoring but can't fix it.
- Changing `max_iterations` default (10). The limit itself is fine; the issue is that there's no progressive enforcement within those 10 rounds.
