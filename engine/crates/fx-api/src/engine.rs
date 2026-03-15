use crate::types::{
    AuthProviderDto, ContextInfoDto, ErrorRecordDto, ModelInfoDto, ModelSwitchDto, SkillSummaryDto,
    ThinkingLevelDto,
};
use async_trait::async_trait;
use fx_bus::SessionBus;
use fx_config::manager::ConfigManager;
use fx_core::types::InputSource;
use fx_kernel::StreamCallback;
use fx_llm::{ImageAttachment, Message};
use std::sync::{Arc, Mutex};

pub type ConfigManagerHandle = Arc<Mutex<ConfigManager>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleResult {
    pub response: String,
    pub model: String,
    pub iterations: u32,
}

#[async_trait]
pub trait AppEngine: Send + Sync {
    async fn process_message(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<CycleResult, anyhow::Error>;

    async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
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

    fn reload_auth_state(&mut self) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn recent_errors(&self, limit: usize) -> Vec<ErrorRecordDto>;
}
