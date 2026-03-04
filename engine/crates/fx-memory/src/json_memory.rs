//! JSON file-backed persistent memory.

use fx_core::memory::{MemoryEntry, MemoryProvider, MemorySource, MemoryTouchProvider};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_MAX_ENTRIES: usize = 1000;
const DEFAULT_MAX_VALUE_SIZE: usize = 10240; // 10 KB
const MS_PER_DAY: f64 = 86_400_000.0;

/// Configuration for time-based memory decay and pruning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayConfig {
    /// Decay factor per day of non-access. Range: (0.0, 1.0]. Default: 0.95.
    pub decay_factor: f64,
    /// Entries with decayed weight below this are pruned. Default: 0.1.
    pub prune_threshold: f64,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            decay_factor: 0.95,
            prune_threshold: 0.1,
        }
    }
}

/// Compute the time-decayed weight of a memory entry.
///
/// Weight = `base_weight * decay_factor^days_since_access`, where
/// `base_weight = access_count.max(1)` so new entries start at 1.0.
pub(crate) fn decayed_weight(entry: &MemoryEntry, now_ms: u64, config: &DecayConfig) -> f64 {
    let last_active_ms = if entry.last_accessed_at_ms == 0 {
        entry.created_at_ms
    } else {
        entry.last_accessed_at_ms
    };
    let days_since_access = now_ms.saturating_sub(last_active_ms) as f64 / MS_PER_DAY;
    let base_weight = entry.access_count.max(1) as f64;
    base_weight * config.decay_factor.powf(days_since_access)
}

#[derive(Debug, Clone)]
pub struct JsonMemoryConfig {
    pub max_entries: usize,
    pub max_value_size: usize,
    pub decay_config: DecayConfig,
}

impl Default for JsonMemoryConfig {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_value_size: DEFAULT_MAX_VALUE_SIZE,
            decay_config: DecayConfig::default(),
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

    /// Remove entries with decayed weight below the prune threshold.
    ///
    /// Persists changes if any entries were pruned. Returns the number
    /// of entries removed.
    pub fn prune(&mut self) -> usize {
        let now = now_ms();
        let before = self.data.len();
        let decay = &self.config.decay_config;
        self.data
            .retain(|_key, entry| decayed_weight(entry, now, decay) >= decay.prune_threshold);
        let pruned = before - self.data.len();
        if pruned > 0 {
            if let Err(e) = self.persist() {
                eprintln!("warning: memory persist after prune failed: {e}");
            }
        }
        pruned
    }
}

impl MemoryProvider for JsonFileMemory {
    fn read(&self, key: &str) -> Option<String> {
        self.data.get(key).map(|entry| entry.value.clone())
    }

