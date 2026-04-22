use super::*;
use crate::budget::BudgetTracker;
use crate::signals::{LoopStep, Signal, SignalKind};
use fx_llm::ToolCall;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
        Vec::new()
    }
}

#[derive(Debug)]
struct ObsSignalExecutor {
    emitted_signals: Mutex<HashMap<String, Vec<Signal>>>,
}

impl ObsSignalExecutor {
    fn new(signal: Signal) -> Self {
        let mut emitted_signals = HashMap::new();
        emitted_signals.insert("1".to_string(), vec![signal]);
        Self {
            emitted_signals: Mutex::new(emitted_signals),
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for ObsSignalExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult::success(call.id.clone(), call.name.clone(), "ok"))
            .collect())
    }

    fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
        Vec::new()
    }

    fn take_emitted_signals(&self, call_id: &str) -> Option<Vec<Signal>> {
        self.emitted_signals
            .lock()
            .expect("emitted_signals lock")
            .remove(call_id)
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
            failure_class: None,
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
            failure_class: None,
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

#[test]
fn imported_tool_signals_attach_to_base_tool_signal() {
    let signal = Signal::new(
        LoopStep::Act,
        SignalKind::MemoryHit,
        "memory search returned relevant results",
        serde_json::json!({
            "query": "project auth",
            "result_count": 1,
        }),
        current_time_ms(),
    );
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(ObsSignalExecutor::new(signal)))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("test engine build");
    let calls = vec![ToolCall {
        id: "1".to_string(),
        name: "memory_search".to_string(),
        arguments: serde_json::json!({"query": "project auth"}),
    }];
    let results = vec![ToolResult::success(
        "1",
        "memory_search",
        "Found 1 relevant memories",
    )];

    engine.capture_tool_execution_diagnostics(&results);
    engine.emit_action_signals(&calls, &results);

    let signals = engine.signals.drain_all();
    let base_signal = signals
        .iter()
        .find(|signal| signal.kind == SignalKind::Success)
        .expect("base tool signal");
    let memory_signal = signals
        .iter()
        .find(|signal| signal.kind == SignalKind::MemoryHit)
        .expect("memory hit signal");

    assert_eq!(memory_signal.metadata["query"], "project auth");
    assert_eq!(memory_signal.cause_id, Some(base_signal.id));
}
