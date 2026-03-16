use crate::{ExampleKind, TrainingError, TrainingExample};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    OpenAiJsonl,
    AlpacaJsonl,
    DpoJsonl,
    RawJson,
}

impl fmt::Display for ExportFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::OpenAiJsonl => "openai-jsonl",
            Self::AlpacaJsonl => "alpaca-jsonl",
            Self::DpoJsonl => "dpo-jsonl",
            Self::RawJson => "raw-json",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone)]
pub struct ExportReport {
    pub examples_exported: usize,
    pub format: ExportFormat,
    pub output_path: PathBuf,
    pub file_size_bytes: u64,
}

pub fn export_examples(
    examples: &[TrainingExample],
    format: &ExportFormat,
    output: &Path,
) -> Result<ExportReport, TrainingError> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(output)?;
    let mut count = 0;
    for example in examples {
        if let Some(line) = format_line(example, format) {
            writeln!(file, "{line}")?;
            count += 1;
        }
    }
    let metadata = std::fs::metadata(output)?;
    Ok(ExportReport {
        examples_exported: count,
        format: format.clone(),
        output_path: output.to_path_buf(),
        file_size_bytes: metadata.len(),
    })
}

fn format_line(example: &TrainingExample, format: &ExportFormat) -> Option<String> {
    match format {
        ExportFormat::OpenAiJsonl => format_openai(example),
        ExportFormat::AlpacaJsonl => format_alpaca(example),
        ExportFormat::DpoJsonl => format_dpo(example),
        ExportFormat::RawJson => format_raw(example),
    }
}

fn format_openai(example: &TrainingExample) -> Option<String> {
    match &example.kind {
        ExampleKind::Completion(c) => {
            let value = json!({
                "messages": [
                    {"role": "system", "content": c.system_prompt},
                    {"role": "user", "content": c.user_prompt},
                    {"role": "assistant", "content": c.assistant_response},
                ]
            });
            serde_json::to_string(&value).ok()
        }
        ExampleKind::Preference(_) => None,
    }
}

fn format_alpaca(example: &TrainingExample) -> Option<String> {
    match &example.kind {
        ExampleKind::Completion(c) => {
            let value = json!({
                "instruction": c.system_prompt,
                "input": c.user_prompt,
                "output": c.assistant_response,
            });
            serde_json::to_string(&value).ok()
        }
        ExampleKind::Preference(_) => None,
    }
}

fn format_dpo(example: &TrainingExample) -> Option<String> {
    match &example.kind {
        ExampleKind::Preference(p) => {
            let prompt = format!("{}\n\n{}", p.system_prompt, p.user_prompt);
            let value = json!({
                "prompt": prompt,
                "chosen": p.chosen,
                "rejected": p.rejected,
            });
            serde_json::to_string(&value).ok()
        }
        ExampleKind::Completion(_) => None,
    }
}

fn format_raw(example: &TrainingExample) -> Option<String> {
    serde_json::to_string(example).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompletionExample, PreferenceExample};
    use chrono::Utc;
    use uuid::Uuid;

    fn completion_example() -> TrainingExample {
        TrainingExample {
            id: Uuid::new_v4(),
            source_chain_index: 0,
            source_chain_path: PathBuf::from("c.json"),
            curated_at: Utc::now(),
            kind: ExampleKind::Completion(CompletionExample {
                system_prompt: "sys".to_owned(),
                user_prompt: "user".to_owned(),
                assistant_response: "resp".to_owned(),
            }),
            quality_score: 0.9,
            tags: vec![],
        }
    }

    fn preference_example() -> TrainingExample {
        TrainingExample {
            id: Uuid::new_v4(),
            source_chain_index: 0,
            source_chain_path: PathBuf::from("c.json"),
            curated_at: Utc::now(),
            kind: ExampleKind::Preference(PreferenceExample {
                system_prompt: "sys".to_owned(),
                user_prompt: "user".to_owned(),
                chosen: "good".to_owned(),
                rejected: "bad".to_owned(),
                score_delta: 0.5,
            }),
            quality_score: 0.5,
            tags: vec![],
        }
    }

    #[test]
    fn openai_format_completion() {
        let line = format_openai(&completion_example()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        let messages = parsed["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["role"], "assistant");
    }

    #[test]
    fn openai_skips_preference() {
        assert!(format_openai(&preference_example()).is_none());
    }

    #[test]
    fn alpaca_format_completion() {
        let line = format_alpaca(&completion_example()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(parsed.get("instruction").is_some());
        assert!(parsed.get("input").is_some());
        assert!(parsed.get("output").is_some());
    }

    #[test]
    fn dpo_format_preference() {
        let line = format_dpo(&preference_example()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(parsed.get("prompt").is_some());
        assert!(parsed.get("chosen").is_some());
        assert!(parsed.get("rejected").is_some());
    }

    #[test]
    fn dpo_skips_completion() {
        assert!(format_dpo(&completion_example()).is_none());
    }

    #[test]
    fn export_format_display_is_stable() {
        assert_eq!(ExportFormat::OpenAiJsonl.to_string(), "openai-jsonl");
        assert_eq!(ExportFormat::AlpacaJsonl.to_string(), "alpaca-jsonl");
        assert_eq!(ExportFormat::DpoJsonl.to_string(), "dpo-jsonl");
        assert_eq!(ExportFormat::RawJson.to_string(), "raw-json");
    }

    #[test]
    fn raw_exports_everything() {
        assert!(format_raw(&completion_example()).is_some());
        assert!(format_raw(&preference_example()).is_some());
    }

    #[test]
    fn export_writes_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("out.jsonl");
        let examples = vec![completion_example(), preference_example()];

        let report = export_examples(&examples, &ExportFormat::OpenAiJsonl, &path).unwrap();

        assert_eq!(report.examples_exported, 1);
        assert_eq!(report.format, ExportFormat::OpenAiJsonl);
        assert!(report.file_size_bytes > 0);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 1);
    }

    #[test]
    fn export_dpo_only_preferences() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("dpo.jsonl");
        let examples = vec![completion_example(), preference_example()];

        let report = export_examples(&examples, &ExportFormat::DpoJsonl, &path).unwrap();

        assert_eq!(report.examples_exported, 1);
    }

    #[test]
    fn export_raw_all() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("raw.jsonl");
        let examples = vec![completion_example(), preference_example()];

        let report = export_examples(&examples, &ExportFormat::RawJson, &path).unwrap();

        assert_eq!(report.examples_exported, 2);
    }
}
