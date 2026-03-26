use crate::experiment_panel::ExperimentPanel;
use crate::fawx_backend::{
    friendly_error_message, try_send, BackendEvent, EngineBackend, EngineStatus,
};
use async_trait::async_trait;
use fx_consensus::{format_progress_event, ProgressCallback};
use fx_kernel::{StreamCallback, StreamEvent};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex as StdMutex,
};
use tokio::sync::{mpsc::UnboundedSender, Mutex};

pub(crate) type SharedExperimentPanel = Arc<StdMutex<ExperimentPanel>>;
type ActiveExperimentTools = StdMutex<HashSet<String>>;

pub struct EmbeddedBackend {
    app: Arc<Mutex<fx_cli::headless::HeadlessApp>>,
    experiment_panel: SharedExperimentPanel,
}

impl EmbeddedBackend {
    #[cfg(test)]
    pub fn new(app: fx_cli::headless::HeadlessApp) -> Self {
        Self::with_panel(app, Arc::new(StdMutex::new(ExperimentPanel::new())))
    }

    pub fn build() -> anyhow::Result<(Self, SharedExperimentPanel)> {
        let experiment_panel = Arc::new(StdMutex::new(ExperimentPanel::new()));
        let app = fx_cli::build_headless_app_with_progress(
            None,
            Some(build_experiment_progress_callback(Arc::clone(
                &experiment_panel,
            ))),
        )?;
        Ok((
            Self::with_panel(app, Arc::clone(&experiment_panel)),
            experiment_panel,
        ))
    }

    pub fn with_panel(
        app: fx_cli::headless::HeadlessApp,
        experiment_panel: SharedExperimentPanel,
    ) -> Self {
        Self {
            app: Arc::new(Mutex::new(app)),
            experiment_panel,
        }
    }

    async fn engine_status(&self) -> EngineStatus {
        let app = self.app.lock().await;
        let data_dir =
            fx_cli::headless::configured_data_dir(&fx_cli::headless::fawx_data_dir(), app.config());
        let memory_entries =
            fx_cli::persisted_memory_entry_count(&data_dir.join("memory").join("memory.json"));
        EngineStatus {
            status: "running".to_string(),
            model: app.active_model().to_string(),
            memory_entries,
        }
    }
}

