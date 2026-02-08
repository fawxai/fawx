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
/// The router maintains typed references to local and cloud providers,
/// ensuring type-safe routing strategies.
#[derive(Debug)]
pub struct LlmRouter {
    local: Option<Box<dyn LlmProvider>>,
    cloud: Option<Box<dyn LlmProvider>>,
}

impl LlmRouter {
    /// Create a new router with optional local and cloud providers.
    ///
    /// # Arguments
    /// * `local` - Optional local LLM provider
    /// * `cloud` - Optional cloud LLM provider
    ///
    /// # Errors
    /// Returns `LlmError::Model` if both providers are None
    pub fn new(
        local: Option<Box<dyn LlmProvider>>,
        cloud: Option<Box<dyn LlmProvider>>,
    ) -> Result<Self, LlmError> {
        if local.is_none() && cloud.is_none() {
            return Err(LlmError::Model(
                "LlmRouter requires at least one provider".to_string(),
            ));
        }
        Ok(Self { local, cloud })
    }

    /// Generate completion using the specified routing strategy.
    ///
    /// # Arguments
    /// * `prompt` - Input text
    /// * `max_tokens` - Maximum number of tokens to generate
    /// * `routing` - Strategy for selecting provider
    ///
    /// # Returns
    /// Generated text or error if all applicable providers fail
    pub async fn generate(
        &self,
        prompt: &str,
        max_tokens: u32,
        routing: RoutingStrategy,
    ) -> Result<String, LlmError> {
        match routing {
            RoutingStrategy::LocalFirst => self.try_local_then_cloud(prompt, max_tokens).await,
            RoutingStrategy::CloudFirst => self.try_cloud_then_local(prompt, max_tokens).await,
            RoutingStrategy::LocalOnly => self.try_local_only(prompt, max_tokens).await,
            RoutingStrategy::CloudOnly => self.try_cloud_only(prompt, max_tokens).await,
        }
    }

    /// Try local provider first, fall back to cloud.
    async fn try_local_then_cloud(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, LlmError> {
        if let Some(ref local) = self.local {
            match local.generate(prompt, max_tokens).await {
                Ok(result) => {
                    debug!("Local generation succeeded");
                    Ok(result)
                }
                Err(e) => {
                    warn!("Local generation failed: {}, trying cloud fallback", e);
                    if let Some(ref cloud) = self.cloud {
                        cloud.generate(prompt, max_tokens).await
                    } else {
                        Err(LlmError::Inference(
                            "Local failed and no cloud fallback available".to_string(),
                        ))
                    }
                }
            }
        } else if let Some(ref cloud) = self.cloud {
            // No local provider, use cloud
            cloud.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No providers available".to_string()))
        }
    }

    /// Try cloud provider first, fall back to local.
    async fn try_cloud_then_local(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, LlmError> {
        if let Some(ref cloud) = self.cloud {
            match cloud.generate(prompt, max_tokens).await {
                Ok(result) => {
                    debug!("Cloud generation succeeded");
                    Ok(result)
                }
                Err(e) => {
                    warn!("Cloud generation failed: {}, trying local fallback", e);
                    if let Some(ref local) = self.local {
                        local.generate(prompt, max_tokens).await
                    } else {
                        Err(LlmError::Inference(
                            "Cloud failed and no local fallback available".to_string(),
                        ))
                    }
                }
            }
        } else if let Some(ref local) = self.local {
            // No cloud provider, use local
            local.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No providers available".to_string()))
        }
    }

    /// Only use local provider.
    async fn try_local_only(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError> {
        if let Some(ref local) = self.local {
            local.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No local provider available".to_string()))
        }
    }

    /// Only use cloud provider.
    async fn try_cloud_only(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError> {
        if let Some(ref cloud) = self.cloud {
            cloud.generate(prompt, max_tokens).await
        } else {
            Err(LlmError::Model("No cloud provider available".to_string()))
        }
    }

    /// Get number of registered providers.
    pub fn provider_count(&self) -> usize {
        let mut count = 0;
        if self.local.is_some() {
            count += 1;
        }
        if self.cloud.is_some() {
            count += 1;
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Mock provider that tracks call count
    #[derive(Debug)]
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

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::LocalFirst)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "local response");
    }

    #[tokio::test]
    async fn test_local_first_fallback() {
        let local = Box::new(MockProvider::new_failure("local", "local failed"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::LocalFirst)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cloud response");
    }

    #[tokio::test]
    async fn test_cloud_only() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::CloudOnly)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cloud response");
    }

    #[tokio::test]
    async fn test_local_only() {
        let local = Box::new(MockProvider::new_success("local", "local response"));
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::LocalOnly)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "local response");
    }

    #[tokio::test]
    async fn test_local_only_fails_when_no_local() {
        let cloud = Box::new(MockProvider::new_success("cloud", "cloud response"));

        let router = LlmRouter::new(None, Some(cloud)).unwrap();
        let result = router
            .generate("test", 512, RoutingStrategy::LocalOnly)
            .await;

        // LocalOnly with no local provider should fail
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_provider_count() {
        let local = Box::new(MockProvider::new_success("local", "ok"));
        let cloud = Box::new(MockProvider::new_success("cloud", "ok"));

        let router = LlmRouter::new(Some(local), Some(cloud)).unwrap();
        assert_eq!(router.provider_count(), 2);
    }

    #[tokio::test]
    async fn test_provider_count_single() {
        let local = Box::new(MockProvider::new_success("local", "ok"));

        let router = LlmRouter::new(Some(local), None).unwrap();
        assert_eq!(router.provider_count(), 1);
    }

    #[test]
    fn test_empty_providers_returns_error() {
        let result = LlmRouter::new(None, None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LlmError::Model(_)));
    }
}
