//! Dynamic model discovery with provider-aware filtering and cache fallback.

use async_trait::async_trait;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::anthropic::AnthropicProvider;
use crate::openai::OpenAiProvider;
use crate::provider::{CompletionStream, LlmProvider as CompletionProvider, ProviderCapabilities};
use crate::types::{CompletionRequest, CompletionResponse, LlmError};

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Maximum model age in seconds (~180 days). Models older than this are filtered out.
const MODEL_AGE_CUTOFF_SECS: u64 = 180 * 24 * 60 * 60;
/// Minimum input price per token (USD) to filter out weak-tier models.
/// $3/M tokens = 0.000003 per token. Roughly sonnet-tier floor.
const MIN_INPUT_PRICE_PER_TOKEN: f64 = 0.000003;

/// A discovered model entry from a provider catalog endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogModel {
    pub id: String,
    pub display_name: Option<String>,
    pub provider: String,
}

/// In-memory dynamic model catalog with provider-scoped cache.
#[derive(Debug)]
pub struct ModelCatalog {
    cache: HashMap<String, CacheEntry>,
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    models: Vec<CatalogModel>,
    fetched_at: Instant,
}

#[derive(Debug)]
struct UnknownCatalogProvider {
    name: String,
}

impl UnknownCatalogProvider {
    fn new(name: &str) -> Self {
        Self {
            name: normalize_provider(name),
        }
    }
}

#[async_trait]
impl CompletionProvider for UnknownCatalogProvider {
    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Err(LlmError::Provider(format!(
            "provider '{}' does not support completions",
            self.name
        )))
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionStream, LlmError> {
        Err(LlmError::Provider(format!(
            "provider '{}' does not support streaming completions",
            self.name
        )))
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn supported_models(&self) -> Vec<String> {
        Vec::new()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_temperature: false,
            requires_streaming: false,
        }
    }
}

impl ModelCatalog {
    /// Create a new catalog with empty cache.
    pub fn new() -> Self {
        Self::with_timeout(Duration::from_secs(20))
    }

