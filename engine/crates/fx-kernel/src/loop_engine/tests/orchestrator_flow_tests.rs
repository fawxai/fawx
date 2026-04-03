use super::*;
use async_trait::async_trait;
use fx_core::error::LlmError as CoreLlmError;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_llm::{CompletionResponse, ContentBlock, Message, ProviderError, ToolCall, ToolDefinition};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug, Default)]
struct StubToolExecutor;

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
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }
}

#[derive(Debug, Default)]
struct FailingToolExecutor;

#[async_trait]
impl ToolExecutor for FailingToolExecutor {
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
                output: "path escapes working directory".to_string(),
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
struct CacheAwareToolExecutor {
    clear_calls: Arc<AtomicUsize>,
    stats: crate::act::ToolCacheStats,
}

impl CacheAwareToolExecutor {
    fn new(clear_calls: Arc<AtomicUsize>, stats: crate::act::ToolCacheStats) -> Self {
        Self { clear_calls, stats }
    }
}

#[async_trait]
impl ToolExecutor for CacheAwareToolExecutor {
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

    fn clear_cache(&self) {
        self.clear_calls.fetch_add(1, Ordering::Relaxed);
    }

    fn cache_stats(&self) -> Option<crate::act::ToolCacheStats> {
        Some(self.stats)
    }
}

#[derive(Debug)]
struct SequentialMockLlm {
    responses: Mutex<VecDeque<CompletionResponse>>,
}

impl SequentialMockLlm {
    fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }
}

#[async_trait]
impl LlmProvider for SequentialMockLlm {
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

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        self.responses
            .lock()
            .expect("lock")
            .pop_front()
            .ok_or_else(|| ProviderError::Provider("no response".to_string()))
    }
}

fn test_engine() -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

fn failing_tool_engine() -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(FailingToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

fn test_snapshot(text: &str) -> PerceptionSnapshot {
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

fn text_response(
    text: &str,
    stop_reason: Option<&str>,
    usage: Option<fx_llm::Usage>,
) -> CompletionResponse {
    CompletionResponse {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        tool_calls: Vec::new(),
        usage,
        stop_reason: stop_reason.map(|value| value.to_string()),
    }
}

fn tool_call_response(id: &str, name: &str, arguments: serde_json::Value) -> CompletionResponse {
    CompletionResponse {
        content: Vec::new(),
        tool_calls: vec![ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments,
        }],
        usage: None,
        stop_reason: Some("tool_use".to_string()),
    }
}

fn mixed_tool_response_with_content(
    content: Vec<ContentBlock>,
    id: &str,
    name: &str,
    arguments: serde_json::Value,
) -> CompletionResponse {
    CompletionResponse {
        content,
        tool_calls: vec![ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments,
        }],
        usage: None,
        stop_reason: Some("tool_use".to_string()),
    }
}

fn mixed_tool_response(
    text: &str,
    id: &str,
    name: &str,
    arguments: serde_json::Value,
) -> CompletionResponse {
    mixed_tool_response_with_content(
        vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        id,
        name,
        arguments,
    )
}

fn expect_complete(result: LoopResult) -> (String, u32, Vec<Signal>) {
    match result {
        LoopResult::Complete {
            response,
            iterations,
            signals,
            ..
        } => (response, iterations, signals),
        other => panic!("expected LoopResult::Complete, got: {other:?}"),
    }
}

fn has_truncation_trace(signals: &[Signal], step: LoopStep) -> bool {
    signals.iter().any(|signal| {
        signal.step == step
            && signal.kind == SignalKind::Trace
            && signal.message.starts_with("response truncated, continuing")
    })
}

#[derive(Debug)]
struct StreamingCaptureLlm {
    streamed_max_tokens: Mutex<Vec<u32>>,
    complete_calls: Mutex<u32>,
    output: String,
}

impl StreamingCaptureLlm {
    fn new(output: &str) -> Self {
        Self {
            streamed_max_tokens: Mutex::new(Vec::new()),
            complete_calls: Mutex::new(0),
            output: output.to_string(),
        }
    }

    fn streamed_max_tokens(&self) -> Vec<u32> {
        self.streamed_max_tokens.lock().expect("lock").clone()
    }

    fn complete_calls(&self) -> u32 {
        *self.complete_calls.lock().expect("lock")
    }
}

#[async_trait]
impl LlmProvider for StreamingCaptureLlm {
    async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
        Ok(self.output.clone())
    }

    async fn generate_streaming(
        &self,
        _: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, CoreLlmError> {
        self.streamed_max_tokens
            .lock()
            .expect("lock")
            .push(max_tokens);
        callback(self.output.clone());
        Ok(self.output.clone())
    }

    fn model_name(&self) -> &str {
        "stream-capture"
    }

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let mut calls = self.complete_calls.lock().expect("lock");
        *calls = calls.saturating_add(1);
        Err(ProviderError::Provider(
            "complete should not be called".to_string(),
        ))
    }
}

