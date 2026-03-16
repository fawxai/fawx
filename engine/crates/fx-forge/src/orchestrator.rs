use crate::backend::{TrainBackend, TrainProgress};
use crate::error::ForgeError;
use crate::objective::TrainObjective;
use crate::{CostRecord, ForgeJob, JobStatus};
use chrono::Utc;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub type ForgeProgressCallback = Arc<dyn Fn(&ForgeEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum ForgeEvent {
    JobCreated {
        job_id: Uuid,
        objective: String,
    },
    BackendSelected {
        backend: String,
        estimated_cost_usd: Option<f64>,
    },
    TrainProgress(TrainProgress),
    TrainComplete {
        loss: Option<f64>,
        duration_secs: u64,
        cost: Option<CostRecord>,
    },
    ArtifactRegistered {
        artifact_id: Uuid,
    },
    JobComplete {
        job_id: Uuid,
    },
    JobFailed {
        job_id: Uuid,
        error: String,
    },
}

pub struct JobValidation {
    pub backend: String,
    pub estimated_cost_usd: Option<f64>,
    pub warnings: Vec<String>,
}

pub struct ForgeOrchestrator {
    backends: Vec<Box<dyn TrainBackend>>,
    jobs_dir: PathBuf,
}

impl ForgeOrchestrator {
    pub fn new(
        backends: Vec<Box<dyn TrainBackend>>,
        jobs_dir: PathBuf,
    ) -> Result<Self, ForgeError> {
        std::fs::create_dir_all(&jobs_dir)?;
        Ok(Self { backends, jobs_dir })
    }

    pub fn select_backend(
        &self,
        objective: &TrainObjective,
    ) -> Result<&dyn TrainBackend, ForgeError> {
        let required = objective_label(objective);
        self.backends
            .iter()
            .find(|backend| supports_objective(backend.capabilities(), objective))
            .map(|backend| backend.as_ref())
            .ok_or_else(|| ForgeError::NoBackendAvailable(required.to_owned()))
    }

    pub fn validate_job(&self, objective: &TrainObjective) -> Result<JobValidation, ForgeError> {
        let backend = self.select_backend(objective)?;
        backend.validate(objective)?;
        Ok(JobValidation {
            backend: backend.capabilities().name.clone(),
            estimated_cost_usd: backend.capabilities().estimated_cost_per_gpu_hour,
            warnings: Vec::new(),
        })
    }

    pub async fn create_job(
        &self,
        name: String,
        objective: TrainObjective,
    ) -> Result<ForgeJob, ForgeError> {
        objective.validate()?;
        let backend = self.select_backend(&objective)?;
        let job = ForgeJob {
            id: Uuid::new_v4(),
            name,
            status: JobStatus::Pending,
            objective,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
            backend: backend.capabilities().name.clone(),
            cost: None,
            handle: None,
        };
        self.save_job(&job)?;
        Ok(job)
    }

    pub async fn run_job(
        &self,
        job_id: Uuid,
        progress: Option<ForgeProgressCallback>,
    ) -> Result<ForgeJob, ForgeError> {
        let mut job = self.load_existing_job(job_id)?;
        let backend = self.select_backend(&job.objective)?;

        emit(
            &progress,
            ForgeEvent::JobCreated {
                job_id,
                objective: objective_label(&job.objective).to_owned(),
            },
        );

        self.start_job(&mut job)?;
        let handle = match backend.submit(&job.objective).await {
            Ok(h) => h,
            Err(e) => return self.fail_job(&mut job, &progress, e),
        };
        job.handle = Some(handle.clone());
        self.save_job(&job)?;
        emit_backend_selected(&progress, backend);

        if let Ok(train_progress) = backend.progress(&handle).await {
            emit(&progress, ForgeEvent::TrainProgress(train_progress));
        }

        let artifact = match backend.wait(&handle).await {
            Ok(a) => a,
            Err(e) => return self.fail_job(&mut job, &progress, e),
        };
        emit_train_complete(&progress, &artifact);

        self.complete_job(&mut job, artifact)?;
        emit(&progress, ForgeEvent::JobComplete { job_id });
        Ok(job)
    }

    pub fn job_status(&self, job_id: Uuid) -> Result<Option<ForgeJob>, ForgeError> {
        self.load_job(job_id)
    }

    pub fn list_jobs(&self) -> Result<Vec<ForgeJob>, ForgeError> {
        let mut jobs = Vec::new();
        for entry in std::fs::read_dir(&self.jobs_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let content = std::fs::read_to_string(&path)?;
            let job = serde_json::from_str(&content)?;
            jobs.push(job);
        }
        jobs.sort_by(|left: &ForgeJob, right: &ForgeJob| right.created_at.cmp(&left.created_at));
        Ok(jobs)
    }

    pub async fn cancel_job(&self, job_id: Uuid) -> Result<(), ForgeError> {
        let mut job = self.load_existing_job(job_id)?;
        // Attempt backend cancel if we have a handle.
        // Best-effort: if backend cancel fails, we still mark the job cancelled locally.
        if let Some(ref handle) = job.handle {
            if let Ok(backend) = self.select_backend(&job.objective) {
                let _ = backend.cancel(handle).await;
            }
        }
        job.status = JobStatus::Cancelled;
        job.completed_at = Some(Utc::now());
        self.save_job(&job)
    }

    fn fail_job(
        &self,
        job: &mut ForgeJob,
        progress: &Option<ForgeProgressCallback>,
        error: ForgeError,
    ) -> Result<ForgeJob, ForgeError> {
        job.status = JobStatus::Failed;
        job.error = Some(error.to_string());
        job.completed_at = Some(Utc::now());
        let _ = self.save_job(job);
        emit(
            progress,
            ForgeEvent::JobFailed {
                job_id: job.id,
                error: error.to_string(),
            },
        );
        Err(error)
    }

    fn start_job(&self, job: &mut ForgeJob) -> Result<(), ForgeError> {
        job.status = JobStatus::Training;
        job.started_at = Some(Utc::now());
        self.save_job(job)
    }

    fn complete_job(
        &self,
        job: &mut ForgeJob,
        artifact: crate::backend::TrainArtifact,
    ) -> Result<(), ForgeError> {
        job.status = JobStatus::Completed;
        job.completed_at = Some(Utc::now());
        job.cost = artifact.cost.clone();
        job.result = Some(artifact);
        self.save_job(job)
    }

    fn save_job(&self, job: &ForgeJob) -> Result<(), ForgeError> {
        let path = self.jobs_dir.join(format!("{}.json", job.id));
        let json = serde_json::to_string_pretty(job)?;
        crate::storage::atomic_write(&path, &json)
    }

    fn load_job(&self, job_id: Uuid) -> Result<Option<ForgeJob>, ForgeError> {
        let path = self.jobs_dir.join(format!("{job_id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        Ok(Some(serde_json::from_str(&content)?))
    }

    fn load_existing_job(&self, job_id: Uuid) -> Result<ForgeJob, ForgeError> {
        self.load_job(job_id)?
            .ok_or(ForgeError::JobNotFound(job_id))
    }
}

fn supports_objective(
    caps: &crate::backend::BackendCapabilities,
    objective: &TrainObjective,
) -> bool {
    match objective {
        TrainObjective::Lora(_) => caps.supports_lora,
        TrainObjective::FullFinetune(_) => caps.supports_full_finetune,
        TrainObjective::ContinuedPretrain(_) => caps.supports_pretraining,
    }
}

fn objective_label(objective: &TrainObjective) -> &str {
    match objective {
        TrainObjective::Lora(_) => "lora",
        TrainObjective::FullFinetune(_) => "full_finetune",
        TrainObjective::ContinuedPretrain(_) => "continued_pretrain",
    }
}

fn emit(progress: &Option<ForgeProgressCallback>, event: ForgeEvent) {
    if let Some(callback) = progress {
        callback(&event);
    }
}

fn emit_backend_selected(progress: &Option<ForgeProgressCallback>, backend: &dyn TrainBackend) {
    emit(
        progress,
        ForgeEvent::BackendSelected {
            backend: backend.capabilities().name.clone(),
            estimated_cost_usd: backend.capabilities().estimated_cost_per_gpu_hour,
        },
    );
}

fn emit_train_complete(
    progress: &Option<ForgeProgressCallback>,
    artifact: &crate::backend::TrainArtifact,
) {
    emit(
        progress,
        ForgeEvent::TrainComplete {
            loss: artifact.final_loss,
            duration_secs: artifact.training_duration_secs,
            cost: artifact.cost.clone(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::mock::MockBackend;

    fn valid_lora() -> TrainObjective {
        TrainObjective::Lora(crate::LoraConfig {
            base_model: "llama-8b".to_owned(),
            ..crate::LoraConfig::default()
        })
    }

    #[tokio::test]
    async fn create_and_run_job() {
        let directory = tempfile::TempDir::new().unwrap();
        let orchestrator = ForgeOrchestrator::new(
            vec![Box::new(MockBackend::new())],
            directory.path().join("jobs"),
        )
        .unwrap();

        let job = orchestrator
            .create_job("test".to_owned(), valid_lora())
            .await
            .unwrap();
        assert_eq!(job.status, JobStatus::Pending);

        let completed = orchestrator.run_job(job.id, None).await.unwrap();
        assert_eq!(completed.status, JobStatus::Completed);
        assert!(completed.result.is_some());
    }

    #[tokio::test]
    async fn run_job_emits_progress_events() {
        let directory = tempfile::TempDir::new().unwrap();
        let orchestrator = ForgeOrchestrator::new(
            vec![Box::new(MockBackend::new())],
            directory.path().join("jobs"),
        )
        .unwrap();
        let job = orchestrator
            .create_job("test".to_owned(), valid_lora())
            .await
            .unwrap();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let callback: ForgeProgressCallback = Arc::new(move |event| {
            captured.lock().unwrap().push(event.clone());
        });

        orchestrator.run_job(job.id, Some(callback)).await.unwrap();

        let events = events.lock().unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, ForgeEvent::JobCreated { .. })));
        assert!(events
            .iter()
            .any(|event| matches!(event, ForgeEvent::TrainProgress(_))));
        assert!(events
            .iter()
            .any(|event| matches!(event, ForgeEvent::JobComplete { .. })));
    }

    #[tokio::test]
    async fn cancel_job() {
        let directory = tempfile::TempDir::new().unwrap();
        let orchestrator = ForgeOrchestrator::new(
            vec![Box::new(MockBackend::new())],
            directory.path().join("jobs"),
        )
        .unwrap();

        let job = orchestrator
            .create_job("test".to_owned(), valid_lora())
            .await
            .unwrap();
        orchestrator.cancel_job(job.id).await.unwrap();

        let status = orchestrator.job_status(job.id).unwrap().unwrap();
        assert_eq!(status.status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn list_jobs() {
        let directory = tempfile::TempDir::new().unwrap();
        let orchestrator = ForgeOrchestrator::new(
            vec![Box::new(MockBackend::new())],
            directory.path().join("jobs"),
        )
        .unwrap();

        orchestrator
            .create_job("a".to_owned(), valid_lora())
            .await
            .unwrap();
        orchestrator
            .create_job("b".to_owned(), valid_lora())
            .await
            .unwrap();

        let jobs = orchestrator.list_jobs().unwrap();
        assert_eq!(jobs.len(), 2);
    }

    #[test]
    fn no_backend_available() {
        let directory = tempfile::TempDir::new().unwrap();
        let orchestrator =
            ForgeOrchestrator::new(Vec::new(), directory.path().join("jobs")).unwrap();
        let error = orchestrator.select_backend(&valid_lora()).err().unwrap();
        assert!(error.to_string().contains("no backend"));
    }

    #[test]
    fn validate_job_succeeds() {
        let directory = tempfile::TempDir::new().unwrap();
        let orchestrator = ForgeOrchestrator::new(
            vec![Box::new(MockBackend::new())],
            directory.path().join("jobs"),
        )
        .unwrap();
        let validation = orchestrator.validate_job(&valid_lora()).unwrap();
        assert_eq!(validation.backend, "mock");
    }
}
