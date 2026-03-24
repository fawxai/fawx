//! Ed25519 signing and verification for WASM skills.

use fx_core::error::SkillError;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};

/// Sign WASM bytes with an Ed25519 private key.
pub fn sign_skill(wasm_bytes: &[u8], private_key: &[u8]) -> Result<Vec<u8>, SkillError> {
    let key_pair = Ed25519KeyPair::from_pkcs8(private_key)
        .map_err(|e| SkillError::Load(format!("Invalid Ed25519 private key: {}", e)))?;

    let signature = key_pair.sign(wasm_bytes);
    Ok(signature.as_ref().to_vec())
}

/// Verify WASM bytes against an Ed25519 signature and public key.
pub fn verify_skill(
    wasm_bytes: &[u8],
    signature: &[u8],
    public_key: &[u8],
) -> Result<bool, SkillError> {
    let public_key = UnparsedPublicKey::new(&ED25519, public_key);

    match public_key.verify(wasm_bytes, signature) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Generate an Ed25519 keypair for development.
///
/// Returns `(private_key_pkcs8, public_key)`.
pub fn generate_keypair() -> Result<(Vec<u8>, Vec<u8>), SkillError> {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng)
        .map_err(|e| SkillError::Load(format!("Failed to generate keypair: {}", e)))?;

    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|e| SkillError::Load(format!("Failed to parse generated keypair: {}", e)))?;

    Ok((
        pkcs8.as_ref().to_vec(),
        key_pair.public_key().as_ref().to_vec(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_keypair() {
        let (private_key, public_key) = generate_keypair().expect("Should generate");
        assert!(!private_key.is_empty());
        assert!(!public_key.is_empty());
        assert_eq!(public_key.len(), 32); // Ed25519 public keys are 32 bytes
    }

    #[test]
    fn test_sign_and_verify() {
        let (private_key, public_key) = generate_keypair().expect("Should generate");
        let data = b"test wasm module";

        let signature = sign_skill(data, &private_key).expect("Should sign");
        assert!(!signature.is_empty());

        let valid = verify_skill(data, &signature, &public_key).expect("Should verify");
        assert!(valid);
    }

    #[test]
    fn test_verify_wrong_key() {
        let (private_key1, _) = generate_keypair().expect("Should generate");
        let (_, public_key2) = generate_keypair().expect("Should generate");
        let data = b"test wasm module";

        let signature = sign_skill(data, &private_key1).expect("Should sign");

        let valid = verify_skill(data, &signature, &public_key2).expect("Should verify");
        assert!(!valid);
    }

    #[test]
    fn test_verify_tampered_data() {
        let (private_key, public_key) = generate_keypair().expect("Should generate");
        let data = b"test wasm module";

        let signature = sign_skill(data, &private_key).expect("Should sign");

        let tampered = b"test wasm module modified";
        let valid = verify_skill(tampered, &signature, &public_key).expect("Should verify");
        assert!(!valid);
    }

    #[test]
    fn test_sign_with_invalid_key() {
        let invalid_key = vec![0u8; 32];
        let data = b"test wasm module";

        let result = sign_skill(data, &invalid_key);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Load(_))));
    }

    #[test]
    fn test_verify_with_invalid_signature() {
        let (_, public_key) = generate_keypair().expect("Should generate");
        let data = b"test wasm module";
        let invalid_signature = vec![0u8; 64];

        let valid = verify_skill(data, &invalid_signature, &public_key).expect("Should verify");
        assert!(!valid);
    }
}