    /// Create a new catalog with a custom request timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            cache: HashMap::new(),
            client,
        }
    }

    /// Validate provider credentials by making a live models request.
    pub async fn verify_credentials(
        &self,
        provider: &str,
        api_key: &str,
        auth_mode: &str,
    ) -> Result<usize, String> {
        let provider = catalog_provider(provider, api_key)?;
        self.verify_provider_credentials(provider.as_ref(), api_key, auth_mode)
            .await
    }

    /// Fetch models for a provider. Uses cache if fresh, falls back on error.
    pub async fn get_models(
        &mut self,
        provider: &str,
        api_key: &str,
        auth_mode: &str,
    ) -> Vec<CatalogModel> {
        match catalog_provider(provider, api_key) {
            Ok(provider) => {
                self.get_provider_models(provider.as_ref(), api_key, auth_mode)
                    .await
            }
            Err(_) => self.cached_or_fallback_models(&UnknownCatalogProvider::new(provider)),
        }
    }

    /// Force refresh models for a provider.
    pub async fn refresh_models(
        &mut self,
        provider: &str,
        api_key: &str,
        auth_mode: &str,
    ) -> Vec<CatalogModel> {
        match catalog_provider(provider, api_key) {
            Ok(provider) => {
                self.refresh_provider_models(provider.as_ref(), api_key, auth_mode)
                    .await
            }
            Err(_) => self.cached_or_fallback_models(&UnknownCatalogProvider::new(provider)),
        }
    }

    async fn verify_provider_credentials(
        &self,
        provider: &dyn CompletionProvider,
        api_key: &str,
        auth_mode: &str,
    ) -> Result<usize, String> {
        let models = self
            .fetch_provider_models(provider, api_key, auth_mode)
            .await?;
        Ok(models.len())
    }

    async fn get_provider_models(
        &mut self,
        provider: &dyn CompletionProvider,
        api_key: &str,
        auth_mode: &str,
    ) -> Vec<CatalogModel> {
        if let Some(entry) = self.cache.get(&provider_key(provider)) {
            if Self::is_cache_fresh(entry) {
                return entry.models.clone();
            }
        }

        self.refresh_provider_models(provider, api_key, auth_mode)
            .await
    }

    async fn refresh_provider_models(
        &mut self,
        provider: &dyn CompletionProvider,
        api_key: &str,
        auth_mode: &str,
    ) -> Vec<CatalogModel> {
        let fetch_result = self
            .fetch_provider_models(provider, api_key, auth_mode)
            .await;
        self.apply_fetch_result(provider, fetch_result)
    }

    fn apply_fetch_result(
        &mut self,
        provider: &dyn CompletionProvider,
        fetch_result: Result<Vec<CatalogModel>, String>,
    ) -> Vec<CatalogModel> {
        match fetch_result {
            Ok(models) => {
                self.cache.insert(
                    provider_key(provider),
                    CacheEntry {
                        models: models.clone(),
                        fetched_at: Instant::now(),
                    },
                );
                models
            }
            Err(_) => self.cached_or_fallback_models(provider),
        }
    }

    fn cached_or_fallback_models(&self, provider: &dyn CompletionProvider) -> Vec<CatalogModel> {
        self.cache
            .get(&provider_key(provider))
            .map(|entry| entry.models.clone())
            .unwrap_or_else(|| Self::provider_fallback_models(provider))
    }

    fn provider_fallback_models(provider: &dyn CompletionProvider) -> Vec<CatalogModel> {
        let provider_key = provider_key(provider);
        provider
            .fallback_models()
            .into_iter()
            .map(|id| CatalogModel {
                id: id.to_string(),
                display_name: None,
                provider: provider_key.clone(),
            })
            .collect()
    }

    async fn fetch_provider_models(
        &self,
        provider: &dyn CompletionProvider,
        api_key: &str,
        auth_mode: &str,
    ) -> Result<Vec<CatalogModel>, String> {
        let request = self.build_models_request(provider, api_key, auth_mode)?;
        let response = self
            .client
            .execute(request)
            .await
            .map_err(|error| format!("request failed: {error}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| format!("failed to read body: {error}"))?;

        if !status.is_success() {
            return Err(format!("models endpoint returned {}", status.as_u16()));
        }

        Self::parse_models(provider, &body)
    }

    fn build_models_request(
        &self,
        provider: &dyn CompletionProvider,
        api_key: &str,
        auth_mode: &str,
    ) -> Result<reqwest::Request, String> {
        let endpoint = provider
            .models_endpoint()
            .ok_or_else(|| format!("unsupported provider '{}'", provider.name()))?;
        let headers = provider.catalog_auth_headers(api_key, auth_mode)?;

        self.client
            .get(endpoint)
            .headers(headers)
            .build()
            .map_err(|error| format!("failed to build request: {error}"))
    }

    fn parse_models(
        provider: &dyn CompletionProvider,
        json_body: &str,
    ) -> Result<Vec<CatalogModel>, String> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self::parse_models_with_now(provider, json_body, now_secs)
    }

    fn parse_models_with_now(
        provider: &dyn CompletionProvider,
        json_body: &str,
        now_secs: u64,
    ) -> Result<Vec<CatalogModel>, String> {
        let provider_key = provider_key(provider);
        let parsed = serde_json::from_str::<ModelsEnvelope>(json_body)
            .map_err(|error| format!("invalid models payload: {error}"))?;

        let mut seen = HashSet::new();
        let mut models = Vec::new();

        for model in parsed.data {
            let Some(id) = model.id.as_ref() else {
                continue;
            };

            if !provider.is_chat_capable(id) {
                continue;
            }

            if !quality_filters_allow(provider, &model, now_secs) {
                continue;
            }

            if !seen.insert(id.clone()) {
                continue;
            }

            models.push(CatalogModel {
                id: id.clone(),
                display_name: model.display_name.or(model.name),
                provider: provider_key.clone(),
            });
        }

        models.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(models)
    }

    fn is_cache_fresh(entry: &CacheEntry) -> bool {
        entry.fetched_at.elapsed() <= CACHE_TTL
    }
}

