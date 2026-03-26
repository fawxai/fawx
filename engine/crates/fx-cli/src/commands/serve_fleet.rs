use super::fleet::{build_registration_request, default_fleet_dir, identity_path};
use async_trait::async_trait;
use fx_fleet::{
    FleetError, FleetHeartbeat, FleetHttpClient, FleetIdentity, FleetRegistrationRequest,
    FleetRegistrationResponse, FleetTaskRequest, FleetTaskResult, FleetTaskStatus, FleetTaskType,
    WorkerState,
};
use serde_json::json;
#[cfg(test)]
use std::sync::{LazyLock, Mutex};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use tokio::time::{self, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing;

const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
static TEST_EXIT_CODE: LazyLock<Mutex<Option<i32>>> = LazyLock::new(|| Mutex::new(None));

#[derive(Debug, Clone, Copy)]
struct WorkerLoopConfig {
    heartbeat_interval: Duration,
    poll_interval: Duration,
}

impl Default for WorkerLoopConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }
}

#[derive(Debug, Clone)]
struct WorkerStateSnapshot {
    status: WorkerState,
    current_task: Option<String>,
}

impl WorkerStateSnapshot {
    fn idle() -> Self {
        Self {
            status: WorkerState::Idle,
            current_task: None,
        }
    }
}

struct FleetWorker<E> {
    client: FleetHttpClient,
    identity: FleetIdentity,
    executor: E,
    config: WorkerLoopConfig,
    state: WorkerStateSnapshot,
}

