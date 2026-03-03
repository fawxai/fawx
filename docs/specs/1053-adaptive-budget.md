# #1053 — Adaptive Budget Allocation for Recursive Decomposition

**Status:** Draft  
**Author:** Clawdio (scoping agent)  
**Date:** 2026-03-02  

---

## 1. Problem Statement

### Current behavior

The decomposition budget model currently splits child budgets inside decomposition orchestration:

- `run_cycle` enters decomposition through `Decision::Decompose` and calls `execute_decomposition` (`loop_engine.rs`, lines 732–739).
- `execute_decomposition` dispatches by strategy (`lines 754–763`) and remains the top-level control point for child execution.
- Child budgets are created in `run_sub_goal` (`lines 884–905`) via `BudgetTracker::child_tracker` (`budget.rs`, lines 222–228), which delegates to `partition_child` (`lines 197–214`).

Current split rules are fixed fractions:

- Sequential branch uses `per_goal_fraction = 0.5` (`loop_engine.rs`, line 785), producing a geometric series: 50%, 25%, 12.5%, 6.25%, …
- Parallel branch uses `per_goal_fraction = 1.0 / count` (`line 811`) from one parent remaining-budget snapshot.

This spec introduces adaptive concurrent allocation as **new functionality** at `execute_decomposition` strategy dispatch; it is not framed as modifying a separate pre-existing concurrent allocator API.

### Why this causes a budget death spiral

1. **Geometric starvation in sequential mode.** With 3 sequential sub-goals, allocations are 50% → 25% → 12.5%. The third sub-goal gets 1/8th of the original budget — often too little to complete even a single LLM call. At recursion depth 2 (a sub-goal itself decomposes), the innermost sub-goals receive ~1.5% of the root budget.

2. **No parent continuation budget.** After sub-goal execution, the parent calls `aggregate_sub_goal_results` (line 1684), and then `run_cycle` continues. Without a reserved continuation budget, children can consume everything and starve parent follow-up reasoning. (Today this mainly affects parent continuation; later it will also cover LLM synthesis.)

3. **No complexity awareness.** A trivial sub-goal ("check if file exists") gets the same fraction as a complex one ("refactor the authentication module"). Budget is wasted on simple tasks and starved on complex ones.

4. **No minimum floor.** `partition_child` with fraction=0 returns `max(1, 0)` = 1 for each resource — but there's no check that this minimum is actually viable. A sub-goal allocated 1 LLM call and 1 token is dead on arrival, yet the engine still spawns it, wastes wall time, and should instead return `SubGoalOutcome::Skipped` immediately.

5. **DRY violation in decomposition budget split logic.** There is no unified allocator API today; split policy is duplicated between sequential fixed-fraction selection (`loop_engine.rs`, line 785), parallel fixed-fraction selection (`line 811`), and child budget construction in `run_sub_goal` (`lines 891–893`) via `child_tracker`/`partition_child` (`budget.rs`, lines 197–228). A single `BudgetAllocator::allocate(..., AllocationMode, ...)` path reduces drift risk.

### Concrete failure scenario

```
Root budget: 64 LLM calls, depth 0
├── Decompose (3 sequential sub-goals)
│   ├── Sub-goal 1: 32 calls (50% of 64)
│   │   └── Decompose (2 sequential sub-goals)
│   │       ├── Sub-goal 1.1: 16 calls (50% of 32)
│   │       └── Sub-goal 1.2: 8 calls (50% of 16)
│   ├── Sub-goal 2: 4 calls (50% of remaining ~8 after sub-goal 1 consumed ~32)
│   │   └── Cannot decompose further — 4 calls insufficient
│   └── Sub-goal 3: 2 calls (50% of remaining ~4)
│       └── Single LLM call possible; likely BudgetExhausted
```

Sub-goal 3 at depth 0 gets 2 LLM calls. If it needs to decompose, its children get 1 call each — guaranteed failure.

---

## 2. Files to Change

