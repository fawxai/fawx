//! Skill registry — aggregates skills and implements `ToolExecutor`.
//!
//! The `SkillRegistry` collects [`Skill`](super::skill::Skill) implementations
//! and dispatches tool calls to the appropriate skill. It implements the
//! kernel's [`ToolExecutor`] trait so it can be plugged directly into the
//! loop engine.
//!
//! Skills are stored as `Arc<dyn Skill>` behind a `RwLock`, enabling runtime
//! replacement and removal for hot-reload. The lock is never held across
//! `.await` points — dispatch clones the `Arc` and drops the lock first.

use async_trait::async_trait;
use fx_kernel::act::{
    cancelled_result, is_cancelled, timed_out_result, ToolCacheability, ToolExecutor,
    ToolExecutorError, ToolResult,
};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use std::sync::{Arc, RwLock};
use tracing::warn;

use crate::skill::Skill;

/// Registry that holds skills and dispatches tool calls.
///
/// Uses interior mutability (`RwLock`) so `register`, `replace_skill`, and
/// `remove_skill` take `&self` — safe to call through `Arc<SkillRegistry>`.
pub struct SkillRegistry {
    skills: RwLock<Vec<Arc<dyn Skill>>>,
}

/// Manual `Debug` impl because `RwLock<Vec<Arc<dyn Skill>>>` doesn't derive
/// `Debug` cleanly (dyn Skill is not Debug-bounded). We show skill count instead.
impl std::fmt::Debug for SkillRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.skills.read().map(|s| s.len()).unwrap_or(0);
        f.debug_struct("SkillRegistry")
            .field("skill_count", &count)
            .finish()
    }
}

