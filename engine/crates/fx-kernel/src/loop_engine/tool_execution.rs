use super::bounded_local::{partition_by_bounded_local_phase_semantics, TurnExecutionProfile};
use super::retry::{partition_by_retry_policy, BlockedToolCall};
use super::{
    loop_error, CycleStream, LoopEngine, ToolRoundState, NOTIFY_TOOL_NAME,
    OBSERVATION_ONLY_CALL_BLOCK_REASON,
};
use crate::act::{ToolCacheability, ToolCallClassification, ToolExecutor, ToolResult};
use crate::budget::{truncate_tool_result, ActionCost};
use crate::signals::{LoopStep, SignalKind};
use crate::streaming::ErrorCategory;
use crate::types::LoopError;
use fx_core::message::{InternalMessage, ToolRoundCall, ToolRoundResult};
use fx_llm::{ContentBlock, Message, MessageRole, ToolCall};
use std::collections::{HashMap, HashSet};

struct PreparedToolCalls {
    allowed: Vec<ToolCall>,
    blocked: Vec<BlockedToolCall>,
}

impl PreparedToolCalls {
    fn new(allowed: Vec<ToolCall>, blocked: Vec<BlockedToolCall>) -> Self {
        Self { allowed, blocked }
    }

    fn filtered(mut self, allowed: Vec<ToolCall>, blocked: Vec<BlockedToolCall>) -> Self {
        self.allowed = allowed;
        self.blocked.extend(blocked);
        self
    }
}

impl LoopEngine {
    pub(super) fn publish_tool_calls(&self, calls: &[ToolCall], stream: CycleStream<'_>) {
        for call in calls {
            stream.tool_call_start(call);
            stream.tool_call_complete(call);
            self.publish_tool_use(call);
        }
    }

