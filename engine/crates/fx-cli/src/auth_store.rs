//! Encrypted credential storage for authentication data.
//!
//! Replaces plaintext `auth.json` with AES-256-GCM encrypted storage
//! backed by SQLite via `fx-storage`. Uses machine-specific key derivation
//! so credentials are unreadable if the database file is copied elsewhere.

use fx_auth::auth::AuthManager;
use fx_storage::{derive_key, CredentialStore, EncryptedStore, EncryptionKey, Storage};
use std::fs;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// Key name used to store the serialized `AuthManager` in the credential store.
///
/// ## Design choice: single blob vs per-provider storage (NH9)
///
/// The entire `AuthManager` is serialized as one JSON blob under this key.
/// This is simple and atomic \u{2014} a single put/get round-trips all providers.
/// The tradeoff: if this blob corrupts, *all* credentials are lost rather
/// than just one.  For the current scale (a handful of providers) single-blob
/// is the right call.  Worth revisiting if the provider count grows
/// significantly.
const AUTH_MANAGER_KEY: &str = "auth_manager";

/// Application pepper baked into the key derivation to namespace Fawx keys.
const APP_PEPPER: &[u8] = b"fawx-auth-store-v1";
const PROVIDER_TOKEN_SUFFIX: &str = "_token";

/// Encrypted auth credential store backed by `fx-storage`.
///
/// Uses an **open-per-operation** pattern: the database path and encryption
/// key are stored, and a fresh `CredentialStore` connection is opened for
/// each read/write operation. This avoids holding a `redb` exclusive file
/// lock for the process lifetime, which previously blocked concurrent
/// access from the HTTP server and model router.
pub struct AuthStore {
    db_path: PathBuf,
    key: EncryptionKey,
    /// Holds the temp directory for test instances so it is cleaned up on drop.
    #[cfg(test)]
    _temp_dir: Option<tempfile::TempDir>,
}

#[allow(dead_code)] // Used by binary-only auth/setup command flows.
pub struct RecoveredAuthStore {
    pub store: AuthStore,
    pub recreated: bool,
}

impl AuthStore {
    /// Open (or create) the encrypted auth store in `data_dir`.
    ///
    /// Creates `auth.db` and `.auth-salt` inside `data_dir` on first run.
    /// The database is opened briefly to verify accessibility, then closed
    /// immediately — no long-lived file lock is held.
    pub fn open(data_dir: &Path) -> Result<Self, String> {
        let key = get_or_create_auth_key(data_dir)?;
        let db_path = data_dir.join("auth.db");

        // Verify the DB can be opened (creates if needed), then close.
        let _verify =
            Storage::open(&db_path).map_err(|e| format!("failed to open auth database: {e}"))?;

        Ok(Self {
            db_path,
            key,
            #[cfg(test)]
            _temp_dir: None,
        })
    }

