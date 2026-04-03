use super::*;
use async_trait::async_trait;
use fx_core::error::LlmError as CoreLlmError;
use fx_llm::{CompletionRequest, CompletionResponse, ContentBlock, ProviderError, ToolCall};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug)]
struct EmptySummaryInspectionLlm {
    responses: Mutex<VecDeque<CompletionResponse>>,
    complete_calls: AtomicUsize,
    generate_calls: AtomicUsize,
}

impl EmptySummaryInspectionLlm {
    fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            complete_calls: AtomicUsize::new(0),
            generate_calls: AtomicUsize::new(0),
        }
    }

    fn complete_calls(&self) -> usize {
        self.complete_calls.load(Ordering::SeqCst)
    }

    fn generate_calls(&self) -> usize {
        self.generate_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl LlmProvider for EmptySummaryInspectionLlm {
    async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
        self.generate_calls.fetch_add(1, Ordering::SeqCst);
        Ok(String::new())
    }

    async fn generate_streaming(
        &self,
        _: &str,
        _: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, CoreLlmError> {
        self.generate_calls.fetch_add(1, Ordering::SeqCst);
        callback(String::new());
        Ok(String::new())
    }

    fn model_name(&self) -> &str {
        "empty-summary-inspection"
    }

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        self.complete_calls.fetch_add(1, Ordering::SeqCst);
        self.responses
            .lock()
            .expect("responses lock")
            .pop_front()
            .ok_or_else(|| ProviderError::Provider("no scripted response".to_string()))
    }
}

#[tokio::test]
async fn direct_inspection_successful_read_file_completes_terminally() {
    let prompt = "Read ~/.zshrc and tell me exactly what it says.";
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");
    let decision = Decision::UseTools(vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"~/.zshrc"}),
    }]);
    let llm = RecordingLlm::ok(vec![text_response("The file says alias ll='ls -la'.")]);

    let action = engine
        .act(
            &decision,
            &llm,
            &processed.context_window,
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            assert_eq!(response, "The file says alias ll='ls -la'.");
        }
        other => panic!("expected terminal completion, got {other:?}"),
    }
    assert_eq!(llm.requests().len(), 1);
}

#[tokio::test]
async fn direct_inspection_with_mixed_text_terminates_terminally() {
    let prompt = "Read ~/.zshrc and explain what each line does.";
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let llm = RecordingLlm::ok(vec![
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "Here is what I found so far.".to_string(),
            }],
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"~/.zshrc"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        },
        text_response("Line 1 configures the shell environment."),
    ]);

    let result = engine
        .run_cycle(test_snapshot(prompt), &llm)
        .await
        .expect("run_cycle should succeed");

    match result {
        LoopResult::Complete {
            response,
            iterations,
            ..
        } => {
            assert_eq!(iterations, 1);
            assert_eq!(
                response,
                "Here is what I found so far.\n\nLine 1 configures the shell environment."
            );
        }
        other => panic!("expected terminal completion, got {other:?}"),
    }
    assert_eq!(llm.requests().len(), 2);
}

#[tokio::test]
async fn direct_inspection_does_not_request_mutation_only_scope_after_observation() {
    let prompt = "Read ~/.zshrc and tell me exactly what it says.";
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let _processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");
    let mut state = ToolRoundState::new(&[], &[Message::user(prompt)], None);
    state.used_observation_tools = true;

    assert_eq!(engine.continuation_tool_scope_for_round(&state), None);
}

#[tokio::test]
async fn direct_inspection_empty_post_tool_response_gets_one_synthesis_pass_then_completes() {
    let prompt = "Read ~/.zshrc and tell me exactly what it says.";
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");
    let decision = Decision::UseTools(vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"~/.zshrc"}),
    }]);
    let llm = EmptySummaryInspectionLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: String::new(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let action = engine
        .act(
            &decision,
            &llm,
            &processed.context_window,
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            assert_eq!(response, "Inspection completed but produced no summary.");
        }
        other => panic!("expected terminal completion after one synthesis pass, got {other:?}"),
    }
    assert_eq!(llm.complete_calls(), 1);
    assert_eq!(llm.generate_calls(), 1);
}

#[tokio::test]
async fn standard_turns_still_continue_normally_after_observation_only_tool_rounds() {
    let prompt = "Research first, then implement.";
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");
    let decision = Decision::UseTools(vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }]);
    let llm = RecordingLlm::ok(vec![text_response(
        "I have enough context to implement it now.",
    )]);

    let action = engine
        .act(
            &decision,
            &llm,
            &processed.context_window,
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    match action.next_step {
        ActionNextStep::Continue(continuation) => {
            assert_eq!(
                continuation.next_tool_scope,
                Some(ContinuationToolScope::MutationOnly)
            );
        }
        other => panic!("expected standard continuation, got {other:?}"),
    }
}
