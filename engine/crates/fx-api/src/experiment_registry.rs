use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

const MAX_ID_ATTEMPTS: usize = 1000;
const RESTART_INTERRUPTION_ERROR: &str = "interrupted by server restart";

type RunningExperiments = Arc<Mutex<HashMap<String, CancellationToken>>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Experiment {
    pub id: String,
    pub name: String,
    pub kind: ExperimentKind,
    pub status: ExperimentStatus,
    pub config: ExperimentConfig,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub fleet_nodes: Vec<String>,
    pub progress: Option<ExperimentProgress>,
    pub result: Option<ExperimentResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentKind {
    ProofOfFitness,
    AnalysisOnly,
    Tournament,
}

impl ExperimentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProofOfFitness => "proof_of_fitness",
            Self::AnalysisOnly => "analysis_only",
            Self::Tournament => "tournament",
        }
    }
}

impl Display for ExperimentKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ExperimentKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "proof_of_fitness" => Ok(Self::ProofOfFitness),
            "analysis_only" => Ok(Self::AnalysisOnly),
            "tournament" => Ok(Self::Tournament),
            _ => {
                Err("kind must be one of: proof_of_fitness, analysis_only, tournament".to_string())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
    Queued,
    Running,
    Completed,
    Stopped,
    Failed,
}

impl ExperimentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

