use crate::act::{
    ConcurrencyPolicy, JournalAction, SubGoalToolRoutingRequest, ToolCacheStats, ToolCacheability,
    ToolExecutor, ToolExecutorError, ToolResult,
};
use crate::cancellation::CancellationToken;
use crate::ToolAuthoritySurface;
use async_trait::async_trait;
use fx_llm::{ToolCall, ToolDefinition};
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) fn scope_tool_executor(
    inner: Arc<dyn ToolExecutor>,
    allowed_tools: &[String],
) -> Arc<dyn ToolExecutor> {
    if allowed_tools.is_empty() {
        inner
    } else {
        Arc::new(ScopedToolExecutor::new(inner, allowed_tools))
    }
}

#[derive(Clone)]
struct ScopedToolExecutor {
    inner: Arc<dyn ToolExecutor>,
    allowed_lookup: HashSet<String>,
    allowed_order: Vec<String>,
}

impl ScopedToolExecutor {
    fn new(inner: Arc<dyn ToolExecutor>, allowed_tools: &[String]) -> Self {
        let mut allowed_order = Vec::new();
        let mut allowed_lookup = HashSet::new();
        for tool_name in allowed_tools {
            if allowed_lookup.insert(tool_name.clone()) {
                allowed_order.push(tool_name.clone());
            }
        }
        Self {
            inner,
            allowed_lookup,
            allowed_order,
        }
    }

    fn allows(&self, tool_name: &str) -> bool {
        self.allowed_lookup.contains(tool_name)
    }

    fn blocked_tool_result(&self, call: &ToolCall) -> ToolResult {
        ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: false,
            output: format!(
                "Tool '{}' is not available in this sub-goal. Allowed tools: {}",
                call.name,
                self.allowed_order.join(", ")
            ),
        }
    }
}

#[async_trait]
impl ToolExecutor for ScopedToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        let mut allowed_indexed = Vec::new();
        let mut blocked_indexed = Vec::new();

        for (index, call) in calls.iter().cloned().enumerate() {
            if self.allows(&call.name) {
                allowed_indexed.push((index, call));
            } else {
                blocked_indexed.push((index, self.blocked_tool_result(&call)));
            }
        }

        let mut indexed_results = blocked_indexed;
        if !allowed_indexed.is_empty() {
            let allowed_calls: Vec<ToolCall> = allowed_indexed
                .iter()
                .map(|(_, call)| call.clone())
                .collect();
            let delegated = self.inner.execute_tools(&allowed_calls, cancel).await?;
            if delegated.len() != allowed_calls.len() {
                return Err(ToolExecutorError {
                    message: format!(
                        "scoped executor expected {} delegated results but received {}",
                        allowed_calls.len(),
                        delegated.len()
                    ),
                    recoverable: false,
                });
            }
            indexed_results.extend(
                allowed_indexed
                    .into_iter()
                    .map(|(index, _)| index)
                    .zip(delegated),
            );
        }

        indexed_results.sort_by_key(|(index, _)| *index);
        Ok(indexed_results
            .into_iter()
            .map(|(_, result)| result)
            .collect())
    }

    fn concurrency_policy(&self) -> ConcurrencyPolicy {
        self.inner.concurrency_policy()
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.inner
            .tool_definitions()
            .into_iter()
            .filter(|tool| self.allows(&tool.name))
            .collect()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        if self.allows(tool_name) {
            self.inner.cacheability(tool_name)
        } else {
            ToolCacheability::NeverCache
        }
    }

    fn classify_call(&self, call: &ToolCall) -> crate::act::ToolCallClassification {
        if self.allows(&call.name) {
            self.inner.classify_call(call)
        } else {
            crate::act::ToolCallClassification::Mutation
        }
    }

    fn action_category(&self, call: &ToolCall) -> &'static str {
        if self.allows(&call.name) {
            self.inner.action_category(call)
        } else {
            "unknown"
        }
    }

    fn authority_surface(&self, call: &ToolCall) -> ToolAuthoritySurface {
        if self.allows(&call.name) {
            self.inner.authority_surface(call)
        } else {
            ToolAuthoritySurface::Other
        }
    }

    fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
        if self.allows(&call.name) {
            self.inner.journal_action(call, result)
        } else {
            None
        }
    }

    fn route_sub_goal_call(
        &self,
        request: &SubGoalToolRoutingRequest,
        call_id: &str,
    ) -> Option<ToolCall> {
        if request
            .required_tools
            .iter()
            .any(|tool_name| !self.allows(tool_name))
        {
            return None;
        }

        self.inner
            .route_sub_goal_call(request, call_id)
            .filter(|call| self.allows(&call.name))
    }

    fn clear_cache(&self) {
        self.inner.clear_cache();
    }

    fn cache_stats(&self) -> Option<ToolCacheStats> {
        self.inner.cache_stats()
    }
}

