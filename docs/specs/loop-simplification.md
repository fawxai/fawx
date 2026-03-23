# Spec: Loop Simplification — Boundaries, Not Checkpoints (v2)

## Architecture Overview

### Current Fawx Loop (7 steps, 2 nested loops)

**Outer loop** (`run_cycle_streaming_inner`): Iterates up to `max_iterations`. Each iteration runs the full pipeline. `IterationStep::Progress` feeds back via `apply_iteration_outcome` which may re-perceive with a continuation directive. `IterationStep::Terminal` exits immediately.

**Inner loop** (`act_with_tools`): Within a single `act()` call, executes tool rounds. Each round: execute tools → send results to LLM → if LLM returns more tool calls → loop. Continues until LLM returns text with no tool calls, budget runs low, or max_iterations hit.

```
run_cycle_streaming_inner:
  while iteration_count < max_iterations:
    execute_iteration:
      budget_terminal check → perceive → reason → decide → act:
        act_with_tools (inner loop):
          for round in 0..max_iterations:
            execute_tool_round → request_tool_continuation
            if response.tool_calls.is_empty() → finalize, return
            else → continue inner loop
      → verify(action) → learn(verification) → should_continue(decision, verification, learning)
    if Terminal → return
    if Progress → apply_iteration_outcome:
      match continuation:
        Complete → return LoopResult::Complete
        NeedsInput → return LoopResult::NeedsInput
        Continue(msg) → mutate perception, continue outer loop
```

### Key Insight: Where Tool Feedback Happens

**All tool result feedback happens inside `act_with_tools`.** The inner loop handles:
- Tool execution → results back to LLM → more tools? → loop
- Fan-out cap (deferred tool results)
- Tool continuation compaction
- Budget checks between rounds
- Cancellation between rounds

**The outer loop (verify → should_continue → re-perceive) does NOT handle tool feedback.** It handles three specific cases:
1. Empty/fallback response → inject "use your tools" directive → re-perceive
2. Tool failure with no response → inject "try a different approach" → re-perceive
3. Low confidence → return NeedsInput (ask for clarification)

### Reference: OpenClaw's Approach

OpenClaw uses a similar two-loop architecture but with different intervention points:

**Agent loop** (`agent-loop.ts`): Simple — stream assistant response, if tool calls → execute → loop. No verify, no satisfaction heuristic. Model decides when it's done by not emitting tool calls.

**Tool loop detection** (`tool-loop-detection.ts`, 625 lines): Per-tool-call intervention via `beforeToolCall` hook:
- Generic repeat: same tool+args > N times → warning/block
- Poll no-progress: same tool+args+results repeated → warning/block
- Ping-pong: alternating between two patterns → warning/block
- Global circuit breaker: any tool repeated 30+ times → hard block
- Sliding window of 30 calls, content-aware hashing

**Key difference**: OpenClaw intervenes at the tool-call level with evidence-based pattern detection. Fawx intervenes at the iteration level with heuristic quality judgment. OpenClaw's approach is more granular, deterministic, and doesn't require judging response quality.

## What Changes

### Remove: Iteration-Level Heuristics

