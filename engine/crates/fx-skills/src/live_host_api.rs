//! Live implementation of HostApi with real HTTP support.

use crate::host_api::{HostApi, HostApiBase};
use crate::manifest::Capability;
use fx_core::error::SkillError;
use std::io::Read;
use std::sync::Arc;
use zeroize::Zeroizing;

/// Maximum response body size: 1 MB.
const MAX_RESPONSE_BYTES: u64 = 1_048_576;

/// HTTP request timeout in seconds.
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Trait for providing credentials to WASM skills via `kv_get`.
///
/// This is a bridge trait so `fx-skills` doesn't depend on `fx-auth` directly.
/// The TUI initialization implements this trait using the real credential store.
pub trait CredentialProvider: Send + Sync {
    /// Get a credential by key name (e.g., "github_token").
    ///
    /// Returns the credential wrapped in [`Zeroizing`] so the secret
    /// is automatically zeroed on drop, preventing leaks to the allocator.
    fn get_credential(&self, key: &str) -> Option<Zeroizing<String>>;
}

/// Configuration for creating a LiveHostApi instance.
pub struct LiveHostApiConfig {
    /// Input text for the skill.
    pub input: String,
    /// Capabilities granted to the skill.
    pub capabilities: Vec<Capability>,
    /// Optional credential provider for bridging secrets to skills.
    pub credential_provider: Option<Arc<dyn CredentialProvider>>,
}

/// Live implementation of HostApi that makes real HTTP requests.
pub struct LiveHostApi {
    base: HostApiBase,
    capabilities: Vec<Capability>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
}

impl LiveHostApi {
    /// Create a new LiveHostApi from configuration.
    pub fn new(config: LiveHostApiConfig) -> Self {
        Self {
            base: HostApiBase::new(config.input),
            capabilities: config.capabilities,
            credential_provider: config.credential_provider,
        }
    }

    /// Get the output that was set by the skill.
    pub fn get_output(&self) -> String {
        self.base.get_output()
    }

    /// Check if the skill has a given capability.
    fn has_capability(&self, cap: &Capability) -> bool {
        self.capabilities.contains(cap)
    }
}

/// Parse a JSON string of headers into key-value pairs.
///
/// Expected format: `{"Header-Name": "value", ...}`
/// Returns None if the JSON is invalid or not an object.
fn parse_headers(headers_json: &str) -> Option<Vec<(String, String)>> {
    let parsed: serde_json::Value = serde_json::from_str(headers_json).ok()?;
    let obj = parsed.as_object()?;
    let mut result = Vec::with_capacity(obj.len());
    for (key, value) in obj {
        let val_str = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result.push((key.clone(), val_str));
    }
    Some(result)
}

/// Execute an HTTP request via ureq.
///
/// Enforces HTTPS-only, 30s timeout, and 1MB response limit.
fn execute_http_request(method: &str, url: &str, headers: &str, body: &str) -> Option<String> {
    // HTTPS only
    if !url.starts_with("https://") {
        tracing::error!("http_request denied: URL must use HTTPS, got: {}", url);
        return None;
    }

    tracing::info!("http_request: {} {}", method, url);

    // Parse headers
    let header_pairs = if headers.is_empty() || headers == "{}" {
        Vec::new()
    } else {
        match parse_headers(headers) {
            Some(h) => h,
            None => {
                tracing::error!("http_request: invalid headers JSON: {}", headers);
                return None;
            }
        }
    };

    // Build agent with timeout
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build();

    // Build request
    let mut request = match method.to_uppercase().as_str() {
        "GET" => agent.get(url),
        "POST" => agent.post(url),
        "PUT" => agent.put(url),
        "DELETE" => agent.delete(url),
        "PATCH" => agent.request("PATCH", url),
        "HEAD" => agent.head(url),
        other => {
            tracing::error!("http_request: unsupported method: {}", other);
            return None;
        }
    };

    // Apply headers
    for (key, value) in &header_pairs {
        request = request.set(key, value);
    }

    // Send request
    let response = if body.is_empty() {
        request.call()
    } else {
        request.send_string(body)
    };

    match response {
        Ok(resp) => read_response_body(resp),
        Err(ureq::Error::Status(status, resp)) => {
            tracing::warn!("http_request: HTTP {} for {}", status, url);
            read_response_body(resp)
        }
        Err(e) => {
            tracing::error!("http_request failed for {}: {}", url, e);
            None
        }
    }
}

/// Read response body with size limit enforcement.
fn read_response_body(response: ureq::Response) -> Option<String> {
    let reader = response.into_reader().take(MAX_RESPONSE_BYTES);
    read_limited_body(reader)
}