#[async_trait]
impl EngineBackend for EmbeddedBackend {
    async fn stream_message(&self, message: String, tx: UnboundedSender<BackendEvent>) {
        let mut app = self.app.lock().await;
        let saw_text_delta = Arc::new(AtomicBool::new(false));
        let callback = build_stream_callback(
            tx.clone(),
            Arc::clone(&saw_text_delta),
            Arc::clone(&self.experiment_panel),
        );
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

pub(crate) fn build_experiment_progress_callback(
    experiment_panel: SharedExperimentPanel,
) -> ProgressCallback {
    Arc::new(move |event| push_progress_line(&experiment_panel, format_progress_event(event)))
}

fn build_stream_callback(
    tx: UnboundedSender<BackendEvent>,
    saw_text_delta: Arc<AtomicBool>,
    experiment_panel: SharedExperimentPanel,
) -> StreamCallback {
    // Per-message scope: each stream_message gets its own active set.
    // Experiment tool calls don't span multiple messages.
    let active_experiments = StdMutex::new(HashSet::new());
    Arc::new(move |event| {
        handle_stream_event(
            &tx,
            saw_text_delta.as_ref(),
            &experiment_panel,
            &active_experiments,
            event,
        )
    })
}

fn handle_stream_event(
    tx: &UnboundedSender<BackendEvent>,
    saw_text_delta: &AtomicBool,
    experiment_panel: &SharedExperimentPanel,
    active_experiments: &ActiveExperimentTools,
    event: StreamEvent,
) {
    match event {
        StreamEvent::TextDelta { text } => send_text_delta(tx, saw_text_delta, text),
        StreamEvent::ToolCallStart { id, name } => {
            track_experiment_tool(active_experiments, experiment_panel, &id, &name);
            send_tool_call_start(tx, name);
        }
        StreamEvent::ToolCallComplete {
            id,
            name,
            arguments,
        } => {
            track_experiment_tool(active_experiments, experiment_panel, &id, &name);
            send_tool_call_complete(tx, name, &arguments);
        }
        StreamEvent::ToolResult {
            id,
            output,
            is_error,
        } => {
            complete_experiment_tool(active_experiments, experiment_panel, &id);
            if !is_error {
                send_tool_result(tx, None, output, true);
            }
        }
        StreamEvent::ToolError { tool_name, error } => {
            tracing::warn!(tool = %tool_name, "tool error in embedded mode: {error}");
            send_tool_result(tx, Some(tool_name), error, false);
        }
        StreamEvent::Error { message, .. } => {
            tracing::warn!("stream error in embedded mode: {message}");
        }
        StreamEvent::Done { .. }
        | StreamEvent::PhaseChange { .. }
        | StreamEvent::PermissionPrompt(_) => {}
    }
}

fn track_experiment_tool(
    active_experiments: &ActiveExperimentTools,
    experiment_panel: &SharedExperimentPanel,
    tool_id: &str,
    tool_name: &str,
) {
    if tool_name != "run_experiment" {
        return;
    }
    let Ok(mut active) = active_experiments.lock() else {
        return;
    };
    if active.insert(tool_id.to_string()) {
        clear_experiment_panel(experiment_panel);
    }
}

fn complete_experiment_tool(
    active_experiments: &ActiveExperimentTools,
    experiment_panel: &SharedExperimentPanel,
    tool_id: &str,
) {
    let Ok(mut active) = active_experiments.lock() else {
        return;
    };
    if active.remove(tool_id) {
        mark_experiment_complete(experiment_panel);
    }
}

fn push_progress_line(experiment_panel: &SharedExperimentPanel, line: String) {
    let Ok(mut panel) = experiment_panel.lock() else {
        return;
    };
    panel.push_line(line);
}

fn clear_experiment_panel(experiment_panel: &SharedExperimentPanel) {
    let Ok(mut panel) = experiment_panel.lock() else {
        return;
    };
    panel.clear();
}

fn mark_experiment_complete(experiment_panel: &SharedExperimentPanel) {
    let Ok(mut panel) = experiment_panel.lock() else {
        return;
    };
    panel.mark_complete();
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

fn send_tool_result(
    tx: &UnboundedSender<BackendEvent>,
    name: Option<String>,
    output: String,
    success: bool,
) {
    try_send(
        tx,
        BackendEvent::ToolResult {
            name,
            success,
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
    use std::path::{Path, PathBuf};
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
        test_headless_app_with_provider_and_config(TestProvider, FawxConfig::default())
    }

    fn test_headless_app_with_provider(provider: impl CompletionProvider + 'static) -> HeadlessApp {
        test_headless_app_with_provider_and_config(provider, FawxConfig::default())
    }

    fn test_headless_app_with_provider_and_config(
        provider: impl CompletionProvider + 'static,
        config: FawxConfig,
    ) -> HeadlessApp {
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

    fn headless_app_with_data_dir(data_dir: PathBuf) -> HeadlessApp {
        let mut config = FawxConfig::default();
        config.general.data_dir = Some(data_dir);
        test_headless_app_with_provider_and_config(TestProvider, config)
    }

    fn write_memory_entries(data_dir: &Path, count: usize) {
        let memory_dir = data_dir.join("memory");
        std::fs::create_dir_all(&memory_dir).expect("create memory dir");
        let mut store = serde_json::Map::new();
        for index in 0..count {
            store.insert(
                format!("memory-{index}"),
                serde_json::json!({
                    "value": format!("entry-{index}"),
                    "created_at_ms": 1,
                    "last_accessed_at_ms": 2,
                    "access_count": 3,
                    "source": "User",
                    "tags": []
                }),
            );
        }
        std::fs::write(
            memory_dir.join("memory.json"),
            serde_json::Value::Object(store).to_string(),
        )
        .expect("write memory entries");
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

    fn test_experiment_panel() -> SharedExperimentPanel {
        Arc::new(StdMutex::new(ExperimentPanel::new()))
    }

    fn test_active_experiments() -> ActiveExperimentTools {
        StdMutex::new(HashSet::new())
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
        let experiment_panel = test_experiment_panel();
        let active_experiments = test_active_experiments();

        handle_stream_event(
            &tx,
            &saw_text_delta,
            &experiment_panel,
            &active_experiments,
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
        let experiment_panel = test_experiment_panel();
        let active_experiments = test_active_experiments();

        handle_stream_event(
            &tx,
            &saw_text_delta,
            &experiment_panel,
            &active_experiments,
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
    async fn handle_stream_event_maps_successful_tool_result_to_backend_tool_result() {
        let (tx, mut rx) = unbounded_channel();
        let saw_text_delta = AtomicBool::new(false);
        let experiment_panel = test_experiment_panel();
        let active_experiments = test_active_experiments();

        handle_stream_event(
            &tx,
            &saw_text_delta,
            &experiment_panel,
            &active_experiments,
            StreamEvent::ToolResult {
                id: "call-1".to_string(),
                output: "file contents".to_string(),
                is_error: false,
            },
        );

        match recv_event(&mut rx).await {
            BackendEvent::ToolResult {
                name,
                success,
                content,
            } => {
                assert!(name.is_none());
                assert!(success);
                assert_eq!(content, "file contents");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_stream_event_maps_tool_error_to_backend_tool_result() {
        let (tx, mut rx) = unbounded_channel();
        let saw_text_delta = AtomicBool::new(false);
        let experiment_panel = test_experiment_panel();
        let active_experiments = test_active_experiments();

        handle_stream_event(
            &tx,
            &saw_text_delta,
            &experiment_panel,
            &active_experiments,
            StreamEvent::ToolError {
                tool_name: "read".to_string(),
                error: "denied".to_string(),
            },
        );

        match recv_event(&mut rx).await {
            BackendEvent::ToolResult {
                name,
                success,
                content,
            } => {
                assert_eq!(name.as_deref(), Some("read"));
                assert!(!success);
                assert_eq!(content, "denied");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn progress_callback_formats_events_into_panel_lines() {
        let experiment_panel = test_experiment_panel();
        let callback = build_experiment_progress_callback(Arc::clone(&experiment_panel));
        let event = fx_consensus::ProgressEvent::RoundStarted {
            round: 1,
            max_rounds: 3,
            signal: "signal".to_string(),
        };

        callback(&event);

        let panel = experiment_panel.lock().expect("experiment panel");
        assert_eq!(panel.lines(), &[format_progress_event(&event)]);
        assert!(panel.is_visible());
    }

    #[test]
    fn run_experiment_tool_start_clears_panel_and_tracks_active_tool() {
        let experiment_panel = test_experiment_panel();
        let active_experiments = test_active_experiments();
        let mut panel = experiment_panel.lock().expect("experiment panel");
        panel.push_line("stale".to_string());
        drop(panel);

        track_experiment_tool(
            &active_experiments,
            &experiment_panel,
            "call-1",
            "run_experiment",
        );

        let panel = experiment_panel.lock().expect("experiment panel");
        assert!(panel.lines().is_empty());
        drop(panel);
        let active = active_experiments.lock().expect("active experiments");
        assert!(active.contains("call-1"));
    }

    #[test]
    fn run_experiment_tool_result_removes_active_tool() {
        let experiment_panel = test_experiment_panel();
        let active_experiments = test_active_experiments();
        track_experiment_tool(
            &active_experiments,
            &experiment_panel,
            "call-1",
            "run_experiment",
        );
        push_progress_line(&experiment_panel, "progress".to_string());

        complete_experiment_tool(&active_experiments, &experiment_panel, "call-1");

        let active = active_experiments.lock().expect("active experiments");
        assert!(!active.contains("call-1"));
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
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn check_health_reads_memory_entry_count_from_disk() {
        let data_dir = unique_temp_dir();
        write_memory_entries(&data_dir, 2);
        let backend = EmbeddedBackend::new(headless_app_with_data_dir(data_dir));
        let (tx, mut rx) = unbounded_channel();

        backend.check_health(tx).await;

        match recv_event(&mut rx).await {
            BackendEvent::Connected(status) => assert_eq!(status.memory_entries, 2),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn check_health_reports_zero_when_memory_store_is_missing() {
        let data_dir = unique_temp_dir();
        let backend = EmbeddedBackend::new(headless_app_with_data_dir(data_dir));
        let (tx, mut rx) = unbounded_channel();

        backend.check_health(tx).await;

        match recv_event(&mut rx).await {
            BackendEvent::Connected(status) => assert_eq!(status.memory_entries, 0),
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
