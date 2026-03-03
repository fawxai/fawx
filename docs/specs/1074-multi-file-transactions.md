# #1074 — Multi-File Edit Transactions

**Status:** Implementation Spec  
**Date:** 2026-03-03  
**Crate scope:** New `fx-transactions` crate (implements `Skill` trait from `fx-loadable`)  
**Prerequisites:** None (standalone loadable skill)

---

## 1. Problem Statement

Fawx writes files one at a time via `write_file`. During refactoring — rename a struct, update 8 call sites — partial writes leave broken code. The model wastes 2-3 iterations diagnosing the inconsistency it created, then fixing the remaining files one by one.

### What this solves

Atomic multi-file writes: batch all changes, validate together, commit all or rollback all.

### What this does NOT do

- Does NOT replace `write_file` for single-file writes (still the simple/fast path)
- Does NOT provide VCS operations (that's `GitSkill`)
- Does NOT require human approval for `allow`-tier paths (that's `SelfModifyConfig`)
- Does NOT persist transactions across sessions (in-memory, session-scoped)

---

## 2. Existing Infrastructure

| Component | Relevance |
|-----------|-----------|
| `BuiltinToolsSkill.write_file` | Current single-file write. Transactions complement, don't replace. |
| `BuiltinToolsSkill.run_command` | Validation step runs configurable check commands via this. |
| `SelfModifyConfig` in `GitSkill` | `allow`/`propose`/`deny` tiers. Transactions respect these — `deny` paths rejected at stage time, `propose` paths require approval. |
| `Skill` trait | Transactions implement this. Standard pattern. |
| `SkillRegistry` | Registration point. |

---

## 3. Data Model

### 3.1 Transaction State

```rust
// fx-tools/src/transaction_skill.rs

#[derive(Debug, Clone)]
pub struct StagedWrite {
    /// Target file path.
    pub path: PathBuf,
    /// Original file content (for rollback). None if file didn't exist.
    pub original: Option<String>,
    /// New content to write.
    pub content: String,
    /// When this write was staged.
    pub staged_at: u32,  // iteration number
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    /// Accepting staged writes.
    Open,
    /// Validation passed, writes applied.
    Committed,
    /// Validation failed or explicit rollback.
    RolledBack,
    /// Cancelled without attempting validation.
    Cancelled,
}

#[derive(Debug)]
pub struct Transaction {
    pub id: u32,
    pub label: String,
    pub status: TransactionStatus,
    pub staged: Vec<StagedWrite>,
    pub validation_command: Option<String>,
    pub created_at_iteration: u32,
}

#[derive(Debug, Default)]
pub struct TransactionStore {
    transactions: HashMap<u32, Transaction>,
    next_id: u32,
}
```

### 3.2 TransactionStore API

```rust
impl TransactionStore {
    pub fn new() -> Self;

    /// Start a new transaction. Returns transaction ID.
    pub fn begin(&mut self, label: String, validation_command: Option<String>,
                 iteration: u32) -> u32;

    /// Stage a file write in an open transaction.
    pub fn stage(&mut self, tx_id: u32, path: PathBuf, content: String,
                 iteration: u32) -> Result<(), TransactionError>;

    /// List staged writes in a transaction.
    pub fn staged_files(&self, tx_id: u32) -> Result<Vec<&StagedWrite>, TransactionError>;

    /// Get transaction status.
    pub fn status(&self, tx_id: u32) -> Result<TransactionStatus, TransactionError>;

    /// Mark transaction as committed (after writes applied).
    pub fn mark_committed(&mut self, tx_id: u32) -> Result<(), TransactionError>;

    /// Mark transaction as rolled back.
    pub fn mark_rolled_back(&mut self, tx_id: u32) -> Result<(), TransactionError>;

    /// Cancel an open transaction without applying.
    pub fn cancel(&mut self, tx_id: u32) -> Result<(), TransactionError>;

    /// List all transactions (for debugging/status).
    pub fn list(&self) -> Vec<(u32, &str, TransactionStatus, usize)>;
}
```

---

## 4. Tool Definitions

### 4.1 TransactionSkill

| Tool | Args | Returns |
|------|------|---------|
| `tx_begin` | `label`, `validation_command?` | `"Transaction #N opened: {label}"` |
| `tx_stage` | `tx_id`, `path`, `content` | `"Staged {path} in transaction #N (M files total)"` |
| `tx_commit` | `tx_id` | `"Transaction #N committed: M files written"` or rollback message |
| `tx_status` | `tx_id?` | Status of specific transaction, or list all |
| `tx_cancel` | `tx_id` | `"Transaction #N cancelled"` |

### 4.2 Commit Flow (inside `execute` for `tx_commit`)

```
1. Read transaction from store
2. For each staged write:
   a. Read current file content → store as `original` (snapshot for rollback)
   b. Validate path against SelfModifyConfig (deny → abort)
3. Apply all writes (write files to disk)
4. If validation_command is set:
   a. Run command via run_command tool
   b. If exit code != 0:
      i.  Rollback: restore all originals
      ii. Return error with validation output
5. Mark transaction committed
6. Return success summary
```

### 4.3 Rollback Flow

```
1. For each staged write (reverse order):
   a. If original is Some → write original back
   b. If original is None → delete the file
2. Mark transaction rolled back
3. Return rollback summary with validation error
```

### 4.4 Error Cases

| Error | Behavior |
|-------|----------|
| Stage to non-Open transaction | Return error, no state change |
| Commit with 0 staged writes | Return error, transaction stays Open |
| Path in `deny` tier | Abort at commit time, full rollback |
| Validation command fails | Full rollback, return stderr/stdout |
| File read error during snapshot | Abort commit, no files written |
| File write error during apply | Partial rollback of already-written files, return error |
| Commit already-committed tx | Return error |

---

## 5. Skill Implementation

```rust
// fx-transactions/src/skill.rs

#[derive(Debug)]
pub struct TransactionSkill {
    store: Arc<Mutex<TransactionStore>>,
    repo_root: PathBuf,
    /// Used for validation command execution.
    command_runner: Arc<dyn Fn(&str) -> Result<CommandOutput, String> + Send + Sync>,
}

impl TransactionSkill {
    pub fn new(repo_root: PathBuf,
               command_runner: Arc<dyn Fn(&str) -> Result<CommandOutput, String> + Send + Sync>)
        -> Self;
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
impl Skill for TransactionSkill {
    fn name(&self) -> &str { "transactions" }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        // 5 tools: tx_begin, tx_stage, tx_commit, tx_status, tx_cancel
    }

    fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
        ToolCacheability::NeverCache  // stateful
    }

    async fn execute(&self, tool_name: &str, arguments: &str,
                     cancel: Option<&CancellationToken>)
        -> Option<Result<String, SkillError>>;
}
```

---

## 6. Implementation Plan

### Phase 1: Transaction Store

1. Create `fx-transactions` crate (`engine/crates/fx-transactions/`). Deps: `fx-loadable` (Skill trait), `fx-core`, `fx-kernel` (CancellationToken), `serde`, `thiserror`, `async-trait`, `tokio` (fs).
2. Implement `StagedWrite`, `Transaction`, `TransactionStatus`, `TransactionStore`
3. All pure in-memory operations, no I/O
4. Unit tests for begin/stage/cancel/list

### Phase 2: Commit + Rollback

1. Implement commit flow: snapshot → write → validate → rollback-on-fail
2. Rollback: restore originals, delete new files
3. Inject command runner for validation
4. Tests with temp directories: successful commit, failed validation → rollback, partial write error

### Phase 3: Skill Wiring

1. Implement `TransactionSkill` as `Skill`
2. 5 tool definitions with JSON schemas
3. Register in `SkillRegistry` in fx-cli
4. Integration tests: full tool-call flow through skill interface

---

## 7. Test Plan

### TransactionStore Tests

| Test | Assertion |
|------|-----------|
| `begin_returns_monotonic_ids` | IDs increment |
| `stage_to_open_transaction_succeeds` | Write staged, count incremented |
| `stage_to_committed_transaction_fails` | Error returned |
| `stage_to_cancelled_transaction_fails` | Error returned |
| `cancel_open_transaction_succeeds` | Status → Cancelled |
| `cancel_committed_transaction_fails` | Error returned |
| `list_returns_all_transactions` | All txs with correct metadata |
| `staged_files_returns_correct_list` | Correct paths and content |

### Commit/Rollback Tests

| Test | Assertion |
|------|-----------|
| `commit_writes_all_files` | All staged files exist on disk with correct content |
| `commit_empty_transaction_fails` | Error, status unchanged |
| `commit_snapshots_originals` | Original content captured before overwrite |
| `rollback_restores_originals` | Files restored to pre-commit state |
| `rollback_deletes_new_files` | Files that didn't exist pre-commit are removed |
| `validation_failure_triggers_rollback` | Failed check → all files restored |
| `validation_success_commits` | Passed check → files persist |
| `validation_output_included_in_error` | Stderr/stdout in error message |
| `deny_path_aborts_before_any_writes` | No files written if any path is denied |

### Skill Tests

| Test | Assertion |
|------|-----------|
| `tool_definitions_returns_five_tools` | Correct count and names |
| `tx_begin_creates_transaction` | Returns success, store has entry |
| `tx_stage_adds_file` | File in staged list |
| `tx_commit_applies_writes` | Files on disk |
| `tx_status_shows_transaction_info` | Correct status/file count |
| `tx_cancel_marks_cancelled` | Status updated |

### Integration Tests

| Test | Assertion |
|------|-----------|
| `full_refactor_flow` | Begin → stage 3 files → commit with `cargo check` → all pass |
| `failed_validation_full_rollback` | Begin → stage 3 files → commit with failing check → all files restored |
| `multiple_concurrent_transactions` | Two open transactions don't interfere |

---

## 8. Estimated Complexity

| Phase | Lines (code) | Lines (tests) | Effort |
|-------|-------------|---------------|--------|
| Phase 1: Store | ~150 | ~120 | 0.5 day |
| Phase 2: Commit/rollback | ~200 | ~180 | 1 day |
| Phase 3: Skill wiring | ~120 | ~80 | 0.5 day |
| **Total** | **~470** | **~380** | **2 days** |

---

## 9. Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Partial rollback failure (can't restore a file) | Log error per file, continue rolling back remaining. Return list of unrecoverable files. |
| Validation command hangs | Use existing `run_command` timeout (configurable). Cancel token support. |
| Large file staging OOMs | Cap staged content at 1MB per file, 10MB per transaction. Return error on exceed. |
| Race condition: external process modifies file between snapshot and rollback | Accept as known limitation in V1. Snapshot is best-effort. |
| Model forgets to commit | Transaction auto-expires after 10 iterations with warning in tool output. |
