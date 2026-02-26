//! Transparent encryption layer on top of Storage.

use crate::crypto::{decrypt, encrypt, EncryptionKey};
use crate::store::Storage;
use fx_core::error::StorageError;
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, StorageError>;

/// Encrypted key-value store with transparent encryption/decryption.
pub struct EncryptedStore {
    storage: Storage,
    key: EncryptionKey,
}

impl EncryptedStore {
    /// Create a new encrypted store wrapping the given storage and encryption key.
    pub fn new(storage: Storage, key: EncryptionKey) -> Self {
        Self { storage, key }
    }

    /// Store an encrypted value.
    pub fn put(&self, table: &str, key: &str, value: &[u8]) -> Result<()> {
        let encrypted = encrypt(&self.key, value)?;
        self.storage.put(table, key, &encrypted)
    }

    /// Retrieve and decrypt a value.
    pub fn get(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let encrypted = self.storage.get(table, key)?;
        match encrypted {
            Some(data) => {
                let decrypted = decrypt(&self.key, &data)?;
                Ok(Some(decrypted))
            }
            None => Ok(None),
        }
    }

    /// Delete a key.
    pub fn delete(&self, table: &str, key: &str) -> Result<bool> {
        self.storage.delete(table, key)
    }

    /// Store a JSON-serializable value.
    pub fn put_json<T: Serialize>(&self, table: &str, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_vec(value)
            .map_err(|e| StorageError::Database(format!("Failed to serialize JSON: {e}")))?;
        self.put(table, key, &json)
    }

    /// Retrieve and deserialize a JSON value.
    pub fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        table: &str,
        key: &str,
    ) -> Result<Option<T>> {
        let data = self.get(table, key)?;
        match data {
            Some(json) => {
                let value = serde_json::from_slice(&json).map_err(|e| {
                    StorageError::Database(format!("Failed to deserialize JSON: {e}"))
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// List all keys in a table.
    pub fn list_keys(&self, table: &str) -> Result<Vec<String>> {
        self.storage.list_keys(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    fn test_key() -> EncryptionKey {
        EncryptionKey::from_bytes(&[1u8; 32])
    }

    fn create_store() -> EncryptedStore {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        EncryptedStore::new(storage, test_key())
    }

    #[test]
    fn test_transparent_encrypt_decrypt() {
        let store = create_store();
        let plaintext = b"secret data";

        store.put("test", "key1", plaintext).expect("Failed to put");
        let retrieved = store.get("test", "key1").expect("Failed to get");

        assert_eq!(retrieved, Some(plaintext.to_vec()));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let store = create_store();
        let result = store.get("test", "nonexistent").expect("Failed to get");
        assert_eq!(result, None);
    }

    #[test]
    fn test_delete() {
        let store = create_store();

        store.put("test", "key1", b"value1").expect("Failed to put");
        let existed = store.delete("test", "key1").expect("Failed to delete");
        assert!(existed);

        let result = store.get("test", "key1").expect("Failed to get");
        assert_eq!(result, None);
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestStruct {
        name: String,
        value: i32,
        active: bool,
    }

    #[test]
    fn test_put_json_get_json() {
        let store = create_store();
        let data = TestStruct {
            name: "test".to_string(),
            value: 42,
            active: true,
        };

        store
            .put_json("test", "struct1", &data)
            .expect("Failed to put JSON");
        let retrieved: Option<TestStruct> = store
            .get_json("test", "struct1")
            .expect("Failed to get JSON");

        assert_eq!(retrieved, Some(data));
    }

    #[test]
    fn test_get_json_nonexistent() {
        let store = create_store();
        let result: Option<TestStruct> = store
            .get_json("test", "nonexistent")
            .expect("Failed to get JSON");
        assert_eq!(result, None);
    }

    #[test]
    fn test_put_json_string() {
        let store = create_store();
        let value = "hello world".to_string();

        store
            .put_json("test", "string1", &value)
            .expect("Failed to put JSON");
        let retrieved: Option<String> = store
            .get_json("test", "string1")
            .expect("Failed to get JSON");

        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_put_json_number() {
        let store = create_store();
        let value = 12345i64;

        store
            .put_json("test", "number1", &value)
            .expect("Failed to put JSON");
        let retrieved: Option<i64> = store
            .get_json("test", "number1")
            .expect("Failed to get JSON");

        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_put_json_bool() {
        let store = create_store();
        let value = true;

        store
            .put_json("test", "bool1", &value)
            .expect("Failed to put JSON");
        let retrieved: Option<bool> = store.get_json("test", "bool1").expect("Failed to get JSON");

        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_list_keys() {
        let store = create_store();

        store.put("test", "key1", b"value1").expect("Failed to put");
        store.put("test", "key2", b"value2").expect("Failed to put");
        store.put("test", "key3", b"value3").expect("Failed to put");

        let mut keys = store.list_keys("test").expect("Failed to list");
        keys.sort();

        assert_eq!(keys, vec!["key1", "key2", "key3"]);
    }
}
