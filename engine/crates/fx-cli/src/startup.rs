use crate::auth_store::{migrate_if_needed, AuthStore};
use crate::helpers::{format_memory_for_prompt, thinking_config_from_budget};
use async_trait::async_trait;
use fx_analysis::AnalysisError;
use fx_auth::auth::{AuthManager, AuthMethod};
use fx_auth::credential_store::CredentialStore as CredentialStoreTrait;
use fx_config::manager::ConfigManager;
use fx_config::{FawxConfig, ImprovementToolsConfig};
use fx_core::memory::{MemoryProvider, MemoryStore};
use fx_core::runtime_info::{ConfigSummary, RuntimeInfo, SkillInfo};
use fx_core::EventBus;
use fx_fleet::{NodeRegistry, NodeTransport, SshTransport};
use fx_journal::JournalSkill;
use fx_kernel::act::ToolExecutor;
use fx_kernel::budget::{BudgetConfig, BudgetTracker};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::context_manager::ContextCompactor;
use fx_kernel::loop_engine::{LoopEngine, LoopEngineBuilder, ScratchpadProvider};
use fx_kernel::{CachingExecutor, ProposalGateExecutor, ProposalGateState};
use fx_llm::{
    AnthropicProvider, CompletionRequest, ModelRouter, OpenAiProvider, OpenAiResponsesProvider,
};
use fx_loadable::{SignaturePolicy, SkillRegistry, TransactionSkill};
use fx_memory::{JsonFileMemory, JsonMemoryConfig, SignalStore};
use fx_scratchpad::skill::ScratchpadSkill;
use fx_scratchpad::Scratchpad;
use fx_skills::live_host_api::CredentialProvider;
use fx_subagent::SubagentControl;
use fx_tools::{
    BuiltinToolsSkill, FawxToolExecutor, GitSkill, ImprovementToolsState, NodeRunState,
    SessionToolsSkill, ToolConfig,
};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

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
const DEFAULT_OPENAI_MODELS: &[&str] = &["gpt-4.1", "gpt-4o", "gpt-4o-mini"];
const DEFAULT_OPENAI_SUBSCRIPTION_MODELS: &[&str] =
    &["gpt-5.3-codex", "gpt-5.2", "gpt-5.1", "o4-mini"];
const DEFAULT_OPENROUTER_MODELS: &[&str] = &[
    "openai/gpt-4o-mini",
    "anthropic/claude-3.5-sonnet",
    "google/gemini-2.0-flash-001",
];

pub(crate) type SharedMemoryStore = Arc<Mutex<dyn MemoryStore>>;

/// Load the user config from ~/.fawx/config.toml (or return defaults).
pub fn load_config() -> Result<FawxConfig, StartupError> {
    let base_data_dir = fawx_data_dir();
    FawxConfig::load(&base_data_dir).map_err(StartupError::Store)
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
}

#[derive(Clone, Default)]
pub struct HeadlessLoopBuildOptions {
    pub working_dir: Option<PathBuf>,
    pub memory_enabled: bool,
    pub subagent_control: Option<Arc<dyn SubagentControl>>,
    pub config_manager: Option<Arc<Mutex<ConfigManager>>>,
    pub cancel_token: Option<CancellationToken>,
}

impl HeadlessLoopBuildOptions {
    pub fn parent(subagent_control: Arc<dyn SubagentControl>) -> Self {
        Self {
            working_dir: None,
            memory_enabled: true,
            subagent_control: Some(subagent_control),
            config_manager: None,
            cancel_token: None,
        }
    }

    pub fn subagent(working_dir: Option<PathBuf>, cancel_token: CancellationToken) -> Self {
        Self {
            working_dir,
            memory_enabled: false,
            subagent_control: None,
            config_manager: None,
            cancel_token: Some(cancel_token),
        }
    }
}

