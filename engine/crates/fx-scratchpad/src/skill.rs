//! ScratchpadSkill -- exposes scratchpad operations as tools.

use crate::{AddParams, Confidence, EntryKind, EntryStatus, Scratchpad};
use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_loadable::{Skill, SkillError};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct ScratchpadSkill {
    scratchpad: Arc<Mutex<Scratchpad>>,
    iteration: Arc<std::sync::atomic::AtomicU32>,
}

impl ScratchpadSkill {
    pub fn new(
        scratchpad: Arc<Mutex<Scratchpad>>,
        iteration: Arc<std::sync::atomic::AtomicU32>,
    ) -> Self {
        Self {
            scratchpad,
            iteration,
        }
    }

    fn current_iteration(&self) -> u32 {
        self.iteration.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn lock_scratchpad(&self) -> Result<std::sync::MutexGuard<'_, Scratchpad>, SkillError> {
        self.scratchpad
            .lock()
            .map_err(|e| format!("scratchpad lock poisoned: {e}"))
    }

    fn execute_add(&self, arguments: &str) -> Result<String, SkillError> {
        let args: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| format!("invalid arguments: {e}"))?;
        let kind_str = args["kind"]
            .as_str()
            .ok_or_else(|| "missing required argument: kind".to_string())?;
        let kind = EntryKind::from_str_loose(kind_str).ok_or_else(|| {
            format!("invalid kind '{kind_str}': use hypothesis, observation, conclusion, or note")
        })?;
        let label = args["label"]
            .as_str()
            .ok_or_else(|| "missing required argument: label".to_string())?
            .to_string();
        let content = args["content"]
            .as_str()
            .ok_or_else(|| "missing required argument: content".to_string())?
            .to_string();
        let confidence_str = args["confidence"].as_str().unwrap_or("medium");
        let confidence = Confidence::from_str_loose(confidence_str).ok_or_else(|| {
            format!("invalid confidence '{confidence_str}': use high, medium, or low")
        })?;
        let parent_id = args["parent_id"].as_u64().map(|v| v as u32);
        let mut sp = self.lock_scratchpad()?;
        let result = sp
            .add(AddParams {
                kind,
                label: label.clone(),
                content,
                confidence,
                parent_id,
                iteration: self.current_iteration(),
            })
            .map_err(|e| e.to_string())?;
        match result.parent_dropped {
            Some(pid) => Ok(format!(
                "Added entry #{}: {} (warning: parent #{} not found, created as top-level)",
                result.id, label, pid
            )),
            None => Ok(format!("Added entry #{}: {}", result.id, label)),
        }
    }

    fn execute_update(&self, arguments: &str) -> Result<String, SkillError> {
        let args: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| format!("invalid arguments: {e}"))?;
        let id = args["id"]
            .as_u64()
            .ok_or_else(|| "missing required argument: id".to_string())? as u32;
        let content = args["content"].as_str().map(|s| s.to_string());
        let confidence = args["confidence"]
            .as_str()
            .map(|s| {
                Confidence::from_str_loose(s)
                    .ok_or_else(|| format!("invalid confidence '{s}': use high, medium, or low"))
            })
            .transpose()?;
        let status = args["status"]
            .as_str()
            .map(|s| {
                EntryStatus::from_str_loose(s).ok_or_else(|| {
                    format!("invalid status '{s}': use active, superseded, or invalidated")
                })
            })
            .transpose()?;
        if content.is_none() && confidence.is_none() && status.is_none() {
            return Err(
                "at least one of content, confidence, or status must be provided".to_string(),
            );
        }
        let mut sp = self.lock_scratchpad()?;
        sp.update(id, content, confidence, status, self.current_iteration())
            .map_err(|e| e.to_string())?;
        Ok(format!("Updated entry #{id}"))
    }

    fn execute_remove(&self, arguments: &str) -> Result<String, SkillError> {
        let args: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| format!("invalid arguments: {e}"))?;
        let id = args["id"]
            .as_u64()
            .ok_or_else(|| "missing required argument: id".to_string())? as u32;
        let mut sp = self.lock_scratchpad()?;
        let removed = sp.remove(id).map_err(|e| e.to_string())?;
        Ok(format!("Removed entry #{id}: {}", removed.label))
    }

    fn execute_list(&self, arguments: &str) -> Result<String, SkillError> {
        let args: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| format!("invalid arguments: {e}"))?;
        let kind_filter = args["kind"].as_str().and_then(EntryKind::from_str_loose);
        let active_only = args["active_only"].as_bool().unwrap_or(true);
        let sp = self.lock_scratchpad()?;
        let entries: Vec<_> = if active_only {
            sp.active_entries()
        } else {
            sp.all_entries().iter().collect()
        };
        let entries: Vec<_> = if let Some(kind) = kind_filter {
            entries.into_iter().filter(|e| e.kind == kind).collect()
        } else {
            entries
        };
        if entries.is_empty() {
            return Ok("No entries found.".to_string());
        }
        let mut lines = Vec::new();
        for entry in &entries {
            let parent_tag = entry
                .parent_id
                .map(|pid| format!(" (parent: #{pid})"))
                .unwrap_or_default();
            lines.push(format!(
                "#{} [{}] [{}] [{}] {}: {}{}",
                entry.id,
                entry.kind,
                entry.confidence,
                entry.status,
                entry.label,
                entry.content,
                parent_tag,
            ));
        }
        Ok(lines.join("\n"))
    }
}

