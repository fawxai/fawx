//! Skill trait — the unit of pluggable tool behavior.
//!
//! A `Skill` provides a set of tool definitions and handles execution
//! for those tools. Skills are registered into a [`super::registry::SkillRegistry`]
//! which dispatches tool calls to the appropriate skill.

use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;

/// Error type for skill execution failures.
///
/// V1 uses a plain `String` because skill errors are surfaced directly as
/// human-readable text in `ToolResult::output`. There is no programmatic
/// dispatch on error variants today — the agent simply reads the message.
/// When we add retry logic or structured error handling, this should become
/// an enum with proper variants. For now, a type alias makes the intent
/// explicit and provides a single point to swap in a real type later.
pub type SkillError = String;

/// A pluggable skill that provides tool definitions and handles tool calls.
///
/// Each skill has a unique name, exposes one or more tool definitions to the
/// reasoning model, and can execute tool calls by name. If a skill does not
/// handle a particular tool, its `execute` method returns `None`.
#[async_trait]
pub trait Skill: Send + Sync + std::fmt::Debug {
    /// Unique name identifying this skill.
    fn name(&self) -> &str;

    /// Human-readable description of this skill.
    fn description(&self) -> &str {
        ""
    }

    /// Tool definitions this skill provides to the reasoning model.
    fn tool_definitions(&self) -> Vec<ToolDefinition>;

    /// Declared skill capabilities / permissions.
    fn capabilities(&self) -> Vec<String> {
        Vec::new()
    }

    /// Cacheability classification for the given tool name.
    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        let _ = tool_name;
        ToolCacheability::NeverCache
    }

    /// Execute a tool call by name.
    ///
    /// # Arguments
    ///
    /// * `tool_name` — the name of the tool being invoked.
    /// * `arguments` — a JSON string of the tool call arguments. We accept
    ///   `&str` rather than `&serde_json::Value` deliberately: skills own their
    ///   own deserialization (they know their schema), keeping the trait free of
    ///   a `serde_json` dependency at the API boundary. The registry already has
    ///   the `Value`; it serializes once via `to_string()`. Skills that need
    ///   structured access call `serde_json::from_str` internally — a cheap
    ///   operation on the small payloads tool calls carry.
    /// * `cancel` — optional cancellation token for cooperative cancellation.
    ///
    /// Returns `Some(Ok(output))` on success, `Some(Err(message))` on failure,
    /// or `None` if this skill does not handle the given tool.
    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestSkill;

    #[async_trait]
    impl Skill for TestSkill {
        fn name(&self) -> &str {
            "test_skill"
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "greet".to_string(),
                description: "Says hello".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }]
        }

        async fn execute(
            &self,
            tool_name: &str,
            _arguments: &str,
            _cancel: Option<&CancellationToken>,
        ) -> Option<Result<String, SkillError>> {
            match tool_name {
                "greet" => Some(Ok("hello".to_string())),
                _ => None,
            }
        }
    }

    #[test]
    fn mock_skill_provides_definitions() {
        let skill = TestSkill;
        let defs = skill.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "greet");
        assert_eq!(skill.name(), "test_skill");
    }

    #[test]
    fn skill_default_cacheability_is_never_cache() {
        let skill = TestSkill;
        assert_eq!(skill.cacheability("greet"), ToolCacheability::NeverCache);
    }

    #[tokio::test]
    async fn mock_skill_handles_known_call() {
        let skill = TestSkill;
        let result = skill.execute("greet", "{}", None).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap(), Ok("hello".to_string()));
    }

    #[tokio::test]
    async fn mock_skill_returns_none_for_unknown_call() {
        let skill = TestSkill;
        let result = skill.execute("unknown", "{}", None).await;
        assert!(result.is_none());
    }

    // ── T-6: Skill execute signature provides minimal surface ──

    #[test]
    fn t6_skill_execute_takes_minimal_parameters() {
        // Compile-time assertion. If Skill::execute() signature changes to
        // include additional parameters (registry, executor, signal collector),
        // the TestSkill implementation above fails to compile.
        //
        // Trait signature:
        //   async fn execute(&self, &str, &str, Option<&CancellationToken>)
        //       -> Option<Result<String, SkillError>>
        //
        // No kernel types appear in the signature.

        let skill = TestSkill;
        let tool_name: &str = "greet";
        let arguments: &str = "{}";
        let cancel: Option<&CancellationToken> = None;

        let _future = skill.execute(tool_name, arguments, cancel);
    }

    // ── T-10: Skill has no signal access ──

    #[test]
    fn t10_skill_has_no_signal_access() {
        // Skill::execute() returns Option<Result<String, SkillError>>.
        // No method on the Skill trait accepts or returns SignalCollector,
        // Signal, or any signal-related type. The complete trait surface is:
        //   - name(&self) -> &str
        //   - tool_definitions(&self) -> Vec<ToolDefinition>
        //   - cacheability(&self, &str) -> ToolCacheability
        //   - execute(&self, &str, &str, Option<&CancellationToken>)
        //       -> Option<Result<String, SkillError>>
        //
        // If any of these gains a SignalCollector parameter, this test file
        // will fail to compile because the TestSkill mock implements the
        // current signatures exactly.
        //
        // Runtime verification: a skill returns String output, not signals.
        let skill = TestSkill;
        let defs = skill.tool_definitions();
        assert_eq!(defs.len(), 1, "skill exposes only tool definitions");
        assert_eq!(skill.cacheability("greet"), ToolCacheability::NeverCache);
        assert_eq!(skill.name(), "test_skill");
    }
}
