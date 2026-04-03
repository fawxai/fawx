use super::*;
use async_trait::async_trait;
use fx_core::error::LlmError as CoreLlmError;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_llm::{CompletionResponse, ContentBlock, Message, ProviderError, ToolCall, ToolDefinition};
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Debug, Default)]
struct TestStubToolExecutor;

#[async_trait]
impl ToolExecutor for TestStubToolExecutor {
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

fn default_engine() -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            0,
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(TestStubToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

fn base_snapshot(text: &str) -> PerceptionSnapshot {
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

#[test]
fn system_prompt_includes_tool_use_guidance() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(prompt.contains("Use tools when you need information not already in the conversation"));
    assert!(
            prompt.contains(
                "When the user's request relates to an available tool's purpose, prefer calling the tool"
            ),
            "system prompt should encourage proactive tool usage for matching requests"
        );
}

#[test]
fn system_prompt_prohibits_greeting_and_preamble() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(
        prompt.contains("Never introduce yourself"),
        "system prompt must prohibit self-introduction (issue #959)"
    );
    assert!(
        prompt.contains("greet the user"),
        "system prompt must prohibit greeting (issue #959)"
    );
}

#[test]
fn system_prompt_without_memory_omits_persistent_memory_block() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(
        !prompt.contains("You have persistent memory across sessions"),
        "system prompt without memory context should NOT include the persistent memory block"
    );
}

#[test]
fn system_prompt_omits_notify_guidance_without_notification_channel() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(
        !prompt.contains("You have a `notify` tool"),
        "system prompt should omit notify guidance when no notification channel is active"
    );
}

#[test]
fn system_prompt_includes_notify_guidance_when_notification_channel_is_active() {
    let prompt = build_reasoning_system_prompt_with_notify_guidance(None, None, true);
    assert!(
        prompt.contains("You have a `notify` tool"),
        "system prompt should include notify guidance when notifications are available"
    );
}

#[test]
fn system_prompt_with_memory_includes_memory_instruction() {
    let prompt = build_reasoning_system_prompt(Some("user prefers dark mode"), None);
    assert!(
        prompt.contains("memory_write"),
        "system prompt with memory context should mention memory_write via MEMORY_INSTRUCTION"
    );
    assert!(
        prompt.contains("user prefers dark mode"),
        "system prompt should include the memory context"
    );
}

/// Regression test: tool definitions must NOT appear as text in the system
/// prompt. They are already provided via the structured `tools` field of
/// `CompletionRequest`. Duplicating them in the system prompt caused 9×
/// token bloat on OpenAI and broke multi-step instruction following.
#[test]
fn system_prompt_does_not_contain_tool_descriptions() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(
        !prompt.contains("Available tools:"),
        "system prompt must not contain 'Available tools:' text — \
             tool definitions belong in the structured tools field, not the prompt"
    );

    // Also verify with memory context (second code path).
    let prompt_with_memory = build_reasoning_system_prompt(Some("user likes cats"), None);
    assert!(
        !prompt_with_memory.contains("Available tools:"),
        "system prompt with memory must not contain 'Available tools:' text"
    );
}

#[test]
fn tool_continuation_prompt_prioritizes_answering_from_existing_results() {
    let prompt = build_tool_continuation_system_prompt(None, None);
    assert!(
        prompt.contains("Treat successful tool results as the primary evidence"),
        "tool continuation prompt should prioritize existing tool results"
    );
    assert!(
        prompt.contains("answer immediately instead of calling more tools"),
        "tool continuation prompt should prefer answering once results suffice"
    );
    assert!(
        prompt.contains("Never repeat an identical successful tool call in the same cycle"),
        "tool continuation prompt should discourage redundant tool retries"
    );
}

#[test]
fn continuation_request_includes_tool_continuation_directive_once() {
    let request = build_continuation_request(ContinuationRequestParams::new(
        &[Message::assistant("intermediate")],
        "mock-model",
        ToolRequestConfig::new(vec![], true),
        RequestBuildContext::new(None, None, None, false),
    ));
    let prompt = request
        .system_prompt
        .expect("continuation request should include a system prompt");
    assert_eq!(
        prompt.matches(TOOL_CONTINUATION_DIRECTIVE).count(),
        1,
        "continuation request should include the tool continuation directive exactly once"
    );
}

#[test]
fn tool_synthesis_prompt_content_is_complete() {
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "current_time".to_string(),
        output: "2026-02-28T14:00:00Z".to_string(),
        success: true,
    }];
    let prompt = tool_synthesis_prompt(&results, "Tell the user the time.");
    assert!(
        prompt.contains("You are Fawx"),
        "synthesis prompt must include assistant identity"
    );
    assert!(
        prompt.contains("Answer the user's question using these tool results"),
        "synthesis prompt must instruct direct answering"
    );
    assert!(
        prompt.contains("Do NOT describe what tools were called"),
        "synthesis prompt must block meta-narration"
    );
    assert!(
        prompt.contains(
            "If the user asked for a specific format or value type, preserve that exact format."
        ),
        "synthesis prompt must preserve requested output formats"
    );
    assert!(
            prompt.contains(
                "Do not convert timestamps to human-readable, counts to lists, or raw values to prose unless the user explicitly asked for that."
            ),
            "synthesis prompt must forbid format rewriting"
        );
    assert!(
        prompt.contains("Tell the user the time."),
        "synthesis prompt must include the instruction"
    );
    assert!(
        prompt.contains("current_time: 2026-02-28T14:00:00Z"),
        "synthesis prompt must include tool results"
    );
}