impl Display for ExperimentStatus {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ExperimentStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "stopped" => Ok(Self::Stopped),
            "failed" => Ok(Self::Failed),
            _ => Err(
                "status must be one of: queued, running, completed, stopped, failed".to_string(),
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExperimentConfig {
    #[serde(default = "default_population")]
    pub population: usize,
    #[serde(default = "default_rounds")]
    pub rounds: usize,
    #[serde(default)]
    pub min_confidence: Option<String>,
    #[serde(default)]
    pub output_mode: Option<String>,
}

fn default_population() -> usize {
    16
}

fn default_rounds() -> usize {
    4
}

impl Default for ExperimentConfig {
    fn default() -> Self {
        Self {
            population: default_population(),
            rounds: default_rounds(),
            min_confidence: None,
            output_mode: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExperimentProgress {
    pub completed_matches: usize,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExperimentResult {
    pub plans_generated: usize,
    pub proposals_written: Vec<String>,
    pub branches_created: Vec<String>,
    #[serde(default)]
    pub score_summary: Option<String>,
    pub skipped: Vec<SkippedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkippedItem {
    pub name: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct ExperimentRegistry {
    experiments: HashMap<String, Experiment>,
    data_dir: PathBuf,
    running_experiments: RunningExperiments,
}

impl ExperimentRegistry {
    pub fn new(data_dir: &Path) -> Result<Self, io::Error> {
        create_storage_dir(data_dir)?;
        let mut experiments = load_experiments(data_dir)?;
        let recovered = recover_interrupted_experiments(&mut experiments);
        let registry = Self {
            experiments,
            data_dir: data_dir.to_path_buf(),
            running_experiments: Arc::new(Mutex::new(HashMap::new())),
        };
        if recovered {
            registry.persist()?;
        }
        Ok(registry)
    }

    pub fn create(
        &mut self,
        name: String,
        kind: ExperimentKind,
        config: ExperimentConfig,
    ) -> Result<Experiment, String> {
        let experiment = new_experiment(&self.experiments, name, kind, config)?;
        self.experiments
            .insert(experiment.id.clone(), experiment.clone());
        self.persist().map_err(persistence_error)?;
        Ok(experiment)
    }

    pub fn get(&self, id: &str) -> Option<&Experiment> {
        self.experiments.get(id)
    }

    pub fn list(&self) -> Vec<&Experiment> {
        sorted_experiments(self.experiments.values())
    }

    pub fn list_by_status(&self, status: ExperimentStatus) -> Vec<&Experiment> {
        sorted_experiments(
            self.experiments
                .values()
                .filter(|experiment| experiment.status == status),
        )
    }

    pub fn has_running_experiment(&self) -> bool {
        self.experiments
            .values()
            .any(|experiment| experiment.status == ExperimentStatus::Running)
    }

    pub fn cancel_token(&self, id: &str) -> Option<CancellationToken> {
        with_running_experiments(&self.running_experiments, |running| {
            running.get(id).cloned()
        })
        .ok()
        .flatten()
    }

    pub fn start(&mut self, id: &str) -> Result<(), String> {
        self.update_experiment(id, |experiment| {
            require_status(experiment.status, ExperimentStatus::Queued, "queued")?;
            experiment.status = ExperimentStatus::Running;
            experiment.started_at = Some(current_timestamp());
            Ok(())
        })?;
        self.track_running_experiment(id)
    }

    pub fn complete(&mut self, id: &str, result: ExperimentResult) -> Result<(), String> {
        self.update_experiment(id, |experiment| {
            require_status(experiment.status, ExperimentStatus::Running, "running")?;
            experiment.status = ExperimentStatus::Completed;
            experiment.completed_at = Some(current_timestamp());
            experiment.progress = None;
            experiment.result = Some(result);
            experiment.error = None;
            Ok(())
        })?;
        self.finish_running_experiment(id, false)
    }

    pub fn fail(&mut self, id: &str, error: String) -> Result<(), String> {
        self.update_experiment(id, |experiment| {
            require_status(experiment.status, ExperimentStatus::Running, "running")?;
            experiment.status = ExperimentStatus::Failed;
            experiment.completed_at = Some(current_timestamp());
            experiment.progress = None;
            experiment.result = None;
            experiment.error = Some(error);
            Ok(())
        })?;
        self.finish_running_experiment(id, false)
    }

    pub fn stop(&mut self, id: &str) -> Result<(), String> {
        let status = self
            .experiments
            .get(id)
            .map(|experiment| experiment.status)
            .ok_or_else(|| experiment_not_found(id))?;
        require_stoppable(status)?;
        self.update_experiment(id, |experiment| {
            experiment.status = ExperimentStatus::Stopped;
            experiment.completed_at = Some(current_timestamp());
            experiment.progress = None;
            experiment.result = None;
            experiment.error = None;
            Ok(())
        })?;
        self.finish_running_experiment(id, status == ExperimentStatus::Running)
    }

    pub fn update_progress(
        &mut self,
        id: &str,
        progress: ExperimentProgress,
    ) -> Result<(), String> {
        self.update_experiment(id, |experiment| {
            require_status(experiment.status, ExperimentStatus::Running, "running")?;
            experiment.progress = Some(progress);
            Ok(())
        })
    }

    pub fn persist(&self) -> Result<(), io::Error> {
        let content = serde_json::to_vec_pretty(&self.experiments).map_err(invalid_data_error)?;
        let path = experiments_file(&self.data_dir);
        let tmp_path = tmp_path_for(&path);
        fs::write(&tmp_path, content)?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    fn update_experiment<F>(&mut self, id: &str, update: F) -> Result<(), String>
    where
        F: FnOnce(&mut Experiment) -> Result<(), String>,
    {
        {
            let experiment = self
                .experiments
                .get_mut(id)
                .ok_or_else(|| experiment_not_found(id))?;
            update(experiment)?;
        }
        self.persist().map_err(persistence_error)
    }

    fn track_running_experiment(&self, id: &str) -> Result<(), String> {
        with_running_experiments(&self.running_experiments, |running| {
            running.insert(id.to_string(), CancellationToken::new());
        })
    }

    fn finish_running_experiment(&self, id: &str, cancel: bool) -> Result<(), String> {
        let token =
            with_running_experiments(&self.running_experiments, |running| running.remove(id))?;
        if cancel {
            if let Some(token) = token {
                token.cancel();
            }
        }
        Ok(())
    }
}

fn create_storage_dir(data_dir: &Path) -> Result<(), io::Error> {
    fs::create_dir_all(experiments_dir(data_dir))
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn experiments_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("experiments")
}

fn experiments_file(data_dir: &Path) -> PathBuf {
    experiments_dir(data_dir).join("experiments.json")
}

fn experiment_not_found(id: &str) -> String {
    format!("Experiment '{id}' not found")
}

fn invalid_data_error(error: impl Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn load_experiments(data_dir: &Path) -> Result<HashMap<String, Experiment>, io::Error> {
    let path = experiments_file(data_dir);
    if !path.is_file() {
        return Ok(HashMap::new());
    }

    let content = fs::read(path)?;
    serde_json::from_slice(&content).map_err(invalid_data_error)
}

fn new_experiment(
    existing: &HashMap<String, Experiment>,
    name: String,
    kind: ExperimentKind,
    config: ExperimentConfig,
) -> Result<Experiment, String> {
    Ok(Experiment {
        id: generate_id(existing)?,
        name,
        kind,
        status: ExperimentStatus::Queued,
        config,
        created_at: current_timestamp(),
        started_at: None,
        completed_at: None,
        fleet_nodes: Vec::new(),
        progress: None,
        result: None,
        error: None,
    })
}

fn generate_id(existing: &HashMap<String, Experiment>) -> Result<String, String> {
    let mut random = rand::thread_rng();
    generate_id_with(existing, || {
        format!("exp_{}_{:04x}", current_timestamp(), random.gen::<u16>())
    })
}

fn generate_id_with<F>(
    existing: &HashMap<String, Experiment>,
    mut next_id: F,
) -> Result<String, String>
where
    F: FnMut() -> String,
{
    for _ in 0..MAX_ID_ATTEMPTS {
        let id = next_id();
        if !existing.contains_key(&id) {
            return Ok(id);
        }
    }

    Err(format!(
        "failed to generate unique experiment id after {MAX_ID_ATTEMPTS} attempts"
    ))
}

fn persistence_error(error: io::Error) -> String {
    format!("failed to persist experiments: {error}")
}

fn recover_interrupted_experiments(experiments: &mut HashMap<String, Experiment>) -> bool {
    let recovered_at = current_timestamp();
    let mut recovered = false;
    for experiment in experiments.values_mut() {
        if experiment.status == ExperimentStatus::Running {
            mark_restart_interrupted(experiment, recovered_at);
            recovered = true;
        }
    }
    recovered
}

fn mark_restart_interrupted(experiment: &mut Experiment, recovered_at: u64) {
    experiment.status = ExperimentStatus::Failed;
    experiment.completed_at = Some(recovered_at);
    experiment.progress = None;
    experiment.result = None;
    experiment.error = Some(RESTART_INTERRUPTION_ERROR.to_string());
}

fn require_status(
    actual: ExperimentStatus,
    expected: ExperimentStatus,
    expected_label: &str,
) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    Err(format!(
        "experiment is not {expected_label} (status: {actual})"
    ))
}

fn require_stoppable(status: ExperimentStatus) -> Result<(), String> {
    if matches!(status, ExperimentStatus::Queued | ExperimentStatus::Running) {
        return Ok(());
    }
    Err(format!("experiment is not running (status: {status})"))
}

fn sorted_experiments<'a>(
    experiments: impl Iterator<Item = &'a Experiment>,
) -> Vec<&'a Experiment> {
    let mut items: Vec<_> = experiments.collect();
    items.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    items
}

fn tmp_path_for(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.tmp", path.display()))
}

fn with_running_experiments<T, F>(
    running_experiments: &RunningExperiments,
    update: F,
) -> Result<T, String>
where
    F: FnOnce(&mut HashMap<String, CancellationToken>) -> T,
{
    let mut running = running_experiments
        .lock()
        .map_err(|_| "running experiments mutex poisoned".to_string())?;
    Ok(update(&mut running))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_result() -> ExperimentResult {
        ExperimentResult {
            plans_generated: 2,
            proposals_written: vec!["proposal.md".to_string()],
            branches_created: vec!["feature/branch".to_string()],
            score_summary: None,
            skipped: vec![SkippedItem {
                name: "candidate".to_string(),
                reason: "not enough signal".to_string(),
            }],
        }
    }

    fn test_experiment(id: &str) -> Experiment {
        Experiment {
            id: id.to_string(),
            name: "Existing".to_string(),
            kind: ExperimentKind::ProofOfFitness,
            status: ExperimentStatus::Queued,
            config: ExperimentConfig::default(),
            created_at: current_timestamp(),
            started_at: None,
            completed_at: None,
            fleet_nodes: Vec::new(),
            progress: None,
            result: None,
            error: None,
        }
    }

    #[test]
    fn experiment_result_deserializes_without_score_summary_for_backwards_compatibility() {
        let result: ExperimentResult = serde_json::from_str(
            r#"{
                "plans_generated": 1,
                "proposals_written": ["proposal.md"],
                "branches_created": [],
                "skipped": []
            }"#,
        )
        .expect("deserialize legacy result");

        assert_eq!(result.plans_generated, 1);
        assert_eq!(result.score_summary, None);
    }

    #[test]
    fn create_persists_and_loads_from_disk() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut registry = ExperimentRegistry::new(temp_dir.path()).expect("registry");

        let created = registry
            .create(
                "Prompt tournament".to_string(),
                ExperimentKind::ProofOfFitness,
                ExperimentConfig::default(),
            )
            .expect("create");

        drop(registry);

        let loaded = ExperimentRegistry::new(temp_dir.path()).expect("reloaded registry");
        let experiment = loaded.get(&created.id).expect("stored experiment");
        assert_eq!(experiment.name, "Prompt tournament");
        assert_eq!(experiment.status, ExperimentStatus::Queued);
        assert_eq!(experiment.config, ExperimentConfig::default());
    }

    #[test]
    fn create_assigns_unique_ids() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut registry = ExperimentRegistry::new(temp_dir.path()).expect("registry");

        let first = registry
            .create(
                "First".to_string(),
                ExperimentKind::AnalysisOnly,
                ExperimentConfig::default(),
            )
            .expect("first");
        let second = registry
            .create(
                "Second".to_string(),
                ExperimentKind::Tournament,
                ExperimentConfig::default(),
            )
            .expect("second");

        assert_ne!(first.id, second.id);
    }

    #[test]
    fn list_sorts_by_created_at_descending() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut registry = ExperimentRegistry::new(temp_dir.path()).expect("registry");
        let first = registry
            .create(
                "First".to_string(),
                ExperimentKind::AnalysisOnly,
                ExperimentConfig::default(),
            )
            .expect("first");
        let second = registry
            .create(
                "Second".to_string(),
                ExperimentKind::Tournament,
                ExperimentConfig::default(),
            )
            .expect("second");

        registry
            .experiments
            .get_mut(&first.id)
            .expect("first")
            .created_at = 10;
        registry
            .experiments
            .get_mut(&second.id)
            .expect("second")
            .created_at = 20;

        let listed = registry.list();
        assert_eq!(listed[0].id, second.id);
        assert_eq!(listed[1].id, first.id);
    }

    #[test]
    fn lifecycle_transitions_update_state() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut registry = ExperimentRegistry::new(temp_dir.path()).expect("registry");
        let created = registry
            .create(
                "Lifecycle".to_string(),
                ExperimentKind::ProofOfFitness,
                ExperimentConfig::default(),
            )
            .expect("create");

        registry.start(&created.id).expect("start");
        registry
            .update_progress(
                &created.id,
                ExperimentProgress {
                    completed_matches: 2,
                    total_matches: 4,
                },
            )
            .expect("progress");
        registry
            .complete(&created.id, test_result())
            .expect("complete");

        let completed = registry.get(&created.id).expect("completed experiment");
        assert_eq!(completed.status, ExperimentStatus::Completed);
        assert!(completed.started_at.is_some());
        assert!(completed.completed_at.is_some());
        assert!(completed.progress.is_none());
        assert_eq!(completed.result.as_ref(), Some(&test_result()));
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut registry = ExperimentRegistry::new(temp_dir.path()).expect("registry");
        let created = registry
            .create(
                "Stopped".to_string(),
                ExperimentKind::Tournament,
                ExperimentConfig::default(),
            )
            .expect("create");

        registry.stop(&created.id).expect("stop queued experiment");
        let error = registry.stop(&created.id).expect_err("stop should fail");
        assert_eq!(error, "experiment is not running (status: stopped)");

        let progress_error = registry
            .update_progress(
                &created.id,
                ExperimentProgress {
                    completed_matches: 1,
                    total_matches: 2,
                },
            )
            .expect_err("progress should fail");
        assert_eq!(
            progress_error,
            "experiment is not running (status: stopped)"
        );
    }

    #[test]
    fn start_rejects_non_queued_experiments() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut registry = ExperimentRegistry::new(temp_dir.path()).expect("registry");
        let created = registry
            .create(
                "Already stopped".to_string(),
                ExperimentKind::Tournament,
                ExperimentConfig::default(),
            )
            .expect("create");

        registry.stop(&created.id).expect("stop");
        let error = registry.start(&created.id).expect_err("start should fail");
        assert_eq!(error, "experiment is not queued (status: stopped)");
    }

    #[test]
    fn stop_cancels_running_experiment_token() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut registry = ExperimentRegistry::new(temp_dir.path()).expect("registry");
        let created = registry
            .create(
                "Running".to_string(),
                ExperimentKind::ProofOfFitness,
                ExperimentConfig::default(),
            )
            .expect("create");

        registry.start(&created.id).expect("start");
        let token = registry
            .cancel_token(&created.id)
            .expect("cancellation token");

        registry.stop(&created.id).expect("stop");

        assert!(token.is_cancelled());
        assert!(registry.cancel_token(&created.id).is_none());
    }

