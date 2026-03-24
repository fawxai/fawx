//! Dynamic model discovery with provider-aware filtering and cache fallback.

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Maximum model age in seconds (~180 days). Models older than this are filtered out.
const MODEL_AGE_CUTOFF_SECS: u64 = 180 * 24 * 60 * 60;
/// Minimum input price per token (USD) to filter out weak-tier models.
/// $3/M tokens = 0.000003 per token. Roughly sonnet-tier floor.
const MIN_INPUT_PRICE_PER_TOKEN: f64 = 0.000003;
const ANTHROPIC_MODELS_ENDPOINT: &str = "https://api.anthropic.com/v1/models";
const OPENAI_MODELS_ENDPOINT: &str = "https://api.openai.com/v1/models";
const OPENROUTER_MODELS_ENDPOINT: &str = "https://openrouter.ai/api/v1/models";

const ANTHROPIC_SETUP_TOKEN_BETA: &str = "claude-code-20250219,oauth-2025-04-20";

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
        let provider_key = normalize_provider(provider);
        let models = self.fetch_models(&provider_key, api_key, auth_mode).await?;
        Ok(models.len())
    }

    /// Fetch models for a provider. Uses cache if fresh, falls back on error.
    pub async fn get_models(
        &mut self,
        provider: &str,
        api_key: &str,
        auth_mode: &str,
    ) -> Vec<CatalogModel> {
        let provider_key = normalize_provider(provider);

        if let Some(entry) = self.cache.get(&provider_key) {
            if Self::is_cache_fresh(entry) {
                return entry.models.clone();
            }
        }

        self.refresh_models(&provider_key, api_key, auth_mode).await
    }

    /// Force refresh models for a provider.
    pub async fn refresh_models(
        &mut self,
        provider: &str,
        api_key: &str,
        auth_mode: &str,
    ) -> Vec<CatalogModel> {
        let provider_key = normalize_provider(provider);
        let fetch_result = self.fetch_models(&provider_key, api_key, auth_mode).await;
        self.apply_fetch_result(&provider_key, fetch_result)
    }

    fn apply_fetch_result(
        &mut self,
        provider_key: &str,
        fetch_result: Result<Vec<CatalogModel>, String>,
    ) -> Vec<CatalogModel> {
        match fetch_result {
            Ok(models) => {
                self.cache.insert(
                    provider_key.to_string(),
                    CacheEntry {
                        models: models.clone(),
                        fetched_at: Instant::now(),
                    },
                );
                models
            }
            Err(_) => self.cached_or_fallback_models(provider_key),
        }
    }

    fn cached_or_fallback_models(&self, provider_key: &str) -> Vec<CatalogModel> {
        self.cache
            .get(provider_key)
            .map(|entry| entry.models.clone())
            .unwrap_or_else(|| Self::hardcoded_fallback(provider_key))
    }

    async fn fetch_models(
        &self,
        provider: &str,
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
        provider: &str,
        api_key: &str,
        auth_mode: &str,
    ) -> Result<reqwest::Request, String> {
        let provider = normalize_provider(provider);
        let endpoint = models_endpoint(&provider)?;

        let mut headers = HeaderMap::new();

        match provider.as_str() {
            "anthropic" => {
                headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

                match auth_mode {
                    "api_key" => {
                        let key = HeaderValue::from_str(api_key)
                            .map_err(|error| format!("invalid api key header: {error}"))?;
                        headers.insert("x-api-key", key);
                    }
                    "setup_token" => {
                        let bearer = format!("Bearer {api_key}");
                        let bearer = HeaderValue::from_str(&bearer)
                            .map_err(|error| format!("invalid authorization header: {error}"))?;
                        headers.insert(AUTHORIZATION, bearer);
                        headers.insert(
                            "anthropic-beta",
                            HeaderValue::from_static(ANTHROPIC_SETUP_TOKEN_BETA),
                        );
                    }
                    other => {
                        return Err(format!(
                            "unsupported auth mode '{other}' for provider '{provider}'"
                        ));
                    }
                }
            }
            "openai" | "openrouter" => match auth_mode {
                "bearer" | "oauth" => {
                    let bearer = format!("Bearer {api_key}");
                    let bearer = HeaderValue::from_str(&bearer)
                        .map_err(|error| format!("invalid authorization header: {error}"))?;
                    headers.insert(AUTHORIZATION, bearer);
                }
                other => {
                    return Err(format!(
                        "unsupported auth mode '{other}' for provider '{provider}'"
                    ));
                }
            },
            _ => {
                return Err(format!("unsupported provider '{provider}'"));
            }
        }

        self.client
            .get(endpoint)
            .headers(headers)
            .build()
            .map_err(|error| format!("failed to build request: {error}"))
    }

    fn parse_models(provider: &str, json_body: &str) -> Result<Vec<CatalogModel>, String> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self::parse_models_with_now(provider, json_body, now_secs)
    }

    fn parse_models_with_now(
        provider: &str,
        json_body: &str,
        now_secs: u64,
    ) -> Result<Vec<CatalogModel>, String> {
        let provider = normalize_provider(provider);
        let parsed = serde_json::from_str::<ModelsEnvelope>(json_body)
            .map_err(|error| format!("invalid models payload: {error}"))?;

        let mut seen = HashSet::new();
        let mut models = Vec::new();

        for model in parsed.data {
            let Some(id) = model.id else {
                continue;
            };

            if !Self::is_chat_capable(provider.as_str(), &id) {
                continue;
            }

            if provider == "openrouter" {
                if !is_model_recent_enough(model.created, now_secs) {
                    continue;
                }
                if !is_model_capable_enough(&model.pricing) {
                    continue;
                }
            }

            if !seen.insert(id.clone()) {
                continue;
            }

            models.push(CatalogModel {
                id,
                display_name: model.display_name.or(model.name),
                provider: provider.clone(),
            });
        }

        models.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(models)
    }

    fn is_chat_capable(provider: &str, model_id: &str) -> bool {
        let id = model_id.to_ascii_lowercase();
        match provider {
            "anthropic" => id.starts_with("claude-"),
            "openai" => {
                let includes = id.starts_with("gpt-")
                    || id.starts_with("gpt-5")
                    || id.starts_with("o1")
                    || id.starts_with("o3")
                    || id.starts_with("o4");

                let excludes = id.contains("embedding")
                    || id.contains("tts")
                    || id.contains("whisper")
                    || id.contains("dall-e")
                    || id.contains("moderation")
                    || id.contains("audio")
                    || id.contains("realtime")
                    || id.contains("search")
                    || id.contains("instruct");

                includes && !excludes
            }
            "openrouter" => {
                id.contains("claude")
                    || id.contains("gpt-")
                    || id.contains("o4")
                    || id.contains("grok")
                    || id.contains("qwen")
                    || id.contains("minimax")
                    || id.contains("liquidai")
                    || id.contains("lfm")
                    || id.contains("deepseek")
            }
            _ => false,
        }
    }

    fn is_cache_fresh(entry: &CacheEntry) -> bool {
        entry.fetched_at.elapsed() <= CACHE_TTL
    }

    fn hardcoded_fallback(provider: &str) -> Vec<CatalogModel> {
        let provider = normalize_provider(provider);
        let ids: Vec<&str> = match provider.as_str() {
            "anthropic" => vec![
                "claude-opus-4-6-20250929",
                "claude-opus-4-6",
                "claude-sonnet-4-6-20250929",
                "claude-sonnet-4-6",
                "claude-opus-4-5-20251101",
                "claude-sonnet-4-5-20250929",
                "claude-haiku-4-5-20251001",
                "claude-opus-4-20250514",
                "claude-sonnet-4-20250514",
            ],
            "openai" => vec![
                "gpt-5.4",
                "gpt-4.1",
                "o3",
                "o4-mini",
                "gpt-4o",
                "gpt-4o-mini",
            ],
            "openrouter" => vec![
                "anthropic/claude-sonnet-4",
                "openai/gpt-4o",
                "x-ai/grok-3",
                "qwen/qwen-2.5-72b-instruct",
                "deepseek/deepseek-chat-v3",
            ],
            _ => vec!["gpt-4o-mini"],
        };

        ids.into_iter()
            .map(|id| CatalogModel {
                id: id.to_string(),
                display_name: None,
                provider: provider.clone(),
            })
            .collect()
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

fn models_endpoint(provider: &str) -> Result<&'static str, String> {
    match provider {
        "anthropic" => Ok(ANTHROPIC_MODELS_ENDPOINT),
        "openai" => Ok(OPENAI_MODELS_ENDPOINT),
        "openrouter" => Ok(OPENROUTER_MODELS_ENDPOINT),
        _ => Err(format!("unsupported provider '{provider}'")),
    }
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

    fn make_model(id: &str, provider: &str) -> CatalogModel {
        CatalogModel {
            id: id.to_string(),
            display_name: None,
            provider: provider.to_string(),
        }
    }

    #[test]
    fn parse_models_supports_anthropic_payload_shape() {
        let json = r#"{
            "data": [
                {"id": "claude-sonnet-4-20250514", "display_name": "Claude Sonnet 4"},
                {"id": "claude-opus-4-20250514", "name": "Claude Opus 4"},
                {"id": "not-chat-model"}
            ]
        }"#;

        let parsed = ModelCatalog::parse_models("anthropic", json).unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "claude-opus-4-20250514");
        assert_eq!(parsed[0].display_name.as_deref(), Some("Claude Opus 4"));
        assert_eq!(parsed[1].id, "claude-sonnet-4-20250514");
        assert_eq!(parsed[1].display_name.as_deref(), Some("Claude Sonnet 4"));
    }

    #[test]
    fn parse_models_supports_openai_payload_shape() {
        let json = r#"{
            "data": [
                {"id": "gpt-4o", "display_name": "GPT-4o"},
                {"id": "gpt-4o-mini", "display_name": "GPT-4o mini"},
                {"id": "text-embedding-3-large", "display_name": "Embedding"}
            ]
        }"#;

        let parsed = ModelCatalog::parse_models("openai", json).unwrap();

        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|model| model.id.starts_with("gpt-4o")));
    }

    #[test]
    fn parse_models_supports_openrouter_payload_shape() {
        let json = r#"{
            "data": [
                {"id": "anthropic/claude-sonnet-4"},
                {"id": "x-ai/grok-3"},
                {"id": "openai/text-embedding-3-large"}
            ]
        }"#;

        let parsed = ModelCatalog::parse_models("openrouter", json).unwrap();

        assert_eq!(parsed.len(), 2);
        assert!(parsed
            .iter()
            .any(|model| model.id == "anthropic/claude-sonnet-4"));
        assert!(parsed.iter().any(|model| model.id == "x-ai/grok-3"));
    }

    #[test]
    fn is_chat_model_accepts_openrouter_xai() {
        assert!(ModelCatalog::is_chat_capable("openrouter", "x-ai/grok-3"));
        assert!(ModelCatalog::is_chat_capable(
            "openrouter",
            "x-ai/grok-3-mini"
        ));
    }

    #[test]
    fn is_chat_model_accepts_openrouter_qwen() {
        assert!(ModelCatalog::is_chat_capable(
            "openrouter",
            "qwen/qwen-2.5-72b-instruct"
        ));
    }

    #[test]
    fn is_chat_model_accepts_openrouter_deepseek() {
        assert!(ModelCatalog::is_chat_capable(
            "openrouter",
            "deepseek/deepseek-chat-v3"
        ));
    }

    #[test]
    fn is_chat_model_accepts_openrouter_o4() {
        assert!(ModelCatalog::is_chat_capable(
            "openrouter",
            "openai/o4-mini"
        ));
    }

    #[test]
    fn is_chat_capable_filters_each_provider() {
        assert!(ModelCatalog::is_chat_capable(
            "anthropic",
            "claude-sonnet-4"
        ));
        assert!(!ModelCatalog::is_chat_capable(
            "anthropic",
            "text-embedding-3-large"
        ));

        assert!(ModelCatalog::is_chat_capable("openai", "gpt-4o"));
        assert!(ModelCatalog::is_chat_capable("openai", "o3-mini-high"));
        assert!(!ModelCatalog::is_chat_capable(
            "openai",
            "text-embedding-3-large"
        ));
        assert!(!ModelCatalog::is_chat_capable(
            "openai",
            "gpt-4o-realtime-preview"
        ));
        assert!(!ModelCatalog::is_chat_capable(
            "openai",
            "gpt-4o-audio-preview"
        ));

        assert!(ModelCatalog::is_chat_capable("openrouter", "x-ai/grok-3"));
        assert!(!ModelCatalog::is_chat_capable(
            "openrouter",
            "openai/text-embedding-3-large"
        ));
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
        let anthropic = ModelCatalog::hardcoded_fallback("anthropic")
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

        let openai = ModelCatalog::hardcoded_fallback("openai")
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

        let openrouter = ModelCatalog::hardcoded_fallback("openrouter")
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
    fn hardcoded_fallback_openrouter_includes_new_providers() {
        let fallback = ModelCatalog::hardcoded_fallback("openrouter");
        let ids: Vec<&str> = fallback.iter().map(|model| model.id.as_str()).collect();

        assert!(ids.iter().any(|id| id.contains("grok")));
        assert!(ids.iter().any(|id| id.contains("qwen")));
        assert!(ids.iter().any(|id| id.contains("deepseek")));
    }

    #[test]
    fn auth_headers_match_expected_modes() {
        let catalog = ModelCatalog::new();

        let anthropic_api_key = catalog
            .build_models_request("anthropic", "anthropic-key", "api_key")
            .unwrap();
        let headers = anthropic_api_key.headers();
        assert_eq!(headers.get("x-api-key").unwrap(), "anthropic-key");
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
        assert!(headers.get(AUTHORIZATION).is_none());

        let anthropic_setup = catalog
            .build_models_request("anthropic", "setup-token", "setup_token")
            .unwrap();
        let headers = anthropic_setup.headers();
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer setup-token");
        assert_eq!(
            headers.get("anthropic-beta").unwrap(),
            ANTHROPIC_SETUP_TOKEN_BETA
        );
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");

        let openai_bearer = catalog
            .build_models_request("openai", "openai-key", "bearer")
            .unwrap();
        let headers = openai_bearer.headers();
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer openai-key");
        assert!(headers.get("anthropic-version").is_none());
    }

    #[test]
    fn apply_fetch_result_updates_cache_on_successful_fetch() {
        let mut catalog = ModelCatalog::new();
        let expected = vec![make_model("gpt-4o", "openai")];

        let models = catalog.apply_fetch_result("openai", Ok(expected.clone()));

        assert_eq!(models, expected);
        let cached = catalog.cache.get("openai").expect("cache entry");
        assert_eq!(cached.models, expected);
    }

    #[test]
    fn apply_fetch_result_uses_cached_models_when_fetch_fails() {
        let mut catalog = ModelCatalog::new();
        let cached_models = vec![make_model("gpt-4o-mini", "openai")];
        catalog.cache.insert(
            "openai".to_string(),
            CacheEntry {
                models: cached_models.clone(),
                fetched_at: Instant::now(),
            },
        );

        let models =
            catalog.apply_fetch_result("openai", Err("simulated network failure".to_string()));

        assert_eq!(models, cached_models);
    }

    #[test]
    fn apply_fetch_result_returns_empty_when_fetch_succeeds_with_empty_payload() {
        let mut catalog = ModelCatalog::new();

        let models = catalog.apply_fetch_result("openai", Ok(Vec::new()));

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

        let parsed = ModelCatalog::parse_models_with_now("openrouter", &json, now_secs).unwrap();

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

        let parsed = ModelCatalog::parse_models_with_now("openrouter", &json, now_secs).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "anthropic/claude-sonnet-4");
    }

    #[test]
    fn parse_models_openrouter_allows_model_with_malformed_price() {
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

        let parsed = ModelCatalog::parse_models_with_now("openrouter", &json, now_secs).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "anthropic/claude-sonnet-4");
    }

    #[test]
    fn build_models_request_accepts_oauth_for_openai() {
        let catalog = ModelCatalog::new();

        let request = catalog
            .build_models_request("openai", "oauth-token-123", "oauth")
            .expect("oauth auth mode should be accepted for openai");

        let headers = request.headers();
        assert_eq!(
            headers.get(AUTHORIZATION).unwrap(),
            "Bearer oauth-token-123"
        );
    }

    #[test]
    fn hardcoded_fallback_includes_modern_models() {
        let fallback = ModelCatalog::hardcoded_fallback("openai");
        let ids: Vec<&str> = fallback.iter().map(|model| model.id.as_str()).collect();

        assert!(ids.contains(&"gpt-5.4"), "fallback should include gpt-5.4");
        assert!(ids.contains(&"o3"), "fallback should include o3");
        assert!(ids.contains(&"o4-mini"), "fallback should include o4-mini");
    }

    #[test]
    fn parse_models_anthropic_ignores_age_and_pricing_filters() {
        // Anthropic direct API models should not be filtered by age/pricing
        let json = r#"{
            "data": [
                {
                    "id": "claude-sonnet-4-20250514",
                    "display_name": "Claude Sonnet 4"
                }
            ]
        }"#;

        let parsed = ModelCatalog::parse_models("anthropic", json).unwrap();
        assert_eq!(parsed.len(), 1);
    }
}
