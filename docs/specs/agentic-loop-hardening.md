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

### Bug 1: Tool-only turn counter resets on interstitial text (latent, cross-cycle)

**Location:** `loop_engine.rs` → `update_tool_only_turns()` (line ~1837)

```rust
fn update_tool_only_turns(&mut self, action: &ActionResult) {
    if !action.tool_results.is_empty() && action.response_text.trim().is_empty() {
        self.consecutive_tool_only_turns = self.consecutive_tool_only_turns.saturating_add(1);
    } else if !action.response_text.trim().is_empty() {
        self.consecutive_tool_only_turns = 0;  // ← RESETS even if tools were also called
    }
}
```

**Scope clarification:** This counter operates at the **cross-cycle** level. `update_tool_only_turns` is called once per cycle (line 1660), after `act_with_tools` returns. In the current single-pass architecture, the nudge_at_6 threshold means 6 separate user messages each producing tool-containing responses. **This bug did NOT cause the incident** (that was Bug 2). However, it is a latent bug: in multi-cycle scenarios, a model that always emits interstitial text alongside tools will never trigger the cross-cycle nudge/strip, allowing indefinite tool usage across messages.

**Problem:** When the model emits text + tool calls in the same turn (e.g., "Now let me look at the MLX training script..." + `read_file`), the counter resets to 0. The TerminationConfig's nudge_at_6/strip_at_9 never fires because the counter never reaches 6.

**Fix:** The counter should track _consecutive turns that include tool calls_ (regardless of whether text is also present), not _turns that are tool-only_. The reset condition should be: a turn that produces text with NO tool results. A turn that produces both text and tool results should still increment the counter.

### Bug 2: Nudge/strip only fires at perceive-time, not within tool continuation loop

**Location:** `loop_engine.rs` → `perceive()` (nudge injection), `reason()` (tool strip)

The current architecture is single-pass: `perceive → reason → decide → act`. The `act_with_tools` function runs the tool continuation loop (up to `max_iterations=10` rounds), but there is **no nudge/strip check inside this loop**. The nudge is only injected during `perceive` and tools are only stripped during `reason`, both of which execute once per cycle.

This means: within a single user turn, the model can chain 10 rounds of tool calls with no termination enforcement beyond the budget ceiling. The cross-cycle nudge_at_6 threshold (Bug 1) is irrelevant here because it only evaluates between cycles, not within the tool continuation loop. **This is the root cause of the incident.**

**Fix:** The tool continuation loop in `act_with_tools` must enforce its own progress check. After N tool rounds without the model producing a final text response (i.e., every round produces more tool calls), inject the nudge directive into the continuation messages. After N+M more rounds, strip tools from the continuation request, forcing a text response. This mirrors the cycle-level nudge/strip but operates within the tool continuation loop.

### Bug 3: Interstitial text in tool continuation constitutes "double response" (#1614)

**Location:** `act_with_tools` → `execute_tool_round` → `request_tool_continuation`

When the model returns text + tool_calls in a tool continuation response, the text gets included in the `response_text` of the eventual `ActionResult`. If the model does this on round 3 of 7, that interstitial text (e.g., "Now let me look at...") becomes visible to the user as a response, followed by a second actual response when the tool chain completes.

This is the root cause of the double-response pattern filed as #1614. The model emits partial "thinking aloud" text alongside tool calls.

**Fix:** During tool continuation rounds, if the response contains both text and tool calls, the text should be treated as internal reasoning (not streamed to the user as a response). Only the final response (the one with no tool calls) should be delivered. The interstitial text should be preserved in the continuation context (so the model maintains coherence) and accumulated in an internal buffer, but should not be streamed. If the accumulated interstitial text contains information relevant to the final answer, the implementer should consider whether to append it to the final response context.

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
        self.consecutive_tool_turns = self.consecutive_tool_turns.saturating_add(1);
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
// Inside act_with_tools loop (checks in escalation order for readability):
let tc = &self.budget.config().termination;
let nudge_threshold = tc.tool_round_nudge_after;
let strip_threshold = nudge_threshold.saturating_add(tc.tool_round_strip_after_nudge);

