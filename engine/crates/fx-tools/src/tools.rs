use crate::experiment_tool::{
    handle_run_experiment, spawn_background_experiment, ExperimentRegistrar, ExperimentToolState,
};
pub use crate::tool_trait::ToolConfig;
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_config::manager::ConfigManager;
use fx_consensus::ProgressCallback;
use fx_core::kernel_manifest::BudgetSummary;
use fx_core::memory::MemoryStore;
use fx_core::runtime_info::RuntimeInfo;
use fx_core::self_modify::SelfModifyConfig;
use fx_kernel::act::{
    cancelled_result, is_cancelled, timed_out_result, ConcurrencyPolicy, JournalAction,
    ToolCacheability, ToolCallClassification, ToolExecutor, ToolExecutorError, ToolResult,
};
use fx_kernel::budget::BudgetConfig as KernelBudgetConfig;
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::ToolAuthoritySurface;
use fx_kernel::{ProcessConfig, ProcessRegistry};
use fx_llm::{ToolCall, ToolDefinition};
use fx_memory::embedding_index::EmbeddingIndex;
use fx_subagent::SubagentControl;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

mod config;
mod experiment;
mod filesystem;
#[cfg(feature = "improvement")]
mod improvement;
mod memory;
mod node;
mod process;
mod runtime;
mod shell;
mod subagent;

#[cfg(test)]
use self::filesystem::{is_builtin_ignored_directory, MAX_SEARCH_MATCHES};
#[cfg(test)]
use self::runtime::{day_of_week_from_epoch, iso8601_utc_from_epoch};

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

type ToolRef = Arc<dyn Tool>;

#[derive(Clone, Default)]
struct ToolRegistry {
    ordered: Vec<ToolRef>,
    by_name: HashMap<String, ToolRef>,
}

impl ToolRegistry {
    fn register<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        let tool: ToolRef = Arc::new(tool);
        let name = tool.name().to_string();
        match self.by_name.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(_) => {
                tracing::error!(tool = %name, "duplicate tool registration");
                debug_assert!(false, "duplicate tool registration: {name}");
                return;
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Arc::clone(&tool));
            }
        }
        self.ordered.push(tool);
    }

    fn get(&self, name: &str) -> Option<ToolRef> {
        self.by_name.get(name).cloned()
    }

    fn authority_surface(&self, call: &ToolCall) -> ToolAuthoritySurface {
        self.get(call.name.as_str())
            .map_or(ToolAuthoritySurface::Other, |tool| {
                tool.authority_surface(call)
            })
    }

    fn definitions(&self) -> Vec<ToolDefinition> {
        self.ordered
            .iter()
            .filter(|tool| tool.is_available())
            .map(|tool| tool.definition())
            .collect()
    }
}

#[derive(Clone)]
pub struct FawxToolExecutor {
    context: Arc<ToolContext>,
    tools: Arc<ToolRegistry>,
    concurrency_policy: ConcurrencyPolicy,
}

impl FawxToolExecutor {
    pub fn new(working_dir: PathBuf, config: ToolConfig) -> Self {
        let context = Arc::new(ToolContext {
            process_registry: default_process_registry(&working_dir),
            working_dir,
            config,
            memory: None,
            embedding_index: None,
            runtime_info: None,
            self_modify: None,
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
        });
        Self {
            tools: Arc::new(build_registry(&context)),
            context,
            concurrency_policy: ConcurrencyPolicy::default(),
        }
    }

    /// Set the concurrency policy for parallel tool execution.
    #[must_use]
    pub fn with_concurrency_policy(mut self, policy: ConcurrencyPolicy) -> Self {
        self.concurrency_policy = policy;
        self
    }

    fn update_context(&mut self, update: impl FnOnce(&mut ToolContext)) {
        update(Arc::make_mut(&mut self.context));
        self.rebuild_tools();
    }

    fn rebuild_tools(&mut self) {
        self.tools = Arc::new(build_registry(&self.context));
    }

    /// Attach a persistent memory provider.
    pub fn with_memory(mut self, memory: Arc<Mutex<dyn MemoryStore>>) -> Self {
        self.update_context(|context| context.memory = Some(memory));
        self
    }