fn provider_key(provider: &dyn CompletionProvider) -> String {
    normalize_provider(provider.name())
}

fn quality_filters_allow(
    provider: &dyn CompletionProvider,
    model: &ModelEntry,
    now_secs: u64,
) -> bool {
    if !provider.catalog_filters().apply_recency_and_price_floor {
        return true;
    }
    is_model_recent_enough(model.created, now_secs) && is_model_capable_enough(&model.pricing)
}

fn metadata_credential(credential: &str) -> &str {
    if credential.trim().is_empty() {
        "placeholder-token"
    } else {
        credential
    }
}

/// Provider-name matching is intentional here: this factory chooses which
/// explicit provider contract type to instantiate for catalog operations.
fn catalog_provider(
    provider_name: &str,
    credential: &str,
) -> Result<Box<dyn CompletionProvider>, String> {
    let provider_name = normalize_provider(provider_name);
    let credential = metadata_credential(credential);
    match provider_name.as_str() {
        "anthropic" => AnthropicProvider::new(AnthropicProvider::default_base_url(), credential)
            .map(|provider| Box::new(provider) as Box<dyn CompletionProvider>)
            .map_err(|error| format!("failed to build provider metadata: {error}")),
        "openai" => OpenAiProvider::openai(OpenAiProvider::default_base_url(), credential)
            .map(|provider| Box::new(provider) as Box<dyn CompletionProvider>)
            .map_err(|error| format!("failed to build provider metadata: {error}")),
        "openrouter" => {
            OpenAiProvider::openrouter(OpenAiProvider::openrouter_base_url(), credential)
                .map(|provider| Box::new(provider) as Box<dyn CompletionProvider>)
                .map_err(|error| format!("failed to build provider metadata: {error}"))
        }
        _ => Ok(Box::new(UnknownCatalogProvider::new(&provider_name))),
    }
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self::new()
    }
}

fn is_model_recent_enough(created: Option<u64>, now_secs: u64) -> bool {
    let Some(created) = created else {
        return true; // No timestamp = allow (don't block on missing metadata)
    };
    let age_secs = now_secs.saturating_sub(created);
    age_secs <= MODEL_AGE_CUTOFF_SECS
}

fn is_model_capable_enough(pricing: &Option<ModelPricing>) -> bool {
    let Some(pricing) = pricing else {
        return true; // No pricing = allow (direct providers don't have pricing)
    };
    let Some(prompt_price) = pricing.prompt else {
        return true; // No prompt price = allow
    };
    prompt_price >= MIN_INPUT_PRICE_PER_TOKEN
}

fn normalize_provider(provider: &str) -> String {
    provider.trim().to_ascii_lowercase()
}

#[derive(Debug, Deserialize)]
struct ModelsEnvelope {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    pricing: Option<ModelPricing>,
}

#[derive(Debug, Deserialize)]
struct ModelPricing {
    #[serde(default, deserialize_with = "deserialize_price_value")]
    prompt: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PriceValue {
    String(String),
    Number(f64),
}

fn deserialize_price_value<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<PriceValue> = Option::deserialize(deserializer)?;
    Ok(value.and_then(parse_price_value))
}

