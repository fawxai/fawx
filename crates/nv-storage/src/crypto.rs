//! Encryption layer using AES-256-GCM.

use nv_core::error::StorageError;
use ring::aead::{
    Aad, BoundKey, Nonce, NonceSequence, OpeningKey, SealingKey, UnboundKey, AES_256_GCM,
};
use ring::error::Unspecified;
use ring::rand::{SecureRandom, SystemRandom};

type Result<T> = std::result::Result<T, StorageError>;

const NONCE_LEN: usize = 12;

/// Encryption key for AES-256-GCM.
#[derive(Clone)]
pub struct EncryptionKey {
    key_bytes: [u8; 32],
}

impl EncryptionKey {
    /// Create an encryption key from a 32-byte array.
    pub fn from_bytes(key: &[u8; 32]) -> Self {
        Self { key_bytes: *key }
    }
}

/// Encrypt plaintext using AES-256-GCM with a random nonce.
/// The nonce is prepended to the ciphertext.
pub fn encrypt(key: &EncryptionKey, plaintext: &[u8]) -> Result<Vec<u8>> {
    let rng = SystemRandom::new();

    // Generate random nonce
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
/// The nonce is expected to be prepended to the ciphertext.
pub fn decrypt(key: &EncryptionKey, ciphertext: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.len() < NONCE_LEN {
        return Err(StorageError::Encryption("Ciphertext too short".to_string()));
    }

    // Extract nonce
    let (nonce_bytes, encrypted_data) = ciphertext.split_at(NONCE_LEN);
    let nonce_array: [u8; NONCE_LEN] = nonce_bytes
        .try_into()
        .map_err(|_| StorageError::Encryption("Invalid nonce".to_string()))?;

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

/// Single-use nonce sequence for AES-GCM.
struct SingleUseNonce(Option<Nonce>);

impl NonceSequence for SingleUseNonce {
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