// NB2-3: decide extracts multiple tool calls
#[tokio::test]
async fn decide_extracts_multiple_tool_calls() {
    let mut engine = test_engine();
    let response = CompletionResponse {
        content: Vec::new(),
        tool_calls: vec![
            ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"a.txt"}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path":"b.txt","content":"hi"}),
            },
            ToolCall {
                id: "3".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"cmd":"ls"}),
            },
        ],
        usage: None,
        stop_reason: None,
    };

    let decision = engine.decide(&response).await.expect("decision");

    match decision {
        Decision::UseTools(calls) => {
            assert_eq!(calls.len(), 3, "all 3 tool calls should be preserved");
            assert_eq!(calls[0].name, "read_file");
            assert_eq!(calls[1].name, "write_file");
            assert_eq!(calls[2].name, "run_command");
        }
        other => panic!("expected Decision::UseTools, got: {other:?}"),
    }
}

// NB2-4: run_cycle completes with a direct tool call
#[tokio::test]
async fn run_cycle_completes_with_direct_tool_call() {
    let mut engine = test_engine();

    // First response: LLM returns a tool call
    // Second response: LLM synthesizes the tool results into a final answer
    // Third response: continuation re-prompt gets text-only, ending the outer loop
    let llm = SequentialMockLlm::new(vec![
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        },
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "README loaded".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
        // Outer loop continuation: model re-prompted, responds text-only
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "README loaded".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the readme"), &llm)
        .await
        .expect("run_cycle");

    assert!(
        matches!(result, LoopResult::Complete { .. }),
        "expected LoopResult::Complete, got: {result:?}"
    );
}

#[tokio::test]
async fn act_preserves_mixed_text_in_partial_response() {
    let mut engine = test_engine();
    let response = mixed_tool_response(
        "Initial findings",
        "call-1",
        "read_file",
        serde_json::json!({"path":"README.md"}),
    );
    let decision = engine.decide(&response).await.expect("decision");
    let llm = SequentialMockLlm::new(vec![text_response("Final answer", None, None)]);

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user("read the file")],
            CycleStream::disabled(),
        )
        .await
        .expect("act");

    assert_eq!(action.response_text, "Initial findings\n\nFinal answer");
    match action.next_step {
        ActionNextStep::Continue(ActionContinuation {
            partial_response,
            context_message,
            context_messages,
            ..
        }) => {
            assert_eq!(
                partial_response.as_deref(),
                Some("Initial findings\n\nFinal answer")
            );
            assert_eq!(context_message, None);
            assert!(context_messages.iter().any(|message| {
                message.content.iter().any(|block| {
                    matches!(
                        block,
                        ContentBlock::ToolResult { content, .. }
                            if content == &serde_json::json!("ok")
                    )
                })
            }));
        }
        other => panic!("expected continuation, got {other:?}"),
    }
}

#[tokio::test]
async fn run_cycle_preserves_mixed_text_in_final_output() {
    let mut engine = test_engine();
    let expected = "Initial findings\n\nFinal answer";
    let llm = SequentialMockLlm::new(vec![
        mixed_tool_response(
            "Initial findings",
            "call-1",
            "read_file",
            serde_json::json!({"path":"README.md"}),
        ),
        text_response("Final answer", None, None),
        text_response(expected, None, None),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the file"), &llm)
        .await
        .expect("run_cycle");
    let (response, _, _) = expect_complete(result);

    assert_eq!(response, expected);
}

#[tokio::test]
async fn mixed_text_with_tool_calls_preserves_text_fragments() {
    let mut engine = test_engine();
    let expected = "First note\n\nSecond note\n\nFinal answer";
    let llm = SequentialMockLlm::new(vec![
        mixed_tool_response(
            "First note",
            "call-1",
            "read_file",
            serde_json::json!({"path":"README.md"}),
        ),
        mixed_tool_response(
            "Second note",
            "call-2",
            "read_file",
            serde_json::json!({"path":"Cargo.toml"}),
        ),
        text_response("Final answer", None, None),
        text_response(expected, None, None),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read both files"), &llm)
        .await
        .expect("run_cycle");
    let (response, _, _) = expect_complete(result);

    assert_eq!(response, expected);
}

#[tokio::test]
async fn empty_current_round_does_not_continue_from_accumulated_text() {
    let mut engine = test_engine();
    let response = mixed_tool_response(
        "Initial findings",
        "call-1",
        "read_file",
        serde_json::json!({"path":"README.md"}),
    );
    let decision = engine.decide(&response).await.expect("decision");
    let llm = test_fixtures::RecordingLlm::with_generated_summary(
        vec![Ok(text_response("", None, None))],
        String::new(),
    );

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user("read the file")],
            CycleStream::disabled(),
        )
        .await
        .expect("act");

    assert!(
        action.response_text.is_empty(),
        "empty rounds should not become response text via accumulated fragments"
    );
    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Incomplete {
            partial_response,
            reason,
        }) => {
            assert!(reason.contains("did not produce a usable final response"));
            assert!(partial_response
                .as_deref()
                .is_some_and(|text| text.contains("Initial findings")));
        }
        other => panic!("expected terminal incomplete action, got {other:?}"),
    }
    assert_eq!(llm.requests().len(), 1);
}

