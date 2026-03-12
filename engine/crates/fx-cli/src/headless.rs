//! Headless mode for Fawx — stdin/stdout REPL without the TUI.
//!
//! Provides `HeadlessApp` which drives the full agentic loop via
//! `LoopEngine::run_cycle()` while reading input from stdin and writing
//! responses to stdout. All diagnostic/error output goes to stderr so
//! downstream consumers can safely pipe stdout.

use async_trait::async_trait;
use fx_analysis::{AnalysisEngine, AnalysisError, AnalysisFinding, Confidence};
use fx_canary::CanaryMonitor;
use fx_config::manager::ConfigManager;
use fx_config::FawxConfig;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_improve::{CyclePaths, ImprovementConfig, OutputMode};
use fx_kernel::act::TokenUsage;
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::loop_engine::{LoopEngine, LoopResult};
use fx_kernel::signals::Signal;
use fx_kernel::types::PerceptionSnapshot;
use fx_kernel::StreamCallback;
use fx_llm::CompletionProvider;
use fx_llm::{Message, ModelInfo, ModelRouter};
use fx_memory::SignalStore;
use sha2::{Digest, Sha256};

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing_appender::non_blocking::WorkerGuard;

use crate::commands::slash::{
    apply_thinking_budget, client_only_command_message, config_reload_success_message,
    execute_command, init_default_config, is_command_input, parse_command, persist_default_model,
    reload_runtime_config, render_budget_text, render_debug_dump, render_loop_status,
    render_signals_summary, CommandContext, CommandHost, ImproveFlags, ParsedCommand,
    DEFAULT_SYNTHESIS_INSTRUCTION, MAX_SYNTHESIS_INSTRUCTION_LENGTH,
};
use crate::context::load_context_files;
use crate::helpers::{
    available_provider_names, format_memory_for_prompt, render_model_menu_text, render_status_text,
    resolve_model_alias, thinking_config_from_budget, trim_history, AnalysisCompletionProvider,
    RouterLoopLlmProvider,
};
use crate::proposal_review::{approve_pending, reject_pending, render_pending, ReviewContext};
use crate::startup::{
    build_headless_loop_engine_bundle, configured_data_dir as startup_configured_data_dir,
    configured_working_dir, fawx_data_dir as startup_fawx_data_dir, HeadlessLoopBuildOptions,
    SharedMemoryStore,
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

const HEADLESS_SIGNAL_SESSION_ID: &str = "headless";

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
    pub router: Arc<ModelRouter>,
    pub config: FawxConfig,
    pub memory: Option<SharedMemoryStore>,
    pub embedding_index_persistence: Option<crate::startup::EmbeddingIndexPersistence>,
    pub system_prompt_path: Option<PathBuf>,
    pub config_manager: Option<Arc<Mutex<ConfigManager>>>,
    pub system_prompt_text: Option<String>,
    pub subagent_manager: Arc<SubagentManager>,
    pub canary_monitor: Option<CanaryMonitor>,
}

/// Headless Fawx agent: drives `LoopEngine` via stdin/stdout.
pub struct HeadlessApp {
    loop_engine: LoopEngine,
    router: Arc<ModelRouter>,
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
}

#[derive(Clone)]
pub struct HeadlessSubagentFactoryDeps {
    pub router: Arc<ModelRouter>,
    pub config: FawxConfig,
    pub improvement_provider: Option<Arc<dyn CompletionProvider + Send + Sync>>,
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
struct AuthProviderStatus {
    provider: String,
    auth_methods: BTreeSet<String>,
    model_count: usize,
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
        let Some(persistence) = &self.embedding_index_persistence else {
            return;
        };
        if let Err(error) = persistence.save_if_dirty() {
            tracing::warn!(error = %error, "failed to save embedding index on shutdown");
        }
    }
}

