use crate::{example_content_hash, signal_tag, ExampleKind, TrainingExample};
use std::collections::{HashMap, HashSet};

pub trait QualityFilter: Send + Sync {
    fn filter(&self, examples: Vec<TrainingExample>) -> Vec<TrainingExample>;
}

pub struct DeduplicationFilter;

impl QualityFilter for DeduplicationFilter {
    fn filter(&self, examples: Vec<TrainingExample>) -> Vec<TrainingExample> {
        let mut seen = HashSet::new();
        examples
            .into_iter()
            .filter(|example| seen.insert(example_content_hash(example)))
            .collect()
    }
}

pub struct PatchQualityFilter;

impl QualityFilter for PatchQualityFilter {
    fn filter(&self, examples: Vec<TrainingExample>) -> Vec<TrainingExample> {
        examples.into_iter().map(adjust_patch_quality).collect()
    }
}

fn adjust_patch_quality(mut example: TrainingExample) -> TrainingExample {
    match &example.kind {
        ExampleKind::Completion(c) => {
            if !has_diff_markers(&c.assistant_response) {
                example.quality_score = (example.quality_score - 0.3).max(0.0);
            }
            if c.assistant_response.trim().is_empty() {
                example.quality_score = 0.0;
            }
        }
        ExampleKind::Preference(p) => {
            if p.chosen.trim().is_empty() {
                example.quality_score = 0.0;
            }
        }
    }
    example
}

fn has_diff_markers(text: &str) -> bool {
    text.contains("diff --git") || (text.contains("---") && text.contains("+++"))
}

pub struct DiversityFilter {
    pub max_signal_ratio: f64,
}

impl Default for DiversityFilter {
    fn default() -> Self {
        Self {
            max_signal_ratio: 0.5,
        }
    }
}

impl QualityFilter for DiversityFilter {
    fn filter(&self, examples: Vec<TrainingExample>) -> Vec<TrainingExample> {
        let total = examples.len();
        if total == 0 {
            return examples;
        }
        let max_per_signal = ((total as f64) * self.max_signal_ratio).ceil() as usize;
        let max_per_signal = max_per_signal.max(1);
        let mut signal_counts: HashMap<String, usize> = HashMap::new();
        examples
            .into_iter()
            .filter(|example| {
                let signal = signal_tag(&example.tags).unwrap_or("unknown").to_owned();
                let count = signal_counts.entry(signal).or_insert(0);
                *count += 1;
                *count <= max_per_signal
            })
            .collect()
    }
}

pub struct ScoreThresholdFilter {
    pub min_score: f64,
}

impl QualityFilter for ScoreThresholdFilter {
    fn filter(&self, examples: Vec<TrainingExample>) -> Vec<TrainingExample> {
        examples
            .into_iter()
            .filter(|e| e.quality_score >= self.min_score)
            .collect()
    }
}

pub struct FilterPipeline {
    filters: Vec<Box<dyn QualityFilter>>,
}

impl FilterPipeline {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    pub fn with_filter(mut self, filter: Box<dyn QualityFilter>) -> Self {
        self.filters.push(filter);
        self
    }
}

impl Default for FilterPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl QualityFilter for FilterPipeline {
    fn filter(&self, mut examples: Vec<TrainingExample>) -> Vec<TrainingExample> {
        for f in &self.filters {
            examples = f.filter(examples);
        }
        examples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompletionExample, PreferenceExample};
    use chrono::Utc;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn completion(prompt: &str, response: &str, score: f64, signal: &str) -> TrainingExample {
        TrainingExample {
            id: Uuid::new_v4(),
            source_chain_index: 0,
            source_chain_path: PathBuf::from("chain.json"),
            curated_at: Utc::now(),
            kind: ExampleKind::Completion(CompletionExample {
                system_prompt: "sys".to_owned(),
                user_prompt: prompt.to_owned(),
                assistant_response: response.to_owned(),
            }),
            quality_score: score,
            tags: vec![format!("signal:{signal}")],
        }
    }

