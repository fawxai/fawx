//! Conversation history storage.

use crate::encrypted_store::EncryptedStore;
use fx_core::error::StorageError;
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, StorageError>;

/// Table name for encrypted conversation history in the database.
///
/// This constant is public to allow external tools (debugging, backup, etc.)
/// to inspect the database structure if needed.
pub const CONVERSATIONS_TABLE: &str = "conversations";

/// A message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// The role (e.g., "user", "assistant", "system")
    pub role: String,
    /// The message content
    pub content: String,
}

impl Message {
    /// Create a new message.
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

/// Encrypted conversation history storage.
pub struct StoredConversationHistory {
    store: EncryptedStore,
}

impl StoredConversationHistory {
    /// Create a new conversation history store.
    pub fn new(store: EncryptedStore) -> Self {
        Self { store }
    }

    /// Save a conversation.
    pub fn save_conversation(&self, id: &str, messages: &[Message]) -> Result<()> {
        self.store
            .put_json(CONVERSATIONS_TABLE, id, &messages.to_vec())
    }

    /// Load a conversation by ID.
    pub fn load_conversation(&self, id: &str) -> Result<Option<Vec<Message>>> {
        self.store.get_json(CONVERSATIONS_TABLE, id)
    }

    /// List all conversation IDs.
    pub fn list_conversations(&self) -> Result<Vec<String>> {
        self.store.list_keys(CONVERSATIONS_TABLE)
    }

    /// Delete a conversation. Returns true if it existed.
    pub fn delete_conversation(&self, id: &str) -> Result<bool> {
        self.store.delete(CONVERSATIONS_TABLE, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::EncryptionKey;
    use crate::store::Storage;

    fn create_conversation_store() -> StoredConversationHistory {
        let storage = Storage::open_in_memory().expect("Failed to create storage");
        let key = EncryptionKey::from_bytes(&[7u8; 32]);
        let encrypted = EncryptedStore::new(storage, key);
        StoredConversationHistory::new(encrypted)
    }

    #[test]
    fn test_save_and_load_conversation() {
        let store = create_conversation_store();
        let messages = vec![
            Message::new("user", "Hello"),
            Message::new("assistant", "Hi there!"),
            Message::new("user", "How are you?"),
        ];

        store
            .save_conversation("conv1", &messages)
            .expect("Failed to save");
        let loaded = store.load_conversation("conv1").expect("Failed to load");

        assert_eq!(loaded, Some(messages));
    }

    #[test]
    fn test_load_nonexistent_conversation() {
        let store = create_conversation_store();
        let result = store
            .load_conversation("nonexistent")
            .expect("Failed to load");
        assert_eq!(result, None);
    }

    #[test]
    fn test_delete_conversation() {
        let store = create_conversation_store();
        let messages = vec![Message::new("user", "test")];

        store
            .save_conversation("conv1", &messages)
            .expect("Failed to save");
        let existed = store
            .delete_conversation("conv1")
            .expect("Failed to delete");
        assert!(existed);

        let result = store.load_conversation("conv1").expect("Failed to load");
        assert_eq!(result, None);
    }

    #[test]
    fn test_delete_nonexistent_conversation() {
        let store = create_conversation_store();
        let existed = store
            .delete_conversation("nonexistent")
            .expect("Failed to delete");
        assert!(!existed);
    }

    #[test]
    fn test_overwrite_conversation() {
        let store = create_conversation_store();
        let messages1 = vec![Message::new("user", "First")];
        let messages2 = vec![Message::new("user", "Second")];

        store
            .save_conversation("conv1", &messages1)
            .expect("Failed to save");
        store
            .save_conversation("conv1", &messages2)
            .expect("Failed to overwrite");

        let loaded = store.load_conversation("conv1").expect("Failed to load");
        assert_eq!(loaded, Some(messages2));
    }

    #[test]
    fn test_empty_conversation() {
        let store = create_conversation_store();
        let messages: Vec<Message> = vec![];

        store
            .save_conversation("empty", &messages)
            .expect("Failed to save");
        let loaded = store.load_conversation("empty").expect("Failed to load");

        assert_eq!(loaded, Some(messages));
    }

    #[test]
    fn test_list_conversations() {
        let store = create_conversation_store();
        let msg = vec![Message::new("user", "test")];

        store
            .save_conversation("conv1", &msg)
            .expect("Failed to save");
        store
            .save_conversation("conv2", &msg)
            .expect("Failed to save");
        store
            .save_conversation("conv3", &msg)
            .expect("Failed to save");

        let mut convs = store.list_conversations().expect("Failed to list");
        convs.sort();

        assert_eq!(convs, vec!["conv1", "conv2", "conv3"]);
    }

    #[test]
    fn test_list_conversations_empty() {
        let store = create_conversation_store();
        let convs = store.list_conversations().expect("Failed to list");
        assert_eq!(convs, Vec::<String>::new());
    }
}
