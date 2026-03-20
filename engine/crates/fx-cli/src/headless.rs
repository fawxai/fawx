//! Headless mode for Fawx — stdin/stdout REPL without the TUI.
//!
//! Provides `HeadlessApp` which drives the full agentic loop via
//! `LoopEngine::run_cycle()` while reading input from stdin and writing
//! responses to stdout. All diagnostic/error output goes to stderr so
//! downstream consumers can safely pipe stdout.

use async_trait::async_trait;
use fx_analysis::{AnalysisEngine, AnalysisError, AnalysisFinding, Confidence};
#[cfg(feature = "http")]
use fx_api::engine::{AppEngine, ConfigManagerHandle, CycleResult as ApiCycleResult};
#[cfg(feature = "http")]
use fx_api::{
    AuthProviderDto, ContextInfoDto, ModelInfoDto, ModelSwitchDto, SkillSummaryDto,
    ThinkingAdjustedDto, ThinkingLevelDto,
};
use fx_bus::{Envelope, SessionBus};
use fx_canary::CanaryMonitor;
use fx_config::manager::ConfigManager;
use fx_config::{FawxConfig, ThinkingBudget};
use fx_core::runtime_info::RuntimeInfo;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_improve::{CyclePaths, ImprovementConfig, OutputMode};
use fx_kernel::act::TokenUsage;
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::loop_engine::{LoopEngine, LoopResult};
use fx_kernel::signals::Signal;
use fx_kernel::types::PerceptionSnapshot;
use fx_kernel::{ErrorCategory, StreamCallback, StreamEvent};
use fx_llm::CompletionProvider;
use fx_llm::{valid_thinking_levels, ImageAttachment, Message, ModelInfo, ModelRouter};
use fx_memory::SignalStore;
use fx_session::SessionKey;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tracing_appender::non_blocking::WorkerGuard;

use crate::auth_store::AuthStore;
use crate::commands::slash::{
    apply_thinking_budget, client_only_command_message, config_reload_success_message,
    execute_command, init_default_config, is_command_input, parse_command, persist_default_model,
    reload_runtime_config, render_budget_text, render_debug_dump, render_loop_status,
    render_signals_summary, CommandContext, CommandHost, ImproveFlags, ParsedCommand,
    DEFAULT_SYNTHESIS_INSTRUCTION, MAX_SYNTHESIS_INSTRUCTION_LENGTH,
};
use crate::context::load_context_files;
use crate::helpers::{
    available_provider_names, fetch_shared_available_models, format_memory_for_prompt, read_router,
    render_model_menu_text, render_status_text, resolve_model_alias,
    thinking_config_for_active_model, trim_history, write_router, AnalysisCompletionProvider,
    RouterLoopLlmProvider, SharedModelRouter,
};
use crate::proposal_review::{approve_pending, reject_pending, render_pending, ReviewContext};
use crate::startup::{
    build_headless_loop_engine_bundle, configured_data_dir as startup_configured_data_dir,
    configured_working_dir, fawx_data_dir as startup_fawx_data_dir, HeadlessLoopBuildOptions,
    SharedMemoryStore, SharedTokenBroker,
};
use fx_subagent::{
    CreatedSubagentSession, SpawnConfig, SubagentError, SubagentFactory, SubagentLimits,
    SubagentManager, SubagentManagerDeps, SubagentSession, SubagentTurn,
};

/// Fallback model when `config.model.default_model` is `None`.
///
/// [`HeadlessApp::apply_http_defaults`] reads the configured default first;
/// this constant is only used when no config value is set (e.g. fresh install
/// without a `config.toml`). Keeping a hardcoded fallback avoids a startup
/// failure when the config file is absent.
#[cfg(feature = "http")]
const DEFAULT_HTTP_MODEL: &str = "claude-opus-4-6";

pub const MAIN_SESSION_KEY: &str = "main";
const HEADLESS_SIGNAL_SESSION_ID: &str = "headless";

pub fn main_session_key() -> SessionKey {
    match SessionKey::new(MAIN_SESSION_KEY) {
        Ok(key) => key,
        Err(_) => unreachable!("main session key constant must be valid"),
    }
}

pub fn fawx_data_dir() -> PathBuf {
    startup_fawx_data_dir()
}

pub fn configured_data_dir(base_data_dir: &Path, config: &FawxConfig) -> PathBuf {
    startup_configured_data_dir(base_data_dir, config)
}

// ── JSON I/O types ──────────────────────────────────────────────────────────

/// JSON-mode input envelope.
#[derive(serde::Deserialize)]
struct JsonInput {
    message: String,
}

/// JSON-mode output envelope.
#[derive(serde::Serialize)]
struct JsonOutput {
    response: String,
    model: String,
    iterations: u32,
}

// ── CycleResult ─────────────────────────────────────────────────────────────

