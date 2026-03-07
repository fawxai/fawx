//! Journal storage and retrieval — the core of reflective memory.
//!
//! Entries are persisted as JSONL (one JSON object per line). Writes are
//! append-only; the full set of entries is loaded into memory on startup.

use crate::error::JournalError;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single journal entry — a lesson learned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEntry {
    /// Unique identifier (UUID v4).
    pub id: String,
    /// The insight or lesson.
    pub lesson: String,
    /// Categorization tags (e.g., "spec-quality", "streaming", "review").
    pub tags: Vec<String>,
    /// What this applies to (e.g., "fx-llm", "orchestration", "testing").
    pub applies_to: String,
    /// Optional context about when/why this was learned.
    pub context: Option<String>,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
}

/// Journal storage and retrieval.
///
/// Entries live in memory after load; writes append to the JSONL file
/// without rewriting existing content.
#[derive(Debug)]
pub struct Journal {
    entries: Vec<JournalEntry>,
    path: PathBuf,
}

impl Journal {
    /// Create or load a journal from a JSONL file.
    ///
    /// If the file does not exist, creates parent directories and starts
    /// with an empty journal. If the file exists, parses all lines.
    /// Malformed lines are skipped with a warning (not a hard error).
    pub fn load(path: PathBuf) -> Result<Self, JournalError> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            return Ok(Self {
                entries: Vec::new(),
                path,
            });
        }

        let file = fs::File::open(&path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<JournalEntry>(trimmed) {
                Ok(entry) => entries.push(entry),
                Err(err) => {
                    tracing::warn!("skipping malformed journal line: {err}");
                }
            }
        }

        Ok(Self { entries, path })
    }

    /// Write a new entry, appending it to the JSONL file.
    pub fn write(
        &mut self,
        lesson: String,
        tags: Vec<String>,
        applies_to: String,
        context: Option<String>,
    ) -> Result<JournalEntry, JournalError> {
        let entry = JournalEntry {
            id: uuid::Uuid::new_v4().to_string(),
            lesson,
            tags,
            applies_to,
            context,
            timestamp: now_millis(),
        };

        self.append_to_file(&entry)?;
        self.entries.push(entry.clone());
        Ok(entry)
    }

    /// Search entries by text query and optional tag filter.
    ///
    /// Text search: case-insensitive substring match on `lesson`, `context`,
    /// and `applies_to`. Tag filter: entry must contain ALL specified tags.
    pub fn search(
        &self,
        query: &str,
        tags: Option<Vec<String>>,
        limit: usize,
    ) -> Vec<&JournalEntry> {
        let query_lower = query.to_lowercase();

        self.entries
            .iter()
            .rev()
            .filter(|entry| matches_text(entry, &query_lower))
            .filter(|entry| matches_tags(entry, &tags))
            .take(limit)
            .collect()
    }

    /// List all entries, most recent first, with optional limit.
    pub fn list(&self, limit: Option<usize>) -> Vec<&JournalEntry> {
        let iter = self.entries.iter().rev();
        match limit {
            Some(n) => iter.take(n).collect(),
            None => iter.collect(),
        }
    }

    /// Count total entries.
    #[must_use]
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Append a single entry as a JSON line to the file.
    fn append_to_file(&self, entry: &JournalEntry) -> Result<(), JournalError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let json = serde_json::to_string(entry)?;
        writeln!(file, "{json}")?;
        Ok(())
    }
}

/// Case-insensitive substring match on lesson, context, and applies_to.
fn matches_text(entry: &JournalEntry, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    let lesson_lower = entry.lesson.to_lowercase();
    let applies_lower = entry.applies_to.to_lowercase();
    if lesson_lower.contains(query_lower) || applies_lower.contains(query_lower) {
        return true;
    }
    if let Some(ctx) = &entry.context {
        if ctx.to_lowercase().contains(query_lower) {
            return true;
        }
    }
    false
}

/// Entry must have ALL specified tags (if any).
fn matches_tags(entry: &JournalEntry, tags: &Option<Vec<String>>) -> bool {
    match tags {
        None => true,
        Some(required) => required.iter().all(|tag| entry.tags.contains(tag)),
    }
}

