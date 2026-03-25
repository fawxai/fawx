//! Host API trait and implementations for WASM skills.

use fx_core::error::SkillError;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Host API functions exposed to WASM skills.
///
/// This trait defines the host_api_v1 contract.
pub trait HostApi: Send + Sync {
    /// Log a message at a specific level.
    ///
    /// Levels: 0=trace, 1=debug, 2=info, 3=warn, 4=error
    fn log(&self, level: u32, message: &str);

    /// Get a value from skill's key-value storage.
    fn kv_get(&self, key: &str) -> Option<String>;

    /// Set a value in skill's key-value storage.
    fn kv_set(&mut self, key: &str, value: &str) -> Result<(), SkillError>;

    /// Get the user's input text.
    fn get_input(&self) -> String;

    /// Set the skill's response output.
    fn set_output(&mut self, text: &str);

    /// Make an HTTP request. Returns the response body, or None on failure.
    ///
    /// - `method`: HTTP method (GET, POST, etc.)
    /// - `url`: Target URL (HTTPS only in live implementations)
    /// - `headers`: JSON-encoded header map (e.g., `{"Content-Type": "application/json"}`)
    /// - `body`: Request body (empty string for no body)
    fn http_request(&self, method: &str, url: &str, headers: &str, body: &str) -> Option<String>;

    /// Execute a shell command. Returns JSON: {"stdout": "...", "stderr": "...", "exit_code": N}
    /// Returns None if Shell capability is not granted.
    fn exec_command(&self, command: &str, timeout_ms: u32) -> Option<String> {
        let _ = (command, timeout_ms);
        None
    }

    /// Read a file's contents as UTF-8. Returns None if Filesystem capability is not granted.
    fn read_file(&self, path: &str) -> Option<String> {
        let _ = path;
        None
    }

    /// Write content to a file. Returns true on success.
    fn write_file(&self, path: &str, content: &str) -> bool {
        let _ = (path, content);
        false
    }

    /// Get the output that was set by the skill.
    fn get_output(&self) -> String;

    /// Downcast to concrete type for accessing implementation-specific methods.
    fn as_any(&self) -> &dyn std::any::Any;

    // === host_api_v2 additions ===

    /// Get metadata about the current execution context.
    fn get_context(&self) -> String {
        "{}".to_string()
    }

    /// Register this skill as the message handler for a channel.
    fn register_channel(
        &mut self,
        _channel_id: &str,
        _display_name: &str,
    ) -> Result<(), SkillError> {
        Err(SkillError::Unsupported(
            "register_channel requires host_api_v2".into(),
        ))
    }

    /// Emit an event for other skills or the orchestrator.
    fn emit_event(&mut self, _event_type: &str, _payload: &str) -> Result<(), SkillError> {
        Err(SkillError::Unsupported(
            "emit_event requires host_api_v2".into(),
        ))
    }

    /// Send a response to a specific channel.
    fn send_to_channel(&self, _channel_id: &str, _message: &str) -> Result<(), SkillError> {
        Err(SkillError::Unsupported(
            "send_to_channel requires host_api_v2".into(),
        ))
    }
}

/// Shared base for HostApi implementations.
///
/// Provides common storage, input/output, and lock-recovery patterns
/// used by both `MockHostApi` (testing) and `LiveHostApi` (production).
#[derive(Debug, Clone)]
pub struct HostApiBase {
    storage: Arc<Mutex<HashMap<String, String>>>,
    input: String,
    output: Arc<Mutex<String>>,
}

