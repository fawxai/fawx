use super::{
    build_budget_summary, parse_args, schedule_sighup_restart, to_tool_result, ConfigSetRequest,
    ToolRegistry, MAX_RESTART_DELAY_SECS,
};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_config::{manager::ConfigManager, FawxConfig};
use fx_core::kernel_manifest::{build_kernel_manifest, ManifestSources};
use fx_core::runtime_info::RuntimeInfo;
use fx_kernel::act::{ToolCacheability, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use serde::Deserialize;
use std::sync::Arc;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(ConfigGetTool::new(context));
    registry.register(ConfigSetTool::new(context));
    registry.register(FawxStatusTool::new(context));
    registry.register(KernelManifestTool::new(context));
    registry.register(FawxRestartTool::new(context));
}

struct ConfigGetTool {
    context: Arc<ToolContext>,
}

struct ConfigSetTool {
    context: Arc<ToolContext>,
}

struct FawxStatusTool {
    context: Arc<ToolContext>,
}

struct KernelManifestTool {
    context: Arc<ToolContext>,
}

struct FawxRestartTool {
    context: Arc<ToolContext>,
}

impl ConfigGetTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl ConfigSetTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl FawxStatusTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl KernelManifestTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl FawxRestartTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[async_trait]
impl Tool for ConfigGetTool {
    fn name(&self) -> &'static str {
        "config_get"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Read current Fawx configuration".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "section": {
                        "type": "string",
                        "description": "Config section (model, general, tools, memory, http, telegram, etc.) or 'all'"
                    }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_config_get(&call.arguments),
        )
    }

    fn is_available(&self) -> bool {
        self.context.config_manager.is_some()
    }
}

#[async_trait]
impl Tool for ConfigSetTool {
    fn name(&self) -> &'static str {
        "config_set"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Update a configuration value. Validates before applying.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Dot-separated path (e.g. 'model.default_model')"
                    },
                    "value": {
                        "type": "string",
                        "description": "New value"
                    }
                },
                "required": ["key", "value"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_config_set(&call.arguments),
        )
    }

    fn is_available(&self) -> bool {
        self.context.config_manager.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}

#[async_trait]
impl Tool for FawxStatusTool {
    fn name(&self) -> &'static str {
        "fawx_status"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Get server status: uptime, model, memory entries".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(&call.id, self.name(), self.context.handle_fawx_status())
    }

    fn is_available(&self) -> bool {
        self.context.config_manager.is_some()
    }
}

#[async_trait]
impl Tool for KernelManifestTool {
    fn name(&self) -> &'static str {
        "kernel_manifest"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Get a structured description of the kernel's current configuration, permissions, budget limits, sandbox rules, and available tools. Use this at the start of complex tasks to understand your capabilities and constraints before planning."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(&call.id, self.name(), self.context.handle_kernel_manifest())
    }

    fn is_available(&self) -> bool {
        self.context.config_manager.is_some()
    }
}

#[async_trait]
impl Tool for FawxRestartTool {
    fn name(&self) -> &'static str {
        "fawx_restart"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Gracefully restart the Fawx server. Use after config changes."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "Why restarting"
                    },
                    "delay_seconds": {
                        "type": "integer",
                        "description": "Delay before restart (default: 2)"
                    }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_fawx_restart(&call.arguments),
        )
    }

    fn is_available(&self) -> bool {
        self.context.config_manager.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}

#[derive(Deserialize)]
struct ConfigGetArgs {
    section: Option<String>,
}

#[derive(Deserialize)]
struct FawxRestartArgs {
    reason: Option<String>,
    delay_seconds: Option<u64>,
}

impl ToolContext {
    pub(crate) fn handle_config_get(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ConfigGetArgs = parse_args(args)?;
        let mgr = self.locked_config_manager()?;
        let section = parsed.section.as_deref().unwrap_or("all");
        let value = mgr.get(section)?;
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to format config: {error}"))
    }

    pub(crate) fn handle_config_set(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ConfigSetRequest = parse_args(args)?;
        let mut mgr = self
            .config_manager
            .as_ref()
            .ok_or_else(|| "config manager not configured".to_string())?
            .lock()
            .map_err(|error| format!("failed to lock config manager: {error}"))?;
        mgr.set(&parsed.key, &parsed.value)?;
        Ok(format!("updated {} = {}", parsed.key, parsed.value))
    }

