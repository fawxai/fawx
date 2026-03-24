//! Transaction error types.

use std::path::PathBuf;

use fx_core::self_modify::PathTier;

/// Errors produced by transaction store and executor operations.
#[derive(Debug, thiserror::Error)]
pub enum TransactionError {
    /// Transaction IDs exhausted `u32` range.
    #[error("transaction ID overflow")]
    IdOverflow,

    /// Transaction with the given ID was not found.
    #[error("transaction #{0} not found")]
    NotFound(u32),

    /// Operation is invalid for the transaction's current status.
    #[error("transaction #{id} is {status}, expected {expected}")]
    InvalidState {
        id: u32,
        status: String,
        expected: String,
    },

    /// Attempted to commit a transaction with no staged writes.
    #[error("transaction #{0} has no staged writes")]
    EmptyTransaction(u32),

    /// Staged path violates self-modification path-tier policy during commit prechecks.
    #[error("transaction #{tx_id} self-modify policy violation [{tier:?}] for {path}: {detail}")]
    SelfModifyPathViolation {
        tx_id: u32,
        path: PathBuf,
        tier: PathTier,
        detail: String,
    },

    /// Failed to read a file during commit snapshot.
    #[error("failed to read {path}: {reason}")]
    SnapshotFailed { path: PathBuf, reason: String },

    /// Failed to write a file during commit.
    #[error("failed to write {path}: {reason}")]
    WriteFailed { path: PathBuf, reason: String },

    /// Validation command failed after writes were applied (rollback triggered).
    #[error("validation failed (exit {exit_code}): {detail}")]
    ValidationFailed {
        exit_code: i32,
        detail: String,
        rollback_errors: Vec<String>,
    },

    /// One or more files could not be rolled back.
    #[error("rollback encountered errors: {}", errors.join("; "))]
    RollbackPartial { errors: Vec<String> },

    /// Failed to create parent directories for a staged file.
    #[error("failed to create directory for {path}: {reason}")]
    DirectoryCreationFailed { path: PathBuf, reason: String },

    /// File or transaction size exceeded configured limits.
    #[error("{target} exceeds size limit: {actual_bytes} bytes > {max_bytes} bytes")]
    SizeLimit {
        target: String,
        actual_bytes: usize,
        max_bytes: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn not_found_displays_id() {
        let err = TransactionError::NotFound(42);
        assert_eq!(err.to_string(), "transaction #42 not found");
    }

    #[test]
    fn id_overflow_displays_message() {
        let err = TransactionError::IdOverflow;
        assert_eq!(err.to_string(), "transaction ID overflow");
    }

    #[test]
    fn invalid_state_displays_details() {
        let err = TransactionError::InvalidState {
            id: 1,
            status: "Committed".to_string(),
            expected: "Open".to_string(),
        };
        assert!(err.to_string().contains("Committed"));
        assert!(err.to_string().contains("Open"));
    }

    #[test]
    fn empty_transaction_displays_id() {
        let err = TransactionError::EmptyTransaction(5);
        assert!(err.to_string().contains("#5"));
    }

    #[test]
    fn self_modify_violation_displays_path_and_tier() {
        let err = TransactionError::SelfModifyPathViolation {
            tx_id: 7,
            path: PathBuf::from("secret.key"),
            tier: PathTier::Deny,
            detail: "policy violation".to_string(),
        };
        let message = err.to_string();
        assert!(message.contains("#7"));
        assert!(message.contains("secret.key"));
        assert!(message.contains("Deny"));
    }

    #[test]
    fn snapshot_failed_displays_path() {
        let err = TransactionError::SnapshotFailed {
            path: PathBuf::from("/tmp/foo.rs"),
            reason: "permission denied".to_string(),
        };
        assert!(err.to_string().contains("/tmp/foo.rs"));
        assert!(err.to_string().contains("permission denied"));
    }

    #[test]
    fn validation_failed_displays_exit_code() {
        let err = TransactionError::ValidationFailed {
            exit_code: 1,
            detail: "cargo check failed".to_string(),
            rollback_errors: vec![],
        };
        assert!(err.to_string().contains("exit 1"));
    }

    #[test]
    fn rollback_partial_joins_errors() {
        let err = TransactionError::RollbackPartial {
            errors: vec!["file a".to_string(), "file b".to_string()],
        };
        let msg = err.to_string();
        assert!(msg.contains("file a"));
        assert!(msg.contains("file b"));
    }

    #[test]
    fn directory_creation_failed_displays_path() {
        let err = TransactionError::DirectoryCreationFailed {
            path: PathBuf::from("/tmp/deep/nested"),
            reason: "permission denied".to_string(),
        };
        assert!(err.to_string().contains("/tmp/deep/nested"));
        assert!(err.to_string().contains("permission denied"));
    }

    #[test]
    fn size_limit_displays_details() {
        let err = TransactionError::SizeLimit {
            target: "file src/main.rs".to_string(),
            actual_bytes: 1_100_000,
            max_bytes: 1_048_576,
        };
        let message = err.to_string();
        assert!(message.contains("file src/main.rs"));
        assert!(message.contains("1100000 bytes"));
        assert!(message.contains("1048576 bytes"));
    }
}
