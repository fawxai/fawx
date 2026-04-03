use super::*;
use crate::cancellation::CancellationToken;
use crate::input::{loop_input_channel, LoopCommand};
use async_trait::async_trait;
use futures_util::StreamExt;
use fx_core::error::LlmError as CoreLlmError;
use fx_core::message::{InternalMessage, StreamPhase};
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_llm::{
    CompletionRequest, CompletionResponse, CompletionStream, ContentBlock, Message, ProviderError,
    StreamChunk, ToolCall, ToolDefinition, ToolUseDelta, Usage,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::time::{Duration, Instant};

#[derive(Debug, Default)]
struct NoopToolExecutor;

#[async_trait]
impl ToolExecutor for NoopToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls.iter().map(success_result).collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![read_file_definition()]
    }
}

#[derive(Debug)]
struct DelayedToolExecutor {
    delay: Duration,
}

impl DelayedToolExecutor {
    fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

#[async_trait]
impl ToolExecutor for DelayedToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        wait_for_delay_or_cancel(self.delay, cancel).await;
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            return Ok(Vec::new());
        }
        Ok(calls.iter().map(success_result).collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![read_file_definition()]
    }
}

#[derive(Debug)]
struct RoundCancellingToolExecutor {
    delay: Duration,
    rounds: Arc<AtomicUsize>,
    cancel_after_round: usize,
}

impl RoundCancellingToolExecutor {
    fn new(delay: Duration, rounds: Arc<AtomicUsize>, cancel_after_round: usize) -> Self {
        Self {
            delay,
            rounds,
            cancel_after_round,
        }
    }
}

#[async_trait]
impl ToolExecutor for RoundCancellingToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        tokio::time::sleep(self.delay).await;
        let current_round = self.rounds.fetch_add(1, Ordering::SeqCst) + 1;
        let results = calls.iter().map(success_result).collect();
        if current_round >= self.cancel_after_round {
            if let Some(token) = cancel {
                token.cancel();
            }
        }
        Ok(results)
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![read_file_definition()]
    }
}

#[derive(Debug)]
struct ScriptedLlm {
    responses: Mutex<VecDeque<CompletionResponse>>,
}

impl ScriptedLlm {
    fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
        }
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
            .ok_or_else(|| ProviderError::Provider("no response".to_string()))
    }
}

#[derive(Debug)]
struct PartialErrorStreamLlm;

#[derive(Debug)]
struct FailingBufferedStreamLlm;

#[async_trait]
impl LlmProvider for PartialErrorStreamLlm {
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
        "partial-error-stream"
    }

    async fn complete_stream(
        &self,
        _: CompletionRequest,
    ) -> Result<CompletionStream, ProviderError> {
        let chunks = vec![
            Ok(StreamChunk {
                delta_content: Some("partial".to_string()),
                tool_use_deltas: Vec::new(),
                usage: None,
                stop_reason: None,
            }),
            Err(ProviderError::Streaming(
                "simulated stream failure".to_string(),
            )),
        ];
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }
}

#[async_trait]
impl LlmProvider for FailingBufferedStreamLlm {
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
        "failing-buffered-stream"
    }

    async fn complete_stream(
        &self,
        _: CompletionRequest,
    ) -> Result<CompletionStream, ProviderError> {
        Err(ProviderError::Provider(
            "simulated stream setup failure".to_string(),
        ))
    }
}

#[derive(Debug)]
struct FailingStreamingLlm;

#[async_trait]
impl LlmProvider for FailingStreamingLlm {
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
        "failing-streaming"
    }

    async fn stream(
        &self,
        _: CompletionRequest,
        _: ProviderStreamCallback,
    ) -> Result<CompletionResponse, ProviderError> {
        Err(ProviderError::Provider(
            "simulated streaming failure".to_string(),
        ))
    }
}

fn engine_with_executor(executor: Arc<dyn ToolExecutor>, max_iterations: u32) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            0,
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(max_iterations)
        .tool_executor(executor)
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

fn read_file_definition() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({"type":"object"}),
    }
}

fn read_file_call(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }
}

fn success_result(call: &ToolCall) -> ToolResult {
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        success: true,
        output: "ok".to_string(),
    }
}

fn tool_use_response(call_id: &str) -> CompletionResponse {
    CompletionResponse {
        content: Vec::new(),
        tool_calls: vec![read_file_call(call_id)],
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

fn stream_recorder() -> (StreamCallback, Arc<Mutex<Vec<StreamEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&events);
    let callback: StreamCallback = Arc::new(move |event| {
        captured.lock().expect("lock").push(event);
    });
    (callback, events)
}

#[test]
fn error_callback_guard_restores_original_value_after_panic() {
    let (original, original_events) = stream_recorder();
    let (replacement, replacement_events) = stream_recorder();
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    engine.error_callback = Some(original.clone());

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let guard = ErrorCallbackGuard::install(&mut engine, Some(replacement.clone()));
        guard
            .error_callback
            .as_ref()
            .expect("replacement should be installed")(StreamEvent::Done {
            response: "replacement".to_string(),
        });
        panic!("boom");
    }));

    assert!(result.is_err());
    engine
        .error_callback
        .as_ref()
        .expect("original should be restored")(StreamEvent::Done {
        response: "original".to_string(),
    });

    let original_events = original_events.lock().expect("lock").clone();
    let replacement_events = replacement_events.lock().expect("lock").clone();
    assert_eq!(original_events.len(), 1);
    assert_eq!(replacement_events.len(), 1);
    assert!(matches!(
        original_events.as_slice(),
        [StreamEvent::Done { response }] if response == "original"
    ));
    assert!(matches!(
        replacement_events.as_slice(),
        [StreamEvent::Done { response }] if response == "replacement"
    ));
}

