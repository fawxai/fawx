use crate::{
    format_model_response, CompletionExample, ExampleKind, PreferenceExample, TrainingExample,
};
use chrono::Utc;
use fx_consensus::chain::ChainEntry;
use fx_consensus::types::Decision;
use fx_consensus::{build_experiment_prompt, format_chain_history};
use std::collections::BTreeMap;
use std::path::Path;
use uuid::Uuid;

pub const DEFAULT_SYSTEM_PROMPT: &str =
    "You are a code improvement agent. Generate patches to fix issues.";

#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    pub min_winner_score: f64,
    pub min_preference_delta: f64,
    pub include_negative_examples: bool,
    pub max_patch_bytes: usize,
    pub signal_filter: Option<String>,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            min_winner_score: 0.7,
            min_preference_delta: 0.2,
            include_negative_examples: true,
            max_patch_bytes: 50_000,
            signal_filter: None,
        }
    }
}

pub trait ChainExtractor: Send + Sync {
    fn extract(
        &self,
        entries: &[ChainEntry],
        chain_path: &Path,
        config: &ExtractionConfig,
    ) -> Vec<TrainingExample>;
}

pub struct DefaultChainExtractor;

impl ChainExtractor for DefaultChainExtractor {
    fn extract(
        &self,
        entries: &[ChainEntry],
        chain_path: &Path,
        config: &ExtractionConfig,
    ) -> Vec<TrainingExample> {
        let mut examples = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let chain_history = &entries[..index];
            if !signal_matches(entry, config) {
                continue;
            }
            if let Some(ex) = try_completion(entry, chain_path, chain_history, config) {
                examples.push(ex);
            }
            if let Some(ex) = try_preference(entry, chain_path, chain_history, config) {
                examples.push(ex);
            }
            if config.include_negative_examples {
                if let Some(ex) = try_negative(entry, chain_path, chain_history) {
                    examples.push(ex);
                }
            }
        }
        examples
    }
}

fn signal_matches(entry: &ChainEntry, config: &ExtractionConfig) -> bool {
    match &config.signal_filter {
        Some(filter) => entry.experiment.trigger.name.eq_ignore_ascii_case(filter),
        None => true,
    }
}

fn build_user_prompt(entry: &ChainEntry, chain_history: &[ChainEntry]) -> String {
    build_experiment_prompt(&entry.experiment, &format_chain_history(chain_history))
}

fn best_aggregate_score(entry: &ChainEntry) -> f64 {
    entry
        .result
        .aggregate_scores
        .values()
        .copied()
        .fold(0.0_f64, f64::max)
}

fn build_tags(entry: &ChainEntry) -> Vec<String> {
    let mut tags = vec![format!("signal:{}", entry.experiment.trigger.name)];
    if let Some(first) = entry.experiment.scope.allowed_files.first() {
        tags.push(format!("scope:{}", first.0));
    }
    tags
}

fn try_completion(
    entry: &ChainEntry,
    chain_path: &Path,
    chain_history: &[ChainEntry],
    config: &ExtractionConfig,
) -> Option<TrainingExample> {
    if entry.result.decision != Decision::Accept {
        return None;
    }
    let patch = entry.winning_patch.as_ref()?;
    if patch.trim().is_empty() || patch.len() > config.max_patch_bytes {
        return None;
    }
    let score = best_aggregate_score(entry);
    if score < config.min_winner_score {
        return None;
    }
    let user_prompt = build_user_prompt(entry, chain_history);
    let metrics = BTreeMap::from([("aggregate_score".to_owned(), score)]);
    let response = format_model_response(patch, "Accepted patch", &metrics);
    Some(TrainingExample {
        id: Uuid::new_v4(),
        source_chain_index: entry.index,
        source_chain_path: chain_path.to_path_buf(),
        curated_at: Utc::now(),
        kind: ExampleKind::Completion(CompletionExample {
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_owned(),
            user_prompt,
            assistant_response: response,
        }),
        quality_score: score,
        tags: build_tags(entry),
    })
}

fn try_preference(
    entry: &ChainEntry,
    chain_path: &Path,
    chain_history: &[ChainEntry],
    config: &ExtractionConfig,
) -> Option<TrainingExample> {
    let patches = &entry.result.candidate_patches;
    let scores = &entry.result.aggregate_scores;
    if patches.len() < 2 || scores.len() < 2 {
        return None;
    }
    let (best_id, best_score) = scores.iter().max_by(|a, b| a.1.total_cmp(b.1))?;
    let (worst_id, worst_score) = scores.iter().min_by(|a, b| a.1.total_cmp(b.1))?;
    if best_id == worst_id {
        return None;
    }
    let delta = best_score - worst_score;
    if delta < config.min_preference_delta {
        return None;
    }
    let chosen = patches.get(best_id)?;
    let rejected = patches.get(worst_id)?;
    let user_prompt = build_user_prompt(entry, chain_history);
    Some(TrainingExample {
        id: Uuid::new_v4(),
        source_chain_index: entry.index,
        source_chain_path: chain_path.to_path_buf(),
        curated_at: Utc::now(),
        kind: ExampleKind::Preference(PreferenceExample {
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_owned(),
            user_prompt,
            chosen: chosen.clone(),
            rejected: rejected.clone(),
            score_delta: delta,
        }),
        quality_score: delta,
        tags: build_tags(entry),
    })
}

