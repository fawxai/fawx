use std::sync::Arc;
use tokio::sync::Mutex;

pub mod eval;
pub mod scheduler;
pub mod store;
pub mod types;

pub use eval::{is_due, next_run_time, validate_schedule};
pub use scheduler::{execute_job, execute_jobs_due, trigger_job, CronBus, Scheduler};
pub use store::CronStore;
pub type SharedCronStore = Arc<Mutex<CronStore>>;
pub use types::{now_ms, CronJob, JobPayload, JobRun, RunStatus, Schedule};

#[derive(Debug, thiserror::Error)]
pub enum CronError {
    #[error("storage error: {0}")]
    Storage(#[from] fx_core::error::StorageError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),
    #[error("invalid schedule: {0}")]
    InvalidSchedule(String),
    #[error("session key error: {0}")]
    SessionKey(String),
    #[error("bus error: {0}")]
    Bus(#[from] fx_bus::BusError),
}