#[tokio::test]
async fn standard_turn_with_mixed_text_terminates_normally() {
    let prompt = "Read the README then make a small improvement to it.";
    let mut engine = test_engine();
    let llm = test_fixtures::RecordingLlm::with_generated_summary(
        vec![
            Ok::<CompletionResponse, ProviderError>(mixed_tool_response(
                "I am reading the README first.",
                "call-1",
                "read_file",
                serde_json::json!({"path":"README.md"}),
            )),
            Ok(text_response("", None, None)),
            Err(ProviderError::Provider(
                "unexpected continuation after an empty tool round".to_string(),
            )),
        ],
        String::new(),
    );

    let result = engine
        .run_cycle(test_snapshot(prompt), &llm)
        .await
        .expect("run_cycle");

    match result {
        LoopResult::Incomplete {
            partial_response,
            iterations,
            ..
        } => {
            assert_eq!(iterations, 1);
            assert!(partial_response
                .as_deref()
                .is_some_and(|text| text.contains("I am reading the README first.")));
        }
        other => panic!("expected incomplete termination, got {other:?}"),
    }
    assert_eq!(llm.requests().len(), 2);
}

#[tokio::test]
async fn run_cycle_whitespace_only_mixed_text_is_unchanged() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![
        mixed_tool_response(
            "   ",
            "call-1",
            "read_file",
            serde_json::json!({"path":"README.md"}),
        ),
        text_response("Final answer", None, None),
        text_response("Final answer", None, None),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the file"), &llm)
        .await
        .expect("run_cycle");
    let (response, _, _) = expect_complete(result);

    assert_eq!(response, "Final answer");
}

#[tokio::test]
async fn run_cycle_preserves_multiple_text_blocks_in_mixed_response() {
    let mut engine = test_engine();
    let expected = "First block\nSecond block\n\nFinal answer";
    let llm = SequentialMockLlm::new(vec![
        mixed_tool_response_with_content(
            vec![
                ContentBlock::Text {
                    text: "First block".to_string(),
                },
                ContentBlock::Text {
                    text: "Second block".to_string(),
                },
            ],
            "call-1",
            "read_file",
            serde_json::json!({"path":"README.md"}),
        ),
        text_response("Final answer", None, None),
        text_response(expected, None, None),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the file"), &llm)
        .await
        .expect("run_cycle");
    let (response, _, _) = expect_complete(result);

    assert_eq!(response, expected);
}

#[tokio::test]
async fn run_cycle_tool_only_response_is_unchanged() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![
        tool_call_response(
            "call-1",
            "read_file",
            serde_json::json!({"path":"README.md"}),
        ),
        text_response("Tool answer", None, None),
        text_response("Tool answer", None, None),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the file"), &llm)
        .await
        .expect("run_cycle");
    let (response, _, _) = expect_complete(result);

    assert_eq!(response, "Tool answer");
}

#[tokio::test]
async fn run_cycle_text_only_response_is_unchanged() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![text_response("Just text", None, None)]);

    let result = engine
        .run_cycle(test_snapshot("say hi"), &llm)
        .await
        .expect("run_cycle");
    let (response, _, _) = expect_complete(result);

    assert_eq!(response, "Just text");
}

