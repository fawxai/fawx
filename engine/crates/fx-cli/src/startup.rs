use crate::auth_store::{migrate_if_needed, AuthStore};
use crate::headless::StartupWarning;
use crate::helpers::{format_memory_for_prompt, thinking_config_for_active_model};
use async_trait::async_trait;
use chrono::NaiveDate;
use fx_analysis::AnalysisError;
#[cfg(feature = "http")]
use fx_api::experiment_bridge::RegistryBridge;
#[cfg(feature = "http")]
use fx_api::SharedExperimentRegistry;
use fx_auth::auth::{AuthManager, AuthMethod};
use fx_auth::credential_store::CredentialStore as CredentialStoreTrait;
use fx_bus::{BusStore, SessionBus};
use fx_config::manager::ConfigManager;
use fx_config::{
    parse_log_level as parse_config_log_level, FawxConfig, ImprovementToolsConfig, LoggingConfig,
};
use fx_consensus::ProgressCallback;
use fx_core::memory::{MemoryProvider, MemoryStore};
use fx_core::runtime_info::{ConfigSummary, RuntimeInfo, SkillInfo};
use fx_core::EventBus;
use fx_embeddings::EmbeddingModel;
use fx_fleet::{NodeRegistry, NodeTransport, SshTransport};
use fx_journal::JournalSkill;
use fx_kernel::act::ToolExecutor;
use fx_kernel::budget::{BudgetConfig, BudgetTracker};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::context_manager::ContextCompactor;
use fx_kernel::loop_engine::{LoopEngine, LoopEngineBuilder, ScratchpadProvider};
use fx_kernel::streaming::{StreamCallback, StreamEvent};
use fx_kernel::ErrorCategory;
use fx_kernel::{
    CachingExecutor, PermissionGateExecutor, PermissionPolicy, PermissionPromptState,
    ProcessConfig, ProcessRegistry, ProposalGateExecutor, ProposalGateState,
};
use fx_llm::{
    AnthropicProvider, CompletionRequest, ModelRouter, OpenAiProvider, OpenAiResponsesProvider,
};
use fx_loadable::{
    NotificationSender, NotifySkill, SessionMemorySkill, SignaturePolicy, SkillRegistry,
    TransactionSkill,
};
use fx_memory::embedding_index::EmbeddingIndex;
use fx_memory::{JsonFileMemory, JsonMemoryConfig, SignalStore};
use fx_ripcord::{resolve_tripwires, RipcordJournal, TripwireEvaluator};
use fx_scratchpad::skill::ScratchpadSkill;
use fx_scratchpad::Scratchpad;
use fx_skills::live_host_api::CredentialProvider;
use fx_subagent::SubagentControl;
use fx_tools::{
    BuiltinToolsSkill, ExperimentToolState, FawxToolExecutor, GitSkill, ImprovementToolsState,
    NodeRunState, SessionToolsSkill, ToolConfig,
};
use std::fmt;
use std::fs;
use std::io;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::Dispatch;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::prelude::*;

const DEFAULT_CONTEXT_MAX_TOKENS: usize = 8_000;
const DEFAULT_CONTEXT_COMPACT_TARGET: usize = 6_000;
const DEFAULT_SYNTHESIS_INSTRUCTION: &str =
    "Use the tool output to directly answer the user's question. Be natural and specific — \
 don't dump raw tool output, but don't hide data either. Match your response format to what \
 the user asked for: if they asked for a specific format (e.g., a count, a timestamp, a \
 raw value), use exactly that format — do not reformat into a 'friendlier' version unless \
 explicitly asked. If they asked a simple question, give a simple answer. If they asked \
 for a listing or search results, present it cleanly formatted.";
const DEFAULT_ANTHROPIC_MODELS: &[&str] = &[
    "claude-opus-4-6-20250929",
    "claude-opus-4-6",
    "claude-sonnet-4-6-20250929",
    "claude-sonnet-4-6",
    "claude-opus-4-5-20251101",
    "claude-sonnet-4-5-20250929",
    "claude-haiku-4-5-20251001",
    "claude-opus-4-20250514",
    "claude-sonnet-4-20250514",
];
const DEFAULT_OPENAI_MODELS: &[&str] = &[
    "gpt-5.4",
    "gpt-5.4-mini",
    "o3",
    "o4-mini",
    "gpt-4.1",
    "gpt-4.1-mini",
    "gpt-4.1-nano",
    "gpt-4o",
    "gpt-4o-mini",
];
const DEFAULT_OPENAI_SUBSCRIPTION_MODELS: &[&str] = &[
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.2",
    "gpt-5.1",
    "o4-mini",
];
const DEFAULT_OPENROUTER_MODELS: &[&str] = &[
    "openai/gpt-4o-mini",
    "anthropic/claude-3.5-sonnet",
    "google/gemini-2.0-flash-001",
];
const DEFAULT_FILE_LEVEL: &str = "info";
const DEFAULT_STDERR_LEVEL: &str = "warn";
const DEFAULT_MAX_LOG_FILES: usize = 7;
const DEFAULT_LOG_DIR: &str = "~/.fawx/logs";
const LOG_FILE_PREFIX: &str = "fawx";
const LOG_FILE_SUFFIX: &str = "log";

pub(crate) type SharedMemoryStore = Arc<Mutex<dyn MemoryStore>>;
pub(crate) type SharedEmbeddingIndex = Arc<Mutex<EmbeddingIndex>>;
pub(crate) type SharedCredentialStore =
    Arc<fx_auth::credential_store::EncryptedFileCredentialStore>;
pub(crate) type SharedTokenBroker = Arc<dyn fx_auth::token_broker::TokenBroker>;

#[derive(Clone)]
pub struct EmbeddingIndexPersistence {
    pub index: SharedEmbeddingIndex,
    pub path: PathBuf,
}

