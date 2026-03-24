//! Encrypted persistent storage.
//!
//! Provides encrypted key-value storage for credentials, conversation history,
//! preferences, and other persistent data.
//!
//! This crate provides both synchronous and asynchronous interfaces:
//! - Sync: `Storage`, `EncryptedStore`
//! - Async: `AsyncStorage`, `AsyncEncryptedStore` (using `tokio::task::spawn_blocking`)

// Memory stores (planned — four types: working, episodic, semantic, procedural)
pub mod graph;

pub mod async_encrypted;
pub mod async_store;
pub mod conversation;
pub mod credentials;
pub mod crypto;
pub mod encrypted_store;
pub mod key_derivation;
pub mod preferences;
pub mod store;

// Re-export key types for convenience
pub use async_encrypted::AsyncEncryptedStore;
pub use async_store::AsyncStorage;
pub use conversation::{Message, StoredConversationHistory};
pub use credentials::CredentialStore;
pub use crypto::EncryptionKey;
pub use encrypted_store::EncryptedStore;
pub use key_derivation::{derive_key, derive_key_from_password};
pub use preferences::Preferences;
pub use store::Storage;
