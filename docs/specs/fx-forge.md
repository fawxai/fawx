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

fx-forge supports stages 1-3 in the type system. Stage 4 (from-scratch) is deferred — `TrainObjective` is `#[non_exhaustive]` so it can be added later without breaking changes.

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
    /// Cost tracking for this job.
    pub cost: Option<CostRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Preparing,      // downloading model, formatting dataset, validating
    Uploading,      // uploading data to cloud/cluster
    Training,       // compute running
    Converting,     // format conversion (safetensors → GGUF, etc.)
    Evaluating,     // benchmarking the result
    Completed,
    Failed,
    Cancelled,
}

/// What kind of training to do.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TrainObjective {
    Lora(LoraConfig),
    FullFinetune(FinetuneConfig),
    ContinuedPretrain(PretrainConfig),
}

/// Cost tracking for a training job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRecord {
    /// Estimated cost before starting.
    pub estimated_usd: Option<f64>,
    /// Actual cost after completion (from backend).
    pub actual_usd: Option<f64>,
    /// GPU-hours consumed.
    pub gpu_hours: Option<f64>,
    /// Backend-specific billing details.
    pub details: serde_json::Value,
}
```

### Training Configs

All configs include `extra_params` for backend-specific knobs that aren't modeled in the struct.

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
    pub dropout: f64,           // default: 0.05
    pub max_grad_norm: Option<f64>,  // gradient clipping
    pub fp16: bool,             // default: true
    pub bf16: bool,             // default: false
    pub output_dir: PathBuf,
    pub output_format: ModelFormat,  // what format to produce
    /// Backend-specific parameters not modeled above.
    #[serde(default)]
    pub extra_params: HashMap<String, serde_json::Value>,
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
    pub dropout: f64,           // default: 0.0
    pub max_grad_norm: Option<f64>,
    pub fp16: bool,
    pub bf16: bool,
    pub output_dir: PathBuf,
    pub output_format: ModelFormat,
    #[serde(default)]
    pub extra_params: HashMap<String, serde_json::Value>,
}

/// Continued pre-training on domain corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PretrainConfig {
    pub base_model: String,
    pub corpus: DatasetRef,     // uses PlainText or Jsonl format
    pub learning_rate: f64,     // default: 1e-5
    pub max_steps: u64,
    pub batch_size: u32,
    pub context_length: u32,    // default: 4096
    pub warmup_steps: u64,
    pub max_grad_norm: Option<f64>,
    pub fp16: bool,
    pub bf16: bool,
    pub output_dir: PathBuf,
    pub output_format: ModelFormat,
    #[serde(default)]
    pub extra_params: HashMap<String, serde_json::Value>,
}
```

### Dataset and Format References

```rust
/// Reference to a training dataset — supports both local and remote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DatasetRef {
    /// Local filesystem path.
    Local { path: PathBuf, format: DatasetFormat },
    /// Remote URL (S3, HTTP, etc.) — backend downloads it.
    Remote { url: String, format: DatasetFormat },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DatasetFormat {
    OpenAiJsonl,
    AlpacaJsonl,
    DpoJsonl,
    RawJson,
    PlainText,      // for pre-training corpus
    Jsonl,          // one document per line
    Parquet,
}

/// Model weight format — critical for serving compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelFormat {
    /// PyTorch safetensors (default training output).
    Safetensors,
    /// llama.cpp GGUF (quantized, for local inference).
    Gguf,
    /// HuggingFace format (config.json + model files).
    HuggingFace,
    /// Raw PyTorch .bin files.
    PyTorchBin,
}
```

### Model Format Conversion

Training backends typically output safetensors. Serving backends (especially llama.cpp) need GGUF. The pipeline needs an explicit conversion step.

