# Step 1: Thought State & Operations

## Goal

Define the core data types: `ThoughtState` (the mutable reasoning state flowing through the graph) and `GraphOperation` (the typed operations that transform thoughts).

## Why This Slice Exists

Every subsequent step depends on these types. They are pure data + trait definitions with no execution logic, making them safe to land first.

---

## Expected Targets

- `engine/crates/fx-decompose/src/thought.rs` (new)
- `engine/crates/fx-decompose/src/operations.rs` (new)
- `engine/crates/fx-decompose/src/lib.rs` (add `pub mod thought; pub mod operations;`)

---

## `thought.rs` — Thought State Model

### `ThoughtId`

```rust
/// Unique identifier for a thought node in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThoughtId(pub u64);
```

Use a monotonic counter within a single graph execution, not UUIDs. Cheap, debuggable, no allocations.

### `GraphNodeId`

```rust
/// Typed wrapper for a node index in the operation graph.
///
/// Prevents raw `usize` indices from being confused with other integer
/// quantities (thought IDs, branch counts, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GraphNodeId(pub usize);
```

Used by `ThoughtState::origin_operation` and throughout `graph_topology.rs` (Step 2) instead of bare `usize`.

### `ThoughtMetadata`

```rust
/// Typed metadata carried by a thought.
///
/// This replaces an untyped `serde_json::Value` to satisfy ENGINEERING.md §2:
/// "No `Any`, `Object`, or stringly-typed APIs where a concrete type or enum exists."
///
/// The variants cover all known use cases for v1. If future operations need
/// additional metadata, add a variant here rather than falling back to a
/// freeform map.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum ThoughtMetadata {
    /// No metadata (default for most operations).
    #[default]
    Empty,
    /// Key-value pairs for domain-specific state (e.g., partial solutions).
    /// Keys are constrained to `String` but values are typed.
    Fields(HashMap<String, ThoughtMetadataValue>),
}

/// A single metadata value. Covers the types GoT operations actually produce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThoughtMetadataValue {
    Text(String),
    Number(f64),
    Bool(bool),
}
```

### `ThoughtState`

```rust
/// A single thought in the graph — carries content, score, and lineage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtState {
    /// Unique ID within this graph execution.
    pub id: ThoughtId,
    /// The textual content of this thought (LLM response, merged result, etc).
    pub content: String,
    /// Score assigned by a Score operation. None until scored.
    pub score: Option<f64>,
    /// Typed metadata for domain-specific state.
    pub metadata: ThoughtMetadata,
    /// IDs of parent thoughts. Empty for root thoughts.
    pub parent_ids: Vec<ThoughtId>,
    /// Which graph operation produced this thought.
    pub origin_operation: Option<GraphNodeId>,
}
```

**Design notes:**
- `metadata` uses `ThoughtMetadata` instead of `serde_json::Value` — avoids stringly-typed `Any` escape hatch per ENGINEERING.md §2.
- `origin_operation` uses `GraphNodeId` instead of raw `usize` — prevents index confusion and survives refactoring.
- `created_at: Option<std::time::Instant>` was removed. `Instant` is not serializable (`#[serde(skip)]` would produce a ghost field always `None` after deserialization). `ThoughtId` already provides monotonic creation order since the allocator is sequential.

### `ThoughtIdAllocator`

```rust
/// Allocator for thought IDs within a single graph execution.
///
/// Uses a plain `u64` counter — not `AtomicU64` — because `ThoughtPool`
/// takes `&mut self` for all mutation, so this is always single-owner.
#[derive(Debug, Default)]
pub struct ThoughtIdAllocator {
    next: u64,
}

impl ThoughtIdAllocator {
    pub fn next(&mut self) -> ThoughtId {
        let id = ThoughtId(self.next);
        self.next += 1;
        id
    }
}
```

### `ThoughtPool`

Container for all thoughts in a graph execution. Simple `HashMap<ThoughtId, ThoughtState>` with helper methods.

