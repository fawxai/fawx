//! PKCE OAuth primitives for ChatGPT subscription authentication.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// OpenAI OAuth client ID (matches Codex CLI / OpenClaw).
pub const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_OAUTH_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
/// OpenAI OAuth token endpoint.
pub const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const DEFAULT_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const DEFAULT_ORIGINATOR: &str = "codex_cli_rs";
const DEFAULT_SCOPE: &str = "openid profile email offline_access";
/// JWT claim path for extracting account ID from OpenAI access tokens.
pub const OPENAI_JWT_CLAIM_PATH: &str = "https://api.openai.com/auth";

/// PKCE OAuth flow for ChatGPT subscription auth.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PkceFlow {
    code_verifier: String,
    code_challenge: String,
    state: String,
    redirect_uri: String,
}

impl fmt::Debug for PkceFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PkceFlow")
            .field("code_verifier", &"<redacted>")
            .field("code_challenge", &self.code_challenge)
            .field("state", &"<redacted>")
            .field("redirect_uri", &self.redirect_uri)
            .finish()
    }
}

impl PkceFlow {
    /// Generate a new PKCE flow with random verifier and state.
    pub fn try_new() -> Result<Self, AuthError> {
        let verifier_random = random_bytes::<32>()?;
        let code_verifier = URL_SAFE_NO_PAD.encode(verifier_random);
        let code_challenge = pkce_challenge(&code_verifier);
        let state = random_hex_string::<32>()?;

        Ok(Self {
            code_verifier,
            code_challenge,
            state,
            redirect_uri: DEFAULT_REDIRECT_URI.to_string(),
        })
    }

    /// Override the redirect URI (e.g. for native app callback schemes).
    pub fn with_redirect_uri(mut self, uri: impl Into<String>) -> Self {
        self.redirect_uri = uri.into();
        self
    }

    /// Build the authorization URL with all required OAuth + OpenAI params.
    pub fn authorization_url(&self, client_id: &str) -> String {
        format!(
            "{base}?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}&id_token_add_organizations=true&codex_cli_simplified_flow=true&originator={originator}",
            base = OPENAI_OAUTH_AUTHORIZE_URL,
            client_id = percent_encode(client_id),
            redirect_uri = percent_encode(&self.redirect_uri),
            scope = percent_encode(DEFAULT_SCOPE),
            code_challenge = percent_encode(&self.code_challenge),
            state = percent_encode(&self.state),
            originator = percent_encode(DEFAULT_ORIGINATOR),
        )
    }

    /// Parse the callback URL, validate state, and extract the authorization code.
    pub fn parse_callback(&self, callback_url: &str) -> Result<String, AuthError> {
        let params = parse_query_params(callback_url)?;

        let returned_state = params.get("state").ok_or(AuthError::InvalidState)?;
        if returned_state != &self.state {
            return Err(AuthError::InvalidState);
        }

        let code = params.get("code").ok_or(AuthError::MissingCode)?;
        if code.trim().is_empty() {
            return Err(AuthError::MissingCode);
        }

        Ok(code.clone())
    }

    /// Borrow the generated PKCE code verifier.
    pub fn code_verifier(&self) -> &str {
        &self.code_verifier
    }

    /// Borrow the generated PKCE code challenge.
    pub fn code_challenge(&self) -> &str {
        &self.code_challenge
    }

    /// Borrow the generated anti-CSRF state value.
    pub fn state(&self) -> &str {
        &self.state
    }

    /// Borrow the redirect URI associated with this flow.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }
}

/// Token exchange request body.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenExchangeRequest {
    /// OAuth grant type (`authorization_code`).
    pub grant_type: String,
    /// Authorization code received from callback.
    pub code: String,
    /// Redirect URI used in the authorization request.
    pub redirect_uri: String,
    /// Original PKCE verifier used to derive the challenge.
    pub code_verifier: String,
    /// OAuth client identifier.
    pub client_id: String,
}

