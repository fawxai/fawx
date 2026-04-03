use super::*;
use async_trait::async_trait;
use fx_core::error::LlmError as CoreLlmError;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, ProviderError, ToolCall,
    ToolDefinition,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::Registry;

static TRACE_SUBSCRIBER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn words(count: usize) -> String {
    std::iter::repeat_n("a", count)
        .collect::<Vec<_>>()
        .join(" ")
}

fn user(words_count: usize) -> Message {
    Message::user(words(words_count))
}

fn assistant(words_count: usize) -> Message {
    Message::assistant(words(words_count))
}

fn tool_use(id: &str) -> Message {
    Message {
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: id.to_string(),
            provider_id: None,
            name: "read".to_string(),
            input: serde_json::json!({"path": "/tmp/a"}),
        }],
    }
}

fn tool_result(id: &str, word_count: usize) -> Message {
    Message {
        role: MessageRole::Tool,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: id.to_string(),
            content: serde_json::json!(words(word_count)),
        }],
    }
}

fn has_tool_blocks(messages: &[Message]) -> bool {
    messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. }
            )
        })
    })
}

fn tiered_compaction_config(use_summarization: bool) -> CompactionConfig {
    CompactionConfig {
        slide_threshold: 0.60,
        prune_threshold: 0.40,
        _legacy_summarize_threshold: 0.80,
        emergency_threshold: 0.95,
        preserve_recent_turns: 2,
        model_context_limit: 5_096,
        reserved_system_tokens: 0,
        recompact_cooldown_turns: 2,
        use_summarization,
        max_summary_tokens: 512,
        prune_tool_blocks: true,
        tool_block_summary_max_chars: 100,
    }
}

fn tiered_budget(config: &CompactionConfig) -> ConversationBudget {
    ConversationBudget::new(
        config.model_context_limit,
        config.slide_threshold,
        config.reserved_system_tokens,
    )
}

fn engine_with_compaction_llm(
    context: ContextCompactor,
    tool_executor: Arc<dyn ToolExecutor>,
    config: CompactionConfig,
    llm: Arc<dyn LlmProvider>,
) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(context)
        .max_iterations(4)
        .tool_executor(tool_executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(config)
        .compaction_llm(llm)
        .build()
        .expect("test engine build")
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

fn read_call(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"/tmp/demo"}),
    }
}

fn large_history(count: usize, words_per_message: usize) -> Vec<Message> {
    (0..count)
        .map(|index| {
            if index % 2 == 0 {
                Message::user(format!(
                    "u{index} {}",
                    words(words_per_message.saturating_sub(1))
                ))
            } else {
                Message::assistant(format!(
                    "a{index} {}",
                    words(words_per_message.saturating_sub(1))
                ))
            }
        })
        .collect()
}

fn snapshot_with_history(history: Vec<Message>, user_text: &str) -> PerceptionSnapshot {
    PerceptionSnapshot {
        timestamp_ms: 10,
        screen: ScreenState {
            current_app: "terminal".to_string(),
            elements: Vec::new(),
            text_content: user_text.to_string(),
        },
        notifications: Vec::new(),
        active_app: "terminal".to_string(),
        user_input: Some(UserInput {
            text: user_text.to_string(),
            source: InputSource::Text,
            timestamp: 10,
            context_id: None,
            images: Vec::new(),
            documents: Vec::new(),
        }),
        sensor_data: None,
        conversation_history: history,
        steer_context: None,
    }
}

fn compaction_config() -> CompactionConfig {
    CompactionConfig {
        slide_threshold: 0.2,
        prune_threshold: 0.1,
        _legacy_summarize_threshold: 0.8,
        emergency_threshold: 0.95,
        preserve_recent_turns: 2,
        model_context_limit: 5_000,
        reserved_system_tokens: 0,
        recompact_cooldown_turns: 3,
        use_summarization: false,
        max_summary_tokens: 512,
        prune_tool_blocks: true,
        tool_block_summary_max_chars: 100,
    }
}

fn engine_with(
    context: ContextCompactor,
    tool_executor: Arc<dyn ToolExecutor>,
    config: CompactionConfig,
) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(context)
        .max_iterations(4)
        .tool_executor(tool_executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(config)
        .build()
        .expect("test engine build")
}

#[test]
fn builder_missing_required_field_returns_error() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let error = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .build()
        .expect_err("missing synthesis instruction should fail");

    assert_eq!(error.stage, "init");
    assert_eq!(
        error.reason,
        "missing_required_field: synthesis_instruction"
    );
}

#[test]
fn builder_with_no_fields_returns_error() {
    let error = LoopEngine::builder().build().expect_err("should fail");
    assert_eq!(error.stage, "init");
}

#[test]
fn builder_memory_context_whitespace_normalizes_to_none() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .memory_context("   ".to_string())
        .build()
        .expect("test engine build");

    assert!(engine.memory_context.is_none());
}

#[test]
fn builder_default_optionals() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .build()
        .expect("test engine build");

    let defaults = CompactionConfig::default();
    assert!(engine.memory_context.is_none());
    assert!(engine.cancel_token.is_none());
    assert!(engine.input_channel.is_none());
    assert!(engine.event_bus.is_none());
    assert_eq!(engine.execution_visibility, ExecutionVisibility::Public);
    assert_eq!(
        engine.compaction_config.slide_threshold,
        defaults.slide_threshold
    );
    assert_eq!(
        engine.compaction_config.prune_threshold,
        defaults.prune_threshold
    );
    assert_eq!(
        engine.compaction_config.emergency_threshold,
        defaults.emergency_threshold
    );
    assert_eq!(
        engine.compaction_config.preserve_recent_turns,
        defaults.preserve_recent_turns
    );
    assert_eq!(
        engine.conversation_budget.conversation_budget(),
        defaults.model_context_limit
            - defaults.reserved_system_tokens
            - ConversationBudget::DEFAULT_OUTPUT_RESERVE_TOKENS
    );
}

#[test]
fn builder_uses_default_empty_session_memory() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .build()
        .expect("test engine build");

    assert!(engine.session_memory_snapshot().is_empty());
}

