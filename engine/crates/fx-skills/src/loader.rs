//! Skill loader with signature verification.

use crate::manifest::SkillManifest;
use crate::signing::verify_skill;
use crate::{manifest::validate_manifest, manifest::Capability};
use fx_core::error::SkillError;
use wasmtime::{Engine, Module};

/// A loaded and verified WASM skill.
#[derive(Clone)]
pub struct LoadedSkill {
    module: Module,
    manifest: SkillManifest,
}

impl LoadedSkill {
    /// Get the compiled WASM module.
    pub fn module(&self) -> &Module {
        &self.module
    }

    /// Get the skill manifest.
    pub fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    /// Get the skill's capabilities.
    pub fn capabilities(&self) -> &[Capability] {
        &self.manifest.capabilities
    }
}

/// Loads and verifies WASM skills.
pub struct SkillLoader {
    engine: Engine,
    trusted_keys: Vec<Vec<u8>>,
}

impl SkillLoader {
    /// Create a new skill loader with trusted public keys.
    pub fn new(trusted_keys: Vec<Vec<u8>>) -> Self {
        let engine = Engine::default();
        Self {
            engine,
            trusted_keys,
        }
    }

    /// Create a skill loader that shares an engine with a SkillRuntime.
    ///
    /// Wasmtime requires modules and stores to use the same Engine instance.
    /// Use this when loading skills that will be executed by a specific runtime.
    pub fn with_engine(engine: Engine, trusted_keys: Vec<Vec<u8>>) -> Self {
        Self {
            engine,
            trusted_keys,
        }
    }

    /// Load a skill from WASM bytes, manifest, and optional signature.
    ///
    /// If a signature is provided, it must verify against one of the trusted keys.
    /// Uses module caching to speed up repeated loads of the same WASM bytes.
    pub fn load(
        &self,
        wasm_bytes: &[u8],
        manifest: &SkillManifest,
        signature: Option<&[u8]>,
    ) -> Result<LoadedSkill, SkillError> {
        // Validate manifest first
        validate_manifest(manifest)?;

        // If signature provided, verify it
        if let Some(sig) = signature {
            let mut verified = false;

            for public_key in &self.trusted_keys {
                if verify_skill(wasm_bytes, sig, public_key)? {
                    verified = true;
                    break;
                }
            }

            if !verified {
                return Err(SkillError::Load(
                    "Signature verification failed: no trusted key matched".to_string(),
                ));
            }
        }

        // Compile the WASM module safely (always from source, no unsafe deserialize)
        let (module, was_cached) = crate::cache::compile_module(&self.engine, wasm_bytes)?;
        if was_cached {
            tracing::debug!("Recompiled known skill '{}'", manifest.name);
        } else {
            tracing::debug!("Compiled new skill '{}'", manifest.name);
        }

        Ok(LoadedSkill {
            module,
            manifest: manifest.clone(),
        })
    }

    /// Get a reference to the WASM engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::generate_keypair;

    fn create_test_manifest() -> SkillManifest {
        SkillManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test skill".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            entry_point: "run".to_string(),
        }
    }

    fn create_minimal_wasm() -> Vec<u8> {
        // Minimal valid WASM module: magic + version
        vec![
            0x00, 0x61, 0x73, 0x6d, // magic: \0asm
            0x01, 0x00, 0x00, 0x00, // version: 1
        ]
    }

    #[test]
    fn test_load_unsigned_skill() {
        let loader = SkillLoader::new(vec![]);
        let manifest = create_test_manifest();
        let wasm = create_minimal_wasm();

        let skill = loader
            .load(&wasm, &manifest, None)
            .expect("Should load unsigned skill");

        assert_eq!(skill.manifest().name, "test");
    }

    #[test]
    fn test_load_with_valid_signature() {
        let (private_key, public_key) = generate_keypair().expect("Should generate");
        let loader = SkillLoader::new(vec![public_key.clone()]);

        let manifest = create_test_manifest();
        let wasm = create_minimal_wasm();

        let signature = crate::signing::sign_skill(&wasm, &private_key).expect("Should sign");

        let skill = loader
            .load(&wasm, &manifest, Some(&signature))
            .expect("Should load with valid signature");

        assert_eq!(skill.manifest().name, "test");
    }

    #[test]
    fn test_load_with_invalid_signature() {
        let (_, public_key1) = generate_keypair().expect("Should generate");
        let (private_key2, _) = generate_keypair().expect("Should generate");
        let loader = SkillLoader::new(vec![public_key1]);

        let manifest = create_test_manifest();
        let wasm = create_minimal_wasm();

        let signature = crate::signing::sign_skill(&wasm, &private_key2).expect("Should sign");

        let result = loader.load(&wasm, &manifest, Some(&signature));
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Load(_))));
    }

    #[test]
    fn test_load_with_invalid_wasm() {
        let loader = SkillLoader::new(vec![]);
        let manifest = create_test_manifest();
        let invalid_wasm = vec![0x00, 0x01, 0x02, 0x03];

        let result = loader.load(&invalid_wasm, &manifest, None);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Load(_))));
    }

    #[test]
    fn test_load_with_invalid_manifest() {
        let loader = SkillLoader::new(vec![]);
        let mut manifest = create_test_manifest();
        manifest.api_version = "invalid".to_string();

        let wasm = create_minimal_wasm();

        let result = loader.load(&wasm, &manifest, None);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::InvalidManifest(_))));
    }

    #[test]
    fn test_loaded_skill_capabilities() {
        let loader = SkillLoader::new(vec![]);
        let mut manifest = create_test_manifest();
        manifest.capabilities = vec![Capability::Network, Capability::Storage];

        let wasm = create_minimal_wasm();

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");

        assert_eq!(skill.capabilities().len(), 2);
        assert_eq!(skill.capabilities()[0], Capability::Network);
        assert_eq!(skill.capabilities()[1], Capability::Storage);
    }
}