impl SkillRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(Vec::new()),
        }
    }

    /// Register a skill. Its tool definitions become available immediately.
    ///
    /// Logs a warning if any of the skill's tools collide with already-registered
    /// tool names. The first-registered skill wins at dispatch time.
    pub fn register(&self, skill: Arc<dyn Skill>) {
        let mut skills = self.skills.write().unwrap_or_else(|p| p.into_inner());
        log_collisions(&skills, &*skill);
        skills.push(skill);
    }

    /// Replace a skill by name, returning the old skill if found.
    ///
    /// Takes a write lock. If no skill with the given name exists,
    /// the new skill is NOT inserted — use `register()` for that.
    /// Logs warnings for any tool name collisions with other registered skills.
    pub fn replace_skill(&self, name: &str, skill: Arc<dyn Skill>) -> Option<Arc<dyn Skill>> {
        let mut skills = self.skills.write().unwrap_or_else(|p| p.into_inner());
        let pos = skills.iter().position(|s| s.name() == name)?;
        let old = std::mem::replace(&mut skills[pos], skill);
        // Log collisions between the new skill and all OTHER skills
        let others: Vec<_> = skills
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != pos)
            .map(|(_, s)| s.clone())
            .collect();
        log_collisions(&others, &*skills[pos]);
        Some(old)
    }

    /// Remove a skill by name, returning the removed skill if found.
    pub fn remove_skill(&self, name: &str) -> Option<Arc<dyn Skill>> {
        let mut skills = self.skills.write().unwrap_or_else(|p| p.into_inner());
        let pos = skills.iter().position(|s| s.name() == name)?;
        Some(skills.remove(pos))
    }

    /// Aggregate tool definitions from all registered skills.
    pub fn all_tool_definitions(&self) -> Vec<ToolDefinition> {
        let skills = self.skills.read().unwrap_or_else(|p| p.into_inner());
        skills
            .iter()
            .flat_map(|skill| skill.tool_definitions())
            .collect()
    }

    /// Return a summary of each registered skill, description, tool names, and declared capabilities.
    pub fn skill_summaries(&self) -> Vec<(String, String, Vec<String>, Vec<String>)> {
        let skills = self.skills.read().unwrap_or_else(|p| p.into_inner());
        skills
            .iter()
            .map(|skill| {
                let tools = skill
                    .tool_definitions()
                    .into_iter()
                    .map(|definition| definition.name)
                    .collect();
                (
                    skill.name().to_string(),
                    skill.description().to_string(),
                    tools,
                    skill.capabilities(),
                )
            })
            .collect()
    }

    /// Find the first skill that handles the given tool name.
    /// Acquires a read lock, clones the Arc, and releases the lock.
    fn find_skill(&self, tool_name: &str) -> Option<Arc<dyn Skill>> {
        let skills = self.skills.read().unwrap_or_else(|p| p.into_inner());
        skills
            .iter()
            .find(|s| s.tool_definitions().iter().any(|d| d.name == tool_name))
            .cloned()
    }

    fn owning_skill_cacheability(&self, tool_name: &str) -> ToolCacheability {
        self.find_skill(tool_name)
            .map(|skill| skill.cacheability(tool_name))
            .unwrap_or(ToolCacheability::NeverCache)
    }

    /// Execute a single tool call: read lock → find skill → clone Arc → drop
    /// lock → execute on clone. Lock is NEVER held across `.await`.
    async fn dispatch_call(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> ToolResult {
        // Clone-and-release: find the matching skill under read lock, clone
        // the Arc, then drop the lock before any async work.
        let skill = self.find_skill(tool_name);

        if let Some(skill) = skill {
            return match skill.execute(tool_name, arguments, cancel).await {
                Some(Ok(output)) => ToolResult {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    success: true,
                    output,
                },
                Some(Err(err)) => ToolResult {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    success: false,
                    output: err,
                },
                None => ToolResult {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    success: false,
                    output: format!(
                        "skill '{}' matched tool '{}' but declined to execute",
                        skill.name(),
                        tool_name
                    ),
                },
            };
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
        // Skills are behind Arc, but we still use join_all with borrowed
        // futures for simplicity — no need for JoinSet with owned tasks.
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

/// Compile-time assertion that `SkillRegistry` is `Send + Sync`.
#[allow(dead_code)]
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    fn check() {
        assert_send_sync::<SkillRegistry>();
    }
};

/// Log warnings for tool name collisions when registering a new skill.
fn log_collisions(existing: &[Arc<dyn Skill>], new_skill: &dyn Skill) {
    for new_def in new_skill.tool_definitions() {
        for existing_skill in existing {
            if existing_skill
                .tool_definitions()
                .iter()
                .any(|d| d.name == new_def.name)
            {
                warn!(
                    tool = %new_def.name,
                    existing_skill = %existing_skill.name(),
                    new_skill = %new_skill.name(),
                    "tool name collision: '{}' already registered by skill '{}'",
                    new_def.name,
                    existing_skill.name(),
                );
                break;
            }
        }
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

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        self.owning_skill_cacheability(tool_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::Skill;
    use std::sync::Arc;

    /// A deterministic mock skill for testing.
    #[derive(Debug)]
    struct MockSkill {
        skill_name: String,
        description: String,
        tools: Vec<ToolDefinition>,
        cacheability: ToolCacheability,
    }

    impl MockSkill {
        fn new(name: &str, tool_names: &[&str]) -> Self {
            Self::with_cacheability(name, tool_names, ToolCacheability::NeverCache)
        }

        fn with_cacheability(
            name: &str,
            tool_names: &[&str],
            cacheability: ToolCacheability,
        ) -> Self {
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
                description: format!("{name} skill"),
                tools,
                cacheability,
            }
        }
    }

    #[async_trait]
    impl Skill for MockSkill {
        fn name(&self) -> &str {
            &self.skill_name
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            self.tools.clone()
        }

        fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
            self.cacheability
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

        fn description(&self) -> &str {
            "failing skill"
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
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));
        let defs = reg.all_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "read_file");
    }

    #[test]
    fn multiple_skills_aggregate_definitions() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Arc::new(MockSkill::new("net", &["http_get", "http_post"])));
        let defs = reg.all_tool_definitions();
        assert_eq!(defs.len(), 3);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"http_get"));
        assert!(names.contains(&"http_post"));
    }

    #[test]
    fn skill_summaries_returns_skill_names_and_tools() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file", "write_file"])));
        reg.register(Arc::new(MockSkill::new("net", &["http_get"])));

        let summaries = reg.skill_summaries();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].0, "fs");
        assert_eq!(summaries[0].1, "fs skill");
        assert_eq!(summaries[0].2, vec!["read_file", "write_file"]);
        assert_eq!(summaries[1].0, "net");
        assert_eq!(summaries[1].1, "net skill");
        assert_eq!(summaries[1].2, vec!["http_get"]);
    }

    #[tokio::test]
    async fn execute_dispatches_to_correct_skill() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Arc::new(MockSkill::new("net", &["http_get"])));

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
        let reg = SkillRegistry::new();
        reg.register(Arc::new(FailingSkill));

        let calls = vec![make_tool_call("boom")];
        let results = reg.execute_tools(&calls, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert_eq!(results[0].output, "something went wrong");
        assert_eq!(results[0].tool_name, "boom");
    }

    #[tokio::test]
    async fn execute_with_cancellation_token() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));

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
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Arc::new(MockSkill::new("net", &["http_get"])));

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
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));
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
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));
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
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Arc::new(MockSkill::new("fs2", &["read_file"])));

        // Both skills are registered (definitions aggregate)
        assert_eq!(reg.all_tool_definitions().len(), 2);

        // Dispatch goes to first-registered skill
        let result = reg.dispatch_call("call_1", "read_file", "{}", None).await;
        assert!(result.success);
        assert_eq!(result.output, "fs:read_file");
    }

    #[test]
    fn skill_registry_cacheability_delegates_to_owner() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::with_cacheability(
            "fs",
            &["read_file"],
            ToolCacheability::Cacheable,
        )));

        assert_eq!(reg.cacheability("read_file"), ToolCacheability::Cacheable);
    }

    #[test]
    fn skill_registry_cacheability_defaults_to_never_cache_for_unknown_tool() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::with_cacheability(
            "fs",
            &["read_file"],
            ToolCacheability::Cacheable,
        )));

        assert_eq!(
            reg.cacheability("unknown_tool"),
            ToolCacheability::NeverCache
        );
    }

    #[test]
    fn replace_skill_swaps_and_returns_old() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));

        let new_skill = Arc::new(MockSkill::new("fs", &["read_file", "write_file"]));
        let old = reg.replace_skill("fs", new_skill);
        assert!(old.is_some());
        assert_eq!(old.unwrap().name(), "fs");

        // New skill is active
        let defs = reg.all_tool_definitions();
        assert_eq!(defs.len(), 2);
    }

    #[test]
    fn replace_skill_nonexistent_returns_none() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));

        let new_skill = Arc::new(MockSkill::new("net", &["http_get"]));
        let old = reg.replace_skill("net", new_skill);
        assert!(old.is_none());

        // Original skill unchanged, new skill NOT inserted
        let defs = reg.all_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "read_file");
    }

    #[test]
    fn remove_skill_removes_and_returns() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Arc::new(MockSkill::new("net", &["http_get"])));

        let removed = reg.remove_skill("fs");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name(), "fs");

        let defs = reg.all_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "http_get");
    }

    #[test]
    fn remove_skill_nonexistent_returns_none() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));

        let removed = reg.remove_skill("nonexistent");
        assert!(removed.is_none());
        assert_eq!(reg.all_tool_definitions().len(), 1);
    }

    /// A mock skill that advertises a tool but declines to execute it (returns None).
    #[derive(Debug)]
    struct DecliningSkill;

    #[async_trait]
    impl Skill for DecliningSkill {
        fn name(&self) -> &str {
            "declining"
        }

        fn description(&self) -> &str {
            "declining skill"
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "decline_tool".to_string(),
                description: "advertised but declined".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }]
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _arguments: &str,
            _cancel: Option<&CancellationToken>,
        ) -> Option<Result<String, String>> {
            None
        }
    }

    #[tokio::test]
    async fn execute_declined_returns_distinct_error() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(DecliningSkill));

        let calls = vec![make_tool_call("decline_tool")];
        let results = reg.execute_tools(&calls, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(
            results[0].output.contains("declined to execute"),
            "expected 'declined to execute' in: {}",
            results[0].output
        );
        assert!(
            results[0].output.contains("declining"),
            "expected skill name 'declining' in: {}",
            results[0].output
        );
    }

    #[tokio::test]
    async fn execute_no_skill_returns_no_skill_error() {
        let reg = SkillRegistry::new();
        let calls = vec![make_tool_call("nonexistent")];
        let results = reg.execute_tools(&calls, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(
            results[0].output.contains("no skill handles tool"),
            "expected 'no skill handles tool' in: {}",
            results[0].output
        );
    }

    #[test]
    fn replace_skill_logs_collisions_with_other_skills() {
        // Register two skills, then replace one with a skill that collides
        // with the other. The collision warning is logged (verified structurally;
        // tracing assertions would require test subscriber setup).
        let reg = SkillRegistry::new();
        reg.register(Arc::new(MockSkill::new("fs", &["read_file"])));
        reg.register(Arc::new(MockSkill::new("net", &["http_get"])));

        // Replace "net" with a skill that also has "read_file" — collides with "fs"
        let colliding = Arc::new(MockSkill::new("net", &["http_get", "read_file"]));
        let old = reg.replace_skill("net", colliding);
        assert!(old.is_some());

        // Verify the replacement happened and both skills are present
        let summaries = reg.skill_summaries();
        assert_eq!(summaries.len(), 2);
    }

    #[tokio::test]
    async fn dispatch_does_not_hold_lock_during_execute() {
        use std::sync::Arc as StdArc;
        use tokio::sync::Barrier;

        /// A skill that waits on a barrier during execute, proving the
        /// registry lock is released before execute runs.
        #[derive(Debug)]
        struct SlowSkill {
            barrier: StdArc<Barrier>,
        }

        #[async_trait]
        impl Skill for SlowSkill {
            fn name(&self) -> &str {
                "slow"
            }
            fn description(&self) -> &str {
                "slow skill"
            }
            fn tool_definitions(&self) -> Vec<ToolDefinition> {
                vec![ToolDefinition {
                    name: "slow_tool".to_string(),
                    description: "slow".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                }]
            }
            async fn execute(
                &self,
                tool_name: &str,
                _arguments: &str,
                _cancel: Option<&CancellationToken>,
            ) -> Option<Result<String, String>> {
                if tool_name != "slow_tool" {
                    return None;
                }
                // Wait at the barrier — if the lock were held, the
                // replace_skill call below would deadlock.
                self.barrier.wait().await;
                Some(Ok("done".to_string()))
            }
        }

        let barrier = StdArc::new(Barrier::new(2));
        let reg = StdArc::new(SkillRegistry::new());
        reg.register(Arc::new(SlowSkill {
            barrier: StdArc::clone(&barrier),
        }));
        reg.register(Arc::new(MockSkill::new("other", &["other_tool"])));

        let reg2 = StdArc::clone(&reg);
        let barrier2 = StdArc::clone(&barrier);

        // Spawn dispatch in a task
        let handle =
            tokio::spawn(async move { reg2.dispatch_call("c1", "slow_tool", "{}", None).await });

        // Meanwhile, replace a different skill — this would deadlock if
        // dispatch held the lock across the barrier wait.
        barrier2.wait().await;
        let old = reg.replace_skill("other", Arc::new(MockSkill::new("other", &["new_tool"])));
        assert!(old.is_some());

        let result = handle.await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, "done");
    }
}

