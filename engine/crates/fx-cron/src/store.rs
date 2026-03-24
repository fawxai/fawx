use crate::{types::CronJob, CronError, JobRun};
use fx_storage::Storage;
use std::path::Path;
use uuid::Uuid;

const JOBS_TABLE: &str = "cron_jobs";
const RUNS_TABLE: &str = "cron_runs";
const MAX_RUN_HISTORY: usize = 20;

#[derive(Clone)]
pub struct CronStore {
    storage: Storage,
}

impl std::fmt::Debug for CronStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CronStore").finish_non_exhaustive()
    }
}

impl CronStore {
    pub fn open(path: &Path) -> Result<Self, CronError> {
        let storage = Storage::open(path)?;
        Ok(Self { storage })
    }

    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub fn list_jobs(&self) -> Result<Vec<CronJob>, CronError> {
        let keys = self.storage.list_keys(JOBS_TABLE)?;
        let mut jobs = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(job) = self.load_job(&key)? {
                jobs.push(job);
            }
        }
        jobs.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        Ok(jobs)
    }

    pub fn get_job(&self, id: Uuid) -> Result<Option<CronJob>, CronError> {
        self.load_job(&id.to_string())
    }

    pub fn upsert_job(&self, job: &CronJob) -> Result<(), CronError> {
        let bytes = serde_json::to_vec(job)?;
        self.storage.put(JOBS_TABLE, &job.id.to_string(), &bytes)?;
        Ok(())
    }

    pub fn delete_job(&self, id: Uuid) -> Result<bool, CronError> {
        let id_string = id.to_string();
        let deleted_job = self.storage.delete(JOBS_TABLE, &id_string)?;
        let _ = self.storage.delete(RUNS_TABLE, &id_string)?;
        Ok(deleted_job)
    }

    pub fn list_runs(&self, job_id: Uuid) -> Result<Vec<JobRun>, CronError> {
        let Some(bytes) = self.storage.get(RUNS_TABLE, &job_id.to_string())? else {
            return Ok(Vec::new());
        };
        let runs = serde_json::from_slice(&bytes)?;
        Ok(runs)
    }

    pub fn record_run(&self, run: &JobRun) -> Result<(), CronError> {
        let mut runs = self.list_runs(run.job_id)?;
        runs.push(run.clone());
        trim_runs(&mut runs);
        let bytes = serde_json::to_vec(&runs)?;
        self.storage
            .put(RUNS_TABLE, &run.job_id.to_string(), &bytes)?;
        Ok(())
    }

    fn load_job(&self, key: &str) -> Result<Option<CronJob>, CronError> {
        let Some(bytes) = self.storage.get(JOBS_TABLE, key)? else {
            return Ok(None);
        };
        let job = serde_json::from_slice(&bytes)?;
        Ok(Some(job))
    }
}

fn trim_runs(runs: &mut Vec<JobRun>) {
    if runs.len() <= MAX_RUN_HISTORY {
        return;
    }
    let excess = runs.len().saturating_sub(MAX_RUN_HISTORY);
    runs.drain(0..excess);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobPayload, RunStatus, Schedule};

    fn test_store() -> CronStore {
        CronStore::new(Storage::open_in_memory().expect("storage"))
    }

    fn test_job() -> CronJob {
        CronJob {
            id: Uuid::new_v4(),
            name: Some("job".to_string()),
            schedule: Schedule::At { at_ms: 1_000 },
            payload: JobPayload::SystemEvent {
                session_key: "main".to_string(),
                text: "ping".to_string(),
            },
            enabled: true,
            created_at: 1,
            updated_at: 1,
            last_run_at: None,
            next_run_at: Some(1_000),
            run_count: 0,
        }
    }

    #[test]
    fn store_roundtrip_job() {
        let store = test_store();
        let job = test_job();
        store.upsert_job(&job).expect("save");
        let loaded = store.get_job(job.id).expect("load").expect("job");
        assert_eq!(loaded, job);
    }

    #[test]
    fn store_delete_job() {
        let store = test_store();
        let job = test_job();
        store.upsert_job(&job).expect("save");
        assert!(store.delete_job(job.id).expect("delete"));
        assert!(store.get_job(job.id).expect("load").is_none());
    }

    #[test]
    fn store_record_run_ring_buffer() {
        let store = test_store();
        let job = test_job();
        for index in 0..25 {
            store
                .record_run(&JobRun {
                    job_id: job.id,
                    run_id: Uuid::new_v4(),
                    started_at: index,
                    finished_at: Some(index),
                    status: RunStatus::Completed,
                    error: None,
                })
                .expect("record");
        }
        let runs = store.list_runs(job.id).expect("runs");
        assert_eq!(runs.len(), 20);
        assert_eq!(runs.first().expect("first").started_at, 5);
    }

    #[test]
    fn list_jobs_returns_sorted_jobs() {
        let store = test_store();
        let mut first = test_job();
        let mut second = test_job();
        first.created_at = 1;
        second.created_at = 2;
        store.upsert_job(&second).expect("save second");
        store.upsert_job(&first).expect("save first");
        let jobs = store.list_jobs().expect("list");
        assert_eq!(jobs[0].created_at, 1);
    }
}