#[test]
fn loop_engine_builder_debug_skips_error_callback() {
    let (callback, _) = stream_recorder();
    let builder = LoopEngine::builder().error_callback(callback);
    let debug = format!("{builder:?}");
    assert!(debug.contains("LoopEngineBuilder"));
    assert!(!debug.contains("error_callback"));
}

fn assert_done_event(events: &[StreamEvent], expected: &str) {
    assert!(matches!(events.last(), Some(StreamEvent::Done { response }) if response == expected));
}

fn tool_delta(id: &str, name: Option<&str>, arguments_delta: &str, done: bool) -> ToolUseDelta {
    ToolUseDelta {
        id: Some(id.to_string()),
        provider_id: None,
        name: name.map(ToString::to_string),
        arguments_delta: Some(arguments_delta.to_string()),
        arguments_done: done,
    }
}

fn single_tool_chunk(delta: ToolUseDelta, stop_reason: Option<&str>) -> StreamChunk {
    StreamChunk {
        delta_content: None,
        tool_use_deltas: vec![delta],
        usage: None,
        stop_reason: stop_reason.map(ToString::to_string),
    }
}

fn assert_tool_path(response: &CompletionResponse, id: &str, path: &str) {
    let call = response
        .tool_calls
        .iter()
        .find(|call| call.id == id)
        .expect("tool call exists");
    assert_eq!(call.arguments, serde_json::json!({"path": path}));
}

fn reason_perception(message: &str) -> ProcessedPerception {
    ProcessedPerception {
        user_message: message.to_string(),
        images: Vec::new(),
        documents: Vec::new(),
        context_window: vec![Message::user(message)],
        active_goals: vec!["reply".to_string()],
        budget_remaining: BudgetRemaining {
            llm_calls: 3,
            tool_invocations: 3,
            tokens: 100,
            cost_cents: 10,
            wall_time_ms: 1_000,
        },
        steer_context: None,
    }
}

async fn wait_for_cancel(token: &CancellationToken) {
    while !token.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn wait_for_delay_or_cancel(delay: Duration, cancel: Option<&CancellationToken>) {
    if let Some(token) = cancel {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = wait_for_cancel(token) => {}
        }
        return;
    }
    tokio::time::sleep(delay).await;
}

async fn run_cycle_with_inflight_command(command: LoopCommand) -> (LoopResult, usize) {
    let rounds = Arc::new(AtomicUsize::new(0));
    let executor = RoundCancellingToolExecutor::new(
        Duration::from_millis(120),
        Arc::clone(&rounds),
        usize::MAX,
    );
    let mut engine = engine_with_executor(Arc::new(executor), 4);
    let (sender, channel) = loop_input_channel();
    engine.set_input_channel(channel);
    let llm = ScriptedLlm::new(vec![
        tool_use_response("call-1"),
        tool_use_response("call-2"),
        text_response("done"),
    ]);

    let send_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        sender.send(command).expect("send command");
    });

    let result = engine
        .run_cycle(test_snapshot("read file"), &llm)
        .await
        .expect("run_cycle");
    send_task.await.expect("send task");
    (result, rounds.load(Ordering::SeqCst))
}

#[tokio::test]
async fn run_cycle_streaming_emits_text_and_done_events() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let llm = ScriptedLlm::new(vec![text_response("done")]);
    let (callback, events) = stream_recorder();

    let result = engine
        .run_cycle_streaming(test_snapshot("hello"), &llm, Some(callback))
        .await
        .expect("run_cycle_streaming");

    let response = match result {
        LoopResult::Complete { response, .. } => response,
        other => panic!("expected complete result, got {other:?}"),
    };
    let events = events.lock().expect("lock").clone();
    assert_eq!(response, "done");
    assert!(events.contains(&StreamEvent::PhaseChange {
        phase: Phase::Perceive,
    }));
    assert!(events.contains(&StreamEvent::PhaseChange {
        phase: Phase::Reason,
    }));
    assert!(events.contains(&StreamEvent::PhaseChange { phase: Phase::Act }));
    assert!(events.contains(&StreamEvent::TextDelta {
        text: "done".to_string(),
    }));
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::Progress { kind: ProgressKind::Researching, message }
            if message == "Researching the request and planning the next step..."
    )));
    assert!(matches!(events.last(), Some(StreamEvent::Done { response }) if response == "done"));
}

#[tokio::test]
async fn request_streaming_completion_suppresses_reason_text_when_tool_calls_present() {
    let engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let llm = ScriptedLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "I know which file to edit.".to_string(),
        }],
        tool_calls: vec![read_file_call("call-1")],
        usage: None,
        stop_reason: Some("tool_use".to_string()),
    }]);
    let (callback, events) = stream_recorder();

    let response = engine
        .request_streaming_completion(
            &llm,
            CompletionRequest {
                model: "scripted".to_string(),
                messages: vec![Message::user("fix it")],
                tools: vec![read_file_definition()],
                temperature: None,
                max_tokens: None,
                system_prompt: None,
                thinking: None,
            },
            StreamingRequestContext::new(
                "reason",
                StreamPhase::Reason,
                TextStreamVisibility::Public,
            ),
            &callback,
        )
        .await
        .expect("streaming completion");

    assert_eq!(response.tool_calls.len(), 1);
    let events = events.lock().expect("lock").clone();
    assert!(
        !events.iter().any(|event| matches!(
            event,
            StreamEvent::TextDelta { text } if text == "I know which file to edit."
        )),
        "streaming reason text should stay buffered when the final response contains tool calls"
    );
}

