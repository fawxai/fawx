use super::*;
use crate::budget::BudgetTracker;
use fx_llm::ToolCall;
use std::sync::Arc;

#[derive(Debug, Default)]
struct ObsNoopExecutor;

#[async_trait::async_trait]
impl ToolExecutor for ObsNoopExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|c| ToolResult {
                tool_call_id: c.id.clone(),
                tool_name: c.name.clone(),
                success: true,
                output: "ok".to_string(),
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
        Vec::new()
    }
}

fn obs_test_engine() -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(ObsNoopExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("test engine build")
}

#[test]
fn emits_tool_failure_with_response_signal() {
    let mut engine = obs_test_engine();
    let action = ActionResult {
        decision: Decision::UseTools(vec![ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "missing.txt"}),
        }]),
        tool_results: vec![ToolResult {
            tool_call_id: "1".to_string(),
            tool_name: "read_file".to_string(),
            success: false,
            output: "file not found".to_string(),
        }],
        response_text: "I couldn't find that file.".to_string(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Continue(ActionContinuation::new(
            Some("I couldn't find that file.".to_string()),
            Some("I couldn't find that file.".to_string()),
        )),
    };

    engine.emit_action_observations(&action);

    let signals = engine.signals.drain_all();
    let obs: Vec<_> = signals
        .iter()
        .filter(|s| s.message == "tool_failure_with_response")
        .collect();
    assert_eq!(obs.len(), 1);
    let failed_count = obs[0]
        .metadata
        .get("failed_tools")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len);
    assert_eq!(failed_count, Some(1));
}

#[test]
fn emits_empty_response_signal() {
    let mut engine = obs_test_engine();
    let action = ActionResult {
        decision: Decision::Respond(String::new()),
        tool_results: Vec::new(),
        response_text: String::new(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Finish(ActionTerminal::Complete {
            response: String::new(),
        }),
    };

    engine.emit_action_observations(&action);

    let signals = engine.signals.drain_all();
    let obs: Vec<_> = signals
        .iter()
        .filter(|s| s.message == "empty_response")
        .collect();
    assert_eq!(obs.len(), 1);
}

#[test]
fn emits_tool_only_turn_signal() {
    let mut engine = obs_test_engine();
    let action = ActionResult {
        decision: Decision::UseTools(vec![ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.txt"}),
        }]),
        tool_results: vec![ToolResult {
            tool_call_id: "1".to_string(),
            tool_name: "read_file".to_string(),
            success: true,
            output: "contents".to_string(),
        }],
        response_text: String::new(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Continue(ActionContinuation::new(
            Some("Completed tool execution: read_file".to_string()),
            Some("Tool execution completed: read_file".to_string()),
        )),
    };

    engine.emit_action_observations(&action);

    let signals = engine.signals.drain_all();
    let obs: Vec<_> = signals
        .iter()
        .filter(|s| s.message == "tool_only_turn")
        .collect();
    assert_eq!(obs.len(), 1);
    let count = obs[0]
        .metadata
        .get("tool_count")
        .and_then(serde_json::Value::as_u64);
    assert_eq!(count, Some(1));
}

#[test]
fn empty_response_treated_as_no_response() {
    let mut engine = obs_test_engine();
    let action = ActionResult {
        decision: Decision::Respond(String::new()),
        tool_results: Vec::new(),
        response_text: String::new(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Finish(ActionTerminal::Complete {
            response: String::new(),
        }),
    };

    engine.emit_action_observations(&action);

    let signals = engine.signals.drain_all();
    let obs: Vec<_> = signals
        .iter()
        .filter(|s| s.message == "empty_response")
        .collect();
    assert_eq!(obs.len(), 1, "empty response should be treated as empty");
}