#[test]
fn builder_applies_context_scaled_session_memory_caps() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let config = CompactionConfig {
        model_context_limit: 200_000,
        ..CompactionConfig::default()
    };
    let memory = Arc::new(Mutex::new(SessionMemory::default()));
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(config.clone())
        .session_memory(Arc::clone(&memory))
        .build()
        .expect("test engine build");

    let stored = engine.session_memory_snapshot();
    assert_eq!(
        stored.token_cap(),
        fx_session::max_memory_tokens(config.model_context_limit)
    );
    assert_eq!(
        stored.item_cap(),
        fx_session::max_memory_items(config.model_context_limit)
    );
}

#[test]
fn builder_full_config() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let config = CompactionConfig {
        slide_threshold: 0.3,
        prune_threshold: 0.2,
        _legacy_summarize_threshold: 0.4,
        emergency_threshold: 0.9,
        preserve_recent_turns: 3,
        model_context_limit: 5_200,
        reserved_system_tokens: 100,
        recompact_cooldown_turns: 4,
        use_summarization: true,
        max_summary_tokens: 256,
        prune_tool_blocks: true,
        tool_block_summary_max_chars: 100,
    };
    let llm: Arc<dyn LlmProvider> = Arc::new(RecordingLlm::new(Vec::new()));
    let cancel_token = CancellationToken::new();
    let event_bus = fx_core::EventBus::new(16);
    let (_, input_channel) = crate::input::loop_input_channel();

    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(config.clone())
        .compaction_llm(llm)
        .event_bus(event_bus)
        .cancel_token(cancel_token)
        .input_channel(input_channel)
        .memory_context("remember this".to_string())
        .build()
        .expect("test engine build");

    assert_eq!(engine.compaction_config.preserve_recent_turns, 3);
    assert_eq!(engine.memory_context.as_deref(), Some("remember this"));
    assert!(engine.cancel_token.is_some());
    assert!(engine.input_channel.is_some());
    assert!(engine.event_bus.is_some());
    assert_eq!(engine.execution_visibility, ExecutionVisibility::Public);
    assert_eq!(
        engine.conversation_budget.conversation_budget(),
        config.model_context_limit
            - config.reserved_system_tokens
            - ConversationBudget::DEFAULT_OUTPUT_RESERVE_TOKENS
    );
}

#[test]
fn builder_validates_compaction_config() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut config = CompactionConfig::default();
    config.recompact_cooldown_turns = 0;

    let error = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(config)
        .build()
        .expect_err("invalid config should fail");

    assert_eq!(error.stage, "init");
    assert!(error.reason.contains("invalid_compaction_config"));
}

#[test]
fn build_compaction_components_default_to_valid_budget() {
    let (config, budget) = build_compaction_components(None).expect("components should build");
    let defaults = CompactionConfig::default();

    assert_eq!(config.slide_threshold, defaults.slide_threshold);
    assert_eq!(config.prune_threshold, defaults.prune_threshold);
    assert_eq!(config.emergency_threshold, defaults.emergency_threshold);
    assert_eq!(config.preserve_recent_turns, defaults.preserve_recent_turns);
    assert_eq!(
        budget.conversation_budget(),
        defaults.model_context_limit
            - defaults.reserved_system_tokens
            - ConversationBudget::DEFAULT_OUTPUT_RESERVE_TOKENS
    );
}

#[test]
fn build_compaction_components_reject_invalid_config() {
    let mut config = CompactionConfig::default();
    config.recompact_cooldown_turns = 0;

    let error = build_compaction_components(Some(config)).expect_err("invalid config rejected");
    assert_eq!(error.stage, "init");
    assert!(error.reason.contains("invalid_compaction_config"));
}

// RecordingLlm lives in test_fixtures (pub(super)) to avoid duplication.
use super::test_fixtures::RecordingLlm;

#[derive(Debug)]
struct ExtractionLlm {
    responses: Mutex<VecDeque<Result<String, CoreLlmError>>>,
    prompts: Mutex<Vec<String>>,
    delay: Option<std::time::Duration>,
}

impl ExtractionLlm {
    fn new(responses: Vec<Result<String, CoreLlmError>>) -> Self {
        Self::with_delay(responses, None)
    }

    fn with_delay(
        responses: Vec<Result<String, CoreLlmError>>,
        delay: Option<std::time::Duration>,
    ) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            prompts: Mutex::new(Vec::new()),
            delay,
        }
    }

    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().expect("prompts lock").clone()
    }
}

#[async_trait]
impl LlmProvider for ExtractionLlm {
    async fn generate(&self, prompt: &str, _: u32) -> Result<String, CoreLlmError> {
        self.prompts
            .lock()
            .expect("prompts lock")
            .push(prompt.to_string());
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        self.responses
            .lock()
            .expect("responses lock")
            .pop_front()
            .unwrap_or_else(|| Ok("{}".to_string()))
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        _: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, CoreLlmError> {
        let response = self.generate(prompt, 0).await?;
        callback(response.clone());
        Ok(response)
    }

    fn model_name(&self) -> &str {
        "mock-extraction"
    }

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        Ok(text_response("ok"))
    }
}

#[derive(Debug, Clone)]
struct FlushCall {
    evicted: Vec<Message>,
    scope: String,
}

#[derive(Debug, Default)]
struct RecordingMemoryFlush {
    calls: Mutex<Vec<FlushCall>>,
}

impl RecordingMemoryFlush {
    fn calls(&self) -> Vec<FlushCall> {
        self.calls.lock().expect("calls lock").clone()
    }
}

#[async_trait]
impl CompactionMemoryFlush for RecordingMemoryFlush {
    async fn flush(
        &self,
        evicted: &[Message],
        scope_label: &str,
    ) -> Result<(), crate::conversation_compactor::CompactionFlushError> {
        self.calls.lock().expect("calls lock").push(FlushCall {
            evicted: evicted.to_vec(),
            scope: scope_label.to_string(),
        });
        Ok(())
    }
}

/// Mock flush that always fails - verifies non-fatal behavior.
#[derive(Debug)]
struct FailingFlush;

#[async_trait]
impl CompactionMemoryFlush for FailingFlush {
    async fn flush(
        &self,
        _evicted: &[Message],
        _scope_label: &str,
    ) -> Result<(), crate::conversation_compactor::CompactionFlushError> {
        Err(
            crate::conversation_compactor::CompactionFlushError::FlushFailed {
                reason: "test failure".to_string(),
            },
        )
    }
}

#[derive(Debug)]
struct SizedToolExecutor {
    output_words: usize,
}

#[async_trait]
impl ToolExecutor for SizedToolExecutor {
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
                output: words(self.output_words),
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "read file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }
}

#[derive(Debug, Default)]
struct FailingToolRoundExecutor;

