//! WASM skill adapter — bridges [`fx_skills::SkillRuntime`] into the
//! [`Skill`] trait consumed by [`SkillRegistry`].
//!
//! Each installed WASM skill can expose one or more tools declared in its
//! manifest. The kernel dispatches tool calls to the adapter, which forwards
//! normalized JSON input to the WASM runtime with a [`LiveHostApi`].

use crate::lifecycle::{current_time_millis, hash_string, SignatureStatus, SkillRevision};
use crate::skill::{Skill, SkillError};
use crate::wasm_host::{LiveHostApi, LiveHostApiConfig};
use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::ToolAuthoritySurface;
use fx_llm::{ToolCall, ToolDefinition};
use fx_skills::live_host_api::CredentialProvider;
use fx_skills::loader::LoadedSkill;
use fx_skills::manifest::{SkillManifest, SkillToolAuthoritySurface, SkillToolManifest};
use fx_skills::runtime::SkillRuntime;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Complete signature verification policy — trusted keys and enforcement mode.
///
/// Groups `trusted_keys` and `require_signatures` into a single value so they
/// travel together through load / watch / validate call chains without inflating
/// parameter lists.
#[derive(Debug, Clone, Default)]
pub struct SignaturePolicy {
    /// Ed25519 public keys trusted for WASM skill verification.
    pub trusted_keys: Vec<Vec<u8>>,
    /// When true, reject unsigned skills. Invalid signatures are always rejected
    /// regardless of this flag.
    pub require_signatures: bool,
}

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
    credential_provider: Option<Arc<dyn CredentialProvider>>,
}

pub struct LoadedWasmArtifact {
    pub skill: WasmSkill,
    pub revision: SkillRevision,
    pub manifest_toml: String,
    pub wasm_bytes: Vec<u8>,
    pub signature_bytes: Option<Vec<u8>>,
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
    pub fn new(
        loaded: LoadedSkill,
        credential_provider: Option<Arc<dyn CredentialProvider>>,
    ) -> Result<Self, SkillError> {
        let manifest = loaded.manifest().clone();
        let engine = loaded.module().engine().clone();
        let mut runtime = SkillRuntime::with_engine(engine);
        runtime
            .register_skill(loaded)
            .map_err(|e| format!("failed to register WASM skill: {e}"))?;
        Ok(Self {
            manifest,
            runtime: Arc::new(Mutex::new(runtime)),
            credential_provider,
        })
    }

    /// The skill version from the manifest.
    pub fn version(&self) -> &str {
        &self.manifest.version
    }

    /// Build the legacy single-tool [`ToolDefinition`] from the skill manifest.
    ///
    /// Skills without manifest-declared tools still expose one compatibility
    /// tool whose `input` string is forwarded directly to the WASM entrypoint.
    fn build_legacy_tool_definition(&self) -> ToolDefinition {
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

    fn build_manifest_tool_definition(tool: &SkillToolManifest) -> ToolDefinition {
        let properties = tool
            .parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.name.clone(),
                    serde_json::json!({
                        "type": parameter.kind,
                        "description": parameter.description,
                    }),
                )
            })
            .collect::<serde_json::Map<String, serde_json::Value>>();
        let required = tool
            .parameters
            .iter()
            .filter(|parameter| parameter.required)
            .map(|parameter| serde_json::Value::String(parameter.name.clone()))
            .collect::<Vec<_>>();

        let mut parameters = serde_json::Map::new();
        parameters.insert(
            "type".to_string(),
            serde_json::Value::String("object".to_string()),
        );
        parameters.insert(
            "properties".to_string(),
            serde_json::Value::Object(properties),
        );
        parameters.insert("required".to_string(), serde_json::Value::Array(required));
        if tool.direct_utility {
            parameters.insert(
                "x-fawx-direct-utility".to_string(),
                serde_json::json!({
                    "enabled": true,
                    "profile": tool.name,
                    "trigger_patterns": tool.trigger_patterns,
                }),
            );
        }

        ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: serde_json::Value::Object(parameters),
        }
    }

    fn build_tool_definitions(&self) -> Vec<ToolDefinition> {
        if self.manifest.tools.is_empty() {
            vec![self.build_legacy_tool_definition()]
        } else {
            self.manifest
                .tools
                .iter()
                .map(Self::build_manifest_tool_definition)
                .collect()
        }
    }

    fn handles_tool(&self, tool_name: &str) -> bool {
        tool_name == self.manifest.name
            || self
                .manifest
                .tools
                .iter()
                .any(|tool| tool.name == tool_name)
    }

    fn manifest_tool(&self, tool_name: &str) -> Option<&SkillToolManifest> {
        self.manifest
            .tools
            .iter()
            .find(|tool| tool.name == tool_name)
    }

    fn authority_surface_for_tool(&self, tool_name: &str) -> ToolAuthoritySurface {
        self.manifest_tool(tool_name)
            .and_then(|tool| tool.authority_surface.clone())
            .map(map_manifest_authority_surface)
            .unwrap_or(ToolAuthoritySurface::Other)
    }

    fn encode_runtime_input(&self, tool_name: &str, arguments: &str) -> Result<String, SkillError> {
        let value = serde_json::from_str::<serde_json::Value>(arguments)
            .map_err(|error| format!("invalid arguments JSON: {error}"))?;

        if self.manifest.tools.is_empty() {
            return Ok(extract_legacy_input(value));
        }

        if let Some(tool) = self.manifest_tool(tool_name) {
            return self.encode_manifest_tool_input(tool, value);
        }

        if tool_name == self.manifest.name {
            return self.encode_manifest_alias_input(value);
        }

        Err(format!("unknown manifest tool: {tool_name}"))
    }

    fn encode_manifest_alias_input(&self, value: serde_json::Value) -> Result<String, SkillError> {
        match self.manifest.tools.as_slice() {
            [tool] => self.encode_manifest_tool_input(tool, value),
            _ => normalize_legacy_router_input(value),
        }
    }

    fn encode_manifest_tool_input(
        &self,
        tool: &SkillToolManifest,
        value: serde_json::Value,
    ) -> Result<String, SkillError> {
        let serde_json::Value::Object(mut object) = value else {
            return Err("tool arguments must be a JSON object".to_string());
        };
        if self.manifest.tools.len() > 1 {
            object.insert(
                "tool".to_string(),
                serde_json::Value::String(tool.name.clone()),
            );
        }
        Ok(serde_json::Value::Object(object).to_string())
    }
}

