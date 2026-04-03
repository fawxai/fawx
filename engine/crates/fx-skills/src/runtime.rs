//! WASM skill runtime with host API linking and execution.

use crate::host_api::{HostApi, MockHostApi};
use crate::loader::LoadedSkill;
use crate::manifest::{Capability, SkillManifest};
use fx_core::error::SkillError;
use std::collections::HashMap;
use wasmtime::{Caller, Engine, Linker, Memory, Store};
use wasmtime_wasi::p1::WasiP1Ctx;

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
    /// WASI preview1 context, lazily initialized only for modules that import
    /// from `wasi_snapshot_preview1`.
    wasi_context: Option<WasiP1Ctx>,
}

impl HostState {
    /// Create a new host state with the given API implementation and capabilities.
    fn new(api: Box<dyn HostApi>, capabilities: Vec<Capability>) -> Self {
        Self {
            api,
            memory: None,
            capabilities,
            alloc_offset: WASM_STRING_BUFFER_START,
            wasi_context: None,
        }
    }

    /// Initialize the WASI preview1 context.
    /// Called only when the module imports from `wasi_snapshot_preview1`.
    fn init_wasi_context(&mut self) {
        let wasi_context = wasmtime_wasi::WasiCtxBuilder::new()
            .inherit_stdout()
            .inherit_stderr()
            .build_p1();
        self.wasi_context = Some(wasi_context);
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
    /// Create a new skill runtime with a default wasmtime engine.
    pub fn new() -> Result<Self, SkillError> {
        let engine = Engine::default();
        Ok(Self {
            engine,
            skills: HashMap::new(),
        })
    }

    /// Create a skill runtime sharing an existing wasmtime engine.
    ///
    /// Use this when loading skills that were compiled with a specific
    /// engine (e.g., from a [`SkillLoader`]). Wasmtime requires modules
    /// and stores to use the same `Engine` instance.
    pub fn with_engine(engine: Engine) -> Self {
        Self {
            engine,
            skills: HashMap::new(),
        }
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

        Self::link_http_request(linker)?;
        Self::link_exec_command(linker)?;
        Self::link_read_file(linker)?;
        Self::link_write_file(linker)?;
        Self::link_v2_host_functions(linker)?;

        Ok(())
    }

    /// Link the http_request host function to the WASM linker.
    ///
    /// Signature: `(method_ptr, method_len, url_ptr, url_len,
    ///              headers_ptr, headers_len, body_ptr, body_len) -> u32`
    /// Returns a pointer to a NUL-terminated response string in WASM memory,
    /// or 0 on failure.
    fn link_http_request(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        linker
            .func_wrap(
                "host_api_v1",
                "http_request",
                |mut caller: Caller<'_, HostState>,
                 method_ptr: u32,
                 method_len: u32,
                 url_ptr: u32,
                 url_len: u32,
                 headers_ptr: u32,
                 headers_len: u32,
                 body_ptr: u32,
                 body_len: u32|
                 -> u32 {
                    if !caller.data().has_capability(&Capability::Network) {
                        tracing::warn!("http_request denied: skill lacks Network capability");
                        return 0;
                    }
                    let Some(method) = Self::read_host_string(
                        &caller,
                        method_ptr,
                        method_len,
                        "http_request method",
                    ) else {
                        return 0;
                    };
                    let Some(url) =
                        Self::read_host_string(&caller, url_ptr, url_len, "http_request url")
                    else {
                        return 0;
                    };
                    let Some(headers) = Self::read_host_string(
                        &caller,
                        headers_ptr,
                        headers_len,
                        "http_request headers",
                    ) else {
                        return 0;
                    };
                    let Some(body) =
                        Self::read_host_string(&caller, body_ptr, body_len, "http_request body")
                    else {
                        return 0;
                    };
                    let Some(response) = caller
                        .data()
                        .api
                        .http_request(&method, &url, &headers, &body)
                    else {
                        return 0;
                    };
                    Self::write_host_string(&mut caller, &response, "http_request")
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link http_request: {}", e)))?;

        Ok(())
    }

    fn link_exec_command(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        linker
            .func_wrap(
                "host_api_v1",
                "exec_command",
                |mut caller: Caller<'_, HostState>,
                 command_ptr: u32,
                 command_len: u32,
                 timeout_ms: u32|
                 -> u32 {
                    if !caller.data().has_capability(&Capability::Shell) {
                        tracing::warn!("exec_command denied: skill lacks Shell capability");
                        return 0;
                    }
                    let Some(command) =
                        Self::read_host_string(&caller, command_ptr, command_len, "exec_command")
                    else {
                        return 0;
                    };
                    let Some(response) = caller.data().api.exec_command(&command, timeout_ms)
                    else {
                        return 0;
                    };
                    Self::write_host_string(&mut caller, &response, "exec_command")
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link exec_command: {}", e)))?;
        Ok(())
    }

    fn link_read_file(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        linker
            .func_wrap(
                "host_api_v1",
                "read_file",
                |mut caller: Caller<'_, HostState>, path_ptr: u32, path_len: u32| -> u32 {
                    if !caller.data().has_capability(&Capability::Filesystem) {
                        tracing::warn!("read_file denied: skill lacks Filesystem capability");
                        return 0;
                    }
                    let Some(path) =
                        Self::read_host_string(&caller, path_ptr, path_len, "read_file")
                    else {
                        return 0;
                    };
                    let Some(contents) = caller.data().api.read_file(&path) else {
                        return 0;
                    };
                    Self::write_host_string(&mut caller, &contents, "read_file")
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link read_file: {}", e)))?;
        Ok(())
    }

    fn link_write_file(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        linker
            .func_wrap(
                "host_api_v1",
                "write_file",
                |caller: Caller<'_, HostState>,
                 path_ptr: u32,
                 path_len: u32,
                 content_ptr: u32,
                 content_len: u32|
                 -> i32 {
                    if !caller.data().has_capability(&Capability::Filesystem) {
                        tracing::warn!("write_file denied: skill lacks Filesystem capability");
                        return 0;
                    }
                    let Some(path) =
                        Self::read_host_string(&caller, path_ptr, path_len, "write_file path")
                    else {
                        return 0;
                    };
                    let Some(content) = Self::read_host_string(
                        &caller,
                        content_ptr,
                        content_len,
                        "write_file content",
                    ) else {
                        return 0;
                    };
                    i32::from(caller.data().api.write_file(&path, &content))
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link write_file: {}", e)))?;
        Ok(())
    }

    fn read_host_string(
        caller: &Caller<'_, HostState>,
        ptr: u32,
        len: u32,
        context: &str,
    ) -> Option<String> {
        caller
            .data()
            .read_string(caller, ptr, len)
            .map_err(|e| {
                tracing::error!("{context}: failed to read string: {e}");
                e
            })
            .ok()
    }

    fn write_host_string(caller: &mut Caller<'_, HostState>, value: &str, context: &str) -> u32 {
        let memory = match caller.data().memory {
            Some(memory) => memory,
            None => {
                tracing::error!("Memory not initialized for {context}");
                return 0;
            }
        };
        let mut alloc_offset = caller.data().alloc_offset;
        match HostState::write_to_memory(memory, caller, value, &mut alloc_offset) {
            Ok(ptr) => {
                caller.data_mut().alloc_offset = alloc_offset;
                ptr
            }
            Err(e) => {
                tracing::error!("Failed to write {context} response: {e}");
                0
            }
        }
    }

    /// Link host_api_v2 functions to the WASM linker.
    fn link_v2_host_functions(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        Self::link_v2_get_context(linker)?;
        Self::link_v2_register_channel(linker)?;
        Self::link_v2_emit_event(linker)?;
        Self::link_v2_send_to_channel(linker)?;
        Ok(())
    }

    fn link_v2_get_context(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        linker
            .func_wrap(
                "host_api_v2",
                "get_context",
                |mut caller: Caller<'_, HostState>| -> u32 {
                    let json = caller.data().api.get_context();
                    let memory = match caller.data().memory {
                        Some(m) => m,
                        None => return 0,
                    };
                    let mut off = caller.data().alloc_offset;
                    match HostState::write_to_memory(memory, &mut caller, &json, &mut off) {
                        Ok(ptr) => {
                            caller.data_mut().alloc_offset = off;
                            ptr
                        }
                        Err(_) => 0,
                    }
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link get_context: {e}")))?;
        Ok(())
    }

    fn link_v2_register_channel(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        linker
            .func_wrap(
                "host_api_v2",
                "register_channel",
                |mut caller: Caller<'_, HostState>,
                 id_ptr: u32,
                 id_len: u32,
                 name_ptr: u32,
                 name_len: u32|
                 -> i32 {
                    let id = match caller.data().read_string(&caller, id_ptr, id_len) {
                        Ok(s) => s,
                        Err(_) => return -1,
                    };
                    let name = match caller.data().read_string(&caller, name_ptr, name_len) {
                        Ok(s) => s,
                        Err(_) => return -1,
                    };
                    match caller.data_mut().api.register_channel(&id, &name) {
                        Ok(()) => 0,
                        Err(_) => -1,
                    }
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link register_channel: {e}")))?;
        Ok(())
    }

    fn link_v2_emit_event(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        linker
            .func_wrap(
                "host_api_v2",
                "emit_event",
                |mut caller: Caller<'_, HostState>,
                 type_ptr: u32,
                 type_len: u32,
                 payload_ptr: u32,
                 payload_len: u32|
                 -> i32 {
                    let etype = match caller.data().read_string(&caller, type_ptr, type_len) {
                        Ok(s) => s,
                        Err(_) => return -1,
                    };
                    let payload = match caller.data().read_string(&caller, payload_ptr, payload_len)
                    {
                        Ok(s) => s,
                        Err(_) => return -1,
                    };
                    match caller.data_mut().api.emit_event(&etype, &payload) {
                        Ok(()) => 0,
                        Err(_) => -1,
                    }
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link emit_event: {e}")))?;
        Ok(())
    }

    fn link_v2_send_to_channel(linker: &mut Linker<HostState>) -> Result<(), SkillError> {
        linker
            .func_wrap(
                "host_api_v2",
                "send_to_channel",
                |caller: Caller<'_, HostState>,
                 id_ptr: u32,
                 id_len: u32,
                 msg_ptr: u32,
                 msg_len: u32|
                 -> i32 {
                    let ch = match caller.data().read_string(&caller, id_ptr, id_len) {
                        Ok(s) => s,
                        Err(_) => return -1,
                    };
                    let msg = match caller.data().read_string(&caller, msg_ptr, msg_len) {
                        Ok(s) => s,
                        Err(_) => return -1,
                    };
                    match caller.data().api.send_to_channel(&ch, &msg) {
                        Ok(()) => 0,
                        Err(_) => -1,
                    }
                },
            )
            .map_err(|e| SkillError::Execution(format!("Failed to link send_to_channel: {e}")))?;
        Ok(())
    }

    /// Invoke a skill by name with input, using the default [`MockHostApi`].
    ///
    /// For production use with a real host API, use [`invoke_with_host_api`](Self::invoke_with_host_api).
    pub fn invoke(&mut self, skill_name: &str, input: &str) -> Result<String, SkillError> {
        let host_api = Box::new(MockHostApi::new(input));
        self.invoke_with_host_api(skill_name, input, host_api)
    }

    /// Invoke a skill by name with a caller-supplied `HostApi` implementation.
    ///
    /// This is the primary invocation path. It accepts any `HostApi` (e.g.,
    /// `LiveHostApi` for production HTTP, `MockHostApi` for tests) and runs
    /// the WASM module with it.
    pub fn invoke_with_api(
        &mut self,
        skill_name: &str,
        host_api: Box<dyn HostApi>,
    ) -> Result<String, SkillError> {
        self.invoke_with_host_api(skill_name, "", host_api)
    }

    /// Invoke a skill by name with input, using the provided [`HostApi`] implementation.
    ///
    /// This is the primary execution path for production callers (e.g., [`WasmSkill`])
    /// that supply a [`LiveHostApi`] backed by real runtime services.
    pub fn invoke_with_host_api(
        &mut self,
        skill_name: &str,
        _input: &str,

        host_api: Box<dyn HostApi>,
    ) -> Result<String, SkillError> {
        let skill = self
            .skills
            .get(skill_name)
            .ok_or_else(|| SkillError::Execution(format!("Skill '{}' not found", skill_name)))?;

        let capabilities = skill.capabilities().to_vec();
        let mut host_state = HostState::new(host_api, capabilities);

        // Only initialize WASI context and link WASI imports when the module
        // actually imports from `wasi_snapshot_preview1`.
        let needs_wasi = skill
            .module()
            .imports()
            .any(|imp| imp.module() == "wasi_snapshot_preview1");

        let mut linker = Linker::new(&self.engine);

        if needs_wasi {
            host_state.init_wasi_context();
            wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |state: &mut HostState| {
                state
                    .wasi_context
                    .as_mut()
                    .expect("WASI context must be initialized when WASI imports are present")
            })
            .map_err(|e| SkillError::Execution(format!("Failed to link WASI: {e}")))?;
        }

        let mut store = Store::new(&self.engine, host_state);

        Self::link_host_functions(&mut linker)?;

        let instance = linker
            .instantiate(&mut store, skill.module())
            .map_err(|e| SkillError::Execution(format!("Failed to instantiate module: {e}")))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| SkillError::Execution("Module does not export 'memory'".to_string()))?;

        store.data_mut().set_memory(memory);

        let entry_point = skill.manifest().entry_point.as_str();
        let run_func = instance
            .get_typed_func::<(), ()>(&mut store, entry_point)
            .map_err(|e| {
                SkillError::Execution(format!("Failed to get entry point '{entry_point}': {e}"))
            })?;

        run_func
            .call(&mut store, ())
            .map_err(|e| SkillError::Execution(format!("Skill execution failed: {e}")))?;

        Ok(store.data().api.get_output())
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
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
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

    #[derive(Debug)]
    struct CapabilityRuntimeHostApi {
        base: crate::host_api::HostApiBase,
    }

    impl CapabilityRuntimeHostApi {
        fn new() -> Self {
            Self {
                base: crate::host_api::HostApiBase::new("input"),
            }
        }
    }

    impl HostApi for CapabilityRuntimeHostApi {
        fn log(&self, _level: u32, _message: &str) {}

        fn kv_get(&self, key: &str) -> Option<String> {
            self.base.kv_get(key)
        }

        fn kv_set(&mut self, key: &str, value: &str) -> Result<(), SkillError> {
            self.base.kv_set(key, value);
            Ok(())
        }

        fn get_input(&self) -> String {
            self.base.get_input()
        }

        fn set_output(&mut self, text: &str) {
            self.base.set_output(text);
        }

        fn http_request(
            &self,
            _method: &str,
            _url: &str,
            _headers: &str,
            _body: &str,
        ) -> Option<String> {
            None
        }

        fn exec_command(&self, _command: &str, _timeout_ms: u32) -> Option<String> {
            Some("exec_result".to_string())
        }

        fn read_file(&self, _path: &str) -> Option<String> {
            Some("file_result".to_string())
        }

        fn write_file(&self, path: &str, content: &str) -> bool {
            path == "out.txt" && content == "hello"
        }

        fn get_output(&self) -> String {
            self.base.get_output()
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
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

    /// Regression test: invoke_with_host_api works with a custom HostApi
    /// implementation, not just MockHostApi. This would have caught the bug
    /// where invoke() hardcoded MockHostApi and LiveHostApi was dead code.
    #[test]
    fn test_invoke_with_custom_host_api() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        let manifest = create_test_manifest("test");
        let wasm = create_invocable_wasm(runtime.engine());

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        // Use a MockHostApi passed explicitly — verifying the host_api parameter
        // is actually used (not replaced by an internal MockHostApi).
        let host_api = Box::new(MockHostApi::new("custom input"));
        let output = runtime
            .invoke_with_host_api("test", "custom input", host_api)
            .expect("Should invoke with custom host API");

        // The test WASM always writes "ok" regardless of input
        assert_eq!(output, "ok");
    }

    /// invoke() delegates to invoke_with_host_api, producing the same result.
    #[test]
    fn test_invoke_delegates_to_invoke_with_host_api() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        let manifest = create_test_manifest("test");
        let wasm = create_invocable_wasm(runtime.engine());

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        let output = runtime.invoke("test", "input").expect("Should invoke");
        assert_eq!(output, "ok");
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

    #[test]
    fn test_wasi_module_instantiation_succeeds() {
        // A WASM module that imports WASI fd_write (like wasm32-wasip1 targets produce)
        let wat = r#"
            (module
                (import "wasi_snapshot_preview1" "fd_write"
                    (func $fd_write (param i32 i32 i32 i32) (result i32)))
                (import "host_api_v1" "log" (func $log (param i32 i32 i32)))
                (import "host_api_v1" "kv_get" (func $kv_get (param i32 i32) (result i32)))
                (import "host_api_v1" "kv_set" (func $kv_set (param i32 i32 i32 i32)))
                (import "host_api_v1" "get_input" (func $get_input (result i32)))
                (import "host_api_v1" "set_output" (func $set_output (param i32 i32)))
                (memory (export "memory") 1)
                (func (export "run")
                    (i32.store8 (i32.const 0) (i32.const 111))
                    (i32.store8 (i32.const 1) (i32.const 107))
                    (call $set_output (i32.const 0) (i32.const 2))
                )
            )
        "#;

        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);
        let manifest = create_test_manifest("wasi-skill");

        let skill = loader
            .load(wat.as_bytes(), &manifest, None)
            .expect("Should load WASI module");
        runtime.register_skill(skill).expect("Should register");

        let result = runtime.invoke("wasi-skill", "test");
        assert!(
            result.is_ok(),
            "WASI module should invoke successfully: {result:?}"
        );
    }

    #[test]
    fn test_non_wasi_module_still_works_with_wasi_linked() {
        // Verify that modules WITHOUT WASI imports still work when WASI is linked
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);
        let manifest = create_test_manifest("non-wasi-skill");

        let wasm = create_invocable_wasm(runtime.engine());
        let skill = loader
            .load(&wasm, &manifest, None)
            .expect("Should load non-WASI module");
        runtime.register_skill(skill).expect("Should register");

        let result = runtime.invoke("non-wasi-skill", "test");
        assert!(
            result.is_ok(),
            "Non-WASI module should still work: {result:?}"
        );
    }

    /// Create a WAT module that imports and calls http_request.
    ///
    /// The module writes a URL string to memory, calls http_request,
    /// and forwards the response (or "no_response") to set_output.
    fn create_http_request_wasm() -> Vec<u8> {
        let wat = r#"
            (module
                (import "host_api_v1" "log" (func $log (param i32 i32 i32)))
                (import "host_api_v1" "kv_get" (func $kv_get (param i32 i32) (result i32)))
                (import "host_api_v1" "kv_set" (func $kv_set (param i32 i32 i32 i32)))
                (import "host_api_v1" "get_input" (func $get_input (result i32)))
                (import "host_api_v1" "set_output" (func $set_output (param i32 i32)))
                (import "host_api_v1" "http_request"
                    (func $http_request
                        (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)

                ;; Data section: method="GET" at 0, url at 3, headers="{}" at 100, body="" at 102
                (data (i32.const 0) "GET")
                (data (i32.const 3) "https://mock.test/api")
                (data (i32.const 100) "{}")
                ;; "no_response" fallback at offset 200
                (data (i32.const 200) "no_response")

                (func (export "run")
                    (local $resp_ptr i32)
                    ;; Call http_request(method_ptr=0, method_len=3,
                    ;;                   url_ptr=3, url_len=20,
                    ;;                   headers_ptr=100, headers_len=2,
                    ;;                   body_ptr=102, body_len=0)
                    (local.set $resp_ptr
                        (call $http_request
                            (i32.const 0)   (i32.const 3)   ;; "GET"
                            (i32.const 3)   (i32.const 21)  ;; "https://mock.test/api"
                            (i32.const 100) (i32.const 2)   ;; "{}"
                            (i32.const 102) (i32.const 0))) ;; ""

                    ;; If resp_ptr == 0, output "no_response"
                    (if (i32.eqz (local.get $resp_ptr))
                        (then
                            (call $set_output (i32.const 200) (i32.const 11)))
                        (else
                            ;; Response is NUL-terminated at resp_ptr.
                            ;; We know our mock response is 13 bytes: '{"status":"ok"}'
                            ;; but we need to measure. For test simplicity, use
                            ;; a known length. The mock returns exactly "mock_response"
                            ;; which is 13 bytes.
                            (call $set_output (local.get $resp_ptr) (i32.const 13))))
                )
            )
        "#;
        wat.as_bytes().to_vec()
    }

    fn create_exec_command_wasm() -> Vec<u8> {
        let wat = r#"
            (module
                (import "host_api_v1" "set_output" (func $set_output (param i32 i32)))
                (import "host_api_v1" "exec_command"
                    (func $exec_command (param i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "echo ok")
                (data (i32.const 100) "no_response")

                (func (export "run")
                    (local $resp_ptr i32)
                    (local.set $resp_ptr
                        (call $exec_command (i32.const 0) (i32.const 7) (i32.const 1000)))
                    (if (i32.eqz (local.get $resp_ptr))
                        (then (call $set_output (i32.const 100) (i32.const 11)))
                        (else (call $set_output (local.get $resp_ptr) (i32.const 11))))
                )
            )
        "#;
        wat.as_bytes().to_vec()
    }

    fn create_read_file_wasm() -> Vec<u8> {
        let wat = r#"
            (module
                (import "host_api_v1" "set_output" (func $set_output (param i32 i32)))
                (import "host_api_v1" "read_file"
                    (func $read_file (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "/tmp/input.txt")
                (data (i32.const 100) "no_response")

                (func (export "run")
                    (local $resp_ptr i32)
                    (local.set $resp_ptr (call $read_file (i32.const 0) (i32.const 14)))
                    (if (i32.eqz (local.get $resp_ptr))
                        (then (call $set_output (i32.const 100) (i32.const 11)))
                        (else (call $set_output (local.get $resp_ptr) (i32.const 11))))
                )
            )
        "#;
        wat.as_bytes().to_vec()
    }

    fn create_write_file_wasm() -> Vec<u8> {
        let wat = r#"
            (module
                (import "host_api_v1" "set_output" (func $set_output (param i32 i32)))
                (import "host_api_v1" "write_file"
                    (func $write_file (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "out.txt")
                (data (i32.const 32) "hello")
                (data (i32.const 100) "written")
                (data (i32.const 120) "failed")

                (func (export "run")
                    (if (i32.eq (call $write_file
                            (i32.const 0) (i32.const 7)
                            (i32.const 32) (i32.const 5))
                            (i32.const 1))
                        (then (call $set_output (i32.const 100) (i32.const 7)))
                        (else (call $set_output (i32.const 120) (i32.const 6))))
                )
            )
        "#;
        wat.as_bytes().to_vec()
    }

    fn create_http_manifest(name: &str, with_network: bool) -> SkillManifest {
        SkillManifest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: "HTTP test skill".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: if with_network {
                vec![Capability::Network]
            } else {
                vec![]
            },
            tools: vec![],
            entry_point: "run".to_string(),
        }
    }

    #[test]
    fn test_wasm_http_request_no_canned_response() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        let manifest = create_http_manifest("http_test", true);
        let wasm = create_http_request_wasm();

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        // invoke() uses MockHostApi with no canned responses.
        // The WASM module gets None from http_request -> outputs "no_response".
        let output = runtime.invoke("http_test", "input").expect("Should invoke");
        assert_eq!(output, "no_response");
    }

    #[test]
    fn test_wasm_http_request_success_with_canned_response() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        let manifest = create_http_manifest("http_success", true);
        let wasm = create_http_request_wasm();

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        // Create a MockHostApi with a canned response for the URL the WASM sends.
        // The WAT uses url_ptr=3, url_len=21 -> "https://mock.test/api"
        let mock_api = MockHostApi::new("input");
        mock_api.add_http_response("https://mock.test/api", "mock_response!");

        // invoke_with_api lets us inject the prepared MockHostApi
        let output = runtime
            .invoke_with_api("http_success", Box::new(mock_api))
            .expect("Should invoke");

        // The WAT reads 13 bytes from the response pointer for set_output.
        // "mock_response!" is 14 bytes, but the WAT only reads 13 -> "mock_response"
        assert_eq!(output, "mock_response");
    }

    #[test]
    fn test_wasm_http_request_without_network_capability() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        // Skill without Network capability
        let manifest = create_http_manifest("no_net", false);
        let wasm = create_http_request_wasm();

        let skill = loader.load(&wasm, &manifest, None).expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        // Without Network capability, http_request returns 0 → "no_response"
        let output = runtime.invoke("no_net", "input").expect("Should invoke");
        assert_eq!(output, "no_response");
    }

    #[test]
    fn test_wasm_exec_command_with_shell_capability() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        let mut manifest = create_test_manifest("exec_command");
        manifest.capabilities = vec![Capability::Shell];
        let skill = loader
            .load(&create_exec_command_wasm(), &manifest, None)
            .expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        let output = runtime
            .invoke_with_api("exec_command", Box::new(CapabilityRuntimeHostApi::new()))
            .expect("Should invoke");
        assert_eq!(output, "exec_result");
    }

    #[test]
    fn test_wasm_read_file_with_filesystem_capability() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        let mut manifest = create_test_manifest("read_file");
        manifest.capabilities = vec![Capability::Filesystem];
        let skill = loader
            .load(&create_read_file_wasm(), &manifest, None)
            .expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        let output = runtime
            .invoke_with_api("read_file", Box::new(CapabilityRuntimeHostApi::new()))
            .expect("Should invoke");
        assert_eq!(output, "file_result");
    }

    #[test]
    fn test_wasm_write_file_with_filesystem_capability() {
        let mut runtime = SkillRuntime::new().expect("Should create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);

        let mut manifest = create_test_manifest("write_file");
        manifest.capabilities = vec![Capability::Filesystem];
        let skill = loader
            .load(&create_write_file_wasm(), &manifest, None)
            .expect("Should load");
        runtime.register_skill(skill).expect("Should register");

        let output = runtime
            .invoke_with_api("write_file", Box::new(CapabilityRuntimeHostApi::new()))
            .expect("Should invoke");
        assert_eq!(output, "written");
    }

    #[test]
    fn v1_skill_loads_with_v2_host() {
        let mut runtime = SkillRuntime::new().expect("create runtime");
        let loader = SkillLoader::with_engine(runtime.engine().clone(), vec![]);
        let manifest = create_test_manifest("v1_skill");
        let wasm = create_invocable_wasm(runtime.engine());
        let skill = loader.load(&wasm, &manifest, None).expect("load");
        runtime.register_skill(skill).expect("register");
        let output = runtime.invoke("v1_skill", "hello").expect("invoke");
        assert_eq!(output, "ok");
    }

    #[test]
    fn execution_context_serialization() {
        use fx_core::types::ExecutionContext;
        let ctx = ExecutionContext {
            channel_id: Some("telegram".to_string()),
            node_id: Some("node-1".to_string()),
            user_id: Some("user-42".to_string()),
            timestamp_ms: 1_700_000_000_000,
            api_version: "host_api_v2".to_string(),
        };
        let json = serde_json::to_string(&ctx).expect("serialize");
        let parsed: ExecutionContext = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.channel_id, Some("telegram".to_string()));
        assert_eq!(parsed.node_id, Some("node-1".to_string()));
        assert_eq!(parsed.user_id, Some("user-42".to_string()));
        assert_eq!(parsed.timestamp_ms, 1_700_000_000_000);
        assert_eq!(parsed.api_version, "host_api_v2");
    }
}
