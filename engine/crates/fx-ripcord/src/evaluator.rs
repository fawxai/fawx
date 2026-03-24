use crate::config::TripwireConfig;
use crate::journal::{JournalAction, RipcordJournal};
use async_trait::async_trait;
use fx_kernel::act::{
    ConcurrencyPolicy, ToolCacheStats, ToolCacheability, ToolExecutor, ToolExecutorError,
    ToolResult,
};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// Callback for notifying the user when a tripwire is crossed.
pub type TripwireNotifyFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Tool executor wrapper that activates ripcord monitoring after tripwire matches.
pub struct TripwireEvaluator<T: ToolExecutor> {
    inner: T,
    tripwires: Vec<TripwireConfig>,
    journal: Arc<RipcordJournal>,
    notify: Option<TripwireNotifyFn>,
}

impl<T: ToolExecutor> std::fmt::Debug for TripwireEvaluator<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TripwireEvaluator").finish()
    }
}

impl<T: ToolExecutor> TripwireEvaluator<T> {
    pub fn new(inner: T, tripwires: Vec<TripwireConfig>, journal: Arc<RipcordJournal>) -> Self {
        Self {
            inner,
            tripwires,
            journal,
            notify: None,
        }
    }

    pub fn with_notify(mut self, notify: TripwireNotifyFn) -> Self {
        self.notify = Some(notify);
        self
    }

    async fn evaluate_call(&self, call: &ToolCall, result: &ToolResult) {
        let category = tool_to_action_category(&call.name);
        let path = extract_path(&call.arguments);
        let command = extract_command(&call.arguments);
        self.journal.increment_category(category).await;
        let counts = self.journal.category_counts().await;
        self.activate_if_matched(category, path.as_deref(), command.as_deref(), &counts)
            .await;
        self.record_if_active(call, result).await;
    }

    async fn activate_if_matched(
        &self,
        category: &str,
        path: Option<&str>,
        command: Option<&str>,
        counts: &std::collections::HashMap<String, u32>,
    ) {
        if self.journal.is_active().await {
            return;
        }
        if let Some(tripwire) = self
            .tripwires
            .iter()
            .find(|tripwire| tripwire.matches(category, path, command, counts))
        {
            self.journal
                .activate(&tripwire.id, &tripwire.description)
                .await;
            if let Some(notify) = &self.notify {
                notify(&tripwire.id, &tripwire.description);
            }
        }
    }

    async fn record_if_active(&self, call: &ToolCall, result: &ToolResult) {
        if !self.journal.is_active().await {
            return;
        }
        if let Some(action) = extract_journal_action(call, result) {
            self.journal.record(&call.name, &call.id, action).await;
        }
    }
}

#[async_trait]
impl<T: ToolExecutor> ToolExecutor for TripwireEvaluator<T> {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        let results = self.inner.execute_tools(calls, cancel).await?;
        for (call, result) in calls.iter().zip(results.iter()) {
            self.evaluate_call(call, result).await;
        }
        Ok(results)
    }

    fn concurrency_policy(&self) -> ConcurrencyPolicy {
        self.inner.concurrency_policy()
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.inner.tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        self.inner.cacheability(tool_name)
    }

    fn clear_cache(&self) {
        self.inner.clear_cache();
    }

    fn cache_stats(&self) -> Option<ToolCacheStats> {
        self.inner.cache_stats()
    }
}

fn extract_journal_action(call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
    match call.name.as_str() {
        "write_file" | "create_file" | "edit_file" => file_write_action(call),
        "delete_file" | "remove_file" => file_delete_action(call),
        "git_commit" => git_commit_action(call, result),
        "git_push" => git_push_action(call, result),
        "shell" | "bash" | "execute_command" => shell_action(call, result),
        _ => None,
    }
}

fn file_write_action(call: &ToolCall) -> Option<JournalAction> {
    let path = extract_path_buf(&call.arguments)?;
    let content = string_arg(&call.arguments, "content").unwrap_or_default();
    let size_bytes = content.len() as u64;
    Some(JournalAction::FileWrite {
        path,
        snapshot_hash: None,
        size_bytes,
        created: call.name == "create_file",
    })
}

fn file_delete_action(call: &ToolCall) -> Option<JournalAction> {
    let path = extract_path_buf(&call.arguments)?;
    Some(JournalAction::FileDelete {
        path,
        snapshot_hash: "unknown".to_string(),
    })
}

fn git_commit_action(call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
    let repo = extract_repo(&call.arguments)?;
    let commit_sha = string_arg(&call.arguments, "commit_sha")
        .or_else(|| string_arg(&call.arguments, "hash"))
        .or_else(|| first_word(&result.output))
        .unwrap_or_else(|| "unknown".to_string());
    let pre_ref = string_arg(&call.arguments, "pre_ref")
        .or_else(|| string_arg(&call.arguments, "ref"))
        .unwrap_or_else(|| "HEAD~1".to_string());
    Some(JournalAction::GitCommit {
        repo,
        pre_ref,
        commit_sha,
    })
}

