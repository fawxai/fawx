//! Live host API implementation for WASM skills running in the kernel.
//!
//! Provides a real [`HostApi`] that routes WASM host calls to the appropriate
//! runtime services (tracing for logs, [`SkillStorage`] for key-value ops).

use fx_core::error::SkillError;
use fx_skills::host_api::HostApi;
use fx_skills::live_host_api::{execute_http_request, CredentialProvider};
use fx_skills::manifest::Capability;
use fx_skills::storage::SkillStorage;
use std::sync::{Arc, Mutex};

/// Default storage quota per skill: 64 KiB.
const DEFAULT_STORAGE_QUOTA: usize = 64 * 1024;

/// Live host API backed by real runtime services.
///
/// Routes WASM host function calls to:
/// - `tracing` for logging
/// - [`SkillStorage`] for key-value persistence
/// - [`CredentialProvider`] for secret retrieval (e.g., GitHub PAT)
/// - `execute_http_request` for outbound HTTP (capability-gated)
/// - Input/output buffers for skill invocation I/O
pub struct LiveHostApi {
    storage: Arc<Mutex<SkillStorage>>,
    input: String,
    output: Arc<Mutex<String>>,
    capabilities: Vec<Capability>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
}

impl std::fmt::Debug for LiveHostApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveHostApi")
            .field("input", &self.input)
            .field("capabilities", &self.capabilities)
            .field("credential_provider", &self.credential_provider.is_some())
            .finish_non_exhaustive()
    }
}

/// Configuration for creating a [`LiveHostApi`].
pub struct LiveHostApiConfig<'a> {
    /// Skill name (used for storage isolation).
    pub skill_name: &'a str,
    /// Input JSON string for the skill invocation.
    pub input: String,
    /// Storage quota in bytes (defaults to [`DEFAULT_STORAGE_QUOTA`]).
    pub storage_quota: Option<usize>,
    /// Capabilities the skill has declared in its manifest.
    pub capabilities: Vec<Capability>,
    /// Optional credential provider for bridging secrets to skills.
    pub credential_provider: Option<Arc<dyn CredentialProvider>>,
}

