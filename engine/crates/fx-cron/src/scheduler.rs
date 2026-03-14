use crate::{
    eval::validate_schedule, is_due, next_run_time, now_ms, CronError, CronJob, CronStore,
    JobPayload, JobRun, RunStatus, Schedule,
};
use async_trait::async_trait;
use fx_bus::{Envelope, Payload, SessionBus};
use fx_session::SessionKey;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const DEFAULT_TICK_SECONDS: u64 = 15;
const CRON_SESSION_PREFIX: &str = "cron";

#[async_trait]
pub trait CronBus: Send + Sync + 'static {
    async fn send(&self, envelope: Envelope) -> Result<(), CronError>;
}

#[async_trait]
impl CronBus for SessionBus {
    async fn send(&self, envelope: Envelope) -> Result<(), CronError> {
        self.send(envelope)
            .await
            .map(|_| ())
            .map_err(CronError::from)
    }
}

pub struct Scheduler<B: CronBus> {
    store: Arc<Mutex<CronStore>>,
    bus: B,
    cancel: CancellationToken,
    tick: Duration,
}

impl Scheduler<SessionBus> {
    pub fn new(store: Arc<Mutex<CronStore>>, bus: SessionBus, cancel: CancellationToken) -> Self {
        Self::with_bus(
            store,
            bus,
            cancel,
            Duration::from_secs(DEFAULT_TICK_SECONDS),
        )
    }
}

impl<B: CronBus> Scheduler<B> {
    pub fn with_bus(
        store: Arc<Mutex<CronStore>>,
        bus: B,
        cancel: CancellationToken,
        tick: Duration,
    ) -> Self {
        Self {
            store,
            bus,
            cancel,
            tick,
        }
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run_loop().await;
        })
    }

    async fn run_loop(self) {
        let mut interval = tokio::time::interval(self.tick);
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => break,
                _ = interval.tick() => {
                    if let Err(error) = self.tick_once().await {
                        tracing::warn!(error = %error, "cron scheduler tick failed");
                    }
                }
            }
        }
    }

    pub async fn tick_once(&self) -> Result<(), CronError> {
        execute_jobs_due(&self.store, &self.bus, now_ms()).await
    }
}

pub async fn execute_jobs_due<B: CronBus>(
    store: &Arc<Mutex<CronStore>>,
    bus: &B,
    now_ms: u64,
) -> Result<(), CronError> {
    let jobs = { store.lock().await.list_jobs()? };
    for job in jobs {
        if is_due(&job, now_ms) {
            execute_due_job(store, bus, job, now_ms).await?;
        }
    }
    Ok(())
}

pub async fn trigger_job<B: CronBus>(
    store: &Arc<Mutex<CronStore>>,
    bus: &B,
    job_id: Uuid,
) -> Result<Option<JobRun>, CronError> {
    let Some(mut job) = ({ store.lock().await.get_job(job_id)? }) else {
        return Ok(None);
    };
    let started_at = now_ms();
    let result = execute_job(bus, &job).await;
    let run = build_run(
        job.id,
        started_at,
        result.as_ref().err().map(ToString::to_string),
    );
    apply_post_run_updates(&mut job, started_at);
    persist_run_and_job(store, &run, &job).await?;
    result?;
    Ok(Some(run))
}

async fn execute_due_job<B: CronBus>(
    store: &Arc<Mutex<CronStore>>,
    bus: &B,
    mut job: CronJob,
    now_ms: u64,
) -> Result<(), CronError> {
    advance_job_before_run(store, &mut job, now_ms).await?;
    let result = execute_job(bus, &job).await;
    let run = build_run(
        job.id,
        now_ms,
        result.as_ref().err().map(ToString::to_string),
    );
    finalize_due_job(store, &mut job, now_ms, run).await?;
    Ok(())
}

async fn advance_job_before_run(
    store: &Arc<Mutex<CronStore>>,
    job: &mut CronJob,
    now_ms: u64,
) -> Result<(), CronError> {
    job.next_run_at = next_run_after_execution(&job.schedule, now_ms);
    job.updated_at = now_ms;
    store.lock().await.upsert_job(job)?;
    Ok(())
}

async fn finalize_due_job(
    store: &Arc<Mutex<CronStore>>,
    job: &mut CronJob,
    now_ms: u64,
    run: JobRun,
) -> Result<(), CronError> {
    apply_post_run_updates(job, now_ms);
    persist_run_and_job(store, &run, job).await
}

async fn persist_run_and_job(
    store: &Arc<Mutex<CronStore>>,
    run: &JobRun,
    job: &CronJob,
) -> Result<(), CronError> {
    let store = store.lock().await;
    store.record_run(run)?;
    store.upsert_job(job)?;
    Ok(())
}

pub async fn execute_job<B: CronBus>(bus: &B, job: &CronJob) -> Result<(), CronError> {
    validate_schedule(&job.schedule)?;
    let envelope = job_envelope(job)?;
    bus.send(envelope).await
}

