# Step 2: Cyclic Graph Topology

## Goal

Implement `GraphOfOperations` — a directed graph of operations that supports cycles (for refinement loops), with bounded iteration to prevent runaway execution.

## Why This Slice Exists

The existing `ExecutionDag` in `dag.rs` is strictly acyclic (levels of parallel groups). GoT requires back-edges for refinement loops (e.g., score → refine → score again). This is a new topology type, not a modification of `ExecutionDag`.

---

## Expected Targets

- `engine/crates/fx-decompose/src/graph_topology.rs` (new)
- `engine/crates/fx-decompose/src/lib.rs` (add `pub mod graph_topology;`)

---

## `graph_topology.rs` — Graph of Operations

### `GraphNode`

```rust
/// A node in the operation graph.
#[derive(Debug, Clone)]
pub struct GraphNode {
    /// Index of this node in the graph's node list.
    pub index: usize,
    /// The operation this node performs.
    pub operation: GraphOperation,
    /// Human-readable label for debugging/logging.
    pub label: Option<String>,
}
```

### `GraphEdge`

```rust
/// A directed edge in the operation graph.
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    /// If true, this is a back-edge (cycle). Subject to iteration limits.
    pub is_back_edge: bool,
}
```

### `GraphOfOperations`

```rust
/// A directed graph of reasoning operations.
///
/// Unlike `ExecutionDag` (which is strictly level-based and acyclic), this
/// supports arbitrary edges including back-edges for refinement loops.
///
/// Back-edges are bounded by `max_iterations_per_cycle` to prevent runaway execution.
pub struct GraphOfOperations {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    /// Entry point — index of the first node to execute.
    entry: usize,
    /// Maximum times a back-edge can be traversed before forced termination.
    max_iterations_per_cycle: usize,
    /// Total token budget for the entire graph execution.
    /// Acts as a secondary safety bound alongside iteration limits.
    max_total_tokens: Option<usize>,
}
```

### Core Methods

```rust
impl GraphOfOperations {
    /// Create a new graph. Entry defaults to node 0.
    pub fn new(max_iterations_per_cycle: usize) -> Self;

    /// Add a node, return its index.
    pub fn add_node(&mut self, operation: GraphOperation, label: Option<String>) -> usize;

    /// Add a forward edge (from → to). `from` must be < `to` for forward edges.
    pub fn add_edge(&mut self, from: usize, to: usize) -> Result<(), GraphTopologyError>;

    /// Add a back-edge (from → to where to <= from). Used for refinement loops.
    pub fn add_back_edge(&mut self, from: usize, to: usize) -> Result<(), GraphTopologyError>;

    /// Set the entry node index.
    pub fn set_entry(&mut self, index: usize) -> Result<(), GraphTopologyError>;

    /// Get successors of a node (forward edges only).
    pub fn successors(&self, index: usize) -> Vec<usize>;

    /// Get all successors including back-edges.
    pub fn all_successors(&self, index: usize) -> Vec<(usize, bool)>;  // (target, is_back_edge)

    /// Get the node at an index.
    pub fn node(&self, index: usize) -> Option<&GraphNode>;

    /// Number of nodes.
    pub fn len(&self) -> usize;

    /// Whether the graph is empty.
    pub fn is_empty(&self) -> bool;

    /// Maximum iterations per cycle.
    pub fn max_iterations(&self) -> usize;

    /// Validate the graph: entry exists, all edge indices valid, at least one node.
    pub fn validate(&self) -> Result<(), GraphTopologyError>;

    /// Detect whether an edge is a back-edge (target index <= source index).
    fn classify_edge(from: usize, to: usize) -> bool;

    /// Return all terminal nodes (no outgoing forward edges).
    pub fn terminal_nodes(&self) -> Vec<usize>;
}
```

### `GraphTopologyError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum GraphTopologyError {
    #[error("node index {0} out of bounds (graph has {1} nodes)")]
    NodeOutOfBounds(usize, usize),

    #[error("forward edge from {0} to {1} is invalid (target must be > source for forward edges)")]
    InvalidForwardEdge(usize, usize),

    #[error("empty graph (no nodes)")]
    EmptyGraph,

    #[error("entry node {0} does not exist")]
    InvalidEntry(usize),

    #[error("duplicate edge from {0} to {1}")]
    DuplicateEdge(usize, usize),
}
```

---

## Execution Semantics

The `GraphOfOperations` defines **topology only**. Execution is handled by `GraphDispatcher` (Step 3). The topology's contract:

1. Start at `entry` node
2. Execute the node's operation on the current `ThoughtPool`
3. Follow outgoing edges:
   - Forward edges: always follow
   - Back-edges: follow only if iteration count < `max_iterations_per_cycle`
4. If multiple outgoing edges exist, the **first valid forward edge** is followed (sequential execution model — parallelism is within operations like `Generate`, not between nodes)
5. Terminate when a terminal node completes or all back-edges have exhausted their iteration budget

---

## Acceptance Criteria

- `GraphOfOperations` supports adding nodes and both forward/back edges
- Back-edge classification is automatic based on index comparison
- `validate()` catches: empty graph, out-of-bounds indices, invalid entry
- Forward edge addition rejects edges where `to <= from`
- Back-edge addition rejects edges where `to > from`
- `terminal_nodes()` correctly identifies nodes with no forward outgoing edges
- Unit tests for all error paths and basic graph construction
- All existing `dag.rs` tests pass unchanged

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
cargo test -p fx-decompose
```

## Estimated Size

~250 lines including tests.
