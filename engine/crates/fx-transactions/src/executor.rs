//! Commit and rollback logic with filesystem I/O.
//!
//! The executor operates on a [`TransactionStore`] and performs the actual
//! file writes, validation, and rollback steps. It is intentionally separated
//! from the store to keep I/O concerns isolated.

use std::path::{Path, PathBuf};

use fx_core::self_modify::{classify_path, format_tier_violation, PathTier, SelfModifyConfig};
use tokio::process::Command;
use tracing::{info, warn};

use crate::error::TransactionError;
use crate::store::{Transaction, TransactionStatus, TransactionStore};

const SIGNAL_EXIT_CODE: i32 = -1;

/// Result of a validation command execution.
// TODO(#1090): consolidate with skill-layer CommandOutput in Phase 3
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Commit a transaction: snapshot originals, write files, validate, rollback on failure.
///
/// # Errors
///
/// Returns `TransactionError` on:
/// - Transaction not found or not in `Open` state
/// - Empty transaction (no staged writes)
/// - File snapshot failure (before any writes)
/// - File write failure (with partial rollback)
/// - Validation command failure (with full rollback)
pub async fn commit(
    store: &mut TransactionStore,
    tx_id: u32,
    work_dir: Option<&Path>,
    self_modify: &SelfModifyConfig,
    policy_base_dir: &Path,
) -> Result<usize, TransactionError> {
    validate_commit_preconditions(store, tx_id, self_modify, policy_base_dir)?;
    snapshot_originals(store, tx_id).await?;
    let written_count = apply_writes(store, tx_id).await?;
    validate_or_rollback(store, tx_id, work_dir, written_count).await?;

    store.mark_committed(tx_id)?;
    info!(tx_id, file_count = written_count, "transaction committed");
    Ok(written_count)
}

/// Validate that a transaction is in the correct state for commit.
fn validate_commit_preconditions(
    store: &TransactionStore,
    tx_id: u32,
    self_modify: &SelfModifyConfig,
    policy_base_dir: &Path,
) -> Result<(), TransactionError> {
    let status = store.status(tx_id)?;
    if status != TransactionStatus::Open {
        return Err(TransactionError::InvalidState {
            id: tx_id,
            status: status.to_string(),
            expected: "Open".to_string(),
        });
    }

    let tx = store.get(tx_id)?;
    if tx.staged.is_empty() {
        return Err(TransactionError::EmptyTransaction(tx_id));
    }

    validate_staged_paths(tx_id, tx, self_modify, policy_base_dir)
}

fn validate_staged_paths(
    tx_id: u32,
    tx: &Transaction,
    self_modify: &SelfModifyConfig,
    policy_base_dir: &Path,
) -> Result<(), TransactionError> {
    for write in &tx.staged {
        let tier = classify_path(&write.path, policy_base_dir, self_modify);
        if tier != PathTier::Allow {
            return Err(policy_violation_error(tx_id, &write.path, tier));
        }
    }

    Ok(())
}

fn policy_violation_error(tx_id: u32, path: &Path, tier: PathTier) -> TransactionError {
    let detail = format_tier_violation(path, tier).unwrap_or_else(|| {
        format!(
            "Self-modify policy violation [{tier:?}] for {}",
            path.display()
        )
    });

    TransactionError::SelfModifyPathViolation {
        tx_id,
        path: path.to_path_buf(),
        tier,
        detail,
    }
}

/// Phase 1: Read current file contents and store as originals for rollback.
async fn snapshot_originals(
    store: &mut TransactionStore,
    tx_id: u32,
) -> Result<(), TransactionError> {
    let tx = store.get(tx_id)?;
    let mut snapshots: Vec<Option<String>> = Vec::with_capacity(tx.staged.len());

    for write in &tx.staged {
        let original = read_file_if_exists(&write.path).await?;
        snapshots.push(original);
    }

    let tx = store.get_mut(tx_id)?;
    for (i, original) in snapshots.into_iter().enumerate() {
        tx.staged[i].original = original;
    }
    Ok(())
}

