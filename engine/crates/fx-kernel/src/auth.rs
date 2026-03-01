//! Authentication model and credential management for LLM providers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Authentication method for an LLM provider.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthMethod {
    /// Direct API key (BYO key).
    ApiKey {
        /// Provider identifier (e.g., "openai", "anthropic", "openrouter").
        provider: String,
        /// Secret API key value.
        key: String,
    },
    /// Claude setup-token (Anthropic subscription).
    SetupToken {
        /// Setup token produced by `claude setup-token`.
        token: String,
    },
    /// OAuth tokens (ChatGPT subscription).
    OAuth {
        /// Provider identifier (e.g., "openai").
        provider: String,
        /// OAuth access token used as bearer auth.
        access_token: String,
        /// OAuth refresh token used to mint new access tokens.
        refresh_token: String,
        /// Access token expiry (Unix timestamp in milliseconds).
        expires_at: u64,
        /// Optional provider account identifier.
        account_id: Option<String>,
    },
}

impl fmt::Debug for AuthMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey { provider, .. } => f
                .debug_struct("ApiKey")
                .field("provider", provider)
                .field("key", &"<redacted>")
                .finish(),
            Self::SetupToken { .. } => f
                .debug_struct("SetupToken")
                .field("token", &"<redacted>")
                .finish(),
            Self::OAuth {
                provider,
                expires_at,
                account_id,
                ..
            } => f
                .debug_struct("OAuth")
                .field("provider", provider)
                .field("access_token", &"<redacted>")
                .field("refresh_token", &"<redacted>")
                .field("expires_at", expires_at)
                .field("account_id", account_id)
                .finish(),
        }
    }
}

impl AuthMethod {
    /// Get the bearer token for HTTP requests.
    pub fn bearer_token(&self) -> &str {
        match self {
            Self::ApiKey { key, .. } => key,
            Self::SetupToken { token } => token,
            Self::OAuth { access_token, .. } => access_token,
        }
    }

    /// Check whether this auth method needs token refresh.
    ///
    /// Returns `true` only for OAuth credentials when `current_time_ms`
    /// is at or beyond `expires_at`.
    pub fn needs_refresh(&self, current_time_ms: u64) -> bool {
        match self {
            Self::OAuth { expires_at, .. } => current_time_ms >= *expires_at,
            Self::ApiKey { .. } | Self::SetupToken { .. } => false,
        }
    }

    /// Provider name for routing.
    pub fn provider_name(&self) -> &str {
        match self {
            Self::ApiKey { provider, .. } => provider,
            Self::SetupToken { .. } => "anthropic",
            Self::OAuth { provider, .. } => provider,
        }
    }
}

/// Manages authentication credentials.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct AuthManager {
    credentials: HashMap<String, AuthMethod>,
}

impl fmt::Debug for AuthManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthManager")
            .field("providers", &self.providers())
            .field("credential_count", &self.credentials.len())
            .finish()
    }
}

impl AuthManager {
    /// Create an empty auth manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a credential.
    pub fn store(&mut self, provider: &str, mut auth: AuthMethod) {
        match &mut auth {
            AuthMethod::ApiKey {
                provider: embedded_provider,
                ..
            }
            | AuthMethod::OAuth {
                provider: embedded_provider,
                ..
            } => {
                if embedded_provider != provider {
                    *embedded_provider = provider.to_owned();
                }
            }
            AuthMethod::SetupToken { .. } => {}
        }

        self.credentials.insert(provider.to_owned(), auth);
    }

    /// Get credential for a provider.
    pub fn get(&self, provider: &str) -> Option<&AuthMethod> {
        self.credentials.get(provider)
    }

    /// Remove a credential.
    pub fn remove(&mut self, provider: &str) -> Option<AuthMethod> {
        self.credentials.remove(provider)
    }

    /// List all configured providers.
    pub fn providers(&self) -> Vec<String> {
        let mut providers: Vec<String> = self.credentials.keys().cloned().collect();
        providers.sort();
        providers
    }

    /// Check if any auth is configured.
    pub fn has_any(&self) -> bool {
        !self.credentials.is_empty()
    }

