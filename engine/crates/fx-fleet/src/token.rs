use crate::{current_time_ms, fs_utils::write_private};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::Path;
use subtle::ConstantTimeEq;
use thiserror::Error;

const FLEET_KEY_LEN: usize = 32;
const TOKEN_SECRET_LEN: usize = 32;
const TOKEN_ID_LEN: usize = 16;

/// Errors from fleet token and key operations.
#[derive(Debug, Error)]
pub enum FleetError {
    /// An I/O error occurred while reading or writing fleet state.
    #[error("fleet I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// The fleet signing key file was not found.
    #[error("fleet signing key not found")]
    KeyNotFound,

    /// The fleet signing key contents are invalid.
    #[error("invalid fleet signing key")]
    InvalidKey,

    /// The fleet token has been revoked.
    #[error("fleet token revoked")]
    TokenRevoked,

    /// Fleet state could not be serialized or deserialized.
    #[error("fleet state serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// An HTTP request failed during fleet communication.
    #[error("fleet HTTP error: {0}")]
    HttpError(String),

    /// Attempted to register a duplicate node.
    #[error("duplicate node")]
    DuplicateNode,

    /// Attempted to access a node that does not exist.
    #[error("node not found")]
    NodeNotFound,
}

/// Fleet signing key for the primary node.
pub struct FleetKey {
    /// Raw key bytes (32 bytes).
    key_bytes: Vec<u8>,
}

/// A fleet bearer token for node authentication.
#[derive(Clone, Serialize, Deserialize)]
pub struct FleetToken {
    /// Unique token identifier.
    pub token_id: String,
    /// Node this token was issued for.
    pub node_id: String,
    /// When the token was issued (unix ms).
    pub issued_at_ms: u64,
    /// Whether the token has been revoked.
    pub revoked: bool,
    /// The bearer token string (hex-encoded random bytes).
    pub secret: String,
}

impl FleetKey {
    /// Generate a new random fleet key.
    pub fn generate() -> Result<Self, FleetError> {
        let key_bytes = random_bytes(FLEET_KEY_LEN)?;
        Ok(Self { key_bytes })
    }

    /// Load from a file path.
    pub fn load(path: &Path) -> Result<Self, FleetError> {
        let key_bytes = load_key_bytes(path)?;
        validate_key_bytes(&key_bytes)?;
        Ok(Self { key_bytes })
    }

    /// Save to a file path (mode 0600).
    pub fn save(&self, path: &Path) -> Result<(), FleetError> {
        validate_key_bytes(&self.key_bytes)?;
        write_private(path, &self.key_bytes)
    }

    /// Sign a message (HMAC-SHA256).
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let key = hmac_key(&self.key_bytes);
        hmac::sign(&key, message).as_ref().to_vec()
    }

    /// Verify a signature.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        let key = hmac_key(&self.key_bytes);
        hmac::verify(&key, message, signature).is_ok()
    }
}

impl Drop for FleetKey {
    fn drop(&mut self) {
        self.key_bytes.iter_mut().for_each(|byte| *byte = 0);
        std::hint::black_box(&self.key_bytes);
    }
}

impl fmt::Debug for FleetToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FleetToken")
            .field("token_id", &self.token_id)
            .field("node_id", &self.node_id)
            .field("issued_at_ms", &self.issued_at_ms)
            .field("revoked", &self.revoked)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

impl FleetToken {
    /// Generate a new token for a node.
    pub fn generate(node_id: &str) -> Result<Self, FleetError> {
        Ok(Self {
            token_id: random_hex(TOKEN_ID_LEN)?,
            node_id: node_id.to_string(),
            issued_at_ms: current_time_ms(),
            revoked: false,
            secret: random_hex(TOKEN_SECRET_LEN)?,
        })
    }

    /// Check if the token matches a presented bearer string.
    pub fn verify_secret(&self, presented: &str) -> bool {
        self.secret.as_bytes().ct_eq(presented.as_bytes()).into()
    }

    /// Revoke this token.
    pub fn revoke(&mut self) {
        self.revoked = true;
    }
}

fn hmac_key(key_bytes: &[u8]) -> hmac::Key {
    hmac::Key::new(hmac::HMAC_SHA256, key_bytes)
}

fn load_key_bytes(path: &Path) -> Result<Vec<u8>, FleetError> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(FleetError::KeyNotFound),
        Err(error) => Err(FleetError::IoError(error)),
    }
}

fn validate_key_bytes(key_bytes: &[u8]) -> Result<(), FleetError> {
    if key_bytes.len() == FLEET_KEY_LEN {
        Ok(())
    } else {
        Err(FleetError::InvalidKey)
    }
}

