use crate::export::{export_examples, ExportFormat, ExportReport};
use crate::filters::QualityFilter;
use crate::{example_content_hash, signal_tag, ExampleKind, TrainingError, TrainingExample};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Manifest {
    schema_version: u32,
    next_id: u64,
    total_examples: usize,
    updated_at: DateTime<Utc>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            next_id: 1,
            total_examples: 0,
            updated_at: Utc::now(),
        }
    }
}

pub struct DatasetManager {
    dataset_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct IngestReport {
    pub added: usize,
    pub duplicates_skipped: usize,
    pub filtered_out: usize,
}

#[derive(Debug, Clone)]
pub struct DatasetStats {
    pub total_examples: usize,
    pub completion_examples: usize,
    pub preference_examples: usize,
    pub signals_represented: Vec<String>,
    pub avg_quality_score: f64,
    pub total_size_bytes: u64,
    pub oldest_example: Option<DateTime<Utc>>,
    pub newest_example: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct ExampleFilter {
    pub kind: Option<ExampleKindFilter>,
    pub min_quality: Option<f64>,
    pub tags: Option<Vec<String>>,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub enum ExampleKindFilter {
    CompletionOnly,
    PreferenceOnly,
}

impl DatasetManager {
    pub fn new(dataset_dir: PathBuf) -> Result<Self, TrainingError> {
        let examples_dir = dataset_dir.join("examples");
        std::fs::create_dir_all(&examples_dir)?;
        let manifest_path = dataset_dir.join("manifest.json");
        if !manifest_path.exists() {
            write_manifest(&manifest_path, &Manifest::default())?;
        }
        Ok(Self { dataset_dir })
    }

    pub fn ingest(
        &self,
        examples: &[TrainingExample],
        filter: Option<&dyn QualityFilter>,
    ) -> Result<IngestReport, TrainingError> {
        let mut manifest = self.load_manifest()?;
        let mut existing_hashes = self.load_existing_hashes()?;
        let filtered = apply_optional_filter(examples.to_vec(), filter);
        let filtered_out = examples.len().saturating_sub(filtered.len());
        let mut added = 0;
        let mut duplicates_skipped = 0;
        for example in &filtered {
            let hash = example_content_hash(example);
            if existing_hashes.contains(&hash) {
                duplicates_skipped += 1;
                continue;
            }
            existing_hashes.insert(hash);
            let filename = format!("{:08}.json", manifest.next_id);
            let path = self.examples_dir().join(filename);
            write_example(&path, example)?;
            manifest.next_id += 1;
            added += 1;
        }
        manifest.total_examples += added;
        manifest.updated_at = Utc::now();
        self.save_manifest(&manifest)?;
        Ok(IngestReport {
            added,
            duplicates_skipped,
            filtered_out,
        })
    }

    pub fn export(
        &self,
        format: ExportFormat,
        output: &Path,
    ) -> Result<ExportReport, TrainingError> {
        let examples = self.load_all_examples()?;
        export_examples(&examples, &format, output)
    }

    pub fn stats(&self) -> Result<DatasetStats, TrainingError> {
        let examples = self.load_all_examples()?;
        let total_size_bytes = self.total_example_size_bytes()?;
        Ok(compute_stats(&examples, total_size_bytes))
    }

    pub fn prune(&self, min_quality: f64) -> Result<usize, TrainingError> {
        let mut removed = 0;
        for entry in std::fs::read_dir(self.examples_dir())? {
            let entry = entry?;
            let path = entry.path();
            if !is_json_file(&path) {
                continue;
            }
            let example = read_example(&path)?;
            if example.quality_score < min_quality {
                std::fs::remove_file(&path)?;
                removed += 1;
            }
        }
        let mut manifest = self.load_manifest()?;
        manifest.total_examples = manifest.total_examples.saturating_sub(removed);
        manifest.updated_at = Utc::now();
        self.save_manifest(&manifest)?;
        Ok(removed)
    }

    pub fn list(
        &self,
        filter: Option<&ExampleFilter>,
    ) -> Result<Vec<TrainingExample>, TrainingError> {
        let examples = self.load_all_examples()?;
        match filter {
            Some(f) => Ok(apply_example_filter(examples, f)),
            None => Ok(examples),
        }
    }

    fn examples_dir(&self) -> PathBuf {
        self.dataset_dir.join("examples")
    }

    fn manifest_path(&self) -> PathBuf {
        self.dataset_dir.join("manifest.json")
    }

    fn load_manifest(&self) -> Result<Manifest, TrainingError> {
        let content = std::fs::read_to_string(self.manifest_path())?;
        serde_json::from_str(&content).map_err(TrainingError::Serialization)
    }

    fn save_manifest(&self, manifest: &Manifest) -> Result<(), TrainingError> {
        write_manifest(&self.manifest_path(), manifest)
    }

    fn total_example_size_bytes(&self) -> Result<u64, TrainingError> {
        let dir = self.examples_dir();
        if !dir.exists() {
            return Ok(0);
        }
        let mut total_size_bytes = 0;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if is_json_file(&path) {
                total_size_bytes += entry.metadata()?.len();
            }
        }
        Ok(total_size_bytes)
    }

    fn load_existing_hashes(&self) -> Result<HashSet<String>, TrainingError> {
        let mut hashes = HashSet::new();
        let dir = self.examples_dir();
        if !dir.exists() {
            return Ok(hashes);
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !is_json_file(&path) {
                continue;
            }
            if let Ok(example) = read_example(&path) {
                hashes.insert(example_content_hash(&example));
            }
        }
        Ok(hashes)
    }

    fn load_all_examples(&self) -> Result<Vec<TrainingExample>, TrainingError> {
        let dir = self.examples_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut examples = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if is_json_file(&path) {
                examples.push(read_example(&path)?);
            }
        }
        examples.sort_by_key(|e| e.source_chain_index);
        Ok(examples)
    }
}

fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), TrainingError> {
    let json = serde_json::to_string_pretty(manifest)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn write_example(path: &Path, example: &TrainingExample) -> Result<(), TrainingError> {
    let json = serde_json::to_string_pretty(example)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn read_example(path: &Path) -> Result<TrainingExample, TrainingError> {
    let content = std::fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(TrainingError::Serialization)
}

fn is_json_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("json")
        && path.file_name().and_then(|n| n.to_str()) != Some("manifest.json")
}

fn apply_optional_filter(
    examples: Vec<TrainingExample>,
    filter: Option<&dyn QualityFilter>,
) -> Vec<TrainingExample> {
    match filter {
        Some(f) => f.filter(examples),
        None => examples,
    }
}

fn apply_example_filter(
    examples: Vec<TrainingExample>,
    filter: &ExampleFilter,
) -> Vec<TrainingExample> {
    examples
        .into_iter()
        .filter(|e| matches_filter(e, filter))
        .collect()
}

fn matches_filter(example: &TrainingExample, filter: &ExampleFilter) -> bool {
    if let Some(kind) = &filter.kind {
        match kind {
            ExampleKindFilter::CompletionOnly => {
                if !matches!(example.kind, ExampleKind::Completion(_)) {
                    return false;
                }
            }
            ExampleKindFilter::PreferenceOnly => {
                if !matches!(example.kind, ExampleKind::Preference(_)) {
                    return false;
                }
            }
        }
    }
    if let Some(min) = filter.min_quality {
        if example.quality_score < min {
            return false;
        }
    }
    if let Some(tags) = &filter.tags {
        if !tags.iter().any(|t| example.tags.contains(t)) {
            return false;
        }
    }
    if let Some(after) = filter.after {
        if example.curated_at < after {
            return false;
        }
    }
    if let Some(before) = filter.before {
        if example.curated_at > before {
            return false;
        }
    }
    true
}

fn compute_stats(examples: &[TrainingExample], total_size_bytes: u64) -> DatasetStats {
    let mut completions = 0;
    let mut preferences = 0;
    let mut signals = HashSet::new();
    let mut total_score = 0.0;
    let mut oldest: Option<DateTime<Utc>> = None;
    let mut newest: Option<DateTime<Utc>> = None;
    for example in examples {
        match &example.kind {
            ExampleKind::Completion(_) => completions += 1,
            ExampleKind::Preference(_) => preferences += 1,
        }
        if let Some(sig) = signal_tag(&example.tags) {
            signals.insert(sig.to_owned());
        }
        total_score += example.quality_score;
        update_time_bounds(&mut oldest, &mut newest, example.curated_at);
    }
    let avg = if examples.is_empty() {
        0.0
    } else {
        total_score / examples.len() as f64
    };
    DatasetStats {
        total_examples: examples.len(),
        completion_examples: completions,
        preference_examples: preferences,
        signals_represented: signals.into_iter().collect(),
        avg_quality_score: avg,
        total_size_bytes,
        oldest_example: oldest,
        newest_example: newest,
    }
}

fn update_time_bounds(
    oldest: &mut Option<DateTime<Utc>>,
    newest: &mut Option<DateTime<Utc>>,
    time: DateTime<Utc>,
) {
    *oldest = Some(oldest.map_or(time, |o| o.min(time)));
    *newest = Some(newest.map_or(time, |n| n.max(time)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CompletionExample;
    use chrono::Utc;
    use uuid::Uuid;

    fn sample_example(prompt: &str, response: &str, score: f64) -> TrainingExample {
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
            tags: vec!["signal:latency".to_owned()],
        }
    }

    #[test]
    fn create_new_dataset() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let stats = manager.stats().unwrap();
        assert_eq!(stats.total_examples, 0);
    }

    #[test]
    fn ingest_and_stats() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let examples = vec![
            sample_example("p1", "r1", 0.9),
            sample_example("p2", "r2", 0.8),
        ];

        let report = manager.ingest(&examples, None).unwrap();

        assert_eq!(report.added, 2);
        assert_eq!(report.duplicates_skipped, 0);
        let stats = manager.stats().unwrap();
        assert_eq!(stats.total_examples, 2);
        assert_eq!(stats.completion_examples, 2);
    }

    #[test]
    fn ingest_dedup() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let examples = vec![sample_example("p1", "r1", 0.9)];

        manager.ingest(&examples, None).unwrap();
        let report = manager.ingest(&examples, None).unwrap();

        assert_eq!(report.added, 0);
        assert_eq!(report.duplicates_skipped, 1);
    }

    #[test]
    fn ingest_skips_duplicates_within_batch() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let examples = vec![
            sample_example("p1", "r1", 0.9),
            sample_example("p1", "r1", 0.8),
        ];

        let report = manager.ingest(&examples, None).unwrap();

        assert_eq!(report.added, 1);
        assert_eq!(report.duplicates_skipped, 1);
        assert_eq!(manager.list(None).unwrap().len(), 1);
    }

    #[test]
    fn prune_removes_low_quality() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let examples = vec![
            sample_example("p1", "r1", 0.9),
            sample_example("p2", "r2", 0.3),
        ];
        manager.ingest(&examples, None).unwrap();

        let removed = manager.prune(0.5).unwrap();

        assert_eq!(removed, 1);
        let stats = manager.stats().unwrap();
        assert_eq!(stats.total_examples, 1);
    }

    #[test]
    fn list_with_filter() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let examples = vec![
            sample_example("p1", "r1", 0.9),
            sample_example("p2", "r2", 0.3),
        ];
        manager.ingest(&examples, None).unwrap();

        let filter = ExampleFilter {
            kind: None,
            min_quality: Some(0.5),
            tags: None,
            after: None,
            before: None,
        };
        let result = manager.list(Some(&filter)).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn export_from_dataset() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let examples = vec![sample_example("p1", "r1", 0.9)];
        manager.ingest(&examples, None).unwrap();

        let out = dir.path().join("export.jsonl");
        let report = manager.export(ExportFormat::OpenAiJsonl, &out).unwrap();

        assert_eq!(report.examples_exported, 1);
        assert!(out.exists());
    }

    #[test]
    fn list_kind_filter() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let examples = vec![sample_example("p1", "r1", 0.9)];
        manager.ingest(&examples, None).unwrap();

        let filter = ExampleFilter {
            kind: Some(ExampleKindFilter::PreferenceOnly),
            min_quality: None,
            tags: None,
            after: None,
            before: None,
        };
        let result = manager.list(Some(&filter)).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn stats_reports_total_example_file_size() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let examples = vec![
            sample_example("p1", "r1", 0.9),
            sample_example("p2", "r2", 0.8),
        ];
        manager.ingest(&examples, None).unwrap();

        let expected_size = std::fs::read_dir(manager.examples_dir())
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| is_json_file(&entry.path()))
            .map(|entry| entry.metadata().unwrap().len())
            .sum::<u64>();

        let stats = manager.stats().unwrap();

        assert!(stats.total_size_bytes > 0);
        assert_eq!(stats.total_size_bytes, expected_size);
    }

    #[test]
    fn stats_empty_dataset() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = DatasetManager::new(dir.path().join("ds")).unwrap();
        let stats = manager.stats().unwrap();
        assert_eq!(stats.avg_quality_score, 0.0);
        assert_eq!(stats.total_size_bytes, 0);
        assert!(stats.oldest_example.is_none());
    }
}
