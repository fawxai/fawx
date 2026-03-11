use crate::response_parser::parse_patch_response;
use crate::{Experiment, PatchSource};
use fx_llm::{completion_text, CompletionRequest, Message, ModelRouter};
use std::sync::Arc;

pub struct LlmPatchSource {
    router: Arc<ModelRouter>,
    model: String,
}

impl LlmPatchSource {
    pub fn new(router: Arc<ModelRouter>, model: String) -> Self {
        Self { router, model }
    }
}

#[async_trait::async_trait]
impl PatchSource for LlmPatchSource {
    async fn generate_patch(
        &self,
        system_prompt: &str,
        experiment: &Experiment,
    ) -> crate::Result<crate::PatchResponse> {
        let request = CompletionRequest {
            model: self.model.clone(),
            messages: vec![Message::user(build_experiment_prompt(experiment))],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            system_prompt: Some(system_prompt.to_owned()),
            thinking: None,
        };
        let response = self.router.complete(request).await.map_err(|error| {
            crate::ConsensusError::Protocol(format!("LLM completion failed: {error}"))
        })?;
        let text = completion_text(&response);
        parse_patch_response(&text)
    }
}

pub fn build_experiment_prompt(experiment: &Experiment) -> String {
    let scope = experiment
        .scope
        .allowed_files
        .iter()
        .map(|path| path.0.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let criteria = experiment
        .fitness_criteria
        .iter()
        .map(|criterion| criterion.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        concat!(
            "You are participating in a proof-of-fitness experiment.\n\n",
            "Signal: {} — {}\n",
            "Hypothesis: {}\n",
            "Allowed files: {}\n",
            "Fitness criteria: {}\n\n",
            "Return exactly three tagged sections in this order:\n",
            "<PATCH>\n",
            "[unified diff patch]\n",
            "</PATCH>\n",
            "<APPROACH>\n",
            "[1-2 sentence approach summary]\n",
            "</APPROACH>\n",
            "<METRICS>\n",
            "{{\"build_success\": 0.0-1.0, \"test_pass_rate\": 0.0-1.0, \"signal_resolution\": 0.0-1.0}}\n",
            "</METRICS>"
        ),
        experiment.trigger.name,
        experiment.trigger.description,
        experiment.hypothesis,
        scope,
        criteria,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use futures::stream;
    use fx_llm::{
        CompletionResponse, CompletionStream, ContentBlock, ProviderCapabilities, ProviderError,
    };
    use std::time::Duration;
    use uuid::Uuid;

    struct MockProvider {
        response: String,
    }

    #[async_trait::async_trait]
    impl fx_llm::CompletionProvider for MockProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            Ok(CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: self.response.clone(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            })
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, ProviderError> {
            Ok(Box::pin(stream::empty()))
        }

        fn name(&self) -> &str {
            "mock"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["mock-model".to_owned()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: true,
                requires_streaming: false,
            }
        }
    }

    #[tokio::test]
    async fn generate_patch_extracts_patch_approach_and_metrics() {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(MockProvider {
            response: concat!(
                "<PATCH>\n",
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -1 +1 @@\n",
                "-old\n",
                "+new\n",
                "</PATCH>\n\n",
                "<APPROACH>\n",
                "Tightened the implementation around the failing path.\n",
                "</APPROACH>\n",
                "<METRICS>\n",
                "{\"build_success\":1.0,\"test_pass_rate\":0.9,\"signal_resolution\":0.8}\n",
                "</METRICS>"
            )
            .to_owned(),
        }));
        router.set_active("mock-model").expect("active model");
        let source = LlmPatchSource::new(Arc::new(router), "mock-model".to_owned());

        let response = source
            .generate_patch("system", &sample_experiment())
            .await
            .expect("patch response");

        assert!(response
            .patch
            .contains("diff --git a/src/lib.rs b/src/lib.rs"));
        assert_eq!(
            response.approach,
            "Tightened the implementation around the failing path."
        );
        assert_eq!(response.self_metrics.get("build_success"), Some(&1.0));
        assert_eq!(response.self_metrics.get("test_pass_rate"), Some(&0.9));
        assert_eq!(response.self_metrics.get("signal_resolution"), Some(&0.8));
    }

    fn sample_experiment() -> crate::Experiment {
        crate::Experiment {
            id: Uuid::new_v4(),
            trigger: crate::Signal {
                id: Uuid::new_v4(),
                name: "latency".to_owned(),
                description: "High latency detected".to_owned(),
                severity: crate::Severity::High,
            },
            hypothesis: "parallelism helps".to_owned(),
            fitness_criteria: vec![crate::FitnessCriterion {
                name: "build_success".to_owned(),
                metric_type: crate::MetricType::Higher,
                weight: 1.0,
            }],
            scope: crate::ModificationScope {
                allowed_files: vec![crate::PathPattern::from("src/**/*.rs")],
                proposal_tier: crate::ProposalTier::Tier1,
            },
            timeout: Duration::from_secs(60),
            min_candidates: 1,
            created_at: Utc::now(),
        }
    }
}