#[test]
fn tool_synthesis_prompt_explicitly_prohibits_intro_and_greeting() {
    let prompt = tool_synthesis_prompt(&[], "Combine outputs");
    assert!(
        prompt.contains("Never introduce yourself, greet the user, or add preamble"),
        "synthesis prompt should mirror no-intro guidance from reasoning prompt"
    );
}

#[test]
fn synthesis_includes_all_results() {
    let results = vec![
        ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            output: "alpha".to_string(),
            success: true,
        },
        ToolResult {
            tool_call_id: "call-2".to_string(),
            tool_name: "search".to_string(),
            output: "beta".to_string(),
            success: true,
        },
    ];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(prompt.contains("read_file: alpha"));
    assert!(prompt.contains("search: beta"));

    let tool_results_section = prompt
        .split("Tool results:\n")
        .nth(1)
        .expect("prompt should include tool results section");
    let result_count = tool_results_section
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .filter(|line| line.starts_with("- "))
        .count();
    assert_eq!(
        result_count, 2,
        "prompt should include exactly 2 tool results"
    );
}

#[test]
fn synthesis_includes_failed_tool_results() {
    let results = vec![
        ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            output: "alpha".to_string(),
            success: true,
        },
        ToolResult {
            tool_call_id: "call-2".to_string(),
            tool_name: "run_command".to_string(),
            output: "permission denied".to_string(),
            success: false,
        },
    ];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(prompt.contains("read_file: alpha"));
    assert!(prompt.contains("run_command: permission denied"));
}

#[test]
fn synthesis_prompt_includes_error_relay_instruction_when_tool_failed() {
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        output: "file not found: /foo/bar".to_string(),
        success: false,
    }];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(prompt.contains("If any tool returned an error, tell the user exactly what went wrong: include the actual error message. Do not soften, hedge, or paraphrase errors."));
}

#[test]
fn synthesis_prompt_omits_error_relay_when_all_tools_succeed() {
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        output: "alpha".to_string(),
        success: true,
    }];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(!prompt.contains("If any tool returned an error, tell the user exactly what went wrong: include the actual error message. Do not soften, hedge, or paraphrase errors."));
}

#[test]
fn synthesis_prompt_error_relay_with_mixed_results() {
    let results = vec![
        ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            output: "alpha".to_string(),
            success: true,
        },
        ToolResult {
            tool_call_id: "call-2".to_string(),
            tool_name: "run_command".to_string(),
            output: "permission denied".to_string(),
            success: false,
        },
    ];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(prompt.contains("If any tool returned an error, tell the user exactly what went wrong: include the actual error message. Do not soften, hedge, or paraphrase errors."));
}

#[test]
fn synthesis_prompt_handles_empty_tool_results() {
    let prompt = tool_synthesis_prompt(&[], "Combine outputs");

    assert!(!prompt.contains("If any tool returned an error, tell the user exactly what went wrong: include the actual error message. Do not soften, hedge, or paraphrase errors."));
    assert!(prompt.contains("Tool results:\n"));
}

#[tokio::test]
async fn reason_returns_completion_response_with_tool_calls() {
    let mut engine = default_engine();
    let llm = MockLlm::new(vec![CompletionResponse {
        content: Vec::new(),
        tool_calls: vec![ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"Cargo.toml"}),
        }],
        usage: None,
        stop_reason: None,
    }]);

    let perception = engine
        .perceive(&base_snapshot("read"))
        .await
        .expect("perceive");
    let response = engine
        .reason(&perception, &llm, CycleStream::disabled())
        .await
        .expect("reason");
    assert_eq!(response.tool_calls.len(), 1);
}

#[tokio::test]
async fn decide_maps_text_response_to_respond_decision() {
    let mut engine = default_engine();
    let response = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "hello".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    };
    let decision = engine.decide(&response).await.expect("decision");
    assert!(matches!(decision, Decision::Respond(text) if text == "hello"));
}

#[tokio::test]
async fn decide_extracts_single_tool_call() {
    let mut engine = default_engine();
    let response = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "ignore me".to_string(),
        }],
        tool_calls: vec![ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"Cargo.toml"}),
        }],
        usage: None,
        stop_reason: None,
    };
    let decision = engine.decide(&response).await.expect("decision");
    assert!(matches!(decision, Decision::UseTools(calls) if calls.len() == 1));
}

#[tokio::test]
async fn decide_no_tool_calls_returns_empty_response() {
    let mut engine = default_engine();
    let response = CompletionResponse {
        content: Vec::new(),
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    };
    let decision = engine.decide(&response).await.expect("decision");
    assert!(matches!(decision, Decision::Respond(text) if text.is_empty()));
}
