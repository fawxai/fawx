//! Intelligent fallback router with provider health tracking.
//!
//! This module provides a smarter routing layer that tracks provider health
//! and automatically falls back to alternative providers when failures occur.

use fx_core::error::LlmError;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

use crate::router::RoutingStrategy;
use crate::routing::{resolve_strategy, RoutingConfig, RoutingContext};
use crate::LlmProvider;

/// Health state of an LLM provider.
///
/// Tracks consecutive failures and automatically marks providers as unhealthy
/// after repeated failures, allowing the router to skip them temporarily.
#[derive(Debug, Clone)]
pub struct ProviderHealth {
    /// Number of consecutive failures
    pub consecutive_failures: u32,
    /// Timestamp of last failure (seconds since UNIX epoch)
    pub last_failure: Option<u64>,
    /// Whether provider is currently considered healthy
    pub is_healthy: bool,
}

impl ProviderHealth {
    /// Create a new healthy provider state.
    pub fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_failure: None,
            is_healthy: true,
        }
    }

    /// Maximum consecutive failures before marking unhealthy.
    ///
    /// After 3 failures in a row, we assume the provider has a persistent
    /// issue and stop trying it temporarily. This prevents cascading delays
    /// from repeatedly attempting to use a broken provider. The counter resets
    /// to zero on any successful generation.
    const MAX_FAILURES: u32 = 3;

    /// Cooldown period in seconds before auto-recovery (5 minutes).
    ///
    /// After marking a provider unhealthy, we wait this long before
    /// automatically retrying. This gives transient issues (network blips,
    /// API rate limits) time to resolve without requiring manual intervention.
    /// After cooldown, the next request will attempt the provider again.
    const COOLDOWN_SECONDS: u64 = 300;

    /// Record a failure and update health status.
    pub fn record_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.last_failure = Some(current_timestamp());

        if self.consecutive_failures >= Self::MAX_FAILURES {
            self.is_healthy = false;
            warn!(
                "Provider marked unhealthy after {} consecutive failures",
                self.consecutive_failures
            );
        }
    }

    /// Record a success and reset health status.
    pub fn record_success(&mut self) {
        if self.consecutive_failures > 0 {
            debug!(
                "Provider recovered after {} failures",
                self.consecutive_failures
            );
        }
        self.consecutive_failures = 0;
        self.last_failure = None;
        self.is_healthy = true;
    }

    /// Check if provider should auto-recover based on cooldown period.
    pub fn check_recovery(&mut self) {
        if !self.is_healthy {
            if let Some(last_fail) = self.last_failure {
                let now = current_timestamp();
                if now.saturating_sub(last_fail) >= Self::COOLDOWN_SECONDS {
                    debug!("Provider auto-recovered after cooldown period");
                    self.consecutive_failures = 0;
                    self.is_healthy = true;
                }
            }
        }
    }
}

impl Default for ProviderHealth {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a fallback generation attempt.
#[derive(Debug, Clone)]
pub struct FallbackResult {
    /// Generated text
    pub text: String,
    /// Which provider was used ("local" or "cloud")
    pub provider_used: String,
    /// Whether fallback was triggered
    pub fallback_used: bool,
}

/// Intelligent router with fallback and health tracking.
///
/// Wraps local and cloud providers and automatically falls back to alternatives
/// when providers fail. Tracks provider health to avoid repeatedly trying
/// unhealthy providers.
pub struct FallbackRouter {
    local: Option<Box<dyn LlmProvider>>,
    cloud: Option<Box<dyn LlmProvider>>,
    config: RoutingConfig,
    local_health: Arc<Mutex<ProviderHealth>>,
    cloud_health: Arc<Mutex<ProviderHealth>>,
}

impl FallbackRouter {
    /// Create a new fallback router.
    ///
    /// # Arguments
    /// * `local` - Optional local LLM provider
    /// * `cloud` - Optional cloud LLM provider
    /// * `config` - Routing configuration with rules
    ///
    /// # Returns
    /// A new fallback router instance
    pub fn new(
        local: Option<Box<dyn LlmProvider>>,
        cloud: Option<Box<dyn LlmProvider>>,
        config: RoutingConfig,
    ) -> Self {
        Self {
            local,
            cloud,
            config,
            local_health: Arc::new(Mutex::new(ProviderHealth::new())),
            cloud_health: Arc::new(Mutex::new(ProviderHealth::new())),
        }
    }