impl EmbeddingIndexPersistence {
    pub fn save_if_dirty(&self) -> Result<(), String> {
        let guard = self
            .index
            .lock()
            .map_err(|error| format!("embedding index lock poisoned: {error}"))?;
        if !guard.is_dirty() {
            return Ok(());
        }
        guard.save(&self.path).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoggingMode {
    Tui,
    Serve,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedLoggingConfig {
    file_logging: bool,
    file_level: LevelFilter,
    stderr_level: LevelFilter,
    max_files: usize,
    log_dir: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct UtcMillisTimer;

impl FormatTime for UtcMillisTimer {
    fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result {
        write!(
            writer,
            "{}",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ")
        )
    }
}

/// Load the user config from ~/.fawx/config.toml (or return defaults).
pub fn load_config() -> Result<FawxConfig, StartupError> {
    let base_data_dir = fawx_data_dir();
    FawxConfig::load(&base_data_dir).map_err(StartupError::Store)
}

pub fn init_logging(
    config: &LoggingConfig,
    mode: LoggingMode,
) -> Result<WorkerGuard, StartupError> {
    let resolved = resolve_logging_config(config, mode)?;
    let (dispatch, guard) = build_logging_dispatch(&resolved)?;
    tracing::dispatcher::set_global_default(dispatch)
        .map_err(|error| StartupError::Logging(format!("failed to install logger: {error}")))?;
    Ok(guard)
}

fn build_logging_dispatch(
    config: &ResolvedLoggingConfig,
) -> Result<(Dispatch, WorkerGuard), StartupError> {
    let (file_writer, guard) = build_file_writer(config)?;
    let file_level = file_level_filter(config.file_logging, config.file_level);
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_timer(UtcMillisTimer)
                .with_writer(file_writer)
                .with_ansi(false)
                .with_target(true)
                .with_filter(file_level),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_timer(UtcMillisTimer)
                .with_writer(std::io::stderr)
                .with_ansi(std::io::stderr().is_terminal())
                .with_target(true)
                .with_filter(config.stderr_level),
        );
    Ok((Dispatch::new(subscriber), guard))
}

fn build_file_writer(
    config: &ResolvedLoggingConfig,
) -> Result<(tracing_appender::non_blocking::NonBlocking, WorkerGuard), StartupError> {
    if !config.file_logging {
        return Ok(tracing_appender::non_blocking(io::sink()));
    }
    fs::create_dir_all(&config.log_dir)?;
    cleanup_old_logs(&config.log_dir, config.max_files);
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(config.max_files)
        .build(&config.log_dir)
        .map_err(|error| {
            StartupError::Logging(format!("failed to create log appender: {error}"))
        })?;
    Ok(tracing_appender::non_blocking(appender))
}

/// Retention is enforced in two places: this startup scan catches up on
/// dated log files that piled up while the process was not running, while
/// `RollingFileAppender::max_log_files()` trims files during live rotation.
/// We need both to cover offline accumulation and normal runtime rotation.
fn cleanup_old_logs(log_dir: &Path, max_files: usize) {
    let mut log_files = match dated_log_files(log_dir) {
        Ok(files) => files,
        Err(error) => {
            eprintln!(
                "warning: failed to scan old log files in {}: {error}",
                log_dir.display()
            );
            return;
        }
    };
    if log_files.len() <= max_files {
        return;
    }
    log_files.sort();
    let remove_count = log_files.len().saturating_sub(max_files);
    for path in log_files.into_iter().take(remove_count) {
        if let Err(error) = fs::remove_file(&path) {
            eprintln!(
                "warning: failed to remove old log file {}: {error}",
                path.display()
            );
        }
    }
}

fn dated_log_files(log_dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !log_dir.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(log_dir)? {
        let path = entry?.path();
        if is_dated_log_file(&path) {
            files.push(path);
        }
    }
    Ok(files)
}

fn is_dated_log_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(date_part) = file_name
        .strip_prefix(&format!("{LOG_FILE_PREFIX}."))
        .and_then(|name| name.strip_suffix(&format!(".{LOG_FILE_SUFFIX}")))
    else {
        return false;
    };
    is_iso_date(date_part)
}

fn is_iso_date(value: &str) -> bool {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
}

fn file_level_filter(enabled: bool, level: LevelFilter) -> LevelFilter {
    if enabled {
        level
    } else {
        LevelFilter::OFF
    }
}

fn resolve_logging_config(
    config: &LoggingConfig,
    mode: LoggingMode,
) -> Result<ResolvedLoggingConfig, StartupError> {
    Ok(ResolvedLoggingConfig {
        file_logging: resolve_file_logging(config, mode),
        file_level: resolve_log_level(config.file_level.as_deref(), DEFAULT_FILE_LEVEL)?,
        stderr_level: resolve_log_level(config.stderr_level.as_deref(), DEFAULT_STDERR_LEVEL)?,
        max_files: config.max_files.unwrap_or(DEFAULT_MAX_LOG_FILES),
        log_dir: resolve_log_dir(config),
    })
}

fn resolve_file_logging(config: &LoggingConfig, mode: LoggingMode) -> bool {
    config
        .file_logging
        .unwrap_or(matches!(mode, LoggingMode::Serve))
}

pub(crate) fn resolve_log_dir(config: &LoggingConfig) -> PathBuf {
    config
        .log_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(default_log_dir)
}

fn default_log_dir() -> PathBuf {
    let relative = DEFAULT_LOG_DIR.trim_start_matches("~/");
    dirs::home_dir()
        .map(|home| home.join(relative))
        .unwrap_or_else(|| PathBuf::from(relative))
}

fn resolve_log_level(value: Option<&str>, default: &str) -> Result<LevelFilter, StartupError> {
    let selected = value.unwrap_or(default);
    parse_config_log_level(selected)
        .ok_or_else(|| StartupError::Logging(format!("invalid log level '{}'", selected.trim())))
}

/// Build a loop engine with sensible defaults for the TUI shell.
/// Convenience wrapper used by tests.
#[cfg(test)]
fn build_loop_engine() -> LoopEngine {
    build_loop_engine_bundle().engine
}

#[cfg(test)]
fn build_loop_engine_bundle() -> LoopEngineBundle {
    let config = load_config().unwrap_or_else(|error| {
        eprintln!("warning: failed to load config: {error}");
        FawxConfig::default()
    });
    build_loop_engine_from_config(&config, None).expect("loop engine config should be valid")
}

/// Bundle returned by the loop engine builder functions.
pub struct LoopEngineBundle {
    pub engine: LoopEngine,
    pub memory: Option<SharedMemoryStore>,
    pub embedding_index_persistence: Option<EmbeddingIndexPersistence>,
    pub runtime_info: Arc<RwLock<RuntimeInfo>>,
    pub event_bus: EventBus,
    pub scratchpad: Arc<Mutex<Scratchpad>>,
    pub skill_registry: Arc<SkillRegistry>,
    pub credential_provider: Option<Arc<dyn CredentialProvider>>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    pub credential_store: Option<Arc<fx_auth::credential_store::EncryptedFileCredentialStore>>,
    pub config_manager: Option<Arc<Mutex<ConfigManager>>>,
    /// Signature policy loaded once at startup, shared with skill watcher.
    pub signature_policy: SignaturePolicy,
    pub cron_store: Option<fx_cron::SharedCronStore>,
    pub startup_warnings: Vec<StartupWarning>,
    /// Shared callback slot for SSE stream events that need executor-side access.
    pub stream_callback_slot: Arc<std::sync::Mutex<Option<StreamCallback>>>,
    pub ripcord_journal: Arc<RipcordJournal>,
    /// LLM provider for experiment/improvement pipelines.
    pub improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
}

#[derive(Clone, Default)]
pub struct HeadlessLoopBuildOptions {
    pub working_dir: Option<PathBuf>,
    pub memory_enabled: bool,
    pub subagent_control: Option<Arc<dyn SubagentControl>>,
    pub config_manager: Option<Arc<Mutex<ConfigManager>>>,
    pub cancel_token: Option<CancellationToken>,
    pub experiment_progress: Option<ProgressCallback>,
    pub session_registry: Option<fx_session::SessionRegistry>,
    pub session_bus: Option<SessionBus>,
    pub permission_prompt_state: Option<Arc<PermissionPromptState>>,
    pub ripcord_journal: Option<Arc<RipcordJournal>>,
    pub credential_store: Option<SharedCredentialStore>,
    pub token_broker: Option<SharedTokenBroker>,
    #[cfg(feature = "http")]
    pub experiment_registry: Option<SharedExperimentRegistry>,
}

impl HeadlessLoopBuildOptions {
    pub fn parent(subagent_control: Arc<dyn SubagentControl>) -> Self {
        Self {
            working_dir: None,
            memory_enabled: true,
            subagent_control: Some(subagent_control),
            config_manager: None,
            cancel_token: None,
            experiment_progress: None,
            session_registry: None,
            session_bus: None,
            permission_prompt_state: None,
            ripcord_journal: None,
            credential_store: None,
            token_broker: None,
            #[cfg(feature = "http")]
            experiment_registry: None,
        }
    }

    pub fn subagent(working_dir: Option<PathBuf>, cancel_token: CancellationToken) -> Self {
        Self {
            working_dir,
            memory_enabled: false,
            subagent_control: None,
            config_manager: None,
            cancel_token: Some(cancel_token),
            experiment_progress: None,
            session_registry: None,
            session_bus: None,
            permission_prompt_state: None,
            ripcord_journal: None,
            credential_store: None,
            token_broker: None,
            #[cfg(feature = "http")]
            experiment_registry: None,
        }
    }
}

#[derive(Clone)]
struct SkillRegistryBuildOptions {
    working_dir: PathBuf,
    memory_enabled: bool,
    subagent_control: Option<Arc<dyn SubagentControl>>,
    config_manager: Option<Arc<Mutex<ConfigManager>>>,
    experiment_progress: Option<ProgressCallback>,
    session_registry: Option<fx_session::SessionRegistry>,
    session_bus: Option<SessionBus>,
    kernel_budget: BudgetConfig,
    stream_callback_slot: Arc<std::sync::Mutex<Option<StreamCallback>>>,
    credential_store: Option<SharedCredentialStore>,
    token_broker: Option<SharedTokenBroker>,
    #[cfg(feature = "http")]
    experiment_registry: Option<SharedExperimentRegistry>,
}

/// Build a loop engine from an already-loaded config.
pub fn build_loop_engine_from_config(
    config: &FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
) -> Result<LoopEngineBundle, StartupError> {
    build_loop_engine_from_config_with_options(
        config,
        improvement_provider,
        HeadlessLoopBuildOptions {
            memory_enabled: true,
            ..HeadlessLoopBuildOptions::default()
        },
    )
}

pub fn build_loop_engine_from_config_with_options(
    config: &FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    options: HeadlessLoopBuildOptions,
) -> Result<LoopEngineBundle, StartupError> {
    let base_data_dir = fawx_data_dir();
    let data_dir = configured_data_dir(&base_data_dir, config);
    build_loop_engine_with_options(data_dir, config.clone(), improvement_provider, options)
}

pub fn build_headless_loop_engine_bundle(
    config: &FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    options: HeadlessLoopBuildOptions,
) -> Result<LoopEngineBundle, StartupError> {
    let base_data_dir = fawx_data_dir();
    let data_dir = configured_data_dir(&base_data_dir, config);
    build_loop_engine_with_options(data_dir, config.clone(), improvement_provider, options)
}

#[cfg(feature = "http")]
pub(crate) fn build_shared_experiment_registry(
    data_dir: &Path,
) -> Result<SharedExperimentRegistry, StartupError> {
    let registry =
        fx_api::experiment_registry::ExperimentRegistry::new(data_dir).map_err(|error| {
            StartupError::Store(format!("failed to load experiment registry: {error}"))
        })?;
    Ok(Arc::new(tokio::sync::Mutex::new(registry)))
}

/// Capacity of the streaming event bus broadcast channel.
const EVENT_BUS_CAPACITY: usize = 256;

fn build_loop_engine_with_options(
    data_dir: PathBuf,
    config: FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    options: HeadlessLoopBuildOptions,
) -> Result<LoopEngineBundle, StartupError> {
    let event_bus = EventBus::new(EVENT_BUS_CAPACITY);
    let stream_callback_slot = Arc::new(std::sync::Mutex::new(None));
    let kernel_budget = BudgetConfig::default();
    let budget = BudgetTracker::new(kernel_budget.clone(), current_time_ms(), 0);
    let context = ContextCompactor::new(DEFAULT_CONTEXT_MAX_TOKENS, DEFAULT_CONTEXT_COMPACT_TARGET);
    let registry_options = build_skill_registry_options(
        &config,
        &options,
        &kernel_budget,
        Arc::clone(&stream_callback_slot),
    );
    let working_dir = registry_options.working_dir.clone();
    let improvement_provider_for_bundle = improvement_provider.clone();
    let skills = build_skill_registry(&data_dir, &config, improvement_provider, registry_options);
    let synthesis = config
        .model
        .synthesis_instruction
        .clone()
        .unwrap_or_else(|| DEFAULT_SYNTHESIS_INSTRUCTION.to_string());

    let bridge: Arc<dyn ScratchpadProvider> = Arc::new(ScratchpadBridge {
        scratchpad: Arc::clone(&skills.scratchpad),
    });

    let caching_registry =
        CachingExecutor::new(SharedSkillRegistry::new(Arc::clone(&skills.registry)));

    // Build executor chain:
    // PermissionGateExecutor → TripwireEvaluator → ProposalGateExecutor → CachingExecutor → SkillRegistry
    let self_modify_config = crate::config_bridge::to_core_self_modify(&config.self_modify);
    let proposals_dir = data_dir.join("proposals");
    let gate_state = ProposalGateState::new(self_modify_config, working_dir.clone(), proposals_dir);
    let proposal_gate = ProposalGateExecutor::new(caching_registry, gate_state);
    let permission_policy = permissions_to_policy(&config.permissions);
    let prompt_state = options
        .permission_prompt_state
        .unwrap_or_else(|| Arc::new(PermissionPromptState::new()));
    let permission_gate =
        PermissionGateExecutor::new(proposal_gate, permission_policy, prompt_state)
            .with_stream_callback_slot(Arc::clone(&stream_callback_slot));
    let ripcord_journal = options.ripcord_journal.unwrap_or_else(|| {
        let snapshot_dir = data_dir.join("ripcord").join("snapshots");
        Arc::new(RipcordJournal::new(&snapshot_dir))
    });
    let mut tripwires = fx_ripcord::config::default_tripwires();
    let working_dir_str = working_dir.to_string_lossy().to_string();
    resolve_tripwires(&mut tripwires, &working_dir_str);
    let tripwire_evaluator =
        TripwireEvaluator::new(permission_gate, tripwires, Arc::clone(&ripcord_journal));
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(tripwire_evaluator);

    let mut builder = LoopEngine::builder()
        .budget(budget)
        .context(context)
        .max_iterations(config.general.max_iterations)
        .tool_executor(Arc::clone(&tool_executor))
        .synthesis_instruction(synthesis)
        .event_bus(event_bus.clone())
        .iteration_counter(Arc::clone(&skills.iteration_counter))
        .session_memory(Arc::clone(&skills.session_memory))
        .scratchpad_provider(bridge);
    if let Some(cancel_token) = options.cancel_token {
        builder = builder.cancel_token(cancel_token);
    }
    if let Some(snapshot_text) = skills.memory_snapshot {
        builder = builder.memory_context(snapshot_text);
    }
    let thinking_budget = config.general.thinking.unwrap_or_default();
    let model_id = config.model.default_model.as_deref().unwrap_or("");
    if let Some(thinking) = thinking_config_for_active_model(&thinking_budget, model_id) {
        builder = builder.thinking_config(thinking);
    }
    if let Some(journal) = &skills.journal {
        builder = builder.memory_flush(Arc::new(fx_journal::JournalCompactionFlush::new(
            Arc::clone(journal),
        ))
            as Arc<dyn fx_kernel::conversation_compactor::CompactionMemoryFlush>);
    }

    let engine = build_loop_engine_from_builder(builder)?;
    Ok(LoopEngineBundle {
        engine,
        memory: skills.memory,
        embedding_index_persistence: skills.embedding_index_persistence,
        runtime_info: skills.runtime_info,
        event_bus,
        scratchpad: skills.scratchpad,
        skill_registry: skills.registry,
        credential_provider: skills.credential_provider,
        tool_executor,
        credential_store: skills.credential_store,
        config_manager: options.config_manager.clone(),
        signature_policy: skills.signature_policy,
        cron_store: skills.cron_store,
        startup_warnings: skills.startup_warnings,
        stream_callback_slot,
        improvement_provider: improvement_provider_for_bundle,
        ripcord_journal,
    })
}

fn build_skill_registry_options(
    config: &FawxConfig,
    options: &HeadlessLoopBuildOptions,
    kernel_budget: &BudgetConfig,
    stream_callback_slot: Arc<std::sync::Mutex<Option<StreamCallback>>>,
) -> SkillRegistryBuildOptions {
    SkillRegistryBuildOptions {
        working_dir: options
            .working_dir
            .clone()
            .unwrap_or_else(|| configured_working_dir(config)),
        memory_enabled: options.memory_enabled,
        subagent_control: options.subagent_control.clone(),
        config_manager: options.config_manager.clone(),
        experiment_progress: options.experiment_progress.clone(),
        session_registry: options.session_registry.clone(),
        session_bus: options.session_bus.clone(),
        kernel_budget: kernel_budget.clone(),
        stream_callback_slot,
        credential_store: options.credential_store.clone(),
        token_broker: options.token_broker.clone(),
        #[cfg(feature = "http")]
        experiment_registry: options.experiment_registry.clone(),
    }
}

struct StreamNotificationSender {
    callback_slot: Arc<std::sync::Mutex<Option<StreamCallback>>>,
}

impl fmt::Debug for StreamNotificationSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamNotificationSender").finish()
    }
}