#[async_trait]
impl Skill for ScratchpadSkill {
    fn name(&self) -> &str {
        "scratchpad"
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "scratchpad_add".to_string(),
                description: "Add a new entry to your scratchpad (working notes). Use to track hypotheses, observations, conclusions, or notes.".to_string(),
                parameters: json!({"type":"object","properties":{"kind":{"type":"string","enum":["hypothesis","observation","conclusion","note"],"description":"Entry type"},"label":{"type":"string","description":"Short label"},"content":{"type":"string","description":"Content body"},"confidence":{"type":"string","enum":["high","medium","low"],"description":"Confidence (default: medium)"},"parent_id":{"type":"integer","description":"ID of an existing entry to nest under. Omit entirely for top-level entries. Do not pass 0 as a default."}},"required":["kind","label","content"]}),
            },
            ToolDefinition {
                name: "scratchpad_update".to_string(),
                description: "Update an existing scratchpad entry's content, confidence, or status.".to_string(),
                parameters: json!({"type":"object","properties":{"id":{"type":"integer","description":"Entry ID"},"content":{"type":"string","description":"New content"},"confidence":{"type":"string","enum":["high","medium","low"],"description":"New confidence"},"status":{"type":"string","enum":["active","superseded","invalidated"],"description":"New status"}},"required":["id"]}),
            },
            ToolDefinition {
                name: "scratchpad_remove".to_string(),
                description: "Remove an entry from the scratchpad by ID.".to_string(),
                parameters: json!({"type":"object","properties":{"id":{"type":"integer","description":"Entry ID"}},"required":["id"]}),
            },
            ToolDefinition {
                name: "scratchpad_list".to_string(),
                description: "List scratchpad entries. Optionally filter by kind or include all statuses.".to_string(),
                parameters: json!({"type":"object","properties":{"kind":{"type":"string","enum":["hypothesis","observation","conclusion","note"],"description":"Filter by kind"},"active_only":{"type":"boolean","description":"If true (default), exclude invalidated"}}}),
            },
        ]
    }

    fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
        ToolCacheability::NeverCache
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        match tool_name {
            "scratchpad_add" => Some(self.execute_add(arguments)),
            "scratchpad_update" => Some(self.execute_update(arguments)),
            "scratchpad_remove" => Some(self.execute_remove(arguments)),
            "scratchpad_list" => Some(self.execute_list(arguments)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_skill() -> ScratchpadSkill {
        ScratchpadSkill::new(
            Arc::new(Mutex::new(Scratchpad::new())),
            Arc::new(std::sync::atomic::AtomicU32::new(0)),
        )
    }

    #[test]
    fn tool_definitions_returns_four_tools() {
        let defs = test_skill().tool_definitions();
        assert_eq!(defs.len(), 4);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"scratchpad_add"));
        assert!(names.contains(&"scratchpad_update"));
        assert!(names.contains(&"scratchpad_remove"));
        assert!(names.contains(&"scratchpad_list"));
    }

    #[test]
    fn skill_name() {
        assert_eq!(test_skill().name(), "scratchpad");
    }

    #[test]
    fn cacheability() {
        assert_eq!(
            test_skill().cacheability("scratchpad_add"),
            ToolCacheability::NeverCache
        );
    }

    #[tokio::test]
    async fn add_creates_entry() {
        let s = test_skill();
        let r = s
            .execute(
                "scratchpad_add",
                r#"{"kind":"hypothesis","label":"test","content":"testing"}"#,
                None,
            )
            .await;
        assert!(r.expect("h").expect("s").contains("Added entry #0: test"));
    }

    #[tokio::test]
    async fn add_validates_kind() {
        let s = test_skill();
        assert!(s
            .execute(
                "scratchpad_add",
                r#"{"kind":"invalid","label":"t","content":"t"}"#,
                None
            )
            .await
            .expect("h")
            .is_err());
    }

    #[tokio::test]
    async fn add_validates_missing_label() {
        let s = test_skill();
        assert!(s
            .execute("scratchpad_add", r#"{"kind":"note","content":"t"}"#, None)
            .await
            .expect("h")
            .is_err());
    }

    #[tokio::test]
    async fn update_changes_status() {
        let s = test_skill();
        s.execute(
            "scratchpad_add",
            r#"{"kind":"hypothesis","label":"h1","content":"t"}"#,
            None,
        )
        .await;
        assert!(s
            .execute(
                "scratchpad_update",
                r#"{"id":0,"status":"invalidated"}"#,
                None
            )
            .await
            .expect("h")
            .expect("s")
            .contains("Updated"));
        let list = s
            .execute("scratchpad_list", r#"{"active_only":false}"#, None)
            .await
            .expect("h")
            .expect("s");
        assert!(list.contains("invalidated"));
    }

    #[tokio::test]
    async fn update_requires_field() {
        let s = test_skill();
        s.execute(
            "scratchpad_add",
            r#"{"kind":"note","label":"n","content":"c"}"#,
            None,
        )
        .await;
        assert!(s
            .execute("scratchpad_update", r#"{"id":0}"#, None)
            .await
            .expect("h")
            .is_err());
    }

    #[tokio::test]
    async fn remove_deletes() {
        let s = test_skill();
        s.execute(
            "scratchpad_add",
            r#"{"kind":"note","label":"gone","content":"bye"}"#,
            None,
        )
        .await;
        assert!(s
            .execute("scratchpad_remove", r#"{"id":0}"#, None)
            .await
            .expect("h")
            .expect("s")
            .contains("Removed"));
        assert!(s
            .execute("scratchpad_list", r#"{}"#, None)
            .await
            .expect("h")
            .expect("s")
            .contains("No entries"));
    }

    #[tokio::test]
    async fn list_active_only() {
        let s = test_skill();
        s.execute(
            "scratchpad_add",
            r#"{"kind":"note","label":"active","content":"v"}"#,
            None,
        )
        .await;
        s.execute(
            "scratchpad_add",
            r#"{"kind":"note","label":"hidden","content":"h"}"#,
            None,
        )
        .await;
        s.execute(
            "scratchpad_update",
            r#"{"id":1,"status":"invalidated"}"#,
            None,
        )
        .await;
        let list = s
            .execute("scratchpad_list", r#"{}"#, None)
            .await
            .expect("h")
            .expect("s");
        assert!(list.contains("active"));
        assert!(!list.contains("hidden"));
    }

    #[tokio::test]
    async fn list_with_kind_filter() {
        let s = test_skill();
        s.execute(
            "scratchpad_add",
            r#"{"kind":"hypothesis","label":"h","content":"hypo"}"#,
            None,
        )
        .await;
        s.execute(
            "scratchpad_add",
            r#"{"kind":"note","label":"n","content":"note"}"#,
            None,
        )
        .await;
        let list = s
            .execute("scratchpad_list", r#"{"kind":"hypothesis"}"#, None)
            .await
            .expect("h")
            .expect("s");
        assert!(list.contains("hypo"));
        assert!(!list.contains("note"));
    }

    #[tokio::test]
    async fn unknown_tool() {
        assert!(test_skill()
            .execute("unknown", r#"{}"#, None)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn add_with_parent() {
        let s = test_skill();
        s.execute(
            "scratchpad_add",
            r#"{"kind":"hypothesis","label":"p","content":"r"}"#,
            None,
        )
        .await;
        assert!(s
            .execute(
                "scratchpad_add",
                r#"{"kind":"observation","label":"c","content":"s","parent_id":0}"#,
                None
            )
            .await
            .expect("h")
            .expect("s")
            .contains("Added entry #1"));
    }

    #[tokio::test]
    async fn add_invalid_parent_falls_back_with_warning() {
        let s = test_skill();
        let result = s
            .execute(
                "scratchpad_add",
                r#"{"kind":"note","label":"o","content":"n","parent_id":99}"#,
                None,
            )
            .await
            .expect("handler returned")
            .expect("should succeed with fallback");
        assert!(
            result.contains("warning"),
            "response should contain warning: {result}"
        );
        assert!(
            result.contains("parent #99 not found"),
            "response should mention dropped parent: {result}"
        );
    }
}