fn job_envelope(job: &CronJob) -> Result<Envelope, CronError> {
    match &job.payload {
        JobPayload::SystemEvent { session_key, text } => {
            let to = SessionKey::new(session_key.clone())
                .map_err(|error| CronError::SessionKey(error.to_string()))?;
            Ok(Envelope::new(None, to, Payload::System(text.clone())))
        }
        JobPayload::AgentTurn { message } => {
            let to = cron_session_key(job.id)?;
            Ok(Envelope::new(None, to, Payload::Text(message.clone())))
        }
    }
}

fn cron_session_key(job_id: Uuid) -> Result<SessionKey, CronError> {
    SessionKey::new(format!("{CRON_SESSION_PREFIX}-{job_id}"))
        .map_err(|error| CronError::SessionKey(error.to_string()))
}

fn next_run_after_execution(schedule: &Schedule, now_ms: u64) -> Option<u64> {
    match schedule {
        Schedule::At { .. } => None,
        _ => next_run_time(schedule, now_ms),
    }
}

fn apply_post_run_updates(job: &mut CronJob, now_ms: u64) {
    job.last_run_at = Some(now_ms);
    job.updated_at = now_ms;
    job.run_count = job.run_count.saturating_add(1);
    if matches!(job.schedule, Schedule::At { .. }) {
        job.enabled = false;
        job.next_run_at = None;
    }
}

fn build_run(job_id: Uuid, now_ms: u64, error: Option<String>) -> JobRun {
    JobRun {
        job_id,
        run_id: Uuid::new_v4(),
        started_at: now_ms,
        finished_at: Some(now_ms),
        status: if error.is_some() {
            RunStatus::Failed
        } else {
            RunStatus::Completed
        },
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CronJob, JobPayload, Schedule};
    use std::sync::{Arc, Mutex as StdMutex};

    #[derive(Clone, Default)]
    struct MockBus {
        sent: Arc<StdMutex<Vec<Envelope>>>,
    }

    #[async_trait]
    impl CronBus for MockBus {
        async fn send(&self, envelope: Envelope) -> Result<(), CronError> {
            self.sent.lock().expect("sent").push(envelope);
            Ok(())
        }
    }

    fn due_job() -> CronJob {
        CronJob {
            id: Uuid::new_v4(),
            name: Some("due".to_string()),
            schedule: Schedule::Every {
                every_ms: 60_000,
                anchor_ms: Some(0),
            },
            payload: JobPayload::SystemEvent {
                session_key: "main".to_string(),
                text: "ping".to_string(),
            },
            enabled: true,
            created_at: 0,
            updated_at: 0,
            last_run_at: None,
            next_run_at: Some(0),
            run_count: 0,
        }
    }

    #[tokio::test]
    async fn scheduler_executes_due_job() {
        let bus = MockBus::default();
        let storage = fx_storage::Storage::open_in_memory().expect("storage");
        let store = Arc::new(Mutex::new(CronStore::new(storage)));
        let job = due_job();
        store.lock().await.upsert_job(&job).expect("save");
        let scheduler = Scheduler::with_bus(
            store.clone(),
            bus.clone(),
            CancellationToken::new(),
            Duration::from_secs(60),
        );
        scheduler.tick_once().await.expect("tick");
        let sent = bus.sent.lock().expect("sent");
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].payload, Payload::System("ping".to_string()));
    }

    #[tokio::test]
    async fn schedule_at_fires_once_then_disables() {
        let bus = MockBus::default();
        let store = Arc::new(Mutex::new(CronStore::new(
            fx_storage::Storage::open_in_memory().expect("storage"),
        )));
        let mut job = due_job();
        job.schedule = Schedule::At { at_ms: 0 };
        job.next_run_at = Some(0);
        store.lock().await.upsert_job(&job).expect("save");
        let scheduler = Scheduler::with_bus(
            store.clone(),
            bus,
            CancellationToken::new(),
            Duration::from_secs(60),
        );
        scheduler.tick_once().await.expect("tick");
        let updated = store
            .lock()
            .await
            .get_job(job.id)
            .expect("load")
            .expect("job");
        assert!(!updated.enabled);
        assert_eq!(updated.next_run_at, None);
    }

    #[tokio::test]
    async fn trigger_job_records_completed_run() {
        let bus = MockBus::default();
        let store = Arc::new(Mutex::new(CronStore::new(
            fx_storage::Storage::open_in_memory().expect("storage"),
        )));
        let job = due_job();
        store.lock().await.upsert_job(&job).expect("save");
        let run = trigger_job(&store, &bus, job.id)
            .await
            .expect("run")
            .expect("job exists");
        assert_eq!(run.status, RunStatus::Completed);
        let runs = store.lock().await.list_runs(job.id).expect("runs");
        assert_eq!(runs.len(), 1);
    }

    #[tokio::test]
    async fn due_job_advances_next_run_before_execution() {
        let bus = MockBus::default();
        let store = Arc::new(Mutex::new(CronStore::new(
            fx_storage::Storage::open_in_memory().expect("storage"),
        )));
        let job = due_job();
        store.lock().await.upsert_job(&job).expect("save");

        execute_jobs_due(&store, &bus, 60_000).await.expect("tick");

        let updated = store
            .lock()
            .await
            .get_job(job.id)
            .expect("load")
            .expect("job");
        assert_eq!(updated.next_run_at, Some(120_000));
        assert_eq!(updated.run_count, 1);
    }
}
