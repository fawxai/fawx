//! WASM skill runtime with host API linking and execution.

use crate::host_api::{HostApi, MockHostApi};
use crate::loader::LoadedSkill;
use crate::manifest::SkillManifest;
use nv_core::error::SkillError;
use std::collections::HashMap;
use wasmtime::Engine;

/// WASM skill runtime manager.
pub struct SkillRuntime {
    engine: Engine,
    skills: HashMap<String, LoadedSkill>,
}

impl SkillRuntime {
    /// Create a new skill runtime.
    pub fn new() -> Result<Self, SkillError> {
        let engine = Engine::default();
        Ok(Self {
            engine,
            skills: HashMap::new(),
        })
    }

    /// Register a loaded skill.
    pub fn register_skill(&mut self, skill: LoadedSkill) -> Result<(), SkillError> {
        let name = skill.manifest().name.clone();

        if self.skills.contains_key(&name) {
            return Err(SkillError::Load(format!(
                "Skill '{}' is already registered",
                name
            )));
        }

        self.skills.insert(name, skill);
        Ok(())
    }

    /// Invoke a skill by name with input (synchronous).
    ///
    /// Note: This is a simplified implementation. Full WASM execution
    /// requires proper host function linking and calling conventions.
    pub fn invoke(&mut self, skill_name: &str, input: &str) -> Result<String, SkillError> {
        let skill = self
            .skills
            .get(skill_name)
            .ok_or_else(|| SkillError::Execution(format!("Skill '{}' not found", skill_name)))?;

        // Create host API state
        let mut host_api = MockHostApi::new(input);

        // For now, we use a simplified execution model
        // In a real implementation, this would:
        // 1. Create a Store with HostState
        // 2. Create a Linker and link host functions
        // 3. Instantiate the module
        // 4. Call the entry point function
        // 5. Return the output

        // Simplified: just return a placeholder showing the skill was called
        let output = format!(
            "Skill '{}' invoked (placeholder execution)",
            skill.manifest().name
        );
        host_api.set_output(&output);

        Ok(host_api.get_output())
    }

    /// Invoke a skill by name with input (asynchronous).
    ///
    /// This is an async wrapper around the synchronous `invoke()` method.
    /// WASM execution is moved to a blocking thread pool to avoid blocking
    /// the async runtime.
    ///
    /// # Arguments
    /// * `skill_name` - Name of the skill to invoke
    /// * `input` - Input string for the skill
    ///
    /// # Returns
    /// * `Ok(String)` - Output from the skill
    /// * `Err(SkillError)` - If skill not found or execution fails
    pub async fn invoke_skill_async(
        &mut self,
        skill_name: &str,
        input: &str,
    ) -> Result<String, SkillError> {
        // Clone the necessary data to move into the blocking task
        let skill_name = skill_name.to_string();
        let input = input.to_string();

        // We need to check if the skill exists first (before moving into blocking)
        if !self.skills.contains_key(&skill_name) {
            return Err(SkillError::Execution(format!(
                "Skill '{}' not found",
                skill_name
            )));
        }

        // Get the skill manifest name for output
        let manifest_name = self.skills.get(&skill_name).unwrap().manifest().name.clone();

        // Execute WASM in a blocking thread pool
        // In a real implementation, this would perform actual WASM execution
        tokio::task::spawn_blocking(move || {
            // Simulate WASM execution
            let mut host_api = MockHostApi::new(&input);
            let output = format!("Skill '{}' invoked (placeholder execution)", manifest_name);
            host_api.set_output(&output);
            Ok(host_api.get_output())
        })
        .await
        .map_err(|e| SkillError::Execution(format!("Async task panicked: {}", e)))?
    }

    /// List all registered skills.
    pub fn list_skills(&self) -> Vec<&SkillManifest> {
        self.skills.values().map(|s| s.manifest()).collect()
    }

    /// Remove a skill by name.
    ///
    /// Returns `true` if the skill was removed, `false` if it didn't exist.
    pub fn remove_skill(&mut self, name: &str) -> Result<bool, SkillError> {
        Ok(self.skills.remove(name).is_some())
    }

    /// Get a reference to a registered skill.
    pub fn get_skill(&self, name: &str) -> Option<&LoadedSkill> {
        self.skills.get(name)
    }

    /// Get the WASM engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}

impl Default for SkillRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create default SkillRuntime")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::SkillLoader;
    use crate::manifest::SkillManifest;

