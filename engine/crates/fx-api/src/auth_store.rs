use fx_auth::auth::AuthManager;
use fx_storage::{derive_key, CredentialStore, EncryptedStore, EncryptionKey, Storage};
use std::fs;
use std::path::{Path, PathBuf};

const APP_PEPPER: &[u8] = b"fawx-auth-store-v1";
const AUTH_MANAGER_KEY: &str = "auth_manager";

pub(crate) struct AuthStore {
    db_path: PathBuf,
    key: EncryptionKey,
}

impl AuthStore {
    pub(crate) fn open(data_dir: &Path) -> Result<Self, String> {
        let key = auth_key(data_dir)?;
        let db_path = data_dir.join("auth.db");
        Storage::open(&db_path)
            .map_err(|error| format!("failed to open auth database: {error}"))?;
        Ok(Self { db_path, key })
    }

    pub(crate) fn load_auth_manager(&self) -> Result<AuthManager, String> {
        let store = self.open_store()?;
        match store.get_credential(AUTH_MANAGER_KEY) {
            Ok(Some(json)) => AuthManager::from_json(&json)
                .map_err(|error| format!("corrupted credential data: {error}")),
            Ok(None) => Ok(AuthManager::new()),
            Err(error) => decode_credential_error(error),
        }
    }

    pub(crate) fn save_auth_manager(&self, auth: &AuthManager) -> Result<(), String> {
        let store = self.open_store()?;
        let json = auth.to_json().map_err(|error| error.to_string())?;
        store
            .store_credential(AUTH_MANAGER_KEY, &json)
            .map_err(|error| format!("failed to store credentials: {error}"))
    }

    fn open_store(&self) -> Result<CredentialStore, String> {
        let storage = Storage::open(&self.db_path)
            .map_err(|error| format!("failed to open auth database: {error}"))?;
        let encrypted = EncryptedStore::new(storage, self.key.clone());
        Ok(CredentialStore::new(encrypted))
    }
}

fn decode_credential_error(error: impl std::fmt::Display) -> Result<AuthManager, String> {
    let message = error.to_string();
    if message.contains("decrypt") || message.contains("ncrypt") {
        Err("credentials encrypted with a different machine identity — \
             re-authenticate to recreate the encrypted auth store"
            .to_string())
    } else {
        Err(format!("failed to read credentials: {error}"))
    }
}

fn auth_key(data_dir: &Path) -> Result<EncryptionKey, String> {
    let salt_path = data_dir.join(".auth-salt");
    let salt = read_or_create_salt(&salt_path, data_dir)?;
    let mut material = machine_context();
    material.extend_from_slice(&salt);
    derive_key(&material, "auth-credentials")
        .map_err(|error| format!("key derivation failed: {error}"))
}

fn machine_context() -> Vec<u8> {
    let mut context = Vec::new();
    if let Ok(output) = std::process::Command::new("hostname").output() {
        context.extend_from_slice(&output.stdout);
    }
    if let Ok(user) = std::env::var("USER") {
        context.extend_from_slice(user.as_bytes());
    }
    context.extend_from_slice(APP_PEPPER);
    context
}

fn read_or_create_salt(salt_path: &Path, data_dir: &Path) -> Result<Vec<u8>, String> {
    if salt_path.exists() {
        return fs::read(salt_path).map_err(|error| format!("failed to read auth salt: {error}"));
    }

    if data_dir.join("auth.db").exists() {
        tracing::warn!("auth salt was regenerated — previous credentials cannot be decrypted");
    }

    let salt = generate_random_salt()?;
    write_salt_file(salt_path, &salt)?;
    Ok(salt)
}

fn generate_random_salt() -> Result<Vec<u8>, String> {
    use ring::rand::{SecureRandom, SystemRandom};

    let mut salt = vec![0u8; 32];
    SystemRandom::new()
        .fill(&mut salt)
        .map_err(|_| "failed to generate random salt".to_string())?;
    Ok(salt)
}

fn write_salt_file(path: &Path, salt: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create auth dir: {error}"))?;
    }
    fs::write(path, salt).map_err(|error| format!("failed to write auth salt: {error}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("failed to set salt permissions: {error}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_auth::auth::AuthMethod;

    #[test]
    fn auth_store_round_trips_provider_credentials() {
        let temp_dir = tempfile::TempDir::new().expect("tempdir");
        let store = AuthStore::open(temp_dir.path()).expect("open auth store");
        let mut auth = AuthManager::new();
        auth.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "ast-test-token".to_string(),
            },
        );

        store.save_auth_manager(&auth).expect("save auth manager");
        let restored = store.load_auth_manager().expect("load auth manager");

        assert_eq!(restored, auth);
    }
}