#[tokio::test]
async fn run_cycle_completes_after_tool_fails_with_synthesis() {
    let mut engine = failing_tool_engine();

    let llm = SequentialMockLlm::new(vec![
        // reason: LLM returns a tool call
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        },
        // act_with_tools re-prompt: LLM synthesizes tool failure
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "The file could not be read: path escapes working directory.".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
        // outer loop continuation: re-prompted model responds text-only
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "The file could not be read: path escapes working directory.".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the readme"), &llm)
        .await
        .expect("run_cycle");

    match result {
        LoopResult::Complete {
            response,
            iterations,
            ..
        } => {
            // Tool failure synthesis now becomes internal continuation
            // context, and the next root reasoning pass owns the final
            // user-visible response.
            assert_eq!(
                iterations, 2,
                "expected root continuation after tool synthesis"
            );
            assert_eq!(
                response,
                "The file could not be read: path escapes working directory."
            );
        }
        other => panic!("expected LoopResult::Complete, got: {other:?}"),
    }
}

// NB2-5: run_cycle returns budget exhausted when budget is 0
#[tokio::test]
async fn run_cycle_returns_budget_exhausted() {
    let zero_budget = crate::budget::BudgetConfig {
        max_llm_calls: 0,
        max_tool_invocations: 0,
        max_tokens: 0,
        max_cost_cents: 0,
        max_wall_time_ms: 0,
        max_recursion_depth: 0,
        decompose_depth_mode: DepthMode::Adaptive,
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(zero_budget, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");

    let llm = SequentialMockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "hello".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let result = engine
        .run_cycle(test_snapshot("hello"), &llm)
        .await
        .expect("run_cycle");

    assert!(
        matches!(result, LoopResult::BudgetExhausted { .. }),
        "expected LoopResult::BudgetExhausted, got: {result:?}"
    );
}

#[test]
fn build_continuation_messages_omits_empty_assistant_text() {
    let base_messages = vec![Message::user("Start here")];
    let messages = build_continuation_messages(&base_messages, "");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0], Message::user("Start here"));
    assert_eq!(
        messages[1],
        Message::user("Continue from exactly where you left off. Do not repeat prior text.")
    );
}

#[tokio::test]
async fn budget_exhaustion_emits_blocked_signal() {
    let zero_budget = crate::budget::BudgetConfig {
        max_llm_calls: 0,
        max_tool_invocations: 0,
        max_tokens: 0,
        max_cost_cents: 0,
        max_wall_time_ms: 0,
        max_recursion_depth: 0,
        decompose_depth_mode: DepthMode::Adaptive,
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(zero_budget, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");

    let llm = SequentialMockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "hello".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let result = engine
        .run_cycle(test_snapshot("hello"), &llm)
        .await
        .expect("run_cycle");

    let signals = match result {
        LoopResult::Complete { signals, .. }
        | LoopResult::BudgetExhausted { signals, .. }
        | LoopResult::Incomplete { signals, .. }
        | LoopResult::UserStopped { signals, .. }
        | LoopResult::Error { signals, .. } => signals,
    };

    assert!(signals
        .iter()
        .any(|s| s.step == LoopStep::Act && s.kind == SignalKind::Blocked));
}

#[tokio::test]
async fn run_cycle_emits_signals() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "hello".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: Some(fx_llm::Usage {
            input_tokens: 8,
            output_tokens: 4,
        }),
        stop_reason: None,
    }]);

    let result = engine
        .run_cycle(test_snapshot("hello"), &llm)
        .await
        .expect("run_cycle");

    let signals = match result {
        LoopResult::Complete { signals, .. }
        | LoopResult::BudgetExhausted { signals, .. }
        | LoopResult::Incomplete { signals, .. }
        | LoopResult::UserStopped { signals, .. }
        | LoopResult::Error { signals, .. } => signals,
    };

    // Verify expected signal types for a text-response cycle.
    assert!(signals
        .iter()
        .any(|s| s.step == LoopStep::Perceive && s.kind == SignalKind::Trace));
    assert!(signals
        .iter()
        .any(|s| s.step == LoopStep::Reason && s.kind == SignalKind::Trace));
    assert!(signals
        .iter()
        .any(|s| s.step == LoopStep::Reason && s.kind == SignalKind::Performance));
    assert!(signals
        .iter()
        .any(|s| s.step == LoopStep::Decide && s.kind == SignalKind::Decision));
    // A clean text response (no tools, no failures) should NOT emit
    // any observation signals — observations are only for noteworthy events.
    assert!(
        !signals
            .iter()
            .any(|s| s.step == LoopStep::Act && s.kind == SignalKind::Observation),
        "clean text response should not emit observation signals"
    );
}