#[tokio::test]
async fn run_cycle_streaming_emits_tool_events_and_synthesize_phase() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    // Third response: outer loop continuation re-prompt returns text-only
    let llm = ScriptedLlm::new(vec![
        tool_use_response("call-1"),
        text_response("done"),
        text_response("done"),
    ]);
    let (callback, events) = stream_recorder();

    let result = engine
        .run_cycle_streaming(test_snapshot("read file"), &llm, Some(callback))
        .await
        .expect("run_cycle_streaming");

    let response = match result {
        LoopResult::Complete { response, .. } => response,
        other => panic!("expected complete result, got {other:?}"),
    };
    let events = events.lock().expect("lock").clone();
    assert_eq!(response, "done");
    assert!(events.contains(&StreamEvent::PhaseChange {
        phase: Phase::Synthesize,
    }));
    assert!(events.contains(&StreamEvent::ToolCallStart {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
    }));
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::ToolCallComplete { id, name, .. }
            if id == "call-1" && name == "read_file"
    )));
    assert!(events.contains(&StreamEvent::ToolResult {
        id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        output: "ok".to_string(),
        is_error: false,
    }));
    assert_done_event(&events, "done");
}

#[test]
fn progress_for_turn_state_prioritizes_artifact_gate() {
    let (kind, message) = progress_for_turn_state_with_profile(
        None,
        None,
        Some("/tmp/x.md"),
        &NoopToolExecutor,
        &TurnExecutionProfile::Standard,
        BoundedLocalPhase::Discovery,
    );

    assert_eq!(kind, ProgressKind::WritingArtifact);
    assert_eq!(message, "Writing the requested artifact to /tmp/x.md...");
}

#[test]
fn progress_for_turn_state_marks_mutation_commitment_as_implementing() {
    let commitment = TurnCommitment::ProceedUnderConstraints(ProceedUnderConstraints {
        goal: "Scaffold and implement the skill".to_string(),
        success_target: Some("Write the skill files locally".to_string()),
        unsupported_items: Vec::new(),
        assumptions: Vec::new(),
        allowed_tools: Some(ContinuationToolScope::MutationOnly),
    });

    let (kind, message) = progress_for_turn_state_with_profile(
        Some(&commitment),
        None,
        None,
        &NoopToolExecutor,
        &TurnExecutionProfile::Standard,
        BoundedLocalPhase::Discovery,
    );

    assert_eq!(kind, ProgressKind::Implementing);
    assert_eq!(
        message,
        "Implementing the committed plan: Write the skill files locally"
    );
}

#[test]
fn progress_for_tool_round_describes_specific_workspace_search_activity() {
    let calls = vec![ToolCall {
        id: "call-1".to_string(),
        name: "search_text".to_string(),
        arguments: serde_json::json!({
            "pattern": "x-post",
            "path": "skills/"
        }),
    }];

    let (kind, message) = progress_for_tool_round(
        progress::ToolRoundProgressContext {
            commitment: None,
            pending_tool_scope: None,
            pending_artifact_write_target: None,
            turn_execution_profile: &TurnExecutionProfile::Standard,
            bounded_local_phase: BoundedLocalPhase::Discovery,
            tool_executor: &NoopToolExecutor,
        },
        &calls,
    )
    .expect("tool round progress");

    assert_eq!(kind, ProgressKind::Researching);
    assert_eq!(message, "Searching skills for x-post");
}

#[test]
fn activity_progress_expires_back_to_turn_state() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (callback, events) = stream_recorder();
    let stream = CycleStream::enabled(&callback);
    let calls = vec![ToolCall {
        id: "call-1".to_string(),
        name: "search_text".to_string(),
        arguments: serde_json::json!({
            "pattern": "x-post",
            "path": "skills/"
        }),
    }];

    engine.maybe_publish_reason_progress(stream);
    engine.maybe_publish_tool_round_progress(3, &calls, stream);
    engine.expire_activity_progress(stream);

    let events = events.lock().expect("lock").clone();
    let progress: Vec<(ProgressKind, String)> = events
        .into_iter()
        .filter_map(|event| match event {
            StreamEvent::Progress { kind, message } => Some((kind, message)),
            _ => None,
        })
        .collect();

    assert_eq!(
        progress,
        vec![
            (
                ProgressKind::Researching,
                "Researching the request and planning the next step...".to_string()
            ),
            (
                ProgressKind::Researching,
                "Searching skills for x-post".to_string()
            ),
            (
                ProgressKind::Researching,
                "Researching the request and planning the next step...".to_string()
            ),
        ]
    );
}

#[test]
fn bounded_local_phase_change_refreshes_turn_state_progress_before_activity_expires() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Discovery;
    let (callback, events) = stream_recorder();
    let stream = CycleStream::enabled(&callback);

    engine.maybe_publish_reason_progress(stream);
    engine.publish_activity_progress(
        ProgressKind::Researching,
        "Searching the local workspace...",
        stream,
    );

    let discovery_call = ToolCall {
        id: "d1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path": "src/lib.rs"}),
    };
    let discovery_result = ToolResult {
        tool_call_id: "d1".to_string(),
        tool_name: "read_file".to_string(),
        success: true,
        output: "ok".to_string(),
    };

    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&discovery_call),
        std::slice::from_ref(&discovery_result),
    );
    engine.expire_activity_progress(stream);

    let events = events.lock().expect("lock").clone();
    let progress: Vec<(ProgressKind, String)> = events
        .into_iter()
        .filter_map(|event| match event {
            StreamEvent::Progress { kind, message } => Some((kind, message)),
            _ => None,
        })
        .collect();

    assert_eq!(
        progress.last(),
        Some(&(
            ProgressKind::Implementing,
            "Applying the local code change...".to_string()
        ))
    );
}