impl HostApiBase {
    /// Create a new base with the given input.
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            input: input.into(),
            output: Arc::new(Mutex::new(String::new())),
        }
    }

    /// Get a value from key-value storage.
    pub fn kv_get(&self, key: &str) -> Option<String> {
        self.storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned()
    }

    /// Set a value in key-value storage.
    pub fn kv_set(&self, key: &str, value: &str) {
        self.storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key.to_string(), value.to_string());
    }

    /// Get the input text.
    pub fn get_input(&self) -> String {
        self.input.clone()
    }

    /// Set the output text.
    pub fn set_output(&self, text: &str) {
        *self
            .output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = text.to_string();
    }

    /// Get the current output text.
    pub fn get_output(&self) -> String {
        self.output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Get the current storage state (for testing/inspection).
    pub fn get_storage(&self) -> HashMap<String, String> {
        self.storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// Mock implementation of HostApi for testing.
#[derive(Debug, Clone)]
pub struct MockHostApi {
    base: HostApiBase,
    logs: Arc<Mutex<Vec<(u32, String)>>>,
    /// Canned HTTP responses: maps URL to response body.
    http_responses: Arc<Mutex<HashMap<String, String>>>,
}

impl MockHostApi {
    /// Create a new mock host API with the given input.
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            base: HostApiBase::new(input),
            logs: Arc::new(Mutex::new(Vec::new())),
            http_responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a canned HTTP response for a URL.
    ///
    /// When `http_request` is called with this URL, the canned response is returned.
    pub fn add_http_response(&self, url: impl Into<String>, response: impl Into<String>) {
        self.http_responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(url.into(), response.into());
    }

    /// Get the output that was set by the skill.
    pub fn get_output(&self) -> String {
        self.base.get_output()
    }

    /// Get all logged messages.
    pub fn get_logs(&self) -> Vec<(u32, String)> {
        self.logs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Get the current storage state.
    pub fn get_storage(&self) -> HashMap<String, String> {
        self.base.get_storage()
    }
}

impl HostApi for MockHostApi {
    fn log(&self, level: u32, message: &str) {
        self.logs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((level, message.to_string()));

        match level {
            0 => tracing::trace!("{}", message),
            1 => tracing::debug!("{}", message),
            2 => tracing::info!("{}", message),
            3 => tracing::warn!("{}", message),
            4 => tracing::error!("{}", message),
            _ => tracing::info!("Unknown level {}: {}", level, message),
        }
    }

    fn kv_get(&self, key: &str) -> Option<String> {
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

    fn http_request(
        &self,
        _method: &str,
        url: &str,
        _headers: &str,
        _body: &str,
    ) -> Option<String> {
        self.http_responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(url)
            .cloned()
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

    #[test]
    fn test_mock_host_api_input_output() {
        let mut api = MockHostApi::new("test input");
        assert_eq!(api.get_input(), "test input");

        api.set_output("test output");
        assert_eq!(api.get_output(), "test output");
    }

    #[test]
    fn test_mock_host_api_kv_storage() {
        let mut api = MockHostApi::new("");

        assert_eq!(api.kv_get("key1"), None);

        api.kv_set("key1", "value1").expect("Should set");
        assert_eq!(api.kv_get("key1"), Some("value1".to_string()));

        api.kv_set("key1", "value2").expect("Should update");
        assert_eq!(api.kv_get("key1"), Some("value2".to_string()));
    }

    #[test]
    fn test_mock_host_api_logging() {
        let api = MockHostApi::new("");

        api.log(0, "trace message");
        api.log(2, "info message");
        api.log(4, "error message");

        let logs = api.get_logs();
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0], (0, "trace message".to_string()));
        assert_eq!(logs[1], (2, "info message".to_string()));
        assert_eq!(logs[2], (4, "error message".to_string()));
    }

    #[test]
    fn test_mock_host_api_empty_input() {
        let api = MockHostApi::new("");
        assert_eq!(api.get_input(), "");
    }

    #[test]
    fn test_mock_host_api_empty_value() {
        let mut api = MockHostApi::new("");
        api.kv_set("key", "").expect("Should set empty value");
        assert_eq!(api.kv_get("key"), Some("".to_string()));
    }

    #[test]
    fn test_mock_host_api_http_request_canned_response() {
        let api = MockHostApi::new("");
        api.add_http_response("https://api.example.com/data", r#"{"result": "ok"}"#);

        let response = api.http_request("GET", "https://api.example.com/data", "{}", "");
        assert_eq!(response, Some(r#"{"result": "ok"}"#.to_string()));
    }

    #[test]
    fn test_mock_host_api_http_request_unmatched_url() {
        let api = MockHostApi::new("");
        api.add_http_response("https://api.example.com/data", "response");

        let response = api.http_request("GET", "https://api.example.com/other", "{}", "");
        assert_eq!(response, None);
    }

    #[test]
    fn test_mock_host_api_http_request_no_canned_responses() {
        let api = MockHostApi::new("");

        let response = api.http_request("GET", "https://example.com", "{}", "");
        assert_eq!(response, None);
    }

    #[test]
    fn test_host_api_base_kv_operations() {
        let base = HostApiBase::new("input");
        assert_eq!(base.kv_get("missing"), None);

        base.kv_set("k", "v");
        assert_eq!(base.kv_get("k"), Some("v".to_string()));

        base.kv_set("k", "v2");
        assert_eq!(base.kv_get("k"), Some("v2".to_string()));
    }

    #[test]
    fn register_channel_v1_fails() {
        let mut api = MockHostApi::new("test");
        let result = api.register_channel("telegram", "Telegram");
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Unsupported(_))));
    }

    #[test]
    fn send_to_channel_v1_fails() {
        let api = MockHostApi::new("test");
        let result = api.send_to_channel("telegram", "hello");
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Unsupported(_))));
    }

    #[test]
    fn emit_event_v1_fails() {
        let mut api = MockHostApi::new("test");
        let result = api.emit_event("some.event", r#"{"key":"value"}"#);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::Unsupported(_))));
    }

    #[test]
    fn get_context_returns_json() {
        let api = MockHostApi::new("test");
        let ctx = api.get_context();
        assert_eq!(ctx, "{}");
        let parsed: serde_json::Value = serde_json::from_str(&ctx).expect("valid JSON");
        assert!(parsed.is_object());
    }

    #[test]
    fn test_host_api_base_io() {
        let base = HostApiBase::new("hello");
        assert_eq!(base.get_input(), "hello");
        assert_eq!(base.get_output(), "");

        base.set_output("world");
        assert_eq!(base.get_output(), "world");
    }
}