#[tokio::test]
async fn run_cycle_clears_tool_cache_at_cycle_boundary() {
    let clear_calls = Arc::new(AtomicUsize::new(0));
    let stats = crate::act::ToolCacheStats::default();
    let executor = CacheAwareToolExecutor::new(Arc::clone(&clear_calls), stats);
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            0,
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(executor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");

    let llm = SequentialMockLlm::new(vec![
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "one".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "two".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
    ]);

    engine
        .run_cycle(test_snapshot("hello"), &llm)
        .await
        .expect("first cycle");
    engine
        .run_cycle(test_snapshot("hello"), &llm)
        .await
        .expect("second cycle");

    assert_eq!(clear_calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn run_cycle_emits_tool_cache_stats_signal() {
    let clear_calls = Arc::new(AtomicUsize::new(0));
    let stats = crate::act::ToolCacheStats {
        hits: 2,
        misses: 1,
        entries: 4,
        evictions: 1,
    };
    let executor = CacheAwareToolExecutor::new(Arc::clone(&clear_calls), stats);
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            0,
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(executor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");

    let llm = SequentialMockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "done".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let result = engine
        .run_cycle(test_snapshot("hello"), &llm)
        .await
        .expect("run cycle");
    let signals = match result {
        LoopResult::Complete { signals, .. }
        | LoopResult::BudgetExhausted { signals, .. }
        | LoopResult::Incomplete { signals, .. }
        | LoopResult::UserStopped { signals, .. }
        | LoopResult::Error { signals, .. } => signals,
    };

    let cache_signal = signals
        .iter()
        .find(|signal| {
            signal.step == LoopStep::Act
                && signal.kind == SignalKind::Performance
                && signal.message == "tool cache stats"
        })
        .expect("cache stats signal");

    assert_eq!(cache_signal.metadata["hits"], serde_json::json!(2));
    assert_eq!(cache_signal.metadata["misses"], serde_json::json!(1));
    assert_eq!(cache_signal.metadata["entries"], serde_json::json!(4));
    assert_eq!(cache_signal.metadata["evictions"], serde_json::json!(1));
    assert_eq!(
        cache_signal.metadata["hit_rate"],
        serde_json::json!(2.0 / 3.0)
    );
    assert_eq!(clear_calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn signals_include_decision_on_tool_call() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            }],
            usage: Some(fx_llm::Usage {
                input_tokens: 10,
                output_tokens: 2,
            }),
            stop_reason: Some("tool_use".to_string()),
        },
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
        // Outer loop continuation: text-only response ends the loop
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the readme"), &llm)
        .await
        .expect("run_cycle");

    let signals = match result {
        LoopResult::Complete { signals, .. }
        | LoopResult::BudgetExhausted { signals, .. }
        | LoopResult::Incomplete { signals, .. }
        | LoopResult::UserStopped { signals, .. }
        | LoopResult::Error { signals, .. } => signals,
    };

    assert!(signals
        .iter()
        .any(|signal| { signal.step == LoopStep::Decide && signal.kind == SignalKind::Decision }));
}

#[tokio::test]
async fn tool_continuation_rounds_emit_trace_and_performance_signals() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            }],
            usage: Some(fx_llm::Usage {
                input_tokens: 10,
                output_tokens: 2,
            }),
            stop_reason: Some("tool_use".to_string()),
        },
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-2".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"Cargo.toml"}),
            }],
            usage: Some(fx_llm::Usage {
                input_tokens: 6,
                output_tokens: 3,
            }),
            stop_reason: Some("tool_use".to_string()),
        },
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 5,
                output_tokens: 4,
            }),
            stop_reason: None,
        },
        // Outer loop continuation: text-only response ends the loop
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
    ]);

    let result = engine
        .run_cycle(test_snapshot("read files"), &llm)
        .await
        .expect("run_cycle");

    let signals = match result {
        LoopResult::Complete { signals, .. }
        | LoopResult::BudgetExhausted { signals, .. }
        | LoopResult::Incomplete { signals, .. }
        | LoopResult::UserStopped { signals, .. }
        | LoopResult::Error { signals, .. } => signals,
    };

    let round_trace_count = signals
        .iter()
        .filter(|signal| {
            signal.step == LoopStep::Act
                && signal.kind == SignalKind::Trace
                && signal.message == "tool continuation round"
        })
        .count();
    let round_perf_count = signals
        .iter()
        .filter(|signal| {
            signal.step == LoopStep::Act
                && signal.kind == SignalKind::Performance
                && signal.message == "tool continuation latency"
        })
        .count();
    assert_eq!(round_trace_count, 2, "expected 2 round trace signals");
    assert_eq!(round_perf_count, 2, "expected 2 round performance signals");
}

