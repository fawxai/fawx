//! WASM skill adapter — bridges [`fx_skills::SkillRuntime`] into the
//! [`Skill`] trait consumed by [`SkillRegistry`].
//!
//! Each installed WASM skill becomes a single tool whose name matches the
//! skill's manifest name. The kernel dispatches tool calls to the adapter,
//! which forwards them to the WASM runtime with a [`LiveHostApi`].

use crate::skill::{Skill, SkillError};
use crate::wasm_host::{LiveHostApi, LiveHostApiConfig};
use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_skills::loader::LoadedSkill;
use fx_skills::manifest::SkillManifest;
use fx_skills::runtime::SkillRuntime;
use std::sync::{Arc, Mutex};

/// A WASM skill adapted to the kernel's [`Skill`] trait.
///
/// Wraps a [`SkillRuntime`] containing one loaded skill and exposes it
/// as a single tool whose name and description come from the manifest.
///
/// # 1:1 runtime mapping
/// Each `WasmSkill` owns its own `SkillRuntime` instance. This is intentional:
/// wasmtime requires that `Module` and `Store` share the same `Engine`, so each
/// skill's compiled module must be paired with a runtime using the same engine.
/// Pooling runtimes across skills would require re-compiling modules against a
/// shared engine — acceptable for V2 but not worth the complexity for V1.
pub struct WasmSkill {
    manifest: SkillManifest,
    runtime: Arc<Mutex<SkillRuntime>>,
}

impl std::fmt::Debug for WasmSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmSkill")
            .field("name", &self.manifest.name)
            .field("version", &self.manifest.version)
            .finish()
    }
}

impl WasmSkill {
    /// Create a new WASM skill adapter from a loaded skill.
    ///
    /// Extracts the wasmtime `Engine` from the compiled module so the
    /// runtime uses the same engine (wasmtime requires engine parity
    /// between `Module` and `Store`).
    pub fn new(loaded: LoadedSkill) -> Result<Self, SkillError> {
        let manifest = loaded.manifest().clone();
        let engine = loaded.module().engine().clone();
        let mut runtime = SkillRuntime::with_engine(engine);
        runtime
            .register_skill(loaded)
            .map_err(|e| format!("failed to register WASM skill: {e}"))?;
        Ok(Self {
            manifest,
            runtime: Arc::new(Mutex::new(runtime)),
        })
    }

    /// Build a [`ToolDefinition`] from the skill manifest.
    ///
    /// Each WASM skill exposes exactly one tool. The parameters schema
    /// accepts a single `input` string — the raw JSON payload forwarded
    /// to the WASM entry point via the host API.
    fn build_tool_definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.manifest.name.clone(),
            description: self.manifest.description.clone(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "JSON input for the WASM skill"
                    }
                },
                "required": ["input"]
            }),
        }
    }
}

#[async_trait]
impl Skill for WasmSkill {
    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![self.build_tool_definition()]
    }

    fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
        // WASM skills may have arbitrary side effects we can't predict.
        ToolCacheability::NeverCache
    }

    /// Execute the WASM skill via `spawn_blocking` to avoid blocking the
    /// async executor during potentially long-running WASM computation.
    ///
    /// # Cancellation
    /// `_cancel` is intentionally unused in V1. WASM skills run to completion
    /// once started — there is no safe mid-execution interruption point in the
    /// current wasmtime setup. Future support via wasmtime epoch interruption
    /// is tracked in issue #1136.
    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        if tool_name != self.manifest.name {
            return None;
        }

        // Extract the "input" field from the arguments JSON.
        let input = match serde_json::from_str::<serde_json::Value>(arguments) {
            Ok(val) => val
                .get("input")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            Err(e) => return Some(Err(format!("invalid arguments JSON: {e}"))),
        };

        let skill_name = self.manifest.name.clone();
        let runtime = Arc::clone(&self.runtime);

        // Run WASM execution on a blocking thread to keep the async
        // executor free for other tasks.
        let result = tokio::task::spawn_blocking(move || {
            let host_api = Box::new(LiveHostApi::new(LiveHostApiConfig {
                skill_name: &skill_name,
                input: input.clone(),
                storage_quota: None,
            }));
            let mut rt = runtime.lock().unwrap_or_else(|p| p.into_inner());
            rt.invoke_with_host_api(&skill_name, &input, host_api)
        })
        .await;

        let mapped = match result {
            Ok(inner) => {
                inner.map_err(|e| format!("WASM skill '{}' failed: {e}", self.manifest.name))
            }
            Err(join_err) => Err(format!("WASM task panicked: {join_err}")),
        };

        Some(mapped)
    }
}

