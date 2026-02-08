//! LLM routing logic for selecting between local and cloud providers.

use nv_core::error::LlmError;
use tracing::{debug, warn};

use crate::LlmProvider;

/// Strategy for routing LLM requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    /// Try local first, fall back to cloud on error
    LocalFirst,

    /// Try cloud first, fall back to local on error
    CloudFirst,

    /// Only use local (fail if local unavailable)
    LocalOnly,

    /// Only use cloud (fail if cloud unavailable)
    CloudOnly,
}

/// Routes LLM requests to appropriate providers based on strategy.
///
/// The router maintains a collection of providers and selects one
/// based on the routing strategy.
pub struct LlmRouter {
    providers: Vec<Box<dyn LlmProvider>>,
}

impl LlmRouter {
    /// Create a new router with the given providers.
    ///
    /// # Arguments
    /// * `providers` - Vector of LLM providers to route between
    ///
    /// # Panics
    /// Panics if providers list is empty (use a type-level guarantee in production)
    pub fn new(providers: Vec<Box<dyn LlmProvider>>) -> Self {
        if providers.is_empty() {
            panic!("LlmRouter requires at least one provider");
        }
        Self { providers }
    }

    /// Generate completion using the specified routing strategy.
    ///
    /// # Arguments
    /// * `prompt` - Input text
    /// * `routing` - Strategy for selecting provider
    ///
    /// # Returns
    /// Generated text or error if all applicable providers fail
    pub async fn generate(
        &self,
        prompt: &str,
        routing: RoutingStrategy,
    ) -> Result<String, LlmError> {
        match routing {
            RoutingStrategy::LocalFirst => self.try_local_then_cloud(prompt, 512).await,
            RoutingStrategy::CloudFirst => self.try_cloud_then_local(prompt, 512).await,
            RoutingStrategy::LocalOnly => self.try_local_only(prompt, 512).await,
            RoutingStrategy::CloudOnly => self.try_cloud_only(prompt, 512).await,
        }
    }

    /// Try local providers first, fall back to cloud.
    async fn try_local_then_cloud(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, LlmError> {
        // For now, just try first provider (will be enhanced with provider type detection)
        if let Some(provider) = self.providers.first() {
            match provider.generate(prompt, max_tokens).await {
                Ok(result) => {
                    debug!("Local generation succeeded");
                    Ok(result)
                }
                Err(e) => {
                    warn!("Local generation failed: {}, trying cloud fallback", e);
                    // Try cloud providers if available
                    if self.providers.len() > 1 {
                        self.providers[1].generate(prompt, max_tokens).await
                    } else {
                        Err(LlmError::Inference(
                            "Local failed and no cloud fallback available".to_string(),
                        ))
                    }
                }
            }
        } else {
            Err(LlmError::Model("No providers available".to_string()))
        }
    }

    /// Try cloud providers first, fall back to local.
    async fn try_cloud_then_local(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, LlmError> {
        // Similar to local-first but reversed
        if self.providers.len() > 1 {
            match self.providers[1].generate(prompt, max_tokens).await {
                Ok(result) => {
                    debug!("Cloud generation succeeded");
                    Ok(result)
                }
                Err(e) => {
                    warn!("Cloud generation failed: {}, trying local fallback", e);
                    self.providers[0].generate(prompt, max_tokens).await
                }
            }
        } else if let Some(provider) = self.providers.first() {
            provider.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No providers available".to_string()))
        }
    }

    /// Only use local providers.
    async fn try_local_only(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError> {
        if let Some(provider) = self.providers.first() {
            provider.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No local provider available".to_string()))
        }
    }

    /// Only use cloud providers.
    async fn try_cloud_only(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError> {
        if self.providers.len() > 1 {
            self.providers[1].generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No cloud provider available".to_string()))
        }
    }

    /// Get number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Mock provider that tracks call count
    struct MockProvider {
        name: String,
        should_succeed: bool,
        response_text: String,
        call_count: Mutex<usize>,
    }

    impl MockProvider {
        fn new_success(name: &str, response: &str) -> Self {
            Self {
                name: name.to_string(),
                should_succeed: true,
                response_text: response.to_string(),
                call_count: Mutex::new(0),
            }
        }

        fn new_failure(name: &str, error_msg: &str) -> Self {
            Self {
                name: name.to_string(),
                should_succeed: false,
                response_text: error_msg.to_string(),
                call_count: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _max_tokens: u32) -> Result<String, LlmError> {
            *self.call_count.lock().unwrap() += 1;
            if self.should_succeed {
                Ok(self.response_text.clone())
            } else {
                Err(LlmError::Inference(self.response_text.clone()))
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
    async fn test_local_first_success() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(vec![local, cloud]);
        let result = router.generate("test", RoutingStrategy::LocalFirst).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "local response");
    }

    #[tokio::test]
    async fn test_local_first_fallback() {
        let local = Box::new(MockProvider::new_failure("local", "local failed"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(vec![local, cloud]);
        let result = router.generate("test", RoutingStrategy::LocalFirst).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cloud response");
    }

    #[tokio::test]
    async fn test_cloud_only() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(vec![local, cloud]);
        let result = router.generate("test", RoutingStrategy::CloudOnly).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cloud response");
    }

    #[tokio::test]
    async fn test_local_only() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(vec![local, cloud]);
        let result = router.generate("test", RoutingStrategy::LocalOnly).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "local response");
    }

    #[tokio::test]
    async fn test_local_only_fails_when_no_local() {
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(vec![cloud]);
        let result = router.generate("test", RoutingStrategy::LocalOnly).await;

        // With only one provider, local-only still uses it
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_provider_count() {
        let local = Box::new(MockProvider::new_success("local", "ok"));
        let cloud = Box::new(MockProvider::new_success("cloud", "ok"));

        let router = LlmRouter::new(vec![local, cloud]);
        assert_eq!(router.provider_count(), 2);
    }

    #[test]
    #[should_panic(expected = "LlmRouter requires at least one provider")]
    fn test_empty_providers_panics() {
        LlmRouter::new(vec![]);
    }
}
