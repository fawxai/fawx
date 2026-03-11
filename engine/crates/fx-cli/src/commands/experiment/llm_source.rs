use fx_consensus::{Experiment, PatchResponse, PatchSource};
use fx_llm::{completion_text, CompletionRequest, Message, ModelRouter};
use std::collections::BTreeMap;
use std::sync::Arc;

const PATCH_START: &str = "<PATCH>";
const PATCH_END: &str = "</PATCH>";
const APPROACH_START: &str = "<APPROACH>";
const APPROACH_END: &str = "</APPROACH>";
const METRICS_START: &str = "<METRICS>";
const METRICS_END: &str = "</METRICS>";
const METRIC_KEYS: [&str; 3] = ["build_success", "test_pass_rate", "signal_resolution"];

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
    ) -> fx_consensus::Result<PatchResponse> {
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
            fx_consensus::ConsensusError::Protocol(format!("LLM completion failed: {error}"))
        })?;
        let text = completion_text(&response);
        let patch = extract_patch(&text).unwrap_or_else(|| text.clone());
        let approach = extract_approach(&text, &patch);
        let self_metrics = extract_metrics(&text);
        Ok(PatchResponse {
            patch,
            approach,
            self_metrics,
        })
    }
}

fn build_experiment_prompt(experiment: &Experiment) -> String {
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

fn extract_patch(text: &str) -> Option<String> {
    extract_tagged_block(text, PATCH_START, PATCH_END)
        .or_else(|| extract_fenced_block(text, "diff"))
        .or_else(|| extract_fenced_block(text, "patch"))
}

fn extract_fenced_block(text: &str, language: &str) -> Option<String> {
    let fence = format!("```{language}");
    let start = text.find(&fence)?;
    let after_start = &text[start + fence.len()..];
    let end = after_start.find("```")?;
    Some(after_start[..end].trim().to_owned())
}

fn extract_tagged_block(text: &str, start_tag: &str, end_tag: &str) -> Option<String> {
    let start = text.find(start_tag)? + start_tag.len();
    let end = text[start..].find(end_tag)? + start;
    Some(text[start..end].trim().to_owned())
}

fn extract_approach(text: &str, patch: &str) -> String {
    if let Some(approach) = extract_tagged_block(text, APPROACH_START, APPROACH_END) {
        return fallback_approach(&approach);
    }

    let mut remainder = text.trim().to_owned();
    if let Some(tagged_patch) = extract_tagged_block(text, PATCH_START, PATCH_END) {
        let wrapped = format!("{PATCH_START}\n{tagged_patch}\n{PATCH_END}");
        remainder = remainder.replacen(&wrapped, "", 1);
    } else {
        remainder = remainder.replacen(patch, "", 1);
        remainder = remainder
            .replace("```diff", "")
            .replace("```patch", "")
            .replace("```", "");
    }
    if let Some(metrics_block) = extract_json_block(text) {
        remainder = remainder.replacen(&metrics_block, "", 1);
    }
    fallback_approach(&remainder)
}

fn fallback_approach(text: &str) -> String {
    let approach = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if approach.is_empty() {
        "No approach summary provided".to_owned()
    } else {
        approach
    }
}

fn extract_metrics(text: &str) -> BTreeMap<String, f64> {
    let Some(metrics_block) = extract_json_block(text) else {
        return BTreeMap::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&metrics_block) else {
        return BTreeMap::new();
    };
    let Some(object) = value.as_object() else {
        return BTreeMap::new();
    };
    object
        .iter()
        .filter_map(|(key, value)| value.as_f64().map(|number| (key.clone(), number)))
        .collect()
}

fn extract_json_block(text: &str) -> Option<String> {
    if let Some(tagged) = extract_tagged_block(text, METRICS_START, METRICS_END) {
        return Some(tagged);
    }

    let search_start = patch_search_start(text);
    let search_text = &text[search_start..];
    let mut candidate_starts = search_text.match_indices('{').collect::<Vec<_>>();
    candidate_starts.reverse();

    for (relative_start, _) in candidate_starts {
        let absolute_start = search_start + relative_start;
        if let Some(block) = extract_balanced_json(text, absolute_start) {
            if has_expected_metrics(&block) {
                return Some(block);
            }
        }
    }
    None
}

fn patch_search_start(text: &str) -> usize {
    if let Some(end) = tagged_block_end(text, PATCH_START, PATCH_END) {
        return end;
    }
    for language in ["diff", "patch"] {
        if let Some(end) = fenced_block_end(text, language) {
            return end;
        }
    }
    0
}

fn tagged_block_end(text: &str, start_tag: &str, end_tag: &str) -> Option<usize> {
    let start = text.find(start_tag)? + start_tag.len();
    let end = text[start..].find(end_tag)? + start;
    Some(end + end_tag.len())
}

fn fenced_block_end(text: &str, language: &str) -> Option<usize> {
    let fence = format!("```{language}");
    let start = text.find(&fence)? + fence.len();
    let end = text[start..].find("```")? + start;
    Some(end + 3)
}

fn extract_balanced_json(text: &str, start: usize) -> Option<String> {
    let mut depth = 0_u32;
    let mut end = None;
    for (offset, character) in text[start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(start + offset + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    end.map(|index| text[start..index].to_owned())
}

fn has_expected_metrics(block: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(block) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    METRIC_KEYS.iter().all(|key| object.contains_key(*key))
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

    #[test]
    fn extract_json_block_prefers_metrics_after_patch_content() {
        let text = concat!(
            "```diff\n",
            "diff --git a/src/config.rs b/src/config.rs\n",
            "--- a/src/config.rs\n",
            "+++ b/src/config.rs\n",
            "@@ -1 +1 @@\n",
            "-const DEFAULT: &str = \"{\\\"build_success\\\":0.1}\";\n",
            "+const DEFAULT: &str = \"still not metrics\";\n",
            "```\n",
            "Approach: keep the diff stable.\n",
            "{\"build_success\":1.0,\"test_pass_rate\":0.75,\"signal_resolution\":0.5}"
        );

        assert_eq!(
            extract_json_block(text),
            Some(
                "{\"build_success\":1.0,\"test_pass_rate\":0.75,\"signal_resolution\":0.5}"
                    .to_owned()
            )
        );
    }

    fn sample_experiment() -> fx_consensus::Experiment {
        fx_consensus::Experiment {
            id: Uuid::new_v4(),
            trigger: fx_consensus::Signal {
                id: Uuid::new_v4(),
                name: "latency".to_owned(),
                description: "High latency detected".to_owned(),
                severity: fx_consensus::Severity::High,
            },
            hypothesis: "parallelism helps".to_owned(),
            fitness_criteria: vec![fx_consensus::FitnessCriterion {
                name: "build_success".to_owned(),
                metric_type: fx_consensus::MetricType::Higher,
                weight: 1.0,
            }],
            scope: fx_consensus::ModificationScope {
                allowed_files: vec![fx_consensus::PathPattern::from("src/**/*.rs")],
                proposal_tier: fx_consensus::ProposalTier::Tier1,
            },
            timeout: Duration::from_secs(60),
            min_candidates: 1,
            created_at: Utc::now(),
        }
    }
}
