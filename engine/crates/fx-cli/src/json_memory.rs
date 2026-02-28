//! JSON file-backed persistent memory.

use fx_core::memory::MemoryProvider;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 1000;
const MAX_VALUE_SIZE: usize = 10240; // 10 KB

#[derive(Debug)]
pub struct JsonFileMemory {
    path: PathBuf,
    data: HashMap<String, String>,
}

impl JsonFileMemory {
    /// Create a new memory store rooted at `data_dir/memory/memory.json`.
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let memory_dir = data_dir.join("memory");
        fs::create_dir_all(&memory_dir).map_err(|e| e.to_string())?;
        let path = memory_dir.join("memory.json");
        let data = Self::load_existing(&path)?;
        Ok(Self { path, data })
    }

    fn load_existing(path: &Path) -> Result<HashMap<String, String>, String> {
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        if content.trim().is_empty() {
            return Ok(HashMap::new());
        }
        serde_json::from_str(&content).map_err(|e| {
            format!(
                "corrupt memory file at {}: {e}. Rename or delete it to start fresh.",
                path.display()
            )
        })
    }

    fn persist(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string())?;
        fs::write(&self.path, json).map_err(|e| e.to_string())
    }
}

impl MemoryProvider for JsonFileMemory {
    fn read(&self, key: &str) -> Option<String> {
        self.data.get(key).cloned()
    }

    fn write(&mut self, key: &str, value: &str) -> Result<(), String> {
        if value.len() > MAX_VALUE_SIZE {
            return Err(format!("value exceeds max size ({MAX_VALUE_SIZE} bytes)"));
        }
        if self.data.len() >= MAX_ENTRIES && !self.data.contains_key(key) {
            return Err(format!("memory full ({MAX_ENTRIES} entries max)"));
        }
        self.data.insert(key.to_string(), value.to_string());
        self.persist()
    }

    fn list(&self) -> Vec<(String, String)> {
        let mut entries: Vec<_> = self
            .data
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    fn delete(&mut self, key: &str) -> bool {
        let existed = self.data.remove(key).is_some();
        if existed {
            if let Err(e) = self.persist() {
                eprintln!("warning: memory persist failed: {e}");
            }
        }
        existed
    }

    fn search(&self, query: &str) -> Vec<(String, String)> {
        let query_lower = query.to_lowercase();
        let mut results: Vec<_> = self
            .data
            .iter()
            .filter(|(k, v)| {
                k.to_lowercase().contains(&query_lower) || v.to_lowercase().contains(&query_lower)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        results.sort_by(|a, b| a.0.cmp(&b.0));
        results
    }

    /// v1 pass-through: returns all entries via `list()`. Future implementations
    /// may summarize, prioritize, or limit entries for prompt injection.
    fn snapshot(&self) -> Vec<(String, String)> {
        self.list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_memory(dir: &Path) -> JsonFileMemory {
        JsonFileMemory::new(dir).expect("create test memory")
    }

    #[test]
    fn new_creates_directory() {
        let temp = TempDir::new().expect("tempdir");
        let data_dir = temp.path().join("nonexistent");
        let _memory = JsonFileMemory::new(&data_dir).expect("new");
        assert!(data_dir.join("memory").exists());
    }

    #[test]
    fn write_and_read() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("name", "Alice").expect("write");
        assert_eq!(memory.read("name"), Some("Alice".to_string()));
    }

    #[test]
    fn write_overwrites_existing() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("k", "v1").expect("write");
        memory.write("k", "v2").expect("overwrite");
        assert_eq!(memory.read("k"), Some("v2".to_string()));
    }

    #[test]
    fn delete_removes_key() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("k", "v").expect("write");
        assert!(memory.delete("k"));
        assert_eq!(memory.read("k"), None);
    }

    #[test]
    fn delete_returns_false_for_missing() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        assert!(!memory.delete("missing"));
    }

    #[test]
    fn list_returns_sorted() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("c", "3").expect("write");
        memory.write("a", "1").expect("write");
        memory.write("b", "2").expect("write");
        let list = memory.list();
        let keys: Vec<_> = list.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn search_finds_by_key_and_value() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("user_name", "Bob").expect("write");
        memory.write("project", "user dashboard").expect("write");
        memory.write("color", "blue").expect("write");

        let results = memory.search("user");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn write_rejects_oversized_value() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        let big = "x".repeat(MAX_VALUE_SIZE + 1);
        let result = memory.write("k", &big);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max size"));
    }

    #[test]
    fn write_rejects_when_full() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        for i in 0..MAX_ENTRIES {
            memory.write(&format!("key-{i}"), "v").expect("write");
        }
        let result = memory.write("overflow", "v");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("full"));
    }

    #[test]
    fn write_allows_overwrite_when_full() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        for i in 0..MAX_ENTRIES {
            memory.write(&format!("key-{i}"), "v").expect("write");
        }
        // Overwriting existing key should succeed even at capacity
        memory.write("key-0", "updated").expect("overwrite");
        assert_eq!(memory.read("key-0"), Some("updated".to_string()));
    }

    #[test]
    fn persists_to_disk() {
        let temp = TempDir::new().expect("tempdir");
        {
            let mut memory = test_memory(temp.path());
            memory.write("persist", "yes").expect("write");
        }
        let memory2 = test_memory(temp.path());
        assert_eq!(memory2.read("persist"), Some("yes".to_string()));
    }

    #[test]
    fn corrupt_json_returns_error() {
        let temp = TempDir::new().expect("tempdir");
        let memory_dir = temp.path().join("memory");
        fs::create_dir_all(&memory_dir).expect("create dir");
        fs::write(memory_dir.join("memory.json"), "{not valid json").expect("write");
        let result = JsonFileMemory::new(temp.path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("corrupt memory file"),
            "error should mention corruption, got: {err}"
        );
    }

    #[test]
    fn empty_file_treated_as_fresh() {
        let temp = TempDir::new().expect("tempdir");
        let memory_dir = temp.path().join("memory");
        fs::create_dir_all(&memory_dir).expect("create dir");
        fs::write(memory_dir.join("memory.json"), "  ").expect("write");
        let memory = JsonFileMemory::new(temp.path()).expect("should succeed for empty file");
        assert!(memory.list().is_empty());
    }

    #[test]
    fn snapshot_returns_all() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("a", "1").expect("write");
        memory.write("b", "2").expect("write");
        assert_eq!(memory.snapshot(), memory.list());
    }
}
