//! Credential store abstraction for secure secret management.
//!
//! Provides a trait-based interface for storing, retrieving, and managing
//! credentials (e.g., GitHub PATs) with an encrypted file backend.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

/// Errors from credential store operations.
#[derive(Debug)]
pub enum CredentialStoreError {
    /// Filesystem or database I/O failure.
    Io(String),
    /// Encryption or decryption failure.
    Encryption(String),
    /// Requested credential was not found.
    NotFound,
    /// Serialization or deserialization failure (e.g. corrupted metadata).
    Serialization(String),
    /// Key derivation or salt generation failure.
    KeyDerivation(String),
}

impl fmt::Display for CredentialStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "credential store I/O error: {msg}"),
            Self::Encryption(msg) => write!(f, "credential store encryption error: {msg}"),
            Self::NotFound => write!(f, "credential not found"),
            Self::Serialization(msg) => write!(f, "credential store serialization error: {msg}"),
            Self::KeyDerivation(msg) => write!(f, "credential key derivation error: {msg}"),
        }
    }
}

impl std::error::Error for CredentialStoreError {}

impl From<std::io::Error> for CredentialStoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<serde_json::Error> for CredentialStoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e.to_string())
    }
}

/// Supported authentication providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthProvider {
    /// GitHub (PAT, future: App OAuth, SSH).
    GitHub,
}

impl fmt::Display for AuthProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitHub => write!(f, "github"),
        }
    }
}

impl AuthProvider {
    /// Parse a provider from a string identifier.
    pub fn from_str_id(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "github" => Some(Self::GitHub),
            _ => None,
        }
    }

    /// Storage key prefix for this provider.
    fn key_prefix(&self) -> &str {
        match self {
            Self::GitHub => "github",
        }
    }
}

/// Authentication method variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialMethod {
    /// Personal Access Token.
    Pat,
}

impl fmt::Display for CredentialMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pat => write!(f, "PAT"),
        }
    }
}

/// Metadata stored alongside a credential (never the secret itself).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialMetadata {
    /// Provider this credential belongs to.
    pub provider: AuthProvider,
    /// Authentication method.
    pub method: CredentialMethod,
    /// Unix timestamp (ms) when the credential was last validated.
    pub last_validated_ms: u64,
    /// Username/login associated with the credential (if known).
    pub login: Option<String>,
    /// Scopes/permissions the credential grants (if known).
    pub scopes: Vec<String>,
    /// Token kind hint (e.g. "classic", "fine_grained", "unknown") stored at
    /// set-token time so status display can avoid decrypting the secret.
    #[serde(default)]
    pub token_kind: Option<String>,
}

/// Status of a credential provider.
#[derive(Debug, Clone)]
pub struct ProviderStatus {
    /// Whether a credential is configured.
    pub configured: bool,
    /// Metadata (only present if configured).
    pub metadata: Option<CredentialMetadata>,
}

/// Trait for credential storage backends.
///
/// Implementations must handle encryption, secure deletion, and
/// never expose secrets in logs or debug output.
pub trait CredentialStore: Send + Sync {
    /// Store a credential for the given provider and method.
    fn set(
        &self,
        provider: AuthProvider,
        method: CredentialMethod,
        secret: &Zeroizing<String>,
        metadata: &CredentialMetadata,
    ) -> Result<(), CredentialStoreError>;

    /// Retrieve the secret for a provider/method pair.
    fn get(
        &self,
        provider: AuthProvider,
        method: CredentialMethod,
    ) -> Result<Option<Zeroizing<String>>, CredentialStoreError>;

    /// Remove a stored credential.
    fn clear(
        &self,
        provider: AuthProvider,
        method: CredentialMethod,
    ) -> Result<bool, CredentialStoreError>;

    /// Get the status of a provider (configured, metadata).
    fn status(&self, provider: AuthProvider) -> Result<ProviderStatus, CredentialStoreError>;
}

/// Storage key for a credential secret.
fn secret_key(provider: AuthProvider, method: CredentialMethod) -> String {
    format!("{}_{}_secret", provider.key_prefix(), method)
}

/// Storage key for credential metadata.
fn metadata_key(provider: AuthProvider, method: CredentialMethod) -> String {
    format!("{}_{}_metadata", provider.key_prefix(), method)
}

const GENERIC_CREDENTIAL_PREFIX: &str = "generic::";

fn generic_key(name: &str) -> String {
    format!("{GENERIC_CREDENTIAL_PREFIX}{name}")
}

