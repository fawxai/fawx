use crate::credential_store::{
    AuthProvider, CredentialMethod, CredentialStore, EncryptedFileCredentialStore,
};
use fx_config::BorrowScope;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroizing;

/// A borrowed GitHub credential with an explicit scope.
pub struct TokenBorrow {
    token: Zeroizing<String>,
    scope: BorrowScope,
}

impl fmt::Debug for TokenBorrow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenBorrow")
            .field("scope", &self.scope)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl TokenBorrow {
    pub fn new(token: Zeroizing<String>, scope: BorrowScope) -> Self {
        Self { token, scope }
    }

    pub fn token(&self) -> &str {
        self.token.as_str()
    }

    /// Consume the borrow and return the token, preserving zeroize guarantees.
    pub fn into_token(self) -> Zeroizing<String> {
        self.token
    }

    pub fn scope(&self) -> BorrowScope {
        self.scope
    }
}

/// Errors from token borrowing.
#[derive(Debug)]
pub enum BorrowError {
    NotConfigured,
    StoreError(String),
    ScopeExceeded {
        requested: BorrowScope,
        max: BorrowScope,
    },
}

impl fmt::Display for BorrowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "GitHub PAT is not configured"),
            Self::StoreError(message) => write!(f, "credential store error: {message}"),
            Self::ScopeExceeded { requested, max } => write!(
                f,
                "requested GitHub token scope {requested} exceeds configured maximum {max}"
            ),
        }
    }
}

impl std::error::Error for BorrowError {}

/// Trait for lending scoped credentials to subagents/workers.
pub trait TokenBroker: Send + Sync {
    fn borrow_github(&self, scope: BorrowScope) -> Result<TokenBorrow, BorrowError>;

    fn borrow_github_default(&self) -> Result<TokenBorrow, BorrowError>;
}

/// Brokers GitHub credentials from the encrypted credential store.
pub struct CredentialStoreBroker {
    store: Arc<EncryptedFileCredentialStore>,
    max_scope: BorrowScope,
}

impl CredentialStoreBroker {
    pub fn new(store: Arc<EncryptedFileCredentialStore>, max_scope: BorrowScope) -> Self {
        Self { store, max_scope }
    }
}

impl fmt::Debug for CredentialStoreBroker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialStoreBroker")
            .field("max_scope", &self.max_scope)
            .finish_non_exhaustive()
    }
}

impl TokenBroker for CredentialStoreBroker {
    fn borrow_github(&self, scope: BorrowScope) -> Result<TokenBorrow, BorrowError> {
        if scope_exceeds(scope, self.max_scope) {
            return Err(BorrowError::ScopeExceeded {
                requested: scope,
                max: self.max_scope,
            });
        }

        match self.store.get(AuthProvider::GitHub, CredentialMethod::Pat) {
            Ok(Some(token)) => Ok(TokenBorrow::new(token, scope)),
            Ok(None) => Err(BorrowError::NotConfigured),
            Err(error) => Err(BorrowError::StoreError(error.to_string())),
        }
    }

    fn borrow_github_default(&self) -> Result<TokenBorrow, BorrowError> {
        self.borrow_github(self.max_scope)
    }
}