fn extract_legacy_input(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(mut object) => match object.remove("input") {
            Some(serde_json::Value::String(input)) => input,
            Some(other) => other.to_string(),
            None if object.is_empty() => String::new(),
            None => serde_json::Value::Object(object).to_string(),
        },
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn normalize_legacy_router_input(value: serde_json::Value) -> Result<String, SkillError> {
    match value {
        serde_json::Value::Object(mut object) => {
            if let Some(input) = object.remove("input") {
                normalize_legacy_router_payload(input)
            } else {
                normalize_legacy_router_payload(serde_json::Value::Object(object))
            }
        }
        other => normalize_legacy_router_payload(other),
    }
}

fn normalize_legacy_router_payload(value: serde_json::Value) -> Result<String, SkillError> {
    match value {
        serde_json::Value::String(raw) => {
            if raw.trim().is_empty() {
                Ok(raw)
            } else if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                normalize_legacy_router_payload(parsed)
            } else {
                Ok(raw)
            }
        }
        serde_json::Value::Object(mut object) => {
            if !object.contains_key("tool") {
                if let Some(action) = object.remove("action") {
                    if let Some(action_name) = action.as_str() {
                        object.insert(
                            "tool".to_string(),
                            serde_json::Value::String(action_name.to_string()),
                        );
                    } else {
                        object.insert("action".to_string(), action);
                    }
                }
            }
            Ok(serde_json::Value::Object(object).to_string())
        }
        serde_json::Value::Null => Ok(String::new()),
        other => Ok(other.to_string()),
    }
}

#[async_trait]
impl Skill for WasmSkill {
    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn description(&self) -> &str {
        &self.manifest.description
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.build_tool_definitions()
    }

    fn capabilities(&self) -> Vec<String> {
        self.manifest
            .capabilities
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
        // WASM skills may have arbitrary side effects we can't predict.
        ToolCacheability::NeverCache
    }

