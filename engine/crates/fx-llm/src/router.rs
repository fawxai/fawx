//! LLM routing logic for both legacy fallback strategies and model-provider routing.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use futures::future::join_all;
use fx_core::error::LlmError;
use thiserror::Error;
use tracing::{debug, warn};

use crate::provider::{CompletionStream, LlmProvider as CompletionProvider};
use crate::streaming::StreamCallback;
use crate::types::{CompletionRequest, CompletionResponse, LlmError as ProviderLlmError};
use crate::LlmProvider;

/// Routes completion requests to the currently active model provider.
#[derive(Default)]
pub struct ModelRouter {
    providers: HashMap<String, Arc<dyn CompletionProvider>>,
    active_model: Option<String>,
    model_to_provider: HashMap<String, String>,
    provider_auth_methods: HashMap<String, String>,
}

impl ModelRouter {
    /// Create an empty model router.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider using the auth method declared by the provider instance.
    pub fn register_provider(&mut self, provider: Box<dyn CompletionProvider>) {
        let provider: Arc<dyn CompletionProvider> = provider.into();
        let auth_method = provider.auth_method().to_string();
        self.register_provider_with_auth(provider, auth_method);
    }

    /// Register a provider with an explicit auth method descriptor.
    pub fn register_provider_with_auth(
        &mut self,
        provider: Arc<dyn CompletionProvider>,
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
        if model.trim().is_empty() {
            return Err(RouterError::EmptyModelSelector);
        }

        let resolved_model = self.resolve_model(model)?;
        self.active_model = Some(resolved_model);
        Ok(())
    }

    fn resolve_model(&self, model: &str) -> Result<String, RouterError> {
        if self.model_to_provider.contains_key(model) {
            return Ok(model.to_string());
        }

        let mut prefix_matches = self
            .model_to_provider
            .keys()
            .filter(|candidate| candidate.starts_with(model));

        let Some(first_match) = prefix_matches.next() else {
            return Err(RouterError::ModelNotFound(model.to_string()));
        };

        if prefix_matches.next().is_some() {
            return Err(RouterError::AmbiguousModel(model.to_string()));
        }

        Ok(first_match.to_string())
    }

    /// Return the active model identifier, if any.
    pub fn active_model(&self) -> Option<&str> {
        self.active_model.as_deref()
    }

    /// Return the provider for a model identifier, if registered.
    pub fn provider_for_model(&self, model: &str) -> Option<&str> {
        self.model_to_provider.get(model).map(String::as_str)
    }

    /// Return the provider for the active model, if any.
    pub fn active_provider(&self) -> Option<&str> {
        self.active_model()
            .and_then(|model| self.provider_for_model(model))
    }

    /// Snapshot the registered providers so callers can work without holding a router borrow.
    pub fn provider_catalog(&self) -> Vec<ProviderCatalogEntry> {
        self.providers
            .iter()
            .map(|(provider_name, provider)| ProviderCatalogEntry {
                provider_name: provider_name.clone(),
                auth_method: provider_auth_method(
                    &self.providers,
                    &self.provider_auth_methods,
                    provider_name,
                ),
                provider: Arc::clone(provider),
            })
            .collect()
    }

    /// List all available models across all registered providers.
    pub fn available_models(&self) -> Vec<ModelInfo> {
        build_model_infos(
            &self.model_to_provider,
            &self.providers,
            &self.provider_auth_methods,
        )
    }

    /// Fetch available models from all registered providers dynamically.
    pub async fn fetch_available_models(&self) -> Vec<ModelInfo> {
        fetch_available_models_from_catalog(self.provider_catalog()).await
    }

    pub fn context_window_for_model(&self, model: &str) -> Result<usize, RouterError> {
        let (resolved_model, provider) = self.resolved_provider(model)?;
        Ok(provider.context_window(&resolved_model))
    }

