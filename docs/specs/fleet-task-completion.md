# Fleet Task Completion Spec

**Crate:** `fx-cli` (serve_fleet.rs) + `fx-fleet` (http types)  
**Difficulty:** Medium  
**Status:** 70% done — worker loop, heartbeat, dispatch, stub executor all exist  

---

## What Exists

`ExperimentTaskExecutor` in `serve_fleet.rs`:
- Receives `FleetTaskRequest` with `repo_url`, `branch`, `git_token`, `signal`, `config`, `chain_history`, `scope`
- Calls `fx_improve::run_improvement_cycle()` with a real `CompletionProvider`
- Returns `FleetTaskResult` with `plans_generated` and `proposals_written`

## What's Missing

### 1. Git Workspace Setup

The worker receives `repo_url` and `branch` but ignores them. The improvement cycle runs against whatever happens to be in `data_dir`.

**Fix:** Before running the improvement cycle, the worker should:
1. Clone or fetch the repo into a temp workspace
2. Checkout the specified branch
3. Run the improvement cycle against that workspace
4. Extract the resulting patch via `git diff`

```rust
struct TaskWorkspace {
    temp_dir: TempDir,
    repo_path: PathBuf,
}

impl TaskWorkspace {
    async fn setup(task: &FleetTaskRequest) -> Result<Self, String> {
        let temp_dir = TempDir::new().map_err(|e| format!("temp dir: {e}"))?;
        let repo_path = temp_dir.path().join("repo");
        clone_repo(&task.repo_url, &task.branch, task.git_token.as_deref(), &repo_path).await?;
        Ok(Self { temp_dir, repo_path })
    }
    
    async fn extract_patch(&self) -> Result<Option<String>, String> {
        // git diff HEAD in repo_path
    }
}
```

Git operations use `tokio::process::Command` (same pattern as `fx-consensus` build verification).

### 2. Candidate Patch Extraction

After `run_improvement_cycle()`, the worker should:
1. Run `git diff HEAD` in the workspace
2. If diff is non-empty, include it as `candidate_patch` in the result
3. Include the approach/summary from `ImprovementRunResult`

### 3. Task-Scoped Evaluation

Currently the worker generates proposals but doesn't evaluate them. After generating:
1. Run `cargo check` in the workspace (does it compile?)
2. Run `cargo test` scoped to the target crate if `scope` specifies one
3. Include build/test results in the evaluation field

```rust
async fn evaluate_workspace(repo_path: &Path, scope: &[String]) -> EvaluationResult {
    let build_ok = run_cargo_check(repo_path).await;
    let test_result = if build_ok {
        run_cargo_test(repo_path, scope).await
    } else {
        TestOutcome::Skipped
    };
    EvaluationResult { build_ok, test_result }
}
```

### 4. Chain History Integration

The task includes `chain_history` but it's not passed to the improvement cycle. Wire it through so the LLM prompt includes prior experiment results.

---

## Files to Change

1. **`engine/crates/fx-cli/src/commands/serve_fleet.rs`:**
   - Add `TaskWorkspace` struct with `setup()` and `extract_patch()`
   - Update `run_task()` to: setup workspace → run cycle → extract patch → evaluate → return
   - Wire `chain_history` from task into improvement cycle

2. **`engine/crates/fx-fleet/src/http.rs`:**
   - No changes needed — `FleetTaskResult` already has `candidate_patch` and `evaluation` fields

---

## Test Plan

1. **TaskWorkspace::setup** — clones a test repo (use temp git init), checks out branch
2. **extract_patch** — workspace with changes produces diff, clean workspace produces None
3. **evaluate_workspace** — passing code returns build_ok=true, broken code returns build_ok=false
4. **run_task integration** — mock improvement provider, verify result has candidate_patch and evaluation
5. **chain_history** — verify it reaches the improvement prompt

---

## Implementation Notes

- Use `TempDir` for workspace isolation (auto-cleanup on drop)
- Git clone with optional token: `git clone --depth 1 --branch <branch> https://<token>@github.com/...`
- `cargo check` timeout: 120s (same as `fx-consensus` build verification)
- `cargo test` timeout: 300s
- If clone fails, return `FleetTaskStatus::Failed` with the error — don't fall back to local dir