    fn create_test_manifest(name: &str) -> SkillManifest {
        SkillManifest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: "Test skill".to_string(),
            author: "Nova".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            entry_point: "run".to_string(),
        }
    }

    fn create_minimal_wasm() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6d, // magic: \0asm
            0x01, 0x00, 0x00, 0x00, // version: 1
        ]
    }

    #[test]
    fn test_register_and_list_skills() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::new(vec![]);

        let manifest1 = create_test_manifest("skill1");
        let manifest2 = create_test_manifest("skill2");
        let wasm = create_minimal_wasm();

        let skill1 = loader.load(&wasm, &manifest1, None).expect("Should load");
        let skill2 = loader.load(&wasm, &manifest2, None).expect("Should load");

        runtime.register_skill(skill1).expect("Should register");
        runtime.register_skill(skill2).expect("Should register");

        let skills = runtime.list_skills();
        assert_eq!(skills.len(), 2);

        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"skill1"));
        assert!(names.contains(&"skill2"));
    }

    #[test]
    fn test_remove_skill() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::new(vec![]);

        let manifest = create_test_manifest("test");
        let wasm = create_minimal_wasm();

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        assert_eq!(runtime.list_skills().len(), 1);

        let removed = runtime.remove_skill("test").expect("Should remove");
        assert!(removed);
        assert_eq!(runtime.list_skills().len(), 0);

        let not_removed = runtime.remove_skill("test").expect("Should handle missing");
        assert!(!not_removed);
    }

    #[test]
    fn test_invoke_nonexistent_skill() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");

        let result = runtime.invoke("nonexistent", "input");
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Execution(_))));
    }

    #[test]
    fn test_invoke_skill() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::new(vec![]);

        let manifest = create_test_manifest("test");
        let wasm = create_minimal_wasm();

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        let output = runtime.invoke("test", "test input").expect("Should invoke");
        assert!(output.contains("test"));
    }

    #[test]
    fn test_register_duplicate_skill() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::new(vec![]);

        let manifest = create_test_manifest("test");
        let wasm = create_minimal_wasm();

        let skill1 = loader.load(&wasm, &manifest, None).expect("Should load");
        let skill2 = loader.load(&wasm, &manifest, None).expect("Should load");

        runtime.register_skill(skill1).expect("Should register");

        let result = runtime.register_skill(skill2);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Load(_))));
    }

    #[test]
    fn test_get_skill() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::new(vec![]);

        let manifest = create_test_manifest("test");
        let wasm = create_minimal_wasm();

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        assert!(runtime.get_skill("test").is_some());
        assert!(runtime.get_skill("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_invoke_skill_async() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::new(vec![]);

        let manifest = create_test_manifest("async_test");
        let wasm = create_minimal_wasm();

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        let output = runtime
            .invoke_skill_async("async_test", "async input")
            .await
            .expect("Should invoke async");

        assert!(output.contains("async_test"));
    }

    #[tokio::test]
    async fn test_invoke_skill_async_nonexistent() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");

        let result = runtime.invoke_skill_async("nonexistent", "input").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Execution(_))));
    }

    #[tokio::test]
    async fn test_invoke_skill_async_multiple_concurrent() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::new(vec![]);

        let manifest1 = create_test_manifest("skill1");
        let manifest2 = create_test_manifest("skill2");
        let wasm = create_minimal_wasm();

        let skill1 = loader.load(&wasm, &manifest1, None).expect("Should load");
        let skill2 = loader.load(&wasm, &manifest2, None).expect("Should load");

        runtime.register_skill(skill1).expect("Should register");
        runtime.register_skill(skill2).expect("Should register");

        // Note: Since we have &mut self, we can't actually run these concurrently
        // This test verifies that async invocation works for multiple skills sequentially
        let output1 = runtime
            .invoke_skill_async("skill1", "input1")
            .await
            .expect("Should invoke skill1");

        let output2 = runtime
            .invoke_skill_async("skill2", "input2")
            .await
            .expect("Should invoke skill2");

        assert!(output1.contains("skill1"));
        assert!(output2.contains("skill2"));
    }

    #[tokio::test]
    async fn test_invoke_sync_and_async_compatibility() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::new(vec![]);

        let manifest = create_test_manifest("compat_test");
        let wasm = create_minimal_wasm();

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        // Test sync path
        let sync_output = runtime
            .invoke("compat_test", "sync input")
            .expect("Should invoke sync");

        // Test async path
        let async_output = runtime
            .invoke_skill_async("compat_test", "async input")
            .await
            .expect("Should invoke async");

        // Both should work
        assert!(sync_output.contains("compat_test"));
        assert!(async_output.contains("compat_test"));
    }
}
