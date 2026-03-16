use crate::eval::EvalResults;
use crate::format::ModelFormat;
use crate::progress::ArtifactType;
use crate::storage::atomic_write;
use crate::{CostRecord, ForgeError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub id: Uuid,
    pub name: String,
    pub artifact_type: ArtifactType,
    pub format: ModelFormat,
    pub base_model: Option<String>,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub job_id: Uuid,
    pub eval_results: Option<EvalResults>,
    pub cost: Option<CostRecord>,
    pub active: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ArtifactFilter {
    pub artifact_type: Option<ArtifactType>,
    pub format: Option<ModelFormat>,
    pub base_model: Option<String>,
    pub active_only: bool,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    schema_version: u32,
    next_id: u32,
    total_artifacts: usize,
    updated_at: DateTime<Utc>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            next_id: 1,
            total_artifacts: 0,
            updated_at: Utc::now(),
        }
    }
}

pub struct ArtifactManager {
    artifacts_dir: PathBuf,
}

impl ArtifactManager {
    pub fn new(artifacts_dir: PathBuf) -> Result<Self, ForgeError> {
        let metadata_dir = artifacts_dir.join("metadata");
        std::fs::create_dir_all(&metadata_dir)?;
        let manifest_path = artifacts_dir.join("manifest.json");
        if !manifest_path.exists() {
            write_manifest(&manifest_path, &Manifest::default())?;
        }
        Ok(Self { artifacts_dir })
    }

    pub fn register(&self, info: ArtifactInfo) -> Result<(), ForgeError> {
        let mut manifest = self.load_manifest()?;
        let filename = format!("{:08}.json", manifest.next_id);
        let path = self.metadata_dir().join(filename);
        write_artifact_info(&path, &info)?;
        manifest.next_id += 1;
        manifest.total_artifacts += 1;
        manifest.updated_at = Utc::now();
        self.save_manifest(&manifest)
    }

    pub fn list(&self, filter: Option<&ArtifactFilter>) -> Result<Vec<ArtifactInfo>, ForgeError> {
        let all = self.load_all()?;
        match filter {
            Some(filter) => Ok(apply_filter(all, filter)),
            None => Ok(all),
        }
    }

    pub fn get(&self, id: Uuid) -> Result<Option<ArtifactInfo>, ForgeError> {
        Ok(self
            .load_all()?
            .into_iter()
            .find(|artifact| artifact.id == id))
    }

    pub fn activate(&self, id: Uuid) -> Result<(), ForgeError> {
        let mut artifacts = self.load_all()?;
        let target = artifacts
            .iter()
            .find(|artifact| artifact.id == id)
            .ok_or_else(|| ForgeError::ArtifactError(format!("artifact not found: {id}")))?;
        let base_model = target.base_model.clone();
        for artifact in &mut artifacts {
            if artifact.base_model == base_model {
                artifact.active = artifact.id == id;
            }
        }
        self.save_all(&artifacts)
    }

    pub fn deactivate(&self, base_model: &str) -> Result<(), ForgeError> {
        let mut artifacts = self.load_all()?;
        for artifact in &mut artifacts {
            if artifact.base_model.as_deref() == Some(base_model) {
                artifact.active = false;
            }
        }
        self.save_all(&artifacts)
    }

    pub fn delete(&self, id: Uuid) -> Result<(), ForgeError> {
        let artifacts = self.load_all()?;
        let remaining: Vec<_> = artifacts
            .into_iter()
            .filter(|artifact| artifact.id != id)
            .collect();
        self.save_all(&remaining)?;
        let mut manifest = self.load_manifest()?;
        manifest.total_artifacts = remaining.len();
        manifest.updated_at = Utc::now();
        self.save_manifest(&manifest)
    }

    pub fn active_for_model(&self, base_model: &str) -> Result<Option<ArtifactInfo>, ForgeError> {
        Ok(self
            .load_all()?
            .into_iter()
            .find(|artifact| artifact.active && artifact.base_model.as_deref() == Some(base_model)))
    }

    fn metadata_dir(&self) -> PathBuf {
        self.artifacts_dir.join("metadata")
    }

    fn manifest_path(&self) -> PathBuf {
        self.artifacts_dir.join("manifest.json")
    }