fn random_bytes(len: usize) -> Result<Vec<u8>, FleetError> {
    let mut bytes = vec![0_u8; len];
    SystemRandom::new().fill(&mut bytes).map_err(|_| {
        FleetError::IoError(std::io::Error::other("failed to generate random bytes"))
    })?;
    Ok(bytes)
}

fn random_hex(len: usize) -> Result<String, FleetError> {
    let bytes = random_bytes(len)?;
    Ok(encode_hex(&bytes))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_utils::assert_private_permissions;
    use tempfile::TempDir;

    #[test]
    fn generate_key_produces_32_bytes() {
        let key = FleetKey::generate().expect("fleet key should generate");
        assert_eq!(key.key_bytes.len(), FLEET_KEY_LEN);
    }

    #[test]
    fn save_and_load_key_roundtrip() {
        let temp_dir = TempDir::new().expect("tempdir");
        let path = temp_dir.path().join("fleet.key");
        let key = FleetKey::generate().expect("fleet key should generate");

        key.save(&path).expect("fleet key should save");
        let loaded = FleetKey::load(&path).expect("fleet key should load");

        assert_eq!(loaded.key_bytes, key.key_bytes);
        assert_private_permissions(&path);
    }

    #[test]
    fn load_missing_key_returns_key_not_found() {
        let temp_dir = TempDir::new().expect("tempdir");
        let path = temp_dir.path().join("missing.key");

        let result = FleetKey::load(&path);
        assert!(matches!(result, Err(FleetError::KeyNotFound)));
    }

    #[test]
    fn load_rejects_invalid_key_length() {
        let temp_dir = TempDir::new().expect("tempdir");
        let path = temp_dir.path().join("short.key");

        fs::write(&path, [1_u8; 16]).expect("invalid key file should write");

        let result = FleetKey::load(&path);
        assert!(matches!(result, Err(FleetError::InvalidKey)));
    }

    #[test]
    fn generate_token_produces_unique_secrets() {
        let first = FleetToken::generate("node-1").expect("fleet token should generate");
        let second = FleetToken::generate("node-1").expect("fleet token should generate");

        assert_ne!(first.secret, second.secret);
        assert_ne!(first.token_id, second.token_id);
    }

    #[test]
    fn debug_redacts_secret() {
        let token = FleetToken::generate("node-1").expect("fleet token should generate");
        let debug_output = format!("{token:?}");

        assert!(debug_output.contains("[REDACTED]"));
        assert!(!debug_output.contains(&token.secret));
    }

    #[test]
    fn token_serialization_roundtrip_preserves_fields() {
        let token = FleetToken::generate("node-1").expect("fleet token should generate");
        let json = serde_json::to_string(&token).expect("fleet token should serialize");
        let decoded: FleetToken =
            serde_json::from_str(&json).expect("fleet token should deserialize");

        assert_eq!(decoded.token_id, token.token_id);
        assert_eq!(decoded.node_id, token.node_id);
        assert_eq!(decoded.issued_at_ms, token.issued_at_ms);
        assert_eq!(decoded.revoked, token.revoked);
        assert_eq!(decoded.secret, token.secret);
        assert!(decoded.verify_secret(&token.secret));
    }

    #[test]
    fn verify_secret_accepts_correct_token() {
        let token = FleetToken::generate("node-1").expect("fleet token should generate");
        assert!(token.verify_secret(&token.secret));
    }

    #[test]
    fn verify_secret_rejects_wrong_token() {
        let token = FleetToken::generate("node-1").expect("fleet token should generate");
        let wrong_secret = FleetToken::generate("node-2")
            .expect("fleet token should generate")
            .secret;

        assert!(!token.verify_secret(&wrong_secret));
    }

    #[test]
    fn revoke_sets_flag() {
        let mut token = FleetToken::generate("node-1").expect("fleet token should generate");
        token.revoke();
        assert!(token.revoked);
    }

    #[test]
    fn revoked_token_still_verifies_secret() {
        let mut token = FleetToken::generate("node-1").expect("fleet token should generate");
        let secret = token.secret.clone();

        token.revoke();

        assert!(token.verify_secret(&secret));
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let key = FleetKey::generate().expect("fleet key should generate");
        let message = b"fleet-message";
        let signature = key.sign(message);

        assert!(key.verify(message, &signature));
    }

    #[test]
    fn verify_rejects_tampered_message() {
        let key = FleetKey::generate().expect("fleet key should generate");
        let message = b"fleet-message";
        let signature = key.sign(message);

        assert!(!key.verify(b"tampered-message", &signature));
    }
}
