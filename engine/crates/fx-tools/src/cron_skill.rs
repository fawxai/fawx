use async_trait::async_trait;
use fx_bus::SessionBus;
use fx_cron::{
    next_run_time, now_ms, trigger_job, validate_schedule, CronJob, CronStore, JobPayload, Schedule,
};
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_loadable::{Skill, SkillError};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct CronSkill {
    store: Arc<Mutex<CronStore>>,
    bus: Option<SessionBus>,
    tool_names: HashSet<String>,
}

impl std::fmt::Debug for CronSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CronSkill").finish_non_exhaustive()
    }
}

impl CronSkill {
    pub fn new(store: Arc<Mutex<CronStore>>, bus: Option<SessionBus>) -> Self {
        let tool_names = cron_tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        Self {
            store,
            bus,
            tool_names,
        }
    }

    fn handles_tool(&self, tool_name: &str) -> bool {
        self.tool_names.contains(tool_name)
    }

    async fn execute_tool(&self, tool_name: &str, arguments: &str) -> Result<String, SkillError> {
        let value: serde_json::Value = serde_json::from_str(arguments)
            .map_err(|error| format!("malformed arguments: {error}"))?;
        match tool_name {
            "cron_list" => self.handle_list().await,
            "cron_add" => self.handle_add(value).await,
            "cron_get" => self.handle_get(value).await,
            "cron_remove" => self.handle_remove(value).await,
            "cron_run" => self.handle_run(value).await,
            "cron_history" => self.handle_history(value).await,
            _ => Err(format!("unknown cron tool: {tool_name}")),
        }
    }

    async fn handle_list(&self) -> Result<String, SkillError> {
        let jobs = self
            .store
            .lock()
            .await
            .list_jobs()
            .map_err(to_skill_error)?;
        serde_json::to_string_pretty(&jobs)
            .map_err(|error| format!("serialization failed: {error}"))
    }

    async fn handle_add(&self, value: serde_json::Value) -> Result<String, SkillError> {
        let args: CronAddArgs =
            serde_json::from_value(value).map_err(|error| format!("invalid arguments: {error}"))?;
        let now_ms = now_ms();
        let schedule = parse_quick_schedule(&args.schedule, now_ms)?;
        validate_schedule(&schedule).map_err(to_skill_error)?;
        let job = CronJob {
            id: Uuid::new_v4(),
            name: args.name,
            next_run_at: next_run_time(&schedule, now_ms),
            schedule,
            payload: JobPayload::AgentTurn { message: args.text },
            enabled: true,
            created_at: now_ms,
            updated_at: now_ms,
            last_run_at: None,
            run_count: 0,
        };
        self.store
            .lock()
            .await
            .upsert_job(&job)
            .map_err(to_skill_error)?;
        Ok(job.id.to_string())
    }

    async fn handle_get(&self, value: serde_json::Value) -> Result<String, SkillError> {
        let id = parse_job_id(value)?;
        let job = self
            .store
            .lock()
            .await
            .get_job(id)
            .map_err(to_skill_error)?;
        serde_json::to_string_pretty(&job).map_err(|error| format!("serialization failed: {error}"))
    }

    async fn handle_remove(&self, value: serde_json::Value) -> Result<String, SkillError> {
        let id = parse_job_id(value)?;
        let deleted = self
            .store
            .lock()
            .await
            .delete_job(id)
            .map_err(to_skill_error)?;
        Ok(if deleted { "deleted" } else { "not found" }.to_string())
    }

    async fn handle_run(&self, value: serde_json::Value) -> Result<String, SkillError> {
        let id = parse_job_id(value)?;
        let bus = self
            .bus
            .as_ref()
            .ok_or_else(|| "session bus unavailable".to_string())?;
        let run = trigger_job(&self.store, bus, id)
            .await
            .map_err(to_skill_error)?;
        serde_json::to_string_pretty(&run).map_err(|error| format!("serialization failed: {error}"))
    }

    async fn handle_history(&self, value: serde_json::Value) -> Result<String, SkillError> {
        let id = parse_job_id(value)?;
        let runs = self
            .store
            .lock()
            .await
            .list_runs(id)
            .map_err(to_skill_error)?;
        serde_json::to_string_pretty(&runs)
            .map_err(|error| format!("serialization failed: {error}"))
    }
}

#[async_trait]
impl Skill for CronSkill {
    fn name(&self) -> &str {
        "cron"
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        cron_tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        match tool_name {
            "cron_list" | "cron_get" | "cron_history" => ToolCacheability::NeverCache,
            "cron_add" | "cron_remove" | "cron_run" => ToolCacheability::SideEffect,
            _ => ToolCacheability::NeverCache,
        }
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        if !self.handles_tool(tool_name) {
            return None;
        }
        Some(self.execute_tool(tool_name, arguments).await)
    }
}

