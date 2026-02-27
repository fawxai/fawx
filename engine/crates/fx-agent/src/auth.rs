//! Authentication routing for cloud LLM providers.
//!
//! This module encapsulates backend selection for subscription-first auth flows.
//! It is intentionally provider-agnostic so the agent can evolve from a single
//! provider (Anthropic) to multi-provider routing (e.g., OpenAI).

use crate::claude::error::{AgentError, Result};
use serde::{Deserialize, Serialize};

/// Auth backends Fawx can attempt for cloud calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthBackend {
    /// Direct Anthropic API call using token from `claude setup-token`.
    AnthropicSetupToken,
    /// Claude Code CLI stream bridge (`-p --output-format stream-json`).
    ClaudeSdkBridge,
    /// Direct Anthropic API call using API key.
    AnthropicApiKey,
    /// OpenAI subscription OAuth token.
    OpenAiSubscriptionOauth,
}

/// Available credentials/backends for auth routing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthCredentials {
    /// Token created via `claude setup-token`.
    pub claude_setup_token: Option<String>,
    /// Anthropic API key fallback.
    pub anthropic_api_key: Option<String>,
    /// OpenAI OAuth subscription token.
    pub openai_oauth_token: Option<String>,
    /// Optional websocket URL for Claude SDK bridge.
    pub claude_sdk_url: Option<String>,
}

impl AuthCredentials {
    /// Build credentials from environment variables.
    pub fn from_env() -> Self {
        Self {
            claude_setup_token: first_env(&[
                "CLAUDE_SETUP_TOKEN",
                "CLAUDE_CODE_SETUP_TOKEN",
                "ANTHROPIC_SUBSCRIPTION_TOKEN",
            ]),
            anthropic_api_key: first_env(&["ANTHROPIC_API_KEY"]),
            openai_oauth_token: first_env(&["OPENAI_OAUTH_TOKEN", "OPENAI_SUBSCRIPTION_TOKEN"]),
            claude_sdk_url: first_env(&["CLAUDE_SDK_URL"]),
        }
    }

    /// Whether a backend is configured with usable credentials.
    pub fn is_configured(&self, backend: AuthBackend) -> bool {
        match backend {
            AuthBackend::AnthropicSetupToken => is_present(&self.claude_setup_token),
            AuthBackend::ClaudeSdkBridge => is_present(&self.claude_sdk_url),
            AuthBackend::AnthropicApiKey => is_present(&self.anthropic_api_key),
            AuthBackend::OpenAiSubscriptionOauth => is_present(&self.openai_oauth_token),
        }
    }
}

/// Ordered auth strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthStrategy {
    backends: Vec<AuthBackend>,
}

impl AuthStrategy {
    /// Build the default subscription-first strategy.
    ///
    /// Order:
    /// 1) setup-token direct call
    /// 2) Claude SDK bridge
    /// 3) Anthropic API key
    /// 4) OpenAI OAuth (optional)
    pub fn subscription_first(include_openai_oauth: bool) -> Self {
        let mut backends = vec![
            AuthBackend::AnthropicSetupToken,
            AuthBackend::ClaudeSdkBridge,
            AuthBackend::AnthropicApiKey,
        ];
        if include_openai_oauth {
            backends.push(AuthBackend::OpenAiSubscriptionOauth);
        }
        Self { backends }
    }

    /// Build a custom strategy with explicit backend order.
    pub fn custom(backends: Vec<AuthBackend>) -> Result<Self> {
        if backends.is_empty() {
            return Err(AgentError::Config(
                "Auth strategy must include at least one backend".to_string(),
            ));
        }
        Ok(Self { backends })
    }

    /// Access the ordered backend list.
    pub fn backends(&self) -> &[AuthBackend] {
        &self.backends
    }
}

/// Stateful backend selector with auth-aware failover.
#[derive(Debug, Clone)]
pub struct AuthRouter {
    strategy: AuthStrategy,
    credentials: AuthCredentials,
    active_index: Option<usize>,
}