    pub fn thinking_levels_for_model(
        &self,
        model: &str,
    ) -> Result<&'static [&'static str], RouterError> {
        let (resolved_model, provider) = self.resolved_provider(model)?;
        Ok(provider.thinking_levels(&resolved_model))
    }

    /// Prepare a request for a specific model without borrowing the router across await points.
    pub fn request_for_model(
        &self,
        model: &str,
        mut request: CompletionRequest,
    ) -> Result<(Arc<dyn CompletionProvider>, CompletionRequest), ProviderLlmError> {
        if model.trim().is_empty() {
            return Err(ProviderLlmError::Config(
                RouterError::EmptyModelSelector.to_string(),
            ));
        }

        let (resolved_model, provider) = self
            .resolved_provider(model)
            .map_err(|error| ProviderLlmError::Config(error.to_string()))?;

        request.model = resolved_model;
        if !provider.capabilities().supports_temperature {
            request.temperature = None;
        }
        Ok((provider, request))
    }

    /// Send a completion request using the currently active model/provider pair.
    pub async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderLlmError> {
        let (provider, normalized_request) = self.request_for_active_provider(request)?;
        provider.complete(normalized_request).await
    }

    /// Send a streaming completion request to the active provider.
    pub async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderLlmError> {
        let (provider, normalized_request) = self.request_for_active_provider(request)?;
        provider.complete_stream(normalized_request).await
    }

    /// Send a completion request and emit normalized provider stream events.
    pub async fn stream(
        &self,
        request: CompletionRequest,
        callback: StreamCallback,
    ) -> Result<CompletionResponse, ProviderLlmError> {
        let (provider, normalized_request) = self.request_for_active_provider(request)?;
        provider.stream(normalized_request, callback).await
    }

    fn request_for_active_provider(
        &self,
        request: CompletionRequest,
    ) -> Result<(Arc<dyn CompletionProvider>, CompletionRequest), ProviderLlmError> {
        let active_model = self
            .active_model
            .clone()
            .ok_or_else(|| ProviderLlmError::Config(RouterError::NoActiveModel.to_string()))?;
        self.request_for_model(&active_model, request)
    }

    fn resolved_provider(
        &self,
        model: &str,
    ) -> Result<(String, Arc<dyn CompletionProvider>), RouterError> {
        let resolved_model = self.resolve_model(model)?;
        let provider_name = self
            .model_to_provider
            .get(&resolved_model)
            .ok_or_else(|| RouterError::ModelNotFound(resolved_model.clone()))?;
        let provider = self.providers.get(provider_name).cloned().ok_or_else(|| {
            RouterError::ProviderError(ProviderLlmError::Provider(format!(
                "provider '{provider_name}' was not registered"
            )))
        })?;
        Ok((resolved_model, provider))
    }
}

#[derive(Clone)]
pub struct ProviderCatalogEntry {
    pub provider_name: String,
    pub auth_method: String,
    pub provider: Arc<dyn CompletionProvider>,
}

pub async fn fetch_available_models_from_catalog(
    catalog: Vec<ProviderCatalogEntry>,
) -> Vec<ModelInfo> {
    let fetches = catalog.into_iter().map(|entry| async move {
        (
            entry.provider_name,
            entry.auth_method,
            fetch_provider_models(entry.provider.as_ref()).await,
        )
    });
    let mut model_entries = BTreeMap::new();

    for (provider_name, auth_method, model_ids) in join_all(fetches).await {
        add_provider_models(&mut model_entries, &provider_name, &auth_method, model_ids);
    }

    model_entries.into_values().collect()
}

async fn fetch_provider_models(provider: &dyn CompletionProvider) -> Vec<String> {
    match provider.list_models().await {
        Ok(models) => models,
        Err(error) => {
            warn!(provider = provider.name(), error = %error, "failed to fetch provider models; using static fallback");
            provider.supported_models()
        }
    }
}

fn add_provider_models(
    model_entries: &mut BTreeMap<String, ModelInfo>,
    provider_name: &str,
    auth_method: &str,
    model_ids: Vec<String>,
) {
    for model_id in model_ids {
        model_entries
            .entry(model_id.clone())
            .or_insert_with(|| ModelInfo {
                model_id,
                provider_name: provider_name.to_string(),
                auth_method: auth_method.to_string(),
            });
    }
}