impl fmt::Debug for TokenExchangeRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenExchangeRequest")
            .field("grant_type", &self.grant_type)
            .field("code", &"<redacted>")
            .field("redirect_uri", &self.redirect_uri)
            .field("code_verifier", &"<redacted>")
            .field("client_id", &self.client_id)
            .finish()
    }
}

/// OAuth token response payload.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenResponse {
    /// Access token used for bearer authentication.
    pub access_token: String,
    /// Refresh token used to obtain new access tokens.
    pub refresh_token: String,
    /// Access token lifetime in seconds.
    pub expires_in: u64,
    /// Token type from the provider (typically `Bearer`).
    pub token_type: String,
}

impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .field("token_type", &self.token_type)
            .finish()
    }
}

/// Token refresh request body.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenRefreshRequest {
    /// OAuth grant type (`refresh_token`).
    pub grant_type: String,
    /// Refresh token used to mint a new access token.
    pub refresh_token: String,
    /// OAuth client identifier.
    pub client_id: String,
}

impl fmt::Debug for TokenRefreshRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenRefreshRequest")
            .field("grant_type", &self.grant_type)
            .field("refresh_token", &"<redacted>")
            .field("client_id", &self.client_id)
            .finish()
    }
}

/// OAuth-specific errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// Callback state did not match the original request state.
    InvalidState,
    /// Callback did not include the authorization code.
    MissingCode,
    /// Token exchange request failed.
    ExchangeFailed(String),
    /// Token refresh request failed.
    RefreshFailed(String),
    /// Callback URL was malformed.
    InvalidCallback(String),
    /// Secure random byte generation failed.
    RandomGenerationFailed(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState => write!(f, "oauth state validation failed"),
            Self::MissingCode => write!(f, "oauth callback missing authorization code"),
            Self::ExchangeFailed(reason) => {
                write!(f, "oauth token exchange failed: {reason}")
            }
            Self::RefreshFailed(reason) => write!(f, "oauth token refresh failed: {reason}"),
            Self::InvalidCallback(reason) => write!(f, "invalid oauth callback: {reason}"),
            Self::RandomGenerationFailed(reason) => {
                write!(f, "oauth random generation failed: {reason}")
            }
        }
    }
}

impl std::error::Error for AuthError {}

/// Extract the ChatGPT account ID from an OpenAI OAuth access token JWT.
/// Returns `None` if the token is not a valid JWT or doesn't contain the claim.
pub fn extract_openai_account_id(access_token: &str) -> Option<String> {
    let parts: Vec<&str> = access_token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    // Decode the payload (part 1) — base64url without padding
    let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;
    let account_id = payload
        .get(OPENAI_JWT_CLAIM_PATH)?
        .get("chatgpt_account_id")?
        .as_str()?;
    if account_id.is_empty() {
        return None;
    }
    Some(account_id.to_string())
}

fn parse_query_params(callback_url: &str) -> Result<HashMap<String, String>, AuthError> {
    let query = callback_url
        .split_once('?')
        .map(|(_, q)| q)
        .or_else(|| {
            if callback_url.contains('=') {
                Some(callback_url)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            AuthError::InvalidCallback("missing query string in callback URL".to_string())
        })?;

    let query = query.split('#').next().unwrap_or_default();

    let mut params = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }

        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(raw_key)?;
        let value = percent_decode(raw_value)?;

        params.insert(key, value);
    }

    Ok(params)
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());

    for &b in value.as_bytes() {
        if is_unreserved(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(hex_digit((b >> 4) & 0x0f));
            out.push(hex_digit(b & 0x0f));
        }
    }

    out
}

