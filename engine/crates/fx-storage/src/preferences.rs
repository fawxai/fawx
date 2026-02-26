//! User preferences storage.

use crate::encrypted_store::EncryptedStore;
use fx_core::error::StorageError;
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, StorageError>;

/// Table name for encrypted user preferences in the database.
///
/// This constant is public to allow external tools (debugging, backup, etc.)
/// to inspect the database structure if needed.
pub const PREFERENCES_TABLE: &str = "preferences";

/// Encrypted preferences storage.
pub struct Preferences {
    store: EncryptedStore,
}

impl Preferences {
    /// Create a new preferences store.
    pub fn new(store: EncryptedStore) -> Self {
        Self { store }
    }

    /// Set a preference value.
    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.store.put_json(PREFERENCES_TABLE, key, value)
    }

    /// Get a preference value.
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        self.store.get_json(PREFERENCES_TABLE, key)
    }

    /// Delete a preference. Returns true if it existed.
    pub fn delete(&self, key: &str) -> Result<bool> {
        self.store.delete(PREFERENCES_TABLE, key)
    }

    /// List all preference keys.
    pub fn list_keys(&self) -> Result<Vec<String>> {
        self.store.list_keys(PREFERENCES_TABLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::EncryptionKey;
    use crate::store::Storage;
    use serde::{Deserialize, Serialize};

    fn create_preferences() -> Preferences {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        let key = EncryptionKey::from_bytes(&[9u8; 32]);
        let encrypted = EncryptedStore::new(storage, key);
        Preferences::new(encrypted)
    }

    #[test]
    fn test_set_get_string() {
        let prefs = create_preferences();
        let value = "dark".to_string();

        prefs.set("theme", &value).expect("Failed to set");
        let retrieved: Option<String> = prefs.get("theme").expect("Failed to get");

        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_set_get_number() {
        let prefs = create_preferences();
        let value = 42i32;

        prefs.set("volume", &value).expect("Failed to set");
        let retrieved: Option<i32> = prefs.get("volume").expect("Failed to get");

        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_set_get_bool() {
        let prefs = create_preferences();
        let value = true;

        prefs.set("notifications", &value).expect("Failed to set");
        let retrieved: Option<bool> = prefs.get("notifications").expect("Failed to get");

        assert_eq!(retrieved, Some(value));
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct DisplayPrefs {
        brightness: u8,
        auto_rotate: bool,
    }

    #[test]
    fn test_set_get_struct() {
        let prefs = create_preferences();
        let value = DisplayPrefs {
            brightness: 80,
            auto_rotate: true,
        };

        prefs.set("display", &value).expect("Failed to set");
        let retrieved: Option<DisplayPrefs> = prefs.get("display").expect("Failed to get");

        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_get_nonexistent() {
        let prefs = create_preferences();
        let result: Option<String> = prefs.get("nonexistent").expect("Failed to get");
        assert_eq!(result, None);
    }

    #[test]
    fn test_delete() {
        let prefs = create_preferences();

        prefs.set("temp", &"value").expect("Failed to set");
        let existed = prefs.delete("temp").expect("Failed to delete");
        assert!(existed);

        let result: Option<String> = prefs.get("temp").expect("Failed to get");
        assert_eq!(result, None);
    }

    #[test]
    fn test_delete_nonexistent() {
        let prefs = create_preferences();
        let existed = prefs.delete("nonexistent").expect("Failed to delete");
        assert!(!existed);
    }

    #[test]
    fn test_overwrite() {
        let prefs = create_preferences();

        prefs.set("key", &100).expect("Failed to set");
        prefs.set("key", &200).expect("Failed to overwrite");

        let retrieved: Option<i32> = prefs.get("key").expect("Failed to get");
        assert_eq!(retrieved, Some(200));
    }

    #[test]
    fn test_list_keys() {
        let prefs = create_preferences();

        prefs.set("pref1", &"value1").expect("Failed to set");
        prefs.set("pref2", &42).expect("Failed to set");
        prefs.set("pref3", &true).expect("Failed to set");

        let mut keys = prefs.list_keys().expect("Failed to list");
        keys.sort();

        assert_eq!(keys, vec!["pref1", "pref2", "pref3"]);
    }

    #[test]
    fn test_list_keys_empty() {
        let prefs = create_preferences();
        let keys = prefs.list_keys().expect("Failed to list");
        assert_eq!(keys, Vec::<String>::new());
    }
}
