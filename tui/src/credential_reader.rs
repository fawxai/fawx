//! Minimal read-only access to the Fawx encrypted credential store.
//!
//! Mirrors the key derivation from `fx-cli`'s `AuthStore` to read
//! provider tokens without pulling in the full CLI crate. This module
//! is intentionally read-only — it never creates files or modifies
//! the store.

use fx_storage::{derive_key, CredentialStore, EncryptedStore, EncryptionKey, Storage};
use std::path::Path;

/// Application pepper — must match `fx-cli/src/auth_store.rs`.
const APP_PEPPER: &[u8] = b"fawx-auth-store-v1";

/// Read a provider token from the encrypted credential store.
///
/// Returns `None` if the store doesn't exist, the salt is missing,
/// or the token is not present. Errors are silently mapped to `None`
/// since this is a best-effort fallback — the caller handles the
/// "no token" case gracefully.
pub fn read_provider_token(data_dir: &Path, provider: &str) -> Option<String> {
    let key = derive_auth_key(data_dir)?;
    let db_path = data_dir.join("auth.db");
    if !db_path.exists() {
        return None;
    }
    let storage = Storage::open(&db_path).ok()?;
    let encrypted = EncryptedStore::new(storage, key);
    let cred_store = CredentialStore::new(encrypted);
    let token_key = format!("{}_token", provider.trim().to_ascii_lowercase());
    cred_store.get_credential(&token_key).ok().flatten()
}

/// Derive the encryption key using the same algorithm as `fx-cli`.
///
/// Reads the existing `.auth-salt` file (never creates one) and
/// combines it with machine context to reproduce the same key.
fn derive_auth_key(data_dir: &Path) -> Option<EncryptionKey> {
    let salt_path = data_dir.join(".auth-salt");
    let salt = std::fs::read(&salt_path).ok()?;
    let mut material = machine_context();
    material.extend_from_slice(&salt);
    derive_key(&material, "auth-credentials").ok()
}

/// Reproduce the machine-specific context used by `fx-cli`.
///
/// Must stay in sync with `fx-cli/src/auth_store.rs::machine_context`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_when_data_dir_missing() {
        let result = read_provider_token(Path::new("/nonexistent/path"), "http_bearer");
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_no_salt_file() {
        // Use a unique empty directory with no .auth-salt
        let dir = std::env::temp_dir().join("fawx-cred-test-no-salt");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let result = read_provider_token(&dir, "http_bearer");
        assert!(result.is_none());
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn machine_context_includes_pepper() {
        let ctx = machine_context();
        assert!(
            ctx.windows(APP_PEPPER.len()).any(|w| w == APP_PEPPER),
            "machine context must include the app pepper"
        );
    }
}
