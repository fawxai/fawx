//! In-memory transaction store.
//!
//! Pure data structures and state management — no filesystem I/O.
//! Thread safety is handled by the caller wrapping the store in
//! `Arc<Mutex<TransactionStore>>`.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::TransactionError;

const MAX_STAGED_FILE_BYTES: usize = 1_024 * 1_024;
const MAX_TRANSACTION_BYTES: usize = 10 * MAX_STAGED_FILE_BYTES;

/// A single file write staged within a transaction.
#[derive(Debug, Clone)]
pub struct StagedWrite {
    /// Target file path (absolute or repo-relative).
    pub path: PathBuf,
    /// Original file content for rollback. `None` if the file did not exist.
    pub original: Option<String>,
    /// New content to write.
    pub content: String,
    /// Iteration number when this write was staged.
    pub staged_at: u32,
}

/// Lifecycle status of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    /// Accepting staged writes.
    Open,
    /// All writes applied and validated.
    Committed,
    /// Writes were rolled back (validation failure or explicit rollback).
    RolledBack,
    /// Cancelled without attempting any writes.
    Cancelled,
}

impl std::fmt::Display for TransactionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "Open"),
            Self::Committed => write!(f, "Committed"),
            Self::RolledBack => write!(f, "RolledBack"),
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// A single transaction grouping one or more staged file writes.
#[derive(Debug)]
pub struct Transaction {
    pub(crate) id: u32,
    pub(crate) label: String,
    pub(crate) status: TransactionStatus,
    pub(crate) staged: Vec<StagedWrite>,
    pub(crate) validation_command: Option<String>,
    pub(crate) created_at_iteration: u32,
}

impl Transaction {
    /// Transaction identifier assigned by the store.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// User-provided label describing the transaction.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Current lifecycle status.
    pub fn status(&self) -> TransactionStatus {
        self.status
    }

    /// Staged writes currently tracked by this transaction.
    pub fn staged(&self) -> &[StagedWrite] {
        &self.staged
    }

    /// Optional validation command to run on commit.
    pub fn validation_command(&self) -> Option<&str> {
        self.validation_command.as_deref()
    }

    /// Iteration in which this transaction was created.
    pub fn created_at_iteration(&self) -> u32 {
        self.created_at_iteration
    }
}

/// In-memory store for all transactions in the current session.
#[derive(Debug, Default)]
pub struct TransactionStore {
    transactions: HashMap<u32, Transaction>,
    next_id: u32,
}

impl TransactionStore {
    /// Create a new, empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new transaction. Returns the assigned transaction ID.
    pub fn begin(
        &mut self,
        label: String,
        validation_command: Option<String>,
        iteration: u32,
    ) -> Result<u32, TransactionError> {
        let id = self.next_id;
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or(TransactionError::IdOverflow)?;
        self.next_id = next_id;
        self.transactions.insert(
            id,
            Transaction {
                id,
                label,
                status: TransactionStatus::Open,
                staged: Vec::new(),
                validation_command,
                created_at_iteration: iteration,
            },
        );
        Ok(id)
    }

    /// Stage a file write in an open transaction.
    ///
    /// If the same path is already staged, the previous entry is replaced
    /// (last-write-wins semantics).
    pub fn stage(
        &mut self,
        tx_id: u32,
        path: PathBuf,
        content: String,
        iteration: u32,
    ) -> Result<(), TransactionError> {
        let tx = self
            .transactions
            .get_mut(&tx_id)
            .ok_or(TransactionError::NotFound(tx_id))?;

        if tx.status != TransactionStatus::Open {
            return Err(TransactionError::InvalidState {
                id: tx_id,
                status: tx.status.to_string(),
                expected: "Open".to_string(),
            });
        }

        validate_file_size(&path, &content)?;
        validate_transaction_size(tx, tx_id, &path, &content)?;

        // Last-write-wins for duplicate paths
        if let Some(existing) = tx.staged.iter_mut().find(|w| w.path == path) {
            existing.content = content;
            existing.staged_at = iteration;
            return Ok(());
        }

        tx.staged.push(StagedWrite {
            path,
            original: None,
            content,
            staged_at: iteration,
        });

        Ok(())
    }