impl StreamNotificationSender {
    fn new(callback_slot: Arc<std::sync::Mutex<Option<StreamCallback>>>) -> Self {
        Self { callback_slot }
    }
}

#[async_trait]
impl NotificationSender for StreamNotificationSender {
    async fn send(&self, title: &str, body: &str) -> Result<(), String> {
        let callback = self
            .callback_slot
            .lock()
            .map_err(|error| format!("notification callback unavailable: {error}"))?
            .clone();

        if let Some(callback) = callback {
            callback(StreamEvent::Notification {
                title: title.to_string(),
                body: body.to_string(),
            });
        }

        Ok(())
    }
}

pub fn open_session_registry(data_dir: &Path) -> Option<fx_session::SessionRegistry> {
    let sessions_path = data_dir.join("sessions.redb");
    let storage = match open_with_retry("session registry", &sessions_path, || {
        fx_storage::Storage::open(&sessions_path)
    }) {
        Ok(storage) => storage,
        Err(error) => {
            tracing::warn!(path = %sessions_path.display(), error = %error, "session storage unavailable");
            return None;
        }
    };

    match fx_session::SessionRegistry::new(fx_session::SessionStore::new(storage)) {
        Ok(registry) => Some(registry),
        Err(error) => {
            tracing::warn!(path = %sessions_path.display(), error = %error, "session registry unavailable");
            None
        }
    }
}

pub(crate) fn open_credential_store(data_dir: &Path) -> Result<SharedCredentialStore, String> {
    let credential_store_path = data_dir.join("credentials.db");
    open_with_retry("credential store", &credential_store_path, || {
        fx_auth::credential_store::EncryptedFileCredentialStore::open(data_dir)
    })
    .map(Arc::new)
}

pub(crate) fn build_token_broker(
    config: &FawxConfig,
    credential_store: Option<&SharedCredentialStore>,
) -> Option<SharedTokenBroker> {
    credential_store.map(|store| {
        Arc::new(fx_auth::token_broker::CredentialStoreBroker::new(
            Arc::clone(store),
            config.security.github_borrow_scope,
        )) as SharedTokenBroker
    })
}

fn build_loop_engine_from_builder(builder: LoopEngineBuilder) -> Result<LoopEngine, StartupError> {
    builder.build().map_err(|error| {
        StartupError::Loop(format!(
            "failed to build loop engine: stage={} reason={}",
            error.stage, error.reason
        ))
    })
}

/// Bridges `fx_scratchpad::Scratchpad` into the kernel's [`ScratchpadProvider`]
/// trait without introducing a circular crate dependency.
struct ScratchpadBridge {
    scratchpad: Arc<Mutex<Scratchpad>>,
}

impl ScratchpadProvider for ScratchpadBridge {
    fn render_for_context(&self) -> String {
        match self.scratchpad.lock() {
            Ok(sp) => sp.render_for_context(),
            Err(_) => String::new(),
        }
    }

    fn compact_if_needed(&self, current_iteration: u32) {
        let Ok(mut sp) = self.scratchpad.lock() else {
            return;
        };
        let rendered_len = sp.render_for_context().len();
        if rendered_len > fx_scratchpad::SCRATCHPAD_COMPACT_THRESHOLD_CHARS {
            sp.compact(
                fx_scratchpad::SCRATCHPAD_COMPACT_TARGET_TOKENS,
                current_iteration,
                fx_scratchpad::SCRATCHPAD_AGE_THRESHOLD,
            );
        }
    }
}

/// Bridges stored auth and encrypted skill credentials into the
/// [`CredentialProvider`] trait so WASM skills can retrieve secrets via
/// `kv_get`.
///
/// Well-known mappings:
/// - `"openai_api_key"` → OpenAI API key from the auth manager
/// - `"anthropic_api_key"` → Anthropic API key from the auth manager
/// - `"github_token"` → GitHub PAT from the credential store
///
/// Unknown keys fall back to generic encrypted skill credentials.
struct CredentialStoreBridge {
    data_dir: PathBuf,
    store: SharedCredentialStore,
    token_broker: Option<SharedTokenBroker>,
}

impl CredentialStoreBridge {
    fn well_known_credential(&self, key: &str) -> Option<zeroize::Zeroizing<String>> {
        match key {
            "openai_api_key" => self.auth_api_key("openai"),
            "anthropic_api_key" => self.auth_api_key("anthropic"),
            "github_token" => self.borrow_github_token(),
            _ => None,
        }
    }

    fn auth_api_key(&self, provider: &str) -> Option<zeroize::Zeroizing<String>> {
        let manager = match self.load_auth_manager() {
            Ok(manager) => manager,
            Err(error) => {
                tracing::warn!(
                    provider,
                    error = %error,
                    "failed to load auth manager while resolving credential bridge API key"
                );
                return None;
            }
        };

        match manager.get(provider) {
            Some(AuthMethod::ApiKey { key, .. }) => Some(zeroize::Zeroizing::new(key.clone())),
            _ => None,
        }
    }

    fn borrow_github_token(&self) -> Option<zeroize::Zeroizing<String>> {
        // Try broker first (scoped borrow for subagents).
        if let Some(broker) = self.token_broker.as_ref() {
            match broker.borrow_github_default() {
                Ok(borrow) => return Some(borrow.into_token()),
                Err(fx_auth::token_broker::BorrowError::NotConfigured) => {}
                Err(error) => {
                    tracing::warn!(error = %error, "failed to borrow GitHub token");
                }
            }
        }
        // Try encrypted credential store (fawx setup).
        if let Some(token) = self.github_token() {
            return Some(token);
        }
        // Fall back to AuthManager (legacy /auth github set-token).
        self.auth_api_key("github")
    }

    fn github_token(&self) -> Option<zeroize::Zeroizing<String>> {
        use fx_auth::credential_store::{AuthProvider, CredentialMethod};

        match self.store.get(AuthProvider::GitHub, CredentialMethod::Pat) {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "failed to read GitHub token while resolving credential bridge credential"
                );
                None
            }
        }
    }

    fn generic_credential(&self, key: &str) -> Option<zeroize::Zeroizing<String>> {
        match self.store.get_generic(key) {
            Ok(credential) => credential,
            Err(error) => {
                tracing::warn!(
                    credential = key,
                    error = %error,
                    "failed to read generic credential while resolving credential bridge credential"
                );
                None
            }
        }
    }

    fn load_auth_manager(&self) -> Result<AuthManager, String> {
        AuthStore::open(&self.data_dir)?.load_auth_manager()
    }
}

impl CredentialProvider for CredentialStoreBridge {
    fn get_credential(&self, key: &str) -> Option<zeroize::Zeroizing<String>> {
        self.well_known_credential(key)
            .or_else(|| self.generic_credential(key))
    }
}

#[derive(Debug, Clone)]
struct SharedSkillRegistry {
    registry: Arc<SkillRegistry>,
}

impl SharedSkillRegistry {
    fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl ToolExecutor for SharedSkillRegistry {
    async fn execute_tools(
        &self,
        calls: &[fx_llm::ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<fx_kernel::act::ToolResult>, fx_kernel::act::ToolExecutorError> {
        self.registry.execute_tools(calls, cancel).await
    }

    fn concurrency_policy(&self) -> fx_kernel::act::ConcurrencyPolicy {
        self.registry.concurrency_policy()
    }

    fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
        self.registry.tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> fx_kernel::act::ToolCacheability {
        self.registry.cacheability(tool_name)
    }

    fn cache_stats(&self) -> Option<fx_kernel::act::ToolCacheStats> {
        self.registry.cache_stats()
    }
}

/// Result of [`build_skill_registry`]: groups related outputs to avoid a
/// large tuple return type.
struct SkillRegistryBundle {
    registry: Arc<SkillRegistry>,
    memory: Option<SharedMemoryStore>,
    embedding_index_persistence: Option<EmbeddingIndexPersistence>,
    memory_snapshot: Option<String>,
    runtime_info: Arc<RwLock<RuntimeInfo>>,
    scratchpad: Arc<Mutex<Scratchpad>>,
    session_memory: Arc<Mutex<fx_session::SessionMemory>>,
    iteration_counter: Arc<std::sync::atomic::AtomicU32>,
    journal: Option<Arc<Mutex<fx_journal::Journal>>>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    credential_store: Option<Arc<fx_auth::credential_store::EncryptedFileCredentialStore>>,
    /// Signature policy loaded once at startup, shared with the skill watcher
    /// to avoid redundant filesystem reads.
    signature_policy: SignaturePolicy,
    cron_store: Option<fx_cron::SharedCronStore>,
    startup_warnings: Vec<StartupWarning>,
}

fn build_skill_registry(
    data_dir: &Path,
    config: &FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    options: SkillRegistryBuildOptions,
) -> SkillRegistryBundle {
    let tool_config = ToolConfig {
        max_read_size: config.tools.max_read_size,
        search_exclude: config.tools.search_exclude.clone(),
        ..ToolConfig::default()
    };
    let process_registry = Arc::new(ProcessRegistry::new(ProcessConfig {
        allowed_dirs: vec![options.working_dir.clone()],
        ..ProcessConfig::default()
    }));
    ProcessRegistry::spawn_cleanup_task(&process_registry);
    let mut startup_warnings = Vec::new();
    let executor = build_tool_executor(&options, tool_config, process_registry);
    let (mut executor, memory, embedding_index_persistence, snapshot_text, memory_enabled) =
        attach_memory_if_enabled(
            executor,
            data_dir,
            config,
            options.memory_enabled,
            &mut startup_warnings,
        );

    let self_modify_config = crate::config_bridge::to_core_self_modify(&config.self_modify);
    let sm = self_modify_config.enabled.then_some(self_modify_config);
    if let Some(ref smc) = sm {
        executor = executor.with_self_modify(smc.clone());
    }

    let runtime_info = new_runtime_info(config, memory_enabled);
    executor = executor.with_runtime_info(Arc::clone(&runtime_info));
    if let Some(config_manager) = options.config_manager {
        executor = executor.with_config_manager(config_manager);
    }
    if let Some(control) = options.subagent_control {
        executor = executor.with_subagent_control(control);
    }
    if let Ok(auth_manager) = load_auth_manager() {
        match build_router(&auth_manager) {
            Ok(router) => {
                executor = executor.with_experiment(ExperimentToolState {
                    chain_path: data_dir.join("consensus").join("chain.json"),
                    router: Arc::new(router),
                    config: config.clone(),
                });
            }
            Err(error) => {
                eprintln!("warning: experiment tool unavailable: {error}");
            }
        }
    }
    executor = attach_node_run_if_configured(executor, config);

    // Wire improvement tools when enabled and a provider is available.
    if config.improvement.enabled {
        if let Some(provider) = improvement_provider {
            match wire_improvement_tools(data_dir, provider, &config.improvement) {
                Ok(state) => {
                    executor = executor.with_improvement(state);
                }
                Err(e) => {
                    eprintln!("warning: improvement tools unavailable: {e}");
                }
            }
        }
    }

    let registry = Arc::new(SkillRegistry::new());
    registry.register(Arc::new(BuiltinToolsSkill::new(executor)));
    let notify_sender = Arc::new(StreamNotificationSender::new(Arc::clone(
        &options.stream_callback_slot,
    )));
    let notify_skill = NotifySkill::new(notify_sender);
    registry.register(Arc::new(notify_skill));
    let tx_skill = TransactionSkill::new(options.working_dir.clone(), sm.clone());
    registry.register(Arc::new(tx_skill));
    let scratchpad = Arc::new(Mutex::new(Scratchpad::new()));
    let iteration_counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let scratchpad_skill =
        ScratchpadSkill::new(Arc::clone(&scratchpad), Arc::clone(&iteration_counter));
    registry.register(Arc::new(scratchpad_skill));
    let session_memory = Arc::new(Mutex::new(fx_session::SessionMemory::default()));
    let session_memory_skill = SessionMemorySkill::new(Arc::clone(&session_memory));
    registry.register(Arc::new(session_memory_skill));

    if let Some(session_registry) = options.session_registry.clone() {
        let session_skill = SessionToolsSkill::new(session_registry);
        registry.register(Arc::new(session_skill));
    }

    // Load reflective journal for cross-session learning.
    let journal_path = data_dir.join("journal.jsonl");
    let journal_arc = match fx_journal::Journal::load(journal_path) {
        Ok(journal) => {
            let arc = Arc::new(Mutex::new(journal));
            let journal_skill = JournalSkill::new(Arc::clone(&arc));
            registry.register(Arc::new(journal_skill));
            Some(arc)
        }
        Err(e) => {
            tracing::warn!("journal unavailable: {e}");
            None
        }
    };

    // Open the credential store once and share via Arc between TuiApp and WASM bridge.
    let credential_store = match options.credential_store.clone() {
        Some(store) => Some(store),
        None => match open_credential_store(data_dir) {
            Ok(store) => Some(store),
            Err(error) => {
                tracing::warn!("credential store unavailable: {error}");
                None
            }
        },
    };

    let credential_provider: Option<Arc<dyn CredentialProvider>> =
        credential_store.as_ref().map(|store| {
            Arc::new(CredentialStoreBridge {
                data_dir: data_dir.to_path_buf(),
                store: Arc::clone(store),
                token_broker: options.token_broker.clone(),
            }) as Arc<dyn CredentialProvider>
        });

    // Wire GitSkill with GitHub token provider from credential bridge.
    let github_token_fn: Option<fx_tools::GitHubTokenProvider> =
        credential_provider.clone().map(|cp| {
            std::sync::Arc::new(move || cp.get_credential("github_token"))
                as std::sync::Arc<dyn Fn() -> Option<zeroize::Zeroizing<String>> + Send + Sync>
        });
    let git_skill = GitSkill::new(options.working_dir.clone(), sm, github_token_fn);
    registry.register(Arc::new(git_skill));

    // Load WASM skills from ~/.fawx/skills/
    let trusted_keys = fx_loadable::wasm_skill::load_trusted_keys().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "failed to load trusted keys");
        vec![]
    });
    let signature_policy = SignaturePolicy {
        trusted_keys,
        require_signatures: config.security.require_signatures,
    };
    match fx_loadable::wasm_skill::load_wasm_skills(credential_provider.clone(), &signature_policy)
    {
        Ok(wasm_skills) => {
            for skill in wasm_skills {
                registry.register(skill);
            }
        }
        Err(e) => {
            eprintln!("warning: failed to load WASM skills: {e}");
        }
    }

