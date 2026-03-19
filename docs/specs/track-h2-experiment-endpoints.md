# Track H-2: Experiment Monitor HTTP Endpoints

**Status:** SPEC
**Priority:** High — unblocks Swift experiment monitor screen (Phase 5.5)
**Endpoints:** GET/POST `/v1/experiments`, GET `/v1/experiments/{id}`, GET `/v1/experiments/{id}/results`, POST `/v1/experiments/{id}/stop`

---

## Overview

Add five endpoints to fx-api that let the Swift app list, create, inspect, and stop self-improvement experiments. The engine already has two relevant crates:

- **fx-improve** — the self-improvement pipeline: signals → analysis → detect candidates → plan fixes → execute proposals. Key types: `ImprovementConfig`, `ImprovementRunResult`, `ImprovementCandidate`, `FixPlan`, `RiskLevel`, `OutputMode`.
- **fx-analysis** — the analysis engine that finds recurring patterns from runtime signals. Key types: `AnalysisEngine`, `AnalysisFinding`, `Confidence`, `SignalEvidence`.

Currently, improvement cycles run programmatically via `run_improvement_cycle()`. There is no experiment registry, no persistent experiment state, and no way to manage experiments via HTTP. This track introduces:

1. An `ExperimentRegistry` to track experiment lifecycle (created → running → completed/stopped/failed)
2. HTTP endpoints that expose experiment CRUD and results
3. Integration with the existing `fx-improve` pipeline for execution

---

## Data Model

### Experiment

An experiment represents a single improvement cycle run with a specific configuration and tracked lifecycle.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Full improvement cycle: analyze → detect → plan → execute.
    ProofOfFitness,
    /// Analysis only: detect patterns but don't generate fixes.
    AnalysisOnly,
    /// Tournament: run multiple candidates head-to-head.
    Tournament,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentConfig {
    /// Number of candidates (population) to evaluate.
    #[serde(default = "default_population")]
    pub population: usize,
    /// Number of tournament rounds.
    #[serde(default = "default_rounds")]
    pub rounds: usize,
    /// Minimum confidence threshold for candidates.
    #[serde(default)]
    pub min_confidence: Option<String>,
    /// Output mode: proposal_only, proposal_with_branch, dry_run.
    #[serde(default)]
    pub output_mode: Option<String>,
}

