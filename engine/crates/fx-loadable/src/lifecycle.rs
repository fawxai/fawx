use crate::registry::SkillRegistry;
use crate::skill::Skill;
use crate::wasm_skill::{load_wasm_artifact_from_dir, LoadedWasmArtifact, SignaturePolicy};
use fx_llm::ToolDefinition;
use fx_skills::live_host_api::CredentialProvider;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const LIFECYCLE_DIR: &str = ".fawx-lifecycle";
const ACTIVATION_FILE: &str = "activation.json";
const REVISIONS_DIR: &str = "revisions";
const SOURCE_FILE: &str = "source.json";
pub const SOURCE_METADATA_FILE: &str = ".fawx-source.json";

pub type LifecycleError = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SkillSource {
    Published {
        publisher: String,
        registry_url: String,
    },
    LocalDev {
        source_path: PathBuf,
    },
    Builtin,
    Installed {
        artifact_path: PathBuf,
    },
}

impl SkillSource {
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Published {
                publisher,
                registry_url,
            } => format!("published ({publisher} via {registry_url})"),
            Self::LocalDev { source_path } => {
                format!("local_dev ({})", source_path.display())
            }
            Self::Builtin => "builtin".to_string(),
            Self::Installed { artifact_path } => {
                format!("installed ({})", artifact_path.display())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum SignatureStatus {
    Valid { signer: String },
    Invalid,
    Unsigned,
}

impl SignatureStatus {
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Valid { signer } => format!("valid ({signer})"),
            Self::Invalid => "invalid".to_string(),
            Self::Unsigned => "unsigned".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillRevision {
    pub content_hash: String,
    pub manifest_hash: String,
    pub version: String,
    pub signature: SignatureStatus,
    pub tool_contracts: Vec<ToolDefinition>,
    pub staged_at: u64,
}

impl SkillRevision {
    #[must_use]
    pub fn revision_hash(&self) -> String {
        hash_string(&format!(
            "{}:{}:{}",
            self.content_hash,
            self.manifest_hash,
            self.signature.display()
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillActivation {
    pub revision: SkillRevision,
    pub source: SkillSource,
    pub activated_at: u64,
    pub previous: Option<Box<SkillRevision>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDrift {
    pub source_manifest_hash: String,
    pub active_manifest_hash: String,
}

impl fmt::Display for SourceDrift {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "source manifest drift (source={}, active={})",
            self.source_manifest_hash, self.active_manifest_hash
        )
    }
}

#[derive(Debug, Clone)]
pub struct SkillStatusSummary {
    pub name: String,
    pub description: String,
    pub tool_names: Vec<String>,
    pub capabilities: Vec<String>,
    pub activation: SkillActivation,
    pub source_drift: Option<SourceDrift>,
}

#[derive(Clone)]
pub struct SkillLifecycleConfig {
    pub skills_dir: PathBuf,
    pub registry: Arc<SkillRegistry>,
    pub credential_provider: Option<Arc<dyn CredentialProvider>>,
    pub signature_policy: SignaturePolicy,
}

struct StagedSkill {
    skill: Arc<dyn Skill>,
    revision: SkillRevision,
    revision_dir: PathBuf,
    source: SkillSource,
}

struct ActiveSkill {
    activation: SkillActivation,
}

pub struct SkillLifecycleManager {
    skills_dir: PathBuf,
    registry: Arc<SkillRegistry>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    signature_policy: SignaturePolicy,
    staged: HashMap<String, StagedSkill>,
    active: HashMap<String, ActiveSkill>,
}

impl SkillLifecycleManager {
    #[must_use]
    pub fn new(config: SkillLifecycleConfig) -> Self {
        Self {
            skills_dir: config.skills_dir,
            registry: config.registry,
            credential_provider: config.credential_provider,
            signature_policy: config.signature_policy,
            staged: HashMap::new(),
            active: HashMap::new(),
        }
    }

    pub fn load_startup_skills(&mut self) -> Result<(), LifecycleError> {
        for skill_dir in skill_source_dirs(&self.skills_dir)? {
            self.load_startup_skill(&skill_dir)?;
        }
        Ok(())
    }

    fn load_startup_skill(&mut self, skill_dir: &Path) -> Result<(), LifecycleError> {
        let skill_name = skill_dir_name(skill_dir)?;
        if let Some(active) = self.load_existing_activation(skill_dir)? {
            self.log_loaded_activation(&skill_name, &active.activation);
            self.active.insert(skill_name.clone(), active);
        }
        self.reconcile_startup_skill(skill_dir, &skill_name)
    }

    fn load_existing_activation(
        &self,
        skill_dir: &Path,
    ) -> Result<Option<ActiveSkill>, LifecycleError> {
        let skill_name = skill_dir_name(skill_dir)?;
        let Some(activation) = read_activation_record(&self.skills_dir, &skill_name)? else {
            return Ok(None);
        };
        let revision_dir =
            existing_revision_dir(&self.skills_dir, &skill_name, &activation.revision);
        let staged = load_revision_skill(
            &revision_dir,
            activation.source.clone(),
            self.credential_provider.clone(),
            &self.signature_policy,
        )?;
        self.registry
            .upsert_with_activation(skill_name.as_str(), staged.skill, activation.clone());
        Ok(Some(ActiveSkill { activation }))
    }

    fn reconcile_startup_skill(
        &mut self,
        skill_dir: &Path,
        skill_name: &str,
    ) -> Result<(), LifecycleError> {
        match self.stage_from_source(skill_dir) {
            Ok(_) => {
                let _ = self.activate(skill_name)?;
                Ok(())
            }
            Err(error) if self.active.contains_key(skill_name) => {
                tracing::warn!(
                    skill = %skill_name,
                    error = %error,
                    "failed to stage installed artifact on startup; continuing with persisted activation"
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub fn stage_from_source(&mut self, skill_dir: &Path) -> Result<SkillRevision, LifecycleError> {
        let source = read_source_metadata(skill_dir)?.unwrap_or_else(|| SkillSource::Installed {
            artifact_path: skill_dir.to_path_buf(),
        });
        let staged = load_source_skill(
            skill_dir,
            &self.skills_dir,
            source,
            self.credential_provider.clone(),
            &self.signature_policy,
        )?;
        self.persist_revision_snapshot(skill_dir_name(skill_dir)?.as_str(), &staged)?;
        let revision = staged.revision.clone();
        self.staged.insert(skill_dir_name(skill_dir)?, staged);
        Ok(revision)
    }

    pub fn activate(&mut self, name: &str) -> Result<bool, LifecycleError> {
        let Some(staged) = self.staged.remove(name) else {
            return Err(format!("no staged revision for skill '{name}'"));
        };
        ensure_signature_gate(&staged.source, &staged.revision.signature)?;
        if self.active_matches(name, &staged) {
            return Ok(false);
        }
        let activation = self.build_activation(name, &staged);
        self.registry
            .upsert_with_activation(name, Arc::clone(&staged.skill), activation.clone());
        write_activation_record(&self.skills_dir, name, &activation)?;
        self.log_loaded_activation(name, &activation);
        self.active
            .insert(name.to_string(), ActiveSkill { activation });
        Ok(true)
    }

    pub fn rollback(&mut self, name: &str) -> Result<bool, LifecycleError> {
        let Some(current) = self.active.get(name) else {
            return Err(format!("skill '{name}' has no active revision"));
        };
        let Some(previous) = current.activation.previous.clone() else {
            return Err(format!("skill '{name}' has no previous revision"));
        };
        let previous_dir = existing_revision_dir(&self.skills_dir, name, previous.as_ref());
        let source = read_revision_source(&previous_dir)?;
        let staged = load_revision_skill(
            &previous_dir,
            source,
            self.credential_provider.clone(),
            &self.signature_policy,
        )?;
        self.staged.insert(name.to_string(), staged);
        self.activate(name)
    }

    pub fn remove_skill(&mut self, name: &str) -> Result<bool, LifecycleError> {
        let removed = self.registry.remove_skill(name).is_some();
        self.active.remove(name);
        self.staged.remove(name);
        remove_lifecycle_skill_dir(&self.skills_dir, name)?;
        Ok(removed)
    }

    #[must_use]
    pub fn active(&self, name: &str) -> Option<&SkillActivation> {
        self.active.get(name).map(|entry| &entry.activation)
    }

    #[must_use]
    pub fn statuses(&self) -> Vec<SkillStatusSummary> {
        self.registry
            .skill_statuses()
            .into_iter()
            .map(|status| SkillStatusSummary {
                source_drift: detect_source_drift(&status.activation).ok().flatten(),
                ..status
            })
            .collect()
    }

    fn active_matches(&self, name: &str, staged: &StagedSkill) -> bool {
        self.active.get(name).is_some_and(|active| {
            active.activation.revision.revision_hash() == staged.revision.revision_hash()
                && active.activation.source == staged.source
        })
    }

    fn build_activation(&self, name: &str, staged: &StagedSkill) -> SkillActivation {
        let previous = self
            .active
            .get(name)
            .map(|active| Box::new(active.activation.revision.clone()));
        SkillActivation {
            revision: staged.revision.clone(),
            source: staged.source.clone(),
            activated_at: current_time_millis(),
            previous,
        }
    }

    fn persist_revision_snapshot(
        &self,
        _name: &str,
        staged: &StagedSkill,
    ) -> Result<(), LifecycleError> {
        fs::create_dir_all(&staged.revision_dir)
            .map_err(|error| format!("failed to create revision dir: {error}"))?;
        write_json(&staged.revision_dir.join(SOURCE_FILE), &staged.source)?;
        Ok(())
    }

    fn log_loaded_activation(&self, name: &str, activation: &SkillActivation) {
        tracing::info!(
            skill = %name,
            source = %activation.source.display(),
            version = %activation.revision.version,
            revision = %short_hash(&activation.revision.revision_hash()),
            signature = %activation.revision.signature.display(),
            "loaded active skill revision"
        );
        if let Ok(Some(drift)) = detect_source_drift(activation) {
            tracing::warn!(skill = %name, drift = %drift, "active skill source is stale");
        }
    }
}

pub fn read_statuses(skills_dir: &Path) -> Result<Vec<SkillStatusSummary>, LifecycleError> {
    let mut statuses = Vec::new();
    for skill_dir in skill_source_dirs(skills_dir)? {
        let name = skill_dir_name(&skill_dir)?;
        let Some(activation) = read_activation_record(skills_dir, &name)? else {
            continue;
        };
        let manifest = crate::wasm_skill::read_manifest(&skill_dir)?;
        let tool_names = activation
            .revision
            .tool_contracts
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        statuses.push(SkillStatusSummary {
            name,
            description: manifest.description,
            tool_names,
            capabilities: manifest
                .capabilities
                .iter()
                .map(ToString::to_string)
                .collect(),
            source_drift: detect_source_drift(&activation)?,
            activation,
        });
    }
    statuses.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(statuses)
}

pub fn read_activation_record(
    skills_dir: &Path,
    skill_name: &str,
) -> Result<Option<SkillActivation>, LifecycleError> {
    let path = activation_path(skills_dir, skill_name);
    read_json_if_exists(&path)
}

pub fn write_activation_record(
    skills_dir: &Path,
    skill_name: &str,
    activation: &SkillActivation,
) -> Result<(), LifecycleError> {
    let path = activation_path(skills_dir, skill_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create lifecycle dir: {error}"))?;
    }
    write_json(&path, activation)
}

pub fn read_source_metadata(skill_dir: &Path) -> Result<Option<SkillSource>, LifecycleError> {
    read_json_if_exists(&skill_dir.join(SOURCE_METADATA_FILE))
}

pub fn write_source_metadata(skill_dir: &Path, source: &SkillSource) -> Result<(), LifecycleError> {
    write_json(&skill_dir.join(SOURCE_METADATA_FILE), source)
}

pub fn revision_snapshot_dir(
    skills_dir: &Path,
    skill_name: &str,
    revision: &SkillRevision,
) -> PathBuf {
    lifecycle_skill_dir(skills_dir, skill_name)
        .join(REVISIONS_DIR)
        .join(revision.revision_hash())
}

#[must_use]
pub fn find_revision_snapshot_dir(
    skills_dir: &Path,
    skill_name: &str,
    revision: &SkillRevision,
) -> Option<PathBuf> {
    let current = revision_snapshot_dir(skills_dir, skill_name, revision);
    if current.exists() {
        return Some(current);
    }
    let legacy = legacy_revision_snapshot_dir(skills_dir, skill_name, revision);
    legacy.exists().then_some(legacy)
}

pub fn read_revision_source_metadata(revision_dir: &Path) -> Result<SkillSource, LifecycleError> {
    read_revision_source(revision_dir)
}

pub fn detect_source_drift(
    activation: &SkillActivation,
) -> Result<Option<SourceDrift>, LifecycleError> {
    let Some(source_path) = activation_source_path(&activation.source) else {
        return Ok(None);
    };
    let manifest_path = source_path.join("manifest.toml");
    if !manifest_path.exists() {
        return Ok(None);
    }
    let source_manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let source_manifest_hash = hash_string(&source_manifest);
    if source_manifest_hash == activation.revision.manifest_hash {
        return Ok(None);
    }
    Ok(Some(SourceDrift {
        source_manifest_hash,
        active_manifest_hash: activation.revision.manifest_hash.clone(),
    }))
}

pub fn builtin_activation(skill: &dyn Skill) -> SkillActivation {
    let revision = builtin_revision(skill);
    SkillActivation {
        revision,
        source: SkillSource::Builtin,
        activated_at: current_time_millis(),
        previous: None,
    }
}

pub fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

pub fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

pub fn format_revision_timestamp(timestamp_ms: u64) -> String {
    timestamp_ms.to_string()
}

fn load_source_skill(
    skill_dir: &Path,
    skills_dir: &Path,
    source: SkillSource,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    signature_policy: &SignaturePolicy,
) -> Result<StagedSkill, LifecycleError> {
    let artifact = load_wasm_artifact_from_dir(skill_dir, credential_provider, signature_policy)?;
    let name = artifact.skill.name().to_string();
    let revision_dir = revision_snapshot_dir(skills_dir, &name, &artifact.revision);
    persist_artifact_files(&revision_dir, &name, &artifact)?;
    Ok(StagedSkill {
        skill: Arc::new(artifact.skill),
        revision: artifact.revision,
        revision_dir,
        source,
    })
}

fn load_revision_skill(
    revision_dir: &Path,
    source: SkillSource,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    signature_policy: &SignaturePolicy,
) -> Result<StagedSkill, LifecycleError> {
    let artifact =
        load_wasm_artifact_from_dir(revision_dir, credential_provider, signature_policy)?;
    Ok(StagedSkill {
        skill: Arc::new(artifact.skill),
        revision: artifact.revision,
        revision_dir: revision_dir.to_path_buf(),
        source,
    })
}

fn persist_artifact_files(
    revision_dir: &Path,
    skill_name: &str,
    artifact: &LoadedWasmArtifact,
) -> Result<(), LifecycleError> {
    fs::create_dir_all(revision_dir)
        .map_err(|error| format!("failed to create revision dir: {error}"))?;
    fs::write(revision_dir.join("manifest.toml"), &artifact.manifest_toml)
        .map_err(|error| format!("failed to persist manifest: {error}"))?;
    fs::write(
        revision_dir.join(format!("{skill_name}.wasm")),
        &artifact.wasm_bytes,
    )
    .map_err(|error| format!("failed to persist wasm: {error}"))?;
    if let Some(signature) = &artifact.signature_bytes {
        fs::write(
            revision_dir.join(format!("{skill_name}.wasm.sig")),
            signature,
        )
        .map_err(|error| format!("failed to persist signature: {error}"))?;
    }
    Ok(())
}

fn ensure_signature_gate(
    source: &SkillSource,
    signature: &SignatureStatus,
) -> Result<(), LifecycleError> {
    if matches!(source, SkillSource::Published { .. })
        && !matches!(signature, SignatureStatus::Valid { .. })
    {
        return Err("published skills require a valid signature before activation".to_string());
    }
    Ok(())
}

fn builtin_revision(skill: &dyn Skill) -> SkillRevision {
    let serialized = serde_json::json!({
        "name": skill.name(),
        "description": skill.description(),
        "capabilities": skill.capabilities(),
        "tools": skill.tool_definitions(),
    });
    let hash = hash_string(&serialized.to_string());
    SkillRevision {
        content_hash: hash.clone(),
        manifest_hash: hash,
        version: "builtin".to_string(),
        signature: SignatureStatus::Unsigned,
        tool_contracts: skill.tool_definitions(),
        staged_at: current_time_millis(),
    }
}

fn activation_source_path(source: &SkillSource) -> Option<PathBuf> {
    match source {
        SkillSource::LocalDev { source_path } => Some(source_path.clone()),
        SkillSource::Installed { artifact_path } => Some(artifact_path.clone()),
        SkillSource::Published { .. } | SkillSource::Builtin => None,
    }
}

fn read_revision_source(revision_dir: &Path) -> Result<SkillSource, LifecycleError> {
    let path = revision_dir.join(SOURCE_FILE);
    read_json(&path)
}

fn skill_source_dirs(skills_dir: &Path) -> Result<Vec<PathBuf>, LifecycleError> {
    let entries = match fs::read_dir(skills_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "failed to read skills directory {}: {error}",
                skills_dir.display()
            ))
        }
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(error = %error, "failed to read skill directory entry");
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() && !is_lifecycle_dir(&path) {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn is_lifecycle_dir(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some(LIFECYCLE_DIR)
}

fn existing_revision_dir(skills_dir: &Path, skill_name: &str, revision: &SkillRevision) -> PathBuf {
    find_revision_snapshot_dir(skills_dir, skill_name, revision)
        .unwrap_or_else(|| revision_snapshot_dir(skills_dir, skill_name, revision))
}

fn legacy_revision_snapshot_dir(
    skills_dir: &Path,
    skill_name: &str,
    revision: &SkillRevision,
) -> PathBuf {
    lifecycle_skill_dir(skills_dir, skill_name)
        .join(REVISIONS_DIR)
        .join(revision.content_hash.clone())
}

fn activation_path(skills_dir: &Path, skill_name: &str) -> PathBuf {
    lifecycle_skill_dir(skills_dir, skill_name).join(ACTIVATION_FILE)
}

fn lifecycle_skill_dir(skills_dir: &Path, skill_name: &str) -> PathBuf {
    skills_dir.join(LIFECYCLE_DIR).join(skill_name)
}

fn remove_lifecycle_skill_dir(skills_dir: &Path, skill_name: &str) -> Result<(), LifecycleError> {
    let path = lifecycle_skill_dir(skills_dir, skill_name);
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to remove {}: {error}", path.display())),
    }
}

fn skill_dir_name(skill_dir: &Path) -> Result<String, LifecycleError> {
    skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string)
        .ok_or_else(|| format!("invalid skill directory: {}", skill_dir.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), LifecycleError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("failed to serialize {}: {error}", path.display()))?;
    fs::write(path, json).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, LifecycleError> {
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn read_json_if_exists<T: for<'de> Deserialize<'de>>(
    path: &Path,
) -> Result<Option<T>, LifecycleError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| format!("failed to parse {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to read {}: {error}", path.display())),
    }
}

pub(crate) fn hash_string(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    encode_hex(&hasher.finalize())
}

fn encode_hex(bytes: &[u8]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        invocable_wasm_bytes, test_manifest_toml, versioned_manifest_toml, write_test_skill,
        write_versioned_test_skill,
    };
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn new_manager(skills_dir: &Path) -> SkillLifecycleManager {
        SkillLifecycleManager::new(SkillLifecycleConfig {
            skills_dir: skills_dir.to_path_buf(),
            registry: Arc::new(SkillRegistry::new()),
            credential_provider: None,
            signature_policy: SignaturePolicy::default(),
        })
    }

    #[test]
    fn hash_string_is_deterministic() {
        assert_eq!(hash_string("abc"), hash_string("abc"));
        assert_ne!(hash_string("abc"), hash_string("def"));
    }

    #[test]
    fn detect_source_drift_reports_manifest_mismatch() {
        let tmp = TempDir::new().expect("tempdir");
        let source = tmp.path().join("weather");
        fs::create_dir_all(&source).expect("create source");
        fs::write(
            source.join("manifest.toml"),
            versioned_manifest_toml("weather", "2.0.0"),
        )
        .expect("write manifest");
        let activation = SkillActivation {
            revision: SkillRevision {
                content_hash: hash_string("wasm"),
                manifest_hash: hash_string(&test_manifest_toml("weather")),
                version: "1.0.0".to_string(),
                signature: SignatureStatus::Unsigned,
                tool_contracts: Vec::new(),
                staged_at: 1,
            },
            source: SkillSource::LocalDev {
                source_path: source.clone(),
            },
            activated_at: 2,
            previous: None,
        };

        let drift = detect_source_drift(&activation)
            .expect("detect")
            .expect("expected drift");

        assert_ne!(drift.source_manifest_hash, drift.active_manifest_hash);
    }

    #[test]
    fn source_metadata_round_trips() {
        let tmp = TempDir::new().expect("tempdir");
        let skill_dir = tmp.path().join("weather");
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        let source = SkillSource::LocalDev {
            source_path: PathBuf::from("/tmp/weather-src"),
        };

        write_source_metadata(&skill_dir, &source).expect("write metadata");
        let loaded = read_source_metadata(&skill_dir)
            .expect("read metadata")
            .expect("expected metadata");

        assert_eq!(loaded, source);
    }

    #[test]
    fn revision_dir_uses_content_hash() {
        let revision = SkillRevision {
            content_hash: hash_string("content"),
            manifest_hash: hash_string("manifest"),
            version: "1.0.0".to_string(),
            signature: SignatureStatus::Unsigned,
            tool_contracts: Vec::new(),
            staged_at: 10,
        };

        let path = revision_snapshot_dir(Path::new("/tmp/skills"), "weather", &revision);
        assert!(path.ends_with(revision.revision_hash()));
    }

    #[test]
    fn revision_snapshot_dir_changes_when_manifest_changes() {
        let original = SkillRevision {
            content_hash: hash_string("content"),
            manifest_hash: hash_string("manifest-a"),
            version: "1.0.0".to_string(),
            signature: SignatureStatus::Unsigned,
            tool_contracts: Vec::new(),
            staged_at: 10,
        };
        let updated = SkillRevision {
            manifest_hash: hash_string("manifest-b"),
            ..original.clone()
        };

        assert_ne!(original.revision_hash(), updated.revision_hash());
        assert_ne!(
            revision_snapshot_dir(Path::new("/tmp/skills"), "weather", &original),
            revision_snapshot_dir(Path::new("/tmp/skills"), "weather", &updated)
        );
    }

    #[test]
    fn find_revision_snapshot_dir_supports_legacy_content_hash_paths() {
        let tmp = TempDir::new().expect("tempdir");
        let revision = SkillRevision {
            content_hash: hash_string("content"),
            manifest_hash: hash_string("manifest"),
            version: "1.0.0".to_string(),
            signature: SignatureStatus::Unsigned,
            tool_contracts: Vec::new(),
            staged_at: 10,
        };
        let legacy_dir = tmp
            .path()
            .join(".fawx-lifecycle")
            .join("weather")
            .join("revisions")
            .join(revision.content_hash.clone());
        fs::create_dir_all(&legacy_dir).expect("create legacy dir");

        let found = find_revision_snapshot_dir(tmp.path(), "weather", &revision)
            .expect("expected legacy revision dir");

        assert_eq!(found, legacy_dir);
    }

    #[test]
    fn persist_artifact_files_writes_manifest_wasm_and_signature() {
        let tmp = TempDir::new().expect("tempdir");
        let revision_dir = tmp.path().join("rev");
        let artifact = LoadedWasmArtifact {
            skill: crate::wasm_skill::WasmSkill::new(
                fx_skills::loader::SkillLoader::new(vec![])
                    .load(
                        &invocable_wasm_bytes(),
                        &fx_skills::manifest::parse_manifest(&test_manifest_toml("weather"))
                            .expect("manifest"),
                        None,
                    )
                    .expect("load"),
                None,
            )
            .expect("skill"),
            revision: SkillRevision {
                content_hash: hash_string("content"),
                manifest_hash: hash_string("manifest"),
                version: "1.0.0".to_string(),
                signature: SignatureStatus::Unsigned,
                tool_contracts: Vec::new(),
                staged_at: 10,
            },
            manifest_toml: test_manifest_toml("weather"),
            wasm_bytes: invocable_wasm_bytes(),
            signature_bytes: Some(vec![1, 2, 3]),
        };

        persist_artifact_files(&revision_dir, "weather", &artifact).expect("persist");

        assert!(revision_dir.join("manifest.toml").exists());
        assert!(revision_dir.join("weather.wasm").exists());
        assert!(revision_dir.join("weather.wasm.sig").exists());
    }

    #[test]
    fn load_startup_skills_reconciles_offline_installed_updates() {
        let tmp = TempDir::new().expect("tempdir");
        write_test_skill(tmp.path(), "weather").expect("write initial skill");

        let initial = {
            let mut manager = new_manager(tmp.path());
            manager.load_startup_skills().expect("initial startup");
            manager
                .active("weather")
                .cloned()
                .expect("initial activation")
        };

        fs::write(
            tmp.path().join("weather").join("manifest.toml"),
            versioned_manifest_toml("weather", "2.0.0"),
        )
        .expect("write updated manifest");

        let mut restarted = new_manager(tmp.path());
        restarted.load_startup_skills().expect("restarted startup");
        let active = restarted.active("weather").expect("reconciled activation");

        assert_eq!(active.revision.version, "2.0.0");
        assert_ne!(
            active.revision.manifest_hash,
            initial.revision.manifest_hash
        );
    }

    #[test]
    fn startup_reconciliation_preserves_lifecycle_metadata_after_offline_update() {
        let tmp = TempDir::new().expect("tempdir");
        write_test_skill(tmp.path(), "weather").expect("write initial skill");

        let initial = {
            let mut manager = new_manager(tmp.path());
            manager.load_startup_skills().expect("initial startup");
            manager
                .active("weather")
                .cloned()
                .expect("initial activation")
        };

        fs::write(
            tmp.path().join("weather").join("manifest.toml"),
            versioned_manifest_toml("weather", "2.0.0"),
        )
        .expect("write updated manifest");

        let mut restarted = new_manager(tmp.path());
        restarted.load_startup_skills().expect("restarted startup");
        let active = restarted
            .active("weather")
            .cloned()
            .expect("active weather");
        let persisted = read_activation_record(tmp.path(), "weather")
            .expect("read activation")
            .expect("persisted activation");

        assert_eq!(
            active.source,
            SkillSource::Installed {
                artifact_path: tmp.path().join("weather"),
            }
        );
        assert_eq!(active.previous.as_deref(), Some(&initial.revision));
        assert_eq!(persisted, active);
    }

    #[test]
    fn rollback_restores_previous_revision_after_offline_startup_reconciliation() {
        let tmp = TempDir::new().expect("tempdir");
        write_versioned_test_skill(tmp.path(), "weather", "1.0.0").expect("write initial skill");

        let initial = {
            let mut manager = new_manager(tmp.path());
            manager.load_startup_skills().expect("initial startup");
            manager
                .active("weather")
                .cloned()
                .expect("initial activation")
        };

        fs::write(
            tmp.path().join("weather").join("manifest.toml"),
            versioned_manifest_toml("weather", "2.0.0"),
        )
        .expect("write updated manifest");

        let mut restarted = new_manager(tmp.path());
        restarted.load_startup_skills().expect("restarted startup");
        assert!(restarted.rollback("weather").expect("rollback result"));

        let rolled_back = restarted.active("weather").expect("rolled back activation");
        assert_eq!(rolled_back.revision.version, "1.0.0");
        assert_eq!(
            rolled_back.revision.revision_hash(),
            initial.revision.revision_hash()
        );
    }
}
