//! Encryption layer using AES-256-GCM.
//!
//! This module provides authenticated encryption using AES-256-GCM from the `ring` crate.
//! All encryption operations use randomly generated nonces to ensure semantic security.

use fx_core::error::StorageError;
use ring::aead::{
    Aad, BoundKey, Nonce, NonceSequence, OpeningKey, SealingKey, UnboundKey, AES_256_GCM,
};
use ring::error::Unspecified;
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroize;

type Result<T> = std::result::Result<T, StorageError>;

/// Nonce length for AES-GCM (96 bits / 12 bytes as per NIST SP 800-38D)
const NONCE_LEN: usize = 12;

/// Encryption key for AES-256-GCM authenticated encryption.
///
/// Wraps a 32-byte (256-bit) key used for AES-256-GCM operations.
/// The key material is automatically zeroed when dropped to prevent
/// leakage via memory dumps or swap.
///
/// # Security
///
/// Keys should be derived using [`crate::derive_key`] (HKDF) or
/// [`crate::derive_key_from_password`] (PBKDF2) rather than
/// constructed directly from arbitrary bytes.
///
/// # Example
///
/// ```no_run
/// use fx_storage::{EncryptionKey, derive_key};
///
/// let master_key = b"secure_master_key_32_bytes_long!";
/// let derived_key = derive_key(master_key, "context").expect("Key derivation failed");
/// ```
#[derive(Clone)]
pub struct EncryptionKey {
    /// The raw 32-byte AES-256 key material.
    /// This is zeroed on drop to prevent key leakage.
    key_bytes: [u8; 32],
}

impl Drop for EncryptionKey {
    fn drop(&mut self) {
        self.key_bytes.zeroize();
    }
}

impl EncryptionKey {
    /// Create an encryption key from a 32-byte array.
    ///
    /// # Arguments
    ///
    /// * `key` - A 32-byte array containing the key material
    ///
    /// # Security
    ///
    /// The key should be derived from a secure source using HKDF or PBKDF2.
    /// **Do not use predictable or hardcoded values in production.**
    ///
    /// For production use, prefer [`crate::derive_key`] or
    /// [`crate::derive_key_from_password`].
    pub fn from_bytes(key: &[u8; 32]) -> Self {
        Self { key_bytes: *key }
    }
}

impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionKey")
            .field("key_bytes", &"<redacted>")
            .finish()
    }
}

/// Encrypt plaintext using AES-256-GCM with a random nonce.
///
/// Uses `ring::rand::SystemRandom` to generate a cryptographically secure
/// random 96-bit nonce. The nonce is prepended to the ciphertext for use
/// during decryption.
///
/// # Arguments
///
/// * `key` - The encryption key (must be 256 bits)
/// * `plaintext` - The data to encrypt
///
/// # Returns
///
/// `[nonce (12 bytes) | ciphertext | auth_tag (16 bytes)]`
///
/// # Errors
///
/// Returns [`StorageError::Encryption`] if:
/// - Random nonce generation fails
/// - Encryption operation fails
pub fn encrypt(key: &EncryptionKey, plaintext: &[u8]) -> Result<Vec<u8>> {
    let rng = SystemRandom::new();

    // Generate cryptographically secure random nonce (12 bytes for GCM).
    // SystemRandom::fill() guarantees it fills the entire buffer or errors,
    // so we don't need to verify the length.
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| StorageError::Encryption("Failed to generate nonce".to_string()))?;

    // Create sealing key
    let unbound_key = UnboundKey::new(&AES_256_GCM, &key.key_bytes)
        .map_err(|_| StorageError::Encryption("Failed to create encryption key".to_string()))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut sealing_key = SealingKey::new(unbound_key, SingleUseNonce(Some(nonce)));

    // Encrypt
    let mut ciphertext = plaintext.to_vec();
    sealing_key
        .seal_in_place_append_tag(Aad::empty(), &mut ciphertext)
        .map_err(|_| StorageError::Encryption("Failed to encrypt".to_string()))?;

    // Prepend nonce to ciphertext
    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt ciphertext using AES-256-GCM.
