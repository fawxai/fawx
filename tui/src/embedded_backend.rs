use async_trait::async_trait;
use fx_kernel::{StreamCallback, StreamEvent};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{mpsc::UnboundedSender, Mutex};

use crate::fawx_backend::{
    friendly_error_message, try_send, BackendEvent, EngineBackend, EngineStatus,
};

pub struct EmbeddedBackend {
    app: Arc<Mutex<fx_cli::headless::HeadlessApp>>,
}

impl EmbeddedBackend {
    pub fn new(app: fx_cli::headless::HeadlessApp) -> Self {
        Self {
            app: Arc::new(Mutex::new(app)),
        }
    }

    async fn engine_status(&self) -> EngineStatus {
        let app = self.app.lock().await;
        EngineStatus {
            status: "running".to_string(),
            model: app.active_model().to_string(),
            memory_entries: 0,
        }
    }
}

#[async_trait]
impl EngineBackend for EmbeddedBackend {
    async fn stream_message(&self, message: String, tx: UnboundedSender<BackendEvent>) {
        let mut app = self.app.lock().await;
        let saw_text_delta = Arc::new(AtomicBool::new(false));
        let callback = build_stream_callback(tx.clone(), Arc::clone(&saw_text_delta));
        let result = fx_cli::headless::process_input_with_commands_streaming(
            &mut app, &message, None, callback,
        )
        .await;

        match result {
            Ok(result) => {
                emit_unstreamed_response(&tx, saw_text_delta.as_ref(), &result.response);
                send_done_event(&tx, result);
            }
            Err(error) => {
                try_send(
                    &tx,
                    BackendEvent::StreamError(friendly_error_message(&error.to_string())),
                );
            }
        }
    }

    async fn check_health(&self, tx: UnboundedSender<BackendEvent>) {
        try_send(&tx, BackendEvent::Connected(self.engine_status().await));
    }
}

fn build_stream_callback(
    tx: UnboundedSender<BackendEvent>,
    saw_text_delta: Arc<AtomicBool>,
) -> StreamCallback {
    Arc::new(move |event| handle_stream_event(&tx, saw_text_delta.as_ref(), event))
}

fn handle_stream_event(
    tx: &UnboundedSender<BackendEvent>,
    saw_text_delta: &AtomicBool,
    event: StreamEvent,
) {
    match event {
        StreamEvent::TextDelta { text } => send_text_delta(tx, saw_text_delta, text),
        StreamEvent::ToolCallStart { name, .. } => send_tool_call_start(tx, name),
        StreamEvent::ToolCallComplete {
            name, arguments, ..
        } => send_tool_call_complete(tx, name, &arguments),
        StreamEvent::ToolResult {
            output, is_error, ..
        } => send_tool_result(tx, output, is_error),
        StreamEvent::Done { .. } | StreamEvent::PhaseChange { .. } => {}
    }
}

fn send_text_delta(tx: &UnboundedSender<BackendEvent>, saw_text_delta: &AtomicBool, text: String) {
    saw_text_delta.store(true, Ordering::Relaxed);
    try_send(tx, BackendEvent::TextDelta(text));
}

fn send_tool_call_start(tx: &UnboundedSender<BackendEvent>, name: String) {
    try_send(
        tx,
        BackendEvent::ToolUse {
            name,
            arguments: Value::Object(Default::default()),
        },
    );
}

fn send_tool_call_complete(tx: &UnboundedSender<BackendEvent>, name: String, arguments: &str) {
    try_send(
        tx,
        BackendEvent::ToolUse {
            name,
            arguments: parse_tool_arguments(arguments),
        },
    );
}

fn send_tool_result(tx: &UnboundedSender<BackendEvent>, output: String, is_error: bool) {
    try_send(
        tx,
        BackendEvent::ToolResult {
            name: None,
            success: !is_error,
            content: output,
        },
    );
}

fn emit_unstreamed_response(
    tx: &UnboundedSender<BackendEvent>,
    saw_text_delta: &AtomicBool,
    response: &str,
) {
    if !saw_text_delta.load(Ordering::Relaxed) && !response.is_empty() {
        try_send(tx, BackendEvent::TextDelta(response.to_string()));
    }
}

fn parse_tool_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or(Value::Null)
}

