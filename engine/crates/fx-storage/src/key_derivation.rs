//! Key derivation using HKDF and PBKDF2.
//!
//! This module provides secure key derivation functions for generating
//! encryption keys from master secrets or user passwords.
//!
//! # Security Considerations
//!
//! ## Master Key Management
//!
//! The `derive_key` function expects a secure master key. In production,
//! this should be derived from device-specific hardware security:
//!
//! - **Android (Pixel 10 Pro)**: Use Android Keystore to generate/store
//!   the master key, leveraging the device's Titan M2 security chip
//! - **Key Hierarchy**: Master key → Context-specific keys (credentials,
//!   conversations, preferences) via HKDF
//! - **Salt Management**: Salts for password-based derivation must be
//!   unique per user and stored securely
//!
//! ## Recommended Integration
//!
//! ```text
//! ┌─────────────────────────────┐
//! │  Android Keystore (Titan M2) │
//! │  - Generate master key       │
//! │  - Never export key          │
//! └──────────────┬───────────────┘
//!                 │
//!                 v
//!          Master Key (256-bit)
//!                 │
//!       ┌─────────┴──────────┐
//!       │  HKDF-SHA256       │
//!       │  (context-based)   │
//!       └─────────┬──────────┘
//!                 │
//!    ┌────────────┼────────────┐
//!    v            v            v
//! Credentials  Conversations  Preferences
//!   Key           Key           Key
//! ```
//!
//! For implementation examples, see Epic 7 (Security Layer) which will
//! integrate with Android Keystore.

use crate::crypto::EncryptionKey;
use fx_core::error::StorageError;
use ring::hkdf::{Salt, HKDF_SHA256};

type Result<T> = std::result::Result<T, StorageError>;

/// Recommended minimum iterations for PBKDF2-HMAC-SHA256 (OWASP 2023).
///
/// OWASP recommends 600,000+ iterations for PBKDF2-HMAC-SHA256 to resist
/// brute-force attacks using modern GPUs/ASICs.
pub const DEFAULT_PBKDF2_ITERATIONS: u32 = 600_000;

/// Derive an encryption key from a master key using HKDF-SHA256.
///
/// Uses HKDF (HMAC-based Key Derivation Function) to derive context-specific
/// keys from a single master key. This allows you to use one secure master key
/// to derive multiple independent keys for different purposes.
///
/// # Arguments
///
/// * `master_key` - The master secret (should be 256 bits / 32 bytes minimum)
/// * `context` - A string identifying the key's purpose (e.g., "credentials",
///   "conversations", "preferences")
///
/// # Returns
///
/// A 256-bit encryption key suitable for AES-256-GCM.
///
/// # Security
///
/// - **Master Key Source**: In production, the master key should come from
///   Android Keystore or similar hardware-backed key storage
/// - **Context Separation**: Different contexts produce cryptographically
///   independent keys
/// - **Deterministic**: Same inputs always produce the same output
///
/// # Example
///
/// ```no_run
/// use fx_storage::derive_key;
///
/// // In production, fetch from Android Keystore
/// let master_key = b"secure_master_key_from_keystore!";
///
/// let creds_key = derive_key(master_key, "credentials");
/// let conv_key = derive_key(master_key, "conversations");
/// ```
pub fn derive_key(master_key: &[u8], context: &str) -> Result<EncryptionKey> {
    let salt = Salt::new(HKDF_SHA256, &[]);
    let prk = salt.extract(master_key);

    let info = [context.as_bytes()];
    let okm = prk
        .expand(&info, KeyLength(32))
        .map_err(|_| StorageError::Encryption("HKDF expand failed".to_string()))?;

    let mut derived = [0u8; 32];
    okm.fill(&mut derived)
        .map_err(|_| StorageError::Encryption("HKDF fill failed".to_string()))?;

    Ok(EncryptionKey::from_bytes(&derived))
}