    fn write(&mut self, key: &str, value: &str) -> Result<(), String> {
        if key.is_empty() {
            return Err("memory key must not be empty".to_string());
        }
        if value.len() > self.config.max_value_size {
            return Err(format!(
                "value exceeds max size ({} bytes)",
                self.config.max_value_size
            ));
        }
        if self.data.len() >= self.config.max_entries && !self.data.contains_key(key) {
            self.prune();
            if self.data.len() >= self.config.max_entries {
                return Err(format!(
                    "memory full ({} entries max)",
                    self.config.max_entries
                ));
            }
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

    fn search_relevant(&self, query: &str, max_results: usize) -> Vec<(String, String)> {
        if max_results == 0 {
            return Vec::new();
        }

        let query_terms = normalized_query_terms(query);
        if query_terms.is_empty() {
            return Vec::new();
        }

        let mut ranked: Vec<_> = self
            .data
            .iter()
            .filter_map(|(key, entry)| {
                let key_lower = key.to_lowercase();
                let value_lower = entry.value.to_lowercase();
                let match_count = relevance_match_count(&key_lower, &value_lower, &query_terms);
                (match_count > 0).then(|| (key.clone(), entry.value.clone(), match_count))
            })
            .collect();

        ranked.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        ranked
            .into_iter()
            .take(max_results)
            .map(|(key, value, _)| (key, value))
            .collect()
    }

    fn snapshot(&self) -> Vec<(String, String)> {
        let now = now_ms();
        let decay = &self.config.decay_config;
        let mut entries: Vec<_> = self
            .data
            .iter()
            .map(|(key, entry)| {
                let weight = decayed_weight(entry, now, decay);
                (key.clone(), entry.value.clone(), weight)
            })
            .collect();
        entries.sort_by(|a, b| {
            b.2.partial_cmp(&a.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
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

fn normalized_query_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<_> = query
        .split_whitespace()
        .map(|term| term.to_lowercase())
        .collect();
    terms.sort();
    terms.dedup();
    terms
}

fn relevance_match_count(key_lower: &str, value_lower: &str, query_terms: &[String]) -> usize {
    query_terms
        .iter()
        .filter(|term| key_lower.contains(term.as_str()) || value_lower.contains(term.as_str()))
        .count()
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
    fn search_relevant_returns_empty_for_no_matches() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("pet", "cat").expect("write");
        memory.write("city", "denver").expect("write");

        let results = memory.search_relevant("volcano glacier", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn search_relevant_ranks_by_match_count() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory
            .write("project_launch", "shipping auth flow")
            .expect("write");
        memory
            .write("project_notes", "shipping soon")
            .expect("write");

        let results = memory.search_relevant("project auth", 5);
        let keys: Vec<_> = results.iter().map(|(key, _)| key.as_str()).collect();

        assert_eq!(keys, vec!["project_launch", "project_notes"]);
    }

    #[test]
    fn search_relevant_caps_at_max_results() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());

        for i in 0..10 {
            memory
                .write(&format!("match-{i}"), "contains token")
                .expect("write");
        }

        let results = memory.search_relevant("token", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn search_relevant_zero_max_results_returns_empty() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("project", "auth rollout").expect("write");
        memory.write("notes", "project status").expect("write");

        let results = memory.search_relevant("project", 0);
        assert!(results.is_empty());
    }

    #[test]
    fn search_relevant_deduplicates_query_terms() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory
            .write("z_project_only", "project milestones")
            .expect("write project");
        memory
            .write("a_auth_only", "auth milestones")
            .expect("write auth");

        let repeated_project = memory.search_relevant("project project project", 5);
        let single_project = memory.search_relevant("project", 5);
        assert_eq!(repeated_project, single_project);

        let repeated_mixed = memory.search_relevant("project project auth", 5);
        let unique_mixed = memory.search_relevant("project auth", 5);
        assert_eq!(repeated_mixed, unique_mixed);
    }

    #[test]
    fn search_relevant_handles_special_characters() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory
            .write("cpp_speed", "c++ (fast) iteration tricks")
            .expect("write cpp speed");
        memory
            .write("cpp_basics", "c++ starter notes")
            .expect("write cpp basics");

        let results = memory.search_relevant("c++ (fast)", 5);
        let keys: Vec<_> = results.iter().map(|(key, _)| key.as_str()).collect();

        assert_eq!(keys.first().copied(), Some("cpp_speed"));
        assert!(keys.contains(&"cpp_basics"));
    }

    #[test]
    fn search_relevant_is_case_insensitive() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("greeting", "hello world").expect("write");

        let results = memory.search_relevant("HELLO", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "greeting");
    }

    #[test]
    fn search_relevant_empty_query_returns_empty() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory.write("anything", "value").expect("write");

        assert!(memory.search_relevant("", 5).is_empty());
        assert!(memory.search_relevant("   ", 5).is_empty());
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
        let mut snapshot = memory.snapshot();
        snapshot.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(snapshot, memory.list());
    }

    #[test]
    fn snapshot_sorted_by_decayed_weight() {
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
        // b: access_count=2, weight≈2.0 (highest)
        // c: access_count=1, last_accessed=now, weight≈1.0
        // a: access_count=0, base_weight=max(1)=1, created slightly before c,
        //    so marginally more elapsed time → marginally lower weight
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
    fn snapshot_documents_sort_by_decayed_weight_then_key() {
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
        // m: access_count=2, weight≈2.0
        // a: access_count=1, weight≈1.0
        // z: access_count=0, base_weight=max(1)=1, weight≈1.0
        // a and z tie on weight, sorted by key ascending
        assert_eq!(keys, vec!["m", "a", "z"]);
    }

    // ===== Memory Decay and Pruning Tests (spec #1103) =====

    fn make_entry(access_count: u32, last_accessed_at_ms: u64, created_at_ms: u64) -> MemoryEntry {
        MemoryEntry {
            value: "test".to_string(),
            created_at_ms,
            last_accessed_at_ms,
            access_count,
            source: MemorySource::User,
            tags: Vec::new(),
        }
    }

    fn config_with_decay(factor: f64, threshold: f64, max: usize) -> JsonMemoryConfig {
        JsonMemoryConfig {
            max_entries: max,
            max_value_size: DEFAULT_MAX_VALUE_SIZE,
            decay_config: DecayConfig {
                decay_factor: factor,
                prune_threshold: threshold,
            },
        }
    }

    // --- Decay function tests (1-6) ---

    #[test]
    fn decay_entry_accessed_today_weight_approx_one() {
        let now = 1_700_000_000_000u64;
        let entry = make_entry(1, now, now - 86_400_000);
        let config = DecayConfig::default();
        let weight = decayed_weight(&entry, now, &config);
        assert!(
            (weight - 1.0).abs() < 0.01,
            "weight={weight}, expected ≈1.0"
        );
    }

    #[test]
    fn decay_entry_accessed_14_days_ago() {
        let now = 1_700_000_000_000u64;
        let fourteen_days_ms = 14 * 86_400_000u64;
        let entry = make_entry(1, now - fourteen_days_ms, now - 30 * 86_400_000);
        let config = DecayConfig::default(); // factor=0.95
        let weight = decayed_weight(&entry, now, &config);
        // 1.0 * 0.95^14 ≈ 0.488
        assert!(
            (weight - 0.488).abs() < 0.01,
            "weight={weight}, expected ≈0.488"
        );
        assert!(weight > 0.1, "should be above default prune threshold");
    }

    #[test]
    fn decay_entry_accessed_45_days_ago_below_threshold() {
        let now = 1_700_000_000_000u64;
        let forty_five_days_ms = 45 * 86_400_000u64;
        let entry = make_entry(1, now - forty_five_days_ms, now - 60 * 86_400_000);
        let config = DecayConfig::default();
        let weight = decayed_weight(&entry, now, &config);
        // 1.0 * 0.95^45 ≈ 0.099
        assert!(weight < 0.1, "weight={weight}, expected < 0.1 (threshold)");
    }

    #[test]
    fn decay_high_access_count_survives_45_days() {
        let now = 1_700_000_000_000u64;
        let forty_five_days_ms = 45 * 86_400_000u64;
        let entry = make_entry(10, now - forty_five_days_ms, now - 60 * 86_400_000);
        let config = DecayConfig::default();
        let weight = decayed_weight(&entry, now, &config);
        // 10.0 * 0.95^45 ≈ 0.99
        assert!(
            (weight - 0.99).abs() < 0.05,
            "weight={weight}, expected ≈0.99"
        );
    }

    #[test]
    fn decay_never_accessed_uses_created_at() {
        let now = 1_700_000_000_000u64;
        let seven_days_ms = 7 * 86_400_000u64;
        let entry = make_entry(1, 0, now - seven_days_ms);
        let config = DecayConfig::default();
        let weight = decayed_weight(&entry, now, &config);
        // 1.0 * 0.95^7 ≈ 0.698
        assert!(
            (weight - 0.698).abs() < 0.01,
            "weight={weight}, expected ≈0.698"
        );
    }

    #[test]
    fn decay_brand_new_entry_weight_approx_one() {
        let now = 1_700_000_000_000u64;
        // access_count=0 → base_weight = max(1) = 1
        let entry = make_entry(0, 0, now);
        let config = DecayConfig::default();
        let weight = decayed_weight(&entry, now, &config);
        assert!(
            (weight - 1.0).abs() < 0.001,
            "weight={weight}, expected ≈1.0"
        );
    }

    // --- Pruning tests (7-11) ---

    #[test]
    fn prune_removes_entries_below_threshold() {
        let temp = TempDir::new().expect("tempdir");
        let config = config_with_decay(0.95, 0.1, 1000);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");

        let now = now_ms();
        let old_ms = now.saturating_sub(50 * 86_400_000); // 50 days ago
                                                          // Insert 5 entries: 3 fresh, 2 stale
        for i in 0..3 {
            memory
                .data
                .insert(format!("fresh-{i}"), make_entry(1, now, now - 86_400_000));
        }
        for i in 0..2 {
            memory.data.insert(
                format!("stale-{i}"),
                make_entry(1, old_ms, old_ms - 86_400_000),
            );
        }

        let pruned = memory.prune();
        assert_eq!(pruned, 2);
        assert_eq!(memory.data.len(), 3);
        assert!(memory.data.contains_key("fresh-0"));
        assert!(!memory.data.contains_key("stale-0"));
    }

    #[test]
    fn prune_no_entries_below_threshold() {
        let temp = TempDir::new().expect("tempdir");
        let config = config_with_decay(0.95, 0.1, 1000);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");

        let now = now_ms();
        for i in 0..5 {
            memory
                .data
                .insert(format!("fresh-{i}"), make_entry(1, now, now - 86_400_000));
        }

        let pruned = memory.prune();
        assert_eq!(pruned, 0);
        assert_eq!(memory.data.len(), 5);
    }

    #[test]
    fn write_prunes_before_rejecting_when_full() {
        let temp = TempDir::new().expect("tempdir");
        let max = 5;
        let config = config_with_decay(0.95, 0.1, max);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");

        let now = now_ms();
        let old_ms = now.saturating_sub(50 * 86_400_000);
        // Fill with 2 fresh + 3 stale
        for i in 0..2 {
            memory
                .data
                .insert(format!("fresh-{i}"), make_entry(1, now, now - 86_400_000));
        }
        for i in 0..3 {
            memory.data.insert(
                format!("stale-{i}"),
                make_entry(1, old_ms, old_ms - 86_400_000),
            );
        }
        memory.persist().expect("persist");

        // Should succeed: prune removes 3 stale entries first
        memory
            .write("new-entry", "hello")
            .expect("write after prune");
        assert_eq!(memory.data.len(), 3); // 2 fresh + 1 new
    }

    #[test]
    fn write_rejects_when_prune_frees_nothing() {
        let temp = TempDir::new().expect("tempdir");
        let max = 5;
        let config = config_with_decay(0.95, 0.1, max);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");

        let now = now_ms();
        for i in 0..5 {
            memory
                .data
                .insert(format!("fresh-{i}"), make_entry(1, now, now - 86_400_000));
        }
        memory.persist().expect("persist");

        let result = memory.write("overflow", "value");
        assert!(result.is_err());
        assert!(result.expect_err("full").contains("full"));
    }

    #[test]
    fn prune_persists_to_disk() {
        let temp = TempDir::new().expect("tempdir");
        let config = config_with_decay(0.95, 0.1, 1000);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config.clone()).expect("create memory");

        let now = now_ms();
        let old_ms = now.saturating_sub(50 * 86_400_000);
        memory
            .data
            .insert("fresh".to_string(), make_entry(1, now, now - 86_400_000));
        memory.data.insert(
            "stale".to_string(),
            make_entry(1, old_ms, old_ms - 86_400_000),
        );
        memory.persist().expect("persist");

        let pruned = memory.prune();
        assert_eq!(pruned, 1);

        // Reload from disk
        let reloaded = JsonFileMemory::new_with_config(temp.path(), config).expect("reload memory");
        assert_eq!(reloaded.data.len(), 1);
        assert!(reloaded.data.contains_key("fresh"));
        assert!(!reloaded.data.contains_key("stale"));
    }

    // --- Snapshot ordering tests (12-13) ---

    #[test]
    fn snapshot_prefers_recent_over_historically_popular() {
        let temp = TempDir::new().expect("tempdir");
        let config = config_with_decay(0.95, 0.1, 1000);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");

        let now = now_ms();
        let sixty_days_ms = 60 * 86_400_000u64;
        // A: access_count=10, last accessed 60 days ago
        // weight: 10 * 0.95^60 ≈ 0.461
        memory.data.insert(
            "popular_old".to_string(),
            make_entry(10, now - sixty_days_ms, now - 90 * 86_400_000),
        );
        // B: access_count=2, last accessed today
        // weight: 2 * 0.95^0 ≈ 2.0
        memory
            .data
            .insert("recent".to_string(), make_entry(2, now, now - 86_400_000));

        let snapshot = memory.snapshot();
        let keys: Vec<_> = snapshot.iter().map(|(k, _)| k.as_str()).collect();
        // B (2.0) > A (0.461) → recent first
        assert_eq!(keys[0], "recent");
        assert_eq!(keys[1], "popular_old");
    }

    #[test]
    fn snapshot_order_matches_decayed_weight_with_tiebreaker() {
        let temp = TempDir::new().expect("tempdir");
        let config = config_with_decay(0.95, 0.1, 1000);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");

        let now = now_ms();
        // Three entries with same access_count=1, same last_accessed=now
        // → same decayed_weight, should sort by key name ascending
        memory
            .data
            .insert("charlie".to_string(), make_entry(1, now, now));
        memory
            .data
            .insert("alpha".to_string(), make_entry(1, now, now));
        memory
            .data
            .insert("bravo".to_string(), make_entry(1, now, now));

        let snapshot = memory.snapshot();
        let keys: Vec<_> = snapshot.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["alpha", "bravo", "charlie"]);
    }

