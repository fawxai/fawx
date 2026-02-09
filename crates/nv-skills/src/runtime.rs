//! WASM skill runtime with host API linking and execution.

use crate::host_api::{HostApi, MockHostApi};
use crate::loader::LoadedSkill;
use crate::manifest::{Capability, SkillManifest};
use nv_core::error::SkillError;
use std::collections::HashMap;
use wasmtime::{Caller, Engine, Linker, Memory, Store};

/// Starting offset for string allocations in WASM linear memory.
const WASM_STRING_BUFFER_START: u32 = 1024;

/// Host state that holds the HostApi implementation and linear memory.
struct HostState {
    api: Box<dyn HostApi>,
    memory: Option<Memory>,
    /// Capabilities granted to the current skill.
    capabilities: Vec<Capability>,
    /// Next free offset for string allocation in WASM memory.
    alloc_offset: u32,
}

impl HostState {
    /// Create a new host state with the given API implementation and capabilities.
    fn new(api: Box<dyn HostApi>, capabilities: Vec<Capability>) -> Self {
        Self {
            api,
            memory: None,
            capabilities,
            alloc_offset: WASM_STRING_BUFFER_START,
        }
    }

    /// Set the linear memory reference.
    fn set_memory(&mut self, memory: Memory) {
        self.memory = Some(memory);
    }

    /// Check if the skill has a required capability.
    fn has_capability(&self, cap: &Capability) -> bool {
        self.capabilities.contains(cap)
    }

    /// Reset allocation offset (e.g., between host function calls).
    #[allow(dead_code)]
    fn reset_alloc(&mut self) {
        self.alloc_offset = WASM_STRING_BUFFER_START;
    }

    /// Read a string from WASM memory.
    fn read_string(
        &self,
        store: &impl wasmtime::AsContext,
        ptr: u32,
        len: u32,
    ) -> Result<String, SkillError> {
        let memory = self
            .memory
            .as_ref()
            .ok_or_else(|| SkillError::Execution("Memory not initialized".to_string()))?;

        let end = ptr.checked_add(len).ok_or_else(|| {
            SkillError::Execution("Integer overflow in memory access".to_string())
        })?;

        let data = memory
            .data(store)
            .get(ptr as usize..end as usize)
            .ok_or_else(|| SkillError::Execution("Invalid memory access".to_string()))?;

        String::from_utf8(data.to_vec())
            .map_err(|e| SkillError::Execution(format!("Invalid UTF-8: {}", e)))
    }