fn percent_decode(value: &str) -> Result<String, AuthError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());

    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'%' => {
                if idx + 2 >= bytes.len() {
                    return Err(AuthError::InvalidCallback(
                        "incomplete percent-encoded sequence".to_string(),
                    ));
                }

                let hi = from_hex_digit(bytes[idx + 1]).ok_or_else(|| {
                    AuthError::InvalidCallback("invalid percent-encoded sequence".to_string())
                })?;
                let lo = from_hex_digit(bytes[idx + 2]).ok_or_else(|| {
                    AuthError::InvalidCallback("invalid percent-encoded sequence".to_string())
                })?;

                decoded.push((hi << 4) | lo);
                idx += 3;
            }
            b'+' => {
                decoded.push(b' ');
                idx += 1;
            }
            ch => {
                decoded.push(ch);
                idx += 1;
            }
        }
    }

    String::from_utf8(decoded)
        .map_err(|_| AuthError::InvalidCallback("query value is not valid UTF-8".to_string()))
}

fn is_unreserved(b: u8) -> bool {
    matches!(
        b,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'
    )
}

fn from_hex_digit(d: u8) -> Option<u8> {
    match d {
        b'0'..=b'9' => Some(d - b'0'),
        b'a'..=b'f' => Some(d - b'a' + 10),
        b'A'..=b'F' => Some(d - b'A' + 10),
        _ => None,
    }
}

fn hex_digit(v: u8) -> char {
    match v {
        0..=9 => (b'0' + v) as char,
        10..=15 => (b'A' + (v - 10)) as char,
        _ => unreachable!("value must be in 0..=15"),
    }
}

fn random_bytes<const N: usize>() -> Result<[u8; N], AuthError> {
    random_bytes_with_fill::<N>(|dest| {
        SystemRandom::new().fill(dest).map_err(|error| {
            AuthError::RandomGenerationFailed(format!("system RNG unavailable: {error:?}"))
        })
    })
}

fn random_bytes_with_fill<const N: usize>(
    fill: impl FnOnce(&mut [u8]) -> Result<(), AuthError>,
) -> Result<[u8; N], AuthError> {
    let mut out = [0u8; N];
    fill(&mut out)?;
    Ok(out)
}

fn random_hex_string<const N: usize>() -> Result<String, AuthError> {
    let bytes = random_bytes::<N>()?;
    let mut out = String::with_capacity(N * 2);

    for byte in bytes {
        out.push(hex_digit((byte >> 4) & 0x0f));
        out.push(hex_digit(byte & 0x0f));
    }

    Ok(out)
}

fn pkce_challenge(code_verifier: &str) -> String {
    let digest = digest(&SHA256, code_verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.as_ref())
}

#[cfg(test)]
mod tests {
    use super::{
        extract_openai_account_id, percent_encode, pkce_challenge, random_bytes_with_fill,
        AuthError, PkceFlow, TokenExchangeRequest, TokenRefreshRequest, TokenResponse,
        OPENAI_JWT_CLAIM_PATH,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    #[test]
    fn pkce_flow_generates_valid_verifier_and_challenge() {
        let flow = PkceFlow::try_new().expect("pkce flow");

        assert!(flow.code_verifier().len() >= 43);
        assert!(flow.code_verifier().len() <= 128);
        assert!(flow
            .code_verifier()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));

        let expected_challenge = pkce_challenge(flow.code_verifier());
        assert_eq!(flow.code_challenge(), expected_challenge);

        assert_eq!(flow.state().len(), 64);
        assert!(flow.state().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn authorization_url_contains_required_parameters() {
        let flow = PkceFlow::try_new().expect("pkce flow");
        let client_id = "fawx-client-id";

        let url = flow.authorization_url(client_id);

        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
        assert!(url.contains(&format!("client_id={}", percent_encode(client_id))));
        assert!(url.contains(&format!(
            "redirect_uri={}",
            percent_encode(flow.redirect_uri())
        )));
        assert!(url.contains(&format!(
            "code_challenge={}",
            percent_encode(flow.code_challenge())
        )));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("state={}", percent_encode(flow.state()))));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope="));
        assert!(url.contains("originator=codex_cli_rs"));
    }