#[tokio::test]
async fn empty_tool_continuation_emits_empty_text_trace() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        },
        CompletionResponse {
            content: Vec::new(),
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
        // Outer loop continuation: text-only response ends the loop
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the readme"), &llm)
        .await
        .expect("run_cycle");

    let (partial_response, reason, signals) = match result {
        LoopResult::Incomplete {
            partial_response,
            reason,
            signals,
            ..
        } => (partial_response, reason, signals),
        other => panic!("expected LoopResult::Incomplete, got: {other:?}"),
    };

    assert_eq!(
        partial_response.as_deref(),
        Some("completed tool work: read_file")
    );
    assert_eq!(
        reason,
        "tool continuation did not produce a usable final response"
    );
    assert!(signals.iter().any(|signal| {
        signal.step == LoopStep::Act
            && signal.kind == SignalKind::Trace
            && signal.message == "tool continuation returned empty text"
    }));
}

#[test]
fn is_truncated_detects_anthropic_stop_reason() {
    assert!(is_truncated(Some("max_tokens")));
    assert!(is_truncated(Some("MAX_TOKENS")));
}

#[test]
fn is_truncated_detects_openai_finish_reason() {
    assert!(is_truncated(Some("length")));
    assert!(is_truncated(Some("LENGTH")));
}

#[test]
fn is_truncated_handles_none_and_unknown() {
    assert!(!is_truncated(None));
    assert!(!is_truncated(Some("stop")));
    assert!(!is_truncated(Some("tool_use")));
}

#[test]
fn merge_usage_combines_token_counts() {
    let merged = merge_usage(
        Some(fx_llm::Usage {
            input_tokens: 100,
            output_tokens: 25,
        }),
        Some(fx_llm::Usage {
            input_tokens: 30,
            output_tokens: 10,
        }),
    )
    .expect("usage should merge");
    assert_eq!(merged.input_tokens, 130);
    assert_eq!(merged.output_tokens, 35);

    let right_only = merge_usage(
        None,
        Some(fx_llm::Usage {
            input_tokens: 7,
            output_tokens: 3,
        }),
    )
    .expect("right usage should be preserved");
    assert_eq!(right_only.input_tokens, 7);
    assert_eq!(right_only.output_tokens, 3);

    assert!(merge_usage(None, None).is_none());
}

#[test]
fn merge_continuation_response_preserves_tool_calls_when_continuation_has_none() {
    let previous = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "preface".to_string(),
        }],
        tool_calls: vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }],
        usage: None,
        stop_reason: Some("max_tokens".to_string()),
    };
    let continued = text_response(" continuation", Some("stop"), None);
    let mut full_text = "preface".to_string();

    let merged = merge_continuation_response(previous, continued, &mut full_text);

    assert_eq!(merged.tool_calls.len(), 1);
    assert_eq!(merged.tool_calls[0].id, "call-1");
}

#[test]
fn build_truncation_continuation_request_enables_tools_only_for_reason_step() {
    let tool_definitions = vec![ToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({"type":"object"}),
    }];
    let messages = vec![Message::user("continue")];

    let reason_request =
        build_truncation_continuation_request(TruncationContinuationRequestParams::new(
            "mock",
            &messages,
            ToolRequestConfig::new(tool_definitions.clone(), true),
            RequestBuildContext::new(None, None, None, false),
            LoopStep::Reason,
        ));
    let act_request =
        build_truncation_continuation_request(TruncationContinuationRequestParams::new(
            "mock",
            &messages,
            ToolRequestConfig::new(tool_definitions, true),
            RequestBuildContext::new(None, None, None, false),
            LoopStep::Act,
        ));

    assert!(reason_request
        .tools
        .iter()
        .any(|tool| tool.name == "read_file"));
    assert!(act_request.tools.is_empty());
}

#[tokio::test]
async fn continue_truncated_response_stitches_text() {
    let mut engine = test_engine();
    let initial = text_response(
        "Hello",
        Some("max_tokens"),
        Some(fx_llm::Usage {
            input_tokens: 10,
            output_tokens: 4,
        }),
    );
    let llm = SequentialMockLlm::new(vec![text_response(
        " world",
        Some("stop"),
        Some(fx_llm::Usage {
            input_tokens: 3,
            output_tokens: 2,
        }),
    )]);

    let stitched = engine
        .continue_truncated_response(
            initial,
            &[Message::user("hello")],
            &llm,
            LoopStep::Reason,
            CycleStream::disabled(),
        )
        .await
        .expect("continuation should succeed");

    assert_eq!(extract_response_text(&stitched), "Hello world");
    assert_eq!(stitched.stop_reason.as_deref(), Some("stop"));
    let usage = stitched.usage.expect("usage should be merged");
    assert_eq!(usage.input_tokens, 13);
    assert_eq!(usage.output_tokens, 6);
}