```rust
pub struct ThoughtPool {
    thoughts: HashMap<ThoughtId, ThoughtState>,
    allocator: ThoughtIdAllocator,
}

impl ThoughtPool {
    pub fn new() -> Self;
    pub fn insert(&mut self, state: ThoughtState) -> ThoughtId;
    pub fn create(
        &mut self,
        content: String,
        parents: Vec<ThoughtId>,
        metadata: ThoughtMetadata,
    ) -> ThoughtId;
    pub fn get(&self, id: ThoughtId) -> Option<&ThoughtState>;
    pub fn get_mut(&mut self, id: ThoughtId) -> Option<&mut ThoughtState>;
    pub fn remove(&mut self, id: ThoughtId) -> Option<ThoughtState>;
    pub fn scored(&self) -> Vec<&ThoughtState>;  // all thoughts with score.is_some()
    pub fn top_n(&self, n: usize) -> Vec<&ThoughtState>;  // top-N by score descending
    pub fn active_ids(&self) -> Vec<ThoughtId>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

---

## `operations.rs` — Graph Operation Types

### `GraphOperation`

```rust
/// A typed operation in the Graph of Thoughts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphOperation {
    /// Generate N thought branches from each active thought.
    /// Maps to GoT's `Generate` operation.
    Generate {
        /// Number of branches to create per input thought.
        num_branches: usize,
        /// Optional prompt template override for generation.
        prompt_override: Option<String>,
    },

    /// Score each active thought using a scoring strategy.
    /// Maps to GoT's `Score` operation.
    Score {
        strategy: ScoringStrategy,
    },

    /// Keep only the top-N scored thoughts, prune the rest.
    /// Maps to GoT's `KeepBestN` operation.
    KeepBest {
        n: usize,
    },

    /// Merge all active thoughts into a single combined thought.
    /// Maps to GoT's `Aggregate` operation.
    Merge {
        strategy: MergeStrategy,
    },

    /// Refine active thoughts through iterative score-then-improve cycles.
    /// Extension beyond base GoT — combines Score + conditional re-generation.
    Refine {
        max_iterations: usize,
        /// Minimum score to accept (stop refining if reached).
        target_score: f64,
        scoring: ScoringStrategy,
    },

    /// Validate active thoughts against a ground truth or acceptance function.
    /// Maps to GoT's `GroundTruth` operation.
    Validate {
        strategy: ValidationStrategy,
    },
}
```

### `ScoringStrategy`

```rust
/// How to score a thought.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScoringStrategy {
    /// Ask the LLM to rate the thought on a 0.0-1.0 scale.
    LlmRating {
        /// Criteria description for the LLM to evaluate against.
        criteria: String,
    },
    /// Use a regex or substring match to compute a heuristic score.
    Heuristic {
        /// Regex pattern. Score = 1.0 if matches, 0.0 otherwise.
        pattern: String,
    },
    /// Use an external scoring function provided at runtime.
    /// (For integration with experiment fitness criteria.)
    External,
}
```

### `MergeStrategy`

```rust
/// How to merge multiple thoughts into one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeStrategy {
    /// Ask the LLM to synthesize all active thoughts into one.
    LlmSynthesis {
        /// Optional instruction for the LLM on how to merge.
        instruction: Option<String>,
    },
    /// Concatenate all thought contents with a separator.
    Concatenate {
        separator: String,
    },
}
```

### `ValidationStrategy`

```rust
/// How to validate a thought against ground truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationStrategy {
    /// Exact string match against expected output.
    ExactMatch { expected: String },
    /// Substring containment check.
    Contains { expected: String },
    /// LLM-based validation (ask the model if the thought is correct).
    LlmJudge { criteria: String },
    /// Always passes (no-op validation).
    AlwaysPass,
}
```

---

## Acceptance Criteria

- `thought.rs` compiles with all types, derives, and helper methods
- `operations.rs` compiles with all operation enums and strategy types
- `ThoughtPool` has unit tests for: insert, create, get, remove, scored(), top_n()
- `ThoughtMetadata` has unit tests for: default (Empty), Fields construction, serde round-trip
- `GraphNodeId` is used instead of raw `usize` for operation references
- All types implement `Debug, Clone, Serialize, Deserialize`
- `lib.rs` exports both modules
- All existing tests pass unchanged

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
cargo test -p fx-decompose
```

## Estimated Size

~350 lines including tests.