    pub(super) fn publish_tool_use(&self, call: &ToolCall) {
        let Some(bus) = self.public_event_bus() else {
            return;
        };
        let _ = bus.publish(InternalMessage::ToolUse {
            call_id: call.id.clone(),
            provider_id: self.tool_call_provider_ids.get(&call.id).cloned(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        });
    }

    pub(super) fn publish_tool_results(&mut self, results: &[ToolResult], stream: CycleStream<'_>) {
        for result in results {
            stream.tool_result(result);
            self.publish_tool_result(result);
        }
    }

    pub(super) fn publish_tool_round(
        &mut self,
        calls: &[ToolCall],
        results: &[ToolResult],
        stream: CycleStream<'_>,
    ) {
        self.publish_tool_calls(calls, stream);
        self.publish_tool_results(results, stream);

        let Some(bus) = self.public_event_bus() else {
            return;
        };
        let _ = bus.publish(InternalMessage::ToolRound {
            calls: calls
                .iter()
                .map(|call| ToolRoundCall {
                    call_id: call.id.clone(),
                    provider_id: self.tool_call_provider_ids.get(&call.id).cloned(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                })
                .collect(),
            results: results
                .iter()
                .map(|result| ToolRoundResult {
                    call_id: result.tool_call_id.clone(),
                    name: result.tool_name.clone(),
                    success: result.success,
                    content: result.output.clone(),
                })
                .collect(),
        });
    }

    pub(super) fn emit_tool_errors(&self, results: &[ToolResult], stream: CycleStream<'_>) -> bool {
        let mut has_errors = false;
        for result in results.iter().filter(|result| !result.success) {
            has_errors = true;
            stream.tool_error(&result.tool_name, &result.output);
        }
        has_errors
    }

    pub(super) fn publish_tool_result(&mut self, result: &ToolResult) {
        if result.success && result.tool_name == NOTIFY_TOOL_NAME {
            self.notify_called_this_cycle = true;
        }
        let Some(bus) = self.public_event_bus() else {
            return;
        };
        let _ = bus.publish(InternalMessage::ToolResult {
            call_id: result.tool_call_id.clone(),
            name: result.tool_name.clone(),
            success: result.success,
            content: result.output.clone(),
        });
    }

    pub(super) fn record_tool_execution_cost(&mut self, tool_count: usize) {
        self.budget.record(&ActionCost {
            llm_calls: 0,
            tool_invocations: tool_count as u32,
            tokens: 0,
            cost_cents: tool_count as u64,
        });
    }

    pub(super) fn record_successful_tool_classifications(
        &self,
        state: &mut ToolRoundState,
        calls: &[ToolCall],
        results: &[ToolResult],
    ) {
        for result in results.iter().filter(|result| result.success) {
            let classification = calls
                .iter()
                .find(|call| call.id == result.tool_call_id)
                .map(|call| self.tool_executor.classify_call(call))
                .unwrap_or_else(|| {
                    classification_for_tool_name(self.tool_executor.as_ref(), result)
                });
            match classification {
                ToolCallClassification::Observation => state.used_observation_tools = true,
                ToolCallClassification::Mutation => state.used_mutation_tools = true,
            }
            if state.used_observation_tools && state.used_mutation_tools {
                break;
            }
        }
    }

    #[cfg(test)]
    pub(super) async fn execute_tool_calls(
        &mut self,
        calls: &[ToolCall],
    ) -> Result<Vec<ToolResult>, LoopError> {
        self.execute_tool_calls_with_stream(calls, CycleStream::disabled())
            .await
    }

    pub(super) async fn execute_tool_calls_with_stream(
        &mut self,
        calls: &[ToolCall],
        stream: CycleStream<'_>,
    ) -> Result<Vec<ToolResult>, LoopError> {
        let prepared = self.prepare_tool_calls_for_execution(calls);
        self.emit_blocked_tool_errors(&prepared.blocked, stream);
        let mut results = self
            .execute_allowed_tool_calls(&prepared.allowed, stream)
            .await?;
        self.tool_retry_tracker
            .record_results(&prepared.allowed, &results);
        results.extend(build_blocked_tool_results(&prepared.blocked));
        Ok(reorder_results_by_calls(calls, results))
    }

    fn prepare_tool_calls_for_execution(&self, calls: &[ToolCall]) -> PreparedToolCalls {
        let retry_policy = self.budget.config().retry_policy();
        let (allowed, blocked) =
            partition_by_retry_policy(calls, &self.tool_retry_tracker, &retry_policy);
        let prepared = PreparedToolCalls::new(allowed, blocked);
        let prepared = self.filter_calls_by_profile_tool_names(prepared);
        let prepared = self.filter_calls_by_bounded_local_semantics(prepared);
        self.filter_calls_by_observation_controls(prepared)
    }

    fn filter_calls_by_profile_tool_names(&self, prepared: PreparedToolCalls) -> PreparedToolCalls {
        let (Some(allowed_names), Some(reason)) = (
            self.turn_execution_profile_tool_names(),
            self.turn_execution_profile_block_reason(),
        ) else {
            return prepared;
        };
        let (allowed, blocked) =
            partition_by_allowed_tool_names(&prepared.allowed, &allowed_names, reason);
        prepared.filtered(allowed, blocked)
    }

    fn filter_calls_by_bounded_local_semantics(
        &self,
        prepared: PreparedToolCalls,
    ) -> PreparedToolCalls {
        if !matches!(
            &self.turn_execution_profile,
            TurnExecutionProfile::BoundedLocal
        ) {
            return prepared;
        }
        let artifact_target = self
            .pending_artifact_write_target
            .as_deref()
            .or(self.requested_artifact_target.as_deref());
        let (allowed, blocked) = partition_by_bounded_local_phase_semantics(
            &prepared.allowed,
            self.bounded_local_phase,
            artifact_target,
        );
        prepared.filtered(allowed, blocked)
    }

    fn filter_calls_by_observation_controls(
        &self,
        prepared: PreparedToolCalls,
    ) -> PreparedToolCalls {
        if !self
            .turn_execution_profile
            .uses_standard_observation_controls()
            || !self.observation_only_call_restriction_active()
        {
            return prepared;
        }
        let (allowed, blocked) = partition_by_call_classification(
            &prepared.allowed,
            self.tool_executor.as_ref(),
            ToolCallClassification::Mutation,
            OBSERVATION_ONLY_CALL_BLOCK_REASON,
        );
        prepared.filtered(allowed, blocked)
    }

    pub(super) fn emit_blocked_tool_errors(
        &mut self,
        blocked: &[BlockedToolCall],
        stream: CycleStream<'_>,
    ) {
        for blocked_call in blocked {
            let call = &blocked_call.call;
            let signature_failures = self.tool_retry_tracker.consecutive_failures_for(call);
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Blocked,
                format!("tool '{}' blocked: {}", call.name, blocked_call.reason),
                serde_json::json!({
                    "tool": call.name,
                    "reason": blocked_call.reason,
                    "signature_failures": signature_failures,
                    "cycle_total_failures": self.tool_retry_tracker.cycle_total_failures(),
                }),
            );
            stream.emit_error(
                ErrorCategory::ToolExecution,
                blocked_tool_message(&call.name, &blocked_call.reason),
                true,
            );
        }
    }

    pub(super) async fn execute_allowed_tool_calls(
        &mut self,
        allowed: &[ToolCall],
        stream: CycleStream<'_>,
    ) -> Result<Vec<ToolResult>, LoopError> {
        if allowed.is_empty() {
            return Ok(Vec::new());
        }

        let mut malformed_results = Vec::new();
        let valid = collect_valid_tool_calls(allowed, &mut malformed_results);
        let max_bytes = self.budget.config().max_tool_result_bytes;
        let executed = self
            .tool_executor
            .execute_tools(&valid, self.cancel_token.as_ref())
            .await
            .map_err(|error| {
                stream.emit_error(
                    ErrorCategory::ToolExecution,
                    tool_execution_failure_message(allowed, &error.message),
                    error.recoverable,
                );
                loop_error(
                    "act",
                    &format!("tool execution failed: {}", error.message),
                    error.recoverable,
                )
            })?;
        let mut results = truncate_tool_results(executed, max_bytes);
        results.append(&mut malformed_results);
        Ok(results)
    }
}

fn classification_for_tool_name(
    executor: &dyn ToolExecutor,
    result: &ToolResult,
) -> ToolCallClassification {
    match executor.cacheability(&result.tool_name) {
        ToolCacheability::SideEffect => ToolCallClassification::Mutation,
        ToolCacheability::Cacheable | ToolCacheability::NeverCache => {
            ToolCallClassification::Observation
        }
    }
}

fn collect_valid_tool_calls(
    allowed: &[ToolCall],
    malformed_results: &mut Vec<ToolResult>,
) -> Vec<ToolCall> {
    allowed
        .iter()
        .filter_map(|call| {
            if call.arguments.get("__fawx_raw_args").is_some() {
                tracing::warn!(
                    tool = %call.name,
                    "skipping tool call with malformed arguments"
                );
                malformed_results.push(ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success: false,
                    output: "Tool call failed: arguments could not be parsed as valid JSON".into(),
                });
                None
            } else {
                Some(call.clone())
            }
        })
        .collect()
}