    // --- Configuration tests (14-17) ---

    #[test]
    fn decay_factor_one_means_no_decay() {
        let now = 1_700_000_000_000u64;
        let hundred_days_ms = 100 * 86_400_000u64;
        let entry = make_entry(1, now - hundred_days_ms, now - 200 * 86_400_000);
        let config = DecayConfig {
            decay_factor: 1.0,
            prune_threshold: 0.1,
        };
        let weight = decayed_weight(&entry, now, &config);
        // 1.0 * 1.0^100 = 1.0 — no decay
        assert!(
            (weight - 1.0).abs() < 0.001,
            "weight={weight}, expected 1.0 with decay_factor=1.0"
        );
    }

    #[test]
    fn aggressive_decay_factor_half() {
        let now = 1_700_000_000_000u64;
        let one_day_ms = 86_400_000u64;
        let entry = make_entry(1, now - one_day_ms, now - 2 * one_day_ms);
        let config = DecayConfig {
            decay_factor: 0.5,
            prune_threshold: 0.1,
        };
        let weight = decayed_weight(&entry, now, &config);
        // 1.0 * 0.5^1 = 0.5
        assert!(
            (weight - 0.5).abs() < 0.001,
            "weight={weight}, expected 0.5"
        );
    }

    #[test]
    fn prune_threshold_zero_never_prunes() {
        let temp = TempDir::new().expect("tempdir");
        let config = config_with_decay(0.95, 0.0, 1000);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");

        let now = now_ms();
        let old_ms = now.saturating_sub(365 * 86_400_000);
        // Very old entry — would be pruned with default threshold
        memory
            .data
            .insert("ancient".to_string(), make_entry(1, old_ms, old_ms));

        let pruned = memory.prune();
        assert_eq!(pruned, 0, "threshold=0.0 should never prune");
        assert_eq!(memory.data.len(), 1);
    }

