use serde_json::Value;
use std::fs;
use std::path::Path;

pub fn persisted_memory_entry_count(path: &Path) -> usize {
    let Some(content) = fs::read_to_string(path).ok() else {
        return 0;
    };
    persisted_memory_entry_count_from_str(&content).unwrap_or(0)
}

fn persisted_memory_entry_count_from_str(content: &str) -> Option<usize> {
    if content.trim().is_empty() {
        return Some(0);
    }
    let json = serde_json::from_str::<Value>(content).ok()?;
    Some(match json {
        Value::Object(entries) => entries.len(),
        Value::Array(entries) => entries.len(),
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::memory::{MemoryEntry, MemorySource};
    use std::collections::HashMap;

    fn write_json(path: &Path, json: &serde_json::Value) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, serde_json::to_string(json).expect("serialize json")).expect("write json");
    }

    fn sample_entry(value: &str) -> MemoryEntry {
        MemoryEntry {
            value: value.to_string(),
            created_at_ms: 1,
            last_accessed_at_ms: 2,
            access_count: 3,
            source: MemorySource::User,
            tags: Vec::new(),
        }
    }

    #[test]
    fn counts_current_memory_store_object_shape() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("memory.json");
        let entries = HashMap::from([
            ("project".to_string(), sample_entry("wave b")),
            ("repo".to_string(), sample_entry("fawx")),
        ]);
        write_json(&path, &serde_json::to_value(entries).expect("memory json"));

        assert_eq!(persisted_memory_entry_count(&path), 2);
    }

    #[test]
    fn counts_legacy_string_map_shape() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("memory.json");
        write_json(
            &path,
            &serde_json::json!({
                "project": "wave b",
                "repo": "fawx"
            }),
        );

        assert_eq!(persisted_memory_entry_count(&path), 2);
    }

    #[test]
    fn returns_zero_when_memory_store_is_missing() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("missing.json");

        assert_eq!(persisted_memory_entry_count(&path), 0);
    }
}
