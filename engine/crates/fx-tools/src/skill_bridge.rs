use crate::tools::FawxToolExecutor;
use async_trait::async_trait;
use fx_kernel::act::{JournalAction, ToolCacheability, ToolExecutor, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::ToolAuthoritySurface;
use fx_llm::ToolCall;
#[cfg(test)]
use fx_loadable::SkillRegistry;
use fx_loadable::{Skill, SkillError};
use std::collections::HashSet;

/// Built-in fx-cli tools exposed through the loadable skill registry.
#[derive(Debug, Clone)]
pub struct BuiltinToolsSkill {
    executor: FawxToolExecutor,
    tool_names: HashSet<String>,
}

impl BuiltinToolsSkill {
    pub fn new(executor: FawxToolExecutor) -> Self {
        let tool_names = executor
            .tool_definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect();
        Self {
            executor,
            tool_names,
        }
    }

    fn handles_tool(&self, tool_name: &str) -> bool {
        self.tool_names.contains(tool_name)
    }

    fn build_tool_call(tool_name: &str, arguments: &str) -> Result<ToolCall, SkillError> {
        let parsed_args: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(value) => value,
            Err(error) => return Err(format!("malformed tool arguments: {error}")),
        };
        Ok(ToolCall {
            id: String::new(),
            name: tool_name.to_string(),
            arguments: parsed_args,
        })
    }

    fn metadata_call(tool_name: &str) -> ToolCall {
        ToolCall {
            id: String::new(),
            name: tool_name.to_string(),
            arguments: serde_json::json!({}),
        }
    }
}

#[async_trait]
impl Skill for BuiltinToolsSkill {
    fn name(&self) -> &str {
        "fawx-builtin"
    }

    fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
        self.executor.tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        self.executor.cacheability(tool_name)
    }

    fn action_category(&self, tool_name: &str) -> &'static str {
        if !self.handles_tool(tool_name) {
            return "unknown";
        }
        self.executor
            .action_category(&Self::metadata_call(tool_name))
    }

    fn authority_surface(&self, call: &ToolCall) -> ToolAuthoritySurface {
        if !self.handles_tool(&call.name) {
            return ToolAuthoritySurface::Other;
        }
        self.executor.authority_surface(call)
    }

    fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
        if !self.handles_tool(&call.name) {
            return None;
        }
        self.executor.journal_action(call, result)
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        if !self.handles_tool(tool_name) {
            return None;
        }
        let call = match Self::build_tool_call(tool_name, arguments) {
            Ok(call) => call,
            Err(error) => return Some(Err(error)),
        };
        let result = self.executor.execute_call(&call, cancel).await;
        Some(if result.success {
            Ok(result.output)
        } else {
            Err(result.output)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolConfig;
    use fx_memory::JsonFileMemory;
    use fx_subagent::test_support::StubSubagentControl;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn build_memory_executor(temp: &TempDir) -> FawxToolExecutor {
        let memory = JsonFileMemory::new(temp.path()).expect("memory init");
        let memory = std::sync::Arc::new(std::sync::Mutex::new(memory));
        FawxToolExecutor::new(temp.path().to_path_buf(), ToolConfig::default()).with_memory(memory)
    }

    #[test]
    fn builtin_tools_skill_provides_expected_tool_definitions() {
        let temp = TempDir::new().expect("tempdir");
        let skill = BuiltinToolsSkill::new(build_memory_executor(&temp));
        let defs = skill.tool_definitions();
        let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();

        let expected = [
            "read_file",
            "write_file",
            "list_directory",
            "run_command",
            "search_text",
            "self_info",
            "current_time",
            "memory_write",
            "memory_read",
            "memory_list",
            "memory_delete",
        ];
        for tool_name in expected {
            assert!(names.contains(&tool_name));
        }

        let unique_count = names.iter().copied().collect::<HashSet<_>>().len();
        assert_eq!(unique_count, names.len());
    }

    #[test]
    fn builtin_tools_skill_delegates_cacheability_classification() {
        let temp = TempDir::new().expect("tempdir");
        let skill = BuiltinToolsSkill::new(build_memory_executor(&temp));

        assert_eq!(skill.cacheability("read_file"), ToolCacheability::Cacheable);
        assert_eq!(
            skill.cacheability("write_file"),
            ToolCacheability::SideEffect
        );
        assert_eq!(
            skill.cacheability("current_time"),
            ToolCacheability::NeverCache
        );
    }

    #[test]
    fn builtin_tools_skill_delegates_action_category() {
        let temp = TempDir::new().expect("tempdir");
        let executor = build_memory_executor(&temp);
        let skill = BuiltinToolsSkill::new(executor.clone());
        let call = ToolCall {
            id: "1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({
                "path": "notes.txt",
                "content": "hello"
            }),
        };

        assert_eq!(
            skill.action_category(&call.name),
            executor.action_category(&call)
        );
    }

    #[test]
    fn builtin_tools_skill_delegates_authority_surface() {
        let temp = TempDir::new().expect("tempdir");
        let executor = build_memory_executor(&temp);
        let skill = BuiltinToolsSkill::new(executor.clone());
        let call = ToolCall {
            id: "1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({
                "path": "notes.txt",
                "content": "hello"
            }),
        };

        assert_eq!(
            skill.authority_surface(&call),
            executor.authority_surface(&call)
        );
    }

    #[test]
    fn builtin_tools_skill_delegates_journal_action() {
        let temp = TempDir::new().expect("tempdir");
        let executor = build_memory_executor(&temp);
        let skill = BuiltinToolsSkill::new(executor.clone());
        let call = ToolCall {
            id: "1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({
                "path": "notes.txt",
                "content": "hello"
            }),
        };
        let result = ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: true,
            output: "ok".to_string(),
        };

        assert_eq!(
            skill.journal_action(&call, &result),
            executor.journal_action(&call, &result)
        );
    }

    #[tokio::test]
    async fn builtin_tools_skill_executes_known_tool() {
        let temp = TempDir::new().expect("tempdir");
        let skill = BuiltinToolsSkill::new(build_memory_executor(&temp));

        let result = skill.execute("current_time", "{}", None).await;
        assert!(result.is_some());
        let output = result
            .expect("known tool should return Some")
            .expect("tool should succeed");
        assert!(output.contains("epoch:"));
    }

    #[tokio::test]
    async fn builtin_tools_skill_returns_none_for_unknown_tool() {
        let temp = TempDir::new().expect("tempdir");
        let skill = BuiltinToolsSkill::new(build_memory_executor(&temp));

        let result = skill.execute("nonexistent_tool", "{}", None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn builtin_tools_skill_returns_error_for_malformed_json_arguments() {
        let temp = TempDir::new().expect("tempdir");
        let skill = BuiltinToolsSkill::new(build_memory_executor(&temp));

        let result = skill.execute("current_time", "{", None).await;
        assert!(result.is_some());
        let err = result
            .expect("malformed arguments should return Some")
            .expect_err("malformed arguments should fail");
        assert!(err.contains("malformed tool arguments"));
    }

    #[tokio::test]
    async fn builtin_tools_skill_honors_pre_cancelled_token() {
        let temp = TempDir::new().expect("tempdir");
        let skill = BuiltinToolsSkill::new(build_memory_executor(&temp));
        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = skill.execute("current_time", "{}", Some(&cancel)).await;
        assert!(result.is_some());
        let error = result
            .expect("known tool should return Some")
            .expect_err("cancelled execution should fail");
        assert!(error.contains("cancelled"));
    }

    #[tokio::test]
    async fn registry_dispatches_to_builtin_tools_skill() {
        let temp = TempDir::new().expect("tempdir");
        let registry = SkillRegistry::new();
        registry.register(Arc::new(BuiltinToolsSkill::new(build_memory_executor(
            &temp,
        ))));

        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "current_time".to_string(),
            arguments: serde_json::json!({}),
        }];

        let results = registry
            .execute_tools(&calls, None)
            .await
            .expect("dispatch result");
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert!(results[0].output.contains("day_of_week:"));
    }

    #[test]
    fn builtin_tools_skill_adds_subagent_tools_when_control_is_attached() {
        let temp = TempDir::new().expect("tempdir");
        let control = Arc::new(StubSubagentControl::new());
        let skill =
            BuiltinToolsSkill::new(build_memory_executor(&temp).with_subagent_control(control));
        let names = skill
            .tool_definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"spawn_agent".to_string()));
        assert!(names.contains(&"subagent_status".to_string()));
    }

    #[tokio::test]
    async fn builtin_tools_skill_forwards_spawn_agent() {
        let temp = TempDir::new().expect("tempdir");
        let control = Arc::new(StubSubagentControl::new());
        let skill =
            BuiltinToolsSkill::new(build_memory_executor(&temp).with_subagent_control(control));

        let result = skill
            .execute("spawn_agent", r#"{"task":"review this"}"#, None)
            .await;
        let output = result
            .expect("known tool should return Some")
            .expect("spawn should succeed");
        let json: serde_json::Value =
            serde_json::from_str(&output).expect("spawn output should be valid json");
        assert_eq!(json["id"], "agent-1");
    }
}
