//! Policy file signing and verification using HMAC-SHA256.

use ring::hmac;

/// Sign policy content using HMAC-SHA256.
///
/// # Arguments
/// * `content` - Policy file content
/// * `key` - Secret key for signing
///
/// # Returns
/// HMAC signature bytes
pub fn sign_policy(content: &[u8], key: &[u8]) -> Vec<u8> {
    let signing_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let signature = hmac::sign(&signing_key, content);
    signature.as_ref().to_vec()
}

/// Verify policy signature using HMAC-SHA256.
///
/// # Arguments
/// * `content` - Policy file content
/// * `signature` - Expected signature
/// * `key` - Secret key for verification
///
/// # Returns
/// `true` if signature is valid, `false` otherwise
pub fn verify_policy(content: &[u8], signature: &[u8], key: &[u8]) -> bool {
    let verification_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::verify(&verification_key, content, signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_consistent() {
        let content = b"test policy content";
        let key = b"secret_key";

        let sig1 = sign_policy(content, key);
        let sig2 = sign_policy(content, key);

        assert_eq!(sig1, sig2, "Signatures should be consistent");
    }

    #[test]
    fn test_verify_valid() {
        let content = b"test policy content";
        let key = b"secret_key";

        let signature = sign_policy(content, key);
        assert!(verify_policy(content, &signature, key));
    }

    #[test]
    fn test_verify_invalid_signature() {
        let content = b"test policy content";
        let key = b"secret_key";
        let wrong_signature = vec![0u8; 32];

        assert!(!verify_policy(content, &wrong_signature, key));
    }

    #[test]
    fn test_verify_tampered_content() {
        let content = b"test policy content";
        let tampered_content = b"tampered policy content";
        let key = b"secret_key";

        let signature = sign_policy(content, key);
        assert!(!verify_policy(tampered_content, &signature, key));
    }

    #[test]
    fn test_verify_wrong_key() {
        let content = b"test policy content";
        let key = b"secret_key";
        let wrong_key = b"wrong_key";

        let signature = sign_policy(content, key);
        assert!(!verify_policy(content, &signature, wrong_key));
    }
}
