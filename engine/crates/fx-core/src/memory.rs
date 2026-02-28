//! Persistent agent memory trait.
//!
//! The kernel defines this contract; implementations live in the loadable layer
//! (e.g. `JsonFileMemory` in fx-cli).

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

    /// Snapshot for system prompt injection.
    fn snapshot(&self) -> Vec<(String, String)>;
}
