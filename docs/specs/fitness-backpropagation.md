# Fitness Backpropagation Spec

**Crate:** `fx-decompose` (engine.rs, context.rs) + `fx-consensus` (chain integration)  
**Difficulty:** Medium  
**Status:** 0% — chain stores scores but nothing feeds them back  

---

## Problem

The experiment chain records what worked and what didn't:
- Winning patches with aggregate scores
- Failed approaches with reasons
- Score deltas between candidates

The auto-decomposition engine currently ignores all of this. When decomposing a new problem, the `LlmDecomposer` doesn't know:
1. Which sub-goal strategies worked last time
2. Which approaches were tried and failed
3. What score ranges to expect
4. Whether to allocate more budget to hard sub-goals

---

## Solution: FitnessContext

Enrich `DecompositionContext` with historical fitness data that the decomposer uses to make better plans.

### Data Flow

```
Chain entries for signal
  → extract per-sub-goal outcomes (which worked, which failed, scores)
  → format as FitnessContext
  → pass to Decomposer alongside DecompositionContext
  → LLM prompt includes: "Previous attempts: X worked (score 0.8), Y failed (build error)"
  → Decomposer avoids repeating failures, builds on successes
```

### Types

```rust
/// Historical fitness data from prior decomposition attempts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FitnessContext {
    /// Prior decomposition attempts for this signal.
    pub prior_attempts: Vec<DecompositionAttempt>,
    /// Aggregate statistics across all attempts.
    pub stats: FitnessStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecompositionAttempt {
    /// When this attempt was made.
    pub timestamp: DateTime<Utc>,
    /// Sub-goals that were attempted.
    pub sub_goals: Vec<SubGoalAttempt>,
    /// Overall result of the experiment.
    pub decision: String,  // "accept", "reject", "inconclusive"
    /// Best aggregate score achieved.
    pub best_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubGoalAttempt {
    /// Description of the sub-goal.
    pub description: String,
    /// What happened.
    pub outcome: String,  // "completed", "failed", "skipped", "budget_exhausted"
    /// Score contribution (if measurable).
    pub score: Option<f64>,
    /// Why it failed (if it did).
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FitnessStats {
    /// Total attempts for this signal.
    pub total_attempts: usize,
    /// How many resulted in Accept.
    pub accepts: usize,
    /// How many resulted in Reject.
    pub rejects: usize,
    /// Average best score across attempts.
    pub avg_best_score: f64,
    /// Most common failure reasons.
    pub common_failures: Vec<(String, usize)>,
    /// Sub-goal descriptions that have succeeded before.
    pub successful_approaches: Vec<String>,
}
```

### Extraction from Chain

New function in `fx-decompose` (or standalone module):

```rust
/// Extract fitness context from experiment chain entries.
pub fn extract_fitness_context(entries: &[ChainEntry]) -> FitnessContext {
    let prior_attempts = entries.iter().map(chain_entry_to_attempt).collect();
    let stats = compute_fitness_stats(&prior_attempts);
    FitnessContext { prior_attempts, stats }
}
```

This reuses `ChainEntry` from `fx-consensus` — but since `fx-decompose` can't depend on `fx-consensus` (circular dep), we have two options:

**Option A:** Put extraction logic in a new `fx-fitness` crate that depends on both.
**Option B:** Put extraction logic in `fx-consensus` and pass the result as data to `fx-decompose`.
**Option C:** Use the local `ChainEntry` stub in `fx-decompose/context.rs` and convert at the call site.

**Recommendation: Option B.** The extraction lives in `fx-consensus` (which already has `ChainEntry`), produces a `FitnessContext` struct defined in `fx-decompose`, and the orchestrator passes it through. The `FitnessContext` struct has no `fx-consensus` dependencies — it's pure data (strings, floats, timestamps).

### Integration with Decomposer

Update `DecompositionContext`:

```rust
pub struct DecompositionContext {
    // ... existing fields ...
    
    /// Historical fitness data from prior decomposition attempts.
    pub fitness: FitnessContext,
}
```

Update `LlmDecomposer` prompt to include fitness context:

```
Previous decomposition attempts for this signal:
- Attempt 1 (2026-03-15): REJECT, score 0.4
  - Sub-goal "profile hot path": completed (score 0.6)
  - Sub-goal "optimize serialization": failed (build error: missing import)
  - Sub-goal "add caching": skipped (budget exhausted)

Statistics: 3 attempts, 0 accepts, 3 rejects, avg score 0.35
Common failures: "build error" (2x), "test regression" (1x)
Successful approaches: "profile hot path", "add benchmarks"

Use this history to:
1. Avoid repeating approaches that failed (especially "optimize serialization" patterns)
2. Build on approaches that scored well ("profile hot path")
3. Allocate more sub-goal budget to areas that previously failed
```

### Feedback to Sub-Goal Complexity Hints

The fitness stats can also inform `ComplexityHint` assignment:
- Sub-goals similar to previously failed ones → `Complex` (more budget)
- Sub-goals similar to previously successful ones → `Trivial` or `Moderate`
- Novel sub-goals (no history) → `Moderate` (default)

This is a heuristic applied by the `LlmDecomposer` based on the fitness context, not a hard rule.

---

## Files to Change

1. **`engine/crates/fx-decompose/src/context.rs`:**
   - Add `FitnessContext`, `DecompositionAttempt`, `SubGoalAttempt`, `FitnessStats`
   - Add `fitness: FitnessContext` field to `DecompositionContext` (with Default)

2. **`engine/crates/fx-decompose/src/engine.rs`:**
   - Update `LlmDecomposer` prompt to include fitness context
   - Add `format_fitness_context()` helper for prompt building

3. **`engine/crates/fx-consensus/src/fitness.rs`** (new file):
   - `extract_fitness_context(entries: &[ChainEntry]) -> FitnessContext`
   - `chain_entry_to_attempt(entry: &ChainEntry) -> DecompositionAttempt`
   - `compute_fitness_stats(attempts: &[DecompositionAttempt]) -> FitnessStats`

4. **`engine/crates/fx-consensus/Cargo.toml`:**
   - Add `fx-decompose` as dependency (for `FitnessContext` type) — wait, circular dep again.

   **Resolution:** Define `FitnessContext` and friends in `fx-decompose` (which has no fx-consensus dep). The extraction function that reads `ChainEntry` lives in a thin adapter at the call site (orchestrator code in fx-cli or fx-api), not in either crate. Both crates stay independent.

---

## Test Plan

1. **FitnessContext from empty entries** — returns empty stats, no attempts
2. **FitnessContext from mixed entries** — Accept + Reject entries produce correct stats
3. **Common failures** — repeated failure reasons counted and ranked
4. **Successful approaches** — extracted from winning sub-goals
5. **format_fitness_context** — produces readable prompt text
6. **DecompositionContext with fitness** — fitness field defaults to empty, roundtrips through serde

---

## Implementation Notes

- `FitnessContext` is Serialize/Deserialize so it can be passed around, cached, or logged
- The extraction from `ChainEntry` is best done at the orchestration layer (fx-cli/fx-api), not inside either crate, to avoid circular dependencies
- Fitness stats are simple aggregations — no ML, no embeddings, just counting and averaging
- The LLM prompt formatting is the most impactful part — the model needs clear, structured history to avoid repeating mistakes
- `common_failures` should be capped (top 5) to avoid prompt bloat
- `successful_approaches` should also be capped (top 10)
