use super::*;
use crate::act::{ToolExecutor, ToolResult};
use crate::budget::{BudgetConfig, BudgetTracker, DepthMode};
use crate::cancellation::CancellationToken;
use crate::context_manager::ContextCompactor;
use async_trait::async_trait;
use fx_core::error::LlmError as CoreLlmError;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_decompose::{AggregationStrategy, DecompositionPlan, SubGoal};
use fx_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, ProviderError, ToolCall,
    ToolDefinition,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// -- LLM providers --------------------------------------------------------

#[derive(Debug)]
pub(super) struct ScriptedLlm {
    responses: Mutex<VecDeque<Result<CompletionResponse, ProviderError>>>,
}

impl ScriptedLlm {
    pub(super) fn new(responses: Vec<Result<CompletionResponse, ProviderError>>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }

    pub(super) fn ok(responses: Vec<CompletionResponse>) -> Self {
        Self::new(responses.into_iter().map(Ok).collect())
    }
}

/// Mock LLM that records requests and replays scripted responses.
/// Consolidated from context_compaction_tests + test_fixtures to avoid duplication.
#[derive(Debug)]
pub(super) struct RecordingLlm {
    responses: Mutex<VecDeque<Result<CompletionResponse, ProviderError>>>,
    requests: Mutex<Vec<CompletionRequest>>,
    generated_summary: String,
}

impl RecordingLlm {
    pub(super) fn new(responses: Vec<Result<CompletionResponse, ProviderError>>) -> Self {
        Self::with_generated_summary(responses, "summary".to_string())
    }

    pub(super) fn ok(responses: Vec<CompletionResponse>) -> Self {
        Self::new(responses.into_iter().map(Ok).collect())
    }

    pub(super) fn with_generated_summary(
        responses: Vec<Result<CompletionResponse, ProviderError>>,
        generated_summary: String,
    ) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            requests: Mutex::new(Vec::new()),
            generated_summary,
        }
    }

    pub(super) fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().expect("requests lock").clone()
    }
}

#[async_trait]
impl LlmProvider for RecordingLlm {
    async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
        Ok(self.generated_summary.clone())
    }

    async fn generate_streaming(
        &self,
        _: &str,
        _: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, CoreLlmError> {
        callback(self.generated_summary.clone());
        Ok(self.generated_summary.clone())
    }

    fn model_name(&self) -> &str {
        "recording"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        self.requests.lock().expect("requests lock").push(request);
        self.responses
            .lock()
            .expect("response lock")
            .pop_front()
            .unwrap_or_else(|| Ok(text_response("ok")))
    }
}

#[async_trait]
impl LlmProvider for ScriptedLlm {
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
        "scripted"
    }

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        self.responses
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or_else(|| Err(ProviderError::Provider("no scripted response".to_string())))
    }
}

/// LLM that cancels a token after the N-th call to `complete()`.
#[derive(Debug)]
pub(super) struct CancelAfterNthCallLlm {
    cancel_token: CancellationToken,
    cancel_after: usize,
    call_count: AtomicUsize,
    responses: Mutex<VecDeque<Result<CompletionResponse, ProviderError>>>,
}

impl CancelAfterNthCallLlm {
    pub(super) fn new(
        cancel_token: CancellationToken,
        cancel_after: usize,
        responses: Vec<Result<CompletionResponse, ProviderError>>,
    ) -> Self {
        Self {
            cancel_token,
            cancel_after,
            call_count: AtomicUsize::new(0),
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }
}

#[async_trait]
impl LlmProvider for CancelAfterNthCallLlm {
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
        "cancel-after-nth"
    }

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let call_number = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
        if call_number >= self.cancel_after {
            self.cancel_token.cancel();
        }
        self.responses
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or_else(|| Err(ProviderError::Provider("no scripted response".to_string())))
    }
}

// -- Tool executors -------------------------------------------------------

#[derive(Debug, Default)]
pub(super) struct StubToolExecutor;

#[async_trait]
impl ToolExecutor for StubToolExecutor {
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
                output: "ok".to_string(),
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![read_file_def()]
    }
}

