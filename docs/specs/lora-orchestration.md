# LoRA Orchestration Spec

**Track:** #5 — LoRA Orchestration  
**Crate:** new `fx-lora` crate  
**Difficulty:** Hard  
**Dependencies:** fx-training (#4 ✅), fx-consensus (experiment chain), fx-fleet (distributed execution)  

---

## Problem

Fawx's experiment pipeline generates winning patches, and the training data curation system (#4) extracts these into fine-tuning datasets. The missing piece is orchestrating the actual fine-tuning: taking a curated dataset, running LoRA fine-tuning on a base model (via llama.cpp or similar), evaluating the resulting adapter, and managing the lifecycle of trained adapters.

This is "the holy grail" — Fawx improving its own model weights based on its own experiment results.

---

## Architecture

### Core Types

```rust
/// A LoRA fine-tuning job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraJob {
    pub id: Uuid,
    pub status: JobStatus,
    pub config: LoraConfig,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<LoraResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Preparing,     // downloading base model, formatting dataset
    Training,      // fine-tuning in progress
    Evaluating,    // running eval suite on trained adapter
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraConfig {
    /// Base model to fine-tune (e.g., "llama-3.3-8b-instruct").
    pub base_model: String,
    /// Path or ID of the training dataset (fx-training export).
    pub dataset_path: PathBuf,
    /// Dataset format (matches fx-training ExportFormat).
    pub dataset_format: String,
    /// LoRA hyperparameters.
    pub hyperparams: LoraHyperparams,
    /// Optional: evaluation config to run after training.
    pub eval_config: Option<EvalConfig>,
    /// Where to save the trained adapter.
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraHyperparams {
    /// LoRA rank (default: 16).
    pub rank: u32,
    /// LoRA alpha (default: 32).
    pub alpha: u32,
    /// Learning rate (default: 1e-4).
    pub learning_rate: f64,
    /// Number of training epochs (default: 3).
    pub epochs: u32,
    /// Batch size (default: 4).
    pub batch_size: u32,
    /// Target modules (default: ["q_proj", "v_proj"]).
    pub target_modules: Vec<String>,
    /// Warmup steps (default: 10).
    pub warmup_steps: u32,
    /// Gradient accumulation steps (default: 1).
    pub gradient_accumulation: u32,
}

impl Default for LoraHyperparams {
    fn default() -> Self {
        Self {
            rank: 16,
            alpha: 32,
            learning_rate: 1e-4,
            epochs: 3,
            batch_size: 4,
            target_modules: vec!["q_proj".to_owned(), "v_proj".to_owned()],
            warmup_steps: 10,
            gradient_accumulation: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraResult {
    /// Path to the trained adapter weights.
    pub adapter_path: PathBuf,
    /// Training loss (final epoch).
    pub final_loss: f64,
    /// Training duration in seconds.
    pub training_duration_secs: u64,
    /// Number of training examples processed.
    pub examples_trained: usize,
    /// Evaluation results (if eval was configured).
    pub eval_results: Option<EvalResults>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalConfig {
    /// Eval prompts to test the adapter against.
    pub prompts: Vec<EvalPrompt>,
    /// Base model responses for comparison (without adapter).
    pub baseline_responses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalPrompt {
    pub system: String,
    pub user: String,
    pub expected_contains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResults {
    /// How many eval prompts produced improved responses.
    pub improved: usize,
    /// How many eval prompts produced worse responses.
    pub regressed: usize,
    /// How many were neutral.
    pub neutral: usize,
    /// Average quality score delta (positive = improvement).
    pub avg_quality_delta: f64,
}
```

### Phase 1: Training Backend Trait

```rust
/// Backend that actually runs LoRA fine-tuning.
#[async_trait]
pub trait TrainingBackend: Send + Sync {
    /// Start a fine-tuning job. Returns when training begins.
    async fn start(&self, config: &LoraConfig) -> Result<(), LoraError>;
    
    /// Poll training progress.
    async fn progress(&self) -> Result<TrainingProgress, LoraError>;
    
    /// Wait for training to complete.
    async fn wait(&self) -> Result<LoraResult, LoraError>;
    
    /// Cancel a running job.
    async fn cancel(&self) -> Result<(), LoraError>;
}

#[derive(Debug, Clone)]
pub struct TrainingProgress {
    pub epoch: u32,
    pub total_epochs: u32,
    pub step: u64,
    pub total_steps: u64,
    pub loss: f64,
    pub learning_rate: f64,
    pub elapsed_secs: u64,
    pub estimated_remaining_secs: Option<u64>,
}
```

**Implementations:**

1. **`LlamaCppBackend`** — shells out to `llama.cpp`'s `finetune` binary. Parses stdout for progress. This is the primary backend.
2. **`MockBackend`** — for testing. Returns configurable results after a configurable delay.

### Phase 2: Adapter Manager

Manages trained LoRA adapters on disk:

```rust
pub struct AdapterManager {
    adapters_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterInfo {
    pub id: Uuid,
    pub name: String,
    pub base_model: String,
    pub adapter_path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub training_job_id: Uuid,
    pub dataset_stats: DatasetSummary,
    pub eval_results: Option<EvalResults>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetSummary {
    pub total_examples: usize,
    pub completion_examples: usize,
    pub preference_examples: usize,
}

impl AdapterManager {
    pub fn new(adapters_dir: PathBuf) -> Result<Self, LoraError>;
    
    /// Register a new adapter after successful training.
    pub fn register(&self, info: AdapterInfo) -> Result<(), LoraError>;
    
    /// List all adapters, optionally filtered by base model.
    pub fn list(&self, base_model: Option<&str>) -> Result<Vec<AdapterInfo>, LoraError>;
    
    /// Get adapter by ID.
    pub fn get(&self, id: Uuid) -> Result<Option<AdapterInfo>, LoraError>;
    
    /// Set which adapter is active for a base model.
    pub fn activate(&self, id: Uuid) -> Result<(), LoraError>;
    
    /// Deactivate an adapter (use base model without adapter).
    pub fn deactivate(&self, base_model: &str) -> Result<(), LoraError>;
    
    /// Delete an adapter and its files.
    pub fn delete(&self, id: Uuid) -> Result<(), LoraError>;
    
    /// Get the active adapter for a base model, if any.
    pub fn active_for_model(&self, base_model: &str) -> Result<Option<AdapterInfo>, LoraError>;
}
```

### Phase 3: Job Orchestrator

Coordinates the full pipeline: dataset export → training → evaluation → adapter registration.

```rust
pub struct LoraOrchestrator {
    backend: Box<dyn TrainingBackend>,
    adapter_manager: AdapterManager,
    jobs_dir: PathBuf,
}

impl LoraOrchestrator {
    /// Create and enqueue a new training job.
    pub async fn create_job(&self, config: LoraConfig) -> Result<LoraJob, LoraError>;
    
    /// Run a job through the full pipeline.
    pub async fn run_job(
        &self,
        job_id: Uuid,
        progress: Option<LoraProgressCallback>,
    ) -> Result<LoraJob, LoraError>;
    
    /// Get job status.
    pub fn job_status(&self, job_id: Uuid) -> Result<Option<LoraJob>, LoraError>;
    
    /// List all jobs.
    pub fn list_jobs(&self) -> Result<Vec<LoraJob>, LoraError>;
    
    /// Cancel a running job.
    pub async fn cancel_job(&self, job_id: Uuid) -> Result<(), LoraError>;
}

pub type LoraProgressCallback = Arc<dyn Fn(&LoraProgressEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum LoraProgressEvent {
    JobCreated { job_id: Uuid },
    DatasetPreparing { examples: usize },
    TrainingStarted { base_model: String },
    TrainingProgress(TrainingProgress),
    TrainingCompleted { loss: f64, duration_secs: u64 },
    EvaluationStarted { prompts: usize },
    EvaluationCompleted(EvalResults),
    AdapterRegistered { adapter_id: Uuid },
    JobFailed { error: String },
}
```

### Phase 4: Pipeline Integration

Connect to the existing experiment system:

```rust
/// Full self-improvement pipeline:
/// 1. Run experiments → chain winners
/// 2. Extract training data (fx-training)
/// 3. Fine-tune base model (fx-lora)
/// 4. Evaluate adapter against experiments
/// 5. If improved, activate adapter
pub struct SelfImprovementPipeline {
    training_extractor: DefaultChainExtractor,
    dataset_manager: DatasetManager,
    lora_orchestrator: LoraOrchestrator,
}
```

This is a follow-up integration — the core fx-lora crate can ship independently.

---

## Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum LoraError {
    #[error("training failed: {0}")]
    TrainingFailed(String),
    #[error("backend not available: {0}")]
    BackendUnavailable(String),
    #[error("dataset error: {0}")]
    DatasetError(String),
    #[error("evaluation failed: {0}")]
    EvaluationFailed(String),
    #[error("adapter error: {0}")]
    AdapterError(String),
    #[error("job not found: {0}")]
    JobNotFound(Uuid),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
```

---

## File Layout

```
fx-lora/
├── Cargo.toml
├── src/
│   ├── lib.rs           ← core types, re-exports
│   ├── error.rs         ← LoraError
│   ├── backend/
│   │   ├── mod.rs       ← TrainingBackend trait
│   │   ├── llamacpp.rs  ← LlamaCppBackend implementation
│   │   └── mock.rs      ← MockBackend for testing
│   ├── adapter.rs       ← AdapterManager
│   ├── orchestrator.rs  ← LoraOrchestrator + progress events
│   └── eval.rs          ← EvalConfig, EvalResults, evaluation logic
```

---

## External Dependencies

- **llama.cpp** — must be installed on the system or fleet node with GPU. The `LlamaCppBackend` shells out to the `llama-finetune` binary.
- **GGUF base models** — user must have a base model downloaded (e.g., llama-3.3-8b-instruct.Q4_K_M.gguf).
- **GPU** — LoRA training without GPU is impractical for anything beyond toy datasets. Fleet nodes with `GpuCompute` capability are preferred targets.

---

## Implementation Phases

**Phase 1 (this PR):** Core types, MockBackend, AdapterManager, basic orchestrator. No real training — just the data structures, file management, and pipeline skeleton with mocks.

**Phase 2 (follow-up):** LlamaCppBackend — shell out to llama.cpp, parse output, handle errors. Requires llama.cpp installed.

**Phase 3 (follow-up):** Evaluation pipeline — run trained adapter against eval prompts, compare to base model, score delta.

**Phase 4 (follow-up):** SelfImprovementPipeline integration — wire fx-training + fx-lora + fx-consensus into an automated loop.

---

## Test Plan

### Unit Tests (Phase 1)

1. **LoraHyperparams** — default values correct, serialization roundtrip
2. **LoraConfig** — validation (dataset_path exists, base_model non-empty)
3. **MockBackend** — start/progress/wait cycle, cancel behavior
4. **AdapterManager** — register, list, get, activate, deactivate, delete, active_for_model
5. **LoraOrchestrator** — create job, run with mock backend, job status transitions
6. **Progress events** — correct events emitted in order during mock run
7. **Error handling** — backend failure → job status Failed, missing dataset → DatasetError

---

## Implementation Notes

- Phase 1 ships the full API surface with MockBackend. This lets the Swift app wire up a "Training" UI section even before real training works.
- AdapterManager stores metadata as JSON files (same pattern as DatasetManager in fx-training).
- Job state is persisted to `jobs_dir` as JSON so training can survive process restarts (jobs resume on startup if backend supports it).
- LlamaCppBackend (Phase 2) will use `tokio::process::Command` for async execution, parsing stdout line-by-line for progress updates.
- The evaluation pipeline (Phase 3) uses the same `CompletionProvider` trait from fx-llm to compare base vs adapter responses.
