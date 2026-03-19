//! Live implementation of HostApi with real HTTP support.

use crate::host_api::{HostApi, HostApiBase};
use crate::manifest::Capability;
use fx_core::error::SkillError;
use std::io::Read;
use std::sync::Arc;
use zeroize::Zeroizing;

/// Maximum response body size: 1 MB.
const MAX_RESPONSE_BYTES: u64 = 1_048_576;

/// Prefix for binary HTTP responses passed through the string-only WASM ABI.
///
/// Skills detect this sentinel and decode the remaining base64 back into the
/// original bytes. Collision with a real text response is extremely unlikely
/// because this prefix is not valid JSON, HTML, or any known API response
/// format.
// COUPLING: This sentinel must match the one in skills/tts-skill/src/lib.rs
// and skills/stt-skill/src/lib.rs (and any skill that handles binary HTTP
// responses).
const HOST_BINARY_BASE64_PREFIX: &str = "__fawx_binary_base64__:";
/// Prefix for binary HTTP request bodies passed over the string-only WASM ABI.
///
/// Skills encode raw request bytes as base64 with this sentinel so the host can
/// reconstruct multipart/form-data or other non-UTF-8 payloads before sending.
// COUPLING: This sentinel must match the one in skills/stt-skill/src/lib.rs.
const HOST_REQUEST_BINARY_BASE64_PREFIX: &str = "__fawx_request_binary_base64__:";

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

enum RequestBody<'a> {
    Empty,
    Text(&'a str),
    Binary(Vec<u8>),
}

fn prepare_request_body(body: &str) -> RequestBody<'_> {
    if body.is_empty() {
        return RequestBody::Empty;
    }

    match body.strip_prefix(HOST_REQUEST_BINARY_BASE64_PREFIX) {
        Some(encoded) => match base64_decode(encoded) {
            Some(bytes) => RequestBody::Binary(bytes),
            None => RequestBody::Text(body),
        },
        None => RequestBody::Text(body),
    }
}

/// Execute an HTTP request via ureq.
///
/// Enforces HTTPS-only, 30s timeout, and 1MB response limit.
pub fn execute_http_request(method: &str, url: &str, headers: &str, body: &str) -> Option<String> {
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
    let response = match prepare_request_body(body) {
        RequestBody::Empty => request.call(),
        RequestBody::Text(text) => request.send_string(text),
        RequestBody::Binary(bytes) => request.send_bytes(&bytes),
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
/// Text responses are passed through unchanged. Binary responses are encoded as
/// base64 with a sentinel prefix so skills can recover the original bytes over
/// the string-only WASM host ABI.
fn read_limited_body(mut reader: impl Read) -> Option<String> {
    let mut body = Vec::new();
    if let Err(error) = reader.read_to_end(&mut body) {
        tracing::error!("http_request: failed to read response body: {}", error);
        return None;
    }

    Some(encode_http_response_body(&body))
}

fn encode_http_response_body(body: &[u8]) -> String {
    if body.contains(&0) {
        return format!("{HOST_BINARY_BASE64_PREFIX}{}", base64_encode(body));
    }

    match String::from_utf8(body.to_vec()) {
        Ok(text) => text,
        Err(error) => {
            let bytes = error.into_bytes();
            format!("{HOST_BINARY_BASE64_PREFIX}{}", base64_encode(&bytes))
        }
    }
}

// COUPLING: This encoder must match the one in skills/tts-skill/src/lib.rs.
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let combined = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;

        output.push(TABLE[((combined >> 18) & 0x3F) as usize] as char);
        output.push(TABLE[((combined >> 12) & 0x3F) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((combined >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(combined & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    output
}

// COUPLING: This decoder powers the binary request-body path and must stay
// compatible with the request-body encoder in skills/stt-skill/src/lib.rs.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();

    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }

    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        decode_base64_chunk(chunk, &mut output)?;
    }
    Some(output)
}

fn decode_base64_chunk(chunk: &[u8], output: &mut Vec<u8>) -> Option<()> {
    let v0 = decode_base64_value(chunk[0])?;
    let v1 = decode_base64_value(chunk[1])?;
    let v2 = decode_optional_base64_value(chunk[2])?;
    let v3 = decode_optional_base64_value(chunk[3])?;
    let combined = ((v0 as u32) << 18)
        | ((v1 as u32) << 12)
        | ((v2.unwrap_or(0) as u32) << 6)
        | v3.unwrap_or(0) as u32;

    output.push(((combined >> 16) & 0xFF) as u8);
    if v2.is_some() {
        output.push(((combined >> 8) & 0xFF) as u8);
    }
    if v3.is_some() {
        output.push((combined & 0xFF) as u8);
    }
    Some(())
}

fn decode_optional_base64_value(byte: u8) -> Option<Option<u8>> {
    if byte == b'=' {
        return Some(None);
    }
    decode_base64_value(byte).map(Some)
}

fn decode_base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
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
    fn prepare_request_body_decodes_binary_prefix() {
        let payload = prepare_request_body("__fawx_request_binary_base64__:AQID");

        match payload {
            RequestBody::Binary(bytes) => assert_eq!(bytes, vec![1, 2, 3]),
            _ => panic!("expected binary payload"),
        }
    }

    #[test]
    fn prepare_request_body_preserves_invalid_prefixed_text() {
        let payload = prepare_request_body("__fawx_request_binary_base64__:%%%");

        match payload {
            RequestBody::Text(body) => {
                assert_eq!(body, "__fawx_request_binary_base64__:%%%");
            }
            _ => panic!("expected text payload"),
        }
    }

    #[test]
    fn prepare_request_body_empty_returns_empty() {
        let payload = prepare_request_body("");
        assert!(matches!(payload, RequestBody::Empty));
    }

    #[test]
    fn prepare_request_body_plain_text_passes_through() {
        let payload = prepare_request_body(r#"{"key":"value"}"#);

        match payload {
            RequestBody::Text(body) => assert_eq!(body, r#"{"key":"value"}"#),
            _ => panic!("expected text payload"),
        }
    }

    #[test]
    fn read_limited_body_under_limit() {
        let data = "hello world";
        let reader = std::io::Cursor::new(data);
        let result = read_limited_body(reader);
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn read_limited_body_empty_returns_empty_string() {
        let reader = std::io::Cursor::new(Vec::<u8>::new());
        let result = read_limited_body(reader);
        assert_eq!(result, Some(String::new()));
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
    fn read_limited_body_invalid_utf8_returns_binary_base64() {
        let data: Vec<u8> = vec![0xFF, 0xFE, 0x01];
        let reader = std::io::Cursor::new(data);
        let result = read_limited_body(reader).expect("binary response should encode");
        assert_eq!(result, format!("{HOST_BINARY_BASE64_PREFIX}//4B"));
    }

    #[test]
    fn read_limited_body_with_nul_byte_returns_binary_base64() {
        let data: Vec<u8> = vec![b'O', 0, b'K'];
        let reader = std::io::Cursor::new(data);
        let result = read_limited_body(reader).expect("binary response should encode");
        assert_eq!(result, format!("{HOST_BINARY_BASE64_PREFIX}TwBL"));
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