fn send_done_event(tx: &UnboundedSender<BackendEvent>, result: fx_cli::headless::CycleResult) {
    try_send(
        tx,
        BackendEvent::Done {
            model: Some(result.model),
            iterations: Some(result.iterations),
            input_tokens: Some(result.tokens_used.input_tokens),
            output_tokens: Some(result.tokens_used.output_tokens),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_cli::headless::{
        HeadlessApp, HeadlessAppDeps, HeadlessSubagentFactory, HeadlessSubagentFactoryDeps,
    };
    use fx_config::{test_support::CurrentDirGuard, FawxConfig};
    use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
    use fx_kernel::cancellation::CancellationToken;
    use fx_kernel::context_manager::ContextCompactor;
    use fx_kernel::loop_engine::LoopEngine;
    use fx_kernel::{budget::BudgetConfig, budget::BudgetTracker};
    use fx_llm::{
        CompletionProvider, CompletionRequest, CompletionResponse, CompletionStream, ContentBlock,
        ModelRouter, ProviderCapabilities, ProviderError, StreamChunk, Usage,
    };
    use fx_subagent::{SubagentLimits, SubagentManager, SubagentManagerDeps};
    use std::path::PathBuf;
    use std::sync::{atomic::AtomicBool, Arc};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    #[derive(Debug)]
    struct StubToolExecutor;

    #[async_trait]
    impl ToolExecutor for StubToolExecutor {
        async fn execute_tools(
            &self,
            _calls: &[fx_llm::ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(Vec::new())
        }
    }

    #[derive(Debug)]
    struct TestProvider;

    #[async_trait]
    impl CompletionProvider for TestProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            Ok(CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: response_payload(),
                }],
                tool_calls: Vec::new(),
                usage: Some(test_usage()),
                stop_reason: Some("end_turn".to_string()),
            })
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, ProviderError> {
            let chunks = streamed_response_chunks()
                .into_iter()
                .map(Ok)
                .collect::<Vec<_>>();
            Ok(Box::pin(futures::stream::iter(chunks)))
        }

        fn name(&self) -> &str {
            "embedded-test"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["mock-model".to_string()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }
    }

    #[derive(Debug)]
    struct ErrorProvider;

    #[async_trait]
    impl CompletionProvider for ErrorProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            Err(ProviderError::Provider("stream failed".to_string()))
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, ProviderError> {
            Err(ProviderError::Provider("stream failed".to_string()))
        }

        fn name(&self) -> &str {
            "embedded-error"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["mock-model".to_string()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }
    }

    fn response_payload() -> String {
        r#"{"action":{"Respond":{"text":"ok"}},"rationale":"r","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#.to_string()
    }

    fn streamed_response_chunks() -> Vec<StreamChunk> {
        let response = response_payload();
        let midpoint = response.len() / 2;
        vec![
            StreamChunk {
                delta_content: Some(response[..midpoint].to_string()),
                ..Default::default()
            },
            StreamChunk {
                delta_content: Some(response[midpoint..].to_string()),
                usage: Some(test_usage()),
                stop_reason: Some("end_turn".to_string()),
                ..Default::default()
            },
        ]
    }

    fn test_usage() -> Usage {
        Usage {
            input_tokens: 7,
            output_tokens: 11,
        }
    }

    fn test_engine() -> LoopEngine {
        LoopEngine::builder()
            .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
            .context(ContextCompactor::new(2048, 256))
            .max_iterations(3)
            .tool_executor(Arc::new(StubToolExecutor))
            .synthesis_instruction("Summarize".to_string())
            .build()
            .expect("test engine")
    }

    fn test_router_with_provider(provider: impl CompletionProvider + 'static) -> Arc<ModelRouter> {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(provider));
        router.set_active("mock-model").expect("set active model");
        Arc::new(router)
    }

    fn test_subagent_manager(
        router: Arc<ModelRouter>,
        config: &FawxConfig,
    ) -> Arc<SubagentManager> {
        let factory = HeadlessSubagentFactory::new(HeadlessSubagentFactoryDeps {
            router,
            config: config.clone(),
            improvement_provider: None,
        });
        Arc::new(SubagentManager::new(SubagentManagerDeps {
            factory: Arc::new(factory),
            limits: SubagentLimits::default(),
        }))
    }

    fn test_headless_app() -> HeadlessApp {
        test_headless_app_with_provider(TestProvider)
    }

    fn test_headless_app_with_provider(provider: impl CompletionProvider + 'static) -> HeadlessApp {
        let config = FawxConfig::default();
        let router = test_router_with_provider(provider);
        let subagent_manager = test_subagent_manager(Arc::clone(&router), &config);
        HeadlessApp::new(HeadlessAppDeps {
            loop_engine: test_engine(),
            router,
            config,
            memory: None,
            embedding_index_persistence: None,
            system_prompt_path: None,
            config_manager: None,
            system_prompt_text: None,
            subagent_manager,
            canary_monitor: None,
        })
        .expect("headless app")
    }

    fn unique_temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "fawx-embedded-backend-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    async fn recv_event(rx: &mut UnboundedReceiver<BackendEvent>) -> BackendEvent {
        rx.recv().await.expect("backend event")
    }

    async fn recv_events_until_done(rx: &mut UnboundedReceiver<BackendEvent>) -> Vec<BackendEvent> {
        let mut events = Vec::new();
        loop {
            let event = recv_event(rx).await;
            let done = matches!(event, BackendEvent::Done { .. });
            events.push(event);
            if done {
                return events;
            }
        }
    }

    #[test]
    fn prepare_embedded_config_defaults_working_dir_to_process_current_dir() {
        let temp_dir = unique_temp_dir();
        let _guard = CurrentDirGuard::set(&temp_dir).expect("set current dir");

        let config = fx_cli::prepare_embedded_config(FawxConfig::default());

        assert_eq!(config.tools.working_dir, Some(temp_dir));
    }

    #[tokio::test]
    async fn handle_stream_event_maps_tool_call_start_to_tool_use() {
        let (tx, mut rx) = unbounded_channel();
        let saw_text_delta = AtomicBool::new(false);

        handle_stream_event(
            &tx,
            &saw_text_delta,
            StreamEvent::ToolCallStart {
                id: "call-1".to_string(),
                name: "read".to_string(),
            },
        );

        match recv_event(&mut rx).await {
            BackendEvent::ToolUse { name, arguments } => {
                assert_eq!(name, "read");
                assert_eq!(arguments, serde_json::json!({}));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_stream_event_maps_tool_call_complete_to_tool_use() {
        let (tx, mut rx) = unbounded_channel();
        let saw_text_delta = AtomicBool::new(false);

        handle_stream_event(
            &tx,
            &saw_text_delta,
            StreamEvent::ToolCallComplete {
                id: "call-1".to_string(),
                name: "read".to_string(),
                arguments: r#"{"path":"Cargo.toml"}"#.to_string(),
            },
        );

        match recv_event(&mut rx).await {
            BackendEvent::ToolUse { name, arguments } => {
                assert_eq!(name, "read");
                assert_eq!(arguments, serde_json::json!({"path": "Cargo.toml"}));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_stream_event_maps_tool_result_to_backend_tool_result() {
        let (tx, mut rx) = unbounded_channel();
        let saw_text_delta = AtomicBool::new(false);

        handle_stream_event(
            &tx,
            &saw_text_delta,
            StreamEvent::ToolResult {
                id: "call-1".to_string(),
                output: "denied".to_string(),
                is_error: true,
            },
        );

        match recv_event(&mut rx).await {
            BackendEvent::ToolResult {
                name,
                success,
                content,
            } => {
                assert!(name.is_none());
                assert!(!success);
                assert_eq!(content, "denied");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_message_emits_stream_error_when_streaming_fails() {
        let backend = EmbeddedBackend::new(test_headless_app_with_provider(ErrorProvider));
        let (tx, mut rx) = unbounded_channel();

        backend.stream_message("hello".to_string(), tx).await;

        match recv_event(&mut rx).await {
            BackendEvent::StreamError(message) => assert!(message.contains("stream failed")),
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn check_health_reports_connected_status() {
        let backend = EmbeddedBackend::new(test_headless_app());
        let (tx, mut rx) = unbounded_channel();

        backend.check_health(tx).await;

        match recv_event(&mut rx).await {
            BackendEvent::Connected(status) => {
                assert_eq!(status.status, "running");
                assert_eq!(status.model, "mock-model");
                assert_eq!(status.memory_entries, 0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn slash_commands_are_processed_via_headless_command_pipeline() {
        let backend = EmbeddedBackend::new(test_headless_app());
        let (tx, mut rx) = unbounded_channel();

        backend.stream_message("/status".to_string(), tx).await;

        match recv_event(&mut rx).await {
            BackendEvent::TextDelta(text) => assert!(text.contains("Fawx Status")),
            other => panic!("unexpected event: {other:?}"),
        }
        match recv_event(&mut rx).await {
            BackendEvent::Done {
                iterations,
                input_tokens,
                output_tokens,
                ..
            } => {
                assert_eq!(iterations, Some(0));
                assert_eq!(input_tokens, Some(0));
                assert_eq!(output_tokens, Some(0));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn regular_messages_stream_text_delta_events() {
        let backend = EmbeddedBackend::new(test_headless_app());
        let (tx, mut rx) = unbounded_channel();

        backend.stream_message("hello".to_string(), tx).await;

        let events = recv_events_until_done(&mut rx).await;
        let text_deltas = events
            .iter()
            .filter_map(|event| match event {
                BackendEvent::TextDelta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let combined = text_deltas.concat();
        // At least one text delta must arrive (streaming or fallback).
        // With real providers, multiple deltas arrive per-token.
        assert!(
            !text_deltas.is_empty(),
            "expected at least one text delta event"
        );
        assert!(combined.contains("\"ok\""));

        match events.last().expect("done event") {
            BackendEvent::Done {
                model,
                iterations,
                input_tokens,
                output_tokens,
            } => {
                assert_eq!(model.as_deref(), Some("mock-model"));
                assert!(iterations.expect("iterations") > 0);
                assert!(input_tokens.expect("input tokens") > 0);
                assert!(output_tokens.expect("output tokens") > 0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
