//! LLM routing logic for both legacy fallback strategies and model-provider routing.

use std::collections::HashMap;

use ct_core::error::LlmError;
use thiserror::Error;
use tracing::{debug, warn};

use crate::provider::LlmProvider as CompletionProvider;
use crate::types::{CompletionRequest, CompletionResponse};
use crate::LlmProvider;

/// Routes completion requests to the currently active model provider.
#[derive(Default)]
pub struct ModelRouter {
    providers: HashMap<String, Box<dyn CompletionProvider>>,
    active_model: Option<String>,
    model_to_provider: HashMap<String, String>,
    provider_auth_methods: HashMap<String, String>,
}

impl ModelRouter {
    /// Create an empty model router.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider and infer auth method metadata from its name.
    pub fn register_provider(&mut self, provider: Box<dyn CompletionProvider>) {
        let inferred_auth_method = infer_auth_method(provider.name());
        self.register_provider_with_auth(provider, inferred_auth_method);
    }

    /// Register a provider with an explicit auth method descriptor.
    pub fn register_provider_with_auth(
        &mut self,
        provider: Box<dyn CompletionProvider>,
        auth_method: impl Into<String>,
    ) {
        let provider_name = provider.name().to_string();
        let auth_method = auth_method.into();
        let supported_models = provider.supported_models();

        for model in supported_models {
            self.model_to_provider.insert(model, provider_name.clone());
        }

        self.provider_auth_methods
            .insert(provider_name.clone(), auth_method);
        self.providers.insert(provider_name, provider);
    }

    /// Set the active model.
    pub fn set_active(&mut self, model: &str) -> Result<(), RouterError> {
        if !self.model_to_provider.contains_key(model) {
            return Err(RouterError::ModelNotFound(model.to_string()));
        }

        self.active_model = Some(model.to_string());
        Ok(())
    }

    /// Return the active model identifier, if any.
    pub fn active_model(&self) -> Option<&str> {
        self.active_model.as_deref()
    }

    /// List all available models across all registered providers.
    pub fn available_models(&self) -> Vec<ModelInfo> {
        let mut models = self
            .model_to_provider
            .iter()
            .map(|(model_id, provider_name)| ModelInfo {
                model_id: model_id.clone(),
                provider_name: provider_name.clone(),
                auth_method: self
                    .provider_auth_methods
                    .get(provider_name)
                    .cloned()
                    .unwrap_or_else(|| infer_auth_method(provider_name)),
            })
            .collect::<Vec<_>>();

        models.sort_by(|left, right| left.model_id.cmp(&right.model_id));
        models
    }

    /// Send a completion request using the currently active model/provider pair.
    pub async fn complete(
        &self,
        mut request: CompletionRequest,
    ) -> Result<CompletionResponse, crate::types::LlmError> {
        let active_model = self.active_model.clone().ok_or_else(|| {
            crate::types::LlmError::Config(RouterError::NoActiveModel.to_string())
        })?;

        let provider_name = self.model_to_provider.get(&active_model).ok_or_else(|| {
            crate::types::LlmError::Config(
                RouterError::ModelNotFound(active_model.clone()).to_string(),
            )
        })?;

        let provider = self.providers.get(provider_name).ok_or_else(|| {
            crate::types::LlmError::Provider(format!(
                "provider '{provider_name}' was not registered"
            ))
        })?;

        request.model = active_model;
        provider.complete(request).await
    }
}

/// Metadata for an available model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// Provider model identifier.
    pub model_id: String,
    /// Provider slug/name.
    pub provider_name: String,
    /// Auth method category (`subscription`, `api_key`, etc.).
    pub auth_method: String,
}

/// Errors produced by model routing operations.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum RouterError {
    /// No active model is selected.
    #[error("no active model selected")]
    NoActiveModel,
    /// The requested model identifier is unknown.
    #[error("model not found: {0}")]
    ModelNotFound(String),
    /// Provider-level request failure.
    #[error("provider error: {0}")]
    ProviderError(crate::types::LlmError),
}

fn infer_auth_method(provider_name: &str) -> String {
    let provider = provider_name.to_ascii_lowercase();

    if provider.contains("setup") || provider.contains("oauth") || provider.contains("subscription")
    {
        return "subscription".to_string();
    }

    if provider == "anthropic" {
        // Default Anthropic path in Citros currently uses Claude subscriptions.
        return "subscription".to_string();
    }

    "api_key".to_string()
}

/// Strategy for routing LLM requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    /// Try local first, fall back to cloud on error
    LocalFirst,

    /// Try cloud first, fall back to local on error
    CloudFirst,

    /// Only use local (fail if local unavailable)
    LocalOnly,

    /// Only use cloud (fail if cloud unavailable)
    CloudOnly,
}