    #[test]
    fn pkce_flow_with_custom_redirect_uri() {
        let flow = PkceFlow::try_new()
            .expect("pkce flow")
            .with_redirect_uri("fawx-auth://openai/callback");

        assert_eq!(flow.redirect_uri(), "fawx-auth://openai/callback");
        let url = flow.authorization_url("test-client");
        assert!(url.contains("fawx-auth"));
    }

    #[test]
    fn parse_callback_extracts_authorization_code() {
        let flow = PkceFlow::try_new().expect("pkce flow");
        let callback = format!(
            "{}?code=auth-code-123&state={}",
            flow.redirect_uri(),
            flow.state()
        );

        let code = flow
            .parse_callback(&callback)
            .expect("callback should contain valid code");

        assert_eq!(code, "auth-code-123");
    }

    #[test]
    fn parse_callback_without_query_string_returns_invalid_callback() {
        let flow = PkceFlow::try_new().expect("pkce flow");
        let callback = flow.redirect_uri().to_string();

        let err = flow
            .parse_callback(&callback)
            .expect_err("callback without query string should fail");

        assert!(matches!(err, AuthError::InvalidCallback(_)));
    }

    #[test]
    fn parse_callback_with_state_but_no_code_returns_missing_code() {
        let flow = PkceFlow::try_new().expect("pkce flow");
        let callback = format!("{}?state={}", flow.redirect_uri(), flow.state());

        let err = flow
            .parse_callback(&callback)
            .expect_err("callback without code should fail");

        assert_eq!(err, AuthError::MissingCode);
    }

    #[test]
    fn parse_callback_with_code_but_no_state_returns_invalid_state() {
        let flow = PkceFlow::try_new().expect("pkce flow");
        let callback = format!("{}?code=auth-code-123", flow.redirect_uri());

        let err = flow
            .parse_callback(&callback)
            .expect_err("callback without state should fail");

        assert_eq!(err, AuthError::InvalidState);
    }

    #[test]
    fn parse_callback_with_wrong_state_returns_invalid_state() {
        let flow = PkceFlow::try_new().expect("pkce flow");
        let callback = format!(
            "{}?code=auth-code-123&state=different-state",
            flow.redirect_uri()
        );

        let err = flow
            .parse_callback(&callback)
            .expect_err("callback with mismatched state should fail");

        assert_eq!(err, AuthError::InvalidState);
    }

    #[test]
    fn parse_callback_decodes_percent_encoded_values() {
        let flow = PkceFlow::try_new().expect("pkce flow");
        let encoded_state = flow
            .state()
            .bytes()
            .map(|byte| format!("%{byte:02X}"))
            .collect::<String>();
        let callback = format!(
            "{}?code=auth%2Bcode%2F123%3D&state={encoded_state}",
            flow.redirect_uri()
        );

        let code = flow
            .parse_callback(&callback)
            .expect("percent-encoded callback should parse");

        assert_eq!(code, "auth+code/123=");
    }

    #[test]
    fn token_exchange_request_construction_preserves_expected_fields() {
        let flow = PkceFlow::try_new().expect("pkce flow");
        let request = TokenExchangeRequest {
            grant_type: "authorization_code".to_string(),
            code: "oauth-code-xyz".to_string(),
            redirect_uri: flow.redirect_uri().to_string(),
            code_verifier: flow.code_verifier().to_string(),
            client_id: "fawx-cli".to_string(),
        };

        assert_eq!(request.grant_type, "authorization_code");
        assert_eq!(request.code, "oauth-code-xyz");
        assert_eq!(request.redirect_uri, flow.redirect_uri());
        assert_eq!(request.code_verifier, flow.code_verifier());
        assert_eq!(request.client_id, "fawx-cli");
    }

    #[test]
    fn token_response_deserializes_from_json() {
        let json = r#"{
            "access_token": "access-token-value",
            "refresh_token": "refresh-token-value",
            "expires_in": 3600,
            "token_type": "Bearer"
        }"#;

        let parsed: TokenResponse = serde_json::from_str(json).expect("valid token response json");

