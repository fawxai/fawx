use fx_auth::auth::{AuthManager, AuthMethod};
use fx_storage::{derive_key, CredentialStore, EncryptedStore, Storage};
use std::fs;
use std::path::{Path, PathBuf};

const AUTH_MANAGER_KEY: &str = "auth_manager";
const APP_PEPPER: &[u8] = b"fawx-auth-store-v1";

pub fn first_run_required() -> bool {
    let data_dir = fawx_data_dir();
    if data_dir.join("credentials.db").exists() {
        return false;
    }

    load_auth_manager()
        .map(|auth_manager| !auth_manager.has_any())
        .unwrap_or(true)
}

pub fn auth_status_lines() -> Result<Vec<String>, String> {
    let data_dir = fawx_data_dir();
    let mut lines = Vec::new();

    if data_dir.join("credentials.db").exists() {
        lines.push("Claude credentials detected in ~/.fawx/credentials.db.".to_string());
    }

    let auth_manager = load_auth_manager()?;
    if auth_manager.has_any() {
        let providers = auth_manager
            .providers()
            .into_iter()
            .map(display_provider_name)
            .collect::<Vec<_>>();
        lines.push(format!("Configured providers: {}", providers.join(", ")));
    } else if lines.is_empty() {
        lines.push("No API credentials are configured yet.".to_string());
    }

    Ok(lines)
}

pub fn claude_status_line() -> Result<String, String> {
    let auth_manager = load_auth_manager()?;
    if auth_manager.get("anthropic").is_some() {
        Ok("Claude auth status: configured".to_string())
    } else if fawx_data_dir().join("credentials.db").exists() {
        Ok("Claude auth status: configured (credentials.db detected)".to_string())
    } else {
        Ok("Claude auth status: not configured".to_string())
    }
}

pub fn save_claude_token(token: &str) -> Result<(), String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err("usage: /auth claude set-token <your-key>".to_string());
    }

    let store = AuthStore::open(&fawx_data_dir())?;
    let mut auth_manager = store.load_auth_manager()?;
    auth_manager.store(
        "anthropic",
        AuthMethod::ApiKey {
            provider: "anthropic".to_string(),
            key: trimmed.to_string(),
        },
    );
    store.save_auth_manager(&auth_manager)
}

pub fn clear_claude_token() -> Result<bool, String> {
    let store = AuthStore::open(&fawx_data_dir())?;
    let mut auth_manager = store.load_auth_manager()?;
    let removed = auth_manager.remove("anthropic").is_some();
    store.save_auth_manager(&auth_manager)?;
    Ok(removed)
}

pub fn display_provider_name(provider: String) -> String {
    match provider.as_str() {
        "anthropic" => "claude".to_string(),
        other => other.to_string(),
    }
}

fn load_auth_manager() -> Result<AuthManager, String> {
    let store = AuthStore::open(&fawx_data_dir())?;
    store.load_auth_manager()
}

fn fawx_data_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .map(|home| home.join(".fawx"))
        .unwrap_or_else(|| PathBuf::from(".fawx"))
}

struct AuthStore {
    credential_store: CredentialStore,
}

impl AuthStore {
    fn open(data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(data_dir).map_err(|e| format!("failed to create auth dir: {e}"))?;
        let key = get_or_create_auth_key(data_dir)?;
        let db_path = data_dir.join("auth.db");
        let storage =
            Storage::open(&db_path).map_err(|e| format!("failed to open auth database: {e}"))?;
        let encrypted = EncryptedStore::new(storage, key);
        Ok(Self {
            credential_store: CredentialStore::new(encrypted),
        })
    }

    fn save_auth_manager(&self, auth: &AuthManager) -> Result<(), String> {
        let json = auth.to_json().map_err(|e| e.to_string())?;
        self.credential_store
            .store_credential(AUTH_MANAGER_KEY, &json)
            .map_err(|e| format!("failed to store credentials: {e}"))
    }

