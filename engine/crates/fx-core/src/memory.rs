//! Persistent agent memory trait.
//!
//! The kernel defines this contract; implementations live in dedicated
//! implementation crates (e.g. `JsonFileMemory` in fx-memory).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Where a memory entry originated.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    #[default]
    User,
    SignalAnalysis,
    Consolidation,
}

impl fmt::Display for MemorySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::SignalAnalysis => write!(f, "signal_analysis"),
            Self::Consolidation => write!(f, "consolidation"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryEntry {
    pub value: String,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub last_accessed_at_ms: u64,
    #[serde(default)]
    pub access_count: u32,
    #[serde(default)]
    pub source: MemorySource,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Trait for persistent agent memory.
///
/// Kernel defines the contract; implementations live in the loadable layer.
pub trait MemoryProvider: Send + Sync + std::fmt::Debug {
    /// Read a value by key. Returns `None` if not found.
    fn read(&self, key: &str) -> Option<String>;

    /// Write a key-value pair. Overwrites if key exists.
    fn write(&mut self, key: &str, value: &str) -> Result<(), String>;

    /// List all key-value pairs, sorted by key.
    fn list(&self) -> Vec<(String, String)>;

    /// Delete a key. Returns `true` if it existed.
    fn delete(&mut self, key: &str) -> bool;

    /// Search keys and values by substring query.
    fn search(&self, query: &str) -> Vec<(String, String)>;

    /// Search memories by relevance to a query string.
    ///
    /// Implementors can override this for custom relevance ranking. The default
    /// behavior falls back to `search()` and truncates to `max_results`.
    fn search_relevant(&self, query: &str, max_results: usize) -> Vec<(String, String)> {
        let results = self.search(query);
        results.into_iter().take(max_results).collect()
    }

    /// Return all entries for system prompt injection.
    ///
    /// Sort contract: entries are ordered by descending `access_count`,
    /// with ties broken by ascending key name. This puts the most-accessed
    /// memories first in the prompt context window.
    fn snapshot(&self) -> Vec<(String, String)>;
}

/// Optional metadata operations for memory providers.
///
/// `touch()` is intentionally separate from `MemoryProvider::read()` because
/// `read()` takes `&self` (no mutation needed for value retrieval) while `touch()`
/// requires `&mut self` to update access timestamps. This keeps the read path
/// non-mutating for callers that only need the value (e.g. `snapshot()`, `list()`).
///
/// Callers that want read-with-tracking should use `MemoryStore` and call
/// `touch()` after `read()` — see `FawxToolExecutor::handle_memory_read` for
/// the canonical pattern.
pub trait MemoryTouchProvider: Send + Sync + std::fmt::Debug {
    /// Bump access metadata (last-accessed timestamp, access count) for a key.
    ///
    /// No-op if the key does not exist. Callers should call this after
    /// `MemoryProvider::read()` when access tracking is desired.
    fn touch(&mut self, key: &str) -> Result<(), String>;
}

/// Combined trait object used by shells that need both value access and metadata touch.
pub trait MemoryStore: MemoryProvider + MemoryTouchProvider {}

impl<T> MemoryStore for T where T: MemoryProvider + MemoryTouchProvider {}