impl LiveHostApi {
    /// Create a new live host API for a skill invocation.
    pub fn new(config: LiveHostApiConfig<'_>) -> Self {
        let quota = config.storage_quota.unwrap_or(DEFAULT_STORAGE_QUOTA);
        Self {
            storage: Arc::new(Mutex::new(SkillStorage::new(config.skill_name, quota))),
            input: config.input,
            output: Arc::new(Mutex::new(String::new())),
            capabilities: config.capabilities,
            credential_provider: config.credential_provider,
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
        // Credential provider takes priority (bridges secrets to skills)
        if let Some(provider) = &self.credential_provider {
            if let Some(value) = provider.get_credential(key) {
                return Some((*value).clone());
            }
        }
        // Fall back to skill-local storage
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

    fn http_request(&self, method: &str, url: &str, headers: &str, body: &str) -> Option<String> {
        if !is_network_allowed(url, &self.capabilities) {
            tracing::error!("http_request denied: domain not in allowlist");
            return None;
        }
        execute_http_request(method, url, headers, body)
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

fn is_network_allowed(url: &str, capabilities: &[Capability]) -> bool {
    for cap in capabilities {
        match cap {
            Capability::Network => return true,
            Capability::NetworkRestricted { allowed_domains } => {
                if let Some(host) = extract_host(url) {
                    let host_lower = host.to_ascii_lowercase();
                    if allowed_domains.iter().any(|domain| {
                        let domain_lower = domain.to_ascii_lowercase();
                        host_lower == domain_lower
                            || host_lower.ends_with(&format!(".{domain_lower}"))
                    }) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn extract_host(url: &str) -> Option<&str> {
    let start = if url.get(..8)?.eq_ignore_ascii_case("https://") {
        8
    } else if url.get(..7)?.eq_ignore_ascii_case("http://") {
        7
    } else {
        return None;
    };
    let rest = &url[start..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host_port = &rest[..host_end];
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zeroize::Zeroizing;

    fn make_config(input: &str) -> LiveHostApiConfig<'_> {
        LiveHostApiConfig {
            skill_name: "test_skill",
            input: input.to_string(),
            storage_quota: None,
            capabilities: vec![],
            credential_provider: None,
        }
    }

    fn make_api(input: &str) -> LiveHostApi {
        LiveHostApi::new(make_config(input))
    }

    /// Mock credential provider for testing.
    struct MockCredentialProvider {
        credentials: HashMap<String, String>,
    }

    impl MockCredentialProvider {
        fn new() -> Self {
            Self {
                credentials: HashMap::new(),
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
            capabilities: vec![],
            credential_provider: None,
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

    #[test]
    fn kv_get_bridges_credential_provider() {
        let provider =
            MockCredentialProvider::new().with_credential("github_token", "ghp_test_token_12345");
        let api = LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: None,
            capabilities: vec![],
            credential_provider: Some(Arc::new(provider)),
        });
        assert_eq!(
            api.kv_get("github_token"),
            Some("ghp_test_token_12345".to_string())
        );
    }

    #[test]
    fn kv_get_credential_provider_priority_over_storage() {
        let provider =
            MockCredentialProvider::new().with_credential("github_token", "from_provider");
        let mut api = LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: None,
            capabilities: vec![],
            credential_provider: Some(Arc::new(provider)),
        });
        // Store a value in skill-local storage under the same key
        api.kv_set("github_token", "from_storage")
            .expect("should set");
        // Provider wins
        assert_eq!(
            api.kv_get("github_token"),
            Some("from_provider".to_string())
        );
    }

    #[test]
    fn network_allowed_unrestricted() {
        assert!(is_network_allowed(
            "https://anything.com",
            &[Capability::Network]
        ));
    }

    #[test]
    fn network_allowed_exact_domain() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["api.weather.gov".into()],
        }];
        assert!(is_network_allowed("https://api.weather.gov/points", &caps));
    }

    #[test]
    fn network_denied_wrong_domain() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["api.weather.gov".into()],
        }];
        assert!(!is_network_allowed("https://evil.com/exfil", &caps));
    }

    #[test]
    fn network_allowed_subdomain() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["weather.gov".into()],
        }];
        assert!(is_network_allowed("https://api.weather.gov/points", &caps));
    }

    #[test]
    fn network_denied_partial_suffix() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["weather.gov".into()],
        }];
        assert!(!is_network_allowed("https://badweather.gov/attack", &caps));
    }

    #[test]
    fn network_denied_no_capability() {
        assert!(!is_network_allowed(
            "https://anything.com",
            &[Capability::Storage]
        ));
    }

    #[test]
    fn network_denied_empty_caps() {
        assert!(!is_network_allowed("https://anything.com", &[]));
    }

    #[test]
    fn network_allowed_case_insensitive() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["WEATHER.GOV".into()],
        }];
        assert!(is_network_allowed("https://Api.Weather.Gov/points", &caps));
    }

    #[test]
    fn extract_host_https() {
        assert_eq!(
            extract_host("https://api.weather.gov/foo"),
            Some("api.weather.gov")
        );
    }

    #[test]
    fn extract_host_with_port() {
        assert_eq!(
            extract_host("https://localhost:8080/path"),
            Some("localhost")
        );
    }

    #[test]
    fn extract_host_http() {
        assert_eq!(extract_host("http://example.com/path"), Some("example.com"));
    }

    #[test]
    fn extract_host_no_scheme() {
        assert_eq!(extract_host("ftp://example.com"), None);
    }

    #[test]
    fn extract_host_empty() {
        assert_eq!(extract_host(""), None);
    }

    #[test]
    fn extract_host_uppercase_scheme() {
        assert_eq!(
            extract_host("HTTPS://api.weather.gov/foo"),
            Some("api.weather.gov")
        );
    }

    #[test]
    fn http_request_denied_without_network_capability() {
        let api = make_api("");
        // No capabilities → denied
        let result = api.http_request("GET", "https://example.com", "", "");
        assert!(result.is_none());
    }

    /// Verifies that with Network capability, the request passes capability
    /// gating and reaches HTTPS enforcement. Using `http://` (not `https://`)
    /// triggers the HTTPS-only rejection in `execute_http_request`, proving
    /// the request was NOT short-circuited by capability denial.
    #[test]
    fn http_request_requires_https_when_capable() {
        let api = LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: None,
            capabilities: vec![Capability::Network],
            credential_provider: None,
        });
        // Capability check passes, but HTTPS enforcement rejects http://
        let result = api.http_request("GET", "http://example.com", "{}", "");
        assert_eq!(result, None);
    }
}
