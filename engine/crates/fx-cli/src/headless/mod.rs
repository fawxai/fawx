//! Headless mode for Fawx — stdin/stdout REPL without the TUI.
//!
//! Provides `HeadlessApp` which drives the full agentic loop via
//! `LoopEngine::run_cycle()` while reading input from stdin and writing
//! responses to stdout. All diagnostic/error output goes to stderr so
//! downstream consumers can safely pipe stdout.

mod auth;
mod command;
mod engine;
mod keys;
mod model;
mod output;
mod session;
pub mod startup;

use async_trait::async_trait;
use futures::Stream;
use fx_analysis::{AnalysisEngine, AnalysisError, AnalysisFinding, Confidence};
#[cfg(feature = "http")]
use fx_api::engine::{
    AppEngine, ConfigManagerHandle, CycleResult as ApiCycleResult, ResultKind as ApiResultKind,
};
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
use fx_kernel::loop_engine::{LlmProvider as LoopLlmProvider, LoopEngine, LoopResult};
use fx_kernel::signals::Signal;
use fx_kernel::types::PerceptionSnapshot;
use fx_kernel::{ErrorCategory, PermissionPromptState, StreamCallback, StreamEvent};
use fx_llm::CompletionProvider;
use fx_llm::{
    CompletionRequest, CompletionResponse, CompletionStream, DocumentAttachment, ImageAttachment,
    Message, ModelInfo, ModelRouter, ProviderError, StreamCallback as ProviderStreamCallback,
    StreamChunk, ToolCall, ToolUseDelta, Usage,
};
use fx_memory::SignalStore;
use fx_session::{
    prune_unresolved_tool_history, MessageRole as SessionRecordRole, SessionContentBlock,
    SessionKey, SessionMessage,
};
use uuid::Uuid;

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tracing_appender::non_blocking::WorkerGuard;

#[cfg(test)]
use self::auth::StoredAuthProviderEntry;
use self::auth::{
    auth_provider_dto, auth_provider_statuses, handle_headless_auth_command,
    stored_auth_provider_entries,
};
#[cfg(test)]
use self::command::process_command_input;
use self::keys::handle_headless_keys_command;
use self::model::{
    active_model_thinking_levels, apply_headless_active_model, handle_headless_synthesis_command,
    preferred_supported_budget, resolve_headless_model_selector, sync_headless_model_from_config,
    thinking_adjustment_reason, update_context_limit_for_active_model,
};
#[cfg(test)]
use self::output::json_output_from_cycle;
#[cfg(test)]
use self::session::is_quit_command;
pub use self::session::{process_input_with_commands, process_input_with_commands_streaming};
use crate::auth_store::AuthStore;
#[cfg(test)]
use crate::commands::slash::CommandHost;
use crate::commands::slash::{
    apply_thinking_budget, is_command_input, persist_default_model, ImproveFlags,
    DEFAULT_SYNTHESIS_INSTRUCTION, MAX_SYNTHESIS_INSTRUCTION_LENGTH,
};
use crate::context::load_context_files;
#[cfg(test)]
use crate::helpers::render_model_menu_text;
use crate::helpers::{
    format_memory_for_prompt, read_router, resolve_model_alias, trim_history, write_router,
    AnalysisCompletionProvider, RouterLoopLlmProvider, SharedModelRouter,
};
use crate::proposal_review::ReviewContext;
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_errors: Vec<String>,
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

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    Complete,
    Partial,
    Error,
    Empty,
}

#[cfg(feature = "http")]
impl From<ResultKind> for ApiResultKind {
    fn from(value: ResultKind) -> Self {
        match value {
            ResultKind::Complete => Self::Complete,
            ResultKind::Partial => Self::Partial,
            ResultKind::Error => Self::Error,
            ResultKind::Empty => Self::Empty,
        }
    }
}

const MAX_ERROR_HISTORY: usize = 50;
const NO_AI_PROVIDERS_STARTUP_WARNING: &str =
    "no AI providers configured; HTTP API available but chat disabled until a provider is added";
const BUDGET_EXHAUSTED_FALLBACK_RESPONSE: &str =
    "I ran out of processing budget before finishing. Could you try again or simplify the request?";

const TIMEOUT_ERROR_RESPONSE: &str =
    "The request timed out. The operation may still be running. You can try again.";
const PERMISSION_ERROR_RESPONSE: &str =
    "That action isn't allowed under the current permission settings.";
const RATE_LIMIT_ERROR_RESPONSE: &str =
    "The API rate limit was hit. Please wait a moment and try again.";
const AUTH_ERROR_RESPONSE: &str =
    "There was an authentication issue with the API provider. Check your credentials in settings.";
const NETWORK_ERROR_RESPONSE: &str =
    "A network error occurred. Check your internet connection and try again.";
const GENERIC_ERROR_RESPONSE: &str =
    "Something went wrong while processing your request. Please try again.";

pub struct CycleResult {
    /// The assistant's response text.
    pub response: String,
    /// The model identifier used for the cycle.
    pub model: String,
    /// Number of loop iterations consumed.
    pub iterations: u32,
    /// Token usage reported for the cycle.
    pub tokens_used: TokenUsage,
    /// High-level classification used by downstream clients.
    pub result_kind: ResultKind,
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
    pub stream_callback_slot: Arc<std::sync::Mutex<Option<fx_kernel::streaming::StreamCallback>>>,
    pub permission_prompt_state: Option<Arc<PermissionPromptState>>,
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
    /// Structured messages recorded for the most recent completed turn.
    last_session_messages: Vec<SessionMessage>,
    /// Shared callback slot for executor-triggered SSE stream events.
    stream_callback_slot: Arc<std::sync::Mutex<Option<fx_kernel::streaming::StreamCallback>>>,
    permission_prompt_state: Option<Arc<PermissionPromptState>>,
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
    pub credential_store: Option<crate::startup::SharedCredentialStore>,
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

#[derive(Debug, Clone)]
struct RecordingLoopLlmProvider<T> {
    inner: T,
    collector: SessionTurnCollector,
}

impl<T> RecordingLoopLlmProvider<T> {
    fn new(inner: T, collector: SessionTurnCollector) -> Self {
        Self { inner, collector }
    }
}

#[async_trait]
impl<T> LoopLlmProvider for RecordingLoopLlmProvider<T>
where
    T: LoopLlmProvider,
{
    async fn generate(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, fx_core::error::LlmError> {
        self.inner.generate(prompt, max_tokens).await
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, fx_core::error::LlmError> {
        self.inner
            .generate_streaming(prompt, max_tokens, callback)
            .await
    }

    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let response = self.inner.complete(request).await?;
        self.collector.record_response(&response);
        Ok(response)
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderError> {
        let stream = self.inner.complete_stream(request).await?;
        Ok(Box::pin(RecordingCompletionStream::new(
            stream,
            self.collector.clone(),
        )))
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        callback: ProviderStreamCallback,
    ) -> Result<CompletionResponse, ProviderError> {
        let response = self.inner.stream(request, callback).await?;
        self.collector.record_response(&response);
        Ok(response)
    }
}

#[derive(Debug, Default)]
struct StreamedToolCallState {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    arguments_done: bool,
}

#[derive(Debug, Default)]
struct StreamedCompletionState {
    text: String,
    usage: Option<Usage>,
    stop_reason: Option<String>,
    tool_calls_by_index: HashMap<usize, StreamedToolCallState>,
    id_to_index: HashMap<String, usize>,
}

impl StreamedCompletionState {
    fn apply_chunk(&mut self, chunk: StreamChunk) {
        if let Some(delta) = chunk.delta_content {
            self.text.push_str(&delta);
        }
        self.usage = merge_stream_usage(self.usage, chunk.usage);
        self.stop_reason = chunk.stop_reason.or(self.stop_reason.take());
        self.apply_tool_deltas(chunk.tool_use_deltas);
    }

    fn apply_tool_deltas(&mut self, deltas: Vec<ToolUseDelta>) {
        for (chunk_index, delta) in deltas.into_iter().enumerate() {
            let index = streamed_tool_index(
                chunk_index,
                &delta,
                &self.tool_calls_by_index,
                &self.id_to_index,
            );
            let entry = self.tool_calls_by_index.entry(index).or_default();
            merge_streamed_tool_delta(entry, delta, &mut self.id_to_index, index);
        }
    }

    fn into_response(self) -> CompletionResponse {
        CompletionResponse {
            content: vec![fx_llm::ContentBlock::Text { text: self.text }],
            tool_calls: finalize_streamed_tool_calls(self.tool_calls_by_index),
            usage: self.usage,
            stop_reason: self.stop_reason,
        }
    }
}

struct RecordingCompletionStream {
    inner: CompletionStream,
    collector: SessionTurnCollector,
    state: StreamedCompletionState,
    recorded: bool,
}

impl RecordingCompletionStream {
    fn new(inner: CompletionStream, collector: SessionTurnCollector) -> Self {
        Self {
            inner,
            collector,
            state: StreamedCompletionState::default(),
            recorded: false,
        }
    }

    fn record_completed_response(&mut self) {
        if self.recorded {
            return;
        }

        let response = std::mem::take(&mut self.state).into_response();
        self.collector.record_response(&response);
        self.recorded = true;
    }
}

impl Stream for RecordingCompletionStream {
    type Item = Result<StreamChunk, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.state.apply_chunk(chunk.clone());
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error))),
            Poll::Ready(None) => {
                this.record_completed_response();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Debug, Default, Clone)]