    /// Open a temporary on-disk auth store for testing.
    ///
    /// Creates a `tempfile::TempDir` that is automatically cleaned up when
    /// the returned `AuthStore` is dropped, so test temp files don't leak.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn open_for_testing() -> Result<Self, String> {
        let temp_dir =
            tempfile::TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
        let db_path = temp_dir.path().join("auth-test.db");
        let key = EncryptionKey::from_bytes(&[42u8; 32]);
        // Create the DB file so subsequent open_store calls succeed.
        let _verify = Storage::open(&db_path).map_err(|e| format!("test storage: {e}"))?;
        Ok(Self {
            db_path,
            key,
            _temp_dir: Some(temp_dir),
        })
    }

    /// Open a short-lived connection for a single operation.
    fn open_store(&self) -> Result<CredentialStore, String> {
        let storage = Storage::open(&self.db_path)
            .map_err(|e| format!("failed to open auth database: {e}"))?;
        let encrypted = EncryptedStore::new(storage, self.key.clone());
        Ok(CredentialStore::new(encrypted))
    }

    /// Persist an `AuthManager` into the encrypted store.
    pub fn save_auth_manager(&self, auth: &AuthManager) -> Result<(), String> {
        let store = self.open_store()?;
        let json = auth.to_json().map_err(|e| e.to_string())?;
        store
            .store_credential(AUTH_MANAGER_KEY, &json)
            .map_err(|e| format!("failed to store credentials: {e}"))
    }

    /// Load the `AuthManager` from the encrypted store.
    ///
    /// Returns an empty `AuthManager` when no data has been stored yet.
    pub fn load_auth_manager(&self) -> Result<AuthManager, String> {
        let store = self.open_store()?;
        match store.get_credential(AUTH_MANAGER_KEY) {
            Ok(Some(json)) => {
                AuthManager::from_json(&json).map_err(|e| format!("corrupted credential data: {e}"))
            }
            Ok(None) => Ok(AuthManager::new()),
            Err(e) => decode_credential_error(e),
        }
    }

    /// Store a provider token under the `<provider>_token` key.
    #[allow(dead_code)] // Used by binary-only setup/auth command flows
    pub fn store_provider_token(&self, provider: &str, token: &str) -> Result<(), String> {
        let store = self.open_store()?;
        let key = provider_token_key(provider);
        store
            .store_credential(&key, token)
            .map_err(|e| format!("failed to store provider token: {e}"))
    }

    /// Read a provider token from the `<provider>_token` key.
    ///
    /// Returns the token wrapped in [`Zeroizing`] so it is automatically
    /// zeroed when dropped, preventing secret material from lingering in
    /// memory.
    pub fn get_provider_token(&self, provider: &str) -> Result<Option<Zeroizing<String>>, String> {
        let store = self.open_store()?;
        let key = provider_token_key(provider);
        store
            .get_credential(&key)
            .map(|opt| opt.map(Zeroizing::new))
            .map_err(|e| format!("failed to read provider token: {e}"))
    }

    /// Delete a provider token. Returns true when a token existed.
    #[allow(dead_code)] // Used by feat/auth-tui-wiring PR #1166
    pub fn clear_provider_token(&self, provider: &str) -> Result<bool, String> {
        let store = self.open_store()?;
        let key = provider_token_key(provider);
        store
            .delete_credential(&key)
            .map_err(|e| format!("failed to clear provider token: {e}"))
    }

    /// List provider names that currently have `<provider>_token` entries.
    #[allow(dead_code)] // Used by feat/auth-tui-wiring PR #1166
    pub fn list_provider_tokens(&self) -> Result<Vec<String>, String> {
        let store = self.open_store()?;
        let mut providers = store
            .list_credentials()
            .map_err(|e| format!("failed to list provider tokens: {e}"))?
            .into_iter()
            .filter_map(|key| {
                key.strip_suffix(PROVIDER_TOKEN_SUFFIX)
                    .map(|p| p.to_string())
            })
            .collect::<Vec<_>>();
        providers.sort();
        providers.dedup();
        Ok(providers)
    }
}

#[cfg(feature = "http")]
impl fx_api::token::BearerTokenStore for AuthStore {
    fn get_provider_token(&self, provider: &str) -> Result<Option<String>, String> {
        AuthStore::get_provider_token(self, provider)
            .map(|token| token.map(|token| token.to_string()))
    }
}

#[allow(dead_code)] // Used by binary-only auth/setup command flows.
pub fn open_auth_store_with_recovery(data_dir: &Path) -> Result<RecoveredAuthStore, String> {
    match open_verified_auth_store(data_dir) {
        Ok(store) => Ok(RecoveredAuthStore {
            store,
            recreated: false,
        }),
        Err(error) if should_recreate_auth_store(&error) => recreate_auth_store(data_dir),
        Err(error) => Err(error),
    }
}

#[allow(dead_code)] // Reachable through recovery helpers in binary-only command flows.
fn open_verified_auth_store(data_dir: &Path) -> Result<AuthStore, String> {
    let store = AuthStore::open(data_dir)?;
    store.load_auth_manager().map(|_| store)
}

#[allow(dead_code)] // Reachable through recovery helpers in binary-only command flows.
fn recreate_auth_store(data_dir: &Path) -> Result<RecoveredAuthStore, String> {
    remove_if_exists(&data_dir.join("auth.db"))?;
    remove_if_exists(&data_dir.join(".auth-salt"))?;
    let store = AuthStore::open(data_dir)?;
    Ok(RecoveredAuthStore {
        store,
        recreated: true,
    })
}

