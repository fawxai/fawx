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
├── graph_dispatcher.rs    # Step 3: fan-out/fan-in dispatcher with scoring + mock test infra
├── graph_builder.rs       # Step 4: fluent builder API
├── reasoning_mode.rs      # Step 5: kernel integration types
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

Five PR-sized slices, executed sequentially. Each slice uses **test-driven development**: tests are written first, verified to fail, then implementation makes them pass.

| Step | Title | New Code | Difficulty |
|---|---|---|---|
| 1 | [Thought State & Operations](step1-thought-state-and-operations.md) | `thought.rs`, `operations.rs` | Easy |
| 2 | [Cyclic Graph Topology](step2-cyclic-graph-topology.md) | `graph_topology.rs` | Medium |
| 3 | [Graph Dispatcher](step3-graph-dispatcher.md) | `graph_dispatcher.rs` + mock infra | Medium |
| 4 | [Builder API](step4-builder-api.md) | `graph_builder.rs` | Easy |
| 5 | [Kernel Integration](step5-kernel-integration.md) | `reasoning_mode.rs` + kernel loop | Easy |

**Estimated total:** ~1,900 lines of new code (including tests), 2–3 days of implementation.

---

## TDD Workflow — Every Step

Each step follows this strict sequence:

1. **Write tests first.** Define the test module with all test functions. Each test asserts the behavior specified in the step's "Tests First" section.
2. **Red.** Confirm the tests fail to compile or fail at runtime. This proves the tests are meaningful.
3. **Implement.** Write the minimum code to make all tests pass.
4. **Green.** Run the full validation gate. All new and existing tests must pass.
5. **Refactor.** Clean up implementation if needed. Tests must still pass.

Tests are not an afterthought — they are the **first artifact** of each PR.

## Execution Rules

- One file in this folder = one PR-sized slice
- Run sequentially, not in parallel
- Tests are written and committed **before** implementation within each PR
- Each step spec defines its exact test signatures in a "Tests First" section
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

6. **Single budget mechanism.** Budget enforcement lives in the session-level `Arc<AtomicUsize>` counter passed into `GraphDispatcher`. There is no separate per-graph token budget — the session budget is the single source of truth. See Step 5 for details.

---

## Non-Goals (v1)

- **Interactive thought visualization in TUI** — future work, not blocking
- **Persisting thought graphs to disk** — chain entries capture the final result
- **Cross-session GoT resumption** — not needed for v1
- **Custom user-defined operations** — the built-in set covers the paper's full taxonomy
