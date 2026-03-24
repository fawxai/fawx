//! Core storage layer using redb.

use fx_core::error::StorageError;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Arc;

type Result<T> = std::result::Result<T, StorageError>;

/// Core storage backend using redb (embedded key-value database).
#[derive(Clone)]
pub struct Storage {
    db: Arc<Database>,
}

impl Storage {
    /// Open or create a database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Database::create(path)
            .map_err(|e| StorageError::Database(format!("Failed to open database: {e}")))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Create an in-memory database for testing.
    pub fn open_in_memory() -> Result<Self> {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .map_err(|e| {
                StorageError::Database(format!("Failed to create in-memory database: {e}"))
            })?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Store a key-value pair in the specified table.
    pub fn put(&self, table: &str, key: &str, value: &[u8]) -> Result<()> {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table);
        let write_txn = self.db.begin_write().map_err(|e| {
            StorageError::Database(format!("Failed to begin write transaction: {e}"))
        })?;

        {
            let mut table_handle = write_txn.open_table(table_def).map_err(|e| {
                StorageError::Database(format!("Failed to open table '{table}': {e}"))
            })?;

            table_handle.insert(key, value).map_err(|e| {
                StorageError::Database(format!("Failed to insert key '{key}': {e}"))
            })?;
        }

        write_txn
            .commit()
            .map_err(|e| StorageError::Database(format!("Failed to commit transaction: {e}")))?;

        Ok(())
    }

    /// Retrieve a value by key from the specified table.
    pub fn get(&self, table: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table);
        let read_txn = self.db.begin_read().map_err(|e| {
            StorageError::Database(format!("Failed to begin read transaction: {e}"))
        })?;

