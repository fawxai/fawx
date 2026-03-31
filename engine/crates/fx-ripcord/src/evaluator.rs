use crate::config::TripwireConfig;
use crate::journal::{JournalAction, RipcordJournal};
use async_trait::async_trait;
use fx_kernel::act::{
    ConcurrencyPolicy, ToolCacheStats, ToolCacheability, ToolCallClassification, ToolExecutor,
    ToolExecutorError, ToolResult,
};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use serde_json::Value;
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
        let category = self.inner.action_category(call);
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
        if let Some(action) = self.inner.journal_action(call, result) {
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

    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
        self.inner.classify_call(call)
    }

    fn action_category(&self, call: &ToolCall) -> &'static str {
        self.inner.action_category(call)
    }

    fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
        self.inner.journal_action(call, result)
    }

    fn clear_cache(&self) {
        self.inner.clear_cache();
    }

    fn cache_stats(&self) -> Option<ToolCacheStats> {
        self.inner.cache_stats()
    }
}

fn extract_path(arguments: &Value) -> Option<String> {
    string_arg(arguments, "path").or_else(|| string_arg(arguments, "file_path"))
}

fn extract_command(arguments: &Value) -> Option<String> {
    string_arg(arguments, "command")
}

fn string_arg(arguments: &Value, key: &str) -> Option<String> {
    arguments.get(key)?.as_str().map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_loadable::{Skill, SkillError, SkillRegistry};
    use std::path::{Path, PathBuf};
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

        fn action_category(&self, call: &ToolCall) -> &'static str {
            test_action_category(&call.name)
        }

        fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
            test_journal_action(call, result)
        }

        fn clear_cache(&self) {}

        fn cache_stats(&self) -> Option<ToolCacheStats> {
            None
        }

        fn concurrency_policy(&self) -> ConcurrencyPolicy {
            ConcurrencyPolicy::default()
        }
    }

    #[derive(Debug)]
    struct RegistryMetadataSkill;

    #[async_trait]
    impl Skill for RegistryMetadataSkill {
        fn name(&self) -> &str {
            "registry-metadata"
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            ["shell", "write_file", "delete_file"]
                .into_iter()
                .map(|name| ToolDefinition {
                    name: name.to_string(),
                    description: format!("test/{name}"),
                    parameters: serde_json::json!({"type": "object"}),
                })
                .collect()
        }

        fn action_category(&self, tool_name: &str) -> &'static str {
            test_action_category(tool_name)
        }

        fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
            test_journal_action(call, result)
        }

        async fn execute(
            &self,
            tool_name: &str,
            _arguments: &str,
            _cancel: Option<&CancellationToken>,
        ) -> Option<Result<String, SkillError>> {
            match tool_name {
                "shell" | "write_file" | "delete_file" => Some(Ok("executed".to_string())),
                _ => None,
            }
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

    fn test_action_category(tool_name: &str) -> &'static str {
        match tool_name {
            "shell" => "shell",
            "write_file" => "file_write",
            "delete_file" => "file_delete",
            _ => "unknown",
        }
    }

    fn test_journal_action(call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
        match call.name.as_str() {
            "shell" => Some(JournalAction::ShellCommand {
                command: extract_command(&call.arguments).unwrap_or_default(),
                exit_code: if result.success { 0 } else { 1 },
            }),
            "write_file" => Some(JournalAction::FileWrite {
                path: PathBuf::from(extract_path(&call.arguments)?),
                snapshot_hash: None,
                size_bytes: string_arg(&call.arguments, "content")
                    .map_or(0, |content| content.len() as u64),
                created: false,
            }),
            _ => None,
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

    fn registry_executor() -> SkillRegistry {
        let registry = SkillRegistry::new();
        registry.register(Arc::new(RegistryMetadataSkill));
        registry
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
    async fn registry_path_tripwire_activates_ripcord_on_match() {
        let journal = test_journal();
        let executor = TripwireEvaluator::new(
            registry_executor(),
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
    async fn registry_path_records_action_when_active() {
        let journal = test_journal();
        journal.activate("manual", "already active").await;
        let executor = TripwireEvaluator::new(
            registry_executor(),
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
        assert!(matches!(
            &entries[0].action,
            JournalAction::FileWrite {
                path,
                size_bytes: 5,
                created: false,
                ..
            } if path == Path::new("/tmp/notes.txt")
        ));
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
    fn journal_action_builds_shell_entry() {
        let call = test_call("shell", serde_json::json!({"command": "echo hi"}));
        let result = ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: true,
            output: "exit_code: 0\nstdout:\nhi".into(),
        };
        let executor = PassthroughExecutor::executed();

        let action = executor
            .journal_action(&call, &result)
            .expect("shell action");

        assert!(matches!(
            action,
            JournalAction::ShellCommand {
                command,
                exit_code: 0
            } if command == "echo hi"
        ));
    }
}