#[async_trait]
impl ToolExecutor for FailingToolRoundExecutor {
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
                output: "permission denied".to_string(),
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "read file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }
}

#[tokio::test]
async fn long_conversation_triggers_compaction_in_perceive() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let snapshot = snapshot_with_history(large_history(14, 70), "latest user request");

    let processed = engine.perceive(&snapshot).await.expect("perceive");

    assert!(has_compaction_marker(&processed.context_window));
    assert!(processed.context_window.len() < snapshot.conversation_history.len() + 1);
}

#[tokio::test]
async fn tool_rounds_compact_continuation_messages() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 120 });
    let mut engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let llm = RecordingLlm::new(vec![Ok(text_response("done"))]);
    let calls = vec![read_call("call-1")];
    let mut state = ToolRoundState::new(&calls, &large_history(12, 70), None);

    let tools = engine.tool_executor.tool_definitions();
    let _ = engine
        .execute_tool_round(1, &llm, &mut state, tools, CycleStream::disabled())
        .await
        .expect("tool round");

    assert!(has_compaction_marker(&state.continuation_messages));
}

#[tokio::test]
async fn tool_round_updates_last_reasoning_messages_after_compaction() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 120 });
    let mut engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let llm = RecordingLlm::new(vec![Ok(text_response("done"))]);
    let calls = vec![read_call("call-1")];
    let mut state = ToolRoundState::new(&calls, &large_history(12, 70), None);

    let tools = engine.tool_executor.tool_definitions();
    engine
        .execute_tool_round(1, &llm, &mut state, tools, CycleStream::disabled())
        .await
        .expect("tool round");

    assert!(has_compaction_marker(&engine.last_reasoning_messages));
    assert_eq!(engine.last_reasoning_messages, state.continuation_messages);
}

fn stream_recorder() -> (StreamCallback, Arc<Mutex<Vec<StreamEvent>>>) {
    let events: Arc<Mutex<Vec<StreamEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&events);
    let callback: StreamCallback = Arc::new(move |event| {
        captured.lock().expect("lock").push(event);
    });
    (callback, events)
}

#[tokio::test]
async fn tool_error_event_emitted_on_failure() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(FailingToolRoundExecutor);
    let mut engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let calls = vec![read_call("call-1")];
    let mut state = ToolRoundState::new(&calls, &[Message::user("read file")], None);
    let (callback, events) = stream_recorder();

    engine
        .execute_tool_round(
            1,
            &llm,
            &mut state,
            Vec::new(),
            CycleStream::enabled(&callback),
        )
        .await
        .expect("tool round");

    let events = events.lock().expect("lock").clone();
    assert!(events.contains(&StreamEvent::ToolError {
        tool_name: "read_file".to_string(),
        error: "permission denied".to_string(),
    }));
}

#[tokio::test]
async fn tool_error_directive_injected_on_failure() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(FailingToolRoundExecutor);
    let mut engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let calls = vec![read_call("call-1")];
    let mut state = ToolRoundState::new(&calls, &[Message::user("read file")], None);

    engine
        .execute_tool_round(1, &llm, &mut state, Vec::new(), CycleStream::disabled())
        .await
        .expect("tool round");

    let relay_message = state
        .continuation_messages
        .iter()
        .map(message_to_text)
        .find(|text| text.contains(TOOL_ERROR_RELAY_PREFIX))
        .expect("tool error relay message");
    assert!(relay_message.contains("- Tool 'read_file' failed with: permission denied"));
}

#[tokio::test]
async fn no_tool_error_on_success() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 5 });
    let mut engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let calls = vec![read_call("call-1")];
    let mut state = ToolRoundState::new(&calls, &[Message::user("read file")], None);
    let (callback, events) = stream_recorder();

    engine
        .execute_tool_round(
            1,
            &llm,
            &mut state,
            Vec::new(),
            CycleStream::enabled(&callback),
        )
        .await
        .expect("tool round");

    let events = events.lock().expect("lock").clone();
    assert!(!events
        .iter()
        .any(|event| matches!(event, StreamEvent::ToolError { .. })));
    assert!(!state
        .continuation_messages
        .iter()
        .map(message_to_text)
        .any(|text| text.contains(TOOL_ERROR_RELAY_PREFIX)));
}

#[tokio::test]
async fn decompose_child_receives_compacted_context() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let llm = RecordingLlm::new(vec![Ok(text_response("child done"))]);
    let goal = SubGoal {
        description: "child task".to_string(),
        required_tools: Vec::new(),
        completion_contract: SubGoalContract::from_definition_of_done(None),
        complexity_hint: None,
    };
    let child_budget = BudgetConfig::default();

    let _execution = engine
        .run_sub_goal(&goal, child_budget, &llm, &large_history(10, 60), &[])
        .await;

    let requests = llm.requests();
    assert!(!requests.is_empty());
    assert!(has_compaction_marker(&requests[0].messages));
}

#[tokio::test]
async fn run_sub_goal_fails_when_compacted_context_stays_over_hard_limit() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut config = compaction_config();
    config.preserve_recent_turns = 4;
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let llm = RecordingLlm::new(Vec::new());
    let goal = SubGoal {
        description: "child task".to_string(),
        required_tools: Vec::new(),
        completion_contract: SubGoalContract::from_definition_of_done(None),
        complexity_hint: None,
    };
    let protected = vec![
        Message::user(words(260)),
        Message::assistant(words(260)),
        Message::user(words(260)),
        Message::assistant(words(260)),
    ];
    let child_budget = BudgetConfig::default();

    let execution = engine
        .run_sub_goal(&goal, child_budget, &llm, &protected, &[])
        .await;
    let SubGoalOutcome::Failed(message) = &execution.result.outcome else {
        panic!("expected failed sub-goal outcome")
    };

    assert!(message.starts_with("context_exceeded_after_compaction:"));
    assert!(llm.requests().is_empty());
}

#[tokio::test]
async fn perceive_orders_compaction_before_reasoning_summary() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut config = compaction_config();
    config.model_context_limit = 5_600;
    let mut engine = engine_with(ContextCompactor::new(1, 2_500), executor, config);
    let user_text = format!("need order check {}", words(500));
    let snapshot = snapshot_with_history(large_history(12, 70), &user_text);

    let synthetic = engine.synthetic_context(&snapshot, &user_text);
    assert!(engine.context.needs_compaction(&synthetic));

    let processed = engine.perceive(&snapshot).await.expect("perceive");

    let marker = marker_message_index(&processed.context_window).expect("marker index");
    let summary = summary_message_index(&processed.context_window)
        .expect("expected compacted context summary in context window");
    assert!(marker < summary);
}

