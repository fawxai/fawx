use async_trait::async_trait;
use tokio::sync::{mpsc::UnboundedSender, Mutex};

use crate::fawx_backend::{
    friendly_error_message, try_send, BackendEvent, EngineBackend, EngineStatus,
};

pub struct EmbeddedBackend {
    app: std::sync::Arc<Mutex<fx_cli::headless::HeadlessApp>>,
}

impl EmbeddedBackend {
    pub fn new(app: fx_cli::headless::HeadlessApp) -> Self {
        Self {
            app: std::sync::Arc::new(Mutex::new(app)),
        }
    }

    async fn process_message(
        &self,
        message: &str,
    ) -> anyhow::Result<fx_cli::headless::CycleResult> {
        let mut app = self.app.lock().await;
        fx_cli::headless::process_input_with_commands(&mut app, message, None).await
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
        match self.process_message(&message).await {
            Ok(result) => send_cycle_result(&tx, result),
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

fn send_cycle_result(tx: &UnboundedSender<BackendEvent>, result: fx_cli::headless::CycleResult) {
    let fx_cli::headless::CycleResult {
        response,
        model,
        iterations,
        tokens_used,
    } = result;
    try_send(tx, BackendEvent::TextDelta(response));
    try_send(
        tx,
        BackendEvent::Done {
            model: Some(model),
            iterations: Some(iterations),
            input_tokens: Some(tokens_used.input_tokens),
            output_tokens: Some(tokens_used.output_tokens),
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
    use fx_config::FawxConfig;
    use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
    use fx_kernel::budget::{BudgetConfig, BudgetTracker};
    use fx_kernel::cancellation::CancellationToken;
    use fx_kernel::context_manager::ContextCompactor;
    use fx_kernel::loop_engine::LoopEngine;
    use fx_llm::{
        CompletionProvider, CompletionRequest, CompletionResponse, CompletionStream, ContentBlock,
        ModelRouter, ProviderCapabilities, ProviderError, StreamChunk, Usage,
    };
    use fx_subagent::{SubagentLimits, SubagentManager, SubagentManagerDeps};
    use std::sync::Arc;
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
            let chunk = StreamChunk {
                delta_content: Some(response_payload()),
                tool_use_deltas: Vec::new(),
                usage: Some(test_usage()),
                stop_reason: Some("end_turn".to_string()),
            };
            Ok(Box::pin(futures::stream::iter(vec![Ok(chunk)])))
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

    fn response_payload() -> String {
        r#"{"action":{"Respond":{"text":"ok"}},"rationale":"r","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#.to_string()
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

    fn test_router() -> Arc<ModelRouter> {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(TestProvider));
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
        let config = FawxConfig::default();
        let router = test_router();
        let subagent_manager = test_subagent_manager(Arc::clone(&router), &config);
        HeadlessApp::new(HeadlessAppDeps {
            loop_engine: test_engine(),
            router,
            config,
            memory: None,
            system_prompt_path: None,
            config_manager: None,
            system_prompt_text: None,
            subagent_manager,
            canary_monitor: None,
        })
        .expect("headless app")
    }

    async fn recv_event(rx: &mut UnboundedReceiver<BackendEvent>) -> BackendEvent {
        rx.recv().await.expect("backend event")
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
    async fn regular_messages_are_processed_via_headless_message_pipeline() {
        let backend = EmbeddedBackend::new(test_headless_app());
        let (tx, mut rx) = unbounded_channel();

        backend.stream_message("hello".to_string(), tx).await;

        match recv_event(&mut rx).await {
            BackendEvent::TextDelta(text) => assert!(text.contains("\"ok\"")),
            other => panic!("unexpected event: {other:?}"),
        }
        match recv_event(&mut rx).await {
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
