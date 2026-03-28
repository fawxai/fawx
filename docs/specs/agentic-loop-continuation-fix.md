commit 64e01ea6e8dce0308fa4ba3b2d7d22ac506ae056
Author: Clawdio <clawdio@clawd.bot>
Date:   Thu Mar 26 03:59:22 2026 +0000

    docs: add perception bite 1 capture spec and agentic loop continuation fix spec

diff --git a/docs/specs/agentic-loop-continuation-fix.md b/docs/specs/agentic-loop-continuation-fix.md
new file mode 100644
index 00000000..88e74233
--- /dev/null
+++ b/docs/specs/agentic-loop-continuation-fix.md
@@ -0,0 +1,116 @@
+# Agentic Loop Continuation Fix
+
+**Status:** Spec
+**Date:** 2026-03-26
+**Issue:** #1630
+**Priority:** Blocking
+
+---
+
+## Problem
+
+PR #1582 (March 23) removed the outer iteration loop (`verify/learn/should_continue`) and collapsed `run_cycle_streaming_inner` to single-pass. The rationale was "all tool chaining happens inside `act_with_tools`."
+
+This is true for continuous tool use (model calls tools without stopping). But it broke multi-step agentic behavior where the model emits intermediate text between tool batches. When the model says "I've done the research, now let me scaffold the skill..." (text, no tool calls), `act_with_tools` returns, and the single-pass wrapper returns `LoopResult::Complete`. The model never gets another turn.
+
+## How Every Other Framework Does It
+
+OpenClaw, OpenAI Agents SDK, and Hermes all use the same pattern:
+
+```
+while true:
+  response = call_model(messages + tool_results)
+  if response has tool calls:
+    execute tools
+    append tool results to messages
+    continue
+  else:
+    return response.text  // model chose to stop
+```
+
+The key: the **model decides** when it's done by not calling tools. The loop is unconditional.
+
+Fawx's `act_with_tools` already implements this inner loop correctly. The bug is the outer layer that wraps it in exactly one pass.
+
+## Root Cause
+
+In `run_cycle_streaming_inner` (line ~1620), after perceive → reason → act:
+
+```rust
+Ok(self.finish_streaming_result(
+    LoopResult::Complete {
+        response: action.response_text,
+        ...
+    },
+    stream,
+))
+```
+
+Always returns Complete. No re-entry.
+
+## Fix
+
+Restore an outer loop around `act`, but much simpler than the old verify/learn/should_continue:
+
+This outer loop should remain aligned with Fawx's Composite Pattern / fractal architecture: the same runner contract should apply whether we're executing the root task or a child sub-goal. Child execution should be a composed instance of the same loop with a narrower tool surface, not a bespoke wrapper path.
+
+```rust
+loop {
+    stream.phase(Phase::Act);
+    let action = self.act(&decision, llm, &processed.context_window, stream).await?;
+
+    // ... budget accounting, tool turn tracking, cancellation checks ...
+
+    // If model used tools and then produced text, it may have more work.
+    // Feed the response back and let it decide whether to call more tools.
+    if !action.tool_results.is_empty() {
+        // Model used tools this round. Re-prompt to let it continue.
+        // Update context with the action's response text + tool results.
+        // Then loop back to reason+act (or just act, since context is updated).
+        continue;
+    }
+
+    // Model produced text-only response with no tool use. It's done.
+    break;
+}
+```
+
+### Specifically:
+
+1. After `act_with_tools` returns, check `action.tool_results.is_empty()`.
+2. If tools were used: the model did work and then responded with text. Feed the response + tool results into context and call the model again (re-enter reason → act).
+3. If no tools were used: the model's first response was text-only. It's genuinely done. Return Complete.
+4. Cap iterations with `max_iterations` (existing field) to prevent runaway loops.
+5. Increment `self.iteration_count` each pass for observability.
+
+### What NOT to restore:
+- No `verify()` — removed correctly, was heuristic noise
+- No `learn()` — removed correctly, was heuristic noise  
+- No `should_continue()` — replaced by the simple "did tools run?" check
+- No `LoopResult::NeedsInput` — not needed
+- No `IterationStep`/`IterationOutcome` enums — not needed
+
+### Edge cases:
+- **TerminationConfig (nudge/strip)**: already works inside `act_with_tools`. The outer loop should also respect it by checking tool-only turn count.
+- **Budget exhaustion**: already checked after each act. No change needed.
+- **Cancellation**: already checked after each act. No change needed.
+- **Max iterations**: use existing `self.max_iterations` as the outer loop cap.
+
+## Test Plan
+
+### Unit tests (modify existing):
+1. `act_with_tools_executes_all_calls_and_returns_completion_text` — still passes (single tool batch, no re-entry needed)
+2. New: `loop_continues_after_tool_batch_with_text_response` — model does tools, emits text, then on re-prompt does more tools, then emits final text
+3. New: `loop_stops_on_text_only_response` — model responds with text and no tools on first turn, loop returns immediately
+4. New: `loop_respects_max_iterations` — model keeps doing tools forever, loop caps at max_iterations
+5. New: `loop_continues_three_tool_batches_with_intermediate_text` — simulates the real X skill demo scenario (research → scaffold → build)
+
+### TUI smoke test:
+- Prompt: "Research the X API, then create a file with your findings, then read it back and summarize"
+- Expected: Fawx does web searches, writes file, reads file, summarizes — multiple tool batches with intermediate text
+- This is the gate for the demo
+
+## Files to Change
+
+1. `engine/crates/fx-kernel/src/loop_engine.rs` — restore outer loop in `run_cycle_streaming_inner`
+2. Tests in same file — add new test cases
+
+No other files should need changes. The `act_with_tools` inner loop, budget system, termination config, and streaming all remain unchanged.
