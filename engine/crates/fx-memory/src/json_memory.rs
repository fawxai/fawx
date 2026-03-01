//! JSON file-backed persistent memory.

use fx_core::memory::{MemoryEntry, MemoryProvider, MemorySource, MemoryTouchProvider};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_MAX_ENTRIES: usize = 1000;
const DEFAULT_MAX_VALUE_SIZE: usize = 10240; // 10 KB

#[derive(Debug, Clone)]
pub struct JsonMemoryConfig {
    pub max_entries: usize,
    pub max_value_size: usize,
}

impl Default for JsonMemoryConfig {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_value_size: DEFAULT_MAX_VALUE_SIZE,
        }
    }
}

#[derive(Debug)]
struct LoadResult {
    data: HashMap<String, MemoryEntry>,
    migrated_from_legacy: bool,
}

impl LoadResult {
    fn fresh() -> Self {
        Self {
            data: HashMap::new(),
            migrated_from_legacy: false,
        }
    }

    fn from_data(data: HashMap<String, MemoryEntry>, migrated_from_legacy: bool) -> Self {
        Self {
            data,
            migrated_from_legacy,
        }
    }
}

#[derive(Debug)]
pub struct JsonFileMemory {
    path: PathBuf,
    data: HashMap<String, MemoryEntry>,
    config: JsonMemoryConfig,
}

impl JsonFileMemory {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        Self::new_with_config(data_dir, JsonMemoryConfig::default())
    }

    pub fn new_with_config(data_dir: &Path, config: JsonMemoryConfig) -> Result<Self, String> {
        let memory_dir = data_dir.join("memory");
        fs::create_dir_all(&memory_dir).map_err(|e| e.to_string())?;
        let path = memory_dir.join("memory.json");
        let load_result = Self::load_existing(&path)?;
        let memory = Self {
            path,
            data: load_result.data,
            config,
        };
        if load_result.migrated_from_legacy {
            memory.persist()?;
        }
        Ok(memory)
    }

    fn load_existing(path: &Path) -> Result<LoadResult, String> {
        if !path.exists() {
            return Ok(LoadResult::fresh());
        }
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        if content.trim().is_empty() {
            return Ok(LoadResult::fresh());
        }
        Self::parse_content(&content, path)
    }

    fn parse_content(content: &str, path: &Path) -> Result<LoadResult, String> {
        if let Ok(data) = serde_json::from_str::<HashMap<String, MemoryEntry>>(content) {
            return Ok(LoadResult::from_data(data, false));
        }
        Self::parse_legacy_content(content, path)
    }

    fn parse_legacy_content(content: &str, path: &Path) -> Result<LoadResult, String> {
        let legacy_data: HashMap<String, String> =
            serde_json::from_str(content).map_err(|e| Self::corrupt_file_error(path, e))?;
        let created_at_ms = Self::file_modified_at_ms(path);
        let migrated_data = Self::migrate_legacy_entries(legacy_data, created_at_ms);
        Ok(LoadResult::from_data(migrated_data, true))
    }

    fn migrate_legacy_entries(
        legacy_data: HashMap<String, String>,
        created_at_ms: u64,
    ) -> HashMap<String, MemoryEntry> {
        legacy_data
            .into_iter()
            .map(|(key, value)| {
                (
                    key,
                    MemoryEntry {
                        value,
                        created_at_ms,
                        last_accessed_at_ms: 0,
                        access_count: 0,
                        source: MemorySource::User,
                        tags: Vec::new(),
                    },
                )
            })
            .collect()
    }

    fn file_modified_at_ms(path: &Path) -> u64 {
        fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    fn corrupt_file_error(path: &Path, error: serde_json::Error) -> String {
        format!(
            "corrupt memory file at {}: {error}. Rename or delete it to start fresh.",
            path.display()
        )
    }

    fn persist(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string())?;
        fs::write(&self.path, json).map_err(|e| e.to_string())
    }
}

impl MemoryProvider for JsonFileMemory {
    fn read(&self, key: &str) -> Option<String> {
        self.data.get(key).map(|entry| entry.value.clone())
    }

    fn write(&mut self, key: &str, value: &str) -> Result<(), String> {
        if value.len() > self.config.max_value_size {
            return Err(format!(
                "value exceeds max size ({} bytes)",
                self.config.max_value_size
            ));
        }
        if self.data.len() >= self.config.max_entries && !self.data.contains_key(key) {
            return Err(format!(
                "memory full ({} entries max)",
                self.config.max_entries
            ));
        }
        self.data.insert(
            key.to_string(),
            MemoryEntry {
                value: value.to_string(),
                created_at_ms: now_ms(),
                last_accessed_at_ms: 0,
                access_count: 0,
                source: MemorySource::User,
                tags: Vec::new(),
            },
        );
        self.persist()
    }