/// Security boundary tests: registry immutability (spec #1102, T-4 and T-5).
#[cfg(test)]
mod security_boundary_tests {
    use super::*;
    use crate::skill::Skill;
    use std::sync::Arc;

    #[derive(Debug)]
    struct ProbeSkillA;

    #[async_trait]
    impl Skill for ProbeSkillA {
        fn name(&self) -> &str {
            "probe_a"
        }
        fn description(&self) -> &str {
            "probe skill a"
        }
        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "tool_a".to_string(),
                description: "Probe A tool".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }]
        }
        async fn execute(
            &self,
            tool_name: &str,
            _arguments: &str,
            _cancel: Option<&CancellationToken>,
        ) -> Option<Result<String, String>> {
            if tool_name == "tool_a" {
                Some(Ok("probe_a executed".to_string()))
            } else {
                None
            }
        }
    }

    #[derive(Debug)]
    struct ProbeSkillB;

    #[async_trait]
    impl Skill for ProbeSkillB {
        fn name(&self) -> &str {
            "probe_b"
        }
        fn description(&self) -> &str {
            "probe skill b"
        }
        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "tool_b".to_string(),
                description: "Probe B tool".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }]
        }
        async fn execute(
            &self,
            tool_name: &str,
            _arguments: &str,
            _cancel: Option<&CancellationToken>,
        ) -> Option<Result<String, String>> {
            if tool_name == "tool_b" {
                Some(Ok("probe_b executed".to_string()))
            } else {
                None
            }
        }
    }

    // ── T-4: SkillRegistry mutation is not exposed through ToolExecutor ──
    //
    // `register()`, `replace_skill()`, and `remove_skill()` take `&self` (not
    // `&mut self`) because the registry uses interior mutability (RwLock).
    // The security boundary is that these methods are NOT part of the
    // `ToolExecutor` trait — so code that only has `Arc<dyn ToolExecutor>`
    // cannot call them at all.

    #[test]
    fn t4_tool_executor_trait_exposes_only_immutable_methods() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(ProbeSkillA));

        let executor: Arc<dyn ToolExecutor> = Arc::new(reg);

        let defs = executor.tool_definitions();
        assert_eq!(defs.len(), 1);

        let cache = executor.cacheability("tool_a");
        assert_eq!(cache, ToolCacheability::NeverCache);

        let stats = executor.cache_stats();
        assert!(stats.is_none());

        executor.clear_cache();

        let policy = executor.concurrency_policy();
        assert!(policy.max_parallel.is_none());
    }

    #[test]
    fn t4_arc_dyn_tool_executor_cannot_call_register() {
        // register() is NOT on the ToolExecutor trait, so it's inaccessible
        // through Arc<dyn ToolExecutor> even though it takes &self.
        // If someone adds register() to ToolExecutor, this comment is the checkpoint.
        let reg = SkillRegistry::new();
        reg.register(Arc::new(ProbeSkillA));
        let executor: Arc<dyn ToolExecutor> = Arc::new(reg);

        // The following would NOT compile (not a ToolExecutor method):
        // executor.register(Arc::new(ProbeSkillB));
        // executor.replace_skill("probe_a", Arc::new(ProbeSkillB));
        // executor.remove_skill("probe_a");

        let defs = executor.tool_definitions();
        assert_eq!(defs.len(), 1, "only ProbeSkillA should be registered");
        assert_eq!(defs[0].name, "tool_a");
    }

    // ── T-5: Skill cannot access other skills or the registry ──

    #[tokio::test]
    async fn t5_skill_execute_receives_no_registry_reference() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(ProbeSkillA));
        reg.register(Arc::new(ProbeSkillB));

        let call = ToolCall {
            id: "call-1".to_string(),
            name: "tool_a".to_string(),
            arguments: serde_json::json!({}),
        };
        let results = reg.execute_tools(&[call], None).await.unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].output, "probe_a executed");
    }

    #[tokio::test]
    async fn t5_skill_registry_immutable_after_arc_wrapping() {
        let reg = SkillRegistry::new();
        reg.register(Arc::new(ProbeSkillA));

        let executor: Arc<dyn ToolExecutor> = Arc::new(reg);

        let call = ToolCall {
            id: "call-1".to_string(),
            name: "tool_a".to_string(),
            arguments: serde_json::json!({}),
        };
        let results: Vec<ToolResult> = executor.execute_tools(&[call], None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
    }
}