#[tokio::test]
async fn run_cycle_streaming_hides_internal_tool_synthesis_until_root_completion() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let llm = ScriptedLlm::new(vec![
        tool_use_response("call-1"),
        text_response("Internal tool synthesis"),
        text_response("Final root answer"),
    ]);
    let (callback, events) = stream_recorder();

    let result = engine
        .run_cycle_streaming(test_snapshot("read file"), &llm, Some(callback))
        .await
        .expect("run_cycle_streaming");

    let response = match result {
        LoopResult::Complete { response, .. } => response,
        other => panic!("expected complete result, got {other:?}"),
    };
    let events = events.lock().expect("lock").clone();

    assert_eq!(response, "Final root answer");
    assert!(
        !events.iter().any(|event| matches!(
            event,
            StreamEvent::TextDelta { text } if text == "Internal tool synthesis"
        )),
        "intermediate tool synthesis should remain internal"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::TextDelta { text } if text == "Final root answer"
    )));
    assert_done_event(&events, "Final root answer");
}

#[test]
fn finish_streaming_result_emits_notification_for_multi_iteration_completion_without_notify() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (callback, events) = stream_recorder();

    let result = engine.finish_streaming_result(
        LoopResult::Complete {
            response: "done".to_string(),
            iterations: 2,
            tokens_used: TokenUsage::default(),
            signals: Vec::new(),
        },
        CycleStream::enabled(&callback),
    );

    let response = match result {
        LoopResult::Complete { response, .. } => response,
        other => panic!("expected complete result, got {other:?}"),
    };
    let events = events.lock().expect("lock").clone();

    assert_eq!(response, "done");
    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamEvent::Notification { title, body }
                if title == "Fawx" && body == "Task complete (2 steps)"
        )
    }));
    assert_done_event(&events, "done");
}

#[test]
fn finish_streaming_result_skips_notification_when_notify_tool_already_ran() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    engine.notify_called_this_cycle = true;
    let (callback, events) = stream_recorder();

    let _ = engine.finish_streaming_result(
        LoopResult::Complete {
            response: "done".to_string(),
            iterations: 2,
            tokens_used: TokenUsage::default(),
            signals: Vec::new(),
        },
        CycleStream::enabled(&callback),
    );

    let events = events.lock().expect("lock").clone();
    assert!(!events
        .iter()
        .any(|event| matches!(event, StreamEvent::Notification { .. })));
    assert_done_event(&events, "done");
}

#[test]
fn finish_streaming_result_skips_notification_for_single_iteration_completion() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (callback, events) = stream_recorder();

    let _ = engine.finish_streaming_result(
        LoopResult::Complete {
            response: "done".to_string(),
            iterations: 1,
            tokens_used: TokenUsage::default(),
            signals: Vec::new(),
        },
        CycleStream::enabled(&callback),
    );

    let events = events.lock().expect("lock").clone();
    assert!(!events
        .iter()
        .any(|event| matches!(event, StreamEvent::Notification { .. })));
    assert_done_event(&events, "done");
}

#[test]
fn finish_streaming_result_uses_polished_incomplete_fallback_when_no_partial_exists() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (callback, events) = stream_recorder();

    let _ = engine.finish_streaming_result(
        LoopResult::Incomplete {
            partial_response: None,
            reason: "iteration limit reached before a usable final response was produced"
                .to_string(),
            iterations: 2,
            signals: Vec::new(),
        },
        CycleStream::enabled(&callback),
    );

    let events = events.lock().expect("lock").clone();
    assert_done_event(&events, INCOMPLETE_FALLBACK_RESPONSE);
}

#[tokio::test]
async fn run_cycle_streaming_emits_done_when_budget_exhausted() {
    // With single-pass loop, zero budget triggers BudgetExhausted
    // immediately (before perceive), so partial_response is None.
    let zero_budget = crate::budget::BudgetConfig {
        max_llm_calls: 0,
        max_tool_invocations: 0,
        max_tokens: 0,
        max_cost_cents: 0,
        max_wall_time_ms: 60_000,
        max_recursion_depth: 0,
        decompose_depth_mode: DepthMode::Adaptive,
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(zero_budget, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(NoopToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");
    let llm = ScriptedLlm::new(vec![text_response("hello")]);
    let (callback, events) = stream_recorder();

    let result = engine
        .run_cycle_streaming(test_snapshot("hello"), &llm, Some(callback))
        .await
        .expect("run_cycle_streaming");

    match result {
        LoopResult::BudgetExhausted {
            partial_response,
            iterations,
            ..
        } => {
            // With single-pass and zero budget, budget_terminal fires
            // before perceive — no LLM call happens, so no partial response.
            assert!(
                partial_response.is_none()
                    || partial_response.as_deref() == Some(BUDGET_EXHAUSTED_FALLBACK_RESPONSE),
                "expected None or fallback, got: {partial_response:?}"
            );
            assert_eq!(iterations, 1);
        }
        other => panic!("expected BudgetExhausted, got: {other:?}"),
    }
    let events = events.lock().expect("lock").clone();
    assert!(
        events.iter().any(|e| matches!(e, StreamEvent::Done { .. })),
        "should emit a Done event"
    );
}

#[tokio::test]
async fn run_cycle_streaming_emits_done_when_user_stopped() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (sender, channel) = loop_input_channel();
    engine.set_input_channel(channel);
    sender.send(LoopCommand::Stop).expect("send Stop");
    let llm = ScriptedLlm::new(vec![text_response("hello")]);
    let (callback, events) = stream_recorder();

    let result = engine
        .run_cycle_streaming(test_snapshot("hello"), &llm, Some(callback))
        .await
        .expect("run_cycle_streaming");

    assert!(matches!(result, LoopResult::UserStopped { .. }));
    let events = events.lock().expect("lock").clone();
    assert_done_event(&events, "user stopped");
}

#[test]
fn check_user_input_priority_order_is_abort_stop_wait_resume_status_steer() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (sender, channel) = loop_input_channel();
    engine.set_input_channel(channel);

    sender
        .send(LoopCommand::Steer("first".to_string()))
        .expect("steer");
    sender.send(LoopCommand::StatusQuery).expect("status");
    sender.send(LoopCommand::Wait).expect("wait");
    sender.send(LoopCommand::Resume).expect("resume");
    sender.send(LoopCommand::Stop).expect("stop");
    sender.send(LoopCommand::Abort).expect("abort");

    assert_eq!(engine.check_user_input(), Some(LoopCommand::Abort));
}

#[test]
fn check_user_input_prioritizes_stop_over_wait_resume() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (sender, channel) = loop_input_channel();
    engine.set_input_channel(channel);

    sender.send(LoopCommand::Wait).expect("wait");
    sender.send(LoopCommand::Resume).expect("resume");
    sender.send(LoopCommand::Stop).expect("stop");

    assert_eq!(engine.check_user_input(), Some(LoopCommand::Stop));
}