///
/// Expects the nonce to be prepended to the ciphertext as produced by [`encrypt`].
///
/// # Arguments
///
/// * `key` - The encryption key (must match the key used for encryption)
/// * `ciphertext` - The encrypted data: `[nonce (12 bytes) | ciphertext | auth_tag (16 bytes)]`
///
/// # Returns
///
/// The original plaintext if decryption and authentication succeed.
///
/// # Errors
///
/// Returns [`StorageError::Encryption`] if:
/// - Ciphertext is too short (< 12 bytes for nonce)
/// - Authentication tag verification fails (data was tampered with or wrong key)
/// - Decryption operation fails
///
/// # Security
///
/// AES-GCM provides authenticated encryption. If the authentication tag doesn't
/// match, the data was either:
/// - Encrypted with a different key
/// - Modified/tampered with after encryption
/// - Corrupted during storage/transmission
pub fn decrypt(key: &EncryptionKey, ciphertext: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.len() < NONCE_LEN {
        tracing::debug!(
            "Ciphertext too short: {} bytes (need at least {})",
            ciphertext.len(),
            NONCE_LEN
        );
        return Err(StorageError::Encryption("Ciphertext too short".to_string()));
    }

    // Extract nonce from the first 12 bytes
    let (nonce_bytes, encrypted_data) = ciphertext.split_at(NONCE_LEN);
    let nonce_array: [u8; NONCE_LEN] = nonce_bytes.try_into().map_err(|e| {
        tracing::debug!("Nonce conversion failed: {:?}", e);
        StorageError::Encryption("Invalid nonce".to_string())
    })?;

    // Create opening key
    let unbound_key = UnboundKey::new(&AES_256_GCM, &key.key_bytes)
        .map_err(|_| StorageError::Encryption("Failed to create decryption key".to_string()))?;
    let nonce = Nonce::assume_unique_for_key(nonce_array);
    let mut opening_key = OpeningKey::new(unbound_key, SingleUseNonce(Some(nonce)));

    // Decrypt
    let mut plaintext = encrypted_data.to_vec();
    let decrypted = opening_key
        .open_in_place(Aad::empty(), &mut plaintext)
        .map_err(|_| StorageError::Encryption("Failed to decrypt".to_string()))?;

    Ok(decrypted.to_vec())
}

/// Single-use nonce sequence for AES-GCM operations.
///
/// This type ensures each nonce can only be used once, as required by
/// the `ring::aead` API. Attempting to use the nonce a second time will
/// return an error.
struct SingleUseNonce(Option<Nonce>);

impl NonceSequence for SingleUseNonce {
    /// Advance to the next nonce (which doesn't exist for single-use).
    ///
    /// Returns the nonce on first call, then errors on subsequent calls.
    fn advance(&mut self) -> std::result::Result<Nonce, Unspecified> {
        self.0.take().ok_or(Unspecified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> EncryptionKey {
        EncryptionKey::from_bytes(&[42u8; 32])
    }

    fn different_key() -> EncryptionKey {
        EncryptionKey::from_bytes(&[99u8; 32])
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = test_key();
        let plaintext = b"Hello, World!";

        let ciphertext = encrypt(&key, plaintext).expect("Failed to encrypt");
        let decrypted = decrypt(&key, &ciphertext).expect("Failed to decrypt");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_with_wrong_key() {
        let key1 = test_key();
        let key2 = different_key();
        let plaintext = b"Secret data";

        let ciphertext = encrypt(&key1, plaintext).expect("Failed to encrypt");
        let result = decrypt(&key2, &ciphertext);

        assert!(result.is_err());
        assert!(matches!(result, Err(StorageError::Encryption(_))));
    }

    #[test]
    fn test_decrypt_tampered_ciphertext() {
        let key = test_key();
        let plaintext = b"Original message";

        let mut ciphertext = encrypt(&key, plaintext).expect("Failed to encrypt");

        // Tamper with the ciphertext (skip nonce, modify encrypted data)
        if ciphertext.len() > NONCE_LEN + 5 {
            ciphertext[NONCE_LEN + 5] ^= 0xFF;
        }

        let result = decrypt(&key, &ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn test_different_plaintexts_produce_different_ciphertexts() {
        let key = test_key();
        let plaintext = b"Same message";

        let ciphertext1 = encrypt(&key, plaintext).expect("Failed to encrypt");
        let ciphertext2 = encrypt(&key, plaintext).expect("Failed to encrypt");

        // Due to random nonces, ciphertexts should be different
        assert_ne!(ciphertext1, ciphertext2);

        // But both should decrypt to the same plaintext
        let decrypted1 = decrypt(&key, &ciphertext1).expect("Failed to decrypt");
        let decrypted2 = decrypt(&key, &ciphertext2).expect("Failed to decrypt");
        assert_eq!(decrypted1, plaintext);
        assert_eq!(decrypted2, plaintext);
    }

    #[test]
    fn test_decrypt_too_short_ciphertext() {
        let key = test_key();
        let short_ciphertext = b"tooshort";

        let result = decrypt(&key, short_ciphertext);
        assert!(result.is_err());
        assert!(matches!(result, Err(StorageError::Encryption(_))));
    }

    #[test]
    fn test_empty_plaintext() {
        let key = test_key();
        let plaintext = b"";

        let ciphertext = encrypt(&key, plaintext).expect("Failed to encrypt");
        let decrypted = decrypt(&key, &ciphertext).expect("Failed to decrypt");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_large_plaintext() {
        let key = test_key();
        let plaintext = vec![0x42u8; 10_000];

        let ciphertext = encrypt(&key, &plaintext).expect("Failed to encrypt");
        let decrypted = decrypt(&key, &ciphertext).expect("Failed to decrypt");

        assert_eq!(decrypted, plaintext);
    }
}
