//! Fawx memory — JSON file-backed memory and JSONL signal persistence.
pub mod embedding_index;
pub mod json_memory;
pub mod signal_store;

pub use json_memory::{DecayConfig, JsonFileMemory, JsonMemoryConfig};
pub use signal_store::{SignalQuery, SignalStore};
