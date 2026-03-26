use crate::experiment_tool::{
    handle_run_experiment, run_experiment_tool_definition, spawn_background_experiment,
    ExperimentRegistrar, ExperimentToolState,
};
use async_trait::async_trait;
use fx_config::{manager::ConfigManager, FawxConfig};
use fx_consensus::ProgressCallback;
use fx_core::kernel_manifest::{build_kernel_manifest, BudgetSummary, ManifestSources};
use fx_core::memory::MemoryStore;
use fx_core::runtime_info::RuntimeInfo;
use fx_core::self_modify::{classify_path, format_tier_violation, PathTier, SelfModifyConfig};
use fx_kernel::act::{
    cancelled_result, is_cancelled, timed_out_result, ConcurrencyPolicy, ToolCacheability,
    ToolExecutor, ToolExecutorError, ToolResult,
};
use fx_kernel::budget::BudgetConfig as KernelBudgetConfig;
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::{ListEntry, ProcessConfig, ProcessRegistry, SpawnResult, StatusResult};
use fx_llm::{ToolCall, ToolDefinition};
use fx_memory::embedding_index::EmbeddingIndex;
use fx_propose::{build_proposal_content, current_file_hash, Proposal, ProposalWriter};
use fx_ripcord::git_guard::{check_push_allowed, extract_push_targets};
use fx_subagent::{
    SpawnConfig, SpawnMode, SubagentControl, SubagentHandle, SubagentId, SubagentStatus,
};
use serde::Deserialize;
use std::fs;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;

/// Expand a leading `~` or `~/` prefix to the user's home directory.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

const MAX_RECURSION_DEPTH: usize = 5;
const MAX_SEARCH_MATCHES: usize = 100;
const DEFAULT_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const DEFAULT_MAX_READ_SIZE: u64 = 1024 * 1024;
const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MEMORY_SEARCH_RESULTS: usize = 5;

fn default_process_registry(working_dir: &Path) -> Arc<ProcessRegistry> {
    Arc::new(ProcessRegistry::new(ProcessConfig {
        allowed_dirs: vec![working_dir.to_path_buf()],
        ..ProcessConfig::default()
    }))
}

fn build_budget_summary(config: &KernelBudgetConfig) -> BudgetSummary {
    BudgetSummary {
        max_llm_calls: config.max_llm_calls,
        max_tool_invocations: config.max_tool_invocations,
        max_tokens: config.max_tokens,
        max_wall_time_seconds: config.max_wall_time_ms / 1_000,
        max_retries_per_tool: u32::from(config.max_tool_retries),
        max_fan_out: config.max_fan_out,
    }
}

#[derive(Clone)]
pub struct FawxToolExecutor {
    working_dir: PathBuf,
    config: ToolConfig,
    process_registry: Arc<ProcessRegistry>,
    memory: Option<Arc<Mutex<dyn MemoryStore>>>,
    embedding_index: Option<Arc<Mutex<EmbeddingIndex>>>,
    runtime_info: Option<Arc<RwLock<RuntimeInfo>>>,
    self_modify: Option<SelfModifyConfig>,
    concurrency_policy: ConcurrencyPolicy,
    config_manager: Option<Arc<Mutex<ConfigManager>>>,
    protected_branches: Vec<String>,
    kernel_budget: KernelBudgetConfig,
    start_time: std::time::Instant,
    subagent_control: Option<Arc<dyn SubagentControl>>,
    experiment: Option<ExperimentToolState>,
    experiment_progress: Option<ProgressCallback>,
    experiment_registrar: Option<Arc<dyn ExperimentRegistrar>>,
    background_experiments: bool,
    node_run: Option<crate::node_run::NodeRunState>,
    #[cfg(feature = "improvement")]
    improvement: Option<crate::improvement_tools::ImprovementToolsState>,
}

#[derive(Debug, Clone)]
pub struct ToolConfig {
    /// Maximum file size for write operations (bytes)
    pub max_file_size: u64,
    /// Maximum file size for read_file operations (bytes)
    pub max_read_size: u64,
    /// Additional directories to exclude from search
    pub search_exclude: Vec<String>,
    /// Command execution timeout
    pub command_timeout: Duration,
    /// Whether to allow commands outside working_dir
    pub jail_to_working_dir: bool,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            max_read_size: DEFAULT_MAX_READ_SIZE,
            search_exclude: Vec::new(),
            command_timeout: Duration::from_secs(DEFAULT_COMMAND_TIMEOUT_SECS),
            jail_to_working_dir: true,
        }
    }
}

impl FawxToolExecutor {
    pub fn new(working_dir: PathBuf, config: ToolConfig) -> Self {
        let process_registry = default_process_registry(&working_dir);
        Self {
            working_dir,
            config,
            process_registry,
            memory: None,
            embedding_index: None,
            runtime_info: None,
            self_modify: None,
            concurrency_policy: ConcurrencyPolicy::default(),
            config_manager: None,
            protected_branches: Vec::new(),
            kernel_budget: KernelBudgetConfig::default(),
            start_time: std::time::Instant::now(),
            subagent_control: None,
            experiment: None,
            experiment_progress: None,
            experiment_registrar: None,
            background_experiments: false,
            node_run: None,
            #[cfg(feature = "improvement")]
            improvement: None,
        }
    }

    /// Set the concurrency policy for parallel tool execution.
    #[must_use]
    pub fn with_concurrency_policy(mut self, policy: ConcurrencyPolicy) -> Self {
        self.concurrency_policy = policy;
        self
    }

