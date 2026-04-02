//! Session management tools: list, history, send.
//!
//! Exposes `SessionToolsSkill` which implements `Skill` and delegates
//! to a `SessionRegistry` for cross-session operations.

use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_loadable::{Skill, SkillError};
use fx_session::{SessionKey, SessionKind, SessionRegistry};
use serde::Deserialize;
use std::collections::HashSet;

/// Skill that provides session management tools.
#[derive(Debug, Clone)]
pub struct SessionToolsSkill {
    registry: SessionRegistry,
    tool_names: HashSet<String>,
}

impl SessionToolsSkill {
    /// Create a new session tools skill backed by the given registry.
    pub fn new(registry: SessionRegistry) -> Self {
        let tool_names = session_tool_definitions()
            .into_iter()
            .map(|d| d.name)
            .collect();
        Self {
            registry,
            tool_names,
        }
    }

    fn handles_tool(&self, tool_name: &str) -> bool {
        self.tool_names.contains(tool_name)
    }

    fn execute_tool(&self, tool_name: &str, args: &str) -> Result<String, SkillError> {
        let parsed: serde_json::Value =
            serde_json::from_str(args).map_err(|e| format!("malformed arguments: {e}"))?;
        match tool_name {
            "session_list" => self.handle_list(&parsed),
            "session_history" => self.handle_history(&parsed),
            "session_send" => self.handle_send(&parsed),
            _ => Err(format!("unknown session tool: {tool_name}")),
        }
    }

    fn handle_list(&self, args: &serde_json::Value) -> Result<String, SkillError> {
        let parsed: SessionListArgs = parse_args(args)?;
        let filter = match parsed.kind.as_deref() {
            Some(k) => Some(parse_session_kind(k)?),
            None => None,
        };
        let sessions = self
            .registry
            .list(filter)
            .map_err(|e| format!("session list failed: {e}"))?;
        if sessions.is_empty() {
            return Ok("no sessions found".to_string());
        }
        let output = serde_json::to_string_pretty(&sessions)
            .map_err(|e| format!("serialization failed: {e}"))?;
        Ok(output)
    }

    fn handle_history(&self, args: &serde_json::Value) -> Result<String, SkillError> {
        let parsed: SessionHistoryArgs = parse_args(args)?;
        let key = SessionKey::new(&parsed.session_key)
            .map_err(|e| format!("invalid session key: {e}"))?;
        let limit = parsed.limit.unwrap_or(20);
        let messages = self
            .registry
            .history(&key, limit)
            .map_err(|e| format!("session history failed: {e}"))?;
        if messages.is_empty() {
            return Ok("no messages in session".to_string());
        }
        let output = serde_json::to_string_pretty(&messages)
            .map_err(|e| format!("serialization failed: {e}"))?;
        Ok(output)
    }

    fn handle_send(&self, args: &serde_json::Value) -> Result<String, SkillError> {
        let parsed: SessionSendArgs = parse_args(args)?;
        let key = SessionKey::new(&parsed.session_key)
            .map_err(|e| format!("invalid session key: {e}"))?;
        let ack = self
            .registry
            .send(&key, &parsed.message)
            .map_err(|e| format!("session send failed: {e}"))?;
        Ok(ack)
    }
}

#[async_trait]
impl Skill for SessionToolsSkill {
    fn name(&self) -> &str {
        "session-tools"
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        session_tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        match tool_name {
            "session_list" | "session_history" => ToolCacheability::NeverCache,
            "session_send" => ToolCacheability::SideEffect,
            _ => ToolCacheability::NeverCache,
        }
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        if !self.handles_tool(tool_name) {
            return None;
        }
        Some(self.execute_tool(tool_name, arguments))
    }
}

/// Tool definitions for the three session management tools.
pub fn session_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        session_list_tool_def(),
        session_history_tool_def(),
        session_send_tool_def(),
    ]
}

fn session_list_tool_def() -> ToolDefinition {
    ToolDefinition {
        name: "session_list".to_string(),
        description:
            "List active sessions with metadata (kind, status, model, age, message count)."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["main", "subagent", "channel", "cron"],
                    "description": "Filter by session kind. Omit for all."
                }
            },
            "required": []
        }),
    }
}

fn session_history_tool_def() -> ToolDefinition {
    ToolDefinition {
        name: "session_history".to_string(),
        description: "Fetch conversation history from a session.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "session_key": {
                    "type": "string",
                    "description": "Key of the session to inspect."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max number of recent messages. Default 20."
                }
            },
            "required": ["session_key"]
        }),
    }
}

fn session_send_tool_def() -> ToolDefinition {
    ToolDefinition {
        name: "session_send".to_string(),
        description: "Send a message into another session.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "session_key": {
                    "type": "string",
                    "description": "Key of the target session."
                },
                "message": {
                    "type": "string",
                    "description": "Message content to send."
                }
            },
            "required": ["session_key", "message"]
        }),
    }
}