// Nudge: inject progress directive
if nudge_threshold > 0 && round >= nudge_threshold {
    state.continuation_messages.push(Message::system(TOOL_ROUND_PROGRESS_NUDGE.to_string()));
}
// Strip: force text-only response (escalation — superset of nudge)
if nudge_threshold > 0 && round >= strip_threshold {
    continuation_request.tools = vec![];
}
```

New `TerminationConfig` fields (with serde defaults for backward compatibility):
```rust
/// Tool rounds within act_with_tools before injecting a progress nudge.
/// 0 disables. Default: 4.
#[serde(default = "default_tool_round_nudge_after")]
pub tool_round_nudge_after: u16,

/// Additional tool rounds after nudge before stripping tools.
/// Default: 2.
#[serde(default = "default_tool_round_strip_after_nudge")]
pub tool_round_strip_after_nudge: u16,
```

Corresponding default functions:
```rust
fn default_tool_round_nudge_after() -> u16 { 4 }
fn default_tool_round_strip_after_nudge() -> u16 { 2 }
```

The `Default` impl for `TerminationConfig` must also include these fields.

### Change 3: Suppress interstitial text during tool continuation

**File:** `engine/crates/fx-kernel/src/loop_engine.rs`

In `act_with_tools`, when a tool continuation response contains both text and tool_calls:
1. Do NOT stream the text to the user (don't emit `StreamEvent::TextDelta` for it).
2. Preserve the text in continuation_messages for model coherence.
3. Only stream/return the text from the final response (no tool calls).

**Streaming timing constraint:** Text deltas are forwarded eagerly during `request_completion` via `ProviderStreamEvent::TextDelta` callbacks. You cannot decide to suppress text retroactively after the response completes. The implementation must use a **two-phase buffering approach**:

1. During ALL tool continuation rounds, use a buffering callback wrapper that captures `TextDelta` events into a buffer instead of forwarding them to the user-facing callback.
2. After the continuation response completes: if `response.tool_calls.is_empty()` (this is the final round), flush the buffered text deltas to the real callback as if they streamed normally. If `response.tool_calls` is non-empty (more tool rounds coming), discard the buffer.
3. This means text in continuation rounds is always buffered and only flushed retroactively for the final round. The user never sees interstitial "thinking aloud" text.
4. Non-text stream events (`ToolCallStart`, `ToolCallEnd`, `ToolResult`, etc.) should still be forwarded immediately — only `TextDelta` is buffered.
5. The discarded interstitial text should still be included in `continuation_messages` for model coherence (the model needs to see what it said to maintain context).
6. **`continue_truncated_response` interaction:** After the tool loop exits with a final text-only response, `continue_truncated_response` (line ~3851) may fire if the response was truncated. These continuation calls are part of the final response and should flush through the real callback (not be buffered). Once the tool loop determines a response is final (no tool_calls), all subsequent streaming for that response, including truncation continuations, should use the real callback directly.

### Change 4: Surface tool errors in streaming + continuation directive

**Files:**
- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-kernel/src/streaming.rs`

Add `StreamEvent::ToolError { tool_name: String, error: String }` variant.

**Backward compatibility:** `StreamEvent` derives `Serialize`/`Deserialize` (streaming.rs). The new variant must be additive. Consumer handling differs by type:
- **TUI (Rust):** Rust's exhaustive match will produce a compile error when the new variant is added, which is desirable: it forces the display code to be written. The TUI must add a `ToolError` arm that renders the error inline.
- **Swift app / SSE clients (JSON deserialization):** These consumers deserialize from JSON and won't have compile-time enforcement. They should gracefully ignore unknown variants. Serde's default tagged enum handling produces an unknown variant key for old consumers. The Swift app's stream event decoder should include a catch-all/default case that ignores unrecognized event types.

In `execute_tool_round`, after executing tool calls, check for failures. If any:
1. Emit `StreamEvent::ToolError` for each failed tool.
2. Append a system message to continuation_messages using a named constant (e.g., `TOOL_ERROR_RELAY_DIRECTIVE`). The directive should be parameterized with tool name and error text. Example wording: "Tool '{name}' failed with: {error}. Report this error to the user before continuing." The exact phrasing may need tuning per model; using a named constant makes this easy to adjust.