struct SessionTurnCollector {
    responses: Arc<Mutex<Vec<CompletionResponse>>>,
    tool_result_rounds: Arc<Mutex<Vec<Vec<SessionContentBlock>>>>,
    pending_tool_results: Arc<Mutex<Vec<SessionContentBlock>>>,
}

#[derive(Debug, Default)]
struct SessionTurnSnapshot {
    responses: Vec<CompletionResponse>,
    tool_result_rounds: Vec<Vec<SessionContentBlock>>,
}

#[derive(Debug)]
struct RecordedAssistantTurn {
    message: SessionMessage,
    has_tool_use: bool,
}

struct FinalizeTurnContext<'a> {
    images: &'a [ImageAttachment],
    documents: &'a [DocumentAttachment],
    collector: Option<&'a SessionTurnCollector>,
    user_timestamp: u64,
    assistant_timestamp: u64,
}

impl SessionTurnCollector {
    fn record_response(&self, response: &CompletionResponse) {
        match self.responses.lock() {
            Ok(mut guard) => {
                guard.push(response.clone());
            }
            Err(error) => {
                tracing::warn!(error = %error, "failed to record session turn response");
            }
        }
    }

    fn callback(&self, forward: Option<StreamCallback>) -> StreamCallback {
        let collector = self.clone();
        Arc::new(move |event| {
            collector.observe(&event);
            if let Some(callback) = forward.as_ref() {
                callback(event);
            }
        })
    }

    fn session_messages_for_turn(
        &self,
        user_text: &str,
        images: &[ImageAttachment],
        documents: &[DocumentAttachment],
        fallback_response: &str,
        user_timestamp: u64,
        assistant_timestamp: u64,
    ) -> Vec<SessionMessage> {
        self.flush_pending_tool_results();
        let snapshot = self.snapshot();

        let mut messages = vec![user_session_message(
            user_text,
            images,
            documents,
            user_timestamp,
        )];
        let mut assistant_messages = build_turn_tool_history_messages(
            SessionTurnSnapshot {
                responses: snapshot.responses.clone(),
                tool_result_rounds: snapshot.tool_result_rounds,
            },
            assistant_timestamp,
        );
        if let Some(terminal_message) =
            terminal_assistant_message(&snapshot.responses, fallback_response, assistant_timestamp)
        {
            assistant_messages.push(terminal_message);
        }

        messages.extend(assistant_messages);

        prune_unresolved_tool_history(&messages)
    }

    fn observe(&self, event: &StreamEvent) {
        match event {
            StreamEvent::ToolCallStart { .. } | StreamEvent::ToolCallComplete { .. } => {
                self.flush_pending_tool_results();
            }
            StreamEvent::ToolResult {
                id,
                tool_name: _,
                output,
                is_error,
            } => match self.pending_tool_results.lock() {
                Ok(mut guard) => {
                    guard.push(SessionContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: serde_json::Value::String(output.clone()),
                        is_error: Some(*is_error),
                    });
                }
                Err(error) => {
                    tracing::warn!(error = %error, "failed to record pending tool result");
                }
            },
            StreamEvent::Done { .. } | StreamEvent::Error { .. } => {
                self.flush_pending_tool_results();
            }
            StreamEvent::ToolError { .. }
            | StreamEvent::TextDelta { .. }
            | StreamEvent::Progress { .. }
            | StreamEvent::Notification { .. }
            | StreamEvent::PermissionPrompt(_)
            | StreamEvent::PhaseChange { .. }
            | StreamEvent::ContextCompacted { .. } => {}
        }
    }

    fn flush_pending_tool_results(&self) {
        let pending_blocks = match self.pending_tool_results.lock() {
            Ok(mut pending) => {
                if pending.is_empty() {
                    return;
                }
                std::mem::take(&mut *pending)
            }
            Err(error) => {
                tracing::warn!(error = %error, "failed to flush pending tool results");
                return;
            }
        };

        match self.tool_result_rounds.lock() {
            Ok(mut rounds) => {
                rounds.push(pending_blocks);
            }
            Err(error) => {
                tracing::warn!(error = %error, "failed to store flushed tool results");
                match self.pending_tool_results.lock() {
                    Ok(mut pending) => pending.extend(pending_blocks),
                    Err(recover_error) => {
                        tracing::warn!(
                            error = %recover_error,
                            "failed to restore pending tool results after flush failure"
                        );
                    }
                }
            }
        }
    }

    fn snapshot(&self) -> SessionTurnSnapshot {
        SessionTurnSnapshot {
            responses: self.snapshot_responses(),
            tool_result_rounds: self.snapshot_tool_result_rounds(),
        }
    }

    fn snapshot_responses(&self) -> Vec<CompletionResponse> {
        match self.responses.lock() {
            Ok(guard) => guard.clone(),
            Err(error) => {
                tracing::warn!(error = %error, "failed to snapshot recorded session responses");
                Vec::new()
            }
        }
    }

    fn snapshot_tool_result_rounds(&self) -> Vec<Vec<SessionContentBlock>> {
        match self.tool_result_rounds.lock() {
            Ok(guard) => guard.clone(),
            Err(error) => {
                tracing::warn!(error = %error, "failed to snapshot recorded tool results");
                Vec::new()
            }
        }
    }
}

fn user_session_message(
    user_text: &str,
    images: &[ImageAttachment],
    documents: &[DocumentAttachment],
    timestamp: u64,
) -> SessionMessage {
    SessionMessage::structured(
        SessionRecordRole::User,
        user_message_blocks(user_text, images, documents),
        timestamp,
        None,
    )
}

fn fallback_assistant_message(fallback_response: &str, timestamp: u64) -> SessionMessage {
    SessionMessage::structured(
        SessionRecordRole::Assistant,
        vec![SessionContentBlock::Text {
            text: fallback_response.to_string(),
        }],
        timestamp,
        None,
    )
}

fn fallback_assistant_message_from_template(
    fallback_response: &str,
    timestamp: u64,
    template: Option<&SessionMessage>,
) -> SessionMessage {
    if let Some(template) = template {
        return SessionMessage {
            role: SessionRecordRole::Assistant,
            content: vec![SessionContentBlock::Text {
                text: fallback_response.to_string(),
            }],
            timestamp,
            token_count: template.token_count,
            input_token_count: template.input_token_count,
            output_token_count: template.output_token_count,
        };
    }

    fallback_assistant_message(fallback_response, timestamp)
}