/// Current time as Unix milliseconds.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_journal(tmp: &TempDir) -> Journal {
        let path = tmp.path().join("journal.jsonl");
        Journal::load(path).unwrap()
    }

    #[test]
    fn journal_write_and_read_back() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        let entry = journal
            .write(
                "Use small PRs for faster reviews".into(),
                vec!["review".into()],
                "orchestration".into(),
                None,
            )
            .unwrap();

        let list = journal.list(None);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, entry.id);
        assert_eq!(list[0].lesson, "Use small PRs for faster reviews");
    }

    #[test]
    fn journal_search_by_text() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        journal
            .write(
                "Streaming requires careful buffering".into(),
                vec!["streaming".into()],
                "fx-llm".into(),
                None,
            )
            .unwrap();
        journal
            .write(
                "Tests should be deterministic".into(),
                vec!["testing".into()],
                "general".into(),
                None,
            )
            .unwrap();

        let results = journal.search("buffering", None, 10);
        assert_eq!(results.len(), 1);
        assert!(results[0].lesson.contains("buffering"));
    }

    #[test]
    fn journal_search_by_tags() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        journal
            .write(
                "Lesson A".into(),
                vec!["alpha".into(), "beta".into()],
                "area-a".into(),
                None,
            )
            .unwrap();
        journal
            .write(
                "Lesson B".into(),
                vec!["alpha".into()],
                "area-b".into(),
                None,
            )
            .unwrap();

        let results = journal.search("", Some(vec!["alpha".into(), "beta".into()]), 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].lesson, "Lesson A");

        let results_alpha = journal.search("", Some(vec!["alpha".into()]), 10);
        assert_eq!(results_alpha.len(), 2);
    }

    #[test]
    fn journal_search_combined() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        journal
            .write(
                "Streaming needs buffering".into(),
                vec!["streaming".into()],
                "fx-llm".into(),
                None,
            )
            .unwrap();
        journal
            .write(
                "Buffering is important for tests".into(),
                vec!["testing".into()],
                "general".into(),
                None,
            )
            .unwrap();

        let results = journal.search("buffering", Some(vec!["streaming".into()]), 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].lesson, "Streaming needs buffering");
    }

    #[test]
    fn journal_persistence() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("journal.jsonl");

        {
            let mut journal = Journal::load(path.clone()).unwrap();
            journal
                .write(
                    "Persisted lesson".into(),
                    vec!["persistence".into()],
                    "storage".into(),
                    Some("testing persistence".into()),
                )
                .unwrap();
        }

        let reloaded = Journal::load(path).unwrap();
        assert_eq!(reloaded.count(), 1);
        assert_eq!(reloaded.list(None)[0].lesson, "Persisted lesson");
        assert_eq!(
            reloaded.list(None)[0].context.as_deref(),
            Some("testing persistence")
        );
    }

    #[test]
    fn journal_empty_search() {
        let tmp = TempDir::new().unwrap();
        let journal = test_journal(&tmp);

        let results = journal.search("anything", None, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn journal_list_ordering() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        journal
            .write("First".into(), vec![], "a".into(), None)
            .unwrap();
        journal
            .write("Second".into(), vec![], "a".into(), None)
            .unwrap();
        journal
            .write("Third".into(), vec![], "a".into(), None)
            .unwrap();

        let list = journal.list(None);
        assert_eq!(list[0].lesson, "Third");
        assert_eq!(list[1].lesson, "Second");
        assert_eq!(list[2].lesson, "First");
    }

    #[test]
    fn journal_count() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        assert_eq!(journal.count(), 0);

        journal
            .write("One".into(), vec![], "a".into(), None)
            .unwrap();
        assert_eq!(journal.count(), 1);

        journal
            .write("Two".into(), vec![], "b".into(), None)
            .unwrap();
        assert_eq!(journal.count(), 2);
    }

    #[test]
    fn journal_search_matches_applies_to() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        journal
            .write("Some lesson".into(), vec![], "fx-orchestrator".into(), None)
            .unwrap();

        let results = journal.search("orchestrator", None, 10);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn journal_search_matches_context() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        journal
            .write(
                "Some lesson".into(),
                vec![],
                "area".into(),
                Some("discovered during wave 7 session".into()),
            )
            .unwrap();

        let results = journal.search("wave 7", None, 10);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn journal_search_is_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        journal
            .write(
                "Use STREAMING carefully".into(),
                vec![],
                "fx-llm".into(),
                None,
            )
            .unwrap();

        let results = journal.search("streaming", None, 10);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn journal_load_skips_malformed_lines() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("journal.jsonl");

        // Write two valid entries with a malformed line in between.
        {
            let mut journal = Journal::load(path.clone()).unwrap();
            journal
                .write("First valid".into(), vec![], "area".into(), None)
                .unwrap();
            journal
                .write("Second valid".into(), vec![], "area".into(), None)
                .unwrap();
        }

        // Insert a malformed line between the two valid entries.
        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);

        let corrupted = format!("{}\n{{not valid json at all\n{}\n", lines[0], lines[1]);
        std::fs::write(&path, corrupted).unwrap();

        // Load should skip the malformed line and return both valid entries.
        let reloaded = Journal::load(path).unwrap();
        assert_eq!(reloaded.count(), 2);
        let list = reloaded.list(None);
        assert_eq!(list[0].lesson, "Second valid");
        assert_eq!(list[1].lesson, "First valid");
    }

    #[test]
    fn journal_list_with_limit() {
        let tmp = TempDir::new().unwrap();
        let mut journal = test_journal(&tmp);

        for i in 0..5 {
            journal
                .write(format!("Lesson {i}"), vec![], "a".into(), None)
                .unwrap();
        }

        let limited = journal.list(Some(3));
        assert_eq!(limited.len(), 3);
        assert_eq!(limited[0].lesson, "Lesson 4");
    }
}