    // Register cron/scheduler skill.
    let cron_store_path = data_dir.join("cron.redb");
    let cron_store = match open_with_retry("cron store", &cron_store_path, || {
        fx_cron::CronStore::open(&cron_store_path)
    }) {
        Ok(store) => {
            let arc = Arc::new(tokio::sync::Mutex::new(store));
            let cron_skill = Arc::new(fx_tools::CronSkill::new(
                Arc::clone(&arc),
                options.session_bus.clone(),
            ));
            registry.register(cron_skill);
            Some(arc)
        }
        Err(error) => {
            tracing::warn!(error = %error, "cron store unavailable");
            startup_warnings.push(StartupWarning {
                category: ErrorCategory::System,
                message: format!("Cron store unavailable: {error}"),
            });
            None
        }
    };

    apply_skill_summaries(&runtime_info, registry.as_ref());

    SkillRegistryBundle {
        registry,
        memory,
        embedding_index_persistence,
        memory_snapshot: snapshot_text,
        runtime_info,
        scratchpad,
        session_memory,
        iteration_counter,
        journal: journal_arc,
        credential_provider,
        credential_store,
        signature_policy,
        cron_store,
        startup_warnings,
    }
}

fn build_tool_executor(
    options: &SkillRegistryBuildOptions,
    tool_config: ToolConfig,
    process_registry: Arc<ProcessRegistry>,
) -> FawxToolExecutor {
    let executor = FawxToolExecutor::new(options.working_dir.clone(), tool_config)
        .with_process_registry(process_registry)
        .with_kernel_budget(options.kernel_budget.clone());
    #[cfg(feature = "http")]
    let executor = attach_experiment_registrar(executor, options.experiment_registry.as_ref());
    #[cfg(feature = "http")]
    let executor = executor.with_background_experiments(true);
    match options.experiment_progress.clone() {
        Some(progress) => executor.with_experiment_progress(progress),
        None => executor,
    }
}

#[cfg(feature = "http")]
fn attach_experiment_registrar(
    executor: FawxToolExecutor,
    registry: Option<&SharedExperimentRegistry>,
) -> FawxToolExecutor {
    let Some(registry) = registry else {
        return executor;
    };
    executor.with_experiment_registrar(Arc::new(RegistryBridge::new(Arc::clone(registry))))
}

fn attach_node_run_if_configured(
    executor: FawxToolExecutor,
    config: &FawxConfig,
) -> FawxToolExecutor {
    if config.fleet.nodes.is_empty() {
        return executor;
    }
    let state = build_node_run_state(config);
    executor.with_node_run(state)
}

fn build_node_run_state(config: &FawxConfig) -> NodeRunState {
    let registry = build_node_registry(config);
    let transport = build_node_transport();
    NodeRunState {
        registry: Arc::new(tokio::sync::RwLock::new(registry)),
        transport,
    }
}

fn build_node_registry(config: &FawxConfig) -> NodeRegistry {
    let threshold_ms = config.fleet.stale_timeout_seconds.saturating_mul(1_000);
    let mut registry = NodeRegistry::with_stale_threshold(threshold_ms);
    for node in &config.fleet.nodes {
        registry.register(node.into());
    }
    registry
}

fn build_node_transport() -> Arc<dyn NodeTransport> {
    Arc::new(SshTransport::new(default_ssh_key_path()))
}

fn default_ssh_key_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ssh/id_ed25519")
}

/// Build `ImprovementToolsState` from a dedicated provider and data directory.
///
/// Creates its own `SignalStore` (reads from the same on-disk directory as
/// `TuiApp`'s store, so all signals are visible to analysis).
fn wire_improvement_tools(
    data_dir: &Path,
    provider: Arc<dyn fx_llm::CompletionProvider + Send + Sync>,
    config: &ImprovementToolsConfig,
) -> Result<ImprovementToolsState, String> {
    let signal_store = SignalStore::new(data_dir, "improvement-analysis")
        .map_err(|e| format!("signal store for improvement tools: {e}"))?;
    Ok(ImprovementToolsState::new(
        Arc::new(signal_store),
        provider,
        config.clone(),
    ))
}

fn attach_memory_if_enabled(
    mut executor: FawxToolExecutor,
    data_dir: &Path,
    config: &FawxConfig,
    enabled: bool,
    startup_warnings: &mut Vec<StartupWarning>,
) -> (
    FawxToolExecutor,
    Option<SharedMemoryStore>,
    Option<EmbeddingIndexPersistence>,
    Option<String>,
    bool,
) {
    if !enabled {
        return (executor, None, None, None, false);
    }
    match build_memory_state(data_dir, config, startup_warnings) {
        Ok((memory_store, snapshot_text, persistence)) => {
            let memory: SharedMemoryStore = Arc::new(Mutex::new(memory_store));
            executor = executor.with_memory(Arc::clone(&memory));
            if let Some(persistence) = &persistence {
                executor = executor.with_embedding_index(Arc::clone(&persistence.index));
            }
            (executor, Some(memory), persistence, snapshot_text, true)
        }
        Err(error) => {
            eprintln!("warning: failed to initialize memory: {error}");
            startup_warnings.push(StartupWarning {
                category: ErrorCategory::Memory,
                message: format!("Failed to initialize memory: {error}"),
            });
            (executor, None, None, None, false)
        }
    }
}

fn build_memory_state(
    data_dir: &Path,
    config: &FawxConfig,
    startup_warnings: &mut Vec<StartupWarning>,
) -> Result<
    (
        JsonFileMemory,
        Option<String>,
        Option<EmbeddingIndexPersistence>,
    ),
    String,
> {
    let mut memory_store = JsonFileMemory::new_with_config(data_dir, memory_config(config))?;
    log_pruned_memories(&mut memory_store);
    let persistence =
        load_embedding_index_persistence(data_dir, config, &memory_store, startup_warnings)?;
    let snapshot = memory_store.snapshot();
    let text = format_memory_for_prompt(&snapshot, config.memory.max_snapshot_chars);
    Ok((memory_store, text, persistence))
}

fn memory_config(config: &FawxConfig) -> JsonMemoryConfig {
    JsonMemoryConfig {
        max_entries: config.memory.max_entries,
        max_value_size: config.memory.max_value_size,
        decay_config: fx_memory::DecayConfig::default(),
    }
}

fn log_pruned_memories(memory_store: &mut JsonFileMemory) {
    let pruned = memory_store.prune();
    if pruned > 0 {
        eprintln!("memory: pruned {pruned} stale entries at session start");
    }
}

fn load_embedding_index_persistence(
    data_dir: &Path,
    config: &FawxConfig,
    memory_store: &JsonFileMemory,
    startup_warnings: &mut Vec<StartupWarning>,
) -> Result<Option<EmbeddingIndexPersistence>, String> {
    if !config.memory.embeddings_enabled {
        tracing::info!("memory embeddings disabled in config");
        return Ok(None);
    }
    let paths = embedding_paths(data_dir);
    let Some(model) = load_embedding_model_or_warn(&paths.model_dir, startup_warnings) else {
        return Ok(None);
    };
    let Some(index) = load_or_build_embedding_index_or_warn(
        memory_store,
        &paths.index_path,
        model,
        startup_warnings,
    ) else {
        return Ok(None);
    };
    log_embedding_index_status(&index, &paths.index_path)?;
    Ok(Some(EmbeddingIndexPersistence {
        index,
        path: paths.index_path,
    }))
}

struct EmbeddingPaths {
    model_dir: PathBuf,
    index_path: PathBuf,
}

fn embedding_paths(data_dir: &Path) -> EmbeddingPaths {
    EmbeddingPaths {
        model_dir: data_dir.join("models").join("nomic-embed-text-v1.5"),
        index_path: data_dir.join("memory").join("embeddings.bin"),
    }
}

fn load_embedding_model_or_warn(
    model_dir: &Path,
    startup_warnings: &mut Vec<StartupWarning>,
) -> Option<Arc<EmbeddingModel>> {
    match load_embedding_model(model_dir) {
        Ok(model) => model,
        Err(error) => {
            tracing::warn!(path = %model_dir.display(), error = %error, "failed to initialize memory embeddings");
            startup_warnings.push(StartupWarning {
                category: ErrorCategory::Memory,
                message: format!("Failed to initialize memory embeddings: {error}"),
            });
            None
        }
    }
}

fn load_embedding_model(model_dir: &Path) -> Result<Option<Arc<EmbeddingModel>>, String> {
    if !model_dir.exists() {
        tracing::info!(path = %model_dir.display(), "memory embedding model not found; skipping semantic memory search");
        return Ok(None);
    }
    EmbeddingModel::load(model_dir)
        .map(Arc::new)
        .map(Some)
        .map_err(|error| format!("failed to load embedding model: {error}"))
}

fn load_or_build_embedding_index_or_warn(
    memory_store: &JsonFileMemory,
    index_path: &Path,
    model: Arc<EmbeddingModel>,
    startup_warnings: &mut Vec<StartupWarning>,
) -> Option<SharedEmbeddingIndex> {
    match load_or_build_embedding_index(memory_store, index_path, model) {
        Ok(index) => Some(index),
        Err(error) => {
            tracing::warn!(path = %index_path.display(), error = %error, "failed to initialize embedding index");
            startup_warnings.push(StartupWarning {
                category: ErrorCategory::Memory,
                message: format!("Failed to initialize embedding index: {error}"),
            });
            None
        }
    }
}