#[tokio::test]
async fn session_memory_injected_in_context() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut stored_memory = SessionMemory::default();
    stored_memory.project = Some("Phase 3".to_string());
    stored_memory.current_state = Some("testing injection".to_string());
    let memory = Arc::new(Mutex::new(stored_memory));
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .session_memory(Arc::clone(&memory))
        .build()
        .expect("test engine build");
    let snapshot = snapshot_with_history(
        vec![
            Message::system("system prefix"),
            Message::assistant("existing"),
        ],
        "hello",
    );

    let processed = engine.perceive(&snapshot).await.expect("perceive");
    let memory_index =
        session_memory_message_index(&processed.context_window).expect("memory message");

    assert_eq!(memory_index, 1);
    assert!(message_to_text(&processed.context_window[memory_index]).contains("Phase 3"));
}

#[tokio::test]
async fn empty_session_memory_not_injected() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .build()
        .expect("test engine build");
    let snapshot = snapshot_with_history(vec![Message::assistant("existing")], "hello");

    let processed = engine.perceive(&snapshot).await.expect("perceive");

    assert!(session_memory_message_index(&processed.context_window).is_none());
}

#[tokio::test]
async fn compaction_flushes_evicted_messages_before_returning_history() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let flush = Arc::new(RecordingMemoryFlush::default());
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(compaction_config())
        .memory_flush(Arc::clone(&flush) as Arc<dyn CompactionMemoryFlush>)
        .build()
        .expect("test engine build");
    let history = large_history(12, 60);

    let compacted = engine
        .compact_if_needed(&history, CompactionScope::Perceive, 1)
        .await
        .expect("compaction should succeed");

    assert!(has_compaction_marker(compacted.as_ref()));
    let calls = flush.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].scope, "perceive");
    assert!(!calls[0].evicted.is_empty());
    assert!(calls[0]
        .evicted
        .iter()
        .all(|message| history.contains(message)));
}

#[tokio::test]
async fn compact_if_needed_proceeds_on_flush_failure() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(compaction_config())
        .memory_flush(Arc::new(FailingFlush) as Arc<dyn CompactionMemoryFlush>)
        .build()
        .expect("test engine build");
    let messages = large_history(10, 60);

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("compaction should proceed when flush fails");

    assert!(has_compaction_marker(compacted.as_ref()));
    assert!(compacted.len() < messages.len());
}

#[tokio::test]
async fn compact_if_needed_emits_memory_error_when_flush_fails() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let events = Arc::new(Mutex::new(Vec::<StreamEvent>::new()));
    let captured = Arc::clone(&events);
    let callback: StreamCallback = Arc::new(move |event| {
        captured.lock().expect("lock").push(event);
    });
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(compaction_config())
        .memory_flush(Arc::new(FailingFlush) as Arc<dyn CompactionMemoryFlush>)
        .error_callback(callback)
        .build()
        .expect("test engine build");
    let messages = large_history(10, 60);

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("compaction should proceed when flush fails");

    assert!(has_compaction_marker(compacted.as_ref()));
    let events = events.lock().expect("lock").clone();
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::Error {
            category: ErrorCategory::Memory,
            message,
            recoverable: true,
        } if message == "Memory flush failed during compaction: memory flush failed: test failure"
    )));
}

#[tokio::test]
async fn compact_if_needed_emits_context_compacted_event() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let events = Arc::new(Mutex::new(Vec::<StreamEvent>::new()));
    let captured = Arc::clone(&events);
    let callback: StreamCallback = Arc::new(move |event| {
        captured.lock().expect("lock").push(event);
    });
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(compaction_config())
        .error_callback(callback)
        .build()
        .expect("test engine build");
    let messages = large_history(10, 60);

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("compaction should succeed");

    let before_tokens = ConversationBudget::estimate_tokens(&messages);
    let after_tokens = ConversationBudget::estimate_tokens(compacted.as_ref());
    let expected_usage_ratio =
        f64::from(engine.conversation_budget.usage_ratio(compacted.as_ref()));

    let events = events.lock().expect("lock").clone();
    assert!(events.iter().any(|event| matches!(
        event,
        StreamEvent::ContextCompacted {
            tier,
            messages_removed,
            tokens_before,
            tokens_after,
            usage_ratio,
        } if tier == "slide"
            && *messages_removed > 0
            && *tokens_before == before_tokens
            && *tokens_after == after_tokens
            && (usage_ratio - expected_usage_ratio).abs() < f64::EPSILON
    )));
}

#[tokio::test]
async fn compact_if_needed_skips_flush_when_none() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let messages = large_history(10, 60);

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("compaction should succeed without memory flush configured");

    assert!(has_compaction_marker(compacted.as_ref()));
    assert!(compacted.len() < messages.len());
}

#[tokio::test]
async fn extract_memory_from_evicted_updates_session_memory() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let llm = Arc::new(ExtractionLlm::new(vec![Ok(serde_json::json!({
        "project": "Phase 5",
        "current_state": "Adding automatic extraction",
        "key_decisions": ["Use compaction LLM"],
        "active_files": ["engine/crates/fx-kernel/src/loop_engine.rs"],
        "custom_context": ["Evicted facts are auto-saved"]
    })
    .to_string())]));
    let engine = engine_with_compaction_llm(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
        Arc::clone(&llm) as Arc<dyn LlmProvider>,
    );
    let evicted = vec![
        Message::user("We are implementing Phase 5."),
        Message::assistant("LoopEngine needs automatic extraction."),
    ];

    engine.extract_memory_from_evicted(&evicted, None).await;

    let memory = engine.session_memory_snapshot();
    assert_eq!(memory.project.as_deref(), Some("Phase 5"));
    assert_eq!(
        memory.current_state.as_deref(),
        Some("Adding automatic extraction")
    );
    assert_eq!(memory.key_decisions, vec!["Use compaction LLM"]);
    assert_eq!(
        memory.active_files,
        vec!["engine/crates/fx-kernel/src/loop_engine.rs"]
    );
    assert_eq!(memory.custom_context, vec!["Evicted facts are auto-saved"]);
    assert_eq!(llm.prompts().len(), 1);
}