    #[test]
    fn default_decay_config_values() {
        let config = DecayConfig::default();
        assert!((config.decay_factor - 0.95).abs() < f64::EPSILON);
        assert!((config.prune_threshold - 0.1).abs() < f64::EPSILON);
    }

    // --- Edge case tests (18-20) ---

    #[test]
    fn prune_empty_memory_returns_zero() {
        let temp = TempDir::new().expect("tempdir");
        let config = config_with_decay(0.95, 0.1, 1000);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");
        assert!(memory.data.is_empty());
        let pruned = memory.prune();
        assert_eq!(pruned, 0);
    }

    #[test]
    fn prune_removes_all_entries_when_all_below_threshold() {
        let temp = TempDir::new().expect("tempdir");
        let config = config_with_decay(0.95, 0.1, 1000);
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");

        let now = now_ms();
        let old_ms = now.saturating_sub(50 * 86_400_000);
        for i in 0..5 {
            memory.data.insert(
                format!("stale-{i}"),
                make_entry(1, old_ms, old_ms - 86_400_000),
            );
        }
        memory.persist().expect("persist");

        let pruned = memory.prune();
        assert_eq!(pruned, 5);
        assert!(memory.data.is_empty());
    }

    #[test]
    fn decay_clock_at_zero_means_no_decay() {
        // If system clock returns 0 (before epoch), days_since_access = 0
        let entry = make_entry(5, 0, 0);
        let config = DecayConfig::default();
        let weight = decayed_weight(&entry, 0, &config);
        // 5.0 * 0.95^0 = 5.0
        assert!(
            (weight - 5.0).abs() < 0.001,
            "weight={weight}, expected 5.0 when now=0"
        );
    }