| File | Lines (verified) | Change |
|------|------------------|--------|
| `engine/crates/fx-decompose/src/lib.rs` | `SubGoal` / `SubGoalOutcome` (`5–39`) | Add `ComplexityHint` enum, `ComplexityHint::weight()`, `SubGoal.complexity_hint`, and `SubGoalOutcome::Skipped`; keep `expected_output: Option<String>` unchanged |
| `engine/crates/fx-kernel/src/budget.rs` | `partition_child` (`197–214`) + `child_tracker` (`222–228`) | Migrate loop-engine callers from legacy fraction helpers to allocator outputs; remove obsolete helpers once wiring is complete |
| `engine/crates/fx-kernel/src/budget.rs` | type-definition area near `BudgetRemaining` / `BudgetResource` (`305–349`) | Add `AllocationMode`, `BudgetFloor`, `AllocationPlan`, `BudgetAllocator`, `estimate_complexity`, and deterministic integer-rounding helpers (using `fx_decompose::ComplexityHint`) |
| `engine/crates/fx-kernel/src/budget.rs` | `BudgetConfig` (`10–25`) + `BudgetTracker::remaining` (`142–157`) | Add `DepthMode` enum, `decompose_depth_mode` config field (default `Adaptive`), and `effective_max_depth(&BudgetRemaining) -> u32` threshold helper for runtime decomposition depth capping |
| `engine/crates/fx-kernel/src/loop_engine.rs` | imports (`3–29`) | Add `BudgetAllocator`, `BudgetConfig`, `AllocationMode` imports from `budget`; add `ComplexityHint` from `fx_decompose` |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `execute_decomposition` (`739–774`) | Keep as top-level orchestration point; compute one `AllocationPlan`, then dispatch sequential/concurrent helpers using allocator output |
| `engine/crates/fx-kernel/src/loop_engine.rs` | strategy dispatch in `execute_decomposition` (`754–761`) | Keep dispatch shape, but pass allocator-derived budgets/skipped metadata into both strategy helper paths |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `execute_sub_goals_sequential` (`776–799`) | Modify to consume allocator-produced per-sub-goal `BudgetConfig` values and skipped indices; remove hard-coded `per_goal_fraction = 0.5` |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `execute_sub_goals_concurrent` (`801–821`) | Modify to consume allocator-produced per-sub-goal `BudgetConfig` values and skipped indices; remove hard-coded `per_goal_fraction = 1.0 / count` |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `build_concurrent_futures` (`829–856`) | Change inputs from fraction-based budgeting to precomputed child `BudgetConfig` values while preserving concurrent execution model |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `collect_concurrent_results` (`858–871`) | Keep as aggregation helper; update to merge executed + skipped outcomes in stable sub-goal order while preserving signal/budget roll-up |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `run_sub_goal` (`884–905`) | Accept pre-computed `BudgetConfig`; preserve child `depth` increments and propagate effective depth cap via `max_recursion_depth = min(static_max, adaptive_cap)` when adaptive mode is enabled |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `DecomposeSubGoalArguments` + `From` impl (`222–236`) | Add optional `complexity_hint: Option<ComplexityHint>` field; keep `expected_output: Option<String>` for compatibility |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `parse_decomposition_plan` (`1719–1753`) | Preserve current strategy behavior: reject only `Custom`; keep `Sequential` and `Parallel` as supported runtime modes |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `format_sub_goal_outcome` (`1705–1711`) | Add exhaustive `SubGoalOutcome::Skipped` match arm |

---

## 3. API Design

### 3.1 Shared decomposition types in `fx-decompose`

`ComplexityHint` lives in `fx-decompose` (not `fx-kernel`) so `SubGoal` can reference it without introducing a `fx-decompose -> fx-kernel` dependency.

```rust
// fx-decompose/src/lib.rs
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ComplexityHint {
    /// Trivial: very short, no-tools checks.
    Trivial,
    /// Moderate: bounded work requiring some reasoning/tooling.
    Moderate,
    /// Complex: multi-step work, heavy tooling, or likely recursive decomposition.
    Complex,
}

impl ComplexityHint {
    /// Trivial=1, Moderate=2, Complex=4.
    pub const fn weight(self) -> u32 {
        match self {
            ComplexityHint::Trivial => 1,
            ComplexityHint::Moderate => 2,
            ComplexityHint::Complex => 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubGoal {
    pub description: String,
    pub required_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity_hint: Option<ComplexityHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubGoalOutcome {
    Completed(String),
    Failed(String),
    BudgetExhausted,
    /// Sub-goal intentionally not executed because allocation was below floor.
    Skipped,
}
```

Compatibility note (verified against current code): `expected_output` is currently `Option<String>` in both `SubGoal` (`fx-decompose/src/lib.rs`, line 9) and `DecomposeSubGoalArguments` (`loop_engine.rs`, line 227). This spec keeps that type unchanged to avoid a breaking serde/`From` conversion change.