/// Result of a single agentic cycle, returned by [`HeadlessApp::process_message`].

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupWarning {
    pub category: ErrorCategory,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ErrorRecord {
    pub timestamp: String,
    pub category: ErrorCategory,
    pub message: String,
    pub recoverable: bool,
}

const MAX_ERROR_HISTORY: usize = 50;
const NO_AI_PROVIDERS_STARTUP_WARNING: &str =
    "no AI providers configured; HTTP API available but chat disabled until a provider is added";

pub struct CycleResult {
    /// The assistant's response text.
    pub response: String,
    /// The model identifier used for the cycle.
    pub model: String,
    /// Number of loop iterations consumed.
    pub iterations: u32,
    /// Token usage reported for the cycle.
    pub tokens_used: TokenUsage,
}

// ── HeadlessApp ─────────────────────────────────────────────────────────────

/// Dependencies for constructing a [`HeadlessApp`]. Avoids > 5 bare params.
pub struct HeadlessAppDeps {
    pub loop_engine: LoopEngine,
    pub router: SharedModelRouter,
    pub runtime_info: Arc<RwLock<RuntimeInfo>>,
    pub config: FawxConfig,
    pub memory: Option<SharedMemoryStore>,
    pub embedding_index_persistence: Option<crate::startup::EmbeddingIndexPersistence>,
    pub system_prompt_path: Option<PathBuf>,
    pub config_manager: Option<Arc<Mutex<ConfigManager>>>,
    pub system_prompt_text: Option<String>,
    pub subagent_manager: Arc<SubagentManager>,
    pub canary_monitor: Option<CanaryMonitor>,
    pub session_bus: Option<SessionBus>,
    pub session_key: Option<SessionKey>,
    pub cron_store: Option<fx_cron::SharedCronStore>,
    pub startup_warnings: Vec<StartupWarning>,
    pub permission_callback_slot:
        Arc<std::sync::Mutex<Option<fx_kernel::streaming::StreamCallback>>>,
    pub ripcord_journal: Arc<fx_ripcord::RipcordJournal>,
    #[cfg(feature = "http")]
    pub experiment_registry: Option<fx_api::SharedExperimentRegistry>,
}

/// Headless Fawx agent: drives `LoopEngine` via stdin/stdout.
pub struct HeadlessApp {
    loop_engine: LoopEngine,
    router: SharedModelRouter,
    runtime_info: Arc<RwLock<RuntimeInfo>>,
    config: FawxConfig,
    memory: Option<SharedMemoryStore>,
    embedding_index_persistence: Option<crate::startup::EmbeddingIndexPersistence>,
    _subagent_manager: Arc<SubagentManager>,
    active_model: String,
    conversation_history: Vec<Message>,
    last_signals: Vec<Signal>,
    max_history: usize,
    custom_system_prompt: Option<String>,
    canary_monitor: Option<CanaryMonitor>,
    /// Config manager for runtime config tools. Read via `config_manager()`
    /// when the `http` feature is enabled.
    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    config_manager: Option<Arc<Mutex<ConfigManager>>>,
    session_bus: Option<SessionBus>,
    session_key: Option<SessionKey>,
    cron_store: Option<fx_cron::SharedCronStore>,
    startup_warnings: Vec<StartupWarning>,
    #[cfg(feature = "http")]
    experiment_registry: Option<fx_api::SharedExperimentRegistry>,
    error_history: VecDeque<ErrorRecord>,
    /// Cumulative token usage across all cycles in this session.
    cumulative_tokens: TokenUsage,
    /// Shared callback slot for permission prompt SSE events.
    permission_callback_slot: Arc<std::sync::Mutex<Option<fx_kernel::streaming::StreamCallback>>>,
    ripcord_journal: Arc<fx_ripcord::RipcordJournal>,
    /// Bus message receiver. Stored for Phase 2 loop integration —
    /// will be polled via `tokio::select!` alongside user input to
    /// process incoming cross-session messages during conversation.
    #[allow(dead_code)]
    bus_receiver: Option<mpsc::Receiver<Envelope>>,
}

#[derive(Clone)]
pub struct HeadlessSubagentFactoryDeps {
    pub router: SharedModelRouter,
    pub config: FawxConfig,
    pub improvement_provider: Option<Arc<dyn CompletionProvider + Send + Sync>>,
    pub session_bus: Option<SessionBus>,
    pub token_broker: Option<SharedTokenBroker>,
}

#[derive(Clone)]
pub struct HeadlessSubagentFactory {
    deps: HeadlessSubagentFactoryDeps,
    disabled_manager: Arc<SubagentManager>,
}

struct HeadlessSubagentSession {
    app: HeadlessApp,
}

#[derive(Debug)]
struct DisabledSubagentFactory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthProviderStatus {
    pub provider: String,
    pub auth_methods: BTreeSet<String>,
    pub model_count: usize,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextInfoSnapshot {
    pub used_tokens: usize,
    pub max_tokens: usize,
    pub percentage: f32,
    pub compaction_threshold: f32,
}

#[cfg(feature = "http")]
impl fx_api::ContextInfoSnapshotLike for ContextInfoSnapshot {
    fn used_tokens(&self) -> usize {
        self.used_tokens
    }

    fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    fn percentage(&self) -> f32 {
        self.percentage
    }

    fn compaction_threshold(&self) -> f32 {
        self.compaction_threshold
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrustedKeyEntry {
    file_name: String,
    fingerprint: String,
    file_size: u64,
}

pub fn init_serve_logging(
    config: &FawxConfig,
) -> Result<WorkerGuard, crate::startup::StartupError> {
    crate::startup::init_logging(&config.logging, crate::startup::LoggingMode::Serve)
}

impl Drop for HeadlessApp {
    fn drop(&mut self) {
        self.unsubscribe_from_session_bus();
        self.persist_embedding_index();
    }
}

fn headless_stream_callback(callback: StreamCallback) -> StreamCallback {
    Arc::new(move |event| {
        HeadlessApp::report_stream_error(&event);
        callback(event);
    })
}

impl HeadlessApp {
    fn initial_bus_receiver(deps: &HeadlessAppDeps) -> Option<mpsc::Receiver<Envelope>> {
        match (deps.session_bus.as_ref(), deps.session_key.as_ref()) {
            (Some(bus), Some(session_key)) => Some(bus.subscribe(session_key)),
            _ => None,
        }
    }

    fn persist_embedding_index(&self) {
        let Some(persistence) = &self.embedding_index_persistence else {
            return;
        };
        if let Err(error) = persistence.save_if_dirty() {
            tracing::warn!(error = %error, "failed to save embedding index on shutdown");
        }
    }

    fn unsubscribe_from_session_bus(&self) {
        let (Some(bus), Some(session_key)) = (&self.session_bus, self.session_key.as_ref()) else {
            return;
        };
        bus.unsubscribe(session_key);
    }

    /// Build from the standard startup bundle + router + config.
    pub fn new(mut deps: HeadlessAppDeps) -> Result<Self, anyhow::Error> {
        // Callers must seed the router's active model before construction.
        let active_model = read_router(&deps.router, |router| {
            resolve_active_model(router, &deps.config)
        })
        .unwrap_or_default();
        if active_model.is_empty() {
            tracing::warn!("{NO_AI_PROVIDERS_STARTUP_WARNING}");
            deps.startup_warnings.push(StartupWarning {
                category: ErrorCategory::System,
                message: NO_AI_PROVIDERS_STARTUP_WARNING.to_string(),
            });
        }
        let bus_receiver = Self::initial_bus_receiver(&deps);
        let max_history = deps.config.general.max_history;
        let data_dir = configured_data_dir(&fawx_data_dir(), &deps.config);
        let custom_system_prompt = resolve_system_prompt(
            deps.system_prompt_text,
            deps.system_prompt_path.as_deref(),
            &data_dir,
        );

        let mut app = Self {
            loop_engine: deps.loop_engine,
            router: deps.router,
            runtime_info: deps.runtime_info,
            config: deps.config,
            memory: deps.memory,
            embedding_index_persistence: deps.embedding_index_persistence,
            _subagent_manager: deps.subagent_manager,
            active_model,
            conversation_history: Vec::new(),
            last_signals: Vec::new(),
            max_history,
            custom_system_prompt,
            canary_monitor: deps.canary_monitor,
            config_manager: deps.config_manager,
            session_bus: deps.session_bus,
            session_key: deps.session_key,
            cron_store: deps.cron_store,
            startup_warnings: deps.startup_warnings,
            #[cfg(feature = "http")]
            experiment_registry: deps.experiment_registry,
            error_history: VecDeque::new(),
            cumulative_tokens: TokenUsage::default(),
            permission_callback_slot: deps.permission_callback_slot,
            ripcord_journal: deps.ripcord_journal,
            bus_receiver,
        };
        app.seed_runtime_info();
        if !app.active_model.is_empty() {
            app.loop_engine
                .update_context_limit(fx_llm::context_window_for_model(&app.active_model));
        }
        app.record_startup_warning_history();
        Ok(app)
    }

    fn seed_runtime_info(&self) {
        let provider = read_router(&self.router, |router| {
            router
                .provider_for_model(&self.active_model)
                .unwrap_or("")
                .to_string()
        });
        if let Ok(mut info) = self.runtime_info.write() {
            info.active_model = self.active_model.clone();
            info.provider = provider;
        }
    }

    fn record_startup_warning_history(&mut self) {
        let warnings: Vec<_> = self
            .startup_warnings
            .iter()
            .map(|warning| (warning.category, warning.message.clone()))
            .collect();
        for (category, message) in warnings {
            self.record_error(category, message, true);
        }
    }

    fn emit_startup_warnings(&mut self, callback: Option<&StreamCallback>) {
        let Some(callback) = callback else {
            return;
        };
        for warning in std::mem::take(&mut self.startup_warnings) {
            let event = StreamEvent::Error {
                category: warning.category,
                message: warning.message,
                recoverable: true,
            };
            Self::report_stream_error(&event);
            callback(event);
        }
    }

    fn clear_startup_warnings(&mut self) {
        self.startup_warnings.clear();
    }

    fn emit_error(
        &mut self,
        callback: Option<&StreamCallback>,
        category: ErrorCategory,
        message: String,
        recoverable: bool,
    ) {
        self.record_error(category, message.clone(), recoverable);
        let Some(callback) = callback else {
            return;
        };
        let event = StreamEvent::Error {
            category,
            message,
            recoverable,
        };
        Self::report_stream_error(&event);
        callback(event);
    }

    fn record_error(&mut self, category: ErrorCategory, message: String, recoverable: bool) {
        if self.error_history.len() == MAX_ERROR_HISTORY {
            self.error_history.pop_front();
        }
        self.error_history.push_back(ErrorRecord {
            timestamp: chrono::Utc::now().to_rfc3339(),
            category,
            message,
            recoverable,
        });
    }

    pub fn recent_errors(&self, limit: usize) -> Vec<ErrorRecord> {
        let capped = limit.min(MAX_ERROR_HISTORY);
        self.error_history
            .iter()
            .rev()
            .take(capped)
            .cloned()
            .collect()
    }

    /// REPL mode: read lines from stdin, run the loop, print to stdout.
    pub async fn run(&mut self, json_mode: bool) -> Result<i32, anyhow::Error> {
        install_sigpipe_handler();
        self.apply_custom_system_prompt();
        self.print_startup_info();

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                break; // EOF
            }

            let input = if json_mode {
                match self.parse_json_input(&line) {
                    Ok(msg) => msg,
                    Err(e) => {
                        eprintln!("error: invalid JSON input: {e}");
                        continue;
                    }
                }
            } else {
                line.trim().to_string()
            };

            if input.is_empty() {
                continue;
            }

            if is_quit_command(&input) {
                break;
            }

            self.process_input(&input, json_mode).await?;
        }

        Ok(0)
    }

    /// Single-shot mode: one input, one response, exit.
    pub async fn run_single(&mut self, json_mode: bool) -> Result<i32, anyhow::Error> {
        install_sigpipe_handler();
        self.apply_custom_system_prompt();

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let input = if json_mode {
            self.parse_json_input(&line)?
        } else {
            line.trim().to_string()
        };

        if input.is_empty() {
            return Ok(0);
        }

        self.process_input(&input, json_mode).await?;
        Ok(0)
    }

    /// Process a single message and return the result.
    ///
    /// Shared by the stdin REPL, single-shot mode, and the HTTP server.
    /// Updates memory context, runs a loop cycle, records the turn in
    /// conversation history, and returns the extracted response.
    pub async fn process_message(&mut self, input: &str) -> Result<CycleResult, anyhow::Error> {
        let source = InputSource::Text;
        self.process_message_for_source(input, &source).await
    }

    pub async fn process_message_streaming(
        &mut self,
        input: &str,
        callback: StreamCallback,
    ) -> Result<CycleResult, anyhow::Error> {
        let source = InputSource::Text;
        self.process_message_for_source_streaming(input, &source, callback)
            .await
    }

    pub async fn process_message_for_source(
        &mut self,
        input: &str,
        source: &InputSource,
    ) -> Result<CycleResult, anyhow::Error> {
        self.run_cycle_result(input, source).await
    }

    pub async fn process_message_with_images(
        &mut self,
        input: &str,
        images: &[ImageAttachment],
        source: &InputSource,
    ) -> Result<CycleResult, anyhow::Error> {
        self.clear_startup_warnings();
        self.update_memory_context(input);
        let snapshot = self.build_perception_snapshot_with_images(input, source, images);
        let llm = RouterLoopLlmProvider::new(Arc::clone(&self.router), self.active_model.clone());
        let result = self
            .loop_engine
            .run_cycle(snapshot, &llm)
            .await
            .map_err(|e| anyhow::anyhow!("loop error: stage={} reason={}", e.stage, e.reason))?;
        self.evaluate_canary(&result);
        Ok(self.finalize_cycle(input, &result))
    }

    pub async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        context: Vec<Message>,
        source: &InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<(CycleResult, Vec<Message>), anyhow::Error> {
        let original_history = std::mem::replace(&mut self.conversation_history, context);
        let result = match (images.is_empty(), callback) {
            (true, Some(callback)) => {
                process_input_with_commands_streaming(self, input, Some(source), callback).await
            }
            (true, None) => process_input_with_commands(self, input, Some(source)).await,
            (false, _) => {
                self.process_message_with_images(input, &images, source)
                    .await
            }
        };
        let updated_history = self.conversation_history.clone();
        self.conversation_history = original_history;
        result.map(|cycle| (cycle, updated_history))
    }

    pub async fn process_message_for_source_streaming(
        &mut self,
        input: &str,
        source: &InputSource,
        callback: StreamCallback,
    ) -> Result<CycleResult, anyhow::Error> {
        self.run_cycle_result_streaming(input, source, callback)
            .await
    }

    /// Return the active model identifier.
    pub fn active_model(&self) -> &str {
        &self.active_model
    }

    pub fn available_models(&self) -> Vec<ModelInfo> {
        read_router(&self.router, ModelRouter::available_models)
    }

    pub fn thinking_budget(&self) -> fx_config::ThinkingBudget {
        self.current_thinking_budget()
    }

    pub fn auth_provider_statuses(&self) -> Vec<AuthProviderStatus> {
        let data_dir = configured_data_dir(&fawx_data_dir(), &self.config);
        auth_provider_statuses(
            self.available_models(),
            stored_auth_provider_entries(&data_dir),
        )
    }

    /// Return the loaded configuration.
    #[allow(dead_code)]
    pub fn config(&self) -> &FawxConfig {
        &self.config
    }

    /// Return the shared config manager (if configured).
    pub fn config_manager(&self) -> Option<&Arc<Mutex<ConfigManager>>> {
        self.config_manager.as_ref()
    }

    pub fn ripcord_journal(&self) -> &Arc<fx_ripcord::RipcordJournal> {
        &self.ripcord_journal
    }

    pub fn session_bus(&self) -> Option<&SessionBus> {
        self.session_bus.as_ref()
    }

    pub fn cron_store(&self) -> Option<&fx_cron::SharedCronStore> {
        self.cron_store.as_ref()
    }

    #[cfg(feature = "http")]
    pub fn experiment_registry(&self) -> Option<&fx_api::SharedExperimentRegistry> {
        self.experiment_registry.as_ref()
    }

    pub fn thinking_available_levels(&self) -> Vec<String> {
        valid_thinking_levels(&self.active_model)
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    pub fn set_active_model(&mut self, selector: &str) -> anyhow::Result<String> {
        self.apply_active_model_selection(selector)
            .map(|(active_model, _)| active_model)
    }

    #[cfg(feature = "http")]
    pub fn switch_active_model(&mut self, selector: &str) -> anyhow::Result<ModelSwitchDto> {
        let previous_model = self.active_model.clone();
        let (active_model, thinking_adjusted) = self.apply_active_model_selection(selector)?;
        Ok(ModelSwitchDto {
            previous_model,
            active_model,
            thinking_adjusted: thinking_adjusted.map(|(from, to)| ThinkingAdjustedDto {
                from: from.to_string(),
                to: to.to_string(),
                reason: thinking_adjustment_reason(
                    from,
                    to,
                    self.active_provider_name().as_deref(),
                ),
            }),
        })
    }

    pub fn handle_thinking(&mut self, level: Option<&str>) -> anyhow::Result<String> {
        let Some(level) = level else {
            return Ok(format!(
                "Current thinking budget: {}",
                self.config.general.thinking.unwrap_or_default()
            ));
        };
        self.set_supported_thinking_level(level)?;
        Ok(format!(
            "Thinking budget set to: {}",
            self.thinking_budget()
        ))
    }

    #[cfg(feature = "http")]
    fn active_provider_name(&self) -> Option<String> {
        read_router(&self.router, |router| {
            router
                .provider_for_model(&self.active_model)
                .map(ToString::to_string)
        })
    }

    fn current_thinking_budget(&self) -> ThinkingBudget {
        self.config.general.thinking.unwrap_or_default()
    }

    fn apply_supported_thinking_level(&mut self, level: &str) -> anyhow::Result<()> {
        let budget: ThinkingBudget = level
            .parse()
            .map_err(|error: String| anyhow::anyhow!(error))?;
        self.ensure_supported_thinking_budget(budget)?;
        apply_thinking_budget(
            &mut self.config,
            &mut self.loop_engine,
            self.config_manager.as_ref(),
            &fawx_data_dir(),
            &self.active_model,
            Some(&budget.to_string()),
        )?;
        Ok(())
    }

    #[cfg(feature = "http")]
    pub fn set_supported_thinking_level(
        &mut self,
        level: &str,
    ) -> anyhow::Result<ThinkingLevelDto> {
        self.apply_supported_thinking_level(level)?;
        Ok(self.thinking_level_dto())
    }

    #[cfg(not(feature = "http"))]
    pub fn set_supported_thinking_level(&mut self, level: &str) -> anyhow::Result<()> {
        self.apply_supported_thinking_level(level)
    }

    fn ensure_supported_thinking_budget(&self, budget: ThinkingBudget) -> anyhow::Result<()> {
        let level = budget.to_string();
        let available = self.thinking_available_levels();
        if available.iter().any(|candidate| candidate == &level) {
            return Ok(());
        }
        Err(anyhow::anyhow!(
            "Thinking level '{}' is not supported by the current model. Available: {}",
            level,
            available.join(", ")
        ))
    }

    #[cfg(feature = "http")]
    pub fn thinking_level_dto(&self) -> ThinkingLevelDto {
        ThinkingLevelDto {
            level: self.current_thinking_budget().to_string(),
            budget_tokens: self.current_thinking_budget().budget_tokens(),
            available: self.thinking_available_levels(),
        }
    }

    fn apply_active_model_selection(
        &mut self,
        selector: &str,
    ) -> anyhow::Result<(String, Option<(ThinkingBudget, ThinkingBudget)>)> {
        let active_model = read_router(&self.router, |router| {
            resolve_headless_model_selector(router, selector)
        })?;
        apply_headless_active_model(self, &active_model);
        persist_default_model(
            &mut self.config,
            self.config_manager.as_ref(),
            &fawx_data_dir(),
            &active_model,
        )?;
        let thinking_adjusted = self.align_thinking_for_active_model()?;
        Ok((active_model, thinking_adjusted))
    }

    fn align_thinking_for_active_model(
        &mut self,
    ) -> anyhow::Result<Option<(ThinkingBudget, ThinkingBudget)>> {
        let current = self.current_thinking_budget();
        if self.is_supported_thinking_budget(current) {
            return Ok(None);
        }
        let adjusted = preferred_supported_budget(&self.thinking_available_levels());
        apply_thinking_budget(
            &mut self.config,
            &mut self.loop_engine,
            self.config_manager.as_ref(),
            &fawx_data_dir(),
            &self.active_model,
            Some(&adjusted.to_string()),
        )?;
        Ok(Some((current, adjusted)))
    }

    fn is_supported_thinking_budget(&self, budget: ThinkingBudget) -> bool {
        let level = budget.to_string();
        self.thinking_available_levels()
            .iter()
            .any(|candidate| candidate == &level)
    }

    pub fn context_info_snapshot(&self) -> ContextInfoSnapshot {
        self.context_info_snapshot_for_messages(&self.conversation_history)
    }

    pub fn context_info_snapshot_for_messages(&self, messages: &[Message]) -> ContextInfoSnapshot {
        let budget = self.loop_engine.conversation_budget_ref();
        let used_tokens =
            fx_kernel::conversation_compactor::ConversationBudget::estimate_tokens(messages);
        let max_tokens = budget.conversation_budget();
        ContextInfoSnapshot {
            used_tokens,
            max_tokens,
            percentage: context_usage_percentage(used_tokens, max_tokens),
            compaction_threshold: budget.compaction_threshold_value(),
        }
    }

    #[cfg(feature = "http")]
    pub fn context_info(&self) -> ContextInfoDto {
        ContextInfoDto::from_snapshot(&self.context_info_snapshot())
    }

    pub fn skill_summaries(&self) -> Vec<(String, String, Vec<String>, Vec<String>)> {
        match self.runtime_info.read() {
            Ok(info) => runtime_skill_summaries(&info),
            Err(error) => {
                tracing::warn!(error = %error, "runtime info lock poisoned");
                Vec::new()
            }
        }
    }

    /// Apply the custom system prompt (if any). Must be called once
    /// before the first `process_message` invocation when not using
    /// the built-in `run()` or `run_single()` methods.
    pub fn initialize(&mut self) {
        self.apply_custom_system_prompt();
    }

    pub(crate) async fn analyze_signals_command(&mut self) -> anyhow::Result<String> {
        let signal_store = headless_signal_store(&self.config)?;
        let provider =
            AnalysisCompletionProvider::new(Arc::clone(&self.router), self.active_model.clone());
        let engine = AnalysisEngine::new(&signal_store);
        match engine.analyze(&provider).await {
            Ok(findings) => Ok(render_analysis_output(&findings, self.memory.as_ref())),
            Err(AnalysisError::ParseError(error)) => Ok(format!(
                "Analysis model responded, but output was unparseable JSON: {error}"
            )),
            Err(error) => Err(anyhow::Error::new(error)),
        }
    }

    pub(crate) async fn improve_command(&mut self, flags: &ImproveFlags) -> anyhow::Result<String> {
        if let Some(unknown) = &flags.has_unknown_flag {
            return Ok(format!(
                "Unknown flag: {unknown}\nUsage: /improve [--dry-run]"
            ));
        }
        let signal_store = headless_signal_store(&self.config)?;
        let provider =
            AnalysisCompletionProvider::new(Arc::clone(&self.router), self.active_model.clone());
        let (config, data_dir, repo_root, proposals_dir) =
            build_headless_improve_context(&self.config, flags);
        let paths = CyclePaths {
            data_dir: &data_dir,
            repo_root: &repo_root,
            proposals_dir: &proposals_dir,
        };
        let result = fx_improve::run_improvement_cycle(&signal_store, &provider, &config, &paths)
            .await
            .map_err(anyhow::Error::from)?;
        Ok(render_improve_output(&result, flags.dry_run))
    }

    #[cfg(feature = "http")]
    pub fn apply_http_defaults(&mut self) {
        let selector = self
            .config
            .model
            .default_model
            .as_deref()
            .unwrap_or(DEFAULT_HTTP_MODEL);

        let active_model = write_router(&self.router, |router| {
            if let Err(error) = router.set_active(selector) {
                tracing::warn!(
                    model = selector,
                    error = %error,
                    "failed to set HTTP default model"
                );
                return None;
            }

            router.active_model().map(ToString::to_string)
        });

        if let Some(active_model) = active_model {
            self.active_model = active_model.clone();
            self.loop_engine
                .update_context_limit(fx_llm::context_window_for_model(&self.active_model));
            if self.config.model.default_model.is_none() {
                self.config.model.default_model = Some(active_model);
            }
        }
    }

    fn apply_reloaded_router(&mut self, mut router: ModelRouter) -> anyhow::Result<()> {
        let next_active_model = if !self.active_model.is_empty()
            && headless_model_available(&router, &self.active_model)
        {
            Some(self.active_model.clone())
        } else {
            resolve_active_model(&router, &self.config).ok()
        };

        if let Some(active_model) = next_active_model {
            router.set_active(&active_model)?;
            self.active_model = active_model;
            self.loop_engine
                .update_context_limit(fx_llm::context_window_for_model(&self.active_model));
        } else {
            self.active_model.clear();
        }

        write_router(&self.router, |shared_router| *shared_router = router);
        self.seed_runtime_info();
        Ok(())
    }

    pub fn reload_providers(&mut self) -> anyhow::Result<()> {
        let auth_manager = crate::startup::load_auth_manager()?;
        let router = crate::startup::build_router(&auth_manager)?;
        self.apply_reloaded_router(router)
    }

    // ── internal helpers ────────────────────────────────────────────────

    async fn process_input(&mut self, input: &str, json_mode: bool) -> Result<(), anyhow::Error> {
        let result = self.process_message(input).await?;
        if json_mode {
            let output = JsonOutput {
                response: result.response,
                model: result.model,
                iterations: result.iterations,
            };
            let json = serde_json::to_string(&output)?;
            println!("{json}");
            io::stdout().flush()?;
        } else {
            println!("{}", result.response);
            io::stdout().flush()?;
        }
        Ok(())
    }

    async fn run_cycle_result(
        &mut self,
        input: &str,
        source: &InputSource,
    ) -> Result<CycleResult, anyhow::Error> {
        self.clear_startup_warnings();
        self.update_memory_context(input);
        let snapshot = self.build_perception_snapshot(input, source);
        let llm = RouterLoopLlmProvider::new(Arc::clone(&self.router), self.active_model.clone());
        let result = self
            .loop_engine
            .run_cycle(snapshot, &llm)
            .await
            .map_err(|e| anyhow::anyhow!("loop error: stage={} reason={}", e.stage, e.reason))?;
        self.evaluate_canary(&result);
        Ok(self.finalize_cycle(input, &result))
    }

    async fn run_cycle_result_streaming(
        &mut self,
        input: &str,
        source: &InputSource,
        callback: StreamCallback,
    ) -> Result<CycleResult, anyhow::Error> {
        let callback = headless_stream_callback(callback);
        // Set permission prompt callback for this cycle so SSE events fire
        self.set_permission_callback(Some(Arc::clone(&callback)));
        self.emit_startup_warnings(Some(&callback));
        self.update_memory_context(input);
        let snapshot = self.build_perception_snapshot(input, source);
        let llm = RouterLoopLlmProvider::new(Arc::clone(&self.router), self.active_model.clone());
        let result = self
            .loop_engine
            .run_cycle_streaming(snapshot, &llm, Some(callback))
            .await
            .map_err(|e| anyhow::anyhow!("loop error: stage={} reason={}", e.stage, e.reason))?;
        self.set_permission_callback(None); // Clear after cycle
        self.evaluate_canary(&result);
        Ok(self.finalize_cycle(input, &result))
    }

    fn report_stream_error(event: &StreamEvent) {
        if let StreamEvent::Error {
            category,
            message,
            recoverable,
        } = event
        {
            let level = if *recoverable { "warning" } else { "error" };
            eprintln!("[{level}] [{category}] {message}");
        }
    }

    fn finalize_cycle(&mut self, input: &str, result: &LoopResult) -> CycleResult {
        let response = extract_response_text(result);
        let iterations = extract_iterations(result);
        let tokens_used = extract_token_usage(result);
        self.cumulative_tokens.input_tokens = self
            .cumulative_tokens
            .input_tokens
            .saturating_add(tokens_used.input_tokens);
        self.cumulative_tokens.output_tokens = self
            .cumulative_tokens
            .output_tokens
            .saturating_add(tokens_used.output_tokens);
        self.last_signals = result.signals().to_vec();
        let signals = self.last_signals.clone();
        persist_headless_signals(self, &signals);
        self.record_turn(input, &response);
        CycleResult {
            response,
            model: self.active_model.clone(),
            iterations,
            tokens_used,
        }
    }

    fn set_permission_callback(&self, callback: Option<fx_kernel::streaming::StreamCallback>) {
        if let Ok(mut guard) = self.permission_callback_slot.lock() {
            *guard = callback;
        }
    }

    fn evaluate_canary(&mut self, result: &LoopResult) {
        let Some(monitor) = self.canary_monitor.as_mut() else {
            return;
        };
        if let Some(verdict) = monitor.on_cycle_complete(result.signals().to_vec()) {
            tracing::info!(?verdict, "canary verdict");
        }
    }

    fn apply_custom_system_prompt(&mut self) {
        if self.custom_system_prompt.is_some() {
            // Initial memory context injection; update_memory_context()
            // will re-inject the custom prompt on each cycle.
            self.update_memory_context("");
        }
    }

    fn print_startup_info(&self) {
        eprintln!("fawx serve — headless mode");
        eprintln!("model: {}", self.active_model);
        if self.custom_system_prompt.is_some() {
            eprintln!("system prompt: custom prompt/context loaded");
        }
        eprintln!("ready (type /quit to exit)");
    }

    fn update_memory_context(&mut self, input: &str) {
        let mut context_parts: Vec<String> = Vec::new();

        if let Some(prompt) = &self.custom_system_prompt {
            context_parts.push(prompt.clone());
        }

        if let Some(mem) = self.relevant_memory_context(input) {
            context_parts.push(mem);
        }

        let combined = context_parts.join("\n\n");
        self.loop_engine.set_memory_context(combined);
    }

    fn relevant_memory_context(&self, input: &str) -> Option<String> {
        let entries = self.search_memory_entries(input)?;
        format_memory_for_prompt(&entries, self.config.memory.max_snapshot_chars)
    }

    fn search_memory_entries(&self, input: &str) -> Option<Vec<(String, String)>> {
        let memory = self.memory.as_ref()?;
        match memory.lock() {
            Ok(store) => {
                let max = self.config.memory.max_relevant_results;
                Some((*store).search_relevant(input, max))
            }
            Err(e) => {
                eprintln!("warning: failed to lock memory store: {e}");
                None
            }
        }
    }

    fn build_perception_snapshot(&self, input: &str, source: &InputSource) -> PerceptionSnapshot {
        self.build_perception_snapshot_with_images(input, source, &[])
    }

    fn build_perception_snapshot_with_images(
        &self,
        input: &str,
        source: &InputSource,
        images: &[ImageAttachment],
    ) -> PerceptionSnapshot {
        let timestamp_ms = current_time_ms();
        let image_pairs = images.to_vec();
        PerceptionSnapshot {
            screen: ScreenState {
                current_app: "fawx.headless".to_string(),
                elements: Vec::new(),
                text_content: input.to_string(),
            },
            notifications: Vec::new(),
            active_app: "fawx.headless".to_string(),
            timestamp_ms,
            sensor_data: None,
            user_input: Some(UserInput {
                text: input.to_string(),
                source: source.clone(),
                timestamp: timestamp_ms,
                context_id: None,
                images: image_pairs,
            }),
            conversation_history: self.conversation_history.clone(),
            steer_context: None,
        }
    }

    fn record_turn(&mut self, user_text: &str, assistant_text: &str) {
        self.conversation_history
            .push(Message::user(user_text.to_string()));
        self.conversation_history
            .push(Message::assistant(assistant_text.to_string()));
        trim_history(&mut self.conversation_history, self.max_history);
    }

    fn parse_json_input(&self, raw: &str) -> Result<String, serde_json::Error> {
        let parsed: JsonInput = serde_json::from_str(raw)?;
        Ok(parsed.message)
    }

    async fn list_models_dynamic(&self) -> anyhow::Result<String> {
        let models = self.dynamic_models_or_fallback().await?;
        Ok(render_model_menu_text(
            Some(self.active_model.as_str()),
            &models,
        ))
    }

    async fn dynamic_models_or_fallback(&self) -> anyhow::Result<Vec<ModelInfo>> {
        let models = fetch_shared_available_models(&self.router).await;
        if models.is_empty() {
            return Ok(self.available_models());
        }
        Ok(models)
    }
}

#[cfg(feature = "http")]
#[async_trait]
impl AppEngine for HeadlessApp {
    async fn process_message(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<ApiCycleResult, anyhow::Error> {
        let (result, updated_history) = HeadlessApp::process_message_with_context(
            self,
            input,
            images,
            self.conversation_history.clone(),
            &source,
            callback,
        )
        .await?;
        self.conversation_history = updated_history;

        Ok(ApiCycleResult {
            response: result.response,
            model: result.model,
            iterations: result.iterations,
        })
    }

    async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        context: Vec<Message>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<(ApiCycleResult, Vec<Message>), anyhow::Error> {
        let (result, updated_history) = HeadlessApp::process_message_with_context(
            self, input, images, context, &source, callback,
        )
        .await?;

        Ok((
            ApiCycleResult {
                response: result.response,
                model: result.model,
                iterations: result.iterations,
            },
            updated_history,
        ))
    }

    fn active_model(&self) -> &str {
        HeadlessApp::active_model(self)
    }

    fn available_models(&self) -> Vec<ModelInfoDto> {
        HeadlessApp::available_models(self)
            .into_iter()
            .map(ModelInfoDto::from)
            .collect()
    }

    fn set_active_model(&mut self, selector: &str) -> Result<ModelSwitchDto, anyhow::Error> {
        HeadlessApp::switch_active_model(self, selector)
    }

    fn thinking_level(&self) -> ThinkingLevelDto {
        HeadlessApp::thinking_level_dto(self)
    }

    fn context_info(&self) -> ContextInfoDto {
        HeadlessApp::context_info(self)
    }

    fn context_info_for_messages(&self, messages: &[Message]) -> ContextInfoDto {
        ContextInfoDto::from_snapshot(&HeadlessApp::context_info_snapshot_for_messages(
            self, messages,
        ))
    }

    fn set_thinking_level(&mut self, level: &str) -> Result<ThinkingLevelDto, anyhow::Error> {
        HeadlessApp::set_supported_thinking_level(self, level)
    }

    fn skill_summaries(&self) -> Vec<SkillSummaryDto> {
        HeadlessApp::skill_summaries(self)
            .into_iter()
            .map(SkillSummaryDto::from)
            .collect()
    }

    fn auth_provider_statuses(&self) -> Vec<AuthProviderDto> {
        HeadlessApp::auth_provider_statuses(self)
            .into_iter()
            .map(auth_provider_dto)
            .collect()
    }

    fn config_manager(&self) -> Option<ConfigManagerHandle> {
        HeadlessApp::config_manager(self).cloned()
    }

    fn session_bus(&self) -> Option<&SessionBus> {
        HeadlessApp::session_bus(self)
    }

    fn reload_providers(&mut self) -> Result<(), anyhow::Error> {
        HeadlessApp::reload_providers(self)
    }

    fn recent_errors(&self, limit: usize) -> Vec<fx_api::ErrorRecordDto> {
        HeadlessApp::recent_errors(self, limit)
            .into_iter()
            .map(|record| fx_api::ErrorRecordDto {
                timestamp: record.timestamp,
                category: record.category,
                message: record.message,
                recoverable: record.recoverable,
            })
            .collect()
    }

    fn max_history(&self) -> usize {
        self.max_history
    }

    fn session_token_usage(&self) -> (u64, u64) {
        (
            self.cumulative_tokens.input_tokens,
            self.cumulative_tokens.output_tokens,
        )
    }
}

impl CommandHost for HeadlessApp {
    fn supports_embedded_slash_commands(&self) -> bool {
        true
    }

    fn list_models(&self) -> String {
        render_model_menu_text(Some(self.active_model.as_str()), &self.available_models())
    }

    fn set_active_model(&mut self, selector: &str) -> anyhow::Result<String> {
        HeadlessApp::set_active_model(self, selector)
    }

    fn proposals(&self, selector: Option<&str>) -> anyhow::Result<String> {
        render_pending(headless_review_context(&self.config), selector).map_err(anyhow::Error::new)
    }

    fn approve(&self, selector: &str, force: bool) -> anyhow::Result<String> {
        approve_pending(headless_review_context(&self.config), selector, force)
            .map_err(anyhow::Error::new)
    }

    fn reject(&self, selector: &str) -> anyhow::Result<String> {
        reject_pending(headless_review_context(&self.config), selector).map_err(anyhow::Error::new)
    }

    fn show_config(&self) -> anyhow::Result<String> {
        let config_path = headless_config_path(&self.config, self.config_manager.as_ref())?;
        let data_dir = configured_data_dir(&fawx_data_dir(), &self.config);
        let json = headless_config_json(&self.config, self.config_manager.as_ref())?;
        render_headless_config(&config_path, &data_dir, &self.active_model, &json)
    }

    fn init_config(&mut self) -> anyhow::Result<String> {
        init_default_config(&fawx_data_dir())
    }

    fn reload_config(&mut self) -> anyhow::Result<String> {
        let config_path = headless_config_path(&self.config, self.config_manager.as_ref())?;
        self.config = reload_runtime_config(self.config_manager.as_ref(), &config_path)?;
        self.max_history = self.config.general.max_history;
        let thinking_budget = self.config.general.thinking.unwrap_or_default();
        sync_headless_model_from_config(self, self.config.model.default_model.clone())?;
        self.loop_engine
            .set_thinking_config(thinking_config_for_active_model(
                &thinking_budget,
                &self.active_model,
            ));
        Ok(config_reload_success_message(&config_path))
    }

    fn show_status(&self) -> String {
        let providers = read_router(&self.router, available_provider_names);
        render_status_text(
            &self.active_model,
            &providers,
            self.loop_engine.status(current_time_ms()),
        )
    }

    fn show_budget_status(&self) -> String {
        render_budget_text(self.loop_engine.status(current_time_ms()))
    }

    fn show_signals_summary(&self) -> String {
        render_signals_summary(&self.last_signals)
    }

    fn handle_thinking(&mut self, level: Option<&str>) -> anyhow::Result<String> {
        HeadlessApp::handle_thinking(self, level)
    }

    fn show_history(&self) -> anyhow::Result<String> {
        Ok(format!(
            "Conversation history: {} messages in current session",
            self.conversation_history.len()
        ))
    }

    fn new_conversation(&mut self) -> anyhow::Result<String> {
        self.conversation_history.clear();
        Ok("Started a new conversation.".to_string())
    }

    fn show_loop_status(&self) -> anyhow::Result<String> {
        Ok(render_loop_status(
            self.loop_engine.status(current_time_ms()),
        ))
    }

    fn show_debug(&self) -> anyhow::Result<String> {
        Ok(render_debug_dump(&self.last_signals))
    }

    fn handle_synthesis(&mut self, instruction: Option<&str>) -> anyhow::Result<String> {
        handle_headless_synthesis_command(&mut self.loop_engine, instruction)
    }

    fn handle_auth(
        &self,
        subcommand: Option<&str>,
        action: Option<&str>,
        value: Option<&str>,
        has_extra_args: bool,
    ) -> anyhow::Result<String> {
        read_router(&self.router, |router| {
            handle_headless_auth_command(router, subcommand, action, value, has_extra_args)
        })
    }

    fn handle_keys(
        &self,
        subcommand: Option<&str>,
        value: Option<&str>,
        option: Option<&str>,
        has_extra_args: bool,
    ) -> anyhow::Result<String> {
        let data_dir = configured_data_dir(&fawx_data_dir(), &self.config);
        handle_headless_keys_command(&data_dir, subcommand, value, option, has_extra_args)
    }

    fn handle_sign(&self, _target: Option<&str>, _has_extra_args: bool) -> anyhow::Result<String> {
        Ok("Use `fawx sign <skill>` CLI to sign WASM packages.".to_string())
    }
}

fn preferred_supported_budget(levels: &[String]) -> ThinkingBudget {
    for budget in [
        ThinkingBudget::High,
        ThinkingBudget::Adaptive,
        ThinkingBudget::Low,
        ThinkingBudget::Off,
    ] {
        if levels.iter().any(|level| level == &budget.to_string()) {
            return budget;
        }
    }
    ThinkingBudget::Off
}

#[cfg(feature = "http")]
fn thinking_adjustment_reason(
    from: ThinkingBudget,
    to: ThinkingBudget,
    provider: Option<&str>,
) -> String {
    let provider = provider.unwrap_or("unknown");
    format!("{} not supported by {}; adjusted to {}", from, provider, to)
}

fn handle_headless_synthesis_command(
    loop_engine: &mut LoopEngine,
    instruction: Option<&str>,
) -> anyhow::Result<String> {
    match instruction {
        None => Ok("Usage: /synthesis <instruction> or /synthesis reset".to_string()),
        Some(value) if value.trim().is_empty() => {
            Ok("Synthesis instruction cannot be empty.".to_string())
        }
        Some(value) if value.eq_ignore_ascii_case("reset") => {
            loop_engine
                .set_synthesis_instruction(DEFAULT_SYNTHESIS_INSTRUCTION.to_string())
                .map_err(|error| anyhow::anyhow!(error.reason))?;
            Ok("Synthesis instruction reset to default.".to_string())
        }
        Some(value) => update_headless_synthesis_instruction(loop_engine, value),
    }
}

fn update_headless_synthesis_instruction(
    loop_engine: &mut LoopEngine,
    value: &str,
) -> anyhow::Result<String> {
    if value.len() > MAX_SYNTHESIS_INSTRUCTION_LENGTH {
        return Ok(format!(
            "Synthesis instruction exceeds {} characters.",
            MAX_SYNTHESIS_INSTRUCTION_LENGTH
        ));
    }
    loop_engine
        .set_synthesis_instruction(value.to_string())
        .map_err(|error| anyhow::anyhow!(error.reason))?;
    Ok(format!("Synthesis instruction updated: {}", value.trim()))
}

fn handle_headless_auth_command(
    router: &ModelRouter,
    subcommand: Option<&str>,
    action: Option<&str>,
    value: Option<&str>,
    has_extra_args: bool,
) -> anyhow::Result<String> {
    if is_auth_write_action(action) {
        return Ok("Use `fawx setup` to manage credentials.".to_string());
    }
    match (subcommand, action, value, has_extra_args) {
        (None, None, None, false) | (Some("list-providers"), None, None, false) => {
            Ok(render_auth_overview(router))
        }
        (Some(provider), Some("show-status"), None, false) => {
            Ok(render_auth_provider_status(router, provider))
        }
        _ => Ok(auth_usage_message()),
    }
}

fn is_auth_write_action(action: Option<&str>) -> bool {
    matches!(action, Some("set-token") | Some("clear-token"))
}

fn auth_usage_message() -> String {
    "Usage: /auth {provider} <set-token|show-status|clear-token> [TOKEN]".to_string()
}

fn render_auth_overview(router: &ModelRouter) -> String {
    let statuses = auth_provider_statuses(router.available_models(), Vec::new());
    if statuses.is_empty() {
        return "No credentials configured.".to_string();
    }
    let mut lines = vec!["Configured credentials:".to_string()];
    lines.extend(statuses.iter().map(render_auth_status_line));
    lines.join("\n")
}

fn render_auth_status_line(status: &AuthProviderStatus) -> String {
    let state_label = match status.status.as_str() {
        "saved" => "saved",
        _ => "configured",
    };
    format!(
        "  ✓ {}: {} ({}) — {}",
        status.provider,
        state_label,
        format_auth_methods(&status.auth_methods),
        model_count_label(status.model_count)
    )
}

fn render_auth_provider_status(router: &ModelRouter, provider: &str) -> String {
    let provider = normalize_provider_name(provider);
    match auth_provider_statuses(router.available_models(), Vec::new())
        .into_iter()
        .find(|status| status.provider == provider)
    {
        Some(status) => format!(
            "{} auth status:\n  Status: {} ({})\n  Models available: {}",
            status.provider,
            status.status,
            format_auth_methods(&status.auth_methods),
            status.model_count
        ),
        None => format!("{provider} auth status:\n  Status: not configured"),
    }
}

fn auth_provider_statuses(
    models: Vec<ModelInfo>,
    stored_auth_entries: Vec<StoredAuthProviderEntry>,
) -> Vec<AuthProviderStatus> {
    let mut statuses = BTreeMap::new();
    for entry in stored_auth_entries {
        update_saved_auth_provider_status(&mut statuses, entry);
    }
    for model in models {
        update_auth_provider_status(&mut statuses, model);
    }
    statuses.into_values().collect()
}

fn runtime_skill_summaries(info: &RuntimeInfo) -> Vec<(String, String, Vec<String>, Vec<String>)> {
    info.skills
        .iter()
        .map(|skill| {
            (
                skill.name.clone(),
                skill.description.clone().unwrap_or_default(),
                skill.tool_names.clone(),
                skill.capabilities.clone(),
            )
        })
        .collect()
}

fn context_usage_percentage(used_tokens: usize, max_tokens: usize) -> f32 {
    if max_tokens == 0 {
        0.0
    } else {
        (used_tokens as f32 / max_tokens as f32) * 100.0
    }
}

#[cfg(feature = "http")]
fn auth_provider_dto(status: AuthProviderStatus) -> AuthProviderDto {
    AuthProviderDto {
        provider: status.provider,
        auth_methods: status.auth_methods.into_iter().collect(),
        model_count: status.model_count,
        status: status.status,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredAuthProviderEntry {
    provider: String,
    auth_method: String,
}

fn stored_auth_provider_entries(data_dir: &Path) -> Vec<StoredAuthProviderEntry> {
    let store = match AuthStore::open(data_dir) {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(error = %error, "failed to open auth store while building auth statuses");
            return Vec::new();
        }
    };
    let auth_manager = match store.load_auth_manager() {
        Ok(auth_manager) => auth_manager,
        Err(error) => {
            tracing::warn!(error = %error, "failed to load auth manager while building auth statuses");
            return Vec::new();
        }
    };

    auth_manager
        .providers()
        .into_iter()
        .filter_map(|provider| {
            let auth_method = auth_manager
                .get(&provider)
                .map(stored_auth_method_label)?
                .to_string();
            Some(StoredAuthProviderEntry {
                provider: normalize_provider_name(&provider),
                auth_method,
            })
        })
        .collect()
}

fn stored_auth_method_label(auth_method: &fx_auth::auth::AuthMethod) -> &'static str {
    match auth_method {
        fx_auth::auth::AuthMethod::ApiKey { .. } => "api_key",
        fx_auth::auth::AuthMethod::SetupToken { .. } => "setup_token",
        fx_auth::auth::AuthMethod::OAuth { .. } => "oauth",
    }
}

fn update_saved_auth_provider_status(
    statuses: &mut BTreeMap<String, AuthProviderStatus>,
    entry: StoredAuthProviderEntry,
) {
    let status = statuses
        .entry(entry.provider.clone())
        .or_insert_with(|| AuthProviderStatus {
            provider: entry.provider,
            auth_methods: BTreeSet::new(),
            model_count: 0,
            status: "saved".to_string(),
        });
    status.auth_methods.insert(entry.auth_method);
    if status.model_count == 0 {
        status.status = "saved".to_string();
    }
}

fn update_auth_provider_status(
    statuses: &mut BTreeMap<String, AuthProviderStatus>,
    model: ModelInfo,
) {
    let provider = normalize_provider_name(&model.provider_name);
    let status = statuses
        .entry(provider.clone())
        .or_insert_with(|| AuthProviderStatus {
            provider,
            auth_methods: BTreeSet::new(),
            model_count: 0,
            status: "registered".to_string(),
        });
    status.auth_methods.insert(model.auth_method);
    status.model_count += 1;
    // GitHub models use the same PAT-backed auth path as the dedicated
    // settings card, so keep a persisted token visible as "saved" instead of
    // collapsing it back to the generic "registered" model-provider state.
    if status.provider == "github" && status.status == "saved" {
        return;
    }
    status.status = "registered".to_string();
}

fn format_auth_methods(auth_methods: &BTreeSet<String>) -> String {
    auth_methods
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn model_count_label(model_count: usize) -> String {
    match model_count {
        1 => "1 model".to_string(),
        count => format!("{count} models"),
    }
}

fn normalize_provider_name(value: &str) -> String {
    let lower = value.trim().to_ascii_lowercase();
    match lower.as_str() {
        "gh" => "github".to_string(),
        other => other.to_string(),
    }
}

fn handle_headless_keys_command(
    base_dir: &Path,
    subcommand: Option<&str>,
    value: Option<&str>,
    option: Option<&str>,
    has_extra_args: bool,
) -> anyhow::Result<String> {
    match subcommand {
        Some("list") if value.is_none() && option.is_none() && !has_extra_args => {
            render_trusted_key_list(base_dir)
        }
        Some("list") => Ok("Usage: /keys list".to_string()),
        Some(other) => Ok(keys_redirect_message(other)),
        None => Ok("Usage: /keys list".to_string()),
    }
}

fn keys_redirect_message(subcommand: &str) -> String {
    format!("Use `fawx keys {subcommand}` CLI for key management.")
}

fn render_trusted_key_list(base_dir: &Path) -> anyhow::Result<String> {
    let keys = trusted_key_entries_from_dir(&trusted_keys_dir(base_dir))?;
    if keys.is_empty() {
        return Ok("No trusted public keys.".to_string());
    }
    let mut lines = vec!["Trusted public keys:".to_string()];
    lines.extend(keys.into_iter().map(render_trusted_key_line));
    Ok(lines.join("\n"))
}

fn render_trusted_key_line(key: TrustedKeyEntry) -> String {
    format!(
        "  {} {} {} bytes",
        key.file_name, key.fingerprint, key.file_size
    )
}

fn trusted_keys_dir(base_dir: &Path) -> PathBuf {
    base_dir.join("trusted_keys")
}

fn trusted_key_entries_from_dir(trusted_dir: &Path) -> anyhow::Result<Vec<TrustedKeyEntry>> {
    let mut keys = Vec::new();
    if !trusted_dir.exists() {
        return Ok(keys);
    }
    for entry in std::fs::read_dir(trusted_dir)? {
        let path = entry?.path();
        if is_public_key_path(&path) {
            keys.push(trusted_key_entry_from_path(&path)?);
        }
    }
    keys.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(keys)
}

fn trusted_key_entry_from_path(path: &Path) -> anyhow::Result<TrustedKeyEntry> {
    let public_key = read_public_key_file(path)?;
    let file_name = display_file_name(path);
    Ok(TrustedKeyEntry {
        file_name,
        fingerprint: public_key_fingerprint(&public_key),
        file_size: std::fs::metadata(path)?.len(),
    })
}

fn read_public_key_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    let public_key = std::fs::read(path)?;
    if public_key.len() != 32 {
        return Err(anyhow::anyhow!(
            "invalid public key length at {}: expected 32 bytes, found {}",
            path.display(),
            public_key.len()
        ));
    }
    Ok(public_key)
}

fn is_public_key_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("pub")
}

fn public_key_fingerprint(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    hex_encode(&digest[..8])
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn display_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn resolve_headless_model_selector(router: &ModelRouter, selector: &str) -> anyhow::Result<String> {
    let model_ids = router
        .available_models()
        .into_iter()
        .map(|model| model.model_id)
        .collect::<Vec<_>>();
    if model_ids.iter().any(|model_id| model_id == selector) {
        return Ok(selector.to_string());
    }
    resolve_model_alias(selector, &model_ids)
        .ok_or_else(|| anyhow::anyhow!("model not found: {selector}"))
}

fn sync_headless_model_from_config(
    app: &mut HeadlessApp,
    default_model: Option<String>,
) -> anyhow::Result<()> {
    let resolved = read_router(&app.router, |router| {
        resolve_requested_model(router, default_model.as_deref())
    })?;
    apply_headless_active_model(app, &resolved);
    Ok(())
}

fn apply_headless_active_model(app: &mut HeadlessApp, model: &str) {
    let error_message = write_router(&app.router, |router| {
        if let Err(error) = router.set_active(model) {
            tracing::warn!(error = %error, model, "failed to apply reloaded model to router");
            Some(format!("Model reload failed after config change: {error}"))
        } else {
            None
        }
    });
    if let Some(message) = error_message {
        app.record_error(ErrorCategory::System, message, true);
    }
    app.active_model = model.to_string();
    app.loop_engine
        .update_context_limit(fx_llm::context_window_for_model(&app.active_model));
}

fn headless_signal_store(config: &FawxConfig) -> anyhow::Result<SignalStore> {
    let data_dir = configured_data_dir(&fawx_data_dir(), config);
    SignalStore::new(&data_dir, HEADLESS_SIGNAL_SESSION_ID).map_err(anyhow::Error::new)
}

fn persist_headless_signals(app: &mut HeadlessApp, signals: &[Signal]) {
    if let Ok(signal_store) = headless_signal_store(&app.config) {
        if let Err(error) = signal_store.persist(signals) {
            let message = format!("Signal persist failed: {error}");
            eprintln!("warning: signal persist failed: {error}");
            app.emit_error(None, ErrorCategory::System, message, true);
        }
        return;
    }
    eprintln!("warning: signal store unavailable for headless session");
    app.emit_error(
        None,
        ErrorCategory::System,
        "Signal store unavailable for headless session".to_string(),
        true,
    );
}

fn build_headless_improve_context(
    config: &FawxConfig,
    flags: &ImproveFlags,
) -> (ImprovementConfig, PathBuf, PathBuf, PathBuf) {
    let data_dir = configured_data_dir(&fawx_data_dir(), config);
    let proposals_dir = data_dir.join("proposals");
    let repo_root = configured_working_dir(config);
    let mut improve_config = ImprovementConfig::default();
    if flags.dry_run {
        improve_config.output_mode = OutputMode::DryRun;
    }
    (improve_config, data_dir, repo_root, proposals_dir)
}

fn headless_review_context(config: &FawxConfig) -> ReviewContext {
    let data_dir = configured_data_dir(&fawx_data_dir(), config);
    ReviewContext {
        proposals_dir: data_dir.join("proposals"),
        working_dir: configured_working_dir(config),
    }
}

fn headless_config_json(
    config: &FawxConfig,
    config_manager: Option<&Arc<Mutex<ConfigManager>>>,
) -> anyhow::Result<serde_json::Value> {
    if let Some(manager) = config_manager {
        let guard = manager
            .lock()
            .map_err(|error| anyhow::anyhow!("config manager lock poisoned: {error}"))?;
        return guard.get("all").map_err(anyhow::Error::msg);
    }
    serde_json::to_value(config).map_err(anyhow::Error::from)
}

fn headless_config_path(
    config: &FawxConfig,
    config_manager: Option<&Arc<Mutex<ConfigManager>>>,
) -> anyhow::Result<PathBuf> {
    if let Some(manager) = config_manager {
        let guard = manager
            .lock()
            .map_err(|error| anyhow::anyhow!("config manager lock poisoned: {error}"))?;
        return Ok(guard.config_path().to_path_buf());
    }
    Ok(configured_data_dir(&fawx_data_dir(), config).join("config.toml"))
}

fn render_headless_config(
    config_path: &std::path::Path,
    data_dir: &std::path::Path,
    active_model: &str,
    json: &serde_json::Value,
) -> anyhow::Result<String> {
    let pretty = serde_json::to_string_pretty(json)?;
    Ok(format!(
        "Config path: {}\nRuntime data dir: {}\nmodel.active = {}\nLoaded values:\n{}",
        config_path.display(),
        data_dir.display(),
        active_model,
        pretty
    ))
}

fn render_analysis_output(
    findings: &[AnalysisFinding],
    memory: Option<&SharedMemoryStore>,
) -> String {
    if findings.is_empty() {
        return "No patterns found. Collect more signals first.".to_string();
    }
    let mut lines = render_analysis_findings(findings);
    let (stored, surfaced, logged) = route_findings_by_confidence(findings, memory);
    lines.push(format!(
        "Wrote {} patterns to memory, surfaced {} for review, logged {}",
        stored, surfaced, logged
    ));
    lines.join("\n")
}

fn render_analysis_findings(findings: &[AnalysisFinding]) -> Vec<String> {
    let mut lines = Vec::new();
    for finding in findings {
        lines.push(format!(
            "{} | {}",
            analysis_confidence_badge(finding.confidence),
            finding.pattern_name
        ));
        lines.push(format!("  {}", finding.description));
        lines.push(format!("  Evidence: {} signals", finding.evidence.len()));
        if let Some(action) = &finding.suggested_action {
            lines.push(format!("  Suggested: {action}"));
        }
        lines.push(String::new());
    }
    lines.push(format!("Found {} patterns total.", findings.len()));
    lines
}

fn route_findings_by_confidence(
    findings: &[AnalysisFinding],
    memory: Option<&SharedMemoryStore>,
) -> (usize, usize, usize) {
    findings
        .iter()
        .fold((0, 0, 0), |counts, finding| match finding.confidence {
            Confidence::High if store_high_confidence_finding(memory, finding) => {
                (counts.0 + 1, counts.1, counts.2)
            }
            Confidence::Medium => (counts.0, counts.1 + 1, counts.2),
            Confidence::Low => (counts.0, counts.1, counts.2 + 1),
            Confidence::High => counts,
        })
}

fn store_high_confidence_finding(
    memory: Option<&SharedMemoryStore>,
    finding: &AnalysisFinding,
) -> bool {
    let Some(memory_store) = memory else {
        return false;
    };
    let Ok(mut store) = memory_store.lock() else {
        return false;
    };
    let key = format!("pattern/{}", finding.pattern_name);
    store.write(&key, &finding.description).is_ok()
}

fn analysis_confidence_badge(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::High => "🔴 HIGH",
        Confidence::Medium => "🟡 MEDIUM",
        Confidence::Low => "🟢 LOW",
    }
}

fn render_improve_output(result: &fx_improve::ImprovementRunResult, dry_run: bool) -> String {
    let mut lines = vec![if dry_run {
        "⚡ Dry run complete.".to_string()
    } else {
        "⚡ Improvement cycle complete.".to_string()
    }];

    if let Some(summary) = render_improve_summary(result) {
        lines.push(summary);
    }
    if improve_result_is_empty(result) {
        lines.push("  No actionable improvements found.".to_string());
        return lines.join("\n");
    }

    lines.extend(
        result
            .proposals_written
            .iter()
            .map(|path| format!("  Proposal: {}", path.display())),
    );
    lines.extend(
        result
            .branches_created
            .iter()
            .map(|branch| format!("  Branch: {branch}")),
    );
    lines.extend(render_skipped_candidates(&result.skipped_candidates));
    lines.extend(
        result
            .skipped
            .iter()
            .map(|(name, reason)| format!("  Skipped: {name} — {reason}")),
    );
    lines.join("\n")
}

fn render_skipped_candidates(skipped_candidates: &[fx_improve::SkippedCandidate]) -> Vec<String> {
    skipped_candidates
        .iter()
        .map(|candidate| {
            format!(
                "  Skipped candidate: {} — {}",
                candidate.name, candidate.reason
            )
        })
        .collect()
}

fn render_improve_summary(result: &fx_improve::ImprovementRunResult) -> Option<String> {
    if result.plans_generated == 0 && result.skipped_candidates.is_empty() {
        return None;
    }

    let mut summary = format!(
        "  {} {} generated",
        result.plans_generated,
        pluralize(result.plans_generated, "plan", "plans")
    );
    if !result.skipped_candidates.is_empty() {
        summary.push_str(&format!(
            ", {}",
            fx_improve::skipped_candidate_summary(&result.skipped_candidates)
        ));
    }
    Some(summary)
}

fn improve_result_is_empty(result: &fx_improve::ImprovementRunResult) -> bool {
    result.plans_generated == 0
        && result.proposals_written.is_empty()
        && result.branches_created.is_empty()
        && result.skipped.is_empty()
        && result.skipped_candidates.is_empty()
}

fn pluralize<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 {
        singular
    } else {
        plural
    }
}

fn new_subagent_session_key(bus: Option<&SessionBus>) -> Result<Option<SessionKey>, SubagentError> {
    if bus.is_none() {
        return Ok(None);
    }
    SessionKey::new(format!("subagent-{}", Uuid::new_v4()))
        .map(Some)
        .map_err(|error| SubagentError::Spawn(error.to_string()))
}

impl HeadlessSubagentFactory {
    pub fn new(deps: HeadlessSubagentFactoryDeps) -> Self {
        Self {
            deps,
            disabled_manager: new_disabled_subagent_manager(),
        }
    }

    fn build_app(
        &self,
        config: &SpawnConfig,
        cancel_token: CancellationToken,
    ) -> Result<HeadlessApp, SubagentError> {
        let mut options = HeadlessLoopBuildOptions::subagent(config.cwd.clone(), cancel_token);
        options.token_broker = self.deps.token_broker.clone();
        let bundle = build_headless_loop_engine_bundle(
            &self.deps.config,
            self.deps.improvement_provider.clone(),
            options,
        )
        .map_err(|error| SubagentError::Spawn(error.to_string()))?;
        let deps = HeadlessAppDeps {
            loop_engine: bundle.engine,
            router: Arc::clone(&self.deps.router),
            runtime_info: bundle.runtime_info,
            config: self.deps.config.clone(),
            memory: bundle.memory,
            embedding_index_persistence: bundle.embedding_index_persistence,
            system_prompt_path: None,
            config_manager: None,
            system_prompt_text: config.system_prompt.clone(),
            subagent_manager: Arc::clone(&self.disabled_manager),
            canary_monitor: None,
            session_bus: self.deps.session_bus.clone(),
            session_key: new_subagent_session_key(self.deps.session_bus.as_ref())?,
            cron_store: None,
            startup_warnings: bundle.startup_warnings,
            permission_callback_slot: bundle.permission_callback_slot,
            ripcord_journal: bundle.ripcord_journal,
            #[cfg(feature = "http")]
            experiment_registry: None,
        };
        let mut app =
            HeadlessApp::new(deps).map_err(|error| SubagentError::Spawn(error.to_string()))?;
        if let Some(model) = &config.model {
            if let Err(error) = app.set_active_model(model) {
                tracing::warn!(model = %model, error = %error, "subagent model override failed");
            }
        }
        if let Some(thinking) = &config.thinking {
            if let Err(error) = app.set_supported_thinking_level(thinking) {
                tracing::warn!(thinking = %thinking, error = %error, "subagent thinking override failed");
            }
        }
        Ok(app)
    }
}

impl std::fmt::Debug for HeadlessSubagentFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessSubagentFactory")
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for HeadlessSubagentSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessSubagentSession")
            .field("active_model", &self.app.active_model)
            .finish()
    }
}

