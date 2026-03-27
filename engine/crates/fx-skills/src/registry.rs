//! Skill registry for discovering and loading installed skills.

use crate::loader::{LoadedSkill, SkillLoader};
use crate::manifest::{parse_manifest, SkillManifest};
use fx_core::error::SkillError;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Registry of installed skills.
pub struct SkillRegistry {
    loader: SkillLoader,
    skills_dir: PathBuf,
}

impl SkillRegistry {
    /// Create a new skill registry with a default engine.
    ///
    /// Uses `~/.fawx/skills/` as the default skills directory.
    pub fn new() -> Result<Self, SkillError> {
        let loader = SkillLoader::new(vec![]);
        Self::with_loader(loader)
    }

    /// Create a skill registry sharing an existing wasmtime Engine.
    ///
    /// This ensures skills loaded from this registry can be executed
    /// by a `SkillRuntime` using the same Engine (wasmtime requires
    /// Module and Store to share an Engine).
    pub fn with_engine(engine: wasmtime::Engine) -> Result<Self, SkillError> {
        let loader = SkillLoader::with_engine(engine, vec![]);
        Self::with_loader(loader)
    }

    /// Internal: create registry with a specific loader.
    fn with_loader(loader: SkillLoader) -> Result<Self, SkillError> {
        let home = dirs::home_dir()
            .ok_or_else(|| SkillError::Load("Failed to get home directory".to_string()))?;

        let skills_dir = home.join(".fawx").join("skills");

        // Create directory if it doesn't exist
        fs::create_dir_all(&skills_dir)
            .map_err(|e| SkillError::Load(format!("Failed to create skills directory: {}", e)))?;

        Ok(Self { loader, skills_dir })
    }

    /// Get the skills directory path.
    pub fn skills_dir(&self) -> &PathBuf {
        &self.skills_dir
    }

    /// List all installed skill manifests.
    pub fn list_manifests(&self) -> Result<Vec<SkillManifest>, SkillError> {
        let entries: Vec<_> = fs::read_dir(&self.skills_dir)
            .map_err(|e| SkillError::Load(format!("Failed to read skills directory: {}", e)))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();

        let mut manifests = Vec::new();

        for entry in entries {
            let skill_dir = entry.path();
            let manifest_path = skill_dir.join("manifest.toml");

            if !manifest_path.exists() {
                tracing::warn!("Skipping {:?}: manifest.toml not found", skill_dir);
                continue;
            }

            // Read manifest
            let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| {
                SkillError::Load(format!(
                    "Failed to read manifest at {:?}: {}",
                    manifest_path, e
                ))
            })?;

            let manifest = parse_manifest(&manifest_content)?;
            manifests.push(manifest);
        }