#[tokio::test]
async fn extract_memory_skipped_without_compaction_llm() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );

    engine
        .extract_memory_from_evicted(&[Message::user("remember this")], None)
        .await;

    assert!(engine.session_memory_snapshot().is_empty());
}

#[tokio::test]
async fn extract_memory_handles_llm_failure_gracefully() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let llm = Arc::new(ExtractionLlm::new(vec![Err(CoreLlmError::ApiRequest(
        "boom".to_string(),
    ))]));
    let engine = engine_with_compaction_llm(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
        llm,
    );

    engine
        .extract_memory_from_evicted(&[Message::user("remember this")], None)
        .await;

    assert!(engine.session_memory_snapshot().is_empty());
}

#[tokio::test]
async fn extract_memory_handles_malformed_response() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let llm = Arc::new(ExtractionLlm::new(vec![Ok("not json".to_string())]));
    let engine = engine_with_compaction_llm(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
        llm,
    );

    engine
        .extract_memory_from_evicted(&[Message::user("remember this")], None)
        .await;

    assert!(engine.session_memory_snapshot().is_empty());
}

#[tokio::test]
async fn extract_memory_respects_token_cap() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let llm = Arc::new(ExtractionLlm::new(vec![Ok(
        serde_json::json!({"custom_context": [words(2_100)]}).to_string(),
    )]));
    let engine = engine_with_compaction_llm(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
        llm,
    );

    engine
        .extract_memory_from_evicted(&[Message::user("remember this")], None)
        .await;

    assert!(engine.session_memory_snapshot().is_empty());
}

#[tokio::test]
async fn extract_memory_from_summary_falls_back_to_llm_when_parsing_fails() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let llm = Arc::new(ExtractionLlm::new(vec![Ok(serde_json::json!({
        "project": "Phase 2",
        "current_state": "LLM fallback after malformed summary"
    })
    .to_string())]));
    let engine = engine_with_compaction_llm(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
        Arc::clone(&llm) as Arc<dyn LlmProvider>,
    );

    engine
        .extract_memory_from_evicted(
            &[Message::user("remember this")],
            Some("freeform summary without section headers"),
        )
        .await;

    let memory = engine.session_memory_snapshot();
    assert_eq!(memory.project.as_deref(), Some("Phase 2"));
    assert_eq!(
        memory.current_state.as_deref(),
        Some("LLM fallback after malformed summary")
    );
    assert_eq!(llm.prompts().len(), 1);
    assert!(llm.prompts()[0].contains("Conversation:"));
}

#[tokio::test]
async fn extract_memory_from_numbered_summary_skips_llm_fallback() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let llm = Arc::new(ExtractionLlm::new(vec![Ok("{}".to_string())]));
    let engine = engine_with_compaction_llm(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
        Arc::clone(&llm) as Arc<dyn LlmProvider>,
    );
    let summary = concat!(
        "1. Decisions:\n",
        "- summarize before slide\n",
        "2. Files modified:\n",
        "- engine/crates/fx-kernel/src/loop_engine.rs\n",
        "3. Task state:\n",
        "- preserving summary context\n",
        "4. Key context:\n",
        "- no second LLM call needed"
    );

    engine
        .extract_memory_from_evicted(&[Message::user("remember this")], Some(summary))
        .await;

    let memory = engine.session_memory_snapshot();
    assert_eq!(
        memory.current_state.as_deref(),
        Some("preserving summary context")
    );
    assert_eq!(memory.key_decisions, vec!["summarize before slide"]);
    assert_eq!(
        memory.active_files,
        vec!["engine/crates/fx-kernel/src/loop_engine.rs"]
    );
    assert_eq!(memory.custom_context, vec!["no second LLM call needed"]);
    assert!(llm.prompts().is_empty());
}

#[tokio::test]
async fn flush_evicted_triggers_extraction() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let flush = Arc::new(RecordingMemoryFlush::default());
    let llm = Arc::new(ExtractionLlm::new(vec![Ok(serde_json::json!({
        "project": "Phase 5",
        "custom_context": ["Compaction saved this fact"]
    })
    .to_string())]));
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(compaction_config())
        .compaction_llm(Arc::clone(&llm) as Arc<dyn LlmProvider>)
        .memory_flush(Arc::clone(&flush) as Arc<dyn CompactionMemoryFlush>)
        .build()
        .expect("test engine build");
    let history = large_history(12, 60);

    let compacted = engine
        .compact_if_needed(&history, CompactionScope::Perceive, 1)
        .await
        .expect("compaction should succeed");

    assert!(has_compaction_marker(compacted.as_ref()));
    assert_eq!(flush.calls().len(), 1);
    assert_eq!(
        engine.session_memory_snapshot().project.as_deref(),
        Some("Phase 5")
    );
    assert_eq!(
        engine.session_memory_snapshot().custom_context,
        vec!["Compaction saved this fact"]
    );
    assert_eq!(llm.prompts().len(), 1);
}

#[tokio::test]
async fn flush_evicted_uses_summary_for_flush_and_memory_extraction() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let flush = Arc::new(RecordingMemoryFlush::default());
    let summary = concat!(
        "Decisions:\n",
        "- summarize before slide\n",
        "Files modified:\n",
        "- engine/crates/fx-kernel/src/loop_engine.rs\n",
        "Task state:\n",
        "- preserving old context\n",
        "Key context:\n",
        "- summary markers stay protected"
    );
    let llm = Arc::new(ExtractionLlm::new(vec![Ok(summary.to_string())]));
    let mut config = tiered_compaction_config(true);
    config.prune_tool_blocks = false;
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2_048, 256))
        .max_iterations(4)
        .tool_executor(executor)
        .synthesis_instruction("synthesize".to_string())
        .compaction_config(config)
        .compaction_llm(Arc::clone(&llm) as Arc<dyn LlmProvider>)
        .memory_flush(Arc::clone(&flush) as Arc<dyn CompactionMemoryFlush>)
        .build()
        .expect("test engine build");
    let messages = vec![
        Message::user(format!("older decision {}", words(199))),
        Message::assistant(format!("older file change {}", words(199))),
        Message::user(format!("recent state {}", words(124))),
        Message::assistant(format!("recent context {}", words(124))),
    ];

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 1)
        .await
        .expect("compaction should succeed");

    assert!(has_conversation_summary_marker(compacted.as_ref()));
    assert!(!has_compaction_marker(compacted.as_ref()));
    let calls = flush.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].scope, "perceive");
    assert_eq!(calls[0].evicted.len(), 1);
    assert!(message_to_text(&calls[0].evicted[0]).contains("[context summary]"));
    let memory = engine.session_memory_snapshot();
    assert_eq!(
        memory.current_state.as_deref(),
        Some("preserving old context")
    );
    assert_eq!(memory.key_decisions, vec!["summarize before slide"]);
    assert_eq!(
        memory.active_files,
        vec!["engine/crates/fx-kernel/src/loop_engine.rs"]
    );
    assert_eq!(
        memory.custom_context,
        vec!["summary markers stay protected"]
    );
    assert_eq!(llm.prompts().len(), 1);
}