| Component | Location | Why Remove |
|-----------|----------|------------|
| `verify()` | `loop_engine.rs` fn `verify` | Heuristic quality judgment. Causes stalling (accepts failure text as completion) and false retries. |
| `learn()` | `loop_engine.rs` fn `learn` | Dead code. `Learning` struct is never consumed downstream. |
| `should_continue()` | `loop_engine.rs` fn `should_continue` | Routes verify output. With verify gone, no input. |
| `Verification` struct | `verify.rs` | Only used by verify/should_continue. |
| `Learning` struct | `learn.rs` | Never consumed. `learnings` field on `LoopResult::Complete` always ignored. |
| `Continuation` enum | `continuation.rs` | `Complete` becomes default. `Continue(msg)` was for re-perceive. `NeedsInput` handled by model naturally. |
| `IterationOutcome.learning` | `loop_engine.rs` struct `IterationOutcome` | Dead field. |
| `CycleState.learnings` | `loop_engine.rs` struct `CycleState` | Dead field. |
| `LoopResult::NeedsInput` | `loop_engine.rs` enum `LoopResult` | Model asks naturally. |
| `LoopResult::Complete.learnings` | `loop_engine.rs` enum `LoopResult` | Dead field. |
| `next_perception_from_sub_goal()` | `loop_engine.rs` fn `next_perception_from_sub_goal` | Only used by `Continue` re-perceive path. |
| `build_verification()` | `loop_engine.rs` fn `build_verification` | Only used by verify. |
| `VERIFICATION_CONFIDENCE_*` | `loop_engine.rs` | Only used by build_verification. |
| `CONFIDENCE_CLARIFY_THRESHOLD` import | `loop_engine.rs` use statement | Only used by should_continue. |
| `SAFE_FALLBACK_RESPONSE` detection in verify | `loop_engine.rs` fn `verify` | Replaced by tool loop detection. |
| `emit_verification_signals()` | `loop_engine.rs` fn `emit_verification_signals` | Replaced by tool-call-level signals. |
| `emit_continue_signal()` | `loop_engine.rs` fn `emit_continue_signal` | No continuation decisions to emit. |
| `LoopStep::Verify` | `fx-core/src/signals.rs` (`LoopStep` is defined in fx-core, re-exported by fx-kernel) | No verify step. |
| `LoopStep::Continue` | `fx-core/src/signals.rs` (same crate as above) | No continue step. |
| `IterationOutcome` | `loop_engine.rs` struct `IterationOutcome` | No outer iteration progression. |
| `IterationStep::Progress` | `loop_engine.rs` enum `IterationStep` | Remove variant; `Terminal` remains or `IterationStep` deleted if `budget_terminal`/`check_cancellation` return `Option<LoopResult>` directly. |

### Remove: Dead Types in types.rs

| Type | Why |
|------|-----|
| `LearningOutcome` | Zero references outside definition |
| `EpisodicEntry` | Zero references outside definition |
| `SemanticProposal` | Zero references outside definition |
| `ProceduralProposal` | Zero references outside definition |
| `ContinuationDecision` | Only referenced by its own test |
| `VerificationResult` | Only referenced by its own test and `ContinuationDecision` |

### Keep: Everything That Works

| Component | Why Keep |
|-----------|----------|
| `perceive()` | Context building — essential |
| `reason()` | LLM call — essential |
| `decide()` | Parse LLM response into Decision — essential |
| `act()` / `act_with_tools()` | Tool execution with inner loop — essential, this is where tool feedback happens |
| `ToolRetryTracker` | Per-call failure limits — essential, but needs enhancement (see below) |
| `TerminationConfig` | Hard boundary: nudge, tool stripping, forced synthesis — essential |
| `BudgetTracker` | Hard boundary: token/cost/time — essential |
| `consecutive_tool_only_turns` | Feeds TerminationConfig — essential |
| `forced_synthesis_turn` | Last-resort response — essential |
| `max_iterations` safety limit | Infinite loop prevention — essential |
| `streaming`, `cancellation`, `compaction` | Infrastructure — unaffected |
| `decomposition` | Sub-goal execution — unaffected (returns ActionResult, loop just sees text/no-tools) |
| `LoopResult::Complete` | Still needed (minus learnings field) |
| `LoopResult::BudgetExhausted` | Still needed |
| `LoopResult::UserStopped` | Still needed |
| `LoopResult::Error` | Still needed |

### Add: Tool-Call-Level Loop Detection

Enhance `ToolRetryTracker` to include pattern-based detection inspired by OpenClaw:

**Current state**: Fawx's `ToolRetryTracker` tracks `(tool_name, args_hash) → failure_count`. Blocks after N consecutive failures on the same signature, or M total failures per cycle.

**Enhancement**: Add no-progress detection. Currently, `ToolRetryTracker` only counts failures. It doesn't detect the model calling the same tool with the same args successfully but making no progress (e.g., reading the same file 10 times).

New detection patterns:
1. **Same-signature repeat** (regardless of success): tool+args called N times → inject warning as tool result
2. **No-progress repeat**: tool+args+result_hash identical N times → inject warning, then block
3. **Ping-pong**: alternating between two tool signatures → inject warning, then block

This is a separate spec (`tool-loop-detection.md`) because it's a standalone enhancement to `ToolRetryTracker` that provides value regardless of the loop simplification. But it becomes the primary defense against tool spiraling once verify is removed.

