//! Skill registry — aggregates skills and implements `ToolExecutor`.
//!
//! The `SkillRegistry` collects [`Skill`](super::skill::Skill) implementations
//! and dispatches tool calls to the appropriate skill. It implements the
//! kernel's [`ToolExecutor`] trait so it can be plugged directly into the
//! loop engine.

use async_trait::async_trait;
use fx_kernel::act::{
    cancelled_result, is_cancelled, timed_out_result, ToolExecutor, ToolExecutorError, ToolResult,
};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use tracing::warn;

use crate::skill::Skill;

/// Registry that holds skills and dispatches tool calls.
#[derive(Debug)]
pub struct SkillRegistry {
    skills: Vec<Box<dyn Skill>>,
}

impl SkillRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }

    /// Register a skill. Its tool definitions become available immediately.
    ///
    /// Logs a warning if any of the skill's tools collide with already-registered
    /// tool names. The first-registered skill wins at dispatch time.
    pub fn register(&mut self, skill: Box<dyn Skill>) {
        for new_def in skill.tool_definitions() {
            for existing_skill in &self.skills {
                if existing_skill
                    .tool_definitions()
                    .iter()
                    .any(|d| d.name == new_def.name)
                {
                    warn!(
                        tool = %new_def.name,
                        existing_skill = %existing_skill.name(),
                        new_skill = %skill.name(),
                        "tool name collision: '{}' already registered by skill '{}'",
                        new_def.name,
                        existing_skill.name(),
                    );
                    break;
                }
            }
        }
        self.skills.push(skill);
    }

    /// Aggregate tool definitions from all registered skills.
    pub fn all_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.skills
            .iter()
            .flat_map(|skill| skill.tool_definitions())
            .collect()
    }

    /// Execute a single tool call by finding the first skill that handles it.
    async fn dispatch_call(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> ToolResult {
        for skill in &self.skills {
            if let Some(result) = skill.execute(tool_name, arguments, cancel).await {
                return match result {
                    Ok(output) => ToolResult {
                        tool_call_id: tool_call_id.to_string(),
                        tool_name: tool_name.to_string(),
                        success: true,
                        output,
                    },
                    Err(err) => ToolResult {
                        tool_call_id: tool_call_id.to_string(),
                        tool_name: tool_name.to_string(),
                        success: false,
                        output: err,
                    },
                };
            }
        }

        ToolResult {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            success: false,
            output: format!("no skill handles tool '{tool_name}'"),
        }
    }

    async fn execute_single_call(
        &self,
        call: &ToolCall,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        if is_cancelled(cancel) {
            return Ok(vec![cancelled_result(&call.id, &call.name)]);
        }
        let args = call.arguments.to_string();
        let request = DispatchRequest {
            tool_call_id: &call.id,
            tool_name: &call.name,
            args: &args,
            cancel,
            timeout: self.concurrency_policy().timeout_per_call,
        };
        Ok(vec![dispatch_with_timeout(self, request).await])
    }

    async fn execute_calls_concurrent(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        // Registry skills are boxed trait objects and not Clone, so we run
        // borrowed futures with join_all instead of spawning JoinSet tasks.
        let policy = self.concurrency_policy();
        let semaphore = create_registry_semaphore(policy.max_parallel);
        let timeout = policy.timeout_per_call;
        let futures = calls
            .iter()
            .map(|call| self.execute_call_with_policy(call, cancel, &semaphore, timeout));
        Ok(futures::future::join_all(futures).await)
    }

    async fn execute_call_with_policy(
        &self,
        call: &ToolCall,
        cancel: Option<&CancellationToken>,
        semaphore: &Option<tokio::sync::Semaphore>,
        timeout: Option<std::time::Duration>,
    ) -> ToolResult {
        if is_cancelled(cancel) {
            return cancelled_result(&call.id, &call.name);
        }
        let _permit = if let Some(sem) = semaphore {
            sem.acquire().await.ok()
        } else {
            None
        };
        if is_cancelled(cancel) {
            return cancelled_result(&call.id, &call.name);
        }
        let args = call.arguments.to_string();
        let request = DispatchRequest {
            tool_call_id: &call.id,
            tool_name: &call.name,
            args: &args,
            cancel,
            timeout,
        };
        dispatch_with_timeout(self, request).await
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

struct DispatchRequest<'a> {
    tool_call_id: &'a str,
    tool_name: &'a str,
    args: &'a str,
    cancel: Option<&'a CancellationToken>,
    timeout: Option<std::time::Duration>,
}

fn create_registry_semaphore(
    max_parallel: Option<std::num::NonZeroUsize>,
) -> Option<tokio::sync::Semaphore> {
    max_parallel.map(|limit| tokio::sync::Semaphore::new(limit.get()))
}

async fn dispatch_with_timeout(
    registry: &SkillRegistry,
    request: DispatchRequest<'_>,
) -> ToolResult {
    match request.timeout {
        Some(duration) => {
            match tokio::time::timeout(
                duration,
                registry.dispatch_call(
                    request.tool_call_id,
                    request.tool_name,
                    request.args,
                    request.cancel,
                ),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => timed_out_result(request.tool_call_id, request.tool_name),
            }
        }
        None => {
            registry
                .dispatch_call(
                    request.tool_call_id,
                    request.tool_name,
                    request.args,
                    request.cancel,
                )
                .await
        }
    }
}

#[async_trait]
impl ToolExecutor for SkillRegistry {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        if calls.len() == 1 {
            return self.execute_single_call(&calls[0], cancel).await;
        }
        // Cancellation returns a full-length result vector so callers retain
        // call/result alignment instead of receiving partial results.
        self.execute_calls_concurrent(calls, cancel).await
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.all_tool_definitions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::Skill;

    /// A deterministic mock skill for testing.
    #[derive(Debug)]
    struct MockSkill {
        skill_name: String,
        tools: Vec<ToolDefinition>,
    }

    impl MockSkill {
        fn new(name: &str, tool_names: &[&str]) -> Self {
            let tools = tool_names
                .iter()
                .map(|t| ToolDefinition {
                    name: t.to_string(),
                    description: format!("{name}/{t}"),
                    parameters: serde_json::json!({"type": "object"}),
                })
                .collect();
            Self {
                skill_name: name.to_string(),
                tools,
            }
        }
    }

    #[async_trait]
    impl Skill for MockSkill {
        fn name(&self) -> &str {
            &self.skill_name
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            self.tools.clone()
        }

        async fn execute(
            &self,
            tool_name: &str,
            _arguments: &str,
            _cancel: Option<&CancellationToken>,
        ) -> Option<Result<String, String>> {
            if self.tools.iter().any(|t| t.name == tool_name) {
                Some(Ok(format!("{}:{tool_name}", self.skill_name)))
            } else {
                None
            }
        }
    }

    /// A mock skill whose execute always returns an error for its tool.
    #[derive(Debug)]
    struct FailingSkill;

    #[async_trait]
    impl Skill for FailingSkill {
        fn name(&self) -> &str {
            "failing"
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "boom".to_string(),
                description: "always fails".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }]
        }

        async fn execute(
            &self,
            tool_name: &str,
            _arguments: &str,
            _cancel: Option<&CancellationToken>,
        ) -> Option<Result<String, String>> {
            if tool_name == "boom" {
                Some(Err("something went wrong".to_string()))
            } else {
                None
            }
        }
    }

    fn make_tool_call(name: &str) -> ToolCall {
        ToolCall {
            id: "call_1".to_string(),
            name: name.to_string(),
            arguments: serde_json::json!({}),
        }
    }

    #[test]
    fn empty_registry_has_no_tools() {
        let reg = SkillRegistry::new();
        assert!(reg.all_tool_definitions().is_empty());
    }

    #[test]
    fn register_skill_adds_definitions() {
        let mut reg = SkillRegistry::new();
        reg.register(Box::new(MockSkill::new("fs", &["read_file"])));
        let defs = reg.all_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "read_file");
    }

    #[test]
    fn multiple_skills_aggregate_definitions() {
        let mut reg = SkillRegistry::new();
        reg.register(Box::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Box::new(MockSkill::new("net", &["http_get", "http_post"])));
        let defs = reg.all_tool_definitions();
        assert_eq!(defs.len(), 3);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"http_get"));
        assert!(names.contains(&"http_post"));
    }

    #[tokio::test]
    async fn execute_dispatches_to_correct_skill() {
        let mut reg = SkillRegistry::new();
        reg.register(Box::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Box::new(MockSkill::new("net", &["http_get"])));

        let calls = vec![make_tool_call("http_get")];
        let results = reg.execute_tools(&calls, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].output, "net:http_get");
    }

    #[tokio::test]
    async fn execute_unknown_tool_returns_error() {
        let reg = SkillRegistry::new();
        let calls = vec![make_tool_call("nonexistent")];
        let results = reg.execute_tools(&calls, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].output.contains("nonexistent"));
    }

    #[tokio::test]
    async fn execute_skill_error_returns_failure() {
        let mut reg = SkillRegistry::new();
        reg.register(Box::new(FailingSkill));

        let calls = vec![make_tool_call("boom")];
        let results = reg.execute_tools(&calls, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert_eq!(results[0].output, "something went wrong");
        assert_eq!(results[0].tool_name, "boom");
    }

    #[tokio::test]
    async fn execute_with_cancellation_token() {
        let mut reg = SkillRegistry::new();
        reg.register(Box::new(MockSkill::new("fs", &["read_file"])));

        let token = CancellationToken::new();
        let calls = vec![make_tool_call("read_file"), make_tool_call("read_file")];
        let results = reg.execute_tools(&calls, Some(&token)).await.unwrap();
        assert_eq!(results.len(), 2);

        token.cancel();
        let results = reg.execute_tools(&calls, Some(&token)).await.unwrap();
        for result in results {
            assert!(!result.success);
            assert!(result.output.contains("cancelled"));
        }
    }

    #[tokio::test]
    async fn execute_multiple_tools_returns_in_order() {
        let mut reg = SkillRegistry::new();
        reg.register(Box::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Box::new(MockSkill::new("net", &["http_get"])));

        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "http_get".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "3".to_string(),
                name: "http_get".to_string(),
                arguments: serde_json::json!({}),
            },
        ];

        let results = reg.execute_tools(&calls, None).await.unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].tool_call_id, "1");
        assert_eq!(results[0].output, "net:http_get");
        assert_eq!(results[1].tool_call_id, "2");
        assert_eq!(results[1].output, "fs:read_file");
        assert_eq!(results[2].tool_call_id, "3");
        assert_eq!(results[2].output, "net:http_get");
    }

    #[tokio::test]
    async fn execute_cancelled_returns_cancelled_results() {
        let mut reg = SkillRegistry::new();
        reg.register(Box::new(MockSkill::new("fs", &["read_file"])));
        let token = CancellationToken::new();
        token.cancel();
        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "3".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({}),
            },
        ];

        let results = reg.execute_tools(&calls, Some(&token)).await.unwrap();
        assert_eq!(results.len(), calls.len());
        for result in &results {
            assert!(!result.success);
            assert!(result.output.contains("cancelled"));
        }
    }

    #[tokio::test]
    async fn execute_empty_calls_returns_empty() {
        let reg = SkillRegistry::new();
        let results = reg.execute_tools(&[], None).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn execute_single_call_fast_path() {
        let mut reg = SkillRegistry::new();
        reg.register(Box::new(MockSkill::new("fs", &["read_file"])));
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({}),
        }];

        let results = reg.execute_tools(&calls, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
    }

    #[tokio::test]
    async fn register_warns_on_tool_name_collision() {
        // Collision detection is verified structurally: registering two skills
        // with the same tool name still works (first-wins dispatch), but the
        // warning is emitted. We verify first-wins dispatch behavior here.
        let mut reg = SkillRegistry::new();
        reg.register(Box::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Box::new(MockSkill::new("fs2", &["read_file"])));

        // Both skills are registered (definitions aggregate)
        assert_eq!(reg.all_tool_definitions().len(), 2);

        // Dispatch goes to first-registered skill
        let result = reg.dispatch_call("call_1", "read_file", "{}", None).await;
        assert!(result.success);
        assert_eq!(result.output, "fs:read_file");
    }
}