fn load_or_build_embedding_index(
    memory_store: &JsonFileMemory,
    index_path: &Path,
    model: Arc<EmbeddingModel>,
) -> Result<SharedEmbeddingIndex, String> {
    let index = match try_load_embedding_index(index_path, Arc::clone(&model)) {
        Some(Ok(index)) => index,
        Some(Err(error)) => {
            tracing::warn!(path = %index_path.display(), error = %error, "failed to load embedding index; rebuilding from memory");
            build_embedding_index(memory_store, &model)?
        }
        None => build_embedding_index(memory_store, &model)?,
    };
    Ok(Arc::new(Mutex::new(index)))
}

fn try_load_embedding_index(
    index_path: &Path,
    model: Arc<EmbeddingModel>,
) -> Option<Result<EmbeddingIndex, String>> {
    if !index_path.exists() {
        return None;
    }
    Some(load_embedding_index(index_path, model))
}

fn load_embedding_index(
    index_path: &Path,
    model: Arc<EmbeddingModel>,
) -> Result<EmbeddingIndex, String> {
    EmbeddingIndex::load(index_path, model).map_err(|error| error.to_string())
}

fn build_embedding_index(
    memory_store: &JsonFileMemory,
    model: &Arc<EmbeddingModel>,
) -> Result<EmbeddingIndex, String> {
    let entries = memory_store.list();
    EmbeddingIndex::build_from(&entries, model).map_err(|error| error.to_string())
}

fn log_embedding_index_status(
    index: &SharedEmbeddingIndex,
    index_path: &Path,
) -> Result<(), String> {
    let entries = index.lock().map_err(|error| format!("{error}"))?.len();
    tracing::info!(
        entries,
        path = %index_path.display(),
        "memory embeddings enabled"
    );
    Ok(())
}