## Simplified Loop

### `execute_action_and_finalize` (was verify → learn → should_continue)

> **Note**: This code block shows the intermediate reasoning that led to the "outer loop disappears" insight. The final design (below, in "Actually simplified `run_cycle_streaming_inner`") is the implementation target. This intermediate version is preserved to show why `IterationStep::Progress` is never reached.

```rust
async fn execute_action_and_finalize(
    &mut self,
    decision: &Decision,
    llm: &dyn LlmProvider,
    state: &mut CycleState,
    context_messages: &[Message],
    stream: CycleStream<'_>,
) -> Result<IterationStep, LoopError> {
    let action = self.act(decision, llm, context_messages, stream).await?;

    // Budget accounting (unchanged)
    if action.tool_results.is_empty() {
        let action_cost = self.action_cost_from_result(&action);
        if let Some(step) = self.budget_terminal(action_cost, Some(action.response_text.clone())) {
            return Ok(step);
        }
        self.budget.record(&action_cost);
    } else if let Some(step) = self.budget_terminal(ActionCost::default(), Some(action.response_text.clone())) {
        return Ok(step);
    }

    state.tokens.accumulate(action.tokens_used);
    state.partial_response = Some(action.response_text.clone());
    self.update_tool_only_turns(&action);

    if let Some(step) = self.check_cancellation(state.partial_response.clone()) {
        return Ok(step);
    }

    // Emit observation signals (replaces verify/learn/continue signals).
    // emit_action_observations emits:
    // - "tool_failure_with_response" (SignalKind::Observation) when tool failures + text response
    // - "empty_response" (SignalKind::Observation) when response_text is empty/fallback
    // - "tool_only_turn" (SignalKind::Observation) when tools called but no text
    // Data: failed tool names, response preview, tool count.
    self.emit_action_observations(&action);

    // Model-driven completion: tool calls present = continue, text only = done
    if !action.tool_results.is_empty() {
        // Tools were executed in this action. Continue the outer loop.
        // The tool results are already in context via act_with_tools inner loop.
        // If act_with_tools ended with a text response, it's in response_text.
        // The outer loop continues to let the model decide next steps.
        Ok(IterationStep::Progress(IterationOutcome {
            response_text: action.response_text,
        }))
    } else {
        // No tool calls — model produced text only. This is the final response.
        Ok(IterationStep::Terminal(LoopResult::Complete {
            response: action.response_text,
            iterations: self.iteration_count,
            tokens_used: state.tokens,
            signals: Vec::new(),
        }))
    }
}
```

### `IterationOutcome` simplified

```rust
struct IterationOutcome {
    response_text: String,
}
```

No `learning`, no `continuation`.

### `apply_iteration_outcome` simplified

```rust
fn apply_iteration_outcome(
    &mut self,
    outcome: IterationOutcome,
    state: &mut CycleState,
) -> Option<LoopResult> {
    // Progress means tools were called. The model will be called again
    // next iteration with updated context. No re-perceive injection needed —
    // perceive() rebuilds context from conversation_history which includes
    // the tool results from act_with_tools.
    None // always continue
}
```

Wait — **this is a critical question**. When `IterationStep::Progress` fires, the outer loop calls `apply_iteration_outcome` and continues. But `perceive()` builds context from `PerceptionSnapshot` which has `user_input` and `conversation_history`. The tool results from `act_with_tools` are in `self.last_reasoning_messages`, not in the perception snapshot.

Currently, `Continuation::Continue(msg)` calls `next_perception_from_sub_goal` which injects the continuation message as `user_input`. Without this, the next `perceive()` call uses the SAME `perception` snapshot — the original user message. The tool results are NOT re-perceived.

**But this means the outer loop re-perceive is currently injecting a SYNTHETIC user message.** The model sees:
1. Turn 1: User's actual message → tool calls → tools fail → empty response
2. verify detects empty → Continue("use your tools")
3. Turn 2: "use your tools" as new user_input → model reasons again

With the simplified loop, when would the outer loop actually continue? Only when `act_with_tools` returned tool results in the ActionResult but the model also produced text. That's the end of the inner loop — the model already stopped calling tools. So `IterationStep::Progress` should actually almost never fire.

Let me check: when does `act_with_tools` return tool_results that are non-empty?

