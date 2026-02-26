//! Credential storage wrapper.

use crate::encrypted_store::EncryptedStore;
use fx_core::error::StorageError;

type Result<T> = std::result::Result<T, StorageError>;

/// Table name for encrypted credentials in the database.
///
/// This constant is public to allow external tools (debugging, backup, etc.)
/// to inspect the database structure if needed.
pub const CREDENTIALS_TABLE: &str = "credentials";

/// Encrypted credential storage.
pub struct CredentialStore {
    store: EncryptedStore,
}

impl CredentialStore {
    /// Create a new credential store wrapping an encrypted store.
    pub fn new(store: EncryptedStore) -> Self {
        Self { store }
    }

    /// Store a credential.
    pub fn store_credential(&self, name: &str, value: &str) -> Result<()> {
        self.store.put(CREDENTIALS_TABLE, name, value.as_bytes())
    }

    /// Retrieve a credential by name.
    pub fn get_credential(&self, name: &str) -> Result<Option<String>> {
        let data = self.store.get(CREDENTIALS_TABLE, name)?;
        match data {
            Some(bytes) => {
                let s = String::from_utf8(bytes).map_err(|e| {
                    StorageError::Database(format!("Invalid UTF-8 in credential: {e}"))
                })?;
                Ok(Some(s))
            }
            None => Ok(None),
        }
    }

    /// Delete a credential. Returns true if the credential existed.
    pub fn delete_credential(&self, name: &str) -> Result<bool> {
        self.store.delete(CREDENTIALS_TABLE, name)
    }

    /// List all credential names.
    pub fn list_credentials(&self) -> Result<Vec<String>> {
        self.store.list_keys(CREDENTIALS_TABLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::EncryptionKey;
    use crate::store::Storage;

    fn create_credential_store() -> CredentialStore {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        let key = EncryptionKey::from_bytes(&[5u8; 32]);
        let encrypted = EncryptedStore::new(storage, key);
        CredentialStore::new(encrypted)
    }

    #[test]
    fn test_store_and_get_credential() {
        let store = create_credential_store();

        store
            .store_credential("api_key", "secret123")
            .expect("Failed to store");
        let value = store.get_credential("api_key").expect("Failed to get");

        assert_eq!(value, Some("secret123".to_string()));
    }

    #[test]
    fn test_get_nonexistent_credential() {
        let store = create_credential_store();
        let value = store.get_credential("nonexistent").expect("Failed to get");
        assert_eq!(value, None);
    }

    #[test]
    fn test_delete_credential() {
        let store = create_credential_store();

        store
            .store_credential("temp_key", "temp_value")
            .expect("Failed to store");
        let existed = store
            .delete_credential("temp_key")
            .expect("Failed to delete");
        assert!(existed);

        let value = store.get_credential("temp_key").expect("Failed to get");
        assert_eq!(value, None);
    }

    #[test]
    fn test_delete_nonexistent_credential() {
        let store = create_credential_store();
        let existed = store
            .delete_credential("nonexistent")
            .expect("Failed to delete");
        assert!(!existed);
    }

    #[test]
    fn test_overwrite_credential() {
        let store = create_credential_store();

        store
            .store_credential("key1", "value1")
            .expect("Failed to store");
        store
            .store_credential("key1", "value2")
            .expect("Failed to overwrite");

        let value = store.get_credential("key1").expect("Failed to get");
        assert_eq!(value, Some("value2".to_string()));
    }

    #[test]
    fn test_list_credentials() {
        let store = create_credential_store();

        store
            .store_credential("cred1", "value1")
            .expect("Failed to store");
        store
            .store_credential("cred2", "value2")
            .expect("Failed to store");
        store
            .store_credential("cred3", "value3")
            .expect("Failed to store");

        let mut creds = store.list_credentials().expect("Failed to list");
        creds.sort();

        assert_eq!(creds, vec!["cred1", "cred2", "cred3"]);
    }

    #[test]
    fn test_list_credentials_empty() {
        let store = create_credential_store();
        let creds = store.list_credentials().expect("Failed to list");
        assert_eq!(creds, Vec::<String>::new());
    }
}
