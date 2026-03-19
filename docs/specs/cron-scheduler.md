# Spec: Cron / Scheduled Tasks

**Status**: Ready for implementation
**Crates**: `fx-cron` (new), `fx-api`, `fx-cli`
**Estimated scope**: ~600 lines production + ~300 lines tests

---

## Problem

Fawx has no way to schedule recurring or deferred tasks. Users can't set
reminders, schedule periodic checks, or run automated workflows on a timer.
This is a critical gap for the "daily driver" use case and a prerequisite for
Fawx replacing OpenClaw.

## Solution

A new `fx-cron` crate providing daemon-level scheduled task execution that
persists across restarts. Jobs are stored in a redb database, evaluated on a
tick loop, and executed by injecting messages into sessions via `fx-bus`.

---

## Design

### 1. Data Model (`fx-cron/src/types.rs`)

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A scheduled job definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: Uuid,
    pub name: Option<String>,
    pub schedule: Schedule,
    pub payload: JobPayload,
    pub enabled: bool,
    pub created_at: u64,      // epoch ms
    pub updated_at: u64,      // epoch ms
    pub last_run_at: Option<u64>,
    pub next_run_at: Option<u64>,
    pub run_count: u64,
}

/// When to run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Schedule {
    /// One-shot at an absolute time. Auto-disables after firing.
    At { at_ms: u64 },
    /// Recurring interval.
    Every { every_ms: u64, anchor_ms: Option<u64> },
    /// Cron expression (standard 5-field).
    Cron { expr: String, tz: Option<String> },
}

/// What to do when the schedule fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum JobPayload {
    /// Inject a system event message into a session.
    SystemEvent {
        session_key: String,
        text: String,
    },
    /// Run an agent turn in an isolated session.
    AgentTurn {
        message: String,
        model: Option<String>,
        timeout_seconds: Option<u64>,
    },
}

/// Result of a single job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRun {
    pub job_id: Uuid,
    pub run_id: Uuid,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: RunStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}
```

### 2. Persistence (`fx-cron/src/store.rs`)

Use `fx-storage` (redb) for persistence. Two tables:
- `jobs` — `Uuid → CronJob` (serialized as JSON bytes)
- `runs` — `Uuid → Vec<JobRun>` (last 20 runs per job, ring buffer)

```rust
pub struct CronStore {
    storage: fx_storage::Storage,
}

impl CronStore {
    pub fn open(path: &Path) -> Result<Self, CronError>;
    pub fn list_jobs(&self) -> Result<Vec<CronJob>, CronError>;
    pub fn get_job(&self, id: Uuid) -> Result<Option<CronJob>, CronError>;
    pub fn upsert_job(&self, job: &CronJob) -> Result<(), CronError>;
    pub fn delete_job(&self, id: Uuid) -> Result<bool, CronError>;
    pub fn list_runs(&self, job_id: Uuid) -> Result<Vec<JobRun>, CronError>;
    pub fn record_run(&self, run: &JobRun) -> Result<(), CronError>;
}
```

### 3. Scheduler Loop (`fx-cron/src/scheduler.rs`)

A tokio task that ticks every 15 seconds, checks for due jobs, and executes
them:

```rust
pub struct Scheduler {
    store: Arc<Mutex<CronStore>>,
    bus: Arc<SessionBus>,
    cancel: CancellationToken,
}

impl Scheduler {
    pub fn new(
        store: Arc<Mutex<CronStore>>,
        bus: Arc<SessionBus>,
        cancel: CancellationToken,
    ) -> Self;

    /// Start the tick loop. Returns a JoinHandle.
    pub fn start(self) -> tokio::task::JoinHandle<()>;
}
```

Tick logic:
1. Load all enabled jobs from store
2. For each job where `next_run_at <= now`:
   a. Execute payload (send message via bus, or spawn isolated session)
   b. Record `JobRun` (status, timing)
   c. Compute `next_run_at` from schedule (or disable if `At` type)
   d. Update job in store
3. Sleep until next tick (15s default, configurable)

For `AgentTurn` payloads: create a `Cron`-kind session, send the message,
and let the engine process it. The session is ephemeral (cleaned up after
completion or timeout).

### 4. Schedule Evaluation (`fx-cron/src/eval.rs`)

```rust
/// Compute the next run time for a schedule.
pub fn next_run_time(schedule: &Schedule, now_ms: u64) -> Option<u64>;

/// Check if a job is due.
pub fn is_due(job: &CronJob, now_ms: u64) -> bool;
```

For `Cron` expressions, use the `cron` crate (lightweight, well-maintained,
no unsafe). For `At` and `Every`, compute directly.

### 5. HTTP API (`fx-api`)

New route group under `/v1/cron/`:

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/cron/jobs` | List all jobs |
| `POST` | `/v1/cron/jobs` | Create a job |
| `GET` | `/v1/cron/jobs/{id}` | Get job details |
| `PUT` | `/v1/cron/jobs/{id}` | Update a job |
| `DELETE` | `/v1/cron/jobs/{id}` | Delete a job |
| `POST` | `/v1/cron/jobs/{id}/run` | Trigger immediate run |
| `GET` | `/v1/cron/jobs/{id}/runs` | Get run history |

