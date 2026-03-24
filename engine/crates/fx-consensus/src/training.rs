//! Training data curation from experiment chain winners.
//!
//! Extracts winning patches from experiment chains into structured
//! training examples for LoRA fine-tuning.

use crate::chain::ChainEntry;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A single training example derived from a chain winner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainingExample {
    pub id: String,
    pub source_hash: String,
    pub prompt: String,
    pub completion: String,
    pub score: f64,
    pub scope: String,
    pub metadata: TrainingMetadata,
}

/// Metadata about a training example's provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainingMetadata {
    pub experiment_id: String,
    pub round: u64,
    pub timestamp: Option<String>,
    pub from_chain: bool,
    pub num_evaluations: usize,
}

/// Configuration for training data curation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurationConfig {
    pub min_score: f64,
    pub max_per_chain: usize,
}

impl Default for CurationConfig {
    fn default() -> Self {
        Self {
            min_score: 0.5,
            max_per_chain: 10,
        }
    }
}

/// A curated training dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingDataset {
    pub examples: Vec<TrainingExample>,
    pub config: CurationConfig,
    pub chains_processed: usize,
    pub entries_scanned: usize,
    pub filtered_count: usize,
}

impl TrainingDataset {
    pub fn new(config: CurationConfig) -> Self {
        Self {
            examples: Vec::new(),
            config,
            chains_processed: 0,
            entries_scanned: 0,
            filtered_count: 0,
        }
    }

    /// Extract training examples from chain entries.
    pub fn extract_from_entries(&mut self, entries: &[ChainEntry]) {
        self.chains_processed += 1;
        let mut extracted = 0;

        for entry in entries {
            self.entries_scanned += 1;
            if extracted >= self.config.max_per_chain {
                break;
            }
            if let Some(example) = entry_to_example(entry) {
                if example.score >= self.config.min_score {
                    self.examples.push(example);
                    extracted += 1;
                } else {
                    self.filtered_count += 1;
                }
            }
        }
    }

    /// Save to JSONL (one example per line).
    pub fn save_jsonl(&self, path: &Path) -> Result<(), std::io::Error> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(path)?;
        for example in &self.examples {
            let line = serde_json::to_string(example).map_err(std::io::Error::other)?;
            writeln!(file, "{line}")?;
        }
        Ok(())
    }

    /// Load from JSONL.
    pub fn load_jsonl(path: &Path, config: CurationConfig) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        let examples: Vec<TrainingExample> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        Ok(Self {
            examples,
            config,
            chains_processed: 0,
            entries_scanned: 0,
            filtered_count: 0,
        })
    }

    pub fn summary(&self) -> String {
        format!(
            "{} examples from {} chains ({} scanned, {} filtered)",
            self.examples.len(),
            self.chains_processed,
            self.entries_scanned,
            self.filtered_count,
        )
    }
}

fn entry_to_example(entry: &ChainEntry) -> Option<TrainingExample> {
    let patch = entry.winning_patch.as_ref()?;
    if patch.trim().is_empty() {
        return None;
    }

    let result = &entry.result;
    let best_score = best_aggregate_score(result);

    let experiment_id = result.experiment_id.to_string();
    let scope = entry
        .experiment
        .scope
        .allowed_files
        .iter()
        .map(|p| p.0.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let prompt = build_prompt(&entry.experiment.trigger, &scope);
    let timestamp = entry.applied_at.map(|t| t.to_rfc3339());

    Some(TrainingExample {
        id: format!("{}_{}", experiment_id, entry.index),
        source_hash: entry.hash.clone(),
        prompt,
        completion: patch.clone(),
        score: best_score,
        scope,
        metadata: TrainingMetadata {
            experiment_id,
            round: entry.index,
            timestamp,
            from_chain: entry.index > 0,
            num_evaluations: result.evaluations.len(),
        },
    })
}

fn best_aggregate_score(result: &crate::types::ConsensusResult) -> f64 {
    result
        .aggregate_scores
        .values()
        .copied()
        .fold(0.0_f64, f64::max)
}

fn build_prompt(signal: &crate::types::Signal, scope: &str) -> String {
    let signal_str = serde_json::to_string_pretty(signal).unwrap_or_default();
    format!("Signal:\n{signal_str}\n\nScope: {scope}\n\nGenerate a patch.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn training_example_serializes() {
        let example = TrainingExample {
            id: "exp1_0".into(),
            source_hash: "abc123".into(),
            prompt: "Fix the bug".into(),
            completion: "+ fix".into(),
            score: 0.95,
            scope: "src/main.rs".into(),
            metadata: TrainingMetadata {
                experiment_id: "exp1".into(),
                round: 0,
                timestamp: None,
                from_chain: false,
                num_evaluations: 3,
            },
        };

        let json = serde_json::to_string(&example).unwrap();
        let decoded: TrainingExample = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, example);
    }

    #[test]
    fn curation_config_defaults() {
        let config = CurationConfig::default();
        assert_eq!(config.min_score, 0.5);
        assert_eq!(config.max_per_chain, 10);
    }

    #[test]
    fn dataset_summary_formats() {
        let mut dataset = TrainingDataset::new(CurationConfig::default());
        dataset.chains_processed = 3;
        dataset.entries_scanned = 15;
        dataset.filtered_count = 2;

        let summary = dataset.summary();
        assert!(summary.contains("0 examples from 3 chains"));
    }

    #[test]
    fn jsonl_round_trip() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("test.jsonl");

        let mut dataset = TrainingDataset::new(CurationConfig::default());
        dataset.examples.push(TrainingExample {
            id: "test_0".into(),
            source_hash: "hash".into(),
            prompt: "prompt".into(),
            completion: "completion".into(),
            score: 1.0,
            scope: "scope".into(),
            metadata: TrainingMetadata {
                experiment_id: "test".into(),
                round: 0,
                timestamp: None,
                from_chain: false,
                num_evaluations: 1,
            },
        });

        dataset.save_jsonl(&path).unwrap();
        let loaded = TrainingDataset::load_jsonl(&path, CurationConfig::default()).unwrap();
        assert_eq!(loaded.examples.len(), 1);
        assert_eq!(loaded.examples[0].completion, "completion");
    }
}