impl std::fmt::Debug for ScopedToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedToolExecutor")
            .field("inner", &"ToolExecutor")
            .field("allowed_tools", &self.allowed_order)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::ToolCallClassification;
    use serde_json::json;

    #[derive(Debug, Default)]
    struct StubToolExecutor;

    #[async_trait]
    impl ToolExecutor for StubToolExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success: true,
                    output: format!("executed {}", call.name),
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![
                ToolDefinition {
                    name: "read_file".to_string(),
                    description: "read".to_string(),
                    parameters: json!({"type":"object","required":["path"]}),
                },
                ToolDefinition {
                    name: "current_time".to_string(),
                    description: "time".to_string(),
                    parameters: json!({"type":"object","required":[]}),
                },
            ]
        }

        fn cacheability(&self, tool_name: &str) -> ToolCacheability {
            match tool_name {
                "read_file" | "current_time" => ToolCacheability::Cacheable,
                _ => ToolCacheability::NeverCache,
            }
        }

        fn classify_call(&self, _call: &ToolCall) -> ToolCallClassification {
            ToolCallClassification::Observation
        }

        fn route_sub_goal_call(
            &self,
            request: &SubGoalToolRoutingRequest,
            call_id: &str,
        ) -> Option<ToolCall> {
            Some(ToolCall {
                id: call_id.to_string(),
                name: request.required_tools.first()?.clone(),
                arguments: json!({}),
            })
        }
    }

    fn scoped_executor(allowed_tools: &[&str]) -> ScopedToolExecutor {
        ScopedToolExecutor::new(
            Arc::new(StubToolExecutor),
            &allowed_tools
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn tool_definitions_are_filtered_to_scope() {
        let executor = scoped_executor(&["read_file"]);
        let tool_names: Vec<String> = executor
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(tool_names, vec!["read_file"]);
    }

    #[tokio::test]
    async fn execute_tools_blocks_calls_outside_scope() {
        let executor = scoped_executor(&["read_file"]);
        let calls = vec![
            ToolCall {
                id: "call-1".to_string(),
                name: "current_time".to_string(),
                arguments: json!({}),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "read_file".to_string(),
                arguments: json!({"path":"Cargo.toml"}),
            },
        ];

        let results = executor.execute_tools(&calls, None).await.expect("results");
        assert_eq!(results.len(), 2);
        assert!(!results[0].success);
        assert!(results[0].output.contains("Allowed tools: read_file"));
        assert!(results[1].success);
        assert_eq!(results[1].tool_name, "read_file");
    }

    #[test]
    fn route_sub_goal_call_respects_scope() {
        let executor = scoped_executor(&["read_file"]);
        let allowed_request = SubGoalToolRoutingRequest {
            description: "Read config".to_string(),
            required_tools: vec!["read_file".to_string()],
        };
        let blocked_request = SubGoalToolRoutingRequest {
            description: "Check time".to_string(),
            required_tools: vec!["current_time".to_string()],
        };

        assert!(executor
            .route_sub_goal_call(&allowed_request, "call-1")
            .is_some());
        assert!(executor
            .route_sub_goal_call(&blocked_request, "call-2")
            .is_none());
    }
}