    #[test]
    fn new_recovers_running_experiments_after_restart() {
        let temp_dir = TempDir::new().expect("tempdir");
        let id = {
            let mut registry = ExperimentRegistry::new(temp_dir.path()).expect("registry");
            let created = registry
                .create(
                    "Interrupted".to_string(),
                    ExperimentKind::ProofOfFitness,
                    ExperimentConfig::default(),
                )
                .expect("create");
            registry.start(&created.id).expect("start");
            created.id
        };

        let recovered = ExperimentRegistry::new(temp_dir.path()).expect("recovered registry");
        let experiment = recovered.get(&id).expect("recovered experiment");
        assert_eq!(experiment.status, ExperimentStatus::Failed);
        assert_eq!(
            experiment.error.as_deref(),
            Some(RESTART_INTERRUPTION_ERROR)
        );
        assert!(experiment.completed_at.is_some());

        drop(recovered);

        let persisted = ExperimentRegistry::new(temp_dir.path()).expect("persisted registry");
        let experiment = persisted.get(&id).expect("persisted experiment");
        assert_eq!(experiment.status, ExperimentStatus::Failed);
        assert_eq!(
            experiment.error.as_deref(),
            Some(RESTART_INTERRUPTION_ERROR)
        );
    }

    #[test]
    fn generate_id_rejects_after_max_attempts() {
        let id = "exp_collision".to_string();
        let existing = HashMap::from([(id.clone(), test_experiment(&id))]);

        let error = generate_id_with(&existing, || id.clone()).expect_err("should fail");
        assert_eq!(
            error,
            format!("failed to generate unique experiment id after {MAX_ID_ATTEMPTS} attempts")
        );
    }

    #[test]
    fn create_returns_persistence_error_instead_of_panicking() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut registry = ExperimentRegistry::new(temp_dir.path()).expect("registry");
        let storage_dir = experiments_dir(temp_dir.path());
        fs::remove_dir(&storage_dir).expect("remove experiments dir");
        fs::write(&storage_dir, b"not a directory").expect("replace with file");

        let error = registry
            .create(
                "Persist failure".to_string(),
                ExperimentKind::AnalysisOnly,
                ExperimentConfig::default(),
            )
            .expect_err("persist should fail");

        assert!(error.starts_with("failed to persist experiments:"));
    }
}
