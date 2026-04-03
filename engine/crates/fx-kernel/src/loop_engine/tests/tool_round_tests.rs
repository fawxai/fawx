use super::*;
use crate::budget::{BudgetConfig, BudgetTracker, TerminationConfig};
use crate::cancellation::CancellationToken;
use crate::input::{loop_input_channel, LoopCommand};
use async_trait::async_trait;
use fx_core::error::LlmError as CoreLlmError;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_llm::{CompletionResponse, ContentBlock, Message, ProviderError, ToolCall, ToolDefinition};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Tool executor that tracks how many calls were actually executed
/// and supports cooperative cancellation.
#[derive(Debug)]
struct CountingToolExecutor {
    executed_count: Arc<AtomicU32>,
}

#[async_trait]
impl ToolExecutor for CountingToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        let mut results = Vec::new();
        for call in calls {
            if let Some(token) = cancel {
                if token.is_cancelled() {
                    break;
                }
            }
            self.executed_count.fetch_add(1, Ordering::SeqCst);
            results.push(ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
            });
            // Cancel after first tool call to test partial execution
            if let Some(token) = cancel {
                token.cancel();
            }
        }
        Ok(results)
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        }]
    }
}

#[derive(Debug, Default)]
struct Phase4StubToolExecutor;

#[async_trait]
impl ToolExecutor for Phase4StubToolExecutor {
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
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }
}

#[derive(Debug, Default)]
struct Phase4NoDecomposeExecutor;

#[async_trait]
impl ToolExecutor for Phase4NoDecomposeExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        if let Some(call) = calls.iter().find(|call| call.name == DECOMPOSE_TOOL_NAME) {
            return Err(crate::act::ToolExecutorError {
                message: format!("decompose leaked to tool executor: {}", call.id),
                recoverable: false,
            });
        }

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
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }
}

#[derive(Debug)]
struct Phase4MockLlm {
    responses: Mutex<VecDeque<CompletionResponse>>,
    requests: Mutex<Vec<CompletionRequest>>,
}

impl Phase4MockLlm {
    fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().expect("lock").clone()
    }
}

/// Mock LLM that cancels a token during `complete()` to simulate
/// mid-cycle cancellation (e.g. user pressing Ctrl+C while the LLM
/// is generating a response).
#[derive(Debug)]
struct CancellingMockLlm {
    token: CancellationToken,
    responses: Mutex<VecDeque<CompletionResponse>>,
}

impl CancellingMockLlm {
    fn new(token: CancellationToken, responses: Vec<CompletionResponse>) -> Self {
        Self {
            token,
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }
}

#[async_trait]
impl LlmProvider for CancellingMockLlm {
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
        "mock-cancelling"
    }

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        // Cancel the token mid-cycle (simulates Ctrl+C during LLM call)
        self.token.cancel();
        self.responses
            .lock()
            .expect("lock")
            .pop_front()
            .ok_or_else(|| ProviderError::Provider("no response".to_string()))
    }
}

#[async_trait]
impl LlmProvider for Phase4MockLlm {
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
        "mock"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        self.requests.lock().expect("lock").push(request);
        self.responses
            .lock()
            .expect("lock")
            .pop_front()
            .ok_or_else(|| ProviderError::Provider("no response".to_string()))
    }
}

fn p4_engine() -> LoopEngine {
    p4_engine_with_config(BudgetConfig::default(), 3)
}

fn p4_engine_with_config(config: BudgetConfig, max_iterations: u32) -> LoopEngine {
    p4_engine_with_executor(config, max_iterations, Arc::new(Phase4StubToolExecutor))
}

fn p4_engine_with_executor(
    config: BudgetConfig,
    max_iterations: u32,
    tool_executor: Arc<dyn ToolExecutor>,
) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(max_iterations)
        .tool_executor(tool_executor)
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

fn has_tool_round_progress_nudge(messages: &[Message]) -> bool {
    messages.iter().any(|message| {
        message.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains(TOOL_ROUND_PROGRESS_NUDGE),
            _ => false,
        })
    })
}

fn tool_round_budget_config(nudge_after: u16, strip_after_nudge: u16) -> BudgetConfig {
    BudgetConfig {
        termination: TerminationConfig {
            tool_round_nudge_after: nudge_after,
            tool_round_strip_after_nudge: strip_after_nudge,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    }
}

fn p4_snapshot(text: &str) -> PerceptionSnapshot {
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

fn read_file_call(id: &str, path: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path": path}),
    }
}

fn decompose_call(id: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: DECOMPOSE_TOOL_NAME.to_string(),
        arguments,
    }
}

