//! Host API trait and implementations for WASM skills.

use nv_core::error::SkillError;
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

    /// Downcast to concrete type for accessing implementation-specific methods.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Mock implementation of HostApi for testing.
#[derive(Debug, Clone)]
pub struct MockHostApi {
    storage: Arc<Mutex<HashMap<String, String>>>,
    input: String,
    output: Arc<Mutex<String>>,
    logs: Arc<Mutex<Vec<(u32, String)>>>,
}

impl MockHostApi {
    /// Create a new mock host API with the given input.
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            input: input.into(),
            output: Arc::new(Mutex::new(String::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get the output that was set by the skill.
    pub fn get_output(&self) -> String {
        self.output.lock().expect("Lock poisoned").clone()
    }

    /// Get all logged messages.
    pub fn get_logs(&self) -> Vec<(u32, String)> {
        self.logs.lock().expect("Lock poisoned").clone()
    }

    /// Get the current storage state.
    pub fn get_storage(&self) -> HashMap<String, String> {
        self.storage.lock().expect("Lock poisoned").clone()
    }
}

impl HostApi for MockHostApi {
    fn log(&self, level: u32, message: &str) {
        self.logs
            .lock()
            .expect("Lock poisoned")
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
        self.storage
            .lock()
            .expect("Lock poisoned")
            .get(key)
            .cloned()
    }

    fn kv_set(&mut self, key: &str, value: &str) -> Result<(), SkillError> {
        self.storage
            .lock()
            .expect("Lock poisoned")
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get_input(&self) -> String {
        self.input.clone()
    }

    fn set_output(&mut self, text: &str) {
        *self.output.lock().expect("Lock poisoned") = text.to_string();
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
}
