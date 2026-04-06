# Step 5: Kernel Integration

## Goal

Wire GoT into the Fawx kernel loop so the agent can use Graph of Thoughts reasoning when appropriate. Add a `ReasoningMode` enum, selection logic, and the bridge between the kernel's existing decomposition path and the new `GraphDispatcher`.

## Why This Slice Exists

Steps 1–4 built the GoT engine inside `fx-decompose`. This step connects it to the live system so it's actually usable. Without this, GoT is library code with no entry point.

---

## Expected Targets

- `engine/crates/fx-decompose/src/reasoning_mode.rs` (new)
- `engine/crates/fx-kernel/src/loop_engine/decomposition.rs` (modify — add GoT branch)
- `engine/crates/fx-tools/src/decompose_tool.rs` or equivalent (modify — expose GoT mode in `decompose` tool)
- `engine/crates/fx-decompose/src/lib.rs` (add `pub mod reasoning_mode;`)

---

## Dependencies

- `GraphOfOperations`, `GraphDispatcher`, `GraphBuilder` from Steps 1–4
- `fx-kernel` loop engine — the existing decomposition entry point
- `fx-llm` — `ModelRouter` for constructing LLM-backed generator/scorer/merger

---

## `ReasoningMode`

```rust
/// How the agent reasons through a complex task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReasoningMode {
    /// Current default behavior: decompose into sub-goals, dispatch sequentially or in parallel.
    Standard,

    /// Graph of Thoughts: branch, score, prune, merge, refine reasoning paths.
    GraphOfThoughts {
        /// The graph topology to execute.
        graph: GraphOfOperationsSpec,
    },
}
```

### `GraphOfOperationsSpec`

A serializable specification that can be passed through the tool interface:

```rust
/// Serializable GoT graph specification.
/// Used in tool calls and config — converted to a live `GraphOfOperations` at execution time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphOfOperationsSpec {
    /// Use a named preset.
    Preset {
        name: GoTPreset,
        /// Override default branch count.
        branches: Option<usize>,
        /// Override default keep count.
        keep: Option<usize>,
        /// Override max refinement iterations.
        refine_iterations: Option<usize>,
        /// Override target score threshold.
        target_score: Option<f64>,
        /// Scoring/evaluation criteria.
        criteria: String,
    },
    /// Custom graph (advanced usage — raw operation list).
    Custom {
        operations: Vec<GraphOperation>,
        /// DAG-like spec for operation ordering, or empty for linear chain.
        edges: Vec<(usize, usize, bool)>,  // (from, to, is_back_edge)
        max_iterations_per_cycle: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GoTPreset {
    ChainOfThought,
    TreeOfThought,
    GraphOfThought,
    Consensus,
}
```

---

## Decompose Tool Changes

Extend the existing `decompose` tool to accept a `reasoning_mode` parameter:

```json
{
  "name": "decompose",
  "parameters": {
    "sub_goals": [...],
    "strategy": "Sequential | Parallel | Custom",
    "reasoning_mode": {
      "type": "string",
      "enum": ["standard", "got_tree", "got_graph", "got_consensus"],
      "description": "Reasoning strategy. Default: standard. GoT modes use Graph of Thoughts."
    },
    "got_branches": {
      "type": "integer",
      "description": "Number of thought branches for GoT modes. Default: 3"
    },
    "got_criteria": {
      "type": "string",
      "description": "Evaluation criteria for GoT scoring. Required for GoT modes."
    }
  }
}
```

When `reasoning_mode` is a GoT variant, the tool:
1. Ignores `sub_goals` and `strategy` (GoT constructs its own graph)
2. Builds a `GraphOfOperations` from the preset + parameters
3. Constructs a `GraphDispatcher` using the session's `ModelRouter`
4. Executes the graph with the user's original task as the initial thought
5. Returns the best thought from `GraphExecutionResult`

When `reasoning_mode` is `standard` (or absent), behavior is identical to today.

---

## Kernel Loop Integration

In `decomposition.rs`, the existing flow is:

```
detect decomposition needed → parse plan → dispatch sub-goals → aggregate → return
```

Add a branch:

```
detect decomposition needed →
  if reasoning_mode == GoT:
    build graph from spec
    construct GraphDispatcher (generator, scorer, merger from ModelRouter)
    execute graph
    return best thought as the synthesized response
  else:
    existing sub-goal path (unchanged)
```

### Constructing LLM-backed traits

```rust
fn build_got_dispatcher(router: &ModelRouter) -> GraphDispatcher {
    let generator = Arc::new(LlmThoughtGenerator::new(router.clone()));
    let scorer = Arc::new(LlmThoughtScorer::new(router.clone()));
    let merger = Arc::new(LlmThoughtMerger::new(router.clone()));
    GraphDispatcher::new(generator, scorer, merger)
}
```

The `LlmThoughtGenerator`, `LlmThoughtScorer`, and `LlmThoughtMerger` structs wrap `ModelRouter` and implement the traits from Step 3. They live in `graph_dispatcher.rs` alongside the trait definitions.

---

## Budget Integration

GoT executes multiple LLM calls. Each call must count against the session's existing tool budget:

- Each `Generate` call = N LLM calls (N = num_branches × active thoughts)
- Each `Score` with LlmRating = 1 LLM call per active thought
- Each `Merge` with LlmSynthesis = 1 LLM call
- Each `Validate` with LlmJudge = 1 LLM call per active thought

The `GraphDispatcher` receives a budget counter (e.g., `Arc<AtomicUsize>`) and increments it per LLM call. If the session budget is exhausted mid-graph, execution terminates early and returns the best thought so far.

---

## What Does NOT Change

- The existing `SequentialDispatcher`, `ParallelDispatcher`, `DagDispatcher` — untouched
- The kernel loop for standard decomposition — untouched
- Budget enforcement mechanics — GoT uses the same budget, just counts more calls
- Proposal gate — GoT doesn't execute tools directly, it produces reasoning text. If the reasoning text leads to tool calls, those go through the normal gated path.

---

## Acceptance Criteria

- `ReasoningMode` enum is defined and serializable
- `GraphOfOperationsSpec` converts to a live `GraphOfOperations` via the builder presets
- The `decompose` tool accepts `reasoning_mode` and GoT-specific parameters
- When `reasoning_mode` is a GoT variant, the graph executes and returns a result
- When `reasoning_mode` is absent or `standard`, existing behavior is unchanged
- Budget counting works — each LLM call within GoT increments the session counter
- Early termination on budget exhaustion returns partial results gracefully
- Integration test: construct a GoT graph via the tool interface, execute with mock LLM, verify result
- All existing decomposition tests pass unchanged

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

## Estimated Size

~200 lines of new code (reasoning_mode.rs + tool/kernel modifications), plus ~50 lines of LLM trait implementations in graph_dispatcher.rs.
