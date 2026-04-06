# Step 3: Graph Dispatcher

## Goal

Implement `GraphDispatcher` — the execution engine that walks a `GraphOfOperations`, executes each operation against a `ThoughtPool`, handles fan-out/fan-in, scoring, pruning, and bounded refinement loops.

## Why This Slice Exists

This is the core runtime. Steps 1–2 defined the data types and topology; this slice makes them execute. It is the Rust equivalent of GoT's `Controller` class.

---

## Expected Targets

- `engine/crates/fx-decompose/src/graph_dispatcher.rs` (new)
- `engine/crates/fx-decompose/src/lib.rs` (add `pub mod graph_dispatcher;` and re-exports)

---

## Dependencies

- `ThoughtState`, `ThoughtPool`, `ThoughtIdAllocator`, `GraphNodeId` from `thought.rs` (Step 1)
- `GraphOperation`, `ScoringStrategy`, `MergeStrategy`, `ValidationStrategy` from `operations.rs` (Step 1)
- `GraphOfOperations`, `GraphNode` from `graph_topology.rs` (Step 2)
- `fx-llm` — `ModelRouter` for LLM calls (Generate, LlmRating, LlmSynthesis, LlmJudge)

---

## Traits

### `ThoughtScorer`

```rust
/// Pluggable scoring for thoughts. Used by Score and Refine operations.
#[async_trait::async_trait]
pub trait ThoughtScorer: Send + Sync {
    async fn score(&self, thought: &ThoughtState, criteria: &str) -> Result<f64, DecomposeError>;
}
```

Provide two implementations:

1. **`LlmThoughtScorer`** — sends the thought content + criteria to the LLM, parses a 0.0–1.0 score from the response. Prompt template:

```
Rate the following reasoning on a scale of 0.0 to 1.0 based on this criteria:
{criteria}

Reasoning:
{thought.content}

Respond with only a number between 0.0 and 1.0.
```

**Implementation note:** Local models (27B–70B) frequently wrap the numeric response in prose (e.g., "I'd rate this 0.7 because..."). The implementation must use regex extraction (`r"(0\.\d+|1\.0|0|1)"`) to pull the first valid float from the response, not naive `parse::<f64>()`. Fall back to 0.5 if no number is found, with a `tracing::warn!` noting the unparseable response.

2. **`HeuristicThoughtScorer`** — regex match: 1.0 if pattern matches content, 0.0 otherwise.

### `ThoughtGenerator`

```rust
/// Generates new thoughts from a parent thought.
#[async_trait::async_trait]
pub trait ThoughtGenerator: Send + Sync {
    async fn generate(
        &self,
        parent: &ThoughtState,
        num_branches: usize,
        prompt_override: Option<&str>,
    ) -> Result<Vec<String>, DecomposeError>;
}
```

**`LlmThoughtGenerator`** — sends the parent content to the LLM N times (or requests N alternatives in a single call). Each response becomes a new thought.

### `ThoughtMerger`

```rust
/// Merges multiple thoughts into one.
#[async_trait::async_trait]
pub trait ThoughtMerger: Send + Sync {
    async fn merge(
        &self,
        thoughts: &[&ThoughtState],
        instruction: Option<&str>,
    ) -> Result<String, DecomposeError>;
}
```

**`LlmThoughtMerger`** — sends all thought contents to the LLM with merge instructions.
**`ConcatMerger`** — joins thought contents with a separator string. No LLM call.

---

## Structured Tracing

Each operation execution opens a `tracing::info_span!` with the node ID, operation type, and cycle count. This is critical for debugging multi-cycle GoT runs with branching and refinement, which would otherwise be opaque in logs.

```rust
let span = tracing::info_span!(
    "got_operation",
    node = %node.id.0,
    op = %operation_name,
    cycle = cycle_count,
);
let _guard = span.enter();
```

---

## `GraphDispatcher`

```rust
pub struct GraphDispatcher {
    generator: Arc<dyn ThoughtGenerator>,
    scorer: Arc<dyn ThoughtScorer>,
    merger: Arc<dyn ThoughtMerger>,
}
```

### Core Method

```rust
impl GraphDispatcher {
    pub fn new(
        generator: Arc<dyn ThoughtGenerator>,
        scorer: Arc<dyn ThoughtScorer>,
        merger: Arc<dyn ThoughtMerger>,
    ) -> Self;

    /// Execute the full graph, starting from the entry node with the given initial thought.
    pub async fn execute(
        &self,
        graph: &GraphOfOperations,
        initial_content: String,
        initial_metadata: ThoughtMetadata,
        progress: Option<&DecompositionProgressCallback>,
    ) -> Result<GraphExecutionResult, DecomposeError>;
}
```