#[test]
fn check_user_input_keeps_latest_wait_resume_when_no_stop_or_abort() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (sender, channel) = loop_input_channel();
    engine.set_input_channel(channel);

    sender.send(LoopCommand::Wait).expect("wait");
    sender.send(LoopCommand::Resume).expect("resume");

    assert_eq!(engine.check_user_input(), Some(LoopCommand::Resume));
}

#[test]
fn status_query_publishes_system_status_without_altering_flow() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let bus = fx_core::EventBus::new(4);
    let mut receiver = bus.subscribe();
    engine.set_event_bus(bus);

    let (sender, channel) = loop_input_channel();
    engine.set_input_channel(channel);
    sender.send(LoopCommand::StatusQuery).expect("status");

    assert_eq!(engine.check_user_input(), None);
    let event = receiver.try_recv().expect("status event");
    assert!(matches!(event, InternalMessage::SystemStatus { .. }));
}

#[test]
fn format_system_status_message_matches_spec_template() {
    let status = LoopStatus {
        iteration_count: 2,
        max_iterations: 7,
        llm_calls_used: 3,
        tool_invocations_used: 5,
        tokens_used: 144,
        cost_cents_used: 11,
        remaining: BudgetRemaining {
            llm_calls: 4,
            tool_invocations: 6,
            tokens: 856,
            cost_cents: 89,
            wall_time_ms: 12_000,
        },
    };

    assert_eq!(
            format_system_status_message(&status),
            "status: iter=2/7 llm=3 tools=5 tokens=144 cost_cents=11 remaining(llm=4,tools=6,tokens=856,cost_cents=89)"
        );
}

#[tokio::test]
async fn steer_dedups_and_applies_latest_value_in_perceive_window() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (sender, channel) = loop_input_channel();
    engine.set_input_channel(channel);

    sender
        .send(LoopCommand::Steer("earlier".to_string()))
        .expect("steer");
    sender
        .send(LoopCommand::Steer("latest".to_string()))
        .expect("steer");

    assert_eq!(engine.check_user_input(), None);

    let processed = engine
        .perceive(&test_snapshot("hello"))
        .await
        .expect("perceive");
    assert_eq!(processed.steer_context.as_deref(), Some("latest"));

    let next = engine
        .perceive(&test_snapshot("hello again"))
        .await
        .expect("perceive");
    assert_eq!(next.steer_context, None);
}

#[test]
fn reasoning_user_prompt_includes_steer_context() {
    let perception = ProcessedPerception {
        user_message: "hello".to_string(),
        images: Vec::new(),
        documents: Vec::new(),
        context_window: vec![Message::user("hello")],
        active_goals: vec!["reply".to_string()],
        budget_remaining: BudgetRemaining {
            llm_calls: 3,
            tool_invocations: 3,
            tokens: 100,
            cost_cents: 1,
            wall_time_ms: 100,
        },
        steer_context: Some("be concise".to_string()),
    };

    let prompt = reasoning_user_prompt(&perception);
    assert!(prompt.contains("User steer (latest): be concise"));
}

#[test]
fn check_cancellation_without_token_or_input_returns_none() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    assert!(engine.check_cancellation(None).is_none());
}

#[tokio::test]
async fn consume_stream_with_events_publishes_delta_events() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let bus = fx_core::EventBus::new(8);
    let mut receiver = bus.subscribe();
    engine.set_event_bus(bus);

    let mut stream: CompletionStream = Box::pin(futures_util::stream::iter(vec![
        Ok(StreamChunk {
            delta_content: Some("Hel".to_string()),
            tool_use_deltas: Vec::new(),
            usage: None,
            stop_reason: None,
        }),
        Ok(StreamChunk {
            delta_content: Some("lo".to_string()),
            tool_use_deltas: Vec::new(),
            usage: None,
            stop_reason: Some("stop".to_string()),
        }),
    ]));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Reason,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert_eq!(extract_response_text(&response), "Hello");
    assert_eq!(response.stop_reason.as_deref(), Some("stop"));

    let first = receiver.try_recv().expect("first delta");
    let second = receiver.try_recv().expect("second delta");
    assert!(matches!(
        first,
        InternalMessage::StreamDelta { delta, phase }
            if delta == "Hel" && phase == StreamPhase::Reason
    ));
    assert!(matches!(
        second,
        InternalMessage::StreamDelta { delta, phase }
            if delta == "lo" && phase == StreamPhase::Reason
    ));
}

#[tokio::test]
async fn consume_stream_with_events_assembles_tool_calls_from_deltas() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let mut stream: CompletionStream = Box::pin(futures_util::stream::iter(vec![
        Ok(StreamChunk {
            delta_content: None,
            tool_use_deltas: vec![ToolUseDelta {
                id: Some("call-1".to_string()),
                provider_id: None,
                name: Some("read_file".to_string()),
                arguments_delta: Some("{\"path\":\"READ".to_string()),
                arguments_done: false,
            }],
            usage: None,
            stop_reason: None,
        }),
        Ok(StreamChunk {
            delta_content: None,
            tool_use_deltas: vec![ToolUseDelta {
                id: Some("call-1".to_string()),
                provider_id: None,
                name: None,
                arguments_delta: Some("ME.md\"}".to_string()),
                arguments_done: true,
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }),
    ]));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Synthesize,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "call-1");
    assert_eq!(response.tool_calls[0].name, "read_file");
    assert_eq!(
        response.tool_calls[0].arguments,
        serde_json::json!({"path":"README.md"})
    );
}

