use crate::act::{TokenUsage, ToolExecutor, ToolResult};
use crate::budget::{BudgetConfig, BudgetTracker};
use crate::cancellation::CancellationToken;
use crate::context_manager::ContextCompactor;
use crate::signals::{LoopStep, Signal, SignalKind};
use async_trait::async_trait;
use fx_llm::ToolCall;
use fx_memory::signal_store::SignalStoreError;
use fx_memory::{SignalSink, SignalStore};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

use super::super::{current_time_ms, LoopEngine, LoopResult};
use crate::types::LoopError;

#[derive(Debug, Default)]
struct StubToolExecutor;

#[async_trait]
impl ToolExecutor for StubToolExecutor {
    async fn execute_tools(
        &self,
        _calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(Vec::new())
    }
}

fn test_engine_with_signal_store<S>(signal_store: S) -> LoopEngine
where
    S: SignalSink + 'static,
{
    LoopEngine::builder()
        .budget(BudgetTracker::new(
            BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .signal_store(signal_store)
        .build()
        .expect("test engine")
}

#[derive(Debug)]
struct FailingSignalSink {
    append_calls: Arc<AtomicUsize>,
}

impl SignalSink for FailingSignalSink {
    fn append(&self, _signals: &[Signal]) -> Result<(), SignalStoreError> {
        self.append_calls.fetch_add(1, Ordering::SeqCst);
        Err(SignalStoreError::FileWrite(io::Error::other(
            "simulated append failure",
        )))
    }

    fn flush(&self) -> Result<(), SignalStoreError> {
        Ok(())
    }

    fn session_id(&self) -> Option<&str> {
        Some("failing-sink")
    }
}

#[test]
fn finalize_result_persists_drained_signals_to_store() {
    let tmp = TempDir::new().expect("tempdir");
    let store = SignalStore::open(tmp.path(), "loop-session").expect("signal store");
    let mut engine = test_engine_with_signal_store(store);

    let signal_id = engine.emit_signal(
        LoopStep::Act,
        SignalKind::Success,
        "cycle completed",
        serde_json::json!({"source": "test"}),
    );

    let result = engine.finalize_result(LoopResult::Complete {
        response: "ok".to_string(),
        iterations: 1,
        tokens_used: TokenUsage::default(),
        signals: Vec::new(),
    });

    let persisted = SignalStore::read_session(tmp.path(), "loop-session").expect("read session");
    assert_eq!(persisted, result.signals().to_vec());
    assert_eq!(persisted.len(), 2);
    assert_eq!(persisted[0].id, signal_id);
    assert_eq!(persisted[1].metadata["decision_kind"], "turn_stop");
    assert_eq!(persisted[1].metadata["decision"], "completed");
    assert_eq!(persisted[1].metadata["failed"], false);
}

#[test]
fn finalize_result_swallows_signal_store_write_failures() {
    let append_calls = Arc::new(AtomicUsize::new(0));
    let store = FailingSignalSink {
        append_calls: Arc::clone(&append_calls),
    };
    let mut engine = test_engine_with_signal_store(store);

    engine.emit_signal(
        LoopStep::Act,
        SignalKind::Friction,
        "persist me if you can",
        serde_json::json!({"source": "test"}),
    );

    let result = engine.finalize_result(LoopResult::Complete {
        response: "still ok".to_string(),
        iterations: 1,
        tokens_used: TokenUsage::default(),
        signals: Vec::new(),
    });

    assert!(matches!(result, LoopResult::Complete { .. }));
    assert_eq!(append_calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.signals().len(), 2);
    let stop = result
        .signals()
        .iter()
        .find(|signal| signal.metadata["decision_kind"] == "turn_stop")
        .expect("turn stop signal");
    assert_eq!(stop.metadata["decision"], "completed");
    assert_eq!(stop.metadata["failed"], false);
}

#[test]
fn finalize_error_result_persists_decide_failure_to_store() {
    let tmp = TempDir::new().expect("tempdir");
    let store = SignalStore::open(tmp.path(), "error-session").expect("signal store");
    let mut engine = test_engine_with_signal_store(store);

    engine.finalize_error_result(&LoopError {
        stage: "decide".to_string(),
        reason: "invalid decision payload".to_string(),
        recoverable: false,
    });

    let persisted = SignalStore::read_session(tmp.path(), "error-session").expect("read session");
    assert_eq!(persisted.len(), 2);
    assert_eq!(persisted[0].step, LoopStep::Decide);
    assert_eq!(persisted[0].kind, SignalKind::Blocked);
    assert_eq!(persisted[0].message, "loop error");
    assert_eq!(persisted[0].metadata["stage"], "decide");
    assert_eq!(persisted[0].metadata["reason"], "invalid decision payload");

    let stop = &persisted[1];
    assert_eq!(stop.metadata["decision_kind"], "turn_stop");
    assert_eq!(stop.metadata["decision"], "failed");
    assert_eq!(stop.metadata["failed"], true);
    assert_eq!(stop.metadata["result_kind"], "error");
    assert_eq!(stop.metadata["recoverable"], false);
}