    /// Generate text using intelligent routing with fallback.
    ///
    /// # Arguments
    /// * `prompt` - Input text to generate from
    /// * `max_tokens` - Maximum tokens to generate
    /// * `context` - Routing context (intent, confidence, etc.)
    ///
    /// # Returns
    /// Result with generated text and metadata about which provider was used
    pub async fn generate(
        &self,
        prompt: &str,
        max_tokens: u32,
        context: &RoutingContext,
    ) -> Result<FallbackResult, LlmError> {
        // Check for auto-recovery (ignore poison - health tracking is not critical)
        self.local_health
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .check_recovery();
        self.cloud_health
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .check_recovery();

        // Resolve strategy based on context
        let strategy = resolve_strategy(&self.config, context);

        // Execute strategy with health-aware fallback
        match strategy {
            RoutingStrategy::LocalFirst => self.try_local_first(prompt, max_tokens).await,
            RoutingStrategy::CloudFirst => self.try_cloud_first(prompt, max_tokens).await,
            RoutingStrategy::LocalOnly => self.try_local_only(prompt, max_tokens).await,
            RoutingStrategy::CloudOnly => self.try_cloud_only(prompt, max_tokens).await,
        }
    }

    /// Try local provider first, fall back to cloud on failure.
    async fn try_local_first(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<FallbackResult, LlmError> {
        if let Some(result) = self
            .attempt_provider(
                &self.local,
                &self.local_health,
                "local",
                prompt,
                max_tokens,
                true,
            )
            .await?
        {
            return Ok(Self::build_result(result, "local", false));
        }

        if let Some(result) = self
            .attempt_provider(
                &self.cloud,
                &self.cloud_health,
                "cloud",
                prompt,
                max_tokens,
                false,
            )
            .await?
        {
            return Ok(Self::build_result(result, "cloud", true));
        }

        Err(LlmError::Inference(
            "LocalFirst strategy failed: all providers unavailable".to_string(),
        ))
    }

    /// Try cloud provider first, fall back to local on failure.
    async fn try_cloud_first(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<FallbackResult, LlmError> {
        if let Some(result) = self
            .attempt_provider(
                &self.cloud,
                &self.cloud_health,
                "cloud",
                prompt,
                max_tokens,
                true,
            )
            .await?
        {
            return Ok(Self::build_result(result, "cloud", false));
        }

        if let Some(result) = self
            .attempt_provider(
                &self.local,
                &self.local_health,
                "local",
                prompt,
                max_tokens,
                false,
            )
            .await?
        {
            return Ok(Self::build_result(result, "local", true));
        }

        Err(LlmError::Inference(
            "CloudFirst strategy failed: all providers unavailable".to_string(),
        ))
    }

    async fn attempt_provider(
        &self,
        provider: &Option<Box<dyn LlmProvider>>,
        health: &Arc<Mutex<ProviderHealth>>,
        provider_name: &str,
        prompt: &str,
        max_tokens: u32,
        continue_on_error: bool,
    ) -> Result<Option<String>, LlmError> {
        if !Self::is_provider_available_and_healthy(provider, health) {
            return Ok(None);
        }

        match self.try_provider(provider, prompt, max_tokens).await {
            Ok(text) => {
                Self::record_provider_success(health);
                Ok(Some(text))
            }
            Err(error) => {
                warn!("{} provider failed: {}", provider_name, error);
                Self::record_provider_failure(health);
                if continue_on_error {
                    Ok(None)
                } else {
                    Err(error)
                }
            }
        }
    }

    fn is_provider_available_and_healthy(
        provider: &Option<Box<dyn LlmProvider>>,
        health: &Arc<Mutex<ProviderHealth>>,
    ) -> bool {
        provider.is_some() && health.lock().unwrap_or_else(|e| e.into_inner()).is_healthy
    }

    fn record_provider_success(health: &Arc<Mutex<ProviderHealth>>) {
        health
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record_success();
    }

    fn record_provider_failure(health: &Arc<Mutex<ProviderHealth>>) {
        health
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record_failure();
    }

    fn build_result(text: String, provider_used: &str, fallback_used: bool) -> FallbackResult {
        FallbackResult {
            text,
            provider_used: provider_used.to_string(),
            fallback_used,
        }
    }

    /// Only use local provider (no fallback).
    async fn try_local_only(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<FallbackResult, LlmError> {
        if self.local.is_none() {
            return Err(LlmError::Model("No local provider available".to_string()));
        }

        match self.try_provider(&self.local, prompt, max_tokens).await {
            Ok(text) => {
                self.local_health
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .record_success();
                Ok(FallbackResult {
                    text,
                    provider_used: "local".to_string(),
                    fallback_used: false,
                })
            }
            Err(e) => {
                self.local_health
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .record_failure();
                Err(e)
            }
        }
    }

    /// Only use cloud provider (no fallback).
    async fn try_cloud_only(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<FallbackResult, LlmError> {
        if self.cloud.is_none() {
            return Err(LlmError::Model("No cloud provider available".to_string()));
        }

        match self.try_provider(&self.cloud, prompt, max_tokens).await {
            Ok(text) => {
                self.cloud_health
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .record_success();
                Ok(FallbackResult {
                    text,
                    provider_used: "cloud".to_string(),
                    fallback_used: false,
                })
            }
            Err(e) => {
                self.cloud_health
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .record_failure();
                Err(e)
            }
        }
    }

    /// Helper to try a provider and return result.
    async fn try_provider(
        &self,
        provider: &Option<Box<dyn LlmProvider>>,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, LlmError> {
        match provider {
            Some(p) => p.generate(prompt, max_tokens).await,
            None => Err(LlmError::Model("Provider not available".to_string())),
        }
    }

    /// Get current health status of local provider.
    pub fn local_health(&self) -> ProviderHealth {
        self.local_health
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Get current health status of cloud provider.
    pub fn cloud_health(&self) -> ProviderHealth {
        self.cloud_health
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// Get current timestamp in seconds since UNIX epoch.
///
/// Returns 0 if system time is somehow before UNIX_EPOCH (should never happen
/// on real systems, but handles the edge case gracefully without panicking).
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Mock provider for testing
    #[derive(Debug)]
    struct MockProvider {
        name: String,
        should_fail: Arc<AtomicBool>,
        response: String,
    }

    impl MockProvider {
        fn new_success(name: &str, response: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail: Arc::new(AtomicBool::new(false)),
                response: response.to_string(),
            }
        }

        fn new_failure(name: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail: Arc::new(AtomicBool::new(true)),
                response: String::new(),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _max_tokens: u32) -> Result<String, LlmError> {
            if self.should_fail.load(Ordering::SeqCst) {
                Err(LlmError::Inference(format!("{} failed", self.name)))
            } else {
                Ok(self.response.clone())
            }
        }

        async fn generate_streaming(
            &self,
            prompt: &str,
            max_tokens: u32,
            _callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, LlmError> {
            self.generate(prompt, max_tokens).await
        }

        fn model_name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_local_first_local_succeeds() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));
        let config = RoutingConfig::new_simple(RoutingStrategy::LocalFirst);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");
        let result = router.generate("test", 512, &context).await.unwrap();

        assert_eq!(result.text, "local response");
        assert_eq!(result.provider_used, "local");
        assert!(!result.fallback_used);
    }

    #[tokio::test]
    async fn test_local_first_fallback_to_cloud() {
        let local = Box::new(MockProvider::new_failure("local"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));
        let config = RoutingConfig::new_simple(RoutingStrategy::LocalFirst);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");
        let result = router.generate("test", 512, &context).await.unwrap();

        assert_eq!(result.text, "cloud response");
        assert_eq!(result.provider_used, "cloud");
        assert!(result.fallback_used);
    }

    #[tokio::test]
    async fn test_cloud_first_cloud_succeeds() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));
        let config = RoutingConfig::new_simple(RoutingStrategy::CloudFirst);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");
        let result = router.generate("test", 512, &context).await.unwrap();

        assert_eq!(result.text, "cloud response");
        assert_eq!(result.provider_used, "cloud");
        assert!(!result.fallback_used);
    }

    #[tokio::test]
    async fn test_cloud_first_fallback_to_local() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_failure("cloud"));
        let config = RoutingConfig::new_simple(RoutingStrategy::CloudFirst);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");
        let result = router.generate("test", 512, &context).await.unwrap();

        assert_eq!(result.text, "local response");
        assert_eq!(result.provider_used, "local");
        assert!(result.fallback_used);
    }