    /// Attach a semantic embedding index for memory search.
    pub fn with_embedding_index(mut self, index: Arc<Mutex<EmbeddingIndex>>) -> Self {
        self.update_context(|context| context.embedding_index = Some(index));
        self
    }

    /// Attach runtime self-introspection state.
    pub fn with_runtime_info(mut self, info: Arc<RwLock<RuntimeInfo>>) -> Self {
        self.update_context(|context| context.runtime_info = Some(info));
        self
    }

    /// Attach a self-modification path enforcement config.
    pub fn with_self_modify(mut self, config: SelfModifyConfig) -> Self {
        self.update_context(|context| context.self_modify = Some(config));
        self
    }

    /// Attach a config manager for runtime config read/write tools.
    pub fn with_config_manager(mut self, mgr: Arc<Mutex<ConfigManager>>) -> Self {
        self.update_context(|context| context.config_manager = Some(mgr));
        self
    }

    #[must_use]
    pub fn with_protected_branches(mut self, protected_branches: Vec<String>) -> Self {
        self.update_context(|context| context.protected_branches = protected_branches);
        self
    }

    /// Attach the active kernel budget configuration.
    pub fn with_kernel_budget(mut self, budget: KernelBudgetConfig) -> Self {
        self.update_context(|context| context.kernel_budget = budget);
        self
    }

    /// Attach subagent lifecycle tools (spawn_agent, subagent_status).
    pub fn with_subagent_control(mut self, control: Arc<dyn SubagentControl>) -> Self {
        self.update_context(|context| context.subagent_control = Some(control));
        self
    }

    /// Attach experiment execution state for run_experiment.
    pub fn with_experiment(mut self, state: ExperimentToolState) -> Self {
        self.update_context(|context| context.experiment = Some(state));
        self
    }

    /// Attach an experiment progress callback for run_experiment.
    pub fn with_experiment_progress(mut self, progress: ProgressCallback) -> Self {
        self.update_context(|context| context.experiment_progress = Some(progress));
        self
    }

    /// Attach an experiment registry bridge for background run_experiment calls.
    pub fn with_experiment_registrar(mut self, registrar: Arc<dyn ExperimentRegistrar>) -> Self {
        self.update_context(|context| context.experiment_registrar = Some(registrar));
        self
    }

    /// Toggle spawn-and-return behavior for run_experiment.
    #[must_use]
    pub fn with_background_experiments(mut self, background: bool) -> Self {
        self.update_context(|context| context.background_experiments = background);
        self
    }

    pub fn set_experiment(&mut self, state: ExperimentToolState) {
        self.update_context(|context| context.experiment = Some(state));
    }

    /// Attach node_run tool state for remote command execution.
    pub fn with_node_run(mut self, state: crate::node_run::NodeRunState) -> Self {
        self.update_context(|context| context.node_run = Some(state));
        self
    }

    /// Attach a background process registry shared with the engine lifecycle.
    pub fn with_process_registry(mut self, registry: Arc<ProcessRegistry>) -> Self {
        self.update_context(|context| context.process_registry = registry);
        self
    }

    /// Attach self-improvement tools (analyze_signals, propose_improvement).
    #[cfg(feature = "improvement")]
    pub fn with_improvement(
        mut self,
        state: crate::improvement_tools::ImprovementToolsState,
    ) -> Self {
        self.update_context(|context| context.improvement = Some(state));
        self
    }

    pub(crate) async fn execute_call(
        &self,
        call: &ToolCall,
        cancel: Option<&CancellationToken>,
    ) -> ToolResult {
        if is_cancelled(cancel) {
            return cancelled_result(&call.id, &call.name);
        }
        match self.tools.get(call.name.as_str()) {
            Some(tool) => tool.execute(call, cancel).await,
            None => to_tool_result(
                &call.id,
                &call.name,
                Err(format!("unknown tool: {}", call.name)),
            ),
        }
    }
}

#[cfg(test)]
impl FawxToolExecutor {
    fn handle_read_file(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_read_file(args)
    }

    fn handle_write_file(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_write_file(args)
    }

    fn handle_edit_file(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_edit_file(args)
    }