fn git_push_action(call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
    let repo = extract_repo(&call.arguments)?;
    let remote = string_arg(&call.arguments, "remote")
        .or_else(|| find_json_string(&result.output, "remote"))
        .unwrap_or_else(|| "origin".to_string());
    let branch = string_arg(&call.arguments, "branch")
        .or_else(|| find_json_string(&result.output, "branch"))
        .unwrap_or_else(|| "HEAD".to_string());
    let pre_ref = string_arg(&call.arguments, "pre_ref")
        .or_else(|| string_arg(&call.arguments, "ref"))
        .unwrap_or_else(|| "unknown".to_string());
    Some(JournalAction::GitPush {
        repo,
        remote,
        branch,
        pre_ref,
    })
}

fn shell_action(call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
    let command = extract_command(&call.arguments)?;
    Some(JournalAction::ShellCommand {
        command,
        exit_code: extract_exit_code(&result.output, result.success),
    })
}

fn extract_path(arguments: &Value) -> Option<String> {
    string_arg(arguments, "path").or_else(|| string_arg(arguments, "file_path"))
}

fn extract_path_buf(arguments: &Value) -> Option<PathBuf> {
    extract_path(arguments).map(PathBuf::from)
}

fn extract_command(arguments: &Value) -> Option<String> {
    string_arg(arguments, "command")
}

fn extract_repo(arguments: &Value) -> Option<PathBuf> {
    string_arg(arguments, "repo")
        .or_else(|| string_arg(arguments, "working_dir"))
        .or_else(|| string_arg(arguments, "cwd"))
        .or_else(|| string_arg(arguments, "path"))
        .map(PathBuf::from)
}

fn string_arg(arguments: &Value, key: &str) -> Option<String> {
    arguments.get(key)?.as_str().map(ToString::to_string)
}

fn first_word(text: &str) -> Option<String> {
    text.split_whitespace().next().map(ToString::to_string)
}

fn extract_exit_code(output: &str, success: bool) -> i32 {
    parse_exit_code(output).unwrap_or(if success { 0 } else { -1 })
}

fn parse_exit_code(output: &str) -> Option<i32> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("exit_code: "))?
        .trim()
        .parse()
        .ok()
}

fn find_json_string(output: &str, key: &str) -> Option<String> {
    let value: Value = serde_json::from_str(output).ok()?;
    value.get(key)?.as_str().map(ToString::to_string)
}