    fn authority_surface(&self, call: &ToolCall) -> ToolAuthoritySurface {
        self.authority_surface_for_tool(&call.name)
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
        if !self.handles_tool(tool_name) {
            return None;
        }

        let input = match self.encode_runtime_input(tool_name, arguments) {
            Ok(input) => input,
            Err(error) => return Some(Err(error)),
        };

        let skill_name = self.manifest.name.clone();
        let runtime = Arc::clone(&self.runtime);
        let capabilities = self.manifest.capabilities.clone();
        let credential_provider = self.credential_provider.clone();

        // Run WASM execution on a blocking thread to keep the async
        // executor free for other tasks.
        let result = tokio::task::spawn_blocking(move || {
            let host_api = Box::new(LiveHostApi::new(LiveHostApiConfig {
                skill_name: &skill_name,
                input: input.clone(),
                storage_quota: None,
                capabilities,
                credential_provider,
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

fn map_manifest_authority_surface(surface: SkillToolAuthoritySurface) -> ToolAuthoritySurface {
    match surface {
        SkillToolAuthoritySurface::PathRead => ToolAuthoritySurface::PathRead,
        SkillToolAuthoritySurface::PathWrite => ToolAuthoritySurface::PathWrite,
        SkillToolAuthoritySurface::PathDelete => ToolAuthoritySurface::PathDelete,
        SkillToolAuthoritySurface::GitCheckpoint => ToolAuthoritySurface::GitCheckpoint,
        SkillToolAuthoritySurface::Command => ToolAuthoritySurface::Command,
        SkillToolAuthoritySurface::Network => ToolAuthoritySurface::Network,
        SkillToolAuthoritySurface::Other => ToolAuthoritySurface::Other,
    }
}

/// Load a single WASM skill from a directory.
///
/// Reads `manifest.toml` and `{name}.wasm` from `skill_dir`, computes
/// a SHA-256 hash of the WASM bytes, compiles and validates the module,
/// and returns the constructed [`WasmSkill`] alongside the content hash.
///
/// Used by both startup loading ([`load_wasm_skills`]) and the hot-reload
/// watcher to ensure a single validation path.
pub fn load_wasm_skill_from_dir(
    skill_dir: &Path,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    policy: &SignaturePolicy,
) -> Result<(WasmSkill, [u8; 32]), SkillError> {
    let artifact = load_wasm_artifact_from_dir(skill_dir, credential_provider, policy)?;
    let hash = compute_wasm_hash(&artifact.wasm_bytes);
    Ok((artifact.skill, hash))
}

pub fn load_wasm_artifact_from_dir(
    skill_dir: &Path,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    policy: &SignaturePolicy,
) -> Result<LoadedWasmArtifact, SkillError> {
    let manifest_toml = read_manifest_toml(skill_dir)?;
    let manifest = fx_skills::manifest::parse_manifest(&manifest_toml)
        .map_err(|error| format!("invalid manifest in {}: {error}", skill_dir.display()))?;
    let wasm_bytes = read_wasm_bytes(skill_dir, &manifest.name)?;
    let signature = read_signature_file(skill_dir, &manifest.name)?;
    let signature = normalize_signature_status(&wasm_bytes, &signature, policy);
    validate_signature_policy(&signature.bytes, policy, &manifest.name)?;
    let effective_signature = signature
        .bytes
        .as_deref()
        .filter(|_| !policy.trusted_keys.is_empty());
    let loaded = compile_skill(
        &wasm_bytes,
        &manifest,
        effective_signature,
        &policy.trusted_keys,
    )?;
    let skill = WasmSkill::new(loaded, credential_provider)?;
    let revision = build_revision(
        &skill,
        &manifest,
        &manifest_toml,
        signature.status,
        &wasm_bytes,
    );
    Ok(LoadedWasmArtifact {
        skill,
        revision,
        manifest_toml,
        wasm_bytes,
        signature_bytes: signature.bytes,
    })
}

/// Read and parse `manifest.toml` from a skill directory.
pub(crate) fn read_manifest(skill_dir: &Path) -> Result<SkillManifest, SkillError> {
    let content = read_manifest_toml(skill_dir)?;
    fx_skills::manifest::parse_manifest(&content)
        .map_err(|e| format!("invalid manifest in {}: {e}", skill_dir.display()))
}

pub(crate) fn read_manifest_toml(skill_dir: &Path) -> Result<String, SkillError> {
    let manifest_path = skill_dir.join("manifest.toml");
    std::fs::read_to_string(&manifest_path).map_err(|error| {
        format!(
            "failed to read manifest at {}: {error}",
            manifest_path.display()
        )
    })
}

/// Read `{name}.wasm` from a skill directory.
fn read_wasm_bytes(skill_dir: &Path, name: &str) -> Result<Vec<u8>, SkillError> {
    let wasm_path = skill_dir.join(format!("{name}.wasm"));
    std::fs::read(&wasm_path)
        .map_err(|e| format!("failed to read WASM at {}: {e}", wasm_path.display()))
}

/// Compute SHA-256 hash of WASM bytes.
pub fn compute_wasm_hash(wasm_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(wasm_bytes);
    hasher.finalize().into()
}

fn build_revision(
    skill: &WasmSkill,
    manifest: &SkillManifest,
    manifest_toml: &str,
    signature: SignatureStatus,
    wasm_bytes: &[u8],
) -> SkillRevision {
    SkillRevision {
        content_hash: hash_wasm_bytes(wasm_bytes),
        manifest_hash: hash_string(manifest_toml),
        version: manifest.version.clone(),
        signature,
        tool_contracts: skill.tool_definitions(),
        staged_at: current_time_millis(),
    }
}

fn hash_wasm_bytes(wasm_bytes: &[u8]) -> String {
    encode_hash_bytes(&compute_wasm_hash(wasm_bytes))
}

fn encode_hash_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(nibble_to_hex(byte >> 4));
        output.push(nibble_to_hex(byte & 0x0f));
    }
    output
}

fn nibble_to_hex(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'a' + (value - 10)) as char,
    }
}

/// Compile a WASM skill from bytes and manifest, with optional signature verification.
fn compile_skill(
    wasm_bytes: &[u8],
    manifest: &SkillManifest,
    signature: Option<&[u8]>,
    trusted_keys: &[Vec<u8>],
) -> Result<LoadedSkill, SkillError> {
    fx_skills::loader::SkillLoader::new(trusted_keys.to_vec())
        .load(wasm_bytes, manifest, signature)
        .map_err(|e| format!("failed to compile skill '{}': {e}", manifest.name))
}

/// Read `{name}.wasm.sig` from a skill directory, if present.
/// Returns `None` if file doesn't exist, `Err` on read failure.
fn read_signature_file(skill_dir: &Path, name: &str) -> Result<Option<Vec<u8>>, SkillError> {
    let sig_path = skill_dir.join(format!("{name}.wasm.sig"));
    match std::fs::read(&sig_path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!(
            "failed to read signature file {}: {e}",
            sig_path.display()
        )),
    }
}

struct NormalizedSignature {
    bytes: Option<Vec<u8>>,
    status: SignatureStatus,
}

/// Load Ed25519 public keys from `~/.fawx/trusted_keys/*.pub`.
/// Each file contains a raw 32-byte Ed25519 public key.
/// Returns empty vec if directory doesn't exist.
///
/// Delegates to [`load_trusted_keys_from`] with the default keys directory.
pub fn load_trusted_keys() -> Result<Vec<Vec<u8>>, SkillError> {
    let home = dirs::home_dir().ok_or_else(|| "failed to determine home directory".to_string())?;
    let keys_dir = home.join(".fawx").join("trusted_keys");
    load_trusted_keys_from(&keys_dir)
}

/// Load Ed25519 public keys from `*.pub` files in `keys_dir`.
///
/// Each `.pub` file must contain exactly 32 bytes (a raw Ed25519 public key).
/// Files that are not 32 bytes are logged and skipped.
/// Directory entry read errors are logged and skipped rather than silently
/// swallowed, since this is a security-critical path.
///
/// Returns empty vec if `keys_dir` doesn't exist.
pub fn load_trusted_keys_from(keys_dir: &Path) -> Result<Vec<Vec<u8>>, SkillError> {
    let entries = match std::fs::read_dir(keys_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(format!(
                "failed to read trusted keys directory {}: {e}",
                keys_dir.display()
            ))
        }
    };

    let mut keys = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("pub") {
                    let key_bytes = std::fs::read(&path).map_err(|e| {
                        format!("failed to read trusted key {}: {e}", path.display())
                    })?;
                    if key_bytes.len() != 32 {
                        tracing::warn!(
                            path = %path.display(),
                            len = key_bytes.len(),
                            "skipping invalid trusted key (expected 32 bytes)"
                        );
                        continue;
                    }
                    keys.push(key_bytes);
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "failed to read directory entry in trusted keys"
                );
            }
        }
    }

    Ok(keys)
}