    fn handle_list_directory(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_list_directory(args)
    }

    async fn handle_run_command(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_run_command(args).await
    }

    fn handle_exec_background(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_exec_background(args)
    }

    fn handle_exec_status(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_exec_status(args)
    }

    async fn handle_exec_kill(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_exec_kill(args).await
    }

    fn handle_search_text(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_search_text(args)
    }

    fn handle_current_time(&self) -> Result<String, String> {
        self.context.handle_current_time()
    }

    fn handle_self_info(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_self_info(args)
    }

    fn handle_config_get(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_config_get(args)
    }

    fn handle_config_set(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_config_set(args)
    }

    fn handle_fawx_status(&self) -> Result<String, String> {
        self.context.handle_fawx_status()
    }

    fn handle_kernel_manifest(&self) -> Result<String, String> {
        self.context.handle_kernel_manifest()
    }

    fn handle_fawx_restart(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_fawx_restart(args)
    }

    fn handle_memory_write(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_memory_write(args)
    }

    fn handle_memory_read(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_memory_read(args)
    }

    fn handle_memory_list(&self) -> Result<String, String> {
        self.context.handle_memory_list()
    }

    fn handle_memory_search(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_memory_search(args)
    }

    fn handle_memory_delete(&self, args: &serde_json::Value) -> Result<String, String> {
        self.context.handle_memory_delete(args)
    }
}

impl ToolContext {
    pub(crate) async fn handle_run_experiment(
        &self,
        args: &serde_json::Value,
    ) -> Result<String, String> {
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
}

impl FawxToolExecutor {
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
        self.tools.definitions()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        self.tools
            .get(tool_name)
            .map_or(ToolCacheability::NeverCache, |tool| tool.cacheability())
    }

    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
        self.tools
            .get(call.name.as_str())
            .map_or(ToolCallClassification::Observation, |tool| {
                tool.classify_call(call)
            })
    }

    fn action_category(&self, call: &ToolCall) -> &'static str {
        self.tools
            .get(call.name.as_str())
            .map_or("unknown", |tool| tool.action_category())
    }

    fn authority_surface(&self, call: &ToolCall) -> ToolAuthoritySurface {
        self.tools.authority_surface(call)
    }

    fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
        self.tools
            .get(call.name.as_str())
            .and_then(|tool| tool.journal_action(call, result))
    }

    fn route_sub_goal_call(
        &self,
        request: &fx_kernel::act::SubGoalToolRoutingRequest,
        call_id: &str,
    ) -> Option<ToolCall> {
        let tool_name = request.required_tools.first()?;
        let tool = self.tools.get(tool_name)?;
        if !tool.is_available() {
            return None;
        }
        tool.route_sub_goal(request, call_id)
    }
}

impl std::fmt::Debug for FawxToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("FawxToolExecutor");
        debug
            .field("working_dir", &self.context.working_dir)
            .field("config", &self.context.config)
            .field("registered_tools", &self.tools.ordered.len())
            .field("process_registry", &true)
            .field("memory", &self.context.memory.is_some())
            .field("embedding_index", &self.context.embedding_index.is_some())
            .field("runtime_info", &self.context.runtime_info.is_some())
            .field("self_modify", &self.context.self_modify)
            .field("concurrency_policy", &self.concurrency_policy)
            .field("config_manager", &self.context.config_manager.is_some())
            .field("kernel_budget", &self.context.kernel_budget)
            .field("subagent_control", &self.context.subagent_control.is_some())
            .field("experiment", &self.context.experiment.is_some())
            .field(
                "experiment_progress",
                &self.context.experiment_progress.is_some(),
            )
            .field(
                "experiment_registrar",
                &self.context.experiment_registrar.is_some(),
            )
            .field(
                "background_experiments",
                &self.context.background_experiments,
            );
        #[cfg(feature = "improvement")]
        debug.field("improvement", &self.context.improvement.is_some());
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