The inner loop runs until the model stops calling tools. When it ends:
- `finalize_tool_response` builds ActionResult with `all_tool_results` (non-empty) and `response_text` from the final LLM response
- OR `synthesize_tool_fallback` when max rounds exceeded — returns ActionResult with tool results and fallback text

So `action.tool_results` is non-empty when `act_with_tools` ran (model called tools). The `response_text` is the FINAL text after all tool rounds completed.

In the simplified loop, if `tool_results` is non-empty, we continue the outer loop. But the model ALREADY produced its final text response in the inner loop. Re-perceiving would give the model the same user input again, and it would... do what? The tool results from the previous iteration aren't in the perception snapshot.

**This reveals a deeper issue**: the outer loop's re-perceive is not how tool results get back to the model. Tool results are handled entirely inside `act_with_tools`. The outer loop re-perceive was only ever used for injecting synthetic directives ("use your tools", "try a different approach").

So in the simplified loop:
- `tool_results` non-empty + `response_text` non-empty → the inner loop completed: tools were called, results processed, model gave final text → **this IS the completion**
- `tool_results` non-empty + `response_text` empty → the inner loop completed but model produced no text after tools (fell through to `synthesize_tool_fallback`) → **this is also completion** (with whatever fallback text was generated)
- `tool_results` empty + `response_text` non-empty → model gave text, no tools → **completion**
- `tool_results` empty + `response_text` empty → model produced nothing → **this is the empty response edge case**

**The outer iteration loop becomes single-pass.** Every path through `execute_action_and_finalize` is terminal. The only reason to loop was `Continuation::Continue` which we're removing.

### Actually simplified `run_cycle_streaming_inner`

```rust
async fn run_cycle_streaming_inner(
    &mut self,
    perception: PerceptionSnapshot,
    llm: &dyn LlmProvider,
    stream_callback: Option<&StreamCallback>,
) -> Result<LoopResult, LoopError> {
    self.prepare_cycle();
    self.notify_tool_guidance_enabled = stream_callback.is_some();
    let mut state = CycleState::default();
    let stream = stream_callback.map_or_else(CycleStream::disabled, CycleStream::enabled);

    // Single iteration — no outer loop needed.
    // All tool chaining happens inside act_with_tools.
    self.iteration_count = 1;
    self.refresh_iteration_state();

    if let Some(step) = self.budget_terminal(ActionCost::default(), None) {
        return Ok(self.finish_streaming_result(
            match step { IterationStep::Terminal(r) => r, _ => unreachable!() },
            stream,
        ));
    }

    if let Some(step) = self.check_cancellation(None) {
        return Ok(self.finish_streaming_result(
            match step { IterationStep::Terminal(r) => r, _ => unreachable!() },
            stream,
        ));
    }

    stream.phase(Phase::Perceive);
    let processed = self.perceive(&perception).await?;
    let reason_cost = self.estimate_reasoning_cost(&processed);
    if let Some(step) = self.budget_terminal(reason_cost, None) {
        return Ok(self.finish_streaming_result(
            match step { IterationStep::Terminal(r) => r, _ => unreachable!() },
            stream,
        ));
    }

    stream.phase(Phase::Reason);
    let response = self.reason(&processed, llm, stream).await?;
    self.record_reasoning_cost(reason_cost, &mut state);

    let decision = self.decide(&response).await?;
    if let Some(step) = self.budget_terminal(
        self.estimate_action_cost(&decision), None,
    ) {
        return Ok(self.finish_streaming_result(
            match step { IterationStep::Terminal(r) => r, _ => unreachable!() },
            stream,
        ));
    }

    stream.phase(Phase::Act);
    let action = self.act(&decision, llm, &processed.context_window, stream).await?;

    // Budget accounting
    if action.tool_results.is_empty() {
        let action_cost = self.action_cost_from_result(&action);
        if let Some(step) = self.budget_terminal(action_cost, Some(action.response_text.clone())) {
            return Ok(self.finish_streaming_result(
                match step { IterationStep::Terminal(r) => r, _ => unreachable!() },
                stream,
            ));
        }
        self.budget.record(&action_cost);
    }

    state.tokens.accumulate(action.tokens_used);
    self.update_tool_only_turns(&action);
    self.emit_action_observations(&action);

    Ok(self.finish_streaming_result(
        LoopResult::Complete {
            response: action.response_text,
            iterations: self.iteration_count,
            tokens_used: state.tokens,
            signals: Vec::new(),
        },
        stream,
    ))
}
```