    fn load_manifest(&self) -> Result<Manifest, ForgeError> {
        let content = std::fs::read_to_string(self.manifest_path())?;
        Ok(serde_json::from_str(&content)?)
    }

    fn save_manifest(&self, manifest: &Manifest) -> Result<(), ForgeError> {
        write_manifest(&self.manifest_path(), manifest)
    }

    fn load_all(&self) -> Result<Vec<ArtifactInfo>, ForgeError> {
        let metadata_dir = self.metadata_dir();
        let mut artifacts = Vec::new();
        if !metadata_dir.exists() {
            return Ok(artifacts);
        }
        for entry in std::fs::read_dir(&metadata_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !is_json_file(&path) {
                continue;
            }
            let content = std::fs::read_to_string(&path)?;
            let artifact = serde_json::from_str(&content)?;
            artifacts.push(artifact);
        }
        Ok(artifacts)
    }

    fn save_all(&self, artifacts: &[ArtifactInfo]) -> Result<(), ForgeError> {
        let metadata_dir = self.metadata_dir();
        write_metadata_temp_files(&metadata_dir, artifacts)?;
        replace_metadata_files(&metadata_dir, artifacts.len())
    }
}

fn replace_metadata_files(metadata_dir: &Path, artifact_count: usize) -> Result<(), ForgeError> {
    clear_metadata_files(metadata_dir)?;
    rename_metadata_temp_files(metadata_dir, artifact_count)
}

fn clear_metadata_files(metadata_dir: &Path) -> Result<(), ForgeError> {
    remove_matching_files(metadata_dir, is_json_file)
}

fn clear_temp_metadata_files(metadata_dir: &Path) -> Result<(), ForgeError> {
    remove_matching_files(metadata_dir, is_temp_metadata_file)
}