#[tokio::test]
async fn consume_stream_with_events_suppresses_synthesize_deltas_when_tool_calls_present() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let bus = fx_core::EventBus::new(8);
    let mut receiver = bus.subscribe();
    engine.set_event_bus(bus);

    let mut stream: CompletionStream =
        Box::pin(futures_util::stream::iter(vec![Ok(StreamChunk {
            delta_content: Some("[web_search]".to_string()),
            tool_use_deltas: vec![ToolUseDelta {
                id: Some("call-1".to_string()),
                provider_id: None,
                name: Some("web_search".to_string()),
                arguments_delta: Some(r#"{"query":"x api"}"#.to_string()),
                arguments_done: true,
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        })]));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Synthesize,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert_eq!(response.tool_calls.len(), 1);

    let events: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
    assert!(
            !events.iter().any(|event| matches!(
                event,
                InternalMessage::StreamDelta { phase, .. } if *phase == StreamPhase::Synthesize
            )),
            "synthesize stream should not publish text deltas when the final response contains tool calls"
        );
}

#[tokio::test]
async fn consume_stream_with_events_suppresses_reason_deltas_when_tool_calls_present() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let bus = fx_core::EventBus::new(8);
    let mut receiver = bus.subscribe();
    engine.set_event_bus(bus);

    let mut stream: CompletionStream =
        Box::pin(futures_util::stream::iter(vec![Ok(StreamChunk {
            delta_content: Some("I'll inspect the repo first.".to_string()),
            tool_use_deltas: vec![ToolUseDelta {
                id: Some("call-1".to_string()),
                provider_id: None,
                name: Some("read_file".to_string()),
                arguments_delta: Some(r#"{"path":"README.md"}"#.to_string()),
                arguments_done: true,
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        })]));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Reason,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert_eq!(response.tool_calls.len(), 1);

    let events: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
    assert!(
        !events.iter().any(|event| matches!(
            event,
            InternalMessage::StreamDelta { phase, .. } if *phase == StreamPhase::Reason
        )),
        "reason stream should not publish text deltas when the final response contains tool calls"
    );
}

#[tokio::test]
async fn consume_stream_with_events_preserves_provider_ids_in_content() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let mut stream: CompletionStream =
        Box::pin(futures_util::stream::iter(vec![Ok(StreamChunk {
            delta_content: None,
            tool_use_deltas: vec![ToolUseDelta {
                id: Some("call-1".to_string()),
                provider_id: Some("fc-1".to_string()),
                name: Some("read_file".to_string()),
                arguments_delta: Some(r#"{"path":"README.md"}"#.to_string()),
                arguments_done: true,
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        })]));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Synthesize,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert!(matches!(
        response.content.as_slice(),
        [ContentBlock::ToolUse {
            id,
            provider_id: Some(provider_id),
            name,
            input,
        }] if id == "call-1"
            && provider_id == "fc-1"
            && name == "read_file"
            && input == &serde_json::json!({"path":"README.md"})
    ));
}

#[tokio::test]
async fn consume_stream_with_events_promotes_call_id_over_provider_id() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let mut stream: CompletionStream = Box::pin(futures_util::stream::iter(vec![
        Ok(StreamChunk {
            delta_content: None,
            tool_use_deltas: vec![ToolUseDelta {
                id: Some("fc-123".to_string()),
                provider_id: Some("fc-123".to_string()),
                name: Some("weather".to_string()),
                arguments_delta: Some(r#"{"location":"Denver, CO"}"#.to_string()),
                arguments_done: false,
            }],
            usage: None,
            stop_reason: None,
        }),
        Ok(StreamChunk {
            delta_content: None,
            tool_use_deltas: vec![ToolUseDelta {
                id: Some("call-123".to_string()),
                provider_id: Some("fc-123".to_string()),
                name: None,
                arguments_delta: None,
                arguments_done: true,
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }),
    ]));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Synthesize,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert_eq!(response.tool_calls[0].id, "call-123");
    assert!(matches!(
        response.content.as_slice(),
        [ContentBlock::ToolUse {
            id,
            provider_id: Some(provider_id),
            ..
        }] if id == "call-123" && provider_id == "fc-123"
    ));
}

#[tokio::test]
async fn consume_stream_with_events_keeps_distinct_calls_when_new_id_reuses_chunk_index_zero() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let chunks = vec![
        Ok(single_tool_chunk(
            tool_delta("call-1", Some("read_file"), "{\"path\":\"alpha.md\"}", true),
            None,
        )),
        Ok(single_tool_chunk(
            tool_delta("call-2", Some("read_file"), "{\"path\":\"beta.md\"}", true),
            Some("tool_use"),
        )),
    ];
    let mut stream: CompletionStream = Box::pin(futures_util::stream::iter(chunks));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Synthesize,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert_eq!(response.tool_calls.len(), 2);
    assert_tool_path(&response, "call-1", "alpha.md");
    assert_tool_path(&response, "call-2", "beta.md");
}

#[tokio::test]
async fn consume_stream_with_events_supports_multi_tool_ids_across_chunks_same_local_index() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let chunks = vec![
        Ok(single_tool_chunk(
            tool_delta("call-1", Some("read_file"), "{\"path\":\"al", false),
            None,
        )),
        Ok(single_tool_chunk(
            tool_delta("call-2", Some("read_file"), "{\"path\":\"be", false),
            None,
        )),
        Ok(single_tool_chunk(
            tool_delta("call-1", None, "pha.md\"}", true),
            None,
        )),
        Ok(single_tool_chunk(
            tool_delta("call-2", None, "ta.md\"}", true),
            Some("tool_use"),
        )),
    ];
    let mut stream: CompletionStream = Box::pin(futures_util::stream::iter(chunks));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Synthesize,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert_eq!(response.tool_calls.len(), 2);
    assert_tool_path(&response, "call-1", "alpha.md");
    assert_tool_path(&response, "call-2", "beta.md");
}

#[tokio::test]
async fn consume_stream_with_events_replaces_partial_args_with_done_payload() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let chunks = vec![
        Ok(single_tool_chunk(
            tool_delta("call-1", Some("read_file"), "{\"path\":\"READ", false),
            None,
        )),
        Ok(single_tool_chunk(
            tool_delta("call-1", None, "ME.md\"}", false),
            None,
        )),
        Ok(single_tool_chunk(
            tool_delta("call-1", None, "{\"path\":\"README.md\"}", true),
            Some("tool_use"),
        )),
    ];
    let mut stream: CompletionStream = Box::pin(futures_util::stream::iter(chunks));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Synthesize,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert_eq!(response.tool_calls.len(), 1);
    assert_tool_path(&response, "call-1", "README.md");
}

#[tokio::test]
async fn reason_stream_error_after_partial_delta_emits_streaming_finished_once() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let bus = fx_core::EventBus::new(8);
    let mut receiver = bus.subscribe();
    engine.set_event_bus(bus);

    let error = engine
        .reason(
            &reason_perception("hello"),
            &PartialErrorStreamLlm,
            CycleStream::disabled(),
        )
        .await
        .expect_err("stream should fail");
    assert!(error.reason.contains("stream consumption failed"));

    let mut events = Vec::with_capacity(3);
    while events.len() < 3 {
        let event = receiver.recv().await.expect("event");
        if matches!(
            event,
            InternalMessage::StreamingStarted { .. }
                | InternalMessage::StreamDelta { .. }
                | InternalMessage::StreamingFinished { .. }
        ) {
            events.push(event);
        }
    }
    let started = &events[0];
    let delta = &events[1];
    let finished = &events[2];
    assert!(matches!(
        started,
        InternalMessage::StreamingStarted { phase } if *phase == StreamPhase::Reason
    ));
    assert!(matches!(
        delta,
        InternalMessage::StreamDelta { delta, phase }
            if delta == "partial" && *phase == StreamPhase::Reason
    ));
    assert!(matches!(
        finished,
        InternalMessage::StreamingFinished { phase } if *phase == StreamPhase::Reason
    ));
    assert!(
        receiver.try_recv().is_err(),
        "finished should be emitted once"
    );
}

#[tokio::test]
async fn reason_does_not_publish_stream_events_when_buffered_stream_setup_fails() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let bus = fx_core::EventBus::new(8);
    let mut receiver = bus.subscribe();
    engine.set_event_bus(bus);

    let error = engine
        .reason(
            &reason_perception("hello"),
            &FailingBufferedStreamLlm,
            CycleStream::disabled(),
        )
        .await
        .expect_err("stream setup should fail");
    assert!(error.reason.contains("completion failed"));
    while let Ok(event) = receiver.try_recv() {
        assert!(
            !matches!(
                event,
                InternalMessage::StreamingStarted { .. }
                    | InternalMessage::StreamDelta { .. }
                    | InternalMessage::StreamingFinished { .. }
            ),
            "no stream events expected"
        );
    }
}

#[tokio::test]
async fn reason_emits_background_error_on_buffered_stream_setup_failure() {
    let (callback, events) = stream_recorder();
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    engine.error_callback = Some(callback);

    let error = engine
        .reason(
            &reason_perception("hello"),
            &FailingBufferedStreamLlm,
            CycleStream::disabled(),
        )
        .await
        .expect_err("stream setup should fail");
    assert!(error.reason.contains("completion failed"));

    let events = events.lock().expect("lock").clone();
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::Error {
            category: ErrorCategory::Provider,
            message,
            recoverable: false,
        } if message == "LLM request failed: provider error: simulated stream setup failure"
    )));
}