        assert_eq!(parsed.expires_in, 3600);
        assert_eq!(parsed.token_type, "Bearer");
        assert_eq!(parsed.access_token, "access-token-value");
        assert_eq!(parsed.refresh_token, "refresh-token-value");
    }

    #[test]
    fn auth_error_display_strings_are_stable() {
        assert_eq!(
            AuthError::InvalidState.to_string(),
            "oauth state validation failed"
        );
        assert_eq!(
            AuthError::MissingCode.to_string(),
            "oauth callback missing authorization code"
        );
        assert_eq!(
            AuthError::ExchangeFailed("boom".to_string()).to_string(),
            "oauth token exchange failed: boom"
        );
        assert_eq!(
            AuthError::RefreshFailed("expired".to_string()).to_string(),
            "oauth token refresh failed: expired"
        );
        assert_eq!(
            AuthError::InvalidCallback("bad url".to_string()).to_string(),
            "invalid oauth callback: bad url"
        );
    }

    #[test]
    fn debug_impls_redact_secret_fields() {
        let flow = PkceFlow::try_new().expect("pkce flow");
        let flow_debug = format!("{flow:?}");
        assert!(flow_debug.contains("<redacted>"));
        assert!(!flow_debug.contains(flow.code_verifier()));

        let exchange = TokenExchangeRequest {
            grant_type: "authorization_code".to_string(),
            code: "auth-code".to_string(),
            redirect_uri: flow.redirect_uri().to_string(),
            code_verifier: flow.code_verifier().to_string(),
            client_id: "client-id".to_string(),
        };
        let exchange_debug = format!("{exchange:?}");
        assert!(exchange_debug.contains("<redacted>"));
        assert!(!exchange_debug.contains("auth-code"));

        let refresh = TokenRefreshRequest {
            grant_type: "refresh_token".to_string(),
            refresh_token: "refresh-secret".to_string(),
            client_id: "client-id".to_string(),
        };
        let refresh_debug = format!("{refresh:?}");
        assert!(refresh_debug.contains("<redacted>"));
        assert!(!refresh_debug.contains("refresh-secret"));

        let response = TokenResponse {
            access_token: "access-secret".to_string(),
            refresh_token: "refresh-secret".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
        };
        let response_debug = format!("{response:?}");
        assert!(response_debug.contains("<redacted>"));
        assert!(!response_debug.contains("access-secret"));
        assert!(!response_debug.contains("refresh-secret"));
    }

    #[test]
    fn random_bytes_returns_error_when_rng_fails() {
        let result = random_bytes_with_fill::<8>(|_| {
            Err(AuthError::RandomGenerationFailed("rng failed".to_string()))
        });

        assert!(matches!(result, Err(AuthError::RandomGenerationFailed(_))));
    }

    #[test]
    fn extract_openai_account_id_returns_expected_claim() {
        let payload = serde_json::json!({
            OPENAI_JWT_CLAIM_PATH: {"chatgpt_account_id": "acct_123"}
        });
        let payload_json = serde_json::to_vec(&payload).expect("payload json");
        let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json);
        let token = format!("header.{payload_b64}.sig");

        let account = extract_openai_account_id(&token);
        assert_eq!(account.as_deref(), Some("acct_123"));
    }

    #[test]
    fn extract_openai_account_id_returns_none_for_invalid_jwt() {
        assert_eq!(extract_openai_account_id("not-a-jwt"), None);

        let invalid_payload_token = "header.not-base64.sig";
        assert_eq!(extract_openai_account_id(invalid_payload_token), None);
    }

    #[test]
    fn extract_openai_account_id_returns_none_when_claim_missing() {
        let payload = serde_json::json!({
            OPENAI_JWT_CLAIM_PATH: {"other": "value"}
        });
        let payload_json = serde_json::to_vec(&payload).expect("payload json");
        let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json);
        let token = format!("header.{payload_b64}.sig");

        assert_eq!(extract_openai_account_id(&token), None);
    }
}