pub(super) fn partition_by_call_classification(
    calls: &[ToolCall],
    executor: &dyn ToolExecutor,
    required: ToolCallClassification,
    reason: &str,
) -> (Vec<ToolCall>, Vec<BlockedToolCall>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    for call in calls {
        if executor.classify_call(call) == required {
            allowed.push(call.clone());
        } else {
            blocked.push(BlockedToolCall {
                call: call.clone(),
                reason: reason.to_string(),
            });
        }
    }
    (allowed, blocked)
}

pub(super) fn partition_by_allowed_tool_names(
    calls: &[ToolCall],
    allowed_names: &[String],
    reason: &str,
) -> (Vec<ToolCall>, Vec<BlockedToolCall>) {
    let allowed_names: HashSet<&str> = allowed_names.iter().map(String::as_str).collect();
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    for call in calls {
        if allowed_names.contains(call.name.as_str()) {
            allowed.push(call.clone());
        } else {
            blocked.push(BlockedToolCall {
                call: call.clone(),
                reason: reason.to_string(),
            });
        }
    }
    (allowed, blocked)
}

pub(super) fn build_uniform_blocked_calls(
    calls: &[ToolCall],
    reason: &str,
) -> Vec<BlockedToolCall> {
    calls
        .iter()
        .cloned()
        .map(|call| BlockedToolCall {
            call,
            reason: reason.to_string(),
        })
        .collect()
}

pub(super) fn blocked_tool_message(tool_name: &str, reason: &str) -> String {
    format!(
        "Tool '{}' blocked: {}. Try a different approach.",
        tool_name, reason
    )
}