Request/response shapes follow the data model. `POST /jobs` accepts a
`CreateJobRequest` (schedule + payload + optional name), returns the created
`CronJob` with generated `id` and computed `next_run_at`.

### 6. CLI Commands (`fx-cli`)

```
fawx cron list                    # List all scheduled jobs
fawx cron add <schedule> <text>   # Quick-add a reminder/task
fawx cron remove <id>             # Delete a job
fawx cron run <id>                # Trigger immediate execution
fawx cron history <id>            # Show run history
```

### 7. Agent Tool (`fx-tools`)

A `CronSkill` exposing tools to the agent:
- `cron_list` — list scheduled jobs
- `cron_add` — create a new job
- `cron_remove` — delete a job
- `cron_run` — trigger immediate execution

This allows the agent to self-schedule tasks, set reminders, and manage
recurring workflows conversationally.

### 8. Startup Wiring (`fx-cli/src/startup.rs`)

In `build_loop_engine_with_options`:
1. Open `cron.redb` in data dir
2. Create `CronStore`
3. Create `Scheduler` with store + bus + cancel token
4. Start scheduler tick loop
5. Pass store to `CronSkill` for agent tool access

In `http_serve.rs`:
- Pass `CronStore` to fx-api for HTTP endpoints
- Same double-open avoidance pattern as sessions.redb (caller opens once)

---

## What NOT to Do

- **No cron expression parsing from scratch.** Use the `cron` crate.
- **No separate daemon process.** The scheduler runs as a tokio task inside
  `fawx serve`. It starts with the server and stops with it.
- **No job queuing/retry.** V1 is fire-and-forget. If a job fails, it records
  the error and moves on. Retry logic is V2.
- **No webhook/HTTP callback payloads.** V1 supports SystemEvent and AgentTurn
  only. External callbacks are V2.
- **No sub-second scheduling.** Minimum interval is 60 seconds for `Every`,
  tick granularity is 15 seconds. This is a cron system, not a real-time
  scheduler.

---

## Dependencies

- `cron` crate (for cron expression parsing) — add to `fx-cron/Cargo.toml`
- `fx-storage` (redb) — existing
- `fx-bus` (session messaging) — existing
- `fx-session` (session types) — existing
- `uuid` — already in workspace

---

## Testing

### Unit tests (`fx-cron`)

1. `schedule_at_fires_once_then_disables` — At schedule: due at time, disabled after
2. `schedule_every_computes_next_run` — Every 60s: verify next_run_at increments
3. `schedule_cron_parses_expression` — "0 * * * *" → hourly
4. `is_due_returns_true_when_past_next_run` — now > next_run_at
5. `is_due_returns_false_when_before_next_run` — now < next_run_at
6. `is_due_returns_false_when_disabled` — enabled=false
7. `store_roundtrip_job` — Create, save, load, verify fields match
8. `store_delete_job` — Delete returns true, get returns None
9. `store_record_run_ring_buffer` — 25 runs recorded, only last 20 returned
10. `scheduler_executes_due_job` — Mock bus, verify message sent

### Integration tests (`fx-api`)

11. `api_create_job_returns_201` — POST /v1/cron/jobs with valid schedule
12. `api_list_jobs_includes_created` — Create then list, verify present
13. `api_delete_job_returns_204` — Delete, verify gone
14. `api_trigger_run_returns_200` — POST /v1/cron/jobs/{id}/run

### Agent tool tests (`fx-tools`)

15. `cron_add_creates_job` — Agent tool creates a job in the store
16. `cron_list_returns_jobs` — Agent tool lists existing jobs

---

## File Changes Summary

| File | Change |
|------|--------|
| `engine/crates/fx-cron/` | New crate: types, store, scheduler, eval |
| `engine/crates/fx-cron/Cargo.toml` | New: deps on fx-storage, fx-bus, fx-session, cron, uuid |
| `engine/crates/fx-api/src/handlers/cron.rs` | New: HTTP handlers for /v1/cron/* |
| `engine/crates/fx-api/src/lib.rs` | Wire cron routes |
| `engine/crates/fx-tools/src/cron_skill.rs` | New: CronSkill agent tool |
| `engine/crates/fx-cli/src/startup.rs` | Wire CronStore + Scheduler |
| `engine/crates/fx-cli/src/http_serve.rs` | Pass CronStore to fx-api |
| `engine/Cargo.toml` | Add fx-cron to workspace members |
