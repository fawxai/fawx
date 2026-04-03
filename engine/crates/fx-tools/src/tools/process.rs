use super::{parse_args, to_tool_result, ToolRegistry};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_kernel::act::{ToolCacheability, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::{ListEntry, SpawnResult, StatusResult};
use fx_llm::{ToolCall, ToolDefinition};
use serde::Deserialize;
use std::sync::Arc;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(ExecBackgroundTool::new(context));
    registry.register(ExecStatusTool::new(context));
    registry.register(ExecKillTool::new(context));
}

struct ExecBackgroundTool {
    context: Arc<ToolContext>,
}

struct ExecStatusTool {
    context: Arc<ToolContext>,
}

struct ExecKillTool {
    context: Arc<ToolContext>,
}

impl ExecBackgroundTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl ExecStatusTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl ExecKillTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[async_trait]
impl Tool for ExecBackgroundTool {
    fn name(&self) -> &'static str {
        "exec_background"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Start a command in the background and return a session ID for monitoring."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "working_dir": { "type": "string" },
                    "label": { "type": "string" }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_exec_background(&call.arguments),
        )
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "code_execute"
    }
}

#[async_trait]
impl Tool for ExecStatusTool {
    fn name(&self) -> &'static str {
        "exec_status"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Check one background process or list all background processes."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "tail": { "type": "integer" }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_exec_status(&call.arguments),
        )
    }

    fn action_category(&self) -> &'static str {
        "code_execute"
    }
}

#[async_trait]
impl Tool for ExecKillTool {
    fn name(&self) -> &'static str {
        "exec_kill"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Kill a background process by session ID.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_exec_kill(&call.arguments).await,
        )
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "code_execute"
    }
}

#[derive(Deserialize)]
struct ExecBackgroundArgs {
    command: String,
    working_dir: Option<String>,
    label: Option<String>,
}

#[derive(Deserialize)]
struct ExecStatusArgs {
    session_id: Option<String>,
    tail: Option<usize>,
}

#[derive(Deserialize)]
struct ExecKillArgs {
    session_id: String,
}

impl ToolContext {
    pub(crate) fn handle_exec_background(
        &self,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        let parsed: ExecBackgroundArgs = parse_args(args)?;
        let working_dir = self.resolve_command_dir(parsed.working_dir.as_deref())?;
        self.guard_push_command(&parsed.command)?;
        let result = self
            .process_registry
            .spawn(parsed.command, working_dir, parsed.label)?;
        serialize_output(exec_spawn_value(result))
    }

    pub(crate) fn handle_exec_status(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ExecStatusArgs = parse_args(args)?;
        let tail = parsed.tail.unwrap_or(20);
        if let Some(session_id) = parsed.session_id.as_deref() {
            let status = self
                .process_registry
                .status(session_id, tail)
                .ok_or_else(|| format!("unknown session_id: {session_id}"))?;
            return serialize_output(exec_status_value(status));
        }
        serialize_output(exec_list_value(self.process_registry.list()))
    }

    pub(crate) async fn handle_exec_kill(
        &self,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        let parsed: ExecKillArgs = parse_args(args)?;
        self.process_registry.kill(&parsed.session_id).await?;
        serialize_output(serde_json::json!({
            "session_id": parsed.session_id,
            "status": "killed",
        }))
    }
}

fn serialize_output(value: serde_json::Value) -> Result<String, String> {
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

fn exec_spawn_value(result: SpawnResult) -> serde_json::Value {
    serde_json::json!({
        "session_id": result.session_id,
        "pid": result.pid,
        "label": result.label,
        "status": result.status,
    })
}

fn exec_status_value(status: StatusResult) -> serde_json::Value {
    serde_json::json!({
        "session_id": status.session_id,
        "label": status.label,
        "working_dir": status.working_dir,
        "status": status.status.name(),
        "exit_code": status.status.exit_code(),
        "runtime_seconds": status.runtime_seconds,
        "output_lines": status.output_lines,
        "tail": status.tail,
    })
}

fn exec_list_value(processes: Vec<ListEntry>) -> serde_json::Value {
    let items = processes
        .into_iter()
        .map(exec_list_entry_value)
        .collect::<Vec<_>>();
    serde_json::json!({ "processes": items })
}

fn exec_list_entry_value(entry: ListEntry) -> serde_json::Value {
    serde_json::json!({
        "session_id": entry.session_id,
        "label": entry.label,
        "working_dir": entry.working_dir,
        "status": entry.status.name(),
        "exit_code": entry.status.exit_code(),
        "runtime_seconds": entry.runtime_seconds,
        "output_lines": entry.output_lines,
    })
}