fn tool_execution_failure_message(calls: &[ToolCall], error_message: &str) -> String {
    match calls {
        [call] => format!("Tool '{}' failed: {error_message}", call.name),
        _ => {
            let names = calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("Tool batch failed for [{names}]: {error_message}")
        }
    }
}

pub(super) fn build_blocked_tool_results(blocked: &[BlockedToolCall]) -> Vec<ToolResult> {
    blocked
        .iter()
        .map(|blocked_call| ToolResult {
            tool_call_id: blocked_call.call.id.clone(),
            tool_name: blocked_call.call.name.clone(),
            success: false,
            output: blocked_tool_message(&blocked_call.call.name, &blocked_call.reason),
        })
        .collect()
}

pub(super) fn reorder_results_by_calls(
    calls: &[ToolCall],
    results: Vec<ToolResult>,
) -> Vec<ToolResult> {
    if results.len() <= 1 {
        return results;
    }
    let mut by_id: HashMap<String, ToolResult> = HashMap::with_capacity(results.len());
    for result in results {
        by_id.insert(result.tool_call_id.clone(), result);
    }
    let mut ordered = Vec::with_capacity(calls.len());
    for call in calls {
        if let Some(result) = by_id.remove(&call.id) {
            ordered.push(result);
        }
    }
    ordered.extend(by_id.into_values());
    ordered
}

pub(super) fn truncate_tool_results(results: Vec<ToolResult>, max_bytes: usize) -> Vec<ToolResult> {
    results
        .into_iter()
        .map(|mut result| {
            if result.output.len() > max_bytes {
                result.output = truncate_tool_result(&result.output, max_bytes).into_owned();
            }
            result
        })
        .collect()
}

#[cfg(test)]
pub(super) fn append_tool_round_messages(
    context_messages: &mut Vec<Message>,
    calls: &[ToolCall],
    provider_item_ids: &HashMap<String, String>,
    results: &[ToolResult],
) -> Result<(), LoopError> {
    let (assistant_message, result_message) =
        build_tool_round_messages(calls, provider_item_ids, results)?;
    context_messages.push(assistant_message);
    context_messages.push(result_message);
    Ok(())
}

fn build_tool_round_messages(
    calls: &[ToolCall],
    provider_item_ids: &HashMap<String, String>,
    results: &[ToolResult],
) -> Result<(Message, Message), LoopError> {
    let assistant_message = build_tool_use_assistant_message(calls, provider_item_ids);
    let result_message = build_tool_result_message(calls, results)?;
    Ok((assistant_message, result_message))
}

pub(super) fn record_tool_round_messages(
    continuation_messages: &mut Vec<Message>,
    evidence_messages: &mut Vec<Message>,
    calls: &[ToolCall],
    provider_item_ids: &HashMap<String, String>,
    results: &[ToolResult],
) -> Result<(), LoopError> {
    let (assistant_message, result_message) =
        build_tool_round_messages(calls, provider_item_ids, results)?;
    continuation_messages.push(assistant_message.clone());
    continuation_messages.push(result_message.clone());
    evidence_messages.push(assistant_message);
    evidence_messages.push(result_message);
    Ok(())
}

pub(super) fn build_tool_use_assistant_message(
    calls: &[ToolCall],
    provider_item_ids: &HashMap<String, String>,
) -> Message {
    let content = calls
        .iter()
        .map(|call| ContentBlock::ToolUse {
            id: call.id.clone(),
            provider_id: provider_item_ids.get(&call.id).cloned(),
            name: call.name.clone(),
            input: call.arguments.clone(),
        })
        .collect();
    Message {
        role: MessageRole::Assistant,
        content,
    }
}

