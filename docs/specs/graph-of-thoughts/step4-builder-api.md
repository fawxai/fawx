# Step 4: Builder API

## Goal

Implement a fluent builder for constructing `GraphOfOperations` without manually managing node indices and edges. This is the user-facing API that makes GoT graphs easy to compose.

## Why This Slice Exists

Manually calling `add_node`, `add_edge`, `add_back_edge` is tedious and error-prone for common patterns. The builder encodes the most useful GoT topologies as composable method chains. This is the Rust equivalent of GoT's Python `GraphOfOperations().append_operation()` pattern, but more expressive.

---

## Expected Targets

- `engine/crates/fx-decompose/src/graph_builder.rs` (new)
- `engine/crates/fx-decompose/src/lib.rs` (add `pub mod graph_builder;` and re-exports)

---

## Dependencies

- `GraphOfOperations`, `GraphNode`, `GraphEdge`, `GraphNodeId` from `graph_topology.rs` (Step 2)
- `GraphOperation`, `ScoringStrategy`, `MergeStrategy`, `ValidationStrategy` from `operations.rs` (Step 1)

---

## `GraphBuilder`

```rust
/// Fluent builder for constructing a `GraphOfOperations`.
///
/// Each method appends an operation node and automatically wires edges
/// from the previous node. Back-edges for refinement loops are handled
/// by the `refine()` method internally.
pub struct GraphBuilder {
    graph: GraphOfOperations,
    /// Index of the last appended node (for auto-wiring sequential edges).
    last_node: Option<GraphNodeId>,
}
```

### Core Methods

```rust
impl GraphBuilder {
    /// Start a new graph with the given max iterations per refinement cycle.
    pub fn new(max_iterations_per_cycle: usize) -> Self;

    /// Append a Generate operation. Branches each active thought into N alternatives.
    pub fn generate(self, num_branches: usize) -> Self;

    /// Append a Generate operation with a custom prompt override.
    pub fn generate_with_prompt(self, num_branches: usize, prompt: impl Into<String>) -> Self;

    /// Append a Score operation with LLM-based rating.
    pub fn score(self, criteria: impl Into<String>) -> Self;

    /// Append a Score operation with a heuristic regex pattern.
    pub fn score_heuristic(self, pattern: impl Into<String>) -> Self;

    /// Append a KeepBest operation. Prune to top-N thoughts.
    pub fn keep_best(self, n: usize) -> Self;

    /// Append a Merge operation using LLM synthesis.
    pub fn merge(self) -> Self;

    /// Append a Merge operation with custom LLM instructions.
    pub fn merge_with_instruction(self, instruction: impl Into<String>) -> Self;

    /// Append a Merge operation using simple concatenation.
    pub fn concat(self, separator: impl Into<String>) -> Self;

    /// Append a Refine operation: iterative score→improve loop.
    ///
    /// This internally creates multiple nodes (Score + Generate) with a back-edge
    /// from Generate → Score. After `refine()` returns, `last_node` points to the
    /// **last internal node** (the Generate node), so subsequent chained methods
    /// wire correctly from the end of the refinement cycle.
    pub fn refine(self, max_iterations: usize, target_score: f64, criteria: impl Into<String>) -> Self;

    /// Append a Validate operation with exact match.
    pub fn validate_exact(self, expected: impl Into<String>) -> Self;

    /// Append a Validate operation with substring containment.
    pub fn validate_contains(self, expected: impl Into<String>) -> Self;

    /// Append a Validate operation with LLM judge.
    pub fn validate_llm(self, criteria: impl Into<String>) -> Self;

    /// Append a raw GraphOperation node (escape hatch for custom ops).
    pub fn operation(self, op: GraphOperation) -> Self;

    /// Build and validate the final graph.
    pub fn build(self) -> Result<GraphOfOperations, GraphTopologyError>;
}
```

---

## Preset Graphs

Common GoT patterns as static constructors:

```rust
impl GraphBuilder {
    /// Chain-of-Thought equivalent: Generate(1) → Score → done.
    /// Single linear reasoning path.
    pub fn chain_of_thought(criteria: impl Into<String>) -> GraphOfOperations;

    /// Tree-of-Thought equivalent: Generate(N) → Score → KeepBest(1).
    /// Branch, evaluate, pick the best.
    pub fn tree_of_thought(branches: usize, criteria: impl Into<String>) -> GraphOfOperations;

    /// Full GoT: Generate(N) → Score → KeepBest(K) → Merge → Refine → Validate.
    /// Branch, evaluate, prune, merge insights, refine, validate.
    pub fn graph_of_thought(
        branches: usize,
        keep: usize,
        refine_iterations: usize,
        target_score: f64,
        criteria: impl Into<String>,
    ) -> GraphOfOperations;

    /// Simple consensus: Generate(N) → Score → Merge.
    /// Generate multiple perspectives, score them, merge the best insights.
    pub fn consensus(branches: usize, criteria: impl Into<String>) -> GraphOfOperations;
}
```

---

## Usage Examples

### Manual construction

```rust
let graph = GraphBuilder::new(3)
    .generate(4)              // Branch into 4 alternatives
    .score("correctness and clarity")
    .keep_best(2)             // Keep top 2
    .merge()                  // Merge insights
    .refine(2, 0.9, "final answer quality")
    .build()?;
```

### Preset

```rust
// Full GoT with 4 branches, keep 2, refine up to 3 times, target 0.85
let graph = GraphBuilder::graph_of_thought(4, 2, 3, 0.85, "mathematical correctness");
```

### Chain-of-Thought (degenerate case)

```rust
let graph = GraphBuilder::chain_of_thought("reasoning quality");
```

---

## Auto-Wiring Rules

1. The first node added becomes the entry node (index 0)
2. Each subsequent node gets a forward edge from the previous node
3. `refine()` creates two internal nodes (Score + Generate) with a back-edge from Generate → Score. After `refine()`, `last_node` points to the Generate node (the last one created), so the next chained method wires from the correct exit point of the cycle.
4. `build()` calls `graph.validate()` before returning

---

## Acceptance Criteria

- Builder produces valid `GraphOfOperations` for all common patterns
- Auto-wiring creates correct edge sequences (verified by inspecting `graph.successors()`)
- `refine()` correctly creates a back-edge with appropriate iteration limit
- `refine()` sets `last_node` to the last internal node, not the first
- All four presets (`chain_of_thought`, `tree_of_thought`, `graph_of_thought`, `consensus`) build without error
- Presets produce topologically valid graphs (pass `validate()`)
- `build()` returns `Err` if the graph is empty or invalid
- Builder methods are chainable (move semantics via `self`)
- Unit tests for each preset and for manual construction
- Unit test verifying that `refine()` followed by another operation wires correctly
- All existing tests pass unchanged

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
cargo test -p fx-decompose
```

## Estimated Size

~250 lines including presets and tests.
