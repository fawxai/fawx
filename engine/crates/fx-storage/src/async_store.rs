//! Async wrapper for Storage using tokio::task::spawn_blocking.

use crate::store::Storage;
use fx_core::error::StorageError;
use std::path::Path;
use std::sync::Arc;

type Result<T> = std::result::Result<T, StorageError>;

/// Async wrapper for Storage.
///
/// Wraps the synchronous redb-based Storage in async interfaces using
/// `tokio::task::spawn_blocking` to avoid blocking the async runtime.
#[derive(Clone)]
pub struct AsyncStorage {
    storage: Arc<Storage>,
}

impl AsyncStorage {
    /// Open or create a database at the given path.
    pub async fn open(path: &Path) -> Result<Self> {
        let path = path.to_path_buf();
        let storage = tokio::task::spawn_blocking(move || Storage::open(&path))
            .await
            .map_err(|e| {
                StorageError::Database(format!("Task join error in AsyncStorage::open: {e}"))
            })??;
        Ok(Self {
            storage: Arc::new(storage),
        })
    }

    /// Create an in-memory database for testing.
    pub async fn open_in_memory() -> Result<Self> {
        let storage = tokio::task::spawn_blocking(Storage::open_in_memory)
            .await
            .map_err(|e| {
                StorageError::Database(format!(
                    "Task join error in AsyncStorage::open_in_memory: {e}"
                ))
            })??;
        Ok(Self {
            storage: Arc::new(storage),
        })
    }

    /// Store a key-value pair in the specified table.
    pub async fn put(&self, table: &str, key: &str, value: &[u8]) -> Result<()> {
        let storage = Arc::clone(&self.storage);
        let err_table = table.to_string();
        let err_key = key.to_string();
        let table = table.to_string();
        let key = key.to_string();
        let value = value.to_vec();

        tokio::task::spawn_blocking(move || storage.put(&table, &key, &value))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncStorage::put(table='{}', key='{}'): {}",
                    err_table, err_key, e
                ))
            })?
    }

    /// Retrieve a value by key from the specified table.
    pub async fn get(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let storage = Arc::clone(&self.storage);
        let err_table = table.to_string();
        let err_key = key.to_string();
        let table = table.to_string();
        let key = key.to_string();

        tokio::task::spawn_blocking(move || storage.get(&table, &key))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncStorage::get(table='{}', key='{}'): {}",
                    err_table, err_key, e
                ))
            })?
    }

    /// Delete a key from the specified table. Returns true if the key existed.
    pub async fn delete(&self, table: &str, key: &str) -> Result<bool> {
        let storage = Arc::clone(&self.storage);
        let err_table = table.to_string();
        let err_key = key.to_string();
        let table = table.to_string();
        let key = key.to_string();

        tokio::task::spawn_blocking(move || storage.delete(&table, &key))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncStorage::delete(table='{}', key='{}'): {}",
                    err_table, err_key, e
                ))
            })?
    }

    /// List all keys in the specified table.
    pub async fn list_keys(&self, table: &str) -> Result<Vec<String>> {
        let storage = Arc::clone(&self.storage);
        let err_table = table.to_string();
        let table = table.to_string();

        tokio::task::spawn_blocking(move || storage.list_keys(&table))
            .await
            .map_err(move |e| {
                StorageError::Database(format!(
                    "Task join error in AsyncStorage::list_keys(table='{}'): {}",
                    err_table, e
                ))
            })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_async_put_get() {
        let storage = AsyncStorage::open_in_memory()
            .await
            .expect("Failed to create storage");

        storage
            .put("test", "key1", b"value1")
            .await
            .expect("Failed to put");

        let value = storage.get("test", "key1").await.expect("Failed to get");
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[tokio::test]
    async fn test_async_get_nonexistent() {
        let storage = AsyncStorage::open_in_memory()
            .await
            .expect("Failed to create storage");
        let value = storage
            .get("test", "nonexistent")
            .await
            .expect("Failed to get");
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_async_delete() {
        let storage = AsyncStorage::open_in_memory()
            .await
            .expect("Failed to create storage");

        storage
            .put("test", "key1", b"value1")
            .await
            .expect("Failed to put");

        let existed = storage
            .delete("test", "key1")
            .await
            .expect("Failed to delete");
        assert!(existed);

        let value = storage.get("test", "key1").await.expect("Failed to get");
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_async_delete_nonexistent() {
        let storage = AsyncStorage::open_in_memory()
            .await
            .expect("Failed to create storage");
        let existed = storage
            .delete("test", "nonexistent")
            .await
            .expect("Failed to delete");
        assert!(!existed);
    }

    #[tokio::test]
    async fn test_async_list_keys() {
        let storage = AsyncStorage::open_in_memory()
            .await
            .expect("Failed to create storage");

        storage
            .put("test", "key1", b"value1")
            .await
            .expect("Failed to put");
        storage
            .put("test", "key2", b"value2")
            .await
            .expect("Failed to put");
        storage
            .put("test", "key3", b"value3")
            .await
            .expect("Failed to put");

        let mut keys = storage
            .list_keys("test")
            .await
            .expect("Failed to list keys");
        keys.sort();

        assert_eq!(keys, vec!["key1", "key2", "key3"]);
    }

    #[tokio::test]
    async fn test_async_list_keys_empty() {
        let storage = AsyncStorage::open_in_memory()
            .await
            .expect("Failed to create storage");
        let keys = storage
            .list_keys("empty")
            .await
            .expect("Failed to list keys");
        assert_eq!(keys, Vec::<String>::new());
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        let storage = AsyncStorage::open_in_memory()
            .await
            .expect("Failed to create storage");

        // Spawn multiple concurrent operations
        let storage1 = storage.clone();
        let storage2 = storage.clone();
        let storage3 = storage.clone();

        let handle1 = tokio::spawn(async move { storage1.put("test", "key1", b"value1").await });

        let handle2 = tokio::spawn(async move { storage2.put("test", "key2", b"value2").await });

        let handle3 = tokio::spawn(async move { storage3.put("test", "key3", b"value3").await });

        // Wait for all to complete
        handle1.await.unwrap().expect("Failed to put key1");
        handle2.await.unwrap().expect("Failed to put key2");
        handle3.await.unwrap().expect("Failed to put key3");

        // Verify all keys exist
        let mut keys = storage
            .list_keys("test")
            .await
            .expect("Failed to list keys");
        keys.sort();
        assert_eq!(keys, vec!["key1", "key2", "key3"]);
    }
}
