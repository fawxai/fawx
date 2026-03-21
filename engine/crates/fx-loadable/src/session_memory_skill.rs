//! SessionMemorySkill — update_session_memory tool for persistent session context.

use crate::skill::{Skill, SkillError};
use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_session::{SessionMemory, SessionMemoryUpdate};
use std::sync::{Arc, Mutex, MutexGuard};

pub struct SessionMemorySkill {
    memory: Arc<Mutex<SessionMemory>>,
}

impl std::fmt::Debug for SessionMemorySkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionMemorySkill").finish()
    }
}

impl SessionMemorySkill {
    pub fn new(memory: Arc<Mutex<SessionMemory>>) -> Self {
        Self { memory }
    }

    fn handle_update(&self, arguments: &str) -> Result<String, SkillError> {
        let update: SessionMemoryUpdate = serde_json::from_str(arguments)
            .map_err(|error| format!("invalid arguments: {error}"))?;
        let mut memory = lock_memory(&self.memory);
        memory.apply_update(update)?;
        response_payload(&memory)
    }
}

#[async_trait]
impl Skill for SessionMemorySkill {
    fn name(&self) -> &str {
        "session_memory"
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![tool_definition()]
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
        if tool_name != "update_session_memory" {
            return None;
        }
        Some(self.handle_update(arguments))
    }
}

fn tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "update_session_memory".to_string(),
        description: concat!(
            "Update persistent session memory with key facts about this session. ",
            "Use when you learn something important about the project, make a decision, ",
            "or the state of work changes. This memory survives conversation compaction ",
            "and keeps you oriented across long sessions."
        )
        .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "project": {
                    "type": "string",
                    "description": "What this session is about"
                },
                "current_state": {
                    "type": "string",
                    "description": "Current state of work"
                },
                "key_decisions": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Key decisions to remember (appended, max 20)"
                },
                "active_files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Files actively being worked on (replaces list)"
                },
                "custom_context": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Any other context to remember (appended, max 20)"
                }
            }
        }),
    }
}

fn lock_memory(memory: &Arc<Mutex<SessionMemory>>) -> MutexGuard<'_, SessionMemory> {
    match memory.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn response_payload(memory: &SessionMemory) -> Result<String, SkillError> {
    let rendered = memory.render();
    serde_json::to_string(&serde_json::json!({
        "status": "updated",
        "estimated_tokens": memory.estimated_tokens(),
        "preview": preview_text(&rendered, 200),
    }))
    .map_err(|error| format!("serialize response: {error}"))
}

fn preview_text(text: &str, max_chars: usize) -> String {
    let preview: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        format!("{preview}...")
    } else {
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_provides_one_tool() {
        let skill = SessionMemorySkill::new(Arc::new(Mutex::new(SessionMemory::default())));
        let definitions = skill.tool_definitions();

        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].name, "update_session_memory");
    }

    #[tokio::test]
    async fn skill_update_modifies_memory() {
        let memory = Arc::new(Mutex::new(SessionMemory::default()));
        let skill = SessionMemorySkill::new(Arc::clone(&memory));

        let result = skill
            .execute(
                "update_session_memory",
                r#"{"project":"Phase 3","active_files":["session.rs"]}"#,
                None,
            )
            .await;

        assert!(matches!(result, Some(Ok(_))));
        let stored = lock_memory(&memory).clone();
        assert_eq!(stored.project.as_deref(), Some("Phase 3"));
        assert_eq!(stored.active_files, vec!["session.rs".to_string()]);
    }

    #[tokio::test]
    async fn skill_rejects_oversized_memory() {
        let memory = Arc::new(Mutex::new(SessionMemory::default()));
        let skill = SessionMemorySkill::new(memory);
        let long_text = "x".repeat(20_000);
        let arguments = serde_json::json!({ "project": long_text }).to_string();

        let result = skill
            .execute("update_session_memory", &arguments, None)
            .await;

        assert!(matches!(result, Some(Err(error)) if error.contains("token cap")));
    }

    #[tokio::test]
    async fn skill_returns_none_for_unknown_tool() {
        let skill = SessionMemorySkill::new(Arc::new(Mutex::new(SessionMemory::default())));

        let result = skill.execute("unknown_tool", "{}", None).await;

        assert!(result.is_none());
    }
}