**The outer while loop disappears entirely.** `max_iterations` still applies inside `act_with_tools` to limit tool rounds. `IterationOutcome` can be deleted. `IterationStep` simplifies: `Progress` variant is removed; `Terminal` remains as the return type for `budget_terminal()` and `check_cancellation()`. Alternatively, those methods can return `Option<LoopResult>` directly, eliminating `IterationStep` entirely.

### `emit_action_observations` specification

```rust
fn emit_action_observations(&mut self, action: &ActionResult) {
    let has_tool_failure = action.tool_results.iter().any(|r| !r.success);
    let has_response = !action.response_text.trim().is_empty()
        && action.response_text != SAFE_FALLBACK_RESPONSE;
    let has_tools = !action.tool_results.is_empty();

    if has_tool_failure && has_response {
        let failed: Vec<&str> = action.tool_results.iter()
            .filter(|r| !r.success)
            .map(|r| r.tool_call_id.as_str())
            .collect();
        self.emit_signal(LoopStep::Act, SignalKind::Observation,
            "tool_failure_with_response",
            json!({
                "failed_tools": failed,
                "response_preview": &action.response_text[..100.min(action.response_text.len())]
            }));
    }
    if !has_response && !has_tools {
        self.emit_signal(LoopStep::Act, SignalKind::Observation,
            "empty_response", json!({}));
    }
    if has_tools && !has_response {
        self.emit_signal(LoopStep::Act, SignalKind::Observation,
            "tool_only_turn",
            json!({"tool_count": action.tool_results.len()}));
    }
}
```

## Downstream Impact Analysis

### fx-kernel/src/lib.rs — Public API changes

**Removed exports:**
- `pub use learn::Learning;`
- `pub use continuation::Continuation;`
- `pub use verify::Verification;`
- `pub use types::{ContinuationDecision, EscalationContext, LoopEvidence};` (ContinuationDecision)

**Modified exports:**
- `LoopResult` — remove `learnings` from `Complete`, remove `NeedsInput` variant

### fx-cli/src/headless.rs — Consumer changes

1. `ResultKind::NeedsInput` → remove variant, or map to `Complete` (model asked naturally)
2. `extract_response_text`: remove `LoopResult::NeedsInput` match arm
3. `extract_result_kind`: remove `NeedsInput` arm
4. `extract_iterations`: remove `NeedsInput` arm
5. `NEEDS_INPUT_FALLBACK_RESPONSE` constant → remove
6. Tests constructing `LoopResult::NeedsInput` → remove
7. Tests constructing `LoopResult::Complete { learnings }` → remove field

### fx-api/src/engine.rs — Consumer changes

1. `ResultKind::NeedsInput` → remove variant
2. `fx-api/src/tests.rs` — remove `NeedsInput` mapping in `ResultKind` conversion

### fx-core/src/signals.rs — Signal changes

1. `LoopStep::Verify` → remove (or keep as dead variant for signal compatibility). `LoopStep` is defined in `fx-core/src/signals.rs` and re-exported by `fx-kernel`. Check both crates for test references.
2. `LoopStep::Continue` → remove (or keep as dead variant). Same crate as above.

### fx-kernel/src/types.rs — Dead type removal

Remove `LearningOutcome`, `EpisodicEntry`, `SemanticProposal`, `ProceduralProposal`, `ContinuationDecision`, `VerificationResult` and their tests.

### fx-kernel/src/decide.rs — CONFIDENCE_CLARIFY_THRESHOLD

`CONFIDENCE_CLARIFY_THRESHOLD` is also used inside `decide()` itself (not just should_continue). Check if removing the import breaks decide:

The constant gates `Decision::Clarify` production in decide. If we keep decide unchanged (which we should — it parses LLM responses), we keep the constant but remove the import in loop_engine.rs.

### Files to delete entirely

- `engine/crates/fx-kernel/src/learn.rs` (7 lines of struct + test)
- `engine/crates/fx-kernel/src/verify.rs` (14 lines of struct + test)
- `engine/crates/fx-kernel/src/continuation.rs` (17 lines of enum + test)