/// Validate signature policy before compilation.
///
/// - Signature present + no trusted keys → warn, allow load
/// - No signature + require_signatures → reject
/// - No signature + !require_signatures → warn, allow load
/// - Signature verification itself is handled by `SkillLoader::load()`
fn validate_signature_policy(
    signature: &Option<Vec<u8>>,
    policy: &SignaturePolicy,
    skill_name: &str,
) -> Result<(), SkillError> {
    match signature {
        Some(_) if policy.trusted_keys.is_empty() => {
            tracing::warn!(
                skill = %skill_name,
                "signature found but no trusted keys configured — cannot verify"
            );
            Ok(())
        }
        Some(_) => Ok(()),
        None if policy.require_signatures => Err(format!(
            "skill '{}' has no signature but require_signatures is enabled",
            skill_name
        )),
        None => {
            tracing::warn!(skill = %skill_name, "loading unsigned WASM skill");
            Ok(())
        }
    }
}

fn normalize_signature_status(
    wasm_bytes: &[u8],
    signature: &Option<Vec<u8>>,
    policy: &SignaturePolicy,
) -> NormalizedSignature {
    let Some(bytes) = signature.clone() else {
        return NormalizedSignature {
            bytes: None,
            status: SignatureStatus::Unsigned,
        };
    };
    let status = matching_signer(wasm_bytes, &bytes, &policy.trusted_keys)
        .map(|signer| SignatureStatus::Valid { signer })
        .unwrap_or(SignatureStatus::Invalid);
    NormalizedSignature {
        bytes: Some(bytes),
        status,
    }
}

fn matching_signer(
    wasm_bytes: &[u8],
    signature: &[u8],
    trusted_keys: &[Vec<u8>],
) -> Option<String> {
    trusted_keys
        .iter()
        .find_map(|key| signature_matches(wasm_bytes, signature, key))
}

fn signature_matches(wasm_bytes: &[u8], signature: &[u8], key: &[u8]) -> Option<String> {
    match fx_skills::signing::verify_skill(wasm_bytes, signature, key) {
        Ok(true) => Some(format!("ed25519:{}", encode_hash_bytes(key))),
        Ok(false) | Err(_) => None,
    }
}

/// Load all installed WASM skills from `skills_dir` and return them as
/// [`Arc<dyn Skill>`] trait objects ready for registry insertion.
///
/// The optional `credential_provider` bridges the encrypted credential
/// store so skills can retrieve secrets (e.g., GitHub PAT) via `kv_get`.
///
/// Errors from individual skills are logged and skipped; only a
/// directory-level failure propagates as an error.
pub fn load_wasm_skills_from(
    skills_dir: &Path,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    policy: &SignaturePolicy,
) -> Result<Vec<Arc<dyn Skill>>, SkillError> {
    let entries = read_skill_directories(skills_dir)?;

    let mut skills: Vec<Arc<dyn Skill>> = Vec::new();

    for entry in entries {
        let skill_dir = entry.path();
        match load_wasm_skill_from_dir(&skill_dir, credential_provider.clone(), policy) {
            Ok((wasm_skill, _hash)) => {
                tracing::info!(skill = %wasm_skill.name(), "loaded WASM skill");
                skills.push(Arc::new(wasm_skill));
            }
            Err(e) => {
                tracing::warn!(dir = %skill_dir.display(), error = %e, "skipping WASM skill");
            }
        }
    }

    Ok(skills)
}

/// Load all installed WASM skills from `~/.fawx/skills/` and return
/// them as [`Arc<dyn Skill>`] trait objects ready for registry insertion.
pub fn load_wasm_skills(
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    policy: &SignaturePolicy,
) -> Result<Vec<Arc<dyn Skill>>, SkillError> {
    let skills_dir = skills_directory()?;
    load_wasm_skills_from(&skills_dir, credential_provider, policy)
}