        let table_handle = match read_txn.open_table(table_def) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => {
                return Err(StorageError::Database(format!(
                    "Failed to open table '{table}': {e}"
                )))
            }
        };

        let value = table_handle
            .get(key)
            .map_err(|e| StorageError::Database(format!("Failed to get key '{key}': {e}")))?
            .map(|v| v.value().to_vec());

        Ok(value)
    }

    /// Delete a key from the specified table. Returns true if the key existed.
    pub fn delete(&self, table: &str, key: &str) -> Result<bool> {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table);
        let write_txn = self.db.begin_write().map_err(|e| {
            StorageError::Database(format!("Failed to begin write transaction: {e}"))
        })?;

        let existed = {
            let mut table_handle = match write_txn.open_table(table_def) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(false),
                Err(e) => {
                    return Err(StorageError::Database(format!(
                        "Failed to open table '{table}': {e}"
                    )))
                }
            };

            let result = table_handle
                .remove(key)
                .map_err(|e| StorageError::Database(format!("Failed to delete key '{key}': {e}")))?
                .is_some();
            result
        };

        write_txn
            .commit()
            .map_err(|e| StorageError::Database(format!("Failed to commit transaction: {e}")))?;

        Ok(existed)
    }

    /// Delete multiple keys in a single write transaction.
    pub fn delete_many(&self, table: &str, keys: &[String]) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table);
        let write_txn = self.db.begin_write().map_err(|e| {
            StorageError::Database(format!("Failed to begin write transaction: {e}"))
        })?;

        {
            let mut table_handle = match write_txn.open_table(table_def) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(e) => {
                    return Err(StorageError::Database(format!(
                        "Failed to open table '{table}': {e}"
                    )))
                }
            };
            for key in keys {
                table_handle.remove(key.as_str()).map_err(|e| {
                    StorageError::Database(format!("Failed to delete key '{key}': {e}"))
                })?;
            }
        }

        write_txn
            .commit()
            .map_err(|e| StorageError::Database(format!("Failed to commit transaction: {e}")))?;

        Ok(())
    }

    /// List all keys in the specified table.
    pub fn list_keys(&self, table: &str) -> Result<Vec<String>> {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table);
        let read_txn = self.db.begin_read().map_err(|e| {
            StorageError::Database(format!("Failed to begin read transaction: {e}"))
        })?;

        let table_handle = match read_txn.open_table(table_def) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => {
                return Err(StorageError::Database(format!(
                    "Failed to open table '{table}': {e}"
                )))
            }
        };

        let mut keys = Vec::new();
        let iter = table_handle.iter().map_err(|e| {
            StorageError::Database(format!("Failed to iterate table '{table}': {e}"))
        })?;

        for item in iter {
            let (k, _) = item.map_err(|e| {
                StorageError::Database(format!("Failed to read item from table '{table}': {e}"))
            })?;
            keys.push(k.value().to_string());
        }

        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_storage() {
        let storage = Storage::open_in_memory().expect("Failed to create in-memory storage");

        // Put and get
        storage
            .put("test", "key1", b"value1")
            .expect("Failed to put");
        let value = storage.get("test", "key1").expect("Failed to get");
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_put_and_get() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");

        storage
            .put("table1", "key1", b"value1")
            .expect("Failed to put");
        storage
            .put("table1", "key2", b"value2")
            .expect("Failed to put");

        let value1 = storage.get("table1", "key1").expect("Failed to get");
        let value2 = storage.get("table1", "key2").expect("Failed to get");

        assert_eq!(value1, Some(b"value1".to_vec()));
        assert_eq!(value2, Some(b"value2".to_vec()));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        let value = storage.get("test", "nonexistent").expect("Failed to get");
        assert_eq!(value, None);
    }

    #[test]
    fn test_get_nonexistent_table() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        let value = storage
            .get("nonexistent_table", "key")
            .expect("Failed to get");
        assert_eq!(value, None);
    }

    #[test]
    fn test_delete() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");

        storage
            .put("test", "key1", b"value1")
            .expect("Failed to put");

        let existed = storage.delete("test", "key1").expect("Failed to delete");
        assert!(existed);

        let value = storage.get("test", "key1").expect("Failed to get");
        assert_eq!(value, None);
    }

    #[test]
    fn test_delete_nonexistent_key() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        let existed = storage
            .delete("test", "nonexistent")
            .expect("Failed to delete");
        assert!(!existed);
    }

    #[test]
    fn test_delete_many_removes_multiple_keys() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        storage.put("test", "key1", b"value1").expect("put key1");
        storage.put("test", "key2", b"value2").expect("put key2");
        storage.put("test", "key3", b"value3").expect("put key3");

        storage
            .delete_many("test", &["key1".to_string(), "key3".to_string()])
            .expect("delete many");

        assert_eq!(storage.get("test", "key1").expect("get key1"), None);
        assert_eq!(
            storage.get("test", "key2").expect("get key2"),
            Some(b"value2".to_vec())
        );
        assert_eq!(storage.get("test", "key3").expect("get key3"), None);
    }

    #[test]
    fn test_delete_from_nonexistent_table() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        let existed = storage
            .delete("nonexistent_table", "key")
            .expect("Failed to delete");
        assert!(!existed);
    }

    #[test]
    fn test_list_keys() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");

        storage
            .put("test", "key1", b"value1")
            .expect("Failed to put");
        storage
            .put("test", "key2", b"value2")
            .expect("Failed to put");
        storage
            .put("test", "key3", b"value3")
            .expect("Failed to put");

        let mut keys = storage.list_keys("test").expect("Failed to list keys");
        keys.sort();

        assert_eq!(keys, vec!["key1", "key2", "key3"]);
    }

    #[test]
    fn test_list_keys_empty_table() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        let keys = storage
            .list_keys("nonexistent_table")
            .expect("Failed to list keys");
        assert_eq!(keys, Vec::<String>::new());
    }

    #[test]
    fn test_multiple_tables() {
        let storage = Storage::open_in_memory().expect("Failed to create storage");

        storage
            .put("table1", "key", b"value1")
            .expect("Failed to put");
        storage
            .put("table2", "key", b"value2")
            .expect("Failed to put");

        let value1 = storage.get("table1", "key").expect("Failed to get");
        let value2 = storage.get("table2", "key").expect("Failed to get");

        assert_eq!(value1, Some(b"value1".to_vec()));
        assert_eq!(value2, Some(b"value2".to_vec()));
    }
}