/// Routes LLM requests to appropriate providers based on strategy.
///
/// The router maintains typed references to local and cloud providers,
/// ensuring type-safe routing strategies.
#[derive(Debug)]
pub struct LlmRouter {
    local: Option<Box<dyn LlmProvider>>,
    cloud: Option<Box<dyn LlmProvider>>,
}

impl LlmRouter {
    /// Create a new router with optional local and cloud providers.
    ///
    /// # Arguments
    /// * `local` - Optional local LLM provider
    /// * `cloud` - Optional cloud LLM provider
    ///
    /// # Errors
    /// Returns `LlmError::Model` if both providers are None
    pub fn new(
        local: Option<Box<dyn LlmProvider>>,
        cloud: Option<Box<dyn LlmProvider>>,
    ) -> Result<Self, LlmError> {
        if local.is_none() && cloud.is_none() {
            return Err(LlmError::Model(
                "LlmRouter requires at least one provider".to_string(),
            ));
        }
        Ok(Self { local, cloud })
    }

    /// Generate completion using the specified routing strategy.
    ///
    /// # Arguments
    /// * `prompt` - Input text
    /// * `max_tokens` - Maximum number of tokens to generate
    /// * `routing` - Strategy for selecting provider
    ///
    /// # Returns
    /// Generated text or error if all applicable providers fail
    pub async fn generate(
        &self,
        prompt: &str,
        max_tokens: u32,
        routing: RoutingStrategy,
    ) -> Result<String, LlmError> {
        match routing {
            RoutingStrategy::LocalFirst => self.try_local_then_cloud(prompt, max_tokens).await,
            RoutingStrategy::CloudFirst => self.try_cloud_then_local(prompt, max_tokens).await,
            RoutingStrategy::LocalOnly => self.try_local_only(prompt, max_tokens).await,
            RoutingStrategy::CloudOnly => self.try_cloud_only(prompt, max_tokens).await,
        }
    }

    /// Try local provider first, fall back to cloud.
    async fn try_local_then_cloud(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, LlmError> {
        if let Some(ref local) = self.local {
            match local.generate(prompt, max_tokens).await {
                Ok(result) => {
                    debug!("Local generation succeeded");
                    Ok(result)
                }
                Err(e) => {
                    warn!("Local generation failed: {}, trying cloud fallback", e);
                    if let Some(ref cloud) = self.cloud {
                        cloud.generate(prompt, max_tokens).await
                    } else {
                        Err(LlmError::Inference(
                            "Local failed and no cloud fallback available".to_string(),
                        ))
                    }
                }
            }
        } else if let Some(ref cloud) = self.cloud {
            // No local provider, use cloud
            cloud.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No providers available".to_string()))
        }
    }

    /// Try cloud provider first, fall back to local.
    async fn try_cloud_then_local(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, LlmError> {
        if let Some(ref cloud) = self.cloud {
            match cloud.generate(prompt, max_tokens).await {
                Ok(result) => {
                    debug!("Cloud generation succeeded");
                    Ok(result)
                }
                Err(e) => {
                    warn!("Cloud generation failed: {}, trying local fallback", e);
                    if let Some(ref local) = self.local {
                        local.generate(prompt, max_tokens).await
                    } else {
                        Err(LlmError::Inference(
                            "Cloud failed and no local fallback available".to_string(),
                        ))
                    }
                }
            }
        } else if let Some(ref local) = self.local {
            // No cloud provider, use local
            local.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No providers available".to_string()))
        }
    }

    /// Only use local provider.
    async fn try_local_only(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError> {
        if let Some(ref local) = self.local {
            local.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No local provider available".to_string()))
        }
    }

    /// Only use cloud provider.
    async fn try_cloud_only(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError> {
        if let Some(ref cloud) = self.cloud {
            cloud.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No cloud provider available".to_string()))
        }
    }

    /// Get number of registered providers.
    pub fn provider_count(&self) -> usize {
        let mut count = 0;
        if self.local.is_some() {
            count += 1;
        }
        if self.cloud.is_some() {
            count += 1;
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Mock provider that tracks call count
    #[derive(Debug)]
    struct MockProvider {
        name: String,
        should_succeed: bool,
        response_text: String,
        call_count: Mutex<usize>,
    }

    impl MockProvider {
        fn new_success(name: &str, response: &str) -> Self {
            Self {
                name: name.to_string(),
                should_succeed: true,
                response_text: response.to_string(),
                call_count: Mutex::new(0),
            }
        }

        fn new_failure(name: &str, error_msg: &str) -> Self {
            Self {
                name: name.to_string(),
                should_succeed: false,
                response_text: error_msg.to_string(),
                call_count: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _max_tokens: u32) -> Result<String, LlmError> {
            *self.call_count.lock().unwrap() += 1;
            if self.should_succeed {
                Ok(self.response_text.clone())
            } else {
                Err(LlmError::Inference(self.response_text.clone()))
            }
        }

        async fn generate_streaming(
            &self,
            prompt: &str,
            max_tokens: u32,
            _callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, LlmError> {
            self.generate(prompt, max_tokens).await
        }

        fn model_name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_local_first_success() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::LocalFirst)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "local response");
    }

    #[tokio::test]
    async fn test_local_first_fallback() {
        let local = Box::new(MockProvider::new_failure("local", "local failed"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::LocalFirst)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cloud response");
    }

    #[tokio::test]
    async fn test_cloud_only() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::CloudOnly)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cloud response");
    }

    #[tokio::test]
    async fn test_local_only() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::LocalOnly)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "local response");
    }

    #[tokio::test]
    async fn test_local_only_fails_when_no_local() {
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(None, Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::LocalOnly)
            .await;

        // LocalOnly with no local provider should fail
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_provider_count() {
        let local = Box::new(MockProvider::new_success("local", "ok"));
        let cloud = Box::new(MockProvider::new_success("cloud", "ok"));

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        assert_eq!(router.provider_count(), 2);
    }

    #[tokio::test]
    async fn test_provider_count_single() {
        let local = Box::new(MockProvider::new_success("local", "ok"));

        let router = LlmRouter::new(Some(local), None).unwrap();
        assert_eq!(router.provider_count(), 1);
    }

    #[test]
    fn test_empty_providers_returns_error() {
        let result = LlmRouter::new(None, None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LlmError::Model(_)));
    }
}

#[cfg(test)]
mod model_router_tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    use crate::provider::{CompletionStream, LlmProvider as CompletionProvider};
    use crate::types::{CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message};

    #[derive(Debug)]
    struct MockCompletionProvider {
        provider_name: String,
        models: Vec<String>,
        response_text: String,
        captured_models: Arc<Mutex<Vec<String>>>,
    }

    impl MockCompletionProvider {
        fn new(
            provider_name: &str,
            models: Vec<&str>,
            response_text: &str,
            captured_models: Arc<Mutex<Vec<String>>>,
        ) -> Self {
            Self {
                provider_name: provider_name.to_string(),
                models: models.into_iter().map(ToString::to_string).collect(),
                response_text: response_text.to_string(),
                captured_models,
            }
        }
    }

    #[async_trait]
    impl CompletionProvider for MockCompletionProvider {
        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            self.captured_models.lock().unwrap().push(request.model);

            Ok(CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: self.response_text.clone(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            })
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, LlmError> {
            Err(LlmError::Streaming(
                "streaming not implemented for mock provider".to_string(),
            ))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            self.models.clone()
        }
    }

    fn request_with_model(model: &str) -> CompletionRequest {
        CompletionRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(256),
            system_prompt: None,
        }
    }