fn generic_names(keys: Vec<String>) -> Vec<String> {
    let mut names = keys
        .into_iter()
        .filter_map(|key| {
            key.strip_prefix(GENERIC_CREDENTIAL_PREFIX)
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// Encrypted file-backed credential store.
///
/// Reuses the existing `fx-storage` encrypted store infrastructure
/// (AES-256-GCM via `ring`). Stores secrets and metadata as separate
/// entries so metadata can be read without decrypting the secret.
pub struct EncryptedFileCredentialStore {
    store: fx_storage::CredentialStore,
}

impl EncryptedFileCredentialStore {
    /// Open (or create) the credential store at the given data directory.
    ///
    /// Creates `credentials.db` and `.credentials-salt` inside `data_dir`.
    pub fn open(data_dir: &Path) -> Result<Self, CredentialStoreError> {
        let key = derive_credential_key(data_dir)?;
        let db_path = data_dir.join("credentials.db");
        let storage = fx_storage::Storage::open(&db_path)
            .map_err(|e| CredentialStoreError::Io(format!("failed to open database: {e}")))?;
        let encrypted = fx_storage::EncryptedStore::new(storage, key);
        Ok(Self {
            store: fx_storage::CredentialStore::new(encrypted),
        })
    }

    /// Open an in-memory credential store (for testing).
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, CredentialStoreError> {
        let key = fx_storage::EncryptionKey::from_bytes(&[42u8; 32]);
        let storage = fx_storage::Storage::open_in_memory()
            .map_err(|e| CredentialStoreError::Io(format!("in-memory storage: {e}")))?;
        let encrypted = fx_storage::EncryptedStore::new(storage, key);
        Ok(Self {
            store: fx_storage::CredentialStore::new(encrypted),
        })
    }

    pub fn set_generic(&self, name: &str, value: &str) -> Result<(), CredentialStoreError> {
        self.store
            .store_credential(&generic_key(name), value)
            .map_err(|e| CredentialStoreError::Encryption(format!("store generic credential: {e}")))
    }

    pub fn get_generic(
        &self,
        name: &str,
    ) -> Result<Option<Zeroizing<String>>, CredentialStoreError> {
        self.store
            .get_credential(&generic_key(name))
            .map(|value| value.map(Zeroizing::new))
            .map_err(|e| {
                CredentialStoreError::Encryption(format!("retrieve generic credential: {e}"))
            })
    }

    pub fn list_generic_names(&self) -> Result<Vec<String>, CredentialStoreError> {
        self.store
            .list_credentials()
            .map(generic_names)
            .map_err(|e| CredentialStoreError::Io(format!("list generic credentials: {e}")))
    }
}

impl CredentialStore for EncryptedFileCredentialStore {
    fn set(
        &self,
        provider: AuthProvider,
        method: CredentialMethod,
        secret: &Zeroizing<String>,
        metadata: &CredentialMetadata,
    ) -> Result<(), CredentialStoreError> {
        let skey = secret_key(provider, method);
        self.store
            .store_credential(&skey, secret.as_str())
            .map_err(|e| CredentialStoreError::Encryption(format!("store secret: {e}")))?;

        let mkey = metadata_key(provider, method);
        let meta_json = serde_json::to_string(metadata)
            .map_err(|e| CredentialStoreError::Serialization(format!("serialize metadata: {e}")))?;
        self.store
            .store_credential(&mkey, &meta_json)
            .map_err(|e| CredentialStoreError::Encryption(format!("store metadata: {e}")))?;

        Ok(())
    }

    fn get(
        &self,
        provider: AuthProvider,
        method: CredentialMethod,
    ) -> Result<Option<Zeroizing<String>>, CredentialStoreError> {
        let skey = secret_key(provider, method);
        match self.store.get_credential(&skey) {
            Ok(Some(value)) => Ok(Some(Zeroizing::new(value))),
            Ok(None) => Ok(None),
            Err(e) => Err(CredentialStoreError::Encryption(format!("retrieve: {e}"))),
        }
    }

    fn clear(
        &self,
        provider: AuthProvider,
        method: CredentialMethod,
    ) -> Result<bool, CredentialStoreError> {
        let skey = secret_key(provider, method);
        let secret_existed = self
            .store
            .delete_credential(&skey)
            .map_err(|e| CredentialStoreError::Io(format!("delete secret: {e}")))?;

        let mkey = metadata_key(provider, method);
        let _ = self.store.delete_credential(&mkey);

        Ok(secret_existed)
    }

    fn status(&self, provider: AuthProvider) -> Result<ProviderStatus, CredentialStoreError> {
        // NOTE: Only checks Pat method. When CredentialMethod gains variants
        // (OAuth, SSH), iterate all methods here.
        let method = CredentialMethod::Pat;
        let mkey = metadata_key(provider, method);
        match self.store.get_credential(&mkey) {
            Ok(Some(json)) => {
                let metadata: CredentialMetadata = serde_json::from_str(&json)
                    .map_err(|e| CredentialStoreError::Serialization(format!("corrupted: {e}")))?;
                Ok(ProviderStatus {
                    configured: true,
                    metadata: Some(metadata),
                })
            }
            Ok(None) => Ok(ProviderStatus {
                configured: false,
                metadata: None,
            }),
            Err(e) => Err(CredentialStoreError::Encryption(format!(
                "read status: {e}"
            ))),
        }
    }
}

/// Application pepper for credential key derivation (distinct from auth store).
const CREDENTIAL_APP_PEPPER: &[u8] = b"fawx-credential-store-v1";

/// Derive the encryption key for the credential store.
fn derive_credential_key(
    data_dir: &Path,
) -> Result<fx_storage::EncryptionKey, CredentialStoreError> {
    let salt_path = data_dir.join(".credentials-salt");
    let salt = read_or_create_credential_salt(&salt_path, data_dir)?;

    let mut master_material = credential_machine_context();
    master_material.extend_from_slice(&salt);

    fx_storage::derive_key(&master_material, "skill-credentials")
        .map_err(|e| CredentialStoreError::KeyDerivation(e.to_string()))
}

/// Machine-specific context for credential key derivation.
///
/// Key derivation mixes hostname + `$USER` + pepper. If any of these
/// change (hostname reassignment, username rename, migrating the data
/// directory to another machine), stored credentials become undecryptable.
/// Users must re-store credentials after such changes.
fn credential_machine_context() -> Vec<u8> {
    let mut ctx = Vec::new();
    if let Ok(output) = std::process::Command::new("hostname").output() {
        ctx.extend_from_slice(&output.stdout);
    }
    if let Ok(user) = std::env::var("USER") {
        ctx.extend_from_slice(user.as_bytes());
    }
    ctx.extend_from_slice(CREDENTIAL_APP_PEPPER);
    ctx
}

/// Read an existing salt or create a fresh one.
fn read_or_create_credential_salt(
    salt_path: &Path,
    data_dir: &Path,
) -> Result<Vec<u8>, CredentialStoreError> {
    if salt_path.exists() {
        return Ok(std::fs::read(salt_path)?);
    }

    let db_path = data_dir.join("credentials.db");
    if db_path.exists() {
        tracing::warn!(
            "credential salt was regenerated — previous credentials cannot be decrypted"
        );
    }

    let salt = generate_random_bytes(32)?;
    write_restrictive_file(salt_path, &salt)?;
    Ok(salt)
}

/// Generate cryptographically random bytes.
fn generate_random_bytes(len: usize) -> Result<Vec<u8>, CredentialStoreError> {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut buf = vec![0u8; len];
    rng.fill(&mut buf)
        .map_err(|_| CredentialStoreError::Encryption("random byte generation failed".into()))?;
    Ok(buf)
}

/// Write file with restrictive permissions (0o600 on Unix).
fn write_restrictive_file(path: &Path, data: &[u8]) -> Result<(), CredentialStoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, data)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Get the current Unix timestamp in milliseconds.
pub fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> EncryptedFileCredentialStore {
        EncryptedFileCredentialStore::open_in_memory().expect("should create in-memory store")
    }

    fn test_metadata() -> CredentialMetadata {
        CredentialMetadata {
            provider: AuthProvider::GitHub,
            method: CredentialMethod::Pat,
            last_validated_ms: 1_000_000,
            login: Some("testuser".to_string()),
            scopes: vec!["repo".to_string(), "workflow".to_string()],
            token_kind: None,
        }
    }

    #[test]
    fn set_get_roundtrip() {
        let store = test_store();
        let secret = Zeroizing::new("ghp_test_token_12345".to_string());
        let metadata = test_metadata();

        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &secret,
                &metadata,
            )
            .expect("set should succeed");

        let retrieved = store
            .get(AuthProvider::GitHub, CredentialMethod::Pat)
            .expect("get should succeed")
            .expect("should have value");

        assert_eq!(*retrieved, *secret);
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let store = test_store();
        let result = store
            .get(AuthProvider::GitHub, CredentialMethod::Pat)
            .expect("get should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn clear_removes_credential() {
        let store = test_store();
        let secret = Zeroizing::new("ghp_token".to_string());
        let metadata = test_metadata();

        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &secret,
                &metadata,
            )
            .expect("set");

        let existed = store
            .clear(AuthProvider::GitHub, CredentialMethod::Pat)
            .expect("clear");
        assert!(existed);

        let result = store
            .get(AuthProvider::GitHub, CredentialMethod::Pat)
            .expect("get after clear");
        assert!(result.is_none());
    }

    #[test]
    fn clear_nonexistent_returns_false() {
        let store = test_store();
        let existed = store
            .clear(AuthProvider::GitHub, CredentialMethod::Pat)
            .expect("clear nonexistent");
        assert!(!existed);
    }

    #[test]
    fn set_get_generic_roundtrip() {
        let store = test_store();

        store
            .set_generic("brave_api_key", "brv_test_123")
            .expect("set generic");

        let retrieved = store
            .get_generic("brave_api_key")
            .expect("get generic")
            .expect("should have generic value");

        assert_eq!(*retrieved, "brv_test_123");
    }

    #[test]
    fn get_generic_nonexistent_returns_none() {
        let store = test_store();

        let retrieved = store
            .get_generic("nonexistent")
            .expect("get missing generic");

        assert!(retrieved.is_none());
    }

    #[test]
    fn list_generic_names_filters_non_generic_entries() {
        let store = test_store();
        let metadata = test_metadata();

        store
            .set_generic("brave_api_key", "brv_test_123")
            .expect("set brave credential");
        store
            .set_generic("custom_webhook_token", "hook_test_456")
            .expect("set webhook credential");
        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &Zeroizing::new("ghp_token".to_string()),
                &metadata,
            )
            .expect("set github credential");

        let names = store.list_generic_names().expect("list generic names");

        assert_eq!(
            names,
            vec![
                "brave_api_key".to_string(),
                "custom_webhook_token".to_string(),
            ]
        );
    }

    #[test]
    fn status_unconfigured() {
        let store = test_store();
        let status = store.status(AuthProvider::GitHub).expect("status");
        assert!(!status.configured);
        assert!(status.metadata.is_none());
    }

    #[test]
    fn status_configured_shows_metadata() {
        let store = test_store();
        let secret = Zeroizing::new("ghp_token".to_string());
        let metadata = test_metadata();

        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &secret,
                &metadata,
            )
            .expect("set");

        let status = store.status(AuthProvider::GitHub).expect("status");
        assert!(status.configured);

        let meta = status.metadata.expect("should have metadata");
        assert_eq!(meta.provider, AuthProvider::GitHub);
        assert_eq!(meta.login.as_deref(), Some("testuser"));
        assert_eq!(meta.scopes, vec!["repo", "workflow"]);
    }

    #[test]
    fn overwrite_credential() {
        let store = test_store();
        let secret1 = Zeroizing::new("ghp_first".to_string());
        let secret2 = Zeroizing::new("ghp_second".to_string());
        let metadata = test_metadata();

        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &secret1,
                &metadata,
            )
            .expect("set first");
        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &secret2,
                &metadata,
            )
            .expect("set second");

        let retrieved = store
            .get(AuthProvider::GitHub, CredentialMethod::Pat)
            .expect("get")
            .expect("should have value");
        assert_eq!(*retrieved, "ghp_second");
    }

    #[test]
    fn status_after_clear_is_unconfigured() {
        let store = test_store();
        let secret = Zeroizing::new("ghp_token".to_string());
        let metadata = test_metadata();

        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &secret,
                &metadata,
            )
            .expect("set");
        store
            .clear(AuthProvider::GitHub, CredentialMethod::Pat)
            .expect("clear");

        let status = store
            .status(AuthProvider::GitHub)
            .expect("status after clear");
        assert!(!status.configured);
    }

    #[test]
    fn auth_provider_display() {
        assert_eq!(AuthProvider::GitHub.to_string(), "github");
    }

    #[test]
    fn auth_provider_from_str() {
        assert_eq!(
            AuthProvider::from_str_id("github"),
            Some(AuthProvider::GitHub)
        );
        assert_eq!(
            AuthProvider::from_str_id("GitHub"),
            Some(AuthProvider::GitHub)
        );
        assert_eq!(
            AuthProvider::from_str_id("GITHUB"),
            Some(AuthProvider::GitHub)
        );
        assert_eq!(AuthProvider::from_str_id("unknown"), None);
    }

    #[test]
    fn credential_method_display() {
        assert_eq!(CredentialMethod::Pat.to_string(), "PAT");
    }

    #[test]
    fn file_backed_store_roundtrip() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = EncryptedFileCredentialStore::open(dir.path()).expect("open");

        let secret = Zeroizing::new("ghp_file_token".to_string());
        let metadata = test_metadata();

        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &secret,
                &metadata,
            )
            .expect("set");

        // Re-open the store
        drop(store);
        let store2 = EncryptedFileCredentialStore::open(dir.path()).expect("reopen");

        let retrieved = store2
            .get(AuthProvider::GitHub, CredentialMethod::Pat)
            .expect("get")
            .expect("should have value");
        assert_eq!(*retrieved, "ghp_file_token");
    }

    #[test]
    fn corrupt_metadata_returns_error() {
        let store = test_store();

        // Manually write invalid metadata JSON
        store
            .store
            .store_credential(
                &metadata_key(AuthProvider::GitHub, CredentialMethod::Pat),
                "not valid json {{{",
            )
            .expect("store raw");

        let result = store.status(AuthProvider::GitHub);
        assert!(result.is_err());
    }
}