/// Phase 2: Write all staged files to disk. On failure, roll back already-written files.
async fn apply_writes(store: &mut TransactionStore, tx_id: u32) -> Result<usize, TransactionError> {
    let tx = store.get(tx_id)?;
    let writes: Vec<(PathBuf, String)> = tx
        .staged
        .iter()
        .map(|w| (w.path.clone(), w.content.clone()))
        .collect();

    let mut written_count = 0;
    for (path, content) in &writes {
        if let Err(err) = write_file(path, content).await {
            warn!(
                tx_id,
                path = %path.display(),
                "write failed, rolling back {written_count} already-written files",
            );
            let rollback_errors = rollback_files(store, tx_id, written_count).await;
            store
                .mark_rolled_back(tx_id)
                .unwrap_or_else(|e| warn!("failed to mark rolled back: {e}"));

            return Err(combine_write_error_with_rollback(err, rollback_errors));
        }
        written_count += 1;
    }
    Ok(written_count)
}

/// Combine a write error with any rollback errors into a single error.
fn combine_write_error_with_rollback(
    err: TransactionError,
    rollback_errors: Vec<String>,
) -> TransactionError {
    if rollback_errors.is_empty() {
        return err;
    }
    let rollback_detail = rollback_errors.join("; ");
    match err {
        TransactionError::WriteFailed { path, reason } => TransactionError::WriteFailed {
            path,
            reason: format!("{reason}; rollback errors: {rollback_detail}"),
        },
        TransactionError::DirectoryCreationFailed { path, reason } => {
            TransactionError::WriteFailed {
                path,
                reason: format!("directory creation: {reason}; rollback errors: {rollback_detail}"),
            }
        }
        other => other,
    }
}

/// Phase 3: Run validation command. On failure, roll back all written files.
async fn validate_or_rollback(
    store: &mut TransactionStore,
    tx_id: u32,
    work_dir: Option<&Path>,
    written_count: usize,
) -> Result<(), TransactionError> {
    let validation_command = store.get(tx_id)?.validation_command.clone();
    let Some(ref cmd) = validation_command else {
        return Ok(());
    };

    let output = execute_validation_command(cmd, work_dir).await;
    if output.exit_code == 0 {
        return Ok(());
    }

    info!(
        tx_id,
        exit_code = output.exit_code,
        "validation failed, rolling back all files",
    );
    let rollback_errors = rollback_files(store, tx_id, written_count).await;
    store
        .mark_rolled_back(tx_id)
        .unwrap_or_else(|e| warn!("failed to mark rolled back: {e}"));

    Err(TransactionError::ValidationFailed {
        exit_code: output.exit_code,
        detail: format_validation_detail(&output),
        rollback_errors,
    })
}

/// Explicitly roll back an open transaction.
///
/// If the transaction is still `Open` (no commit was attempted), no writes
/// were applied to disk, so the filesystem is untouched — we simply mark
/// the transaction as `RolledBack`. Use `cancel()` for the common case of
/// abandoning an un-committed transaction; this function exists for symmetry
/// with the commit-triggered rollback path.
pub async fn rollback(
    store: &mut TransactionStore,
    tx_id: u32,
) -> Result<Vec<String>, TransactionError> {
    let status = store.status(tx_id)?;
    if status != TransactionStatus::Open {
        return Err(TransactionError::InvalidState {
            id: tx_id,
            status: status.to_string(),
            expected: "Open".to_string(),
        });
    }

    // No writes have been applied (commit was never called), so there is
    // nothing to undo on disk. Just mark the transaction as rolled back.
    store.mark_rolled_back(tx_id)?;
    Ok(vec![])
}