fn parse_session_kind(kind: &str) -> Result<SessionKind, SkillError> {
    match kind {
        "main" => Ok(SessionKind::Main),
        "subagent" => Ok(SessionKind::Subagent),
        "channel" => Ok(SessionKind::Channel),
        "cron" => Ok(SessionKind::Cron),
        other => Err(format!(
            "invalid session kind '{other}': expected one of main, subagent, channel, cron"
        )),
    }
}

fn parse_args<T: for<'de> Deserialize<'de>>(value: &serde_json::Value) -> Result<T, SkillError> {
    serde_json::from_value(value.clone()).map_err(|e| format!("invalid arguments: {e}"))
}

#[derive(Deserialize)]
struct SessionListArgs {
    kind: Option<String>,
}

#[derive(Deserialize)]
struct SessionHistoryArgs {
    session_key: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct SessionSendArgs {
    session_key: String,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_session::{
        MessageRole, Session, SessionConfig, SessionContentBlock, SessionMemory, SessionMessage,
        SessionStatus, SessionStore,
    };
    use fx_storage::Storage;

    fn test_skill() -> SessionToolsSkill {
        let storage = Storage::open_in_memory().expect("storage");
        let store = SessionStore::new(storage);
        let registry = SessionRegistry::new(store).expect("registry");
        SessionToolsSkill::new(registry)
    }

    fn skill_with_sessions() -> SessionToolsSkill {
        let storage = Storage::open_in_memory().expect("storage");
        let store = SessionStore::new(storage);
        let registry = SessionRegistry::new(store).expect("registry");
        registry
            .create(
                SessionKey::new("main-1").unwrap(),
                SessionKind::Main,
                SessionConfig {
                    label: Some("primary".to_string()),
                    model: "gpt-4".to_string(),
                },
            )
            .expect("create main");
        registry
            .create(
                SessionKey::new("sub-1").unwrap(),
                SessionKind::Subagent,
                SessionConfig {
                    label: Some("worker".to_string()),
                    model: "claude".to_string(),
                },
            )
            .expect("create sub");
        SessionToolsSkill::new(registry)
    }

    fn skill_with_grouped_tool_history() -> SessionToolsSkill {
        let storage = Storage::open_in_memory().expect("storage");
        let store = SessionStore::new(storage);
        let registry = SessionRegistry::new(store).expect("registry");
        let key = SessionKey::new("main-1").expect("session key");
        registry
            .create(
                key.clone(),
                SessionKind::Main,
                SessionConfig {
                    label: Some("primary".to_string()),
                    model: "gpt-4".to_string(),
                },
            )
            .expect("create session");
        registry
            .record_turn(
                &key,
                vec![
                    SessionMessage::structured(
                        MessageRole::Assistant,
                        vec![
                            SessionContentBlock::ToolUse {
                                id: "call_1".to_string(),
                                provider_id: Some("fc_1".to_string()),
                                name: "read_file".to_string(),
                                input: serde_json::json!({"path": "README.md"}),
                            },
                            SessionContentBlock::ToolUse {
                                id: "call_2".to_string(),
                                provider_id: Some("fc_2".to_string()),
                                name: "list_dir".to_string(),
                                input: serde_json::json!({"path": "."}),
                            },
                        ],
                        1,
                        None,
                    ),
                    SessionMessage::structured(
                        MessageRole::Tool,
                        vec![
                            SessionContentBlock::ToolResult {
                                tool_use_id: "call_1".to_string(),
                                content: serde_json::json!("read ok"),
                                is_error: Some(false),
                            },
                            SessionContentBlock::ToolResult {
                                tool_use_id: "call_2".to_string(),
                                content: serde_json::json!(["Cargo.toml"]),
                                is_error: Some(false),
                            },
                        ],
                        2,
                        None,
                    ),
                    SessionMessage::text(MessageRole::Assistant, "Done.", 3),
                ],
                SessionMemory::default(),
            )
            .expect("record turn");
        SessionToolsSkill::new(registry)
    }

    fn skill_with_poisoned_session() -> SessionToolsSkill {
        let storage = Storage::open_in_memory().expect("storage");
        let store = SessionStore::new(storage.clone());
        store
            .save(&poisoned_session("poisoned"))
            .expect("save poisoned session");
        let registry = SessionRegistry::new(SessionStore::new(storage)).expect("registry");
        SessionToolsSkill::new(registry)
    }

    fn poisoned_session(id: &str) -> Session {
        Session {
            key: SessionKey::new(id).expect("session key"),
            kind: SessionKind::Main,
            status: SessionStatus::Idle,
            label: Some("poisoned".to_string()),
            model: "gpt-4".to_string(),
            created_at: 1,
            updated_at: 2,
            archived_at: None,
            messages: vec![
                SessionMessage::structured(
                    MessageRole::Tool,
                    vec![SessionContentBlock::ToolResult {
                        tool_use_id: "call_bad".to_string(),
                        content: serde_json::json!("bad"),
                        is_error: Some(false),
                    }],
                    1,
                    None,
                ),
                SessionMessage::structured(
                    MessageRole::Assistant,
                    vec![SessionContentBlock::ToolUse {
                        id: "call_bad".to_string(),
                        provider_id: Some("fc_bad".to_string()),
                        name: "read_file".to_string(),
                        input: serde_json::json!({"path": "bad.txt"}),
                    }],
                    2,
                    None,
                ),
            ],
            memory: SessionMemory::default(),
        }
    }

    #[test]
    fn tool_definitions_includes_all_three_tools() {
        let defs = session_tool_definitions();
        let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"session_list"));
        assert!(names.contains(&"session_history"));
        assert!(names.contains(&"session_send"));
        assert_eq!(defs.len(), 3);
    }

    #[test]
    fn skill_name_is_session_tools() {
        let skill = test_skill();
        assert_eq!(skill.name(), "session-tools");
    }

    #[test]
    fn list_empty_registry_returns_message() {
        let skill = test_skill();
        let result = skill.execute_tool("session_list", "{}");
        assert_eq!(result.expect("should succeed"), "no sessions found");
    }

    #[test]
    fn list_returns_all_sessions() {
        let skill = skill_with_sessions();
        let result = skill
            .execute_tool("session_list", "{}")
            .expect("should succeed");
        assert!(result.contains("main-1"));
        assert!(result.contains("sub-1"));
    }

    #[test]
    fn list_filters_by_kind() {
        let skill = skill_with_sessions();
        let result = skill
            .execute_tool("session_list", r#"{"kind": "subagent"}"#)
            .expect("should succeed");
        assert!(result.contains("sub-1"));
        assert!(!result.contains("main-1"));
    }

    #[test]
    fn history_empty_session_returns_message() {
        let skill = skill_with_sessions();
        let result = skill
            .execute_tool("session_history", r#"{"session_key": "main-1"}"#)
            .expect("should succeed");
        assert_eq!(result, "no messages in session");
    }

    #[test]
    fn history_nonexistent_session_returns_error() {
        let skill = test_skill();
        let result = skill.execute_tool("session_history", r#"{"session_key": "missing"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn history_returns_turn_scoped_grouped_tool_history() {
        let skill = skill_with_grouped_tool_history();
        let result = skill
            .execute_tool("session_history", r#"{"session_key": "main-1"}"#)
            .expect("history should succeed");
        let json: serde_json::Value = serde_json::from_str(&result).expect("history json");

        assert_eq!(json.as_array().expect("messages").len(), 3);
        assert_eq!(json[0]["role"], "assistant");
        assert_eq!(json[0]["content"].as_array().expect("tool uses").len(), 2);
        assert_eq!(json[0]["content"][0]["provider_id"], "fc_1");
        assert_eq!(json[0]["content"][1]["provider_id"], "fc_2");
        assert_eq!(json[1]["role"], "tool");
        assert_eq!(
            json[1]["content"].as_array().expect("tool results").len(),
            2
        );
        assert_eq!(json[2]["role"], "assistant");
        assert_eq!(json[2]["content"][0]["text"], "Done.");
    }

    #[test]
    fn history_rejects_corrupted_session_history() {
        let skill = skill_with_poisoned_session();
        let result = skill.execute_tool("session_history", r#"{"session_key": "poisoned"}"#);

        let error = result.expect_err("corrupted history should fail");
        assert!(error.contains("corrupted session 'poisoned'"));
        assert!(error.contains("call_bad"));
    }

    #[test]
    fn send_records_message_and_returns_ack() {
        let skill = skill_with_sessions();
        let ack = skill
            .execute_tool(
                "session_send",
                r#"{"session_key": "main-1", "message": "hello"}"#,
            )
            .expect("should succeed");
        assert!(ack.contains("main-1"));

        // Verify message appears in history
        let history = skill
            .execute_tool("session_history", r#"{"session_key": "main-1"}"#)
            .expect("history");
        assert!(history.contains("hello"));
    }

    #[test]
    fn send_to_nonexistent_session_returns_error() {
        let skill = test_skill();
        let result = skill.execute_tool(
            "session_send",
            r#"{"session_key": "nope", "message": "hi"}"#,
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_returns_none_for_unknown_tool() {
        let skill = test_skill();
        let result = skill.execute("unknown_tool", "{}", None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn execute_dispatches_session_list() {
        let skill = skill_with_sessions();
        let result = skill.execute("session_list", "{}", None).await;
        assert!(result.is_some());
        let output = result.unwrap().expect("should succeed");
        assert!(output.contains("main-1"));
    }

    #[test]
    fn cacheability_classifies_session_tools() {
        let skill = test_skill();
        assert_eq!(
            skill.cacheability("session_list"),
            ToolCacheability::NeverCache
        );
        assert_eq!(
            skill.cacheability("session_history"),
            ToolCacheability::NeverCache
        );
        assert_eq!(
            skill.cacheability("session_send"),
            ToolCacheability::SideEffect
        );
    }

    #[test]
    fn list_with_invalid_kind_returns_error() {
        let skill = skill_with_sessions();
        let result = skill.execute_tool("session_list", r#"{"kind": "bogus"}"#);
        let err = result.expect_err("invalid kind should fail");
        assert!(err.contains("invalid session kind"));
        assert!(err.contains("bogus"));
    }

    #[test]
    fn malformed_json_returns_error() {
        let skill = test_skill();
        let result = skill.execute_tool("session_list", "not json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("malformed"));
    }
}