### Files NOT affected

- `fx-decompose` — unchanged (returns sub-goal results to act, which returns to the now-single-pass loop)
- `fx-tools` — unchanged (tool execution is inside act_with_tools)
- `fx-journal` — unchanged (reads signals, no verify/learn/continue signals were ever consumed)
- `fx-scratchpad` — unchanged
- `fx-loadable` — unchanged
- `fx-session` — unchanged
- `fx-fleet` — unchanged
- `fx-security` — unchanged
- `fx-transactions` — unchanged

## Edge Case Analysis

### Empty response (model produces nothing)

**Current**: verify detects → Continue("use your tools") → re-perceive → model tries again
**Simplified**: `action.response_text` will be `SAFE_FALLBACK_RESPONSE` (from `ensure_non_empty_response`). This becomes the final output.
**Mitigation**: This is extremely rare with modern models. When it happens, the retry rarely helps — the model typically produces the same empty response. The `SAFE_FALLBACK_RESPONSE` text is user-visible and actionable ("I wasn't able to process that. Could you try rephrasing?").
**Decision**: Accept this behavioral change. If data shows it's a real problem, address upstream (better system prompt, model quality) rather than at the loop level.

### Tool failure with explanatory text (the stalling bug)

**Current**: verify sees non-empty text → satisfactory → Complete. The stalling behavior IS the current behavior.
**Simplified**: Same outcome — text response with no tool calls → Complete.
**Net change**: None. The simplified loop doesn't make this worse. The real fix is tool-call-level loop detection (catching the failure pattern before it produces bad output).

### Decision::Clarify / Decision::Defer

**Current**: should_continue maps these to `Continuation::NeedsInput`.
**Simplified**: These decisions ARE actively produced by `decide()` (in fn `decide`, low-confidence and degenerate-intent branches) for low-confidence and degenerate-intent cases. In the current loop, `should_continue()` maps them to `Continuation::NeedsInput`. In the simplified loop, `act()` already handles them correctly — `Decision::Clarify`/`Defer` produce an `ActionResult` with the clarification/deferral text and no tool calls → `Complete` with that text as the response.
**Decision**: Keep `Decision::Clarify`/`Defer` in decide.rs. They flow through `act()` → text response → Complete. **Tests must cover this path** since it's a live code path, not speculative. No need for a special NeedsInput result — the model's clarification text becomes the response naturally.

### Decomposition

**Current**: `execute_decomposition` runs sub-goals, returns ActionResult with aggregated text and empty tool_results → verify → Complete.
**Simplified**: Same ActionResult → no tool calls → Complete.
**Net change**: None.

### Yield between iterations

**Current**: `check_yield_between_iterations` runs after each Progress iteration.
**Simplified**: No outer loop iterations → no yield points between iterations. Yield still works inside act_with_tools (between tool rounds via cancellation checks).
**Risk**: If anyone depends on inter-iteration yield for steering. Check: `check_yield_between_iterations` is only called in the outer loop. It's used for session yield (sessions_yield). Need to verify this isn't critical.

### Streaming events

**Current**: `finish_streaming_result` called on every terminal result. `stream.phase(Phase::...)` called during pipeline.
**Simplified**: Same — phases still emitted (Perceive, Reason, Act, Synthesize). The removed phases (Verify, Learn, Continue) were never emitted as stream phases — they're only signal steps.
**Net change**: None for streaming.

## Test Impact

### Tests to delete (verify/learn/continue behavior)

All tests that assert on `Verification`, `Learning`, `Continuation`, `IterationOutcome.learning`, `LoopResult::NeedsInput`, or `CycleState.learnings`.

### Tests to update

Tests that construct `LoopResult::Complete { learnings: Vec::new(), ... }` → remove the field.

### Tests to add

1. `single_pass_completes_on_text_response` — model returns text, no tools → Complete
2. `single_pass_completes_after_tool_chain` — model calls tools, inner loop resolves → Complete
3. `empty_response_returns_fallback` — model produces nothing → Complete with SAFE_FALLBACK_RESPONSE
4. `decompose_completes_after_subgoals` — decompose returns aggregated text → Complete
5. `budget_exhaustion_still_works` — budget runs out → BudgetExhausted with synthesis