#[allow(dead_code)] // Reachable through recovery helpers in binary-only command flows.
fn should_recreate_auth_store(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("different machine identity")
        || error.contains("decrypt")
        || error.contains("failed to open auth database")
        || error.contains("key derivation failed")
}

#[allow(dead_code)] // Reachable through recovery helpers in binary-only command flows.
fn remove_if_exists(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
    }
    Ok(())
}

/// Decode a credential-read error, providing actionable hints for key mismatches.
fn decode_credential_error(e: impl std::fmt::Display) -> Result<AuthManager, String> {
    let msg = e.to_string();
    // NB3: When decryption fails, it is likely a key mismatch
    // caused by hostname/username change. Provide an actionable hint.
    if msg.contains("decrypt") || msg.contains("ncrypt") {
        Err(
            "credentials encrypted with a different machine identity \u{2014} \
             delete ~/.fawx/auth.db and ~/.fawx/.auth-salt to re-authenticate"
                .to_string(),
        )
    } else {
        Err(format!("failed to read credentials: {e}"))
    }
}

#[allow(dead_code)] // Used by feat/auth-tui-wiring PR #1166
fn provider_token_key(provider: &str) -> String {
    format!(
        "{}{}",
        provider.trim().to_ascii_lowercase(),
        PROVIDER_TOKEN_SUFFIX
    )
}

/// Migrate plaintext `auth.json` into the encrypted store, then delete it.
///
/// Does nothing when `auth.json` does not exist. If the encrypted store
/// already contains data the plaintext file is simply removed.
pub fn migrate_if_needed(data_dir: &Path, store: &AuthStore) -> Result<(), String> {
    let plaintext_path = data_dir.join("auth.json");
    if !plaintext_path.exists() {
        return Ok(());
    }

    // If encrypted store already has providers, just remove plaintext.
    let existing = store.load_auth_manager()?;
    if !existing.providers().is_empty() {
        fs::remove_file(&plaintext_path).ok();
        return Ok(());
    }

    let raw = fs::read_to_string(&plaintext_path)
        .map_err(|e| format!("failed to read plaintext auth: {e}"))?;
    let auth = match AuthManager::from_json(&raw) {
        Ok(auth) => auth,
        Err(e) => {
            // Invalid JSON in the plaintext file \u{2014} skip migration instead of
            // aborting startup.  The broken file is left in place for manual
            // inspection.
            eprintln!("\u{26a0} plaintext auth.json contains invalid data: {e}");
            eprintln!(
                "  Skipping migration. Inspect or delete manually: {}",
                plaintext_path.display()
            );
            return Ok(());
        }
    };

    if auth.providers().is_empty() {
        fs::remove_file(&plaintext_path).ok();
        return Ok(());
    }

    store.save_auth_manager(&auth)?;

    // NB5: If plaintext deletion fails after successful migration, log a
    // warning but don't abort. The data is safely in the encrypted store
    // and a subsequent launch will clean up.
    if let Err(e) = fs::remove_file(&plaintext_path) {
        eprintln!("\u{26a0} Could not delete plaintext auth.json: {e}");
        eprintln!(
            "  Credentials were migrated successfully. Delete manually: {}",
            plaintext_path.display()
        );
    }

    eprintln!("\u{2713} Migrated credentials to encrypted store");
    Ok(())
}

// ---------------------------------------------------------------------------
// Key derivation helpers
// ---------------------------------------------------------------------------

/// Collect machine-specific context bytes (hostname + username + pepper).
fn machine_context() -> Vec<u8> {
    let mut ctx = Vec::new();
    if let Ok(output) = std::process::Command::new("hostname").output() {
        ctx.extend_from_slice(&output.stdout);
    }
    if let Ok(user) = std::env::var("USER") {
        ctx.extend_from_slice(user.as_bytes());
    }
    ctx.extend_from_slice(APP_PEPPER);
    ctx
}