impl AuthRouter {
    /// Create a router and select the first configured backend.
    pub fn new(strategy: AuthStrategy, credentials: AuthCredentials) -> Self {
        let mut router = Self {
            strategy,
            credentials,
            active_index: None,
        };
        router.active_index = router.next_configured_index_from(0);
        router
    }

    /// Return the currently active backend, if any are configured.
    pub fn active_backend(&self) -> Option<AuthBackend> {
        self.active_index
            .and_then(|idx| self.strategy.backends.get(idx).copied())
            .filter(|backend| self.credentials.is_configured(*backend))
    }

    /// Returns true if at least one backend is currently configured.
    pub fn has_any_backend(&self) -> bool {
        self.active_backend().is_some()
    }

    /// Replace credentials and recompute active backend.
    pub fn update_credentials(&mut self, credentials: AuthCredentials) {
        self.credentials = credentials;
        self.active_index = self.next_configured_index_from(0);
    }

    /// Record a request failure and fail over when it's an auth failure.
    ///
    /// Returns true if the active backend changed.
    pub fn on_error(&mut self, error: &AgentError) -> bool {
        if !is_auth_failure(error) {
            return false;
        }
        let old_backend = self.active_backend();
        let switched = self.advance_to_next_backend();
        if switched {
            let new_backend = self.active_backend();
            tracing::debug!(
                "Auth failure on backend {:?}, switching to {:?}",
                old_backend,
                new_backend
            );
        }
        switched
    }

    /// Record success. Currently a no-op, reserved for future health tracking
    /// and success rate metrics.
    pub fn on_success(&mut self) {}

    fn advance_to_next_backend(&mut self) -> bool {
        let Some(current) = self.active_index else {
            return false;
        };
        let next = self.next_configured_index_from(current + 1);
        if next != self.active_index {
            self.active_index = next;
            return true;
        }
        false
    }

    fn next_configured_index_from(&self, start: usize) -> Option<usize> {
        self.strategy
            .backends
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, backend)| self.credentials.is_configured(**backend))
            .map(|(idx, _)| idx)
    }
}

/// Whether an error is auth-related and should trigger backend failover.
///
/// Only `Auth` and `Config` errors trigger failover. Rate limits and network errors
/// are intentionally excluded to avoid cascading failures: if one backend is
/// rate-limited or experiencing network issues, switching to another backend
/// with the same underlying problem would amplify the issue rather than resolve it.
pub fn is_auth_failure(error: &AgentError) -> bool {
    matches!(error, AgentError::Auth(_) | AgentError::Config(_))
}