/// Roll back the first `count` staged writes in a transaction (reverse order).
///
/// Best-effort: logs and collects errors but continues with remaining files.
async fn rollback_files(store: &TransactionStore, tx_id: u32, count: usize) -> Vec<String> {
    let tx = match store.get(tx_id) {
        Ok(tx) => tx,
        Err(e) => return vec![format!("cannot read transaction: {e}")],
    };

    let mut errors = Vec::new();

    // Process in reverse order
    for write in tx.staged.iter().take(count).rev() {
        match &write.original {
            Some(original) => {
                // File existed before — restore original content
                if let Err(e) = write_file(&write.path, original).await {
                    let msg = format!("failed to restore {}: {e}", write.path.display());
                    warn!("{msg}");
                    errors.push(msg);
                }
            }
            None => {
                // File was new — delete it
                if let Err(e) = tokio::fs::remove_file(&write.path).await {
                    // File might already be gone; that's fine
                    if e.kind() != std::io::ErrorKind::NotFound {
                        let msg =
                            format!("failed to delete new file {}: {e}", write.path.display());
                        warn!("{msg}");
                        errors.push(msg);
                    }
                }
            }
        }
    }

    errors
}

/// Read a file's content if it exists, returning `None` for non-existent files.
async fn read_file_if_exists(path: &Path) -> Result<Option<String>, TransactionError> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(TransactionError::SnapshotFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        }),
    }
}

/// Write content to a file, creating parent directories as needed.
async fn write_file(path: &Path, content: &str) -> Result<(), TransactionError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            TransactionError::DirectoryCreationFailed {
                path: path.to_path_buf(),
                reason: e.to_string(),
            }
        })?;
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|e| TransactionError::WriteFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })
}

/// Run a validation command and capture its output.
async fn execute_validation_command(cmd: &str, work_dir: Option<&Path>) -> CommandOutput {
    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd);
    if let Some(dir) = work_dir {
        command.current_dir(dir);
    }

    match command.output().await {
        Ok(output) => CommandOutput {
            exit_code: output.status.code().unwrap_or(SIGNAL_EXIT_CODE),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        },
        Err(e) => CommandOutput {
            exit_code: SIGNAL_EXIT_CODE,
            stdout: String::new(),
            stderr: format!("failed to run command: {e}"),
        },
    }
}