/// Derive (or re-derive) the encryption key for the auth store.
///
/// On first run a random 32-byte salt is generated and persisted to
/// `.auth-salt` with `0o600` permissions. Subsequent calls read the
/// existing salt so the same key is derived deterministically.
///
/// ## HKDF salt usage (NH7)
///
/// `fx_storage::derive_key` passes an empty salt to HKDF-SHA256 and
/// concatenates the random file-salt into the IKM instead.  This is
/// intentional: the random salt already provides the entropy that the
/// HKDF extract step needs (RFC 5869 \u{00a7}4.2 notes the salt is optional
/// and the IKM can carry the randomness).  Because the random bytes
/// are mixed in at the IKM level, the derived key is still unique per
/// installation and cryptographically sound.
fn get_or_create_auth_key(data_dir: &Path) -> Result<fx_storage::EncryptionKey, String> {
    let salt_path = data_dir.join(".auth-salt");
    let salt = read_or_create_salt(&salt_path, data_dir)?;

    let mut master_material = machine_context();
    master_material.extend_from_slice(&salt);

    derive_key(&master_material, "auth-credentials")
        .map_err(|e| format!("key derivation failed: {e}"))
}

/// Read an existing salt file or generate a fresh one.
///
/// If the salt is missing but `auth.db` already exists, the user is warned
/// that previously encrypted credentials cannot be recovered (NB4).
fn read_or_create_salt(salt_path: &Path, data_dir: &Path) -> Result<Vec<u8>, String> {
    if salt_path.exists() {
        return fs::read(salt_path).map_err(|e| format!("failed to read auth salt: {e}"));
    }

    // NB4: Warn when regenerating salt while an encrypted DB already exists.
    let db_path = data_dir.join("auth.db");
    if db_path.exists() {
        eprintln!(
            "\u{26a0} auth salt was regenerated \u{2014} previous credentials cannot \
             be decrypted. Re-authenticate with /auth"
        );
    }

    let salt = generate_random_salt()?;
    write_salt_file(salt_path, &salt)?;
    Ok(salt)
}

/// Generate 32 cryptographically-random bytes using the OS CSPRNG.
///
/// Uses `ring::rand::SystemRandom` for consistency with the rest of the
/// crypto stack (`fx-storage::crypto`) which also uses `SystemRandom`.
fn generate_random_salt() -> Result<Vec<u8>, String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut salt = vec![0u8; 32];
    rng.fill(&mut salt)
        .map_err(|_| "failed to generate random salt".to_string())?;
    Ok(salt)
}