fn remove_matching_files(
    metadata_dir: &Path,
    matcher: fn(&Path) -> bool,
) -> Result<(), ForgeError> {
    if !metadata_dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(metadata_dir)? {
        let entry = entry?;
        let path = entry.path();
        if matcher(&path) {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn write_metadata_temp_files(
    metadata_dir: &Path,
    artifacts: &[ArtifactInfo],
) -> Result<(), ForgeError> {
    clear_temp_metadata_files(metadata_dir)?;
    for (index, artifact) in artifacts.iter().enumerate() {
        let filename = format!("{:08}.json.tmp", index + 1);
        write_temp_artifact_info(&metadata_dir.join(filename), artifact)?;
    }
    Ok(())
}

fn rename_metadata_temp_files(
    metadata_dir: &Path,
    artifact_count: usize,
) -> Result<(), ForgeError> {
    for index in 0..artifact_count {
        let temp_path = metadata_dir.join(format!("{:08}.json.tmp", index + 1));
        let final_path = metadata_dir.join(format!("{:08}.json", index + 1));
        std::fs::rename(temp_path, final_path)?;
    }
    Ok(())
}

fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), ForgeError> {
    let json = serde_json::to_string_pretty(manifest)?;
    atomic_write(path, &json)
}

fn write_artifact_info(path: &Path, info: &ArtifactInfo) -> Result<(), ForgeError> {
    let json = serde_json::to_string_pretty(info)?;
    atomic_write(path, &json)
}

fn write_temp_artifact_info(path: &Path, info: &ArtifactInfo) -> Result<(), ForgeError> {
    let json = serde_json::to_string_pretty(info)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn is_json_file(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("json")
}

fn is_temp_metadata_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.ends_with(".json.tmp"))
        .unwrap_or(false)
}

fn apply_filter(artifacts: Vec<ArtifactInfo>, filter: &ArtifactFilter) -> Vec<ArtifactInfo> {
    artifacts
        .into_iter()
        .filter(|artifact| matches_filter(artifact, filter))
        .collect()
}

fn matches_filter(artifact: &ArtifactInfo, filter: &ArtifactFilter) -> bool {
    if let Some(ref artifact_type) = filter.artifact_type {
        if artifact.artifact_type != *artifact_type {
            return false;
        }
    }
    if let Some(ref format) = filter.format {
        if artifact.format != *format {
            return false;
        }
    }
    if let Some(ref base_model) = filter.base_model {
        if artifact.base_model.as_deref() != Some(base_model.as_str()) {
            return false;
        }
    }
    if filter.active_only && !artifact.active {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_artifact(name: &str, base_model: &str) -> ArtifactInfo {
        ArtifactInfo {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            artifact_type: ArtifactType::LoraAdapter,
            format: ModelFormat::Safetensors,
            base_model: Some(base_model.to_owned()),
            path: PathBuf::from(format!("{name}.safetensors")),
            size_bytes: 1024,
            created_at: Utc::now(),
            job_id: Uuid::new_v4(),
            eval_results: None,
            cost: None,
            active: false,
        }
    }

    #[test]
    fn create_and_register() {
        let directory = tempfile::TempDir::new().unwrap();
        let manager = ArtifactManager::new(directory.path().join("artifacts")).unwrap();
        manager.register(sample_artifact("a1", "llama-8b")).unwrap();
        let all = manager.list(None).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn get_returns_registered_artifact() {
        let directory = tempfile::TempDir::new().unwrap();
        let manager = ArtifactManager::new(directory.path().join("artifacts")).unwrap();
        let artifact = sample_artifact("a1", "llama-8b");
        let artifact_id = artifact.id;
        manager.register(artifact).unwrap();
        let loaded = manager.get(artifact_id).unwrap();
        assert_eq!(loaded.unwrap().id, artifact_id);
    }

    #[test]
    fn activate_deactivate() {
        let directory = tempfile::TempDir::new().unwrap();
        let manager = ArtifactManager::new(directory.path().join("artifacts")).unwrap();
        let artifact = sample_artifact("a1", "llama-8b");
        let artifact_id = artifact.id;
        manager.register(artifact).unwrap();
        manager.activate(artifact_id).unwrap();
        let active = manager.active_for_model("llama-8b").unwrap();
        assert!(active.is_some());
        manager.deactivate("llama-8b").unwrap();
        let active = manager.active_for_model("llama-8b").unwrap();
        assert!(active.is_none());
    }

    #[test]
    fn delete_artifact() {
        let directory = tempfile::TempDir::new().unwrap();
        let manager = ArtifactManager::new(directory.path().join("artifacts")).unwrap();
        let artifact = sample_artifact("a1", "llama-8b");
        let artifact_id = artifact.id;
        manager.register(artifact).unwrap();
        manager.delete(artifact_id).unwrap();
        let all = manager.list(None).unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn filter_by_type() {
        let directory = tempfile::TempDir::new().unwrap();
        let manager = ArtifactManager::new(directory.path().join("artifacts")).unwrap();
        manager.register(sample_artifact("a1", "llama-8b")).unwrap();
        let filter = ArtifactFilter {
            artifact_type: Some(ArtifactType::FullModel),
            ..Default::default()
        };
        let filtered = manager.list(Some(&filter)).unwrap();
        assert!(filtered.is_empty());
    }

    #[test]
    fn filter_active_only() {
        let directory = tempfile::TempDir::new().unwrap();
        let manager = ArtifactManager::new(directory.path().join("artifacts")).unwrap();
        let artifact = sample_artifact("a1", "llama-8b");
        let artifact_id = artifact.id;
        manager.register(artifact).unwrap();
        manager.activate(artifact_id).unwrap();
        let filter = ArtifactFilter {
            active_only: true,
            ..Default::default()
        };
        let filtered = manager.list(Some(&filter)).unwrap();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn save_all_preserves_artifacts() {
        let directory = tempfile::TempDir::new().unwrap();
        let manager = ArtifactManager::new(directory.path().join("artifacts")).unwrap();
        manager.register(sample_artifact("a1", "llama-8b")).unwrap();
        manager.register(sample_artifact("a2", "llama-8b")).unwrap();

        let artifacts = manager.list(None).unwrap();
        let expected_ids: Vec<_> = artifacts.iter().map(|artifact| artifact.id).collect();

        manager.save_all(&artifacts).unwrap();

        let loaded = manager.list(None).unwrap();
        let loaded_ids: Vec<_> = loaded.iter().map(|artifact| artifact.id).collect();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded_ids.len(), expected_ids.len());
        for expected_id in expected_ids {
            assert!(loaded_ids.contains(&expected_id));
        }
        let temp_files = std::fs::read_dir(manager.metadata_dir())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| is_temp_metadata_file(path))
            .count();
        assert_eq!(temp_files, 0);
    }
}