/// Load all installed WASM skills from `~/.fawx/skills/` and return
/// them as boxed [`Skill`] trait objects ready for registry insertion.
///
/// Errors from individual skills are logged and skipped; only a
/// directory-level failure propagates as an error.
pub fn load_wasm_skills() -> Result<Vec<Box<dyn Skill>>, SkillError> {
    let wasm_registry = fx_skills::registry::SkillRegistry::new()
        .map_err(|e| format!("failed to create WASM skill registry: {e}"))?;

    let loaded = wasm_registry
        .load_all()
        .map_err(|e| format!("failed to load WASM skills: {e}"))?;

    let mut skills: Vec<Box<dyn Skill>> = Vec::new();

    for (name, loaded_skill) in loaded {
        match WasmSkill::new(loaded_skill) {
            Ok(wasm_skill) => {
                tracing::info!(skill = %name, "loaded WASM skill");
                skills.push(Box::new(wasm_skill));
            }
            Err(e) => {
                tracing::warn!(skill = %name, error = %e, "skipping WASM skill");
            }
        }
    }

    Ok(skills)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_skills::loader::SkillLoader;
    use fx_skills::manifest::SkillManifest;

    fn test_manifest(name: &str) -> SkillManifest {
        SkillManifest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: format!("{name} skill"),
            author: "Test".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            entry_point: "run".to_string(),
        }
    }

    fn invocable_wasm_bytes() -> Vec<u8> {
        let wat = r#"
            (module
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
        wat.as_bytes().to_vec()
    }

    fn load_test_skill(name: &str) -> LoadedSkill {
        let loader = SkillLoader::new(vec![]);
        let manifest = test_manifest(name);
        let wasm = invocable_wasm_bytes();
        loader
            .load(&wasm, &manifest, None)
            .expect("load test skill")
    }

    #[test]
    fn wasm_skill_name_matches_manifest() {
        let skill = WasmSkill::new(load_test_skill("echo")).expect("create");
        assert_eq!(skill.name(), "echo");
    }

    #[test]
    fn wasm_skill_exposes_one_tool() {
        let skill = WasmSkill::new(load_test_skill("echo")).expect("create");
        let defs = skill.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "echo");
        assert_eq!(defs[0].description, "echo skill");
    }

    #[test]
    fn wasm_skill_cacheability_is_never() {
        let skill = WasmSkill::new(load_test_skill("echo")).expect("create");
        assert_eq!(skill.cacheability("echo"), ToolCacheability::NeverCache);
    }

    #[tokio::test]
    async fn wasm_skill_returns_none_for_unknown_tool() {
        let skill = WasmSkill::new(load_test_skill("echo")).expect("create");
        let result = skill.execute("other_tool", "{}", None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn wasm_skill_executes_known_tool() {
        let skill = WasmSkill::new(load_test_skill("echo")).expect("create");
        let result = skill.execute("echo", r#"{"input": "hello"}"#, None).await;
        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.is_ok(), "expected Ok, got: {output:?}");
        // The test WASM always outputs "ok"
        assert_eq!(output.unwrap(), "ok");
    }

    #[tokio::test]
    async fn wasm_skill_handles_missing_input_field() {
        let skill = WasmSkill::new(load_test_skill("echo")).expect("create");
        // No "input" key — falls back to empty string
        let result = skill.execute("echo", "{}", None).await;
        assert!(result.is_some());
        // Should still execute (skill gets empty input)
        assert!(result.unwrap().is_ok());
    }

    #[tokio::test]
    async fn wasm_skill_handles_invalid_json() {
        let skill = WasmSkill::new(load_test_skill("echo")).expect("create");
        let result = skill.execute("echo", "not json", None).await;
        assert!(result.is_some());
        let err = result.unwrap();
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("invalid arguments JSON"));
    }

    #[test]
    fn wasm_skill_debug_format() {
        let skill = WasmSkill::new(load_test_skill("echo")).expect("create");
        let debug = format!("{skill:?}");
        assert!(debug.contains("echo"));
        assert!(debug.contains("1.0.0"));
    }

    #[test]
    fn load_wasm_skills_empty_dir() {
        // Default ~/.fawx/skills/ may be empty or have skills — just verify no panic
        let result = load_wasm_skills();
        assert!(result.is_ok());
    }
}
