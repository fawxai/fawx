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
    /// Typed index of this node in the graph's node list.
    pub id: GraphNodeId,
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
    pub from: GraphNodeId,
    pub to: GraphNodeId,
    /// If true, this is a back-edge (cycle). Subject to iteration limits.
    pub is_back_edge: bool,
}
```

### Edge Classification

Edge classification uses **explicit caller declaration** rather than index arithmetic:

- `add_edge()` creates a forward edge. It enforces `to.0 > from.0` as a sanity check, but the forward/back distinction is declared by the caller via which method they use, not inferred.
- `add_back_edge()` creates a back-edge. It enforces `to.0 <= from.0` as a sanity check.

**Invariant:** Nodes must be added in topological order of the forward-edge-only subgraph. The builder (Step 4) guarantees this by construction. Direct `GraphOfOperations` users must maintain this themselves.

This is explicitly documented on `GraphOfOperations::new()` and `add_node()`.

### `GraphOfOperations`

```rust
/// A directed graph of reasoning operations.
///
/// Unlike `ExecutionDag` (which is strictly level-based and acyclic), this
/// supports arbitrary edges including back-edges for refinement loops.
///
/// Back-edges are bounded by `max_iterations_per_cycle` to prevent runaway execution.
///
/// **Invariant:** Nodes must be added in topological order of the forward-edge subgraph.
/// That is, for any forward edge (A → B), A must have been added before B.
/// The `GraphBuilder` (Step 4) enforces this automatically. Direct construction
/// must maintain this manually.
pub struct GraphOfOperations {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    /// Entry point — index of the first node to execute.
    entry: GraphNodeId,
    /// Maximum times a back-edge can be traversed before forced termination.
    max_iterations_per_cycle: usize,
}
```

**Design note:** `max_total_tokens: Option<usize>` was removed. Budget enforcement lives in the session-level counter passed into `GraphDispatcher` (Step 5). A single budget mechanism avoids ambiguity about which limit takes precedence.

### Core Methods

```rust
impl GraphOfOperations {
    /// Create a new graph. Entry defaults to node 0.
    pub fn new(max_iterations_per_cycle: usize) -> Self;

    /// Add a node, return its `GraphNodeId`.
    pub fn add_node(&mut self, operation: GraphOperation, label: Option<String>) -> GraphNodeId;

    /// Add a forward edge (from → to). `from.0` must be < `to.0` for forward edges.
    pub fn add_edge(&mut self, from: GraphNodeId, to: GraphNodeId) -> Result<(), GraphTopologyError>;

    /// Add a back-edge (from → to where to.0 <= from.0). Used for refinement loops.
    pub fn add_back_edge(&mut self, from: GraphNodeId, to: GraphNodeId) -> Result<(), GraphTopologyError>;

    /// Set the entry node.
    pub fn set_entry(&mut self, id: GraphNodeId) -> Result<(), GraphTopologyError>;

    /// Get successors of a node (forward edges only).
    pub fn successors(&self, id: GraphNodeId) -> Vec<GraphNodeId>;

    /// Get all successors including back-edges.
    pub fn all_successors(&self, id: GraphNodeId) -> Vec<(GraphNodeId, bool)>;  // (target, is_back_edge)

    /// Get the node by ID.
    pub fn node(&self, id: GraphNodeId) -> Option<&GraphNode>;

    /// Number of nodes.
    pub fn len(&self) -> usize;

    /// Whether the graph is empty.
    pub fn is_empty(&self) -> bool;

    /// Maximum iterations per cycle.
    pub fn max_iterations(&self) -> usize;

    /// Validate the graph: entry exists, all edge indices valid, at least one node.
    pub fn validate(&self) -> Result<(), GraphTopologyError>;

    /// Return all terminal nodes (no outgoing forward edges).
    pub fn terminal_nodes(&self) -> Vec<GraphNodeId>;
}
```

### `GraphTopologyError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum GraphTopologyError {
    #[error("node {0:?} out of bounds (graph has {1} nodes)")]
    NodeOutOfBounds(GraphNodeId, usize),

    #[error("forward edge from {0:?} to {1:?} is invalid (target must be > source for forward edges)")]
    InvalidForwardEdge(GraphNodeId, GraphNodeId),

    #[error("back-edge from {0:?} to {1:?} is invalid (target must be <= source for back-edges)")]
    InvalidBackEdge(GraphNodeId, GraphNodeId),

    #[error("empty graph (no nodes)")]
    EmptyGraph,

    #[error("entry node {0:?} does not exist")]
    InvalidEntry(GraphNodeId),

    #[error("duplicate edge from {0:?} to {1:?}")]
    DuplicateEdge(GraphNodeId, GraphNodeId),
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
- All indices use `GraphNodeId` wrapper, not raw `usize`
- Edge classification is explicit (caller-declared via `add_edge` vs `add_back_edge`), with index-order sanity checks
- `validate()` catches: empty graph, out-of-bounds indices, invalid entry
- Forward edge addition rejects edges where `to.0 <= from.0`
- Back-edge addition rejects edges where `to.0 > from.0`
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