fn build_registry(context: &Arc<ToolContext>) -> ToolRegistry {
    let mut registry = ToolRegistry::default();
    experiment::register_tools(&mut registry, context);
    filesystem::register_tools(&mut registry, context);
    shell::register_tools(&mut registry, context);
    process::register_tools(&mut registry, context);
    runtime::register_tools(&mut registry, context);
    subagent::register_tools(&mut registry, context);
    memory::register_tools(&mut registry, context);
    config::register_tools(&mut registry, context);
    node::register_tools(&mut registry, context);
    #[cfg(feature = "improvement")]
    improvement::register_tools(&mut registry, context);
    registry
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

fn parse_args<T: for<'de> Deserialize<'de>>(value: &serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

/// Shared request type for config set — used by both the tool handler and
/// the HTTP endpoint (re-exported for fx-cli).
#[derive(Deserialize)]
pub struct ConfigSetRequest {
    pub key: String,
    pub value: String,
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

    fn executor_with_tool<T>(root: &Path, tool: T) -> FawxToolExecutor
    where
        T: Tool + 'static,
    {
        let mut executor = test_executor(root);
        let mut registry = ToolRegistry::default();
        registry.register(tool);
        executor.tools = Arc::new(registry);
        executor
    }

    fn memory_executor(root: &Path) -> (FawxToolExecutor, Arc<Mutex<fx_memory::JsonFileMemory>>) {
        let memory = Arc::new(Mutex::new(
            fx_memory::JsonFileMemory::new(root).expect("memory"),
        ));
        let executor = test_executor(root).with_memory(memory.clone());
        (executor, memory)
    }

    struct MetadataOnlyTool;

    #[async_trait::async_trait]
    impl Tool for MetadataOnlyTool {
        fn name(&self) -> &'static str {
            "metadata_probe"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name().to_string(),
                description: "probe metadata ownership".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "repo": { "type": "string" },
                        "branch": { "type": "string" }
                    },
                    "required": ["repo", "branch"]
                }),
            }
        }

        async fn execute(
            &self,
            call: &ToolCall,
            _cancel: Option<&CancellationToken>,
        ) -> ToolResult {
            ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
            }
        }

        fn journal_action(&self, call: &ToolCall, _result: &ToolResult) -> Option<JournalAction> {
            let repo = call.arguments.get("repo")?.as_str()?;
            let branch = call.arguments.get("branch")?.as_str()?;
            Some(JournalAction::GitBranchCreate {
                repo: PathBuf::from(repo),
                branch: branch.to_string(),
            })
        }

        fn action_category(&self) -> &'static str {
            "metadata_owned"
        }
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
                version: None,
                source: None,
                revision_hash: None,
                manifest_hash: None,
                activated_at_ms: None,
                signature_status: None,
                stale_source: None,
            }],
            config_summary: fx_core::runtime_info::ConfigSummary {
                max_iterations: 6,
                max_history: 128,
                memory_enabled: true,
            },
            authority: None,
            version: "0.1.0".to_string(),
        }))
    }

    fn sample_runtime_info_with_authority(model: &str) -> Arc<RwLock<RuntimeInfo>> {
        let runtime = sample_runtime_info(model);
        runtime.write().expect("runtime lock").authority =
            Some(fx_core::runtime_info::AuthorityRuntimeInfo {
                resolver: "unified".to_string(),
                approval_scope: "classified_request_identity".to_string(),
                path_policy_source: "self_modify_config".to_string(),
                capability_mode_mutates_path_policy: false,
                kernel_blind_enabled: true,
                sovereign_boundary_enforced: true,
                active_session_approvals: 1,
                active_proposal_override: Some("proposal-1".to_string()),
                recent_decisions: vec![fx_core::runtime_info::AuthorityDecisionInfo {
                    tool_name: "write_file".to_string(),
                    capability: "file_write".to_string(),
                    effect: "write".to_string(),
                    target_kind: "path".to_string(),
                    domain: "project".to_string(),
                    target_summary: "README.md".to_string(),
                    verdict: "prompt".to_string(),
                    reason: "approval required by permission policy".to_string(),
                }],
            });
        runtime
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

    fn tool_definitions(
        include_subagent_tools: bool,
        include_experiment_tool: bool,
    ) -> Vec<ToolDefinition> {
        let temp = TempDir::new().expect("temp");
        let mut executor = test_executor(temp.path());
        if include_experiment_tool {
            executor = executor.with_experiment(experiment_state(temp.path()));
        }
        if include_subagent_tools {
            executor = executor.with_subagent_control(Arc::new(StubSubagentControl::new()));
        }
        executor.tool_definitions()
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

    #[test]
    fn read_file_allows_absolute_outside_workspace_when_enabled() {
        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret").expect("write");
        let executor = FawxToolExecutor::new(
            jail.path().to_path_buf(),
            ToolConfig {
                allow_outside_workspace_reads: true,
                ..ToolConfig::default()
            },
        );

        let output = executor.handle_read_file(&serde_json::json!({
            "path": outside_file.to_string_lossy()
        }));

        assert_eq!(output.expect("read"), "secret");
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
    fn write_file_respects_jail_even_when_outside_workspace_reads_enabled() {
        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let target = outside.path().join("x.txt");
        let executor = FawxToolExecutor::new(
            jail.path().to_path_buf(),
            ToolConfig {
                allow_outside_workspace_reads: true,
                ..ToolConfig::default()
            },
        );

        let result = executor.handle_write_file(&serde_json::json!({
            "path": target.to_string_lossy(),
            "content": "no"
        }));

        assert!(matches!(
            result,
            Err(message) if message.contains("path escapes working directory")
        ));
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
    fn edit_file_does_not_self_enforce_authority_policy() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("secret.txt"), "alpha").expect("write");
        let config = SelfModifyConfig {
            enabled: true,
            deny_paths: vec!["*.txt".to_string()],
            ..SelfModifyConfig::default()
        };
        let executor = FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default())
            .with_self_modify(config);

        let result = executor
            .handle_edit_file(&serde_json::json!({
                "path": "secret.txt",
                "old_text": "alpha",
                "new_text": "beta"
            }))
            .expect("edit should succeed");
        assert!(result.contains("Successfully edited"));
        assert_eq!(
            fs::read_to_string(temp.path().join("secret.txt")).expect("read"),
            "beta"
        );
    }

    #[test]
    fn edit_file_does_not_create_proposal_without_kernel_gate() {
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
            .expect("edit should succeed");
        assert!(message.contains("Successfully edited"));
        assert_eq!(
            fs::read_to_string(kernel_dir.join("loop.rs")).expect("read"),
            "fn new() {}
"
        );
        assert!(!proposals_dir.exists());
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
    fn list_directory_allows_absolute_outside_workspace_when_enabled() {
        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let outside_dir = outside.path().join("secret-dir");
        fs::create_dir_all(&outside_dir).expect("mkdir");
        fs::write(outside_dir.join("secret.txt"), "secret").expect("write");
        let executor = FawxToolExecutor::new(
            jail.path().to_path_buf(),
            ToolConfig {
                allow_outside_workspace_reads: true,
                ..ToolConfig::default()
            },
        );

        let output = executor
            .handle_list_directory(&serde_json::json!({
                "path": outside_dir.to_string_lossy()
            }))
            .expect("list");

        assert!(output.contains("[file] secret.txt"));
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
        let without_experiment = tool_definitions(false, false);
        assert!(!without_experiment
            .iter()
            .any(|tool| tool.name == "run_experiment"));

        let with_experiment = tool_definitions(false, true);
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
        let definitions = tool_definitions(false, false);
        assert!(definitions.iter().any(|tool| tool.name == "current_time"));
    }

    #[test]
    fn edit_file_appears_in_definitions() {
        let definitions = tool_definitions(false, false);
        assert!(definitions.iter().any(|tool| tool.name == "edit_file"));
    }

    #[test]
    fn background_process_tools_appear_in_definitions() {
        let definitions = tool_definitions(false, false);
        assert!(definitions
            .iter()
            .any(|tool| tool.name == "exec_background"));
        assert!(definitions.iter().any(|tool| tool.name == "exec_status"));
        assert!(definitions.iter().any(|tool| tool.name == "exec_kill"));
    }

    #[test]
    fn read_file_definition_exposes_offset_and_limit() {
        let definitions = tool_definitions(false, false);
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
    fn action_category_uses_registered_tool_metadata() {
        let temp = TempDir::new().expect("temp");
        let executor = memory_executor(temp.path()).0;
        let call = ToolCall {
            id: "1".to_string(),
            name: "memory_search".to_string(),
            arguments: serde_json::json!({"query": "preferences"}),
        };

        assert_eq!(executor.action_category(&call), "tool_call");
    }

    #[test]
    fn action_category_uses_custom_tool_metadata() {
        let temp = TempDir::new().expect("temp");
        let executor = executor_with_tool(temp.path(), MetadataOnlyTool);
        let call = ToolCall {
            id: "1".to_string(),
            name: "metadata_probe".to_string(),
            arguments: serde_json::json!({
                "repo": ".",
                "branch": "feature/custom-metadata"
            }),
        };

        assert_eq!(executor.action_category(&call), "metadata_owned");
    }

    #[test]
    fn journal_action_uses_registered_tool_metadata() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({
                "path": "notes.txt",
                "content": "hello"
            }),
        };
        let result = ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: true,
            output: "ok".to_string(),
        };

        let action = executor
            .journal_action(&call, &result)
            .expect("file write action");

        assert!(matches!(
            action,
            JournalAction::FileWrite {
                path,
                size_bytes: 5,
                created: false,
                ..
            } if path == Path::new("notes.txt")
        ));
    }

    #[test]
    fn journal_action_uses_custom_tool_metadata() {
        let temp = TempDir::new().expect("temp");
        let executor = executor_with_tool(temp.path(), MetadataOnlyTool);
        let call = ToolCall {
            id: "1".to_string(),
            name: "metadata_probe".to_string(),
            arguments: serde_json::json!({
                "repo": ".",
                "branch": "feature/custom-metadata"
            }),
        };
        let result = ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: true,
            output: "ok".to_string(),
        };

        let action = executor
            .journal_action(&call, &result)
            .expect("metadata-owned journal action");

        assert!(matches!(
            action,
            JournalAction::GitBranchCreate { repo, branch }
            if repo == Path::new(".") && branch == "feature/custom-metadata"
        ));
    }

    #[test]
    fn classify_call_treats_read_only_run_command_as_observation() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "grep -rn \"kv_get\" ./skills | head -20",
                "shell": true,
            }),
        };

        assert_eq!(
            executor.classify_call(&call),
            ToolCallClassification::Observation
        );
    }

    #[test]
    fn classify_call_treats_mutating_run_command_as_mutation() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "cd ~/fawx && cargo run -- skill create x-post",
                "shell": true,
            }),
        };

        assert_eq!(
            executor.classify_call(&call),
            ToolCallClassification::Mutation
        );
    }

    #[test]
    fn classify_call_treats_redirected_echo_as_mutation() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "echo hello > notes.txt",
                "shell": true,
            }),
        };

        assert_eq!(
            executor.classify_call(&call),
            ToolCallClassification::Mutation
        );
    }

    #[test]
    fn classify_call_treats_quoted_grep_gt_pattern_as_observation() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "grep \"error > warning\" log.txt",
                "shell": true,
            }),
        };

        assert_eq!(
            executor.classify_call(&call),
            ToolCallClassification::Observation
        );
    }

    #[test]
    fn classify_call_treats_quoted_jq_comparison_as_observation() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "jq '.items[] | select(.value > 5)' report.json",
                "shell": true,
            }),
        };

        assert_eq!(
            executor.classify_call(&call),
            ToolCallClassification::Observation
        );
    }

    #[test]
    fn classify_call_treats_quoted_awk_comparison_as_observation() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "awk '$1 > 100' metrics.txt",
                "shell": true,
            }),
        };

        assert_eq!(
            executor.classify_call(&call),
            ToolCallClassification::Observation
        );
    }

    #[test]
    fn classify_call_treats_ps_as_observation() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "ps aux",
                "shell": true,
            }),
        };

        assert_eq!(
            executor.classify_call(&call),
            ToolCallClassification::Observation
        );
    }

    #[test]
    fn classify_call_treats_noninteractive_top_as_observation() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let call = ToolCall {
            id: "1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "top -l 1",
                "shell": true,
            }),
        };

        assert_eq!(
            executor.classify_call(&call),
            ToolCallClassification::Observation
        );
    }

    #[test]
    fn route_sub_goal_call_rejects_tools_with_required_arguments() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let request = fx_kernel::act::SubGoalToolRoutingRequest {
            description: "Scaffold the skill".to_string(),
            required_tools: vec!["run_command".to_string()],
        };

        assert!(
            executor
                .route_sub_goal_call(&request, "decompose-gate-0")
                .is_none(),
            "run_command should not be direct-routed without a declared materializer"
        );
    }

    #[test]
    fn route_sub_goal_call_allows_zero_argument_tools() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let request = fx_kernel::act::SubGoalToolRoutingRequest {
            description: "Check the clock".to_string(),
            required_tools: vec!["current_time".to_string()],
        };

        let call = executor
            .route_sub_goal_call(&request, "decompose-gate-0")
            .expect("current_time should be routable");
        assert_eq!(call.name, "current_time");
        assert_eq!(call.arguments, serde_json::json!({}));
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
        let definition = tool_definitions(true, false)
            .into_iter()
            .find(|tool| tool.name == "spawn_agent")
            .expect("spawn_agent definition");
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
        let definitions = tool_definitions(false, false);
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
    fn write_file_does_not_self_enforce_authority_policy() {
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
        assert!(result.is_ok());
        assert_eq!(
            fs::read_to_string(temp.path().join("secret.txt")).expect("read"),
            "data"
        );
    }

    #[test]
    fn write_file_does_not_create_proposal_without_kernel_gate() {
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
            .expect("write should succeed");
        assert!(message.contains("wrote"));
        assert_eq!(
            fs::read_to_string(temp.path().join("kernel/loop.rs")).expect("read"),
            "fn tick() {}"
        );
        assert!(!proposals_dir.exists());
    }

    #[test]
    fn write_file_updates_target_instead_of_writing_proposal_payload() {
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
            .expect("write should succeed");
        assert_eq!(
            fs::read_to_string(kernel_dir.join("loop.rs")).expect("read target"),
            "fn new() {}"
        );
        assert!(!proposals_dir.exists());
    }

    #[test]
    fn write_file_does_not_emit_sidecar_without_kernel_gate() {
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
            .expect("write should succeed");

        assert!(!proposals_dir.exists());
        assert_eq!(
            fs::read_to_string(&target).expect("read target"),
            "fn new() {}"
        );
    }

    #[test]
    fn write_file_updates_target_without_kernel_gate() {
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
            actual, "new content",
            "target file should be updated directly when no kernel gate is present"
        );
        assert!(!proposals_dir.exists());
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

    #[test]
    fn fawx_status_includes_authority_snapshot() {
        let temp = TempDir::new().expect("tempdir");
        let exec =
            test_executor(temp.path()).with_runtime_info(sample_runtime_info_with_authority("m"));
        let result = exec.handle_fawx_status().expect("status");
        let json: serde_json::Value = serde_json::from_str(&result).expect("parse json");

        assert_eq!(json["authority"]["resolver"], "unified");
        assert_eq!(
            json["authority"]["approval_scope"],
            "classified_request_identity"
        );
        assert_eq!(
            json["authority"]["recent_decisions"][0]["verdict"],
            "prompt"
        );
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
    fn kernel_manifest_includes_authority_snapshot() {
        let temp = TempDir::new().expect("tempdir");
        let config = FawxConfig::default();
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, "").expect("write config");
        let manager = fx_config::manager::ConfigManager::from_config(config, config_path);
        let exec = test_executor(temp.path())
            .with_runtime_info(sample_runtime_info_with_authority("gpt-5.4"))
            .with_config_manager(Arc::new(Mutex::new(manager)));
        let result = exec.handle_kernel_manifest().expect("manifest");
        let json: serde_json::Value = serde_json::from_str(&result).expect("parse json");

        assert_eq!(json["authority"]["resolver"], "unified");
        assert_eq!(
            json["authority"]["path_policy_source"],
            "self_modify_config"
        );
        assert_eq!(
            json["authority"]["recent_decisions"][0]["tool_name"],
            "write_file"
        );
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
