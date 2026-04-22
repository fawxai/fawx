use crate::types::{
    AuthProviderDto, ContextInfoDto, ErrorRecordDto, ModelInfoDto, ModelSwitchDto, SkillSummaryDto,
    ThinkingLevelDto,
};
use async_trait::async_trait;
use fx_bus::SessionBus;
use fx_config::manager::ConfigManager;
use fx_core::types::InputSource;
use fx_kernel::signals::Signal;
use fx_kernel::{CancellationToken, PermissionPromptState, StreamCallback, TokenUsage};
use fx_llm::{DocumentAttachment, ImageAttachment, Message};
use fx_session::SessionKey;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type ConfigManagerHandle = Arc<Mutex<ConfigManager>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleResult {
    pub response: String,
    pub model: String,
    pub iterations: u32,
    pub result_kind: ResultKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    Complete,
    Partial,
    Error,
    Empty,
}

#[async_trait]
#[allow(clippy::too_many_arguments)]
pub trait AppEngine: Send + Sync {
    async fn process_message(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<CycleResult, anyhow::Error>;

    async fn process_message_with_steering(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
        steering: Option<String>,
    ) -> Result<CycleResult, anyhow::Error> {
        let _ = steering;
        self.process_message(input, images, documents, source, callback)
            .await
    }

    async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        context: Vec<Message>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<(CycleResult, Vec<Message>), anyhow::Error>;

    async fn process_message_with_context_and_steering(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        context: Vec<Message>,
        source: InputSource,
        callback: Option<StreamCallback>,
        steering: Option<String>,
    ) -> Result<(CycleResult, Vec<Message>), anyhow::Error> {
        let _ = steering;
        self.process_message_with_context(input, images, documents, context, source, callback)
            .await
    }

    /// Attach the cancellation token that owns the next session turn.
    ///
    /// HTTP session Stop uses this as the server-side contract. Dropping an SSE
    /// client is not sufficient to cancel a turn because the engine may still
    /// be running inside a session-owned loop runner.
    fn set_turn_cancel_token(&mut self, token: CancellationToken) {
        let _ = token;
    }

    /// Attach the user-input channel that owns the next session turn.
    ///
    /// HTTP session steering uses this to deliver live guidance without
    /// cancelling the in-flight loop.
    fn set_turn_input_channel(&mut self, channel: fx_kernel::LoopInputChannel) {
        let _ = channel;
    }

    /// Build an isolated loop runner for a session turn.
    ///
    /// Implementations that return `Some` opt into true cross-session
    /// parallelism: the HTTP layer keeps one runner per session and only locks
    /// that session's runner while a turn is active. Implementations that return
    /// `None` fall back to the legacy shared app mutex.
    fn spawn_session_engine(
        &self,
        session_key: &SessionKey,
        execution_root: PathBuf,
    ) -> Result<Option<Box<dyn AppEngine>>, anyhow::Error> {
        let _ = session_key;
        let _ = execution_root;
        Ok(None)
    }

    fn active_model(&self) -> &str;

    /// List all available models from the router.
    fn available_models(&self) -> Vec<ModelInfoDto>;

    /// Fetch the current available models, allowing implementations to refresh
    /// dynamic provider catalogs before responding.
    async fn available_models_dynamic(&mut self) -> Vec<ModelInfoDto> {
        self.available_models()
    }

    /// Switch the active model and return the resolved model ID.
    fn set_active_model(&mut self, selector: &str) -> Result<ModelSwitchDto, anyhow::Error>;

    /// Temporarily replace the active model without changing the persisted default.
    ///
    /// Implementations that support session-scoped turns return the previous model so callers can
    /// restore it when the turn completes.
    fn replace_active_model(&mut self, selector: &str) -> Result<Option<String>, anyhow::Error> {
        let _ = selector;
        Ok(None)
    }

    /// Apply a turn-scoped thinking level for the currently active model.
    ///
    /// `None` means "use the app's persisted/global thinking preference, coerced to the
    /// active model's supported levels". Implementations that support session-scoped
    /// model routing should call this after any temporary model swap.
    fn apply_turn_thinking_level(&mut self, level: Option<&str>) -> Result<(), anyhow::Error> {
        let _ = level;
        Ok(())
    }

    /// Return thinking levels accepted by a specific model.
    fn thinking_levels_for_model(&self, model: &str) -> Vec<String> {
        let _ = model;
        vec!["off".to_string()]
    }

    /// Return the current thinking level and token budget.
    fn thinking_level(&self) -> ThinkingLevelDto;

    /// Return current conversation budget usage details.
    fn context_info(&self) -> ContextInfoDto;

    /// Return conversation budget usage details for an explicit message list.
    fn context_info_for_messages(&self, messages: &[Message]) -> ContextInfoDto;

    /// Update the thinking level and return the applied value.
    fn set_thinking_level(&mut self, level: &str) -> Result<ThinkingLevelDto, anyhow::Error>;

    /// List loaded skills, descriptions, and their exposed tool names.
    fn skill_summaries(&self) -> Vec<SkillSummaryDto>;

    /// List redacted auth provider configuration status.
    fn auth_provider_statuses(&self) -> Vec<AuthProviderDto>;

    fn config_manager(&self) -> Option<ConfigManagerHandle>;

    fn session_bus(&self) -> Option<&SessionBus>;

    fn permission_prompt_state(&self) -> Option<Arc<PermissionPromptState>> {
        None
    }

    fn reload_auth_state(&mut self) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn reload_providers(&mut self) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn reload_config(&mut self) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn recent_errors(&self, limit: usize) -> Vec<ErrorRecordDto>;

    /// Maximum number of conversation history messages to retain per session.
    fn max_history(&self) -> usize {
        20
    }

    /// Session-level token usage, including provider prompt-cache accounting.
    fn session_token_usage(&self) -> TokenUsage {
        TokenUsage::default()
    }

    /// Replace the active session memory, returning the previous value.
    fn replace_session_memory(
        &mut self,
        memory: fx_session::SessionMemory,
    ) -> fx_session::SessionMemory {
        let _ = memory;
        fx_session::SessionMemory::default()
    }

    /// Snapshot the active session memory.
    fn session_memory(&self) -> fx_session::SessionMemory {
        fx_session::SessionMemory::default()
    }

    /// Session key currently loaded into the in-memory loop engine, when any.
    fn loaded_session_key(&self) -> Option<fx_session::SessionKey> {
        None
    }

    /// Replace the active execution root, returning the previous root when
    /// the engine supports session-scoped workspace binding.
    fn replace_execution_root(&mut self, root: PathBuf) -> Option<PathBuf> {
        let _ = root;
        None
    }

    /// Structured session messages recorded for the most recent completed turn.
    fn take_last_session_messages(&mut self) -> Vec<fx_session::SessionMessage> {
        Vec::new()
    }

    /// Drain structured loop signals recorded for the most recent completed turn.
    fn take_last_cycle_signals(&mut self) -> Vec<Signal> {
        Vec::new()
    }
}