#[tokio::test]
async fn reason_emits_stream_error_on_streaming_provider_failure() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let (callback, events) = stream_recorder();

    let error = engine
        .reason(
            &reason_perception("hello"),
            &FailingStreamingLlm,
            CycleStream::enabled(&callback),
        )
        .await
        .expect_err("streaming request should fail");
    assert!(error.reason.contains("completion failed"));

    let events = events.lock().expect("lock").clone();
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::Error {
            category: ErrorCategory::Provider,
            message,
            recoverable: false,
        } if message == "LLM streaming failed: provider error: simulated streaming failure"
    )));
}

#[tokio::test]
async fn execute_tool_calls_emits_stream_error_on_executor_failure() {
    #[derive(Debug)]
    struct LocalFailingExecutor;

    #[async_trait]
    impl ToolExecutor for LocalFailingExecutor {
        async fn execute_tools(
            &self,
            _calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            Err(crate::act::ToolExecutorError {
                message: "tool crashed".to_string(),
                recoverable: true,
            })
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![read_file_definition()]
        }
    }

    let mut engine = engine_with_executor(Arc::new(LocalFailingExecutor), 3);
    let (callback, events) = stream_recorder();
    let calls = vec![read_file_call("call-1")];

    let error = engine
        .execute_tool_calls_with_stream(&calls, CycleStream::enabled(&callback))
        .await
        .expect_err("tool execution should fail");
    assert!(error.reason.contains("tool execution failed: tool crashed"));

    let events = events.lock().expect("lock").clone();
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::Error {
            category: ErrorCategory::ToolExecution,
            message,
            recoverable: true,
        } if message == "Tool 'read_file' failed: tool crashed"
    )));
}

