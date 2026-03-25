//! Tripwire monitoring and ripcord rollback for AX-first security.
//!
//! Provides journaled monitoring that activates when tool actions cross
//! defined tripwires. Users can pull the ripcord to atomically revert
//! file and git operations since the crossing point.

pub mod config;
pub mod evaluator;
pub mod git_guard;
pub mod journal;
pub mod revert;
pub mod snapshot;

pub use config::{resolve_tripwires, TripwireConfig, TripwireKind};
pub use evaluator::{TripwireEvaluator, TripwireNotifyFn};
pub use journal::{JournalAction, JournalEntry, RipcordJournal, RipcordStatus};
pub use revert::{approve_ripcord, pull_ripcord, RevertedEntry, RipcordReport, SkippedEntry};
pub use snapshot::SnapshotStore;