    /// Attach a persistent memory provider.
    pub fn with_memory(mut self, memory: Arc<Mutex<dyn MemoryStore>>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Attach a semantic embedding index for memory search.
    pub fn with_embedding_index(mut self, index: Arc<Mutex<EmbeddingIndex>>) -> Self {
        self.embedding_index = Some(index);
        self
    }

    /// Attach runtime self-introspection state.
    pub fn with_runtime_info(mut self, info: Arc<RwLock<RuntimeInfo>>) -> Self {
        self.runtime_info = Some(info);
        self
    }

    /// Attach a self-modification path enforcement config.
    pub fn with_self_modify(mut self, config: SelfModifyConfig) -> Self {
        self.self_modify = Some(config);
        self
    }

    /// Attach a config manager for runtime config read/write tools.
    pub fn with_config_manager(mut self, mgr: Arc<Mutex<ConfigManager>>) -> Self {
        self.config_manager = Some(mgr);
        self
    }

    #[must_use]
    pub fn with_protected_branches(mut self, protected_branches: Vec<String>) -> Self {
        self.protected_branches = protected_branches;
        self
    }

    /// Attach the active kernel budget configuration.
    pub fn with_kernel_budget(mut self, budget: KernelBudgetConfig) -> Self {
        self.kernel_budget = budget;
        self
    }

    /// Attach subagent lifecycle tools (spawn_agent, subagent_status).
    pub fn with_subagent_control(mut self, control: Arc<dyn SubagentControl>) -> Self {
        self.subagent_control = Some(control);
        self
    }

    /// Attach experiment execution state for run_experiment.
    pub fn with_experiment(mut self, state: ExperimentToolState) -> Self {
        self.experiment = Some(state);
        self
    }

    /// Attach an experiment progress callback for run_experiment.
    pub fn with_experiment_progress(mut self, progress: ProgressCallback) -> Self {
        self.experiment_progress = Some(progress);
        self
    }

    /// Attach an experiment registry bridge for background run_experiment calls.
    pub fn with_experiment_registrar(mut self, registrar: Arc<dyn ExperimentRegistrar>) -> Self {
        self.experiment_registrar = Some(registrar);
        self
    }

    /// Toggle spawn-and-return behavior for run_experiment.
    #[must_use]
    pub fn with_background_experiments(mut self, background: bool) -> Self {
        self.background_experiments = background;
        self
    }

    pub fn set_experiment(&mut self, state: ExperimentToolState) {
        self.experiment = Some(state);
    }

    /// Attach node_run tool state for remote command execution.
    pub fn with_node_run(mut self, state: crate::node_run::NodeRunState) -> Self {
        self.node_run = Some(state);
        self
    }

    /// Attach a background process registry shared with the engine lifecycle.
    pub fn with_process_registry(mut self, registry: Arc<ProcessRegistry>) -> Self {
        self.process_registry = registry;
        self
    }

    /// Attach self-improvement tools (analyze_signals, propose_improvement).
    #[cfg(feature = "improvement")]
    pub fn with_improvement(
        mut self,
        state: crate::improvement_tools::ImprovementToolsState,
    ) -> Self {
        self.improvement = Some(state);
        self
    }

    /// Whether improvement tools are configured and enabled.
    #[cfg(feature = "improvement")]
    fn improvement_tools_enabled(&self) -> bool {
        self.improvement.as_ref().is_some_and(|s| s.config.enabled)
    }

    fn cacheability_for(tool_name: &str) -> ToolCacheability {
        match tool_name {
            "read_file" | "list_directory" | "search_text" | "memory_read" | "memory_list"
            | "memory_search" => ToolCacheability::Cacheable,
            "write_file" | "edit_file" | "memory_write" | "memory_delete" | "run_command"
            | "exec_background" | "exec_kill" | "config_set" | "fawx_restart" | "spawn_agent"
            | "node_run" | "run_experiment" => ToolCacheability::SideEffect,
            "current_time"
            | "self_info"
            | "config_get"
            | "fawx_status"
            | "kernel_manifest"
            | "exec_status"
            | "subagent_status"
            | "analyze_signals"
            | "propose_improvement" => ToolCacheability::NeverCache,
            _ => ToolCacheability::NeverCache,
        }
    }

    pub(crate) async fn execute_call(
        &self,
        call: &ToolCall,
        cancel: Option<&CancellationToken>,
    ) -> ToolResult {
        if is_cancelled(cancel) {
            return cancelled_result(&call.id, &call.name);
        }
        let output = match call.name.as_str() {
            "read_file" => self.handle_read_file(&call.arguments),
            "write_file" => self.handle_write_file(&call.arguments),
            "edit_file" => self.handle_edit_file(&call.arguments),
            "list_directory" => self.handle_list_directory(&call.arguments),
            "run_command" => self.handle_run_command(&call.arguments).await,
            "exec_background" => self.handle_exec_background(&call.arguments),
            "exec_status" => self.handle_exec_status(&call.arguments),
            "exec_kill" => self.handle_exec_kill(&call.arguments).await,
            "search_text" => self.handle_search_text(&call.arguments),
            "current_time" => self.handle_current_time(),
            "self_info" => self.handle_self_info(&call.arguments),
            "config_get" => self.handle_config_get(&call.arguments),
            "config_set" => self.handle_config_set(&call.arguments),
            "fawx_status" => self.handle_fawx_status(),
            "kernel_manifest" => self.handle_kernel_manifest(),
            "fawx_restart" => self.handle_fawx_restart(&call.arguments),
            "memory_write" => self.handle_memory_write(&call.arguments),
            "memory_read" => self.handle_memory_read(&call.arguments),
            "memory_list" => self.handle_memory_list(),
            "memory_search" => self.handle_memory_search(&call.arguments),
            "memory_delete" => self.handle_memory_delete(&call.arguments),
            "spawn_agent" => self.handle_spawn_agent(&call.arguments).await,
            "subagent_status" => self.handle_subagent_status(&call.arguments).await,
            "run_experiment" => self.handle_run_experiment(&call.arguments).await,
            "node_run" => {
                return self.dispatch_node_run(call).await;
            }
            #[cfg(feature = "improvement")]
            "analyze_signals" => {
                return self.dispatch_analyze_signals(call).await;
            }
            #[cfg(feature = "improvement")]
            "propose_improvement" => {
                return self.dispatch_propose_improvement(call).await;
            }
            _ => Err(format!("unknown tool: {}", call.name)),
        };
        to_tool_result(&call.id, &call.name, output)
    }

    fn subagent_control(&self) -> Result<&Arc<dyn SubagentControl>, String> {
        self.subagent_control
            .as_ref()
            .ok_or_else(|| "subagent control not configured".to_string())
    }

    fn jailed_path(&self, requested: &str) -> Result<PathBuf, String> {
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(requested));
        }
        validate_path(&self.working_dir, requested)
    }

    fn validated_existing_entry(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        if !self.config.jail_to_working_dir {
            return Ok(Some(path.to_path_buf()));
        }
        let requested = path.to_string_lossy().to_string();
        match validate_path(&self.working_dir, &requested) {
            Ok(validated) => Ok(Some(validated)),
            Err(_) => Ok(None),
        }
    }

    fn resolve_tool_path(&self, requested: &str) -> Result<PathBuf, String> {
        let expanded = expand_tilde(requested);
        let expanded_str = expanded
            .to_str()
            .ok_or_else(|| "home directory path is not valid UTF-8".to_string())?;
        self.jailed_path(expanded_str)
    }

    fn read_utf8_file(&self, path: &Path, size_limit: Option<u64>) -> Result<String, String> {
        let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
        if size_limit.is_some_and(|limit| metadata.len() > limit) {
            return Err("file exceeds maximum allowed size".to_string());
        }
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        String::from_utf8(bytes).map_err(|_| "file appears to be binary".to_string())
    }

    fn handle_read_file(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ReadFileArgs = parse_args(args)?;
        let path = self.resolve_tool_path(&parsed.path)?;
        let content = self.read_utf8_file(&path, Some(self.config.max_read_size))?;
        render_read_output(&content, parsed.offset, parsed.limit)
    }

    fn handle_write_file(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: WriteFileArgs = parse_args(args)?;
        let path = self.resolve_tool_path(&parsed.path)?;
        if let Some(message) = self.apply_write_policy(&path, &parsed.content)? {
            return Ok(message);
        }
        write_text_file(&path, &parsed.content)?;
        Ok(format!(
            "wrote {} bytes to {}",
            parsed.content.len(),
            path.display()
        ))
    }

    fn handle_edit_file(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: EditFileArgs = parse_args(args)?;
        validate_edit_args(&parsed)?;
        let path = self.resolve_tool_path(&parsed.path)?;
        let content = self.read_utf8_file(&path, Some(self.config.max_file_size))?;
        let plan = plan_exact_edit(&path, &content, &parsed.old_text, &parsed.new_text)?;
        if let Some(message) = self.apply_write_policy(&path, &plan.updated_content)? {
            return Ok(message);
        }
        write_text_file(&path, &plan.updated_content)?;
        Ok(format!(
            "Successfully edited {} (lines {}-{})",
            path.display(),
            plan.start_line,
            plan.end_line
        ))
    }

    fn apply_write_policy(&self, path: &Path, content: &str) -> Result<Option<String>, String> {
        // Defense-in-depth: ProposalGateExecutor in the kernel is the primary
        // enforcement layer for self-modify policy. This tool-level check is
        // retained as a secondary guard in case the kernel gate is bypassed or
        // misconfigured.
        self.check_max_file_size(content.len())?;
        let Some(ref config) = self.self_modify else {
            return Ok(None);
        };
        let tier = classify_path(path, &self.working_dir, config);
        match tier {
            PathTier::Deny => Err(deny_tier_message(path, tier)),
            PathTier::Propose => self.write_proposal(path, content, config).map(Some),
            PathTier::Allow => Ok(None),
        }
    }

    fn check_max_file_size(&self, len: usize) -> Result<(), String> {
        if (len as u64) > self.config.max_file_size {
            return Err("content exceeds maximum allowed size".to_string());
        }
        Ok(())
    }

    fn write_proposal(
        &self,
        path: &Path,
        content: &str,
        config: &SelfModifyConfig,
    ) -> Result<String, String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("system time error: {e}"))?
            .as_secs();
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        let action = if path.exists() { "replace" } else { "create" };
        let file_hash = current_file_hash(&self.working_dir, path)
            .map_err(|error| format!("failed to inspect target file: {error}"))?;
        let proposal = Proposal {
            title: format!("Modify {filename}"),
            description: format!(
                "Agent attempted to {} propose-tier path: {} ({} bytes)",
                action,
                path.display(),
                content.len()
            ),
            target_path: path.to_path_buf(),
            proposed_content: build_proposed_content(path, content),
            risk: "This path is classified as propose-tier under self-modification policy."
                .to_string(),
            timestamp,
            file_hash,
        };
        let writer = ProposalWriter::new(config.proposals_dir.clone());
        let proposal_path = writer.write(&proposal).map_err(|error| error.to_string())?;
        Ok(format!(
            "Proposal created at {}. The target file '{}' was NOT modified. \
             A human must review and approve this proposal.",
            proposal_path.display(),
            path.display()
        ))
    }

    fn handle_list_directory(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ListDirectoryArgs = parse_args(args)?;
        let expanded = expand_tilde(&parsed.path);
        let expanded_str = expanded
            .to_str()
            .ok_or_else(|| "home directory path is not valid UTF-8".to_string())?;
        let path = self.jailed_path(expanded_str)?;
        let recursive = parsed.recursive.unwrap_or(false);
        if recursive {
            return self.list_recursive(&path, 0);
        }
        self.list_flat(&path)
    }

    fn list_flat(&self, path: &Path) -> Result<String, String> {
        let mut lines = Vec::new();
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry_kind(&entry.path())?;
            lines.push(format!("[{kind}] {}", entry.file_name().to_string_lossy()));
        }
        lines.sort();
        Ok(lines.join("\n"))
    }

    fn list_recursive(&self, path: &Path, depth: usize) -> Result<String, String> {
        if depth > MAX_RECURSION_DEPTH {
            return Ok(String::new());
        }
        let mut lines = Vec::new();
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let entry_path = entry.path();

            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if self.is_ignored_directory(name) && entry_path.is_dir() {
                    continue;
                }
            }

            let Some(validated) = self.validated_existing_entry(&entry_path)? else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = entry_kind(&entry_path)?;
            lines.push(format!("{}[{}] {}", "  ".repeat(depth), kind, name));
            if kind == "dir" {
                let nested = self.list_recursive(&validated, depth + 1)?;
                if !nested.is_empty() {
                    lines.push(nested);
                }
            }
        }
        Ok(lines.join("\n"))
    }

    async fn handle_run_command(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: RunCommandArgs = parse_args(args)?;
        let command = parsed.command.trim();
        if command.is_empty() {
            return Err("command cannot be empty".to_string());
        }
        let working_dir = self.resolve_command_dir(parsed.working_dir.as_deref())?;
        self.guard_push_command(command)?;
        let child = build_command(command, parsed.shell.unwrap_or(false), &working_dir)?
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        let output = wait_with_timeout(child, self.config.command_timeout).await?;
        Ok(format_command_output(output, parsed.shell.unwrap_or(false)))
    }

    fn handle_exec_background(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ExecBackgroundArgs = parse_args(args)?;
        let working_dir = self.resolve_command_dir(parsed.working_dir.as_deref())?;
        self.guard_push_command(&parsed.command)?;
        let result = self
            .process_registry
            .spawn(parsed.command, working_dir, parsed.label)?;
        serialize_output(exec_spawn_value(result))
    }

    fn handle_exec_status(&self, args: &serde_json::Value) -> Result<String, String> {
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

    async fn handle_exec_kill(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ExecKillArgs = parse_args(args)?;
        self.process_registry.kill(&parsed.session_id).await?;
        serialize_output(serde_json::json!({
            "session_id": parsed.session_id,
            "status": "killed",
        }))
    }

    fn guard_push_command(&self, command: &str) -> Result<(), String> {
        let targets = extract_push_targets(command);
        if targets.is_empty() {
            return Ok(());
        }
        check_push_allowed(&targets, &self.protected_branches)
    }

    fn resolve_command_dir(&self, requested: Option<&str>) -> Result<PathBuf, String> {
        let desired = requested.unwrap_or_else(|| self.working_dir.to_str().unwrap_or("."));
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(desired));
        }
        validate_path(&self.working_dir, desired)
    }

    fn handle_search_text(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: SearchTextArgs = parse_args(args)?;
        let root = self.resolve_search_root(parsed.path.as_deref())?;
        let mut results = Vec::new();
        self.search_path(&root, &parsed, &mut results)?;
        Ok(results.join("\n"))
    }

    fn handle_current_time(&self) -> Result<String, String> {
        let now = SystemTime::now();
        let duration = now
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system time before Unix epoch: {error}"))?;
        let epoch = duration.as_secs();
        let iso = iso8601_utc_from_epoch(epoch);
        let day_of_week = day_of_week_from_epoch(epoch);
        Ok(format!(
            "iso8601_utc: {iso}\nepoch: {epoch}\nday_of_week: {day_of_week}"
        ))
    }

    fn handle_self_info(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: SelfInfoArgs = parse_args(args)?;
        let info_lock = self
            .runtime_info
            .as_ref()
            .ok_or_else(|| "runtime info not configured".to_string())?;
        let info = info_lock
            .read()
            .map_err(|error| format!("failed to read runtime info: {error}"))?;
        let section = parsed.section.as_deref().unwrap_or("all");
        serialize_section(&info, section)
    }

    fn handle_config_get(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ConfigGetArgs = parse_args(args)?;
        let mgr = self.locked_config_manager()?;
        let section = parsed.section.as_deref().unwrap_or("all");
        let value = mgr.get(section)?;
        serde_json::to_string_pretty(&value).map_err(|e| format!("failed to format config: {e}"))
    }

    fn handle_config_set(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ConfigSetRequest = parse_args(args)?;
        let mut mgr = self
            .config_manager
            .as_ref()
            .ok_or_else(|| "config manager not configured".to_string())?
            .lock()
            .map_err(|e| format!("failed to lock config manager: {e}"))?;
        mgr.set(&parsed.key, &parsed.value)?;
        Ok(format!("updated {} = {}", parsed.key, parsed.value))
    }

    fn handle_fawx_status(&self) -> Result<String, String> {
        let uptime = self.start_time.elapsed();
        let model = self.active_model_name();
        let memory_entries = self.memory_entry_count();
        let skills_loaded = self.skills_loaded_count();
        let sessions = self.active_session_count();
        let status = serde_json::json!({
            "status": "running",
            "uptime_seconds": uptime.as_secs(),
            "model": model,
            "memory_entries": memory_entries,
            "skills_loaded": skills_loaded,
            "sessions": sessions,
        });
        serde_json::to_string_pretty(&status).map_err(|e| format!("failed to format status: {e}"))
    }

    fn handle_kernel_manifest(&self) -> Result<String, String> {
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
            .map_err(|e| format!("failed to serialize manifest: {e}"))
    }

    fn handle_fawx_restart(&self, args: &serde_json::Value) -> Result<String, String> {
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
            .map_err(|e| format!("failed to lock config manager: {e}"))
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
            .and_then(|m| m.lock().ok())
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

    /// Stub: session count is not yet tracked in the tool executor.
    /// Returns 0 until fx-session wiring is complete.
    fn active_session_count(&self) -> usize {
        0
    }

    fn is_ignored_directory(&self, name: &str) -> bool {
        if is_builtin_ignored_directory(name) {
            return true;
        }
        self.config.search_exclude.iter().any(|item| item == name)
    }

    fn resolve_search_root(&self, requested: Option<&str>) -> Result<PathBuf, String> {
        let default_root = self.working_dir.to_string_lossy().to_string();
        let requested = requested.unwrap_or(&default_root);
        let expanded = expand_tilde(requested);
        let expanded_str = expanded
            .to_str()
            .ok_or_else(|| "home directory path is not valid UTF-8".to_string())?;
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(expanded_str));
        }
        validate_path(&self.working_dir, expanded_str)
    }

    fn search_path(
        &self,
        root: &Path,
        args: &SearchTextArgs,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if out.len() >= MAX_SEARCH_MATCHES {
            return Ok(());
        }
        if root.is_dir() {
            self.search_directory(root, args, out)?;
        } else {
            self.search_file(root, args, out)?;
        }
        Ok(())
    }

    fn search_directory(
        &self,
        dir: &Path,
        args: &SearchTextArgs,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
            if out.len() >= MAX_SEARCH_MATCHES {
                break;
            }
            let entry_path = entry.map_err(|error| error.to_string())?.path();

            // Skip build artifacts, VCS, and dependency directories
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if self.is_ignored_directory(name) && entry_path.is_dir() {
                    continue;
                }
            }

            let Some(validated) = self.validated_existing_entry(&entry_path)? else {
                continue;
            };
            if validated.is_dir() {
                self.search_directory(&validated, args, out)?;
                continue;
            }
            self.search_file(&validated, args, out)?;
        }
        Ok(())
    }

    fn search_file(
        &self,
        file: &Path,
        args: &SearchTextArgs,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if !matches_glob(file, args.file_glob.as_deref()) {
            return Ok(());
        }
        let metadata = fs::metadata(file).map_err(|error| error.to_string())?;
        if metadata.len() > self.config.max_read_size {
            return Ok(());
        }
        let mut bytes = Vec::new();
        let mut reader = fs::File::open(file).map_err(|error| error.to_string())?;
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return Ok(()),
        };
        for (index, line) in text.lines().enumerate() {
            if out.len() >= MAX_SEARCH_MATCHES {
                break;
            }
            if line.contains(&args.pattern) {
                out.push(format!("{}:{}:{}", file.display(), index + 1, line));
            }
        }
        Ok(())
    }
    fn handle_memory_write(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: MemoryWriteArgs = parse_args(args)?;
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let mut guard = memory.lock().map_err(|e| format!("{e}"))?;
        guard.write(&parsed.key, &parsed.value)?;
        drop(guard);
        self.upsert_embedding_memory(&parsed.key, &parsed.value)?;
        Ok(format!("stored key '{}'", parsed.key))
    }

    fn handle_memory_read(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: MemoryReadArgs = parse_args(args)?;
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let mut guard = memory.lock().map_err(|e| format!("{e}"))?;
        let value = guard.read(&parsed.key);
        if value.is_some() {
            guard.touch(&parsed.key)?;
        }
        match value {
            Some(value) => Ok(value),
            None => Ok(format!("key '{}' not found", parsed.key)),
        }
    }

    fn handle_memory_list(&self) -> Result<String, String> {
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let guard = memory.lock().map_err(|e| format!("{e}"))?;
        let entries = guard.list();
        if entries.is_empty() {
            return Ok("no memories stored".to_string());
        }
        let lines = format_memory_list(&entries);
        Ok(lines)
    }

    fn handle_memory_search(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: MemorySearchArgs = parse_args(args)?;
        let max_results = parsed.max_results.unwrap_or(DEFAULT_MEMORY_SEARCH_RESULTS);
        let results = self.memory_search_results(&parsed.query, max_results)?;
        self.touch_memory_search_results(&results)?;
        Ok(format_memory_search_results(&parsed.query, &results))
    }

    fn handle_memory_delete(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: MemoryDeleteArgs = parse_args(args)?;
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let mut guard = memory.lock().map_err(|e| format!("{e}"))?;
        let deleted = guard.delete(&parsed.key);
        drop(guard);
        if deleted {
            self.remove_embedding_memory(&parsed.key)?;
            Ok(format!("deleted key '{}'", parsed.key))
        } else {
            Ok(format!("key '{}' not found", parsed.key))
        }
    }

    fn memory_search_results(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemorySearchResult>, String> {
        if let Some(index) = &self.embedding_index {
            match self.semantic_memory_search(index, query, max_results) {
                Ok(results) => return Ok(results),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "semantic search failed; falling back to keyword search"
                    );
                }
            }
        }
        self.keyword_memory_search(query, max_results)
    }

    fn touch_memory_search_results(&self, results: &[MemorySearchResult]) -> Result<(), String> {
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let mut guard = memory.lock().map_err(|error| format!("{error}"))?;
        results
            .iter()
            .try_for_each(|result| guard.touch(&result.key))
    }

    fn semantic_memory_search(
        &self,
        index: &Arc<Mutex<EmbeddingIndex>>,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemorySearchResult>, String> {
        let hits = index
            .lock()
            .map_err(|e| format!("{e}"))?
            .search(query, max_results)
            .map_err(|error| error.to_string())?;
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let guard = memory.lock().map_err(|e| format!("{e}"))?;
        Ok(hits
            .into_iter()
            .filter_map(|(key, score)| {
                guard.read(&key).map(|value| MemorySearchResult {
                    key,
                    value,
                    score: Some(score),
                })
            })
            .collect())
    }

    fn keyword_memory_search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemorySearchResult>, String> {
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let guard = memory.lock().map_err(|e| format!("{e}"))?;
        Ok(guard
            .search_relevant(query, max_results)
            .into_iter()
            .map(|(key, value)| MemorySearchResult {
                key,
                value,
                score: None,
            })
            .collect())
    }

    fn upsert_embedding_memory(&self, key: &str, value: &str) -> Result<(), String> {
        let Some(index) = &self.embedding_index else {
            return Ok(());
        };
        index
            .lock()
            .map_err(|e| format!("{e}"))?
            .upsert(key, value)
            .map_err(|error| error.to_string())
    }

    fn remove_embedding_memory(&self, key: &str) -> Result<(), String> {
        let Some(index) = &self.embedding_index else {
            return Ok(());
        };
        index.lock().map_err(|e| format!("{e}"))?.remove(key);
        Ok(())
    }

    async fn handle_spawn_agent(&self, args: &serde_json::Value) -> Result<String, String> {
        let control = self.subagent_control()?;
        let parsed: SpawnAgentArgs = parse_args(args)?;
        let config = parsed.into_spawn_config()?;
        let handle = control
            .spawn(config)
            .await
            .map_err(|error| error.to_string())?;
        serialize_output(spawned_handle_value(&handle))
    }

    async fn handle_run_experiment(&self, args: &serde_json::Value) -> Result<String, String> {
        let state = self
            .experiment
            .as_ref()
            .ok_or_else(|| "experiment tool not configured".to_string())?;
        if self.background_experiments {
            spawn_background_experiment(
                state,
                self.subagent_control.clone(),
                &self.working_dir,
                args,
                self.experiment_progress.clone(),
                None,
                self.experiment_registrar.clone(),
            )
        } else {
            handle_run_experiment(
                state,
                self.subagent_control.as_ref(),
                &self.working_dir,
                args,
                self.experiment_progress.clone(),
            )
            .await
        }
    }

    async fn handle_subagent_status(&self, args: &serde_json::Value) -> Result<String, String> {
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

    #[cfg(feature = "improvement")]
    async fn dispatch_analyze_signals(&self, call: &ToolCall) -> ToolResult {
        let state = match &self.improvement {
            Some(s) if s.config.enabled => s,
            _ => {
                return to_tool_result(
                    &call.id,
                    &call.name,
                    Err("improvement tools not enabled".to_string()),
                );
            }
        };
        let output = crate::improvement_tools::handle_analyze_signals(state, &call.arguments).await;
        to_tool_result(&call.id, &call.name, output)
    }

    #[cfg(feature = "improvement")]
    async fn dispatch_propose_improvement(&self, call: &ToolCall) -> ToolResult {
        let state = match &self.improvement {
            Some(s) if s.config.enabled => s,
            _ => {
                return to_tool_result(
                    &call.id,
                    &call.name,
                    Err("improvement tools not enabled".to_string()),
                );
            }
        };
        let output = crate::improvement_tools::handle_propose_improvement(
            state,
            &call.arguments,
            &self.working_dir,
        )
        .await;
        to_tool_result(&call.id, &call.name, output)
    }

    async fn dispatch_node_run(&self, call: &ToolCall) -> ToolResult {
        let state = match &self.node_run {
            Some(s) => s,
            None => {
                return to_tool_result(
                    &call.id,
                    &call.name,
                    Err("node_run not configured".to_string()),
                );
            }
        };
        let output = crate::node_run::handle_node_run(state, &call.arguments).await;
        to_tool_result(&call.id, &call.name, output)
    }

    async fn execute_single_tool(
        &self,
        call: &ToolCall,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        if is_cancelled(cancel) {
            return Ok(vec![cancelled_result(&call.id, &call.name)]);
        }
        let timeout = self.concurrency_policy().timeout_per_call;
        Ok(vec![
            execute_with_timeout(self, call, cancel, timeout).await,
        ])
    }

    async fn execute_tools_parallel(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        // This executor is Clone, so JoinSet lets each task own an executor
        // instance while still enforcing max parallelism via a semaphore.
        let policy = self.concurrency_policy();
        let cancel_token = cancel.cloned();
        let semaphore = create_semaphore(policy.max_parallel);
        let mut join_set = tokio::task::JoinSet::new();

        for (index, call) in calls.iter().enumerate() {
            let executor = self.clone();
            let call = call.clone();
            let token = cancel_token.clone();
            let sem = semaphore.clone();
            let timeout = policy.timeout_per_call;
            join_set.spawn(async move {
                let task = ConcurrentToolTask {
                    executor,
                    index,
                    call,
                    cancel: token,
                    semaphore: sem,
                    timeout,
                };
                execute_one_tool(task).await
            });
        }

        collect_ordered_results(&mut join_set, calls.len()).await
    }
}

fn build_proposed_content(path: &Path, content: &str) -> String {
    let original = if path.exists() {
        Some(fs::read_to_string(path).unwrap_or_else(|_| "(binary or unreadable)".to_string()))
    } else {
        None
    };
    build_proposal_content(original.as_deref(), content)
}

struct EditPlan {
    updated_content: String,
    start_line: usize,
    end_line: usize,
}

fn write_text_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, content.as_bytes()).map_err(|error| error.to_string())
}