    #[tokio::test]
    async fn test_local_only_no_fallback() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));
        let config = RoutingConfig::new_simple(RoutingStrategy::LocalOnly);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");
        let result = router.generate("test", 512, &context).await.unwrap();

        assert_eq!(result.text, "local response");
        assert_eq!(result.provider_used, "local");
        assert!(!result.fallback_used);
    }

    #[tokio::test]
    async fn test_local_only_fails_when_local_fails() {
        let local = Box::new(MockProvider::new_failure("local"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));
        let config = RoutingConfig::new_simple(RoutingStrategy::LocalOnly);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");
        let result = router.generate("test", 512, &context).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cloud_only_no_fallback() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));
        let config = RoutingConfig::new_simple(RoutingStrategy::CloudOnly);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");
        let result = router.generate("test", 512, &context).await.unwrap();

        assert_eq!(result.text, "cloud response");
        assert_eq!(result.provider_used, "cloud");
        assert!(!result.fallback_used);
    }

    #[tokio::test]
    async fn test_cloud_only_fails_when_cloud_fails() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_failure("cloud"));
        let config = RoutingConfig::new_simple(RoutingStrategy::CloudOnly);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");
        let result = router.generate("test", 512, &context).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_both_providers_fail() {
        let local = Box::new(MockProvider::new_failure("local"));
        let cloud = Box::new(MockProvider::new_failure("cloud"));
        let config = RoutingConfig::new_simple(RoutingStrategy::LocalFirst);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");
        let result = router.generate("test", 512, &context).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_provider_health_marks_unhealthy_after_failures() {
        let local = Box::new(MockProvider::new_failure("local"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));
        let config = RoutingConfig::new_simple(RoutingStrategy::LocalFirst);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");

        // First 3 attempts should try local then fall back to cloud
        for _ in 0..3 {
            let result = router.generate("test", 512, &context).await.unwrap();
            assert_eq!(result.provider_used, "cloud");
        }

        // After 3 failures, local should be marked unhealthy
        let health = router.local_health();
        assert!(!health.is_healthy);
        assert_eq!(health.consecutive_failures, 3);
    }

    #[tokio::test]
    async fn test_provider_health_recovers_on_success() {
        let local_provider = MockProvider::new_failure("local");
        let fail_flag = Arc::clone(&local_provider.should_fail);
        let local = Box::new(local_provider);
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));
        let config = RoutingConfig::new_simple(RoutingStrategy::LocalFirst);
        let router = FallbackRouter::new(Some(local), Some(cloud), config);

        let context = RoutingContext::from_prompt("test");

        // Cause a failure
        let _ = router.generate("test", 512, &context).await;
        assert_eq!(router.local_health().consecutive_failures, 1);

        // Fix the provider and try again
        fail_flag.store(false, Ordering::SeqCst);
        let result = router.generate("test", 512, &context).await.unwrap();

        assert_eq!(result.provider_used, "local");
        let health = router.local_health();
        assert!(health.is_healthy);
        assert_eq!(health.consecutive_failures, 0);
    }

    #[test]
    fn test_provider_health_new() {
        let health = ProviderHealth::new();
        assert_eq!(health.consecutive_failures, 0);
        assert!(health.last_failure.is_none());
        assert!(health.is_healthy);
    }

    #[test]
    fn test_provider_health_record_failure() {
        let mut health = ProviderHealth::new();
        health.record_failure();

        assert_eq!(health.consecutive_failures, 1);
        assert!(health.last_failure.is_some());
        assert!(health.is_healthy); // Still healthy after 1 failure

        health.record_failure();
        health.record_failure();

        assert_eq!(health.consecutive_failures, 3);
        assert!(!health.is_healthy); // Unhealthy after 3 failures
    }

    #[test]
    fn test_provider_health_record_success() {
        let mut health = ProviderHealth::new();
        health.record_failure();
        health.record_failure();

        assert_eq!(health.consecutive_failures, 2);

        health.record_success();

        assert_eq!(health.consecutive_failures, 0);
        assert!(health.last_failure.is_none());
        assert!(health.is_healthy);
    }

    // Note: Auto-recovery based on cooldown timer is not tested here because it
    // requires mocking system time. The check_recovery() logic is simple (compare
    // timestamps) and is exercised in the integration flow via the generate() method.
    // For production use, consider adding a test with a mock time provider if this
    // becomes critical path code.
}
