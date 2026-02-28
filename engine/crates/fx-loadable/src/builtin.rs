//! Built-in skill adapter — wraps static tool definitions as a `Skill`.
//!
//! `BuiltinSkill` bridges existing tool definitions (e.g. from `FawxToolExecutor`)
//! into the skill registry. It holds tool definitions and a handler function
//! that executes tool calls.

use async_trait::async_trait;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use std::fmt;

use crate::skill::{Skill, SkillError};

/// Handler function type for built-in tool execution.
pub type ToolHandler = Box<
    dyn Fn(&str, &str, Option<&CancellationToken>) -> Option<Result<String, SkillError>>
        + Send
        + Sync,
>;

/// A skill backed by statically defined tool definitions and a handler function.
///
/// Use this to wrap existing tool implementations (like `FawxToolExecutor`'s
/// tools) into the skill registry without rewriting them.
pub struct BuiltinSkill {
    name: String,
    definitions: Vec<ToolDefinition>,
    handler: ToolHandler,
}

impl fmt::Debug for BuiltinSkill {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuiltinSkill")
            .field("name", &self.name)
            .field("definitions", &self.definitions)
            .field("handler", &"<fn>")
            .finish()
    }
}

impl BuiltinSkill {
    /// Create a built-in skill with the given definitions and handler.
    ///
    /// The handler receives `(tool_name, arguments_json, cancel)` and returns
    /// `Some(result)` if it handles the tool, or `None` if it does not.
    pub fn new(name: String, definitions: Vec<ToolDefinition>, handler: ToolHandler) -> Self {
        Self {
            name,
            definitions,
            handler,
        }
    }
}

#[async_trait]
impl Skill for BuiltinSkill {
    fn name(&self) -> &str {
        &self.name
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.definitions.clone()
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        (self.handler)(tool_name, arguments, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_definitions() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
            ToolDefinition {
                name: "write_file".to_string(),
                description: "Write a file".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            },
        ]
    }

    fn test_handler() -> ToolHandler {
        Box::new(|tool_name, _args, _cancel| match tool_name {
            "read_file" => Some(Ok("file contents".to_string())),
            "write_file" => Some(Ok("written".to_string())),
            _ => None,
        })
    }

    #[test]
    fn builtin_skill_wraps_definitions() {
        let skill = BuiltinSkill::new("builtin".to_string(), test_definitions(), test_handler());

        let defs = skill.tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
    }

    #[tokio::test]
    async fn builtin_skill_executes_known_tool() {
        let skill = BuiltinSkill::new("builtin".to_string(), test_definitions(), test_handler());

        let result = skill.execute("read_file", "{}", None).await;
        assert_eq!(result, Some(Ok("file contents".to_string())));
    }

    #[tokio::test]
    async fn builtin_skill_returns_none_for_unknown_tool() {
        let skill = BuiltinSkill::new("builtin".to_string(), test_definitions(), test_handler());

        let result = skill.execute("nonexistent", "{}", None).await;
        assert!(result.is_none());
    }

    #[test]
    fn builtin_skill_debug_format() {
        let skill = BuiltinSkill::new("test".to_string(), vec![], Box::new(|_, _, _| None));
        let debug = format!("{skill:?}");
        assert!(debug.contains("BuiltinSkill"));
        assert!(debug.contains("test"));
    }
}