fn calls_from_decision(decision: &Decision) -> &[ToolCall] {
    match decision {
        Decision::UseTools(calls) => calls.as_slice(),
        _ => panic!("decision should contain tool calls"),
    }
}

fn tool_use_response(calls: Vec<ToolCall>) -> CompletionResponse {
    CompletionResponse {
        content: Vec::new(),
        tool_calls: calls,
        usage: None,
        stop_reason: Some("tool_use".to_string()),
    }
}

fn text_response(text: &str) -> CompletionResponse {
    CompletionResponse {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }
}

fn assert_tool_result_block(block: &ContentBlock, expected_id: &str, expected_content: &str) {
    match block {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
        } => {
            assert_eq!(tool_use_id, expected_id);
            assert_eq!(content.as_str(), Some(expected_content));
        }
        other => panic!("expected ToolResult block, got: {other:?}"),
    }
}

#[tokio::test]
async fn act_with_tools_executes_all_calls_and_returns_completion_text() {
    let mut engine = p4_engine();
    let decision = Decision::UseTools(vec![
        read_file_call("1", "a.txt"),
        read_file_call("2", "b.txt"),
    ]);
    let llm = Phase4MockLlm::new(vec![text_response("combined tool output")]);
    let context_messages = vec![Message::user("read two files")];

    let action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    assert_eq!(action.tool_results.len(), 2);
    assert_eq!(action.tool_results[0].tool_name, "read_file");
    assert_eq!(action.tool_results[1].tool_name, "read_file");
    assert_eq!(action.response_text, "combined tool output");
}

#[tokio::test]
async fn act_with_tools_reprompts_on_follow_up_tool_calls() {
    let mut engine = p4_engine();
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![read_file_call("call-2", "b.txt")]),
        text_response("done after two rounds"),
    ]);
    let context_messages = vec![Message::user("read files")];

    let action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    assert_eq!(action.tool_results.len(), 2);
    assert_eq!(action.tool_results[0].tool_call_id, "call-1");
    assert_eq!(action.tool_results[1].tool_call_id, "call-2");
    assert_eq!(action.response_text, "done after two rounds");
}

#[tokio::test]
async fn act_with_tools_intercepts_follow_up_decompose_before_executor() {
    let mut engine = p4_engine_with_executor(
        BudgetConfig::default(),
        3,
        Arc::new(Phase4NoDecomposeExecutor),
    );
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![decompose_call(
            "decompose-1",
            serde_json::json!({
                "sub_goals": [{
                    "description": "summarize findings",
                }],
                "strategy": "Sequential"
            }),
        )]),
        text_response("spec complete"),
    ]);
    let context_messages = vec![Message::user("read files, then break work down")];

    let action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    assert_eq!(action.tool_results.len(), 1);
    assert_eq!(action.tool_results[0].tool_name, "read_file");
    assert!(action
        .tool_results
        .iter()
        .all(|result| result.tool_name != DECOMPOSE_TOOL_NAME));
    assert!(
        action
            .response_text
            .contains("summarize findings => skipped (below floor)"),
        "{}",
        action.response_text
    );
}

#[tokio::test]
async fn act_with_tools_chains_three_tool_rounds() {
    let mut engine = p4_engine();
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![read_file_call("call-2", "b.txt")]),
        tool_use_response(vec![read_file_call("call-3", "c.txt")]),
        text_response("done after three rounds"),
    ]);
    let context_messages = vec![Message::user("read files")];

    let action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    assert_eq!(action.tool_results.len(), 3);
    assert_eq!(action.tool_results[0].tool_call_id, "call-1");
    assert_eq!(action.tool_results[1].tool_call_id, "call-2");
    assert_eq!(action.tool_results[2].tool_call_id, "call-3");
    assert_eq!(action.response_text, "done after three rounds");
}