    /// Serialize all credentials for storage.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.credentials)
    }

    /// Deserialize credentials from storage.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let credentials = serde_json::from_str(json)?;
        Ok(Self { credentials })
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthManager, AuthMethod};

    #[test]
    fn bearer_token_returns_expected_value_per_variant() {
        let api_key = AuthMethod::ApiKey {
            provider: "openrouter".to_string(),
            key: "api-key-123".to_string(),
        };
        let setup_token = AuthMethod::SetupToken {
            token: "setup-token-456".to_string(),
        };
        let oauth = AuthMethod::OAuth {
            provider: "openai".to_string(),
            access_token: "access-789".to_string(),
            refresh_token: "refresh-xyz".to_string(),
            expires_at: 42,
            account_id: Some("acct_1".to_string()),
        };

        assert_eq!(api_key.bearer_token(), "api-key-123");
        assert_eq!(setup_token.bearer_token(), "setup-token-456");
        assert_eq!(oauth.bearer_token(), "access-789");
    }

    #[test]
    fn needs_refresh_respects_expiry_for_oauth_only() {
        let oauth = AuthMethod::OAuth {
            provider: "openai".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            expires_at: 1_000,
            account_id: None,
        };
        let api_key = AuthMethod::ApiKey {
            provider: "anthropic".to_string(),
            key: "key".to_string(),
        };

        assert!(!oauth.needs_refresh(999));
        assert!(oauth.needs_refresh(1_000));
        assert!(oauth.needs_refresh(1_001));
        assert!(!api_key.needs_refresh(9_999_999));
    }

    #[test]
    fn provider_name_matches_variant() {
        let api_key = AuthMethod::ApiKey {
            provider: "openrouter".to_string(),
            key: "key".to_string(),
        };
        let setup_token = AuthMethod::SetupToken {
            token: "token".to_string(),
        };
        let oauth = AuthMethod::OAuth {
            provider: "openai".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            expires_at: 0,
            account_id: None,
        };

        assert_eq!(api_key.provider_name(), "openrouter");
        assert_eq!(setup_token.provider_name(), "anthropic");
        assert_eq!(oauth.provider_name(), "openai");
    }

    #[test]
    fn auth_manager_store_get_remove_and_has_any() {
        let mut manager = AuthManager::new();
        assert!(!manager.has_any());

        manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "setup-token".to_string(),
            },
        );
        manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "access-token".to_string(),
                refresh_token: "refresh-token".to_string(),
                expires_at: 5_000,
                account_id: Some("acct_123".to_string()),
            },
        );

        assert!(manager.has_any());
        assert!(matches!(
            manager.get("anthropic"),
            Some(AuthMethod::SetupToken { .. })
        ));
        assert!(matches!(
            manager.get("openai"),
            Some(AuthMethod::OAuth { .. })
        ));
        assert!(manager.get("missing").is_none());

        let removed = manager.remove("anthropic");
        assert!(matches!(removed, Some(AuthMethod::SetupToken { .. })));
        assert!(manager.get("anthropic").is_none());
    }

    #[test]
    fn auth_manager_remove_provider_flow_removes_it_from_state() {
        let mut manager = AuthManager::new();
        manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "token-1".to_string(),
            },
        );
        manager.store(
            "openai",
            AuthMethod::ApiKey {
                provider: "openai".to_string(),
                key: "key-2".to_string(),
            },
        );

        let removed = manager.remove("openai");

        assert!(matches!(removed, Some(AuthMethod::ApiKey { .. })));
        assert!(manager.get("openai").is_none());
        assert_eq!(manager.providers(), vec!["anthropic".to_string()]);
    }

    #[test]
    fn auth_manager_oauth_storage_exposes_bearer_and_expired_refresh_state() {
        let mut manager = AuthManager::new();
        manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "stored-access-token".to_string(),
                refresh_token: "stored-refresh-token".to_string(),
                expires_at: 1_000,
                account_id: Some("acct_oauth".to_string()),
            },
        );

        let stored = manager
            .get("openai")
            .expect("oauth credential should be stored");

        assert!(matches!(
            stored,
            AuthMethod::OAuth {
                provider,
                account_id,
                ..
            } if provider == "openai" && account_id.as_deref() == Some("acct_oauth")
        ));
        assert_eq!(stored.bearer_token(), "stored-access-token");
        assert!(stored.needs_refresh(1_001));
    }

    #[test]
    fn auth_manager_oauth_refresh_detection_changes_with_expiry_window() {
        let mut manager = AuthManager::new();

        manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "future-access-token".to_string(),
                refresh_token: "future-refresh-token".to_string(),
                expires_at: 10_000,
                account_id: None,
            },
        );

        assert!(!manager
            .get("openai")
            .expect("oauth credential should exist")
            .needs_refresh(9_999));

        manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "past-access-token".to_string(),
                refresh_token: "past-refresh-token".to_string(),
                expires_at: 5_000,
                account_id: None,
            },
        );

        assert!(manager
            .get("openai")
            .expect("oauth credential should still exist")
            .needs_refresh(5_001));
    }

    #[test]
    fn auth_manager_store_normalizes_embedded_provider_for_keyed_variants() {
        let mut manager = AuthManager::new();

        manager.store(
            "openai",
            AuthMethod::ApiKey {
                provider: "mismatch".to_string(),
                key: "key-1".to_string(),
            },
        );
        manager.store(
            "openrouter",
            AuthMethod::OAuth {
                provider: "another-mismatch".to_string(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                expires_at: 100,
                account_id: None,
            },
        );

        assert!(matches!(
            manager.get("openai"),
            Some(AuthMethod::ApiKey { provider, .. }) if provider == "openai"
        ));
        assert!(matches!(
            manager.get("openrouter"),
            Some(AuthMethod::OAuth { provider, .. }) if provider == "openrouter"
        ));
    }

    #[test]
    fn auth_manager_providers_is_sorted() {
        let mut manager = AuthManager::new();
        manager.store(
            "openrouter",
            AuthMethod::ApiKey {
                provider: "openrouter".to_string(),
                key: "key-1".to_string(),
            },
        );
        manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "token-1".to_string(),
            },
        );
        manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                expires_at: 100,
                account_id: None,
            },
        );

        assert_eq!(
            manager.providers(),
            vec![
                "anthropic".to_string(),
                "openai".to_string(),
                "openrouter".to_string(),
            ]
        );
    }

    #[test]
    fn auth_manager_serialization_roundtrip() {
        let mut manager = AuthManager::new();
        manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "setup-token".to_string(),
            },
        );
        manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "access-token".to_string(),
                refresh_token: "refresh-token".to_string(),
                expires_at: 123_456,
                account_id: Some("acct_99".to_string()),
            },
        );

        let json = manager.to_json().expect("auth manager should serialize");
        let restored = AuthManager::from_json(&json).expect("auth manager should deserialize");

        assert_eq!(restored.providers(), manager.providers());
        assert_eq!(restored.get("anthropic"), manager.get("anthropic"));
        assert_eq!(restored.get("openai"), manager.get("openai"));
    }

    #[test]
    fn debug_redacts_secrets() {
        let auth = AuthMethod::ApiKey {
            provider: "openrouter".to_string(),
            key: "super-secret-key".to_string(),
        };

        let debug_output = format!("{auth:?}");
        assert!(debug_output.contains("<redacted>"));
        assert!(!debug_output.contains("super-secret-key"));
    }
}