    // ── Security boundary tests: memory write validation (spec #1102, T-12) ──

    #[test]
    fn t12_write_rejects_empty_key() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        let result = memory.write("", "some value");
        assert!(result.is_err(), "writing with empty key should be rejected");
    }

    #[test]
    fn t12_write_rejects_value_exceeding_max_size() {
        let temp = TempDir::new().expect("tempdir");
        let config = JsonMemoryConfig {
            max_value_size: 100,
            ..JsonMemoryConfig::default()
        };
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");
        let big = "x".repeat(101);
        let result = memory.write("key", &big);
        assert!(result.is_err());
    }

    #[test]
    fn t12_write_succeeds_with_valid_key_value() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        memory
            .write("valid_key", "valid_value")
            .expect("write should succeed");
        let entry = memory.data.get("valid_key").expect("entry should exist");
        assert_eq!(entry.value, "valid_value");
        assert_eq!(entry.source, MemorySource::User);
    }

    #[test]
    fn t12_write_handles_null_bytes_without_panic() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        // Must not panic regardless of outcome.
        let _ = memory.write("null_test", "value\0with\0nulls");
    }

    #[test]
    fn t12_write_handles_extremely_long_key_without_panic() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = test_memory(temp.path());
        let long_key = "k".repeat(10_000);
        // Must not panic regardless of outcome.
        let _ = memory.write(&long_key, "value");
    }

    #[test]
    fn t12_write_at_exact_max_value_size_succeeds() {
        let temp = TempDir::new().expect("tempdir");
        let config = JsonMemoryConfig {
            max_value_size: 100,
            ..JsonMemoryConfig::default()
        };
        let mut memory =
            JsonFileMemory::new_with_config(temp.path(), config).expect("create memory");
        let exact = "x".repeat(100);
        memory
            .write("key", &exact)
            .expect("exact-size write should succeed");
        assert_eq!(memory.read("key"), Some(exact));
    }
}
