//! Fawx memory — JSON file-backed memory and JSONL signal persistence.
pub mod analysis;
pub mod json_memory;
pub mod signal_store;

pub use analysis::{AnalysisFinding, Confidence, SignalEvidence};
pub use json_memory::{JsonFileMemory, JsonMemoryConfig};
pub use signal_store::{SignalQuery, SignalStore};