`Skipped` is semantically distinct from `BudgetExhausted`:

- `BudgetExhausted`: sub-goal started execution and ran out of budget.
- `Skipped`: sub-goal never started because pre-flight allocation was non-viable.

### 3.2 New budget allocation types in `budget.rs`

```rust
// fx-kernel/src/budget.rs
use fx_decompose::{ComplexityHint, SubGoal};

/// Allocation policy selector.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AllocationMode {
    Sequential,
    Concurrent,
}

/// Result of budget allocation for a decomposition plan.
#[derive(Debug, Clone)]
pub struct AllocationPlan {
    /// Budget config for each sub-goal, in order.
    pub sub_goal_budgets: Vec<BudgetConfig>,
    /// Budget intentionally reserved for parent continuation after child execution.
    pub parent_continuation_budget: BudgetConfig,
    /// Sub-goals that were below the floor and should not run.
    pub skipped_indices: Vec<usize>,
}

/// Minimum resource thresholds. Any sub-goal below any threshold is skipped.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BudgetFloor {
    pub min_llm_calls: u32,
    pub min_tool_invocations: u32,
    pub min_tokens: u64,
    pub min_cost_cents: u64,
    pub min_wall_time_ms: u64,
}

impl Default for BudgetFloor {
    fn default() -> Self {
        // Rationale: minimum viable child loop for one reasoning step + one tool call,
        // with enough token/cost/time headroom to avoid immediate exhaustion thrash.
        Self {
            min_llm_calls: 2,
            min_tool_invocations: 2,
            min_tokens: 1_000,
            min_cost_cents: 4,
            min_wall_time_ms: 5_000,
        }
    }
}

/// Infallible allocator for decomposition budgets.
pub struct BudgetAllocator {
    /// Fraction of remaining budget reserved for parent continuation (0.0–1.0).
    /// Default: 0.10 (10%).
    pub parent_continuation_fraction: f32,
    /// Minimum viable budget for a sub-goal.
    pub floor: BudgetFloor,
}
```

Default `BudgetFloor` rationale (normative, V1):

- `min_llm_calls = 2`: allows at least one reasoning/planning pass plus one follow-up pass.
- `min_tool_invocations = 2`: allows one primary tool action plus one retry/fallback action.
- `min_tokens = 1_000`: enough for a short context window and non-trivial response without immediate truncation.
- `min_cost_cents = 4`: aligns with the repo's default 2¢ per LLM action heuristic (`2 actions × 2¢`).
- `min_wall_time_ms = 5_000`: avoids scheduling sub-goals that are likely to timeout before meaningful work starts.

### 3.3 `BudgetAllocator` methods

```rust
impl BudgetAllocator {
    pub fn new() -> Self {
        Self {
            parent_continuation_fraction: 0.10,
            floor: BudgetFloor::default(),
        }
    }

    /// Infallible allocation entry point.
    ///
    /// Degenerate inputs (zero remaining budget, zero sub-goals, all below floor)
    /// produce valid `AllocationPlan` values (often with empty `sub_goal_budgets`).
    ///
    /// V1 behavior:
    /// - Both Sequential and Concurrent use the same snapshot-based allocation algorithm.
    /// - Integer rounding strategy: floor each allocation, then distribute remainders
    ///   one-at-a-time to sub-goals in descending weight order.
    ///
    /// V2 compatibility note (non-breaking):
    /// - Keep `AllocationMode::{Sequential, Concurrent}` unchanged.
    /// - If dynamic sequential re-allocation is introduced, add allocator options
    ///   (e.g., `sequential_dynamic_reallocation: bool`, default `false`) rather
    ///   than changing existing enum variants.
    pub fn allocate(
        &self,
        parent: &BudgetTracker,
        sub_goals: &[SubGoal],
        mode: AllocationMode,
        current_time_ms: u64,
    ) -> AllocationPlan {
        // keep wrapper under 40 lines by delegating to:
        // compute_weights(), distribute_by_weight(), enforce_floor(), redistribute_skipped()
        ...
    }
}
```

Implementation note: `allocate()` should be decomposed into helper functions to stay under the 40-line function rule:

- `compute_weights()`
- `distribute_by_weight()`
- `enforce_floor()`
- `redistribute_skipped()`

`allocate()` becomes the only production allocation path. Remove `partition_child` / `child_tracker` in the same PR once loop-engine call sites are migrated.