```rust
/// Converts model artifacts between formats.
#[async_trait]
pub trait FormatConverter: Send + Sync {
    /// Check if this converter supports the given conversion.
    fn supports(&self, from: &ModelFormat, to: &ModelFormat) -> bool;

    /// Convert a model artifact from one format to another.
    async fn convert(
        &self,
        input_path: &Path,
        output_path: &Path,
        from: &ModelFormat,
        to: &ModelFormat,
        quantization: Option<&QuantizationConfig>,
    ) -> Result<ConversionResult, ForgeError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantizationConfig {
    /// Quantization method (e.g., "Q4_K_M", "Q5_K_S", "Q8_0").
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionResult {
    pub output_path: PathBuf,
    pub output_format: ModelFormat,
    pub size_bytes: u64,
    pub quantization: Option<String>,
    pub duration_secs: u64,
}
```

**Implementations:**
- **`LlamaCppConverter`** — shells out to `llama-quantize` / `convert_hf_to_gguf.py` for safetensors → GGUF.
- **`MockConverter`** — for testing.

Conversion is an explicit pipeline step (the `Converting` job status) between training and evaluation/serving.

### Training Backend Trait

```rust
/// Capability descriptor for a training backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub name: String,
    pub supports_lora: bool,
    pub supports_full_finetune: bool,
    pub supports_pretraining: bool,
    pub max_model_params: Option<u64>,
    pub max_dataset_gb: Option<u64>,
    pub has_gpu: bool,
    pub gpu_vram_gb: Option<u32>,
    pub estimated_cost_per_gpu_hour: Option<f64>,
}

/// Handle to a submitted training job on a backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct JobHandle(pub String);

/// Progress of a training job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainProgress {
    pub phase: String,
    pub epoch: Option<u32>,
    pub total_epochs: Option<u32>,
    pub step: u64,
    pub total_steps: Option<u64>,
    pub loss: Option<f64>,
    pub learning_rate: Option<f64>,
    pub elapsed_secs: u64,
    pub estimated_remaining_secs: Option<u64>,
    pub tokens_processed: Option<u64>,
    pub gpu_utilization_pct: Option<f32>,
    pub cost_so_far_usd: Option<f64>,
}

/// Result of a completed training job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainArtifact {
    pub artifact_type: ArtifactType,
    pub format: ModelFormat,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub final_loss: Option<f64>,
    pub training_duration_secs: u64,
    pub examples_or_tokens_processed: u64,
    pub cost: Option<CostRecord>,
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

    /// Validate that a training objective can run on this backend.
    fn validate(&self, objective: &TrainObjective) -> Result<(), ForgeError>;

    /// Estimate cost before starting.
    async fn estimate_cost(&self, objective: &TrainObjective) -> Result<Option<f64>, ForgeError>;

    /// Submit a training job.
    async fn submit(&self, objective: &TrainObjective) -> Result<JobHandle, ForgeError>;

    /// Poll progress.
    async fn progress(&self, handle: &JobHandle) -> Result<TrainProgress, ForgeError>;

    /// Wait for completion and retrieve the artifact.
    async fn wait(&self, handle: &JobHandle) -> Result<TrainArtifact, ForgeError>;

    /// Retrieve logs from the training run.
    async fn logs(&self, handle: &JobHandle, tail: usize) -> Result<Vec<String>, ForgeError>;

    /// Cancel a running job.
    async fn cancel(&self, handle: &JobHandle) -> Result<(), ForgeError>;

    /// Resume a previously interrupted job (if supported).
    /// Returns None if the backend doesn't support resume.
    async fn resume(&self, handle: &JobHandle) -> Result<Option<JobHandle>, ForgeError>;
}
```

**Note on push vs poll:** Phase 1 uses poll-based progress. A future `ProgressStream` return type can be added to `submit()` without breaking the trait (add a default method). The poll approach is simpler, works for all backends, and is sufficient for Phase 1.

### Backend Implementations

#### LocalBackend (Phase 2)
- Shells out to **Unsloth** (Python) for LoRA and full fine-tune
- Falls back to **torchtune** if Unsloth unavailable
- Parses stdout/stderr for progress
- Single machine, needs local GPU
- Auto-detects available VRAM and adjusts batch size
- `validate()` checks Python env, GPU availability, VRAM sufficiency