#[tokio::test]
async fn continue_truncated_response_respects_max_attempts() {
    let mut engine = test_engine();
    let initial = text_response("A", Some("max_tokens"), None);
    let llm = SequentialMockLlm::new(vec![
        text_response("B", Some("max_tokens"), None),
        text_response("C", Some("max_tokens"), None),
        text_response("D", Some("max_tokens"), None),
    ]);

    let stitched = engine
        .continue_truncated_response(
            initial,
            &[Message::user("continue")],
            &llm,
            LoopStep::Reason,
            CycleStream::disabled(),
        )
        .await
        .expect("continuation should stop at max attempts");

    assert_eq!(extract_response_text(&stitched), "ABCD");
    assert_eq!(stitched.stop_reason.as_deref(), Some("max_tokens"));
}

#[tokio::test]
async fn continue_truncated_response_stops_on_natural_end() {
    let mut engine = test_engine();
    let initial = text_response("A", Some("max_tokens"), None);
    let llm = SequentialMockLlm::new(vec![
        text_response("B", Some("stop"), None),
        text_response("C", Some("max_tokens"), None),
    ]);

    let stitched = engine
        .continue_truncated_response(
            initial,
            &[Message::user("continue")],
            &llm,
            LoopStep::Reason,
            CycleStream::disabled(),
        )
        .await
        .expect("continuation should stop when natural stop reason arrives");

    assert_eq!(extract_response_text(&stitched), "AB");
    assert_eq!(stitched.stop_reason.as_deref(), Some("stop"));
}

#[tokio::test]
async fn run_cycle_auto_continues_truncated_response() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![
        text_response("First half", Some("max_tokens"), None),
        text_response(" second half", Some("stop"), None),
    ]);

    let result = engine
        .run_cycle(test_snapshot("finish your sentence"), &llm)
        .await
        .expect("run_cycle should succeed");
    let (response, iterations, _) = expect_complete(result);

    assert_eq!(iterations, 1);
    assert_eq!(response, "First half second half");
}

#[tokio::test]
async fn tool_continuation_auto_continues_truncated_response() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![
        tool_call_response(
            "call-1",
            "read_file",
            serde_json::json!({"path":"README.md"}),
        ),
        text_response("Tool answer part", Some("length"), None),
        text_response(" two", Some("stop"), None),
        text_response("Tool answer part two", None, None),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the file"), &llm)
        .await
        .expect("run_cycle should succeed");
    let (response, iterations, _) = expect_complete(result);

    assert_eq!(iterations, 2);
    assert_eq!(response, "Tool answer part two");
}

#[tokio::test]
async fn reason_truncation_continuation_preserves_initial_tool_calls() {
    let mut engine = test_engine();
    let llm = SequentialMockLlm::new(vec![
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "I will read the file".to_string(),
            }],
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            }],
            usage: None,
            stop_reason: Some("max_tokens".to_string()),
        },
        text_response(" and summarize it", Some("stop"), None),
        text_response("tool executed", Some("stop"), None),
        // Outer loop continuation: text-only response ends the loop
        text_response("tool executed", None, None),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the file"), &llm)
        .await
        .expect("run_cycle should succeed");
    let (response, _, signals) = expect_complete(result);

    assert_eq!(response, "tool executed");
    assert!(has_truncation_trace(&signals, LoopStep::Reason));
    assert!(signals.iter().any(|signal| {
        signal.step == LoopStep::Act
            && signal.kind == SignalKind::Success
            && signal.message == "tool read_file"
    }));
}

#[tokio::test]
async fn finalize_tool_response_receives_stitched_text_after_continuation() {
    let mut engine = test_engine();
    let overlap = "x".repeat(90);
    let first = format!("Start {overlap}");
    let second = format!("{overlap} End");
    let expected = format!("Start {overlap} End");
    let llm = SequentialMockLlm::new(vec![
        tool_call_response(
            "call-1",
            "read_file",
            serde_json::json!({"path":"README.md"}),
        ),
        text_response(&first, Some("max_tokens"), None),
        text_response(&second, Some("stop"), None),
        // Outer loop continuation: text-only response ends the loop
        text_response(&expected, None, None),
    ]);

    let result = engine
        .run_cycle(test_snapshot("summarize tool output"), &llm)
        .await
        .expect("run_cycle should succeed");
    let (response, _, _) = expect_complete(result);

    assert_eq!(response, expected);
}