fn try_negative(
    entry: &ChainEntry,
    chain_path: &Path,
    chain_history: &[ChainEntry],
) -> Option<TrainingExample> {
    if entry.result.decision != Decision::Reject {
        return None;
    }
    let patch = entry.winning_patch.as_ref()?;
    if patch.trim().is_empty() {
        return None;
    }
    let user_prompt = build_user_prompt(entry, chain_history);
    let metrics = BTreeMap::new();
    let response = format_model_response(patch, "Rejected patch", &metrics);
    Some(TrainingExample {
        id: Uuid::new_v4(),
        source_chain_index: entry.index,
        source_chain_path: chain_path.to_path_buf(),
        curated_at: Utc::now(),
        kind: ExampleKind::Completion(CompletionExample {
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_owned(),
            user_prompt,
            assistant_response: response,
        }),
        quality_score: 0.0,
        tags: build_tags(entry),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_consensus::types::*;
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn sample_experiment() -> Experiment {
        Experiment {
            id: uuid::Uuid::new_v4(),
            trigger: Signal {
                id: uuid::Uuid::new_v4(),
                name: "latency".to_owned(),
                description: "High latency".to_owned(),
                severity: Severity::High,
            },
            hypothesis: "test".to_owned(),
            fitness_criteria: vec![FitnessCriterion {
                name: "build_success".to_owned(),
                metric_type: MetricType::Higher,
                weight: 1.0,
            }],
            scope: ModificationScope {
                allowed_files: vec![PathPattern::from("src/**/*.rs")],
                proposal_tier: ProposalTier::Tier1,
            },
            timeout: Duration::from_secs(60),
            min_candidates: 1,
            created_at: Utc::now(),
        }
    }

    fn accept_entry(score: f64, patch: &str) -> ChainEntry {
        let experiment = sample_experiment();
        let candidate_id = uuid::Uuid::new_v4();
        ChainEntry {
            index: 0,
            previous_hash: "genesis".to_owned(),
            experiment: experiment.clone(),
            result: ConsensusResult {
                experiment_id: experiment.id,
                winner: Some(candidate_id),
                candidates: vec![candidate_id],
                candidate_nodes: BTreeMap::from([(candidate_id, NodeId::from("node-a"))]),
                candidate_patches: BTreeMap::from([(candidate_id, patch.to_owned())]),
                evaluations: Vec::new(),
                aggregate_scores: BTreeMap::from([(candidate_id, score)]),
                decision: Decision::Accept,
                timestamp: Utc::now(),
            },
            winning_patch: Some(patch.to_owned()),
            applied_at: None,
            hash: "hash-0".to_owned(),
        }
    }

    fn reject_entry(patch: &str) -> ChainEntry {
        let mut entry = accept_entry(0.1, patch);
        entry.result.decision = Decision::Reject;
        entry
    }

    fn multi_candidate_entry(score_a: f64, score_b: f64) -> ChainEntry {
        let experiment = sample_experiment();
        let id_a = uuid::Uuid::new_v4();
        let id_b = uuid::Uuid::new_v4();
        ChainEntry {
            index: 0,
            previous_hash: "genesis".to_owned(),
            experiment: experiment.clone(),
            result: ConsensusResult {
                experiment_id: experiment.id,
                winner: Some(id_a),
                candidates: vec![id_a, id_b],
                candidate_nodes: BTreeMap::from([
                    (id_a, NodeId::from("node-a")),
                    (id_b, NodeId::from("node-b")),
                ]),
                candidate_patches: BTreeMap::from([
                    (id_a, "diff a".to_owned()),
                    (id_b, "diff b".to_owned()),
                ]),
                evaluations: Vec::new(),
                aggregate_scores: BTreeMap::from([(id_a, score_a), (id_b, score_b)]),
                decision: Decision::Accept,
                timestamp: Utc::now(),
            },
            winning_patch: Some("diff a".to_owned()),
            applied_at: None,
            hash: "hash-0".to_owned(),
        }
    }

    #[test]
    fn extracts_completion_from_accept() {
        let entry = accept_entry(0.9, "diff --git a/src/lib.rs b/src/lib.rs");
        let config = ExtractionConfig::default();
        let extractor = DefaultChainExtractor;

        let examples = extractor.extract(&[entry], Path::new("chain.json"), &config);

        assert_eq!(examples.len(), 1);
        assert!(matches!(examples[0].kind, ExampleKind::Completion(_)));
        assert!(examples[0].quality_score >= 0.9);
    }

    #[test]
    fn skips_low_score_completion() {
        let entry = accept_entry(0.3, "diff");
        let config = ExtractionConfig::default();
        let extractor = DefaultChainExtractor;

        let examples = extractor.extract(&[entry], Path::new("chain.json"), &config);

        assert!(examples
            .iter()
            .all(|e| !matches!(e.kind, ExampleKind::Completion(_)) || e.quality_score == 0.0));
    }

    #[test]
    fn extracts_preference_from_multi_candidate() {
        let entry = multi_candidate_entry(0.9, 0.3);
        let config = ExtractionConfig::default();
        let extractor = DefaultChainExtractor;

        let examples = extractor.extract(&[entry], Path::new("chain.json"), &config);

        let prefs: Vec<_> = examples
            .iter()
            .filter(|e| matches!(e.kind, ExampleKind::Preference(_)))
            .collect();
        assert_eq!(prefs.len(), 1);
    }

    #[test]
    fn extracts_negative_from_reject() {
        let entry = reject_entry("diff --git");
        let config = ExtractionConfig {
            include_negative_examples: true,
            ..ExtractionConfig::default()
        };
        let extractor = DefaultChainExtractor;

        let examples = extractor.extract(&[entry], Path::new("chain.json"), &config);

        let negatives: Vec<_> = examples.iter().filter(|e| e.quality_score == 0.0).collect();
        assert_eq!(negatives.len(), 1);
    }

    #[test]
    fn skips_negative_when_disabled() {
        let entry = reject_entry("diff");
        let config = ExtractionConfig {
            include_negative_examples: false,
            ..ExtractionConfig::default()
        };
        let extractor = DefaultChainExtractor;

        let examples = extractor.extract(&[entry], Path::new("chain.json"), &config);

        assert!(examples.is_empty());
    }

    #[test]
    fn signal_filter_works() {
        let entry = accept_entry(0.9, "diff");
        let config = ExtractionConfig {
            signal_filter: Some("throughput".to_owned()),
            ..ExtractionConfig::default()
        };
        let extractor = DefaultChainExtractor;

        let examples = extractor.extract(&[entry], Path::new("chain.json"), &config);

        assert!(examples.is_empty());
    }

    #[test]
    fn max_patch_bytes_filter() {
        let big_patch = "x".repeat(60_000);
        let entry = accept_entry(0.9, &big_patch);
        let config = ExtractionConfig::default();
        let extractor = DefaultChainExtractor;

        let examples = extractor.extract(&[entry], Path::new("chain.json"), &config);

        assert!(examples
            .iter()
            .all(|e| !matches!(e.kind, ExampleKind::Completion(ref c) if c.assistant_response.contains(&big_patch))));
    }

    fn prompt_for(example: TrainingExample) -> String {
        match example.kind {
            ExampleKind::Completion(c) => c.user_prompt,
            ExampleKind::Preference(p) => p.user_prompt,
        }
    }

    #[test]
    fn completion_uses_chain_history_in_prompt() {
        let history = vec![accept_entry(0.8, "diff --git a/src/lib.rs b/src/lib.rs")];
        let entry = accept_entry(0.9, "diff --git a/src/main.rs b/src/main.rs");

        let example = try_completion(
            &entry,
            Path::new("chain.json"),
            &history,
            &ExtractionConfig::default(),
        )
        .expect("completion example");

        assert!(prompt_for(example).contains("Entry #0"));
    }

    #[test]
    fn preference_uses_chain_history_in_prompt() {
        let history = vec![accept_entry(0.8, "diff --git a/src/lib.rs b/src/lib.rs")];
        let entry = multi_candidate_entry(0.9, 0.3);

        let example = try_preference(
            &entry,
            Path::new("chain.json"),
            &history,
            &ExtractionConfig::default(),
        )
        .expect("preference example");

        assert!(prompt_for(example).contains("Entry #0"));
    }

    #[test]
    fn negative_uses_chain_history_in_prompt() {
        let history = vec![accept_entry(0.8, "diff --git a/src/lib.rs b/src/lib.rs")];
        let entry = reject_entry("diff --git a/src/main.rs b/src/main.rs");

        let example =
            try_negative(&entry, Path::new("chain.json"), &history).expect("negative example");

        assert!(prompt_for(example).contains("Entry #0"));
    }

    #[test]
    fn empty_chain_produces_no_examples() {
        let extractor = DefaultChainExtractor;
        let examples =
            extractor.extract(&[], Path::new("chain.json"), &ExtractionConfig::default());
        assert!(examples.is_empty());
    }

    #[test]
    fn preference_skipped_for_small_delta() {
        let entry = multi_candidate_entry(0.5, 0.45);
        let config = ExtractionConfig::default();
        let extractor = DefaultChainExtractor;

        let examples = extractor.extract(&[entry], Path::new("chain.json"), &config);

        let prefs: Vec<_> = examples
            .iter()
            .filter(|e| matches!(e.kind, ExampleKind::Preference(_)))
            .collect();
        assert!(prefs.is_empty());
    }
}