#[tokio::test]
async fn act_with_tools_refreshes_provider_ids_between_rounds() {
    let mut engine = p4_engine();
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        CompletionResponse {
            content: vec![ContentBlock::ToolUse {
                id: "call-2".to_string(),
                provider_id: Some("fc-2".to_string()),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "b.txt"}),
            }],
            tool_calls: vec![read_file_call("call-2", "b.txt")],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        },
        text_response("done"),
    ]);
    let context_messages = vec![Message::user("read files")];

    let action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    assert_eq!(action.response_text, "done");

    let requests = llm.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].messages.iter().any(|message| {
            message.role == MessageRole::Assistant
                && message.content.iter().any(|block| {
                    matches!(
                        block,
                        ContentBlock::ToolUse {
                            id,
                            provider_id: Some(provider_id),
                            ..
                        } if id == "call-2" && provider_id == "fc-2"
                    )
                })
        }),
        "second continuation request should preserve provider item ids for the next tool round"
    );
}

#[tokio::test]
async fn act_with_tools_nudges_after_threshold() {
    let config = tool_round_budget_config(1, 10);
    let mut engine = p4_engine_with_config(config, 3);
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![read_file_call("call-2", "b.txt")]),
        text_response("done after nudge"),
    ]);
    let context_messages = vec![Message::user("read files")];

    let _action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    let requests = llm.requests();
    assert_eq!(requests.len(), 2);
    assert!(!has_tool_round_progress_nudge(&requests[0].messages));
    assert!(has_tool_round_progress_nudge(&requests[1].messages));
}

#[tokio::test]
async fn act_with_tools_strips_tools_after_threshold() {
    let config = tool_round_budget_config(1, 1);
    let mut engine = p4_engine_with_config(config, 4);
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![read_file_call("call-2", "b.txt")]),
        tool_use_response(vec![read_file_call("call-3", "c.txt")]),
        text_response("done after strip"),
    ]);
    let context_messages = vec![Message::user("read files")];

    let _action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    let requests = llm.requests();
    assert_eq!(requests.len(), 3);
    assert!(!requests[1].tools.is_empty());
    assert!(requests[2].tools.is_empty());
}

#[tokio::test]
async fn act_with_tools_no_nudge_when_disabled() {
    let config = tool_round_budget_config(0, 2);
    let mut engine = p4_engine_with_config(config, 4);
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![read_file_call("call-2", "b.txt")]),
        tool_use_response(vec![read_file_call("call-3", "c.txt")]),
        text_response("done without nudge"),
    ]);
    let context_messages = vec![Message::user("read files")];

    let _action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    let requests = llm.requests();
    assert!(requests.iter().all(|request| {
        !has_tool_round_progress_nudge(&request.messages) && !request.tools.is_empty()
    }));
}

#[tokio::test]
async fn act_with_tools_aggressive_config() {
    let config = tool_round_budget_config(1, 0);
    let mut engine = p4_engine_with_config(config, 3);
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![read_file_call("call-2", "b.txt")]),
        text_response("done after aggressive strip"),
    ]);
    let context_messages = vec![Message::user("read files")];

    let _action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    let requests = llm.requests();
    assert_eq!(requests.len(), 2);
    assert!(has_tool_round_progress_nudge(&requests[1].messages));
    assert!(requests[1].tools.is_empty());
}

#[tokio::test]
async fn act_with_tools_no_nudge_before_threshold() {
    let config = tool_round_budget_config(2, 2);
    let mut engine = p4_engine_with_config(config, 3);
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![read_file_call("call-2", "b.txt")]),
        text_response("done before threshold"),
    ]);
    let context_messages = vec![Message::user("read files")];

    let _action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    let requests = llm.requests();
    assert_eq!(requests.len(), 2);
    assert!(!has_tool_round_progress_nudge(&requests[1].messages));
}

#[tokio::test]
async fn run_cycle_observation_restriction_finishes_incomplete_without_wrap_up_synth() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            observation_only_round_nudge_after: 1,
            observation_only_round_strip_after_nudge: 1,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = p4_engine_with_config(config, 6);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![read_file_call("call-1", "a.txt")]),
        tool_use_response(vec![read_file_call("call-2", "b.txt")]),
        tool_use_response(vec![read_file_call("call-3", "c.txt")]),
    ]);

    let result = engine
        .run_cycle(p4_snapshot("read files"), &llm)
        .await
        .expect("run_cycle");

    match result {
        LoopResult::Incomplete {
            partial_response,
            reason,
            ..
        } => {
            let partial = partial_response.expect("partial response");
            assert!(partial.contains("completed tool work"), "{partial}");
            assert!(
                reason.contains("read-only inspection is disabled"),
                "{reason}"
            );
        }
        other => panic!("expected incomplete result, got {other:?}"),
    }

    assert_eq!(
        llm.requests().len(),
        3,
        "expected only initial reasoning + two continuation requests"
    );
}