pub(super) fn extract_tool_use_provider_ids(content: &[ContentBlock]) -> HashMap<String, String> {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse {
                id,
                provider_id: Some(provider_id),
                ..
            } if !id.trim().is_empty() && !provider_id.trim().is_empty() => {
                Some((id.clone(), provider_id.clone()))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn build_tool_result_message(
    calls: &[ToolCall],
    results: &[ToolResult],
) -> Result<Message, LoopError> {
    let call_order = calls
        .iter()
        .enumerate()
        .map(|(index, call)| (call.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut ordered_results = indexed_tool_results(&call_order, results)?;
    ordered_results.sort_by_key(|(index, _)| *index);
    let content = ordered_results
        .into_iter()
        .map(|(_, result)| ContentBlock::ToolResult {
            tool_use_id: result.tool_call_id.clone(),
            content: result_block_content(result),
        })
        .collect();
    Ok(Message {
        role: MessageRole::Tool,
        content,
    })
}

fn indexed_tool_results<'a>(
    call_order: &HashMap<String, usize>,
    results: &'a [ToolResult],
) -> Result<Vec<(usize, &'a ToolResult)>, LoopError> {
    results
        .iter()
        .map(|result| {
            call_order
                .get(&result.tool_call_id)
                .copied()
                .map(|index| (index, result))
                .ok_or_else(|| unmatched_tool_call_id_error(result))
        })
        .collect()
}

fn result_block_content(result: &ToolResult) -> serde_json::Value {
    if result.success {
        serde_json::Value::String(result.output.clone())
    } else {
        serde_json::Value::String(format!("[ERROR] {}", result.output))
    }
}

fn unmatched_tool_call_id_error(result: &ToolResult) -> LoopError {
    loop_error(
        "act",
        &format!(
            "tool result has unmatched tool_call_id '{}' for tool '{}'",
            result.tool_call_id, result.tool_name
        ),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{BudgetConfig, BudgetTracker};
    use crate::cancellation::CancellationToken;
    use crate::context_manager::ContextCompactor;
    use async_trait::async_trait;
    use fx_llm::ToolDefinition;
    use std::sync::Arc;

    #[derive(Debug)]
    struct DualToolExecutor;

    #[async_trait]
    impl ToolExecutor for DualToolExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success: true,
                    output: format!("ok: {}", call.name),
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![tool_definition("read_file"), tool_definition("write_file")]
        }

        fn cacheability(&self, tool_name: &str) -> ToolCacheability {
            match tool_name {
                "write_file" => ToolCacheability::SideEffect,
                _ => ToolCacheability::Cacheable,
            }
        }
    }

    fn tool_definition(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("{name} tool"),
            parameters: serde_json::json!({"type":"object"}),
        }
    }

    fn tool_execution_engine(executor: Arc<dyn ToolExecutor>) -> LoopEngine {
        LoopEngine::builder()
            .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
            .context(ContextCompactor::new(2048, 256))
            .max_iterations(3)
            .tool_executor(executor)
            .synthesis_instruction("Summarize".to_string())
            .build()
            .expect("build engine")
    }

    #[tokio::test]
    async fn execute_tool_calls_preserves_original_order_with_blocked_results() {
        let mut engine = tool_execution_engine(Arc::new(DualToolExecutor));
        engine.budget = BudgetTracker::new(
            BudgetConfig {
                max_consecutive_failures: 1,
                max_tool_retries: 0,
                ..BudgetConfig::default()
            },
            0,
            0,
        );
        engine.tool_retry_tracker.record_result(
            &ToolCall {
                id: "seed".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            },
            false,
        );
        let calls = vec![
            ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path":"README.md","content":"hi"}),
            },
        ];

        let results = engine
            .execute_tool_calls_with_stream(&calls, CycleStream::disabled())
            .await
            .expect("execute tool calls");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tool_call_id, "call-1");
        assert_eq!(results[1].tool_call_id, "call-2");
        assert!(!results[0].success);
        assert!(results[1].success);
    }

    #[test]
    fn build_tool_round_messages_preserves_provider_ids() {
        let calls = vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }];
        let results = vec![ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            success: true,
            output: "ok".to_string(),
        }];
        let provider_item_ids =
            HashMap::from([(String::from("call-1"), String::from("provider-1"))]);

        let (assistant_message, result_message) =
            build_tool_round_messages(&calls, &provider_item_ids, &results)
                .expect("build tool round messages");

        assert_eq!(result_message.role, MessageRole::Tool);
        match &assistant_message.content[0] {
            ContentBlock::ToolUse { provider_id, .. } => {
                assert_eq!(provider_id.as_deref(), Some("provider-1"));
            }
            other => panic!("expected tool use block, got {other:?}"),
        }
    }
}
