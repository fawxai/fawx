use crate::engine::{AppEngine, ConfigManagerHandle, CycleResult as ApiCycleResult};
use crate::types::{
    AuthProviderDto, ContextInfoDto, ErrorRecordDto, ModelInfoDto, ModelSwitchDto, SkillSummaryDto,
    ThinkingLevelDto,
};
use async_trait::async_trait;
use fx_bus::SessionBus;
use fx_core::types::InputSource;
use fx_kernel::{PermissionPromptState, StreamCallback};
use fx_llm::{DocumentAttachment, ImageAttachment, Message};
use std::sync::Arc;

pub(crate) struct StubAppEngine {
    active_model: String,
    static_models: Vec<ModelInfoDto>,
    dynamic_models: Option<Vec<ModelInfoDto>>,
    permission_prompt_state: Option<Arc<PermissionPromptState>>,
}

impl Default for StubAppEngine {
    fn default() -> Self {
        Self {
            active_model: "mock-model".to_string(),
            static_models: Vec::new(),
            dynamic_models: None,
            permission_prompt_state: None,
        }
    }
}

impl StubAppEngine {
    pub(crate) fn with_active_model(mut self, active_model: impl Into<String>) -> Self {
        self.active_model = active_model.into();
        self
    }

    pub(crate) fn with_static_models(mut self, static_models: Vec<ModelInfoDto>) -> Self {
        self.static_models = static_models;
        self
    }

    pub(crate) fn with_dynamic_models(mut self, dynamic_models: Vec<ModelInfoDto>) -> Self {
        self.dynamic_models = Some(dynamic_models);
        self
    }

    pub(crate) fn with_permission_prompt_state(
        mut self,
        permission_prompt_state: Arc<PermissionPromptState>,
    ) -> Self {
        self.permission_prompt_state = Some(permission_prompt_state);
        self
    }
}

#[async_trait]
impl AppEngine for StubAppEngine {
    async fn process_message(
        &mut self,
        _input: &str,
        _images: Vec<ImageAttachment>,
        _documents: Vec<DocumentAttachment>,
        _source: InputSource,
        _callback: Option<StreamCallback>,
    ) -> Result<ApiCycleResult, anyhow::Error> {
        unreachable!("not used in API test support")
    }

    async fn process_message_with_context(
        &mut self,
        _input: &str,
        _images: Vec<ImageAttachment>,
        _documents: Vec<DocumentAttachment>,
        _context: Vec<Message>,
        _source: InputSource,
        _callback: Option<StreamCallback>,
    ) -> Result<(ApiCycleResult, Vec<Message>), anyhow::Error> {
        unreachable!("not used in API test support")
    }

    fn active_model(&self) -> &str {
        &self.active_model
    }

    fn available_models(&self) -> Vec<ModelInfoDto> {
        self.static_models.clone()
    }

    async fn available_models_dynamic(&self) -> Vec<ModelInfoDto> {
        self.dynamic_models
            .clone()
            .unwrap_or_else(|| self.static_models.clone())
    }

    fn set_active_model(&mut self, selector: &str) -> Result<ModelSwitchDto, anyhow::Error> {
        let previous_model = std::mem::replace(&mut self.active_model, selector.to_string());
        Ok(ModelSwitchDto {
            previous_model,
            active_model: self.active_model.clone(),
            thinking_adjusted: None,
        })
    }

    fn thinking_level(&self) -> ThinkingLevelDto {
        ThinkingLevelDto {
            level: "normal".to_string(),
            budget_tokens: None,
            available: Vec::new(),
        }
    }

    fn context_info(&self) -> ContextInfoDto {
        ContextInfoDto {
            used_tokens: 0,
            max_tokens: 4_096,
            percentage: 0.0,
            compaction_threshold: 0.8,
        }
    }

    fn context_info_for_messages(&self, _messages: &[Message]) -> ContextInfoDto {
        self.context_info()
    }

    fn set_thinking_level(&mut self, _level: &str) -> Result<ThinkingLevelDto, anyhow::Error> {
        Ok(self.thinking_level())
    }

    fn skill_summaries(&self) -> Vec<SkillSummaryDto> {
        Vec::new()
    }

    fn auth_provider_statuses(&self) -> Vec<AuthProviderDto> {
        Vec::new()
    }

    fn config_manager(&self) -> Option<ConfigManagerHandle> {
        None
    }

    fn session_bus(&self) -> Option<&SessionBus> {
        None
    }

    fn permission_prompt_state(&self) -> Option<Arc<PermissionPromptState>> {
        self.permission_prompt_state.as_ref().map(Arc::clone)
    }

    fn recent_errors(&self, _limit: usize) -> Vec<ErrorRecordDto> {
        Vec::new()
    }
}
