use crate::tools::FawxToolExecutor;
use fx_kernel::act::ToolExecutor;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolCall;
#[cfg(test)]
use fx_loadable::SkillRegistry;
use fx_loadable::{Skill, SkillError};
use std::collections::HashSet;
use tokio::runtime::RuntimeFlavor;

/// Built-in fx-cli tools exposed through the loadable skill registry.
///
/// # Runtime requirement
///
/// `execute` bridges async tool execution via `tokio::task::block_in_place` and
/// `Handle::block_on`, which requires a **multi-threaded Tokio runtime context**.
/// fx-cli runs on `#[tokio::main]` (multi-threaded), and callers must preserve
/// this precondition when invoking the skill.
#[derive(Debug, Clone)]
pub struct BuiltinToolsSkill {
    executor: FawxToolExecutor,
    tool_names: HashSet<String>,
}

impl BuiltinToolsSkill {
    pub(crate) fn new(executor: FawxToolExecutor) -> Self {
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

    fn has_multithread_runtime() -> bool {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.runtime_flavor() == RuntimeFlavor::MultiThread,
            Err(_) => false,
        }
    }

    fn runtime_precondition_error() -> SkillError {
        "builtin tools skill requires a multi-threaded Tokio runtime".to_string()
    }
}

impl Skill for BuiltinToolsSkill {
    fn name(&self) -> &str {
        "fawx-builtin"
    }

    fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
        self.executor.tool_definitions()
    }

    fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        // TODO(#960): propagate cancellation once Skill::execute is async
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        if !self.handles_tool(tool_name) {
            return None;
        }
        if !Self::has_multithread_runtime() {
            return Some(Err(Self::runtime_precondition_error()));
        }
        let call = match Self::build_tool_call(tool_name, arguments) {
            Ok(call) => call,
            Err(error) => return Some(Err(error)),
        };
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.executor.execute_call(&call))
        });
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
    use crate::json_memory::JsonFileMemory;
    use crate::tools::ToolConfig;
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

    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_tools_skill_executes_known_tool() {
        let temp = TempDir::new().expect("tempdir");
        let skill = BuiltinToolsSkill::new(build_memory_executor(&temp));

        let result = skill.execute("current_time", "{}", None);
        assert!(result.is_some());
        let output = result
            .expect("known tool should return Some")
            .expect("tool should succeed");
        assert!(output.contains("epoch:"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_tools_skill_requires_multi_thread_runtime() {
        // This assertion documents the runtime precondition that execute depends on.
        assert!(BuiltinToolsSkill::has_multithread_runtime());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_tools_skill_returns_none_for_unknown_tool() {
        let temp = TempDir::new().expect("tempdir");
        let skill = BuiltinToolsSkill::new(build_memory_executor(&temp));

        let result = skill.execute("nonexistent_tool", "{}", None);
        assert!(result.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn builtin_tools_skill_returns_error_for_malformed_json_arguments() {
        let temp = TempDir::new().expect("tempdir");
        let skill = BuiltinToolsSkill::new(build_memory_executor(&temp));

        let result = skill.execute("current_time", "{", None);
        assert!(result.is_some());
        let err = result
            .expect("malformed arguments should return Some")
            .expect_err("malformed arguments should fail");
        assert!(err.contains("malformed tool arguments"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn registry_dispatches_to_builtin_tools_skill() {
        let temp = TempDir::new().expect("tempdir");
        let mut registry = SkillRegistry::new();
        registry.register(Box::new(BuiltinToolsSkill::new(build_memory_executor(
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
}