fn cron_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "cron_list".to_string(),
            description: "List scheduled cron jobs.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "cron_add".to_string(),
            description: "Create a scheduled cron job. schedule accepts either epoch ms or strings like every:60000.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "schedule": { "type": "string" },
                    "text": { "type": "string" },
                    "name": { "type": "string" }
                },
                "required": ["schedule", "text"]
            }),
        },
        ToolDefinition {
            name: "cron_get".to_string(),
            description: "Get a scheduled cron job by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "cron_remove".to_string(),
            description: "Delete a scheduled cron job by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "cron_run".to_string(),
            description: "Trigger a cron job manually by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "cron_history".to_string(),
            description: "List recent runs for a cron job by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
    ]
}

#[derive(Deserialize)]
struct CronAddArgs {
    schedule: String,
    text: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct CronIdArgs {
    id: String,
}

fn parse_job_id(value: serde_json::Value) -> Result<Uuid, SkillError> {
    let args: CronIdArgs =
        serde_json::from_value(value).map_err(|error| format!("invalid arguments: {error}"))?;
    Uuid::parse_str(&args.id).map_err(|error| format!("invalid job id: {error}"))
}

fn parse_quick_schedule(value: &str, now_ms: u64) -> Result<Schedule, SkillError> {
    if let Some(ms) = value.strip_prefix("every:") {
        let every_ms = ms
            .parse::<u64>()
            .map_err(|error| format!("invalid every_ms: {error}"))?;
        return Ok(Schedule::Every {
            every_ms,
            anchor_ms: Some(now_ms),
        });
    }
    if let Ok(at_ms) = value.parse::<u64>() {
        return Ok(Schedule::At { at_ms });
    }
    Ok(Schedule::Cron {
        expr: value.to_string(),
        tz: None,
    })
}

fn to_skill_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_bus::{BusStore, SessionBus};
    use fx_storage::Storage;

    fn test_store() -> Arc<Mutex<CronStore>> {
        Arc::new(Mutex::new(CronStore::new(
            Storage::open_in_memory().expect("storage"),
        )))
    }

    fn test_skill() -> (CronSkill, BusStore) {
        let bus_store = BusStore::new(Storage::open_in_memory().expect("bus storage"));
        let bus = SessionBus::new(bus_store.clone());
        let skill = CronSkill::new(test_store(), Some(bus));
        (skill, bus_store)
    }

    #[tokio::test]
    async fn cron_add_creates_job() {
        let (skill, _) = test_skill();
        let id = skill
            .execute_tool("cron_add", r#"{"schedule":"every:60000","text":"ping"}"#)
            .await
            .expect("id");
        let jobs = skill.store.lock().await.list_jobs().expect("jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id.to_string(), id);
    }

    #[tokio::test]
    async fn cron_list_returns_jobs() {
        let (skill, _) = test_skill();
        skill
            .execute_tool("cron_add", r#"{"schedule":"every:60000","text":"ping"}"#)
            .await
            .expect("add");
        let output = skill.execute_tool("cron_list", "{}").await.expect("list");
        assert!(output.contains("ping"));
    }

    #[tokio::test]
    async fn cron_run_errors_when_session_bus_is_unavailable() {
        let skill = CronSkill::new(test_store(), None);
        let error = skill
            .execute_tool("cron_run", &format!(r#"{{"id":"{}"}}"#, Uuid::new_v4()))
            .await
            .expect_err("missing bus should fail");

        assert!(error.contains("session bus unavailable"));
    }

    #[tokio::test]
    async fn cron_run_records_completed_history() {
        let (skill, bus_store) = test_skill();
        let id = skill
            .execute_tool("cron_add", r#"{"schedule":"every:60000","text":"ping"}"#)
            .await
            .expect("id");

        let run = skill
            .execute_tool("cron_run", &format!(r#"{{"id":"{id}"}}"#))
            .await
            .expect("run");
        let history = skill
            .execute_tool("cron_history", &format!(r#"{{"id":"{id}"}}"#))
            .await
            .expect("history");

        assert!(run.contains("completed"));
        assert!(history.contains("completed"));
        let runs = skill
            .store
            .lock()
            .await
            .list_runs(Uuid::parse_str(&id).expect("uuid"))
            .expect("runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, fx_cron::RunStatus::Completed);
        let session = fx_session::SessionKey::new(format!("cron-{id}")).expect("session key");
        assert_eq!(bus_store.count(&session).expect("queued"), 1);
    }
}