/// Write salt bytes to disk with restrictive permissions.
fn write_salt_file(path: &Path, salt: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("failed to create auth dir: {e}"))?;
    }
    fs::write(path, salt).map_err(|e| format!("failed to write auth salt: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("failed to set salt permissions: {e}"))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use fx_auth::auth::AuthMethod;
    use tempfile::TempDir;

    /// Helper: build a test `AuthManager` with one provider.
    fn sample_auth_manager() -> AuthManager {
        let mut am = AuthManager::new();
        am.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "test-token-abc".to_string(),
            },
        );
        am
    }

    #[test]
    fn open_creates_store_files() {
        let dir = TempDir::new().expect("tempdir");
        let _store = AuthStore::open(dir.path()).expect("open should succeed");

        assert!(
            dir.path().join("auth.db").exists(),
            "auth.db should be created"
        );
        assert!(
            dir.path().join(".auth-salt").exists(),
            ".auth-salt should be created"
        );
    }

    #[cfg(unix)]
    #[test]
    fn salt_file_has_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().expect("tempdir");
        let _store = AuthStore::open(dir.path()).expect("open");

        let mode = fs::metadata(dir.path().join(".auth-salt"))
            .expect("salt metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "salt should be owner-only");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("open");

        let original = sample_auth_manager();
        store.save_auth_manager(&original).expect("save");

        let loaded = store.load_auth_manager().expect("load");
        assert_eq!(loaded, original);
    }

    #[test]
    fn empty_store_returns_empty_manager() {
        let dir = TempDir::new().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("open");

        let loaded = store.load_auth_manager().expect("load");
        assert!(loaded.providers().is_empty());
    }

    #[test]
    fn migrate_from_plaintext() {
        let dir = TempDir::new().expect("tempdir");
        let plaintext_path = dir.path().join("auth.json");
        let original = sample_auth_manager();
        fs::write(&plaintext_path, original.to_json().expect("serialize"))
            .expect("write plaintext");

        let store = AuthStore::open(dir.path()).expect("open");
        migrate_if_needed(dir.path(), &store).expect("migrate");

        // Plaintext file should be deleted.
        assert!(
            !plaintext_path.exists(),
            "plaintext auth.json should be deleted after migration"
        );

        // Encrypted store should contain the same data.
        let loaded = store.load_auth_manager().expect("load");
        assert_eq!(loaded, original);
    }

    #[test]
    fn migrate_skips_if_encrypted_exists() {
        let dir = TempDir::new().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("open");

        // Store data in the encrypted store first.
        let encrypted_auth = sample_auth_manager();
        store
            .save_auth_manager(&encrypted_auth)
            .expect("save encrypted");

        // Write a *different* plaintext file that should be ignored.
        let mut plaintext_auth = AuthManager::new();
        plaintext_auth.store(
            "openai",
            AuthMethod::ApiKey {
                provider: "openai".to_string(),
                key: "should-not-overwrite".to_string(),
            },
        );
        let plaintext_path = dir.path().join("auth.json");
        fs::write(
            &plaintext_path,
            plaintext_auth.to_json().expect("serialize"),
        )
        .expect("write plaintext");

        migrate_if_needed(dir.path(), &store).expect("migrate");

        // Plaintext deleted, encrypted data unchanged.
        assert!(!plaintext_path.exists());
        let loaded = store.load_auth_manager().expect("load");
        assert_eq!(loaded, encrypted_auth);
    }

    #[test]
    fn salt_persists_across_opens() {
        let dir = TempDir::new().expect("tempdir");

        // First open: save data.
        let store1 = AuthStore::open(dir.path()).expect("open 1");
        let original = sample_auth_manager();
        store1.save_auth_manager(&original).expect("save");
        drop(store1);

        // Second open: should derive the same key and decrypt.
        let store2 = AuthStore::open(dir.path()).expect("open 2");
        let loaded = store2.load_auth_manager().expect("load");
        assert_eq!(loaded, original);
    }

    #[test]
    fn different_dirs_produce_different_encryption() {
        let dir_a = TempDir::new().expect("dir a");
        let dir_b = TempDir::new().expect("dir b");

        let store_a = AuthStore::open(dir_a.path()).expect("open a");
        let store_b = AuthStore::open(dir_b.path()).expect("open b");

        let auth = sample_auth_manager();
        store_a.save_auth_manager(&auth).expect("save a");
        store_b.save_auth_manager(&auth).expect("save b");

        // Read the raw database files — they should differ because
        // each directory has a unique random salt.
        let raw_a = fs::read(dir_a.path().join("auth.db")).expect("read a");
        let raw_b = fs::read(dir_b.path().join("auth.db")).expect("read b");
        assert_ne!(raw_a, raw_b, "databases with different salts should differ");
    }

    #[test]
    fn migrate_empty_plaintext_deletes_without_storing() {
        let dir = TempDir::new().expect("tempdir");
        let plaintext_path = dir.path().join("auth.json");
        let empty_auth = AuthManager::new();
        fs::write(&plaintext_path, empty_auth.to_json().expect("serialize")).expect("write");

        let store = AuthStore::open(dir.path()).expect("open");
        migrate_if_needed(dir.path(), &store).expect("migrate");

        assert!(!plaintext_path.exists());
        assert!(store
            .load_auth_manager()
            .expect("load")
            .providers()
            .is_empty());
    }

    #[test]
    fn corrupt_salt_file_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("open");
        let auth = sample_auth_manager();
        store.save_auth_manager(&auth).expect("save");
        drop(store);

        // Corrupt the salt file with garbage bytes (different length).
        fs::write(dir.path().join(".auth-salt"), b"short").expect("corrupt salt");

        // Re-opening derives a different key; loading must fail on decryption.
        let store2 = AuthStore::open(dir.path()).expect("open with corrupt salt");
        let result = store2.load_auth_manager();
        assert!(
            result.is_err(),
            "loading with corrupt salt should fail decryption"
        );
    }

    #[test]
    fn deleted_salt_regenerates_with_warning() {
        let dir = TempDir::new().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("open");
        let auth = sample_auth_manager();
        store.save_auth_manager(&auth).expect("save");
        drop(store);

        // Delete the salt \u{2014} a new salt is generated, producing a different key.
        fs::remove_file(dir.path().join(".auth-salt")).expect("remove salt");

        // Re-opening should succeed (new salt is created), but loading data
        // must fail because the key changed.
        let store2 = AuthStore::open(dir.path()).expect("open after salt deletion");
        let result = store2.load_auth_manager();
        assert!(
            result.is_err(),
            "loading after salt deletion should fail due to key mismatch"
        );
    }

    #[test]
    fn corrupt_db_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("open");
        store
            .save_auth_manager(&sample_auth_manager())
            .expect("save");
        drop(store);

        // Overwrite auth.db with garbage.
        fs::write(dir.path().join("auth.db"), b"not a sqlite database").expect("corrupt db");

        // Opening with a corrupt database should fail.
        let result = AuthStore::open(dir.path());
        assert!(
            result.is_err(),
            "opening corrupt database should return an error"
        );
    }

    #[test]
    fn open_auth_store_with_recovery_recreates_store_after_salt_mismatch() {
        let dir = TempDir::new().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("open");
        store
            .save_auth_manager(&sample_auth_manager())
            .expect("save");
        drop(store);
        fs::write(dir.path().join(".auth-salt"), b"short").expect("corrupt salt");

        let recovered = open_auth_store_with_recovery(dir.path()).expect("recover");
        let loaded = recovered.store.load_auth_manager().expect("load");

        assert!(recovered.recreated);
        assert!(loaded.providers().is_empty());
    }

    #[test]
    fn open_auth_store_with_recovery_recreates_store_after_db_corruption() {
        let dir = TempDir::new().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("open");
        store
            .save_auth_manager(&sample_auth_manager())
            .expect("save");
        drop(store);
        fs::write(dir.path().join("auth.db"), b"not a sqlite database").expect("corrupt db");

        let recovered = open_auth_store_with_recovery(dir.path()).expect("recover");
        let loaded = recovered.store.load_auth_manager().expect("load");

        assert!(recovered.recreated);
        assert!(loaded.providers().is_empty());
    }

    #[test]
    fn migration_with_invalid_json_skips() {
        let dir = TempDir::new().expect("tempdir");
        let plaintext_path = dir.path().join("auth.json");
        fs::write(&plaintext_path, "this is not valid json {{{").expect("write invalid json");

        let store = AuthStore::open(dir.path()).expect("open");
        // Migration should skip gracefully, not panic or abort.
        let result = migrate_if_needed(dir.path(), &store);
        assert!(
            result.is_ok(),
            "migration with invalid JSON should skip, not abort: {result:?}"
        );

        // The encrypted store should have no data.
        let loaded = store.load_auth_manager().expect("load");
        assert!(loaded.providers().is_empty());
    }

    /// Regression test: two `AuthStore` instances can coexist on the same
    /// directory because the open-per-operation pattern only holds the DB
    /// lock for individual reads/writes, not for the store lifetime.
    #[test]
    fn concurrent_stores_do_not_block() {
        let dir = TempDir::new().expect("tempdir");
        let store1 = AuthStore::open(dir.path()).expect("first open");
        let store2 = AuthStore::open(dir.path()).expect("second open (concurrent)");

        // Both stores can read and write without blocking each other.
        let auth = sample_auth_manager();
        store1.save_auth_manager(&auth).expect("store1 save");
        let loaded = store2.load_auth_manager().expect("store2 load");
        assert_eq!(loaded, auth);
    }

    /// Verify that the store works correctly after drop + reopen (sequential).
    #[test]
    fn drop_and_reopen_succeeds() {
        let dir = TempDir::new().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("first open");
        store
            .save_auth_manager(&sample_auth_manager())
            .expect("save");
        drop(store);

        let store2 = AuthStore::open(dir.path()).expect("reopen");
        let loaded = store2.load_auth_manager().expect("load after reopen");
        assert_eq!(loaded, sample_auth_manager());
    }
}