    pub(crate) fn handle_fawx_status(&self) -> Result<String, String> {
        let authority = self
            .runtime_info
            .as_ref()
            .and_then(|info| info.read().ok().and_then(|guard| guard.authority.clone()));
        let status = serde_json::json!({
            "status": "running",
            "uptime_seconds": self.start_time.elapsed().as_secs(),
            "model": self.active_model_name(),
            "memory_entries": self.memory_entry_count(),
            "skills_loaded": self.skills_loaded_count(),
            "sessions": self.active_session_count(),
            "authority": authority,
        });
        serde_json::to_string_pretty(&status)
            .map_err(|error| format!("failed to format status: {error}"))
    }

    pub(crate) fn handle_kernel_manifest(&self) -> Result<String, String> {
        let runtime = self.locked_runtime_info()?;
        let config = self.locked_config()?;
        let (sm_enabled, sm_allow, sm_deny) = match &self.self_modify {
            Some(sm) => (sm.enabled, sm.allow_paths.clone(), sm.deny_paths.clone()),
            None => (false, Vec::new(), Vec::new()),
        };
        let working_dir = self.working_dir.to_string_lossy().into_owned();
        let budget = build_budget_summary(&self.kernel_budget);
        let can_request_capabilities = runtime.skills.iter().any(|skill| {
            skill
                .tool_names
                .iter()
                .any(|tool| tool == "request_capability")
        });
        let sources = ManifestSources {
            version: &runtime.version,
            active_model: &runtime.active_model,
            provider: &runtime.provider,
            preset: Some(config.permissions.preset.as_str()),
            permissions: &config.permissions,
            authority: runtime.authority.as_ref(),
            budget: &budget,
            sandbox: &config.sandbox,
            self_modify_enabled: sm_enabled,
            self_modify_allow: &sm_allow,
            self_modify_deny: &sm_deny,
            skills: &runtime.skills,
            working_dir: &working_dir,
            can_request_capabilities,
        };
        let manifest = build_kernel_manifest(&sources);
        serde_json::to_string_pretty(&manifest)
            .map_err(|error| format!("failed to serialize manifest: {error}"))
    }

    pub(crate) fn handle_fawx_restart(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: FawxRestartArgs = parse_args(args)?;
        let delay = parsed.delay_seconds.unwrap_or(2);
        let reason = parsed.reason.as_deref().unwrap_or("requested by agent");
        tracing::info!(reason, delay, "scheduling SIGHUP restart");
        schedule_sighup_restart(delay, reason.to_string())?;
        let clamped = delay.min(MAX_RESTART_DELAY_SECS);
        Ok(format!(
            "restart scheduled in {clamped}s (reason: {reason})"
        ))
    }

    fn locked_runtime_info(&self) -> Result<RuntimeInfo, String> {
        let info = self
            .runtime_info
            .as_ref()
            .ok_or_else(|| "runtime info not configured".to_string())?;
        info.read()
            .map_err(|error| format!("failed to read runtime info: {error}"))
            .map(|guard| guard.clone())
    }

    fn locked_config(&self) -> Result<FawxConfig, String> {
        let manager = self
            .config_manager
            .as_ref()
            .ok_or_else(|| "config manager not available".to_string())?;
        let guard = manager
            .lock()
            .map_err(|error| format!("config lock failed: {error}"))?;
        Ok(guard.config().clone())
    }

    fn locked_config_manager(&self) -> Result<std::sync::MutexGuard<'_, ConfigManager>, String> {
        self.config_manager
            .as_ref()
            .ok_or_else(|| "config manager not configured".to_string())?
            .lock()
            .map_err(|error| format!("failed to lock config manager: {error}"))
    }

    fn active_model_name(&self) -> String {
        self.runtime_info
            .as_ref()
            .and_then(|info| info.read().ok())
            .map(|info| info.active_model.clone())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn memory_entry_count(&self) -> usize {
        self.memory
            .as_ref()
            .and_then(|memory| memory.lock().ok())
            .map(|store| store.list().len())
            .unwrap_or(0)
    }

    fn skills_loaded_count(&self) -> usize {
        self.runtime_info
            .as_ref()
            .and_then(|info| info.read().ok())
            .map(|info| info.skills.len())
            .unwrap_or(0)
    }

    fn active_session_count(&self) -> usize {
        0
    }
}
