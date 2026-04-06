# Graph of Thoughts (GoT) Reasoning Mode

## Super-Issue

**Goal:** Add a Graph of Thoughts reasoning mode to Fawx, enabling branching, scoring, pruning, merging, and iterative refinement of LLM reasoning paths — all orchestrated from the existing `fx-decompose` crate.

**Reference:** [spcl/graph-of-thoughts](https://github.com/spcl/graph-of-thoughts) (Besta et al., AAAI 2024)

---

## Motivation

Chain-of-thought (CoT) generates a single linear reasoning trace. Tree-of-thought (ToT) branches but only prunes. Graph of Thoughts (GoT) generalizes both by modeling reasoning as a **directed graph** where thoughts can be:

- **Generated** — branch one thought into N alternatives
- **Scored** — evaluate each thought against a fitness function
- **Pruned** — keep only the top-K scored thoughts
- **Merged** — combine insights from multiple thoughts into one
- **Refined** — loop back through score→refine cycles with bounded iteration

This is especially valuable for local models (27B–70B) where a single reasoning pass often fails on hard problems. GoT lets a harness compensate for model limitations by exploring and selecting from multiple reasoning paths.

### Why Fawx is uniquely positioned

`fx-decompose` already implements:
- DAG execution topology (`ExecutionDag`) with parallel/sequential levels
- Sub-goal dispatching (sequential, parallel, DAG-based)
- Result aggregation and scoring
- Budget controls and complexity weighting
- Subagent spawning for isolated execution

GoT maps cleanly onto this existing infrastructure. The port is an **extension**, not a rewrite.

---

## Architecture

### Where it lives

All GoT logic lives inside `engine/crates/fx-decompose/`. No new crate is needed.

New files:
```
engine/crates/fx-decompose/src/
├── operations.rs          # Step 1: typed graph operations
├── thought.rs             # Step 1: thought state model
├── graph_topology.rs      # Step 2: cyclic graph topology (replaces/extends dag.rs for GoT)
├── graph_dispatcher.rs    # Step 3: fan-out/fan-in dispatcher with scoring
├── graph_builder.rs       # Step 4: fluent builder API
└── (existing files unchanged)
```

Integration point:
```
engine/crates/fx-kernel/src/loop_engine/decomposition.rs  # Step 5: reasoning mode branch
```

### How it maps to spcl/graph-of-thoughts

| GoT (Python) | Fawx (Rust) | Status |
|---|---|---|
| `GraphOfOperations` | `GraphOfOperations` (new) | Step 2 |
| `Generate` | `GraphOperation::Generate` (new) | Step 1 |
| `Score` | `GraphOperation::Score` (new) | Step 1 |
| `KeepBestN` | `GraphOperation::KeepBest` (new) | Step 1 |
| `Aggregate` | `GraphOperation::Merge` (new) | Step 1 |
| `GroundTruth` | `GraphOperation::Validate` (new) | Step 1 |
| `Controller` | `GraphDispatcher` (new) | Step 3 |
| `AbstractLanguageModel` | `fx-llm::ModelRouter` (existing) | ✅ Done |
| `Prompter` | System prompt construction (existing) | ✅ Done |
| `Parser` | Response parsing (existing) | ✅ Done |
| State dict | `ThoughtState` (new) | Step 1 |

---

## Execution Plan

Six PR-sized slices, executed sequentially:

| Step | Title | New Code | Difficulty |
|---|---|---|---|
| 1 | [Thought State & Operations](step1-thought-state-and-operations.md) | `thought.rs`, `operations.rs` | Easy |
| 2 | [Cyclic Graph Topology](step2-cyclic-graph-topology.md) | `graph_topology.rs` | Medium |
| 3 | [Graph Dispatcher](step3-graph-dispatcher.md) | `graph_dispatcher.rs` | Medium |
| 4 | [Builder API](step4-builder-api.md) | `graph_builder.rs` | Easy |
| 5 | [Kernel Integration](step5-kernel-integration.md) | kernel loop changes | Easy |
| 6 | [Tests & Validation](step6-tests-and-validation.md) | integration tests | Routine |

**Estimated total:** ~1,100 lines of new code, 2–3 days of implementation.

---

## Execution Rules

- One file in this folder = one PR-sized slice
- Run sequentially, not in parallel
- Each step spec defines its own acceptance criteria and validation gate
- Fresh worktree per slice, branch from current `origin/dev`
- All existing tests must continue to pass — GoT is additive, no existing behavior changes

## Global Validation Gate

Every slice must pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

---

## Design Constraints

1. **No changes to existing dispatch paths.** `SequentialDispatcher`, `ParallelDispatcher`, and `DagDispatcher` remain untouched. GoT uses its own `GraphDispatcher`.

2. **Budget-bounded cycles.** Refinement loops have a hard `max_iterations` cap. No unbounded execution.

3. **Scoring is pluggable.** The `ScoringStrategy` trait allows LLM-based scoring, heuristic scoring, or ground-truth comparison — same extensibility as GoT's Python `scoring_function`.

4. **Kernel safety unchanged.** GoT runs within the existing budget and proposal gate. No new kernel permissions needed. GoT mode is just a different strategy for constructing sub-goals — the execution still flows through the same gated tool executor.

5. **Local-model friendly.** GoT's value proposition is strongest with smaller local models. The implementation must be efficient enough to run with 3–6 tok/s endpoints (no unnecessary round-trips, minimal prompt overhead).

---

## Non-Goals (v1)

- **Interactive thought visualization in TUI** — future work, not blocking
- **Persisting thought graphs to disk** — chain entries capture the final result
- **Cross-session GoT resumption** — not needed for v1
- **Custom user-defined operations** — the built-in set covers the paper's full taxonomy
