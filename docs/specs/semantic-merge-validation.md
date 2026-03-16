# Semantic Merge Validation Spec

**Crate:** `fx-decompose` (aggregator.rs)  
**Difficulty:** Medium  
**Status:** 0% — SimpleAggregator just concatenates patches  

---

## Problem

The auto-decomposition engine breaks problems into sub-goals and dispatches them. Each sub-goal produces a patch. `SimpleAggregator` concatenates these patches, but:

1. Patches may conflict (both modify the same file/region)
2. Combined patches may not compile even if individual ones do
3. No verification that the merged result actually works

---

## Solution: BuildVerifyAggregator

New aggregator that applies patches to a real git workspace, builds, and verifies.

### Pipeline

```
For each completed sub-goal patch (in order):
  1. git apply --3way <patch>
  2. If conflict → attempt resolution or mark sub-goal as MergeConflict
  3. After all patches applied:
     a. cargo check (does it compile?)
     b. cargo test --workspace (do tests pass?)
  4. Extract combined diff: git diff <base-commit>
  5. If build fails → greedy removal: drop last patch, retry build
```

### Types

```rust
pub struct BuildVerifyAggregator {
    /// Working directory for a temporary git workspace.
    workspace_provider: Box<dyn WorkspaceProvider>,
    /// Build timeout.
    build_timeout: Duration,
    /// Test timeout.
    test_timeout: Duration,
}

/// Provides a temporary git workspace for merge verification.
#[async_trait]
pub trait WorkspaceProvider: Send + Sync {
    /// Create a fresh workspace cloned from the experiment's repo.
    async fn create(&self, experiment: &Experiment) -> Result<TempWorkspace, DecomposeError>;
}

pub struct TempWorkspace {
    pub path: PathBuf,
    _temp_dir: TempDir,
}

impl TempWorkspace {
    /// Apply a patch to the workspace.
    pub async fn apply_patch(&self, patch: &str) -> Result<(), PatchApplyError>;
    
    /// Run cargo check.
    pub async fn cargo_check(&self, timeout: Duration) -> Result<(), BuildError>;
    
    /// Run cargo test.
    pub async fn cargo_test(&self, timeout: Duration) -> Result<TestResult, BuildError>;
    
    /// Get the combined diff from base.
    pub async fn combined_diff(&self) -> Result<String, DecomposeError>;
    
    /// Reset to base (undo all patches).
    pub async fn reset(&self) -> Result<(), DecomposeError>;
}

#[derive(Debug)]
pub enum PatchApplyError {
    Conflict { patch_index: usize, detail: String },
    InvalidPatch { detail: String },
    IoError(String),
}

pub struct MergeResult {
    /// Combined patch of all successfully applied sub-goals.
    pub combined_patch: String,
    /// Which sub-goals were successfully merged.
    pub merged: Vec<usize>,
    /// Which sub-goals had conflicts and were dropped.
    pub conflicts: Vec<(usize, String)>,
    /// Build verification result.
    pub build_ok: bool,
    /// Test result (if build passed).
    pub test_result: Option<TestResult>,
}

pub struct TestResult {
    pub passed: usize,
    pub failed: usize,
    pub total: usize,
}
```

### Greedy Conflict Resolution

When `git apply --3way` fails for a patch:
1. Skip that sub-goal's patch
2. Record it as a conflict in `MergeResult.conflicts`
3. Continue with remaining patches
4. At the end, if build fails:
   a. Remove the last successfully-applied patch
   b. Re-run build
   c. Repeat until build passes or no patches remain

This is greedy, not optimal — but it's simple and handles the common case where one bad patch breaks the build.

---

## Integration with ResultAggregator Trait

```rust
#[async_trait]
impl ResultAggregator for BuildVerifyAggregator {
    async fn aggregate(
        &self,
        plan: &DecompositionPlan,
        results: &[SubGoalResult],
        experiment: &Experiment,
    ) -> Result<AggregatedResult, DecomposeError> {
        let workspace = self.workspace_provider.create(experiment).await?;
        let merge = self.merge_and_verify(&workspace, results).await?;
        Ok(AggregatedResult {
            combined_patch: merge.combined_patch,
            approach: format_merge_approach(&merge, results),
            sub_goal_outcomes: build_outcomes(results, &merge),
            completion_rate: merge.merged.len() as f64 / results.len().max(1) as f64,
        })
    }
}
```

---

## Files to Change

1. **`engine/crates/fx-decompose/src/aggregator.rs`:**
   - Add `BuildVerifyAggregator` alongside existing `SimpleAggregator`
   - Add `WorkspaceProvider` trait, `TempWorkspace`, `MergeResult`, `PatchApplyError`

2. **`engine/crates/fx-decompose/Cargo.toml`:**
   - Add `tempfile` to dev-dependencies (may need it for testing)
   - Add `tokio` process feature if not already present

---

## Test Plan

1. **Merge two clean patches** — both apply, build passes, combined diff correct
2. **Second patch conflicts** — first applies, second skipped, combined diff is first only
3. **Combined doesn't compile** — both apply but result broken, greedy removal drops second
4. **All patches conflict** — empty combined patch, completion_rate 0.0
5. **Empty results** — no patches, aggregation succeeds with empty output
6. **Single patch** — trivially applies, build verified

Note: Tests use a mock `WorkspaceProvider` that simulates git apply/cargo check outcomes without real git repos. Real integration tests are for the fleet.

---

## Implementation Notes

- `TempWorkspace` uses `tokio::process::Command` for git and cargo (same as fx-consensus)
- `git apply --3way` requires the workspace to be a git repo with a base commit
- Patch format expected: unified diff (same as experiment chain `winning_patch`)
- Build timeout default: 120s. Test timeout default: 300s.
- The `WorkspaceProvider` abstraction exists so tests can mock it. Real impl clones from local path or remote URL.
