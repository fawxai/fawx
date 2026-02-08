//! Encrypted persistent storage.
//!
//! Provides encrypted key-value storage for credentials, conversation history,
//! preferences, and other persistent data.

pub mod conversation;
pub mod credentials;
pub mod crypto;
pub mod encrypted_store;
pub mod key_derivation;
pub mod preferences;
pub mod store;

// Re-export key types for convenience
pub use conversation::{Message, StoredConversationHistory};
pub use credentials::CredentialStore;
pub use crypto::EncryptionKey;
pub use encrypted_store::EncryptedStore;
pub use key_derivation::{derive_key, derive_key_from_password};
pub use preferences::Preferences;
pub use store::Storage;