## Estimated Impact

- **Lines removed**: ~400-600 (verify, learn, should_continue, helpers, constants, dead types, related tests, outer loop machinery, IterationStep/IterationOutcome)
- **Lines added**: ~50-100 (emit_action_observations, simplified execute_action_and_finalize)
- **Net reduction**: ~350-500 lines from the 18.5k file
- **Files deleted**: 3 (learn.rs, verify.rs, continuation.rs)
- **Behavioral changes**: (1) Empty responses no longer retried — SAFE_FALLBACK_RESPONSE returned directly. (2) NeedsInput no longer a distinct result — model asks naturally or returns text.

## Pressure Test: Real Holes Found

### Hole 1: Provider Tool Call Multiplicity (Joe's observation)

**Claude** rarely returns >1 tool call per response. Each tool call is a separate LLM round in `act_with_tools`.
**OpenAI** returns multiple parallel tool calls in a single response. All execute, results sent back in one round.

**Impact on simplified loop**: `max_iterations` (default 10) caps inner loop rounds in `act_with_tools`. With Claude, 10 rounds = 10 tool calls max. With OpenAI, 10 rounds = potentially 50+ tool calls (5 per round). This is already the case today, but becomes more critical when the outer retry loop is removed — the inner loop is the ONLY loop.

**Mitigation needed**: Per-provider awareness in `max_iterations` semantics. Or: decouple `max_iterations` (outer, now single-pass) from `max_tool_rounds` (inner, `act_with_tools`). Currently they share the counter. After simplification, `max_iterations` should become `max_tool_rounds` explicitly.

### Hole 2: Hermes Has Extensive Empty Response Recovery

Hermes's `run_conversation` (7,374 lines, single function) has sophisticated empty response handling that our simplified loop skips:

1. **`_empty_content_retries`** (3 attempts): If model returns think-block-only or empty content, retries up to 3 times
2. **`_last_content_with_tools` fallback**: If a prior turn had text + tool calls, and the follow-up is empty, salvages the prior turn's content as final response
3. **`_incomplete_scratchpad_retries`**: Retries when scratchpad writes are incomplete
4. **`_invalid_tool_retries`** (3 attempts): Retries when model calls nonexistent tools
5. **`_invalid_json_retries`**: Retries when tool arguments are malformed JSON

Hermes does NOT use a verify/learn/should_continue pipeline. But it DOES have per-failure-type retry counters inside the main while loop. This is different from both Fawx's heuristic approach and my proposed "just return the empty response."

**Impact**: The "empty response" scenario is not as rare as I claimed. Hermes has dedicated retry logic for it — they wouldn't build this without hitting it in production. The think-block-only case (model puts entire response in `<think>` tags) is a real pattern with reasoning models.

**Mitigation needed**: Keep targeted retry for genuinely empty responses (empty string or SAFE_FALLBACK_RESPONSE). Not the full verify/should_continue pipeline — just a simple counter: if response is truly empty and we haven't retried yet, re-request. This is what Hermes does, without the heuristic quality judgment.

### Hole 3: `_last_content_with_tools` Pattern

Hermes tracks when the model produces BOTH content AND tool calls in the same response. If the follow-up turn (after tool results) produces empty content, Hermes uses the content from the tool-call turn as the final response.

**Why this matters**: The model sometimes front-loads its response text alongside tool calls ("Here's what I found: [text]. Let me also save this to memory [tool_call]"). After the tool completes, the model has nothing more to say. Our current loop (and simplified loop) would see "empty response after tools" and either retry uselessly or return nothing.

**Impact**: This is a real UX regression if we don't handle it. The model already said the important thing — we just need to recognize it.

**Mitigation needed**: Track `last_content_with_tools` in `act_with_tools`. When the inner loop's final LLM response is empty but a previous round had content + tools, use that content.

### Hole 4: Invalid Tool Name Recovery

Hermes retries (up to 3x) when the model calls a tool that doesn't exist, injecting an error message listing available tools. Fawx handles this via `ToolRetryTracker` blocking repeated failures, but the initial "tool doesn't exist" error goes back as a tool result.

**Impact**: Minor — Fawx already handles this differently (tool result with error message). No change needed for the simplification.

### Hole 5: Context Compression Mid-Conversation

