# Loop Resilience: Budget Soft-Ceiling, Fan-Out Cap, Tool Result Truncation

**Status:** Approved spec  
**Issue:** Addresses Fawx's self-diagnosed loop failures (2026-03-03 smoke test)  
**Depends on:** PR #1095 (`feat/930-tui-streaming`)  
**Branch:** `feat/loop-resilience`

---

## Problem

Three failure modes compound in production:

1. **Budget exhaustion with no graceful degradation.** The agent iterates until the hard budget wall, then gets cut off mid-sentence. No summary, no "here's what's left." Violates ENGINEERING.md §2 ("Fail Fast and Loudly").

2. **Unbounded fan-out.** The model can call 10+ tools in a single turn. Each tool result inflates context. With large-result tools (read_file on HTML docs), a single turn can exceed the context window: `context_exceeded_after_compaction: estimated_tokens=136289 hard_limit_tokens=121904`.

3. **Unbounded tool result size.** A single `read_file` on a 50KB file consumes ~12K tokens. No per-result limit means one bad tool call can eat 10% of context.

These are independent failure modes that need independent fixes. Together they form a resilience layer that prevents the most common production crashes.

---

## Fix 1: Budget Soft-Ceiling

### Design

Replace boolean `is_exhausted` checks with a `BudgetState` enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BudgetState {
    /// Below soft-ceiling. Full capabilities.
    Normal,
    /// Soft-ceiling crossed. Wrap-up mode: no tools, no decompose, synthesize only.
    Low,
}
```

Only two states. `Exhausted` is already handled by the existing `BudgetExceeded` / `LoopResult::BudgetExhausted` path — no need to duplicate that in the enum (YAGNI).

Transitions are **monotonic within a single `run_cycle()` call** — once `Low`, never back to `Normal` within that cycle. `BudgetTracker::reset()` in `prepare_cycle()` starts fresh.

### Which resource triggers `Low`?

`BudgetTracker` tracks 5 independent resources: LLM calls, tool invocations, tokens, cost (cents), and wall time. A single threshold on "budget" is ambiguous.

**Trigger: cost_cents, with LLM calls as secondary.**

- `cost_cents` is the user-facing budget limit — it's the resource users care about and the one exposed in `BudgetConfig`.
- `llm_calls` is the secondary trigger because it's the next most meaningful limit (each LLM call has real cost/latency).
- Wall time is excluded: hitting 80% wall time while at 10% cost shouldn't force wrap-up.

Implementation: `BudgetTracker::state()` returns `Low` when **either** `cost_cents` or `llm_calls` exceeds the soft-ceiling fraction of their respective max.

### Behavior by state

| State | Tools | Decompose | Retries | Synthesis |
|-------|-------|-----------|---------|-----------|
| Normal | ✅ | ✅ | ✅ | ✅ |
| Low | ❌ | ❌ | ❌ | ✅ + wrap-up prompt |

### Wrap-up flow

When `BudgetState::Low` is entered, the loop performs **one final LLM call** with a wrap-up prompt. This costs budget, so the soft-ceiling must leave enough headroom for that call.

The wrap-up directive is injected during the `perceive` step (where context is already assembled, including `budget_remaining` in `ProcessedPerception`). When `budget.state()` is `Low`, `perceive()` appends to the context window:

> You are running low on budget. Do not call any tools. Do not decompose. Summarize what you have accomplished and what remains undone. Be concise.

The LLM then produces a synthesis response. `decide()` routes it as `Decision::Respond` (no tools). The loop terminates normally.

**Budget reservation:** The soft-ceiling fraction (default 0.80) leaves 20% headroom. A single wrap-up LLM call typically costs ~1-3% of total budget. 20% is more than sufficient.

### Where to change

1. **`engine/crates/fx-kernel/src/budget.rs`** — `BudgetTracker`:
   - Add `pub fn state(&self, current_time_ms: u64) -> BudgetState` method. Computes `cost_cents / max_cost_cents` and `llm_calls / max_llm_calls` ratios. Returns `Low` if either exceeds `soft_ceiling_fraction`.
   - Add `soft_ceiling_fraction: f64` field to `BudgetConfig` (default 0.80). Configurable, consistent with existing pattern where all limits live in `BudgetConfig`.

2. **`engine/crates/fx-kernel/src/loop_engine.rs`** — `execute_iteration()`:
   - Before tool dispatch and decomposition, check `self.budget.state(current_time_ms())`. If `Low`, skip tool/decompose routing — let the response flow to `Decision::Respond` path.
   - The 4 existing `budget_terminal()` call sites remain unchanged (they handle the hard `Exhausted` case).

3. **`engine/crates/fx-kernel/src/loop_engine.rs`** — `perceive()`:
   - When `self.budget.state(snapshot.timestamp_ms) == BudgetState::Low`, append the wrap-up directive as a system message in the context window. This is the cleanest injection point since `perceive()` already builds the context including `budget_remaining`.

4. **`engine/crates/fx-kernel/src/loop_engine.rs`** — `execute_decomposition()` / `find_decompose_tool_call()`:
   - Early-return with descriptive error if `budget.state() == Low`. Prevents decomposition from being attempted during wrap-up.

5. **`engine/crates/fx-kernel/src/loop_engine.rs`** — `execute_tool_calls()` / `act_with_tools()`:
   - Early-return with descriptive error if `budget.state() == Low`. Prevents tool dispatch during wrap-up.

6. **`engine/crates/fx-core/src/signals.rs`** — `SignalKind`:
   - Use existing `SignalKind::Performance` (already exists) for the `Normal → Low` transition signal. No new variant needed — this IS a performance signal. Emitted exactly once per cycle on transition.

---

## Fix 2: Fan-Out Cap

### Design

Limit the number of tool calls executed per LLM response.

When the model requests N tool calls in a single response and N exceeds the cap, execute only the first `max_fan_out` calls. Return results for executed calls plus a system message for deferred calls:

> Tool calls deferred (budget: {executed}/{total}): {deferred_tool_names}. Re-request in your next turn if still needed.

### Where to change

1. **`engine/crates/fx-kernel/src/loop_engine.rs`** — `act_with_tools()`:
   - After receiving `calls: &[ToolCall]`, if `calls.len() > self.max_fan_out`, split into `execute` (first N) and `deferred` (rest). Pass only `execute` to `execute_tool_round()`. Append a synthetic tool result or system message listing deferred call names.

2. **`engine/crates/fx-kernel/src/loop_engine.rs`** — Config:
   - Add `max_fan_out: usize` to `LoopEngine` builder (default: 4). Sourced from `BudgetConfig` or a new `LoopConfig` field.

### Why 4

- Most productive tool batches are 2-3 calls (read 2 files, run a command + read output).
- 4 covers all reasonable parallel-work patterns.
- The model learns to batch across turns instead of dumping 13 reads at once.
- The 5th+ calls are almost always "might as well while I'm here" speculation — exactly what YAGNI prohibits.

---

## Fix 3: Tool Result Truncation

### Design

Cap individual tool result size. When a tool returns content exceeding the limit, truncate and append a marker:

```
[truncated — {remaining_bytes} bytes omitted, {total_bytes} total]
```

### Where to change

1. **`engine/crates/fx-kernel/src/loop_engine.rs`** — after `execute_tool_calls()` returns results, before injecting into conversation:
   - Check each `result.output.len()`. If exceeding `max_tool_result_bytes`, truncate to limit and append marker.

2. **Config** — Add `max_tool_result_bytes: usize` to loop config (default: 16384 = 16KB ≈ 4K tokens).

### Why 16KB

- Most useful file content fits in 16KB (source files, configs, short docs).
- Large files (HTML docs, logs, data) rarely need to be read in full — the model should use search_text for targeted extraction.
- 4 tool calls × 16KB = 64KB worst case ≈ 16K tokens — fits comfortably in context alongside conversation history.
- The truncation marker teaches the model to use more targeted queries.

### Edge cases

- **Binary/non-text results**: Truncation applies to the string representation. Binary tool results that are already base64-encoded get truncated the same way.
- **Structured results (JSON)**: Truncate the raw string. Don't try to truncate at a JSON boundary — the model handles partial JSON fine and the marker makes it clear.
- **Empty results**: No truncation needed.

---

## Test Cases

### Budget soft-ceiling
1. Agent at 79% cost budget → `state()` returns `Normal`.
2. Agent at 81% cost budget → `state()` returns `Low`, wrap-up directive injected into context by `perceive()`.
3. Agent at 81% LLM calls budget (cost still Normal) → `state()` returns `Low` (secondary trigger).
4. Tool dispatch blocked when `state() == Low` — returns descriptive error.
5. `decompose` tool call at 85% cost → blocked, returns error, not a plan.
6. `BudgetState` monotonic within `run_cycle()`: Low never reverts to Normal.
7. `SignalKind::Performance` emitted exactly once on Normal→Low transition.
8. Wrap-up directive is present in `perceive()` output when `state() == Low` (unit-testable without LLM).

### Fan-out cap
9. 3 tool calls with cap=4 → all 3 execute.
10. 6 tool calls with cap=4 → first 4 execute, last 2 deferred with message.
11. Deferred message lists correct tool names.
12. Cap=1 forces strictly sequential tool execution.

### Tool result truncation
13. 8KB result with 16KB limit → no truncation.
14. 32KB result with 16KB limit → truncated to 16KB + marker.
15. Marker includes correct byte counts.
16. Empty result → no truncation, no marker.

---

## Scope & Estimates

| Component | Files touched | Lines (est.) | Risk |
|-----------|--------------|-------------|------|
| `BudgetState` enum + `state()` method | `fx-kernel/src/budget.rs` | ~50 | Low |
| `soft_ceiling_fraction` in `BudgetConfig` | `fx-kernel/src/budget.rs` | ~10 | None |
| Loop driver state checks (tool/decompose gating) | `fx-kernel/src/loop_engine.rs` | ~40 | Low |
| Wrap-up directive injection in `perceive()` | `fx-kernel/src/loop_engine.rs` | ~15 | Low |
| `Performance` signal on transition | `fx-kernel/src/loop_engine.rs` | ~10 | None |
| Fan-out cap in `act_with_tools()` | `fx-kernel/src/loop_engine.rs` | ~30 | None |
| Tool result truncation | `fx-kernel/src/loop_engine.rs` | ~30 | None |
| Config fields (`max_fan_out`, `max_tool_result_bytes`) | `fx-kernel/src/budget.rs` or builder | ~15 | None |
| Tests | test modules | ~200 | None |
| **Total** | | **~400** | **Low** |

No new crates. No new dependencies. All changes are in existing modules within `engine/crates/fx-kernel/`. Most are single-point additions with clear test boundaries.

---

## What This Does NOT Cover

- **True context-aware dispatch** (predicting tool result sizes, tracking running context) — deferred. Truncation + fan-out solve 90% of context overflow cases without the complexity.
- **Incremental markdown rendering during streaming** — separate concern, tracked in #1097.
- **Memory pruning/decay** — orthogonal, valuable but independent.
- **Per-tool retry budgets** — valuable but lower priority. The soft ceiling catches runaway retries at the global level.
- **Per-resource thresholds** — wall time at 80% means something very different from cost at 80%. A future iteration could have per-resource soft ceilings with independent fractions, but YAGNI for v1.

---

## Relationship to Existing Budget Infrastructure

**`BudgetTracker` (execution budget)** tracks cost, calls, tokens, time — the resources consumed by the loop engine. This spec adds soft-ceiling awareness to it.

**`ConversationBudget` (context budget)** in `conversation_compactor.rs` tracks context window token usage and triggers compaction. It is orthogonal — compaction is not affected by the soft ceiling, and vice versa. They protect against different failure modes (execution cost vs context overflow).

---

## Relationship to Streaming PR (#1095)

This PR branches off `feat/930-tui-streaming` because:
1. The streaming path makes budget exhaustion more visible (user watches tokens flow then sees abrupt cutoff).
2. The fan-out cap applies to streaming tool calls too.
3. The context overflow was discovered during streaming smoke testing.

But this PR's changes are in the **kernel and loop engine**, not the TUI or streaming infrastructure. The two PRs can be reviewed and merged independently once #1095 lands.