fn last_visible_assistant_text(message: &SessionMessage) -> Option<String> {
    if message.role != SessionRecordRole::Assistant {
        return None;
    }

    let text = message.render_text().trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn normalize_session_message_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn build_turn_tool_history_messages(
    snapshot: SessionTurnSnapshot,
    timestamp: u64,
) -> Vec<SessionMessage> {
    let tool_turns = tool_turn_messages(snapshot.responses, timestamp);
    let Some(tool_use_message) = aggregate_tool_use_message(&tool_turns, timestamp) else {
        return Vec::new();
    };
    let tool_results = aggregate_tool_result_blocks(&tool_turns, snapshot.tool_result_rounds);
    let mut messages = vec![tool_use_message];
    if !tool_results.is_empty() {
        messages.push(tool_result_message(tool_results, timestamp));
    }
    messages
}

fn tool_turn_messages(responses: Vec<CompletionResponse>, timestamp: u64) -> Vec<SessionMessage> {
    responses
        .into_iter()
        .filter_map(|response| assistant_turn_from_response(response, timestamp))
        .filter(|turn| turn.has_tool_use)
        .map(|turn| turn.message)
        .collect()
}

fn aggregate_tool_use_message(
    tool_turns: &[SessionMessage],
    timestamp: u64,
) -> Option<SessionMessage> {
    let content = tool_turns
        .iter()
        .flat_map(|message| message.content.iter().cloned())
        .collect::<Vec<_>>();
    if content.is_empty() {
        return None;
    }

    let mut message = SessionMessage::structured_with_usage(
        SessionRecordRole::Assistant,
        content,
        timestamp,
        aggregate_message_usage(tool_turns),
    );
    if message.token_count.is_none() {
        message.token_count = aggregate_total_token_count(tool_turns);
    }
    Some(message)
}

fn aggregate_tool_result_blocks(
    tool_turns: &[SessionMessage],
    tool_result_rounds: Vec<Vec<SessionContentBlock>>,
) -> Vec<SessionContentBlock> {
    assign_tool_results_to_turns(tool_turns, tool_result_rounds)
        .into_iter()
        .flatten()
        .collect()
}

fn tool_result_message(content: Vec<SessionContentBlock>, timestamp: u64) -> SessionMessage {
    SessionMessage::structured(SessionRecordRole::Tool, content, timestamp, None)
}

fn aggregate_message_usage(messages: &[SessionMessage]) -> Option<Usage> {
    let mut input_tokens: u32 = 0;
    let mut output_tokens: u32 = 0;
    let mut saw_usage = false;

    for message in messages {
        let (Some(input), Some(output)) = (message.input_token_count, message.output_token_count)
        else {
            continue;
        };
        input_tokens = input_tokens.saturating_add(input);
        output_tokens = output_tokens.saturating_add(output);
        saw_usage = true;
    }

    saw_usage.then_some(Usage {
        input_tokens,
        output_tokens,
    })
}

fn aggregate_total_token_count(messages: &[SessionMessage]) -> Option<u32> {
    let mut total: u32 = 0;
    let mut saw_tokens = false;

    for message in messages {
        let Some(message_total) = message.total_token_count() else {
            continue;
        };
        total = total.saturating_add(message_total);
        saw_tokens = true;
    }

    saw_tokens.then_some(total)
}

fn terminal_assistant_message(
    responses: &[CompletionResponse],
    fallback_response: &str,
    timestamp: u64,
) -> Option<SessionMessage> {
    let recorded_terminal = responses.iter().rev().find_map(|response| {
        let recorded_turn = assistant_turn_from_response(response.clone(), timestamp)?;
        (!recorded_turn.has_tool_use).then_some(recorded_turn.message)
    });

    if has_meaningful_response(Some(fallback_response)) {
        let matching_terminal = recorded_terminal.as_ref().filter(|message| {
            last_visible_assistant_text(message).is_some_and(|existing| {
                normalize_session_message_text(&existing)
                    == normalize_session_message_text(fallback_response)
            })
        });
        return Some(fallback_assistant_message_from_template(
            fallback_response,
            timestamp,
            matching_terminal,
        ));
    }

    recorded_terminal
}

fn assistant_turn_from_response(
    response: CompletionResponse,
    timestamp: u64,
) -> Option<RecordedAssistantTurn> {
    let blocks = session_blocks_from_response(response.content, response.tool_calls);
    if blocks.is_empty() {
        return None;
    }

    let has_tool_use = blocks
        .iter()
        .any(|block| matches!(block, SessionContentBlock::ToolUse { .. }));
    Some(RecordedAssistantTurn {
        message: SessionMessage::structured_with_usage(
            SessionRecordRole::Assistant,
            blocks,
            timestamp,
            response.usage,
        ),
        has_tool_use,
    })
}

fn assign_tool_results_to_turns(
    tool_turns: &[SessionMessage],
    tool_result_rounds: Vec<Vec<SessionContentBlock>>,
) -> Vec<Vec<SessionContentBlock>> {
    let mut turn_indices = HashMap::new();
    for (index, message) in tool_turns.iter().enumerate() {
        for block in &message.content {
            if let SessionContentBlock::ToolUse { id, .. } = block {
                let trimmed = id.trim();
                if !trimmed.is_empty() {
                    turn_indices.entry(trimmed.to_string()).or_insert(index);
                }
            }
        }
    }

    let mut assigned = vec![Vec::new(); tool_turns.len()];
    for block in tool_result_rounds.into_iter().flatten() {
        let SessionContentBlock::ToolResult { tool_use_id, .. } = &block else {
            continue;
        };
        let trimmed = tool_use_id.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(index) = turn_indices.get(trimmed).copied() else {
            tracing::warn!(
                tool_use_id = trimmed,
                "dropping orphaned session tool result without matching tool_use"
            );
            continue;
        };
        assigned[index].push(block);
    }

    assigned
}

fn session_blocks_from_response(
    content: Vec<fx_llm::ContentBlock>,
    tool_calls: Vec<ToolCall>,
) -> Vec<SessionContentBlock> {
    let mut blocks = content
        .into_iter()
        .filter_map(session_block_from_content)
        .collect::<Vec<_>>();
    let mut has_tool_use_blocks = blocks
        .iter()
        .any(|block| matches!(block, SessionContentBlock::ToolUse { .. }));
    if has_tool_use_blocks {
        blocks.retain(|block| !matches!(block, SessionContentBlock::Text { .. }));
    }
    if !has_tool_use_blocks {
        blocks.extend(
            tool_calls
                .into_iter()
                .map(|call| SessionContentBlock::ToolUse {
                    id: call.id,
                    provider_id: None,
                    name: call.name,
                    input: call.arguments,
                }),
        );
        has_tool_use_blocks = blocks
            .iter()
            .any(|block| matches!(block, SessionContentBlock::ToolUse { .. }));
    }
    if has_tool_use_blocks {
        blocks.retain(|block| !matches!(block, SessionContentBlock::Text { .. }));
    }
    blocks
}

fn session_block_from_content(block: fx_llm::ContentBlock) -> Option<SessionContentBlock> {
    match block {
        fx_llm::ContentBlock::Text { text } if text.is_empty() => None,
        other => Some(SessionContentBlock::from(other)),
    }
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn merge_stream_usage(left: Option<Usage>, right: Option<Usage>) -> Option<Usage> {
    if left.is_none() && right.is_none() {
        return None;
    }

    let left_in = left.as_ref().map(|usage| usage.input_tokens).unwrap_or(0);
    let left_out = left.as_ref().map(|usage| usage.output_tokens).unwrap_or(0);
    let right_in = right.as_ref().map(|usage| usage.input_tokens).unwrap_or(0);
    let right_out = right.as_ref().map(|usage| usage.output_tokens).unwrap_or(0);

    Some(Usage {
        input_tokens: left_in.saturating_add(right_in),
        output_tokens: left_out.saturating_add(right_out),
    })
}

fn streamed_tool_index(
    chunk_index: usize,
    delta: &ToolUseDelta,
    tool_calls_by_index: &HashMap<usize, StreamedToolCallState>,
    id_to_index: &HashMap<String, usize>,
) -> usize {
    let Some(id) = delta.id.as_deref() else {
        return chunk_index;
    };

    if let Some(index) = id_to_index.get(id).copied() {
        return index;
    }

    if streamed_chunk_index_usable_for_id(chunk_index, id, tool_calls_by_index) {
        return chunk_index;
    }

    next_streamed_tool_index(tool_calls_by_index)
}

fn streamed_chunk_index_usable_for_id(
    chunk_index: usize,
    id: &str,
    tool_calls_by_index: &HashMap<usize, StreamedToolCallState>,
) -> bool {
    match tool_calls_by_index.get(&chunk_index) {
        None => true,
        Some(state) => match state.id.as_deref() {
            None => true,
            Some(existing_id) => existing_id == id,
        },
    }
}

fn next_streamed_tool_index(tool_calls_by_index: &HashMap<usize, StreamedToolCallState>) -> usize {
    tool_calls_by_index
        .keys()
        .copied()
        .max()
        .map(|index| index.saturating_add(1))
        .unwrap_or(0)
}

fn merge_streamed_tool_delta(
    entry: &mut StreamedToolCallState,
    delta: ToolUseDelta,
    id_to_index: &mut HashMap<String, usize>,
    index: usize,
) {
    if entry.id.is_none() {
        entry.id = delta.id;
    }
    if entry.name.is_none() {
        entry.name = delta.name;
    }
    if let Some(id) = entry.id.clone() {
        id_to_index.insert(id, index);
    }
    if let Some(arguments_delta) = delta.arguments_delta {
        merge_streamed_arguments(&mut entry.arguments, &arguments_delta, delta.arguments_done);
    }
    entry.arguments_done |= delta.arguments_done;
}

fn merge_streamed_arguments(arguments: &mut String, arguments_delta: &str, arguments_done: bool) {
    if arguments_delta.is_empty() {
        return;
    }

    let done_payload_is_complete = arguments_done
        && !arguments.is_empty()
        && serde_json::from_str::<serde_json::Value>(arguments_delta).is_ok();
    if done_payload_is_complete {
        arguments.clear();
    }

    arguments.push_str(arguments_delta);
}

fn finalize_streamed_tool_calls(by_index: HashMap<usize, StreamedToolCallState>) -> Vec<ToolCall> {
    let mut indexed_calls = by_index.into_iter().collect::<Vec<_>>();
    indexed_calls.sort_by_key(|(index, _)| *index);
    indexed_calls
        .into_iter()
        .filter_map(|(_, state)| streamed_tool_call_from_state(state))
        .collect()
}

fn streamed_tool_call_from_state(state: StreamedToolCallState) -> Option<ToolCall> {
    if !state.arguments_done {
        return None;
    }

    let id = state.id?.trim().to_string();
    let name = state.name?.trim().to_string();
    if id.is_empty() || name.is_empty() {
        return None;
    }

    let raw_arguments = if state.arguments.trim().is_empty() {
        "{}"
    } else {
        state.arguments.trim()
    };

    serde_json::from_str(raw_arguments)
        .ok()
        .map(|arguments| ToolCall {
            id,
            name,
            arguments,
        })
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
            last_session_messages: Vec::new(),
            stream_callback_slot: deps.stream_callback_slot,
            permission_prompt_state: deps.permission_prompt_state,
            ripcord_journal: deps.ripcord_journal,
            bus_receiver,
        };
        app.seed_runtime_info();
        if !app.active_model.is_empty() {
            update_context_limit_for_active_model(&mut app);
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

    pub fn permission_prompt_state(&self) -> Option<&Arc<PermissionPromptState>> {
        self.permission_prompt_state.as_ref()
    }

    pub fn ripcord_journal(&self) -> &Arc<fx_ripcord::RipcordJournal> {
        &self.ripcord_journal
    }

    pub fn session_bus(&self) -> Option<&SessionBus> {
        self.session_bus.as_ref()
    }

    pub fn take_last_session_messages(&mut self) -> Vec<SessionMessage> {
        std::mem::take(&mut self.last_session_messages)
    }

    pub fn cron_store(&self) -> Option<&fx_cron::SharedCronStore> {
        self.cron_store.as_ref()
    }

    #[cfg(feature = "http")]
    pub fn experiment_registry(&self) -> Option<&fx_api::SharedExperimentRegistry> {
        self.experiment_registry.as_ref()
    }

    pub fn thinking_available_levels(&self) -> Vec<String> {
        active_model_thinking_levels(&self.router, &self.active_model)
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

    pub fn skill_summaries(&self) -> Vec<SkillSummaryDto> {
        match self.runtime_info.read() {
            Ok(info) => runtime_skill_summary_dtos(&info),
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
            update_context_limit_for_active_model(self);
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
            update_context_limit_for_active_model(self);
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
}

fn user_message_blocks(
    user_text: &str,
    images: &[ImageAttachment],
    documents: &[DocumentAttachment],
) -> Vec<SessionContentBlock> {
    let mut blocks = images
        .iter()
        .map(|image| SessionContentBlock::Image {
            media_type: image.media_type.clone(),
            data: Some(image.data.clone()),
        })
        .collect::<Vec<_>>();
    blocks.extend(
        documents
            .iter()
            .map(|document| SessionContentBlock::Document {
                media_type: document.media_type.clone(),
                data: document.data.clone(),
                filename: document.filename.clone(),
            }),
    );
    if !user_text.is_empty() {
        blocks.push(SessionContentBlock::Text {
            text: user_text.to_string(),
        });
    }
    blocks
}

fn text_turn_messages(
    user_text: &str,
    assistant_text: &str,
    user_timestamp: u64,
    assistant_timestamp: u64,
) -> Vec<SessionMessage> {
    vec![
        SessionMessage::structured(
            SessionRecordRole::User,
            user_message_blocks(user_text, &[], &[]),
            user_timestamp,
            None,
        ),
        SessionMessage::structured(
            SessionRecordRole::Assistant,
            vec![SessionContentBlock::Text {
                text: assistant_text.to_string(),
            }],
            assistant_timestamp,
            None,
        ),
    ]
}

#[cfg(feature = "http")]
#[async_trait]
impl AppEngine for HeadlessApp {
    async fn process_message(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<ApiCycleResult, anyhow::Error> {
        let (result, updated_history) = HeadlessApp::process_message_with_context(
            self,
            input,
            images,
            documents,
            self.conversation_history.clone(),
            &source,
            callback,
        )
        .await?;
        self.conversation_history = updated_history;

        let result_kind = result.result_kind.into();

        Ok(ApiCycleResult {
            response: result.response,
            model: result.model,
            iterations: result.iterations,
            result_kind,
        })
    }

    async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        context: Vec<Message>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<(ApiCycleResult, Vec<Message>), anyhow::Error> {
        let (result, updated_history) = HeadlessApp::process_message_with_context(
            self, input, images, documents, context, &source, callback,
        )
        .await?;

        Ok((
            {
                let result_kind = result.result_kind.into();
                ApiCycleResult {
                    response: result.response,
                    model: result.model,
                    iterations: result.iterations,
                    result_kind,
                }
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

    fn permission_prompt_state(&self) -> Option<Arc<PermissionPromptState>> {
        HeadlessApp::permission_prompt_state(self).cloned()
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

    fn replace_session_memory(
        &mut self,
        memory: fx_session::SessionMemory,
    ) -> fx_session::SessionMemory {
        self.loop_engine.replace_session_memory(memory)
    }

    fn session_memory(&self) -> fx_session::SessionMemory {
        self.loop_engine.session_memory_snapshot()
    }

    fn loaded_session_key(&self) -> Option<SessionKey> {
        self.session_key.clone()
    }

    fn take_last_session_messages(&mut self) -> Vec<SessionMessage> {
        HeadlessApp::take_last_session_messages(self)
    }
}

fn runtime_skill_summary_dtos(info: &RuntimeInfo) -> Vec<SkillSummaryDto> {
    info.skills
        .iter()
        .map(|skill| SkillSummaryDto {
            name: skill.name.clone(),
            description: skill.description.clone().unwrap_or_default(),
            tools: skill.tool_names.clone(),
            capabilities: skill.capabilities.clone(),
            version: skill.version.clone(),
            source: skill.source.clone(),
            revision_hash: skill.revision_hash.clone(),
            activated_at_ms: skill.activated_at_ms,
            signature_status: skill.signature_status.clone(),
            stale_source: skill.stale_source.clone(),
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

    fn subagent_build_options(
        &self,
        config: &SpawnConfig,
        cancel_token: CancellationToken,
    ) -> HeadlessLoopBuildOptions {
        let mut options = HeadlessLoopBuildOptions::subagent(config.cwd.clone(), cancel_token);
        options.credential_store = self.deps.credential_store.clone();
        options.token_broker = self.deps.token_broker.clone();
        options
    }

    fn build_app(
        &self,
        config: &SpawnConfig,
        cancel_token: CancellationToken,
    ) -> Result<HeadlessApp, SubagentError> {
        let options = self.subagent_build_options(config, cancel_token);
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
            stream_callback_slot: bundle.stream_callback_slot,
            permission_prompt_state: Some(bundle.permission_prompt_state),
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

// ── Free functions ──────────────────────────────────────────────────────────

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
        } => {
            if has_meaningful_response(partial_response.as_deref()) {
                partial_response.clone().unwrap_or_default()
            } else {
                BUDGET_EXHAUSTED_FALLBACK_RESPONSE.to_string()
            }
        }
        LoopResult::Incomplete {
            partial_response,
            reason,
            ..
        } => {
            if has_meaningful_response(partial_response.as_deref()) {
                partial_response.clone().unwrap_or_default()
            } else {
                reason.clone()
            }
        }
        LoopResult::UserStopped {
            partial_response, ..
        } => partial_response.clone().unwrap_or_default(),
        LoopResult::Error { message, .. } => classify_error_response(message),
    }
}

fn extract_result_kind(result: &LoopResult) -> ResultKind {
    match result {
        LoopResult::Complete { .. } => ResultKind::Complete,
        LoopResult::BudgetExhausted {
            partial_response, ..
        }
        | LoopResult::Incomplete {
            partial_response, ..
        }
        | LoopResult::UserStopped {
            partial_response, ..
        } => {
            if has_meaningful_response(partial_response.as_deref()) {
                ResultKind::Partial
            } else {
                ResultKind::Empty
            }
        }
        LoopResult::Error { .. } => ResultKind::Error,
    }
}

fn classify_error_response(message: &str) -> String {
    let lower = message.to_ascii_lowercase();

    if lower.contains("timeout") || lower.contains("timed out") {
        return TIMEOUT_ERROR_RESPONSE.to_string();
    }
    if lower.contains("blocked")
        || lower.contains("denied")
        || lower.contains("not permitted")
        || lower.contains("forbidden")
    {
        return PERMISSION_ERROR_RESPONSE.to_string();
    }
    if lower.contains("rate limit") || lower.contains("too many requests") || lower.contains("429")
    {
        return RATE_LIMIT_ERROR_RESPONSE.to_string();
    }
    if lower.contains("authentication") || lower.contains("unauthorized") || lower.contains("401") {
        return AUTH_ERROR_RESPONSE.to_string();
    }
    if lower.contains("network") || lower.contains("connection") || lower.contains("dns") {
        return NETWORK_ERROR_RESPONSE.to_string();
    }

    GENERIC_ERROR_RESPONSE.to_string()
}

fn has_meaningful_response(response: Option<&str>) -> bool {
    response.is_some_and(|response| !response.trim().is_empty())
}

fn extract_iterations(result: &LoopResult) -> u32 {
    match result {
        LoopResult::Complete { iterations, .. }
        | LoopResult::BudgetExhausted { iterations, .. }
        | LoopResult::Incomplete { iterations, .. }
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
    use std::collections::HashMap;
    use std::path::Path;
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
            last_session_messages: Vec::new(),
            stream_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            permission_prompt_state: None,
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            bus_receiver: None,
        }
    }

    fn write_test_signing_key(data_dir: &Path) {
        let (private_key, _) = fx_skills::signing::generate_keypair().expect("generate keypair");
        let keys_dir = data_dir.join("keys");
        std::fs::create_dir_all(&keys_dir).expect("create keys dir");
        std::fs::write(keys_dir.join("signing_key.pem"), private_key).expect("write signing key");
    }

    fn install_test_skill(data_dir: &Path, name: &str, wasm_bytes: &[u8]) {
        let skill_dir = data_dir.join("skills").join(name);
        std::fs::create_dir_all(&skill_dir).expect("create skill dir");
        std::fs::write(
            skill_dir.join("manifest.toml"),
            format!(
                "name = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"test\"\nauthor = \"tester\"\napi_version = \"host_api_v1\"\ncapabilities = []\n"
            ),
        )
        .expect("write manifest");
        std::fs::write(skill_dir.join(format!("{name}.wasm")), wasm_bytes).expect("write wasm");
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

    #[derive(Debug)]
    struct ReplaySafeCaptureProvider {
        captured: Arc<std::sync::Mutex<Vec<fx_llm::CompletionRequest>>>,
    }

    impl ReplaySafeCaptureProvider {
        fn capture_request(
            &self,
            request: &fx_llm::CompletionRequest,
        ) -> Result<(), fx_llm::ProviderError> {
            if request_replays_tool_use(request, "call_orphan") {
                return Err(fx_llm::ProviderError::Provider(
                    "No tool output found for function call fc_orphan".to_string(),
                ));
            }

            self.captured
                .lock()
                .expect("capture lock")
                .push(request.clone());
            Ok(())
        }
    }

    #[async_trait]
    impl fx_llm::CompletionProvider for ReplaySafeCaptureProvider {
        async fn complete(
            &self,
            request: fx_llm::CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            self.capture_request(&request)?;
            Ok(mock_completion_response())
        }

        async fn complete_stream(
            &self,
            request: fx_llm::CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            self.capture_request(&request)?;
            let chunk = fx_llm::StreamChunk {
                delta_content: Some(mock_completion_text()),
                stop_reason: Some("end_turn".to_string()),
                ..Default::default()
            };
            Ok(Box::pin(futures::stream::iter(vec![Ok(chunk)])))
        }

        fn name(&self) -> &str {
            "replay-safe-capture"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["replay-safe-model".to_string()]
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            fx_llm::ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }
    }

    fn request_replays_tool_use(request: &fx_llm::CompletionRequest, id: &str) -> bool {
        request.messages.iter().any(|message| {
            message
                .content
                .iter()
                .any(|block| matches!(block, fx_llm::ContentBlock::ToolUse { id: block_id, .. } if block_id == id))
        })
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

    fn streamed_tool_delta(
        id: Option<&str>,
        name: Option<&str>,
        arguments_delta: Option<&str>,
        arguments_done: bool,
    ) -> fx_llm::ToolUseDelta {
        fx_llm::ToolUseDelta {
            id: id.map(ToString::to_string),
            provider_id: None,
            name: name.map(ToString::to_string),
            arguments_delta: arguments_delta.map(ToString::to_string),
            arguments_done,
        }
    }

    fn seed_resolved_and_orphaned_tool_history(
        collector: &SessionTurnCollector,
        resolved_name: &str,
    ) {
        record_tool_use_response(
            collector,
            "call_resolved",
            "fc_resolved",
            resolved_name,
            serde_json::json!({"path": "README.md"}),
        );
        collector.observe(&StreamEvent::ToolResult {
            id: "call_resolved".to_string(),
            tool_name: resolved_name.to_string(),
            output: "patched".to_string(),
            is_error: false,
        });
        record_tool_use_response(
            collector,
            "call_orphan",
            "fc_orphan",
            "git_status",
            serde_json::json!({}),
        );
    }

    fn record_tool_use_response(
        collector: &SessionTurnCollector,
        id: &str,
        provider_id: &str,
        name: &str,
        input: serde_json::Value,
    ) {
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::ToolUse {
                id: id.to_string(),
                provider_id: Some(provider_id.to_string()),
                name: name.to_string(),
                input,
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        });
    }

    #[test]
    fn session_turn_collector_builds_structured_turn_messages() {
        let collector = SessionTurnCollector::default();
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![
                fx_llm::ContentBlock::Text {
                    text: "Let me check.".to_string(),
                },
                fx_llm::ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: None,
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "README.md"}),
                },
            ],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 10,
                output_tokens: 5,
            }),
            stop_reason: Some("tool_use".to_string()),
        });
        collector.observe(&StreamEvent::ToolResult {
            id: "call_1".to_string(),
            tool_name: "read_file".to_string(),
            output: "file contents".to_string(),
            is_error: false,
        });
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::Text {
                text: "Done.".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 7,
                output_tokens: 3,
            }),
            stop_reason: Some("end_turn".to_string()),
        });

        let messages =
            collector.session_messages_for_turn("open the readme", &[], &[], "Done.", 10, 20);

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, SessionRecordRole::User);
        assert_eq!(messages[1].role, SessionRecordRole::Assistant);
        assert_eq!(messages[2].role, SessionRecordRole::Tool);
        assert_eq!(messages[3].role, SessionRecordRole::Assistant);
        assert_eq!(messages[0].timestamp, 10);
        assert_eq!(messages[1].timestamp, 20);
        assert_eq!(messages[2].timestamp, 20);
        assert_eq!(messages[3].timestamp, 20);
        assert_eq!(messages[1].token_count, Some(15));
        assert_eq!(messages[1].input_token_count, Some(10));
        assert_eq!(messages[1].output_token_count, Some(5));
        assert_eq!(messages[3].token_count, Some(10));
        assert_eq!(messages[3].input_token_count, Some(7));
        assert_eq!(messages[3].output_token_count, Some(3));
        assert!(
            !messages[1]
                .content
                .iter()
                .any(|block| matches!(block, SessionContentBlock::Text { .. })),
            "mixed tool turns should not persist assistant narration text"
        );
        assert!(messages[1].content.iter().any(
            |block| matches!(block, SessionContentBlock::ToolUse { id, .. } if id == "call_1")
        ));
        assert!(
            messages[2]
                .content
                .iter()
                .any(|block| matches!(block, SessionContentBlock::ToolResult { tool_use_id, is_error, .. } if tool_use_id == "call_1" && *is_error == Some(false)))
        );
    }

    #[test]
    fn session_turn_collector_preserves_distinct_user_and_assistant_timestamps() {
        let collector = SessionTurnCollector::default();
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::Text {
                text: "Done.".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: Some("end_turn".to_string()),
        });

        let messages =
            collector.session_messages_for_turn("open the readme", &[], &[], "Done.", 100, 700);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, SessionRecordRole::User);
        assert_eq!(messages[1].role, SessionRecordRole::Assistant);
        assert_eq!(messages[0].timestamp, 100);
        assert_eq!(messages[1].timestamp, 700);
    }

    #[test]
    fn session_turn_collector_preserves_error_metadata_for_tool_results() {
        let collector = SessionTurnCollector::default();
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::ToolUse {
                id: "call_err".to_string(),
                provider_id: Some("fc_err".to_string()),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "missing.txt"}),
            }],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 3,
                output_tokens: 2,
            }),
            stop_reason: Some("tool_use".to_string()),
        });
        collector.observe(&StreamEvent::ToolResult {
            id: "call_err".to_string(),
            tool_name: "read_file".to_string(),
            output: "missing".to_string(),
            is_error: true,
        });

        let messages =
            collector.session_messages_for_turn("open missing", &[], &[], "fallback", 10, 20);

        assert!(
            messages[2]
                .content
                .iter()
                .any(|block| matches!(block, SessionContentBlock::ToolResult { tool_use_id, content, is_error } if tool_use_id == "call_err" && content == &serde_json::Value::String("missing".to_string()) && *is_error == Some(true)))
        );
    }

    #[test]
    fn session_turn_collector_uses_tool_calls_when_response_content_is_empty() {
        let collector = SessionTurnCollector::default();
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::Text {
                text: String::new(),
            }],
            tool_calls: vec![fx_llm::ToolCall {
                id: "call_2".to_string(),
                name: "search".to_string(),
                arguments: serde_json::json!({"q": "rust"}),
            }],
            usage: Some(fx_llm::Usage {
                input_tokens: 8,
                output_tokens: 4,
            }),
            stop_reason: Some("tool_use".to_string()),
        });
        collector.observe(&StreamEvent::ToolResult {
            id: "call_2".to_string(),
            tool_name: "search".to_string(),
            output: "results".to_string(),
            is_error: false,
        });

        let messages = collector.session_messages_for_turn(
            "search rust",
            &[],
            &[],
            "Rust search results are ready.",
            10,
            20,
        );

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1].role, SessionRecordRole::Assistant);
        assert_eq!(messages[1].token_count, Some(12));
        assert!(messages[1].content.iter().any(|block| matches!(
            block,
            SessionContentBlock::ToolUse { id, name, .. } if id == "call_2" && name == "search"
        )));
        assert_eq!(messages[2].role, SessionRecordRole::Tool);
        assert!(messages[2].content.iter().any(|block| matches!(
            block,
            SessionContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call_2"
        )));
        assert_eq!(messages[3].role, SessionRecordRole::Assistant);
        assert_eq!(messages[3].render_text(), "Rust search results are ready.");
    }

    #[test]
    fn session_turn_collector_omits_text_when_tool_calls_are_reconstructed() {
        let collector = SessionTurnCollector::default();
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::Text {
                text: "Let me search for that.".to_string(),
            }],
            tool_calls: vec![fx_llm::ToolCall {
                id: "call_legacy".to_string(),
                name: "search".to_string(),
                arguments: serde_json::json!({"q": "x api"}),
            }],
            usage: Some(fx_llm::Usage {
                input_tokens: 6,
                output_tokens: 4,
            }),
            stop_reason: Some("tool_use".to_string()),
        });

        let messages = collector.session_messages_for_turn(
            "search the X API",
            &[],
            &[],
            "Search results are ready.",
            10,
            20,
        );

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, SessionRecordRole::Assistant);
        assert_eq!(messages[1].render_text(), "Search results are ready.");
        assert!(!messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| matches!(
                block,
                SessionContentBlock::ToolUse { id, .. } if id == "call_legacy"
            )));
    }

    #[test]
    fn session_turn_collector_prefers_content_tool_use_blocks_over_tool_call_fallback() {
        let collector = SessionTurnCollector::default();
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::ToolUse {
                id: "call_3".to_string(),
                provider_id: Some("fc_3".to_string()),
                name: "weather".to_string(),
                input: serde_json::json!({"location": "Denver, CO"}),
            }],
            tool_calls: vec![fx_llm::ToolCall {
                id: "call_3".to_string(),
                name: "weather".to_string(),
                arguments: serde_json::json!({"location": "Denver, CO"}),
            }],
            usage: Some(fx_llm::Usage {
                input_tokens: 8,
                output_tokens: 4,
            }),
            stop_reason: Some("tool_use".to_string()),
        });

        let messages = collector.session_messages_for_turn(
            "weather in denver",
            &[],
            &[],
            "Weather lookup requires executing the recorded tool call.",
            10,
            20,
        );

        assert_eq!(messages.len(), 2);
        assert_eq!(
            messages[1].render_text(),
            "Weather lookup requires executing the recorded tool call."
        );
        assert!(!messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| matches!(
                block,
                SessionContentBlock::ToolUse { id, .. } if id == "call_3"
            )));
    }

    #[test]
    fn session_turn_collector_appends_terminal_summary_after_tool_only_history() {
        let collector = SessionTurnCollector::default();
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::ToolUse {
                id: "call_4".to_string(),
                provider_id: None,
                name: "web_search".to_string(),
                input: serde_json::json!({"query": "X API POST /2/tweets"}),
            }],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 8,
                output_tokens: 4,
            }),
            stop_reason: Some("tool_use".to_string()),
        });
        collector.observe(&StreamEvent::ToolResult {
            id: "call_4".to_string(),
            tool_name: "web_search".to_string(),
            output: "search results".to_string(),
            is_error: false,
        });

        let messages = collector.session_messages_for_turn(
            "Research the X API",
            &[],
            &[],
            "Task decomposition results:\n1. Research X API => budget exhausted\n   Partial response: enough research to proceed with implementation.",
            10,
            20,
        );

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, SessionRecordRole::User);
        assert_eq!(messages[1].role, SessionRecordRole::Assistant);
        assert_eq!(messages[2].role, SessionRecordRole::Tool);
        assert_eq!(messages[3].role, SessionRecordRole::Assistant);
        assert_eq!(
            messages[3].render_text(),
            "Task decomposition results:\n1. Research X API => budget exhausted\n   Partial response: enough research to proceed with implementation."
        );
    }

    #[test]
    fn session_turn_collector_aggregates_multi_round_tool_history_into_single_group() {
        let collector = SessionTurnCollector::default();
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::ToolUse {
                id: "call_a".to_string(),
                provider_id: Some("fc_a".to_string()),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "README.md"}),
            }],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 4,
                output_tokens: 2,
            }),
            stop_reason: Some("tool_use".to_string()),
        });
        collector.observe(&StreamEvent::ToolResult {
            id: "call_a".to_string(),
            tool_name: "read_file".to_string(),
            output: "first result".to_string(),
            is_error: false,
        });
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::ToolUse {
                id: "call_b".to_string(),
                provider_id: Some("fc_b".to_string()),
                name: "list_dir".to_string(),
                input: serde_json::json!({"path": "."}),
            }],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 5,
                output_tokens: 3,
            }),
            stop_reason: Some("tool_use".to_string()),
        });
        collector.observe(&StreamEvent::ToolResult {
            id: "call_b".to_string(),
            tool_name: "list_dir".to_string(),
            output: "second result".to_string(),
            is_error: false,
        });
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::Text {
                text: "Done.".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 6,
                output_tokens: 4,
            }),
            stop_reason: Some("end_turn".to_string()),
        });

        let messages =
            collector.session_messages_for_turn("inspect repo", &[], &[], "Done.", 10, 20);

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, SessionRecordRole::User);
        assert_eq!(messages[1].role, SessionRecordRole::Assistant);
        assert_eq!(messages[2].role, SessionRecordRole::Tool);
        assert_eq!(messages[3].role, SessionRecordRole::Assistant);
        assert_eq!(messages[1].token_count, Some(14));
        assert_eq!(messages[1].input_token_count, Some(9));
        assert_eq!(messages[1].output_token_count, Some(5));
        assert!(matches!(
            messages[1].content.as_slice(),
            [
                SessionContentBlock::ToolUse { id: first_id, provider_id: first_provider, .. },
                SessionContentBlock::ToolUse { id: second_id, provider_id: second_provider, .. },
            ] if first_id == "call_a"
                && first_provider.as_deref() == Some("fc_a")
                && second_id == "call_b"
                && second_provider.as_deref() == Some("fc_b")
        ));
        assert!(matches!(
            messages[2].content.as_slice(),
            [
                SessionContentBlock::ToolResult { tool_use_id: first_id, .. },
                SessionContentBlock::ToolResult { tool_use_id: second_id, .. },
            ] if first_id == "call_a" && second_id == "call_b"
        ));
        assert_eq!(messages[3].render_text(), "Done.");
    }

    #[test]
    fn session_turn_collector_omits_intermediate_text_only_synthesis_between_tool_rounds() {
        let collector = SessionTurnCollector::default();
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::ToolUse {
                id: "call_a".to_string(),
                provider_id: None,
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "~/.fawx/x.md"}),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        });
        collector.observe(&StreamEvent::ToolResult {
            id: "call_a".to_string(),
            tool_name: "read_file".to_string(),
            output: "spec contents".to_string(),
            is_error: false,
        });
        collector.observe(&StreamEvent::ToolCallStart {
            id: "call_b".to_string(),
            name: "run_command".to_string(),
        });
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::Text {
                text: "Current state: the spec file already exists and is complete.".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: Some("end_turn".to_string()),
        });
        collector.record_response(&fx_llm::CompletionResponse {
            content: vec![fx_llm::ContentBlock::ToolUse {
                id: "call_b".to_string(),
                provider_id: None,
                name: "run_command".to_string(),
                input: serde_json::json!({"command": "fawx skill create x-post"}),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        });
        collector.observe(&StreamEvent::ToolResult {
            id: "call_b".to_string(),
            tool_name: "run_command".to_string(),
            output: "working directory is set there".to_string(),
            is_error: false,
        });

        let messages = collector.session_messages_for_turn(
            "Research and implement the X skill",
            &[],
            &[],
            "I can't complete the file creation from here because the required paths are outside my working directory.",
            10,
            20,
        );

        assert_eq!(messages.len(), 4);
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.role == SessionRecordRole::Assistant)
                .count(),
            2,
            "one aggregated tool-use assistant message plus one terminal assistant message should persist",
        );
        assert!(
            !messages.iter().any(|message| {
                message.role == SessionRecordRole::Assistant
                    && message
                        .render_text()
                        .contains("Current state: the spec file already exists")
            }),
            "intermediate text-only synthesis should remain internal to the turn",
        );
        assert!(
            matches!(messages.last(), Some(message) if message.render_text().contains("outside my working directory"))
        );
    }

    #[test]
    fn session_turn_collector_drops_unresolved_tool_use_from_partial_turn_history() {
        let collector = SessionTurnCollector::default();
        seed_resolved_and_orphaned_tool_history(&collector, "read_file");

        let messages = collector.session_messages_for_turn(
            "Read README then make a small improvement to it.",
            &[],
            &[],
            "Updated README.md but could not finish follow-up verification.",
            10,
            20,
        );

        assert_eq!(messages.len(), 4);
        assert!(matches!(
            messages[1].content.as_slice(),
            [SessionContentBlock::ToolUse { id, provider_id, .. }]
                if id == "call_resolved"
                    && provider_id.as_deref() == Some("fc_resolved")
        ));
        assert!(matches!(
            messages[2].content.as_slice(),
            [SessionContentBlock::ToolResult { tool_use_id, .. }]
                if tool_use_id == "call_resolved"
        ));
        assert!(!messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| matches!(
                block,
                SessionContentBlock::ToolUse { id, .. } if id == "call_orphan"
            )));
        assert!(matches!(
            messages.last(),
            Some(message)
                if message
                    .render_text()
                    .contains("could not finish follow-up verification")
        ));
        assert!(fx_session::validate_tool_message_order(&messages).is_ok());
    }

    #[tokio::test]
    async fn follow_up_turn_does_not_replay_unresolved_tool_use() {
        let captured = Arc::new(std::sync::Mutex::new(
            Vec::<fx_llm::CompletionRequest>::new(),
        ));
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(ReplaySafeCaptureProvider {
            captured: Arc::clone(&captured),
        }));
        router
            .set_active("replay-safe-model")
            .expect("set active replay-safe model");

        let mut app = test_app();
        app.router = shared_router(router);
        app.active_model = "replay-safe-model".to_string();

        let collector = SessionTurnCollector::default();
        seed_resolved_and_orphaned_tool_history(&collector, "edit_file");

        let session_messages = collector.session_messages_for_turn(
            "Read README then make a small improvement to it.",
            &[],
            &[],
            "Updated README.md and stopped after the applied change.",
            10,
            20,
        );
        app.record_session_turn_messages(session_messages);

        app.process_message("What changed?")
            .await
            .expect("follow-up turn should succeed");

        let captured_request = captured
            .lock()
            .expect("capture lock")
            .last()
            .cloned()
            .expect("captured request");

        assert!(request_replays_tool_use(&captured_request, "call_resolved"));
        assert!(!request_replays_tool_use(&captured_request, "call_orphan"));
    }

    #[test]
    fn build_turn_tool_history_messages_reassigns_tool_results_by_tool_use_id() {
        let snapshot = SessionTurnSnapshot {
            responses: vec![
                fx_llm::CompletionResponse {
                    content: vec![fx_llm::ContentBlock::ToolUse {
                        id: "call_a".to_string(),
                        provider_id: Some("fc_a".to_string()),
                        name: "read_file".to_string(),
                        input: serde_json::json!({"path": "README.md"}),
                    }],
                    tool_calls: Vec::new(),
                    usage: None,
                    stop_reason: Some("tool_use".to_string()),
                },
                fx_llm::CompletionResponse {
                    content: vec![fx_llm::ContentBlock::ToolUse {
                        id: "call_b".to_string(),
                        provider_id: Some("fc_b".to_string()),
                        name: "edit_file".to_string(),
                        input: serde_json::json!({"path": "README.md"}),
                    }],
                    tool_calls: Vec::new(),
                    usage: None,
                    stop_reason: Some("tool_use".to_string()),
                },
            ],
            tool_result_rounds: vec![
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_b".to_string(),
                    content: serde_json::Value::String("edit ok".to_string()),
                    is_error: Some(false),
                }],
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_a".to_string(),
                    content: serde_json::Value::String("read ok".to_string()),
                    is_error: Some(false),
                }],
            ],
        };

        let messages = build_turn_tool_history_messages(snapshot, 20);

        assert_eq!(messages.len(), 2);
        assert!(matches!(
            messages[0].content.as_slice(),
            [
                SessionContentBlock::ToolUse { id: first_id, provider_id: first_provider, .. },
                SessionContentBlock::ToolUse { id: second_id, provider_id: second_provider, .. },
            ] if first_id == "call_a"
                && first_provider.as_deref() == Some("fc_a")
                && second_id == "call_b"
                && second_provider.as_deref() == Some("fc_b")
        ));
        assert!(matches!(
            messages[1].content.as_slice(),
            [
                SessionContentBlock::ToolResult { tool_use_id: first_id, .. },
                SessionContentBlock::ToolResult { tool_use_id: second_id, .. },
            ] if first_id == "call_a" && second_id == "call_b"
        ));
        assert!(fx_session::validate_tool_message_order(&messages).is_ok());
    }

    #[test]
    fn build_turn_tool_history_messages_drops_orphaned_tool_results() {
        let snapshot = SessionTurnSnapshot {
            responses: vec![fx_llm::CompletionResponse {
                content: vec![fx_llm::ContentBlock::ToolUse {
                    id: "call_real".to_string(),
                    provider_id: None,
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "README.md"}),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: Some("tool_use".to_string()),
            }],
            tool_result_rounds: vec![
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_real".to_string(),
                    content: serde_json::Value::String("read ok".to_string()),
                    is_error: Some(false),
                }],
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_orphan".to_string(),
                    content: serde_json::Value::String("orphan".to_string()),
                    is_error: Some(false),
                }],
            ],
        };

        let messages = build_turn_tool_history_messages(snapshot, 20);

        assert_eq!(messages.len(), 2);
        assert!(matches!(
            messages[0].content.as_slice(),
            [SessionContentBlock::ToolUse { id, .. }] if id == "call_real"
        ));
        assert!(matches!(
            messages[1].content.as_slice(),
            [SessionContentBlock::ToolResult { tool_use_id, .. }] if tool_use_id == "call_real"
        ));
        assert!(fx_session::validate_tool_message_order(&messages).is_ok());
    }

    #[test]
    fn streamed_tool_index_reuses_known_ids_and_splits_reused_chunk_slots() {
        let mut tool_calls_by_index = HashMap::new();
        let mut id_to_index = HashMap::new();

        tool_calls_by_index.insert(
            0,
            StreamedToolCallState {
                id: Some("call_1".to_string()),
                name: Some("search".to_string()),
                arguments: String::new(),
                arguments_done: false,
            },
        );
        id_to_index.insert("call_1".to_string(), 0);

        let reused_index = streamed_tool_index(
            0,
            &streamed_tool_delta(Some("call_1"), None, Some("{}"), true),
            &tool_calls_by_index,
            &id_to_index,
        );
        let next_index = streamed_tool_index(
            0,
            &streamed_tool_delta(Some("call_2"), Some("read_file"), Some("{}"), true),
            &tool_calls_by_index,
            &id_to_index,
        );

        assert_eq!(reused_index, 0);
        assert_eq!(next_index, 1);
    }

    #[test]
    fn merge_streamed_arguments_replaces_partial_buffer_with_complete_done_payload() {
        let mut arguments = r#"{"path":"READ"#.to_string();

        merge_streamed_arguments(&mut arguments, r#"{"path":"README.md"}"#, true);

        assert_eq!(arguments, r#"{"path":"README.md"}"#);
    }

    #[test]
    fn streamed_completion_state_assembles_tool_calls_across_stream_chunks() {
        let mut state = StreamedCompletionState::default();

        state.apply_chunk(fx_llm::StreamChunk {
            delta_content: Some("Working".to_string()),
            tool_use_deltas: vec![streamed_tool_delta(
                Some("call_1"),
                Some("search"),
                Some(r#"{"q":"rus"#),
                false,
            )],
            usage: Some(fx_llm::Usage {
                input_tokens: 3,
                output_tokens: 1,
            }),
            stop_reason: None,
        });

        state.apply_chunk(fx_llm::StreamChunk {
            delta_content: Some(" on it".to_string()),
            tool_use_deltas: vec![streamed_tool_delta(
                Some("call_2"),
                Some("list_files"),
                None,
                true,
            )],
            usage: Some(fx_llm::Usage {
                input_tokens: 2,
                output_tokens: 4,
            }),
            stop_reason: Some("tool_use".to_string()),
        });

        state.apply_chunk(fx_llm::StreamChunk {
            delta_content: None,
            tool_use_deltas: vec![streamed_tool_delta(
                Some("call_1"),
                None,
                Some(r#"{"q":"rust"}"#),
                true,
            )],
            usage: None,
            stop_reason: None,
        });

        let response = state.into_response();

        assert_eq!(
            response.content,
            vec![fx_llm::ContentBlock::Text {
                text: "Working on it".to_string(),
            }]
        );
        assert_eq!(
            response.usage,
            Some(fx_llm::Usage {
                input_tokens: 5,
                output_tokens: 5,
            })
        );
        assert_eq!(response.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(response.tool_calls.len(), 2);
        assert_eq!(response.tool_calls[0].id, "call_1");
        assert_eq!(response.tool_calls[0].name, "search");
        assert_eq!(
            response.tool_calls[0].arguments,
            serde_json::json!({"q": "rust"})
        );
        assert_eq!(response.tool_calls[1].id, "call_2");
        assert_eq!(response.tool_calls[1].name, "list_files");
        assert_eq!(response.tool_calls[1].arguments, serde_json::json!({}));
    }

    #[test]
    fn finalize_streamed_tool_calls_skips_incomplete_and_invalid_states() {
        let tool_calls = finalize_streamed_tool_calls(HashMap::from([
            (
                2,
                StreamedToolCallState {
                    id: Some("call_3".to_string()),
                    name: Some("noop".to_string()),
                    arguments: String::new(),
                    arguments_done: true,
                },
            ),
            (
                0,
                StreamedToolCallState {
                    id: Some("call_1".to_string()),
                    name: Some("search".to_string()),
                    arguments: r#"{"q":"rust"}"#.to_string(),
                    arguments_done: true,
                },
            ),
            (
                1,
                StreamedToolCallState {
                    id: Some("call_2".to_string()),
                    name: Some("read_file".to_string()),
                    arguments: r#"{"path":"README"#.to_string(),
                    arguments_done: false,
                },
            ),
            (
                3,
                StreamedToolCallState {
                    id: Some("call_4".to_string()),
                    name: Some("broken".to_string()),
                    arguments: "{".to_string(),
                    arguments_done: true,
                },
            ),
        ]));

        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].arguments, serde_json::json!({"q": "rust"}));
        assert_eq!(tool_calls[1].id, "call_3");
        assert_eq!(tool_calls[1].arguments, serde_json::json!({}));
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
            stream_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            permission_prompt_state: None,
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
            authority: None,
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
            last_session_messages: Vec::new(),
            stream_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            permission_prompt_state: None,
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
            tool_calls: vec!["read_file".to_string()],
            tool_inputs: vec![r#"{"path":"README.md"}"#.to_string()],
            tool_errors: vec!["missing file".to_string()],
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&output).unwrap()).unwrap();
        assert_eq!(json["response"], "hello");
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["iterations"], 2);
        assert_eq!(json["tool_calls"], serde_json::json!(["read_file"]));
        assert_eq!(
            json["tool_inputs"],
            serde_json::json!([r#"{"path":"README.md"}"#])
        );
        assert_eq!(json["tool_errors"], serde_json::json!(["missing file"]));
    }

    #[test]
    fn json_output_collects_tool_metadata_from_session_messages() {
        let result = CycleResult {
            response: "done".to_string(),
            model: "mock-model".to_string(),
            iterations: 3,
            tokens_used: TokenUsage::default(),
            result_kind: ResultKind::Complete,
        };
        let messages = vec![
            SessionMessage::structured(
                SessionRecordRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: None,
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "README.md"}),
                }],
                1,
                None,
            ),
            SessionMessage::structured(
                SessionRecordRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: serde_json::json!("missing"),
                    is_error: Some(true),
                }],
                2,
                None,
            ),
        ];

        let output = json_output_from_cycle(result, &messages);
        assert_eq!(output.tool_calls, vec!["read_file"]);
        assert_eq!(output.tool_inputs, vec![r#"{"path":"README.md"}"#]);
        assert_eq!(output.tool_errors, vec!["missing"]);
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
        write_test_signing_key(temp.path());
        install_test_skill(temp.path(), "demo", b"demo-wasm");
        install_test_skill(temp.path(), "weather", b"weather-wasm");

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
        assert!(sign.response.contains("Signed skill 'demo'"));
        assert!(temp.path().join("skills/demo/demo.wasm.sig").exists());

        let sign_all = process_input_with_commands(&mut app, "/sign --all", None)
            .await
            .expect("process sign all command");
        assert!(sign_all.response.contains("Signed skill 'demo'"));
        assert!(sign_all.response.contains("Signed skill 'weather'"));
        assert!(temp.path().join("skills/weather/weather.wasm.sig").exists());
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
            last_session_messages: Vec::new(),
            stream_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            permission_prompt_state: None,
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            bus_receiver: None,
        };

        let result = app.process_message("hello").await.expect("process message");
        let session_messages = app.take_last_session_messages();
        let assistant_message = session_messages
            .iter()
            .find(|message| message.role == SessionRecordRole::Assistant)
            .expect("assistant session message should be recorded");

        assert_eq!(result.model, "mock-model");
        assert!(result.iterations > 0);
        assert!(result.tokens_used.total_tokens() >= mock_completion_usage_total());
        assert_eq!(assistant_message.token_count, Some(5));
        assert_eq!(assistant_message.input_token_count, Some(3));
        assert_eq!(assistant_message.output_token_count, Some(2));
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
            last_session_messages: Vec::new(),
            stream_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            permission_prompt_state: None,
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
            signals: Vec::new(),
        };
        assert_eq!(extract_response_text(&result), "done");
        assert_eq!(extract_iterations(&result), 1);
        assert_eq!(extract_token_usage(&result).total_tokens(), 5);
        assert_eq!(extract_result_kind(&result), ResultKind::Complete);
    }

    #[test]
    fn extract_response_from_error_timeout_is_classified() {
        let result = LoopResult::Error {
            message: "request timed out after 30s".to_string(),
            recoverable: false,
            signals: Vec::new(),
        };

        assert_eq!(extract_response_text(&result), TIMEOUT_ERROR_RESPONSE);
        assert_eq!(extract_iterations(&result), 0);
        assert_eq!(extract_result_kind(&result), ResultKind::Error);
    }

    #[test]
    fn extract_response_from_error_blocked_is_classified() {
        let result = LoopResult::Error {
            message: "Tool 'run_command' blocked by policy".to_string(),
            recoverable: true,
            signals: Vec::new(),
        };

        assert_eq!(extract_response_text(&result), PERMISSION_ERROR_RESPONSE);
    }

    #[test]
    fn extract_response_from_error_auth_is_classified() {
        let result = LoopResult::Error {
            message: "provider returned 401 unauthorized".to_string(),
            recoverable: true,
            signals: Vec::new(),
        };

        assert_eq!(extract_response_text(&result), AUTH_ERROR_RESPONSE);
    }

    #[test]
    fn extract_response_from_error_rate_limit_is_classified() {
        let result = LoopResult::Error {
            message: "429 too many requests".to_string(),
            recoverable: true,
            signals: Vec::new(),
        };

        assert_eq!(extract_response_text(&result), RATE_LIMIT_ERROR_RESPONSE);
    }

    #[test]
    fn extract_response_from_error_network_is_classified() {
        let result = LoopResult::Error {
            message: "connection refused".to_string(),
            recoverable: true,
            signals: Vec::new(),
        };

        assert_eq!(extract_response_text(&result), NETWORK_ERROR_RESPONSE);
    }

    #[test]
    fn extract_response_from_error_unknown_uses_generic_fallback() {
        let result = LoopResult::Error {
            message: "boom".to_string(),
            recoverable: false,
            signals: Vec::new(),
        };

        assert_eq!(extract_response_text(&result), GENERIC_ERROR_RESPONSE);
        assert_eq!(extract_iterations(&result), 0);
        assert_eq!(extract_result_kind(&result), ResultKind::Error);
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
        assert_eq!(extract_result_kind(&result), ResultKind::Partial);
    }

    #[test]
    fn extract_response_from_budget_exhausted_without_content_uses_fallback() {
        let result = LoopResult::BudgetExhausted {
            partial_response: None,
            iterations: 3,
            signals: Vec::new(),
        };

        assert_eq!(
            extract_response_text(&result),
            BUDGET_EXHAUSTED_FALLBACK_RESPONSE
        );
        assert_eq!(extract_result_kind(&result), ResultKind::Empty);
    }

    #[test]
    fn extract_response_from_user_stopped_preserves_partial_response() {
        let result = LoopResult::UserStopped {
            partial_response: Some("partial".to_string()),
            iterations: 1,
            signals: Vec::new(),
        };

        assert_eq!(extract_response_text(&result), "partial");
        assert_eq!(extract_iterations(&result), 1);
        assert_eq!(extract_result_kind(&result), ResultKind::Partial);
    }

    #[test]
    fn finalize_cycle_sets_result_kind_for_each_variant() {
        let mut app = test_app();
        let signals = Vec::new();

        let complete = app.finalize_cycle(
            "hello",
            &LoopResult::Complete {
                response: "done".to_string(),
                iterations: 1,
                tokens_used: TokenUsage::default(),
                signals: signals.clone(),
            },
        );
        assert_eq!(complete.result_kind, ResultKind::Complete);

        let partial = app.finalize_cycle(
            "hello",
            &LoopResult::BudgetExhausted {
                partial_response: Some("partial".to_string()),
                iterations: 1,
                signals: signals.clone(),
            },
        );
        assert_eq!(partial.result_kind, ResultKind::Partial);

        let error = app.finalize_cycle(
            "hello",
            &LoopResult::Error {
                message: "network error".to_string(),
                recoverable: true,
                signals: signals.clone(),
            },
        );
        assert_eq!(error.result_kind, ResultKind::Error);

        let empty = app.finalize_cycle(
            "hello",
            &LoopResult::UserStopped {
                partial_response: None,
                iterations: 1,
                signals,
            },
        );
        assert_eq!(empty.result_kind, ResultKind::Empty);
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
            credential_store: None,
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
            credential_store: None,
            token_broker: None,
        };
        let factory = HeadlessSubagentFactory::new(deps);
        let debug = format!("{factory:?}");
        assert!(debug.contains("HeadlessSubagentFactory"));
    }

    #[test]
    fn subagent_build_options_inherit_shared_credential_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data_dir = dir.path().join(".fawx");
        std::fs::create_dir_all(&data_dir).expect("create data dir");
        let credential_store =
            crate::startup::open_credential_store(&data_dir).expect("open shared credential store");
        let factory = HeadlessSubagentFactory::new(HeadlessSubagentFactoryDeps {
            router: shared_router(ModelRouter::new()),
            config: FawxConfig::default(),
            improvement_provider: None,
            session_bus: None,
            credential_store: Some(Arc::clone(&credential_store)),
            token_broker: None,
        });
        let options = factory
            .subagent_build_options(&SpawnConfig::new("check bridge"), CancellationToken::new());

        let inherited = options
            .credential_store
            .as_ref()
            .expect("credential store should be inherited");

        assert!(Arc::ptr_eq(inherited, &credential_store));
    }
}