fn tool_to_action_category(tool_name: &str) -> &'static str {
    match tool_name {
        "web_search" | "brave_search" => "web_search",
        "web_fetch" | "fetch_url" => "web_fetch",
        "read_file" | "search_text" | "list_directory" => "read_any",
        "write_file" | "create_file" | "edit_file" => "file_write",
        "shell" | "bash" | "execute_command" => "shell",
        "git" | "git_status" | "git_diff" | "git_commit" | "git_push" => "git",
        "delete_file" | "remove_file" => "file_delete",
        "run_experiment" | "experiment" => "tool_call",
        "subagent_spawn" | "subagent_status" | "subagent_cancel" => "tool_call",
        "run_command" | "execute" => "code_execute",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[derive(Debug)]
    struct PassthroughExecutor {
        outputs: Vec<ToolResult>,
    }

    impl PassthroughExecutor {
        fn executed() -> Self {
            Self { outputs: vec![] }
        }

        fn with_outputs(outputs: Vec<ToolResult>) -> Self {
            Self { outputs }
        }
    }

    #[async_trait]
    impl ToolExecutor for PassthroughExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            if self.outputs.is_empty() {
                return Ok(calls.iter().map(executed_result).collect());
            }
            Ok(self.outputs.clone())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![]
        }

        fn cacheability(&self, _: &str) -> ToolCacheability {
            ToolCacheability::NeverCache
        }

        fn clear_cache(&self) {}

        fn cache_stats(&self) -> Option<ToolCacheStats> {
            None
        }

        fn concurrency_policy(&self) -> ConcurrencyPolicy {
            ConcurrencyPolicy::default()
        }
    }

    fn executed_result(call: &ToolCall) -> ToolResult {
        ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: true,
            output: "executed".to_string(),
        }
    }

    fn test_call(name: &str, arguments: Value) -> ToolCall {
        ToolCall {
            id: format!("call_{name}"),
            name: name.to_string(),
            arguments,
        }
    }

    fn action_tripwire() -> TripwireConfig {
        TripwireConfig {
            id: "shell_tripwire".into(),
            kind: crate::config::TripwireKind::Action {
                category: "shell".into(),
                pattern: Some("rm".into()),
            },
            description: "Dangerous shell command".into(),
            enabled: true,
        }
    }

    fn threshold_tripwire(min_count: u32) -> TripwireConfig {
        TripwireConfig {
            id: "bulk_delete".into(),
            kind: crate::config::TripwireKind::Threshold {
                category: "file_delete".into(),
                min_count,
            },
            description: "Bulk delete".into(),
            enabled: true,
        }
    }

    fn test_journal() -> Arc<RipcordJournal> {
        let temp_dir = TempDir::new().expect("temp dir");
        Arc::new(RipcordJournal::new(temp_dir.path()))
    }

    #[tokio::test]
    async fn tripwire_does_not_block_execution() {
        let journal = test_journal();
        let executor = TripwireEvaluator::new(
            PassthroughExecutor::executed(),
            vec![action_tripwire()],
            journal,
        );
        let call = test_call("shell", serde_json::json!({"command": "rm -rf tmp"}));

        let results = executor
            .execute_tools(&[call], None)
            .await
            .expect("execute");

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].output, "executed");
    }

    #[tokio::test]
    async fn tripwire_activates_ripcord_on_match() {
        let journal = test_journal();
        let executor = TripwireEvaluator::new(
            PassthroughExecutor::executed(),
            vec![action_tripwire()],
            Arc::clone(&journal),
        );
        let call = test_call("shell", serde_json::json!({"command": "rm -rf tmp"}));

        executor
            .execute_tools(&[call], None)
            .await
            .expect("execute");

        assert!(journal.is_active().await);
    }

    #[tokio::test]
    async fn tripwire_records_action_when_active() {
        let journal = test_journal();
        journal.activate("manual", "already active").await;
        let executor = TripwireEvaluator::new(
            PassthroughExecutor::executed(),
            vec![action_tripwire()],
            Arc::clone(&journal),
        );
        let call = test_call(
            "write_file",
            serde_json::json!({"path": "/tmp/notes.txt", "content": "hello"}),
        );

        executor
            .execute_tools(&[call], None)
            .await
            .expect("execute");

        let entries = journal.entries().await;
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].action, JournalAction::FileWrite { .. }));
    }

    #[tokio::test]
    async fn no_tripwire_match_does_not_activate() {
        let journal = test_journal();
        let executor = TripwireEvaluator::new(
            PassthroughExecutor::executed(),
            vec![action_tripwire()],
            Arc::clone(&journal),
        );
        let call = test_call("web_search", serde_json::json!({"query": "rust"}));

        executor
            .execute_tools(&[call], None)
            .await
            .expect("execute");

        assert!(!journal.is_active().await);
    }

    #[tokio::test]
    async fn notify_called_on_tripwire_cross() {
        let journal = test_journal();
        let notifications = Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
        let captured = Arc::clone(&notifications);
        let notify: TripwireNotifyFn = Arc::new(move |id, description| {
            let mut guard = captured.lock().expect("capture notifications");
            guard.push((id.to_string(), description.to_string()));
        });
        let executor = TripwireEvaluator::new(
            PassthroughExecutor::executed(),
            vec![action_tripwire()],
            journal,
        )
        .with_notify(notify);
        let call = test_call("shell", serde_json::json!({"command": "rm -rf tmp"}));

        executor
            .execute_tools(&[call], None)
            .await
            .expect("execute");

        let captured = notifications.lock().expect("read notifications");
        assert_eq!(
            captured.as_slice(),
            [(
                "shell_tripwire".to_string(),
                "Dangerous shell command".to_string()
            )]
        );
    }

    #[tokio::test]
    async fn threshold_tripwire_fires_after_count() {
        let journal = test_journal();
        let executor = TripwireEvaluator::new(
            PassthroughExecutor::executed(),
            vec![threshold_tripwire(2)],
            Arc::clone(&journal),
        );
        let first = test_call("delete_file", serde_json::json!({"path": "/tmp/a.txt"}));
        let second = test_call("delete_file", serde_json::json!({"path": "/tmp/b.txt"}));

        executor
            .execute_tools(&[first, second], None)
            .await
            .expect("execute");

        let status = journal.status().await;
        assert!(status.active);
        assert_eq!(status.tripwire_id.as_deref(), Some("bulk_delete"));
    }

    #[tokio::test]
    async fn results_pass_through_unchanged() {
        let call = test_call("shell", serde_json::json!({"command": "rm -rf tmp"}));
        let expected = vec![ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: false,
            output: "exit_code: 9\nstderr:\nboom".into(),
        }];
        let plain = PassthroughExecutor::with_outputs(expected.clone())
            .execute_tools(std::slice::from_ref(&call), None)
            .await
            .expect("plain execute");
        let journal = test_journal();
        let wrapped = TripwireEvaluator::new(
            PassthroughExecutor::with_outputs(expected.clone()),
            vec![action_tripwire()],
            journal,
        )
        .execute_tools(&[call], None)
        .await
        .expect("wrapped execute");

        assert_eq!(wrapped, plain);
    }

    #[test]
    fn extract_journal_action_builds_shell_entry() {
        let call = test_call("shell", serde_json::json!({"command": "echo hi"}));
        let result = ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: true,
            output: "exit_code: 0\nstdout:\nhi".into(),
        };

        let action = extract_journal_action(&call, &result).expect("shell action");

        assert!(matches!(
            action,
            JournalAction::ShellCommand {
                command,
                exit_code: 0
            } if command == "echo hi"
        ));
    }
}
