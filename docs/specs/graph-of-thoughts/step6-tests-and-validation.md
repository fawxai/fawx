# Step 6: Tests & Validation

## Goal

End-to-end integration tests that exercise the full GoT pipeline — from builder to dispatcher to result — using mock LLM backends. Validates that all six operations work correctly in composed graphs, back-edges respect iteration limits, budget enforcement works, and preset graphs produce expected topologies.

## Why This Slice Exists

Steps 1–5 each have unit tests for their own components. This step tests the **composition** — operations working together in real graph topologies. This is where subtle bugs in thought pool management, edge traversal, and result aggregation surface.

---

## Expected Targets

- `engine/crates/fx-decompose/src/graph_tests.rs` (new — integration test module)
- `engine/crates/fx-decompose/src/lib.rs` (add `#[cfg(test)] mod graph_tests;`)
- Optionally: `engine/crates/fx-decompose/tests/got_integration.rs` (external integration test)

---

## Dependencies

- All GoT types and implementations from Steps 1–5
- Mock implementations of `ThoughtGenerator`, `ThoughtScorer`, `ThoughtMerger` from Step 3

---

## Mock Infrastructure

### `MockGenerator`

```rust
/// Returns deterministic content based on parent content + branch index.
struct MockGenerator {
    /// If set, all generated thoughts contain this prefix.
    content_prefix: String,
}

impl ThoughtGenerator for MockGenerator {
    async fn generate(&self, parent: &ThoughtState, num_branches: usize, _prompt: Option<&str>) -> Result<Vec<String>> {
        Ok((0..num_branches)
            .map(|i| format!("{}{}-branch-{i}", self.content_prefix, parent.content))
            .collect())
    }
}
```

### `MockScorer`

```rust
/// Scores based on content length (longer = higher score, normalized to 0.0–1.0).
/// Or uses a fixed score map for deterministic testing.
struct MockScorer {
    fixed_scores: Option<HashMap<String, f64>>,
}

impl ThoughtScorer for MockScorer {
    async fn score(&self, thought: &ThoughtState, _criteria: &str) -> Result<f64> {
        if let Some(scores) = &self.fixed_scores {
            Ok(*scores.get(&thought.content).unwrap_or(&0.5))
        } else {
            Ok((thought.content.len() as f64 / 100.0).min(1.0))
        }
    }
}
```

### `MockMerger`

```rust
/// Concatenates thought contents with " + " separator.
struct MockMerger;

impl ThoughtMerger for MockMerger {
    async fn merge(&self, thoughts: &[&ThoughtState], _instruction: Option<&str>) -> Result<String> {
        Ok(thoughts.iter().map(|t| t.content.as_str()).collect::<Vec<_>>().join(" + "))
    }
}
```

---

## Test Cases

### 1. Linear Chain (CoT equivalent)

```rust
#[tokio::test]
async fn cot_equivalent_single_path() {
    let graph = GraphBuilder::chain_of_thought("quality");
    let result = execute_with_mocks(graph, "solve 2+2").await;
    assert_eq!(result.thoughts.len(), 1);
    assert!(result.best.is_some());
    assert_eq!(result.operations_executed, 2); // Generate + Score
}
```

### 2. Tree of Thought (branch + prune)

```rust
#[tokio::test]
async fn tot_branches_and_prunes() {
    let graph = GraphBuilder::tree_of_thought(4, "correctness");
    let result = execute_with_mocks(graph, "sort this list").await;
    assert_eq!(result.thoughts.len(), 1); // KeepBest(1)
    assert!(result.best.unwrap().score.unwrap() > 0.0);
    assert!(result.llm_calls >= 5); // 4 generates + 4 scores + ... 
}
```

### 3. Full GoT (branch + score + prune + merge + refine)

```rust
#[tokio::test]
async fn full_got_pipeline() {
    let graph = GraphBuilder::graph_of_thought(3, 2, 2, 0.95, "mathematical correctness");
    let result = execute_with_mocks(graph, "prove P != NP").await;
    // Should have executed: Generate(3) → Score → KeepBest(2) → Merge → Refine(2 iters) → Validate
    assert!(result.operations_executed >= 5);
    assert!(result.best.is_some());
}
```

### 4. Refinement loop terminates

```rust
#[tokio::test]
async fn refine_terminates_at_max_iterations() {
    let graph = GraphBuilder::new(3)
        .generate(1)
        .refine(3, 0.99, "perfection")  // target_score unreachable with mock
        .build()
        .unwrap();
    let result = execute_with_mocks(graph, "impossible task").await;
    assert!(result.refinement_capped);
}
```