fn is_present(value: &Option<String>) -> bool {
    value
        .as_ref()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn first_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_strategy_without_openai() {
        let strategy = AuthStrategy::subscription_first(false);
        assert_eq!(
            strategy.backends(),
            &[
                AuthBackend::AnthropicSetupToken,
                AuthBackend::ClaudeSdkBridge,
                AuthBackend::AnthropicApiKey
            ]
        );
    }

    #[test]
    fn test_subscription_strategy_with_openai() {
        let strategy = AuthStrategy::subscription_first(true);
        assert_eq!(
            strategy.backends(),
            &[
                AuthBackend::AnthropicSetupToken,
                AuthBackend::ClaudeSdkBridge,
                AuthBackend::AnthropicApiKey,
                AuthBackend::OpenAiSubscriptionOauth
            ]
        );
    }

    #[test]
    fn test_router_prefers_setup_token() {
        let creds = AuthCredentials {
            claude_setup_token: Some("setup-token".to_string()),
            anthropic_api_key: Some("api-key".to_string()),
            openai_oauth_token: Some("openai-oauth".to_string()),
            claude_sdk_url: Some("ws://localhost:4242".to_string()),
        };
        let router = AuthRouter::new(AuthStrategy::subscription_first(true), creds);
        assert_eq!(
            router.active_backend(),
            Some(AuthBackend::AnthropicSetupToken)
        );
    }

    #[test]
    fn test_router_falls_back_on_auth_error() {
        let creds = AuthCredentials {
            claude_setup_token: Some("setup-token".to_string()),
            anthropic_api_key: Some("api-key".to_string()),
            openai_oauth_token: None,
            claude_sdk_url: Some("ws://localhost:4242".to_string()),
        };
        let mut router = AuthRouter::new(AuthStrategy::subscription_first(false), creds);
        assert_eq!(
            router.active_backend(),
            Some(AuthBackend::AnthropicSetupToken)
        );

        let switched = router.on_error(&AgentError::Auth("token rejected".to_string()));
        assert!(switched);
        assert_eq!(router.active_backend(), Some(AuthBackend::ClaudeSdkBridge));

        let switched = router.on_error(&AgentError::Auth("bridge rejected".to_string()));
        assert!(switched);
        assert_eq!(router.active_backend(), Some(AuthBackend::AnthropicApiKey));
    }

    #[test]
    fn test_router_falls_back_to_openai_when_enabled() {
        let creds = AuthCredentials {
            claude_setup_token: Some("setup-token".to_string()),
            anthropic_api_key: None,
            openai_oauth_token: Some("openai-oauth".to_string()),
            claude_sdk_url: None,
        };
        let mut router = AuthRouter::new(AuthStrategy::subscription_first(true), creds);
        assert_eq!(
            router.active_backend(),
            Some(AuthBackend::AnthropicSetupToken)
        );

        let switched = router.on_error(&AgentError::Auth("token rejected".to_string()));
        assert!(switched);
        assert_eq!(
            router.active_backend(),
            Some(AuthBackend::OpenAiSubscriptionOauth)
        );
    }

    #[test]
    fn test_router_does_not_switch_on_non_auth_error() {
        let creds = AuthCredentials {
            claude_setup_token: Some("setup-token".to_string()),
            anthropic_api_key: Some("api-key".to_string()),
            openai_oauth_token: None,
            claude_sdk_url: None,
        };
        let mut router = AuthRouter::new(AuthStrategy::subscription_first(false), creds);
        assert_eq!(
            router.active_backend(),
            Some(AuthBackend::AnthropicSetupToken)
        );

        let switched = router.on_error(&AgentError::RateLimit("slow down".to_string()));
        assert!(!switched);
        assert_eq!(
            router.active_backend(),
            Some(AuthBackend::AnthropicSetupToken)
        );
    }

    #[test]
    fn test_router_without_configured_backends() {
        let creds = AuthCredentials::default();
        let router = AuthRouter::new(AuthStrategy::subscription_first(true), creds);
        assert!(!router.has_any_backend());
        assert_eq!(router.active_backend(), None);
    }

    #[test]
    fn test_custom_strategy_rejects_empty() {
        let result = AuthStrategy::custom(Vec::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_update_credentials_switches_active_backend() {
        // Start with setup token
        let initial_creds = AuthCredentials {
            claude_setup_token: Some("setup-token".to_string()),
            anthropic_api_key: None,
            openai_oauth_token: None,
            claude_sdk_url: None,
        };
        let mut router = AuthRouter::new(AuthStrategy::subscription_first(false), initial_creds);
        assert_eq!(
            router.active_backend(),
            Some(AuthBackend::AnthropicSetupToken)
        );

        // Update to only have API key
        let updated_creds = AuthCredentials {
            claude_setup_token: None,
            anthropic_api_key: Some("api-key".to_string()),
            openai_oauth_token: None,
            claude_sdk_url: None,
        };
        router.update_credentials(updated_creds);

        // Should switch to API key backend
        assert_eq!(router.active_backend(), Some(AuthBackend::AnthropicApiKey));
    }
}