impl SubagentFactory for DisabledSubagentFactory {
    fn create_session(
        &self,
        _config: &SpawnConfig,
    ) -> Result<CreatedSubagentSession, SubagentError> {
        Err(SubagentError::Spawn(
            "nested subagent spawning is disabled".to_string(),
        ))
    }
}

impl SubagentFactory for HeadlessSubagentFactory {
    fn create_session(
        &self,
        config: &SpawnConfig,
    ) -> Result<CreatedSubagentSession, SubagentError> {
        if config.model.is_some() {
            return Err(SubagentError::Spawn(
                "model overrides are not supported with a shared router".to_string(),
            ));
        }
        let cancel_token = CancellationToken::new();
        let app = self.build_app(config, cancel_token.clone())?;
        Ok(CreatedSubagentSession {
            session: Box::new(HeadlessSubagentSession { app }),
            cancel_token,
        })
    }
}

#[async_trait]
impl SubagentSession for HeadlessSubagentSession {
    async fn process_message(&mut self, input: &str) -> Result<SubagentTurn, SubagentError> {
        let result = self
            .app
            .process_message(input)
            .await
            .map_err(|error| SubagentError::Execution(error.to_string()))?;
        Ok(SubagentTurn {
            response: result.response,
            tokens_used: result.tokens_used.total_tokens(),
        })
    }
}