    fn list(&self) -> Vec<(String, String)> {
        let mut entries: Vec<_> = self
            .data
            .iter()
            .map(|(key, entry)| (key.clone(), entry.value.clone()))
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
            .filter(|(key, entry)| {
                key.to_lowercase().contains(&query_lower)
                    || entry.value.to_lowercase().contains(&query_lower)
            })
            .map(|(key, entry)| (key.clone(), entry.value.clone()))
            .collect();
        results.sort_by(|a, b| a.0.cmp(&b.0));
        results
    }

    fn snapshot(&self) -> Vec<(String, String)> {
        let mut entries: Vec<_> = self
            .data
            .iter()
            .map(|(key, entry)| (key.clone(), entry.value.clone(), entry.access_count))
            .collect();
        entries.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        entries
            .into_iter()
            .map(|(key, value, _)| (key, value))
            .collect()
    }
}

impl MemoryTouchProvider for JsonFileMemory {
    fn touch(&mut self, key: &str) -> Result<(), String> {
        let Some(entry) = self.data.get_mut(key) else {
            return Ok(());
        };
        entry.last_accessed_at_ms = now_ms();
        entry.access_count = entry.access_count.saturating_add(1);
        self.persist()
    }
}

fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(error) => {
            eprintln!("warning: system clock before Unix epoch: {error}, using 0");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::memory::MemorySource;
    use tempfile::TempDir;

    fn test_memory(dir: &Path) -> JsonFileMemory {
        JsonFileMemory::new_with_config(dir, JsonMemoryConfig::default())
            .expect("create test memory")
    }

    fn memory_file_path(dir: &Path) -> PathBuf {
        dir.join("memory").join("memory.json")
    }

    fn file_modified_ms(path: &Path) -> u64 {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    #[test]
    fn new_creates_directory() {
        let temp = TempDir::new().expect("tempdir");
        let data_dir = temp.path().join("nonexistent");
        let _memory =
            JsonFileMemory::new_with_config(&data_dir, JsonMemoryConfig::default()).expect("new");
        assert!(data_dir.join("memory").exists());
    }

    #[test]
    fn write_creates_entry_with_metadata() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("name", "Alice").expect("write");
        let entry = memory.data.get("name").expect("entry");
        assert_eq!(entry.value, "Alice");
        assert!(entry.created_at_ms > 0);
        assert_eq!(entry.last_accessed_at_ms, 0);
        assert_eq!(entry.access_count, 0);
        assert_eq!(entry.source, MemorySource::User);
        assert!(entry.tags.is_empty());
    }

    #[test]
    fn read_returns_value_string() {
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
        let big = "x".repeat(DEFAULT_MAX_VALUE_SIZE + 1);
        let result = memory.write("k", &big);
        assert!(result.is_err());
        assert!(result.expect_err("oversize").contains("max size"));
    }

    #[test]
    fn write_rejects_when_full() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        for i in 0..DEFAULT_MAX_ENTRIES {
            memory.write(&format!("key-{i}"), "v").expect("write");
        }
        let result = memory.write("overflow", "v");
        assert!(result.is_err());
        assert!(result.expect_err("overflow").contains("full"));
    }

    #[test]
    fn write_allows_overwrite_when_full() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        for i in 0..DEFAULT_MAX_ENTRIES {
            memory.write(&format!("key-{i}"), "v").expect("write");
        }
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
    fn persists_metadata_across_restarts() {
        let temp = TempDir::new().expect("tempdir");
        {
            let mut memory = test_memory(temp.path());
            memory.write("persist", "yes").expect("write");
            memory.touch("persist").expect("touch");
        }
        let memory = test_memory(temp.path());
        let entry = memory.data.get("persist").expect("entry");
        assert_eq!(entry.value, "yes");
        assert_eq!(entry.source, MemorySource::User);
        assert_eq!(entry.access_count, 1);
        assert!(entry.last_accessed_at_ms > 0);
    }

    #[test]
    fn migrates_old_format_on_load() {
        let temp = TempDir::new().expect("tempdir");
        let memory_dir = temp.path().join("memory");
        fs::create_dir_all(&memory_dir).expect("create dir");

        let mut legacy = HashMap::new();
        legacy.insert("legacy".to_string(), "value".to_string());
        let legacy_json = serde_json::to_string_pretty(&legacy).expect("serialize");
        let memory_path = memory_file_path(temp.path());
        fs::write(&memory_path, legacy_json).expect("write");

        let expected_created_at_ms = file_modified_ms(&memory_path);
        let memory = test_memory(temp.path());

        assert_eq!(memory.read("legacy"), Some("value".to_string()));
        let entry = memory.data.get("legacy").expect("entry");
        assert_eq!(entry.created_at_ms, expected_created_at_ms);
        assert_eq!(entry.last_accessed_at_ms, 0);
        assert_eq!(entry.access_count, 0);
        assert_eq!(entry.source, MemorySource::User);
        assert!(entry.tags.is_empty());

        let persisted = fs::read_to_string(&memory_path).expect("read migrated");
        let migrated: HashMap<String, MemoryEntry> =
            serde_json::from_str(&persisted).expect("parse migrated");
        assert_eq!(migrated["legacy"].value, "value");
    }

    #[test]
    fn corrupt_json_returns_error() {
        let temp = TempDir::new().expect("tempdir");
        let memory_dir = temp.path().join("memory");
        fs::create_dir_all(&memory_dir).expect("create dir");
        fs::write(memory_dir.join("memory.json"), "{not valid json").expect("write");
        let result = JsonFileMemory::new_with_config(temp.path(), JsonMemoryConfig::default());
        assert!(result.is_err());
        let err = result.expect_err("corrupt");
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
        let memory = JsonFileMemory::new_with_config(temp.path(), JsonMemoryConfig::default())
            .expect("should succeed for empty file");
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

    #[test]
    fn snapshot_sorted_by_access_count() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("a", "1").expect("write");
        memory.write("b", "2").expect("write");
        memory.write("c", "3").expect("write");

        memory.touch("b").expect("touch b1");
        memory.touch("b").expect("touch b2");
        memory.touch("c").expect("touch c1");

        let snapshot = memory.snapshot();
        let keys: Vec<_> = snapshot.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, vec!["b", "c", "a"]);
    }

    #[test]
    fn memory_source_serializes_as_snake_case() {
        let entry = MemoryEntry {
            value: "test".to_string(),
            created_at_ms: 100,
            last_accessed_at_ms: 0,
            access_count: 0,
            source: MemorySource::SignalAnalysis,
            tags: Vec::new(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(
            json.contains("\"signal_analysis\""),
            "MemorySource should serialize as snake_case, got: {json}"
        );
    }

    #[test]
    fn memory_source_deserializes_from_snake_case() {
        let json = r#"{
            "value": "test",
            "created_at_ms": 100,
            "last_accessed_at_ms": 0,
            "access_count": 0,
            "source": "signal_analysis",
            "tags": []
        }"#;
        let entry: MemoryEntry = serde_json::from_str(json).expect("deserialize");
        assert_eq!(entry.source, MemorySource::SignalAnalysis);
    }

    #[test]
    fn memory_entry_defaults_new_fields_on_partial_json() {
        let json = r#"{"value": "hello"}"#;
        let entry: MemoryEntry = serde_json::from_str(json).expect("deserialize");
        assert_eq!(entry.value, "hello");
        assert_eq!(entry.created_at_ms, 0);
        assert_eq!(entry.source, MemorySource::User);
        assert!(entry.tags.is_empty());
    }

    #[test]
    fn memory_source_display_matches_serde() {
        assert_eq!(MemorySource::User.to_string(), "user");
        assert_eq!(MemorySource::SignalAnalysis.to_string(), "signal_analysis");
        assert_eq!(MemorySource::Consolidation.to_string(), "consolidation");
    }

    #[test]
    fn snapshot_documents_sort_by_access_count_then_key() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("z", "last-alpha").expect("write");
        memory.write("a", "first-alpha").expect("write");
        memory.write("m", "mid-alpha").expect("write");

        // Give "m" 2 touches, "a" 1 touch, "z" none
        memory.touch("m").expect("touch m1");
        memory.touch("m").expect("touch m2");
        memory.touch("a").expect("touch a1");

        let snapshot = memory.snapshot();
        let keys: Vec<_> = snapshot.iter().map(|(k, _)| k.as_str()).collect();
        // m(2) > a(1) > z(0)
        assert_eq!(keys, vec!["m", "a", "z"]);
    }
}
