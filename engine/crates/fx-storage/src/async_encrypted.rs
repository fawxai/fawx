//! Async wrapper for EncryptedStore using tokio::task::spawn_blocking.

use crate::crypto::EncryptionKey;
use crate::encrypted_store::EncryptedStore;
use crate::store::Storage;
use fx_core::error::StorageError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

type Result<T> = std::result::Result<T, StorageError>;

/// Async wrapper for EncryptedStore.
///
/// Wraps the synchronous EncryptedStore in async interfaces using
/// `tokio::task::spawn_blocking` to avoid blocking the async runtime.
#[derive(Clone)]
pub struct AsyncEncryptedStore {
    store: Arc<EncryptedStore>,
}

impl AsyncEncryptedStore {
    /// Create a new async encrypted store.
    pub fn new(storage: Storage, key: EncryptionKey) -> Self {
        Self {
            store: Arc::new(EncryptedStore::new(storage, key)),
        }
    }

    /// Store an encrypted value.
    pub async fn put(&self, table: &str, key: &str, value: &[u8]) -> Result<()> {
        let store = Arc::clone(&self.store);
        let err_table = table.to_string();
        let err_key = key.to_string();
        let table = table.to_string();
        let key = key.to_string();
        let value = value.to_vec();

        tokio::task::spawn_blocking(move || store.put(&table, &key, &value))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncEncryptedStore::put(table='{}', key='{}'): {}",
                    err_table, err_key, e
                ))
            })?
    }

    /// Retrieve and decrypt a value.
    pub async fn get(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let store = Arc::clone(&self.store);
        let err_table = table.to_string();
        let err_key = key.to_string();
        let table = table.to_string();
        let key = key.to_string();

        tokio::task::spawn_blocking(move || store.get(&table, &key))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncEncryptedStore::get(table='{}', key='{}'): {}",
                    err_table, err_key, e
                ))
            })?
    }

    /// Delete a key.
    pub async fn delete(&self, table: &str, key: &str) -> Result<bool> {
        let store = Arc::clone(&self.store);
        let err_table = table.to_string();
        let err_key = key.to_string();
        let table = table.to_string();
        let key = key.to_string();

        tokio::task::spawn_blocking(move || store.delete(&table, &key))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncEncryptedStore::delete(table='{}', key='{}'): {}",
                    err_table, err_key, e
                ))
            })?
    }

    /// Store a JSON-serializable value.
    pub async fn put_json<T: Serialize + Send + 'static>(
        &self,
        table: &str,
        key: &str,
        value: &T,
    ) -> Result<()> {
        let store = Arc::clone(&self.store);
        let err_table = table.to_string();
        let err_key = key.to_string();
        let table = table.to_string();
        let key = key.to_string();
        let json = serde_json::to_vec(value)
            .map_err(|e| StorageError::Database(format!("Failed to serialize JSON: {e}")))?;

        tokio::task::spawn_blocking(move || store.put(&table, &key, &json))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncEncryptedStore::put_json(table='{}', key='{}'): {}",
                    err_table, err_key, e
                ))
            })?
    }

    /// Retrieve and deserialize a JSON value.
    pub async fn get_json<T: for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        table: &str,
        key: &str,
    ) -> Result<Option<T>> {
        let store = Arc::clone(&self.store);
        let err_table = table.to_string();
        let err_key = key.to_string();
        let table = table.to_string();
        let key = key.to_string();

        tokio::task::spawn_blocking(move || store.get_json::<T>(&table, &key))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncEncryptedStore::get_json(table='{}', key='{}'): {}",
                    err_table, err_key, e
                ))
            })?
    }

    /// List all keys in a table.
    pub async fn list_keys(&self, table: &str) -> Result<Vec<String>> {
        let store = Arc::clone(&self.store);
        let err_table = table.to_string();
        let table = table.to_string();

        tokio::task::spawn_blocking(move || store.list_keys(&table))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncEncryptedStore::list_keys(table='{}'): {}",
                    err_table, e
                ))
            })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    fn test_key() -> EncryptionKey {
        EncryptionKey::from_bytes(&[1u8; 32])
    }

    async fn create_store() -> AsyncEncryptedStore {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        AsyncEncryptedStore::new(storage, test_key())
    }

    #[tokio::test]
    async fn test_async_encrypted_put_get() {
        let store = create_store().await;
        let plaintext = b"secret data";

        store
            .put("test", "key1", plaintext)
            .await
            .expect("Failed to put");
        let retrieved = store.get("test", "key1").await.expect("Failed to get");

        assert_eq!(retrieved, Some(plaintext.to_vec()));
    }

    #[tokio::test]
    async fn test_async_encrypted_get_nonexistent() {
        let store = create_store().await;
        let result = store
            .get("test", "nonexistent")
            .await
            .expect("Failed to get");
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_async_encrypted_delete() {
        let store = create_store().await;

        store
            .put("test", "key1", b"value1")
            .await
            .expect("Failed to put");
        let existed = store
            .delete("test", "key1")
            .await
            .expect("Failed to delete");
        assert!(existed);

        let result = store.get("test", "key1").await.expect("Failed to get");
        assert_eq!(result, None);
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestStruct {
        name: String,
        value: i32,
        active: bool,
    }

    #[tokio::test]
    async fn test_async_put_json_get_json() {
        let store = create_store().await;
        let data = TestStruct {
            name: "test".to_string(),
            value: 42,
            active: true,
        };

        store
            .put_json("test", "struct1", &data)
            .await
            .expect("Failed to put JSON");
        let retrieved: Option<TestStruct> = store
            .get_json("test", "struct1")
            .await
            .expect("Failed to get JSON");

        assert_eq!(retrieved, Some(data));
    }

    #[tokio::test]
    async fn test_async_get_json_nonexistent() {
        let store = create_store().await;
        let result: Option<TestStruct> = store
            .get_json("test", "nonexistent")
            .await
            .expect("Failed to get JSON");
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_async_list_keys() {
        let store = create_store().await;

        store
            .put("test", "key1", b"value1")
            .await
            .expect("Failed to put");
        store
            .put("test", "key2", b"value2")
            .await
            .expect("Failed to put");
        store
            .put("test", "key3", b"value3")
            .await
            .expect("Failed to put");

        let mut keys = store.list_keys("test").await.expect("Failed to list");
        keys.sort();

        assert_eq!(keys, vec!["key1", "key2", "key3"]);
    }

    #[tokio::test]
    async fn test_concurrent_json_operations() {
        let store = create_store().await;

        let data1 = TestStruct {
            name: "test1".to_string(),
            value: 1,
            active: true,
        };
        let data2 = TestStruct {
            name: "test2".to_string(),
            value: 2,
            active: false,
        };
        let data3 = TestStruct {
            name: "test3".to_string(),
            value: 3,
            active: true,
        };

        let store1 = store.clone();
        let store2 = store.clone();
        let store3 = store.clone();

        let handle1 = tokio::spawn(async move { store1.put_json("test", "struct1", &data1).await });

        let handle2 = tokio::spawn(async move { store2.put_json("test", "struct2", &data2).await });

        let handle3 = tokio::spawn(async move { store3.put_json("test", "struct3", &data3).await });

        handle1.await.unwrap().expect("Failed to put struct1");
        handle2.await.unwrap().expect("Failed to put struct2");
        handle3.await.unwrap().expect("Failed to put struct3");

        let mut keys = store.list_keys("test").await.expect("Failed to list");
        keys.sort();
        assert_eq!(keys, vec!["struct1", "struct2", "struct3"]);
    }
}