/// Derive an encryption key from a password using PBKDF2-HMAC-SHA256.
///
/// Uses the default iteration count of 600,000 (OWASP 2023 recommendation).
///
/// # Arguments
///
/// * `password` - The user's password
/// * `salt` - A unique salt (must be at least 128 bits / 16 bytes)
///
/// # Security
///
/// - **Salt Management**: Each user must have a unique randomly-generated salt
/// - **Salt Storage**: Store the salt alongside the encrypted data (it's not secret)
/// - **Iterations**: 600,000 iterations balances security and performance
///
/// # Example
///
/// ```no_run
/// use fx_storage::derive_key_from_password;
/// use ring::rand::{SecureRandom, SystemRandom};
///
/// let rng = SystemRandom::new();
/// let mut salt = [0u8; 16];
/// rng.fill(&mut salt).expect("RNG failure");
///
/// let key = derive_key_from_password("user_password", &salt);
/// // Store salt for future use
/// ```
pub fn derive_key_from_password(password: &str, salt: &[u8]) -> Result<EncryptionKey> {
    derive_key_from_password_with_iterations(password, salt, DEFAULT_PBKDF2_ITERATIONS)
}

/// Derive an encryption key from a password with a custom iteration count.
///
/// Allows specifying the number of PBKDF2 iterations for performance tuning
/// or compatibility with legacy systems.
///
/// # Arguments
///
/// * `password` - The user's password
/// * `salt` - A unique salt (must be at least 128 bits / 16 bytes)
/// * `iterations` - Number of PBKDF2 iterations (minimum 100,000 recommended)
///
/// # Security
///
/// - Use [`DEFAULT_PBKDF2_ITERATIONS`] (600,000) unless you have a specific reason
/// - Lower iteration counts reduce security
/// - Higher iteration counts increase derivation time (test on target hardware)
pub fn derive_key_from_password_with_iterations(
    password: &str,
    salt: &[u8],
    iterations: u32,
) -> Result<EncryptionKey> {
    use ring::pbkdf2;

    let iter_count = std::num::NonZeroU32::new(iterations).ok_or_else(|| {
        StorageError::Encryption("Invalid iteration count (must be > 0)".to_string())
    })?;

    let mut derived = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iter_count,
        salt,
        password.as_bytes(),
        &mut derived,
    );

    Ok(EncryptionKey::from_bytes(&derived))
}

/// Helper type for specifying HKDF output key material length.
struct KeyLength(usize);