fn new_runtime_info(config: &FawxConfig, memory_enabled: bool) -> Arc<RwLock<RuntimeInfo>> {
    Arc::new(RwLock::new(RuntimeInfo {
        active_model: String::new(),
        provider: String::new(),
        skills: Vec::new(),
        config_summary: ConfigSummary {
            max_iterations: config.general.max_iterations,
            max_history: config.general.max_history,
            memory_enabled,
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}

fn apply_skill_summaries(runtime_info: &Arc<RwLock<RuntimeInfo>>, registry: &SkillRegistry) {
    let skills = registry
        .skill_summaries()
        .into_iter()
        .map(|(name, description, tool_names, capabilities)| SkillInfo {
            name,
            description: Some(description),
            tool_names,
            capabilities,
        })
        .collect::<Vec<_>>();

    match runtime_info.write() {
        Ok(mut info) => info.skills = skills,
        Err(error) => eprintln!("warning: runtime info lock poisoned: {error}"),
    }
}

/// Owned `CompletionProvider` wrapping a dedicated `ModelRouter` for
/// improvement tools. Unlike `crate::helpers::AnalysisCompletionProvider`
/// (which borrows), this owns its router so it can be stored in
/// `ImprovementToolsState`.
struct OwnedRouterProvider {
    router: ModelRouter,
}

impl fmt::Debug for OwnedRouterProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnedRouterProvider")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl fx_llm::CompletionProvider for OwnedRouterProvider {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
        self.router.complete(request).await
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
        self.router.complete_stream(request).await
    }

    fn name(&self) -> &str {
        "improvement"
    }

    fn supported_models(&self) -> Vec<String> {
        self.router
            .available_models()
            .into_iter()
            .map(|m| m.model_id)
            .collect()
    }

    fn capabilities(&self) -> fx_llm::ProviderCapabilities {
        fx_llm::ProviderCapabilities {
            supports_temperature: true,
            requires_streaming: false,
        }
    }
}

pub fn fawx_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FAWX_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .map(|home| home.join(".fawx"))
        .unwrap_or_else(|| PathBuf::from(".fawx"))
}

pub(crate) fn configured_data_dir(base_data_dir: &Path, config: &FawxConfig) -> PathBuf {
    config
        .general
        .data_dir
        .clone()
        .unwrap_or_else(|| base_data_dir.to_path_buf())
}

/// Retry brief redb lock contention during startup.
///
/// This is intentionally scoped to startup paths that can race a just-stopped
/// `fawx serve` process. Direct `SessionRegistry::open` callers outside that
/// path still fail fast on contention because they are not part of the
/// stop/start lock-release race.
fn open_with_retry<T, E: fmt::Display>(
    label: &str,
    path: &Path,
    opener: impl Fn() -> Result<T, E>,
) -> Result<T, String> {
    for attempt in 0..3u32 {
        match opener() {
            Ok(result) => return Ok(result),
            Err(error) if attempt < 2 => {
                let message = error.to_string();
                if is_redb_lock_retryable(&message) {
                    tracing::warn!(
                        path = %path.display(),
                        attempt = attempt + 1,
                        "{label} locked, retrying in {}ms",
                        100 * (attempt + 1)
                    );
                    std::thread::sleep(std::time::Duration::from_millis(
                        100 * u64::from(attempt + 1),
                    ));
                    continue;
                }
                return Err(message);
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    unreachable!()
}

/// redb currently reports lock contention via formatted strings instead of a
/// dedicated error type, so this match is a heuristic tied to the current
/// "Database already open" / "Cannot acquire lock" wording.
fn is_redb_lock_retryable(message: &str) -> bool {
    message.contains("Database already open") || message.contains("Cannot acquire lock")
}

pub(crate) fn build_session_bus_for_data_dir(data_dir: &Path) -> Option<SessionBus> {
    let bus_db_path = data_dir.join("bus.redb");
    let storage = match open_with_retry("session bus", &bus_db_path, || {
        fx_storage::Storage::open(&bus_db_path)
    }) {
        Ok(storage) => storage,
        Err(error) => {
            tracing::warn!(path = %bus_db_path.display(), error = %error, "session bus unavailable");
            return None;
        }
    };
    Some(SessionBus::new(BusStore::new(storage)))
}

pub(crate) fn configured_working_dir(config: &FawxConfig) -> PathBuf {
    if let Some(path) = &config.tools.working_dir {
        return path.clone();
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// User-facing TUI errors.
#[derive(Debug)]
pub enum StartupError {
    /// Terminal or filesystem IO failure.
    Io(io::Error),
    /// Authentication flow error.
    Auth(String),
    /// User cancelled interactive input.
    Cancelled,
    /// Conversation store/persistence error.
    Store(String),
    /// Model routing error.
    Router(String),
    /// Logging initialization error.
    Logging(String),
    /// Request execution error.
    Loop(String),
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Auth(message) => write!(f, "auth error: {message}"),
            Self::Cancelled => write!(f, "input cancelled"),
            Self::Store(message) => write!(f, "store error: {message}"),
            Self::Router(message) => write!(f, "router error: {message}"),
            Self::Logging(message) => write!(f, "logging error: {message}"),
            Self::Loop(message) => write!(f, "loop error: {message}"),
        }
    }
}

impl std::error::Error for StartupError {}

impl From<io::Error> for StartupError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<AnalysisError> for StartupError {
    fn from(value: AnalysisError) -> Self {
        Self::Loop(format!("analysis failed: {value}"))
    }
}

impl From<fx_improve::ImprovementError> for StartupError {
    fn from(value: fx_improve::ImprovementError) -> Self {
        Self::Loop(format!("improvement failed: {value}"))
    }
}

/// Load the persisted auth manager from the encrypted store.
pub fn load_auth_manager() -> Result<AuthManager, StartupError> {
    // NB2: Warn if the removed FAWX_AUTH_FILE env var is still set.
    if std::env::var("FAWX_AUTH_FILE").is_ok() {
        eprintln!(
            "warning: FAWX_AUTH_FILE is deprecated; \
             credentials now stored encrypted in ~/.fawx/auth.db"
        );
    }
    let data_dir = fawx_data_dir();
    let store = AuthStore::open(&data_dir)
        .map_err(|e| StartupError::Auth(format!("failed to open auth store: {e}")))?;
    migrate_if_needed(&data_dir, &store)
        .map_err(|e| StartupError::Auth(format!("auth migration failed: {e}")))?;
    store
        .load_auth_manager()
        .map_err(|e| StartupError::Auth(format!("failed to load credentials: {e}")))
}

/// Build a model router from stored authentication credentials.
/// Build an optional `CompletionProvider` for improvement tools.
///
/// Returns `None` when `[improvement] enabled = false` in config. Otherwise,
/// builds a dedicated `ModelRouter` (separate from the main TUI router) so
/// the improvement tools can own their LLM access without borrowing.
pub fn build_improvement_provider(
    auth_manager: &AuthManager,
    config: &FawxConfig,
) -> Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>> {
    if !config.improvement.enabled {
        return None;
    }
    match build_router(auth_manager) {
        Ok(router) => Some(Arc::new(OwnedRouterProvider { router })),
        Err(e) => {
            eprintln!("warning: improvement tools LLM unavailable: {e}");
            None
        }
    }
}

pub fn build_router(auth_manager: &AuthManager) -> Result<ModelRouter, StartupError> {
    let mut router = ModelRouter::new();

    for provider in auth_manager.providers() {
        if let Some(auth_method) = auth_manager.get(&provider) {
            register_auth_provider(&mut router, auth_method)?;
        }
    }

    Ok(router)
}

fn register_auth_provider(
    router: &mut ModelRouter,
    auth_method: &AuthMethod,
) -> Result<(), StartupError> {
    register_auth_provider_with_models(router, auth_method, default_supported_models(auth_method))
}

fn register_auth_provider_with_models(
    router: &mut ModelRouter,
    auth_method: &AuthMethod,
    supported_models: Vec<String>,
) -> Result<(), StartupError> {
    let models = ensure_supported_models(auth_method, supported_models);

    match auth_method {
        AuthMethod::SetupToken { token } => {
            // Setup tokens (sk-ant-oat...) are usable directly with the Messages API
            // via Bearer auth. AnthropicProvider::detect() handles the auth mode.
            register_keyed_provider(router, "anthropic", token, "setup_token", models)?;
        }
        AuthMethod::ApiKey { provider, key } => {
            register_api_key_provider(router, provider, key, models)?;
        }
        AuthMethod::OAuth {
            provider,
            access_token,
            account_id,
            ..
        } => {
            register_oauth_provider(
                router,
                provider,
                access_token,
                account_id.as_deref(),
                models,
            )?;
        }
    }

    Ok(())
}

fn register_api_key_provider(
    router: &mut ModelRouter,
    provider: &str,
    key: &str,
    supported_models: Vec<String>,
) -> Result<(), StartupError> {
    register_keyed_provider(router, provider, key, "api_key", supported_models)
}

fn register_keyed_provider(
    router: &mut ModelRouter,
    provider: &str,
    key: &str,
    auth_label: &str,
    supported_models: Vec<String>,
) -> Result<(), StartupError> {
    if provider == "anthropic" {
        let anthropic = AnthropicProvider::new(base_url_for_provider("anthropic"), key.to_string())
            .map_err(|error| {
                StartupError::Router(format!("failed to configure Anthropic provider: {error}"))
            })?
            .with_supported_models(supported_models);
        router.register_provider_with_auth(Arc::new(anthropic), auth_label);
        return Ok(());
    }

    let provider_client = OpenAiProvider::new(base_url_for_provider(provider), key.to_string())
        .map_err(|error| {
            StartupError::Router(format!("failed to configure {provider} provider: {error}"))
        })?
        .with_name(provider.to_string())
        .with_supported_models(supported_models);

    router.register_provider_with_auth(Arc::new(provider_client), auth_label);
    Ok(())
}

fn register_oauth_provider(
    router: &mut ModelRouter,
    provider: &str,
    access_token: &str,
    account_id: Option<&str>,
    supported_models: Vec<String>,
) -> Result<(), StartupError> {
    if let Some(account_id) = account_id {
        let provider_client =
            OpenAiResponsesProvider::new(access_token.to_string(), account_id.to_string())
                .map_err(|error| {
                    StartupError::Router(format!(
                        "failed to configure {provider} Responses provider: {error}"
                    ))
                })?
                .with_supported_models(supported_models);

        router.register_provider_with_auth(Arc::new(provider_client), "subscription");
        return Ok(());
    }

    let provider_client =
        OpenAiProvider::new(base_url_for_provider(provider), access_token.to_string())
            .map_err(|error| {
                StartupError::Router(format!("failed to configure {provider} provider: {error}"))
            })?
            .with_name(provider.to_string())
            .with_supported_models(supported_models);

    router.register_provider_with_auth(Arc::new(provider_client), "subscription");
    Ok(())
}

fn default_supported_models(auth_method: &AuthMethod) -> Vec<String> {
    match auth_method {
        AuthMethod::SetupToken { .. } => to_strings(DEFAULT_ANTHROPIC_MODELS),
        AuthMethod::ApiKey { provider, .. } => models_for_provider(provider),
        AuthMethod::OAuth {
            account_id,
            provider,
            ..
        } => {
            if account_id.is_some() {
                to_strings(DEFAULT_OPENAI_SUBSCRIPTION_MODELS)
            } else {
                models_for_provider(provider)
            }
        }
    }
}

fn ensure_supported_models(auth_method: &AuthMethod, supported_models: Vec<String>) -> Vec<String> {
    if supported_models.is_empty() {
        default_supported_models(auth_method)
    } else {
        supported_models
    }
}

fn base_url_for_provider(provider: &str) -> String {
    let env_key = format!(
        "FAWX_{}_BASE_URL",
        provider.to_ascii_uppercase().replace('-', "_")
    );

    if let Ok(url) = std::env::var(&env_key) {
        if !url.trim().is_empty() {
            return url;
        }
    }

    match provider {
        "anthropic" => "https://api.anthropic.com".to_string(),
        "openrouter" => "https://openrouter.ai/api".to_string(),
        "openai" => "https://api.openai.com".to_string(),
        _ => std::env::var("FAWX_OPENAI_COMPAT_BASE_URL")
            .ok()
            .filter(|url| !url.trim().is_empty())
            .unwrap_or_else(|| "https://api.openai.com".to_string()),
    }
}

fn models_for_provider(provider: &str) -> Vec<String> {
    match provider {
        "anthropic" => to_strings(DEFAULT_ANTHROPIC_MODELS),
        "openrouter" => to_strings(DEFAULT_OPENROUTER_MODELS),
        "openai" => to_strings(DEFAULT_OPENAI_MODELS),
        _ => vec!["gpt-4o-mini".to_string()],
    }
}

fn to_strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn current_time_ms() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration_millis_u64(elapsed)
}

fn duration_millis_u64(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Convert fx-config `PermissionsConfig` to fx-kernel `PermissionPolicy`.
fn permissions_to_policy(config: &fx_config::PermissionsConfig) -> PermissionPolicy {
    let unrestricted = config
        .unrestricted
        .iter()
        .map(|action| action.as_str().to_string())
        .collect();
    let ask_required = config
        .proposal_required
        .iter()
        .map(|action| action.as_str().to_string())
        .collect();
    let has_ask_entries = !config.proposal_required.is_empty();
    let default_ask = match config.mode {
        // Capability mode: only explicitly listed categories are denied.
        // Unknown tools are allowed — don't block builtins like current_time, calculator, etc.
        fx_config::CapabilityMode::Capability => false,
        // Prompt mode: default to asking for unknown categories (legacy behavior).
        fx_config::CapabilityMode::Prompt => has_ask_entries,
    };
    PermissionPolicy {
        unrestricted,
        ask_required,
        default_ask,
        mode: config.mode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::manager::ConfigManager;
    use fx_core::memory::MemoryProvider;
    use fx_embeddings::test_support::create_test_model_dir;
    use fx_subagent::test_support::StubSubagentControl;
    use std::cell::Cell;
    use std::io;
    use std::io::Write;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use tracing::Level;
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::fmt::writer::MakeWriter;

    fn test_config_with_temp_dir() -> (FawxConfig, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let mut config = FawxConfig::default();
        let data_dir = temp_dir.path().join(".fawx");
        std::fs::create_dir_all(&data_dir).expect("data dir");
        config.general.data_dir = Some(data_dir);
        (config, temp_dir)
    }

    fn test_fleet_node_config() -> fx_config::NodeConfig {
        fx_config::NodeConfig {
            id: "mac-mini".to_string(),
            name: "Node Alpha".to_string(),
            endpoint: Some("https://10.0.0.5:8400".to_string()),
            auth_token: Some("token".to_string()),
            capabilities: vec!["agentic_loop".to_string(), "test".to_string()],
            address: Some("10.0.0.5".to_string()),
            user: Some("admin".to_string()),
            ssh_key: Some("~/.ssh/id_ed25519".to_string()),
        }
    }

    #[test]
    fn open_with_retry_retries_lock_errors() {
        let attempts = Cell::new(0);

        let result = open_with_retry("session registry", Path::new("sessions.redb"), || {
            let attempt = attempts.get() + 1;
            attempts.set(attempt);
            if attempt == 1 {
                Err("Database already open. Cannot acquire lock.")
            } else {
                Ok("opened")
            }
        });

        assert_eq!(result.expect("retry should succeed"), "opened");
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn open_with_retry_does_not_retry_non_lock_errors() {
        let attempts = Cell::new(0);

        let result: Result<(), String> =
            open_with_retry("session registry", Path::new("sessions.redb"), || {
                attempts.set(attempts.get() + 1);
                Err("permission denied")
            });

        let error = result.expect_err("non-lock error should fail immediately");
        assert_eq!(error, "permission denied");
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn session_bus_uses_dedicated_bus_database_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let bus = build_session_bus_for_data_dir(temp_dir.path());

        assert!(bus.is_some(), "expected session bus to initialize");
        assert!(temp_dir.path().join("bus.redb").exists());
        assert!(!temp_dir.path().join("sessions.redb").exists());
    }

    #[test]
    fn permissions_to_policy_does_not_default_ask_in_capability_mode() {
        let policy = permissions_to_policy(&fx_config::PermissionsConfig::power());

        assert!(!policy.default_ask);
        assert_eq!(policy.mode, fx_config::CapabilityMode::Capability);
        assert!(policy.ask_required.contains("file_delete"));
    }

    #[test]
    fn permissions_to_policy_keeps_default_ask_in_prompt_mode() {
        let mut config = fx_config::PermissionsConfig::power();
        config.mode = fx_config::CapabilityMode::Prompt;

        let policy = permissions_to_policy(&config);

        assert!(policy.default_ask);
        assert_eq!(policy.mode, fx_config::CapabilityMode::Prompt);
    }

    #[tokio::test]
    async fn stream_notification_sender_drops_lock_before_invoking_callback() {
        let callback_slot = Arc::new(Mutex::new(None));
        let callback_slot_for_callback = Arc::clone(&callback_slot);
        let lock_reacquired = Arc::new(AtomicBool::new(false));
        let lock_reacquired_for_callback = Arc::clone(&lock_reacquired);
        let received = Arc::new(Mutex::new(None));
        let received_for_callback = Arc::clone(&received);
        let callback: StreamCallback = Arc::new(move |event| {
            lock_reacquired_for_callback.store(
                callback_slot_for_callback.try_lock().is_ok(),
                Ordering::SeqCst,
            );
            *received_for_callback.lock().expect("received lock") = Some(event);
        });
        *callback_slot.lock().expect("callback slot lock") = Some(callback);

        let sender = StreamNotificationSender::new(Arc::clone(&callback_slot));
        sender
            .send("Build done", "Task complete")
            .await
            .expect("send should succeed");

        assert!(
            lock_reacquired.load(Ordering::SeqCst),
            "callback should run after releasing the mutex guard"
        );
        assert_eq!(
            *received.lock().expect("received lock"),
            Some(StreamEvent::Notification {
                title: "Build done".to_string(),
                body: "Task complete".to_string(),
            })
        );
    }

    #[tokio::test]
    async fn stream_notification_sender_returns_error_when_callback_mutex_is_poisoned() {
        let callback_slot = Arc::new(Mutex::new(None));
        let callback_slot_for_panic = Arc::clone(&callback_slot);
        let join_result = std::thread::spawn(move || {
            let _guard = callback_slot_for_panic
                .lock()
                .expect("callback slot lock should succeed");
            panic!("poison callback slot");
        })
        .join();

        assert!(
            join_result.is_err(),
            "helper thread should poison the mutex"
        );

        let sender = StreamNotificationSender::new(callback_slot);
        let error = sender
            .send("Build done", "Task complete")
            .await
            .expect_err("poisoned lock should fail");

        assert!(error.contains("notification callback unavailable"));
    }

    fn create_embedding_model_dir(base: &Path, dimensions: usize) {
        let source = create_test_model_dir(dimensions);
        let model_dir = base.join("models").join("nomic-embed-text-v1.5");
        std::fs::create_dir_all(&model_dir).expect("model dir");
        for file_name in [
            "config.json",
            "tokenizer.json",
            "model.safetensors",
            "checksums.sha256",
        ] {
            std::fs::copy(source.path().join(file_name), model_dir.join(file_name))
                .expect("copy model file");
        }
    }

    fn bridge_for(data_dir: &Path) -> CredentialStoreBridge {
        let store = fx_auth::credential_store::EncryptedFileCredentialStore::open(data_dir)
            .expect("credential store");
        CredentialStoreBridge {
            data_dir: data_dir.to_path_buf(),
            store: Arc::new(store),
            token_broker: None,
        }
    }

    fn store_auth_api_key(data_dir: &Path, provider: &str, key: &str) {
        let store = AuthStore::open(data_dir).expect("auth store");
        let mut manager = AuthManager::new();
        manager.store(
            provider,
            AuthMethod::ApiKey {
                provider: provider.to_string(),
                key: key.to_string(),
            },
        );
        store
            .save_auth_manager(&manager)
            .expect("save auth manager");
    }

    fn store_github_pat(data_dir: &Path, token: &str) {
        use fx_auth::credential_store::{
            AuthProvider, CredentialMetadata, CredentialMethod, CredentialStore,
        };
        use zeroize::Zeroizing;

        let metadata = CredentialMetadata {
            provider: AuthProvider::GitHub,
            method: CredentialMethod::Pat,
            last_validated_ms: 0,
            login: None,
            scopes: Vec::new(),
            token_kind: None,
        };
        let store = fx_auth::credential_store::EncryptedFileCredentialStore::open(data_dir)
            .expect("credential store");
        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &Zeroizing::new(token.to_string()),
                &metadata,
            )
            .expect("set github token");
    }

    struct StubTokenBroker {
        token: String,
        default_scope: fx_config::BorrowScope,
    }

    impl StubTokenBroker {
        fn new(token: &str, default_scope: fx_config::BorrowScope) -> Self {
            Self {
                token: token.to_string(),
                default_scope,
            }
        }
    }

    impl fx_auth::token_broker::TokenBroker for StubTokenBroker {
        fn borrow_github(
            &self,
            scope: fx_config::BorrowScope,
        ) -> Result<fx_auth::token_broker::TokenBorrow, fx_auth::token_broker::BorrowError>
        {
            Ok(fx_auth::token_broker::TokenBorrow::new(
                zeroize::Zeroizing::new(self.token.clone()),
                scope,
            ))
        }

        fn borrow_github_default(
            &self,
        ) -> Result<fx_auth::token_broker::TokenBorrow, fx_auth::token_broker::BorrowError>
        {
            self.borrow_github(self.default_scope)
        }
    }

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("capture logs").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct SharedMakeWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for SharedMakeWriter {
        type Writer = SharedWriter;

        fn make_writer(&'a self) -> Self::Writer {
            SharedWriter(Arc::clone(&self.0))
        }
    }

    fn capture_warn_logs<T>(action: impl FnOnce() -> T) -> (T, String) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(Level::WARN)
            .with_ansi(false)
            .without_time()
            .with_writer(SharedMakeWriter(Arc::clone(&buffer)))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let result = action();
        let logs = String::from_utf8(buffer.lock().expect("capture logs").clone())
            .expect("captured logs should be utf8");
        (result, logs)
    }

    fn remove_credential_salt(data_dir: &Path) {
        std::fs::remove_file(data_dir.join(".credentials-salt")).expect("remove salt");
    }

    fn bundle_tool_names(bundle: &LoopEngineBundle) -> Vec<String> {
        bundle
            .skill_registry
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect()
    }

    fn serve_logging_config(log_dir: &std::path::Path) -> LoggingConfig {
        LoggingConfig {
            file_logging: Some(true),
            file_level: Some("trace".to_string()),
            stderr_level: Some("error".to_string()),
            max_files: Some(DEFAULT_MAX_LOG_FILES),
            log_dir: Some(log_dir.display().to_string()),
        }
    }

    fn read_log_output(log_dir: &std::path::Path) -> (Vec<String>, String) {
        let mut files = std::fs::read_dir(log_dir)
            .expect("read log dir")
            .map(|entry| entry.expect("entry").path())
            .collect::<Vec<_>>();
        files.sort();
        let names = files
            .iter()
            .filter_map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(ToString::to_string)
            })
            .collect::<Vec<_>>();
        let contents = files
            .first()
            .map(|path| std::fs::read_to_string(path).expect("read log file"))
            .unwrap_or_default();
        (names, contents)
    }

    fn emit_test_log(dispatch: &Dispatch, message: &str) {
        tracing::dispatcher::with_default(dispatch, || {
            tracing::info!(target: "fx_cli::tests", "{message}");
        });
    }

    #[test]
    fn resolve_logging_config_applies_mode_defaults() {
        let tui = resolve_logging_config(&LoggingConfig::default(), LoggingMode::Tui)
            .expect("resolve TUI logging");
        let serve = resolve_logging_config(&LoggingConfig::default(), LoggingMode::Serve)
            .expect("resolve serve logging");

        assert!(!tui.file_logging);
        assert_eq!(tui.file_level, LevelFilter::INFO);
        assert_eq!(tui.stderr_level, LevelFilter::WARN);
        assert_eq!(tui.max_files, DEFAULT_MAX_LOG_FILES);
        assert_eq!(tui.log_dir, default_log_dir());
        assert!(serve.file_logging);
    }

    #[test]
    fn build_logging_dispatch_creates_log_file_in_expected_directory() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = serve_logging_config(temp_dir.path());
        let resolved = resolve_logging_config(&config, LoggingMode::Serve).expect("resolve");
        let (dispatch, guard) = build_logging_dispatch(&resolved).expect("dispatch");

        emit_test_log(&dispatch, "hello persistent logs");
        drop(guard);

        let (files, contents) = read_log_output(temp_dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].starts_with("fawx."));
        assert!(files[0].ends_with(".log"));
        assert!(contents.contains("hello persistent logs"));
    }

    #[test]
    fn build_logging_dispatch_writes_file_output_without_ansi_codes() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = serve_logging_config(temp_dir.path());
        let resolved = resolve_logging_config(&config, LoggingMode::Serve).expect("resolve");
        let (dispatch, guard) = build_logging_dispatch(&resolved).expect("dispatch");

        emit_test_log(&dispatch, "ansi free output");
        drop(guard);

        let (_, contents) = read_log_output(temp_dir.path());
        assert!(!contents.contains("\u{1b}["));
        assert!(contents.contains('T'));
        assert!(contents.contains('Z'));
    }

    #[test]
    fn build_logging_dispatch_skips_files_when_file_logging_disabled() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = LoggingConfig {
            file_logging: Some(false),
            log_dir: Some(temp_dir.path().display().to_string()),
            ..LoggingConfig::default()
        };
        let resolved = resolve_logging_config(&config, LoggingMode::Serve).expect("resolve");
        let (dispatch, guard) = build_logging_dispatch(&resolved).expect("dispatch");

        emit_test_log(&dispatch, "stderr only");
        drop(guard);

        let entries = std::fs::read_dir(temp_dir.path())
            .expect("read temp dir")
            .map(|entry| entry.expect("entry").path())
            .collect::<Vec<_>>();
        assert!(entries.is_empty());
    }

    #[test]
    fn build_logging_dispatch_creates_missing_log_directory() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let log_dir = temp_dir.path().join("nested").join("logs");
        let config = serve_logging_config(&log_dir);
        let resolved = resolve_logging_config(&config, LoggingMode::Serve).expect("resolve");
        let (_dispatch, guard) = build_logging_dispatch(&resolved).expect("dispatch");

        drop(guard);

        assert!(log_dir.is_dir());
    }

    #[test]
    fn cleanup_old_logs_keeps_newest_files() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        for name in [
            "fawx.2026-03-01.log",
            "fawx.2026-03-02.log",
            "fawx.2026-03-03.log",
            "fawx.2026-03-04.log",
        ] {
            std::fs::write(temp_dir.path().join(name), name).expect("write log file");
        }

        cleanup_old_logs(temp_dir.path(), 2);

        let (files, _) = read_log_output(temp_dir.path());
        assert_eq!(files, vec!["fawx.2026-03-03.log", "fawx.2026-03-04.log"]);
    }

    #[test]
    fn cleanup_old_logs_is_best_effort_when_removal_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let stuck_log = temp_dir.path().join("fawx.2026-03-01.log");
        std::fs::create_dir(&stuck_log).expect("create stuck log directory");
        let newest_log = temp_dir.path().join("fawx.2026-03-02.log");
        std::fs::write(&newest_log, "newest").expect("write newest log file");

        cleanup_old_logs(temp_dir.path(), 1);

        assert!(stuck_log.exists());
        assert!(newest_log.exists());
    }

    #[test]
    fn resolve_log_dir_trusts_config_layer_expansion() {
        let config = LoggingConfig {
            log_dir: Some("~/already-handled-by-config".to_string()),
            ..LoggingConfig::default()
        };

        assert_eq!(
            resolve_log_dir(&config),
            PathBuf::from("~/already-handled-by-config")
        );
    }

    #[test]
    fn resolve_log_level_accepts_all_supported_values() {
        for value in ["error", "warn", "info", "debug", "trace"] {
            assert!(
                resolve_log_level(Some(value), DEFAULT_FILE_LEVEL).is_ok(),
                "{value}"
            );
        }
    }

    #[test]
    fn resolve_log_level_rejects_invalid_values() {
        let error = resolve_log_level(Some("verbose"), DEFAULT_FILE_LEVEL).unwrap_err();
        assert!(error.to_string().contains("invalid log level 'verbose'"));
    }

    #[test]
    fn is_dated_log_file_rejects_invalid_calendar_dates() {
        assert!(is_dated_log_file(Path::new("fawx.2026-03-09.log")));
        assert!(!is_dated_log_file(Path::new("fawx.2026-13-09.log")));
        assert!(!is_dated_log_file(Path::new("fawx.2026-03-32.log")));
        assert!(!is_dated_log_file(Path::new("fawx.2026-02-30.log")));
    }

    #[test]
    fn worker_guard_flushes_pending_logs_on_drop() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = serve_logging_config(temp_dir.path());
        let resolved = resolve_logging_config(&config, LoggingMode::Serve).expect("resolve");
        let (dispatch, guard) = build_logging_dispatch(&resolved).expect("dispatch");

        emit_test_log(&dispatch, "flush on drop");
        drop(guard);

        let (_, contents) = read_log_output(temp_dir.path());
        assert!(contents.contains("flush on drop"));
    }

    #[test]
    fn loop_engine_from_config_with_options_includes_tui_parity_tools() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let control = Arc::new(StubSubagentControl::new());
        let config_path = configured_data_dir(&fawx_data_dir(), &config).join("config.toml");
        let config_manager = Arc::new(Mutex::new(ConfigManager::from_config(
            config.clone(),
            config_path,
        )));
        let options = HeadlessLoopBuildOptions {
            memory_enabled: true,
            subagent_control: Some(control),
            config_manager: Some(config_manager),
            ..HeadlessLoopBuildOptions::default()
        };

        let bundle = build_loop_engine_from_config_with_options(&config, None, options)
            .expect("bundle should build");
        let names = bundle_tool_names(&bundle);

        assert!(names.contains(&"spawn_agent".to_string()));
        assert!(names.contains(&"subagent_status".to_string()));
        assert!(names.contains(&"config_get".to_string()));
        assert!(names.contains(&"config_set".to_string()));
    }

    #[test]
    fn headless_bundle_includes_subagent_tools_when_control_attached() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let control = Arc::new(StubSubagentControl::new());
        let bundle = build_headless_loop_engine_bundle(
            &config,
            None,
            HeadlessLoopBuildOptions::parent(control),
        )
        .expect("bundle should build");
        let names = bundle_tool_names(&bundle);

        assert!(names.contains(&"spawn_agent".to_string()));
        assert!(names.contains(&"subagent_status".to_string()));
    }

    #[test]
    fn headless_subagent_bundle_excludes_subagent_tools() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let bundle = build_headless_loop_engine_bundle(
            &config,
            None,
            HeadlessLoopBuildOptions::subagent(None, CancellationToken::new()),
        )
        .expect("bundle should build");
        let names = bundle_tool_names(&bundle);

        assert!(!names.contains(&"spawn_agent".to_string()));
        assert!(!names.contains(&"subagent_status".to_string()));
    }

    #[test]
    fn headless_bundle_uses_token_broker_for_github_credentials() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        store_github_pat(&data_dir, "ghp-store-token");
        let bundle = build_headless_loop_engine_bundle(
            &config,
            None,
            HeadlessLoopBuildOptions {
                token_broker: Some(Arc::new(StubTokenBroker::new(
                    "ghp-broker-token",
                    fx_config::BorrowScope::Contribution,
                ))),
                ..HeadlessLoopBuildOptions::default()
            },
        )
        .expect("bundle should build");
        let provider = bundle
            .credential_provider
            .expect("credential provider should be available");
        let token = provider
            .get_credential("github_token")
            .expect("github token should be available");

        assert_eq!(*token, "ghp-broker-token");
    }

    #[test]
    fn headless_bundle_reuses_supplied_credential_store_for_github_credentials() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        store_github_pat(&data_dir, "ghp-store-token");
        let credential_store =
            open_credential_store(&data_dir).expect("shared credential store should open");
        let bundle = build_headless_loop_engine_bundle(
            &config,
            None,
            HeadlessLoopBuildOptions {
                credential_store: Some(credential_store),
                ..HeadlessLoopBuildOptions::default()
            },
        )
        .expect("bundle should build");
        let provider = bundle
            .credential_provider
            .expect("credential provider should be available");
        let token = provider
            .get_credential("github_token")
            .expect("github token should be available");

        assert_eq!(*token, "ghp-store-token");
    }

    #[test]
    fn headless_bundle_includes_node_run_when_fleet_nodes_configured() {
        let (mut config, _temp_dir) = test_config_with_temp_dir();
        config.fleet.nodes.push(test_fleet_node_config());

        let bundle =
            build_headless_loop_engine_bundle(&config, None, HeadlessLoopBuildOptions::default())
                .expect("bundle should build");
        let names = bundle_tool_names(&bundle);

        assert!(names.contains(&"node_run".to_string()));
    }

    #[test]
    fn headless_bundle_excludes_node_run_without_fleet_nodes() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let bundle =
            build_headless_loop_engine_bundle(&config, None, HeadlessLoopBuildOptions::default())
                .expect("bundle should build");
        let names = bundle_tool_names(&bundle);

        assert!(!names.contains(&"node_run".to_string()));
    }

    #[cfg(feature = "http")]
    #[test]
    fn build_tool_executor_attaches_experiment_registrar_when_registry_supplied() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let working_dir = temp_dir.path().to_path_buf();
        let options = SkillRegistryBuildOptions {
            working_dir: working_dir.clone(),
            memory_enabled: false,
            subagent_control: None,
            config_manager: None,
            experiment_progress: None,
            session_registry: None,
            session_bus: None,
            kernel_budget: BudgetConfig::default(),
            stream_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            credential_store: None,
            token_broker: None,
            experiment_registry: Some(
                build_shared_experiment_registry(temp_dir.path()).expect("shared registry"),
            ),
        };
        let process_registry = Arc::new(ProcessRegistry::new(ProcessConfig {
            allowed_dirs: vec![working_dir],
            ..ProcessConfig::default()
        }));

        let executor = build_tool_executor(&options, ToolConfig::default(), process_registry);
        let debug = format!("{executor:?}");

        assert!(debug.contains("experiment_registrar: true"), "{debug}");
        assert!(debug.contains("background_experiments: true"), "{debug}");
    }

    #[test]
    fn headless_bundle_without_session_registry_skips_session_tools_and_lock() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = configured_data_dir(&fawx_data_dir(), &config);
        let bundle = build_headless_loop_engine_bundle(
            &config,
            None,
            HeadlessLoopBuildOptions {
                session_registry: None,
                session_bus: None,
                permission_prompt_state: None,
                ..HeadlessLoopBuildOptions::default()
            },
        )
        .expect("bundle should build");
        let names = bundle_tool_names(&bundle);

        for session_tool_name in ["session_list", "session_history", "session_send"] {
            assert!(!names.contains(&session_tool_name.to_string()));
        }

        let registry = open_session_registry(&data_dir);
        assert!(
            registry.is_some(),
            "session database should remain unlockable"
        );
    }

    #[test]
    fn headless_bundle_registers_session_memory_tool() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let bundle =
            build_headless_loop_engine_bundle(&config, None, HeadlessLoopBuildOptions::default())
                .expect("bundle should build");
        let names = bundle_tool_names(&bundle);

        assert!(names.contains(&"update_session_memory".to_string()));
    }

    #[test]
    fn headless_bundle_with_session_registry_registers_session_tools() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = configured_data_dir(&fawx_data_dir(), &config);
        let bundle = build_headless_loop_engine_bundle(
            &config,
            None,
            HeadlessLoopBuildOptions {
                session_registry: open_session_registry(&data_dir),
                ..HeadlessLoopBuildOptions::default()
            },
        )
        .expect("bundle should build");
        let names = bundle_tool_names(&bundle);

        for session_tool_name in ["session_list", "session_history", "session_send"] {
            assert!(names.contains(&session_tool_name.to_string()));
        }
    }

    #[test]
    fn headless_bundle_accumulates_startup_warning_for_broken_memory_path() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        let memory_dir = data_dir.join("memory");
        std::fs::create_dir_all(&memory_dir).expect("create memory dir");
        std::fs::write(memory_dir.join("memory.json"), b"not valid json")
            .expect("write broken memory");

        let bundle = build_headless_loop_engine_bundle(
            &config,
            None,
            HeadlessLoopBuildOptions {
                memory_enabled: true,
                ..HeadlessLoopBuildOptions::default()
            },
        )
        .expect("bundle should build");

        assert!(bundle.startup_warnings.iter().any(|warning| {
            warning.category == ErrorCategory::Memory
                && warning.message.contains("Failed to initialize memory")
        }));
    }

    #[test]
    fn headless_bundle_builds_embedding_index_from_existing_memory() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        let mut memory = JsonFileMemory::new(&data_dir).expect("memory");
        memory.write("auth", "hello world").expect("write memory");
        create_embedding_model_dir(&data_dir, 8);

        let bundle = build_headless_loop_engine_bundle(
            &config,
            None,
            HeadlessLoopBuildOptions {
                memory_enabled: true,
                ..HeadlessLoopBuildOptions::default()
            },
        )
        .expect("bundle should build");
        let names = bundle_tool_names(&bundle);
        let persistence = bundle
            .embedding_index_persistence
            .as_ref()
            .expect("embedding index should be configured");
        let results = persistence
            .index
            .lock()
            .expect("lock index")
            .search("hello world", 5)
            .expect("search index");

        assert!(results.iter().any(|(key, _)| key == "auth"));
        assert!(names.contains(&"memory_search".to_string()));
    }

    #[test]
    fn headless_bundle_skips_embedding_index_when_disabled_in_config() {
        let (mut config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        create_embedding_model_dir(&data_dir, 8);
        config.memory.embeddings_enabled = false;

        let bundle = build_headless_loop_engine_bundle(
            &config,
            None,
            HeadlessLoopBuildOptions {
                memory_enabled: true,
                ..HeadlessLoopBuildOptions::default()
            },
        )
        .expect("bundle should build");

        assert!(bundle.embedding_index_persistence.is_none());
    }

    #[test]
    fn build_loop_engine_from_builder_returns_startup_error_on_failure() {
        let error = build_loop_engine_from_builder(LoopEngine::builder())
            .expect_err("missing required fields should return an error");

        match error {
            StartupError::Loop(message) => assert!(message.contains("missing_required_field")),
            other => panic!("expected StartupError::Loop, got {other:?}"),
        }
    }

    #[test]
    fn default_anthropic_models_include_claude_opus_4_6() {
        assert!(DEFAULT_ANTHROPIC_MODELS.contains(&"claude-opus-4-6"));
    }

    #[test]
    fn build_router_with_setup_token_registers_anthropic_models() {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "sk-ant-oat01-test-setup-token".to_string(),
            },
        );

        let router = build_router(&auth_manager).expect("router should build");
        let models = router.available_models();

        assert!(
            models
                .iter()
                .any(|model| model.provider_name == "anthropic"
                    && model.auth_method == "setup_token"),
            "setup tokens should register Anthropic models with setup_token auth label"
        );
    }

    #[test]
    fn build_router_with_mixed_setup_token_and_api_key() {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "sk-ant-oat01-test-mixed".to_string(),
            },
        );
        auth_manager.store(
            "openrouter",
            AuthMethod::ApiKey {
                provider: "openrouter".to_string(),
                key: "openrouter-key-mixed".to_string(),
            },
        );

        let router = build_router(&auth_manager).expect("router should build");
        let models = router.available_models();

        assert!(
            models
                .iter()
                .any(|model| model.provider_name == "anthropic"),
            "setup token should register Anthropic models"
        );
        assert!(
            models
                .iter()
                .any(|model| model.provider_name == "openrouter" && model.auth_method == "api_key"),
            "API key should register OpenRouter models"
        );
    }

    #[test]
    fn build_router_with_oauth_credentials_registers_openai_subscription_models() {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "oauth-access-token".to_string(),
                refresh_token: "oauth-refresh-token".to_string(),
                expires_at: 1_700_000_000_000,
                account_id: Some("acct_oauth_router_test".to_string()),
            },
        );

        let router = build_router(&auth_manager).expect("router should build");
        let openai_models = router
            .available_models()
            .into_iter()
            .filter(|model| model.provider_name == "openai")
            .collect::<Vec<_>>();

        assert!(!openai_models.is_empty());
        assert!(openai_models
            .iter()
            .all(|model| model.auth_method == "subscription"));
        assert!(openai_models
            .iter()
            .any(|model| model.model_id == "gpt-5.3-codex"));
    }

    #[test]
    fn build_router_with_setup_token_only_registers_anthropic_models() {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "sk-ant-oat01-test-token-only".to_string(),
            },
        );

        let router = build_router(&auth_manager).expect("router should build");

        assert!(
            !router.available_models().is_empty(),
            "setup token should register Anthropic models"
        );
    }

    #[test]
    fn credential_bridge_returns_openai_api_key_from_auth_manager() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        store_auth_api_key(&data_dir, "openai", "sk-openai-123");

        let bridge = bridge_for(&data_dir);
        let value = bridge
            .get_credential("openai_api_key")
            .expect("openai api key should be available");

        assert_eq!(*value, "sk-openai-123");
    }

    #[test]
    fn credential_bridge_returns_anthropic_api_key_from_auth_manager() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        store_auth_api_key(&data_dir, "anthropic", "sk-ant-123");

        let bridge = bridge_for(&data_dir);
        let value = bridge
            .get_credential("anthropic_api_key")
            .expect("anthropic api key should be available");

        assert_eq!(*value, "sk-ant-123");
    }

    #[test]
    fn credential_bridge_returns_none_for_missing_unknown_key() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        let bridge = bridge_for(&data_dir);

        assert!(bridge.get_credential("missing_skill_key").is_none());
    }

    #[test]
    fn credential_bridge_falls_back_to_generic_skill_credentials() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        let store = fx_auth::credential_store::EncryptedFileCredentialStore::open(&data_dir)
            .expect("credential store");
        store
            .set_generic("brave_api_key", "brv-bridge-test")
            .expect("set generic credential");
        drop(store);

        let bridge = bridge_for(&data_dir);
        let value = bridge
            .get_credential("brave_api_key")
            .expect("generic skill credential should be available");

        assert_eq!(*value, "brv-bridge-test");
    }

    #[test]
    fn credential_bridge_warns_when_github_token_read_fails() {
        use fx_auth::credential_store::{
            AuthProvider, CredentialMetadata, CredentialMethod, CredentialStore,
        };
        use zeroize::Zeroizing;

        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        let store = fx_auth::credential_store::EncryptedFileCredentialStore::open(&data_dir)
            .expect("credential store");
        let metadata = CredentialMetadata {
            provider: AuthProvider::GitHub,
            method: CredentialMethod::Pat,
            last_validated_ms: 0,
            login: None,
            scopes: Vec::new(),
            token_kind: None,
        };
        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &Zeroizing::new("ghp-bridge-test".to_string()),
                &metadata,
            )
            .expect("set github token");
        drop(store);
        remove_credential_salt(&data_dir);

        let bridge = bridge_for(&data_dir);
        let (value, logs) = capture_warn_logs(|| bridge.github_token());

        assert!(value.is_none());
        assert!(logs
            .contains("failed to read GitHub token while resolving credential bridge credential"));
    }

    #[test]
    fn credential_bridge_warns_when_generic_credential_read_fails() {
        let (config, _temp_dir) = test_config_with_temp_dir();
        let data_dir = config.general.data_dir.clone().expect("data dir");
        let store = fx_auth::credential_store::EncryptedFileCredentialStore::open(&data_dir)
            .expect("credential store");
        store
            .set_generic("brave_api_key", "brv-bridge-test")
            .expect("set generic credential");
        drop(store);
        remove_credential_salt(&data_dir);

        let bridge = bridge_for(&data_dir);
        let (value, logs) = capture_warn_logs(|| bridge.get_credential("brave_api_key"));

        assert!(value.is_none());
        assert!(logs.contains(
            "failed to read generic credential while resolving credential bridge credential"
        ));
        assert!(logs.contains("brave_api_key"));
    }

    #[test]
    fn duration_millis_u64_clamps_on_overflow() {
        assert_eq!(
            duration_millis_u64(std::time::Duration::from_secs(u64::MAX)),
            u64::MAX
        );
    }

    /// Tests env var override and fallback in a single test to avoid
    /// parallel test races on the shared `FAWX_DATA_DIR` env var.
    #[test]
    fn fawx_data_dir_respects_env_var_and_falls_back() {
        let key = "FAWX_DATA_DIR";
        let original = std::env::var_os(key);

        // With env var set: should return the override path
        std::env::set_var(key, "/tmp/custom-fawx-data");
        let with_env = fawx_data_dir();
        assert_eq!(with_env, PathBuf::from("/tmp/custom-fawx-data"));

        // Without env var: should fall back to ~/.fawx
        std::env::remove_var(key);
        let without_env = fawx_data_dir();
        assert!(
            without_env.ends_with(".fawx"),
            "should fall back to ~/.fawx, got: {without_env:?}"
        );

        // Restore original value
        if let Some(val) = original {
            std::env::set_var(key, val);
        }
    }
}