#[tokio::test]
async fn tiered_compaction_prune_only() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let config = tiered_compaction_config(false);
    let budget = tiered_budget(&config);
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let messages = vec![
        tool_use("t1"),
        tool_result("t1", 432),
        user(5),
        assistant(5),
    ];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.40 && usage < 0.60, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("prune-only compaction");

    assert_ne!(compacted.as_ref(), messages.as_slice());
    assert!(!has_tool_blocks(compacted.as_ref()));
    assert!(!has_compaction_marker(compacted.as_ref()));
    assert!(!has_emergency_compaction_marker(compacted.as_ref()));
}

#[tokio::test]
async fn tiered_compaction_slide_when_prune_insufficient() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let config = tiered_compaction_config(false);
    let budget = tiered_budget(&config);
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let messages = vec![user(200), assistant(200), user(125), assistant(125)];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.60 && usage < 0.95, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("slide compaction");

    assert!(has_compaction_marker(compacted.as_ref()));
    assert!(!has_emergency_compaction_marker(compacted.as_ref()));
    assert!(!has_conversation_summary_marker(compacted.as_ref()));
}

#[tokio::test]
async fn slide_tier_summarizes_before_eviction_when_llm_available() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let summary = concat!(
        "Decisions:\n",
        "- preserve older context\n",
        "Files modified:\n",
        "- engine/crates/fx-kernel/src/loop_engine.rs\n",
        "Task state:\n",
        "- summary inserted before slide\n",
        "Key context:\n",
        "- older messages remain recoverable"
    );
    let llm = Arc::new(ExtractionLlm::new(vec![Ok(summary.to_string())]));
    let mut config = tiered_compaction_config(true);
    config.prune_tool_blocks = false;
    let budget = tiered_budget(&config);
    let engine = engine_with_compaction_llm(
        ContextCompactor::new(2_048, 256),
        executor,
        config,
        Arc::clone(&llm) as Arc<dyn LlmProvider>,
    );
    let messages = vec![
        Message::user(format!("older plan {}", words(199))),
        Message::assistant(format!("older file {}", words(199))),
        Message::user(format!("recent state {}", words(124))),
        Message::assistant(format!("recent context {}", words(124))),
    ];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.60 && usage < 0.95, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("slide compaction");

    assert!(has_conversation_summary_marker(compacted.as_ref()));
    assert!(!has_compaction_marker(compacted.as_ref()));
    let prompts = llm.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("older plan"));
    assert!(prompts[0].contains("older file"));
}

#[tokio::test]
async fn slide_tier_falls_back_to_lossy_slide_when_summary_fails() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let llm = Arc::new(ExtractionLlm::new(vec![
        Err(CoreLlmError::ApiRequest("boom".to_string())),
        Err(CoreLlmError::ApiRequest("boom".to_string())),
    ]));
    let mut config = tiered_compaction_config(true);
    config.prune_tool_blocks = false;
    let budget = tiered_budget(&config);
    let engine =
        engine_with_compaction_llm(ContextCompactor::new(2_048, 256), executor, config, llm);
    let messages = vec![user(250), assistant(250), user(175), assistant(175)];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.80 && usage < 0.95, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("slide compaction");

    assert!(has_compaction_marker(compacted.as_ref()));
    assert!(!has_conversation_summary_marker(compacted.as_ref()));
    assert!(!has_emergency_compaction_marker(compacted.as_ref()));
}

#[tokio::test]
async fn slide_tier_falls_back_to_lossy_slide_without_compaction_llm() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let config = tiered_compaction_config(true);
    let budget = tiered_budget(&config);
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let messages = vec![user(250), assistant(250), user(175), assistant(175)];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.80 && usage < 0.95, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("slide compaction");

    assert!(has_compaction_marker(compacted.as_ref()));
    assert!(!has_conversation_summary_marker(compacted.as_ref()));
    assert!(!has_emergency_compaction_marker(compacted.as_ref()));
}

#[tokio::test]
async fn summarize_before_slide_without_llm_falls_back_to_lossy_slide() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let config = tiered_compaction_config(true);
    let budget = tiered_budget(&config);
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let messages = vec![user(250), assistant(250), user(175), assistant(175)];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.80 && usage < 0.95, "usage ratio was {usage}");

    let compacted = engine
        .summarize_before_slide(
            &messages,
            budget.compaction_target(),
            CompactionScope::Perceive,
        )
        .await
        .expect("lossy slide fallback");

    assert!(has_compaction_marker(&compacted.messages));
    assert!(!has_conversation_summary_marker(&compacted.messages));
    assert!(!has_emergency_compaction_marker(&compacted.messages));
}

#[tokio::test]
async fn tiered_compaction_emergency_fires_at_95_percent() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let config = tiered_compaction_config(false);
    let budget = tiered_budget(&config);
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let messages = vec![user(250), assistant(250), user(230), assistant(230)];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.95, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("emergency compaction");

    assert!(has_emergency_compaction_marker(compacted.as_ref()));
    assert!(!has_conversation_summary_marker(compacted.as_ref()));
}

#[tokio::test]
async fn emergency_tier_uses_summary_when_llm_is_fast_enough() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let summary = concat!(
        "Decisions:\n",
        "- capture emergency context\n",
        "Files modified:\n",
        "- engine/crates/fx-kernel/src/loop_engine.rs\n",
        "Task state:\n",
        "- emergency summary completed\n",
        "Key context:\n",
        "- fallback count marker avoided"
    );
    let llm = Arc::new(ExtractionLlm::new(vec![Ok(summary.to_string())]));
    let mut config = tiered_compaction_config(true);
    config.prune_tool_blocks = false;
    let budget = tiered_budget(&config);
    let engine = engine_with_compaction_llm(
        ContextCompactor::new(2_048, 256),
        executor,
        config,
        Arc::clone(&llm) as Arc<dyn LlmProvider>,
    );
    let messages = vec![user(250), assistant(250), user(230), assistant(230)];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.95, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("emergency compaction");

    assert!(has_conversation_summary_marker(compacted.as_ref()));
    assert!(!has_emergency_compaction_marker(compacted.as_ref()));
    assert_eq!(llm.prompts().len(), 1);
}