#### Integer rounding strategy (normative, V1)

For each integer budget resource (`llm_calls`, `tool_invocations`, `tokens`, `cost_cents`, `wall_time_ms`):

1. Compute ideal weighted shares: `ideal_i = distributable * weight_i / total_weight`.
2. Allocate `floor(ideal_i)` to each sub-goal.
3. Compute `remainder = distributable - sum(floor(ideal_i))`.
4. Distribute remainder units one-at-a-time to sub-goals sorted by:
   1. higher `weight` first,
   2. then larger fractional remainder (`ideal_i - floor(ideal_i)`),
   3. then lower original sub-goal index (stable tie-breaker).

This guarantees deterministic results and exact conservation (`sum(allocations) == distributable`) per resource dimension.

### 3.4 Complexity estimation function (`budget.rs`)

```rust
/// Exhaustive keyword trigger list for Complex classification.
const COMPLEXITY_KEYWORDS: [&str; 6] = [
    "analyze",
    "refactor",
    "implement",
    "redesign",
    "migrate",
    "rewrite",
];

/// Estimate complexity from description and required tools.
///
/// Exact thresholds:
/// - Trivial  => description length < 50 chars AND required_tools == 0
/// - Moderate => description length in [50, 200] OR required_tools in [1, 2]
/// - Complex  => description length > 200
///               OR required_tools >= 3
///               OR description contains any keyword in COMPLEXITY_KEYWORDS
///
/// Evaluation order (normative, V1): Evaluate Complex conditions first; if none match, evaluate Trivial; default to Moderate.
/// (Equivalent rule: highest matching tier wins.)
///
/// Keyword matching semantics (normative, V1):
/// - case-insensitive via `to_ascii_lowercase()` normalization
/// - tokenize with regex split `r"[^a-z0-9]+"` (ASCII alphanumeric tokens only)
/// - drop empty tokens, then match keywords by exact token equality
/// - no substring matching ("implementation" does NOT match "implement")
pub fn estimate_complexity(sub_goal: &SubGoal) -> ComplexityHint {
    ...
}
```

The keyword list above is **exhaustive** for V1. No additional hidden keywords are used.

If `sub_goal.complexity_hint` is present, it overrides heuristic estimation.

### 3.5 Changes to decomposition argument and `SubGoal`

```rust
// loop_engine.rs
#[derive(Debug, Deserialize)]
struct DecomposeSubGoalArguments {
    description: String,
    #[serde(default)]
    required_tools: Vec<String>,
    #[serde(default)]
    expected_output: Option<String>,
    #[serde(default)]
    complexity_hint: Option<ComplexityHint>,
}

// fx-decompose/src/lib.rs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubGoal {
    pub description: String,
    pub required_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity_hint: Option<ComplexityHint>,
}
```

No `expected_output` type migration is proposed: keep `Option<String>` in both structs for wire compatibility and existing tests.

### 3.6 Loop engine signature changes

```rust
// execute_decomposition: single allocator entry point for decomposition budgeting
async fn execute_decomposition(
    &mut self,
    decision: &Decision,
    plan: &DecompositionPlan,
    llm: &dyn LlmProvider,
    context_messages: &[Message],
) -> Result<ActionResult, LoopError> {
    let allocator = BudgetAllocator::new();
    let mode = match plan.strategy {
        AggregationStrategy::Sequential => AllocationMode::Sequential,
        AggregationStrategy::Parallel => AllocationMode::Concurrent,
        AggregationStrategy::Custom(_) => unreachable!(),
    };
    let allocation = allocator.allocate(
        &self.budget,
        &plan.sub_goals,
        mode,
        current_time_ms(),
    );
    // Route both strategy branches through allocation output.
    // skipped_indices map directly to SubGoalOutcome::Skipped.
    ...
}

// run_sub_goal: receives already-allocated config
async fn run_sub_goal(
    &self,
    sub_goal: &SubGoal,
    child_config: BudgetConfig,
    llm: &dyn LlmProvider,
    context_messages: &[Message],
) -> SubGoalExecution {
    // Preserve the existing recursion accounting model:
    // - depth increases by 1 for each child tracker
    // - in Static mode, max_recursion_depth remains unchanged
    // - in Adaptive mode, child configs inherit the effective capped max depth
    let child_budget = BudgetTracker::new(
        child_config,
        current_time_ms(),
        self.budget.child_depth(),
    );
    ...
}
```