impl<E> FleetWorker<E>
where
    E: TaskExecutor,
{
    fn new(
        client: FleetHttpClient,
        identity: FleetIdentity,
        executor: E,
        config: WorkerLoopConfig,
    ) -> Self {
        Self {
            client,
            identity,
            executor,
            config,
            state: WorkerStateSnapshot::idle(),
        }
    }

    async fn register(&self, request: &FleetRegistrationRequest) -> Result<(), FleetError> {
        let response = self
            .client
            .register(&self.identity.primary_endpoint, request)
            .await?;
        ensure_registration_accepted(&response)?;
        ensure_node_identity(&self.identity, &response)
    }

    async fn run_loop(&mut self, shutdown: CancellationToken) -> Result<(), FleetError> {
        self.send_heartbeat().await?;
        let mut heartbeats = worker_interval(self.config.heartbeat_interval);
        let mut polls = worker_interval(self.config.poll_interval);
        prime_interval(&mut heartbeats).await;
        prime_interval(&mut polls).await;

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return self.shutdown().await,
                _ = heartbeats.tick() => {
                    if let Err(e) = self.send_heartbeat().await {
                        tracing::warn!("heartbeat failed, will retry: {e}");
                    }
                },
                _ = polls.tick() => {
                    if let Err(e) = self.poll_once().await {
                        tracing::warn!("task poll failed, will retry: {e}");
                    }
                },
            }
        }
    }

    async fn poll_once(&mut self) -> Result<(), FleetError> {
        if let Some(task) = self
            .client
            .poll_task(&self.identity.primary_endpoint, &self.identity.bearer_token)
            .await?
        {
            let task_id = task.task_id.clone();
            tracing::info!(task_id = %task_id, "received task");
            match self.handle_task(task).await {
                Ok(()) => tracing::info!(task_id = %task_id, "task completed"),
                Err(e) => {
                    tracing::error!(task_id = %task_id, error = %e, "task failed");
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    async fn handle_task(&mut self, task: FleetTaskRequest) -> Result<(), FleetError> {
        self.set_state(WorkerState::Busy, Some(task.task_id.clone()));
        self.send_heartbeat().await?;
        let result = self.executor.execute(&task).await;
        self.client
            .submit_result(
                &self.identity.primary_endpoint,
                &self.identity.bearer_token,
                &result,
            )
            .await?;
        self.set_state(WorkerState::Idle, None);
        self.send_heartbeat().await
    }

    async fn shutdown(&mut self) -> Result<(), FleetError> {
        tracing::info!("fleet worker shutting down");
        self.set_state(WorkerState::ShuttingDown, None);
        self.send_heartbeat().await
    }

    async fn send_heartbeat(&self) -> Result<(), FleetError> {
        let heartbeat = FleetHeartbeat {
            node_id: self.identity.node_id.clone(),
            status: self.state.status.clone(),
            current_task: self.state.current_task.clone(),
        };
        self.client
            .heartbeat(
                &self.identity.primary_endpoint,
                &self.identity.bearer_token,
                &heartbeat,
            )
            .await?;
        tracing::debug!("heartbeat sent");
        Ok(())
    }

    fn set_state(&mut self, status: WorkerState, current_task: Option<String>) {
        self.state = WorkerStateSnapshot {
            status,
            current_task,
        };
    }
}

#[async_trait]
trait TaskExecutor: Send + Sync {
    async fn execute(&self, task: &FleetTaskRequest) -> FleetTaskResult;
}

#[derive(Debug, Clone, Copy)]
struct StubTaskExecutor;

#[async_trait]
impl TaskExecutor for StubTaskExecutor {
    async fn execute(&self, task: &FleetTaskRequest) -> FleetTaskResult {
        let started_at = Instant::now();
        let outcome = stub_execution_outcome(task);
        build_result(task, outcome, started_at.elapsed())
    }
}

#[derive(Debug)]
struct StubExecutionOutcome {
    status: FleetTaskStatus,
    evaluation: Option<serde_json::Value>,
    build_log: Option<String>,
    error: Option<String>,
}

/// Real task executor that runs experiment pipeline operations.
struct ExperimentTaskExecutor {
    data_dir: std::path::PathBuf,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
}

impl ExperimentTaskExecutor {
    fn new(
        data_dir: std::path::PathBuf,
        improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    ) -> Self {
        Self {
            data_dir,
            improvement_provider,
        }
    }
}

use std::sync::Arc;

#[async_trait]
impl TaskExecutor for ExperimentTaskExecutor {
    async fn execute(&self, task: &FleetTaskRequest) -> FleetTaskResult {
        let started_at = Instant::now();
        let result = match &self.improvement_provider {
            Some(provider) => run_task(task, provider.as_ref(), &self.data_dir).await,
            None => Err("No improvement provider configured".to_string()),
        };
        match result {
            Ok(outcome) => build_result(task, outcome, started_at.elapsed()),
            Err(error) => FleetTaskResult {
                task_id: task.task_id.clone(),
                status: FleetTaskStatus::Failed,
                candidate_patch: None,
                evaluation: None,
                build_log: None,
                error: Some(error),
                duration_ms: started_at
                    .elapsed()
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX),
            },
        }
    }
}

async fn run_task(
    task: &FleetTaskRequest,
    provider: &dyn fx_llm::CompletionProvider,
    data_dir: &Path,
) -> Result<StubExecutionOutcome, String> {
    let signal_store = fx_memory::SignalStore::new(data_dir, "fleet-worker")
        .map_err(|e| format!("signal store: {e}"))?;
    let config = fx_improve::ImprovementConfig::default();
    let proposals_dir = data_dir.join("proposals");
    let paths = fx_improve::CyclePaths {
        data_dir,
        repo_root: data_dir,
        proposals_dir: &proposals_dir,
    };

    tracing::info!(task_id = %task.task_id, task_type = ?task.task_type, "Executing fleet task");
    let run_result = fx_improve::run_improvement_cycle(&signal_store, provider, &config, &paths)
        .await
        .map_err(|e| format!("improvement cycle: {e}"))?;

    Ok(StubExecutionOutcome {
        status: FleetTaskStatus::Complete,
        evaluation: Some(json!({
            "plans_generated": run_result.plans_generated,
            "proposals_written": run_result.proposals_written.len(),
            "branches_created": run_result.branches_created.len(),
        })),
        build_log: Some(format!(
            "Fleet task complete: {} plans, {} proposals",
            run_result.plans_generated,
            run_result.proposals_written.len(),
        )),
        error: None,
    })
}

pub async fn run() -> anyhow::Result<i32> {
    #[cfg(test)]
    if let Some(exit_code) = take_test_exit_code() {
        return Ok(exit_code);
    }

    let shutdown = install_shutdown_token();
    let client = FleetHttpClient::new(DEFAULT_REQUEST_TIMEOUT);
    let data_dir = crate::startup::fawx_data_dir();
    let auth_manager = crate::startup::load_auth_manager().ok();
    let config = crate::startup::load_config().unwrap_or_default();
    let improvement_provider = auth_manager
        .as_ref()
        .and_then(|am| crate::startup::build_improvement_provider(am, &config));
    let executor = ExperimentTaskExecutor::new(data_dir, improvement_provider);
    run_with_dependencies(
        &default_fleet_dir(),
        client,
        executor,
        WorkerLoopConfig::default(),
        shutdown,
    )
    .await
    .map_err(anyhow::Error::from)?;
    Ok(0)
}

async fn run_with_dependencies<E>(
    fleet_dir: &Path,
    client: FleetHttpClient,
    executor: E,
    config: WorkerLoopConfig,
    shutdown: CancellationToken,
) -> Result<(), FleetError>
where
    E: TaskExecutor,
{
    let identity = load_identity(fleet_dir)?;
    let request = build_registration_request(&identity.bearer_token)?;
    let mut worker = FleetWorker::new(client, identity, executor, config);
    tracing::info!("fleet worker registering with primary");
    worker.register(&request).await?;
    tracing::info!("registered with primary");
    worker.run_loop(shutdown).await
}

fn load_identity(fleet_dir: &Path) -> Result<FleetIdentity, FleetError> {
    let path = identity_path(fleet_dir);
    FleetIdentity::load(&path)
}

fn ensure_registration_accepted(response: &FleetRegistrationResponse) -> Result<(), FleetError> {
    if response.accepted {
        Ok(())
    } else {
        Err(FleetError::HttpError(
            "worker registration was rejected by the primary".to_string(),
        ))
    }
}

fn ensure_node_identity(
    identity: &FleetIdentity,
    response: &FleetRegistrationResponse,
) -> Result<(), FleetError> {
    if response.node_id == identity.node_id {
        Ok(())
    } else {
        Err(FleetError::HttpError(format!(
            "worker registration returned node id {} but identity expected {}",
            response.node_id, identity.node_id
        )))
    }
}

fn install_shutdown_token() -> CancellationToken {
    let token = CancellationToken::new();
    let shutdown = token.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        shutdown.cancel();
    });
    token
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        wait_for_unix_shutdown().await;
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(unix)]
async fn wait_for_unix_shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut terminate) => {
            tokio::select! {
                _ = ctrl_c => {}
                _ = terminate.recv() => {}
            }
        }
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