fn deny_tier_message(path: &Path, tier: PathTier) -> String {
    format_tier_violation(path, tier).unwrap_or_else(|| {
        format!(
            "Self-modify policy violation [deny]: {}. This path cannot be modified.",
            path.display()
        )
    })
}

fn validate_edit_args(args: &EditFileArgs) -> Result<(), String> {
    if args.old_text.is_empty() {
        return Err("old_text must not be empty".to_string());
    }
    if args.old_text == args.new_text {
        return Err("old_text and new_text must differ".to_string());
    }
    Ok(())
}

fn render_read_output(
    content: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, String> {
    validate_line_window(offset, limit)?;
    if offset.is_none() && limit.is_none() {
        return Ok(content.to_string());
    }
    let lines = collect_lines(content);
    let start_line = offset.unwrap_or(1);
    if start_line > lines.len() {
        return Ok(offset_past_end_message(start_line, lines.len()));
    }
    let start_index = start_line - 1;
    let end_index = slice_end_index(start_index, limit, lines.len());
    let body = lines[start_index..end_index].concat();
    Ok(partial_read_response(
        start_line,
        end_index,
        lines.len(),
        body,
    ))
}

fn validate_line_window(offset: Option<usize>, limit: Option<usize>) -> Result<(), String> {
    if offset == Some(0) {
        return Err("offset must be at least 1".to_string());
    }
    if limit == Some(0) {
        return Err("limit must be at least 1".to_string());
    }
    Ok(())
}

fn collect_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    content.split_inclusive('\n').collect()
}

fn offset_past_end_message(start_line: usize, total_lines: usize) -> String {
    format!("(no lines returned; offset {start_line} is past end of file with {total_lines} lines)")
}

fn slice_end_index(start_index: usize, limit: Option<usize>, total_lines: usize) -> usize {
    match limit {
        Some(limit) => (start_index + limit).min(total_lines),
        None => total_lines,
    }
}

fn partial_read_response(
    start_line: usize,
    end_index: usize,
    total_lines: usize,
    body: String,
) -> String {
    let header = format!("[Lines {start_line}-{end_index} of {total_lines}]");
    if body.is_empty() {
        header
    } else {
        format!("{header}\n{body}")
    }
}

fn plan_exact_edit(
    path: &Path,
    content: &str,
    old_text: &str,
    new_text: &str,
) -> Result<EditPlan, String> {
    let matches = count_exact_matches(content, old_text);
    if matches == 0 {
        return Err(format!(
            "Could not find the exact text in {}. The old_text must match exactly including all whitespace and newlines.",
            path.display()
        ));
    }
    if matches > 1 {
        return Err(format!(
            "Found {matches} matches for old_text in {}. Please provide more context to uniquely identify the target.",
            path.display()
        ));
    }
    let start = content.find(old_text).ok_or_else(|| {
        format!(
            "Could not find the exact text in {}. The old_text must match exactly including all whitespace and newlines.",
            path.display()
        )
    })?;
    let (start_line, end_line) = line_span(content, start, old_text);
    Ok(EditPlan {
        updated_content: replace_exact_range(content, start, old_text, new_text),
        start_line,
        end_line,
    })
}

fn count_exact_matches(content: &str, needle: &str) -> usize {
    let haystack = content.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || needle.len() > haystack.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn line_span(content: &str, start: usize, old_text: &str) -> (usize, usize) {
    let start_line = content[..start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let line_count = old_text.bytes().filter(|byte| *byte == b'\n').count() + 1;
    (start_line, start_line + line_count - 1)
}

fn replace_exact_range(content: &str, start: usize, old_text: &str, new_text: &str) -> String {
    let mut updated = String::with_capacity(content.len() - old_text.len() + new_text.len());
    updated.push_str(&content[..start]);
    updated.push_str(new_text);
    updated.push_str(&content[start + old_text.len()..]);
    updated
}

fn serialize_section(info: &RuntimeInfo, section: &str) -> Result<String, String> {
    let value = match section {
        "model" => serde_json::json!({
            "model": {
                "active": &info.active_model,
                "provider": &info.provider,
            }
        }),
        "skills" => serde_json::json!({"skills": &info.skills}),
        "config" => serde_json::json!({"config": &info.config_summary}),
        "all" => serde_json::json!({
            "model": {
                "active": &info.active_model,
                "provider": &info.provider,
            },
            "skills": &info.skills,
            "config": &info.config_summary,
            "version": &info.version,
        }),
        other => {
            return Err(format!(
                "unknown section '{other}', valid sections: model, skills, config, all"
            ));
        }
    };
    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
}

#[async_trait]
impl ToolExecutor for FawxToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        if calls.len() == 1 {
            return self.execute_single_tool(&calls[0], cancel).await;
        }
        // Cancellation returns a full-length result vector so callers can keep
        // call/result alignment even when some calls never execute.
        self.execute_tools_parallel(calls, cancel).await
    }

    fn concurrency_policy(&self) -> ConcurrencyPolicy {
        self.concurrency_policy.clone()
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs =
            fawx_tool_definitions(self.subagent_control.is_some(), self.experiment.is_some());
        if self.memory.is_some() {
            defs.extend(memory_tool_definitions());
        }
        if self.config_manager.is_some() {
            defs.extend(config_tool_definitions());
        }
        if self.node_run.is_some() {
            defs.push(crate::node_run::node_run_tool_definition());
        }
        #[cfg(feature = "improvement")]
        if self.improvement_tools_enabled() {
            defs.extend(crate::improvement_tools::improvement_tool_definitions());
        }
        defs
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        Self::cacheability_for(tool_name)
    }
}

impl std::fmt::Debug for FawxToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("FawxToolExecutor");
        debug
            .field("working_dir", &self.working_dir)
            .field("config", &self.config)
            .field("process_registry", &true)
            .field("memory", &self.memory.is_some())
            .field("embedding_index", &self.embedding_index.is_some())
            .field("runtime_info", &self.runtime_info.is_some())
            .field("self_modify", &self.self_modify)
            .field("concurrency_policy", &self.concurrency_policy)
            .field("config_manager", &self.config_manager.is_some())
            .field("kernel_budget", &self.kernel_budget)
            .field("subagent_control", &self.subagent_control.is_some())
            .field("experiment", &self.experiment.is_some())
            .field("experiment_progress", &self.experiment_progress.is_some())
            .field("experiment_registrar", &self.experiment_registrar.is_some())
            .field("background_experiments", &self.background_experiments);
        #[cfg(feature = "improvement")]
        debug.field("improvement", &self.improvement.is_some());
        debug.finish()
    }
}

struct ConcurrentToolTask {
    executor: FawxToolExecutor,
    index: usize,
    call: ToolCall,
    cancel: Option<CancellationToken>,
    semaphore: Option<Arc<tokio::sync::Semaphore>>,
    timeout: Option<Duration>,
}

fn create_semaphore(max_parallel: Option<NonZeroUsize>) -> Option<Arc<tokio::sync::Semaphore>> {
    max_parallel.map(|limit| Arc::new(tokio::sync::Semaphore::new(limit.get())))
}

async fn execute_one_tool(task: ConcurrentToolTask) -> (usize, ToolResult) {
    if is_cancelled(task.cancel.as_ref()) {
        return (task.index, cancelled_result(&task.call.id, &task.call.name));
    }
    let _permit = acquire_permit(&task.semaphore).await;
    if is_cancelled(task.cancel.as_ref()) {
        return (task.index, cancelled_result(&task.call.id, &task.call.name));
    }
    let result = execute_with_timeout(
        &task.executor,
        &task.call,
        task.cancel.as_ref(),
        task.timeout,
    )
    .await;
    (task.index, result)
}

async fn acquire_permit(
    semaphore: &Option<Arc<tokio::sync::Semaphore>>,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    if let Some(sem) = semaphore {
        sem.clone().acquire_owned().await.ok()
    } else {
        None
    }
}

async fn execute_with_timeout(
    executor: &FawxToolExecutor,
    call: &ToolCall,
    cancel: Option<&CancellationToken>,
    timeout: Option<Duration>,
) -> ToolResult {
    match timeout {
        Some(duration) => {
            match tokio::time::timeout(duration, executor.execute_call(call, cancel)).await {
                Ok(result) => result,
                Err(_) => timed_out_result(&call.id, &call.name),
            }
        }
        None => executor.execute_call(call, cancel).await,
    }
}

async fn collect_ordered_results(
    join_set: &mut tokio::task::JoinSet<(usize, ToolResult)>,
    expected: usize,
) -> Result<Vec<ToolResult>, ToolExecutorError> {
    let mut indexed = Vec::with_capacity(expected);
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(pair) => indexed.push(pair),
            Err(err) => {
                return Err(ToolExecutorError {
                    message: format!("tool task panicked: {err}"),
                    recoverable: false,
                });
            }
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    Ok(indexed.into_iter().map(|(_, result)| result).collect())
}

pub fn fawx_tool_definitions(
    include_subagent_tools: bool,
    include_experiment_tool: bool,
) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a UTF-8 text file from disk. Supports `~` to reference the home directory."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": {
                        "type": "integer",
                        "description": "Line number to start reading from (1-indexed)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to return"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write UTF-8 content to a file on disk. Supports `~` to reference the home directory."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDefinition {
            name: "edit_file".to_string(),
            description: "Replace exact text in a file. The old_text must match exactly (including whitespace and newlines). Use for precise, surgical edits."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_text": { "type": "string" },
                    "new_text": { "type": "string" }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        },
        ToolDefinition {
            name: "list_directory".to_string(),
            description:
                "List files and directories, optionally recursively. Supports `~` to reference the home directory."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "recursive": { "type": "boolean" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "run_command".to_string(),
            description: "Run a command and capture exit code, stdout, and stderr".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "working_dir": { "type": "string" },
                    "shell": { "type": "boolean" }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "exec_background".to_string(),
            description: "Start a command in the background and return a session ID for monitoring.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "working_dir": { "type": "string" },
                    "label": { "type": "string" }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "exec_status".to_string(),
            description: "Check one background process or list all background processes.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "tail": { "type": "integer" }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "exec_kill".to_string(),
            description: "Kill a background process by session ID.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
        },
        ToolDefinition {
            name: "search_text".to_string(),
            description:
                "Search text in files and return file:line matches. Supports `~` to reference the home directory."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "file_glob": { "type": "string" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "self_info".to_string(),
            description:
                "Inspect runtime state: active model, loaded skills, configuration, and version"
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "section": {
                        "type": "string",
                        "enum": ["model", "skills", "config", "all"],
                        "description": "Filter to a specific section. Defaults to 'all'."
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "current_time".to_string(),
            description: "Get the current date, time, timezone, and Unix epoch timestamp"
                .to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
    ];
    if include_experiment_tool {
        definitions.insert(0, run_experiment_tool_definition());
    }
    if include_subagent_tools {
        definitions.extend(subagent_tool_definitions());
    }
    definitions
}

fn subagent_tool_definitions() -> Vec<ToolDefinition> {
    vec![spawn_agent_definition(), subagent_status_definition()]
}

fn spawn_agent_definition() -> ToolDefinition {
    ToolDefinition {
        name: "spawn_agent".to_string(),
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

fn subagent_status_definition() -> ToolDefinition {
    ToolDefinition {
        name: "subagent_status".to_string(),
        description: "Check status of a subagent, list all subagents, or cancel one.".to_string(),
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

pub fn memory_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "memory_write".to_string(),
            description: "Store a fact in persistent memory. Use for user preferences, project context, important decisions, or anything worth remembering across sessions."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string" },
                    "value": { "type": "string" }
                },
                "required": ["key", "value"]
            }),
        },
        ToolDefinition {
            name: "memory_read".to_string(),
            description: "Retrieve a stored fact from persistent memory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string" }
                },
                "required": ["key"]
            }),
        },
        ToolDefinition {
            name: "memory_list".to_string(),
            description: "List all stored memory keys with value previews."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "memory_delete".to_string(),
            description: "Remove a stored fact from persistent memory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string" }
                },
                "required": ["key"]
            }),
        },
        ToolDefinition {
            name: "memory_search".to_string(),
            description: "Search agent memory by meaning. Finds semantically related memories even without exact keyword matches."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language search query"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 5)"
                    }
                },
                "required": ["query"]
            }),
        },
    ]
}

fn format_memory_list(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .map(|(k, v)| {
            let preview = truncate_preview(v, 80);
            format!("- {k}: {preview}")
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

struct MemorySearchResult {
    key: String,
    value: String,
    score: Option<f32>,
}

fn format_memory_search_results(query: &str, results: &[MemorySearchResult]) -> String {
    if results.is_empty() {
        return format!("No relevant memories found for: {query}");
    }

    let items = results
        .iter()
        .enumerate()
        .map(|(index, result)| format_memory_search_item(index + 1, result))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("Found {} relevant memories:\n\n{items}", results.len())
}

fn format_memory_search_item(index: usize, result: &MemorySearchResult) -> String {
    let header = match result.score {
        Some(score) => format!("{index}. [{}] (score: {score:.2})", result.key),
        None => format!("{index}. [{}]", result.key),
    };
    let value = indent_memory_value(&result.value);
    format!("{header}\n{value}")
}

fn indent_memory_value(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("   {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_preview(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let mut end = max_len;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
}

pub fn validate_path(base: &Path, requested: &str) -> Result<PathBuf, String> {
    // NOTE: There is an unavoidable TOCTOU window between this validation and later
    // open/read/write calls that operate by path. Tightening this fully requires
    // fd-based operations end-to-end, which is not currently practical across all tools.
    let base_canon = fs::canonicalize(base).map_err(|error| error.to_string())?;
    let candidate = resolve_candidate(&base_canon, requested);
    let requested_canon = canonicalize_existing_or_parent(&candidate)?;
    if requested_canon.starts_with(&base_canon) {
        return Ok(requested_canon);
    }
    Err("path escapes working directory".to_string())
}

fn resolve_candidate(base: &Path, requested: &str) -> PathBuf {
    let requested_path = Path::new(requested);
    if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        base.join(requested_path)
    }
}

fn canonicalize_existing_or_parent(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|error| error.to_string());
    }

    let mut missing_parts = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        let name = cursor
            .file_name()
            .ok_or_else(|| "invalid target path".to_string())?;
        missing_parts.push(name.to_os_string());
        cursor = cursor
            .parent()
            .ok_or_else(|| "invalid target path".to_string())?;
    }

    let mut resolved = fs::canonicalize(cursor).map_err(|error| error.to_string())?;
    while let Some(part) = missing_parts.pop() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn to_tool_result(
    tool_call_id: &str,
    tool_name: &str,
    output: Result<String, String>,
) -> ToolResult {
    match output {
        Ok(content) => ToolResult {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            success: true,
            output: content,
        },
        Err(error) => ToolResult {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            success: false,
            output: error,
        },
    }
}

fn entry_kind(path: &Path) -> Result<&'static str, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    let kind = if metadata.file_type().is_dir() {
        "dir"
    } else if metadata.file_type().is_symlink() {
        "symlink"
    } else {
        "file"
    };
    Ok(kind)
}

fn build_command(command: &str, shell: bool, working_dir: &Path) -> Result<Command, String> {
    if shell {
        let mut built = Command::new("/bin/sh");
        built.kill_on_drop(true);
        built.arg("-c").arg(command).current_dir(working_dir);
        return Ok(built);
    }
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| "command cannot be empty".to_string())?;
    let mut built = Command::new(program);
    built.kill_on_drop(true);
    built.args(parts).current_dir(working_dir);
    Ok(built)
}

async fn wait_with_timeout(
    child: tokio::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let waited = tokio::time::timeout(timeout, child.wait_with_output()).await;
    match waited {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(_) => Err("command timed out".to_string()),
    }
}