    fn preference(prompt: &str, chosen: &str, rejected: &str, score: f64) -> TrainingExample {
        TrainingExample {
            id: Uuid::new_v4(),
            source_chain_index: 0,
            source_chain_path: PathBuf::from("chain.json"),
            curated_at: Utc::now(),
            kind: ExampleKind::Preference(PreferenceExample {
                system_prompt: "sys".to_owned(),
                user_prompt: prompt.to_owned(),
                chosen: chosen.to_owned(),
                rejected: rejected.to_owned(),
                score_delta: score,
            }),
            quality_score: score,
            tags: vec!["signal:latency".to_owned()],
        }
    }

    #[test]
    fn dedup_removes_identical() {
        let a = completion("p", "r", 0.9, "latency");
        let b = completion("p", "r", 0.8, "latency");
        let result = DeduplicationFilter.filter(vec![a, b]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn dedup_keeps_different() {
        let a = completion("p1", "r1", 0.9, "latency");
        let b = completion("p2", "r2", 0.8, "latency");
        let result = DeduplicationFilter.filter(vec![a, b]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn patch_quality_reduces_score_without_diff_markers() {
        let ex = completion("p", "just some text", 0.9, "latency");
        let result = PatchQualityFilter.filter(vec![ex]);
        assert!(result[0].quality_score < 0.9);
    }

    #[test]
    fn patch_quality_keeps_diff_markers() {
        let ex = completion("p", "diff --git a/src b/src\n---\n+++", 0.9, "latency");
        let result = PatchQualityFilter.filter(vec![ex]);
        assert!((result[0].quality_score - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn diff_markers_require_git_header_or_both_file_markers() {
        assert!(has_diff_markers("diff --git a/src/lib.rs b/src/lib.rs"));
        assert!(has_diff_markers(
            "--- a/src/lib.rs
+++ b/src/lib.rs"
        ));
        assert!(!has_diff_markers("--- a/src/lib.rs"));
        assert!(!has_diff_markers("+++ b/src/lib.rs"));
    }

    #[test]
    fn patch_quality_zeros_empty() {
        let ex = completion("p", "", 0.9, "latency");
        let result = PatchQualityFilter.filter(vec![ex]);
        assert_eq!(result[0].quality_score, 0.0);
    }

    #[test]
    fn diversity_caps_signal() {
        let examples: Vec<_> = (0..10)
            .map(|i| completion(&format!("p{i}"), &format!("r{i}"), 0.9, "latency"))
            .collect();
        let filter = DiversityFilter {
            max_signal_ratio: 0.5,
        };
        let result = filter.filter(examples);
        assert!(result.len() <= 5);
    }

    #[test]
    fn diversity_keeps_mixed_signals() {
        let mut examples = Vec::new();
        for i in 0..5 {
            examples.push(completion(
                &format!("a{i}"),
                &format!("r{i}"),
                0.9,
                "latency",
            ));
        }
        for i in 0..5 {
            examples.push(completion(
                &format!("b{i}"),
                &format!("s{i}"),
                0.9,
                "throughput",
            ));
        }
        let filter = DiversityFilter::default();
        let result = filter.filter(examples);
        assert_eq!(result.len(), 10);
    }

    #[test]
    fn threshold_removes_low_scores() {
        let a = completion("p1", "r1", 0.9, "latency");
        let b = completion("p2", "r2", 0.3, "latency");
        let filter = ScoreThresholdFilter { min_score: 0.5 };
        let result = filter.filter(vec![a, b]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn pipeline_chains_filters() {
        let a = completion("p", "r", 0.9, "latency");
        let b = completion("p", "r", 0.8, "latency");
        let c = completion("p2", "r2", 0.3, "latency");
        let pipeline = FilterPipeline::new()
            .with_filter(Box::new(DeduplicationFilter))
            .with_filter(Box::new(ScoreThresholdFilter { min_score: 0.5 }));
        let result = pipeline.filter(vec![a, b, c]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn preference_quality_zeros_empty_chosen() {
        let ex = preference("p", "", "reject", 0.5);
        let result = PatchQualityFilter.filter(vec![ex]);
        assert_eq!(result[0].quality_score, 0.0);
    }
}
