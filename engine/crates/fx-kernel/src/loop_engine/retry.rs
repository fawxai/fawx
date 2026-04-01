use crate::act::ToolResult;
use crate::budget::RetryPolicyConfig;
use fx_llm::ToolCall;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
struct NoProgressState {
    last_result_hash: u64,
    consecutive_same: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RetryTracker {
    signature_failures: HashMap<ToolCallKey, u16>,
    cycle_total_failures: u16,
    no_progress: HashMap<ToolCallKey, NoProgressState>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ToolCallKey {
    tool_name: String,
    args_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RetryVerdict {
    Allow,
    Block { reason: String },
}

#[derive(Debug, Clone)]
pub(super) struct BlockedToolCall {
    pub(super) call: ToolCall,
    pub(super) reason: String,
}

impl RetryTracker {
    fn should_allow(&self, call: &ToolCall, config: &RetryPolicyConfig) -> RetryVerdict {
        if self.cycle_total_failures >= config.max_cycle_failures {
            return RetryVerdict::Block {
                reason: cycle_failure_limit_reason(),
            };
        }

        let failures = self.consecutive_failures_for(call);
        if failures >= config.max_consecutive_failures {
            return RetryVerdict::Block {
                reason: same_call_failure_reason(failures),
            };
        }

        let signature = ToolCallKey::from_call(call);
        if let Some(state) = self.no_progress.get(&signature) {
            if state.consecutive_same >= config.max_no_progress {
                return RetryVerdict::Block {
                    reason: no_progress_reason(&call.name, state.consecutive_same),
                };
            }
        }

        RetryVerdict::Allow
    }

    pub(super) fn record_results(&mut self, calls: &[ToolCall], results: &[ToolResult]) {
        let result_map: HashMap<&str, &ToolResult> = results
            .iter()
            .map(|result| (result.tool_call_id.as_str(), result))
            .collect();
        for call in calls {
            if let Some(result) = result_map.get(call.id.as_str()) {
                self.record_result(call, result.success);
                if result.success {
                    self.record_progress(call, &result.output);
                }
            }
        }
    }

    fn record_progress(&mut self, call: &ToolCall, output: &str) {
        let signature = ToolCallKey::from_call(call);
        let result_hash = hash_string(output);
        let entry = self
            .no_progress
            .entry(signature)
            .or_insert(NoProgressState {
                last_result_hash: result_hash,
                consecutive_same: 0,
            });
        if entry.last_result_hash == result_hash {
            entry.consecutive_same = entry.consecutive_same.saturating_add(1);
        } else {
            entry.last_result_hash = result_hash;
            entry.consecutive_same = 1;
        }
    }

    pub(super) fn record_result(&mut self, call: &ToolCall, success: bool) {
        let signature = ToolCallKey::from_call(call);
        if success {
            self.signature_failures.insert(signature, 0);
            return;
        }

        let failures = self.signature_failures.entry(signature).or_insert(0);
        *failures = failures.saturating_add(1);
        self.cycle_total_failures = self.cycle_total_failures.saturating_add(1);
    }

    pub(super) fn consecutive_failures_for(&self, call: &ToolCall) -> u16 {
        self.signature_failures
            .get(&ToolCallKey::from_call(call))
            .copied()
            .unwrap_or(0)
    }

    pub(super) fn cycle_total_failures(&self) -> u16 {
        self.cycle_total_failures
    }

    pub(super) fn clear(&mut self) {
        self.signature_failures.clear();
        self.cycle_total_failures = 0;
        self.no_progress.clear();
    }
}

impl ToolCallKey {
    fn from_call(call: &ToolCall) -> Self {
        Self {
            tool_name: call.name.clone(),
            args_hash: hash_tool_arguments(&call.arguments),
        }
    }
}

pub(super) fn partition_by_retry_policy(
    calls: &[ToolCall],
    tracker: &RetryTracker,
    config: &RetryPolicyConfig,
) -> (Vec<ToolCall>, Vec<BlockedToolCall>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    for call in calls {
        match tracker.should_allow(call, config) {
            RetryVerdict::Allow => allowed.push(call.clone()),
            RetryVerdict::Block { reason } => blocked.push(BlockedToolCall {
                call: call.clone(),
                reason,
            }),
        }
    }
    (allowed, blocked)
}

pub(super) fn same_call_failure_reason(failures: u16) -> String {
    format!("same call failed {failures} times consecutively")
}

fn cycle_failure_limit_reason() -> String {
    "too many total failures this cycle".to_string()
}

fn no_progress_reason(tool_name: &str, count: u16) -> String {
    format!(
        "tool '{}' returned the same result {} times with identical arguments \
         — no progress detected",
        tool_name, count
    )
}

fn hash_tool_arguments(arguments: &serde_json::Value) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let canonical = serde_json::to_string(arguments).unwrap_or_default();
    canonical.hash(&mut hasher);
    hasher.finish()
}

fn hash_string(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::super::{blocked_tool_message, CycleStream, LlmProvider, LoopEngine};
    use super::*;
    use crate::act::{ToolExecutor, ToolExecutorError};
    use crate::budget::{BudgetConfig, BudgetState, BudgetTracker};
    use crate::cancellation::CancellationToken;
    use crate::context_manager::ContextCompactor;
    use crate::decide::Decision;
    use async_trait::async_trait;
    use fx_llm::{CompletionResponse, Message};
    use std::sync::Arc;

    #[derive(Debug)]
    struct AlwaysSucceedExecutor;

    #[async_trait]
    impl ToolExecutor for AlwaysSucceedExecutor {
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
                    output: format!("ok: {}", call.name),
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
            Vec::new()
        }

        fn clear_cache(&self) {}
    }

    #[derive(Debug)]
    struct AlwaysFailExecutor;

    #[async_trait]
    impl ToolExecutor for AlwaysFailExecutor {
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
                    success: false,
                    output: format!("err: {}", call.name),
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
            Vec::new()
        }

        fn clear_cache(&self) {}
    }