Adaptive concurrent allocation in this spec is **new functionality** added at `execute_decomposition` strategy dispatch, not a modification of a pre-existing concurrent allocator API.

### 3.7 Required import + formatting updates

```rust
// loop_engine.rs imports
use crate::budget::{
    ActionCost, AllocationMode, BudgetAllocator, BudgetConfig, BudgetRemaining,
    BudgetResource, BudgetTracker,
};
use fx_decompose::{
    AggregationStrategy, ComplexityHint, DecompositionPlan, SubGoal, SubGoalOutcome, SubGoalResult,
};

fn format_sub_goal_outcome(outcome: &SubGoalOutcome) -> String {
    match outcome {
        SubGoalOutcome::Completed(response) => format!("completed: {response}"),
        SubGoalOutcome::Failed(message) => format!("failed: {message}"),
        SubGoalOutcome::BudgetExhausted => "budget exhausted".to_string(),
        SubGoalOutcome::Skipped => "skipped (below floor)".to_string(),
    }
}
```

### 3.8 Dynamic Effective Depth Cap (Inspired by Fawx self-analysis spec)

Add a runtime depth cap derived from remaining budget so decomposition automatically becomes shallower as budget shrinks.

```rust
// fx-kernel/src/budget.rs
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DepthMode {
    Static,
    Adaptive,
}

impl Default for DepthMode {
    fn default() -> Self {
        Self::Adaptive
    }
}

pub fn effective_max_depth(remaining: &BudgetRemaining) -> u32 {
    match remaining.llm_calls {
        calls if calls > 32 => 3,
        calls if calls > 16 => 2,
        calls if calls > 6 => 1,
        _ => 0, // no decomposition
    }
}
```

`BudgetConfig` adds decomposition depth mode control:

```rust
pub struct BudgetConfig {
    ...
    pub max_recursion_depth: u32,
    #[serde(default)]
    pub decompose_depth_mode: DepthMode,
}
```

Runtime semantics (normative, V1):

- `DepthMode::Static`: effective cap = configured `max_recursion_depth` (current behavior).
- `DepthMode::Adaptive`: effective cap = `min(max_recursion_depth, effective_max_depth(&remaining))`.
- Effective cap `0` means decomposition is blocked for the current budget state.

`execute_decomposition` integration (normative):

1. Compute `remaining = self.budget.remaining(current_time_ms())`.
2. Compute effective cap from `decompose_depth_mode`.
3. If `self.budget.depth() >= effective_cap`, return the depth-limited decomposition result before strategy dispatch.
4. When constructing child `BudgetConfig`, inherit this effective cap (not only static config max).

Attribution: **Inspired by Fawx self-analysis spec**.

---

## 4. Implementation Plan

### Step 1: Add shared decomposition types (`fx-decompose`)

- Add `ComplexityHint` with `const fn weight()` in `fx-decompose/src/lib.rs`.
- Add `complexity_hint: Option<ComplexityHint>` to `SubGoal`.
- Add `SubGoalOutcome::Skipped` in `fx-decompose` (the enum’s source-of-truth crate).

### Step 2: Implement `BudgetAllocator::allocate()` in `budget.rs`

- Implement `BudgetAllocator::new()` with `parent_continuation_fraction = 0.10`.
- Keep `allocate()` intentionally infallible.
- Inside `allocate()`, call helper functions to stay under 40 lines:
  1. `compute_weights()`
  2. `distribute_by_weight()`
  3. `enforce_floor()`
  4. `redistribute_skipped()`
- Remove `partition_child` / `child_tracker` after allocator wiring completes (dead code cleanup mandated by `ENGINEERING.md`).
- Core flow:
  1. `remaining = parent.remaining(current_time_ms)`
  2. `parent_continuation_budget = reserve from remaining by parent_continuation_fraction` (clamped)
  3. `distributable = remaining - parent_continuation_budget`
  4. Complexity-weighted distribution over sub-goals
  5. Floor each allocation. Distribute remainders one-at-a-time to sub-goals in descending weight order.
  6. Floor enforcement (`BudgetFloor`)
  7. Redistribute budget from skipped goals to viable goals
  8. Return valid `AllocationPlan` (possibly empty)
- V1: `AllocationMode::Sequential` and `AllocationMode::Concurrent` both use snapshot-based distribution.
- V2: keep `AllocationMode` stable; add behavior via allocator config flags (additive only).