#[tokio::test]
async fn truncation_continuation_emits_reason_and_act_trace_signals() {
    let mut reason_engine = test_engine();
    let reason_llm = SequentialMockLlm::new(vec![
        text_response("Reason part", Some("max_tokens"), None),
        text_response(" complete", Some("stop"), None),
    ]);

    let reason_result = reason_engine
        .run_cycle(test_snapshot("reason continuation"), &reason_llm)
        .await
        .expect("reason run should succeed");
    let (_, _, reason_signals) = expect_complete(reason_result);
    assert!(has_truncation_trace(&reason_signals, LoopStep::Reason));

    let mut act_engine = test_engine();
    let act_llm = SequentialMockLlm::new(vec![
        tool_call_response(
            "call-1",
            "read_file",
            serde_json::json!({"path":"README.md"}),
        ),
        text_response("Act part", Some("length"), None),
        text_response(" complete", Some("stop"), None),
        // Outer loop continuation: text-only response ends the loop
        text_response("Act part complete", None, None),
    ]);

    let act_result = act_engine
        .run_cycle(test_snapshot("act continuation"), &act_llm)
        .await
        .expect("act run should succeed");
    let (_, _, act_signals) = expect_complete(act_result);
    assert!(has_truncation_trace(&act_signals, LoopStep::Act));
}

#[tokio::test]
async fn continuation_calls_record_budget() {
    let mut baseline_engine = test_engine();
    let baseline_llm = SequentialMockLlm::new(vec![text_response("done", Some("stop"), None)]);
    baseline_engine
        .run_cycle(test_snapshot("baseline"), &baseline_llm)
        .await
        .expect("baseline run should succeed");
    let baseline_calls = baseline_engine.status(current_time_ms()).llm_calls_used;

    let mut continuation_engine = test_engine();
    let continuation_llm = SequentialMockLlm::new(vec![
        text_response("first", Some("max_tokens"), None),
        text_response(" second", Some("stop"), None),
    ]);
    continuation_engine
        .run_cycle(test_snapshot("needs continuation"), &continuation_llm)
        .await
        .expect("continuation run should succeed");
    let continuation_calls = continuation_engine.status(current_time_ms()).llm_calls_used;

    assert_eq!(continuation_calls, baseline_calls.saturating_add(1));
}

#[test]
fn raised_max_tokens_constants_are_applied() {
    assert_eq!(REASONING_MAX_OUTPUT_TOKENS, 4096);
    assert_eq!(TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS, 1024);

    let perception = ProcessedPerception {
        user_message: "hello".to_string(),
        images: Vec::new(),
        documents: Vec::new(),
        context_window: vec![Message::user("hello")],
        active_goals: vec!["reply".to_string()],
        budget_remaining: BudgetRemaining {
            llm_calls: 8,
            tool_invocations: 16,
            tokens: 10_000,
            cost_cents: 100,
            wall_time_ms: 1_000,
        },
        steer_context: None,
    };

    let reasoning_request = build_reasoning_request(ReasoningRequestParams::new(
        &perception,
        "mock",
        ToolRequestConfig::new(vec![], true),
        RequestBuildContext::new(None, None, None, false),
    ));
    let continuation_request = build_continuation_request(ContinuationRequestParams::new(
        &perception.context_window,
        "mock",
        ToolRequestConfig::new(vec![], true),
        RequestBuildContext::new(None, None, None, false),
    ));

    assert_eq!(reasoning_request.max_tokens, Some(4096));
    assert_eq!(continuation_request.max_tokens, Some(4096));
}

#[tokio::test]
async fn tool_synthesis_uses_raised_token_cap_without_stop_reason_assumptions() {
    let engine = test_engine();
    let llm = StreamingCaptureLlm::new("summary from stream");

    let summary = engine
        .generate_tool_summary(
            "summarize this",
            &llm,
            CycleStream::disabled(),
            TextStreamVisibility::Public,
        )
        .await
        .expect("streaming synthesis should succeed");

    assert_eq!(summary, "summary from stream");
    assert_eq!(
        llm.streamed_max_tokens(),
        vec![TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS]
    );
    assert_eq!(llm.complete_calls(), 0);
}

// B2: extract_readable_text unit tests
#[test]
fn extract_readable_text_passes_plain_text_through() {
    assert_eq!(extract_readable_text("Hello world"), "Hello world");
}

#[test]
fn extract_readable_text_extracts_text_field() {
    let json = r##"{"text": "Hello from JSON"}"##;
    assert_eq!(extract_readable_text(json), "Hello from JSON");
}

#[test]
fn extract_readable_text_extracts_response_field() {
    let json = r#"{"response": "Extracted response"}"#;
    assert_eq!(extract_readable_text(json), "Extracted response");
}

#[test]
fn extract_readable_text_returns_raw_for_unrecognized_json() {
    let json = r#"{"weird_key": "some value"}"#;
    assert_eq!(extract_readable_text(json), json);
}

#[test]
fn extract_readable_text_handles_invalid_json() {
    let broken = r#"{not valid json"#;
    assert_eq!(extract_readable_text(broken), broken);
}