fn scope_exceeds(requested: BorrowScope, max: BorrowScope) -> bool {
    matches!(
        (requested, max),
        (BorrowScope::Contribution, BorrowScope::ReadOnly)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential_store::{AuthProvider, CredentialMetadata, CredentialStore};

    fn test_metadata() -> CredentialMetadata {
        CredentialMetadata {
            provider: AuthProvider::GitHub,
            method: CredentialMethod::Pat,
            last_validated_ms: 0,
            login: None,
            scopes: Vec::new(),
            token_kind: None,
        }
    }

    fn store_github_pat(store: &EncryptedFileCredentialStore, token: &str) {
        store
            .set(
                AuthProvider::GitHub,
                CredentialMethod::Pat,
                &Zeroizing::new(token.to_string()),
                &test_metadata(),
            )
            .expect("set GitHub PAT");
    }

    #[test]
    fn borrow_scope_default_is_read_only() {
        assert_eq!(BorrowScope::default(), BorrowScope::ReadOnly);
    }

    #[test]
    fn borrow_scope_serde_roundtrip() {
        for scope in [BorrowScope::ReadOnly, BorrowScope::Contribution] {
            let encoded = serde_json::to_string(&scope).expect("serialize borrow scope");
            let decoded: BorrowScope = serde_json::from_str(&encoded).expect("deserialize scope");
            assert_eq!(decoded, scope);
        }
    }

    #[test]
    fn borrow_scope_display() {
        assert_eq!(BorrowScope::ReadOnly.to_string(), "read_only");
        assert_eq!(BorrowScope::Contribution.to_string(), "contribution");
    }

    #[test]
    fn scope_exceeds_contribution_over_readonly() {
        assert!(scope_exceeds(
            BorrowScope::Contribution,
            BorrowScope::ReadOnly,
        ));
    }

    #[test]
    fn scope_does_not_exceed_readonly_over_readonly() {
        assert!(!scope_exceeds(BorrowScope::ReadOnly, BorrowScope::ReadOnly));
    }

    #[test]
    fn scope_does_not_exceed_contribution_over_contribution() {
        assert!(!scope_exceeds(
            BorrowScope::Contribution,
            BorrowScope::Contribution,
        ));
    }

    #[test]
    fn borrow_error_display() {
        assert_eq!(
            BorrowError::NotConfigured.to_string(),
            "GitHub PAT is not configured"
        );
        assert_eq!(
            BorrowError::StoreError("boom".to_string()).to_string(),
            "credential store error: boom"
        );
        assert_eq!(
            BorrowError::ScopeExceeded {
                requested: BorrowScope::Contribution,
                max: BorrowScope::ReadOnly,
            }
            .to_string(),
            "requested GitHub token scope contribution exceeds configured maximum read_only"
        );
    }

    #[test]
    fn token_borrow_accessors() {
        let borrow = TokenBorrow::new(
            Zeroizing::new("ghp-test-token".to_string()),
            BorrowScope::Contribution,
        );
        assert_eq!(borrow.token(), "ghp-test-token");
        assert_eq!(borrow.scope(), BorrowScope::Contribution);
    }

    #[test]
    fn broker_returns_not_configured_when_no_token() {
        let store = Arc::new(EncryptedFileCredentialStore::open_in_memory().expect("open store"));
        let broker = CredentialStoreBroker::new(store, BorrowScope::ReadOnly);
        let error = broker
            .borrow_github(BorrowScope::ReadOnly)
            .expect_err("borrow should fail without token");

        assert!(matches!(error, BorrowError::NotConfigured));
    }

    #[test]
    fn broker_returns_token_for_readonly_borrow() {
        let store = Arc::new(EncryptedFileCredentialStore::open_in_memory().expect("open store"));
        store_github_pat(&store, "ghp-readonly-token");
        let broker = CredentialStoreBroker::new(store, BorrowScope::Contribution);
        let borrow = broker
            .borrow_github(BorrowScope::ReadOnly)
            .expect("borrow should succeed");

        assert_eq!(borrow.token(), "ghp-readonly-token");
        assert_eq!(borrow.scope(), BorrowScope::ReadOnly);
    }

    #[test]
    fn broker_returns_token_for_contribution_borrow() {
        let store = Arc::new(EncryptedFileCredentialStore::open_in_memory().expect("open store"));
        store_github_pat(&store, "ghp-contribution-token");
        let broker = CredentialStoreBroker::new(store, BorrowScope::Contribution);
        let borrow = broker
            .borrow_github(BorrowScope::Contribution)
            .expect("borrow should succeed");

        assert_eq!(borrow.token(), "ghp-contribution-token");
        assert_eq!(borrow.scope(), BorrowScope::Contribution);
    }

    #[test]
    fn broker_borrow_default_uses_max_scope() {
        let store = Arc::new(EncryptedFileCredentialStore::open_in_memory().expect("open store"));
        store_github_pat(&store, "ghp-default-token");
        let broker = CredentialStoreBroker::new(store, BorrowScope::Contribution);
        let borrow = broker
            .borrow_github_default()
            .expect("default borrow should succeed");

        assert_eq!(borrow.scope(), BorrowScope::Contribution);
        assert_eq!(borrow.token(), "ghp-default-token");
    }

    #[test]
    fn broker_borrow_default_readonly() {
        let store = Arc::new(EncryptedFileCredentialStore::open_in_memory().expect("open store"));
        store_github_pat(&store, "ghp-ro-token");
        let broker = CredentialStoreBroker::new(store, BorrowScope::ReadOnly);
        let borrow = broker
            .borrow_github_default()
            .expect("default borrow should succeed");

        assert_eq!(borrow.scope(), BorrowScope::ReadOnly);
    }

    #[test]
    fn token_borrow_into_token_consumes_borrow() {
        let borrow = TokenBorrow::new(
            Zeroizing::new("ghp-consume".to_string()),
            BorrowScope::ReadOnly,
        );
        let token = borrow.into_token();
        assert_eq!(token.as_str(), "ghp-consume");
    }

    #[test]
    fn token_borrow_debug_redacts_token() {
        let borrow = TokenBorrow::new(
            Zeroizing::new("ghp-secret-value".to_string()),
            BorrowScope::Contribution,
        );
        let debug = format!("{borrow:?}");
        assert!(
            !debug.contains("ghp-secret-value"),
            "Debug should redact token"
        );
        assert!(debug.contains("REDACTED"), "Debug should show REDACTED");
        assert!(debug.contains("Contribution"), "Debug should show scope");
    }

    #[test]
    fn broker_rejects_contribution_when_max_is_readonly() {
        let store = Arc::new(EncryptedFileCredentialStore::open_in_memory().expect("open store"));
        store_github_pat(&store, "ghp-restricted-token");
        let broker = CredentialStoreBroker::new(store, BorrowScope::ReadOnly);
        let error = broker
            .borrow_github(BorrowScope::Contribution)
            .expect_err("borrow should reject contribution scope");

        assert!(matches!(
            error,
            BorrowError::ScopeExceeded {
                requested: BorrowScope::Contribution,
                max: BorrowScope::ReadOnly,
            }
        ));
    }
}