        Ok(manifests)
    }

    /// Load all installed skills.
    pub fn load_all(&self) -> Result<HashMap<String, LoadedSkill>, SkillError> {
        let entries: Vec<_> = fs::read_dir(&self.skills_dir)
            .map_err(|e| SkillError::Load(format!("Failed to read skills directory: {}", e)))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();

        let mut skills = HashMap::new();

        for entry in entries {
            let skill_dir = entry.path();
            let manifest_path = skill_dir.join("manifest.toml");

            if !manifest_path.exists() {
                tracing::warn!("Skipping {:?}: manifest.toml not found", skill_dir);
                continue;
            }

            // Read manifest
            let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| {
                SkillError::Load(format!(
                    "Failed to read manifest at {:?}: {}",
                    manifest_path, e
                ))
            })?;

            let manifest = parse_manifest(&manifest_content)?;

            // Find WASM file
            let wasm_path = skill_dir.join(format!("{}.wasm", manifest.name));
            if !wasm_path.exists() {
                tracing::warn!(
                    "Skipping skill '{}': WASM file not found at {:?}",
                    manifest.name,
                    wasm_path
                );
                continue;
            }

            // Load WASM
            let wasm_bytes = fs::read(&wasm_path).map_err(|e| {
                SkillError::Load(format!("Failed to read WASM at {:?}: {}", wasm_path, e))
            })?;

            // Load skill
            match self.loader.load(&wasm_bytes, &manifest, None) {
                Ok(skill) => {
                    let name = manifest.name.clone();
                    skills.insert(name.clone(), skill);
                    tracing::debug!("Loaded skill '{}'", name);
                }
                Err(e) => {
                    tracing::error!("Failed to load skill '{}': {}", manifest.name, e);
                }
            }
        }

        Ok(skills)
    }

    /// Load a specific skill by name.
    pub fn load_skill(&self, name: &str) -> Result<LoadedSkill, SkillError> {
        let skill_dir = self.skills_dir.join(name);

        if !skill_dir.exists() {
            return Err(SkillError::Load(format!(
                "Skill '{}' not found in registry",
                name
            )));
        }

        // Read manifest
        let manifest_path = skill_dir.join("manifest.toml");
        let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| {
            SkillError::Load(format!(
                "Failed to read manifest at {:?}: {}",
                manifest_path, e
            ))
        })?;

        let manifest = parse_manifest(&manifest_content)?;

        // Read WASM
        let wasm_path = skill_dir.join(format!("{}.wasm", name));
        let wasm_bytes = fs::read(&wasm_path).map_err(|e| {
            SkillError::Load(format!("Failed to read WASM at {:?}: {}", wasm_path, e))
        })?;

        // Load skill
        self.loader.load(&wasm_bytes, &manifest, None)
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| panic!("Failed to create default SkillRegistry: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Capability;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_test_skill_dir(
        skills_dir: &Path,
        name: &str,
        manifest: &SkillManifest,
    ) -> Result<(), std::io::Error> {
        let skill_dir = skills_dir.join(name);
        fs::create_dir_all(&skill_dir)?;

        // Write manifest
        let manifest_toml = toml::to_string(manifest).expect("Should serialize");
        fs::write(skill_dir.join("manifest.toml"), manifest_toml)?;

        // Write minimal WASM
        let wasm = vec![
            0x00, 0x61, 0x73, 0x6d, // magic: \0asm
            0x01, 0x00, 0x00, 0x00, // version: 1
        ];
        fs::write(skill_dir.join(format!("{}.wasm", name)), wasm)?;

        Ok(())
    }

    fn create_test_manifest(name: &str, description: &str) -> SkillManifest {
        SkillManifest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: description.to_string(),
            author: "Test".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            entry_point: "run".to_string(),
        }
    }

    #[test]
    fn test_list_empty_registry() {
        let temp_dir = TempDir::new().expect("Should create temp dir");
        let skills_dir = temp_dir.path().to_path_buf();

        let loader = SkillLoader::new(vec![]);
        let registry = SkillRegistry { loader, skills_dir };

        let manifests = registry.list_manifests().expect("Should list");
        assert_eq!(manifests.len(), 0);
    }

    #[test]
    fn test_list_manifests() {
        let temp_dir = TempDir::new().expect("Should create temp dir");
        let skills_dir = temp_dir.path().to_path_buf();

        let manifest1 = create_test_manifest("skill1", "Test skill 1");
        let manifest2 = create_test_manifest("skill2", "Test skill 2");

        create_test_skill_dir(&skills_dir, "skill1", &manifest1).expect("Should create");
        create_test_skill_dir(&skills_dir, "skill2", &manifest2).expect("Should create");

        let loader = SkillLoader::new(vec![]);
        let registry = SkillRegistry { loader, skills_dir };

        let manifests = registry.list_manifests().expect("Should list");
        assert_eq!(manifests.len(), 2);

        let names: Vec<&str> = manifests.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"skill1"));
        assert!(names.contains(&"skill2"));
    }

    #[test]
    fn test_load_all() {
        let temp_dir = TempDir::new().expect("Should create temp dir");
        let skills_dir = temp_dir.path().to_path_buf();

        let manifest = create_test_manifest("test", "Test skill");
        create_test_skill_dir(&skills_dir, "test", &manifest).expect("Should create");

        let loader = SkillLoader::new(vec![]);
        let registry = SkillRegistry { loader, skills_dir };

        let skills = registry.load_all().expect("Should load");
        assert_eq!(skills.len(), 1);
        assert!(skills.contains_key("test"));
    }

    #[test]
    fn test_load_skill_by_name() {
        let temp_dir = TempDir::new().expect("Should create temp dir");
        let skills_dir = temp_dir.path().to_path_buf();

        let manifest = create_test_manifest("test", "Test skill");
        create_test_skill_dir(&skills_dir, "test", &manifest).expect("Should create");

        let loader = SkillLoader::new(vec![]);
        let registry = SkillRegistry { loader, skills_dir };

        let skill = registry.load_skill("test").expect("Should load");
        assert_eq!(skill.manifest().name, "test");
    }

    #[test]
    fn test_load_nonexistent_skill() {
        let temp_dir = TempDir::new().expect("Should create temp dir");
        let skills_dir = temp_dir.path().to_path_buf();

        let loader = SkillLoader::new(vec![]);
        let registry = SkillRegistry { loader, skills_dir };

        let result = registry.load_skill("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_skill_with_capabilities() {
        let temp_dir = TempDir::new().expect("Should create temp dir");
        let skills_dir = temp_dir.path().to_path_buf();

        let mut manifest = create_test_manifest("network-skill", "Network test");
        manifest.capabilities = vec![Capability::Network, Capability::Storage];

        create_test_skill_dir(&skills_dir, "network-skill", &manifest).expect("Should create");

        let loader = SkillLoader::new(vec![]);
        let registry = SkillRegistry { loader, skills_dir };

        let skill = registry.load_skill("network-skill").expect("Should load");
        assert_eq!(skill.capabilities().len(), 2);
    }
}
