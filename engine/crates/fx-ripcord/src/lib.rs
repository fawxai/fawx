//! Tripwire monitoring and ripcord rollback for AX-first security.
//!
//! Provides journaled monitoring that activates when tool actions cross
//! defined tripwires. Users can pull the ripcord to atomically revert
//! file and git operations since the crossing point.

pub mod config;
pub mod journal;
pub mod snapshot;

pub use config::{TripwireConfig, TripwireKind};
pub use journal::{JournalAction, JournalEntry, RipcordJournal, RipcordStatus};
pub use snapshot::SnapshotStore;