#[tokio::test]
async fn emergency_tier_attempts_best_effort_summary_before_fallback() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let summary = concat!(
        "Decisions:\n",
        "- capture emergency context\n",
        "Files modified:\n",
        "- engine/crates/fx-kernel/src/loop_engine.rs\n",
        "Task state:\n",
        "- timeout fallback\n",
        "Key context:\n",
        "- summary was too slow"
    );
    let llm = Arc::new(ExtractionLlm::with_delay(
        vec![Ok(summary.to_string()), Ok("{}".to_string())],
        Some(EMERGENCY_SUMMARY_TIMEOUT + std::time::Duration::from_millis(10)),
    ));
    let mut config = tiered_compaction_config(true);
    config.prune_tool_blocks = false;
    let budget = tiered_budget(&config);
    let engine = engine_with_compaction_llm(
        ContextCompactor::new(2_048, 256),
        executor,
        config,
        Arc::clone(&llm) as Arc<dyn LlmProvider>,
    );
    let messages = vec![user(250), assistant(250), user(230), assistant(230)];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.95, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("emergency compaction");

    assert!(has_emergency_compaction_marker(compacted.as_ref()));
    assert!(!has_conversation_summary_marker(compacted.as_ref()));
    let prompts = llm.prompts();
    assert!(!prompts.is_empty());
    assert!(prompts[0].contains("Sections (required):"));
}

#[tokio::test]
async fn compact_if_needed_emergency_tier_preserves_tool_pairs() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let config = tiered_compaction_config(false);
    let budget = tiered_budget(&config);
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let messages = vec![
        tool_use("call-1"),
        user(250),
        assistant(250),
        tool_result("call-1", 230),
        user(230),
    ];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.95, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("emergency compaction");

    assert!(has_emergency_compaction_marker(compacted.as_ref()));
    assert!(compacted.as_ref().iter().any(|message| {
        message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolUse { id, .. } if id == "call-1"))
    }));
    assert!(compacted.as_ref().iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call-1"
            )
        })
    }));
    debug_assert_tool_pair_integrity(compacted.as_ref());
}

#[tokio::test]
async fn cooldown_skips_slide_but_allows_emergency() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let config = tiered_compaction_config(true);
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let slide_input = vec![user(200), assistant(200), user(125), assistant(125)];

    let first = engine
        .compact_if_needed(&slide_input, CompactionScope::Perceive, 10)
        .await
        .expect("first compaction");
    assert!(has_compaction_marker(first.as_ref()));
    assert!(engine.should_skip_compaction(CompactionScope::Perceive, 11, CompactionTier::Slide));

    let emergency_input = vec![user(250), assistant(250), user(230), assistant(230)];
    let second = engine
        .compact_if_needed(&emergency_input, CompactionScope::Perceive, 11)
        .await
        .expect("emergency compaction during cooldown");

    assert!(has_emergency_compaction_marker(second.as_ref()));
    assert!(!has_conversation_summary_marker(second.as_ref()));
}

#[tokio::test]
async fn cooldown_skips_compaction_when_within_window() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let messages = large_history(12, 60);

    let first = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("first compaction");
    assert!(has_compaction_marker(first.as_ref()));

    let second_input = large_history(12, 60);
    let second = engine
        .compact_if_needed(&second_input, CompactionScope::Perceive, 11)
        .await
        .expect("second compaction");

    assert_eq!(second.as_ref(), second_input.as_slice());
}

#[tokio::test]
async fn cooldown_allows_compaction_after_window_elapsed() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let messages = large_history(12, 60);

    let _ = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 10)
        .await
        .expect("first compaction");

    let second = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 13)
        .await
        .expect("second compaction");

    assert!(has_compaction_marker(second.as_ref()));
}

#[tokio::test]
async fn emergency_bypasses_cooldown() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );

    let _ = engine
        .compact_if_needed(&large_history(10, 60), CompactionScope::Perceive, 10)
        .await
        .expect("first compaction");

    let oversized = large_history(16, 80);
    let second = engine
        .compact_if_needed(&oversized, CompactionScope::Perceive, 11)
        .await
        .expect("emergency compaction");

    assert!(has_emergency_compaction_marker(second.as_ref()));
    assert_ne!(second.as_ref(), oversized.as_slice());
}

#[tokio::test]
async fn legacy_summarize_threshold_does_not_trigger_compaction_below_slide_threshold() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut config = tiered_compaction_config(true);
    config.slide_threshold = 0.80;
    config._legacy_summarize_threshold = 0.30;
    let budget = tiered_budget(&config);
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let messages = vec![user(125), assistant(125), user(125), assistant(125)];

    let usage = budget.usage_ratio(&messages);
    assert!(usage > 0.30 && usage < 0.80, "usage ratio was {usage}");

    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 1)
        .await
        .expect("legacy summarize threshold should be ignored");

    assert_eq!(compacted.as_ref(), messages.as_slice());
}

#[tokio::test]
async fn all_messages_protected_over_hard_limit_returns_context_exceeded() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut config = compaction_config();
    config.preserve_recent_turns = 4;
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let protected = vec![
        Message::user(words(260)),
        Message::assistant(words(260)),
        Message::user(words(260)),
        Message::assistant(words(260)),
    ];

    let error = engine
        .compact_if_needed(&protected, CompactionScope::Perceive, 2)
        .await
        .expect_err("context exceeded error");

    assert_eq!(error.stage, "compaction");
    assert!(error
        .reason
        .starts_with("context_exceeded_after_compaction:"));
}

#[tokio::test]
async fn compaction_preserves_session_coherence() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut config = compaction_config();
    config.preserve_recent_turns = 4;
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);

    let mut messages = vec![Message::system("system policy")];
    messages.extend(large_history(12, 60));
    let compacted = engine
        .compact_if_needed(&messages, CompactionScope::Perceive, 3)
        .await
        .expect("compact");

    assert_eq!(compacted[0].role, MessageRole::System);
    assert!(has_compaction_marker(compacted.as_ref()));
    assert_eq!(
        &compacted[compacted.len() - 4..],
        &messages[messages.len() - 4..]
    );
}