### `GraphExecutionResult`

```rust
#[derive(Debug, Clone)]
pub struct GraphExecutionResult {
    /// The final thought pool after all operations complete.
    pub thoughts: Vec<ThoughtState>,
    /// The best thought (highest scored, or last remaining after pruning).
    pub best: Option<ThoughtState>,
    /// Number of LLM calls made during execution.
    pub llm_calls: usize,
    /// Total operations executed (including repeated cycles).
    pub operations_executed: usize,
    /// Whether any refinement loop hit its iteration cap.
    pub refinement_capped: bool,
}
```

---

## Operation Execution Logic

### `Generate`

```
for each active thought in pool:
    branches = generator.generate(thought, num_branches, prompt_override).await?
    for branch_content in branches:
        pool.create(branch_content, vec![thought.id], metadata.clone())
    // Remove the parent thought from active set (children replace it)
```

Fan-out: 1 thought becomes N thoughts.

**Partial failure semantics:** If generation fails partway (e.g., 2 of 4 branches succeed before an error), the parent thought is preserved and the successful branches are discarded. This is an all-or-nothing operation per parent thought — partial branches would leave the pool in an ambiguous state where some parents have children and some don't, making subsequent Score/KeepBest operations unpredictable. The error propagates to the caller.

### `Score`

```
for each active thought in pool:
    match strategy:
        LlmRating { criteria } => score = scorer.score(thought, criteria).await?
        Heuristic { pattern } => score = if regex_match(pattern, thought.content) 1.0 else 0.0
        External => skip (score set externally)
    thought.score = Some(score)
```

### `KeepBest`

```
let mut scored = pool.scored(), sorted descending by score
let keep_ids: top N thought IDs
remove all active thoughts not in keep_ids
```

Fan-in: M thoughts become N thoughts (N <= M).

### `Merge`

```
let active = all active thoughts
match strategy:
    LlmSynthesis { instruction } => content = merger.merge(&active, instruction).await?
    Concatenate { separator } => content = join active contents with separator

let parent_ids = all active thought IDs
pool.create(content, parent_ids, merged_metadata)
remove all parents from pool
```

Fan-in: M thoughts become 1 thought.

### `Refine`

```
for iteration in 0..max_iterations:
    // Score current thoughts
    execute Score { strategy: scoring.clone() }
    // Check if any thought meets target
    if pool.top_n(1).score >= target_score:
        break
    // Re-generate from each active thought (1 branch = improvement attempt)
    execute Generate { num_branches: 1, prompt_override: Some(refine_prompt) }
```

The refine prompt includes the thought's current content and score, asking the LLM to improve it.

### `Validate`

```
for each active thought in pool:
    let passes = match strategy:
        ExactMatch { expected } => thought.content.trim() == expected.trim()
        Contains { expected } => thought.content.contains(&expected)
        LlmJudge { criteria } => scorer.score(thought, criteria).await? >= 0.5
        AlwaysPass => true
    thought.score = Some(if passes 1.0 else 0.0)
```

---

## Back-Edge Execution

The dispatcher maintains a `HashMap<(GraphNodeId, GraphNodeId), usize>` counting traversals per back-edge:

```
for (target, is_back_edge) in graph.all_successors(current_node):
    if is_back_edge:
        let count = back_edge_counts.entry((current_node, target)).or_insert(0)
        if *count >= graph.max_iterations():
            continue  // exhausted, skip
        *count += 1
    next_nodes.push(target)
```

If all successors are exhausted back-edges and there are no forward edges, execution terminates at that node.

---

## Acceptance Criteria

- `GraphDispatcher::execute()` walks a graph from entry to terminal nodes
- `Generate` fans out correctly (N branches per input thought)
- `Generate` is all-or-nothing per parent (partial failures preserve parent, discard partial branches)
- `Score` assigns scores to all active thoughts
- `LlmThoughtScorer` uses regex extraction for score parsing, not naive `parse::<f64>()`
- `KeepBest` prunes to top-N
- `Merge` combines all active thoughts into one
- `Refine` iterates up to max_iterations, stopping early if target_score is met
- `Validate` assigns pass/fail scores
- Back-edges respect iteration limits
- Each operation execution emits a `tracing::info_span!` with node ID, operation type, and cycle count
- `GraphExecutionResult` accurately reports llm_calls and operations_executed
- Unit tests use mock implementations of all three traits (no real LLM calls in tests)
- All existing tests pass unchanged

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
cargo test -p fx-decompose
```

## Estimated Size

~400 lines including mock implementations and tests.
