//! JournalSkill — exposes reflective memory as agent tools.
//!
//! Two tools:
//! - `journal_write`: record a lesson or insight for future sessions
//! - `journal_search`: search past entries for relevant lessons

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use serde::Deserialize;

use crate::journal::Journal;
use fx_loadable::skill::{Skill, SkillError};

/// Skill that provides journal_write and journal_search tools.
pub struct JournalSkill {
    journal: Arc<Mutex<Journal>>,
}

impl std::fmt::Debug for JournalSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalSkill").finish()
    }
}

impl JournalSkill {
    /// Create a new `JournalSkill` wrapping the given journal.
    #[must_use]
    pub fn new(journal: Arc<Mutex<Journal>>) -> Self {
        Self { journal }
    }
}

#[derive(Deserialize)]
struct WriteArgs {
    lesson: String,
    tags: Vec<String>,
    applies_to: String,
    context: Option<String>,
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    tags: Option<Vec<String>>,
    limit: Option<usize>,
}

fn journal_write_definition() -> ToolDefinition {
    ToolDefinition {
        name: "journal_write".to_string(),
        description: concat!(
            "Record a lesson or insight for future sessions. ",
            "Use when you notice something worth remembering — ",
            "a pattern, a gotcha, a technique that worked well. ",
            "Don't force it.",
        )
        .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "lesson": {
                    "type": "string",
                    "description": "The insight or lesson learned"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Categorization tags"
                },
                "applies_to": {
                    "type": "string",
                    "description": "What area this applies to"
                },
                "context": {
                    "type": "string",
                    "description": "Optional context about when/why"
                }
            },
            "required": ["lesson", "tags", "applies_to"]
        }),
    }
}

fn journal_search_definition() -> ToolDefinition {
    ToolDefinition {
        name: "journal_search".to_string(),
        description: concat!(
            "Search past journal entries for lessons relevant ",
            "to current work. Query by text and optional tags.",
        )
        .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search text (matches lesson, context, applies_to)"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Filter by tags (entry must have ALL specified)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default 10)"
                }
            },
            "required": ["query"]
        }),
    }
}

#[async_trait]
impl Skill for JournalSkill {
    fn name(&self) -> &str {
        "journal"
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![journal_write_definition(), journal_search_definition()]
    }

    fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
        // journal_search must not be cached: journal_write in the same turn
        // would make cached results stale.
        ToolCacheability::NeverCache
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        match tool_name {
            "journal_write" => Some(self.handle_write(arguments)),
            "journal_search" => Some(self.handle_search(arguments)),
            _ => None,
        }
    }
}

impl JournalSkill {
    fn handle_write(&self, arguments: &str) -> Result<String, SkillError> {
        let args: WriteArgs =
            serde_json::from_str(arguments).map_err(|e| format!("invalid arguments: {e}"))?;

        let mut journal = self
            .journal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let entry = journal
            .write(args.lesson, args.tags, args.applies_to, args.context)
            .map_err(|e| format!("journal write failed: {e}"))?;

        serde_json::to_string(&serde_json::json!({
            "status": "recorded",
            "id": entry.id,
        }))
        .map_err(|e| format!("serialization failed: {e}"))
    }

    fn handle_search(&self, arguments: &str) -> Result<String, SkillError> {
        let args: SearchArgs =
            serde_json::from_str(arguments).map_err(|e| format!("invalid arguments: {e}"))?;

        let journal = self
            .journal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let limit = args.limit.unwrap_or(10);
        let results = journal.search(&args.query, args.tags, limit);

        let entries: Vec<serde_json::Value> = results
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "lesson": e.lesson,
                    "tags": e.tags,
                    "applies_to": e.applies_to,
                    "context": e.context,
                    "timestamp": format_timestamp(e.timestamp),
                })
            })
            .collect();

        serde_json::to_string(&serde_json::json!({
            "count": entries.len(),
            "entries": entries,
        }))
        .map_err(|e| format!("serialization failed: {e}"))
    }
}

/// Format a Unix-millis timestamp as an ISO 8601 UTC string.
///
/// Uses `std::time` to convert epoch millis to a human-readable
/// timestamp without pulling in a datetime crate.
fn format_timestamp(millis: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};

    let dt = UNIX_EPOCH + Duration::from_millis(millis);
    // `humantime` isn't a dep; format manually via the debug repr
    // which gives RFC 3339. We use `httpdate`-style manual formatting.
    format_system_time_utc(dt)
}

/// Render a `SystemTime` as `YYYY-MM-DDTHH:MM:SSZ` (UTC, second precision).
fn format_system_time_utc(t: std::time::SystemTime) -> String {
    use std::time::{Duration, UNIX_EPOCH};

    let dur = t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let total_secs = dur.as_secs();

    let (year, month, day) = epoch_secs_to_date(total_secs);
    let day_secs = total_secs % 86_400;
    let hour = day_secs / 3_600;
    let minute = (day_secs % 3_600) / 60;
    let second = day_secs % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Convert seconds since Unix epoch to (year, month, day) in UTC.
fn epoch_secs_to_date(secs: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's `civil_from_days`.
    let days = secs / 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_skill(tmp: &TempDir) -> JournalSkill {
        let path = tmp.path().join("journal.jsonl");
        let journal = Journal::load(path).unwrap();
        JournalSkill::new(Arc::new(Mutex::new(journal)))
    }

    #[test]
    fn skill_provides_two_tools() {
        let tmp = TempDir::new().unwrap();
        let skill = test_skill(&tmp);
        let defs = skill.tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"journal_write"));
        assert!(names.contains(&"journal_search"));
    }

    #[test]
    fn skill_name_is_journal() {
        let tmp = TempDir::new().unwrap();
        let skill = test_skill(&tmp);
        assert_eq!(skill.name(), "journal");
    }

    #[tokio::test]
    async fn skill_write_then_search() {
        let tmp = TempDir::new().unwrap();
        let skill = test_skill(&tmp);

        let write_args = serde_json::json!({
            "lesson": "Small PRs get faster reviews",
            "tags": ["review", "process"],
            "applies_to": "orchestration"
        })
        .to_string();

        let result = skill.execute("journal_write", &write_args, None).await;
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());

        let search_args = serde_json::json!({
            "query": "faster reviews"
        })
        .to_string();

        let result = skill.execute("journal_search", &search_args, None).await;
        let output = result.unwrap().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["count"], 1);
    }

    #[tokio::test]
    async fn skill_execute_invalid_json_returns_error() {
        let tmp = TempDir::new().unwrap();
        let skill = test_skill(&tmp);

        // Completely invalid JSON
        let result = skill.execute("journal_write", "not json", None).await;
        let err = result.expect("should return Some").unwrap_err();
        assert!(
            err.to_string().contains("invalid arguments"),
            "error should mention invalid arguments, got: {err}"
        );

        // Valid JSON but wrong shape (missing required fields)
        let result = skill
            .execute("journal_write", r#"{"wrong": "fields"}"#, None)
            .await;
        let err = result.expect("should return Some").unwrap_err();
        assert!(
            err.to_string().contains("invalid arguments"),
            "error should mention invalid arguments, got: {err}"
        );
    }

    #[tokio::test]
    async fn skill_returns_none_for_unknown_tool() {
        let tmp = TempDir::new().unwrap();
        let skill = test_skill(&tmp);
        let result = skill.execute("unknown_tool", "{}", None).await;
        assert!(result.is_none());
    }
}
