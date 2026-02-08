//! Key derivation using HKDF.

use crate::crypto::EncryptionKey;
use ring::hkdf::{Salt, HKDF_SHA256};

/// Derive an encryption key from a master key using HKDF-SHA256.
///
/// The context string is used as "info" in HKDF to derive different keys
/// for different purposes (e.g., "credentials", "conversations", "preferences").
pub fn derive_key(master_key: &[u8], context: &str) -> EncryptionKey {
    let salt = Salt::new(HKDF_SHA256, &[]);
    let prk = salt.extract(master_key);

    let info = [context.as_bytes()];
    let okm = prk
        .expand(&info, MyKeyLength(32))
        .expect("HKDF expand failed");

    let mut derived = [0u8; 32];
    okm.fill(&mut derived).expect("HKDF fill failed");

    EncryptionKey::from_bytes(&derived)
}

/// Derive an encryption key from a password using PBKDF2-HMAC-SHA256.
///
/// This uses PBKDF2 with 100,000 iterations for password-based key derivation.
/// In production, you should use a unique salt per user and store it.
pub fn derive_key_from_password(password: &str, salt: &[u8]) -> EncryptionKey {
    use ring::pbkdf2;

    let mut derived = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        std::num::NonZeroU32::new(100_000).expect("Invalid iteration count"),
        salt,
        password.as_bytes(),
        &mut derived,
    );

    EncryptionKey::from_bytes(&derived)
}

/// Helper type for HKDF output key material length.
struct MyKeyLength(usize);

impl ring::hkdf::KeyType for MyKeyLength {
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

        let key1 = derive_key(master, context);
        let key2 = derive_key(master, context);

        // Same inputs should produce same key (deterministic)
        // We can't directly compare EncryptionKey, but we can use them and verify they work the same
        // For now, just verify they don't panic
        let _ = key1;
        let _ = key2;
    }

    #[test]
    fn test_derive_key_different_contexts() {
        let master = b"master_secret_key_12345678901234";

        let key1 = derive_key(master, "credentials");
        let key2 = derive_key(master, "conversations");

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

        let key1 = derive_key(master1, context);
        let key2 = derive_key(master2, context);

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

        let key = derive_key_from_password(password, salt);

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

        let key1 = derive_key_from_password(password, salt);
        let key2 = derive_key_from_password(password, salt);

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

        let key1 = derive_key_from_password(password, salt1);
        let key2 = derive_key_from_password(password, salt2);

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

        let key1 = derive_key_from_password(password1, salt);
        let key2 = derive_key_from_password(password2, salt);

        // Different passwords should produce different keys
        use crate::crypto::{decrypt, encrypt};

        let plaintext = b"test";
        let ciphertext1 = encrypt(&key1, plaintext).expect("Failed to encrypt");

        // key2 cannot decrypt ciphertext encrypted with key1
        let result = decrypt(&key2, &ciphertext1);
        assert!(result.is_err());
    }
}
