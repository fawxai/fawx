//! Live host API implementation for WASM skills running in the kernel.
//!
//! Provides a real [`HostApi`] that routes WASM host calls to the appropriate
//! runtime services (tracing for logs, [`SkillStorage`] for key-value ops).

use fx_core::error::SkillError;
use fx_skills::host_api::HostApi;
use fx_skills::storage::SkillStorage;
use std::sync::{Arc, Mutex};

/// Default storage quota per skill: 64 KiB.
const DEFAULT_STORAGE_QUOTA: usize = 64 * 1024;

/// Live host API backed by real runtime services.
///
/// Routes WASM host function calls to:
/// - `tracing` for logging
/// - [`SkillStorage`] for key-value persistence
/// - Input/output buffers for skill invocation I/O
#[derive(Debug)]
pub struct LiveHostApi {
    storage: Arc<Mutex<SkillStorage>>,
    input: String,
    output: Arc<Mutex<String>>,
}

/// Configuration for creating a [`LiveHostApi`].
pub struct LiveHostApiConfig<'a> {
    /// Skill name (used for storage isolation).
    pub skill_name: &'a str,
    /// Input JSON string for the skill invocation.
    pub input: String,
    /// Storage quota in bytes (defaults to [`DEFAULT_STORAGE_QUOTA`]).
    pub storage_quota: Option<usize>,
}

impl LiveHostApi {
    /// Create a new live host API for a skill invocation.
    pub fn new(config: LiveHostApiConfig<'_>) -> Self {
        let quota = config.storage_quota.unwrap_or(DEFAULT_STORAGE_QUOTA);
        Self {
            storage: Arc::new(Mutex::new(SkillStorage::new(config.skill_name, quota))),
            input: config.input,
            output: Arc::new(Mutex::new(String::new())),
        }
    }

    /// Extract and drain the output set by the WASM skill.
    ///
    /// Uses `std::mem::take` to move the string out of the mutex,
    /// leaving an empty string behind.
    pub fn take_output(&self) -> String {
        std::mem::take(
            &mut *self
                .output
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }
}

impl HostApi for LiveHostApi {
    fn log(&self, level: u32, message: &str) {
        match level {
            0 => tracing::trace!(target: "wasm_skill", "{}", message),
            1 => tracing::debug!(target: "wasm_skill", "{}", message),
            2 => tracing::info!(target: "wasm_skill", "{}", message),
            3 => tracing::warn!(target: "wasm_skill", "{}", message),
            4 => tracing::error!(target: "wasm_skill", "{}", message),
            _ => tracing::info!(target: "wasm_skill", "level={}: {}", level, message),
        }
    }

    fn kv_get(&self, key: &str) -> Option<String> {
        self.storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
    }

    fn kv_set(&mut self, key: &str, value: &str) -> Result<(), SkillError> {
        self.storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set(key, value)
    }

    fn get_input(&self) -> String {
        self.input.clone()
    }

    fn set_output(&mut self, text: &str) {
        *self
            .output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = text.to_string();
    }

    fn http_request(
        &self,
        _method: &str,
        _url: &str,
        _headers: &str,
        _body: &str,
    ) -> Option<String> {
        // HTTP requests are not yet supported in the loadable LiveHostApi.
        // The WASM linker layer gates on Network capability and this will
        // be wired to a real HTTP client when needed.
        tracing::warn!("http_request called on loadable LiveHostApi (not yet implemented)");
        None
    }

    fn get_output(&self) -> String {
        self.output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_api(input: &str) -> LiveHostApi {
        LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test_skill",
            input: input.to_string(),
            storage_quota: None,
        })
    }

    #[test]
    fn input_output_round_trip() {
        let mut api = make_api("hello world");
        assert_eq!(api.get_input(), "hello world");

        api.set_output("response");
        assert_eq!(api.take_output(), "response");
    }

    #[test]
    fn kv_storage_round_trip() {
        let mut api = make_api("");
        assert_eq!(api.kv_get("key"), None);

        api.kv_set("key", "value").expect("should set");
        assert_eq!(api.kv_get("key"), Some("value".to_string()));
    }

    #[test]
    fn kv_storage_respects_quota() {
        let mut api = LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: Some(10),
        });

        // 3 + 3 = 6 bytes, within quota
        api.kv_set("abc", "def").expect("should fit");

        // 3 + 8 = 11, total would be 17 — exceeds 10 byte quota
        let result = api.kv_set("xyz", "12345678");
        assert!(result.is_err());
    }

    #[test]
    fn log_does_not_panic() {
        let api = make_api("");
        for level in 0..=5 {
            api.log(level, "test message");
        }
    }

    #[test]
    fn empty_output_by_default() {
        let api = make_api("input");
        assert_eq!(api.take_output(), "");
    }

    #[test]
    fn output_overwrites_previous() {
        let mut api = make_api("");
        api.set_output("first");
        api.set_output("second");
        assert_eq!(api.take_output(), "second");
    }

    /// Regression test: take_output() drains the string (uses std::mem::take),
    /// leaving an empty string behind. Previously it cloned, which was
    /// misleading given the "take" naming.
    #[test]
    fn take_output_drains_string() {
        let mut api = make_api("");
        api.set_output("hello");
        let first = api.take_output();
        assert_eq!(first, "hello");
        // After taking, the output should be empty
        let second = api.take_output();
        assert_eq!(second, "");
    }
}