fn format_command_output(output: std::process::Output, shell: bool) -> String {
    let mut lines = vec![format!("exit_code: {}", output.status.code().unwrap_or(-1))];
    if shell {
        lines.push("warning: command executed via shell=true".to_string());
    }
    lines.push(format!(
        "stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    ));
    lines.push(format!(
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    ));
    lines.join("\n")
}

fn matches_glob(path: &Path, file_glob: Option<&str>) -> bool {
    let Some(pattern) = file_glob else {
        return true;
    };
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    simple_glob_match(name, pattern)
}

/// Directories that should never be searched — build artifacts, VCS, dependencies.
fn is_builtin_ignored_directory(name: &str) -> bool {
    matches!(
        name,
        "target"
            | ".git"
            | "node_modules"
            | ".build"
            | "build"
            | ".gradle"
            | "__pycache__"
            | ".mypy_cache"
            | ".pytest_cache"
            | "dist"
            | ".next"
            | ".turbo"
    )
}

fn simple_glob_match(name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return name == pattern;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 2 {
        return name.starts_with(parts[0]) && name.ends_with(parts[1]);
    }
    name.contains(&pattern.replace('*', ""))
}

fn day_of_week_from_epoch(epoch: u64) -> &'static str {
    let days_since_epoch = (epoch / 86_400) as i64;
    let weekday_index = (days_since_epoch + 4).rem_euclid(7);
    match weekday_index {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        _ => "Saturday",
    }
}

fn iso8601_utc_from_epoch(epoch: u64) -> String {
    let days_since_epoch = (epoch / 86_400) as i64;
    let seconds_of_day = epoch % 86_400;
    let (year, month, day) = civil_from_days(days_since_epoch);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

fn parse_args<T: for<'de> Deserialize<'de>>(value: &serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct EditFileArgs {
    path: String,
    old_text: String,
    new_text: String,
}

#[derive(Deserialize)]
struct ListDirectoryArgs {
    path: String,
    recursive: Option<bool>,
}

#[derive(Deserialize)]
struct RunCommandArgs {
    command: String,
    working_dir: Option<String>,
    shell: Option<bool>,
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

#[derive(Deserialize)]
struct SearchTextArgs {
    pattern: String,
    path: Option<String>,
    file_glob: Option<String>,
}

#[derive(Deserialize)]
struct SelfInfoArgs {
    section: Option<String>,
}

#[derive(Deserialize)]
struct MemoryWriteArgs {
    key: String,
    value: String,
}

#[derive(Deserialize)]
struct MemoryReadArgs {
    key: String,
}

#[derive(Deserialize)]
struct MemoryDeleteArgs {
    key: String,
}

#[derive(Deserialize)]
struct MemorySearchArgs {
    query: String,
    max_results: Option<usize>,
}

#[derive(Deserialize)]
struct ConfigGetArgs {
    section: Option<String>,
}

/// Shared request type for config set — used by both the tool handler and
/// the HTTP endpoint (re-exported for fx-cli).
#[derive(Deserialize)]
pub struct ConfigSetRequest {
    pub key: String,
    pub value: String,
}

#[derive(Deserialize)]
struct FawxRestartArgs {
    reason: Option<String>,
    delay_seconds: Option<u64>,
}

/// Maximum allowed restart delay in seconds. Prevents the agent from
/// scheduling restarts hours into the future.
const MAX_RESTART_DELAY_SECS: u64 = 30;

/// Guard preventing concurrent restart scheduling. Only one restart can
/// be in-flight at a time.
static RESTART_IN_PROGRESS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Schedule a SIGHUP signal to self after a delay (Unix only).
///
/// Returns an error if a restart is already in progress. The delay is
/// clamped to [`MAX_RESTART_DELAY_SECS`].
///
/// On non-Unix platforms this is a no-op with a warning.
fn schedule_sighup_restart(delay_secs: u64, reason: String) -> Result<(), String> {
    if RESTART_IN_PROGRESS.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("a restart is already in progress".to_string());
    }
    let clamped = delay_secs.min(MAX_RESTART_DELAY_SECS);
    if clamped != delay_secs {
        tracing::info!(
            requested = delay_secs,
            clamped = clamped,
            "restart delay clamped to maximum"
        );
    }

    #[cfg(unix)]
    {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(clamped));
            tracing::info!(reason, "sending SIGHUP for graceful restart");
            // Use nix::sys::signal to avoid raw unsafe libc calls.
            use nix::sys::signal::{self, Signal};
            use nix::unistd::Pid;
            if let Err(e) = signal::kill(Pid::this(), Signal::SIGHUP) {
                tracing::error!(error = %e, "failed to send SIGHUP");
                RESTART_IN_PROGRESS.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        });
    }
    #[cfg(not(unix))]
    {
        let _ = (clamped, reason);
        tracing::warn!("SIGHUP restart not supported on this platform");
        RESTART_IN_PROGRESS.store(false, std::sync::atomic::Ordering::SeqCst);
    }
    Ok(())
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

pub fn config_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "config_get".to_string(),
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
        },
        ToolDefinition {
            name: "config_set".to_string(),
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
        },
        ToolDefinition {
            name: "fawx_status".to_string(),
            description: "Get server status: uptime, model, memory entries".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "kernel_manifest".to_string(),
            description: "Get a structured description of the kernel's current configuration, \
                permissions, budget limits, sandbox rules, and available tools. Use this at the \
                start of complex tasks to understand your capabilities and constraints before \
                planning."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "fawx_restart".to_string(),
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
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::FawxConfig;
    use fx_consensus::ProgressEvent;
    use fx_core::memory::MemoryProvider;
    use fx_embeddings::{test_support::create_test_model_dir, EmbeddingModel};
    use fx_llm::ModelRouter;
    use fx_memory::embedding_index::EmbeddingIndex;
    use fx_subagent::{test_support::StubSubagentControl, SubagentStatus};
    use tempfile::TempDir;

    /// Minimal no-op transport for testing tool_definitions() inclusion.
    struct StubNodeTransport;

    #[async_trait::async_trait]
    impl fx_fleet::NodeTransport for StubNodeTransport {
        async fn execute(
            &self,
            _node: &fx_fleet::NodeInfo,
            _command: &str,
            _timeout: Duration,
        ) -> Result<fx_fleet::CommandResult, fx_fleet::TransportError> {
            Err(fx_fleet::TransportError::Other("stub".to_string()))
        }

        async fn ping(
            &self,
            _node: &fx_fleet::NodeInfo,
        ) -> Result<Duration, fx_fleet::TransportError> {
            Err(fx_fleet::TransportError::Other("stub".to_string()))
        }
    }

    fn test_executor(root: &Path) -> FawxToolExecutor {
        FawxToolExecutor::new(root.to_path_buf(), ToolConfig::default())
    }

    fn memory_executor(root: &Path) -> (FawxToolExecutor, Arc<Mutex<fx_memory::JsonFileMemory>>) {
        let memory = Arc::new(Mutex::new(
            fx_memory::JsonFileMemory::new(root).expect("memory"),
        ));
        let executor = test_executor(root).with_memory(memory.clone());
        (executor, memory)
    }

    fn embedding_executor(
        root: &Path,
    ) -> (
        FawxToolExecutor,
        Arc<Mutex<fx_memory::JsonFileMemory>>,
        Arc<Mutex<EmbeddingIndex>>,
    ) {
        let (executor, memory) = memory_executor(root);
        let model = test_embedding_model(8);
        let index = Arc::new(Mutex::new(
            EmbeddingIndex::build_from(&[], &model).expect("index"),
        ));
        let executor = executor.with_embedding_index(index.clone());
        (executor, memory, index)
    }

    fn test_embedding_model(dimensions: usize) -> Arc<EmbeddingModel> {
        let model_dir = create_test_model_dir(dimensions);
        Arc::new(EmbeddingModel::load(model_dir.path()).expect("load test model"))
    }

    fn sample_runtime_info(model: &str) -> Arc<RwLock<RuntimeInfo>> {
        Arc::new(RwLock::new(RuntimeInfo {
            active_model: model.to_string(),
            provider: "openai".to_string(),
            skills: vec![fx_core::runtime_info::SkillInfo {
                name: "fawx-builtin".to_string(),
                description: Some("Built-in runtime tools".to_string()),
                tool_names: vec!["read_file".to_string(), "self_info".to_string()],
                capabilities: Vec::new(),
            }],
            config_summary: fx_core::runtime_info::ConfigSummary {
                max_iterations: 6,
                max_history: 128,
                memory_enabled: true,
            },
            version: "0.1.0".to_string(),
        }))
    }

    fn parse_json_output(output: &str) -> serde_json::Value {
        serde_json::from_str(output).expect("valid json output")
    }

    fn executor_with_protected_branches(root: &Path, branches: &[&str]) -> FawxToolExecutor {
        test_executor(root).with_protected_branches(
            branches
                .iter()
                .map(|branch| (*branch).to_string())
                .collect(),
        )
    }

    fn run_git_ok(repo: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should run in tests");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_push_repo() -> (TempDir, TempDir) {
        let repo = TempDir::new().expect("repo tempdir");
        let remote = TempDir::new().expect("remote tempdir");
        let remote_path = remote.path().to_str().expect("utf8 remote path");
        run_git_ok(repo.path(), &["init"]);
        run_git_ok(repo.path(), &["config", "user.email", "test@test.com"]);
        run_git_ok(repo.path(), &["config", "user.name", "Test"]);
        run_git_ok(remote.path(), &["init", "--bare"]);
        run_git_ok(repo.path(), &["remote", "add", "origin", remote_path]);
        fs::write(repo.path().join("file.txt"), "data\n").expect("write seed file");
        run_git_ok(repo.path(), &["add", "file.txt"]);
        run_git_ok(repo.path(), &["commit", "-m", "initial"]);
        run_git_ok(repo.path(), &["checkout", "-b", "dev"]);
        (repo, remote)
    }

    fn test_executor_with_subagents(root: &Path) -> FawxToolExecutor {
        test_executor_with_control(root, Arc::new(StubSubagentControl::new()))
    }

    fn test_executor_with_control(
        root: &Path,
        control: Arc<dyn SubagentControl>,
    ) -> FawxToolExecutor {
        test_executor(root).with_subagent_control(control)
    }

    fn experiment_state(root: &Path) -> ExperimentToolState {
        ExperimentToolState {
            chain_path: root.join("chain.json"),
            router: Arc::new(ModelRouter::new()),
            config: FawxConfig::default(),
        }
    }

    #[test]
    fn validate_path_accepts_path_within_jail() {
        let temp = TempDir::new().expect("tempdir");
        let result = validate_path(temp.path(), "inside.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_path_rejects_traversal_escape() {
        let temp = TempDir::new().expect("tempdir");
        let result = validate_path(temp.path(), "../../etc/passwd");
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn validate_path_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let link_path = jail.path().join("link");
        symlink(outside.path(), &link_path).expect("symlink");

        let result = validate_path(jail.path(), "link/secrets.txt");
        assert!(result.is_err());
    }

    #[test]
    fn validate_path_rejects_absolute_path_outside() {
        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let result = validate_path(jail.path(), &outside.path().to_string_lossy());
        assert!(result.is_err());
    }

    #[test]
    fn validate_path_accepts_jail_boundary() {
        let jail = TempDir::new().expect("jail");
        let result = validate_path(jail.path(), ".");
        assert!(result.is_ok());
    }

    #[test]
    fn read_file_reads_existing_file() {
        let temp = TempDir::new().expect("temp");
        let file = temp.path().join("a.txt");
        fs::write(&file, "hello").expect("write");
        let executor = test_executor(temp.path());

        let output = executor.handle_read_file(&serde_json::json!({"path": "a.txt"}));
        assert_eq!(output.expect("read"), "hello");
    }

    #[test]
    fn read_file_reports_missing_file() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_read_file(&serde_json::json!({"path": "missing.txt"}));
        assert!(output.is_err());
    }

    #[test]
    fn read_file_rejects_oversized_file() {
        let temp = TempDir::new().expect("temp");
        let file = temp.path().join("big.txt");
        fs::write(&file, "0123456789").expect("write");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_read_size: 4,
                ..ToolConfig::default()
            },
        );
        let output = executor.handle_read_file(&serde_json::json!({"path": "big.txt"}));
        assert!(output.is_err());
    }

    #[test]
    fn read_file_rejects_binary_file() {
        let temp = TempDir::new().expect("temp");
        let file = temp.path().join("bin.dat");
        fs::write(&file, [0, 159, 146, 150]).expect("write");
        let executor = test_executor(temp.path());
        let output = executor.handle_read_file(&serde_json::json!({"path": "bin.dat"}));
        assert!(matches!(output, Err(message) if message.contains("binary")));
    }

    #[test]
    fn read_file_rejects_outside_jail() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_read_file(&serde_json::json!({"path": "../escape.txt"}));
        assert!(output.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn read_file_rejects_symlink_pointing_outside_jail() {
        use std::os::unix::fs::symlink;

        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret").expect("write");
        symlink(&outside_file, jail.path().join("escape.txt")).expect("symlink");

        let executor = test_executor(jail.path());
        let output = executor.handle_read_file(&serde_json::json!({"path": "escape.txt"}));
        assert!(output.is_err());
    }

    #[test]
    fn read_file_offset_only_returns_requested_tail() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "one
two
three
four
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let output = executor
            .handle_read_file(&serde_json::json!({"path": "a.txt", "offset": 3}))
            .expect("read");
        assert_eq!(
            output,
            "[Lines 3-4 of 4]
three
four
"
        );
    }

    #[test]
    fn read_file_limit_only_returns_requested_prefix() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "one
two
three
four
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let output = executor
            .handle_read_file(&serde_json::json!({"path": "a.txt", "limit": 2}))
            .expect("read");
        assert_eq!(
            output,
            "[Lines 1-2 of 4]
one
two
"
        );
    }

    #[test]
    fn read_file_offset_and_limit_returns_requested_window() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "one
two
three
four
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let output = executor
            .handle_read_file(&serde_json::json!({"path": "a.txt", "offset": 2, "limit": 2}))
            .expect("read");
        assert_eq!(
            output,
            "[Lines 2-3 of 4]
two
three
"
        );
    }

    #[test]
    fn read_file_offset_past_end_returns_note() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "one
two
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let output = executor
            .handle_read_file(&serde_json::json!({"path": "a.txt", "offset": 3}))
            .expect("read");
        assert_eq!(
            output,
            "(no lines returned; offset 3 is past end of file with 2 lines)"
        );
    }

    #[test]
    fn read_file_limit_larger_than_file_returns_all_lines_with_header() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "one
two
three
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let output = executor
            .handle_read_file(&serde_json::json!({"path": "a.txt", "limit": 100}))
            .expect("read");
        assert_eq!(
            output,
            "[Lines 1-3 of 3]
one
two
three
"
        );
    }

    #[test]
    fn read_file_partial_output_includes_header() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "one
two
three
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let output = executor
            .handle_read_file(&serde_json::json!({"path": "a.txt", "limit": 1}))
            .expect("read");
        assert!(output.starts_with("[Lines 1-1 of 3]"));
    }

    #[test]
    fn read_file_rejects_zero_offset_and_limit() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "one
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let offset_error = executor
            .handle_read_file(&serde_json::json!({"path": "a.txt", "offset": 0}))
            .expect_err("offset should fail");
        assert!(offset_error.contains("offset must be at least 1"));

        let limit_error = executor
            .handle_read_file(&serde_json::json!({"path": "a.txt", "limit": 0}))
            .expect_err("limit should fail");
        assert!(limit_error.contains("limit must be at least 1"));
    }

    #[test]
    fn write_file_creates_file_with_content() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());

        let result =
            executor.handle_write_file(&serde_json::json!({"path": "new.txt", "content": "hello"}));
        assert!(result.is_ok());
        assert_eq!(
            fs::read_to_string(temp.path().join("new.txt")).expect("read"),
            "hello"
        );
    }

    #[test]
    fn write_file_creates_parent_directories() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let result = executor
            .handle_write_file(&serde_json::json!({"path": "a/b/c.txt", "content": "nested"}));
        assert!(result.is_ok());
        assert_eq!(
            fs::read_to_string(temp.path().join("a/b/c.txt")).expect("read"),
            "nested"
        );
    }

    #[test]
    fn write_file_rejects_oversized_content() {
        let temp = TempDir::new().expect("temp");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_file_size: 3,
                ..ToolConfig::default()
            },
        );
        let result =
            executor.handle_write_file(&serde_json::json!({"path": "x.txt", "content": "hello"}));
        assert!(result.is_err());
    }

    #[test]
    fn write_file_rejects_outside_jail() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let result =
            executor.handle_write_file(&serde_json::json!({"path": "../x.txt", "content": "no"}));
        assert!(result.is_err());
    }

    #[test]
    fn edit_file_replaces_exact_match() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "alpha