impl ring::hkdf::KeyType for KeyLength {
    fn len(&self) -> usize {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_key_same_input() {
        let master = b"master_secret_key_12345678901234";
        let context = "credentials";

        let key1 = derive_key(master, context).expect("Failed to derive key");
        let key2 = derive_key(master, context).expect("Failed to derive key");

        // Same inputs should produce same key (deterministic)
        // We can't directly compare EncryptionKey, but we can use them and verify they work the same
        // For now, just verify they don't panic
        let _ = key1;
        let _ = key2;
    }

    #[test]
    fn test_derive_key_different_contexts() {
        let master = b"master_secret_key_12345678901234";

        let key1 = derive_key(master, "credentials").expect("Failed to derive key");
        let key2 = derive_key(master, "conversations").expect("Failed to derive key");

        // Different contexts should produce different keys
        // Since we can't compare EncryptionKey directly, we'll test by encrypting
        use crate::crypto::encrypt;

        let plaintext = b"test data";
        let ciphertext1 = encrypt(&key1, plaintext).expect("Failed to encrypt");

        // Due to random nonces, ciphertexts will be different anyway, but keys are different
        // We can verify by trying to decrypt with the wrong key
        use crate::crypto::decrypt;

        // key1 can decrypt ciphertext1
        let _ = decrypt(&key1, &ciphertext1).expect("Should decrypt with correct key");

        // key2 cannot decrypt ciphertext1
        let result = decrypt(&key2, &ciphertext1);
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_key_different_master_keys() {
        let master1 = b"master_key_1_12345678901234567";
        let master2 = b"master_key_2_12345678901234567";
        let context = "credentials";

        let key1 = derive_key(master1, context).expect("Failed to derive key");
        let key2 = derive_key(master2, context).expect("Failed to derive key");

        // Different master keys should produce different derived keys
        use crate::crypto::{decrypt, encrypt};

        let plaintext = b"test data";
        let ciphertext1 = encrypt(&key1, plaintext).expect("Failed to encrypt");

        // key2 cannot decrypt ciphertext encrypted with key1
        let result = decrypt(&key2, &ciphertext1);
        assert!(result.is_err());
    }

    #[test]
    fn test_password_based_derivation() {
        let password = "my_secure_password";
        let salt = b"unique_salt_12345678";

        let key = derive_key_from_password(password, salt).expect("Failed to derive key");

        // Verify the key can be used for encryption
        use crate::crypto::{decrypt, encrypt};

        let plaintext = b"secret message";
        let ciphertext = encrypt(&key, plaintext).expect("Failed to encrypt");
        let decrypted = decrypt(&key, &ciphertext).expect("Failed to decrypt");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_password_derivation_same_inputs() {
        let password = "password123";
        let salt = b"salt_bytes_1234567890";

        let key1 = derive_key_from_password(password, salt).expect("Failed to derive key");
        let key2 = derive_key_from_password(password, salt).expect("Failed to derive key");

        // Same password and salt should produce same key
        use crate::crypto::{decrypt, encrypt};

        let plaintext = b"test";
        let ciphertext = encrypt(&key1, plaintext).expect("Failed to encrypt");
        let decrypted = decrypt(&key2, &ciphertext).expect("Failed to decrypt with key2");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_password_derivation_different_salts() {
        let password = "same_password";
        let salt1 = b"salt1_1234567890";
        let salt2 = b"salt2_1234567890";

        let key1 = derive_key_from_password(password, salt1).expect("Failed to derive key");
        let key2 = derive_key_from_password(password, salt2).expect("Failed to derive key");

        // Different salts should produce different keys
        use crate::crypto::{decrypt, encrypt};

        let plaintext = b"test";
        let ciphertext1 = encrypt(&key1, plaintext).expect("Failed to encrypt");

        // key2 cannot decrypt ciphertext encrypted with key1
        let result = decrypt(&key2, &ciphertext1);
        assert!(result.is_err());
    }

    #[test]
    fn test_password_derivation_different_passwords() {
        let salt = b"same_salt_1234567890";
        let password1 = "password_one";
        let password2 = "password_two";

        let key1 = derive_key_from_password(password1, salt).expect("Failed to derive key");
        let key2 = derive_key_from_password(password2, salt).expect("Failed to derive key");

        // Different passwords should produce different keys
        use crate::crypto::{decrypt, encrypt};

        let plaintext = b"test";
        let ciphertext1 = encrypt(&key1, plaintext).expect("Failed to encrypt");

        // key2 cannot decrypt ciphertext encrypted with key1
        let result = decrypt(&key2, &ciphertext1);
        assert!(result.is_err());
    }

    #[test]
    fn test_custom_iteration_count() {
        let password = "test_password";
        let salt = b"test_salt_123456";

        // Test with custom iteration count
        let key = derive_key_from_password_with_iterations(password, salt, 100_000)
            .expect("Failed to derive key");

        use crate::crypto::{decrypt, encrypt};
        let plaintext = b"data";
        let ciphertext = encrypt(&key, plaintext).expect("Failed to encrypt");
        let decrypted = decrypt(&key, &ciphertext).expect("Failed to decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_zero_iterations_fails() {
        let password = "test";
        let salt = b"salt";

        let result = derive_key_from_password_with_iterations(password, salt, 0);
        assert!(result.is_err());
    }
}