### Step 3: Add `complexity_hint` plumbing in loop input parsing

- Add `complexity_hint: Option<ComplexityHint>` to `DecomposeSubGoalArguments` (`loop_engine.rs`, lines 222–228).
- Update conversion code (`lines 230–236`) from arguments → `SubGoal` to pass the field through.
- Keep `expected_output` as `Option<String>` (no type migration) and maintain backward compatibility with serde defaults.

### Step 4: Wire allocator at `execute_decomposition` orchestration point (`loop_engine.rs`, lines 739–774)

- In `execute_decomposition`, build exactly one `AllocationPlan` via `allocate(..., mode, ...)`, where `mode` is derived from `plan.strategy`.
- Keep strategy dispatch (`lines 754–761`) as the top-level router, but pass allocator outputs into both helper paths instead of recomputing split fractions.
- Modify `execute_sub_goals_sequential` (`lines 776–799`) to consume allocator-produced per-sub-goal `BudgetConfig` values and skipped indices; remove hard-coded `per_goal_fraction = 0.5`.
- Update `run_sub_goal` (`lines 884–905`) to accept pre-computed `BudgetConfig` instead of a raw fraction.
- For skipped indices, emit `SubGoalOutcome::Skipped` without spawning child loops.

### Step 5: Add adaptive concurrent allocation as NEW functionality

- Modify `execute_sub_goals_concurrent` (`lines 801–821`) to consume allocator-produced per-sub-goal `BudgetConfig` values and skipped indices; remove hard-coded `per_goal_fraction = 1.0 / count`.
- Update `build_concurrent_futures` (`lines 829–856`) to accept pre-computed child `BudgetConfig` values instead of a scalar fraction, while preserving existing `join_all` cooperative-concurrency behavior.
- Keep `collect_concurrent_results` (`lines 858–871`) as the aggregation helper, but update it to merge executed + skipped outcomes in stable sub-goal order.
- This is additive functionality for concurrent allocation at dispatch/runtime wiring, not a modification of a pre-existing concurrent allocator API.

### Step 6: Preserve recursion accounting model (avoid double counting)

- Keep depth enforcement depth-based: recursion guard remains `depth >= max_recursion_depth`.
- Child trackers continue to use `self.budget.child_depth()`.
- Do **not** decrement `max_recursion_depth` arithmetically per recursion step; in adaptive mode, set child `max_recursion_depth` to the precomputed effective cap.

### Step 7: Update imports and exhaustive formatting paths

- Update `loop_engine.rs` imports for allocator + `ComplexityHint` symbols.
- Update `format_sub_goal_outcome` exhaustive match with `Skipped` arm.

### Step 8: Update and add tests

- Update assertions in existing decomposition tests that assumed 50%/1-N split.
- Add allocator, complexity, formatting, and skipped-outcome tests from §5.

### Step 9: Add dynamic effective depth cap wiring (Fawx addendum)

- Add `DepthMode` to `BudgetConfig` with default `Adaptive` and keep `Static` for compatibility.
- Implement `effective_max_depth(remaining: &BudgetRemaining) -> u32` in `budget.rs` with thresholds:
  - `remaining.llm_calls > 32` => depth `3`
  - `remaining.llm_calls > 16` => depth `2`
  - `remaining.llm_calls > 6` => depth `1`
  - `remaining.llm_calls <= 6` => depth `0` (no decomposition)
- In `execute_decomposition`, compute effective cap before strategy dispatch and short-circuit to depth-limited result when cap is reached.
- When preparing child configs in `run_sub_goal` / concurrent builder paths, inherit the effective cap (`min(static_max, adaptive_cap)`) rather than only static max.
- Credit in docs and PR context: **Inspired by Fawx self-analysis spec**.

---

## 5. Test Plan

### 5.1 Unit tests for `BudgetAllocator` (`budget.rs`)