    fn first_text(response: &CompletionResponse) -> Option<String> {
        response.content.iter().find_map(|block| match block {
            ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
    }

    #[test]
    fn register_provider_lists_models_and_auth_metadata() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "openai-oauth",
            vec!["gpt-4o", "gpt-4o-mini"],
            "openai",
            captured,
        );

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));

        let models = router.available_models();
        assert_eq!(models.len(), 2);
        assert!(models
            .iter()
            .all(|model| model.provider_name == "openai-oauth"));
        assert!(models
            .iter()
            .all(|model| model.auth_method == "subscription"));
    }

    #[test]
    fn set_active_returns_error_for_unknown_model() {
        let mut router = ModelRouter::new();
        let result = router.set_active("missing-model");

        assert!(matches!(
            result,
            Err(RouterError::ModelNotFound(model)) if model == "missing-model"
        ));
    }

    #[tokio::test]
    async fn complete_routes_request_to_active_provider() {
        let anthropic_calls = Arc::new(Mutex::new(Vec::new()));
        let openai_calls = Arc::new(Mutex::new(Vec::new()));

        let anthropic = MockCompletionProvider::new(
            "anthropic",
            vec!["claude-opus-4"],
            "from anthropic",
            Arc::clone(&anthropic_calls),
        );
        let openai = MockCompletionProvider::new(
            "openai",
            vec!["gpt-4o"],
            "from openai",
            Arc::clone(&openai_calls),
        );

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(anthropic));
        router.register_provider(Box::new(openai));
        router.set_active("gpt-4o").unwrap();

        let response = router
            .complete(request_with_model("ignored"))
            .await
            .unwrap();

        assert_eq!(first_text(&response).as_deref(), Some("from openai"));
        assert_eq!(anthropic_calls.lock().unwrap().len(), 0);
        assert_eq!(
            openai_calls.lock().unwrap().clone(),
            vec!["gpt-4o".to_string()]
        );
    }

    #[tokio::test]
    async fn complete_without_active_model_returns_config_error() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "anthropic",
            vec!["claude-opus-4"],
            "from anthropic",
            captured,
        );

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));

        let result = router.complete(request_with_model("claude-opus-4")).await;

        assert!(
            matches!(result, Err(LlmError::Config(message)) if message.contains("no active model selected"))
        );
    }
}