/// Read a response body from a size-limited reader.
///
/// Returns `None` if the body is not valid UTF-8 or on read errors.
/// The caller is responsible for applying `.take(MAX_RESPONSE_BYTES)` to
/// enforce the size limit before passing the reader here.
fn read_limited_body(mut reader: impl Read) -> Option<String> {
    let mut body = String::new();
    match reader.read_to_string(&mut body) {
        Ok(_) => Some(body),
        Err(e) => {
            tracing::error!("http_request: failed to read response body: {}", e);
            None
        }
    }
}

impl HostApi for LiveHostApi {
    fn log(&self, level: u32, message: &str) {
        match level {
            0 => tracing::trace!("[skill] {}", message),
            1 => tracing::debug!("[skill] {}", message),
            2 => tracing::info!("[skill] {}", message),
            3 => tracing::warn!("[skill] {}", message),
            4 => tracing::error!("[skill] {}", message),
            _ => tracing::info!("[skill] (level {}) {}", level, message),
        }
    }

    fn kv_get(&self, key: &str) -> Option<String> {
        // Bridge: credential provider keys take priority.
        // Note: the Zeroizing wrapper is consumed here because kv_get
        // returns a plain String for the WASM ABI boundary. The secret
        // is only exposed for the duration of the skill invocation.
        if let Some(provider) = &self.credential_provider {
            if let Some(value) = provider.get_credential(key) {
                return Some((*value).clone());
            }
        }
        self.base.kv_get(key)
    }

    fn kv_set(&mut self, key: &str, value: &str) -> Result<(), SkillError> {
        self.base.kv_set(key, value);
        Ok(())
    }

    fn get_input(&self) -> String {
        self.base.get_input()
    }

    fn set_output(&mut self, text: &str) {
        self.base.set_output(text);
    }

    fn http_request(&self, method: &str, url: &str, headers: &str, body: &str) -> Option<String> {
        // Defense-in-depth: the WASM linker also gates on Network capability,
        // but we check here for direct callers that bypass the linker layer.
        if !self.has_capability(&Capability::Network) {
            tracing::error!("http_request denied: skill lacks Network capability");
            return None;
        }

        execute_http_request(method, url, headers, body)
    }

