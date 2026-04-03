use crate::types::{
    AuthProviderDto, ContextInfoDto, ErrorRecordDto, ModelInfoDto, ModelSwitchDto, SkillSummaryDto,
    ThinkingLevelDto,
};
use async_trait::async_trait;
use fx_bus::SessionBus;
use fx_config::manager::ConfigManager;
use fx_core::types::InputSource;
use fx_kernel::PermissionPromptState;
use fx_kernel::StreamCallback;
use fx_llm::{DocumentAttachment, ImageAttachment, Message};
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
pub trait AppEngine: Send + Sync {
    async fn process_message(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<CycleResult, anyhow::Error>;

    async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        context: Vec<Message>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<(CycleResult, Vec<Message>), anyhow::Error>;

    fn active_model(&self) -> &str;

    /// List all available models from the router.
    fn available_models(&self) -> Vec<ModelInfoDto>;

    /// Switch the active model and return the resolved model ID.
    fn set_active_model(&mut self, selector: &str) -> Result<ModelSwitchDto, anyhow::Error>;

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

    fn recent_errors(&self, limit: usize) -> Vec<ErrorRecordDto>;

    /// Maximum number of conversation history messages to retain per session.
    fn max_history(&self) -> usize {
        20
    }

    /// Session-level token usage (input + output tokens).
    fn session_token_usage(&self) -> (u64, u64) {
        (0, 0)
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

    /// Structured session messages recorded for the most recent completed turn.
    fn take_last_session_messages(&mut self) -> Vec<fx_session::SessionMessage> {
        Vec::new()
    }
}