fn build_model_infos(
    model_to_provider: &HashMap<String, String>,
    providers: &HashMap<String, Arc<dyn CompletionProvider>>,
    provider_auth_methods: &HashMap<String, String>,
) -> Vec<ModelInfo> {
    let mut models = model_to_provider
        .iter()
        .map(|(model_id, provider_name)| ModelInfo {
            model_id: model_id.clone(),
            provider_name: provider_name.clone(),
            auth_method: provider_auth_method(providers, provider_auth_methods, provider_name),
        })
        .collect::<Vec<_>>();
    models.sort_by(|left, right| left.model_id.cmp(&right.model_id));
    models
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
    /// The model selector matches multiple registered models.
    #[error("ambiguous model selector: {0}")]
    AmbiguousModel(String),
    /// The model selector is empty.
    #[error("model selector cannot be empty")]
    EmptyModelSelector,
    /// Provider-level request failure.
    #[error("provider error: {0}")]
    ProviderError(ProviderLlmError),
}

fn provider_auth_method(
    providers: &HashMap<String, Arc<dyn CompletionProvider>>,
    overrides: &HashMap<String, String>,
    provider_name: &str,
) -> String {
    overrides
        .get(provider_name)
        .cloned()
        .or_else(|| {
            providers
                .get(provider_name)
                .map(|provider| provider.auth_method().to_string())
        })
        .unwrap_or_else(|| "api_key".to_string())
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
    use futures::stream;
    use std::sync::{Arc, Mutex};

    use crate::provider::{
        CompletionStream, LlmProvider as CompletionProvider, ProviderCapabilities,
    };
    use crate::types::{CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message};

    #[derive(Debug)]
    struct MockCompletionProvider {
        provider_name: String,
        models: Vec<String>,
        response_text: String,
        dynamic_models: Result<Vec<String>, String>,
        auth_method: &'static str,
        context_window: usize,
        thinking_levels: &'static [&'static str],
        captured_models: Arc<Mutex<Vec<String>>>,
        captured_temperatures: Arc<Mutex<Vec<Option<f32>>>>,
        capabilities: ProviderCapabilities,
        list_models_delay_ms: u64,
    }

    impl MockCompletionProvider {
        fn new(
            provider_name: &str,
            models: Vec<&str>,
            response_text: &str,
            captured_models: Arc<Mutex<Vec<String>>>,
            captured_temperatures: Arc<Mutex<Vec<Option<f32>>>>,
            capabilities: ProviderCapabilities,
        ) -> Self {
            let model_ids = models.iter().map(ToString::to_string).collect::<Vec<_>>();
            Self {
                provider_name: provider_name.to_string(),
                models: model_ids.clone(),
                response_text: response_text.to_string(),
                dynamic_models: Ok(model_ids),
                auth_method: "api_key",
                context_window: 128_000,
                thinking_levels: &["off"],
                captured_models,
                captured_temperatures,
                capabilities,
                list_models_delay_ms: 0,
            }
        }

        fn with_dynamic_models(mut self, dynamic_models: Result<Vec<String>, String>) -> Self {
            self.dynamic_models = dynamic_models;
            self
        }

        fn with_list_models_delay_ms(mut self, delay_ms: u64) -> Self {
            self.list_models_delay_ms = delay_ms;
            self
        }

        fn with_auth_method(mut self, auth_method: &'static str) -> Self {
            self.auth_method = auth_method;
            self
        }

        fn with_context_window(mut self, context_window: usize) -> Self {
            self.context_window = context_window;
            self
        }

        fn with_thinking_levels(mut self, thinking_levels: &'static [&'static str]) -> Self {
            self.thinking_levels = thinking_levels;
            self
        }
    }

    #[async_trait]
    impl CompletionProvider for MockCompletionProvider {
        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            self.captured_models.lock().unwrap().push(request.model);
            self.captured_temperatures
                .lock()
                .unwrap()
                .push(request.temperature);

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
            request: CompletionRequest,
        ) -> Result<CompletionStream, LlmError> {
            self.captured_models.lock().unwrap().push(request.model);
            self.captured_temperatures
                .lock()
                .unwrap()
                .push(request.temperature);

            Ok(Box::pin(stream::empty()))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            self.models.clone()
        }

        async fn list_models(&self) -> Result<Vec<String>, LlmError> {
            if self.list_models_delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.list_models_delay_ms))
                    .await;
            }
            self.dynamic_models.clone().map_err(LlmError::Provider)
        }

        fn capabilities(&self) -> ProviderCapabilities {
            self.capabilities
        }

        fn auth_method(&self) -> &'static str {
            self.auth_method
        }

        fn context_window(&self, _model: &str) -> usize {
            self.context_window
        }

        fn thinking_levels(&self, _model: &str) -> &'static [&'static str] {
            self.thinking_levels
        }
    }

    fn request_with_model(model: &str) -> CompletionRequest {
        request_with_temperature(model, None)
    }

    fn request_with_temperature(model: &str, temperature: Option<f32>) -> CompletionRequest {
        CompletionRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature,
            max_tokens: Some(256),
            system_prompt: None,
            thinking: None,
        }
    }

    fn first_text(response: &CompletionResponse) -> Option<String> {
        response.content.iter().find_map(|block| match block {
            ContentBlock::Text { text } => Some(text.clone()),
            ContentBlock::Image { .. } => None,
            ContentBlock::Document { .. } => None,
            _ => None,
        })
    }

    fn default_capabilities() -> ProviderCapabilities {
        ProviderCapabilities {
            supports_temperature: true,
            requires_streaming: false,
        }
    }

    #[derive(Debug)]
    struct StaticOnlyProvider {
        models: Vec<String>,
    }

    #[async_trait]
    impl CompletionProvider for StaticOnlyProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::Provider("unused".to_string()))
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, LlmError> {
            Err(LlmError::Provider("unused".to_string()))
        }

        fn name(&self) -> &str {
            "static-only"
        }

        fn supported_models(&self) -> Vec<String> {
            self.models.clone()
        }

        fn capabilities(&self) -> ProviderCapabilities {
            default_capabilities()
        }
    }

    #[test]
    fn register_provider_lists_models_and_auth_metadata() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "openai-oauth",
            vec!["gpt-4o", "gpt-4o-mini"],
            "openai",
            captured,
            temperatures,
            default_capabilities(),
        )
        .with_auth_method("subscription");

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
    fn register_provider_with_auth_uses_explicit_auth_method() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "openai",
            vec!["gpt-4o"],
            "from openai",
            captured,
            temperatures,
            default_capabilities(),
        );

        let mut router = ModelRouter::new();
        router.register_provider_with_auth(Arc::new(provider), "api_key");

        let models = router.available_models();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].auth_method, "api_key");
    }

    #[test]
    fn context_window_for_model_uses_provider_contract() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "custom",
            vec!["custom-model"],
            "from custom",
            captured,
            temperatures,
            default_capabilities(),
        )
        .with_context_window(42_000);

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));

        let context_window = router
            .context_window_for_model("custom-model")
            .expect("context window");

        assert_eq!(context_window, 42_000);
    }

    #[test]
    fn thinking_levels_for_model_use_provider_contract() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "custom",
            vec!["custom-model"],
            "from custom",
            captured,
            temperatures,
            default_capabilities(),
        )
        .with_thinking_levels(&["off", "careful"]);

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));

        let levels = router
            .thinking_levels_for_model("custom-model")
            .expect("thinking levels");

        assert_eq!(levels, &["off", "careful"]);
    }

    #[tokio::test]
    async fn router_fetch_merges_providers() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let openai = MockCompletionProvider::new(
            "openai",
            vec!["gpt-4o"],
            "from openai",
            Arc::clone(&captured),
            Arc::clone(&temperatures),
            default_capabilities(),
        )
        .with_dynamic_models(Ok(vec!["gpt-4o".to_string(), "gpt-4.1".to_string()]));
        let anthropic = MockCompletionProvider::new(
            "anthropic",
            vec!["claude-opus-4-1-20250805"],
            "from anthropic",
            Arc::clone(&captured),
            Arc::clone(&temperatures),
            default_capabilities(),
        )
        .with_dynamic_models(Ok(vec![
            "claude-opus-4-1-20250805".to_string(),
            "gpt-4o".to_string(),
        ]));

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(openai));
        router.register_provider(Box::new(anthropic));

        let models = router.fetch_available_models().await;
        let ids = models
            .iter()
            .map(|model| model.model_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["claude-opus-4-1-20250805", "gpt-4.1", "gpt-4o"]);
    }

    #[tokio::test]
    async fn router_fetches_providers_in_parallel() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let openai = MockCompletionProvider::new(
            "openai",
            vec!["gpt-4o"],
            "from openai",
            Arc::clone(&captured),
            Arc::clone(&temperatures),
            default_capabilities(),
        )
        .with_dynamic_models(Ok(vec!["gpt-4o".to_string()]))
        .with_list_models_delay_ms(150);
        let anthropic = MockCompletionProvider::new(
            "anthropic",
            vec!["claude-opus-4-1-20250805"],
            "from anthropic",
            Arc::clone(&captured),
            Arc::clone(&temperatures),
            default_capabilities(),
        )
        .with_dynamic_models(Ok(vec!["claude-opus-4-1-20250805".to_string()]))
        .with_list_models_delay_ms(150);

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(openai));
        router.register_provider(Box::new(anthropic));

        let started = tokio::time::Instant::now();
        let models = router.fetch_available_models().await;

        assert!(started.elapsed() < std::time::Duration::from_millis(275));
        assert_eq!(models.len(), 2);
    }

    #[tokio::test]
    async fn router_fetch_partial_failure() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let openai = MockCompletionProvider::new(
            "openai",
            vec!["gpt-4o"],
            "from openai",
            Arc::clone(&captured),
            Arc::clone(&temperatures),
            default_capabilities(),
        )
        .with_dynamic_models(Ok(vec!["gpt-4.1".to_string()]));
        let anthropic = MockCompletionProvider::new(
            "anthropic",
            vec!["claude-opus-4-1-20250805"],
            "from anthropic",
            Arc::clone(&captured),
            Arc::clone(&temperatures),
            default_capabilities(),
        )
        .with_dynamic_models(Err("boom".to_string()));

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(openai));
        router.register_provider(Box::new(anthropic));

        let models = router.fetch_available_models().await;
        let ids = models
            .iter()
            .map(|model| model.model_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["claude-opus-4-1-20250805", "gpt-4.1"]);
    }

    #[tokio::test]
    async fn list_models_default_impl_returns_supported() {
        let provider = StaticOnlyProvider {
            models: vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()],
        };

        let models = CompletionProvider::list_models(&provider)
            .await
            .expect("default list models");

        assert_eq!(
            models,
            vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]
        );
    }

    #[tokio::test]
    async fn list_models_skips_unconfigured_provider() {
        let provider = StaticOnlyProvider {
            models: vec!["claude-opus-4-1-20250805".to_string()],
        };

        let models = CompletionProvider::list_models(&provider)
            .await
            .expect("static fallback without auth");

        assert_eq!(models, vec!["claude-opus-4-1-20250805".to_string()]);
    }

    #[test]
    fn set_active_rejects_empty_selector() {
        let mut router = ModelRouter::new();

        let result = router.set_active("");

        assert!(matches!(result, Err(RouterError::EmptyModelSelector)));
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

    #[test]
    fn set_active_accepts_unique_model_prefix() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "anthropic",
            vec!["claude-sonnet-4-6-20250929"],
            "from anthropic",
            captured,
            temperatures,
            default_capabilities(),
        );

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));

        router
            .set_active("claude-sonnet-4-6")
            .expect("prefix should resolve to the full model id");

        assert_eq!(router.active_model(), Some("claude-sonnet-4-6-20250929"));
    }

    #[test]
    fn set_active_returns_error_for_ambiguous_model_prefix() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "anthropic",
            vec!["claude-sonnet-4-6-20250929", "claude-sonnet-4-6-20251001"],
            "from anthropic",
            captured,
            temperatures,
            default_capabilities(),
        );

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));

        let result = router.set_active("claude-sonnet-4-6");

        assert!(matches!(
            result,
            Err(RouterError::AmbiguousModel(model)) if model == "claude-sonnet-4-6"
        ));
    }

    #[test]
    fn model_switch_updates_active_model() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "openai",
            vec!["gpt-4o"],
            "from openai",
            captured,
            temperatures,
            default_capabilities(),
        );

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));
        router.set_active("gpt-4o").unwrap();

        assert_eq!(router.active_model(), Some("gpt-4o"));
    }

    #[tokio::test]
    async fn complete_routes_request_to_active_provider() {
        let anthropic_calls = Arc::new(Mutex::new(Vec::new()));
        let openai_calls = Arc::new(Mutex::new(Vec::new()));
        let anthropic_temperatures = Arc::new(Mutex::new(Vec::new()));
        let openai_temperatures = Arc::new(Mutex::new(Vec::new()));

        let anthropic = MockCompletionProvider::new(
            "anthropic",
            vec!["claude-opus-4"],
            "from anthropic",
            Arc::clone(&anthropic_calls),
            Arc::clone(&anthropic_temperatures),
            default_capabilities(),
        );
        let openai = MockCompletionProvider::new(
            "openai",
            vec!["gpt-4o"],
            "from openai",
            Arc::clone(&openai_calls),
            Arc::clone(&openai_temperatures),
            default_capabilities(),
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
        assert_eq!(anthropic_temperatures.lock().unwrap().len(), 0);
        assert_eq!(openai_temperatures.lock().unwrap().clone(), vec![None]);
    }

    #[tokio::test]
    async fn complete_without_active_model_returns_config_error() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "anthropic",
            vec!["claude-opus-4"],
            "from anthropic",
            captured,
            temperatures,
            default_capabilities(),
        );

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));

        let result = router.complete(request_with_model("claude-opus-4")).await;

        assert!(
            matches!(result, Err(LlmError::Config(message)) if message.contains("no active model selected"))
        );
    }

    #[tokio::test]
    async fn complete_strips_temperature_when_provider_does_not_support_it() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "openai-responses",
            vec!["gpt-5"],
            "ok",
            Arc::clone(&calls),
            Arc::clone(&temperatures),
            ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            },
        );

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));
        router.set_active("gpt-5").unwrap();

        let result = router
            .complete(request_with_temperature("ignored", Some(0.9)))
            .await;

        assert!(result.is_ok());
        assert_eq!(calls.lock().unwrap().clone(), vec!["gpt-5".to_string()]);
        assert_eq!(temperatures.lock().unwrap().clone(), vec![None]);
    }

    #[tokio::test]
    async fn complete_stream_strips_temperature_when_provider_does_not_support_it() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = MockCompletionProvider::new(
            "openai-responses",
            vec!["gpt-5"],
            "ok",
            Arc::clone(&calls),
            Arc::clone(&temperatures),
            ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            },
        );

        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));
        router.set_active("gpt-5").unwrap();

        let result = router
            .complete_stream(request_with_temperature("ignored", Some(0.3)))
            .await;

        assert!(result.is_ok());
        assert_eq!(calls.lock().unwrap().clone(), vec!["gpt-5".to_string()]);
        assert_eq!(temperatures.lock().unwrap().clone(), vec![None]);
    }
}

#[cfg(test)]
mod thinking_level_tests {
    use super::ModelRouter;
    use crate::provider::{
        CompletionStream, LlmProvider as CompletionProvider, ProviderCapabilities,
    };
    use crate::types::{CompletionRequest, CompletionResponse, LlmError};
    use async_trait::async_trait;

    #[derive(Debug)]
    struct StaticProvider {
        name: &'static str,
        models: Vec<&'static str>,
    }

    #[async_trait]
    impl CompletionProvider for StaticProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::Provider("unused".to_string()))
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, LlmError> {
            Err(LlmError::Provider("unused".to_string()))
        }

        fn name(&self) -> &str {
            self.name
        }

        fn supported_models(&self) -> Vec<String> {
            self.models.iter().map(ToString::to_string).collect()
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }
    }

    #[test]
    fn active_provider_uses_active_model_mapping() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(StaticProvider {
            name: "anthropic",
            models: vec!["claude-sonnet-4-20250514"],
        }));
        router
            .set_active("claude-sonnet-4-20250514")
            .expect("set active");

        assert_eq!(router.active_provider(), Some("anthropic"));
        assert_eq!(
            router.provider_for_model("claude-sonnet-4-20250514"),
            Some("anthropic")
        );
    }
}
