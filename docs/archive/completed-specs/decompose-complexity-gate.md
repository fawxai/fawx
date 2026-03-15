# Decompose Complexity Gate: Batch Detection, Complexity Floor, Cost Gate

**Status:** Proposed spec  
**Issue:** #1100  
**Depends on:** PR #1098 (`feat/loop-resilience`) — uses `BudgetState`, `BudgetTracker`, `BudgetRemaining`  
**Branch:** `feat/loop-resilience`

---

## Problem

The decompose tool is a powerful mechanism for breaking complex goals into sub-goals, but it's being triggered for work that isn't actually decomposition:

1. **Batching masquerades as decomposition.** "Read these 13 files" produces 13 sub-goals that each call `read_file` with a different path. This isn't decomposition — there are no inter-dependencies, no reasoning chain, no aggregation logic. It's a batch of identical tool calls. Running it through `execute_decomposition()` → `execute_sub_goals_sequential()` / `execute_sub_goals_concurrent()` spawns 13 child `LoopEngine` instances with individual `BudgetTracker`s, each making at least 1 LLM call. That's 13+ LLM calls for what should be a single fan-out-capped tool round.

2. **Trivial plans waste decomposition overhead.** A plan where every sub-goal has no inter-dependencies and requires a single tool call each doesn't benefit from the decomposition pipeline (budget allocation, child loops, aggregation). It should execute as a direct tool round with fan-out cap.

3. **Expensive plans proceed without cost awareness.** A decomposition plan whose estimated budget exceeds the remaining budget by >50% will partially execute, exhaust budget mid-way, and produce incomplete results. Better to reject upfront and let the LLM reformulate a cheaper plan.

---

## Design

Three independent gates evaluated in order after `parse_decomposition_plan()` succeeds but before `execute_decomposition()` proceeds. Each gate returns early with a descriptive result if triggered.

### Gate 1: Batch Detection

**Heuristic:** All sub-goals in the plan use the same single tool.

```
is_batch = plan.sub_goals.len() > 1
    && plan.sub_goals.iter().all(|sg| sg.required_tools.len() == 1)
    && plan.sub_goals.iter().map(|sg| &sg.required_tools[0]).collect::<HashSet<_>>().len() == 1
```

When detected:
- Skip `execute_decomposition()` entirely.
- Convert sub-goals into synthetic `ToolCall`s (one per sub-goal, using the common tool name and description as arguments).
- Execute via `act_with_tools()`, which already enforces fan-out cap (`max_fan_out` from `BudgetConfig`).
- Emit `SignalKind::Trace` with `"decompose_batch_detected"` metadata.

This avoids spawning N child `LoopEngine` instances and their associated LLM calls.

### Gate 2: Complexity Floor

**Heuristic:** No inter-dependencies AND every sub-goal requires ≤1 tool call.

```
is_trivial = plan.sub_goals.iter().all(|sg| {
    sg.required_tools.len() <= 1
        && sg.complexity_hint.unwrap_or_else(|| estimate_complexity(sg)) == ComplexityHint::Trivial
})
```

When detected:
- Same treatment as batch detection: convert to tool calls, route through `act_with_tools()`.
- Emit `SignalKind::Trace` with `"decompose_complexity_floor"` metadata.

The difference from batch detection is that complexity floor triggers even when sub-goals use *different* tools, as long as each is trivially simple.

### Gate 3: Cost Gate

**Heuristic:** Estimated plan cost exceeds remaining budget by >50%.

Estimation uses the existing `estimate_complexity()` function to derive per-sub-goal weights, then maps weights to estimated LLM calls and tool invocations:

```rust
fn estimate_plan_cost(plan: &DecompositionPlan) -> ActionCost {
    plan.sub_goals.iter().fold(ActionCost::default(), |mut acc, sg| {
        let hint = sg.complexity_hint.unwrap_or_else(|| estimate_complexity(sg));
        // Each child loop needs at least 1 LLM call + 1 per required tool
        let llm_calls = match hint {
            ComplexityHint::Trivial => 1,
            ComplexityHint::Moderate => 2,
            ComplexityHint::Complex => 4,
        };
        let tool_invocations = sg.required_tools.len() as u32;
        acc.llm_calls += llm_calls;
        acc.tool_invocations += tool_invocations;
        acc.cost_cents += u64::from(llm_calls) * DEFAULT_LLM_CALL_COST_CENTS
            + u64::from(tool_invocations) * DEFAULT_TOOL_INVOCATION_COST_CENTS;
        acc
    })
}
```

Rejection condition:

```rust
let remaining = self.budget.remaining(current_time_ms());
let estimated = estimate_plan_cost(plan);
let exceeds = estimated.cost_cents > remaining.cost_cents * 3 / 2; // >150% of remaining
```

When triggered:
- Return an `ActionResult` with text explaining the plan was rejected due to cost.
- Emit `SignalKind::Blocked` with `"decompose_cost_gate"` metadata including estimated vs remaining cost.
- The LLM receives the rejection and can reformulate a smaller plan.