| Test | Description |
|------|-------------|
| `allocate_reserves_parent_continuation_budget` | With 10% reserve and 100 tokens remaining, parent continuation gets ≥10 tokens and sub-goals share ≤90 |
| `allocate_distributes_by_complexity_weight` | 1 Trivial (w=1) + 1 Complex (w=4): complex gets ~4x budget |
| `allocate_integer_rounding_conserves_resource_totals` | For each integer resource, distributed allocations sum exactly to distributable amount |
| `allocate_integer_rounding_is_deterministic_for_ties` | Tie cases produce stable allocations via weight/remainder/index ordering |
| `allocate_skips_sub_goals_below_floor` | Very limited budget + 3 sub-goals marks at least one skipped |
| `allocate_redistributes_skipped_budget` | Budget from skipped goals is redistributed to viable ones |
| `allocate_single_sub_goal_gets_full_distributable` | Single sub-goal gets all distributable budget |
| `allocate_all_sub_goals_below_floor_returns_all_skipped` | All indices skipped; plan remains valid |
| `allocate_mode_sequential_and_concurrent_match_in_v1` | Both modes produce same snapshot-based plan in v1 |
| `allocate_zero_sub_goals_returns_empty_plan` | Empty input returns valid empty plan |
| `allocate_zero_remaining_is_infallible` | Zero remaining budget returns empty/zeroed plan, no error |
| `parent_continuation_budget_clamps_to_remaining` | Reserve math clamps safely for tiny remaining budgets |
| `complexity_hint_overrides_heuristic` | Explicit hint wins over heuristic |

### 5.2 Unit tests for `estimate_complexity` (`budget.rs`)

| Test | Description |
|------|-------------|
| `trivial_for_short_description_no_tools` | `<50 chars` and `0 tools` => Trivial |
| `complex_keyword_preempts_trivial_when_conditions_overlap` | Short/no-tools goal containing a complex keyword (e.g., `refactor`) => Complex |
| `boundary_50_chars_is_moderate_not_trivial` | Exactly 50 chars => Moderate |
| `exactly_2_tools_is_moderate` | Exactly 2 tools => Moderate |
| `exactly_3_tools_is_complex` | Exactly 3 tools => Complex |
| `moderate_for_medium_description_few_tools` | 50–200 chars or 1–2 tools => Moderate |
| `complex_for_long_description` | `>200 chars` => Complex |
| `keyword_matching_is_case_insensitive` | `"Refactor"` and `"refactor"` both match |
| `keyword_matching_uses_word_boundaries` | `"implementation"` does not match `"implement"`; `"implement API"` does |
| `complex_for_exhaustive_keyword_list` | Each of ["analyze", "refactor", "implement", "redesign", "migrate", "rewrite"] => Complex |

### 5.3 Unit tests for `fx-decompose` shared types (`fx-decompose/src/lib.rs`)

| Test | Description |
|------|-------------|
| `sub_goal_with_complexity_hint_roundtrip_serde` | `SubGoal` serializes/deserializes with `complexity_hint` |
| `complexity_hint_weight_values_are_stable` | Trivial=1, Moderate=2, Complex=4 |
| `sub_goal_outcome_skipped_roundtrip_serde` | `SubGoalOutcome::Skipped` serializes/deserializes correctly |

### 5.4 Integration tests in `loop_engine.rs`

| Test | Description |
|------|-------------|
| `sequential_adaptive_allocation_gives_more_to_complex_sub_goals` | Complex child gets larger budget than trivial child |
| `concurrent_adaptive_allocation_distributes_proportionally` | Same weighting behavior in concurrent execution |
| `budget_floor_skips_non_viable_sub_goals_with_signal` | Non-viable goals are not run and produce `SubGoalOutcome::Skipped` |
| `parent_continuation_budget_prevents_parent_starvation` | Parent retains at least 10% continuation budget |
| `child_budget_increments_depth_and_inherits_effective_max_depth` | Child uses `child_depth()` and inherits the effective cap (`min(static_max, adaptive_cap)`) |
| `format_sub_goal_outcome_includes_skipped_variant` | Exhaustive formatter returns stable string for `Skipped` |
| `backward_compat_no_complexity_hint` | Existing behavior remains compatible when hints are absent |

### 5.5 Budget death spiral regression tests

| Test | Description |
|------|-------------|
| `third_sequential_sub_goal_gets_viable_budget` | Third sequential goal still gets at least floor budget under moderate root budget |
| `nested_decomposition_all_leaves_get_floor_budget_or_skipped` | Depth-2 leaves are either viable (≥ floor) or explicitly `Skipped` |

### 5.6 Dynamic effective depth cap tests (`budget.rs` + `loop_engine.rs`)