    /// Return references to the staged writes in a transaction.
    pub fn staged_files(&self, tx_id: u32) -> Result<Vec<&StagedWrite>, TransactionError> {
        let tx = self
            .transactions
            .get(&tx_id)
            .ok_or(TransactionError::NotFound(tx_id))?;
        Ok(tx.staged.iter().collect())
    }

    /// Get a transaction's current status.
    pub fn status(&self, tx_id: u32) -> Result<TransactionStatus, TransactionError> {
        self.transactions
            .get(&tx_id)
            .map(|tx| tx.status)
            .ok_or(TransactionError::NotFound(tx_id))
    }

    /// Mark a transaction as committed. Must be `Open`.
    pub fn mark_committed(&mut self, tx_id: u32) -> Result<(), TransactionError> {
        self.transition(tx_id, TransactionStatus::Open, TransactionStatus::Committed)
    }

    /// Mark a transaction as rolled back. Must be `Open`.
    pub fn mark_rolled_back(&mut self, tx_id: u32) -> Result<(), TransactionError> {
        self.transition(
            tx_id,
            TransactionStatus::Open,
            TransactionStatus::RolledBack,
        )
    }

    /// Cancel an open transaction without applying writes.
    pub fn cancel(&mut self, tx_id: u32) -> Result<(), TransactionError> {
        self.transition(tx_id, TransactionStatus::Open, TransactionStatus::Cancelled)
    }

    /// List all transactions as `(id, label, status, staged_count)`.
    pub fn list(&self) -> Vec<(u32, &str, TransactionStatus, usize)> {
        let mut entries: Vec<_> = self
            .transactions
            .values()
            .map(|tx| (tx.id, tx.label.as_str(), tx.status, tx.staged.len()))
            .collect();
        entries.sort_by_key(|(id, _, _, _)| *id);
        entries
    }

    /// Get a mutable reference to a transaction (used by the executor
    /// to update `original` fields during commit snapshot).
    pub fn get_mut(&mut self, tx_id: u32) -> Result<&mut Transaction, TransactionError> {
        self.transactions
            .get_mut(&tx_id)
            .ok_or(TransactionError::NotFound(tx_id))
    }

    /// Get an immutable reference to a transaction.
    pub fn get(&self, tx_id: u32) -> Result<&Transaction, TransactionError> {
        self.transactions
            .get(&tx_id)
            .ok_or(TransactionError::NotFound(tx_id))
    }

    /// Transition a transaction from one status to another.
    fn transition(
        &mut self,
        tx_id: u32,
        expected: TransactionStatus,
        target: TransactionStatus,
    ) -> Result<(), TransactionError> {
        let tx = self
            .transactions
            .get_mut(&tx_id)
            .ok_or(TransactionError::NotFound(tx_id))?;

        if tx.status != expected {
            return Err(TransactionError::InvalidState {
                id: tx_id,
                status: tx.status.to_string(),
                expected: expected.to_string(),
            });
        }

        tx.status = target;
        Ok(())
    }
}

fn validate_file_size(path: &std::path::Path, content: &str) -> Result<(), TransactionError> {
    let file_bytes = content.len();
    if file_bytes > MAX_STAGED_FILE_BYTES {
        return Err(TransactionError::SizeLimit {
            target: format!("file {}", path.display()),
            actual_bytes: file_bytes,
            max_bytes: MAX_STAGED_FILE_BYTES,
        });
    }

    Ok(())
}

fn validate_transaction_size(
    tx: &Transaction,
    tx_id: u32,
    path: &std::path::Path,
    content: &str,
) -> Result<(), TransactionError> {
    let current_total_bytes = total_staged_bytes(tx);
    let existing_bytes = tx
        .staged
        .iter()
        .find(|write| write.path == path)
        .map_or(0, |write| write.content.len());

    let prospective_total_bytes = current_total_bytes
        .saturating_sub(existing_bytes)
        .saturating_add(content.len());

    if prospective_total_bytes > MAX_TRANSACTION_BYTES {
        return Err(TransactionError::SizeLimit {
            target: format!("transaction #{tx_id}"),
            actual_bytes: prospective_total_bytes,
            max_bytes: MAX_TRANSACTION_BYTES,
        });
    }

    Ok(())
}