---

## Where to Change

### 1. `engine/crates/fx-kernel/src/loop_engine.rs` — new gating function

Add `fn evaluate_decompose_gates(&mut self, plan: &DecompositionPlan, decision: &Decision, llm: &dyn LlmProvider, context_messages: &[Message]) -> Option<Result<ActionResult, LoopError>>`.

Call site: in the decomposition branch of `execute_iteration()` (around line ~860–900 where `find_decompose_tool_call()` and `execute_decomposition()` are invoked). Insert gate evaluation between `parse_decomposition_plan()` and `execute_decomposition()`.

### 2. `engine/crates/fx-kernel/src/loop_engine.rs` — batch-to-tool-calls conversion

Add `fn batch_to_tool_calls(&self, plan: &DecompositionPlan) -> Vec<ToolCall>`. Converts sub-goals sharing a common tool into synthetic `ToolCall` structs. Each gets a unique ID, the common tool name, and arguments derived from the sub-goal description.

### 3. `engine/crates/fx-kernel/src/loop_engine.rs` — `estimate_plan_cost()` free function

New function near existing `estimate_cost()` usage. Uses `estimate_complexity()` from `budget.rs` and the existing `DEFAULT_LLM_CALL_COST_CENTS` / `DEFAULT_TOOL_INVOCATION_COST_CENTS` constants.

### 4. `engine/crates/fx-kernel/src/budget.rs` — export constants

Make `DEFAULT_LLM_CALL_COST_CENTS` and `DEFAULT_TOOL_INVOCATION_COST_CENTS` `pub(crate)` (currently file-private). Needed by `estimate_plan_cost()` in `loop_engine.rs`.

### 5. `engine/crates/fx-decompose/src/lib.rs` — no changes

The `DecompositionPlan`, `SubGoal`, `ComplexityHint`, and `AggregationStrategy` types already have all necessary fields. No structural changes needed.

---

## Test Cases

### Batch detection
1. Plan with 5 sub-goals all requiring `["read_file"]` → `is_batch` = true, routed to `act_with_tools()`.
2. Plan with 3 sub-goals requiring `["read_file"]`, `["read_file"]`, `["write_file"]` → `is_batch` = false (different tools).
3. Plan with 1 sub-goal requiring `["read_file"]` → `is_batch` = false (len == 1, not a batch).
4. Plan with 4 sub-goals each requiring `["search_text", "read_file"]` → `is_batch` = false (multi-tool per sub-goal).
5. Batch-detected plan with 8 sub-goals and `max_fan_out=4` → first 4 execute, 4 deferred (fan-out cap applied).

### Complexity floor
6. Plan with 3 trivial sub-goals (short descriptions, 0–1 tools each, different tools) → complexity floor triggers, routed to `act_with_tools()`.
7. Plan with 2 trivial + 1 moderate sub-goal → complexity floor does NOT trigger (not all trivial).
8. Plan with 3 sub-goals, all single-tool, but one has `ComplexityHint::Complex` → floor does NOT trigger.

### Cost gate
9. Plan estimated at 200 cents, remaining budget 100 cents → rejected (200 > 150).
10. Plan estimated at 140 cents, remaining budget 100 cents → NOT rejected (140 ≤ 150).
11. Plan estimated at 151 cents, remaining budget 100 cents → rejected (151 > 150).
12. Rejected plan produces `SignalKind::Blocked` signal with cost metadata.
13. Rejected plan's `ActionResult` text mentions cost rejection (testable via string assertion).

### Gate ordering
14. Plan that triggers both batch detection AND cost gate → batch detection wins (evaluated first, cheaper path).
15. Gates are evaluated in order: batch → floor → cost. First match short-circuits.

---

## Scope & Estimates

| Component | Files touched | Lines (est.) | Risk |
|-----------|--------------|-------------|------|
| `evaluate_decompose_gates()` method | `fx-kernel/src/loop_engine.rs` | ~60 | Low |
| `batch_to_tool_calls()` conversion | `fx-kernel/src/loop_engine.rs` | ~30 | Low |
| `estimate_plan_cost()` function | `fx-kernel/src/loop_engine.rs` | ~25 | Low |
| Export cost constants as `pub(crate)` | `fx-kernel/src/budget.rs` | ~4 | None |
| Gate call site in `execute_iteration()` | `fx-kernel/src/loop_engine.rs` | ~10 | Low |
| Signal emissions (trace + blocked) | `fx-kernel/src/loop_engine.rs` | ~20 | None |
| Tests | test module in `loop_engine.rs` | ~250 | None |
| **Total** | | **~400** | **Low** |

No new crates. No new dependencies. All changes within `engine/crates/fx-kernel/`. The batch-to-tool-call conversion is the trickiest part — it needs to produce valid `ToolCall` structs with arguments the tool executor can handle.
