mod dataset;
mod error;
mod export;
mod extractor;
mod filters;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

pub use dataset::{DatasetManager, DatasetStats, ExampleFilter, ExampleKindFilter, IngestReport};
pub use error::TrainingError;
pub use export::{export_examples, ExportFormat, ExportReport};
pub use extractor::{
    ChainExtractor, DefaultChainExtractor, ExtractionConfig, DEFAULT_SYSTEM_PROMPT,
};
pub use filters::{
    DeduplicationFilter, DiversityFilter, FilterPipeline, PatchQualityFilter, QualityFilter,
    ScoreThresholdFilter,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainingExample {
    pub id: Uuid,
    pub source_chain_index: u64,
    pub source_chain_path: PathBuf,
    pub curated_at: DateTime<Utc>,
    pub kind: ExampleKind,
    pub quality_score: f64,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExampleKind {
    Completion(CompletionExample),
    Preference(PreferenceExample),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionExample {
    pub system_prompt: String,
    pub user_prompt: String,
    pub assistant_response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreferenceExample {
    pub system_prompt: String,
    pub user_prompt: String,
    pub chosen: String,
    pub rejected: String,
    pub score_delta: f64,
}

pub(crate) const PATCH_START: &str = "<PATCH>";
pub(crate) const PATCH_END: &str = "</PATCH>";
pub(crate) const APPROACH_START: &str = "<APPROACH>";
pub(crate) const APPROACH_END: &str = "</APPROACH>";
pub(crate) const METRICS_START: &str = "<METRICS>";
pub(crate) const METRICS_END: &str = "</METRICS>";

pub(crate) fn example_content_hash(example: &TrainingExample) -> String {
    let key = example_content_key(example);
    format!("{:x}", Sha256::digest(key.as_bytes()))
}

fn example_content_key(example: &TrainingExample) -> String {
    match &example.kind {
        ExampleKind::Completion(completion) => format!(
            "completion\n{}\n{}",
            completion.user_prompt, completion.assistant_response
        ),
        ExampleKind::Preference(preference) => format!(
            "preference\n{}\n{}\n{}",
            preference.user_prompt, preference.chosen, preference.rejected
        ),
    }
}

pub(crate) fn format_model_response(
    patch: &str,
    approach: &str,
    metrics: &BTreeMap<String, f64>,
) -> String {
    let metrics_json = match serde_json::to_string(metrics) {
        Ok(value) => value,
        Err(_) => "{}".to_owned(),
    };
    format!(
        "{PATCH_START}
{patch}
{PATCH_END}
{APPROACH_START}
{approach}
{APPROACH_END}
{METRICS_START}
{metrics_json}
{METRICS_END}",
        patch = patch,
        approach = approach,
        metrics_json = metrics_json,
    )
}

pub(crate) fn signal_tag(tags: &[String]) -> Option<&str> {
    tags.iter().find_map(|tag| {
        if tag.starts_with("signal:") {
            Some(tag.as_str())
        } else {
            None
        }
    })
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct ResponseParts {
    patch: String,
    approach: String,
}

#[cfg(test)]
fn extract_response_parts(text: &str) -> ResponseParts {
    ResponseParts {
        patch: extract_tagged_block(text, PATCH_START, PATCH_END),
        approach: extract_tagged_block(text, APPROACH_START, APPROACH_END),
    }
}

#[cfg(test)]
fn extract_tagged_block(text: &str, start: &str, end: &str) -> String {
    text.split_once(start)
        .and_then(|(_, rest)| rest.split_once(end))
        .map(|(value, _)| value.trim().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn example_content_hash_distinguishes_different_responses() {
        let base = completion_example("prompt", "response-one");
        let changed = completion_example("prompt", "response-two");

        assert_ne!(example_content_hash(&base), example_content_hash(&changed));
    }

    #[test]
    fn extract_response_parts_reads_tagged_blocks() {
        let text = format!(
            "{PATCH_START}\ndiff --git a/src/lib.rs b/src/lib.rs\n{PATCH_END}\n{APPROACH_START}\nExplain it well\n{APPROACH_END}"
        );

        let parts = extract_response_parts(&text);

        assert_eq!(parts.patch, "diff --git a/src/lib.rs b/src/lib.rs");
        assert_eq!(parts.approach, "Explain it well");
    }

    #[test]
    fn format_model_response_emits_expected_sections() {
        let response = format_model_response(
            "diff --git a/src/lib.rs b/src/lib.rs",
            "Improve the implementation",
            &BTreeMap::from([("aggregate_score".to_owned(), 0.9)]),
        );

        assert!(response.contains(PATCH_START));
        assert!(response.contains(APPROACH_START));
        assert!(response.contains(METRICS_START));
    }

    fn completion_example(user_prompt: &str, assistant_response: &str) -> TrainingExample {
        TrainingExample {
            id: Uuid::new_v4(),
            source_chain_index: 1,
            source_chain_path: PathBuf::from("chain.json"),
            curated_at: Utc::now(),
            kind: ExampleKind::Completion(CompletionExample {
                system_prompt: "system".to_owned(),
                user_prompt: user_prompt.to_owned(),
                assistant_response: assistant_response.to_owned(),
            }),
            quality_score: 0.9,
            tags: vec!["signal:latency".to_owned()],
        }
    }
}