#### CloudBackend (Phase 3)
- Provisions GPU instances on **RunPod**, **Lambda**, or **vast.ai**
- Uploads dataset (DatasetRef::Remote supported natively), runs training script, downloads artifact
- SSH + rsync for file transfer
- Auto-terminates instance on completion
- Supports spot instances for cost savings
- `estimate_cost()` queries provider pricing API
- `resume()` supported via checkpoints on persistent storage

#### ClusterBackend (Phase 4)
- **Prime Intellect** integration for distributed training
- OpenDiLoCo for multi-node pre-training
- Handles data sharding, checkpoint sync, fault tolerance
- For continued pre-training only (initially)

#### MockBackend (Phase 1 — testing)
- Returns configurable results after configurable delay
- For testing the pipeline without real compute
- Configurable capabilities to test backend selection

### Serving Backend Trait (separate from training)

```rust
/// Backend for serving models with optional LoRA adapters.
#[async_trait]
pub trait ServingBackend: Send + Sync {
    /// Load a base model.
    async fn load_model(&self, model_path: &Path, format: &ModelFormat) -> Result<(), ForgeError>;

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

**Implementations (Phase 5):**
- **LlamaCppServing** — for single-user Mac inference. Manages llama-server process. Requires GGUF format.
- **VllmServing** — for multi-user serving. Hot-swaps LoRA adapters per request. Requires safetensors format. Manages vLLM server process.

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
    pub format: Option<ModelFormat>,
    pub base_model: Option<String>,
    pub active_only: bool,
}
```

**Note on multi-GB artifacts:** ArtifactManager stores **metadata** as JSON files (same pattern as fx-training DatasetManager). The actual model weights live at `path` — typically in the `output_dir` specified in the training config. ArtifactManager doesn't copy or move weight files; it just tracks them. This means deletion is a two-step: remove metadata + optionally remove weight files (with user confirmation for large files).

### Dataset Validation

Before submitting a job, the orchestrator validates the dataset:

```rust
pub struct DatasetValidation {
    pub is_valid: bool,
    pub example_count: usize,
    pub format_errors: Vec<String>,
    pub warnings: Vec<String>,
    pub estimated_tokens: Option<u64>,
}

pub fn validate_dataset(dataset: &DatasetRef) -> Result<DatasetValidation, ForgeError>;
```

