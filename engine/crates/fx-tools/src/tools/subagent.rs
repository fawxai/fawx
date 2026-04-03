use super::{parse_args, to_tool_result, ToolRegistry};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_kernel::act::{ToolCacheability, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use fx_subagent::{
    SpawnConfig, SpawnMode, SubagentControl, SubagentHandle, SubagentId, SubagentStatus,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(SpawnAgentTool::new(context));
    registry.register(SubagentStatusTool::new(context));
}

struct SpawnAgentTool {
    context: Arc<ToolContext>,
}

struct SubagentStatusTool {
    context: Arc<ToolContext>,
}

impl SpawnAgentTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl SubagentStatusTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &'static str {
        "spawn_agent"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Spawn an isolated subagent to handle a task. Returns a subagent ID for monitoring."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The task or prompt for the subagent"
                    },
                    "label": {
                        "type": "string",
                        "description": "Human-readable label for identification"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["run", "session"],
                        "description": "run = one-shot (default), session = persistent"
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Maximum execution time in seconds (default: 600)"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory for the subagent"
                    }
                },
                "required": ["task"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_spawn_agent(&call.arguments).await,
        )
    }

    fn is_available(&self) -> bool {
        self.context.subagent_control.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}

#[async_trait]
impl Tool for SubagentStatusTool {
    fn name(&self) -> &'static str {
        "subagent_status"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Check status of a subagent, list all subagents, or cancel one."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["status", "list", "cancel", "send"],
                        "description": "Action to perform"
                    },
                    "id": {
                        "type": "string",
                        "description": "Subagent ID (required for status/cancel/send)"
                    },
                    "message": {
                        "type": "string",
                        "description": "Message to send (required for send action)"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_subagent_status(&call.arguments).await,
        )
    }

    fn is_available(&self) -> bool {
        self.context.subagent_control.is_some()
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}

#[derive(Deserialize)]
struct SpawnAgentArgs {
    task: String,
    label: Option<String>,
    model: Option<String>,
    mode: Option<String>,
    timeout_seconds: Option<u64>,
    cwd: Option<String>,
}

#[derive(Deserialize)]
struct SubagentStatusArgs {
    action: String,
    id: Option<String>,
    message: Option<String>,
}

enum SubagentAction {
    Status,
    List,
    Cancel,
    Send,
}

impl SpawnAgentArgs {
    fn into_spawn_config(self) -> Result<SpawnConfig, String> {
        reject_model_override(self.model.as_deref())?;
        Ok(SpawnConfig {
            label: self.label,
            task: self.task,
            model: None,
            thinking: None,
            mode: parse_spawn_mode(self.mode.as_deref())?,
            timeout: Duration::from_secs(self.timeout_seconds.unwrap_or(600)),
            max_tokens: None,
            cwd: self.cwd.map(PathBuf::from),
            system_prompt: None,
        })
    }
}

impl ToolContext {
    fn subagent_control(&self) -> Result<&Arc<dyn SubagentControl>, String> {
        self.subagent_control
            .as_ref()
            .ok_or_else(|| "subagent control not configured".to_string())
    }

    pub(crate) async fn handle_spawn_agent(
        &self,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        let control = self.subagent_control()?;
        let parsed: SpawnAgentArgs = parse_args(args)?;
        let config = parsed.into_spawn_config()?;
        let handle = control
            .spawn(config)
            .await
            .map_err(|error| error.to_string())?;
        serialize_output(spawned_handle_value(&handle))
    }

    pub(crate) async fn handle_subagent_status(
        &self,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        let control = self.subagent_control()?;
        let parsed: SubagentStatusArgs = parse_args(args)?;
        let action = parse_subagent_action(&parsed.action)?;
        let output = match action {
            SubagentAction::List => list_subagents_output(control).await?,
            SubagentAction::Status => status_subagent_output(control, parsed.id).await?,
            SubagentAction::Cancel => cancel_subagent_output(control, parsed.id).await?,
            SubagentAction::Send => {
                send_subagent_output(control, parsed.id, parsed.message).await?
            }
        };
        serialize_output(output)
    }
}

