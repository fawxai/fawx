use crate::response_parser::parse_patch_response;
use crate::{ChainEntry, ChainStorage, Experiment, JsonFileChainStorage, PatchSource};
use fx_llm::{completion_text, CompletionRequest, Message, ModelRouter};
use std::path::Path;
use std::sync::Arc;

pub struct LlmPatchSource {
    router: Arc<ModelRouter>,
    model: String,
    chain_history: String,
}

impl LlmPatchSource {
    pub fn new(router: Arc<ModelRouter>, model: String) -> Self {
        Self {
            router,
            model,
            chain_history: String::new(),
        }
    }

    pub fn with_chain_history(mut self, chain_history: String) -> Self {
        self.chain_history = chain_history;
        self
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
            messages: vec![Message::user(build_experiment_prompt(
                experiment,
                &self.chain_history,
            ))],
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

pub const CHAIN_HISTORY_LIMIT: usize = 5;

pub fn load_chain_history_for_signal(path: &Path, signal: &str) -> crate::Result<String> {
    let storage = JsonFileChainStorage::new(path);
    let chain = storage.load()?;
    Ok(format_chain_history(
        &chain.recent_entries_for_signal(signal, CHAIN_HISTORY_LIMIT),
    ))
}

pub fn format_chain_history(entries: &[ChainEntry]) -> String {
    if entries.is_empty() {
        return "- No previous experiments recorded for this signal yet.".to_owned();
    }
    entries
        .iter()
        .map(format_chain_history_entry)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_chain_history_entry(entry: &ChainEntry) -> String {
    format!(
        concat!(
            "- Entry #{index} ({timestamp}) | hypothesis: {hypothesis} | decision: {decision} | ",
            "winner: {winner} | outcome: {outcome} | notes: {notes}"
        ),
        index = entry.index,
        timestamp = entry.result.timestamp.to_rfc3339(),
        hypothesis = entry.experiment.hypothesis,
        decision = entry.result.decision.lowercase_label(),
        winner = winner_label(entry),
        outcome = outcome_label(entry),
        notes = evaluation_notes(entry),
    )
}

fn winner_label(entry: &ChainEntry) -> String {
    entry
        .result
        .winner
        .and_then(|candidate_id| entry.result.candidate_nodes.get(&candidate_id))
        .map(|node_id| node_id.0.clone())
        .unwrap_or_else(|| "none".to_owned())
}

fn outcome_label(entry: &ChainEntry) -> String {
    let signal_resolved = entry
        .result
        .evaluations
        .iter()
        .any(|evaluation| evaluation.signal_resolved);
    let regression_detected = entry
        .result
        .evaluations
        .iter()
        .any(|evaluation| evaluation.regression_detected);
    match (regression_detected, signal_resolved) {
        (true, true) => "signal resolved with regression".to_owned(),
        (true, false) => "regression detected".to_owned(),
        (false, true) => "signal resolved".to_owned(),
        (false, false) => "signal unresolved".to_owned(),
    }
}

fn evaluation_notes(entry: &ChainEntry) -> String {
    let notes = entry
        .result
        .evaluations
        .iter()
        .map(|evaluation| {
            format!(
                "{} on {}: resolved={}, regression={}, safety={} ({})",
                evaluation.evaluator_id.0,
                candidate_name(entry, evaluation.candidate_id),
                bool_label(evaluation.signal_resolved),
                bool_label(evaluation.regression_detected),
                bool_label(evaluation.safety_pass),
                truncate_note(&evaluation.notes),
            )
        })
        .collect::<Vec<_>>();
    if notes.is_empty() {
        "no evaluator notes recorded".to_owned()
    } else {
        notes.join("; ")
    }
}

fn candidate_name(entry: &ChainEntry, candidate_id: uuid::Uuid) -> String {
    entry
        .result
        .candidate_nodes
        .get(&candidate_id)
        .map(|node_id| node_id.0.clone())
        .unwrap_or_else(|| candidate_id.to_string())
}

fn bool_label(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn truncate_note(note: &str) -> String {
    let compact = note.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= 120 {
        return compact;
    }
    let truncated = compact.chars().take(117).collect::<String>();
    format!("{truncated}...")
}

fn chain_history_section(chain_history: &str) -> String {
    format!(
        concat!(
            "Recent experiments for this signal:\n",
            "{chain_history}\n\n",
            "Use this history to avoid repeating failed hypotheses and build on the best prior result."
        ),
        chain_history = chain_history
    )
}

pub fn build_experiment_prompt(experiment: &Experiment, chain_history: &str) -> String {
    let scope = experiment_scope(experiment);
    let criteria = experiment_criteria(experiment);
    let history = chain_history_section(chain_history);
    format!(
        concat!(
            "You are participating in a proof-of-fitness experiment.\n\n",
            "Signal: {} — {}\n",
            "Hypothesis: {}\n",
            "Allowed files: {}\n",
            "Fitness criteria: {}\n\n",
            "{}\n\n",
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
        history,
    )
}

pub fn build_subagent_experiment_prompt(experiment: &Experiment, chain_history: &str) -> String {
    let scope = experiment_scope(experiment);
    let criteria = experiment
        .fitness_criteria
        .iter()
        .map(|criterion| format!("{} (weight: {})", criterion.name, criterion.weight))
        .collect::<Vec<_>>()
        .join(", ");
    let history = chain_history_section(chain_history);
    format!(
        concat!(
            "You are a Fawx agent participating in a proof-of-fitness experiment.\n\n",
            "Signal: {} — {}\n",
            "Hypothesis: {}\n",
            "Target files: {}\n",
            "Fitness criteria: {}\n\n",
            "{}\n\n",
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
            "- Run: `cargo check 2>&1` via run_command (from the project root)\n",
            "- If check fails, read the errors, fix them with edit_file, and rerun it\n",
            "- Run: `cargo test 2>&1` via run_command\n",
            "- If tests fail, read the errors, fix them with edit_file, and retest\n",
            "- REPEAT until both check and test pass with zero errors\n",
            "- You MUST see a successful cargo check and test output before proceeding\n\n",
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
            "- NEVER output <PATCH> until cargo check AND cargo test pass\n",
            "- NEVER assume helper functions exist — define them or verify with read_file\n",
            "- NEVER generate a diff from memory — always use `git diff` output\n",
            "- If you cannot make the code compile after 3 attempts, output an empty <PATCH></PATCH> with build_success: 0.0"
        ),
        experiment.trigger.name,
        experiment.trigger.description,
        experiment.hypothesis,
        scope,
        criteria,
        history,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::write_chain_with_signals;
    use crate::types::Decision;
    use chrono::Utc;
    use futures::stream;
    use fx_llm::{
        CompletionResponse, CompletionStream, ContentBlock, ProviderCapabilities, ProviderError,
    };
    use std::time::Duration;
    use tempfile::tempdir;
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
    fn format_chain_history_returns_empty_message_for_empty_entries() {
        assert_eq!(
            format_chain_history(&[]),
            "- No previous experiments recorded for this signal yet."
        );
    }

    #[test]
    fn format_chain_history_summarizes_previous_attempts() {
        let history = format_chain_history(&[sample_chain_entry(
            3,
            Decision::Reject,
            "build_ok=false; tests=0/12, failed=12",
        )]);

        assert!(history.contains("Entry #3"));
        assert!(history.contains("hypothesis: parallelism helps"));
        assert!(history.contains("decision: reject"));
        assert!(history.contains("winner: node-a"));
        assert!(history.contains("build_ok=false; tests=0/12, failed=12"));
    }

    #[test]
    fn format_chain_history_reports_resolved_with_regression() {
        let mut entry = sample_chain_entry(3, Decision::Accept, "mixed result");
        let candidate_id = entry.result.candidates[0];
        entry.result.evaluations = vec![
            evaluation(candidate_id, "node-b", true, false, "resolved"),
            evaluation(candidate_id, "node-c", false, true, "regressed"),
        ];

        let history = format_chain_history(&[entry]);

        assert!(history.contains("outcome: signal resolved with regression"));
    }

    #[test]
    fn load_chain_history_for_signal_filters_to_matching_signal() {
        let temp = tempdir().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_chain_with_signals(
            &chain_path,
            [
                ("latency", "parallelism helps", "build failed everywhere"),
                ("throughput", "batching helps", "not relevant"),
                ("Latency", "cache warmup helps", "tests improved"),
            ],
        );

        let history = load_chain_history_for_signal(&chain_path, " latency ").expect("history");

        assert!(history.contains("parallelism helps"));
        assert!(history.contains("cache warmup helps"));
        assert!(!history.contains("batching helps"));
    }

    #[test]
    fn build_subagent_experiment_prompt_includes_tool_instructions_and_history() {
        let prompt =
            build_subagent_experiment_prompt(&sample_experiment(), &sample_chain_history());

        assert!(prompt.contains("Recent experiments for this signal:"));
        assert!(prompt.contains("Entry #3"));
        assert!(prompt.contains("Use this history to avoid repeating failed hypotheses"));
        assert!(prompt.contains("You MUST use tools"));
        assert!(prompt.contains("Use read_file to read EVERY target file"));
        assert!(prompt.contains("cargo check 2>&1"));
        assert!(prompt.contains("NEVER output <PATCH> until cargo check AND cargo test pass"));
        assert!(prompt.contains("NEVER assume helper functions exist"));
    }

    #[test]
    fn build_experiment_prompt_stays_direct_llm_focused_and_includes_history() {
        let prompt = build_experiment_prompt(&sample_experiment(), &sample_chain_history());

        assert!(prompt.contains("Recent experiments for this signal:"));
        assert!(prompt.contains("Entry #3"));
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

    fn sample_chain_history() -> String {
        format_chain_history(&[sample_chain_entry(
            3,
            Decision::Reject,
            "build_ok=false; tests=0/12, failed=12",
        )])
    }

    fn evaluation(
        candidate_id: Uuid,
        evaluator_id: &str,
        signal_resolved: bool,
        regression_detected: bool,
        notes: &str,
    ) -> crate::Evaluation {
        crate::Evaluation {
            candidate_id,
            evaluator_id: crate::NodeId::from(evaluator_id),
            fitness_scores: std::collections::BTreeMap::from([("build_success".to_owned(), 0.2)]),
            safety_pass: true,
            signal_resolved,
            regression_detected,
            notes: notes.to_owned(),
            created_at: Utc::now(),
        }
    }

    fn sample_chain_entry(index: u64, decision: Decision, notes: &str) -> ChainEntry {
        let experiment = sample_experiment();
        let candidate_id = Uuid::from_u128(index as u128 + 1);
        ChainEntry {
            index,
            previous_hash: if index == 0 {
                "genesis".to_owned()
            } else {
                format!("hash-{}", index - 1)
            },
            experiment: experiment.clone(),
            result: crate::ConsensusResult {
                experiment_id: experiment.id,
                winner: Some(candidate_id),
                candidates: vec![candidate_id],
                candidate_nodes: std::collections::BTreeMap::from([(
                    candidate_id,
                    crate::NodeId::from("node-a"),
                )]),
                candidate_patches: std::collections::BTreeMap::new(),
                evaluations: vec![crate::Evaluation {
                    candidate_id,
                    evaluator_id: crate::NodeId::from("node-b"),
                    fitness_scores: std::collections::BTreeMap::from([(
                        "build_success".to_owned(),
                        0.2,
                    )]),
                    safety_pass: true,
                    signal_resolved: false,
                    regression_detected: decision == Decision::Reject,
                    notes: notes.to_owned(),
                    created_at: Utc::now(),
                }],
                aggregate_scores: std::collections::BTreeMap::from([(candidate_id, 0.2)]),
                decision,
                timestamp: Utc::now(),
            },
            winning_patch: None,
            applied_at: None,
            hash: format!("hash-{index}"),
        }
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