Checks:
- File/URL exists and is readable
- Format matches declared format (parseable as JSONL, etc.)
- Non-empty (at least 1 example)
- Samples first N lines for format validation (doesn't load entire dataset)
- Estimates total tokens for cost estimation

### Evaluation

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalConfig {
    pub prompts: Vec<EvalPrompt>,
    /// Run base model for comparison.
    pub compare_to_base: bool,
    /// Optional: use an LLM judge for quality scoring.
    pub llm_judge: Option<LlmJudgeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalPrompt {
    pub system: String,
    pub user: String,
    /// Expected behavior (for LLM judge evaluation).
    pub expected_behavior: String,
    /// Optional: exact substrings the response should contain.
    pub expected_contains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmJudgeConfig {
    /// Model to use as judge (should be stronger than the trained model).
    pub judge_model: String,
    /// Scoring rubric for the judge.
    pub rubric: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResults {
    pub improved: usize,
    pub regressed: usize,
    pub neutral: usize,
    pub avg_quality_delta: f64,
    pub prompts_evaluated: usize,
    /// Per-prompt scores if LLM judge was used.
    pub judge_scores: Option<Vec<f64>>,
}
```

LLM-as-judge is the realistic evaluation approach for model quality. Substring matching is a fast sanity check; the judge provides meaningful quality assessment.

### Job Orchestrator

```rust
pub struct ForgeOrchestrator {
    backends: Vec<Box<dyn TrainBackend>>,
    converters: Vec<Box<dyn FormatConverter>>,
    artifact_manager: ArtifactManager,
    jobs_dir: PathBuf,
}

pub type ForgeProgressCallback = Arc<dyn Fn(&ForgeEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum ForgeEvent {
    JobCreated { job_id: Uuid, objective: String },
    BackendSelected { backend: String, estimated_cost_usd: Option<f64> },
    DatasetValidated { examples: usize, estimated_tokens: Option<u64> },
    DatasetUploading { size_bytes: u64 },
    DatasetUploaded,
    TrainProgress(TrainProgress),
    TrainComplete { loss: Option<f64>, duration_secs: u64, cost: Option<CostRecord> },
    ConvertingFormat { from: ModelFormat, to: ModelFormat },
    ConversionComplete(ConversionResult),
    EvalStarted { prompts: usize },
    EvalComplete(EvalResults),
    ArtifactRegistered { artifact_id: Uuid },
    JobComplete { job_id: Uuid, total_cost: Option<CostRecord> },
    JobFailed { job_id: Uuid, error: String },
}

impl ForgeOrchestrator {
    pub fn new(
        backends: Vec<Box<dyn TrainBackend>>,
        converters: Vec<Box<dyn FormatConverter>>,
        artifact_manager: ArtifactManager,
        jobs_dir: PathBuf,
    ) -> Result<Self, ForgeError>;

    /// Select the best backend for a training objective.
    pub fn select_backend(&self, objective: &TrainObjective) -> Result<&dyn TrainBackend, ForgeError>;

    /// Validate a training objective before creating a job.
    pub fn validate_job(&self, objective: &TrainObjective) -> Result<JobValidation, ForgeError>;

    /// Create and enqueue a job.
    pub async fn create_job(&self, name: String, objective: TrainObjective) -> Result<ForgeJob, ForgeError>;

    /// Run a job through the full pipeline:
    /// validate → prepare → upload → train → convert → evaluate → register artifact.
    pub async fn run_job(
        &self,
        job_id: Uuid,
        progress: Option<ForgeProgressCallback>,
    ) -> Result<ForgeJob, ForgeError>;

    pub fn job_status(&self, job_id: Uuid) -> Result<Option<ForgeJob>, ForgeError>;
    pub fn list_jobs(&self) -> Result<Vec<ForgeJob>, ForgeError>;
    pub async fn cancel_job(&self, job_id: Uuid) -> Result<(), ForgeError>;
}

pub struct JobValidation {
    pub backend: String,
    pub estimated_cost_usd: Option<f64>,
    pub dataset_validation: DatasetValidation,
    pub warnings: Vec<String>,
}
```

Backend selection logic:
1. Filter backends by capability (does it support this TrainObjective?)
2. Filter by resource requirements (model size vs backend VRAM/limits)
3. `validate()` each candidate backend
4. Prefer local over cloud if capable (cost optimization)
5. Return error if no backend can handle the objective

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
    #[error("dataset validation failed: {0}")]
    DatasetInvalid(String),
    #[error("format conversion failed: {0}")]
    ConversionFailed(String),
    #[error("evaluation failed: {0}")]
    EvaluationFailed(String),
    #[error("artifact error: {0}")]
    ArtifactError(String),
    #[error("job not found: {0}")]
    JobNotFound(Uuid),
    #[error("job already running: {0}")]
    JobAlreadyRunning(Uuid),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
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
│   ├── lib.rs              ← core types (ForgeJob, JobStatus, CostRecord), re-exports
│   ├── error.rs            ← ForgeError
│   ├── objective.rs        ← TrainObjective, LoraConfig, FinetuneConfig, PretrainConfig
│   ├── dataset.rs          ← DatasetRef, DatasetFormat, DatasetValidation, validate_dataset
│   ├── format.rs           ← ModelFormat, FormatConverter trait, QuantizationConfig, ConversionResult
│   ├── backend/
│   │   ├── mod.rs          ← TrainBackend trait, BackendCapabilities, TrainProgress, TrainArtifact
│   │   └── mock.rs         ← MockBackend for testing
│   ├── serving/
│   │   └── mod.rs          ← ServingBackend trait (types only, no implementations yet)
│   ├── artifact.rs         ← ArtifactManager, ArtifactInfo, ArtifactFilter
│   ├── eval.rs             ← EvalConfig, EvalPrompt, EvalResults, LlmJudgeConfig
│   └── orchestrator.rs     ← ForgeOrchestrator, ForgeEvent, ForgeProgressCallback
```

---

## Implementation Phases

**Phase 1 (this PR):** Core types + MockBackend + MockConverter + ArtifactManager + dataset validation + orchestrator skeleton. Everything compiles, tests pass, no real compute. ~1500-2000 lines.

**Phase 2:** LocalBackend (Unsloth for LoRA/fine-tune) + LlamaCppConverter (safetensors → GGUF). First real training runs on local GPU.

**Phase 3:** CloudBackend (RunPod/Lambda). SSH provisioning, remote training, artifact download. Cost tracking wired to provider APIs.

**Phase 4:** ClusterBackend (Prime Intellect). Distributed continued pre-training.

**Phase 5:** LlamaCppServing + VllmServing. Model serving with hot LoRA swap.

**Phase 6:** Self-improvement loop. Wire fx-training + fx-forge + fx-consensus: experiments → curate data → train → evaluate → activate if improved → repeat.

---

## Test Plan (Phase 1)

1. **TrainObjective** — each variant serializes/deserializes, `#[non_exhaustive]` verified
2. **LoraConfig/FinetuneConfig/PretrainConfig** — defaults sensible, extra_params roundtrip, validation catches bad values (0 epochs, negative lr)
3. **DatasetRef** — Local and Remote variants, format validation
4. **DatasetValidation** — valid JSONL passes, empty file fails, format mismatch caught
5. **ModelFormat** — all variants serialize correctly
6. **MockBackend** — submit → progress → wait cycle, cancel, capabilities, estimate_cost, validate, logs, resume returns None
7. **MockConverter** — convert succeeds, unsupported conversion returns error
8. **ArtifactManager** — register, list, get, activate, deactivate, delete, active_for_model, filter by type/format/model
9. **ForgeOrchestrator** — create job, run with mock backend + mock converter, backend selection, validation, progress events in order, cost tracking
10. **Backend selection** — LoRA picks supports_lora backend, no capable backend → error, prefers local over cloud
11. **Error handling** — backend failure → job Failed, invalid dataset → DatasetInvalid, no converter → ConversionFailed

---

## Design Decisions

1. **`#[non_exhaustive]` on TrainObjective** — FromScratch deferred, but the enum can grow without breaking callers.
2. **Serving separate from training** — different backends, different lifecycles, different formats needed.
3. **Format conversion as explicit pipeline step** — training outputs safetensors, serving may need GGUF. The conversion step is explicit (not hidden) so users know what's happening and can configure quantization.
4. **DatasetRef supports Local + Remote** — local for development, remote for cloud backends that can download directly.
5. **extra_params on every config** — escape hatch for backend-specific knobs. Backends interpret these as they see fit.
6. **Cost tracking is first-class** — CostRecord on jobs, estimate_cost on backends, cost_so_far in progress. Training is expensive; users need visibility.
7. **LLM-as-judge for evaluation** — substring matching is a sanity check, not a quality assessment. Real evaluation needs a stronger model judging the trained model.
8. **ArtifactManager stores metadata, not weights** — weights stay where the training backend put them. No unnecessary copies of multi-GB files.
9. **Poll-based progress for Phase 1** — simpler, works for all backends. Push-based (Stream) can be added as a default trait method later.
10. **resume() designed in now** — even if MockBackend returns None, the trait surface is ready for cloud backends that support checkpoint resume.