impl HeadlessApp {
    /// Build from the standard startup bundle + router + config.
    pub fn new(deps: HeadlessAppDeps) -> Result<Self, anyhow::Error> {
        // Callers must seed the router's active model before construction.
        let active_model = resolve_active_model(&deps.router, &deps.config)?;

        let max_history = deps.config.general.max_history;
        let data_dir = configured_data_dir(&fawx_data_dir(), &deps.config);
        let custom_system_prompt = resolve_system_prompt(
            deps.system_prompt_text,
            deps.system_prompt_path.as_deref(),
            &data_dir,
        );

        Ok(Self {
            loop_engine: deps.loop_engine,
            router: deps.router,
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
        })
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

    /// Return the loaded configuration.
    #[allow(dead_code)]
    pub fn config(&self) -> &FawxConfig {
        &self.config
    }

    /// Return the shared config manager (if configured).
    #[cfg(feature = "http")]
    pub fn config_manager(&self) -> Option<&Arc<Mutex<ConfigManager>>> {
        self.config_manager.as_ref()
    }

    /// Apply the custom system prompt (if any). Must be called once
    /// before the first `process_message` invocation when not using
    /// the built-in `run()` or `run_single()` methods.
    pub fn initialize(&mut self) {
        self.apply_custom_system_prompt();
    }

    pub(crate) async fn analyze_signals_command(&mut self) -> anyhow::Result<String> {
        let signal_store = headless_signal_store(&self.config)?;
        let provider = AnalysisCompletionProvider::new(&self.router, self.active_model.clone());
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
        let provider = AnalysisCompletionProvider::new(&self.router, self.active_model.clone());
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

        let Some(router) = Arc::get_mut(&mut self.router) else {
            tracing::warn!("cannot set HTTP default model: router is shared");
            return;
        };

        if let Err(error) = router.set_active(selector) {
            tracing::warn!(
                model = selector,
                error = %error,
                "failed to set HTTP default model"
            );
            return;
        }

        if let Some(active_model) = self.router.active_model() {
            self.active_model = active_model.to_string();
            if self.config.model.default_model.is_none() {
                self.config.model.default_model = Some(active_model.to_string());
            }
        }
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
        self.update_memory_context(input);
        let snapshot = self.build_perception_snapshot(input, source);
        let llm = RouterLoopLlmProvider::new(&self.router, self.active_model.clone());
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
        self.update_memory_context(input);
        let snapshot = self.build_perception_snapshot(input, source);
        let llm = RouterLoopLlmProvider::new(&self.router, self.active_model.clone());
        let result = self
            .loop_engine
            .run_cycle_streaming(snapshot, &llm, Some(callback))
            .await
            .map_err(|e| anyhow::anyhow!("loop error: stage={} reason={}", e.stage, e.reason))?;
        self.evaluate_canary(&result);
        Ok(self.finalize_cycle(input, &result))
    }

    fn finalize_cycle(&mut self, input: &str, result: &LoopResult) -> CycleResult {
        let response = extract_response_text(result);
        let iterations = extract_iterations(result);
        let tokens_used = extract_token_usage(result);
        self.last_signals = result.signals().to_vec();
        persist_headless_signals(&self.config, &self.last_signals);
        self.record_turn(input, &response);
        CycleResult {
            response,
            model: self.active_model.clone(),
            iterations,
            tokens_used,
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
        let timestamp_ms = current_time_ms();
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
        let models = self.dynamic_models_or_fallback().await;
        Ok(render_model_menu_text(
            Some(self.active_model.as_str()),
            &models,
        ))
    }

    async fn dynamic_models_or_fallback(&self) -> Vec<ModelInfo> {
        let models = self.router.fetch_available_models().await;
        if models.is_empty() {
            return self.router.available_models();
        }
        models
    }
}

impl CommandHost for HeadlessApp {
    fn supports_embedded_slash_commands(&self) -> bool {
        true
    }

    fn list_models(&self) -> String {
        render_model_menu_text(
            Some(self.active_model.as_str()),
            &self.router.available_models(),
        )
    }

    fn set_active_model(&mut self, selector: &str) -> anyhow::Result<String> {
        let resolved = resolve_headless_model_selector(&self.router, selector)?;
        self.active_model = resolved.clone();
        persist_default_model(
            &mut self.config,
            self.config_manager.as_ref(),
            &fawx_data_dir(),
            &resolved,
        )?;
        Ok(resolved)
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
        self.loop_engine
            .set_thinking_config(thinking_config_from_budget(
                &self.config.general.thinking.unwrap_or_default(),
            ));
        sync_headless_model_from_config(self, self.config.model.default_model.clone())?;
        Ok(config_reload_success_message(&config_path))
    }

    fn show_status(&self) -> String {
        render_status_text(
            &self.active_model,
            &available_provider_names(&self.router),
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
        apply_thinking_budget(
            &mut self.config,
            &mut self.loop_engine,
            self.config_manager.as_ref(),
            &fawx_data_dir(),
            level,
        )
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
        handle_headless_auth_command(&self.router, subcommand, action, value, has_extra_args)
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
    let statuses = auth_provider_statuses(router.available_models());
    if statuses.is_empty() {
        return "No credentials configured.".to_string();
    }
    let mut lines = vec!["Configured credentials:".to_string()];
    lines.extend(statuses.iter().map(render_auth_status_line));
    lines.join("\n")
}

fn render_auth_status_line(status: &AuthProviderStatus) -> String {
    format!(
        "  ✓ {}: configured ({}) — {}",
        status.provider,
        format_auth_methods(&status.auth_methods),
        model_count_label(status.model_count)
    )
}

fn render_auth_provider_status(router: &ModelRouter, provider: &str) -> String {
    let provider = normalize_provider_name(provider);
    match auth_provider_statuses(router.available_models())
        .into_iter()
        .find(|status| status.provider == provider)
    {
        Some(status) => format!(
            "{} auth status:\n  Status: configured ({})\n  Models available: {}",
            status.provider,
            format_auth_methods(&status.auth_methods),
            status.model_count
        ),
        None => format!("{provider} auth status:\n  Status: not configured"),
    }
}

fn auth_provider_statuses(models: Vec<ModelInfo>) -> Vec<AuthProviderStatus> {
    let mut statuses = BTreeMap::new();
    for model in models {
        update_auth_provider_status(&mut statuses, model);
    }
    statuses.into_values().collect()
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
        });
    status.auth_methods.insert(model.auth_method);
    status.model_count += 1;
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
    let resolved = resolve_requested_model(&app.router, default_model.as_deref())?;
    apply_headless_active_model(app, &resolved);
    Ok(())
}

fn apply_headless_active_model(app: &mut HeadlessApp, model: &str) {
    if let Some(router) = Arc::get_mut(&mut app.router) {
        if let Err(error) = router.set_active(model) {
            tracing::warn!(error = %error, model, "failed to apply reloaded model to router");
        }
    }
    app.active_model = model.to_string();
}

fn headless_signal_store(config: &FawxConfig) -> anyhow::Result<SignalStore> {
    let data_dir = configured_data_dir(&fawx_data_dir(), config);
    SignalStore::new(&data_dir, HEADLESS_SIGNAL_SESSION_ID).map_err(anyhow::Error::new)
}

fn persist_headless_signals(config: &FawxConfig, signals: &[Signal]) {
    if let Ok(signal_store) = headless_signal_store(config) {
        if let Err(error) = signal_store.persist(signals) {
            eprintln!("warning: signal persist failed: {error}");
        }
        return;
    }
    eprintln!("warning: signal store unavailable for headless session");
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

impl HeadlessSubagentFactory {
    pub fn new(deps: HeadlessSubagentFactoryDeps) -> Self {
        Self {
            deps,
            disabled_manager: new_disabled_subagent_manager(),
        }
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
        let options = HeadlessLoopBuildOptions::subagent(config.cwd.clone(), cancel_token.clone());
        let bundle = build_headless_loop_engine_bundle(
            &self.deps.config,
            self.deps.improvement_provider.clone(),
            options,
        )
        .map_err(|error| SubagentError::Spawn(error.to_string()))?;
        let deps = HeadlessAppDeps {
            loop_engine: bundle.engine,
            router: Arc::clone(&self.deps.router),
            config: self.deps.config.clone(),
            memory: bundle.memory,
            embedding_index_persistence: bundle.embedding_index_persistence,
            system_prompt_path: None,
            config_manager: None,
            system_prompt_text: config.system_prompt.clone(),
            subagent_manager: Arc::clone(&self.disabled_manager),
            canary_monitor: None,
        };
        let app =
            HeadlessApp::new(deps).map_err(|error| SubagentError::Spawn(error.to_string()))?;
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

fn no_headless_models_available() -> anyhow::Error {
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
    use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
    use fx_kernel::budget::{BudgetConfig, BudgetTracker};
    use fx_kernel::cancellation::CancellationToken;
    use fx_kernel::context_manager::ContextCompactor;
    use fx_kernel::loop_engine::LoopEngine;
    use std::sync::{Arc, Mutex};

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

    fn test_app() -> HeadlessApp {
        HeadlessApp {
            loop_engine: test_engine(),
            router: Arc::new(ModelRouter::new()),
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

    fn test_router() -> Arc<ModelRouter> {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(UsageReportingProvider));
        router.set_active("mock-model").expect("set active");
        Arc::new(router)
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

    fn static_model_router(models: &[&'static str]) -> ModelRouter {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(StaticModelsProvider {
            name: "static-models",
            models: models.to_vec(),
            dynamic_models: None,
        }));
        router
    }

    fn headless_deps(mut router: ModelRouter, config: FawxConfig) -> HeadlessAppDeps {
        seed_headless_router_active_model(&mut router, &config);
        HeadlessAppDeps {
            loop_engine: test_engine(),
            router: Arc::new(router),
            config,
            memory: None,
            embedding_index_persistence: None,
            system_prompt_path: None,
            config_manager: None,
            system_prompt_text: None,
            subagent_manager: new_disabled_subagent_manager(),
            canary_monitor: None,
        }
    }

    fn headless_app_with_router(router: ModelRouter, active_model: &str) -> HeadlessApp {
        let mut app = test_app();
        app.router = Arc::new(router);
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

        assert_eq!(
            app.list_models(),
            render_model_menu_text(Some("mock-model"), &app.router.available_models())
        );
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
                vec!["old-model".to_string(), "new-model".to_string()]
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
            "[model]\ndefault_model = \"old-model\"\n",
        )
        .expect("write initial config");
        let manager = Arc::new(Mutex::new(
            ConfigManager::new(temp.path()).expect("config manager"),
        ));
        let mut config = FawxConfig::load(temp.path()).expect("load config");
        config.general.data_dir = Some(temp.path().to_path_buf());

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(ReloadProvider));
        router.set_active("old-model").expect("set old model");

        let mut app = HeadlessApp {
            loop_engine: test_engine(),
            router: Arc::new(router),
            config,
            memory: None,
            embedding_index_persistence: None,
            _subagent_manager: new_disabled_subagent_manager(),
            active_model: "old-model".to_string(),
            conversation_history: Vec::new(),
            last_signals: Vec::new(),
            max_history: 20,
            custom_system_prompt: None,
            canary_monitor: None,
            config_manager: Some(manager),
        };

        std::fs::write(
            temp.path().join("config.toml"),
            "[model]\ndefault_model = \"new-model\"\n",
        )
        .expect("write updated config");

        let response = app.reload_config().expect("reload config");

        assert_eq!(app.active_model, "new-model");
        assert_eq!(app.router.active_model(), Some("new-model"));
        assert_eq!(
            response,
            crate::commands::slash::config_reload_success_message(&temp.path().join("config.toml"))
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
        app.router = Arc::new(router);

        let status = app.show_status();
        assert!(status.contains("providers: usage-reporting"));
        assert!(!status.contains("providers: usage-reporting, usage-reporting"));
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
        assert!(auth_status.response.contains("Status: configured"));

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

    #[tokio::test]
    async fn process_message_reports_token_counts() {
        let mut app = HeadlessApp {
            loop_engine: test_engine(),
            router: test_router(),
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
        assert_eq!(app.router.active_model(), Some("config-model"));
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

        let router = Arc::new(router);
        let cloned_router = Arc::clone(&router);

        assert_eq!(router.active_model(), Some("config-model"));
        assert_eq!(cloned_router.active_model(), Some("config-model"));
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
        assert_eq!(app.router.active_model(), Some("a-model"));
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
        assert_eq!(app.router.active_model(), Some("a-model"));
        assert!(logs.is_empty(), "unexpected warnings: {logs}");
    }

    #[test]
    fn sync_headless_model_from_config_updates_active_model() {
        let mut router = static_model_router(&["old-model", "new-model"]);
        router.set_active("old-model").expect("set active");
        let mut app = headless_app_with_router(router, "old-model");

        sync_headless_model_from_config(&mut app, Some("new-model".to_string())).expect("sync");

        assert_eq!(app.active_model, "new-model");
        assert_eq!(app.router.active_model(), Some("new-model"));
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
        assert_eq!(app.router.active_model(), Some("old-model"));
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
        assert_eq!(app.router.active_model(), Some("a-model"));
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
        assert_eq!(app.router.active_model(), Some("config-model"));
    }

    #[test]
    fn new_returns_clear_error_when_no_models_are_available() {
        let result = HeadlessApp::new(headless_deps(ModelRouter::new(), FawxConfig::default()));
        assert!(result.is_err(), "should fail without any models");

        let error = result.err().expect("missing error");
        assert_eq!(
            error.to_string(),
            "no models available in router; configure a provider and authenticate it before starting headless mode"
        );
    }

    #[test]
    fn headless_subagent_factory_new_builds_disabled_manager() {
        let deps = HeadlessSubagentFactoryDeps {
            router: Arc::new(ModelRouter::new()),
            config: FawxConfig::default(),
            improvement_provider: None,
        };
        let factory = HeadlessSubagentFactory::new(deps);
        let debug = format!("{factory:?}");
        assert!(debug.contains("HeadlessSubagentFactory"));
    }
}