### Change 5: Task anchor in continuation context

**File:** `engine/crates/fx-kernel/src/loop_engine.rs`

After every N tool rounds (configurable via `task_anchor_interval`, default 3), inject a system message into continuation_messages:
```
"Reminder: The user's request was: '{original_user_message}'. Stay focused on delivering this output. If you have enough information, produce the result now."
```

This is the `user_message` from `ProcessedPerception`, which is already available. To make it accessible inside `act_with_tools`, store the user message in engine state during `perceive` (e.g., `self.current_user_message = Some(user_message.clone())`). This avoids changing the `act_with_tools` function signature (`decision, calls, llm, context_messages, stream`) and keeps the anchor injection self-contained. Clear the field in `prepare_cycle`.

---

## TerminationConfig Changes (Summary)

Add three new fields to `TerminationConfig`:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `tool_round_nudge_after` | `u16` | 4 | Tool rounds in continuation loop before progress nudge |
| `tool_round_strip_after_nudge` | `u16` | 2 | Additional rounds after nudge before tool stripping |
| `task_anchor_interval` | `u16` | 3 | Inject task reminder every N rounds (0 = disabled) |

All three fields require `#[serde(default = "...")]` and corresponding default functions for backward compatibility.

Existing fields unchanged:
- `nudge_after_tool_turns` (6) — cross-cycle nudge (still useful for multi-cycle scenarios)
- `strip_tools_after_nudge` (3) — cross-cycle strip
- `synthesize_on_exhaustion` (true) — budget exhaustion behavior

---

## Testing Strategy

### Unit tests (per change):

1. **Counter fix:**
   - Turn with tools+text increments counter.
   - Turn with text-only resets counter.
   - Turn with tools-only increments counter (existing behavior preserved).
   - Counter uses `saturating_add` (no overflow on u16::MAX).

2. **Continuation nudge/strip (boundary conditions):**
   - At round = `tool_round_nudge_after - 1`: no nudge in messages.
   - At round = `tool_round_nudge_after`: nudge directive present in messages.
   - At round = `tool_round_nudge_after + tool_round_strip_after_nudge - 1`: nudge present, tools NOT stripped.
   - At round = `tool_round_nudge_after + tool_round_strip_after_nudge`: tools stripped (empty vec), nudge present.
   - With `tool_round_nudge_after = 0`: nudge and strip disabled, loop runs to `max_iterations`.
   - With `tool_round_nudge_after = 1, tool_round_strip_after_nudge = 0`: tools stripped on first continuation round (aggressive config — document this edge case is intentional).

3. **Interstitial suppression:**
   - Text+tools continuation response does not emit `TextDelta` stream events for the text.
   - Final text-only response DOES emit `TextDelta` stream events.
   - Non-text stream events (`ToolCallStart`, etc.) are forwarded immediately regardless of round.
   - Discarded interstitial text is still present in continuation_messages for model coherence.

4. **Error surfacing:**
   - Failed tool emits `ToolError` stream event with tool name and error message.
   - Error directive appears in continuation messages after a failed tool.
   - Successful tools do not emit `ToolError` or inject error directive.

5. **Task anchor:**
   - After every `task_anchor_interval` rounds, continuation messages include task reminder with original user message.
   - Anchor not injected before the interval threshold.
   - With `task_anchor_interval = 0`: anchor injection disabled.

### Cross-cutting interaction tests:

6. **Strip + flush interaction (Change 2 × Change 3):** When the strip threshold fires and forces a text-only response, the buffering logic from Change 3 should flush that forced response to the user. Verify the strip-forced text-only response reaches the user via stream events.

7. **Original incident regression test:** Simulate the exact failure mode — mock LLM that emits text+tools on every continuation round for 10 rounds. Verify: (a) nudge fires at round 4, (b) tools stripped at round 6, (c) no interstitial text streamed to user, (d) final forced response delivered, (e) task anchor injected at round 3 and 6.

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