#[tokio::test]
async fn act_with_tools_nudge_fires_exactly_once() {
    // With nudge_after=1 and strip_after=3, the model runs 3 rounds past
    // the nudge threshold. Verify the nudge message appears exactly once
    // (not stacked on every round).
    let config = tool_round_budget_config(1, 3);
    let mut engine = p4_engine_with_config(config, 5);
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![
        tool_use_response(vec![read_file_call("call-2", "b.txt")]),
        tool_use_response(vec![read_file_call("call-3", "c.txt")]),
        tool_use_response(vec![read_file_call("call-4", "d.txt")]),
        text_response("done after strip"),
    ]);
    let context_messages = vec![Message::user("read files")];

    let _action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    let requests = llm.requests();
    // The last request has the full continuation_messages history.
    // Count nudge messages in it — should be exactly 1 (not stacked).
    let last_request = requests.last().expect("should have requests");
    let nudge_count = last_request
        .messages
        .iter()
        .filter(|m| {
            m.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Text { text } if text.contains(TOOL_ROUND_PROGRESS_NUDGE)
                )
            })
        })
        .count();
    assert_eq!(
        nudge_count, 1,
        "nudge should appear exactly once, not stack"
    );
}

#[tokio::test]
async fn act_with_tools_falls_back_to_synthesis_on_max_iterations() {
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            0,
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(1)
        .tool_executor(Arc::new(Phase4StubToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");
    let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
    let llm = Phase4MockLlm::new(vec![tool_use_response(vec![read_file_call(
        "call-2", "b.txt",
    )])]);
    let context_messages = vec![Message::user("read files")];

    let action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    assert_eq!(action.tool_results.len(), 1);
    assert_eq!(action.response_text, "summary");
}

/// Regression test for #1105: budget soft-ceiling must be checked within
/// the tool round loop, not only at act_with_tools entry. When budget
/// crosses 80% mid-loop, the loop breaks and falls through to synthesis
/// instead of continuing to burn through rounds.
#[tokio::test]
async fn act_with_tools_breaks_on_budget_soft_ceiling_mid_loop() {
    let config = crate::budget::BudgetConfig {
        max_cost_cents: 100,
        soft_ceiling_percent: 80,
        ..crate::budget::BudgetConfig::default()
    };
    let mut tracker = BudgetTracker::new(config, 0, 0);
    // Pre-record 76% cost. After round 1 (3 tools + 1 LLM continuation),
    // budget will be 76 + 3 + 2 = 81%, crossing the 80% soft ceiling.
    tracker.record(&ActionCost {
        cost_cents: 76,
        ..ActionCost::default()
    });
    assert_eq!(tracker.state(), BudgetState::Normal);

    let mut engine = LoopEngine::builder()
        .budget(tracker)
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(5)
        .tool_executor(Arc::new(Phase4StubToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");

    let decision = Decision::UseTools(vec![
        read_file_call("call-1", "a.txt"),
        read_file_call("call-2", "b.txt"),
        read_file_call("call-3", "c.txt"),
    ]);
    // LLM would return more tool calls for round 2 — but the budget
    // soft-ceiling should prevent round 2 from executing.
    let llm = Phase4MockLlm::new(vec![tool_use_response(vec![read_file_call(
        "call-4", "d.txt",
    )])]);
    let context_messages = vec![Message::user("read many files")];

    let action = engine
        .act_with_tools(
            &decision,
            calls_from_decision(&decision),
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools should succeed via synthesis fallback");

    // Only round 1's 3 tool results should be present.
    // Round 2 should NOT have executed.
    assert_eq!(action.tool_results.len(), 3, "only round 1 tools executed");
    assert_eq!(action.tool_results[0].tool_call_id, "call-1");
    assert_eq!(action.tool_results[1].tool_call_id, "call-2");
    assert_eq!(action.tool_results[2].tool_call_id, "call-3");
    // Falls through to synthesize_tool_fallback which returns "summary"
    assert_eq!(action.response_text, "summary");
}

#[test]
fn tool_round_outcome_budget_low_remains_debuggable() {
    assert_eq!(format!("{:?}", ToolRoundOutcome::BudgetLow), "BudgetLow");
}

#[tokio::test]
async fn tool_result_has_tool_call_id() {
    let executor = Phase4StubToolExecutor;
    let calls = vec![ToolCall {
        id: "call-42".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path": "README.md"}),
    }];

    let results = executor
        .execute_tools(&calls, None)
        .await
        .expect("execute_tools");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tool_call_id, "call-42");
}

#[test]
fn build_tool_use_assistant_message_creates_correct_blocks() {
    let calls = vec![
        ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.txt"}),
        },
        ToolCall {
            id: "call-2".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": "ls"}),
        },
    ];

    let message = build_tool_use_assistant_message(&calls, &HashMap::new());

    assert_eq!(message.role, fx_llm::MessageRole::Assistant);
    assert_eq!(message.content.len(), 2);
    match &message.content[0] {
        ContentBlock::ToolUse {
            id, name, input, ..
        } => {
            assert_eq!(id, "call-1");
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "a.txt");
        }
        other => panic!("expected ToolUse block, got: {other:?}"),
    }
}

#[test]
fn append_tool_round_messages_appends_assistant_then_tool_messages() {
    let calls = vec![read_file_call("call-1", "a.txt")];
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        success: true,
        output: "ok".to_string(),
    }];
    let mut messages = vec![Message::user("prompt")];

    append_tool_round_messages(&mut messages, &calls, &HashMap::new(), &results)
        .expect("append_tool_round_messages");

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1].role, fx_llm::MessageRole::Assistant);
    assert_eq!(messages[2].role, fx_llm::MessageRole::Tool);
}