| Test | Description |
|------|-------------|
| `effective_max_depth_threshold_mapping_is_stable` | Verifies `>32 => 3`, `>16 => 2`, `>6 => 1`, `<=6 => 0` based on `remaining.llm_calls` |
| `depth_mode_default_is_adaptive` | `BudgetConfig` defaults `decompose_depth_mode` to `DepthMode::Adaptive` |
| `depth_mode_static_ignores_budget_derived_depth` | `DepthMode::Static` uses configured `max_recursion_depth` even when budget-derived cap is lower |
| `depth_mode_adaptive_uses_min_of_static_and_effective_cap` | Adaptive mode computes `min(max_recursion_depth, effective_max_depth(...))` |
| `execute_decomposition_blocks_when_effective_cap_zero` | With `remaining.llm_calls <= 6`, decomposition short-circuits before spawning sub-goals |
| `execute_decomposition_blocks_when_current_depth_meets_effective_cap` | Runtime guard blocks decomposition once `depth >= effective_cap` |
| `child_budget_inherits_effective_cap_in_adaptive_mode` | Child `BudgetConfig.max_recursion_depth` uses effective cap instead of static-only inheritance |

---

## 6. Edge Cases and Risks

### Edge cases

| Case | Handling |
|------|----------|
| **Zero sub-goals** | `allocate()` returns a valid empty `AllocationPlan`; no panic, no error. |
| **Single sub-goal** | Gets 100% of distributable budget (remaining minus parent continuation budget). |
| **All sub-goals below floor** | All indices enter `skipped_indices`; execution returns `SubGoalOutcome::Skipped` for each. |
| **Zero remaining budget** | Infallible behavior: return empty/zero budget plan, all sub-goals skipped. |
| **Adaptive cap computes depth 0 (`remaining.llm_calls <= 6`)** | `execute_decomposition` short-circuits to the depth-limited response; no child loops are spawned. |
| **Static max lower than adaptive threshold** | Effective cap is always `min(static_max, adaptive_cap)` so static safety bounds remain authoritative. |
| **Deeply nested decomposition (depth ≥ 3)** | Depth accounting remains `depth`-based; each child increments `depth` by exactly 1. |
| **Concurrent sub-goals total allocation > remaining** | Not possible by construction; allocations are from one distributable snapshot with normalized fractions. |
| **Very large number of sub-goals (existing runtime cap = 5)** | Cap is pre-existing (`MAX_SUB_GOALS = 5` at `loop_engine.rs:250`, enforced by truncation in `parse_decomposition_plan` at `lines 1740–1747`). This spec does not introduce a new validation. Equal-complexity case: each gets 18% of remaining (90% distributable / 5). |

### Risks

| Risk | Mitigation |
|------|------------|
| **Breaking existing test expectations** | Update tests that hard-code 50%/1-N assumptions. |
| **Heuristic misclassification** | Keep heuristic deterministic + explicit override via `complexity_hint`; specify keyword semantics precisely. |
| **Depth-threshold mapping too conservative for some workloads** | Keep thresholds centralized in `effective_max_depth`; retain `DepthMode::Static` fallback and allow future table/log strategy as an additive config option. |
| **10% continuation reserve too small for future synthesis** | Explicitly documented tunable: increase reserve when LLM synthesis is implemented. |
| **Sequential dynamic re-allocation requested later** | Keep `AllocationMode` stable and add behavior via allocator options (non-breaking V2 path). |
| **Cross-crate type placement causing cycle** | Define `ComplexityHint` in `fx-decompose` so `fx-kernel` depends one-way on shared decomposition types. |

---

## 7. Estimated Complexity

| Component | Effort |
|-----------|--------|
| `ComplexityHint`, `SubGoal`, `SubGoalOutcome::Skipped` (`fx-decompose`) | Small (~90 lines) |
| `AllocationMode`, `BudgetFloor`, `AllocationPlan` (`budget.rs`) | Small (~70 lines) |
| `estimate_complexity` exact-threshold heuristic + keyword semantics | Small (~55 lines) |
| `BudgetAllocator::allocate` + helper decomposition | Medium (~120 lines) |
| Loop engine wiring (`Sequential`/`Concurrent` + `Skipped`) | Medium (~110 lines changed) |
| Dynamic effective depth cap (`DepthMode`, cap guard, child cap inheritance) | Small (~60 lines) |
| New/updated unit + integration tests | Medium (~250 lines) |
| **Total** | **~755 lines** |

**Classification:** Standard complexity (single-PR feature). Contained to `fx-kernel` + `fx-decompose`, with additive type changes and explicit behavior updates.

**Estimated review cycles:** 1-2.
