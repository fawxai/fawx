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

fn experiment_scope(experiment: &Experiment) -> String {
    experiment
        .scope
        .allowed_files
        .iter()
        .map(|path| path.0.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn experiment_criteria(experiment: &Experiment) -> String {
    experiment
        .fitness_criteria
        .iter()
        .map(|criterion| criterion.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn build_experiment_prompt(experiment: &Experiment) -> String {
    let scope = experiment_scope(experiment);
    let criteria = experiment_criteria(experiment);
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

pub fn build_subagent_experiment_prompt(experiment: &Experiment) -> String {
    let scope = experiment_scope(experiment);
    let criteria = experiment
        .fitness_criteria
        .iter()
        .map(|criterion| format!("{} (weight: {})", criterion.name, criterion.weight))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        concat!(
            "You are a Fawx agent participating in a proof-of-fitness experiment.\n\n",
            "Signal: {} — {}\n",
            "Hypothesis: {}\n",
            "Target files: {}\n",
            "Fitness criteria: {}\n\n",
            "IMPORTANT: You have full tool access. You MUST use tools — do NOT generate code from memory.\n\n",
            "## Step 1: Understand the code\n",
            "- Use read_file to read EVERY target file completely\n",
            "- Use read_file to read files they import (check `use` statements)\n",
            "- If there are existing tests, read them to understand test patterns and helper functions\n",
            "- Do NOT assume any functions, types, or helpers exist — verify by reading\n\n",
            "## Step 2: Make changes\n",
            "- Use edit_file to modify the target files\n",
            "- If you need test helper functions, define them in the same file — do NOT reference helpers from other files unless you verified they are pub and importable\n\n",
            "## Step 3: Verify (MANDATORY — do not skip)\n",
            "- Run: `cargo build 2>&1` via run_command (from the project root)\n",
            "- If build fails, read the errors, fix them with edit_file, and rebuild\n",
            "- Run: `cargo test 2>&1` via run_command\n",
            "- If tests fail, read the errors, fix them with edit_file, and retest\n",
            "- REPEAT until both build and test pass with zero errors\n",
            "- You MUST see a successful build and test output before proceeding\n\n",
            "## Step 4: Output results (ONLY after Step 3 passes)\n",
            "- Run: `git diff` via run_command to capture your changes\n",
            "- Output the diff inside <PATCH> tags\n\n",
            "<PATCH>\n",
            "[paste the exact output of `git diff` here]\n",
            "</PATCH>\n",
            "<APPROACH>\n",
            "[1-2 sentence summary of what you changed and why]\n",
            "</APPROACH>\n",
            "<METRICS>\n",
            "{{\"build_success\": 1.0, \"test_pass_rate\": <actual_rate>, \"signal_resolution\": <0.0-1.0>}}\n",
            "</METRICS>\n\n",
            "CRITICAL RULES:\n",
            "- NEVER output <PATCH> until cargo build AND cargo test pass\n",
            "- NEVER assume helper functions exist — define them or verify with read_file\n",
            "- NEVER generate a diff from memory — always use `git diff` output\n",
            "- If you cannot make the code compile after 3 attempts, output an empty <PATCH></PATCH> with build_success: 0.0"
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

    #[test]
    fn build_subagent_experiment_prompt_includes_tool_instructions() {
        let prompt = build_subagent_experiment_prompt(&sample_experiment());

        assert!(prompt.contains("You MUST use tools"));
        assert!(prompt.contains("Use read_file to read EVERY target file"));
        assert!(prompt.contains("cargo build 2>&1"));
        assert!(prompt.contains("NEVER output <PATCH> until cargo build AND cargo test pass"));
        assert!(prompt.contains("NEVER assume helper functions exist"));
    }

    #[test]
    fn build_experiment_prompt_stays_direct_llm_focused() {
        let prompt = build_experiment_prompt(&sample_experiment());

        assert!(prompt.contains("Return exactly three tagged sections in this order"));
        assert!(!prompt.contains("IMPORTANT: You have full tool access"));
        assert!(!prompt.contains("read_file"));
        assert!(!prompt.contains("run_command"));
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