#[test]
fn build_tool_result_message_creates_correct_blocks() {
    let calls = vec![
        read_file_call("call-1", "a.txt"),
        ToolCall {
            id: "call-2".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": "ls"}),
        },
    ];
    let results = vec![
        ToolResult {
            tool_call_id: "call-2".to_string(),
            tool_name: "run_command".to_string(),
            success: false,
            output: "permission denied".to_string(),
        },
        ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            success: true,
            output: "ok".to_string(),
        },
    ];

    let message = build_tool_result_message(&calls, &results).expect("build_tool_result_message");

    assert_eq!(message.role, fx_llm::MessageRole::Tool);
    assert_eq!(message.content.len(), 2);
    assert_tool_result_block(&message.content[0], "call-1", "ok");
    assert_tool_result_block(&message.content[1], "call-2", "[ERROR] permission denied");
}

#[test]
fn build_tool_result_message_uses_tool_role() {
    let calls = vec![read_file_call("call-1", "a.txt")];
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        success: true,
        output: "ok".to_string(),
    }];

    let message = build_tool_result_message(&calls, &results).expect("build_tool_result_message");

    assert_eq!(message.role, fx_llm::MessageRole::Tool);
}

#[test]
fn build_tool_result_message_formats_error_with_prefix() {
    let calls = vec![read_file_call("call-1", "a.txt")];
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        success: false,
        output: "permission denied".to_string(),
    }];

    let message = build_tool_result_message(&calls, &results).expect("build_tool_result_message");

    assert_eq!(message.content.len(), 1);
    assert_tool_result_block(&message.content[0], "call-1", "[ERROR] permission denied");
}

#[test]
fn build_tool_result_message_rejects_unmatched_tool_call_id() {
    let calls = vec![read_file_call("call-1", "a.txt")];
    let results = vec![ToolResult {
        tool_call_id: "call-999".to_string(),
        tool_name: "read_file".to_string(),
        success: true,
        output: "ok".to_string(),
    }];

    let error = build_tool_result_message(&calls, &results)
        .expect_err("should reject unmatched tool_call_id");
    assert_eq!(error.stage, "act");
    assert!(
        error.reason.contains("call-999"),
        "error should mention the unmatched id: {}",
        error.reason
    );
}

// P4-1: execute_tools_cancellation_between_calls
#[tokio::test]
async fn execute_tools_cancellation_between_calls() {
    let count = Arc::new(AtomicU32::new(0));
    let executor = CountingToolExecutor {
        executed_count: Arc::clone(&count),
    };
    let token = CancellationToken::new();

    // 3 tool calls — executor cancels after the first
    let calls = vec![
        ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.txt"}),
        },
        ToolCall {
            id: "2".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "b.txt"}),
        },
        ToolCall {
            id: "3".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "c.txt"}),
        },
    ];

    let results = executor
        .execute_tools(&calls, Some(&token))
        .await
        .expect("execute_tools");

    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "only the first call should execute before cancellation"
    );
    assert_eq!(results.len(), 1);
}