beta
gamma
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let result = executor
            .handle_edit_file(&serde_json::json!({
                "path": "a.txt",
                "old_text": "beta",
                "new_text": "delta"
            }))
            .expect("edit");
        assert!(result.contains("Successfully edited"));
        assert!(result.contains("lines 2-2"));
        assert_eq!(
            fs::read_to_string(temp.path().join("a.txt")).expect("read"),
            "alpha
delta
gamma
"
        );
    }

    #[test]
    fn edit_file_reports_missing_exact_match() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "alpha
beta
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let error = executor
            .handle_edit_file(&serde_json::json!({
                "path": "a.txt",
                "old_text": "gamma",
                "new_text": "delta"
            }))
            .expect_err("edit should fail");
        assert!(error.contains("Could not find the exact text"));
        assert!(error.contains("a.txt"));
    }

    #[test]
    fn edit_file_reports_multiple_matches() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "repeat
repeat
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        let error = executor
            .handle_edit_file(&serde_json::json!({
                "path": "a.txt",
                "old_text": "repeat",
                "new_text": "once"
            }))
            .expect_err("edit should fail");
        assert!(error.contains("Found 2 matches"));
    }

    #[test]
    fn edit_file_rejects_empty_old_text() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "alpha").expect("write");
        let executor = test_executor(temp.path());

        let error = executor
            .handle_edit_file(&serde_json::json!({
                "path": "a.txt",
                "old_text": "",
                "new_text": "beta"
            }))
            .expect_err("edit should fail");
        assert!(error.contains("old_text must not be empty"));
    }

    #[test]
    fn edit_file_rejects_noop_replacement() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "alpha").expect("write");
        let executor = test_executor(temp.path());

        let error = executor
            .handle_edit_file(&serde_json::json!({
                "path": "a.txt",
                "old_text": "alpha",
                "new_text": "alpha"
            }))
            .expect_err("edit should fail");
        assert!(error.contains("must differ"));
    }

    #[test]
    fn edit_file_rejects_binary_file() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("bin.dat"), [0, 159, 146, 150]).expect("write");
        let executor = test_executor(temp.path());

        let error = executor
            .handle_edit_file(&serde_json::json!({
                "path": "bin.dat",
                "old_text": "x",
                "new_text": "y"
            }))
            .expect_err("edit should fail");
        assert!(error.contains("binary"));
    }

    #[cfg(unix)]
    #[test]
    fn edit_file_rejects_symlink_pointing_outside_jail() {
        use std::os::unix::fs::symlink;

        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret").expect("write");
        symlink(&outside_file, jail.path().join("escape.txt")).expect("symlink");

        let executor = test_executor(jail.path());
        let result = executor.handle_edit_file(&serde_json::json!({
            "path": "escape.txt",
            "old_text": "secret",
            "new_text": "public"
        }));
        assert!(result.is_err());
    }

    #[test]
    fn edit_file_rejects_path_traversal() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let result = executor.handle_edit_file(&serde_json::json!({
            "path": "../escape.txt",
            "old_text": "x",
            "new_text": "y"
        }));
        assert!(result.is_err());
    }

    #[test]
    fn edit_file_reports_missing_file() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let result = executor.handle_edit_file(&serde_json::json!({
            "path": "missing.txt",
            "old_text": "x",
            "new_text": "y"
        }));
        assert!(result.is_err());
    }

    #[test]
    fn edit_file_allows_deletion_replacement() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "alpha beta gamma").expect("write");
        let executor = test_executor(temp.path());

        executor
            .handle_edit_file(&serde_json::json!({
                "path": "a.txt",
                "old_text": "beta ",
                "new_text": ""
            }))
            .expect("edit");
        assert_eq!(
            fs::read_to_string(temp.path().join("a.txt")).expect("read"),
            "alpha gamma"
        );
    }

    #[test]
    fn edit_file_matches_multiline_text_exactly() {
        let temp = TempDir::new().expect("temp");
        fs::write(
            temp.path().join("a.txt"),
            "one
old
block
three
",
        )
        .expect("write");
        let executor = test_executor(temp.path());

        executor
            .handle_edit_file(&serde_json::json!({
                "path": "a.txt",
                "old_text": "old
block
",
                "new_text": "new
block
"
            }))
            .expect("edit");
        assert_eq!(
            fs::read_to_string(temp.path().join("a.txt")).expect("read"),
            "one
new
block
three
"
        );
    }

    #[test]
    fn edit_file_rejects_source_file_that_exceeds_max_size() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "hello").expect("write");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_file_size: 3,
                ..ToolConfig::default()
            },
        );

        let error = executor
            .handle_edit_file(&serde_json::json!({
                "path": "a.txt",
                "old_text": "h",
                "new_text": "H"
            }))
            .expect_err("edit should fail");
        assert!(error.contains("file exceeds maximum allowed size"));
    }

    #[test]
    fn edit_file_rejects_result_that_exceeds_max_size() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "a").expect("write");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_file_size: 3,
                ..ToolConfig::default()
            },
        );

        let error = executor
            .handle_edit_file(&serde_json::json!({
                "path": "a.txt",
                "old_text": "a",
                "new_text": "long"
            }))
            .expect_err("edit should fail");
        assert!(error.contains("content exceeds maximum allowed size"));
    }

    #[test]
    fn edit_file_denied_by_self_modify() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("secret.txt"), "alpha").expect("write");
        let config = SelfModifyConfig {
            enabled: true,
            deny_paths: vec!["*.txt".to_string()],
            ..SelfModifyConfig::default()
        };
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_self_modify(config);

        let error = executor
            .handle_edit_file(&serde_json::json!({
                "path": "secret.txt",
                "old_text": "alpha",
                "new_text": "beta"
            }))
            .expect_err("edit should fail");
        assert!(error.contains("Self-modify policy violation [deny]"));
    }

    #[test]
    fn edit_file_propose_tier_creates_proposal_without_modifying_target() {
        let temp = TempDir::new().expect("temp");
        let proposals_dir = temp.path().join("proposals");
        let config = SelfModifyConfig {
            enabled: true,
            propose_paths: vec!["kernel/**".to_string()],
            proposals_dir: proposals_dir.clone(),
            ..SelfModifyConfig::default()
        };
        let kernel_dir = temp.path().join("kernel");
        fs::create_dir_all(&kernel_dir).expect("mkdir");
        fs::write(
            kernel_dir.join("loop.rs"),
            "fn old() {}
",
        )
        .expect("write");
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_self_modify(config);

        let message = executor
            .handle_edit_file(&serde_json::json!({
                "path": "kernel/loop.rs",
                "old_text": "old",
                "new_text": "new"
            }))
            .expect("proposal");
        assert!(message.contains("Proposal created"));
        assert_eq!(
            fs::read_to_string(kernel_dir.join("loop.rs")).expect("read"),
            "fn old() {}
"
        );
        assert!(proposals_dir.exists());
    }

    #[test]
    fn list_directory_returns_entries_with_types() {
        let temp = TempDir::new().expect("temp");
        fs::create_dir(temp.path().join("d")).expect("mkdir");
        fs::write(temp.path().join("f.txt"), "x").expect("write");
        let executor = test_executor(temp.path());

        let output = executor
            .handle_list_directory(&serde_json::json!({"path": "."}))
            .expect("list");
        assert!(output.contains("[dir] d"));
        assert!(output.contains("[file] f.txt"));
    }

    #[test]
    fn list_directory_rejects_missing_directory() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_list_directory(&serde_json::json!({"path": "missing"}));
        assert!(output.is_err());
    }

    #[test]
    fn list_directory_rejects_outside_jail() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_list_directory(&serde_json::json!({"path": "../"}));
        assert!(output.is_err());
    }

    #[test]
    fn list_directory_recursive_honors_depth_limit() {
        let temp = TempDir::new().expect("temp");
        let mut current = temp.path().to_path_buf();
        for depth in 0..8 {
            current = current.join(format!("d{depth}"));
            fs::create_dir_all(&current).expect("mkdir");
            fs::write(current.join("f.txt"), "x").expect("write");
        }
        let executor = test_executor(temp.path());
        let output = executor
            .handle_list_directory(&serde_json::json!({"path": ".", "recursive": true}))
            .expect("recursive list");
        assert!(output.contains("d0"));
        assert!(!output.contains("d7"));
    }

    #[cfg(unix)]
    #[test]
    fn list_directory_recursive_skips_symlink_escape() {
        use std::os::unix::fs::symlink;

        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let outside_dir = outside.path().join("secret-dir");
        fs::create_dir_all(&outside_dir).expect("mkdir");
        fs::write(outside_dir.join("secret.txt"), "secret").expect("write");
        symlink(&outside_dir, jail.path().join("escape")).expect("symlink");

        let executor = test_executor(jail.path());
        let output = executor
            .handle_list_directory(&serde_json::json!({"path": ".", "recursive": true}))
            .expect("recursive list");
        assert!(!output.contains("escape"));
        assert!(!output.contains("secret.txt"));
    }

    #[tokio::test]
    async fn run_command_captures_stdout() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_run_command(&serde_json::json!({"command": "echo hello"}))
            .await
            .expect("command");
        assert!(output.contains("exit_code: 0"));
        assert!(output.contains("hello"));
    }

    #[tokio::test]
    async fn run_command_captures_nonzero_exit_code() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_run_command(&serde_json::json!({"command": "false"}))
            .await
            .expect("command");
        assert!(output.contains("exit_code: 1"));
    }

    #[tokio::test]
    async fn run_command_times_out() {
        let temp = TempDir::new().expect("temp");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                command_timeout: Duration::from_millis(1),
                ..ToolConfig::default()
            },
        );
        let output = executor
            .handle_run_command(&serde_json::json!({"command": "sleep 1"}))
            .await;
        assert!(matches!(output, Err(message) if message.contains("timed out")));
    }

    #[tokio::test]
    async fn run_command_validates_working_directory_override() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_run_command(&serde_json::json!({"command": "echo hi", "working_dir": "../"}))
            .await;
        assert!(output.is_err());
    }

    #[tokio::test]
    async fn run_command_blocks_push_to_protected_branch() {
        let temp = TempDir::new().expect("temp");
        let executor = executor_with_protected_branches(temp.path(), &["main"]);

        let error = executor
            .handle_run_command(&serde_json::json!({"command": "git push origin main"}))
            .await
            .expect_err("protected push should be blocked");

        assert!(error.contains("protected branch(es) 'main'"));
    }

    #[tokio::test]
    async fn run_command_allows_push_to_unprotected_branch() {
        let (repo, remote) = init_push_repo();
        let executor = executor_with_protected_branches(repo.path(), &["main"]);

        let output = executor
            .handle_run_command(&serde_json::json!({"command": "git push origin dev"}))
            .await
            .expect("unprotected push should execute");

        assert!(
            output.contains("exit_code: 0"),
            "unexpected output: {output}"
        );
        let remote_path = remote.path().to_str().expect("utf8 remote path");
        let status = std::process::Command::new("git")
            .args([
                "--git-dir",
                remote_path,
                "show-ref",
                "--verify",
                "refs/heads/dev",
            ])
            .output()
            .expect("verify remote ref");
        assert!(status.status.success(), "remote dev branch should exist");
    }

    #[tokio::test]
    async fn exec_background_returns_session_id_and_status() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());

        let output = executor
            .handle_exec_background(&serde_json::json!({"command": "sleep 1", "label": "build"}))
            .expect("background spawn");
        let value = parse_json_output(&output);

        assert_eq!(value["status"], "running");
        assert_eq!(value["label"], "build");
        assert!(value["pid"].is_u64());
        assert!(value["session_id"]
            .as_str()
            .expect("session id")
            .starts_with("bg_"));
    }

    #[tokio::test]
    async fn exec_status_with_session_id_returns_tail() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let created = executor
            .handle_exec_background(&serde_json::json!({"command": "printf 'hello\\nworld\\n'"}))
            .expect("background spawn");
        let session_id = parse_json_output(&created)["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        tokio::time::sleep(Duration::from_millis(100)).await;
        let output = executor
            .handle_exec_status(&serde_json::json!({"session_id": session_id, "tail": 1}))
            .expect("status");
        let value = parse_json_output(&output);

        assert_eq!(value["tail"][0], "world");
        assert!(value.get("pid").is_none());
    }

    #[tokio::test]
    async fn exec_status_without_session_id_lists_all_processes() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        executor
            .handle_exec_background(&serde_json::json!({"command": "sleep 1", "label": "one"}))
            .expect("first spawn");
        executor
            .handle_exec_background(&serde_json::json!({"command": "sleep 1", "label": "two"}))
            .expect("second spawn");

        let output = executor
            .handle_exec_status(&serde_json::json!({}))
            .expect("status list");
        let value = parse_json_output(&output);
        let processes = value["processes"].as_array().expect("process list");

        assert_eq!(processes.len(), 2);
        assert!(processes.iter().all(|entry| entry.get("pid").is_none()));
    }

    #[tokio::test]
    async fn exec_kill_with_invalid_session_id_returns_error() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());

        let error = executor
            .handle_exec_kill(&serde_json::json!({"session_id": "bg_missing"}))
            .await
            .expect_err("invalid session should fail");

        assert!(error.contains("unknown session_id"));
    }

    #[test]
    fn exec_background_validates_working_directory() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());

        let error = executor
            .handle_exec_background(
                &serde_json::json!({"command": "sleep 1", "working_dir": "../"}),
            )
            .expect_err("outside working dir should fail");

        assert!(!error.is_empty());
    }

    #[test]
    fn exec_background_blocks_push_to_protected_branch() {
        let temp = TempDir::new().expect("temp");
        let executor = executor_with_protected_branches(temp.path(), &["main"]);

        let error = executor
            .handle_exec_background(&serde_json::json!({"command": "git push origin main"}))
            .expect_err("protected push should be blocked");

        assert!(error.contains("protected branch(es) 'main'"));
    }

    #[test]
    fn search_text_finds_pattern_with_file_and_line() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "first\nneedle\nthird").expect("write");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");
        assert!(output.contains("a.txt:2:needle"));
    }

    #[test]
    fn search_text_returns_empty_when_not_found() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "first").expect("write");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");
        assert_eq!(output, "");
    }

    #[test]
    fn search_text_limits_results_to_max_matches() {
        let temp = TempDir::new().expect("temp");
        let mut content = String::new();
        for _ in 0..150 {
            content.push_str("needle\n");
        }
        fs::write(temp.path().join("a.txt"), content).expect("write");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");
        assert_eq!(output.lines().count(), MAX_SEARCH_MATCHES);
    }

    #[test]
    fn search_text_rejects_outside_jail() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output =
            executor.handle_search_text(&serde_json::json!({"pattern": "needle", "path": "../"}));
        assert!(output.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn search_text_recursive_skips_symlink_escape() {
        use std::os::unix::fs::symlink;

        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "needle").expect("write");
        symlink(&outside_file, jail.path().join("escape.txt")).expect("symlink");

        let executor = test_executor(jail.path());
        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle", "path": "."}))
            .expect("search");
        assert!(output.is_empty());
    }

    #[test]
    fn search_text_skips_target_directory() {
        let dir = TempDir::new().expect("tempdir");
        let target_dir = dir.path().join("target").join("debug");
        fs::create_dir_all(&target_dir).expect("mkdir target");
        fs::write(target_dir.join("foo.rs"), "needle in target").expect("write target");
        fs::write(dir.path().join("src.rs"), "needle in source").expect("write source");

        let executor = FawxToolExecutor::new(dir.path().to_path_buf(), ToolConfig::default());
        let result = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");

        assert!(result.contains("src.rs"), "should find needle in source");
        assert!(!result.contains("target"), "should skip target directory");
    }

    #[test]
    fn search_text_skips_git_directory() {
        let dir = TempDir::new().expect("tempdir");
        let git_dir = dir.path().join(".git").join("objects");
        fs::create_dir_all(&git_dir).expect("mkdir git");
        fs::write(git_dir.join("pack"), "needle in git").expect("write git");
        fs::write(dir.path().join("main.rs"), "needle in main").expect("write main");

        let executor = FawxToolExecutor::new(dir.path().to_path_buf(), ToolConfig::default());
        let result = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");

        assert!(result.contains("main.rs"), "should find needle in main");
        assert!(!result.contains(".git"), "should skip .git directory");
    }

    #[test]
    fn search_text_does_not_skip_file_named_target() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("target"), "needle in file named target")
            .expect("write target file");

        let executor = FawxToolExecutor::new(dir.path().to_path_buf(), ToolConfig::default());
        let result = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");

        assert!(result.contains("target"), "should search file named target");
    }

    #[test]
    fn search_text_skips_node_modules() {
        let dir = TempDir::new().expect("tempdir");
        let nm_dir = dir.path().join("node_modules").join("lodash");
        fs::create_dir_all(&nm_dir).expect("mkdir node_modules");
        fs::write(nm_dir.join("index.js"), "needle in node_modules").expect("write node_modules");
        fs::write(dir.path().join("app.rs"), "needle in app").expect("write app");

        let executor = FawxToolExecutor::new(dir.path().to_path_buf(), ToolConfig::default());
        let result = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");

        assert!(result.contains("app.rs"), "should find needle in app");
        assert!(!result.contains("node_modules"), "should skip node_modules");
    }

    #[test]
    fn is_ignored_directory_covers_known_dirs() {
        assert!(is_builtin_ignored_directory("target"));
        assert!(is_builtin_ignored_directory(".git"));
        assert!(is_builtin_ignored_directory("node_modules"));
        assert!(is_builtin_ignored_directory(".build"));
        assert!(!is_builtin_ignored_directory("src"));
        assert!(!is_builtin_ignored_directory("engine"));
        assert!(!is_builtin_ignored_directory("docs"));
    }

    #[test]
    fn search_text_ignores_files_over_max_file_size() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("big.txt"), "needle\nneedle").expect("write");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_read_size: 4,
                ..ToolConfig::default()
            },
        );

        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");
        assert!(output.is_empty());
    }

    #[test]
    fn search_text_finds_nested_rust_and_markdown_when_large_files_exist() {
        let temp = TempDir::new().expect("temp");
        for index in 0..MAX_SEARCH_MATCHES {
            let path = temp.path().join(format!("large-{index}.bin"));
            fs::write(path, "x".repeat(64)).expect("write");
        }

        let nested = temp.path().join("engine/crates/fx-foo/src");
        fs::create_dir_all(&nested).expect("mkdir");
        let rust_file = nested.join("lib.rs");
        let markdown_file = temp.path().join("DOCTRINE.md");
        fs::write(&rust_file, "pub struct ToolExecutor;\n").expect("write");
        fs::write(&markdown_file, "ToolExecutor reference\n").expect("write");

        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_read_size: 32,
                ..ToolConfig::default()
            },
        );

        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "ToolExecutor"}))
            .expect("search");
        let matches = output.lines().collect::<Vec<_>>();

        assert_eq!(
            matches.len(),
            2,
            "oversized files must not consume match budget"
        );
        assert!(matches
            .iter()
            .any(|line| line.contains("lib.rs:1:pub struct ToolExecutor;")));
        assert!(matches
            .iter()
            .any(|line| line.contains("DOCTRINE.md:1:ToolExecutor reference")));
    }

    #[test]
    fn current_time_returns_epoch_and_date() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_current_time().expect("current_time");
        assert!(output.contains("epoch:"));
        assert!(output.contains("iso8601_utc:"));

        let epoch_line = output
            .lines()
            .find(|line| line.starts_with("epoch:"))
            .expect("epoch line");
        let epoch = epoch_line
            .split(':')
            .nth(1)
            .expect("epoch value")
            .trim()
            .parse::<u64>()
            .expect("parse epoch");
        assert!(epoch > 1_577_836_800);
    }

    #[test]
    fn time_format_helpers_format_epoch_zero_deterministically() {
        let epoch = 0;

        assert_eq!(iso8601_utc_from_epoch(epoch), "1970-01-01T00:00:00Z");
        assert_eq!(day_of_week_from_epoch(epoch), "Thursday");
    }

    #[test]
    fn time_format_helpers_format_known_friday_deterministically() {
        let epoch = 1_709_251_200;

        assert_eq!(iso8601_utc_from_epoch(epoch), "2024-03-01T00:00:00Z");
        assert_eq!(day_of_week_from_epoch(epoch), "Friday");
    }

    #[tokio::test]
    async fn execute_call_returns_cancelled_when_token_is_pre_cancelled() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "current_time".to_string(),
            arguments: serde_json::json!({}),
        };
        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = executor.execute_call(&call, Some(&cancel)).await;
        assert!(!result.success);
        assert_eq!(result.output, "tool execution cancelled");
    }

    #[tokio::test]
    async fn current_time_tool_dispatch() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "current_time".to_string(),
            arguments: serde_json::json!({}),
        }];

        let results = executor.execute_tools(&calls, None).await.expect("results");
        assert!(results[0].success);
        assert!(results[0].output.contains("day_of_week:"));
    }

    #[test]
    fn run_experiment_definition_only_appears_when_enabled() {
        let without_experiment = fawx_tool_definitions(false, false);
        assert!(!without_experiment
            .iter()
            .any(|tool| tool.name == "run_experiment"));

        let with_experiment = fawx_tool_definitions(false, true);
        assert!(with_experiment
            .iter()
            .any(|tool| tool.name == "run_experiment"));
    }

    #[tokio::test]
    async fn run_experiment_uses_executor_progress_callback() {
        let temp = TempDir::new().expect("tempdir");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::clone(&recorded);
        let executor = test_executor(temp.path())
            .with_experiment(experiment_state(temp.path()))
            .with_experiment_progress(Arc::new(move |event: &ProgressEvent| {
                events.lock().expect("progress lock").push(event.clone());
            }));
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_experiment".to_string(),
            arguments: serde_json::json!({
                "signal": "signal",
                "hypothesis": "test hypothesis",
                "mode": "placeholder",
                "nodes": 1,
                "project": temp.path().display().to_string(),
            }),
        };

        let result = executor.execute_call(&call, None).await;
        let events = recorded.lock().expect("progress lock").clone();

        assert!(result.success, "{}", result.output);
        assert!(matches!(
            events.first(),
            Some(ProgressEvent::RoundStarted { .. })
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_experiment_background_mode_returns_immediately() {
        let temp = TempDir::new().expect("tempdir");
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Arc::new(Mutex::new(Some(release_rx)));
        let executor = test_executor(temp.path())
            .with_experiment(experiment_state(temp.path()))
            .with_background_experiments(true)
            .with_experiment_progress(Arc::new(move |_event: &ProgressEvent| {
                let _ = started_tx.send(());
                if let Some(receiver) = release_rx.lock().expect("release lock").take() {
                    receiver.recv().expect("release signal");
                }
            }));
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_experiment".to_string(),
            arguments: serde_json::json!({
                "signal": "signal",
                "hypothesis": "test hypothesis",
                "mode": "placeholder",
                "nodes": 1,
                "project": temp.path().display().to_string(),
            }),
        };

        let result = tokio::time::timeout(
            Duration::from_millis(500),
            executor.execute_call(&call, None),
        )
        .await
        .expect("background mode should return without waiting for progress");
        assert!(result.success, "{}", result.output);
        assert!(result.output.contains("Experiment started in background"));
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("background experiment should start");
        release_tx.send(()).expect("release background experiment");
    }

    #[test]
    fn current_time_appears_in_definitions() {
        let definitions = fawx_tool_definitions(false, false);
        assert!(definitions.iter().any(|tool| tool.name == "current_time"));
    }

    #[test]
    fn edit_file_appears_in_definitions() {
        let definitions = fawx_tool_definitions(false, false);
        assert!(definitions.iter().any(|tool| tool.name == "edit_file"));
    }

    #[test]
    fn background_process_tools_appear_in_definitions() {
        let definitions = fawx_tool_definitions(false, false);
        assert!(definitions
            .iter()
            .any(|tool| tool.name == "exec_background"));
        assert!(definitions.iter().any(|tool| tool.name == "exec_status"));
        assert!(definitions.iter().any(|tool| tool.name == "exec_kill"));
    }

    #[test]
    fn read_file_definition_exposes_offset_and_limit() {
        let definitions = fawx_tool_definitions(false, false);
        let read_file = definitions
            .iter()
            .find(|tool| tool.name == "read_file")
            .expect("read_file definition");
        assert!(read_file.parameters["properties"].get("offset").is_some());
        assert!(read_file.parameters["properties"].get("limit").is_some());
    }

    #[test]
    fn cacheability_classifies_builtin_tools() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());

        assert_eq!(
            executor.cacheability("read_file"),
            ToolCacheability::Cacheable
        );
        assert_eq!(
            executor.cacheability("memory_list"),
            ToolCacheability::Cacheable
        );
        assert_eq!(
            executor.cacheability("memory_search"),
            ToolCacheability::Cacheable
        );
        assert_eq!(
            executor.cacheability("write_file"),
            ToolCacheability::SideEffect
        );
        assert_eq!(
            executor.cacheability("edit_file"),
            ToolCacheability::SideEffect
        );
        assert_eq!(
            executor.cacheability("run_command"),
            ToolCacheability::SideEffect
        );
        assert_eq!(
            executor.cacheability("exec_background"),
            ToolCacheability::SideEffect
        );
        assert_eq!(
            executor.cacheability("exec_kill"),
            ToolCacheability::SideEffect
        );
        assert_eq!(
            executor.cacheability("spawn_agent"),
            ToolCacheability::SideEffect
        );
        assert_eq!(
            executor.cacheability("current_time"),
            ToolCacheability::NeverCache
        );
        assert_eq!(
            executor.cacheability("exec_status"),
            ToolCacheability::NeverCache
        );
        assert_eq!(
            executor.cacheability("self_info"),
            ToolCacheability::NeverCache
        );
        assert_eq!(
            executor.cacheability("subagent_status"),
            ToolCacheability::NeverCache
        );
        assert_eq!(
            executor.cacheability("unknown_tool"),
            ToolCacheability::NeverCache
        );
    }

    #[test]
    fn subagent_tools_appear_when_control_is_configured() {
        let temp = TempDir::new().expect("temp");
        let names = test_executor_with_subagents(temp.path())
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"spawn_agent".to_string()));
        assert!(names.contains(&"subagent_status".to_string()));
    }

    #[test]
    fn subagent_tools_absent_without_control() {
        let temp = TempDir::new().expect("temp");
        let names = test_executor(temp.path())
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(!names.contains(&"spawn_agent".to_string()));
        assert!(!names.contains(&"subagent_status".to_string()));
    }

    #[tokio::test]
    async fn spawn_agent_dispatches_to_subagent_control() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor_with_subagents(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "spawn_agent".to_string(),
            arguments: serde_json::json!({"task": "Review this", "mode": "session"}),
        };

        let result = executor.execute_call(&call, None).await;
        let output = parse_json_output(&result.output);
        assert!(result.success);
        assert_eq!(output["id"], "agent-1");
        assert_eq!(output["status"]["state"], "running");
    }

    #[tokio::test]
    async fn subagent_status_dispatches_send_action() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor_with_subagents(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "subagent_status".to_string(),
            arguments: serde_json::json!({
                "action": "send",
                "id": "agent-1",
                "message": "try again"
            }),
        };

        let result = executor.execute_call(&call, None).await;
        let output = parse_json_output(&result.output);
        assert!(result.success);
        assert_eq!(output["response"], "reply");
    }

    #[tokio::test]
    async fn subagent_status_rejects_missing_id() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor_with_subagents(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "subagent_status".to_string(),
            arguments: serde_json::json!({"action": "status"}),
        };

        let result = executor.execute_call(&call, None).await;
        assert!(!result.success);
        assert!(result.output.contains("id is required"));
    }

    #[tokio::test]
    async fn subagent_status_rejects_missing_message_for_send() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor_with_subagents(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "subagent_status".to_string(),
            arguments: serde_json::json!({"action": "send", "id": "agent-1"}),
        };

        let result = executor.execute_call(&call, None).await;
        assert!(!result.success);
        assert!(result.output.contains("message is required"));
    }

    #[tokio::test]
    async fn subagent_cancel_returns_actual_post_cancel_status() {
        let temp = TempDir::new().expect("temp");
        let control = Arc::new(
            StubSubagentControl::new().with_status(SubagentStatus::Completed {
                result: "done".to_string(),
                tokens_used: 42,
            }),
        );
        let executor = test_executor_with_control(temp.path(), control);
        let call = ToolCall {
            id: "1".to_string(),
            name: "subagent_status".to_string(),
            arguments: serde_json::json!({"action": "cancel", "id": "agent-1"}),
        };

        let result = executor.execute_call(&call, None).await;
        let output = parse_json_output(&result.output);
        assert!(result.success);
        assert_eq!(output["status"]["state"], "completed");
        assert_eq!(output["status"]["result"], "done");
        assert_eq!(output["status"]["tokens_used"], 42);
    }

    #[tokio::test]
    async fn subagent_status_includes_initial_response_when_available() {
        let temp = TempDir::new().expect("temp");
        let control = Arc::new(StubSubagentControl::new().with_initial_response("initial"));
        let executor = test_executor_with_control(temp.path(), control);
        let call = ToolCall {
            id: "1".to_string(),
            name: "subagent_status".to_string(),
            arguments: serde_json::json!({"action": "status", "id": "agent-1"}),
        };

        let result = executor.execute_call(&call, None).await;
        let output = parse_json_output(&result.output);
        assert!(result.success);
        assert_eq!(output["initial_response"], "initial");
    }

    #[tokio::test]
    async fn spawn_agent_rejects_invalid_mode() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor_with_subagents(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "spawn_agent".to_string(),
            arguments: serde_json::json!({"task": "Review this", "mode": "bad"}),
        };

        let result = executor.execute_call(&call, None).await;
        assert!(!result.success);
        assert!(result.output.contains("unknown spawn mode"));
    }

    #[test]
    fn spawn_agent_schema_omits_model_override() {
        let definition = spawn_agent_definition();
        let properties = definition.parameters["properties"]
            .as_object()
            .expect("spawn properties object");

        assert!(!properties.contains_key("model"));
    }

    #[tokio::test]
    async fn spawn_agent_rejects_model_override() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor_with_subagents(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "spawn_agent".to_string(),
            arguments: serde_json::json!({
                "task": "Review this",
                "model": "anthropic/claude-opus-4-6"
            }),
        };

        let result = executor.execute_call(&call, None).await;
        assert!(!result.success);
        assert!(result.output.contains("model override is not supported"));
    }

    #[test]
    fn self_info_returns_all_sections() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path()).with_runtime_info(sample_runtime_info("model-a"));

        let output = executor
            .handle_self_info(&serde_json::json!({}))
            .expect("self_info");
        let parsed = parse_json_output(&output);
        assert_eq!(parsed["model"]["active"], "model-a");
        assert_eq!(parsed["model"]["provider"], "openai");
        assert!(parsed.get("skills").is_some());
        assert!(parsed.get("config").is_some());
        assert!(parsed.get("version").is_some());
    }

    #[test]
    fn self_info_filters_to_model_section() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path()).with_runtime_info(sample_runtime_info("model-a"));

        let output = executor
            .handle_self_info(&serde_json::json!({"section": "model"}))
            .expect("self_info model");
        let parsed = parse_json_output(&output);
        let object = parsed.as_object().expect("model section object");

        assert_eq!(object.len(), 1);
        assert_eq!(parsed["model"]["active"], "model-a");
        assert_eq!(parsed["model"]["provider"], "openai");
    }

    #[test]
    fn self_info_filters_to_skills_section() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path()).with_runtime_info(sample_runtime_info("model-a"));

        let output = executor
            .handle_self_info(&serde_json::json!({"section": "skills"}))
            .expect("self_info skills");
        let parsed = parse_json_output(&output);
        let object = parsed.as_object().expect("skills section object");

        assert_eq!(object.len(), 1);
        assert_eq!(parsed["skills"][0]["name"], "fawx-builtin");
    }

    #[test]
    fn self_info_filters_to_config_section() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path()).with_runtime_info(sample_runtime_info("model-a"));

        let output = executor
            .handle_self_info(&serde_json::json!({"section": "config"}))
            .expect("self_info config");
        let parsed = parse_json_output(&output);
        let object = parsed.as_object().expect("config section object");

        assert_eq!(object.len(), 1);
        assert_eq!(parsed["config"]["max_iterations"], 6);
        assert!(parsed["config"]["memory_enabled"]
            .as_bool()
            .expect("memory_enabled bool"));
    }

    #[test]
    fn self_info_reflects_runtime_state_changes() {
        let temp = TempDir::new().expect("temp");
        let runtime_info = sample_runtime_info("model-a");
        let executor = test_executor(temp.path()).with_runtime_info(runtime_info.clone());

        let first = executor
            .handle_self_info(&serde_json::json!({"section": "model"}))
            .expect("first self_info");
        assert_eq!(parse_json_output(&first)["model"]["active"], "model-a");

        {
            let mut guard = runtime_info.write().expect("runtime info write lock");
            guard.active_model = "model-b".to_string();
        }

        let second = executor
            .handle_self_info(&serde_json::json!({"section": "model"}))
            .expect("second self_info");
        assert_eq!(parse_json_output(&second)["model"]["active"], "model-b");
    }

    #[test]
    fn self_info_appears_in_tool_definitions() {
        let definitions = fawx_tool_definitions(false, false);
        assert!(definitions.iter().any(|tool| tool.name == "self_info"));
    }

    #[test]
    fn self_info_unknown_section_returns_error() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path()).with_runtime_info(sample_runtime_info("model-a"));

        let error = executor
            .handle_self_info(&serde_json::json!({"section": "invalid"}))
            .expect_err("invalid section should fail");
        assert!(error.contains("unknown section 'invalid'"));
        assert!(error.contains("valid sections: model, skills, config, all"));
    }

    #[test]
    fn self_info_returns_error_when_not_configured() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());

        let error = executor
            .handle_self_info(&serde_json::json!({}))
            .expect_err("missing runtime info should fail");
        assert_eq!(error, "runtime info not configured");
    }

    #[tokio::test]
    async fn tool_dispatch_handles_known_tool() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "hello").expect("write");
        let executor = test_executor(temp.path());
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.txt"}),
        }];
        let results = executor.execute_tools(&calls, None).await.expect("results");
        assert!(results[0].success);
    }

    #[tokio::test]
    async fn tool_dispatch_handles_unknown_tool() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "missing_tool".to_string(),
            arguments: serde_json::json!({}),
        }];
        let results = executor.execute_tools(&calls, None).await.expect("results");
        assert!(!results[0].success);
        assert!(results[0].output.contains("unknown tool"));
    }

    #[test]
    fn node_run_appears_in_definitions_when_configured() {
        let temp = TempDir::new().expect("temp");
        let registry = Arc::new(tokio::sync::RwLock::new(fx_fleet::NodeRegistry::new()));
        let transport: Arc<dyn fx_fleet::NodeTransport> = Arc::new(StubNodeTransport);
        let state = crate::node_run::NodeRunState {
            registry,
            transport,
        };
        let executor = test_executor(temp.path()).with_node_run(state);

        let defs = executor.tool_definitions();
        let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(
            names.contains(&"node_run"),
            "node_run should be present when configured"
        );
    }

    #[test]
    fn node_run_absent_in_definitions_when_not_configured() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());

        let defs = executor.tool_definitions();
        let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(
            !names.contains(&"node_run"),
            "node_run should be absent when not configured"
        );
    }

    #[test]
    fn memory_tools_appear_in_definitions_when_memory_configured() {
        let temp = TempDir::new().expect("temp");
        let (executor, _memory) = memory_executor(temp.path());
        let defs = executor.tool_definitions();
        let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"memory_write"));
        assert!(names.contains(&"memory_read"));
        assert!(names.contains(&"memory_list"));
        assert!(names.contains(&"memory_delete"));
        assert!(names.contains(&"memory_search"));
    }

    #[test]
    fn memory_tools_absent_when_no_memory() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let defs = executor.tool_definitions();
        let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(!names.contains(&"memory_write"));
    }

    #[test]
    fn memory_write_tool_stores_value() {
        let temp = TempDir::new().expect("temp");
        let memory = Arc::new(Mutex::new(
            fx_memory::JsonFileMemory::new(temp.path()).expect("memory"),
        ));
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_memory(memory.clone());
        let result = executor
            .handle_memory_write(&serde_json::json!({"key": "name", "value": "Alice"}))
            .expect("write");
        assert!(result.contains("stored"));
        let guard = memory.lock().expect("lock");
        assert_eq!(guard.read("name"), Some("Alice".to_string()));
    }

    #[test]
    fn memory_read_tool_retrieves_value() {
        let temp = TempDir::new().expect("temp");
        let memory = Arc::new(Mutex::new(
            fx_memory::JsonFileMemory::new(temp.path()).expect("memory"),
        ));
        {
            let mut guard = memory.lock().expect("lock");
            guard.write("city", "Paris").expect("write");
        }
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_memory(memory);
        let result = executor
            .handle_memory_read(&serde_json::json!({"key": "city"}))
            .expect("read");
        assert_eq!(result, "Paris");
    }

    #[test]
    fn memory_list_tool_returns_entries() {
        let temp = TempDir::new().expect("temp");
        let memory = Arc::new(Mutex::new(
            fx_memory::JsonFileMemory::new(temp.path()).expect("memory"),
        ));
        {
            let mut guard = memory.lock().expect("lock");
            guard.write("name", "Alice").expect("write");
            guard.write("color", "blue").expect("write");
        }
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_memory(memory);
        let result = executor.handle_memory_list().expect("list");
        assert!(
            result.contains("name"),
            "list should contain key 'name', got: {result}"
        );
        assert!(
            result.contains("color"),
            "list should contain key 'color', got: {result}"
        );
        assert!(
            result.contains("Alice"),
            "list should contain value 'Alice', got: {result}"
        );
    }

    #[test]
    fn memory_delete_tool_removes_entry() {
        let temp = TempDir::new().expect("temp");
        let memory = Arc::new(Mutex::new(
            fx_memory::JsonFileMemory::new(temp.path()).expect("memory"),
        ));
        {
            let mut guard = memory.lock().expect("lock");
            guard.write("temp_key", "temp_value").expect("write");
        }
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_memory(memory.clone());
        let result = executor
            .handle_memory_delete(&serde_json::json!({"key": "temp_key"}))
            .expect("delete");
        assert!(
            result.contains("deleted"),
            "should confirm deletion, got: {result}"
        );
        // Verify it's actually gone
        let guard = memory.lock().expect("lock");
        assert_eq!(
            guard.read("temp_key"),
            None,
            "key should be deleted from memory"
        );
    }

    #[test]
    fn memory_list_tool_returns_empty_message() {
        let temp = TempDir::new().expect("temp");
        let memory = Arc::new(Mutex::new(
            fx_memory::JsonFileMemory::new(temp.path()).expect("memory"),
        ));
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_memory(memory);
        let result = executor.handle_memory_list().expect("list");
        assert_eq!(result, "no memories stored");
    }

    #[test]
    fn memory_delete_tool_returns_not_found() {
        let temp = TempDir::new().expect("temp");
        let (executor, _memory) = memory_executor(temp.path());
        let result = executor
            .handle_memory_delete(&serde_json::json!({"key": "nonexistent"}))
            .expect("delete");
        assert!(
            result.contains("not found"),
            "should say not found, got: {result}"
        );
    }

    #[test]
    fn memory_search_tool_returns_formatted_results_with_scores() {
        let temp = TempDir::new().expect("temp");
        let (executor, memory, index) = embedding_executor(temp.path());
        {
            let mut guard = memory.lock().expect("lock");
            guard
                .write(
                    "auth_decision",
                    "Switched to PKCE OAuth flow for ChatGPT credentials.",
                )
                .expect("write auth");
            guard
                .write(
                    "security_review",
                    "Bearer token stored in encrypted credential store.",
                )
                .expect("write security");
        }
        {
            let mut guard = index.lock().expect("lock");
            guard
                .upsert(
                    "auth_decision",
                    "Switched to PKCE OAuth flow for ChatGPT credentials.",
                )
                .expect("index auth");
            guard
                .upsert(
                    "security_review",
                    "Bearer token stored in encrypted credential store.",
                )
                .expect("index security");
        }

        let result = executor
            .handle_memory_search(&serde_json::json!({
                "query": "Switched to PKCE OAuth flow for ChatGPT credentials.",
                "max_results": 2
            }))
            .expect("memory search");

        assert!(result.contains("Found 2 relevant memories:"), "{result}");
        assert!(result.contains("[auth_decision]"), "{result}");
        assert!(result.contains("score:"), "{result}");
    }

    #[test]
    fn memory_search_tool_returns_helpful_message_when_empty() {
        let temp = TempDir::new().expect("temp");
        let (executor, _memory, _index) = embedding_executor(temp.path());

        let result = executor
            .handle_memory_search(&serde_json::json!({"query": "oauth"}))
            .expect("memory search");

        assert_eq!(result, "No relevant memories found for: oauth");
    }

    #[test]
    fn memory_search_tool_falls_back_to_keyword_search_without_index() {
        let temp = TempDir::new().expect("temp");
        let (executor, memory) = memory_executor(temp.path());
        {
            let mut guard = memory.lock().expect("lock");
            guard
                .write("project_notes", "shipping auth flow soon")
                .expect("write notes");
        }

        let result = executor
            .handle_memory_search(&serde_json::json!({"query": "project auth"}))
            .expect("keyword fallback");

        assert!(result.contains("Found 1 relevant memories:"), "{result}");
        assert!(result.contains("[project_notes]"), "{result}");
        assert!(!result.contains("score:"), "{result}");
    }

    #[test]
    fn memory_search_tool_falls_back_to_keyword_search_when_index_search_fails() {
        let temp = TempDir::new().expect("temp");
        let (executor, memory, index) = embedding_executor(temp.path());
        {
            let mut guard = memory.lock().expect("lock");
            guard
                .write("project_notes", "shipping auth flow soon")
                .expect("write notes");
        }
        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = index.lock().expect("lock");
            panic!("poison embedding index");
        }));
        assert!(panic_result.is_err(), "expected poisoned index");

        let result = executor
            .handle_memory_search(&serde_json::json!({"query": "project auth"}))
            .expect("keyword fallback after semantic failure");

        assert!(result.contains("Found 1 relevant memories:"), "{result}");
        assert!(result.contains("[project_notes]"), "{result}");
        assert!(!result.contains("score:"), "{result}");
    }

    #[test]
    fn memory_search_touches_returned_keys() {
        let temp = TempDir::new().expect("temp");
        let (executor, memory) = memory_executor(temp.path());
        {
            let mut guard = memory.lock().expect("lock");
            guard.write("alpha", "general notes").expect("write alpha");
            guard
                .write("project_notes", "shipping auth flow soon")
                .expect("write notes");
        }

        // Search for "project auth" — should match project_notes and touch it.
        let result = executor
            .handle_memory_search(&serde_json::json!({"query": "project auth"}))
            .expect("memory search");
        assert!(
            result.contains("project_notes"),
            "search should find project_notes: {result}"
        );
        assert!(
            !result.contains("[alpha]"),
            "search should NOT match alpha: {result}"
        );
    }

    #[test]
    fn memory_search_respects_max_results_parameter() {
        let temp = TempDir::new().expect("temp");
        let (executor, memory, index) = embedding_executor(temp.path());
        let entries = [
            ("alpha", "hello world"),
            ("beta", "hello world"),
            ("gamma", "hello world"),
        ];
        {
            let mut guard = memory.lock().expect("lock");
            for (key, value) in entries {
                guard.write(key, value).expect("write entry");
            }
        }
        {
            let mut guard = index.lock().expect("lock");
            for (key, value) in entries {
                guard.upsert(key, value).expect("index entry");
            }
        }

        let result = executor
            .handle_memory_search(&serde_json::json!({
                "query": "hello world",
                "max_results": 2
            }))
            .expect("memory search");

        assert!(result.contains("Found 2 relevant memories:"), "{result}");
        assert!(!result.contains("3. ["), "{result}");
    }

    #[test]
    fn memory_search_uses_default_max_results_of_five() {
        let temp = TempDir::new().expect("temp");
        let (executor, memory, index) = embedding_executor(temp.path());
        let entries = [
            ("alpha", "hello world"),
            ("beta", "hello world"),
            ("gamma", "hello world"),
            ("delta", "hello world"),
            ("epsilon", "hello world"),
            ("zeta", "hello world"),
        ];
        {
            let mut guard = memory.lock().expect("lock");
            for (key, value) in entries {
                guard.write(key, value).expect("write entry");
            }
        }
        {
            let mut guard = index.lock().expect("lock");
            for (key, value) in entries {
                guard.upsert(key, value).expect("index entry");
            }
        }

        let result = executor
            .handle_memory_search(&serde_json::json!({"query": "hello world"}))
            .expect("memory search");

        assert!(result.contains("Found 5 relevant memories:"), "{result}");
        assert!(!result.contains("6. ["), "{result}");
    }

    #[test]
    fn memory_write_updates_embedding_index() {
        let temp = TempDir::new().expect("temp");
        let (executor, _memory, index) = embedding_executor(temp.path());

        executor
            .handle_memory_write(&serde_json::json!({
                "key": "hello_memory",
                "value": "hello world"
            }))
            .expect("memory write");

        let results = index
            .lock()
            .expect("lock")
            .search("hello world", 5)
            .expect("search index");
        assert!(results.iter().any(|(key, _)| key == "hello_memory"));
    }

    #[test]
    fn memory_delete_removes_from_embedding_index() {
        let temp = TempDir::new().expect("temp");
        let (executor, memory, index) = embedding_executor(temp.path());
        {
            let mut memory_guard = memory.lock().expect("lock memory");
            memory_guard
                .write("hello_memory", "hello world")
                .expect("write");
        }
        {
            let mut index_guard = index.lock().expect("lock index");
            index_guard
                .upsert("hello_memory", "hello world")
                .expect("index entry");
        }

        executor
            .handle_memory_delete(&serde_json::json!({"key": "hello_memory"}))
            .expect("memory delete");

        let results = index
            .lock()
            .expect("lock")
            .search("hello world", 5)
            .expect("search index");
        assert!(results.iter().all(|(key, _)| key != "hello_memory"));
    }

    #[test]
    fn search_exclude_config_excludes_custom_directories() {
        let temp = TempDir::new().expect("temp");
        // Create a custom-excluded directory with a matching file
        let vendor = temp.path().join("vendor");
        fs::create_dir(&vendor).expect("create vendor");
        fs::write(vendor.join("lib.rs"), "fn needle() {}").expect("write vendor");
        // Create a normal directory with a matching file
        let src = temp.path().join("src");
        fs::create_dir(&src).expect("create src");
        fs::write(src.join("main.rs"), "fn needle() {}").expect("write src");

        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                search_exclude: vec!["vendor".to_string()],
                ..ToolConfig::default()
            },
        );

        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");
        assert!(output.contains("main.rs"), "src/main.rs should match");
        assert!(
            !output.contains("vendor"),
            "vendor dir should be excluded, got: {output}"
        );
    }

    #[test]
    fn write_file_denied_by_self_modify() {
        let temp = TempDir::new().expect("temp");
        let config = SelfModifyConfig {
            enabled: true,
            deny_paths: vec!["*.txt".to_string()],
            ..SelfModifyConfig::default()
        };
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_self_modify(config);
        let result = executor
            .handle_write_file(&serde_json::json!({"path": "secret.txt", "content": "data"}));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Self-modify policy violation [deny]"));
    }

    #[test]
    fn write_file_propose_tier_creates_markdown_and_sidecar() {
        let temp = TempDir::new().expect("temp");
        let proposals_dir = temp.path().join("proposals");
        let config = SelfModifyConfig {
            enabled: true,
            propose_paths: vec!["kernel/**".to_string()],
            proposals_dir: proposals_dir.clone(),
            ..SelfModifyConfig::default()
        };
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_self_modify(config);

        let message = executor
            .handle_write_file(
                &serde_json::json!({"path": "kernel/loop.rs", "content": "fn tick() {}"}),
            )
            .expect("propose tier should create proposal");
        assert!(message.contains("Proposal created"));
        assert!(message.contains("NOT modified"));
        assert!(proposals_dir.exists());

        let proposal_path = fs::read_dir(&proposals_dir)
            .expect("read proposals")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
            .expect("markdown proposal");
        let content = fs::read_to_string(&proposal_path).expect("read proposal");
        assert!(content.contains("# Proposal:"));
        assert!(content.contains("fn tick() {}"));
        assert!(content.contains("kernel/loop.rs") || content.contains("loop.rs"));
        assert!(proposal_path.with_extension("json").exists());
    }

    #[test]
    fn write_file_propose_tier_includes_original_in_proposal() {
        let temp = TempDir::new().expect("temp");
        let proposals_dir = temp.path().join("proposals");
        let config = SelfModifyConfig {
            enabled: true,
            propose_paths: vec!["kernel/**".to_string()],
            proposals_dir: proposals_dir.clone(),
            ..SelfModifyConfig::default()
        };
        let kernel_dir = temp.path().join("kernel");
        fs::create_dir_all(&kernel_dir).expect("create kernel dir");
        fs::write(kernel_dir.join("loop.rs"), "fn old() {}").expect("write original");
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_self_modify(config);
        executor
            .handle_write_file(
                &serde_json::json!({"path": "kernel/loop.rs", "content": "fn new() {}"}),
            )
            .expect("propose should succeed");
        let proposal_path = fs::read_dir(&proposals_dir)
            .expect("read proposals")
            .next()
            .expect("entry")
            .expect("entry read")
            .path();
        let proposal = fs::read_to_string(proposal_path).expect("read proposal");
        assert!(
            proposal.contains("fn old() {}"),
            "missing original: {proposal}"
        );
        assert!(
            proposal.contains("fn new() {}"),
            "missing proposed: {proposal}"
        );
        assert!(
            proposal.contains("original"),
            "missing original label: {proposal}"
        );
        assert!(
            proposal.contains("proposed"),
            "missing proposed label: {proposal}"
        );
    }

    #[test]
    fn write_file_propose_tier_records_target_hash_in_sidecar() {
        let temp = TempDir::new().expect("temp");
        let proposals_dir = temp.path().join("proposals");
        let config = SelfModifyConfig {
            enabled: true,
            propose_paths: vec!["kernel/**".to_string()],
            proposals_dir: proposals_dir.clone(),
            ..SelfModifyConfig::default()
        };
        let kernel_dir = temp.path().join("kernel");
        let target = kernel_dir.join("loop.rs");
        fs::create_dir_all(&kernel_dir).expect("create kernel dir");
        fs::write(&target, "fn old() {}\n").expect("write original");
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_self_modify(config);

        executor
            .handle_write_file(
                &serde_json::json!({"path": "kernel/loop.rs", "content": "fn new() {}"}),
            )
            .expect("propose should succeed");

        let sidecar_path = fs::read_dir(&proposals_dir)
            .expect("read proposals")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .expect("sidecar proposal");
        let sidecar = fs::read_to_string(sidecar_path).expect("read sidecar");
        let value: serde_json::Value = serde_json::from_str(&sidecar).expect("parse sidecar");

        assert_eq!(
            value["file_hash_at_creation"],
            serde_json::Value::String(format!(
                "sha256:{}",
                fx_propose::sha256_hex(b"fn old() {}\n")
            ))
        );
    }

    #[test]
    fn write_file_propose_tier_does_not_modify_target() {
        let temp = TempDir::new().expect("temp");
        let proposals_dir = temp.path().join("proposals");
        let config = SelfModifyConfig {
            enabled: true,
            propose_paths: vec!["kernel/**".to_string()],
            proposals_dir,
            ..SelfModifyConfig::default()
        };

        let kernel_dir = temp.path().join("kernel");
        fs::create_dir_all(&kernel_dir).expect("create kernel dir");
        fs::write(kernel_dir.join("loop.rs"), "original content").expect("write original");

        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_self_modify(config);
        let _result = executor
            .handle_write_file(
                &serde_json::json!({"path": "kernel/loop.rs", "content": "new content"}),
            )
            .expect("propose should succeed");

        let actual = fs::read_to_string(kernel_dir.join("loop.rs")).expect("read target");
        assert_eq!(
            actual, "original content",
            "target file should NOT be modified"
        );
    }

    #[test]
    fn write_file_allowed_by_self_modify() {
        let temp = TempDir::new().expect("temp");
        let config = SelfModifyConfig {
            enabled: true,
            allow_paths: vec!["*.rs".to_string()],
            ..SelfModifyConfig::default()
        };
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_self_modify(config);
        let result = executor
            .handle_write_file(&serde_json::json!({"path": "main.rs", "content": "fn main() {}"}));
        assert!(result.is_ok(), "expected Ok but got: {:?}", result);
    }

    #[test]
    fn write_file_no_enforcement_when_disabled() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let result = executor
            .handle_write_file(&serde_json::json!({"path": "secret.key", "content": "data"}));
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn concurrent_single_tool_call_works() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "current_time".to_string(),
            arguments: serde_json::json!({}),
        }];
        let results = executor.execute_tools(&calls, None).await.expect("results");
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
    }

    #[tokio::test]
    async fn concurrent_multiple_tools_return_in_order() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "aaa").expect("write");
        fs::write(temp.path().join("b.txt"), "bbb").expect("write");
        let executor = test_executor(temp.path());
        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "current_time".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "3".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "b.txt"}),
            },
        ];
        let results = executor.execute_tools(&calls, None).await.expect("results");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].tool_call_id, "1");
        assert_eq!(results[0].output, "aaa");
        assert_eq!(results[1].tool_call_id, "2");
        assert!(results[1].output.contains("epoch:"));
        assert_eq!(results[2].tool_call_id, "3");
        assert_eq!(results[2].output, "bbb");
    }

    #[tokio::test]
    async fn concurrent_cancellation_stops_pending() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let token = CancellationToken::new();
        token.cancel();
        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "current_time".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "current_time".to_string(),
                arguments: serde_json::json!({}),
            },
        ];
        let results = executor
            .execute_tools(&calls, Some(&token))
            .await
            .expect("results");
        assert_eq!(results.len(), calls.len());
        for result in &results {
            assert!(!result.success);
            assert!(result.output.contains("cancelled"));
        }
    }

    #[tokio::test]
    async fn concurrent_cancellation_after_wait_returns_cancelled_result() {
        use fx_kernel::act::ConcurrencyPolicy;

        let temp = TempDir::new().expect("temp");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                jail_to_working_dir: false,
                ..ToolConfig::default()
            },
        )
        .with_concurrency_policy(ConcurrencyPolicy {
            max_parallel: Some(NonZeroUsize::new(1).expect("non-zero")),
            timeout_per_call: None,
        });

        let token = CancellationToken::new();
        let cancel_clone = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cancel_clone.cancel();
        });

        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": "sleep 1"}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "current_time".to_string(),
                arguments: serde_json::json!({}),
            },
        ];

        let results = executor
            .execute_tools(&calls, Some(&token))
            .await
            .expect("results");

        assert_eq!(results.len(), calls.len());
        assert_eq!(results[1].tool_call_id, "2");
        assert!(!results[1].success);
        assert!(results[1].output.contains("cancelled"));
    }

    #[tokio::test]
    async fn concurrent_max_parallel_one_is_sequential() {
        use fx_kernel::act::ConcurrencyPolicy;

        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "aaa").expect("write");
        fs::write(temp.path().join("b.txt"), "bbb").expect("write");
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_concurrency_policy(ConcurrencyPolicy {
                max_parallel: Some(NonZeroUsize::new(1).expect("non-zero")),
                timeout_per_call: None,
            });
        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "b.txt"}),
            },
        ];
        let results = executor.execute_tools(&calls, None).await.expect("results");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tool_call_id, "1");
        assert_eq!(results[0].output, "aaa");
        assert_eq!(results[1].tool_call_id, "2");
        assert_eq!(results[1].output, "bbb");
    }

    #[tokio::test]
    async fn concurrent_timeout_returns_error() {
        use fx_kernel::act::ConcurrencyPolicy;

        let temp = TempDir::new().expect("temp");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                jail_to_working_dir: false,
                ..ToolConfig::default()
            },
        )
        .with_concurrency_policy(ConcurrencyPolicy {
            max_parallel: None,
            timeout_per_call: Some(Duration::from_millis(50)),
        });
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": "sleep 5"}),
        }];
        let results = executor.execute_tools(&calls, None).await.expect("results");
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].output.contains("timed out"));
    }

    #[test]
    fn expand_tilde_with_home() {
        let result = expand_tilde("~/foo");
        let home = dirs::home_dir().expect("home dir should exist in test env");
        assert_eq!(result, home.join("foo"));
    }

    #[test]
    fn expand_tilde_bare() {
        let result = expand_tilde("~");
        let home = dirs::home_dir().expect("home dir should exist in test env");
        assert_eq!(result, home);
    }

    #[test]
    fn expand_tilde_no_tilde() {
        let result = expand_tilde("/absolute/path");
        assert_eq!(result, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn expand_tilde_relative() {
        let result = expand_tilde("relative/path");
        assert_eq!(result, PathBuf::from("relative/path"));
    }

    #[test]
    fn expand_tilde_other_user_not_expanded() {
        let result = expand_tilde("~otheruser/foo");
        assert_eq!(result, PathBuf::from("~otheruser/foo"));
    }

    #[test]
    fn tilde_expansion_respects_jail() {
        let jail = TempDir::new().expect("jail");
        let executor = test_executor(jail.path());

        // ~/something expands to $HOME/something which is outside the jail
        let result = executor.handle_read_file(&serde_json::json!({"path": "~/something"}));
        assert!(
            result.is_err(),
            "tilde-expanded path outside jail should be rejected"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("escapes working directory") || err.contains("No such file"),
            "expected jail escape error, got: {err}"
        );
    }

    // ── config_get tests ────────────────────────────────────────────────

    fn executor_with_config(dir: &Path) -> FawxToolExecutor {
        let mgr = fx_config::manager::ConfigManager::new(dir).expect("create config manager");
        test_executor(dir).with_config_manager(Arc::new(Mutex::new(mgr)))
    }

    #[test]
    fn config_get_returns_all_sections() {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(
            temp.path().join("config.toml"),
            "[model]\ndefault_model = \"test-model\"\n",
        )
        .unwrap();
        let exec = executor_with_config(temp.path());
        let result = exec
            .handle_config_get(&serde_json::json!({}))
            .expect("config_get all");
        let json: serde_json::Value = serde_json::from_str(&result).expect("parse json");
        assert!(json.get("model").is_some());
        assert!(json.get("general").is_some());
    }

    #[test]
    fn config_get_returns_single_section() {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(
            temp.path().join("config.toml"),
            "[model]\ndefault_model = \"my-model\"\n",
        )
        .unwrap();
        let exec = executor_with_config(temp.path());
        let result = exec
            .handle_config_get(&serde_json::json!({"section": "model"}))
            .expect("config_get model");
        let json: serde_json::Value = serde_json::from_str(&result).expect("parse json");
        assert_eq!(json["default_model"], "my-model");
    }

    #[test]
    fn config_get_rejects_unknown_section() {
        let temp = TempDir::new().expect("tempdir");
        let exec = executor_with_config(temp.path());
        let err = exec
            .handle_config_get(&serde_json::json!({"section": "nonexistent"}))
            .expect_err("should fail");
        assert!(err.contains("unknown config key or section"));
    }

    #[test]
    fn config_get_returns_nested_key() {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(
            temp.path().join("config.toml"),
            "[model]\ndefault_model = \"my-model\"\n",
        )
        .unwrap();
        let exec = executor_with_config(temp.path());
        let result = exec
            .handle_config_get(&serde_json::json!({"section": "model.default_model"}))
            .expect("config_get key");
        let json: serde_json::Value = serde_json::from_str(&result).expect("parse json");
        assert_eq!(json, "my-model");
    }

    // ── config_set tests ────────────────────────────────────────────────

    #[test]
    fn config_set_updates_and_persists() {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(
            temp.path().join("config.toml"),
            "[model]\ndefault_model = \"old\"\n",
        )
        .unwrap();
        let exec = executor_with_config(temp.path());
        let result = exec
            .handle_config_set(&serde_json::json!({"key": "model.default_model", "value": "new"}));
        assert!(result.is_ok());
        // Verify persisted
        let content = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        assert!(content.contains("new"));
    }

    #[test]
    fn config_set_rejects_immutable_field() {
        let temp = TempDir::new().expect("tempdir");
        let exec = executor_with_config(temp.path());
        let err = exec
            .handle_config_set(&serde_json::json!({"key": "general.data_dir", "value": "/tmp"}))
            .expect_err("should reject immutable");
        assert!(err.contains("immutable"));
    }

    #[test]
    fn config_set_preserves_numeric_type() {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(
            temp.path().join("config.toml"),
            "[general]\nmax_iterations = 10\n",
        )
        .unwrap();
        let exec = executor_with_config(temp.path());
        exec.handle_config_set(
            &serde_json::json!({"key": "general.max_iterations", "value": "20"}),
        )
        .expect("set iterations");
        let content = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        // Should be unquoted integer, not "20"
        assert!(
            content.contains("max_iterations = 20"),
            "expected unquoted integer in: {content}"
        );
    }

    // ── fawx_status tests ───────────────────────────────────────────────

    #[test]
    fn fawx_status_returns_json_with_required_fields() {
        let temp = TempDir::new().expect("tempdir");
        let exec = test_executor(temp.path()).with_runtime_info(sample_runtime_info("test-model"));
        let result = exec.handle_fawx_status().expect("status");
        let json: serde_json::Value = serde_json::from_str(&result).expect("parse json");
        assert_eq!(json["status"], "running");
        assert!(json["uptime_seconds"].is_number());
        assert_eq!(json["model"], "test-model");
        assert!(json.get("memory_entries").is_some());
        assert!(json.get("skills_loaded").is_some());
        assert!(json.get("sessions").is_some());
    }

    #[test]
    fn fawx_status_skills_loaded_matches_runtime_info() {
        let temp = TempDir::new().expect("tempdir");
        let exec = test_executor(temp.path()).with_runtime_info(sample_runtime_info("m"));
        let result = exec.handle_fawx_status().expect("status");
        let json: serde_json::Value = serde_json::from_str(&result).expect("parse json");
        // sample_runtime_info has 1 skill
        assert_eq!(json["skills_loaded"], 1);
    }

    // ── kernel_manifest tests ─────────────────────────────────────────

    #[test]
    fn kernel_manifest_returns_valid_json() {
        let temp = TempDir::new().expect("tempdir");
        let config = fx_config::FawxConfig::default();
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, "").expect("write config");
        let manager = fx_config::manager::ConfigManager::from_config(config, config_path);
        let exec = test_executor(temp.path())
            .with_runtime_info(sample_runtime_info("gpt-5.4"))
            .with_config_manager(Arc::new(Mutex::new(manager)));
        let result = exec.handle_kernel_manifest().expect("manifest");
        let json: serde_json::Value = serde_json::from_str(&result).expect("parse json");
        assert_eq!(json["model"]["active_model"], "gpt-5.4");
        assert!(json["permissions"]["mode"].is_string());
        assert!(json["budget"]["max_llm_calls"].is_number());
        assert!(json.get("tripwire").is_none(), "must not expose tripwires");
        assert!(json.get("ripcord").is_none(), "must not expose ripcord");
    }

    #[test]
    fn kernel_manifest_fails_without_runtime_info() {
        let temp = TempDir::new().expect("tempdir");
        let exec = test_executor(temp.path());
        let error = exec.handle_kernel_manifest().expect_err("should fail");
        assert!(error.contains("runtime info"));
    }

    #[test]
    fn kernel_manifest_fails_without_config_manager() {
        let temp = TempDir::new().expect("tempdir");
        let exec = test_executor(temp.path()).with_runtime_info(sample_runtime_info("m"));
        let error = exec.handle_kernel_manifest().expect_err("should fail");
        assert!(error.contains("config manager"));
    }

    // ── fawx_restart tests ──────────────────────────────────────────────

    #[test]
    fn fawx_restart_clamps_large_delay() {
        let temp = TempDir::new().expect("tempdir");
        let exec = test_executor(temp.path());
        // Reset the global guard for test isolation
        RESTART_IN_PROGRESS.store(false, std::sync::atomic::Ordering::SeqCst);
        let result = exec
            .handle_fawx_restart(&serde_json::json!({"delay_seconds": 999}))
            .expect("restart");
        assert!(result.contains("30s"), "should clamp to 30s, got: {result}");
        // Reset for other tests
        RESTART_IN_PROGRESS.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    #[test]
    fn fawx_restart_rejects_concurrent() {
        let temp = TempDir::new().expect("tempdir");
        let exec = test_executor(temp.path());
        RESTART_IN_PROGRESS.store(true, std::sync::atomic::Ordering::SeqCst);
        let err = exec
            .handle_fawx_restart(&serde_json::json!({}))
            .expect_err("should reject concurrent");
        assert!(err.contains("already in progress"));
        RESTART_IN_PROGRESS.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}
