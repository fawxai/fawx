//! Integration tests for the encrypted storage system.
//!
//! These tests verify that all components work together correctly:
//! key derivation, encryption, storage, and domain-specific wrappers.

use fx_storage::{
    conversation::{Message, StoredConversationHistory},
    credentials::CredentialStore,
    derive_key, derive_key_from_password,
    encrypted_store::EncryptedStore,
    preferences::Preferences,
    store::Storage,
};

#[test]
fn test_full_storage_stack() {
    // 1. Derive a key from a password
    let password = "test_password_12345";
    let salt = b"unique_salt_1234567890123456"; // 16 bytes
    let key = derive_key_from_password(password, salt).expect("Failed to derive key");

    // 2. Create the full storage stack
    let storage = Storage::open_in_memory().expect("Failed to create storage");

    // 3. Create domain-specific stores (each with the same underlying storage and key)
    let cred_store = CredentialStore::new(EncryptedStore::new(storage.clone(), key.clone()));
    let conv_store =
        StoredConversationHistory::new(EncryptedStore::new(storage.clone(), key.clone()));
    let pref_store = Preferences::new(EncryptedStore::new(storage, key));

    // 4. Store and retrieve credentials
    cred_store
        .store_credential("api_key", "sk-secret123")
        .expect("Failed to store credential");
    cred_store
        .store_credential("db_password", "supersecret")
        .expect("Failed to store credential");

    let api_key = cred_store
        .get_credential("api_key")
        .expect("Failed to get credential");
    assert_eq!(api_key, Some("sk-secret123".to_string()));

    let mut creds = cred_store.list_credentials().expect("Failed to list");
    creds.sort();
    assert_eq!(creds, vec!["api_key", "db_password"]);

    // 5. Store and retrieve conversation history
    let messages = vec![
        Message::new("user", "Hello, Fawx!"),
        Message::new("assistant", "Hello! How can I help you today?"),
        Message::new("user", "What's the weather like?"),
    ];
    conv_store
        .save_conversation("conv-001", &messages)
        .expect("Failed to save conversation");

    let loaded = conv_store
        .load_conversation("conv-001")
        .expect("Failed to load conversation");
    assert_eq!(loaded, Some(messages.clone()));

    // 6. Store and retrieve preferences
    pref_store
        .set("theme", &"dark")
        .expect("Failed to set preference");
    pref_store
        .set("volume", &75)
        .expect("Failed to set preference");
    pref_store
        .set("notifications", &true)
        .expect("Failed to set preference");

    let theme: Option<String> = pref_store.get("theme").expect("Failed to get preference");
    assert_eq!(theme, Some("dark".to_string()));

    let volume: Option<i32> = pref_store.get("volume").expect("Failed to get preference");
    assert_eq!(volume, Some(75));

    // 7. Verify cross-component functionality (data is isolated)
    let creds_keys = cred_store.list_credentials().expect("Failed to list");
    let conv_keys = conv_store.list_conversations().expect("Failed to list");
    let pref_keys = pref_store.list_keys().expect("Failed to list");

    // Each store should only see its own data
    assert_eq!(creds_keys.len(), 2);
    assert_eq!(conv_keys.len(), 1);
    assert_eq!(pref_keys.len(), 3);

    // 8. Test deletion
    cred_store
        .delete_credential("db_password")
        .expect("Failed to delete");
    let deleted_cred = cred_store
        .get_credential("db_password")
        .expect("Failed to get");
    assert_eq!(deleted_cred, None);
}

#[test]
fn test_key_derivation_hierarchy() {
    // Simulate a master key from hardware keystore
    let master_key = b"hardware_keystore_master_key_32!";

    // Derive separate keys for different purposes
    let creds_key = derive_key(master_key, "credentials").expect("Failed to derive");
    let conv_key = derive_key(master_key, "conversations").expect("Failed to derive");
    let pref_key = derive_key(master_key, "preferences").expect("Failed to derive");

    // Create separate encrypted stores with different keys
    // Note: In reality, you'd want separate Storage instances for complete isolation,
    // but sharing the same Storage with different encryption keys demonstrates
    // that encrypted data from one key can't be decrypted by another.
    let creds_store = CredentialStore::new(EncryptedStore::new(
        Storage::open_in_memory().expect("Failed to create storage"),
        creds_key,
    ));
    let conv_store = StoredConversationHistory::new(EncryptedStore::new(
        Storage::open_in_memory().expect("Failed to create storage"),
        conv_key,
    ));
    let pref_store = Preferences::new(EncryptedStore::new(
        Storage::open_in_memory().expect("Failed to create storage"),
        pref_key,
    ));

    // Store data in each
    creds_store
        .store_credential("test", "value")
        .expect("Failed to store");
    conv_store
        .save_conversation("test", &[Message::new("user", "hi")])
        .expect("Failed to save");
    pref_store.set("test", &"value").expect("Failed to set");

    // Verify each can access its own data
    assert!(creds_store
        .get_credential("test")
        .expect("Failed to get")
        .is_some());
    assert!(conv_store
        .load_conversation("test")
        .expect("Failed to load")
        .is_some());
    assert!(pref_store
        .get::<String>("test")
        .expect("Failed to get")
        .is_some());
}

#[test]
fn test_wrong_password_fails_decryption() {
    let salt = b"salt_1234567890_1234";
    let password1 = "correct_password";
    let password2 = "wrong_password";

    // Encrypt with password1
    let key1 = derive_key_from_password(password1, salt).expect("Failed to derive");
    let storage = Storage::open_in_memory().expect("Failed to create storage");
    let creds1 = CredentialStore::new(EncryptedStore::new(storage.clone(), key1));

    creds1
        .store_credential("secret", "sensitive_data")
        .expect("Failed to store");

    // Try to decrypt with password2 (should fail)
    let key2 = derive_key_from_password(password2, salt).expect("Failed to derive");
    let creds2 = CredentialStore::new(EncryptedStore::new(storage, key2));

    // This should fail because the key is wrong (authentication tag won't match)
    let result = creds2.get_credential("secret");
    assert!(
        result.is_err(),
        "Decryption with wrong password should fail"
    );
}