/// Format validation command output for error messages.
fn format_validation_detail(output: &CommandOutput) -> String {
    let mut detail = String::new();
    if !output.stdout.is_empty() {
        detail.push_str("stdout: ");
        detail.push_str(&output.stdout);
    }
    if !output.stderr.is_empty() {
        if !detail.is_empty() {
            detail.push('\n');
        }
        detail.push_str("stderr: ");
        detail.push_str(&output.stderr);
    }
    if detail.is_empty() {
        detail.push_str("(no output)");
    }
    detail
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn temp_path(dir: &TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    async fn commit_with_default_policy(
        store: &mut TransactionStore,
        tx_id: u32,
        work_dir: Option<&Path>,
    ) -> Result<usize, TransactionError> {
        let self_modify = SelfModifyConfig::default();
        commit(store, tx_id, work_dir, &self_modify, Path::new("")).await
    }

    fn strict_self_modify(allow_paths: &[&str], deny_paths: &[&str]) -> SelfModifyConfig {
        SelfModifyConfig {
            enabled: true,
            allow_paths: allow_paths.iter().map(|path| path.to_string()).collect(),
            deny_paths: deny_paths.iter().map(|path| path.to_string()).collect(),
            ..SelfModifyConfig::default()
        }
    }

    #[tokio::test]
    async fn commit_writes_all_files() {
        let dir = TempDir::new().expect("tempdir");
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, temp_path(&dir, "a.txt"), "alpha".into(), 0)
            .expect("ok");
        store
            .stage(id, temp_path(&dir, "b.txt"), "beta".into(), 0)
            .expect("ok");

        let count = commit_with_default_policy(&mut store, id, None)
            .await
            .expect("commit ok");
        assert_eq!(count, 2);
        assert_eq!(
            store.status(id).expect("status"),
            TransactionStatus::Committed,
        );

        let a = tokio::fs::read_to_string(temp_path(&dir, "a.txt"))
            .await
            .expect("read a");
        let b = tokio::fs::read_to_string(temp_path(&dir, "b.txt"))
            .await
            .expect("read b");
        assert_eq!(a, "alpha");
        assert_eq!(b, "beta");
    }

    #[tokio::test]
    async fn commit_empty_transaction_fails() {
        let mut store = TransactionStore::new();
        let id = store.begin("empty".into(), None, 0).expect("begin ok");

        let err = commit_with_default_policy(&mut store, id, None)
            .await
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::EmptyTransaction(_)));
        // Transaction should still be open (not mutated on error)
        assert_eq!(store.status(id).expect("status"), TransactionStatus::Open);
    }

    #[tokio::test]
    async fn commit_snapshots_originals() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "existing.txt");
        tokio::fs::write(&path, "original content")
            .await
            .expect("write");

        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, path.clone(), "new content".into(), 0)
            .expect("ok");

        commit_with_default_policy(&mut store, id, None)
            .await
            .expect("commit ok");

        let tx = store.get(id).expect("found");
        assert_eq!(tx.staged[0].original.as_deref(), Some("original content"));

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn rollback_restores_originals() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "file.txt");
        tokio::fs::write(&path, "original").await.expect("write");

        let mut store = TransactionStore::new();
        // Use failing validation to trigger rollback after writes
        let id = store
            .begin("test".into(), Some("false".into()), 0)
            .expect("begin ok");
        store
            .stage(id, path.clone(), "modified".into(), 0)
            .expect("ok");

        let err = commit_with_default_policy(&mut store, id, None)
            .await
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::ValidationFailed { .. }));

        // Original content should be restored after rollback
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert_eq!(content, "original");

        assert_eq!(
            store.status(id).expect("status"),
            TransactionStatus::RolledBack,
        );
    }

    #[tokio::test]
    async fn rollback_deletes_new_files() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "new_file.txt");

        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, path.clone(), "new content".into(), 0)
            .expect("ok");

        // Manually simulate: snapshot (file doesn't exist), write, then rollback
        // Use commit with a failing validation to trigger rollback
        // The file should be deleted since original was None
        commit_with_default_policy(&mut store, id, None)
            .await
            .expect("commit ok");

        // Verify file exists after commit
        assert!(path.exists());

        // Create a new transaction to test rollback of new files
        let id2 = store
            .begin("rollback-test".into(), Some("false".into()), 1)
            .expect("begin ok");
        let new_path = temp_path(&dir, "brand_new.txt");
        store
            .stage(id2, new_path.clone(), "will be rolled back".into(), 1)
            .expect("ok");

        let err = commit_with_default_policy(&mut store, id2, None)
            .await
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::ValidationFailed { .. }));
        // The new file should have been cleaned up during rollback
        assert!(!new_path.exists());
    }

    #[tokio::test]
    async fn validation_failure_triggers_rollback() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "validated.txt");
        tokio::fs::write(&path, "original").await.expect("write");

        let mut store = TransactionStore::new();
        // Use "false" as a command that always fails
        let id = store
            .begin("test".into(), Some("false".into()), 0)
            .expect("begin ok");
        store
            .stage(id, path.clone(), "should be rolled back".into(), 0)
            .expect("ok");

        let err = commit_with_default_policy(&mut store, id, None)
            .await
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::ValidationFailed { .. }));

        // File should be restored to original
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert_eq!(content, "original");

        // Transaction should be marked rolled back
        assert_eq!(
            store.status(id).expect("status"),
            TransactionStatus::RolledBack,
        );
    }

    #[tokio::test]
    async fn validation_success_commits() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "validated.txt");

        let mut store = TransactionStore::new();
        // Use "true" as a command that always succeeds
        let id = store
            .begin("test".into(), Some("true".into()), 0)
            .expect("begin ok");
        store
            .stage(id, path.clone(), "validated content".into(), 0)
            .expect("ok");

        let count = commit_with_default_policy(&mut store, id, None)
            .await
            .expect("commit ok");
        assert_eq!(count, 1);
        assert_eq!(
            store.status(id).expect("status"),
            TransactionStatus::Committed,
        );

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert_eq!(content, "validated content");
    }

    #[tokio::test]
    async fn validation_output_included_in_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "file.txt");

        let mut store = TransactionStore::new();
        let id = store
            .begin(
                "test".into(),
                Some("echo 'check failed' >&2; exit 1".into()),
                0,
            )
            .expect("begin ok");
        store
            .stage(id, path.clone(), "content".into(), 0)
            .expect("ok");

        let err = commit_with_default_policy(&mut store, id, None)
            .await
            .expect_err("should fail");
        match err {
            TransactionError::ValidationFailed { detail, .. } => {
                assert!(
                    detail.contains("check failed"),
                    "detail should contain stderr: {detail}",
                );
            }
            other => panic!("expected ValidationFailed, got: {other}"),
        }
    }

    #[tokio::test]
    async fn commit_creates_parent_directories() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "deep/nested/dir/file.txt");

        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, path.clone(), "nested content".into(), 0)
            .expect("ok");

        commit_with_default_policy(&mut store, id, None)
            .await
            .expect("commit ok");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert_eq!(content, "nested content");
    }

    #[tokio::test]
    async fn commit_non_open_transaction_fails() {
        let dir = TempDir::new().expect("tempdir");
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, temp_path(&dir, "a.txt"), "a".into(), 0)
            .expect("ok");
        store.cancel(id).expect("cancel");

        let err = commit_with_default_policy(&mut store, id, None)
            .await
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::InvalidState { .. }));
    }

    #[tokio::test]
    async fn commit_deny_tier_path_aborts_before_writes() {
        let dir = TempDir::new().expect("tempdir");
        // Use *.dat (not *.key) to avoid ALWAYS_PROPOSE_PATTERNS override.
        let denied_path = temp_path(&dir, "secret.dat");

        let mut store = TransactionStore::new();
        let id = store.begin("deny".into(), None, 0).expect("begin ok");
        store
            .stage(id, denied_path.clone(), "blocked".into(), 0)
            .expect("stage ok");

        let self_modify = strict_self_modify(&["**"], &["*.dat"]);
        let err = commit(&mut store, id, None, &self_modify, dir.path())
            .await
            .expect_err("deny-tier path must fail commit");
        assert!(matches!(
            err,
            TransactionError::SelfModifyPathViolation {
                path,
                tier: PathTier::Deny,
                ..
            } if path == denied_path
        ));
        assert!(!denied_path.exists(), "deny-tier file must not be written");
        assert_eq!(store.status(id).expect("status"), TransactionStatus::Open);
    }

    #[tokio::test]
    async fn commit_allow_tier_paths_still_commit() {
        let dir = TempDir::new().expect("tempdir");
        let allow_path = temp_path(&dir, "src/lib.rs");

        let mut store = TransactionStore::new();
        let id = store.begin("allow".into(), None, 0).expect("begin ok");
        store
            .stage(id, allow_path.clone(), "pub fn ok() {}".into(), 0)
            .expect("stage ok");

        let self_modify = strict_self_modify(&["src/**"], &["*.key"]);
        let count = commit(&mut store, id, None, &self_modify, dir.path())
            .await
            .expect("allow-tier commit must succeed");

        assert_eq!(count, 1);
        assert_eq!(
            store.status(id).expect("status"),
            TransactionStatus::Committed,
        );
        let content = tokio::fs::read_to_string(&allow_path)
            .await
            .expect("read allow");
        assert_eq!(content, "pub fn ok() {}");
    }

    #[tokio::test]
    async fn commit_mixed_paths_with_deny_tier_aborts_atomically() {
        let dir = TempDir::new().expect("tempdir");
        let allow_path = temp_path(&dir, "src/main.rs");
        // Use *.dat (not *.key) to avoid ALWAYS_PROPOSE_PATTERNS override.
        let denied_path = temp_path(&dir, "secret.dat");
        tokio::fs::create_dir_all(allow_path.parent().expect("parent"))
            .await
            .expect("mkdir");
        tokio::fs::write(&allow_path, "original")
            .await
            .expect("seed");

        let mut store = TransactionStore::new();
        let id = store.begin("mixed".into(), None, 0).expect("begin ok");
        store
            .stage(id, allow_path.clone(), "updated".into(), 0)
            .expect("stage allow");
        store
            .stage(id, denied_path.clone(), "blocked".into(), 0)
            .expect("stage deny");

        let self_modify = strict_self_modify(&["**"], &["*.dat"]);
        let err = commit(&mut store, id, None, &self_modify, dir.path())
            .await
            .expect_err("mixed commit must fail when one path is deny-tier");
        assert!(matches!(
            err,
            TransactionError::SelfModifyPathViolation {
                path,
                tier: PathTier::Deny,
                ..
            } if path == denied_path
        ));

        let allow_content = tokio::fs::read_to_string(&allow_path)
            .await
            .expect("read allow");
        assert_eq!(allow_content, "original");
        assert!(!denied_path.exists(), "deny-tier file must not be written");
        let tx = store.get(id).expect("tx");
        assert!(
            tx.staged.iter().all(|write| write.original.is_none()),
            "pre-write policy failure should not snapshot originals",
        );
        assert_eq!(store.status(id).expect("status"), TransactionStatus::Open);
    }

    #[tokio::test]
    async fn explicit_rollback_on_open_transaction() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "rollback_test.txt");
        tokio::fs::write(&path, "original").await.expect("write");

        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, path.clone(), "staged but not committed".into(), 0)
            .expect("ok");

        // Explicit rollback on un-committed transaction is a no-op on disk
        let errors = rollback(&mut store, id).await.expect("rollback ok");
        assert!(errors.is_empty());

        assert_eq!(
            store.status(id).expect("status"),
            TransactionStatus::RolledBack,
        );

        // File must still exist with original content (B2 regression)
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert_eq!(content, "original");
    }

    #[tokio::test]
    async fn rollback_non_open_transaction_fails() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store.cancel(id).expect("cancel");

        let err = rollback(&mut store, id).await.expect_err("should fail");
        assert!(matches!(err, TransactionError::InvalidState { .. }));
    }

    #[tokio::test]
    async fn multiple_concurrent_transactions_do_not_interfere() {
        let dir = TempDir::new().expect("tempdir");
        let mut store = TransactionStore::new();

        let id1 = store.begin("tx1".into(), None, 0).expect("begin ok");
        let id2 = store.begin("tx2".into(), None, 0).expect("begin ok");

        store
            .stage(id1, temp_path(&dir, "file1.txt"), "content1".into(), 0)
            .expect("ok");
        store
            .stage(id2, temp_path(&dir, "file2.txt"), "content2".into(), 0)
            .expect("ok");

        commit_with_default_policy(&mut store, id1, None)
            .await
            .expect("commit tx1");
        commit_with_default_policy(&mut store, id2, None)
            .await
            .expect("commit tx2");

        let c1 = tokio::fs::read_to_string(temp_path(&dir, "file1.txt"))
            .await
            .expect("read");
        let c2 = tokio::fs::read_to_string(temp_path(&dir, "file2.txt"))
            .await
            .expect("read");
        assert_eq!(c1, "content1");
        assert_eq!(c2, "content2");

        assert_eq!(
            store.status(id1).expect("status"),
            TransactionStatus::Committed,
        );
        assert_eq!(
            store.status(id2).expect("status"),
            TransactionStatus::Committed,
        );
    }

    #[tokio::test]
    async fn validation_command_uses_work_dir() {
        let dir = TempDir::new().expect("tempdir");
        let marker = temp_path(&dir, "marker.txt");
        let path = temp_path(&dir, "file.txt");

        let mut store = TransactionStore::new();
        // Validation command creates a marker file in the work dir
        let cmd = format!("touch {}", marker.display());
        let id = store.begin("test".into(), Some(cmd), 0).expect("begin ok");
        store.stage(id, path, "content".into(), 0).expect("ok");

        commit_with_default_policy(&mut store, id, Some(dir.path()))
            .await
            .expect("commit ok");
        assert!(marker.exists(), "marker should exist from validation cmd");
    }

    #[test]
    fn format_validation_detail_with_both_streams() {
        let output = CommandOutput {
            exit_code: 1,
            stdout: "out\n".to_string(),
            stderr: "err\n".to_string(),
        };
        let detail = format_validation_detail(&output);
        assert!(detail.contains("stdout: out"));
        assert!(detail.contains("stderr: err"));
    }

    #[test]
    fn format_validation_detail_empty_output() {
        let output = CommandOutput {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        };
        let detail = format_validation_detail(&output);
        assert_eq!(detail, "(no output)");
    }

    #[tokio::test]
    async fn rollback_of_new_file_deletes_it() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "new_file.txt");

        let mut store = TransactionStore::new();
        // Use a failing validation to trigger automatic rollback
        let id = store
            .begin("test".into(), Some("false".into()), 0)
            .expect("begin ok");
        store
            .stage(id, path.clone(), "ephemeral".into(), 0)
            .expect("ok");

        let err = commit_with_default_policy(&mut store, id, None)
            .await
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::ValidationFailed { .. }));

        // File should not exist after rollback (it was new)
        assert!(!path.exists(), "new file should be deleted on rollback");
    }

    #[tokio::test]
    async fn overwrite_existing_file_and_rollback_restores() {
        let dir = TempDir::new().expect("tempdir");
        let path = temp_path(&dir, "existing.txt");
        tokio::fs::write(&path, "before").await.expect("write");

        let mut store = TransactionStore::new();
        let id = store
            .begin("test".into(), Some("false".into()), 0)
            .expect("begin ok");
        store
            .stage(id, path.clone(), "after".into(), 0)
            .expect("ok");

        let err = commit_with_default_policy(&mut store, id, None)
            .await
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::ValidationFailed { .. }));

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert_eq!(content, "before");
    }

    #[tokio::test]
    async fn snapshot_failure_aborts_before_writes() {
        // Test that if we can't read a file (e.g., permission error),
        // no writes are performed. We simulate this by staging a path
        // that's a directory (reading a directory as a string fails).
        let dir = TempDir::new().expect("tempdir");
        let dir_path = temp_path(&dir, "subdir");
        tokio::fs::create_dir(&dir_path).await.expect("mkdir");

        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store.stage(id, dir_path, "content".into(), 0).expect("ok");

        // Also stage a normal file to verify it's NOT written
        let normal = temp_path(&dir, "normal.txt");
        store
            .stage(id, normal.clone(), "should not exist".into(), 0)
            .expect("ok");

        let err = commit_with_default_policy(&mut store, id, None)
            .await
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::SnapshotFailed { .. }));
        assert!(!normal.exists(), "normal file should not be written");

        // Transaction should still be Open (snapshot failed before any state change)
        assert_eq!(store.status(id).expect("status"), TransactionStatus::Open);
    }

    #[tokio::test]
    async fn rollback_uncommitted_preserves_existing_files() {
        let dir = TempDir::new().expect("tempdir");
        let path_a = temp_path(&dir, "existing_a.txt");
        let path_b = temp_path(&dir, "existing_b.txt");
        tokio::fs::write(&path_a, "content a").await.expect("write");
        tokio::fs::write(&path_b, "content b").await.expect("write");

        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, path_a.clone(), "new a".into(), 0)
            .expect("ok");
        store
            .stage(id, path_b.clone(), "new b".into(), 0)
            .expect("ok");

        // Rollback without commit — files must not be touched
        let errors = rollback(&mut store, id).await.expect("rollback ok");
        assert!(errors.is_empty());

        // Both files should still exist with original content
        let a = tokio::fs::read_to_string(&path_a).await.expect("read a");
        let b = tokio::fs::read_to_string(&path_b).await.expect("read b");
        assert_eq!(a, "content a");
        assert_eq!(b, "content b");
    }
}