fn total_staged_bytes(tx: &Transaction) -> usize {
    tx.staged.iter().map(|write| write.content.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn begin_returns_monotonic_ids() {
        let mut store = TransactionStore::new();
        let id1 = store.begin("first".into(), None, 0).expect("begin ok");
        let id2 = store.begin("second".into(), None, 0).expect("begin ok");
        let id3 = store.begin("third".into(), None, 0).expect("begin ok");
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id3, 2);
    }

    #[test]
    fn begin_overflow_returns_error() {
        let mut store = TransactionStore::new();
        store.next_id = u32::MAX;

        let err = store
            .begin("overflow".into(), None, 0)
            .expect_err("should fail on overflow");

        assert!(matches!(err, TransactionError::IdOverflow));
        assert!(store.list().is_empty(), "overflow should not insert tx");
    }

    #[test]
    fn begin_sets_correct_fields() {
        let mut store = TransactionStore::new();
        let id = store
            .begin("refactor".into(), Some("cargo check".into()), 5)
            .expect("begin ok");
        let tx = store.get(id).expect("transaction exists");
        assert_eq!(tx.label(), "refactor");
        assert_eq!(tx.status(), TransactionStatus::Open);
        assert_eq!(tx.validation_command(), Some("cargo check"));
        assert_eq!(tx.created_at_iteration(), 5);
        assert!(tx.staged().is_empty());
    }

    #[test]
    fn stage_to_open_transaction_succeeds() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, PathBuf::from("src/main.rs"), "fn main() {}".into(), 1)
            .expect("stage succeeds");

        let files = store.staged_files(id).expect("staged files");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(files[0].content, "fn main() {}");
        assert_eq!(files[0].staged_at, 1);
        assert!(files[0].original.is_none());
    }

    #[test]
    fn stage_multiple_files() {
        let mut store = TransactionStore::new();
        let id = store.begin("multi".into(), None, 0).expect("begin ok");
        store
            .stage(id, PathBuf::from("a.rs"), "a".into(), 1)
            .expect("ok");
        store
            .stage(id, PathBuf::from("b.rs"), "b".into(), 2)
            .expect("ok");
        store
            .stage(id, PathBuf::from("c.rs"), "c".into(), 3)
            .expect("ok");

        let files = store.staged_files(id).expect("staged");
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn stage_rejects_file_larger_than_limit() {
        let mut store = TransactionStore::new();
        let id = store.begin("large-file".into(), None, 0).expect("begin ok");
        let oversized_content = "x".repeat(MAX_STAGED_FILE_BYTES + 1);

        let err = store
            .stage(id, PathBuf::from("oversized.txt"), oversized_content, 1)
            .expect_err("should reject oversized file");

        assert!(matches!(
            err,
            TransactionError::SizeLimit {
                max_bytes: MAX_STAGED_FILE_BYTES,
                ..
            }
        ));
    }

    #[test]
    fn stage_rejects_transaction_larger_than_limit() {
        let mut store = TransactionStore::new();
        let id = store
            .begin("large-transaction".into(), None, 0)
            .expect("begin ok");
        let one_megabyte = "x".repeat(MAX_STAGED_FILE_BYTES);

        for index in 0..10 {
            let path = PathBuf::from(format!("file-{index}.txt"));
            store
                .stage(id, path, one_megabyte.clone(), index)
                .expect("within transaction limit");
        }

        let err = store
            .stage(id, PathBuf::from("overflow.txt"), "y".to_string(), 11)
            .expect_err("should reject transaction over limit");

        assert!(matches!(
            err,
            TransactionError::SizeLimit {
                max_bytes: MAX_TRANSACTION_BYTES,
                ..
            }
        ));
    }

    #[test]
    fn stage_to_committed_transaction_fails() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, PathBuf::from("a.rs"), "a".into(), 0)
            .expect("ok");
        store.mark_committed(id).expect("commit ok");

        let err = store
            .stage(id, PathBuf::from("b.rs"), "b".into(), 1)
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::InvalidState { .. }));
    }

    #[test]
    fn stage_to_cancelled_transaction_fails() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store.cancel(id).expect("cancel ok");

        let err = store
            .stage(id, PathBuf::from("a.rs"), "a".into(), 0)
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::InvalidState { .. }));
    }

    #[test]
    fn stage_to_rolled_back_transaction_fails() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store.mark_rolled_back(id).expect("rollback ok");

        let err = store
            .stage(id, PathBuf::from("a.rs"), "a".into(), 0)
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::InvalidState { .. }));
    }

    #[test]
    fn stage_to_nonexistent_transaction_fails() {
        let mut store = TransactionStore::new();
        let err = store
            .stage(99, PathBuf::from("a.rs"), "a".into(), 0)
            .expect_err("should fail");
        assert!(matches!(err, TransactionError::NotFound(99)));
    }

    #[test]
    fn cancel_open_transaction_succeeds() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store.cancel(id).expect("cancel ok");
        assert_eq!(
            store.status(id).expect("status"),
            TransactionStatus::Cancelled,
        );
    }

    #[test]
    fn cancel_committed_transaction_fails() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, PathBuf::from("a.rs"), "a".into(), 0)
            .expect("ok");
        store.mark_committed(id).expect("commit ok");

        let err = store.cancel(id).expect_err("should fail");
        assert!(matches!(err, TransactionError::InvalidState { .. }));
    }

    #[test]
    fn mark_committed_on_open_succeeds() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, PathBuf::from("a.rs"), "a".into(), 0)
            .expect("ok");
        store.mark_committed(id).expect("commit ok");
        assert_eq!(
            store.status(id).expect("status"),
            TransactionStatus::Committed,
        );
    }

    #[test]
    fn mark_committed_on_cancelled_fails() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store.cancel(id).expect("cancel ok");

        let err = store.mark_committed(id).expect_err("should fail");
        assert!(matches!(err, TransactionError::InvalidState { .. }));
    }

    #[test]
    fn mark_rolled_back_on_open_succeeds() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store.mark_rolled_back(id).expect("rollback ok");
        assert_eq!(
            store.status(id).expect("status"),
            TransactionStatus::RolledBack,
        );
    }

    #[test]
    fn status_of_nonexistent_returns_not_found() {
        let store = TransactionStore::new();
        let err = store.status(42).expect_err("should fail");
        assert!(matches!(err, TransactionError::NotFound(42)));
    }

    #[test]
    fn list_returns_all_transactions_sorted() {
        let mut store = TransactionStore::new();
        let id1 = store.begin("alpha".into(), None, 0).expect("begin ok");
        let id2 = store.begin("beta".into(), None, 1).expect("begin ok");
        store
            .stage(id1, PathBuf::from("a.rs"), "a".into(), 0)
            .expect("ok");
        store
            .stage(id1, PathBuf::from("b.rs"), "b".into(), 0)
            .expect("ok");
        store.cancel(id2).expect("cancel ok");

        let entries = store.list();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], (id1, "alpha", TransactionStatus::Open, 2));
        assert_eq!(entries[1], (id2, "beta", TransactionStatus::Cancelled, 0));
    }

    #[test]
    fn list_empty_store() {
        let store = TransactionStore::new();
        assert!(store.list().is_empty());
    }

    #[test]
    fn staged_files_on_nonexistent_returns_not_found() {
        let store = TransactionStore::new();
        let err = store.staged_files(0).expect_err("should fail");
        assert!(matches!(err, TransactionError::NotFound(0)));
    }

    #[test]
    fn get_returns_transaction() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        let tx = store.get(id).expect("found");
        assert_eq!(tx.id(), id);
    }

    #[test]
    fn get_mut_returns_mutable_transaction() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        let tx = store.get_mut(id).expect("found");
        tx.label = "modified".to_string();
        assert_eq!(store.get(id).expect("found").label(), "modified");
    }

    #[test]
    fn double_commit_fails() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        store
            .stage(id, PathBuf::from("a.rs"), "a".into(), 0)
            .expect("ok");
        store.mark_committed(id).expect("first commit ok");

        let err = store.mark_committed(id).expect_err("should fail");
        assert!(matches!(err, TransactionError::InvalidState { .. }));
    }

    #[test]
    fn duplicate_path_staging_uses_last_write() {
        let mut store = TransactionStore::new();
        let id = store.begin("test".into(), None, 0).expect("begin ok");
        let path = PathBuf::from("src/main.rs");

        store
            .stage(id, path.clone(), "first version".into(), 1)
            .expect("ok");
        store
            .stage(id, path.clone(), "second version".into(), 2)
            .expect("ok");

        let files = store.staged_files(id).expect("staged");
        assert_eq!(files.len(), 1, "duplicate path should be deduplicated");
        assert_eq!(files[0].content, "second version");
        assert_eq!(files[0].staged_at, 2);
    }
}