Hermes has a `restart_with_compressed_messages` flag that triggers when the context window fills up. It compresses the conversation and restarts the current API call.

**Impact**: Fawx has compaction (`conversation_compactor`). This is already handled inside `perceive()` and is unaffected by the loop simplification. No change needed.

### Hole 6: `max_iterations` Counter Sharing

Currently `iteration_count` is shared between the outer loop and `act_with_tools`. The outer loop increments it, and `act_with_tools` uses it as the round counter. With the outer loop becoming single-pass, `iteration_count` always enters `act_with_tools` at 1 and counts up from there.

**Impact**: Currently the outer loop increments `iteration_count` to 2, 3, etc. before re-entering `act_with_tools`. With single-pass, the inner loop always starts at 1. `max_iterations` is still the cap for inner rounds. This is actually cleaner — no wasted iterations on outer loop overhead.

### Hole 7: Yield Between Iterations

`check_yield_between_iterations()` is called in the outer loop after each `Progress`. With single-pass, this is never called.

**Impact**: Need to verify if `YieldHandle` / `sessions_yield` depends on inter-iteration yield points. Checked: `yield_primitive.rs` has `WakeCondition::NextTurn` — this triggers at the START of the next `run_cycle_streaming`, not between iterations. The inter-iteration yield is for fine-grained steering (e.g., user sends a message while the agent is looping). With single-pass, this steering can happen between tool rounds inside `act_with_tools` (via cancellation checks that already exist).

**Mitigation**: Add yield check inside `act_with_tools` between tool rounds, alongside the existing cancellation check. This preserves steering responsiveness.

## Revised Design: Targeted Retries, Not Heuristic Judgment

Based on the pressure test, the simplified loop should include:

1. **Empty response retry** (1-2 attempts): If `response_text` is empty/fallback AND no tool calls, retry the LLM call directly (no re-perceive, no synthetic user message). Simple counter, no confidence threshold.

```rust
// In finalize_tool_response or the single-pass completion path:
let (response_text, used_fallback) = ensure_non_empty_response_with_flag(&readable);
if used_fallback && self.empty_response_retries < 2 {
    self.empty_response_retries += 1;
    tracing::info!(
        attempt = self.empty_response_retries,
        "empty response — re-requesting LLM call"
    );
    // Re-request the same completion (same context, fresh generation)
    let retry_response = self
        .request_completion(llm, request, StreamPhase::Synthesize, "retry", stream)
        .await?;
    let retry_text = extract_response_text(&retry_response);
    let (retry_final, still_fallback) = ensure_non_empty_response_with_flag(&retry_text);
    if !still_fallback {
        return Ok(/* ActionResult with retry_final */);
    }
    // Still empty — fall through to return fallback
}
```

2. **Content-with-tools tracking**: When a tool round produces both text and tool calls, save the text. If the final response after all tools is empty, use the saved text.

3. **Yield between tool rounds**: Add yield check inside `act_with_tools` to preserve steering responsiveness.

4. **`max_tool_rounds` rename**: Clarify that the iteration cap controls tool rounds, not outer iterations (which are now always 1).

Everything else from the original spec stands — verify/learn/should_continue are still removed, the outer loop is still single-pass, signals replace heuristics.

## Companion Spec Required

**Tool Loop Detection Enhancement** (`tool-loop-detection.md`): Before or alongside this change, enhance `ToolRetryTracker` with:
1. Same-signature success repeat detection (not just failures)
2. No-progress detection (same args + same result hash)
3. Warning injection as tool result (before blocking)

This replaces the iteration-level intervention (verify/should_continue) with tool-call-level intervention (enhanced ToolRetryTracker) — matching OpenClaw's architecture.

## Implementation Order

1. **First**: Enhance ToolRetryTracker with loop detection (separate PR, independent value)
2. **Second**: Simplify loop (this spec — remove verify/learn/should_continue, collapse outer loop)
3. **Third**: Clean up dead types in types.rs (separate PR, pure cleanup)

**Note**: `stall-after-tool-failure.md` is explicitly superseded by this spec. Its proposed `verify()` modifications will NOT be implemented; the loop simplification eliminates `verify()` entirely, resolving the stalling problem at the root.

This order ensures the tool-level safety net is in place before the iteration-level one is removed.