#[tokio::test]
async fn execute_tool_calls_emits_stream_error_when_retry_budget_blocks_tool() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    engine.budget = BudgetTracker::new(
        crate::budget::BudgetConfig {
            max_consecutive_failures: 1,
            max_tool_retries: 0,
            ..crate::budget::BudgetConfig::default()
        },
        0,
        0,
    );
    engine
        .tool_retry_tracker
        .record_result(&read_file_call("seed"), false);
    let (callback, events) = stream_recorder();
    let calls = vec![read_file_call("call-1")];

    let _ = engine
        .execute_tool_calls_with_stream(&calls, CycleStream::enabled(&callback))
        .await
        .expect("blocked tool call should return synthetic result");
    let events = events.lock().expect("lock").clone();
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::Error {
            category: ErrorCategory::ToolExecution,
            message,
            recoverable: true,
        } if message
            == &blocked_tool_message("read_file", &same_call_failure_reason(1))
    )));
}

#[tokio::test]
async fn consume_stream_with_events_sets_cancelled_stop_reason_on_mid_stream_cancel() {
    let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
    let token = CancellationToken::new();
    engine.set_cancel_token(token.clone());

    let cancel_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        token.cancel();
    });

    let stream_values = vec![
        StreamChunk {
            delta_content: Some("first".to_string()),
            tool_use_deltas: Vec::new(),
            usage: None,
            stop_reason: None,
        },
        StreamChunk {
            delta_content: Some("second".to_string()),
            tool_use_deltas: Vec::new(),
            usage: None,
            stop_reason: Some("stop".to_string()),
        },
    ];
    let delayed =
        futures_util::stream::iter(stream_values)
            .enumerate()
            .then(|(index, chunk)| async move {
                if index == 1 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Ok::<StreamChunk, ProviderError>(chunk)
            });
    let mut stream: CompletionStream = Box::pin(delayed);

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Reason,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");
    cancel_task.await.expect("cancel task");

    assert_eq!(extract_response_text(&response), "first");
    assert_eq!(response.stop_reason.as_deref(), Some("cancelled"));
    assert!(response.tool_calls.is_empty());
}

#[test]
fn response_to_chunk_converts_completion_response() {
    let response = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "hello".to_string(),
        }],
        tool_calls: vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }],
        usage: Some(Usage {
            input_tokens: 3,
            output_tokens: 2,
        }),
        stop_reason: Some("stop".to_string()),
    };

    let chunk = response_to_chunk(response);
    assert_eq!(chunk.delta_content.as_deref(), Some("hello"));
    assert_eq!(chunk.stop_reason.as_deref(), Some("stop"));
    assert_eq!(
        chunk.usage,
        Some(Usage {
            input_tokens: 3,
            output_tokens: 2,
        })
    );
    assert_eq!(chunk.tool_use_deltas.len(), 1);
    assert_eq!(chunk.tool_use_deltas[0].id.as_deref(), Some("call-1"));
    assert_eq!(chunk.tool_use_deltas[0].name.as_deref(), Some("read_file"));
    assert_eq!(
        chunk.tool_use_deltas[0].arguments_delta.as_deref(),
        Some("{\"path\":\"README.md\"}")
    );
    assert!(chunk.tool_use_deltas[0].arguments_done);
}

#[tokio::test]
async fn cancellation_during_delayed_tool_execution_returns_user_stopped_quickly() {
    let token = CancellationToken::new();
    let mut engine = engine_with_executor(
        Arc::new(DelayedToolExecutor::new(Duration::from_secs(5))),
        4,
    );
    engine.set_cancel_token(token.clone());
    let llm = ScriptedLlm::new(vec![tool_use_response("call-1")]);

    let cancel_task = tokio::spawn({
        let token = token.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            token.cancel();
        }
    });

    let started = Instant::now();
    let result = engine
        .run_cycle(test_snapshot("read file"), &llm)
        .await
        .expect("run_cycle");
    cancel_task.await.expect("cancel task");

    assert!(
        matches!(result, LoopResult::UserStopped { .. }),
        "expected UserStopped, got: {result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "cancellation should return quickly"
    );
}

#[tokio::test]
async fn cancellation_between_tool_continuation_rounds_returns_user_stopped() {
    let token = CancellationToken::new();
    let rounds = Arc::new(AtomicUsize::new(0));
    let executor =
        RoundCancellingToolExecutor::new(Duration::from_millis(20), Arc::clone(&rounds), 1);
    let mut engine = engine_with_executor(Arc::new(executor), 4);
    engine.set_cancel_token(token);

    let llm = ScriptedLlm::new(vec![
        tool_use_response("call-1"),
        tool_use_response("call-2"),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read files"), &llm)
        .await
        .expect("run_cycle");

    assert!(
        matches!(result, LoopResult::UserStopped { .. }),
        "expected UserStopped, got: {result:?}"
    );
    assert_eq!(
        rounds.load(Ordering::SeqCst),
        1,
        "cancellation should stop before the second tool round executes"
    );
}

#[tokio::test]
async fn stop_command_sent_during_tool_round_is_caught_at_iteration_boundary() {
    let (result, rounds) = run_cycle_with_inflight_command(LoopCommand::Stop).await;
    assert!(
        matches!(result, LoopResult::UserStopped { .. }),
        "expected UserStopped for Stop, got: {result:?}"
    );
    assert_eq!(
        rounds, 1,
        "Stop should be caught before the second tool round executes"
    );
}

#[tokio::test]
async fn abort_command_sent_during_tool_round_is_caught_at_iteration_boundary() {
    let (result, rounds) = run_cycle_with_inflight_command(LoopCommand::Abort).await;
    assert!(
        matches!(result, LoopResult::UserStopped { .. }),
        "expected UserStopped for Abort, got: {result:?}"
    );
    assert_eq!(
        rounds, 1,
        "Abort should be caught before the second tool round executes"
    );
}