/// Tool executor that always fails.
#[derive(Debug, Default)]
pub(super) struct AlwaysFailingToolExecutor;

#[async_trait]
impl ToolExecutor for AlwaysFailingToolExecutor {
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
                success: false,
                output: "tool crashed: segfault".to_string(),
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![read_file_def()]
    }
}

/// Tool executor that sleeps, then checks cancellation.
#[derive(Debug)]
pub(super) struct SlowToolExecutor {
    pub(super) delay: tokio::time::Duration,
    pub(super) executions: Arc<AtomicUsize>,
}

#[async_trait]
impl ToolExecutor for SlowToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        let step = tokio::time::Duration::from_millis(5);
        let mut remaining = self.delay;
        while !remaining.is_zero() {
            if cancel.is_some_and(CancellationToken::is_cancelled) {
                break;
            }
            let sleep_for = remaining.min(step);
            tokio::time::sleep(sleep_for).await;
            remaining = remaining.saturating_sub(sleep_for);
        }
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            return Ok(calls
                .iter()
                .map(|call| ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success: false,
                    output: "cancelled mid-execution".to_string(),
                })
                .collect());
        }
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "slow result".to_string(),
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![read_file_def()]
    }
}

/// Tool executor producing very large outputs to push context past limits.
#[derive(Debug)]
pub(super) struct LargeOutputToolExecutor {
    pub(super) output_size: usize,
}

#[async_trait]
impl ToolExecutor for LargeOutputToolExecutor {
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
                output: "X".repeat(self.output_size),
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![read_file_def()]
    }
}

// -- Factory functions ----------------------------------------------------

pub(super) fn read_file_def() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({"type":"object"}),
    }
}

pub(super) fn read_file_call(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }
}

pub(super) fn text_response(text: &str) -> CompletionResponse {
    CompletionResponse {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }
}

pub(super) fn tool_use_response(calls: Vec<ToolCall>) -> CompletionResponse {
    CompletionResponse {
        content: Vec::new(),
        tool_calls: calls,
        usage: None,
        stop_reason: Some("tool_use".to_string()),
    }
}

pub(super) fn test_snapshot(text: &str) -> PerceptionSnapshot {
    PerceptionSnapshot {
        timestamp_ms: 1,
        screen: ScreenState {
            current_app: "terminal".to_string(),
            elements: Vec::new(),
            text_content: text.to_string(),
        },
        notifications: Vec::new(),
        active_app: "terminal".to_string(),
        user_input: Some(UserInput {
            text: text.to_string(),
            source: InputSource::Text,
            timestamp: 1,
            context_id: None,
            images: Vec::new(),
            documents: Vec::new(),
        }),
        sensor_data: None,
        conversation_history: vec![Message::user(text)],
        steer_context: None,
    }
}

pub(super) fn budget_config_with_llm_calls(
    max_llm_calls: u32,
    max_recursion_depth: u32,
) -> BudgetConfig {
    BudgetConfig {
        max_llm_calls,
        max_tool_invocations: 20,
        max_tokens: 100_000,
        max_cost_cents: 500,
        max_wall_time_ms: 60_000,
        max_recursion_depth,
        decompose_depth_mode: DepthMode::Static,
        ..BudgetConfig::default()
    }
}

pub(super) fn build_engine_with_executor(
    executor: Arc<dyn ToolExecutor>,
    config: BudgetConfig,
    depth: u32,
    max_iterations: u32,
) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, current_time_ms(), depth))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(max_iterations)
        .tool_executor(executor)
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

pub(super) fn decomposition_plan(descriptions: &[&str]) -> DecompositionPlan {
    DecompositionPlan {
        sub_goals: descriptions
            .iter()
            .map(|desc| {
                SubGoal::with_definition_of_done(
                    (*desc).to_string(),
                    Vec::new(),
                    Some(&format!("output for {desc}")),
                    None,
                )
            })
            .collect(),
        strategy: AggregationStrategy::Sequential,
        truncated_from: None,
    }
}