// P4-2: loop_command_stop_ends_cycle
#[tokio::test]
async fn loop_command_stop_ends_cycle() {
    let mut engine = p4_engine();
    let (sender, channel) = loop_input_channel();
    engine.set_input_channel(channel);

    // Pre-send Stop before the cycle runs
    sender.send(LoopCommand::Stop).expect("send Stop");

    let llm = Phase4MockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "hello".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let result = engine
        .run_cycle(p4_snapshot("hello"), &llm)
        .await
        .expect("run_cycle");

    assert!(
        matches!(result, LoopResult::UserStopped { .. }),
        "expected LoopResult::UserStopped, got: {result:?}"
    );
}

// P4-3: loop_command_abort_ends_immediately
#[tokio::test]
async fn loop_command_abort_ends_immediately() {
    let mut engine = p4_engine();
    let (sender, channel) = loop_input_channel();
    engine.set_input_channel(channel);

    sender.send(LoopCommand::Abort).expect("send Abort");

    let llm = Phase4MockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "hello".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let result = engine
        .run_cycle(p4_snapshot("hello"), &llm)
        .await
        .expect("run_cycle");

    assert!(
        matches!(result, LoopResult::UserStopped { .. }),
        "expected LoopResult::UserStopped, got: {result:?}"
    );
}

// P4-4: cancellation token stops the cycle (cancelled mid-cycle)
#[tokio::test]
async fn cancel_token_stops_cycle() {
    let mut engine = p4_engine();
    let token = CancellationToken::new();
    engine.set_cancel_token(token.clone());

    // LLM cancels the token during complete() to simulate mid-cycle Ctrl+C
    let llm = CancellingMockLlm::new(
        token,
        vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }],
    );

    let result = engine
        .run_cycle(p4_snapshot("hello"), &llm)
        .await
        .expect("run_cycle");

    assert!(
        matches!(result, LoopResult::UserStopped { .. }),
        "expected LoopResult::UserStopped, got: {result:?}"
    );
}

// P4-5: UserStopped signals are attached
#[tokio::test]
async fn user_stopped_includes_signals() {
    let mut engine = p4_engine();
    let token = CancellationToken::new();
    engine.set_cancel_token(token.clone());

    // LLM cancels mid-cycle to produce a UserStopped
    let llm = CancellingMockLlm::new(
        token,
        vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }],
    );

    let result = engine
        .run_cycle(p4_snapshot("hello"), &llm)
        .await
        .expect("run_cycle");

    match result {
        LoopResult::UserStopped { signals, .. } => {
            assert!(
                signals.iter().any(|s| s.kind == SignalKind::Blocked),
                "UserStopped should include a Blocked signal"
            );
        }
        other => panic!("expected UserStopped, got: {other:?}"),
    }
}

// B1: Integration test — verify cancellation resets between cycles
#[tokio::test]
async fn run_cycle_resets_cancellation_between_cycles() {
    let mut engine = p4_engine();
    let token = CancellationToken::new();
    engine.set_cancel_token(token.clone());

    // First cycle: LLM cancels mid-cycle -> UserStopped
    let llm = CancellingMockLlm::new(
        token.clone(),
        vec![
            // First cycle: LLM response (cancelled during complete())
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "first response".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
        ],
    );

    let result1 = engine
        .run_cycle(p4_snapshot("first"), &llm)
        .await
        .expect("first run_cycle");
    assert!(
        matches!(result1, LoopResult::UserStopped { .. }),
        "first cycle should be UserStopped, got: {result1:?}"
    );

    // Second cycle: prepare_cycle() should have reset the token.
    // Use a normal (non-cancelling) LLM to verify the cycle runs clean.
    let llm2 = Phase4MockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "second cycle response".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let result2 = engine
        .run_cycle(p4_snapshot("second"), &llm2)
        .await
        .expect("second run_cycle");
    assert!(
        matches!(result2, LoopResult::Complete { .. }),
        "second cycle should Complete (token was reset), got: {result2:?}"
    );
}