    fn get_output(&self) -> String {
        self.base.get_output()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_config(capabilities: Vec<Capability>) -> LiveHostApiConfig {
        LiveHostApiConfig {
            input: "test".to_string(),
            capabilities,
            credential_provider: None,
        }
    }

    #[test]
    fn capability_gating_no_network_returns_none() {
        let api = LiveHostApi::new(make_config(vec![Capability::Storage]));
        let result = api.http_request("GET", "https://example.com", "{}", "");
        assert_eq!(result, None);
    }

    #[test]
    fn https_only_rejects_http_urls() {
        let api = LiveHostApi::new(make_config(vec![Capability::Network]));
        let result = api.http_request("GET", "http://example.com", "{}", "");
        assert_eq!(result, None);
    }

    #[test]
    fn https_only_rejects_ftp_urls() {
        let api = LiveHostApi::new(make_config(vec![Capability::Network]));
        let result = api.http_request("GET", "ftp://example.com/file", "{}", "");
        assert_eq!(result, None);
    }

    #[test]
    fn parse_headers_valid_json() {
        let headers = r#"{"Content-Type": "application/json", "X-Api-Key": "abc123"}"#;
        let parsed = parse_headers(headers).expect("Should parse valid JSON");
        assert_eq!(parsed.len(), 2);

        let map: HashMap<String, String> = parsed.into_iter().collect();
        assert_eq!(map.get("Content-Type").unwrap(), "application/json");
        assert_eq!(map.get("X-Api-Key").unwrap(), "abc123");
    }

    #[test]
    fn parse_headers_empty_object() {
        let parsed = parse_headers("{}").expect("Should parse empty object");
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_headers_invalid_json_returns_none() {
        assert!(parse_headers("not json").is_none());
    }

    #[test]
    fn parse_headers_array_returns_none() {
        assert!(parse_headers("[1, 2, 3]").is_none());
    }

    #[test]
    fn parse_headers_non_string_values_converted() {
        let headers = r#"{"X-Count": 42}"#;
        let parsed = parse_headers(headers).expect("Should parse");
        assert_eq!(parsed[0].1, "42");
    }

    #[test]
    fn live_host_api_basic_io() {
        let mut api = LiveHostApi::new(make_config(vec![]));
        assert_eq!(api.get_input(), "test");

        api.set_output("hello");
        assert_eq!(api.get_output(), "hello");
    }

    #[test]
    fn live_host_api_kv_storage() {
        let mut api = LiveHostApi::new(make_config(vec![]));
        assert_eq!(api.kv_get("k"), None);

        api.kv_set("k", "v").expect("Should set");
        assert_eq!(api.kv_get("k"), Some("v".to_string()));
    }

    #[test]
    fn unsupported_method_returns_none() {
        // execute_http_request rejects unknown methods
        let result = execute_http_request("CONNECT", "https://example.com", "{}", "");
        assert_eq!(result, None);
    }

    #[test]
    fn read_limited_body_under_limit() {
        let data = "hello world";
        let reader = std::io::Cursor::new(data);
        let result = read_limited_body(reader);
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn read_limited_body_truncates_at_1mb() {
        // Create a reader with 2MB of data, limited to 1MB via take()
        let data = vec![b'a'; 2 * 1024 * 1024]; // 2 MB
        let reader = std::io::Cursor::new(data).take(MAX_RESPONSE_BYTES);
        let result = read_limited_body(reader);
        let body = result.expect("Should read successfully");
        assert_eq!(
            body.len() as u64,
            MAX_RESPONSE_BYTES,
            "Response body should be truncated to exactly 1MB"
        );
        assert!(body.chars().all(|c| c == 'a'));
    }

    #[test]
    fn read_limited_body_exactly_1mb() {
        let data = vec![b'x'; MAX_RESPONSE_BYTES as usize];
        let reader = std::io::Cursor::new(data).take(MAX_RESPONSE_BYTES);
        let result = read_limited_body(reader);
        let body = result.expect("Should read successfully");
        assert_eq!(body.len() as u64, MAX_RESPONSE_BYTES);
    }

    #[test]
    fn read_limited_body_invalid_utf8_returns_none() {
        let data: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x01];
        let reader = std::io::Cursor::new(data);
        let result = read_limited_body(reader);
        assert_eq!(result, None, "Invalid UTF-8 should return None");
    }

    /// Mock credential provider for testing the KV bridge.
    struct MockCredentialProvider {
        credentials: std::collections::HashMap<String, String>,
    }

    impl MockCredentialProvider {
        fn new() -> Self {
            Self {
                credentials: std::collections::HashMap::new(),
            }
        }

        fn with_credential(mut self, key: &str, value: &str) -> Self {
            self.credentials.insert(key.to_string(), value.to_string());
            self
        }
    }

    impl CredentialProvider for MockCredentialProvider {
        fn get_credential(&self, key: &str) -> Option<Zeroizing<String>> {
            self.credentials.get(key).map(|v| Zeroizing::new(v.clone()))
        }
    }

    #[test]
    fn kv_get_bridges_github_token_from_credential_provider() {
        let provider =
            MockCredentialProvider::new().with_credential("github_token", "ghp_test_token_12345");

        let config = LiveHostApiConfig {
            input: "test".to_string(),
            capabilities: vec![],
            credential_provider: Some(Arc::new(provider)),
        };
        let api = LiveHostApi::new(config);

        assert_eq!(
            api.kv_get("github_token"),
            Some("ghp_test_token_12345".to_string())
        );
    }

    #[test]
    fn kv_get_returns_none_for_unknown_credential_key() {
        let provider = MockCredentialProvider::new().with_credential("github_token", "ghp_test");

        let config = LiveHostApiConfig {
            input: "test".to_string(),
            capabilities: vec![],
            credential_provider: Some(Arc::new(provider)),
        };
        let api = LiveHostApi::new(config);

        assert_eq!(api.kv_get("other_key"), None);
    }

    #[test]
    fn kv_get_falls_through_without_credential_provider() {
        let mut api = LiveHostApi::new(make_config(vec![]));
        api.kv_set("regular_key", "regular_value").expect("set");
        assert_eq!(api.kv_get("regular_key"), Some("regular_value".to_string()));
        assert_eq!(api.kv_get("github_token"), None);
    }

    #[test]
    fn credential_provider_takes_priority_over_base_kv() {
        let provider =
            MockCredentialProvider::new().with_credential("github_token", "from_credential_store");

        let config = LiveHostApiConfig {
            input: "test".to_string(),
            capabilities: vec![],
            credential_provider: Some(Arc::new(provider)),
        };
        let mut api = LiveHostApi::new(config);

        // Set a different value in the base KV
        api.kv_set("github_token", "from_base_kv").expect("set");

        // Credential provider should take priority
        assert_eq!(
            api.kv_get("github_token"),
            Some("from_credential_store".to_string())
        );
    }
}
