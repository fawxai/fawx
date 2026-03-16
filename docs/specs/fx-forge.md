# fx-forge Spec — Model Forge

**Crate:** new `fx-forge`  
**Difficulty:** Hard (types + mocks = Medium, real backends = Hard)  
**Dependencies:** fx-training (#4 ✅), fx-consensus (experiment chain), fx-fleet (distributed execution)  

---

## Vision

Fawx doesn't just use models — it builds them. fx-forge is the model forge: a unified pipeline that takes training data (from fx-training), trains models at any scale (LoRA on your Mac → full pre-training on Prime Intellect), evaluates results, and manages the lifecycle of trained models and adapters.

The key insight: **separate the pipeline from the compute**. Fawx orchestrates; backends execute. Same pipeline, different scales.

---

## The Training Progression

| Stage | What | Compute | Cost/Run | What You Learn |
|-------|------|---------|----------|----------------|
| 1. LoRA | Adapter on frozen base | 1 GPU, hours | $10-100 | What good training data looks like |
| 2. Full fine-tune | Update all weights | Multi-GPU, hours | $100-1K | What your model needs to be good at |
| 3. Continued pre-train | Domain absorption | GPU cluster, days | $1K-10K | What data matters |
| 4. From scratch | Custom architecture | Distributed cluster, weeks | $10K-1M+ | Everything |

fx-forge supports all four. Phase 1 implements LoRA + mocks for the rest.

---

## Architecture

### Core Types

```rust
/// A training job submitted to the forge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeJob {
    pub id: Uuid,
    pub name: String,
    pub status: JobStatus,
    pub objective: TrainObjective,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<ForgeResult>,
    pub error: Option<String>,
    /// Which backend is handling this job.
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Preparing,      // downloading model, formatting dataset
    Uploading,      // uploading data to cloud/cluster
    Training,       // compute running
    Evaluating,     // benchmarking the result
    Completed,
    Failed,
    Cancelled,
}

/// What kind of training to do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrainObjective {
    Lora(LoraConfig),
    FullFinetune(FinetuneConfig),
    ContinuedPretrain(PretrainConfig),
    FromScratch(ScratchConfig),
}
```

### Training Configs (per objective)

```rust
/// LoRA adapter training on a frozen base model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraConfig {
    pub base_model: String,
    pub dataset: DatasetRef,
    pub rank: u32,              // default: 16
    pub alpha: u32,             // default: 32
    pub target_modules: Vec<String>,  // default: ["q_proj", "v_proj"]
    pub learning_rate: f64,     // default: 1e-4
    pub epochs: u32,            // default: 3
    pub batch_size: u32,        // default: 4
    pub warmup_steps: u32,      // default: 10
    pub output_dir: PathBuf,
}

/// Full fine-tuning — all weights updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinetuneConfig {
    pub base_model: String,
    pub dataset: DatasetRef,
    pub learning_rate: f64,     // default: 2e-5
    pub epochs: u32,            // default: 3
    pub batch_size: u32,        // default: 2
    pub gradient_accumulation: u32, // default: 4
    pub warmup_ratio: f64,      // default: 0.03
    pub output_dir: PathBuf,
}

/// Continued pre-training on domain corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PretrainConfig {
    pub base_model: String,
    pub corpus: CorpusRef,
    pub learning_rate: f64,     // default: 1e-5
    pub max_steps: u64,
    pub batch_size: u32,
    pub context_length: u32,    // default: 4096
    pub output_dir: PathBuf,
}

/// Train from scratch with custom architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchConfig {
    pub architecture: ArchitectureSpec,
    pub corpus: CorpusRef,
    pub tokenizer: TokenizerSpec,
    pub total_tokens: u64,      // how many tokens to train on
    pub batch_size: u32,
    pub learning_rate: f64,
    pub warmup_steps: u64,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureSpec {
    pub family: String,         // "llama", "mamba", "rwkv", custom
    pub params: ModelParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelParams {
    pub hidden_size: u32,
    pub num_layers: u32,
    pub num_heads: u32,
    pub intermediate_size: u32,
    pub vocab_size: u32,
    pub max_position_embeddings: u32,
}

/// Reference to a training dataset (from fx-training).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetRef {
    pub path: PathBuf,
    pub format: DatasetFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DatasetFormat {
    OpenAiJsonl,
    AlpacaJsonl,
    DpoJsonl,
    RawJson,
    PlainText,  // for pre-training corpus
}

/// Reference to a pre-training corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusRef {
    pub paths: Vec<PathBuf>,    // directories or files
    pub format: CorpusFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CorpusFormat {
    PlainText,
    Jsonl,      // one document per line
    Parquet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenizerSpec {
    pub source: TokenizerSource,
    pub vocab_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TokenizerSource {
    /// Use an existing tokenizer from a model.
    FromModel(String),
    /// Train a new tokenizer on the corpus.
    TrainBpe { vocab_size: u32 },
}
```

### Training Backend Trait

```rust
/// Capability descriptor for a training backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub name: String,
    pub supports_lora: bool,
    pub supports_full_finetune: bool,
    pub supports_pretraining: bool,
    pub supports_from_scratch: bool,
    pub max_model_params: Option<u64>,
    pub max_dataset_gb: Option<u64>,
    pub has_gpu: bool,
    pub gpu_vram_gb: Option<u32>,
    pub estimated_cost_per_hour: Option<f64>,
}

/// Handle to a submitted training job on a backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct JobHandle(pub String);

/// Progress of a training job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainProgress {
    pub phase: String,          // "preparing", "training", "evaluating"
    pub epoch: Option<u32>,
    pub total_epochs: Option<u32>,
    pub step: u64,
    pub total_steps: Option<u64>,
    pub loss: Option<f64>,
    pub learning_rate: Option<f64>,
    pub elapsed_secs: u64,
    pub estimated_remaining_secs: Option<u64>,
    pub tokens_processed: Option<u64>,
}

/// Result of a completed training job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainArtifact {
    pub artifact_type: ArtifactType,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub final_loss: Option<f64>,
    pub training_duration_secs: u64,
    pub examples_or_tokens_processed: u64,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactType {
    LoraAdapter,
    FullModel,
    Checkpoint,
}

#[async_trait]
pub trait TrainBackend: Send + Sync {
    /// What this backend supports.
    fn capabilities(&self) -> &BackendCapabilities;

    /// Submit a training job.
    async fn submit(&self, objective: &TrainObjective) -> Result<JobHandle, ForgeError>;

    /// Poll progress.
    async fn progress(&self, handle: &JobHandle) -> Result<TrainProgress, ForgeError>;

    /// Wait for completion and retrieve the artifact.
    async fn wait(&self, handle: &JobHandle) -> Result<TrainArtifact, ForgeError>;

    /// Cancel a running job.
    async fn cancel(&self, handle: &JobHandle) -> Result<(), ForgeError>;
}
```

### Backend Implementations

#### LocalBackend (Phase 1)
- Shells out to **Unsloth** (Python) for LoRA and full fine-tune
- Falls back to **torchtune** if Unsloth unavailable
- Parses stdout/stderr for progress
- Single machine, needs local GPU
- Auto-detects available VRAM and adjusts batch size

#### CloudBackend (Phase 2)
- Provisions GPU instances on **RunPod**, **Lambda**, or **vast.ai**
- Uploads dataset, runs training script, downloads artifact
- SSH + rsync for file transfer
- Auto-terminates instance on completion
- Supports spot instances for cost savings

#### ClusterBackend (Phase 3)
- **Prime Intellect** integration for distributed training
- OpenDiLoCo for multi-node pre-training
- Handles data sharding, checkpoint sync, fault tolerance
- For continued pre-training and from-scratch only

#### MockBackend (testing)
- Returns configurable results after configurable delay
- For testing the pipeline without real compute

### Serving Backend Trait (separate from training)

```rust
/// Backend for serving models with optional LoRA adapters.
#[async_trait]
pub trait ServingBackend: Send + Sync {
    /// Load a base model.
    async fn load_model(&self, model_path: &Path) -> Result<(), ForgeError>;

    /// Attach a LoRA adapter to the loaded model.
    async fn attach_adapter(&self, adapter_path: &Path) -> Result<(), ForgeError>;

    /// Detach current adapter (revert to base model).
    async fn detach_adapter(&self) -> Result<(), ForgeError>;

    /// Which adapter is currently active, if any.
    fn active_adapter(&self) -> Option<&Path>;

    /// Health check.
    async fn health(&self) -> Result<bool, ForgeError>;
}
```

**Implementations:**
- **LlamaCppServing** — for single-user Mac inference. Manages llama-server process.
- **VllmServing** — for multi-user serving. Hot-swaps LoRA adapters per request. Manages vLLM server process.

### Artifact Manager

```rust
/// Manages trained model artifacts on disk.
pub struct ArtifactManager {
    artifacts_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub id: Uuid,
    pub name: String,
    pub artifact_type: ArtifactType,
    pub base_model: Option<String>,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub job_id: Uuid,
    pub eval_results: Option<EvalResults>,
    pub active: bool,
}

impl ArtifactManager {
    pub fn new(artifacts_dir: PathBuf) -> Result<Self, ForgeError>;
    pub fn register(&self, info: ArtifactInfo) -> Result<(), ForgeError>;
    pub fn list(&self, filter: Option<&ArtifactFilter>) -> Result<Vec<ArtifactInfo>, ForgeError>;
    pub fn get(&self, id: Uuid) -> Result<Option<ArtifactInfo>, ForgeError>;
    pub fn activate(&self, id: Uuid) -> Result<(), ForgeError>;
    pub fn deactivate(&self, base_model: &str) -> Result<(), ForgeError>;
    pub fn delete(&self, id: Uuid) -> Result<(), ForgeError>;
    pub fn active_for_model(&self, base_model: &str) -> Result<Option<ArtifactInfo>, ForgeError>;
}

#[derive(Debug, Clone)]
pub struct ArtifactFilter {
    pub artifact_type: Option<ArtifactType>,
    pub base_model: Option<String>,
    pub active_only: bool,
}
```

### Evaluation

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalConfig {
    pub prompts: Vec<EvalPrompt>,
    /// Run base model for comparison.
    pub compare_to_base: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalPrompt {
    pub system: String,
    pub user: String,
    /// Substrings the response should contain.
    pub expected_contains: Vec<String>,
    /// Keywords for quality scoring.
    pub quality_keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResults {
    pub improved: usize,
    pub regressed: usize,
    pub neutral: usize,
    pub avg_quality_delta: f64,
    pub prompts_evaluated: usize,
}
```

### Job Orchestrator

```rust
pub struct ForgeOrchestrator {
    backends: Vec<Box<dyn TrainBackend>>,
    artifact_manager: ArtifactManager,
    jobs_dir: PathBuf,
}

pub type ForgeProgressCallback = Arc<dyn Fn(&ForgeEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum ForgeEvent {
    JobCreated { job_id: Uuid, objective: String },
    BackendSelected { backend: String },
    DatasetPreparing { path: String },
    UploadStarted { size_bytes: u64 },
    UploadComplete,
    TrainProgress(TrainProgress),
    TrainComplete { loss: Option<f64>, duration_secs: u64 },
    EvalStarted { prompts: usize },
    EvalComplete(EvalResults),
    ArtifactRegistered { artifact_id: Uuid },
    JobComplete { job_id: Uuid },
    JobFailed { job_id: Uuid, error: String },
}

impl ForgeOrchestrator {
    /// Create with available backends.
    pub fn new(
        backends: Vec<Box<dyn TrainBackend>>,
        artifact_manager: ArtifactManager,
        jobs_dir: PathBuf,
    ) -> Result<Self, ForgeError>;

    /// Select the best backend for a training objective.
    pub fn select_backend(&self, objective: &TrainObjective) -> Result<&dyn TrainBackend, ForgeError>;

    /// Create and enqueue a job.
    pub async fn create_job(&self, name: String, objective: TrainObjective) -> Result<ForgeJob, ForgeError>;

    /// Run a job through the full pipeline.
    pub async fn run_job(
        &self,
        job_id: Uuid,
        progress: Option<ForgeProgressCallback>,
    ) -> Result<ForgeJob, ForgeError>;

    /// Get job status.
    pub fn job_status(&self, job_id: Uuid) -> Result<Option<ForgeJob>, ForgeError>;

    /// List all jobs.
    pub fn list_jobs(&self) -> Result<Vec<ForgeJob>, ForgeError>;

    /// Cancel a running job.
    pub async fn cancel_job(&self, job_id: Uuid) -> Result<(), ForgeError>;
}
```

Backend selection logic:
1. Filter backends by capability (does it support this TrainObjective?)
2. Filter by resource requirements (model size vs backend limits)
3. Prefer local over cloud if capable (cost optimization)
4. Return error if no backend can handle the objective

---

## Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    #[error("training failed: {0}")]
    TrainingFailed(String),
    #[error("no backend available for objective: {0}")]
    NoBackendAvailable(String),
    #[error("backend error: {0}")]
    BackendError(String),
    #[error("dataset error: {0}")]
    DatasetError(String),
    #[error("evaluation failed: {0}")]
    EvaluationFailed(String),
    #[error("artifact error: {0}")]
    ArtifactError(String),
    #[error("job not found: {0}")]
    JobNotFound(Uuid),
    #[error("job already running: {0}")]
    JobAlreadyRunning(Uuid),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
```

---

## File Layout

```
fx-forge/
├── Cargo.toml
├── src/
│   ├── lib.rs              ← core types, re-exports
│   ├── error.rs            ← ForgeError
│   ├── objective.rs        ← TrainObjective, LoraConfig, FinetuneConfig, etc.
│   ├── backend/
│   │   ├── mod.rs          ← TrainBackend trait, BackendCapabilities
│   │   ├── mock.rs         ← MockBackend for testing
│   │   ├── local.rs        ← LocalBackend (Unsloth/torchtune) [Phase 2]
│   │   ├── cloud.rs        ← CloudBackend (RunPod/Lambda) [Phase 2]
│   │   └── cluster.rs      ← ClusterBackend (Prime Intellect) [Phase 3]
│   ├── serving/
│   │   ├── mod.rs          ← ServingBackend trait
│   │   ├── llamacpp.rs     ← LlamaCppServing [Phase 2]
│   │   └── vllm.rs         ← VllmServing [Phase 2]
│   ├── artifact.rs         ← ArtifactManager
│   ├── eval.rs             ← EvalConfig, EvalResults
│   ├── orchestrator.rs     ← ForgeOrchestrator, ForgeEvent
│   └── progress.rs         ← TrainProgress, ForgeProgressCallback
```

---

## Implementation Phases

**Phase 1 (this PR):** Core types + MockBackend + ArtifactManager + orchestrator skeleton. Everything compiles, tests pass, no real compute. ~1000-1500 lines.

**Phase 2:** LocalBackend (Unsloth for LoRA/fine-tune) + LlamaCppServing. Requires Python + Unsloth installed. First real training runs.

**Phase 3:** CloudBackend (RunPod/Lambda). SSH provisioning, dataset upload, remote training, artifact download.

**Phase 4:** ClusterBackend (Prime Intellect). Distributed training, OpenDiLoCo integration.

**Phase 5:** VllmServing + hot LoRA swap. Multi-user serving with per-request adapter selection.

**Phase 6:** Self-improvement loop. Wire fx-training + fx-forge + fx-consensus into: run experiments → curate data → train model → evaluate → activate if improved → repeat.

---

## Test Plan (Phase 1)

1. **TrainObjective** — each variant serializes/deserializes correctly
2. **LoraConfig/FinetuneConfig/etc** — defaults are sensible, validation works
3. **MockBackend** — submit → progress → wait cycle, cancel behavior, capabilities reporting
4. **ArtifactManager** — register, list, get, activate, deactivate, delete, active_for_model, filter
5. **ForgeOrchestrator** — create job, run with mock, backend selection (picks capable backend), job status transitions, progress events emitted in order
6. **Backend selection** — LoRA objective selects backend with `supports_lora`, from-scratch selects backend with `supports_from_scratch`, no capable backend → error
7. **EvalResults** — correct delta calculation
8. **Error handling** — backend failure → job Failed, missing dataset → DatasetError

---

## Design Decisions

1. **TrainBackend is async** — even local training is long-running; async lets us poll without blocking.
2. **Serving is separate from training** — different backends, different lifecycles. You might train on cloud but serve locally.
3. **ArtifactManager mirrors DatasetManager pattern** — JSON files in a directory, manifest for metadata. Proven pattern from fx-training.
4. **Backend selection is automatic** — orchestrator picks the best backend based on capabilities and objective. User can override with `backend: "local"` in the job config.
5. **Phase 1 ships the full API surface** — MockBackend means the Swift app can wire up a "Forge" UI section before real training works. Types are stable; backends are pluggable.
6. **No opinion on Python environment** — LocalBackend shells out to `unsloth` or `torchtune` via subprocess. User manages their Python env. We don't bundle Python.
7. **Job persistence** — jobs saved as JSON in `jobs_dir`. If the process restarts, jobs can be listed but not resumed (resume = Phase 2 feature for cloud backends).