pub async fn process_input_with_commands(
    app: &mut HeadlessApp,
    input: &str,
    source: Option<&InputSource>,
) -> Result<CycleResult, anyhow::Error> {
    if is_command_input(input) {
        return process_command_input(app, input).await;
    }
    match source {
        Some(source) => app.process_message_for_source(input, source).await,
        None => app.process_message(input).await,
    }
}

pub async fn process_input_with_commands_streaming(
    app: &mut HeadlessApp,
    input: &str,
    source: Option<&InputSource>,
    callback: StreamCallback,
) -> Result<CycleResult, anyhow::Error> {
    if is_command_input(input) {
        let result = process_command_input(app, input).await?;
        callback(fx_kernel::StreamEvent::Done {
            response: result.response.clone(),
        });
        return Ok(result);
    }
    match source {
        Some(source) => {
            app.process_message_for_source_streaming(input, source, callback)
                .await
        }
        None => app.process_message_streaming(input, callback).await,
    }
}

async fn process_command_input(
    app: &mut HeadlessApp,
    input: &str,
) -> Result<CycleResult, anyhow::Error> {
    let parsed = parse_command(input);
    let response = match execute_headless_async_command(app, &parsed).await? {
        Some(response) => response,
        None => run_sync_command(app, &parsed)?,
    };
    Ok(command_cycle_result(app, response))
}