    /// Write a string to WASM memory using bump allocation.
    /// Returns the pointer to the written string.
    /// Appends a null terminator after the string for guest-side reading.
    ///
    /// Note: This is a standalone method that takes memory + offset explicitly
    /// to avoid double-mutable-borrow issues with wasmtime's `Caller`.
    fn write_to_memory(
        memory: Memory,
        store: &mut impl wasmtime::AsContextMut,
        s: &str,
        alloc_offset: &mut u32,
    ) -> Result<u32, SkillError> {
        let ptr = *alloc_offset;
        let bytes = s.as_bytes();
        // +1 for null terminator
        let total_len = bytes.len() + 1;

        let mem_data = memory.data_mut(store);
        if ptr as usize + total_len > mem_data.len() {
            return Err(SkillError::Execution(format!(
                "String too large for WASM memory: {} bytes (offset {})",
                bytes.len(),
                ptr
            )));
        }
        let dest = &mut mem_data[ptr as usize..ptr as usize + bytes.len()];
        dest.copy_from_slice(bytes);
        // Null terminator
        mem_data[ptr as usize + bytes.len()] = 0;

        // Bump the allocation offset (align to 8 bytes)
        *alloc_offset = ptr + ((total_len as u32 + 7) & !7);

        Ok(ptr)
    }
}

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

    /// Link host API functions to the WASM linker.
    fn link_host_functions(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        // host_api_v1::log(level: u32, msg_ptr: u32, msg_len: u32)
        linker
            .func_wrap(
                "host_api_v1",
                "log",
                |mut caller: Caller<'_, HostState>, level: u32, msg_ptr: u32, msg_len: u32| {
                    match caller.data().read_string(&caller, msg_ptr, msg_len) {
                        Ok(message) => {
                            caller.data_mut().api.log(level, &message);
                        }
                        Err(e) => {
                            tracing::error!("Failed to read log message: {}", e);
                        }
                    }
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link log: {}", e)))?;

        // host_api_v1::kv_get(key_ptr: u32, key_len: u32) -> u32 (0 = not found, ptr otherwise)
        linker
            .func_wrap(
                "host_api_v1",
                "kv_get",
                |mut caller: Caller<'_, HostState>, key_ptr: u32, key_len: u32| -> u32 {
                    // Check capability
                    if !caller.data().has_capability(&Capability::Storage) {
                        tracing::warn!("kv_get denied: skill lacks Storage capability");
                        return 0;
                    }

                    // Read key
                    let key_result = caller.data().read_string(&caller, key_ptr, key_len);
                    let key = match key_result {
                        Ok(k) => k,
                        Err(e) => {
                            tracing::error!("Failed to read kv_get key: {}", e);
                            return 0;
                        }
                    };

                    // Get value
                    let value_opt = caller.data().api.kv_get(&key);
                    let value = match value_opt {
                        Some(v) => v,
                        None => return 0,
                    };

                    // Write value — get memory and alloc_offset before mutable borrow
                    let memory = match caller.data().memory {
                        Some(m) => m,
                        None => {
                            tracing::error!("Memory not initialized for kv_get");
                            return 0;
                        }
                    };
                    let mut alloc_offset = caller.data().alloc_offset;
                    match HostState::write_to_memory(memory, &mut caller, &value, &mut alloc_offset)
                    {
                        Ok(ptr) => {
                            caller.data_mut().alloc_offset = alloc_offset;
                            ptr
                        }
                        Err(e) => {
                            tracing::error!("Failed to write kv_get value: {}", e);
                            0
                        }
                    }
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link kv_get: {}", e)))?;

        // host_api_v1::kv_set(key_ptr: u32, key_len: u32, val_ptr: u32, val_len: u32)
        linker
            .func_wrap(
                "host_api_v1",
                "kv_set",
                |mut caller: Caller<'_, HostState>,
                 key_ptr: u32,
                 key_len: u32,
                 val_ptr: u32,
                 val_len: u32| {
                    // Check capability
                    if !caller.data().has_capability(&Capability::Storage) {
                        tracing::warn!("kv_set denied: skill lacks Storage capability");
                        return;
                    }

                    let key_result = caller.data().read_string(&caller, key_ptr, key_len);
                    let val_result = caller.data().read_string(&caller, val_ptr, val_len);

                    match (key_result, val_result) {
                        (Ok(key), Ok(value)) => {
                            if let Err(e) = caller.data_mut().api.kv_set(&key, &value) {
                                tracing::error!("kv_set failed: {}", e);
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            tracing::error!("Failed to read kv_set parameters: {}", e);
                        }
                    }
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link kv_set: {}", e)))?;

        // host_api_v1::get_input() -> u32 (ptr to input string in memory)
        linker
            .func_wrap(
                "host_api_v1",
                "get_input",
                |mut caller: Caller<'_, HostState>| -> u32 {
                    // Get input
                    let input = caller.data().api.get_input();

                    // Write to memory — get memory and alloc_offset before mutable borrow
                    let memory = match caller.data().memory {
                        Some(m) => m,
                        None => {
                            tracing::error!("Memory not initialized for get_input");
                            return 0;
                        }
                    };
                    let mut alloc_offset = caller.data().alloc_offset;
                    match HostState::write_to_memory(memory, &mut caller, &input, &mut alloc_offset)
                    {
                        Ok(ptr) => {
                            caller.data_mut().alloc_offset = alloc_offset;
                            ptr
                        }
                        Err(e) => {
                            tracing::error!("Failed to write input: {}", e);
                            0
                        }
                    }
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link get_input: {}", e)))?;

        // host_api_v1::set_output(text_ptr: u32, text_len: u32)
        linker
            .func_wrap(
                "host_api_v1",
                "set_output",
                |mut caller: Caller<'_, HostState>, text_ptr: u32, text_len: u32| match caller
                    .data()
                    .read_string(&caller, text_ptr, text_len)
                {
                    Ok(text) => {
                        caller.data_mut().api.set_output(&text);
                    }
                    Err(e) => {
                        tracing::error!("Failed to read output text: {}", e);
                    }
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link set_output: {}", e)))?;

        Ok(())
    }

    /// Invoke a skill by name with input.
    pub fn invoke(&mut self, skill_name: &str, input: &str) -> Result<String, SkillError> {
        let skill = self
            .skills
            .get(skill_name)
            .ok_or_else(|| SkillError::Execution(format!("Skill '{}' not found", skill_name)))?;

        // Create host API state with skill's declared capabilities
        let capabilities = skill.capabilities().to_vec();
        let host_api = Box::new(MockHostApi::new(input));
        let mut store = Store::new(&self.engine, HostState::new(host_api, capabilities));

        // Create linker and link host functions
        let mut linker = Linker::new(&self.engine);
        Self::link_host_functions(&mut linker)?;

        // Instantiate the module
        let instance = linker
            .instantiate(&mut store, skill.module())
            .map_err(|e| SkillError::Execution(format!("Failed to instantiate module: {}", e)))?;

        // Get the memory export and store it in the host state
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| SkillError::Execution("Module does not export 'memory'".to_string()))?;

        store.data_mut().set_memory(memory);

        // Get the entry point function
        let entry_point = skill.manifest().entry_point.as_str();
        let run_func = instance
            .get_typed_func::<(), ()>(&mut store, entry_point)
            .map_err(|e| {
                SkillError::Execution(format!(
                    "Failed to get entry point '{}': {}",
                    entry_point, e
                ))
            })?;

        // Call the entry point
        run_func
            .call(&mut store, ())
            .map_err(|e| SkillError::Execution(format!("Skill execution failed: {}", e)))?;

        // Extract the output
        let api_ref = &store.data().api;
        // We need to downcast to get the output
        // This is safe because we created a MockHostApi above
        let mock_api = api_ref
            .as_any()
            .downcast_ref::<MockHostApi>()
            .ok_or_else(|| SkillError::Execution("Failed to access host API".to_string()))?;

        Ok(mock_api.get_output())
    }

    /// Invoke a skill asynchronously, enabling concurrent skill invocations.
    ///
    /// Runs the WASM execution in a blocking thread pool via `tokio::task::spawn_blocking`,
    /// allowing multiple skills to execute concurrently.
    ///
    /// # Note
    /// This is currently a placeholder implementation that returns a mock result.
    /// Full WASM execution requires cloning the compiled `Module` into the blocking
    /// task, which will be implemented when concurrent skill execution is needed
    /// (tracked in issue backlog).
    ///
    /// # Arguments
    /// * `skill_name` - Name of the skill to invoke
    /// * `input` - JSON input string for the skill
    pub async fn invoke_skill_async(
        &self,
        skill_name: &str,
        input: &str,
    ) -> Result<String, SkillError> {
        let skill_name = skill_name.to_string();
        let input = input.to_string();

        // Check if skill exists and clone manifest name
        let skill = self
            .skills
            .get(&skill_name)
            .ok_or_else(|| SkillError::Execution(format!("Skill '{}' not found", skill_name)))?;

        let manifest_name = skill.manifest().name.clone();

        // TODO(#165): Clone compiled Module into spawn_blocking for real WASM execution.
        // Current implementation returns placeholder output for API compatibility.
        tokio::task::spawn_blocking(move || {
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
        Self::new().unwrap_or_else(|e| {
            // SkillRuntime::new() only fails if Engine::default() fails,
            // which should not happen under normal circumstances.
            panic!("Failed to create default SkillRuntime: {}", e)
        })
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

    /// Create a WASM module that can actually be invoked (has memory + run export).
    fn create_invocable_wasm(_engine: &Engine) -> Vec<u8> {
        // WAT module with memory export and a no-op `run` function
        // Also imports host_api_v1 functions that the linker provides
        let wat = r#"
            (module
                (import "host_api_v1" "log" (func $log (param i32 i32 i32)))
                (import "host_api_v1" "kv_get" (func $kv_get (param i32 i32) (result i32)))
                (import "host_api_v1" "kv_set" (func $kv_set (param i32 i32 i32 i32)))
                (import "host_api_v1" "get_input" (func $get_input (result i32)))
                (import "host_api_v1" "set_output" (func $set_output (param i32 i32)))
                (memory (export "memory") 1)
                (func (export "run")
                    ;; Simple skill: just set output to a fixed string at offset 0
                    ;; Write "ok" to memory at offset 0
                    (i32.store8 (i32.const 0) (i32.const 111)) ;; 'o'
                    (i32.store8 (i32.const 1) (i32.const 107)) ;; 'k'
                    ;; Call set_output(ptr=0, len=2)
                    (call $set_output (i32.const 0) (i32.const 2))
                )
            )
        "#;
        // Module::new accepts WAT text directly, so return as bytes
        wat.as_bytes().to_vec()
    }

    #[test]
    fn test_register_and_list_skills() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

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
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

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
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        let manifest = create_test_manifest("test");
        let wasm = create_invocable_wasm(runtime.engine());

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        let output = runtime.invoke("test", "test input").expect("Should invoke");
        // The invocable WASM sets output to "ok"
        assert_eq!(output, "ok");
    }

    #[test]
    fn test_register_duplicate_skill() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

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
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

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
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

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
        let runtime = SkillRuntime::new().expect("Should create runtime");

        let result = runtime.invoke_skill_async("nonexistent", "input").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Execution(_))));
    }

    #[tokio::test]
    async fn test_invoke_skill_async_multiple_concurrent() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        let manifest1 = create_test_manifest("skill1");
        let manifest2 = create_test_manifest("skill2");
        let wasm = create_minimal_wasm();

        let skill1 = loader.load(&wasm, &manifest1, None).expect("Should load");
        let skill2 = loader.load(&wasm, &manifest2, None).expect("Should load");

        runtime.register_skill(skill1).expect("Should register");
        runtime.register_skill(skill2).expect("Should register");

        let (result1, result2) = tokio::join!(
            runtime.invoke_skill_async("skill1", "input1"),
            runtime.invoke_skill_async("skill2", "input2")
        );

        let output1 = result1.expect("Should invoke skill1");
        let output2 = result2.expect("Should invoke skill2");

        assert!(output1.contains("skill1"));
        assert!(output2.contains("skill2"));
    }
}
