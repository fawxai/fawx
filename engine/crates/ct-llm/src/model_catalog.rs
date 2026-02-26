//! Dynamic model discovery with provider-aware filtering and cache fallback.

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            cache: HashMap::new(),
            client,
        }
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

        match self.fetch_models(&provider_key, api_key, auth_mode).await {
            Ok(models) => {
                self.cache.insert(
                    provider_key,
                    CacheEntry {
                        models: models.clone(),
                        fetched_at: Instant::now(),
                    },
                );
                models
            }
            Err(_) => self
                .cache
                .get(&provider_key)
                .map(|entry| entry.models.clone())
                .unwrap_or_else(|| Self::hardcoded_fallback(&provider_key)),
        }
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
                headers.insert(
                    "anthropic-version",
                    HeaderValue::from_static("2023-06-01"),
                );

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
                "bearer" => {
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
                let includes =
                    id.starts_with("gpt-") || id.starts_with("gpt-5") || id.starts_with("o1") || id.starts_with("o3") || id.starts_with("o4");

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
                    || id.contains("o1")
                    || id.contains("o3")
                    || id.contains("gemini")
                    || id.contains("llama")
                    || id.contains("mistral")
                    || id.contains("command")
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
                "claude-sonnet-4-20250514",
                "claude-opus-4-20250514",
                "claude-3-7-sonnet-latest",
            ],
            "openai" => vec!["gpt-4.1", "gpt-4o", "gpt-4o-mini"],
            "openrouter" => vec!["openai/gpt-4o-mini", "anthropic/claude-3.5-sonnet"],
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
                {"id": "anthropic/claude-3.5-sonnet"},
                {"id": "openai/gpt-4o-mini"},
                {"id": "openai/text-embedding-3-large"}
            ]
        }"#;

        let parsed = ModelCatalog::parse_models("openrouter", json).unwrap();

        assert_eq!(parsed.len(), 2);
        assert!(parsed
            .iter()
            .any(|model| model.id == "anthropic/claude-3.5-sonnet"));
        assert!(parsed
            .iter()
            .any(|model| model.id == "openai/gpt-4o-mini"));
    }

    #[test]
    fn is_chat_capable_filters_each_provider() {
        assert!(ModelCatalog::is_chat_capable("anthropic", "claude-sonnet-4"));
        assert!(!ModelCatalog::is_chat_capable(
            "anthropic",
            "text-embedding-3-large"
        ));

        assert!(ModelCatalog::is_chat_capable("openai", "gpt-4o"));
        assert!(ModelCatalog::is_chat_capable("openai", "o3-mini-high"));
        assert!(!ModelCatalog::is_chat_capable("openai", "text-embedding-3-large"));
        assert!(!ModelCatalog::is_chat_capable("openai", "gpt-4o-realtime-preview"));
        assert!(!ModelCatalog::is_chat_capable("openai", "gpt-4o-audio-preview"));

        assert!(ModelCatalog::is_chat_capable(
            "openrouter",
            "meta-llama/llama-3.3-70b-instruct"
        ));
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
                "claude-sonnet-4-20250514".to_string(),
                "claude-opus-4-20250514".to_string(),
                "claude-3-7-sonnet-latest".to_string(),
            ]
        );

        let openai = ModelCatalog::hardcoded_fallback("openai")
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();
        assert_eq!(
            openai,
            vec![
                "gpt-4.1".to_string(),
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
                "openai/gpt-4o-mini".to_string(),
                "anthropic/claude-3.5-sonnet".to_string(),
            ]
        );
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
        assert_eq!(
            headers.get(AUTHORIZATION).unwrap(),
            "Bearer setup-token"
        );
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
}
