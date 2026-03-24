//! # fx-journal — Reflective Memory for Fawx
//!
//! A journal system for cross-session learning. The model captures lessons
//! learned during work via `journal_write`, then queries them in future
//! sessions via `journal_search`.
//!
//! ## Design Principle: Discovered, Not Forced
//!
//! The journal is available as a tool — the model decides when something
//! is worth recording. No forced post-task reflection, no mandatory
//! classification. Selective writing produces high-signal entries;
//! obligatory writing produces noise.
//!
//! ## Storage
//!
//! JSONL format (one JSON object per line), append-only writes.
//! Loaded into memory on startup. Simple, debuggable, grep-friendly.

pub mod error;
pub mod flush;
pub mod journal;
pub mod skill;

pub use error::JournalError;
pub use flush::JournalCompactionFlush;
pub use journal::{Journal, JournalEntry};
pub use skill::JournalSkill;
