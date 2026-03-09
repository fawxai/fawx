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
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::loop_engine::{LoopEngine, LoopResult};
use fx_kernel::signals::Signal;
use fx_kernel::types::PerceptionSnapshot;
use fx_llm::CompletionProvider;
use fx_llm::{Message, ModelRouter};
use fx_memory::SignalStore;

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::commands::slash::{
    apply_thinking_budget, config_reload_success_message, init_default_config,
    persist_default_model, reload_runtime_config, render_budget_text, render_signals_summary,
    CommandHost, ImproveFlags,
};
use crate::proposal_review::{approve_pending, reject_pending, render_pending, ReviewContext};
use crate::tui::{
    available_provider_names, build_headless_loop_engine_bundle, configured_data_dir,
    configured_working_dir, fawx_data_dir, format_memory_for_prompt, render_model_menu_text,
    render_status_text, resolve_model_alias, thinking_config_from_budget, trim_history,
    AnalysisCompletionProvider, HeadlessLoopBuildOptions, RouterLoopLlmProvider, SharedMemoryStore,
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
    /// Total input + output tokens reported for the cycle.
    pub tokens_used: u64,
}

// ── HeadlessApp ─────────────────────────────────────────────────────────────

/// Dependencies for constructing a [`HeadlessApp`]. Avoids > 5 bare params.
pub struct HeadlessAppDeps {
    pub loop_engine: LoopEngine,
    pub router: Arc<ModelRouter>,
    pub config: FawxConfig,
    pub memory: Option<SharedMemoryStore>,
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

impl HeadlessApp {
    /// Build from the standard startup bundle + router + config.
    pub fn new(mut deps: HeadlessAppDeps) -> Result<Self, anyhow::Error> {
        let active_model = resolve_active_model(&deps.router, &deps.config);
        seed_router_default_model(&mut deps.router, &active_model);

        let max_history = deps.config.general.max_history;
        let custom_system_prompt =
            resolve_system_prompt(deps.system_prompt_text, deps.system_prompt_path.as_deref());

        Ok(Self {
            loop_engine: deps.loop_engine,
            router: deps.router,
            config: deps.config,
            memory: deps.memory,
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

    pub async fn process_message_for_source(
        &mut self,
        input: &str,
        source: &InputSource,
    ) -> Result<CycleResult, anyhow::Error> {
        self.run_cycle_result(input, source).await
    }

    /// Return the active model identifier.
    #[cfg(feature = "http")]
    pub fn active_model(&self) -> &str {
        &self.active_model
    }

    /// Return the shared config manager (if configured).
    #[cfg(feature = "http")]
    pub fn config_manager(&self) -> Option<&Arc<Mutex<ConfigManager>>> {
        self.config_manager.as_ref()
    }

    /// Apply the custom system prompt (if any). Must be called once
    /// before the first `process_message` invocation when not using
    /// the built-in `run()` or `run_single()` methods.
    #[cfg(feature = "http")]
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

    fn finalize_cycle(&mut self, input: &str, result: &LoopResult) -> CycleResult {
        let response = extract_response_text(result);
        let iterations = extract_iterations(result);
        let tokens_used = extract_tokens_used(result);
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
            eprintln!("system prompt: ~/.fawx/system_prompt.md loaded");
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
}

impl CommandHost for HeadlessApp {
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

    fn proposals(&self) -> anyhow::Result<String> {
        render_pending(headless_review_context(&self.config)).map_err(anyhow::Error::new)
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
    let Some(selector) = default_model else {
        return Ok(());
    };
    let resolved = resolve_headless_model_selector(&app.router, &selector)?;
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

fn render_improve_output(result: &fx_improve::ExecutionResult, dry_run: bool) -> String {
    let mut lines = vec![if dry_run {
        "⚡ Dry run complete.".to_string()
    } else {
        "⚡ Improvement cycle complete.".to_string()
    }];
    if result.proposals_written.is_empty()
        && result.branches_created.is_empty()
        && result.skipped.is_empty()
    {
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
    lines.extend(
        result
            .skipped
            .iter()
            .map(|(name, reason)| format!("  Skipped: {name} — {reason}")),
    );
    lines.join("\n")
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
            memory: None,
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
            tokens_used: result.tokens_used,
        })
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
) -> Option<String> {
    inline_prompt
        .filter(|prompt| !prompt.trim().is_empty())
        .or_else(|| load_system_prompt(explicit_path))
}

fn resolve_active_model(router: &ModelRouter, config: &FawxConfig) -> String {
    router
        .active_model()
        .map(ToString::to_string)
        .or_else(|| config.model.default_model.clone())
        .unwrap_or_default()
}

fn seed_router_default_model(router: &mut Arc<ModelRouter>, active_model: &str) {
    if active_model.is_empty() || router.active_model().is_some() {
        return;
    }
    let Some(router) = Arc::get_mut(router) else {
        return;
    };
    if let Err(error) = router.set_active(active_model) {
        tracing::warn!(
            error = %error,
            model = %active_model,
            "config default_model not available in router"
        );
    }
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

fn extract_tokens_used(result: &LoopResult) -> u64 {
    match result {
        LoopResult::Complete { tokens_used, .. } => sum_token_usage(tokens_used),
        _ => 0,
    }
}

fn sum_token_usage(tokens: &fx_kernel::act::TokenUsage) -> u64 {
    tokens.input_tokens + tokens.output_tokens
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
    use std::sync::Arc;

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

        assert!(CommandHost::proposals(&app).is_err());
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

    #[tokio::test]
    async fn process_message_reports_token_counts() {
        let mut app = HeadlessApp {
            loop_engine: test_engine(),
            router: test_router(),
            config: FawxConfig::default(),
            memory: None,
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
        assert!(result.tokens_used >= mock_completion_usage_total());
    }

    #[tokio::test]
    async fn process_message_updates_canary_monitor() {
        let mut app = HeadlessApp {
            loop_engine: test_engine(),
            router: test_router(),
            config: FawxConfig::default(),
            memory: None,
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
        assert_eq!(extract_tokens_used(&result), 5);
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
        let prompt = resolve_system_prompt(Some("inline".to_string()), Some(&path));
        assert_eq!(prompt.as_deref(), Some("inline"));
    }

    #[test]
    fn new_falls_back_to_config_default_model() {
        let mut config = FawxConfig::default();
        config.model.default_model = Some("test-model-fallback".to_string());

        let deps = HeadlessAppDeps {
            loop_engine: test_engine(),
            router: Arc::new(ModelRouter::new()),
            config,
            memory: None,
            system_prompt_path: None,
            config_manager: None,
            system_prompt_text: None,
            subagent_manager: new_disabled_subagent_manager(),
            canary_monitor: None,
        };

        let app = HeadlessApp::new(deps).expect("should build");
        // Router has no providers so set_active will fail, but the
        // active_model string should still be populated from config.
        assert_eq!(app.active_model, "test-model-fallback");
    }

    #[test]
    fn new_uses_router_model_over_config_default() {
        use fx_llm::{
            CompletionProvider, CompletionRequest, CompletionResponse, CompletionStream,
            ProviderCapabilities, ProviderError,
        };

        #[derive(Debug)]
        struct FakeProvider;

        #[async_trait]
        impl CompletionProvider for FakeProvider {
            async fn complete(
                &self,
                _req: CompletionRequest,
            ) -> Result<CompletionResponse, ProviderError> {
                unimplemented!()
            }
            async fn complete_stream(
                &self,
                _req: CompletionRequest,
            ) -> Result<CompletionStream, ProviderError> {
                unimplemented!()
            }
            fn name(&self) -> &str {
                "fake"
            }
            fn supported_models(&self) -> Vec<String> {
                vec!["router-model".to_string()]
            }
            fn capabilities(&self) -> ProviderCapabilities {
                ProviderCapabilities {
                    supports_temperature: false,
                    requires_streaming: false,
                }
            }
        }

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(FakeProvider));
        router
            .set_active("router-model")
            .expect("set active should work");

        let mut config = FawxConfig::default();
        config.model.default_model = Some("config-model".to_string());

        let deps = HeadlessAppDeps {
            loop_engine: test_engine(),
            router: Arc::new(router),
            config,
            memory: None,
            system_prompt_path: None,
            config_manager: None,
            system_prompt_text: None,
            subagent_manager: new_disabled_subagent_manager(),
            canary_monitor: None,
        };

        let app = HeadlessApp::new(deps).expect("should build");
        // Router's explicit model wins over config default.
        assert_eq!(app.active_model, "router-model");
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