fn run_sync_command(
    app: &mut HeadlessApp,
    parsed: &ParsedCommand,
) -> Result<String, anyhow::Error> {
    match execute_command(&mut CommandContext { app }, parsed) {
        Some(result) => result.map(|value| value.response),
        None => Ok(client_only_command_message(parsed)
            .unwrap_or_else(|| "This command is only available in the TUI.".to_string())),
    }
}

async fn execute_headless_async_command(
    app: &mut HeadlessApp,
    parsed: &ParsedCommand,
) -> Result<Option<String>, anyhow::Error> {
    match parsed {
        ParsedCommand::Model(None) => app.list_models_dynamic().await.map(Some),
        ParsedCommand::Analyze => app.analyze_signals_command().await.map(Some),
        ParsedCommand::Improve(flags) => app.improve_command(flags).await.map(Some),
        _ => Ok(None),
    }
}

fn command_cycle_result(app: &HeadlessApp, response: String) -> CycleResult {
    CycleResult {
        response,
        model: app.active_model().to_string(),
        iterations: 0,
        tokens_used: TokenUsage::default(),
    }
}

// ── Free functions ──────────────────────────────────────────────────────────

fn is_quit_command(input: &str) -> bool {
    matches!(input, "/quit" | "/exit")
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn new_disabled_subagent_manager() -> Arc<SubagentManager> {
    Arc::new(SubagentManager::new(SubagentManagerDeps {
        factory: Arc::new(DisabledSubagentFactory),
        limits: SubagentLimits::default(),
    }))
}

/// Load a system prompt from an explicit path or the default location.
///
/// When `explicit_path` is `Some`, only that path is tried. Otherwise
/// the default `~/.fawx/system_prompt.md` is used.
fn load_system_prompt(explicit_path: Option<&std::path::Path>) -> Option<String> {
    let path = match explicit_path {
        Some(p) => p.to_path_buf(),
        None => fawx_data_dir().join("system_prompt.md"),
    };
    std::fs::read_to_string(&path).ok().and_then(
        |s| {
            if s.trim().is_empty() {
                None
            } else {
                Some(s)
            }
        },
    )
}

fn resolve_system_prompt(
    inline_prompt: Option<String>,
    explicit_path: Option<&std::path::Path>,
    data_dir: &Path,
) -> Option<String> {
    let base_prompt = inline_prompt
        .filter(|prompt| !prompt.trim().is_empty())
        .or_else(|| load_system_prompt(explicit_path));
    let context_dir = data_dir.join("context");
    append_context_files(base_prompt, load_context_files(&context_dir))
}

fn append_context_files(
    base_prompt: Option<String>,
    context_files: Option<String>,
) -> Option<String> {
    match (base_prompt, context_files) {
        (Some(prompt), Some(context)) => Some(format!("{prompt}{context}")),
        (Some(prompt), None) => Some(prompt),
        (None, Some(context)) => Some(context),
        (None, None) => None,
    }
}

pub fn resolve_active_model(router: &ModelRouter, config: &FawxConfig) -> anyhow::Result<String> {
    resolve_requested_model(router, config.model.default_model.as_deref())
}

pub fn seed_headless_router_active_model(router: &mut ModelRouter, config: &FawxConfig) {
    let Ok(active_model) = resolve_active_model(router, config) else {
        return;
    };
    if let Err(error) = router.set_active(&active_model) {
        tracing::warn!(
            error = %error,
            model = %active_model,
            "failed to set default model"
        );
    }
}

fn resolve_requested_model(
    router: &ModelRouter,
    configured_default: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(model) = configured_default.filter(|model| !model.is_empty()) {
        return resolve_configured_model_or_fallback(router, model);
    }
    first_runtime_model(router)
}

fn resolve_configured_model_or_fallback(
    router: &ModelRouter,
    configured_model: &str,
) -> anyhow::Result<String> {
    match resolve_headless_model_selector(router, configured_model) {
        Ok(model) => Ok(model),
        Err(error) => {
            tracing::warn!(
                error = %error,
                "configured default_model '{}' not available, falling back",
                configured_model
            );
            first_runtime_model(router)
        }
    }
}

fn first_runtime_model(router: &ModelRouter) -> anyhow::Result<String> {
    router
        .active_model()
        .filter(|model| headless_model_available(router, model))
        .map(ToString::to_string)
        .or_else(|| first_available_model(router))
        .ok_or_else(no_headless_models_available)
}

fn first_available_model(router: &ModelRouter) -> Option<String> {
    router
        .available_models()
        .into_iter()
        .next()
        .map(|model| model.model_id)
}

fn headless_model_available(router: &ModelRouter, model: &str) -> bool {
    router
        .available_models()
        .iter()
        .any(|candidate| candidate.model_id == model)
}

pub(crate) fn no_headless_models_available() -> anyhow::Error {
    anyhow::anyhow!(
        "no models available in router; configure a provider and authenticate it before starting headless mode"
    )
}

/// Reset SIGPIPE to default behavior on Unix so piped output
/// (`fawx serve | head -1`) terminates cleanly instead of producing
/// ugly error messages.
fn install_sigpipe_handler() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn extract_response_text(result: &LoopResult) -> String {
    match result {
        LoopResult::Complete { response, .. } => response.clone(),
        LoopResult::BudgetExhausted {
            partial_response, ..
        } => partial_response.clone().unwrap_or_default(),
        LoopResult::NeedsInput { prompt, .. } => prompt.clone(),
        LoopResult::UserStopped {
            partial_response, ..
        } => partial_response.clone().unwrap_or_default(),
        LoopResult::Error { message, .. } => format!("error: {message}"),
    }
}

fn extract_iterations(result: &LoopResult) -> u32 {
    match result {
        LoopResult::Complete { iterations, .. }
        | LoopResult::BudgetExhausted { iterations, .. }
        | LoopResult::NeedsInput { iterations, .. }
        | LoopResult::UserStopped { iterations, .. } => *iterations,
        LoopResult::Error { .. } => 0,
    }
}

fn extract_token_usage(result: &LoopResult) -> TokenUsage {
    match result {
        LoopResult::Complete { tokens_used, .. } => *tokens_used,
        _ => TokenUsage::default(),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    #[cfg(feature = "http")]
    use fx_api::engine::AppEngine;
    use fx_bus::{BusStore, Payload};
    use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
    use fx_kernel::budget::{BudgetConfig, BudgetTracker};
    use fx_kernel::cancellation::CancellationToken;
    use fx_kernel::context_manager::ContextCompactor;
    use fx_kernel::loop_engine::LoopEngine;
    use fx_session::SessionKey;
    use fx_subagent::SpawnConfig;
    use std::sync::{Arc, Mutex, RwLock};
    use tokio::time::Duration;

    // ── Test helpers ────────────────────────────────────────────────────

    /// Stub tool executor that rejects all calls (no tools in headless tests).
    #[derive(Debug)]
    struct StubToolExecutor;

    #[async_trait]
    impl ToolExecutor for StubToolExecutor {
        async fn execute_tools(
            &self,
            _calls: &[fx_llm::ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(Vec::new())
        }
    }

    fn test_engine() -> LoopEngine {
        LoopEngine::builder()
            .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
            .context(ContextCompactor::new(2048, 256))
            .max_iterations(3)
            .tool_executor(Arc::new(StubToolExecutor))
            .synthesis_instruction("Summarize".to_string())
            .build()
            .expect("test engine")
    }

    fn shared_router(router: ModelRouter) -> SharedModelRouter {
        Arc::new(RwLock::new(router))
    }

    fn router_active_model(router: &SharedModelRouter) -> Option<String> {
        read_router(router, |router| {
            router.active_model().map(ToString::to_string)
        })
    }

    fn router_available_models(router: &SharedModelRouter) -> Vec<ModelInfo> {
        read_router(router, ModelRouter::available_models)
    }

    fn test_app() -> HeadlessApp {
        let mut config = FawxConfig::default();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let data_dir = std::env::temp_dir().join(format!("fawx-headless-tests-{unique}"));
        std::fs::create_dir_all(&data_dir).expect("create temp data dir");
        config.general.data_dir = Some(data_dir);

        HeadlessApp {
            loop_engine: test_engine(),
            router: shared_router(ModelRouter::new()),
            runtime_info: test_runtime_info(),
            config,
            memory: None,
            embedding_index_persistence: None,
            _subagent_manager: new_disabled_subagent_manager(),
            active_model: "mock-model".to_string(),
            conversation_history: Vec::new(),
            last_signals: Vec::new(),
            max_history: 20,
            custom_system_prompt: None,
            canary_monitor: None,
            config_manager: None,
            session_bus: None,
            session_key: None,
            cron_store: None,
            startup_warnings: Vec::new(),
            #[cfg(feature = "http")]
            experiment_registry: None,
            error_history: VecDeque::new(),
            cumulative_tokens: TokenUsage::default(),
            permission_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            bus_receiver: None,
        }
    }

    #[derive(Debug)]
    struct UsageReportingProvider;

    #[async_trait]
    impl fx_llm::CompletionProvider for UsageReportingProvider {
        async fn complete(
            &self,
            _request: fx_llm::CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            Ok(mock_completion_response())
        }

        async fn complete_stream(
            &self,
            _request: fx_llm::CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            let chunk = fx_llm::StreamChunk {
                delta_content: Some(mock_completion_text()),
                tool_use_deltas: Vec::new(),
                usage: Some(fx_llm::Usage {
                    input_tokens: 3,
                    output_tokens: 2,
                }),
                stop_reason: Some("end_turn".to_string()),
            };
            Ok(Box::pin(futures::stream::iter(vec![Ok(chunk)])))
        }

        fn name(&self) -> &str {
            "usage-reporting"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["mock-model".to_string()]
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            fx_llm::ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }
    }

    fn mock_completion_response() -> fx_llm::CompletionResponse {
        fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::Text {
                text: mock_completion_text(),
            }],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 3,
                output_tokens: 2,
            }),
            stop_reason: Some("end_turn".to_string()),
        }
    }

    fn mock_completion_text() -> String {
        r#"{"action":{"Respond":{"text":"ok"}},"rationale":"r","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#.to_string()
    }

    fn mock_completion_usage_total() -> u64 {
        let usage = mock_completion_response()
            .usage
            .expect("mock response should include usage");
        u64::from(usage.input_tokens) + u64::from(usage.output_tokens)
    }

    fn test_router() -> SharedModelRouter {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(UsageReportingProvider));
        router.set_active("mock-model").expect("set active");
        shared_router(router)
    }

    #[derive(Debug)]
    struct StaticModelsProvider {
        name: &'static str,
        models: Vec<&'static str>,
        dynamic_models: Option<Vec<String>>,
    }

    #[async_trait]
    impl fx_llm::CompletionProvider for StaticModelsProvider {
        async fn complete(
            &self,
            _request: fx_llm::CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            Ok(mock_completion_response())
        }

        async fn complete_stream(
            &self,
            _request: fx_llm::CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            let chunk = fx_llm::StreamChunk {
                delta_content: Some(mock_completion_text()),
                stop_reason: Some("end_turn".to_string()),
                ..Default::default()
            };
            Ok(Box::pin(futures::stream::iter(vec![Ok(chunk)])))
        }

        fn name(&self) -> &str {
            self.name
        }

        fn supported_models(&self) -> Vec<String> {
            self.models
                .iter()
                .map(|model| (*model).to_string())
                .collect()
        }

        async fn list_models(&self) -> Result<Vec<String>, fx_llm::ProviderError> {
            Ok(self
                .dynamic_models
                .clone()
                .unwrap_or_else(|| self.supported_models()))
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            fx_llm::ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }
    }

    #[cfg(feature = "http")]
    #[derive(Debug)]
    struct ConversationMemoryProvider;

    #[cfg(feature = "http")]
    #[async_trait]
    impl fx_llm::CompletionProvider for ConversationMemoryProvider {
        async fn complete(
            &self,
            request: fx_llm::CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            Ok(fx_llm::CompletionResponse {
                content: vec![fx_llm::ContentBlock::Text {
                    text: conversation_response(&request),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            })
        }

        async fn complete_stream(
            &self,
            request: fx_llm::CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            let chunk = fx_llm::StreamChunk {
                delta_content: Some(conversation_response(&request)),
                stop_reason: Some("end_turn".to_string()),
                ..Default::default()
            };
            Ok(Box::pin(futures::stream::iter(vec![Ok(chunk)])))
        }

        fn name(&self) -> &str {
            "conversation-memory"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["conversation-memory".to_string()]
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            fx_llm::ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }
    }

    #[cfg(feature = "http")]
    fn conversation_response(request: &fx_llm::CompletionRequest) -> String {
        let response = if request_contains_text(request, "remember cats")
            && request_contains_text(request, "what did I say?")
        {
            "You said remember cats"
        } else {
            "I'll remember cats"
        };
        response_payload(response)
    }

    #[cfg(feature = "http")]
    fn request_contains_text(request: &fx_llm::CompletionRequest, needle: &str) -> bool {
        request.messages.iter().any(|message| {
            message
                .content
                .iter()
                .any(|block| matches!(block, fx_llm::ContentBlock::Text { text } if text == needle))
        })
    }

    #[cfg(feature = "http")]
    fn response_payload(response: &str) -> String {
        serde_json::json!({
            "action": {"Respond": {"text": response}},
            "rationale": "r",
            "confidence": 0.9,
            "expected_outcome": null,
            "sub_goals": []
        })
        .to_string()
    }

    fn static_model_router(models: &[&'static str]) -> ModelRouter {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(StaticModelsProvider {
            name: "static-models",
            models: models.to_vec(),
            dynamic_models: None,
        }));
        router
    }

    fn headless_deps(mut router: ModelRouter, mut config: FawxConfig) -> HeadlessAppDeps {
        if config.general.data_dir.is_none() {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let data_dir = std::env::temp_dir().join(format!("fawx-headless-tests-{unique}"));
            std::fs::create_dir_all(&data_dir).expect("create temp data dir");
            config.general.data_dir = Some(data_dir);
        }

        seed_headless_router_active_model(&mut router, &config);
        HeadlessAppDeps {
            loop_engine: test_engine(),
            router: shared_router(router),
            runtime_info: test_runtime_info(),
            config,
            memory: None,
            embedding_index_persistence: None,
            system_prompt_path: None,
            config_manager: None,
            system_prompt_text: None,
            subagent_manager: new_disabled_subagent_manager(),
            canary_monitor: None,
            session_bus: None,
            session_key: None,
            cron_store: None,
            startup_warnings: Vec::new(),
            permission_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            #[cfg(feature = "http")]
            experiment_registry: None,
        }
    }

    fn test_runtime_info() -> Arc<RwLock<RuntimeInfo>> {
        Arc::new(RwLock::new(RuntimeInfo {
            active_model: String::new(),
            provider: String::new(),
            skills: Vec::new(),
            config_summary: fx_core::runtime_info::ConfigSummary {
                max_iterations: 3,
                max_history: 20,
                memory_enabled: false,
            },
            version: "test".to_string(),
        }))
    }

    fn headless_app_with_router(router: ModelRouter, active_model: &str) -> HeadlessApp {
        let mut app = test_app();
        app.router = shared_router(router);
        app.active_model = active_model.to_string();
        app
    }

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl LogBuffer {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().expect("log buffer lock").clone())
                .expect("log buffer utf8")
        }
    }

    struct LogWriter(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LogWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("log writer lock")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
        type Writer = LogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            LogWriter(self.0.clone())
        }
    }

    fn with_warn_logs<T>(action: impl FnOnce() -> T) -> (T, String) {
        let logs = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .with_writer(logs.clone())
            .finish();
        let result = tracing::subscriber::with_default(subscriber, action);
        (result, logs.contents())
    }

    // ── Unit tests (10) ─────────────────────────────────────────────────

    #[test]
    fn quit_commands_recognized() {
        assert!(is_quit_command("/quit"));
        assert!(is_quit_command("/exit"));
        assert!(!is_quit_command("hello"));
        assert!(!is_quit_command("/help"));
    }

    #[test]
    fn empty_input_not_treated_as_quit() {
        assert!(!is_quit_command(""));
    }

    #[tokio::test]
    async fn headless_model_menu_uses_dynamic_when_available() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(StaticModelsProvider {
            name: "dynamic-models",
            models: vec!["static-model"],
            dynamic_models: Some(
                vec!["dynamic-model"]
                    .into_iter()
                    .map(ToString::to_string)
                    .collect(),
            ),
        }));
        let mut app = headless_app_with_router(router, "dynamic-model");

        let rendered = process_command_input(&mut app, "/model").await;
        let rendered = rendered.expect("command result").response;

        assert!(rendered.contains("dynamic-model"));
        assert!(!rendered.contains("static-model (api_key)"));
    }

    #[test]
    fn list_models_uses_shared_renderer() {
        let mut app = test_app();
        app.router = test_router();
        app.active_model = "mock-model".to_string();

        let available_models = router_available_models(&app.router);

        assert_eq!(
            app.list_models(),
            render_model_menu_text(Some("mock-model"), &available_models)
        );
    }

    #[test]
    fn recent_errors_is_bounded_to_fifty_records() {
        let warnings = (0..55)
            .map(|idx| StartupWarning {
                category: ErrorCategory::System,
                message: format!("warning #{idx}"),
            })
            .collect();
        let mut deps = headless_deps(static_model_router(&["mock-model"]), FawxConfig::default());
        deps.startup_warnings = warnings;

        let app = HeadlessApp::new(deps).expect("app");
        let recent = app.recent_errors(100);

        assert_eq!(recent.len(), 50);
        assert_eq!(recent[0].message, "warning #54");
        assert_eq!(recent[49].message, "warning #5");
    }

    #[tokio::test]
    async fn streaming_message_emits_startup_warnings_before_done() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let callback_events = Arc::clone(&events);
        let mut app = test_app();
        let mut router = static_model_router(&["mock-model"]);
        router.set_active("mock-model").expect("set active");
        app.router = shared_router(router);
        app.startup_warnings.push(StartupWarning {
            category: ErrorCategory::Memory,
            message: "Failed to initialize memory: broken store".to_string(),
        });

        let result = app
            .process_message_streaming(
                "hello",
                Arc::new(move |event| {
                    callback_events.lock().expect("events lock").push(event);
                }),
            )
            .await
            .expect("streaming result");

        assert_eq!(result.response, mock_completion_text());
        let events = events.lock().expect("events lock");
        assert!(matches!(
            events.first(),
            Some(StreamEvent::Error {
                category: ErrorCategory::Memory,
                message,
                recoverable: true,
            }) if message == "Failed to initialize memory: broken store"
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::Done { response }) if *response == mock_completion_text()
        ));
    }

    #[test]
    fn proposal_commands_propagate_errors() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp_dir.path().join("proposals"), "not a directory")
            .expect("write broken proposals path");
        let mut app = test_app();
        app.config.general.data_dir = Some(temp_dir.path().to_path_buf());

        assert!(CommandHost::proposals(&app, None).is_err());
        assert!(CommandHost::approve(&app, "1", false).is_err());
        assert!(CommandHost::reject(&app, "1").is_err());
    }

    #[test]
    fn show_config_includes_active_model_line() {
        let mut app = test_app();
        app.active_model = "runtime-model".to_string();
        app.config.model.default_model = Some("config-model".to_string());

        let rendered = app.show_config().expect("show config");

        assert!(rendered.contains("model.active = runtime-model"));
        assert!(rendered.contains("\"default_model\": \"config-model\""));
    }

    #[test]
    fn reload_config_updates_active_model_from_reloaded_config() {
        #[derive(Debug)]
        struct ReloadProvider;

        #[async_trait]
        impl fx_llm::CompletionProvider for ReloadProvider {
            async fn complete(
                &self,
                _request: fx_llm::CompletionRequest,
            ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
                Ok(mock_completion_response())
            }

            async fn complete_stream(
                &self,
                _request: fx_llm::CompletionRequest,
            ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
                let chunk = fx_llm::StreamChunk {
                    delta_content: Some(mock_completion_text()),
                    stop_reason: Some("end_turn".to_string()),
                    ..Default::default()
                };
                Ok(Box::pin(futures::stream::iter(vec![Ok(chunk)])))
            }

            fn name(&self) -> &str {
                "reload-provider"
            }

            fn supported_models(&self) -> Vec<String> {
                vec!["claude-opus-4-6".to_string(), "gpt-5.4".to_string()]
            }

            fn capabilities(&self) -> fx_llm::ProviderCapabilities {
                fx_llm::ProviderCapabilities {
                    supports_temperature: false,
                    requires_streaming: false,
                }
            }
        }

        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("config.toml"),
            "[general]\nthinking = \"low\"\n\n[model]\ndefault_model = \"claude-opus-4-6\"\n",
        )
        .expect("write initial config");
        let manager = Arc::new(Mutex::new(
            ConfigManager::new(temp.path()).expect("config manager"),
        ));
        let mut config = FawxConfig::load(temp.path()).expect("load config");
        config.general.data_dir = Some(temp.path().to_path_buf());

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(ReloadProvider));
        router.set_active("claude-opus-4-6").expect("set old model");

        let mut app = HeadlessApp {
            loop_engine: test_engine(),
            router: shared_router(router),
            runtime_info: test_runtime_info(),
            config,
            memory: None,
            embedding_index_persistence: None,
            _subagent_manager: new_disabled_subagent_manager(),
            active_model: "claude-opus-4-6".to_string(),
            conversation_history: Vec::new(),
            last_signals: Vec::new(),
            max_history: 20,
            custom_system_prompt: None,
            canary_monitor: None,
            config_manager: Some(manager),
            session_bus: None,
            session_key: None,
            cron_store: None,
            startup_warnings: Vec::new(),
            #[cfg(feature = "http")]
            experiment_registry: None,
            error_history: VecDeque::new(),
            cumulative_tokens: TokenUsage::default(),
            permission_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            bus_receiver: None,
        };

        std::fs::write(
            temp.path().join("config.toml"),
            "[general]\nthinking = \"low\"\n\n[model]\ndefault_model = \"gpt-5.4\"\n",
        )
        .expect("write updated config");

        let response = app.reload_config().expect("reload config");

        assert_eq!(app.active_model, "gpt-5.4");
        assert_eq!(router_active_model(&app.router).as_deref(), Some("gpt-5.4"));
        assert_eq!(
            response,
            crate::commands::slash::config_reload_success_message(&temp.path().join("config.toml"))
        );
    }

    #[test]
    fn reload_providers_adds_new_models() {
        let mut app = headless_app_with_router(ModelRouter::new(), "");

        app.apply_reloaded_router(static_model_router(&["gpt-5.4"]))
            .expect("reload providers");

        let models = router_available_models(&app.router);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "gpt-5.4");
    }

    #[test]
    fn reload_providers_removes_deleted_provider() {
        let mut router = static_model_router(&["gpt-5.4"]);
        router.set_active("gpt-5.4").expect("set active");
        let mut app = headless_app_with_router(router, "gpt-5.4");

        app.apply_reloaded_router(ModelRouter::new())
            .expect("reload providers");

        assert!(router_available_models(&app.router).is_empty());
        assert!(app.active_model.is_empty());
        assert_eq!(router_active_model(&app.router), None);
    }

    #[test]
    fn reload_providers_preserves_active_model() {
        let mut router = static_model_router(&["gpt-5.4", "gpt-5.4-mini"]);
        router.set_active("gpt-5.4-mini").expect("set active");
        let mut app = headless_app_with_router(router, "gpt-5.4-mini");

        app.apply_reloaded_router(static_model_router(&["gpt-5.4", "gpt-5.4-mini"]))
            .expect("reload providers");

        assert_eq!(app.active_model, "gpt-5.4-mini");
        assert_eq!(
            router_active_model(&app.router).as_deref(),
            Some("gpt-5.4-mini")
        );
    }

    #[test]
    fn reload_providers_auto_selects_when_empty() {
        let mut app = headless_app_with_router(ModelRouter::new(), "");

        app.apply_reloaded_router(static_model_router(&["claude-opus-4-6"]))
            .expect("reload providers");

        assert_eq!(app.active_model, "claude-opus-4-6");
        assert_eq!(
            router_active_model(&app.router).as_deref(),
            Some("claude-opus-4-6")
        );
    }

    #[test]
    fn show_status_deduplicates_available_model_providers() {
        #[derive(Debug)]
        struct MultiModelProvider;

        #[async_trait]
        impl fx_llm::CompletionProvider for MultiModelProvider {
            async fn complete(
                &self,
                _request: fx_llm::CompletionRequest,
            ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
                Ok(mock_completion_response())
            }

            async fn complete_stream(
                &self,
                _request: fx_llm::CompletionRequest,
            ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
                let chunk = fx_llm::StreamChunk {
                    delta_content: Some(mock_completion_text()),
                    stop_reason: Some("end_turn".to_string()),
                    ..Default::default()
                };
                Ok(Box::pin(futures::stream::iter(vec![Ok(chunk)])))
            }

            fn name(&self) -> &str {
                "usage-reporting"
            }

            fn supported_models(&self) -> Vec<String> {
                vec!["mock-model".to_string(), "mock-model-2".to_string()]
            }

            fn capabilities(&self) -> fx_llm::ProviderCapabilities {
                fx_llm::ProviderCapabilities {
                    supports_temperature: false,
                    requires_streaming: false,
                }
            }
        }

        let mut app = test_app();
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(MultiModelProvider));
        router.set_active("mock-model").expect("set active");
        app.router = shared_router(router);

        let status = app.show_status();
        assert!(status.contains("providers: usage-reporting"));
        assert!(!status.contains("providers: usage-reporting, usage-reporting"));
    }

    #[test]
    fn auth_provider_statuses_include_saved_non_model_credentials() {
        let statuses = auth_provider_statuses(
            Vec::new(),
            vec![StoredAuthProviderEntry {
                provider: "github".to_string(),
                auth_method: "api_key".to_string(),
            }],
        );

        assert_eq!(
            statuses,
            vec![AuthProviderStatus {
                provider: "github".to_string(),
                auth_methods: BTreeSet::from(["api_key".to_string()]),
                model_count: 0,
                status: "saved".to_string(),
            }]
        );
    }

    #[test]
    fn auth_provider_statuses_prefer_registered_status_when_models_exist() {
        let statuses = auth_provider_statuses(
            vec![ModelInfo {
                model_id: "gpt-4o".to_string(),
                provider_name: "openai".to_string(),
                auth_method: "oauth".to_string(),
            }],
            vec![
                StoredAuthProviderEntry {
                    provider: "github".to_string(),
                    auth_method: "api_key".to_string(),
                },
                StoredAuthProviderEntry {
                    provider: "openai".to_string(),
                    auth_method: "oauth".to_string(),
                },
            ],
        );

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].provider, "github");
        assert_eq!(statuses[0].status, "saved");
        assert_eq!(statuses[1].provider, "openai");
        assert_eq!(statuses[1].status, "registered");
        assert_eq!(statuses[1].model_count, 1);
    }

    #[test]
    fn auth_provider_statuses_keep_github_saved_status_when_models_exist() {
        let statuses = auth_provider_statuses(
            vec![ModelInfo {
                model_id: "gpt-4o-mini".to_string(),
                provider_name: "github".to_string(),
                auth_method: "api_key".to_string(),
            }],
            vec![StoredAuthProviderEntry {
                provider: "github".to_string(),
                auth_method: "api_key".to_string(),
            }],
        );

        assert_eq!(
            statuses,
            vec![AuthProviderStatus {
                provider: "github".to_string(),
                auth_methods: BTreeSet::from(["api_key".to_string()]),
                model_count: 1,
                status: "saved".to_string(),
            }]
        );
    }

    #[test]
    fn json_input_parses_message() {
        let app = test_app();
        let result = app.parse_json_input(r#"{"message": "hello world"}"#);
        assert_eq!(result.unwrap(), "hello world");
    }

    #[test]
    fn json_input_rejects_invalid() {
        let app = test_app();
        assert!(app.parse_json_input("not json").is_err());
    }

    #[test]
    fn json_output_serializes_correctly() {
        let output = JsonOutput {
            response: "hello".to_string(),
            model: "gpt-4".to_string(),
            iterations: 2,
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&output).unwrap()).unwrap();
        assert_eq!(json["response"], "hello");
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["iterations"], 2);
    }

    #[test]
    fn render_improve_output_includes_skipped_candidate_summary() {
        let result = fx_improve::ImprovementRunResult {
            plans_generated: 2,
            proposals_written: vec![PathBuf::from("/tmp/proposal.md")],
            branches_created: Vec::new(),
            skipped: Vec::new(),
            skipped_candidates: vec![fx_improve::SkippedCandidate {
                name: "timeout-loop".to_string(),
                reason: "model did not produce a plan".to_string(),
            }],
        };

        let rendered = render_improve_output(&result, false);

        assert!(rendered
            .contains("2 plans generated, 1 candidate skipped (model did not produce a plan)"));
        assert!(rendered.contains("Skipped candidate: timeout-loop — model did not produce a plan"));
    }

    #[tokio::test]
    async fn process_input_with_commands_handles_server_side_status() {
        let mut app = test_app();

        let result = process_input_with_commands(&mut app, "/status", None)
            .await
            .expect("process status command");

        assert_eq!(result.iterations, 0);
        assert!(result.response.contains("Fawx Status"));
    }

    #[tokio::test]
    async fn process_input_with_commands_returns_client_only_message_for_quit() {
        let mut app = test_app();

        let result = process_input_with_commands(&mut app, "/quit", None)
            .await
            .expect("process quit command");

        assert_eq!(result.iterations, 0);
        assert_eq!(
            result.response,
            "/quit is a client-side command (only available in the TUI)"
        );
    }

    #[tokio::test]
    async fn history_and_new_commands_work_in_headless_mode() {
        let mut app = test_app();
        app.conversation_history
            .push(Message::user("hello".to_string()));
        app.conversation_history
            .push(Message::assistant("hi".to_string()));

        let history = process_input_with_commands(&mut app, "/history", None)
            .await
            .expect("process history command");
        assert_eq!(
            history.response,
            "Conversation history: 2 messages in current session"
        );

        let new_conversation = process_input_with_commands(&mut app, "/new", None)
            .await
            .expect("process new command");
        assert_eq!(new_conversation.response, "Started a new conversation.");
        assert!(app.conversation_history.is_empty());
    }

    #[tokio::test]
    async fn loop_and_debug_commands_work_in_headless_mode() {
        let mut app = test_app();
        app.last_signals.push(Signal {
            step: fx_core::signals::LoopStep::Act,
            kind: fx_core::signals::SignalKind::Friction,
            message: "tool timed out".to_string(),
            metadata: serde_json::Value::Null,
            timestamp_ms: 42,
        });

        let loop_status = process_input_with_commands(&mut app, "/loop", None)
            .await
            .expect("process loop command");
        assert!(loop_status.response.contains("Loop status:"));

        let debug = process_input_with_commands(&mut app, "/debug", None)
            .await
            .expect("process debug command");
        assert_eq!(debug.response, "[Act/Friction] tool timed out (42)");
    }

    #[tokio::test]
    async fn synthesis_command_updates_headless_loop_instruction() {
        let mut app = test_app();

        let updated = process_input_with_commands(&mut app, "/synthesis Be concise", None)
            .await
            .expect("process synthesis update");
        assert_eq!(
            updated.response,
            "Synthesis instruction updated: Be concise"
        );
        assert_eq!(app.loop_engine.synthesis_instruction(), "Be concise");

        let reset = process_input_with_commands(&mut app, "/synthesis reset", None)
            .await
            .expect("process synthesis reset");
        assert_eq!(reset.response, "Synthesis instruction reset to default.");
        assert_eq!(
            app.loop_engine.synthesis_instruction(),
            DEFAULT_SYNTHESIS_INSTRUCTION
        );
    }

    #[tokio::test]
    async fn auth_sign_and_keys_commands_work_in_headless_mode() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("trusted_keys")).expect("trusted keys dir");
        std::fs::write(
            temp.path().join("trusted_keys").join("demo.pub"),
            [7_u8; 32],
        )
        .expect("write trusted key");

        let mut app = test_app();
        app.config.general.data_dir = Some(temp.path().to_path_buf());
        app.router = test_router();

        let auth = process_input_with_commands(&mut app, "/auth", None)
            .await
            .expect("process auth command");
        assert!(auth.response.contains("Configured credentials:"));
        assert!(auth.response.contains("usage-reporting"));

        let auth_status =
            process_input_with_commands(&mut app, "/auth usage-reporting show-status", None)
                .await
                .expect("process auth show-status command");
        assert!(auth_status.response.contains("Status: registered"));

        let auth_write =
            process_input_with_commands(&mut app, "/auth github set-token ghp_test", None)
                .await
                .expect("process auth write command");
        assert_eq!(
            auth_write.response,
            "Use `fawx setup` to manage credentials."
        );

        let keys = process_input_with_commands(&mut app, "/keys list", None)
            .await
            .expect("process keys command");
        assert!(keys.response.contains("Trusted public keys:"));
        assert!(keys.response.contains("demo.pub"));

        let keys_redirect = process_input_with_commands(&mut app, "/keys generate", None)
            .await
            .expect("process keys redirect command");
        assert_eq!(
            keys_redirect.response,
            "Use `fawx keys generate` CLI for key management."
        );

        let sign = process_input_with_commands(&mut app, "/sign demo", None)
            .await
            .expect("process sign command");
        assert_eq!(
            sign.response,
            "Use `fawx sign <skill>` CLI to sign WASM packages."
        );
    }

    #[cfg(feature = "http")]
    #[tokio::test]
    async fn http_process_message_preserves_history_for_implicit_message_endpoint() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(ConversationMemoryProvider));
        router
            .set_active("conversation-memory")
            .expect("set active conversation model");

        let mut app = test_app();
        app.router = shared_router(router);
        app.active_model = "conversation-memory".to_string();

        let first = AppEngine::process_message(
            &mut app,
            "remember cats",
            Vec::new(),
            InputSource::Http,
            None,
        )
        .await
        .expect("first http message");
        assert!(first.response.contains("I'll remember cats"));
        assert_eq!(app.conversation_history.len(), 2);

        let second = AppEngine::process_message(
            &mut app,
            "what did I say?",
            Vec::new(),
            InputSource::Http,
            None,
        )
        .await
        .expect("second http message");
        assert!(second.response.contains("You said remember cats"));
        assert_eq!(app.conversation_history.len(), 4);
    }

    #[tokio::test]
    async fn process_message_reports_token_counts() {
        let mut app = HeadlessApp {
            loop_engine: test_engine(),
            router: test_router(),
            runtime_info: test_runtime_info(),
            config: FawxConfig::default(),
            memory: None,
            embedding_index_persistence: None,
            _subagent_manager: new_disabled_subagent_manager(),
            active_model: "mock-model".to_string(),
            conversation_history: Vec::new(),
            last_signals: Vec::new(),
            max_history: 20,
            custom_system_prompt: None,
            canary_monitor: None,
            config_manager: None,
            session_bus: None,
            session_key: None,
            cron_store: None,
            startup_warnings: Vec::new(),
            #[cfg(feature = "http")]
            experiment_registry: None,
            error_history: VecDeque::new(),
            cumulative_tokens: TokenUsage::default(),
            permission_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            bus_receiver: None,
        };

        let result = app.process_message("hello").await.expect("process message");

        assert_eq!(result.model, "mock-model");
        assert!(result.iterations > 0);
        assert!(result.tokens_used.total_tokens() >= mock_completion_usage_total());
    }

    #[tokio::test]
    async fn process_message_updates_canary_monitor() {
        let mut app = HeadlessApp {
            loop_engine: test_engine(),
            router: test_router(),
            runtime_info: test_runtime_info(),
            config: FawxConfig::default(),
            memory: None,
            embedding_index_persistence: None,
            _subagent_manager: new_disabled_subagent_manager(),
            active_model: "mock-model".to_string(),
            conversation_history: Vec::new(),
            last_signals: Vec::new(),
            max_history: 20,
            custom_system_prompt: None,
            canary_monitor: Some(
                CanaryMonitor::new(
                    fx_canary::CanaryConfig {
                        min_signals_for_baseline: 1,
                        ..fx_canary::CanaryConfig::default()
                    },
                    None,
                )
                .with_intervals(1, 1),
            ),
            config_manager: None,
            session_bus: None,
            session_key: None,
            cron_store: None,
            startup_warnings: Vec::new(),
            #[cfg(feature = "http")]
            experiment_registry: None,
            error_history: VecDeque::new(),
            cumulative_tokens: TokenUsage::default(),
            permission_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            bus_receiver: None,
        };

        app.process_message("hello")
            .await
            .expect("process message should succeed");

        assert!(app
            .canary_monitor
            .as_ref()
            .expect("canary monitor")
            .baseline_captured());
    }

    #[test]
    fn extract_response_from_complete() {
        let result = LoopResult::Complete {
            response: "done".to_string(),
            iterations: 1,
            tokens_used: fx_kernel::act::TokenUsage {
                input_tokens: 3,
                output_tokens: 2,
            },
            learnings: Vec::new(),
            signals: Vec::new(),
        };
        assert_eq!(extract_response_text(&result), "done");
        assert_eq!(extract_iterations(&result), 1);
        assert_eq!(extract_token_usage(&result).total_tokens(), 5);
    }

    #[test]
    fn extract_response_from_error() {
        let result = LoopResult::Error {
            message: "boom".to_string(),
            recoverable: false,
            signals: Vec::new(),
        };
        assert_eq!(extract_response_text(&result), "error: boom");
        assert_eq!(extract_iterations(&result), 0);
    }

    #[test]
    fn extract_response_from_budget_exhausted() {
        let result = LoopResult::BudgetExhausted {
            partial_response: Some("partial".to_string()),
            iterations: 3,
            signals: Vec::new(),
        };
        assert_eq!(extract_response_text(&result), "partial");
        assert_eq!(extract_iterations(&result), 3);
    }

    #[test]
    fn perception_snapshot_has_correct_app_id() {
        let app = test_app();
        let source = InputSource::Text;
        let snap = app.build_perception_snapshot("hi", &source);
        assert_eq!(snap.active_app, "fawx.headless");
        assert_eq!(snap.screen.current_app, "fawx.headless");
        assert_eq!(
            snap.user_input.as_ref().map(|u| u.text.as_str()),
            Some("hi")
        );
    }

    #[test]
    fn perception_snapshot_clones_borrowed_channel_source() {
        let app = test_app();
        let source = InputSource::Channel("telegram".to_string());
        let snap = app.build_perception_snapshot("hi", &source);

        assert_eq!(source, InputSource::Channel("telegram".to_string()));
        assert_eq!(snap.user_input.as_ref().map(|u| &u.source), Some(&source));
    }

    #[test]
    fn conversation_history_trimmed() {
        let mut app = test_app();
        app.max_history = 4;
        for i in 0..10 {
            app.record_turn(&format!("q{i}"), &format!("a{i}"));
        }
        // max_history = 4 means 4 messages retained (2 turns)
        assert_eq!(app.conversation_history.len(), 4);
    }

    #[test]
    fn system_prompt_missing_file_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nonexistent_prompt.md");
        assert!(load_system_prompt(Some(&missing)).is_none());
    }

    #[test]
    fn system_prompt_loads_from_explicit_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("custom_prompt.md");
        std::fs::write(&path, "You are a helpful assistant.").expect("write");
        let prompt = load_system_prompt(Some(&path));
        assert_eq!(prompt.as_deref(), Some("You are a helpful assistant."));
    }

    #[test]
    fn system_prompt_empty_file_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty_prompt.md");
        std::fs::write(&path, "   \n  ").expect("write");
        assert!(load_system_prompt(Some(&path)).is_none());
    }

    #[test]
    fn resolve_system_prompt_prefers_inline_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prompt.md");
        std::fs::write(&path, "from file").expect("write");
        let prompt = resolve_system_prompt(Some("inline".to_string()), Some(&path), dir.path());
        assert_eq!(prompt.as_deref(), Some("inline"));
    }

    #[test]
    fn new_appends_context_files_to_system_prompt() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let data_dir = temp_dir.path().join(".fawx");
        std::fs::create_dir_all(data_dir.join("context")).expect("context dir");
        std::fs::write(data_dir.join("context").join("SOUL.md"), "be helpful")
            .expect("write context");

        let mut config = FawxConfig::default();
        config.general.data_dir = Some(data_dir);

        let mut deps = headless_deps(static_model_router(&["test-model"]), config);
        deps.system_prompt_text = Some("base prompt".to_string());

        let app = HeadlessApp::new(deps).expect("should build");
        let prompt = app.custom_system_prompt.clone().expect("system prompt");

        assert!(prompt.starts_with("base prompt"));
        assert!(prompt.contains("--- SOUL.md ---\nbe helpful\n"));
    }

    #[test]
    fn new_uses_available_config_default_model() {
        let mut router = static_model_router(&["router-model", "config-model"]);
        router
            .set_active("router-model")
            .expect("set active should work");

        let mut config = FawxConfig::default();
        config.model.default_model = Some("config-model".to_string());

        let app = HeadlessApp::new(headless_deps(router, config)).expect("should build");

        assert_eq!(app.active_model, "config-model");
        assert_eq!(
            router_active_model(&app.router).as_deref(),
            Some("config-model")
        );
    }

    #[test]
    fn active_model_remains_set_after_router_arc_is_cloned() {
        let mut router = static_model_router(&["router-model", "config-model"]);
        let mut config = FawxConfig::default();
        config.model.default_model = Some("config-model".to_string());

        let active_model = resolve_active_model(&router, &config).expect("resolve active model");
        router
            .set_active(&active_model)
            .expect("set active before Arc sharing");

        let router = shared_router(router);
        let cloned_router = Arc::clone(&router);

        assert_eq!(
            router_active_model(&router).as_deref(),
            Some("config-model")
        );
        assert_eq!(
            router_active_model(&cloned_router).as_deref(),
            Some("config-model")
        );
    }

    #[test]
    fn new_falls_back_when_config_default_model_is_unavailable() {
        let router = static_model_router(&["z-model", "a-model"]);
        let mut config = FawxConfig::default();
        config.model.default_model = Some("missing-model".to_string());

        let (app, logs) = with_warn_logs(|| {
            HeadlessApp::new(headless_deps(router, config)).expect("should build")
        });

        assert_eq!(app.active_model, "a-model");
        assert_eq!(router_active_model(&app.router).as_deref(), Some("a-model"));
        assert!(
            logs.contains("configured default_model 'missing-model' not available, falling back")
        );
        assert!(logs.contains("error=model not found: missing-model"));
    }

    #[test]
    fn new_treats_empty_config_default_model_like_none() {
        let router = static_model_router(&["z-model", "a-model"]);
        let mut config = FawxConfig::default();
        config.model.default_model = Some(String::new());

        let (app, logs) = with_warn_logs(|| {
            HeadlessApp::new(headless_deps(router, config)).expect("should build")
        });

        assert_eq!(app.active_model, "a-model");
        assert_eq!(router_active_model(&app.router).as_deref(), Some("a-model"));
        assert!(logs.is_empty(), "unexpected warnings: {logs}");
    }

    #[test]
    fn sync_headless_model_from_config_updates_active_model() {
        let mut router = static_model_router(&["old-model", "new-model"]);
        router.set_active("old-model").expect("set active");
        let mut app = headless_app_with_router(router, "old-model");

        sync_headless_model_from_config(&mut app, Some("new-model".to_string())).expect("sync");

        assert_eq!(app.active_model, "new-model");
        assert_eq!(
            router_active_model(&app.router).as_deref(),
            Some("new-model")
        );
    }

    #[test]
    fn sync_headless_model_from_config_falls_back_gracefully() {
        let mut router = static_model_router(&["old-model", "new-model"]);
        router.set_active("old-model").expect("set active");
        let mut app = headless_app_with_router(router, "old-model");

        let (_, logs) = with_warn_logs(|| {
            sync_headless_model_from_config(&mut app, Some("missing-model".to_string()))
                .expect("sync");
        });

        assert_eq!(app.active_model, "old-model");
        assert_eq!(
            router_active_model(&app.router).as_deref(),
            Some("old-model")
        );
        assert!(
            logs.contains("configured default_model 'missing-model' not available, falling back")
        );
    }

    #[test]
    fn new_uses_first_available_model_when_config_default_missing() {
        let router = static_model_router(&["z-model", "a-model"]);
        let config = FawxConfig::default();

        let app = HeadlessApp::new(headless_deps(router, config)).expect("should build");

        assert_eq!(app.active_model, "a-model");
        assert_eq!(router_active_model(&app.router).as_deref(), Some("a-model"));
    }

    #[test]
    fn new_overrides_preselected_router_model_with_config_default_model() {
        let mut router = static_model_router(&["router-model", "config-model"]);
        router
            .set_active("router-model")
            .expect("set active should work");

        let mut config = FawxConfig::default();
        config.model.default_model = Some("config-model".to_string());

        let app = HeadlessApp::new(headless_deps(router, config)).expect("should build");

        assert_eq!(app.active_model, "config-model");
        assert_eq!(
            router_active_model(&app.router).as_deref(),
            Some("config-model")
        );
    }

    #[test]
    fn new_succeeds_when_no_models_are_available() {
        let (app, logs) = with_warn_logs(|| {
            HeadlessApp::new(headless_deps(ModelRouter::new(), FawxConfig::default()))
                .expect("should build without models")
        });

        assert_eq!(app.active_model(), "");
        assert_eq!(router_active_model(&app.router).as_deref(), None);
        assert!(logs.contains(NO_AI_PROVIDERS_STARTUP_WARNING));
        assert!(app.recent_errors(1).iter().any(|warning| {
            warning.category == ErrorCategory::System
                && warning.message == NO_AI_PROVIDERS_STARTUP_WARNING
        }));
    }

    #[tokio::test]
    async fn subagent_sends_status_to_parent() {
        let store = BusStore::new(fx_storage::Storage::open_in_memory().expect("in-memory bus"));
        let bus = SessionBus::new(store);
        let parent_key = SessionKey::new("parent-session").expect("parent key");
        let mut parent_receiver = bus.subscribe(&parent_key);
        let factory = HeadlessSubagentFactory::new(HeadlessSubagentFactoryDeps {
            router: test_router(),
            config: FawxConfig::default(),
            improvement_provider: None,
            session_bus: Some(bus.clone()),
            token_broker: None,
        });
        let app = factory
            .build_app(&SpawnConfig::new("report status"), CancellationToken::new())
            .expect("build app");
        let child_key = app.session_key.clone().expect("child session key");
        let result = app
            .session_bus()
            .expect("child session bus")
            .send(Envelope::new(
                Some(child_key.clone()),
                parent_key.clone(),
                Payload::StatusUpdate {
                    task_id: "task-1".to_string(),
                    progress: "50%".to_string(),
                },
            ))
            .await
            .expect("send status");
        let envelope = tokio::time::timeout(Duration::from_secs(1), parent_receiver.recv())
            .await
            .expect("status update should arrive")
            .expect("parent receiver should stay open");

        assert!(result.delivered);
        assert_eq!(envelope.from, Some(child_key));
        assert_eq!(envelope.to, parent_key);
        assert_eq!(
            envelope.payload,
            Payload::StatusUpdate {
                task_id: "task-1".to_string(),
                progress: "50%".to_string(),
            }
        );
    }

    #[test]
    fn headless_subagent_factory_new_builds_disabled_manager() {
        let deps = HeadlessSubagentFactoryDeps {
            router: shared_router(ModelRouter::new()),
            config: FawxConfig::default(),
            improvement_provider: None,
            session_bus: None,
            token_broker: None,
        };
        let factory = HeadlessSubagentFactory::new(deps);
        let debug = format!("{factory:?}");
        assert!(debug.contains("HeadlessSubagentFactory"));
    }
}
