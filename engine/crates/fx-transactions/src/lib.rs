//! # fx-transactions — Multi-file edit transactions
//!
//! Provides atomic multi-file write support for Fawx: batch file changes,
//! validate together, and commit all or rollback all.
//!
//! ## Modules
//!
//! - **error** — `TransactionError` enum
//! - **store** — In-memory `TransactionStore` (pure data, no I/O)
//! - **executor** — Commit and rollback logic with filesystem I/O

pub mod error;
pub mod executor;
pub mod store;