    fn make_call(id: &str, name: &str) -> ToolCall {
        make_call_with_args(id, name, serde_json::json!({}))
    }

    fn make_call_with_args(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments,
        }
    }

    fn retry_config(max_tool_retries: u8) -> BudgetConfig {
        let max_consecutive_failures = u16::from(max_tool_retries).saturating_add(1);
        BudgetConfig {
            max_consecutive_failures,
            max_tool_retries,
            ..BudgetConfig::default()
        }
    }

    fn retry_engine_with_executor(
        config: BudgetConfig,
        executor: Arc<dyn ToolExecutor>,
    ) -> LoopEngine {
        LoopEngine::builder()
            .budget(BudgetTracker::new(config, 0, 0))
            .context(ContextCompactor::new(2048, 256))
            .max_iterations(5)
            .tool_executor(executor)
            .synthesis_instruction("Summarize".to_string())
            .build()
            .expect("build")
    }

    fn retry_engine(max_tool_retries: u8) -> LoopEngine {
        retry_engine_with_executor(
            retry_config(max_tool_retries),
            Arc::new(AlwaysSucceedExecutor),
        )
    }

    fn failure_engine(max_tool_retries: u8) -> LoopEngine {
        retry_engine_with_executor(retry_config(max_tool_retries), Arc::new(AlwaysFailExecutor))
    }

    fn block_message(tool_name: &str, failures: u16) -> String {
        blocked_tool_message(tool_name, &same_call_failure_reason(failures))
    }

    fn block_signature(engine: &mut LoopEngine, call: &ToolCall) {
        let failures = engine
            .budget
            .config()
            .retry_policy()
            .max_consecutive_failures;
        seed_failures(engine, call, failures);
    }

    fn seed_failures(engine: &mut LoopEngine, call: &ToolCall, failures: u16) {
        for _ in 0..failures {
            engine.tool_retry_tracker.record_result(call, false);
        }
    }

    fn is_signature_tracked(engine: &LoopEngine, call: &ToolCall) -> bool {
        engine
            .tool_retry_tracker
            .signature_failures
            .contains_key(&ToolCallKey::from_call(call))
    }

    #[tokio::test]
    async fn successful_calls_keep_failure_counts_at_zero() {
        let mut engine = retry_engine(2);

        for id in 1..=3 {
            let call = make_call(&id.to_string(), "read_file");
            let results = engine
                .execute_tool_calls(std::slice::from_ref(&call))
                .await
                .expect("execute");
            assert!(results[0].success, "call {id} should succeed");
            assert_eq!(engine.tool_retry_tracker.consecutive_failures_for(&call), 0);
        }

        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[tokio::test]
    async fn consecutive_failures_block_specific_signature() {
        let mut engine = failure_engine(2);

        for id in 1..=3 {
            let call = make_call(&id.to_string(), "read_file");
            let results = engine.execute_tool_calls(&[call]).await.expect("execute");
            assert!(
                !results[0].success,
                "call {id} should fail but not be blocked"
            );
            assert!(!results[0].output.contains("blocked"));
        }

        let call = make_call("4", "read_file");
        let results = engine
            .execute_tool_calls(std::slice::from_ref(&call))
            .await
            .expect("execute blocked call");
        assert!(!results[0].success);
        assert_eq!(results[0].output, block_message("read_file", 3));
        assert_eq!(engine.tool_retry_tracker.consecutive_failures_for(&call), 3);
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 3);
    }

    #[tokio::test]
    async fn blocked_result_contains_tool_name_and_failure_reason() {
        let mut engine = retry_engine(2);
        let call = make_call("blocked", "network_fetch");
        block_signature(&mut engine, &call);

        let results = engine
            .execute_tool_calls(&[call])
            .await
            .expect("execute blocked call");
        let reason = same_call_failure_reason(3);
        assert!(!results[0].success);
        assert!(results[0].output.contains("network_fetch"));
        assert!(results[0].output.contains(&reason));
    }

    #[tokio::test]
    async fn blocked_tool_emits_blocked_signal() {
        let mut engine = retry_engine(2);
        let call = make_call("4", "read_file");
        block_signature(&mut engine, &call);

        engine
            .execute_tool_calls(&[call])
            .await
            .expect("execute blocked call");

        let signals = engine.signals.drain_all();
        let blocked_signals: Vec<_> = signals
            .iter()
            .filter(|signal| signal.kind == crate::signals::SignalKind::Blocked)
            .collect();
        let reason = same_call_failure_reason(3);

        assert_eq!(blocked_signals.len(), 1);
        assert_eq!(
            blocked_signals[0].metadata["tool"],
            serde_json::json!("read_file")
        );
        assert_eq!(
            blocked_signals[0].metadata["reason"],
            serde_json::json!(reason)
        );
        assert_eq!(
            blocked_signals[0].metadata["signature_failures"],
            serde_json::json!(3)
        );
        assert_eq!(
            blocked_signals[0].metadata["cycle_total_failures"],
            serde_json::json!(3)
        );
    }

    #[tokio::test]
    async fn blocked_stays_blocked_within_cycle() {
        let mut engine = retry_engine(2);
        let call = make_call("seed", "read_file");
        block_signature(&mut engine, &call);

        for id in 4..=6 {
            let blocked_call = make_call(&id.to_string(), "read_file");
            let results = engine
                .execute_tool_calls(&[blocked_call])
                .await
                .expect("execute blocked call");
            assert_eq!(results[0].output, block_message("read_file", 3));
        }
    }

    #[tokio::test]
    async fn mixed_batch_blocked_and_fresh() {
        let mut engine = retry_engine(2);
        let blocked_call = make_call("blocked", "read_file");
        block_signature(&mut engine, &blocked_call);

        let calls = vec![
            blocked_call,
            make_call("fresh-1", "write_file"),
            make_call("fresh-2", "list_dir"),
        ];
        let results = engine.execute_tool_calls(&calls).await.expect("execute");

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].output, block_message("read_file", 3));
        assert!(results[1].success);
        assert!(results[2].success);
    }

    #[tokio::test]
    async fn prepare_cycle_allows_previously_blocked_signature() {
        let mut engine = retry_engine(2);
        let call = make_call("blocked", "read_file");
        block_signature(&mut engine, &call);

        let blocked = engine
            .execute_tool_calls(std::slice::from_ref(&call))
            .await
            .expect("execute blocked call");
        assert_eq!(blocked[0].output, block_message("read_file", 3));

        engine.prepare_cycle();

        let results = engine
            .execute_tool_calls(std::slice::from_ref(&call))
            .await
            .expect("execute");
        assert!(results[0].success);
        assert_eq!(engine.tool_retry_tracker.consecutive_failures_for(&call), 0);
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[tokio::test]
    async fn prepare_cycle_clears_retry_tracker() {
        let mut engine = retry_engine(2);
        let call = make_call("1", "read_file");
        seed_failures(&mut engine, &call, 1);

        assert!(!engine.tool_retry_tracker.signature_failures.is_empty());
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 1);

        engine.prepare_cycle();

        assert!(engine.tool_retry_tracker.signature_failures.is_empty());
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[test]
    fn success_resets_failure_count() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 2,
            max_cycle_failures: 10,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        tracker.record_result(&call, false);
        assert_eq!(tracker.consecutive_failures_for(&call), 1);

        tracker.record_result(&call, true);
        assert_eq!(tracker.consecutive_failures_for(&call), 0);

        tracker.record_result(&call, false);
        assert_eq!(tracker.consecutive_failures_for(&call), 1);
        assert_eq!(tracker.cycle_total_failures, 2);
        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn different_args_tracked_independently() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 2,
            max_cycle_failures: 10,
            ..RetryPolicyConfig::default()
        };
        let call_a = make_call_with_args("1", "read_file", serde_json::json!({"path": "a"}));
        let call_b = make_call_with_args("2", "read_file", serde_json::json!({"path": "b"}));
        let mut tracker = RetryTracker::default();

        tracker.record_result(&call_a, false);
        tracker.record_result(&call_a, false);

        assert_eq!(tracker.consecutive_failures_for(&call_a), 2);
        assert_eq!(tracker.consecutive_failures_for(&call_b), 0);
        assert!(matches!(
            tracker.should_allow(&call_a, &config),
            RetryVerdict::Block { ref reason } if reason == &same_call_failure_reason(2)
        ));
        assert!(matches!(
            tracker.should_allow(&call_b, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn circuit_breaker_blocks_all_tools() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 10,
            max_cycle_failures: 2,
            ..RetryPolicyConfig::default()
        };
        let mut tracker = RetryTracker::default();
        let call_a = make_call_with_args("1", "read_file", serde_json::json!({"path": "a"}));
        let call_b = make_call_with_args("2", "read_file", serde_json::json!({"path": "b"}));
        let fresh_call = make_call("3", "write_file");

        tracker.record_result(&call_a, false);
        tracker.record_result(&call_b, false);

        assert_eq!(tracker.cycle_total_failures, 2);
        assert!(matches!(
            tracker.should_allow(&fresh_call, &config),
            RetryVerdict::Block { ref reason } if reason == &cycle_failure_limit_reason()
        ));
    }

    #[test]
    fn no_progress_blocks_after_threshold() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        for _ in 0..3 {
            tracker.record_progress(&call, "same output");
        }

        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Block { ref reason } if reason.contains("no progress detected")
        ));
    }

    #[test]
    fn no_progress_resets_on_different_output() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        tracker.record_progress(&call, "output A");
        tracker.record_progress(&call, "output A");
        tracker.record_progress(&call, "output B");

        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn no_progress_independent_per_signature() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call_a = make_call_with_args("1", "read_file", serde_json::json!({"path": "a"}));
        let call_b = make_call_with_args("2", "read_file", serde_json::json!({"path": "b"}));
        let mut tracker = RetryTracker::default();

        for _ in 0..3 {
            tracker.record_progress(&call_a, "same output");
        }

        assert!(matches!(
            tracker.should_allow(&call_a, &config),
            RetryVerdict::Block { .. }
        ));
        assert!(matches!(
            tracker.should_allow(&call_b, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn no_progress_does_not_affect_failures() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 5,
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        tracker.record_result(&call, false);
        tracker.record_result(&call, false);
        assert_eq!(tracker.consecutive_failures_for(&call), 2);

        tracker.record_progress(&call, "same output");
        tracker.record_progress(&call, "same output");
        assert_eq!(tracker.consecutive_failures_for(&call), 2);

        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn clear_resets_no_progress() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        for _ in 0..3 {
            tracker.record_progress(&call, "same output");
        }
        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Block { .. }
        ));

        tracker.clear();
        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
        assert!(tracker.no_progress.is_empty());
    }

    #[test]
    fn backward_compat_max_tool_retries() {
        let mut value = serde_json::to_value(BudgetConfig::default()).expect("serialize");
        value["max_tool_retries"] = serde_json::json!(0);

        let config: BudgetConfig = serde_json::from_value(value).expect("deserialize");
        assert_eq!(config.max_tool_retries, 0);
        assert_eq!(config.max_consecutive_failures, 1);
        assert_eq!(config.retry_policy().max_consecutive_failures, 1);
    }

    #[tokio::test]
    async fn zero_retries_blocks_after_one_failure() {
        let mut engine = retry_engine(0);
        let call = make_call("1", "read_file");
        seed_failures(&mut engine, &call, 1);

        let results = engine
            .execute_tool_calls(&[call])
            .await
            .expect("execute blocked call");
        assert_eq!(results[0].output, block_message("read_file", 1));
    }

    #[tokio::test]
    async fn max_retries_effectively_unlimited() {
        let config = BudgetConfig {
            max_consecutive_failures: u16::from(u8::MAX).saturating_add(1),
            max_cycle_failures: u16::MAX,
            max_tool_retries: u8::MAX,
            ..BudgetConfig::default()
        };
        let mut engine = retry_engine_with_executor(config, Arc::new(AlwaysFailExecutor));

        for id in 1..=255_u16 {
            let call = make_call(&id.to_string(), "read_file");
            let results = engine.execute_tool_calls(&[call]).await.expect("execute");
            assert!(!results[0].success, "call {id} should not be blocked");
            assert!(!results[0].output.contains("blocked"));
        }

        let call = make_call("255", "read_file");
        assert_eq!(
            engine.tool_retry_tracker.consecutive_failures_for(&call),
            255
        );
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 255);
    }

    #[tokio::test]
    async fn deferred_tools_do_not_count_toward_failures() {
        let config = BudgetConfig {
            max_fan_out: 2,
            max_consecutive_failures: 3,
            max_tool_retries: 2,
            ..BudgetConfig::default()
        };
        let mut engine = retry_engine_with_executor(config, Arc::new(AlwaysSucceedExecutor));
        let calls = vec![
            make_call("1", "tool_a"),
            make_call("2", "tool_b"),
            make_call("3", "tool_c"),
            make_call("4", "tool_d"),
        ];

        let (execute, deferred) = engine.apply_fan_out_cap(&calls);
        let results = engine.execute_tool_calls(&execute).await.expect("execute");

        assert_eq!(results.len(), 2);
        assert!(is_signature_tracked(&engine, &calls[0]));
        assert!(is_signature_tracked(&engine, &calls[1]));
        assert!(!is_signature_tracked(&engine, &deferred[0]));
        assert!(!is_signature_tracked(&engine, &deferred[1]));
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[tokio::test]
    async fn deferred_tools_start_fresh_when_executed() {
        let config = BudgetConfig {
            max_fan_out: 1,
            max_consecutive_failures: 3,
            max_tool_retries: 2,
            ..BudgetConfig::default()
        };
        let mut engine = retry_engine_with_executor(config, Arc::new(AlwaysSucceedExecutor));
        let tool_a = make_call("1", "tool_a");
        let tool_b = make_call("2", "tool_b");

        let (execute, _) = engine.apply_fan_out_cap(&[tool_a.clone(), tool_b.clone()]);
        engine.execute_tool_calls(&execute).await.expect("execute");
        assert!(is_signature_tracked(&engine, &tool_a));
        assert!(!is_signature_tracked(&engine, &tool_b));

        let results = engine
            .execute_tool_calls(std::slice::from_ref(&tool_b))
            .await
            .expect("execute deferred tool");
        assert!(results[0].success);
        assert!(is_signature_tracked(&engine, &tool_b));
        assert_eq!(
            engine.tool_retry_tracker.consecutive_failures_for(&tool_b),
            0
        );
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[tokio::test]
    async fn budget_low_takes_precedence_over_retry_cap() {
        use crate::budget::ActionCost;
        use fx_core::error::LlmError as CoreLlmError;
        use fx_llm::{CompletionRequest, ProviderError};
        use std::collections::VecDeque;
        use std::sync::Mutex;

        #[derive(Debug)]
        struct MockLlm {
            responses: Mutex<VecDeque<CompletionResponse>>,
        }

        impl MockLlm {
            fn new(responses: Vec<CompletionResponse>) -> Self {
                Self {
                    responses: Mutex::new(VecDeque::from(responses)),
                }
            }
        }

        #[async_trait]
        impl LlmProvider for MockLlm {
            async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
                Ok("summary".to_string())
            }

            async fn generate_streaming(
                &self,
                _: &str,
                _: u32,
                callback: Box<dyn Fn(String) + Send + 'static>,
            ) -> Result<String, CoreLlmError> {
                callback("summary".to_string());
                Ok("summary".to_string())
            }

            fn model_name(&self) -> &str {
                "mock-budget-test"
            }

            async fn complete(
                &self,
                _: CompletionRequest,
            ) -> Result<CompletionResponse, ProviderError> {
                self.responses
                    .lock()
                    .expect("lock")
                    .pop_front()
                    .ok_or_else(|| ProviderError::Provider("no response".to_string()))
            }
        }

        let config = BudgetConfig {
            max_cost_cents: 100,
            max_consecutive_failures: 3,
            max_tool_retries: 2,
            ..BudgetConfig::default()
        };
        let mut engine = retry_engine_with_executor(config, Arc::new(AlwaysSucceedExecutor));
        let blocked_call = make_call("blocked", "read_file");
        block_signature(&mut engine, &blocked_call);
        engine.signals.drain_all();

        engine.budget.record(&ActionCost {
            cost_cents: 81,
            ..ActionCost::default()
        });
        assert_eq!(engine.budget.state(), BudgetState::Low);

        let decision = Decision::UseTools(vec![make_call("5", "read_file")]);
        let tool_calls = match &decision {
            Decision::UseTools(calls) => calls.as_slice(),
            _ => unreachable!(),
        };
        let llm = MockLlm::new(Vec::new());
        let context_messages = vec![Message::user("do something")];

        let action = engine
            .act_with_tools(
                &decision,
                tool_calls,
                &llm,
                &context_messages,
                CycleStream::disabled(),
            )
            .await
            .expect("act_with_tools should succeed with budget-low path");

        assert!(action.tool_results.is_empty());
        assert!(
            action.response_text.contains("budget")
                || action.response_text.contains("soft-ceiling")
        );

        let signals = engine.signals.drain_all();
        let blocked_signals: Vec<_> = signals
            .iter()
            .filter(|signal| signal.kind == crate::signals::SignalKind::Blocked)
            .collect();
        assert!(!blocked_signals.is_empty());
        assert_eq!(
            blocked_signals[0].metadata["reason"],
            serde_json::json!("budget_soft_ceiling")
        );
    }

    #[test]
    fn record_results_tracks_no_progress_end_to_end() {
        let config = RetryPolicyConfig::default();
        let mut tracker = RetryTracker::default();

        let calls = vec![make_call("c1", "read_file"), make_call("c2", "write_file")];
        let results = vec![
            ToolResult {
                tool_call_id: "c1".to_string(),
                tool_name: "read_file".to_string(),
                success: true,
                output: "same output".to_string(),
            },
            ToolResult {
                tool_call_id: "c2".to_string(),
                tool_name: "write_file".to_string(),
                success: true,
                output: "ok".to_string(),
            },
        ];

        for _ in 0..3 {
            tracker.record_results(&calls, &results);
        }

        assert!(matches!(
            tracker.should_allow(&calls[0], &config),
            RetryVerdict::Block { ref reason } if reason.contains("no progress detected")
        ));
        assert!(matches!(
            tracker.should_allow(&calls[1], &config),
            RetryVerdict::Block { ref reason } if reason.contains("no progress detected")
        ));
    }

    #[test]
    fn record_results_failures_do_not_trigger_no_progress() {
        let mut tracker = RetryTracker::default();

        let calls = vec![make_call("c1", "read_file")];
        let failure_results = vec![ToolResult {
            tool_call_id: "c1".to_string(),
            tool_name: "read_file".to_string(),
            success: false,
            output: "error: not found".to_string(),
        }];

        for _ in 0..5 {
            tracker.record_results(&calls, &failure_results);
        }

        assert!(tracker.no_progress.is_empty());
        assert_eq!(tracker.consecutive_failures_for(&calls[0]), 5);
    }

    #[test]
    fn record_results_mixed_success_failure_no_progress() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            max_consecutive_failures: 10,
            max_cycle_failures: 20,
        };
        let mut tracker = RetryTracker::default();

        let calls = vec![make_call("c1", "read_file"), make_call("c2", "write_file")];
        let results = vec![
            ToolResult {
                tool_call_id: "c1".to_string(),
                tool_name: "read_file".to_string(),
                success: true,
                output: "same output".to_string(),
            },
            ToolResult {
                tool_call_id: "c2".to_string(),
                tool_name: "write_file".to_string(),
                success: false,
                output: "error: permission denied".to_string(),
            },
        ];

        for _ in 0..3 {
            tracker.record_results(&calls, &results);
        }

        assert!(matches!(
            tracker.should_allow(&calls[0], &config),
            RetryVerdict::Block { ref reason } if reason.contains("no progress detected")
        ));
        assert!(!tracker
            .no_progress
            .contains_key(&ToolCallKey::from_call(&calls[1])));
        assert_eq!(tracker.consecutive_failures_for(&calls[1]), 3);
    }
}
