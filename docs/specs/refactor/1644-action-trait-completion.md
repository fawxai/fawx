# Spec: #1644 — Complete ActionNextStep Migration

## Status
Foundation laid in PR #1648. `ActionNextStep` drives the primary `run_cycle` loop. Secondary code paths still use `tool_results.is_empty()` as an implicit action type signal.

## Goal
Eliminate all non-test uses of `tool_results.is_empty()` as a behavioral branch. Every code path that changes behavior based on whether tools ran should use `ActionNextStep` or `ActionResult` metadata instead.

## Current State (codex/provider-owned-loop-refactor branch)

File: `engine/crates/fx-kernel/src/loop_engine.rs` (24,282 lines)

### Remaining `tool_results.is_empty()` behavioral branches

**1. Budget accounting (line 1979)**
```rust
if action.tool_results.is_empty() {
    let action_cost = self.action_cost_from_result(&action);
    if let Some(result) = self.budget_terminal(action_cost, action_partial.clone()) { ... }
    self.budget.record(&action_cost);
} else if let Some(result) = self.budget_terminal(ActionCost::default(), ...) { ... }
```
Fix: Add `ActionResult::has_tool_activity() -> bool` method. Or derive from `next_step`: a `Continue` always had tools, a `Finish` may or may not have.

**2. Tool turn counter (line 2298)**
```rust
fn update_tool_turns(&mut self, action: &ActionResult) {
    if !action.tool_results.is_empty() {
        self.consecutive_tool_turns = self.consecutive_tool_turns.saturating_add(1);
    } else {
        self.consecutive_tool_turns = 0;
    }
}
```
Fix: Same — use `action.has_tool_activity()` or match on `next_step`.

**3. Sub-goal budget (line 4022)**
```rust
if action.tool_results.is_empty() {
    child.budget.record(&child.action_cost_from_result(&action));
}
```
Fix: Same pattern.

**4. Observation signals (line 4564)**
```rust
let has_tools = !action.tool_results.is_empty();
```
This is used for observability signal emission (trace logging). It's a legitimate direct check on tool count — not a behavioral branch. Keep as-is but consider renaming to `tool_invocation_count`.

**5. Decomposition merge (line 5306)**
```rust
if !prior_tool_results.is_empty() {
    let mut merged_tool_results = prior_tool_results;
    ...
}
```
This merges tool results from prior decomposition steps. It's checking the collection, not inferring behavior. Keep as-is.

**6. Action cost accounting (line 6210-6214)**
```rust
tool_invocations: action.tool_results.len() as u32,
cost_cents: if action.tokens_used.total_tokens() > 0 {
    DEFAULT_LLM_ACTION_COST_CENTS
} else if action.tool_results.is_empty() {
    0
} else {
    1
},
```
Fix: Use `action.tool_results.len()` for counts (fine), but the cost branch should derive from the action type, not tool result emptiness.

## Deliverables

1. Add `ActionResult::has_tool_activity(&self) -> bool` convenience method
2. Replace behavioral branches at lines 1958, 2235, 3880 with `has_tool_activity()` or `next_step` matching
3. Replace cost branch at line 6069 with explicit action type logic
4. Leave observability (4421) and collection merges (5163) as-is — these are legitimate direct checks
5. All existing tests must pass
6. Add regression test: an `ActionResult` with empty `tool_results` but `ActionNextStep::Continue` should still be treated as tool-active (this case doesn't exist today but must not break)

## Files to modify
- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-kernel/src/act.rs` (add `has_tool_activity`)

## Not in scope
- Restructuring `ActionResult` itself
- Decomposing `loop_engine.rs` (that's #1641)