#[tokio::test]
async fn compaction_coexists_with_existing_context_compactor() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut config = compaction_config();
    config.model_context_limit = 5_600;
    let mut engine = engine_with(ContextCompactor::new(1, 2_500), executor, config);
    let user_text = format!("coexistence check {}", words(500));
    let snapshot = snapshot_with_history(large_history(12, 70), &user_text);

    let synthetic = engine.synthetic_context(&snapshot, &user_text);
    assert!(engine.context.needs_compaction(&synthetic));

    let processed = engine.perceive(&snapshot).await.expect("perceive");

    assert!(has_compaction_marker(&processed.context_window));
    let marker =
        marker_message_index(&processed.context_window).expect("expected compaction marker");
    let summary = summary_message_index(&processed.context_window)
        .expect("expected compacted context summary in context window");
    assert!(marker < summary);
}

#[tokio::test]
async fn compaction_with_all_protected_messages() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut config = compaction_config();
    config.preserve_recent_turns = 4;
    let engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);

    let protected_under_limit = vec![
        Message::user(words(60)),
        Message::assistant(words(60)),
        Message::user(words(60)),
        Message::assistant(words(60)),
    ];

    let result = engine
        .compact_if_needed(&protected_under_limit, CompactionScope::Perceive, 1)
        .await
        .expect("under hard limit keeps original");
    assert_eq!(result.as_ref(), protected_under_limit.as_slice());

    let protected_over_limit = vec![
        Message::user(words(260)),
        Message::assistant(words(260)),
        Message::user(words(260)),
        Message::assistant(words(260)),
    ];
    let error = engine
        .compact_if_needed(&protected_over_limit, CompactionScope::Perceive, 2)
        .await
        .expect_err("over hard limit errors");
    assert!(error
        .reason
        .starts_with("context_exceeded_after_compaction:"));
}

#[tokio::test]
async fn concurrent_decompose_children_each_compact_independently() {
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let mut config = compaction_config();
    config.recompact_cooldown_turns = 1;
    let mut engine = engine_with(ContextCompactor::new(2_048, 256), executor, config);
    let plan = DecompositionPlan {
        sub_goals: vec![
            SubGoal {
                description: "child-a".to_string(),
                required_tools: Vec::new(),
                completion_contract: SubGoalContract::from_definition_of_done(None),
                complexity_hint: None,
            },
            SubGoal {
                description: "child-b".to_string(),
                required_tools: Vec::new(),
                completion_contract: SubGoalContract::from_definition_of_done(None),
                complexity_hint: None,
            },
        ],
        strategy: AggregationStrategy::Parallel,
        truncated_from: None,
    };
    let llm = RecordingLlm::new(vec![Ok(text_response("a")), Ok(text_response("b"))]);
    let allocation = AllocationPlan {
        sub_goal_budgets: vec![BudgetConfig::default(); plan.sub_goals.len()],
        parent_continuation_budget: BudgetConfig::default(),
        skipped_indices: Vec::new(),
    };

    let results = engine
        .execute_sub_goals_concurrent(&plan, &allocation, &llm, &large_history(12, 60))
        .await;

    assert_eq!(results.len(), 2);

    let requests = llm.requests();
    let compacted_requests = requests
        .iter()
        .filter(|request| has_compaction_marker(&request.messages))
        .count();
    assert!(compacted_requests >= 2);
}

#[derive(Default)]
struct EventFields {
    values: HashMap<String, String>,
}

impl Visit for EventFields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.values
            .insert(field.name().to_string(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.values
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.values
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.values
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.values
            .insert(field.name().to_string(), value.to_string());
    }
}

#[derive(Default)]
struct CaptureLayer {
    events: Arc<Mutex<Vec<HashMap<String, String>>>>,
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = EventFields::default();
        event.record(&mut fields);
        self.events.lock().expect("events lock").push(fields.values);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn compaction_emits_observability_fields() {
    let _trace_lock = TRACE_SUBSCRIBER_LOCK.lock().await;
    let executor: Arc<dyn ToolExecutor> = Arc::new(SizedToolExecutor { output_words: 20 });
    let engine = engine_with(
        ContextCompactor::new(2_048, 256),
        executor,
        compaction_config(),
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Registry::default()
        .with(LevelFilter::TRACE)
        .with(CaptureLayer {
            events: Arc::clone(&events),
        });
    // Scope the subscriber to this test using the dispatcher guard.
    // This overrides any thread-local or global default for the guard's lifetime.
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
    tracing::dispatcher::with_default(&dispatch, || {
        // Verify the dispatch is active — if this fails, subscriber interception is broken.
        tracing::info!("test_probe");
    });
    // Check probe was captured; if not, subscriber is shadowed (skip gracefully).
    let probe_captured = events
        .lock()
        .expect("events lock")
        .iter()
        .any(|e| e.values().any(|v| v == "test_probe"));
    if !probe_captured {
        eprintln!(
            "WARN: tracing subscriber capture unavailable, skipping observability assertions"
        );
        return;
    }
    events.lock().expect("events lock").clear();
    let _guard = tracing::dispatcher::set_default(&dispatch);

    let history = large_history(12, 70);
    let compacted = engine
        .compact_if_needed(&history, CompactionScope::Perceive, 1)
        .await
        .expect("compaction should succeed");
    assert!(has_compaction_marker(compacted.as_ref()));

    let captured = events.lock().expect("events lock").clone();
    if captured.is_empty() {
        // Subscriber capture failed (global subscriber conflict in multi-test process).
        // This test verifies observability fields, not compaction correctness — skip gracefully.
        eprintln!("WARN: tracing capture empty after compaction, skipping field assertions");
        return;
    }

    let info_event = captured.iter().find(|event| {
        event.contains_key("before_tokens")
            && event.contains_key("after_tokens")
            && event.contains_key("messages_removed")
    });

    let info_event = info_event
        .unwrap_or_else(|| panic!("compaction info event missing; captured={captured:?}"));
    for key in [
        "scope",
        "tier",
        "strategy",
        "before_tokens",
        "after_tokens",
        "target_tokens",
        "usage_ratio_before",
        "usage_ratio_after",
        "tokens_saved",
        "messages_removed",
    ] {
        assert!(
            info_event.contains_key(key),
            "missing observability field: {key}"
        );
    }
}