### 5. Refinement loop early exit

```rust
#[tokio::test]
async fn refine_exits_early_when_target_met() {
    // MockScorer returns 1.0 for content containing "perfect"
    let scorer = MockScorer { fixed_scores: Some(hashmap!{ "perfect-answer".into() => 1.0 }) };
    // MockGenerator always produces "perfect-answer"
    let generator = MockGenerator { content_prefix: "perfect-answer".into() };

    let graph = GraphBuilder::new(5)
        .generate(1)
        .refine(5, 0.9, "quality")
        .build()
        .unwrap();

    let result = execute_with_custom_mocks(graph, "task", generator, scorer).await;
    assert!(!result.refinement_capped); // Should exit after 1 iteration
}
```

### 6. Back-edge iteration limit

```rust
#[tokio::test]
async fn back_edge_respects_max_iterations() {
    let mut graph = GraphOfOperations::new(2); // max 2 iterations per back-edge
    let n0 = graph.add_node(score_op(), Some("score".into()));
    let n1 = graph.add_node(generate_op(1), Some("improve".into()));
    graph.add_edge(n0, n1).unwrap();
    graph.add_back_edge(n1, n0).unwrap(); // cycle: score → improve → score

    let result = execute_with_mocks_raw(graph, "start").await;
    // Should execute: score, improve, score, improve, score (2 back-edge traversals)
    assert_eq!(result.operations_executed, 5);
}
```

### 7. Empty graph rejected

```rust
#[test]
fn empty_graph_build_fails() {
    let result = GraphBuilder::new(3).build();
    assert!(result.is_err());
}
```

### 8. Consensus pattern

```rust
#[tokio::test]
async fn consensus_merges_branches() {
    let graph = GraphBuilder::consensus(3, "factual accuracy");
    let result = execute_with_mocks(graph, "what is the capital of France").await;
    assert_eq!(result.thoughts.len(), 1); // merged into one
    assert!(result.best.unwrap().content.contains("+")); // MockMerger uses "+"
}
```

### 9. Budget exhaustion mid-graph

```rust
#[tokio::test]
async fn budget_exhaustion_returns_partial() {
    let graph = GraphBuilder::graph_of_thought(10, 5, 5, 0.99, "quality");
    // Set budget to allow only 5 LLM calls
    let result = execute_with_budget(graph, "complex task", 5).await;
    assert!(result.best.is_some()); // Should still return best so far
    assert!(result.llm_calls <= 6); // Approximately 5, with possible off-by-one
}
```

### 10. Preset topology validation

```rust
#[test]
fn all_presets_produce_valid_graphs() {
    let cot = GraphBuilder::chain_of_thought("test");
    assert!(cot.validate().is_ok());

    let tot = GraphBuilder::tree_of_thought(3, "test");
    assert!(tot.validate().is_ok());

    let got = GraphBuilder::graph_of_thought(4, 2, 3, 0.8, "test");
    assert!(got.validate().is_ok());

    let con = GraphBuilder::consensus(3, "test");
    assert!(con.validate().is_ok());
}
```

### 11. ThoughtPool invariants under operations

```rust
#[tokio::test]
async fn thought_pool_ids_never_reused() {
    // Run a full GoT graph and collect all thought IDs ever created
    // Verify no ID appears twice
}

#[tokio::test]
async fn parent_ids_reference_existing_thoughts() {
    // After each operation, verify all parent_ids in the pool
    // reference thoughts that existed at some point (may have been pruned)
}
```

---

## Regression Safety

All tests in this file are additive. They must not:
- Modify any existing test file
- Change any existing type signature
- Import anything from the GoT modules into existing test modules

Existing test suites must pass unmodified:

```bash
cargo test -p fx-decompose -- --test-threads=1
cargo test -p fx-kernel
cargo test --workspace
```

---

## Acceptance Criteria

- All 11+ test cases pass
- Mock infrastructure is reusable (exported under `#[cfg(test)]` or `test-support` feature)
- No flaky tests — all mocks are deterministic
- Test execution time < 5 seconds (no real LLM calls, no I/O)
- Full workspace test suite passes unchanged

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
cargo test -p fx-decompose -- got  # run only GoT tests
```

## Estimated Size

~300 lines of test code + ~100 lines of mock infrastructure.