fn serialize_output(value: serde_json::Value) -> Result<String, String> {
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

fn spawned_handle_value(handle: &SubagentHandle) -> serde_json::Value {
    serde_json::json!({
        "id": handle.id.0.clone(),
        "label": handle.label.clone(),
        "mode": spawn_mode_name(&handle.mode),
        "status": subagent_status_value(&handle.status),
        "initial_response": handle.initial_response.clone(),
    })
}

fn subagent_status_value(status: &SubagentStatus) -> serde_json::Value {
    match status {
        SubagentStatus::Running => serde_json::json!({ "state": "running" }),
        SubagentStatus::Completed {
            result,
            tokens_used,
        } => serde_json::json!({
            "state": "completed",
            "result": result,
            "tokens_used": tokens_used,
        }),
        SubagentStatus::Failed { error } => {
            serde_json::json!({ "state": "failed", "error": error })
        }
        SubagentStatus::Cancelled => serde_json::json!({ "state": "cancelled" }),
        SubagentStatus::TimedOut => serde_json::json!({ "state": "timed_out" }),
    }
}

fn spawn_mode_name(mode: &SpawnMode) -> &'static str {
    match mode {
        SpawnMode::Run => "run",
        SpawnMode::Session => "session",
    }
}

async fn list_subagents_output(
    control: &Arc<dyn SubagentControl>,
) -> Result<serde_json::Value, String> {
    let handles = control.list().await.map_err(|error| error.to_string())?;
    let subagents = handles.iter().map(spawned_handle_value).collect::<Vec<_>>();
    Ok(serde_json::json!({ "subagents": subagents }))
}

async fn status_subagent_output(
    control: &Arc<dyn SubagentControl>,
    id: Option<String>,
) -> Result<serde_json::Value, String> {
    let id = required_subagent_id(id, "status")?;
    let handle = require_subagent_handle(control, &id).await?;
    Ok(spawned_handle_value(&handle))
}

async fn cancel_subagent_output(
    control: &Arc<dyn SubagentControl>,
    id: Option<String>,
) -> Result<serde_json::Value, String> {
    let id = required_subagent_id(id, "cancel")?;
    control
        .cancel(&id)
        .await
        .map_err(|error| error.to_string())?;
    let handle = require_subagent_handle(control, &id).await?;
    Ok(spawned_handle_value(&handle))
}

async fn send_subagent_output(
    control: &Arc<dyn SubagentControl>,
    id: Option<String>,
    message: Option<String>,
) -> Result<serde_json::Value, String> {
    let id = required_subagent_id(id, "send")?;
    let message = required_send_message(message)?;
    let response = control
        .send(&id, &message)
        .await
        .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "id": id.0,
        "response": response,
    }))
}

fn required_subagent_id(id: Option<String>, action: &str) -> Result<SubagentId, String> {
    let id = id.ok_or_else(|| format!("id is required for '{action}' action"))?;
    if id.trim().is_empty() {
        return Err(format!("id is required for '{action}' action"));
    }
    Ok(SubagentId(id))
}

fn required_send_message(message: Option<String>) -> Result<String, String> {
    let message = message.ok_or_else(|| "message is required for 'send' action".to_string())?;
    if message.trim().is_empty() {
        return Err("message is required for 'send' action".to_string());
    }
    Ok(message)
}

async fn require_subagent_handle(
    control: &Arc<dyn SubagentControl>,
    id: &SubagentId,
) -> Result<SubagentHandle, String> {
    control
        .list()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|handle| &handle.id == id)
        .ok_or_else(|| format!("subagent '{id}' not found"))
}

fn parse_subagent_action(action: &str) -> Result<SubagentAction, String> {
    match action {
        "status" => Ok(SubagentAction::Status),
        "list" => Ok(SubagentAction::List),
        "cancel" => Ok(SubagentAction::Cancel),
        "send" => Ok(SubagentAction::Send),
        other => Err(format!(
            "unknown subagent action '{other}', valid actions: status, list, cancel, send"
        )),
    }
}

fn parse_spawn_mode(mode: Option<&str>) -> Result<SpawnMode, String> {
    match mode.unwrap_or("run") {
        "run" => Ok(SpawnMode::Run),
        "session" => Ok(SpawnMode::Session),
        other => Err(format!(
            "unknown spawn mode '{other}', valid modes: run, session"
        )),
    }
}

fn reject_model_override(model: Option<&str>) -> Result<(), String> {
    if model.is_some() {
        return Err("model override is not supported for headless subagents".to_string());
    }
    Ok(())
}