fn default_population() -> usize { 16 }
fn default_rounds() -> usize { 4 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentProgress {
    pub completed_matches: usize,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentResult {
    pub plans_generated: usize,
    pub proposals_written: Vec<String>,
    pub branches_created: Vec<String>,
    pub skipped: Vec<SkippedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedItem {
    pub name: String,
    pub reason: String,
}
```

### ExperimentRegistry

Manages experiment lifecycle and persistence.

```rust
pub struct ExperimentRegistry {
    experiments: HashMap<String, Experiment>,
    data_dir: PathBuf,
}
```

- Persists to `{data_dir}/experiments/experiments.json`
- Loads on startup if file exists
- Thread-safe access via `Arc<Mutex<ExperimentRegistry>>` in HttpState

---

## Endpoints

### GET /v1/experiments

List all experiments with summary information.

Query parameters:
- `status` (optional): filter by status (`queued`, `running`, `completed`, `stopped`, `failed`)

Response 200:
```json
{
  "experiments": [
    {
      "id": "exp_1741977000_a1b2",
      "name": "Prompt tournament v3",
      "status": "running",
      "kind": "proof_of_fitness",
      "score_summary": "2 plans generated, 1 proposal written",
      "created_at": 1741977000
    },
    {
      "id": "exp_1741970000_c3d4",
      "name": "Analysis pass",
      "status": "completed",
      "kind": "analysis_only",
      "score_summary": "3 findings detected",
      "created_at": 1741970000
    }
  ],
  "total": 2
}
```

**Implementation notes:**
- `score_summary` is a computed human-readable string summarizing the experiment result:
  - Running: progress description (e.g., "round 2 of 4")
  - Completed: result summary (e.g., "3 plans generated, 2 proposals written")
  - Stopped/Failed: terminal state description
  - Queued: "waiting to start"
- Sorted by `created_at` descending (most recent first)
- When no experiments exist, return empty list with `total: 0`

### POST /v1/experiments

Create and start a new experiment.

Request:
```json
{
  "name": "Prompt tournament v3",
  "kind": "proof_of_fitness",
  "config": {
    "population": 16,
    "rounds": 4
  }
}
```

Response 201:
```json
{
  "id": "exp_1741977000_a1b2",
  "created": true,
  "status": "queued"
}
```

Response 422 (validation error):
```json
{
  "error": "name is required"
}
```

**Implementation notes:**
- `id` is generated server-side: `exp_{timestamp_secs}_{random_4hex}`
- `name` is required, non-empty, max 200 chars
- `kind` is required. Must be one of: `proof_of_fitness`, `analysis_only`, `tournament`
- `config` is optional; defaults apply if omitted
- The experiment is created in `Queued` status and immediately transitioned to `Running`
- Execution runs in a background `tokio::spawn` task:
  1. Maps `ExperimentConfig` → `ImprovementConfig`
  2. Calls `run_improvement_cycle()` from fx-improve
  3. On completion: updates status to `Completed` with results
  4. On error: updates status to `Failed` with error message
- `fleet_nodes` is populated with currently online node IDs when the experiment starts (informational — actual fleet dispatch is Phase 2)

### GET /v1/experiments/{id}

Returns detailed experiment information.

Response 200:
```json
{
  "id": "exp_1741977000_a1b2",
  "name": "Prompt tournament v3",
  "kind": "proof_of_fitness",
  "status": "running",
  "created_at": 1741977000,
  "started_at": 1741977060,
  "completed_at": null,
  "fleet_nodes": ["mac-mini-1a2b3c4d", "vps-primary-5e6f7g8h"],
  "config": {
    "population": 16,
    "rounds": 4
  },
  "progress": {
    "completed_matches": 18,
    "total_matches": 32
  },
  "error": null
}
```

Response 404:
```json
{
  "error": "experiment not found"
}
```

**Implementation notes:**
- `progress` is non-null only when status is `Running`
- `completed_at` is non-null only when status is `Completed`, `Stopped`, or `Failed`
- `error` is non-null only when status is `Failed`

### GET /v1/experiments/{id}/results

Returns experiment results: scores, chains, and tournament data.

Response 200 (completed experiment):
```json
{
  "id": "exp_1741977000_a1b2",
  "status": "completed",
  "leaders": [
    {
      "chain_id": "chain-b",
      "name": "Timeout budget increase",
      "score": 91.2,
      "risk": "low"
    },
    {
      "chain_id": "chain-f",
      "name": "Retry policy refactor",
      "score": 88.7,
      "risk": "medium"
    }
  ],
  "tournament": {
    "round": 4,
    "total_rounds": 4,
    "remaining_matches": 0
  },
  "plans_generated": 2,
  "proposals_written": [
    "proposals/improvement-timeout-budget.md",
    "proposals/improvement-retry-policy.md"
  ],
  "branches_created": [],
  "skipped": [
    {
      "name": "Config restructure",
      "reason": "model did not produce a plan"
    }
  ]
}
```

Response 200 (running experiment — partial results):
```json
{
  "id": "exp_1741977000_a1b2",
  "status": "running",
  "leaders": [],
  "tournament": {
    "round": 2,
    "total_rounds": 4,
    "remaining_matches": 12
  },
  "plans_generated": 0,
  "proposals_written": [],
  "branches_created": [],
  "skipped": []
}
```

Response 404:
```json
{
  "error": "experiment not found"
}
```

**Implementation notes:**
- `leaders` maps from `ImprovementRunResult` — each successfully planned candidate becomes a "leader" entry
- `chain_id` = the candidate's fingerprint (first 12 chars)
- `name` = the finding's `pattern_name`
- `score` = not yet computed (improvement pipeline doesn't produce numeric scores). For Phase 5.5, use a placeholder score derived from confidence: High=90.0, Medium=70.0, Low=50.0. Real scoring is a future track.
- `risk` = `FixPlan::risk` serialized as string
- `tournament` info is derived from `ExperimentConfig` rounds and current progress
- `proposals_written`, `branches_created`, `skipped` map directly from `ImprovementRunResult`

### POST /v1/experiments/{id}/stop

Stop a running experiment.

Response 200:
```json
{
  "id": "exp_1741977000_a1b2",
  "stopping": true
}
```

Response 404:
```json
{
  "error": "experiment not found"
}
```

Response 409 (not running):
```json
{
  "error": "experiment is not running (status: completed)"
}
```

**Implementation notes:**
- Sets a cancellation flag on the experiment. The background task checks this flag between stages.
- If the experiment is `Queued`, transitions directly to `Stopped`.
- If `Running`, sets a `CancellationToken` that the background task monitors. The task should check cancellation between the analyze → detect → plan → execute stages.
- Final status after stop: `Stopped` with `completed_at` set to current time.

---

## ExperimentRegistry Implementation

```rust
impl ExperimentRegistry {
    pub fn new(data_dir: &Path) -> Result<Self, std::io::Error>;
    pub fn load(data_dir: &Path) -> Result<Self, std::io::Error>;
    pub fn create(&mut self, name: String, kind: ExperimentKind, config: ExperimentConfig) -> Experiment;
    pub fn get(&self, id: &str) -> Option<&Experiment>;
    pub fn list(&self) -> Vec<&Experiment>;
    pub fn list_by_status(&self, status: ExperimentStatus) -> Vec<&Experiment>;
    pub fn start(&mut self, id: &str) -> Result<(), String>;
    pub fn complete(&mut self, id: &str, result: ExperimentResult) -> Result<(), String>;
    pub fn fail(&mut self, id: &str, error: String) -> Result<(), String>;
    pub fn stop(&mut self, id: &str) -> Result<(), String>;
    pub fn update_progress(&mut self, id: &str, progress: ExperimentProgress) -> Result<(), String>;
    pub fn persist(&self) -> Result<(), std::io::Error>;
}
```

Persistence: JSON file at `{data_dir}/experiments/experiments.json`. Written atomically (write to `.tmp`, rename).

---

## Cancellation

Use `tokio_util::sync::CancellationToken` for experiment cancellation:

```rust
pub struct RunningExperiment {
    pub cancel_token: CancellationToken,
    pub join_handle: JoinHandle<()>,
}
```

Store active `RunningExperiment` instances in a separate `HashMap<String, RunningExperiment>` alongside the registry (not serialized).

---

## Files to Create/Modify

1. **NEW: `engine/crates/fx-api/src/handlers/experiments.rs`** — all five handler functions + response types + ExperimentRegistry
2. **MODIFY: `engine/crates/fx-api/src/handlers/mod.rs`** — add `pub mod experiments;`
3. **MODIFY: `engine/crates/fx-api/src/router.rs`** — add routes to `v1_router`:
   ```rust
   .route("/experiments", get(handlers::experiments::handle_list_experiments)
       .post(handlers::experiments::handle_create_experiment))
   .route("/experiments/{id}", get(handlers::experiments::handle_get_experiment))
   .route("/experiments/{id}/results", get(handlers::experiments::handle_get_results))
   .route("/experiments/{id}/stop", post(handlers::experiments::handle_stop_experiment))
   ```
4. **MODIFY: `engine/crates/fx-api/src/state.rs`** — add `pub experiment_registry: Arc<Mutex<ExperimentRegistry>>` and `pub running_experiments: Arc<Mutex<HashMap<String, RunningExperiment>>>` to `HttpState`
5. **MODIFY: `engine/crates/fx-api/src/lib.rs`** (or wherever `HttpState` is constructed) — initialize ExperimentRegistry on startup
6. **MODIFY: `engine/crates/fx-improve/src/lib.rs`** — make `run_improvement_cycle` accept an optional `CancellationToken` parameter for cooperative cancellation (check between stages)

**Alternative: separate crate.** If the experiment registry grows complex, consider extracting to `engine/crates/fx-experiment/` with its own types and persistence. For Phase 5.5 MVP, keeping it in the handler module is acceptable — it can be extracted later if it grows.

---

## Tests Required

### Handler tests (in `experiments.rs`)

1. `list_experiments_returns_empty` — GET /v1/experiments with no experiments returns empty list
2. `list_experiments_returns_all` — GET /v1/experiments returns all experiments sorted by created_at desc
3. `list_experiments_filters_by_status` — GET /v1/experiments?status=running returns only running
4. `create_experiment_succeeds` — POST /v1/experiments creates experiment and returns 201
5. `create_experiment_validates_name_required` — POST without name returns 422
6. `create_experiment_validates_name_max_length` — POST with >200 char name returns 422
7. `create_experiment_validates_kind` — POST with invalid kind returns 422
8. `create_experiment_applies_config_defaults` — POST without config uses default population/rounds
9. `get_experiment_returns_detail` — GET /v1/experiments/{id} returns full experiment
10. `get_experiment_returns_404` — GET /v1/experiments/{bad_id} returns 404
11. `get_results_returns_results` — GET /v1/experiments/{id}/results returns results for completed experiment
12. `get_results_returns_partial_for_running` — partial results while running
13. `get_results_returns_404` — GET /v1/experiments/{bad_id}/results returns 404
14. `stop_experiment_succeeds` — POST /v1/experiments/{id}/stop stops running experiment
15. `stop_experiment_returns_409_for_completed` — POST stop on completed returns 409
16. `stop_experiment_returns_404` — POST stop on unknown returns 404
17. `stop_queued_experiment_transitions_to_stopped` — POST stop on queued goes straight to stopped

### ExperimentRegistry unit tests

18. `registry_create_assigns_unique_ids` — two creates produce different IDs
19. `registry_lifecycle_queued_to_running_to_completed` — full lifecycle state transitions
20. `registry_persist_and_load_roundtrip` — save and reload preserves all experiments
21. `registry_list_by_status_filters_correctly` — only matching status returned
22. `registry_start_rejects_non_queued` — start on completed returns error
23. `registry_complete_rejects_non_running` — complete on queued returns error
24. `registry_stop_sets_completed_at` — stop records completion timestamp

### Integration mapping tests

25. `experiment_config_maps_to_improvement_config` — ExperimentConfig → ImprovementConfig mapping is correct
26. `improvement_result_maps_to_experiment_result` — ImprovementRunResult → ExperimentResult mapping is correct
27. `score_summary_formats_correctly` — various states produce correct human-readable summaries

---

## Acceptance Criteria

- `GET /v1/experiments` lists experiments with summary and score info
- `POST /v1/experiments` creates and starts a new experiment in a background task
- `GET /v1/experiments/{id}` returns full experiment detail with config and progress
- `GET /v1/experiments/{id}/results` returns leaders, tournament state, proposals, and skipped candidates
- `POST /v1/experiments/{id}/stop` cancels a running experiment cooperatively
- Experiment state persists across server restarts (completed experiments survive; running experiments transition to `Failed` on restart)
- All endpoints return standard error format `{"error": "<message>"}`
- Authenticated via standard `auth_middleware`
- All existing tests pass, clippy clean

---

## Open Questions

1. **Scoring model:** The existing improvement pipeline doesn't produce numeric scores for candidates. The spec shows `score: 91.2` in results. For Phase 5.5, should we use placeholder scores based on confidence level, or skip the `score` field until real evaluation scoring exists? Spec recommends confidence-based placeholders.

2. **Fleet integration for experiments:** Phase 5.5 spec says experiments run across fleet nodes. The current `run_improvement_cycle()` runs locally. Should this track add fleet dispatch for experiments, or should fleet-distributed experiments be a follow-up? Recommend: local-only for Phase 5.5 MVP, populate `fleet_nodes` with local node ID only. Fleet distribution is a separate track.

3. **SSE for experiment progress:** The Phase 5 spec §18.6 mentions "long-running experiment progress should ideally reuse SSE or an event stream instead of pure polling." Should this track include SSE events for experiment status changes, or is polling sufficient for MVP? Recommend: polling for MVP, SSE in a follow-up.

4. **Experiment limits:** Should there be a limit on concurrent running experiments? The improvement pipeline uses LLM calls which have cost implications. Recommend: limit to 1 concurrent running experiment for MVP, configurable later.

5. **ExperimentRegistry location:** Should `ExperimentRegistry` live in the handler module (simple, co-located) or in a new `fx-experiment` crate (cleaner separation, reusable)? For MVP, handler module is fine. Extract to crate if it grows beyond ~300 lines.

6. **Restart recovery:** When the server restarts, experiments that were `Running` have lost their background task. Should they transition to `Failed` with an error like "server restarted during execution", or to `Stopped`? Recommend: `Failed` with "interrupted by server restart" error message — it's more honest about what happened.