fn parse_price_value(value: PriceValue) -> Option<f64> {
    match value {
        PriceValue::Number(num) => Some(num),
        PriceValue::String(raw) => match raw.parse::<f64>() {
            Ok(parsed) => Some(parsed),
            Err(error) => {
                eprintln!("warning: malformed model pricing '{}': {error}", raw);
                None
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{CompletionStream, LlmProvider as CompletionProvider};
    use crate::types::{CompletionRequest, CompletionResponse, LlmError};
    use async_trait::async_trait;
    use reqwest::header::AUTHORIZATION;

    fn make_model(id: &str, provider: &str) -> CatalogModel {
        CatalogModel {
            id: id.to_string(),
            display_name: None,
            provider: provider.to_string(),
        }
    }

    fn test_provider(name: &str) -> Box<dyn CompletionProvider> {
        catalog_provider(name, "test-key").expect("provider")
    }

    fn parse_models(provider: &dyn CompletionProvider, json_body: &str) -> Vec<CatalogModel> {
        ModelCatalog::parse_models(provider, json_body).expect("parse models")
    }

    fn parse_models_with_now(
        provider: &dyn CompletionProvider,
        json_body: &str,
        now_secs: u64,
    ) -> Vec<CatalogModel> {
        ModelCatalog::parse_models_with_now(provider, json_body, now_secs).expect("parse models")
    }

    #[derive(Debug)]
    struct CustomCatalogProvider;

    #[async_trait]
    impl CompletionProvider for CustomCatalogProvider {
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
            "custom"
        }

        fn supported_models(&self) -> Vec<String> {
            Vec::new()
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }

        fn models_endpoint(&self) -> Option<&str> {
            Some("https://catalog.example.test/v1/models")
        }

        fn catalog_auth_headers(
            &self,
            api_key: &str,
            auth_mode: &str,
        ) -> Result<reqwest::header::HeaderMap, String> {
            let mut headers = reqwest::header::HeaderMap::new();
            let token = reqwest::header::HeaderValue::from_str(api_key)
                .map_err(|error| format!("invalid catalog token header: {error}"))?;
            let mode = reqwest::header::HeaderValue::from_str(auth_mode)
                .map_err(|error| format!("invalid auth mode header: {error}"))?;
            headers.insert("x-catalog-token", token);
            headers.insert("x-auth-mode", mode);
            Ok(headers)
        }

        fn is_chat_capable(&self, model_id: &str) -> bool {
            model_id.starts_with("assistant-")
        }

        fn fallback_models(&self) -> Vec<&'static str> {
            vec!["assistant-fallback"]
        }
    }

    #[test]
    fn parse_models_supports_anthropic_payload_shape() {
        let provider = test_provider("anthropic");
        let json = r#"{
            "data": [
                {"id": "claude-sonnet-4-20250514", "display_name": "Claude Sonnet 4"},
                {"id": "claude-opus-4-20250514", "name": "Claude Opus 4"},
                {"id": "not-chat-model"}
            ]
        }"#;

        let parsed = parse_models(provider.as_ref(), json);

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "claude-opus-4-20250514");
        assert_eq!(parsed[0].display_name.as_deref(), Some("Claude Opus 4"));
        assert_eq!(parsed[1].id, "claude-sonnet-4-20250514");
        assert_eq!(parsed[1].display_name.as_deref(), Some("Claude Sonnet 4"));
    }

    #[test]
    fn parse_models_supports_openai_payload_shape() {
        let provider = test_provider("openai");
        let json = r#"{
            "data": [
                {"id": "gpt-4o", "display_name": "GPT-4o"},
                {"id": "gpt-4o-mini", "display_name": "GPT-4o mini"},
                {"id": "text-embedding-3-large", "display_name": "Embedding"}
            ]
        }"#;

        let parsed = parse_models(provider.as_ref(), json);

        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|model| model.id.starts_with("gpt-4o")));
    }

    #[test]
    fn parse_models_supports_openrouter_payload_shape() {
        let provider = test_provider("openrouter");
        let json = r#"{
            "data": [
                {"id": "anthropic/claude-sonnet-4"},
                {"id": "x-ai/grok-3"},
                {"id": "openai/text-embedding-3-large"}
            ]
        }"#;

        let parsed = parse_models(provider.as_ref(), json);

        assert_eq!(parsed.len(), 2);
        assert!(parsed
            .iter()
            .any(|model| model.id == "anthropic/claude-sonnet-4"));
        assert!(parsed.iter().any(|model| model.id == "x-ai/grok-3"));
    }

    #[test]
    fn is_chat_model_accepts_openrouter_xai() {
        let provider = test_provider("openrouter");
        assert!(provider.is_chat_capable("x-ai/grok-3"));
        assert!(provider.is_chat_capable("x-ai/grok-3-mini"));
    }

    #[test]
    fn is_chat_model_accepts_openrouter_qwen() {
        let provider = test_provider("openrouter");
        assert!(provider.is_chat_capable("qwen/qwen-2.5-72b-instruct"));
    }

    #[test]
    fn is_chat_model_accepts_openrouter_deepseek() {
        let provider = test_provider("openrouter");
        assert!(provider.is_chat_capable("deepseek/deepseek-chat-v3"));
    }

    #[test]
    fn is_chat_model_accepts_openrouter_o4() {
        let provider = test_provider("openrouter");
        assert!(provider.is_chat_capable("openai/o4-mini"));
    }

    #[test]
    fn is_chat_capable_filters_each_provider() {
        let anthropic = test_provider("anthropic");
        let openai = test_provider("openai");
        let openrouter = test_provider("openrouter");

        assert!(anthropic.is_chat_capable("claude-sonnet-4"));
        assert!(!anthropic.is_chat_capable("text-embedding-3-large"));

        assert!(openai.is_chat_capable("gpt-4o"));
        assert!(openai.is_chat_capable("o3-mini-high"));
        assert!(!openai.is_chat_capable("text-embedding-3-large"));
        assert!(!openai.is_chat_capable("gpt-4o-realtime-preview"));
        assert!(!openai.is_chat_capable("gpt-4o-audio-preview"));

        assert!(openrouter.is_chat_capable("x-ai/grok-3"));
        assert!(!openrouter.is_chat_capable("openai/text-embedding-3-large"));
    }

    #[test]
    fn custom_provider_metadata_drives_request_building() {
        let catalog = ModelCatalog::new();
        let provider = CustomCatalogProvider;

        let request = catalog
            .build_models_request(&provider, "secret-token", "custom-mode")
            .expect("request");

        assert_eq!(
            request.url().as_str(),
            "https://catalog.example.test/v1/models"
        );
        assert_eq!(
            request.headers().get("x-catalog-token").unwrap(),
            "secret-token"
        );
        assert_eq!(request.headers().get("x-auth-mode").unwrap(), "custom-mode");
    }

    #[test]
    fn custom_provider_metadata_drives_parsing_and_fallback() {
        let provider = CustomCatalogProvider;
        let parsed = parse_models(
            &provider,
            r#"{"data":[{"id":"assistant-pro"},{"id":"embeddings-v1"}]}"#,
        );

        assert_eq!(parsed, vec![make_model("assistant-pro", "custom")]);
        assert_eq!(
            ModelCatalog::provider_fallback_models(&provider),
            vec![make_model("assistant-fallback", "custom")]
        );
    }

    #[tokio::test]
    async fn cache_ttl_uses_fresh_cache_and_falls_back_to_expired_cache_on_refresh_error() {
        let mut catalog = ModelCatalog::new();
        let fresh_models = vec![make_model("gpt-4o", "custom")];

        catalog.cache.insert(
            "custom".to_string(),
            CacheEntry {
                models: fresh_models.clone(),
                fetched_at: Instant::now(),
            },
        );

        let from_fresh_cache = catalog.get_models("custom", "ignored", "bearer").await;
        assert_eq!(from_fresh_cache, fresh_models);

        let expired_models = vec![make_model("gpt-4o-mini", "custom")];
        catalog.cache.insert(
            "custom".to_string(),
            CacheEntry {
                models: expired_models.clone(),
                fetched_at: Instant::now() - CACHE_TTL - Duration::from_secs(1),
            },
        );

        // Unsupported provider => refresh fails immediately, so expired cache should be returned.
        let from_expired_cache = catalog.get_models("custom", "ignored", "bearer").await;
        assert_eq!(from_expired_cache, expired_models);
    }

    #[test]
    fn fallback_defaults_match_expected_lists() {
        let anthropic = ModelCatalog::provider_fallback_models(test_provider("anthropic").as_ref())
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();
        assert_eq!(
            anthropic,
            vec![
                "claude-opus-4-6-20250929".to_string(),
                "claude-opus-4-6".to_string(),
                "claude-sonnet-4-6-20250929".to_string(),
                "claude-sonnet-4-6".to_string(),
                "claude-opus-4-5-20251101".to_string(),
                "claude-sonnet-4-5-20250929".to_string(),
                "claude-haiku-4-5-20251001".to_string(),
                "claude-opus-4-20250514".to_string(),
                "claude-sonnet-4-20250514".to_string(),
            ]
        );

        let openai = ModelCatalog::provider_fallback_models(test_provider("openai").as_ref())
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();
        assert_eq!(
            openai,
            vec![
                "gpt-5.4".to_string(),
                "gpt-4.1".to_string(),
                "o3".to_string(),
                "o4-mini".to_string(),
                "gpt-4o".to_string(),
                "gpt-4o-mini".to_string(),
            ]
        );

        let openrouter =
            ModelCatalog::provider_fallback_models(test_provider("openrouter").as_ref())
                .into_iter()
                .map(|model| model.id)
                .collect::<Vec<_>>();
        assert_eq!(
            openrouter,
            vec![
                "anthropic/claude-sonnet-4".to_string(),
                "openai/gpt-4o".to_string(),
                "x-ai/grok-3".to_string(),
                "qwen/qwen-2.5-72b-instruct".to_string(),
                "deepseek/deepseek-chat-v3".to_string(),
            ]
        );
    }

    #[test]
    fn provider_fallback_openrouter_includes_new_providers() {
        let provider = test_provider("openrouter");
        let fallback = ModelCatalog::provider_fallback_models(provider.as_ref());
        let ids: Vec<&str> = fallback.iter().map(|model| model.id.as_str()).collect();

        assert!(ids.iter().any(|id| id.contains("grok")));
        assert!(ids.iter().any(|id| id.contains("qwen")));
        assert!(ids.iter().any(|id| id.contains("deepseek")));
    }

    #[test]
    fn auth_headers_match_expected_modes() {
        let catalog = ModelCatalog::new();
        let anthropic = test_provider("anthropic");
        let openai = test_provider("openai");

        let anthropic_api_key = catalog
            .build_models_request(anthropic.as_ref(), "anthropic-key", "api_key")
            .unwrap();
        let headers = anthropic_api_key.headers();
        assert_eq!(headers.get("x-api-key").unwrap(), "anthropic-key");
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
        assert!(headers.get(AUTHORIZATION).is_none());

        let anthropic_setup = catalog
            .build_models_request(anthropic.as_ref(), "setup-token", "setup_token")
            .unwrap();
        let headers = anthropic_setup.headers();
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer setup-token");
        assert_eq!(
            headers.get("anthropic-beta").unwrap(),
            "claude-code-20250219,oauth-2025-04-20"
        );
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");

        let openai_bearer = catalog
            .build_models_request(openai.as_ref(), "openai-key", "bearer")
            .unwrap();
        let headers = openai_bearer.headers();
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer openai-key");
        assert!(headers.get("anthropic-version").is_none());
    }

    #[test]
    fn apply_fetch_result_updates_cache_on_successful_fetch() {
        let mut catalog = ModelCatalog::new();
        let provider = test_provider("openai");
        let expected = vec![make_model("gpt-4o", "openai")];

        let models = catalog.apply_fetch_result(provider.as_ref(), Ok(expected.clone()));

        assert_eq!(models, expected);
        let cached = catalog.cache.get("openai").expect("cache entry");
        assert_eq!(cached.models, expected);
    }

    #[test]
    fn apply_fetch_result_uses_cached_models_when_fetch_fails() {
        let mut catalog = ModelCatalog::new();
        let provider = test_provider("openai");
        let cached_models = vec![make_model("gpt-4o-mini", "openai")];
        catalog.cache.insert(
            "openai".to_string(),
            CacheEntry {
                models: cached_models.clone(),
                fetched_at: Instant::now(),
            },
        );

        let models = catalog.apply_fetch_result(
            provider.as_ref(),
            Err("simulated network failure".to_string()),
        );

        assert_eq!(models, cached_models);
    }

    #[test]
    fn apply_fetch_result_returns_empty_when_fetch_succeeds_with_empty_payload() {
        let mut catalog = ModelCatalog::new();
        let provider = test_provider("openai");

        let models = catalog.apply_fetch_result(provider.as_ref(), Ok(Vec::new()));

        assert!(models.is_empty());
        let cached = catalog.cache.get("openai").expect("cache entry");
        assert!(cached.models.is_empty());
    }

    #[test]
    fn is_model_recent_enough_allows_recent_models() {
        let now_secs = MODEL_AGE_CUTOFF_SECS + 1_000;
        let recent = now_secs - (30 * 24 * 60 * 60);
        assert!(is_model_recent_enough(Some(recent), now_secs));
    }

    #[test]
    fn is_model_recent_enough_filters_old_models() {
        let now_secs = (MODEL_AGE_CUTOFF_SECS + (2 * 24 * 60 * 60)) + 1_000;
        let old = now_secs - (181 * 24 * 60 * 60);
        assert!(!is_model_recent_enough(Some(old), now_secs));
    }

    #[test]
    fn is_model_recent_enough_allows_missing_timestamp() {
        assert!(is_model_recent_enough(None, 123));
    }

    #[test]
    fn is_model_recent_enough_allows_age_exactly_at_cutoff() {
        let now_secs = MODEL_AGE_CUTOFF_SECS + 42;
        let at_cutoff = now_secs - MODEL_AGE_CUTOFF_SECS;
        assert!(is_model_recent_enough(Some(at_cutoff), now_secs));
    }

    #[test]
    fn parse_models_openrouter_enforces_180_day_age_boundary() {
        let provider = test_provider("openrouter");
        let now_secs = 1_900_000_000_u64;
        let within_cutoff = now_secs - (179 * 24 * 60 * 60);
        let beyond_cutoff = now_secs - (181 * 24 * 60 * 60);
        let json = format!(
            r#"{{
                "data": [
                    {{
                        "id": "anthropic/claude-sonnet-within-cutoff",
                        "created": {within_cutoff},
                        "pricing": {{"prompt": "0.000006"}}
                    }},
                    {{
                        "id": "anthropic/claude-sonnet-beyond-cutoff",
                        "created": {beyond_cutoff},
                        "pricing": {{"prompt": "0.000006"}}
                    }}
                ]
            }}"#
        );

        let parsed = parse_models_with_now(provider.as_ref(), &json, now_secs);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "anthropic/claude-sonnet-within-cutoff");
    }

    #[test]
    fn is_model_recent_enough_allows_future_timestamp() {
        let now_secs = 1_000;
        let future = now_secs + 500;
        assert!(is_model_recent_enough(Some(future), now_secs));
    }

    #[test]
    fn is_model_capable_enough_allows_sonnet_tier() {
        let pricing = ModelPricing {
            prompt: Some(0.000006), // $6/M — sonnet tier
        };
        assert!(is_model_capable_enough(&Some(pricing)));
    }

    #[test]
    fn is_model_capable_enough_filters_cheap_models() {
        let pricing = ModelPricing {
            prompt: Some(0.0000005), // $0.50/M — below threshold
        };
        assert!(!is_model_capable_enough(&Some(pricing)));
    }

    #[test]
    fn is_model_capable_enough_allows_price_exactly_at_threshold() {
        let pricing = ModelPricing {
            prompt: Some(MIN_INPUT_PRICE_PER_TOKEN),
        };
        assert!(is_model_capable_enough(&Some(pricing)));
    }

    #[test]
    fn is_model_capable_enough_allows_missing_pricing() {
        assert!(is_model_capable_enough(&None));
    }

    #[test]
    fn is_model_capable_enough_allows_missing_prompt_price() {
        let pricing = ModelPricing { prompt: None };
        assert!(is_model_capable_enough(&Some(pricing)));
    }

    #[derive(Debug, Deserialize)]
    struct PricingEnvelope {
        pricing: ModelPricing,
    }

    #[test]
    fn deserialize_price_value_supports_string_numeric_and_malformed_inputs() {
        let from_string: PricingEnvelope =
            serde_json::from_str(r#"{"pricing":{"prompt":"0.000006"}}"#).unwrap();
        assert_eq!(from_string.pricing.prompt, Some(0.000006));

        let from_number: PricingEnvelope =
            serde_json::from_str(r#"{"pricing":{"prompt":0.000007}}"#).unwrap();
        assert_eq!(from_number.pricing.prompt, Some(0.000007));

        let malformed: PricingEnvelope =
            serde_json::from_str(r#"{"pricing":{"prompt":"not-a-number"}}"#).unwrap();
        assert_eq!(malformed.pricing.prompt, None);
    }

    #[test]
    fn parse_models_openrouter_filters_old_and_cheap_models() {
        let provider = test_provider("openrouter");
        let now_secs = 1_900_000_000_u64;
        let recent = now_secs - (30 * 24 * 60 * 60);
        let old = now_secs - (181 * 24 * 60 * 60);
        let json = format!(
            r#"{{
                "data": [
                    {{
                        "id": "anthropic/claude-sonnet-4",
                        "created": {recent},
                        "pricing": {{"prompt": "0.000006"}}
                    }},
                    {{
                        "id": "anthropic/claude-3.5-sonnet",
                        "created": {old},
                        "pricing": {{"prompt": "0.000006"}}
                    }},
                    {{
                        "id": "some-provider/cheap-model",
                        "created": {recent},
                        "pricing": {{"prompt": "0.0000001"}}
                    }}
                ]
            }}"#
        );

        let parsed = parse_models_with_now(provider.as_ref(), &json, now_secs);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "anthropic/claude-sonnet-4");
    }

    #[test]
    fn parse_models_openrouter_allows_model_with_malformed_price() {
        let provider = test_provider("openrouter");
        let now_secs = 1_900_000_000_u64;
        let created_recent = now_secs - (30 * 24 * 60 * 60);
        let json = format!(
            r#"{{
                "data": [
                    {{
                        "id": "anthropic/claude-sonnet-4",
                        "created": {created_recent},
                        "pricing": {{"prompt": "bad-number"}}
                    }}
                ]
            }}"#
        );

        let parsed = parse_models_with_now(provider.as_ref(), &json, now_secs);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "anthropic/claude-sonnet-4");
    }

    #[test]
    fn build_models_request_accepts_oauth_for_openai() {
        let catalog = ModelCatalog::new();
        let provider = test_provider("openai");

        let request = catalog
            .build_models_request(provider.as_ref(), "oauth-token-123", "oauth")
            .expect("oauth auth mode should be accepted for openai");

        let headers = request.headers();
        assert_eq!(
            headers.get(AUTHORIZATION).unwrap(),
            "Bearer oauth-token-123"
        );
    }

    #[test]
    fn provider_fallback_includes_modern_models() {
        let provider = test_provider("openai");
        let fallback = ModelCatalog::provider_fallback_models(provider.as_ref());
        let ids: Vec<&str> = fallback.iter().map(|model| model.id.as_str()).collect();

        assert!(ids.contains(&"gpt-5.4"), "fallback should include gpt-5.4");
        assert!(ids.contains(&"o3"), "fallback should include o3");
        assert!(ids.contains(&"o4-mini"), "fallback should include o4-mini");
    }

    #[test]
    fn parse_models_anthropic_ignores_age_and_pricing_filters() {
        let provider = test_provider("anthropic");
        // Anthropic direct API models should not be filtered by age/pricing
        let json = r#"{
            "data": [
                {
                    "id": "claude-sonnet-4-20250514",
                    "display_name": "Claude Sonnet 4"
                }
            ]
        }"#;

        let parsed = parse_models(provider.as_ref(), json);
        assert_eq!(parsed.len(), 1);
    }
}