fn worker_interval(period: Duration) -> time::Interval {
    let mut interval = time::interval(period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval
}

async fn prime_interval(interval: &mut time::Interval) {
    interval.tick().await;
}

fn stub_execution_outcome(task: &FleetTaskRequest) -> StubExecutionOutcome {
    match task.task_type {
        FleetTaskType::Generate => completed_outcome("generate", task),
        FleetTaskType::Evaluate => completed_outcome("evaluate", task),
        FleetTaskType::GenerateAndEvaluate => completed_outcome("generate_and_evaluate", task),
        _ => unsupported_outcome(task),
    }
}

fn completed_outcome(mode: &str, task: &FleetTaskRequest) -> StubExecutionOutcome {
    StubExecutionOutcome {
        status: FleetTaskStatus::Complete,
        evaluation: Some(json!({
            "mode": mode,
            "repo_url": task.repo_url,
            "branch": task.branch,
            "status": "stubbed",
        })),
        build_log: Some(format!(
            "Fleet worker task execution is scaffolded for {mode}; experiment integration is pending."
        )),
        error: None,
    }
}

fn unsupported_outcome(task: &FleetTaskRequest) -> StubExecutionOutcome {
    StubExecutionOutcome {
        status: FleetTaskStatus::Failed,
        evaluation: None,
        build_log: None,
        error: Some(format!(
            "Unsupported fleet task type for task {}",
            task.task_id
        )),
    }
}

fn build_result(
    task: &FleetTaskRequest,
    outcome: StubExecutionOutcome,
    elapsed: Duration,
) -> FleetTaskResult {
    FleetTaskResult {
        task_id: task.task_id.clone(),
        status: outcome.status,
        candidate_patch: None,
        evaluation: outcome.evaluation,
        build_log: outcome.build_log,
        error: outcome.error,
        duration_ms: elapsed.as_millis().try_into().unwrap_or(u64::MAX),
    }
}

#[cfg(test)]
pub(crate) fn set_test_exit_code(exit_code: i32) {
    *TEST_EXIT_CODE
        .lock()
        .expect("fleet worker test exit code lock") = Some(exit_code);
}

#[cfg(test)]
fn take_test_exit_code() -> Option<i32> {
    TEST_EXIT_CODE
        .lock()
        .expect("fleet worker test exit code lock")
        .take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{header, HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::{get, post},
        Json, Router,
    };
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::{sync::Arc, time::Duration};
    use tempfile::TempDir;
    use tokio::sync::Mutex as TokioMutex;
    use tokio::{sync::mpsc, task::JoinHandle, time::timeout};

    #[derive(Debug)]
    enum ServerEvent {
        Register(CapturedRegistration),
        Heartbeat(CapturedHeartbeat),
        TaskPoll(CapturedTaskPoll),
        Result(CapturedResult),
    }

    #[derive(Debug)]
    struct CapturedRegistration {
        authorization: Option<String>,
        request: FleetRegistrationRequest,
    }

    #[derive(Debug)]
    struct CapturedHeartbeat {
        authorization: Option<String>,
        request: FleetHeartbeat,
    }

    #[derive(Debug)]
    struct CapturedTaskPoll {
        authorization: Option<String>,
    }

    #[derive(Debug)]
    struct CapturedResult {
        authorization: Option<String>,
        request: FleetTaskResult,
    }

    #[derive(Clone)]
    struct TestServerState {
        events: mpsc::UnboundedSender<ServerEvent>,
        node_id: String,
        tasks: Arc<TokioMutex<VecDeque<Option<FleetTaskRequest>>>>,
    }

    struct TestFleetServer {
        base_url: String,
        events: mpsc::UnboundedReceiver<ServerEvent>,
        handle: JoinHandle<()>,
    }

    impl TestFleetServer {
        async fn spawn(node_id: &str, tasks: Vec<Option<FleetTaskRequest>>) -> Self {
            let (events_tx, events_rx) = mpsc::unbounded_channel();
            let state = TestServerState {
                events: events_tx,
                node_id: node_id.to_string(),
                tasks: Arc::new(TokioMutex::new(tasks.into())),
            };
            let app = Router::new()
                .route("/fleet/register", post(handle_register))
                .route("/fleet/heartbeat", post(handle_heartbeat))
                .route("/fleet/task", get(handle_task))
                .route("/fleet/result", post(handle_result))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("test server should bind");
            let address = listener
                .local_addr()
                .expect("test server should expose local address");
            let handle = tokio::spawn(async move {
                axum::serve(listener, app)
                    .await
                    .expect("test server should serve");
            });
            Self {
                base_url: format!("http://{address}"),
                events: events_rx,
                handle,
            }
        }

        async fn next_event(&mut self) -> ServerEvent {
            timeout(Duration::from_secs(2), self.events.recv())
                .await
                .expect("event should arrive")
                .expect("server event should exist")
        }

        async fn next_registration(&mut self) -> CapturedRegistration {
            loop {
                if let ServerEvent::Register(event) = self.next_event().await {
                    return event;
                }
            }
        }

        async fn next_heartbeat_with_status(&mut self, status: WorkerState) -> CapturedHeartbeat {
            loop {
                if let ServerEvent::Heartbeat(event) = self.next_event().await {
                    if event.request.status == status {
                        return event;
                    }
                }
            }
        }

        async fn next_task_poll(&mut self) -> CapturedTaskPoll {
            loop {
                if let ServerEvent::TaskPoll(event) = self.next_event().await {
                    return event;
                }
            }
        }

        async fn next_result(&mut self) -> CapturedResult {
            loop {
                if let ServerEvent::Result(event) = self.next_event().await {
                    return event;
                }
            }
        }
    }

    impl Drop for TestFleetServer {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }

    async fn handle_register(
        State(state): State<TestServerState>,
        headers: HeaderMap,
        Json(request): Json<FleetRegistrationRequest>,
    ) -> Response {
        let event = ServerEvent::Register(CapturedRegistration {
            authorization: authorization_header(&headers),
            request,
        });
        let _ = state.events.send(event);
        (
            StatusCode::OK,
            Json(FleetRegistrationResponse {
                node_id: state.node_id,
                accepted: true,
                message: "registered".to_string(),
            }),
        )
            .into_response()
    }

    async fn handle_heartbeat(
        State(state): State<TestServerState>,
        headers: HeaderMap,
        Json(request): Json<FleetHeartbeat>,
    ) -> StatusCode {
        let event = ServerEvent::Heartbeat(CapturedHeartbeat {
            authorization: authorization_header(&headers),
            request,
        });
        let _ = state.events.send(event);
        StatusCode::OK
    }

    async fn handle_task(State(state): State<TestServerState>, headers: HeaderMap) -> Response {
        let _ = state.events.send(ServerEvent::TaskPoll(CapturedTaskPoll {
            authorization: authorization_header(&headers),
        }));
        let task = state.tasks.lock().await.pop_front().unwrap_or(None);
        match task {
            Some(task) => (StatusCode::OK, Json(task)).into_response(),
            None => StatusCode::NO_CONTENT.into_response(),
        }
    }

    async fn handle_result(
        State(state): State<TestServerState>,
        headers: HeaderMap,
        Json(request): Json<FleetTaskResult>,
    ) -> StatusCode {
        let event = ServerEvent::Result(CapturedResult {
            authorization: authorization_header(&headers),
            request,
        });
        let _ = state.events.send(event);
        StatusCode::OK
    }

    fn authorization_header(headers: &HeaderMap) -> Option<String> {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    fn test_config() -> WorkerLoopConfig {
        WorkerLoopConfig {
            heartbeat_interval: Duration::from_millis(30),
            poll_interval: Duration::from_millis(15),
        }
    }

    fn sample_identity(endpoint: &str) -> FleetIdentity {
        FleetIdentity {
            node_id: "node-123".to_string(),
            primary_endpoint: endpoint.to_string(),
            bearer_token: "fleet-secret".to_string(),
            registered_at_ms: 1,
        }
    }

    fn sample_task() -> FleetTaskRequest {
        FleetTaskRequest {
            task_id: "task-1".to_string(),
            task_type: FleetTaskType::GenerateAndEvaluate,
            repo_url: "https://github.com/fawxai/fawx".to_string(),
            branch: "dev".to_string(),
            git_token: None,
            signal: json!({"prompt": "improve tests"}),
            config: json!({"temperature": 0.1}),
            chain_history: vec![json!({"step": "baseline"})],
            scope: vec!["engine/crates/fx-cli/src/main.rs".to_string()],
        }
    }

    fn write_identity(temp_dir: &TempDir, identity: &FleetIdentity) -> PathBuf {
        let fleet_dir = temp_dir.path().join("fleet");
        identity
            .save(&fleet_dir.join("identity.json"))
            .expect("identity should save");
        fleet_dir
    }

    fn spawn_worker(
        fleet_dir: PathBuf,
        shutdown: CancellationToken,
    ) -> JoinHandle<Result<(), FleetError>> {
        tokio::spawn(async move {
            run_with_dependencies(
                &fleet_dir,
                FleetHttpClient::new(Duration::from_secs(1)),
                StubTaskExecutor,
                test_config(),
                shutdown,
            )
            .await
        })
    }

    #[tokio::test]
    async fn worker_registration_sends_correct_payload() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut server = TestFleetServer::spawn("node-123", Vec::new()).await;
        let identity = sample_identity(&server.base_url);
        let fleet_dir = write_identity(&temp_dir, &identity);
        let shutdown = CancellationToken::new();
        let worker = spawn_worker(fleet_dir, shutdown.clone());

        let registration = server.next_registration().await;
        assert_eq!(registration.authorization, None);
        assert_eq!(registration.request.bearer_token, identity.bearer_token);
        assert!(!registration.request.node_name.is_empty());
        assert!(registration.request.cpus.is_some());
        assert!(registration
            .request
            .capabilities
            .contains(&"agentic_loop".to_string()));

        shutdown.cancel();
        worker
            .await
            .expect("worker task should join")
            .expect("worker should exit cleanly");
    }

    #[tokio::test]
    async fn heartbeat_loop_sends_idle_status_updates() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut server = TestFleetServer::spawn("node-123", Vec::new()).await;
        let identity = sample_identity(&server.base_url);
        let fleet_dir = write_identity(&temp_dir, &identity);
        let shutdown = CancellationToken::new();
        let worker = spawn_worker(fleet_dir, shutdown.clone());

        let _ = server.next_registration().await;
        let first = server.next_heartbeat_with_status(WorkerState::Idle).await;
        let second = server.next_heartbeat_with_status(WorkerState::Idle).await;

        assert_eq!(first.authorization.as_deref(), Some("Bearer fleet-secret"));
        assert_eq!(first.request.node_id, identity.node_id);
        assert_eq!(first.request.current_task, None);
        assert_eq!(second.authorization.as_deref(), Some("Bearer fleet-secret"));

        shutdown.cancel();
        worker
            .await
            .expect("worker task should join")
            .expect("worker should exit cleanly");
    }

    #[tokio::test]
    async fn task_receipt_triggers_execution_flow() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut server = TestFleetServer::spawn("node-123", vec![Some(sample_task())]).await;
        let identity = sample_identity(&server.base_url);
        let fleet_dir = write_identity(&temp_dir, &identity);
        let shutdown = CancellationToken::new();
        let worker = spawn_worker(fleet_dir, shutdown.clone());

        let _ = server.next_registration().await;
        let _ = server.next_heartbeat_with_status(WorkerState::Idle).await;
        let poll = server.next_task_poll().await;
        let busy = server.next_heartbeat_with_status(WorkerState::Busy).await;
        let result = server.next_result().await;
        let idle = server.next_heartbeat_with_status(WorkerState::Idle).await;

        assert_eq!(poll.authorization.as_deref(), Some("Bearer fleet-secret"));
        assert_eq!(busy.request.current_task.as_deref(), Some("task-1"));
        assert_eq!(result.authorization.as_deref(), Some("Bearer fleet-secret"));
        assert_eq!(result.request.task_id, "task-1");
        assert_eq!(result.request.status, FleetTaskStatus::Complete);
        assert!(result.request.evaluation.is_some());
        assert_eq!(idle.request.current_task, None);

        shutdown.cancel();
        worker
            .await
            .expect("worker task should join")
            .expect("worker should exit cleanly");
    }

    #[tokio::test]
    async fn graceful_shutdown_updates_worker_status() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let mut server = TestFleetServer::spawn("node-123", Vec::new()).await;
        let identity = sample_identity(&server.base_url);
        let fleet_dir = write_identity(&temp_dir, &identity);
        let shutdown = CancellationToken::new();
        let worker = spawn_worker(fleet_dir, shutdown.clone());

        let _ = server.next_registration().await;
        let _ = server.next_heartbeat_with_status(WorkerState::Idle).await;
        shutdown.cancel();
        let shutdown_heartbeat = server
            .next_heartbeat_with_status(WorkerState::ShuttingDown)
            .await;

        assert_eq!(
            shutdown_heartbeat.authorization.as_deref(),
            Some("Bearer fleet-secret")
        );
        assert_eq!(shutdown_heartbeat.request.current_task, None);
        worker
            .await
            .expect("worker task should join")
            .expect("worker should exit cleanly");
    }
}