#[derive(Clone)]
struct SkillRegistryBuildOptions {
    working_dir: PathBuf,
    memory_enabled: bool,
    subagent_control: Option<Arc<dyn SubagentControl>>,
    config_manager: Option<Arc<Mutex<ConfigManager>>>,
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

/// Capacity of the streaming event bus broadcast channel.
const EVENT_BUS_CAPACITY: usize = 256;

fn build_loop_engine_with_options(
    data_dir: PathBuf,
    config: FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    options: HeadlessLoopBuildOptions,
) -> Result<LoopEngineBundle, StartupError> {
    let event_bus = EventBus::new(EVENT_BUS_CAPACITY);
    let budget = BudgetTracker::new(BudgetConfig::default(), current_time_ms(), 0);
    let context = ContextCompactor::new(DEFAULT_CONTEXT_MAX_TOKENS, DEFAULT_CONTEXT_COMPACT_TARGET);
    let registry_options = build_skill_registry_options(&config, &options);
    let working_dir = registry_options.working_dir.clone();
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

    // Build ProposalGateExecutor to wrap the CachingExecutor.
    // Chain: kernel → ProposalGateExecutor → CachingExecutor → SkillRegistry
    let self_modify_config = crate::config_bridge::to_core_self_modify(&config.self_modify);
    let proposals_dir = data_dir.join("proposals");
    let gate_state = ProposalGateState::new(self_modify_config, working_dir.clone(), proposals_dir);
    let tool_executor: Arc<dyn ToolExecutor> =
        Arc::new(ProposalGateExecutor::new(caching_registry, gate_state));

    let mut builder = LoopEngine::builder()
        .budget(budget)
        .context(context)
        .max_iterations(config.general.max_iterations)
        .tool_executor(Arc::clone(&tool_executor))
        .synthesis_instruction(synthesis)
        .event_bus(event_bus.clone())
        .iteration_counter(Arc::clone(&skills.iteration_counter))
        .scratchpad_provider(bridge);
    if let Some(cancel_token) = options.cancel_token {
        builder = builder.cancel_token(cancel_token);
    }
    if let Some(snapshot_text) = skills.memory_snapshot {
        builder = builder.memory_context(snapshot_text);
    }
    let thinking_budget = config.general.thinking.unwrap_or_default();
    if let Some(thinking) = thinking_config_from_budget(&thinking_budget) {
        builder = builder.thinking_config(thinking);
    }

    let engine = build_loop_engine_from_builder(builder)?;
    Ok(LoopEngineBundle {
        engine,
        memory: skills.memory,
        runtime_info: skills.runtime_info,
        event_bus,
        scratchpad: skills.scratchpad,
        skill_registry: skills.registry,
        credential_provider: skills.credential_provider,
        tool_executor,
        credential_store: skills.credential_store,
        config_manager: options.config_manager.clone(),
        signature_policy: skills.signature_policy,
    })
}

fn build_skill_registry_options(
    config: &FawxConfig,
    options: &HeadlessLoopBuildOptions,
) -> SkillRegistryBuildOptions {
    SkillRegistryBuildOptions {
        working_dir: options
            .working_dir
            .clone()
            .unwrap_or_else(|| configured_working_dir(config)),
        memory_enabled: options.memory_enabled,
        subagent_control: options.subagent_control.clone(),
        config_manager: options.config_manager.clone(),
    }
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

/// Bridges the encrypted credential store to the [`CredentialProvider`]
/// trait so WASM skills can retrieve secrets via `kv_get`.
///
/// Maps well-known key names to credential store lookups:
/// - `"github_token"` → GitHub PAT from the encrypted store
struct CredentialStoreBridge {
    store: Arc<fx_auth::credential_store::EncryptedFileCredentialStore>,
}

impl CredentialProvider for CredentialStoreBridge {
    fn get_credential(&self, key: &str) -> Option<zeroize::Zeroizing<String>> {
        use fx_auth::credential_store::{AuthProvider, CredentialMethod};
        match key {
            "github_token" => self
                .store
                .get(AuthProvider::GitHub, CredentialMethod::Pat)
                .ok()
                .flatten(),
            _ => None,
        }
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
    memory_snapshot: Option<String>,
    runtime_info: Arc<RwLock<RuntimeInfo>>,
    scratchpad: Arc<Mutex<Scratchpad>>,
    iteration_counter: Arc<std::sync::atomic::AtomicU32>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    credential_store: Option<Arc<fx_auth::credential_store::EncryptedFileCredentialStore>>,
    /// Signature policy loaded once at startup, shared with the skill watcher
    /// to avoid redundant filesystem reads.
    signature_policy: SignaturePolicy,
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
    let executor = FawxToolExecutor::new(options.working_dir.clone(), tool_config);
    let (mut executor, memory, snapshot_text, memory_enabled) =
        attach_memory_if_enabled(executor, data_dir, config, options.memory_enabled);

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
    let git_skill = GitSkill::new(options.working_dir.clone(), sm.clone());
    registry.register(Arc::new(git_skill));
    let tx_skill = TransactionSkill::new(options.working_dir.clone(), sm);
    registry.register(Arc::new(tx_skill));
    let scratchpad = Arc::new(Mutex::new(Scratchpad::new()));
    let iteration_counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let scratchpad_skill =
        ScratchpadSkill::new(Arc::clone(&scratchpad), Arc::clone(&iteration_counter));
    registry.register(Arc::new(scratchpad_skill));

    // Register session management tools.
    let session_db_path = data_dir.join("sessions.redb");
    match fx_storage::Storage::open(&session_db_path) {
        Ok(storage) => {
            let store = fx_session::SessionStore::new(storage);
            match fx_session::SessionRegistry::new(store) {
                Ok(session_registry) => {
                    let session_skill = SessionToolsSkill::new(session_registry);
                    registry.register(Arc::new(session_skill));
                }
                Err(e) => {
                    tracing::warn!("session registry unavailable: {e}");
                }
            }
        }
        Err(e) => {
            tracing::warn!("session storage unavailable: {e}");
        }
    }

    // Load reflective journal for cross-session learning.
    let journal_path = data_dir.join("journal.jsonl");
    match fx_journal::Journal::load(journal_path) {
        Ok(journal) => {
            let journal_skill = JournalSkill::new(Arc::new(Mutex::new(journal)));
            registry.register(Arc::new(journal_skill));
        }
        Err(e) => {
            tracing::warn!("journal unavailable: {e}");
        }
    }

    // Open the credential store once and share via Arc between TuiApp and WASM bridge.
    let credential_store: Option<Arc<fx_auth::credential_store::EncryptedFileCredentialStore>> =
        match fx_auth::credential_store::EncryptedFileCredentialStore::open(data_dir) {
            Ok(store) => Some(Arc::new(store)),
            Err(e) => {
                tracing::warn!("credential store unavailable: {e}");
                None
            }
        };

    let credential_provider: Option<Arc<dyn CredentialProvider>> =
        credential_store.as_ref().map(|store| {
            Arc::new(CredentialStoreBridge {
                store: Arc::clone(store),
            }) as Arc<dyn CredentialProvider>
        });

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

    apply_skill_summaries(&runtime_info, registry.as_ref());

    SkillRegistryBundle {
        registry,
        memory,
        memory_snapshot: snapshot_text,
        runtime_info,
        scratchpad,
        iteration_counter,
        credential_provider,
        credential_store,
        signature_policy,
    }
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
) -> (
    FawxToolExecutor,
    Option<SharedMemoryStore>,
    Option<String>,
    bool,
) {
    if !enabled {
        return (executor, None, None, false);
    }
    let memory_config = JsonMemoryConfig {
        max_entries: config.memory.max_entries,
        max_value_size: config.memory.max_value_size,
        decay_config: fx_memory::DecayConfig::default(),
    };
    match JsonFileMemory::new_with_config(data_dir, memory_config) {
        Ok(mut memory_store) => {
            let pruned = memory_store.prune();
            if pruned > 0 {
                eprintln!("memory: pruned {pruned} stale entries at session start");
            }
            let snapshot = memory_store.snapshot();
            let text = format_memory_for_prompt(&snapshot, config.memory.max_snapshot_chars);
            let memory: SharedMemoryStore = Arc::new(Mutex::new(memory_store));
            executor = executor.with_memory(Arc::clone(&memory));
            (executor, Some(memory), text, true)
        }
        Err(error) => {
            eprintln!("warning: failed to initialize memory: {error}");
            (executor, None, None, false)
        }
    }
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
        .map(|(name, tool_names)| SkillInfo { name, tool_names })
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

    if let Some(first_model) = router
        .available_models()
        .into_iter()
        .next()
        .map(|model| model.model_id)
    {
        if let Err(error) = router.set_active(&first_model) {
            eprintln!("failed to set initial model {first_model}: {error}");
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
            register_setup_token_provider(router, token, models)?;
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

fn ensure_supported_models(auth_method: &AuthMethod, supported_models: Vec<String>) -> Vec<String> {
    if supported_models.is_empty() {
        default_supported_models(auth_method)
    } else {
        supported_models
    }
}

fn register_setup_token_provider(
    router: &mut ModelRouter,
    token: &str,
    supported_models: Vec<String>,
) -> Result<(), StartupError> {
    let provider = AnthropicProvider::new(base_url_for_provider("anthropic"), token.to_string())
        .map_err(|error| {
            StartupError::Router(format!("failed to configure Anthropic provider: {error}"))
        })?
        .with_supported_models(supported_models);

    router.register_provider_with_auth(Box::new(provider), "subscription");
    Ok(())
}

fn register_api_key_provider(
    router: &mut ModelRouter,
    provider: &str,
    key: &str,
    supported_models: Vec<String>,
) -> Result<(), StartupError> {
    if provider == "anthropic" {
        let anthropic = AnthropicProvider::new(base_url_for_provider("anthropic"), key.to_string())
            .map_err(|error| {
                StartupError::Router(format!("failed to configure Anthropic provider: {error}"))
            })?
            .with_supported_models(supported_models);
        router.register_provider_with_auth(Box::new(anthropic), "api_key");
        return Ok(());
    }

    let provider_client = OpenAiProvider::new(base_url_for_provider(provider), key.to_string())
        .map_err(|error| {
            StartupError::Router(format!("failed to configure {provider} provider: {error}"))
        })?
        .with_name(provider.to_string())
        .with_supported_models(supported_models);

    router.register_provider_with_auth(Box::new(provider_client), "api_key");
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

        router.register_provider_with_auth(Box::new(provider_client), "subscription");
        return Ok(());
    }

    let provider_client =
        OpenAiProvider::new(base_url_for_provider(provider), access_token.to_string())
            .map_err(|error| {
                StartupError::Router(format!("failed to configure {provider} provider: {error}"))
            })?
            .with_name(provider.to_string())
            .with_supported_models(supported_models);

    router.register_provider_with_auth(Box::new(provider_client), "subscription");
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

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::manager::ConfigManager;
    use fx_subagent::test_support::StubSubagentControl;
    use std::sync::{Arc, Mutex};

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
            name: "Mac Mini".to_string(),
            endpoint: Some("https://10.0.0.5:8400".to_string()),
            auth_token: Some("token".to_string()),
            capabilities: vec!["agentic_loop".to_string(), "test".to_string()],
            address: Some("10.0.0.5".to_string()),
            user: Some("joseph".to_string()),
            ssh_key: Some("~/.ssh/id_ed25519".to_string()),
        }
    }

    fn bundle_tool_names(bundle: &LoopEngineBundle) -> Vec<String> {
        bundle
            .skill_registry
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect()
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
    fn build_router_with_mixed_credentials_sets_expected_models_and_auth_labels() {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "setup-token-mixed".to_string(),
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

        assert!(models.iter().any(|model| {
            model.provider_name == "anthropic" && model.auth_method == "subscription"
        }));
        assert!(models.iter().any(|model| {
            model.model_id == "openai/gpt-4o-mini"
                && model.provider_name == "openrouter"
                && model.auth_method == "api_key"
        }));
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
    fn duration_millis_u64_clamps_on_overflow() {
        assert_eq!(
            duration_millis_u64(std::time::Duration::from_secs(u64::MAX)),
            u64::MAX
        );
    }
}