    fn load_auth_manager(&self) -> Result<AuthManager, String> {
        match self.credential_store.get_credential(AUTH_MANAGER_KEY) {
            Ok(Some(json)) => {
                AuthManager::from_json(&json).map_err(|e| format!("corrupted credential data: {e}"))
            }
            Ok(None) => Ok(AuthManager::new()),
            Err(e) => Err(format!("failed to read credentials: {e}")),
        }
    }
}

fn get_or_create_auth_key(data_dir: &Path) -> Result<fx_storage::EncryptionKey, String> {
    let salt_path = data_dir.join(".auth-salt");
    let salt = read_or_create_salt(&salt_path, data_dir)?;

    let mut master_material = machine_context(data_dir);
    master_material.extend_from_slice(&salt);

    derive_key(&master_material, "auth-credentials")
        .map_err(|e| format!("key derivation failed: {e}"))
}

/// Build machine-specific entropy for the credential encryption key.
///
/// # Threat model
///
/// This is **not** a cryptographic key by itself — it is mixed with a
/// random 32-byte salt (stored alongside the DB) and fed through HKDF
/// via [`fx_storage::derive_key`] to produce the actual encryption key.
///
/// The goal is to make the encrypted credential store non-portable:
/// copying `auth.db` + `.auth-salt` to a different machine should fail
/// decryption because the hostname, username, home directory, OS, arch,
/// and data-dir path will differ.
///
/// **What this does NOT protect against:**
/// - An attacker with shell access on the same machine (they can read
///   all the same environment values).
/// - Forensic extraction of the derived key from process memory.
/// - Brute-force if the attacker knows the machine context values (the
///   salt adds the real entropy; the context is a portability barrier,
///   not a secret).
///
/// This is defense-in-depth for local credential storage, not a
/// substitute for OS-level keychain integration (planned future work).
fn machine_context(data_dir: &Path) -> Vec<u8> {
    let mut ctx = Vec::new();
    if let Ok(output) = std::process::Command::new("hostname").output() {
        ctx.extend_from_slice(&output.stdout);
    }
    if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
        ctx.extend_from_slice(user.as_bytes());
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        ctx.extend_from_slice(home.to_string_lossy().as_bytes());
    }
    ctx.extend_from_slice(std::env::consts::OS.as_bytes());
    ctx.extend_from_slice(std::env::consts::ARCH.as_bytes());
    ctx.extend_from_slice(data_dir.to_string_lossy().as_bytes());
    ctx.extend_from_slice(APP_PEPPER);
    ctx
}

fn read_or_create_salt(salt_path: &Path, data_dir: &Path) -> Result<Vec<u8>, String> {
    if salt_path.exists() {
        return fs::read(salt_path).map_err(|e| format!("failed to read auth salt: {e}"));
    }

    let db_path = data_dir.join("auth.db");
    if db_path.exists() {
        tracing::warn!("auth salt was regenerated; previous credentials cannot be decrypted.");
    }

    let salt = generate_random_salt()?;
    write_salt_file(salt_path, &salt)?;
    Ok(salt)
}

fn generate_random_salt() -> Result<Vec<u8>, String> {
    use ring::rand::{SecureRandom, SystemRandom};

    let rng = SystemRandom::new();
    let mut salt = vec![0u8; 32];
    rng.fill(&mut salt)
        .map_err(|_| "failed to generate random salt".to_string())?;
    Ok(salt)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn display_provider_name_maps_anthropic_to_claude() {
        assert_eq!(display_provider_name("anthropic".to_string()), "claude");
        assert_eq!(display_provider_name("github".to_string()), "github");
    }

    #[test]
    fn machine_context_includes_platform_and_data_dir_entropy() {
        let context = machine_context(Path::new("/tmp/fawx-auth-test"));
        let context = String::from_utf8_lossy(&context);

        assert!(context.contains(std::env::consts::OS));
        assert!(context.contains(std::env::consts::ARCH));
        assert!(context.contains("/tmp/fawx-auth-test"));
    }
}