/// Resolve the `~/.fawx/skills/` directory path.
fn skills_directory() -> Result<std::path::PathBuf, SkillError> {
    let home = dirs::home_dir().ok_or_else(|| "failed to determine home directory".to_string())?;
    Ok(home.join(".fawx").join("skills"))
}

/// Read subdirectories from the skills directory.
fn read_skill_directories(skills_dir: &Path) -> Result<Vec<std::fs::DirEntry>, SkillError> {
    match std::fs::read_dir(skills_dir) {
        Ok(entries) => {
            let mut dirs = Vec::new();
            for entry in entries {
                match entry {
                    Ok(e) if e.path().is_dir() => dirs.push(e),
                    Ok(_) => {} // regular file, skip
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "failed to read directory entry in skills directory"
                        );
                    }
                }
            }
            Ok(dirs)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!(
            "failed to read skills directory {}: {e}",
            skills_dir.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::invocable_wasm_bytes;
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
            tools: vec![],
            entry_point: "run".to_string(),
        }
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
        let skill = WasmSkill::new(load_test_skill("echo"), None).expect("create");
        assert_eq!(skill.name(), "echo");
    }

    #[test]
    fn wasm_skill_exposes_one_tool() {
        let skill = WasmSkill::new(load_test_skill("echo"), None).expect("create");
        let defs = skill.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "echo");
        assert_eq!(defs[0].description, "echo skill");
    }

    #[test]
    fn wasm_skill_exposes_manifest_declared_tools() {
        let mut manifest = test_manifest("browser");
        manifest.tools = vec![
            fx_skills::manifest::SkillToolManifest {
                name: "web_search".to_string(),
                description: "Search".to_string(),
                authority_surface: Some(fx_skills::manifest::SkillToolAuthoritySurface::Network),
                direct_utility: false,
                trigger_patterns: Vec::new(),
                parameters: vec![fx_skills::manifest::SkillToolParameterManifest {
                    name: "query".to_string(),
                    kind: "string".to_string(),
                    description: "Search query".to_string(),
                    required: true,
                }],
            },
            fx_skills::manifest::SkillToolManifest {
                name: "web_fetch".to_string(),
                description: "Fetch".to_string(),
                authority_surface: Some(fx_skills::manifest::SkillToolAuthoritySurface::Network),
                direct_utility: false,
                trigger_patterns: Vec::new(),
                parameters: vec![],
            },
        ];
        let loader = SkillLoader::new(vec![]);
        let loaded = loader
            .load(&invocable_wasm_bytes(), &manifest, None)
            .expect("load test skill");
        let skill = WasmSkill::new(loaded, None).expect("create");

        let defs = skill.tool_definitions();
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name, "web_search");
        assert_eq!(defs[1].name, "web_fetch");
        assert_eq!(defs[0].parameters["required"], serde_json::json!(["query"]));
    }

    #[test]
    fn wasm_skill_reports_manifest_authority_surface() {
        let mut manifest = test_manifest("browser");
        manifest.tools = vec![fx_skills::manifest::SkillToolManifest {
            name: "web_search".to_string(),
            description: "Search".to_string(),
            authority_surface: Some(fx_skills::manifest::SkillToolAuthoritySurface::Network),
            direct_utility: false,
            trigger_patterns: Vec::new(),
            parameters: vec![fx_skills::manifest::SkillToolParameterManifest {
                name: "query".to_string(),
                kind: "string".to_string(),
                description: "Search query".to_string(),
                required: true,
            }],
        }];
        let loader = SkillLoader::new(vec![]);
        let loaded = loader
            .load(&invocable_wasm_bytes(), &manifest, None)
            .expect("load test skill");
        let skill = WasmSkill::new(loaded, None).expect("create");
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "web_search".to_string(),
            arguments: serde_json::json!({"query":"rust"}),
        };

        assert_eq!(
            skill.authority_surface(&call),
            ToolAuthoritySurface::Network
        );
    }

    #[tokio::test]
    async fn wasm_skill_named_tool_uses_declared_schema_instead_of_legacy_input_wrapper() {
        let mut manifest = test_manifest("weather");
        manifest.tools = vec![fx_skills::manifest::SkillToolManifest {
            name: "weather".to_string(),
            description: "Weather".to_string(),
            authority_surface: None,
            direct_utility: true,
            trigger_patterns: vec!["weather".to_string(), "forecast".to_string()],
            parameters: vec![fx_skills::manifest::SkillToolParameterManifest {
                name: "location".to_string(),
                kind: "string".to_string(),
                description: "City or location".to_string(),
                required: true,
            }],
        }];
        let loader = SkillLoader::new(vec![]);
        let loaded = loader
            .load(&invocable_wasm_bytes(), &manifest, None)
            .expect("load test skill");
        let skill = WasmSkill::new(loaded, None).expect("create");

        let defs = skill.tool_definitions();
        assert_eq!(defs[0].name, "weather");
        assert!(defs[0].parameters["properties"].get("location").is_some());
        assert!(defs[0].parameters["properties"].get("input").is_none());
        assert_eq!(
            defs[0].parameters["x-fawx-direct-utility"]["trigger_patterns"],
            serde_json::json!(["weather", "forecast"])
        );

        let result = skill
            .execute("weather", r#"{"location":"Denver, CO"}"#, None)
            .await;
        assert!(result.is_some());
        assert!(result.expect("known tool").is_ok());
    }

    #[test]
    fn single_manifest_tool_keeps_structured_arguments_without_tool_wrapper() {
        let mut manifest = test_manifest("calculator");
        manifest.tools = vec![fx_skills::manifest::SkillToolManifest {
            name: "calculate".to_string(),
            description: "Calculate".to_string(),
            authority_surface: None,
            direct_utility: false,
            trigger_patterns: Vec::new(),
            parameters: vec![fx_skills::manifest::SkillToolParameterManifest {
                name: "expression".to_string(),
                kind: "string".to_string(),
                description: "Expression".to_string(),
                required: true,
            }],
        }];
        let loader = SkillLoader::new(vec![]);
        let loaded = loader
            .load(&invocable_wasm_bytes(), &manifest, None)
            .expect("load test skill");
        let skill = WasmSkill::new(loaded, None).expect("create");

        let encoded = skill
            .encode_runtime_input("calculate", r#"{"expression":"2 + 2"}"#)
            .expect("encode");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).expect("json"),
            serde_json::json!({"expression":"2 + 2"})
        );
    }

    #[test]
    fn multi_tool_manifest_inserts_explicit_tool_name_for_runtime_routing() {
        let mut manifest = test_manifest("canvas");
        manifest.tools = vec![
            fx_skills::manifest::SkillToolManifest {
                name: "render_table".to_string(),
                description: "Render table".to_string(),
                authority_surface: None,
                direct_utility: false,
                trigger_patterns: Vec::new(),
                parameters: vec![fx_skills::manifest::SkillToolParameterManifest {
                    name: "headers".to_string(),
                    kind: "string".to_string(),
                    description: "Headers".to_string(),
                    required: true,
                }],
            },
            fx_skills::manifest::SkillToolManifest {
                name: "render_chart".to_string(),
                description: "Render chart".to_string(),
                authority_surface: None,
                direct_utility: false,
                trigger_patterns: Vec::new(),
                parameters: vec![fx_skills::manifest::SkillToolParameterManifest {
                    name: "data".to_string(),
                    kind: "string".to_string(),
                    description: "Data".to_string(),
                    required: true,
                }],
            },
        ];
        let loader = SkillLoader::new(vec![]);
        let loaded = loader
            .load(&invocable_wasm_bytes(), &manifest, None)
            .expect("load test skill");
        let skill = WasmSkill::new(loaded, None).expect("create");

        let encoded = skill
            .encode_runtime_input("render_table", r#"{"headers":"Name,Score"}"#)
            .expect("encode");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).expect("json"),
            serde_json::json!({
                "tool": "render_table",
                "headers": "Name,Score"
            })
        );
    }

    #[test]
    fn wasm_skill_cacheability_is_never() {
        let skill = WasmSkill::new(load_test_skill("echo"), None).expect("create");
        assert_eq!(skill.cacheability("echo"), ToolCacheability::NeverCache);
    }

    #[tokio::test]
    async fn wasm_skill_returns_none_for_unknown_tool() {
        let skill = WasmSkill::new(load_test_skill("echo"), None).expect("create");
        let result = skill.execute("other_tool", "{}", None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn wasm_skill_executes_known_tool() {
        let skill = WasmSkill::new(load_test_skill("echo"), None).expect("create");
        let result = skill.execute("echo", r#"{"input": "hello"}"#, None).await;
        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.is_ok(), "expected Ok, got: {output:?}");
        // The test WASM always outputs "ok"
        assert_eq!(output.unwrap(), "ok");
    }

    #[tokio::test]
    async fn wasm_skill_handles_missing_input_field() {
        let skill = WasmSkill::new(load_test_skill("echo"), None).expect("create");
        // No "input" key — falls back to empty string
        let result = skill.execute("echo", "{}", None).await;
        assert!(result.is_some());
        // Should still execute (skill gets empty input)
        assert!(result.unwrap().is_ok());
    }

    #[tokio::test]
    async fn wasm_skill_handles_invalid_json() {
        let skill = WasmSkill::new(load_test_skill("echo"), None).expect("create");
        let result = skill.execute("echo", "not json", None).await;
        assert!(result.is_some());
        let err = result.unwrap();
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("invalid arguments JSON"));
    }

    #[test]
    fn wasm_skill_debug_format() {
        let skill = WasmSkill::new(load_test_skill("echo"), None).expect("create");
        let debug = format!("{skill:?}");
        assert!(debug.contains("echo"));
        assert!(debug.contains("1.0.0"));
    }

    #[test]
    fn legacy_router_input_normalizes_nested_action_payload() {
        let normalized = normalize_legacy_router_input(serde_json::json!({
            "input": {
                "action": "web_search",
                "query": "rust async",
                "count": "3"
            }
        }))
        .expect("normalize");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&normalized).expect("json"),
            serde_json::json!({
                "tool": "web_search",
                "query": "rust async",
                "count": "3"
            })
        );
    }

    #[test]
    fn legacy_router_input_normalizes_embedded_json_string() {
        let normalized = normalize_legacy_router_input(serde_json::json!({
            "input": "{\"action\":\"web_fetch\",\"url\":\"https://example.com\"}"
        }))
        .expect("normalize");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&normalized).expect("json"),
            serde_json::json!({
                "tool": "web_fetch",
                "url": "https://example.com"
            })
        );
    }

    #[test]
    fn load_wasm_skills_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = load_wasm_skills_from(tmp.path(), None, &SignaturePolicy::default());
        assert!(result.is_ok());
    }

    #[test]
    fn load_wasm_skills_from_reads_requested_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "dirskill");

        let skills = load_wasm_skills_from(tmp.path(), None, &SignaturePolicy::default())
            .expect("skills should load");

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name(), "dirskill");
    }

    fn setup_skill_dir(dir: &std::path::Path, name: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let manifest = test_manifest(name);
        let toml_str = format!(
            r#"name = "{}"
version = "{}"
description = "{}"
author = "{}"
api_version = "{}"
entry_point = "{}"
"#,
            manifest.name,
            manifest.version,
            manifest.description,
            manifest.author,
            manifest.api_version,
            manifest.entry_point,
        );
        std::fs::write(skill_dir.join("manifest.toml"), toml_str).unwrap();
        std::fs::write(
            skill_dir.join(format!("{name}.wasm")),
            invocable_wasm_bytes(),
        )
        .unwrap();
    }

    #[test]
    fn load_wasm_skill_from_dir_valid_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "testskill");
        let result = load_wasm_skill_from_dir(
            &tmp.path().join("testskill"),
            None,
            &SignaturePolicy::default(),
        );
        assert!(result.is_ok());
        let (skill, hash) = result.unwrap();
        assert_eq!(skill.name(), "testskill");
        assert_eq!(hash, compute_wasm_hash(&invocable_wasm_bytes()));
    }

    #[test]
    fn load_wasm_skill_from_dir_missing_manifest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let skill_dir = tmp.path().join("nomanifest");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("nomanifest.wasm"), invocable_wasm_bytes()).unwrap();

        let result = load_wasm_skill_from_dir(&skill_dir, None, &SignaturePolicy::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("manifest"));
    }

    #[test]
    fn load_wasm_skill_from_dir_missing_wasm() {
        let tmp = tempfile::TempDir::new().unwrap();
        let skill_dir = tmp.path().join("nowasm");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let manifest = test_manifest("nowasm");
        let toml_str = format!(
            r#"name = "{}"
version = "{}"
description = "{}"
author = "{}"
api_version = "{}"
entry_point = "{}"
"#,
            manifest.name,
            manifest.version,
            manifest.description,
            manifest.author,
            manifest.api_version,
            manifest.entry_point,
        );
        std::fs::write(skill_dir.join("manifest.toml"), toml_str).unwrap();

        let result = load_wasm_skill_from_dir(&skill_dir, None, &SignaturePolicy::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("WASM"));
    }

    #[test]
    fn load_wasm_skill_from_dir_invalid_wasm() {
        let tmp = tempfile::TempDir::new().unwrap();
        let skill_dir = tmp.path().join("badwasm");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let manifest = test_manifest("badwasm");
        let toml_str = format!(
            r#"name = "{}"
version = "{}"
description = "{}"
author = "{}"
api_version = "{}"
entry_point = "{}"
"#,
            manifest.name,
            manifest.version,
            manifest.description,
            manifest.author,
            manifest.api_version,
            manifest.entry_point,
        );
        std::fs::write(skill_dir.join("manifest.toml"), toml_str).unwrap();
        std::fs::write(skill_dir.join("badwasm.wasm"), b"not wasm").unwrap();

        let result = load_wasm_skill_from_dir(&skill_dir, None, &SignaturePolicy::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("compile"));
    }

    #[test]
    fn compute_wasm_hash_deterministic() {
        let bytes = invocable_wasm_bytes();
        let hash1 = compute_wasm_hash(&bytes);
        let hash2 = compute_wasm_hash(&bytes);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn compute_wasm_hash_different_for_different_bytes() {
        let hash1 = compute_wasm_hash(b"hello");
        let hash2 = compute_wasm_hash(b"world");
        assert_ne!(hash1, hash2);
    }

    // --- Signature verification wiring tests ---

    #[test]
    fn unsigned_skill_loads_when_signatures_not_required() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "unsigned");
        let policy = SignaturePolicy {
            trusted_keys: vec![],
            require_signatures: false,
        };
        let result = load_wasm_skill_from_dir(&tmp.path().join("unsigned"), None, &policy);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0.name(), "unsigned");
    }

    #[test]
    fn unsigned_skill_rejected_when_signatures_required() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "unsigned_req");
        let policy = SignaturePolicy {
            trusted_keys: vec![],
            require_signatures: true,
        };
        let result = load_wasm_skill_from_dir(&tmp.path().join("unsigned_req"), None, &policy);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("no signature") && err.contains("require_signatures"),
            "expected require_signatures error, got: {err}"
        );
    }

    #[test]
    fn signed_skill_with_valid_signature_loads() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "signed_ok");
        let (private_key, public_key) = fx_skills::signing::generate_keypair().expect("keygen");
        let wasm_bytes = invocable_wasm_bytes();
        let signature = fx_skills::signing::sign_skill(&wasm_bytes, &private_key).expect("sign");
        let skill_dir = tmp.path().join("signed_ok");
        std::fs::write(skill_dir.join("signed_ok.wasm.sig"), &signature).unwrap();

        let policy = SignaturePolicy {
            trusted_keys: vec![public_key],
            require_signatures: true,
        };
        let result = load_wasm_skill_from_dir(&skill_dir, None, &policy);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0.name(), "signed_ok");
    }

    #[test]
    fn signed_skill_with_invalid_signature_always_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "bad_sig");
        let (_, wrong_public_key) = fx_skills::signing::generate_keypair().expect("keygen");
        let (other_private, _) = fx_skills::signing::generate_keypair().expect("keygen2");
        let wasm_bytes = invocable_wasm_bytes();
        let bad_signature =
            fx_skills::signing::sign_skill(&wasm_bytes, &other_private).expect("sign");
        let skill_dir = tmp.path().join("bad_sig");
        std::fs::write(skill_dir.join("bad_sig.wasm.sig"), &bad_signature).unwrap();

        // Even with require_signatures = false, invalid sig must be rejected
        let policy = SignaturePolicy {
            trusted_keys: vec![wrong_public_key],
            require_signatures: false,
        };
        let result = load_wasm_skill_from_dir(&skill_dir, None, &policy);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Signature verification failed"),
            "expected signature verification error, got: {err}"
        );
    }

    #[test]
    fn signed_skill_with_tampered_wasm_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "tampered");
        let (private_key, public_key) = fx_skills::signing::generate_keypair().expect("keygen");
        let original_wasm = invocable_wasm_bytes();
        let signature = fx_skills::signing::sign_skill(&original_wasm, &private_key).expect("sign");
        let skill_dir = tmp.path().join("tampered");
        std::fs::write(skill_dir.join("tampered.wasm.sig"), &signature).unwrap();
        // Tamper with the WASM file after signing
        let mut tampered = original_wasm;
        tampered.push(0xFF);
        std::fs::write(skill_dir.join("tampered.wasm"), &tampered).unwrap();

        let policy = SignaturePolicy {
            trusted_keys: vec![public_key],
            require_signatures: true,
        };
        let result = load_wasm_skill_from_dir(&skill_dir, None, &policy);
        assert!(result.is_err());
    }

    #[test]
    fn signature_present_but_no_trusted_keys_loads_skill() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "nokeys");
        let skill_dir = tmp.path().join("nokeys");
        // Write a dummy signature file (won't be verified — no keys)
        std::fs::write(skill_dir.join("nokeys.wasm.sig"), vec![0u8; 64]).unwrap();

        let result = load_wasm_skill_from_dir(&skill_dir, None, &SignaturePolicy::default());
        // Should load (signature present but no keys to verify against)
        assert!(result.is_ok());
    }

    #[test]
    fn load_trusted_keys_from_reads_pub_files_from_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let keys_dir = tmp.path().join("trusted_keys");
        std::fs::create_dir_all(&keys_dir).unwrap();
        let (_, pub_key1) = fx_skills::signing::generate_keypair().expect("keygen");
        let (_, pub_key2) = fx_skills::signing::generate_keypair().expect("keygen");
        std::fs::write(keys_dir.join("key1.pub"), &pub_key1).unwrap();
        std::fs::write(keys_dir.join("key2.pub"), &pub_key2).unwrap();
        // Non-.pub file should be ignored
        std::fs::write(keys_dir.join("readme.txt"), b"ignore me").unwrap();

        let keys = load_trusted_keys_from(&keys_dir).expect("load keys");
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&pub_key1));
        assert!(keys.contains(&pub_key2));
    }

    #[test]
    fn load_trusted_keys_from_skips_invalid_length_keys() {
        let tmp = tempfile::TempDir::new().unwrap();
        let keys_dir = tmp.path().join("trusted_keys");
        std::fs::create_dir_all(&keys_dir).unwrap();
        let (_, valid_key) = fx_skills::signing::generate_keypair().expect("keygen");
        std::fs::write(keys_dir.join("good.pub"), &valid_key).unwrap();
        // Write a key that's not 32 bytes
        std::fs::write(keys_dir.join("bad.pub"), b"too short").unwrap();
        std::fs::write(keys_dir.join("also_bad.pub"), vec![0u8; 64]).unwrap();

        let keys = load_trusted_keys_from(&keys_dir).expect("load keys");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], valid_key);
    }

    #[test]
    fn load_trusted_keys_from_returns_empty_when_dir_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let keys_dir = tmp.path().join("nonexistent");
        let keys = load_trusted_keys_from(&keys_dir).expect("load keys");
        assert!(keys.is_empty());
    }

    #[test]
    fn load_trusted_keys_delegates_to_load_trusted_keys_from() {
        // load_trusted_keys uses ~/.fawx/trusted_keys, which typically doesn't exist
        // in CI/test envs. Verify it returns Ok gracefully (either empty or loaded).
        let result = load_trusted_keys();
        assert!(result.is_ok());
    }

    #[test]
    fn read_signature_file_returns_none_when_no_sig() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "nosig");
        let skill_dir = tmp.path().join("nosig");
        let result = read_signature_file(&skill_dir, "nosig");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn read_signature_file_reads_valid_sig() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_skill_dir(tmp.path(), "withsig");
        let skill_dir = tmp.path().join("withsig");
        let sig_bytes = vec![42u8; 64];
        std::fs::write(skill_dir.join("withsig.wasm.sig"), &sig_bytes).unwrap();
        let result = read_signature_file(&skill_dir, "withsig");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap(), sig_bytes);
    }
}
