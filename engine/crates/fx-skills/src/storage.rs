//! Isolated storage with quota enforcement for skills.

use fx_core::error::SkillError;
use std::collections::HashMap;

/// Per-skill storage with quota enforcement.
#[derive(Debug, Clone)]
pub struct SkillStorage {
    skill_name: String,
    max_bytes: usize,
    data: HashMap<String, String>,
}

impl SkillStorage {
    /// Create a new isolated storage for a skill with a byte quota.
    pub fn new(skill_name: &str, max_bytes: usize) -> Self {
        Self {
            skill_name: skill_name.to_string(),
            max_bytes,
            data: HashMap::new(),
        }
    }

    /// Get a value by key.
    pub fn get(&self, key: &str) -> Option<String> {
        self.data.get(key).cloned()
    }

    /// Set a value, enforcing quota.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), SkillError> {
        let key_bytes = key.len();
        let value_bytes = value.len();
        let pair_bytes = key_bytes + value_bytes;

        // Calculate current usage excluding this key if it exists
        let current_without_key = if let Some(old_value) = self.data.get(key) {
            self.used_bytes() - key_bytes - old_value.len()
        } else {
            self.used_bytes()
        };

        let new_total = current_without_key + pair_bytes;

        if new_total > self.max_bytes {
            return Err(SkillError::Execution(format!(
                "Storage quota exceeded for skill '{}': {} bytes used, {} max",
                self.skill_name, new_total, self.max_bytes
            )));
        }

        self.data.insert(key.to_string(), value.to_string());
        Ok(())
    }

    /// Delete a key-value pair.
    ///
    /// Returns `true` if the key existed, `false` otherwise.
    pub fn delete(&mut self, key: &str) -> bool {
        self.data.remove(key).is_some()
    }

    /// Get the number of bytes currently used.
    pub fn used_bytes(&self) -> usize {
        self.data.iter().map(|(k, v)| k.len() + v.len()).sum()
    }

    /// Get the number of bytes remaining.
    pub fn remaining_bytes(&self) -> usize {
        self.max_bytes.saturating_sub(self.used_bytes())
    }

    /// Get the skill name.
    pub fn skill_name(&self) -> &str {
        &self.skill_name
    }

    /// Get the maximum bytes allowed.
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_set_get_delete() {
        let mut storage = SkillStorage::new("test", 1024);

        assert_eq!(storage.get("key1"), None);

        storage.set("key1", "value1").expect("Should set");
        assert_eq!(storage.get("key1"), Some("value1".to_string()));

        assert!(storage.delete("key1"));
        assert_eq!(storage.get("key1"), None);
        assert!(!storage.delete("key1"));
    }

    #[test]
    fn test_storage_quota_enforcement() {
        let mut storage = SkillStorage::new("test", 20);

        // "key1" (4 bytes) + "value1" (6 bytes) = 10 bytes
        storage.set("key1", "value1").expect("Should fit");

        // "key2" (4 bytes) + "value2long" (10 bytes) = 14 bytes
        // Total would be 24 bytes, exceeds 20
        let result = storage.set("key2", "value2long");
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Execution(_))));
    }

    #[test]
    fn test_storage_used_bytes() {
        let mut storage = SkillStorage::new("test", 1024);

        assert_eq!(storage.used_bytes(), 0);

        storage.set("key1", "value1").expect("Should set");
        // "key1" = 4 bytes, "value1" = 6 bytes
        assert_eq!(storage.used_bytes(), 10);

        storage.set("key2", "abc").expect("Should set");
        // "key2" = 4 bytes, "abc" = 3 bytes
        assert_eq!(storage.used_bytes(), 17);

        storage.delete("key1");
        assert_eq!(storage.used_bytes(), 7);
    }

    #[test]
    fn test_storage_remaining_bytes() {
        let mut storage = SkillStorage::new("test", 100);

        assert_eq!(storage.remaining_bytes(), 100);

        storage.set("key1", "value1").expect("Should set");
        assert_eq!(storage.remaining_bytes(), 90);
    }

    #[test]
    fn test_storage_isolation() {
        let mut storage1 = SkillStorage::new("skill1", 1024);
        let mut storage2 = SkillStorage::new("skill2", 1024);

        storage1.set("shared_key", "value1").expect("Should set");
        storage2.set("shared_key", "value2").expect("Should set");

        assert_eq!(storage1.get("shared_key"), Some("value1".to_string()));
        assert_eq!(storage2.get("shared_key"), Some("value2".to_string()));
    }

    #[test]
    fn test_storage_update_existing_key() {
        let mut storage = SkillStorage::new("test", 50);

        storage.set("key1", "value1").expect("Should set");
        let used_after_first = storage.used_bytes();

        // Update with longer value
        storage.set("key1", "longervalue").expect("Should update");
        let used_after_update = storage.used_bytes();

        assert!(used_after_update > used_after_first);
    }

    #[test]
    fn test_storage_empty_key_value() {
        let mut storage = SkillStorage::new("test", 1024);

        storage.set("", "").expect("Should handle empty");
        assert_eq!(storage.get(""), Some("".to_string()));
        assert_eq!(storage.used_bytes(), 0);
    }

    #[test]
    fn test_storage_quota_with_update() {
        let mut storage = SkillStorage::new("test", 30);

        // Initial: "key1" (4) + "value1" (6) = 10 bytes
        storage.set("key1", "value1").expect("Should set");

        // Update with smaller value should work
        // "key1" (4) + "val" (3) = 7 bytes
        storage.set("key1", "val").expect("Should update");
        assert_eq!(storage.used_bytes(), 7);

        // Now we have 23 bytes remaining
        // "key2" (4) + "x".repeat(20) (20) = 24 bytes would exceed
        storage.set("key2", &"x".repeat(19)).expect("Should fit");
    }
}
