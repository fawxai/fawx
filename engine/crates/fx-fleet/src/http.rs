use crate::FleetError;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

const REDACTED: &str = "[REDACTED]";
const TASK_PATH: &str = "/fleet/task";
const STATUS_PATH: &str = "/fleet/status";
const REGISTER_PATH: &str = "/fleet/register";
const HEARTBEAT_PATH: &str = "/fleet/heartbeat";
const RESULT_PATH: &str = "/fleet/result";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum FleetTaskType {
    Generate,
    Evaluate,
    GenerateAndEvaluate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum FleetTaskStatus {
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    Idle,
    Busy,
    ShuttingDown,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct FleetTaskRequest {
    pub task_id: String,
    pub task_type: FleetTaskType,
    pub repo_url: String,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_token: Option<String>,
    pub signal: serde_json::Value,
    pub config: serde_json::Value,
    pub chain_history: Vec<serde_json::Value>,
    pub scope: Vec<String>,
}

impl fmt::Debug for FleetTaskRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let git_token = self.git_token.as_ref().map(|_| REDACTED);
        formatter
            .debug_struct("FleetTaskRequest")
            .field("task_id", &self.task_id)
            .field("task_type", &self.task_type)
            .field("repo_url", &self.repo_url)
            .field("branch", &self.branch)
            .field("git_token", &git_token)
            .field("signal", &self.signal)
            .field("config", &self.config)
            .field("chain_history", &self.chain_history)
            .field("scope", &self.scope)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FleetTaskResult {
    pub task_id: String,
    pub status: FleetTaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_patch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluation: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_log: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct FleetRegistrationRequest {
    pub node_name: String,
    pub bearer_token: String,
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram_gb: Option<u32>,
}

impl fmt::Debug for FleetRegistrationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FleetRegistrationRequest")
            .field("node_name", &self.node_name)
            .field("bearer_token", &REDACTED)
            .field("capabilities", &self.capabilities)
            .field("rust_version", &self.rust_version)
            .field("os", &self.os)
            .field("cpus", &self.cpus)
            .field("ram_gb", &self.ram_gb)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetRegistrationResponse {
    pub node_id: String,
    pub accepted: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetHeartbeat {
    pub node_id: String,
    pub status: WorkerState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetWorkerStatus {
    pub node_id: String,
    pub status: WorkerState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task: Option<String>,
    pub uptime_seconds: u64,
}

pub struct FleetHttpClient {
    client: reqwest::Client,
    timeout: Duration,
}

impl fmt::Debug for FleetHttpClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FleetHttpClient")
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl FleetHttpClient {
    pub fn new(timeout: Duration) -> Self {
        Self {
            client: reqwest::Client::new(),
            timeout,
        }
    }

    pub async fn send_task(
        &self,
        endpoint: &str,
        bearer: &str,
        request: &FleetTaskRequest,
    ) -> Result<(), FleetError> {
        let url = fleet_url(endpoint, TASK_PATH);
        let request = self
            .authorized_request(Method::POST, &url, bearer)
            .json(request);
        self.send_without_response(request, "task dispatch").await
    }

    pub async fn poll_task(
        &self,
        endpoint: &str,
        bearer: &str,
    ) -> Result<Option<FleetTaskRequest>, FleetError> {
        let url = fleet_url(endpoint, TASK_PATH);
        let response = self
            .authorized_request(Method::GET, &url, bearer)
            .send()
            .await
            .map_err(|error| request_failed_error("task poll", error))?;
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        let response = ensure_success("task poll", response).await?;
        response
            .json()
            .await
            .map(Some)
            .map_err(|error| invalid_json_error("task poll", error))
    }

    pub async fn worker_status(
        &self,
        endpoint: &str,
        bearer: &str,
    ) -> Result<FleetWorkerStatus, FleetError> {
        let url = fleet_url(endpoint, STATUS_PATH);
        let request = self.authorized_request(Method::GET, &url, bearer);
        self.json_response(request, "worker status").await
    }

    pub async fn register(
        &self,
        endpoint: &str,
        request: &FleetRegistrationRequest,
    ) -> Result<FleetRegistrationResponse, FleetError> {
        let url = fleet_url(endpoint, REGISTER_PATH);
        let request = self.request(Method::POST, &url).json(request);
        self.json_response(request, "worker registration").await
    }

    pub async fn heartbeat(
        &self,
        endpoint: &str,
        bearer: &str,
        heartbeat: &FleetHeartbeat,
    ) -> Result<(), FleetError> {
        let url = fleet_url(endpoint, HEARTBEAT_PATH);
        let request = self
            .authorized_request(Method::POST, &url, bearer)
            .json(heartbeat);
        self.send_without_response(request, "heartbeat").await
    }

    pub async fn submit_result(
        &self,
        endpoint: &str,
        bearer: &str,
        result: &FleetTaskResult,
    ) -> Result<(), FleetError> {
        let url = fleet_url(endpoint, RESULT_PATH);
        let request = self
            .authorized_request(Method::POST, &url, bearer)
            .json(result);
        self.send_without_response(request, "result submission")
            .await
    }

    fn request(&self, method: Method, url: &str) -> reqwest::RequestBuilder {
        self.client.request(method, url).timeout(self.timeout)
    }

    fn authorized_request(
        &self,
        method: Method,
        url: &str,
        bearer: &str,
    ) -> reqwest::RequestBuilder {
        self.request(method, url).bearer_auth(bearer)
    }

    async fn send_without_response(
        &self,
        request: reqwest::RequestBuilder,
        operation: &str,
    ) -> Result<(), FleetError> {
        self.execute(request, operation).await?;
        Ok(())
    }

    async fn json_response<T>(
        &self,
        request: reqwest::RequestBuilder,
        operation: &str,
    ) -> Result<T, FleetError>
    where
        T: DeserializeOwned,
    {
        let response = self.execute(request, operation).await?;
        response
            .json()
            .await
            .map_err(|error| invalid_json_error(operation, error))
    }

    async fn execute(
        &self,
        request: reqwest::RequestBuilder,
        operation: &str,
    ) -> Result<reqwest::Response, FleetError> {
        let response = request
            .send()
            .await
            .map_err(|error| request_failed_error(operation, error))?;
        ensure_success(operation, response).await
    }
}

async fn ensure_success(
    operation: &str,
    response: reqwest::Response,
) -> Result<reqwest::Response, FleetError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    Err(FleetError::HttpError(format!(
        "{operation} failed with HTTP {}: {}",
        status.as_u16(),
        body
    )))
}

fn request_failed_error(operation: &str, error: reqwest::Error) -> FleetError {
    FleetError::HttpError(format!(
        "{operation} request failed: {}",
        error.without_url()
    ))
}

fn invalid_json_error(operation: &str, error: reqwest::Error) -> FleetError {
    FleetError::HttpError(format!(
        "{operation} returned invalid JSON: {}",
        error.without_url()
    ))
}

fn fleet_url(endpoint: &str, path: &str) -> String {
    format!("{}{}", endpoint.trim_end_matches('/'), path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, Bytes},
        extract::State,
        http::{header, HeaderMap, StatusCode, Uri},
        response::Response,
        routing::{get, post},
        Router,
    };
    use serde_json::json;
    use std::sync::Arc;
    use tokio::{
        sync::{oneshot, Mutex},
        task::JoinHandle,
        time::timeout,
    };

    #[derive(Debug, Clone)]
    struct TestResponse {
        status: StatusCode,
        body: String,
        content_type: &'static str,
    }

    impl TestResponse {
        fn no_content() -> Self {
            Self {
                status: StatusCode::NO_CONTENT,
                body: String::new(),
                content_type: "text/plain",
            }
        }

        fn json(value: serde_json::Value) -> Self {
            Self {
                status: StatusCode::OK,
                body: value.to_string(),
                content_type: "application/json",
            }
        }

        fn text(status: StatusCode, body: &str) -> Self {
            Self {
                status,
                body: body.to_string(),
                content_type: "text/plain",
            }
        }
    }

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path: String,
        authorization: Option<String>,
        body: Vec<u8>,
    }

    impl CapturedRequest {
        fn json_body(&self) -> serde_json::Value {
            serde_json::from_slice(&self.body).expect("request body should be valid JSON")
        }
    }

    #[derive(Clone)]
    struct TestServerState {
        sender: Arc<Mutex<Option<oneshot::Sender<CapturedRequest>>>>,
        response: TestResponse,
    }

    struct TestServer {
        base_url: String,
        receiver: Option<oneshot::Receiver<CapturedRequest>>,
        handle: JoinHandle<()>,
    }

    impl TestServer {
        async fn spawn(response: TestResponse) -> Self {
            let (sender, receiver) = oneshot::channel();
            let state = TestServerState {
                sender: Arc::new(Mutex::new(Some(sender))),
                response,
            };
            let app = Router::new()
                .route(TASK_PATH, get(capture_request).post(capture_request))
                .route(STATUS_PATH, get(capture_request))
                .route(REGISTER_PATH, post(capture_request))
                .route(HEARTBEAT_PATH, post(capture_request))
                .route(RESULT_PATH, post(capture_request))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("test server should bind");
            let address = listener
                .local_addr()
                .expect("test server should expose a local address");
            let handle = tokio::spawn(async move {
                axum::serve(listener, app)
                    .await
                    .expect("test server should run");
            });

            Self {
                base_url: format!("http://{address}"),
                receiver: Some(receiver),
                handle,
            }
        }

        async fn capture_request(&mut self) -> CapturedRequest {
            let receiver = self
                .receiver
                .take()
                .expect("request receiver should only be consumed once");
            timeout(Duration::from_secs(2), receiver)
                .await
                .expect("request should reach the test server")
                .expect("test server should capture request")
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }

    async fn capture_request(
        State(state): State<TestServerState>,
        method: Method,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let request = CapturedRequest {
            method,
            path: uri.path().to_string(),
            authorization: authorization_header(&headers),
            body: body.to_vec(),
        };
        send_captured_request(&state.sender, request).await;
        build_response(&state.response)
    }

    fn authorization_header(headers: &HeaderMap) -> Option<String> {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    async fn send_captured_request(
        sender: &Arc<Mutex<Option<oneshot::Sender<CapturedRequest>>>>,
        request: CapturedRequest,
    ) {
        if let Some(sender) = sender.lock().await.take() {
            let _ = sender.send(request);
        }
    }

    fn build_response(response: &TestResponse) -> Response {
        Response::builder()
            .status(response.status)
            .header(header::CONTENT_TYPE, response.content_type)
            .body(Body::from(response.body.clone()))
            .expect("test response should be valid")
    }

    fn sample_task_request() -> FleetTaskRequest {
        FleetTaskRequest {
            task_id: "exp-001".to_string(),
            task_type: FleetTaskType::GenerateAndEvaluate,
            repo_url: "https://github.com/fawxai/fawx".to_string(),
            branch: "dev".to_string(),
            git_token: Some("ghp_secret".to_string()),
            signal: json!({"prompt": "improve tests"}),
            config: json!({"temperature": 0.1}),
            chain_history: vec![json!({"result": "baseline"})],
            scope: vec!["src/lib.rs".to_string(), "src/http.rs".to_string()],
        }
    }

    fn sample_task_result() -> FleetTaskResult {
        FleetTaskResult {
            task_id: "exp-001".to_string(),
            status: FleetTaskStatus::Complete,
            candidate_patch: Some("diff --git a/src/lib.rs b/src/lib.rs".to_string()),
            evaluation: Some(json!({"score": 0.92})),
            build_log: None,
            error: None,
            duration_ms: 1_250,
        }
    }

    fn sample_registration_request() -> FleetRegistrationRequest {
        FleetRegistrationRequest {
            node_name: "macmini-01".to_string(),
            bearer_token: "node-secret".to_string(),
            capabilities: vec!["generate".to_string(), "evaluate".to_string()],
            rust_version: Some("1.86.0".to_string()),
            os: Some("macos-arm64".to_string()),
            cpus: None,
            ram_gb: None,
        }
    }

    fn sample_heartbeat() -> FleetHeartbeat {
        FleetHeartbeat {
            node_id: "macmini-01".to_string(),
            status: WorkerState::Idle,
            current_task: None,
        }
    }

    fn sample_worker_status() -> FleetWorkerStatus {
        FleetWorkerStatus {
            node_id: "macmini-01".to_string(),
            status: WorkerState::Busy,
            current_task: Some("exp-001".to_string()),
            uptime_seconds: 42,
        }
    }

    fn http_error_message(error: FleetError) -> String {
        match error {
            FleetError::HttpError(message) => message,
            other => panic!("expected HTTP error, got {other:?}"),
        }
    }

    #[test]
    fn task_request_serialization_roundtrip() {
        let request = sample_task_request();

        let encoded = serde_json::to_string(&request).expect("request should serialize");
        let decoded: FleetTaskRequest =
            serde_json::from_str(&encoded).expect("request should deserialize");

        assert_eq!(decoded, request);
    }

    #[test]
    fn task_result_serialization_roundtrip() {
        let result = sample_task_result();

        let encoded = serde_json::to_value(&result).expect("result should serialize");
        assert!(encoded.get("build_log").is_none());
        assert!(encoded.get("error").is_none());

        let decoded: FleetTaskResult =
            serde_json::from_value(encoded).expect("result should deserialize");

        assert_eq!(decoded, result);
    }

    #[test]
    fn registration_request_serialization_roundtrip() {
        let request = sample_registration_request();

        let encoded = serde_json::to_value(&request).expect("request should serialize");
        assert!(encoded.get("cpus").is_none());
        assert!(encoded.get("ram_gb").is_none());

        let decoded: FleetRegistrationRequest =
            serde_json::from_value(encoded).expect("request should deserialize");

        assert_eq!(decoded, request);
    }

    #[test]
    fn heartbeat_serialization_roundtrip() {
        let heartbeat = sample_heartbeat();

        let encoded = serde_json::to_value(&heartbeat).expect("heartbeat should serialize");
        assert!(encoded.get("current_task").is_none());

        let decoded: FleetHeartbeat =
            serde_json::from_value(encoded).expect("heartbeat should deserialize");

        assert_eq!(decoded, heartbeat);
    }

    #[test]
    fn worker_status_serialization_roundtrip() {
        let status = sample_worker_status();
        let encoded = serde_json::to_value(&status).expect("status should serialize");
        let decoded: FleetWorkerStatus =
            serde_json::from_value(encoded).expect("status should deserialize");

        assert_eq!(decoded, status);
    }

    #[test]
    fn task_type_serializes_as_snake_case() {
        let encoded = serde_json::to_string(&FleetTaskType::GenerateAndEvaluate)
            .expect("task type should serialize");

        assert_eq!(encoded, "\"generate_and_evaluate\"");
    }

    #[test]
    fn task_status_serializes_as_snake_case() {
        let encoded = serde_json::to_string(&FleetTaskStatus::Cancelled)
            .expect("task status should serialize");

        assert_eq!(encoded, "\"cancelled\"");
    }

    #[test]
    fn worker_state_serializes_as_snake_case() {
        let encoded = serde_json::to_string(&WorkerState::ShuttingDown)
            .expect("worker state should serialize");

        assert_eq!(encoded, "\"shutting_down\"");
    }

    #[test]
    fn task_request_debug_redacts_git_token() {
        let request = FleetTaskRequest {
            task_id: "exp-001".to_string(),
            task_type: FleetTaskType::Generate,
            repo_url: "https://github.com/fawxai/fawx".to_string(),
            branch: "dev".to_string(),
            git_token: Some("ghp_secret".to_string()),
            signal: json!({"prompt": "improve tests"}),
            config: json!({"temperature": 0.1}),
            chain_history: Vec::new(),
            scope: vec!["src/lib.rs".to_string()],
        };

        let debug_output = format!("{request:?}");

        assert!(debug_output.contains(REDACTED));
        assert!(!debug_output.contains("ghp_secret"));
    }

    #[test]
    fn registration_debug_redacts_bearer_token() {
        let request = FleetRegistrationRequest {
            node_name: "macmini-01".to_string(),
            bearer_token: "node-secret".to_string(),
            capabilities: vec!["generate".to_string()],
            rust_version: None,
            os: None,
            cpus: Some(8),
            ram_gb: Some(16),
        };

        let debug_output = format!("{request:?}");

        assert!(debug_output.contains(REDACTED));
        assert!(!debug_output.contains("node-secret"));
    }

    #[test]
    fn http_client_constructs_with_timeout() {
        let timeout = Duration::from_secs(15);
        let client = FleetHttpClient::new(timeout);

        assert_eq!(client.timeout, timeout);
    }

    #[test]
    fn http_client_debug_includes_timeout() {
        let client = FleetHttpClient::new(Duration::from_secs(15));
        let debug_output = format!("{client:?}");

        assert!(debug_output.contains("FleetHttpClient"));
        assert!(debug_output.contains("15s"));
    }

    #[tokio::test]
    async fn send_task_sends_json_and_bearer_auth() {
        let request = sample_task_request();
        let mut server = TestServer::spawn(TestResponse::no_content()).await;
        let client = FleetHttpClient::new(Duration::from_secs(2));

        client
            .send_task(&server.base_url, "dispatch-token", &request)
            .await
            .expect("send_task should succeed");

        let captured = server.capture_request().await;
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, TASK_PATH);
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer dispatch-token")
        );
        assert_eq!(
            captured.json_body(),
            serde_json::to_value(&request).unwrap()
        );
    }

    #[tokio::test]
    async fn poll_task_reads_json_response_with_bearer_auth() {
        let response = sample_task_request();
        let mut server = TestServer::spawn(TestResponse::json(
            serde_json::to_value(&response).expect("task should serialize"),
        ))
        .await;
        let client = FleetHttpClient::new(Duration::from_secs(2));

        let task = client
            .poll_task(&server.base_url, "poll-token")
            .await
            .expect("poll_task should succeed");

        let captured = server.capture_request().await;
        assert_eq!(captured.method, Method::GET);
        assert_eq!(captured.path, TASK_PATH);
        assert_eq!(captured.authorization.as_deref(), Some("Bearer poll-token"));
        assert!(captured.body.is_empty());
        assert_eq!(task, Some(response));
    }

    #[tokio::test]
    async fn poll_task_returns_none_for_no_content() {
        let mut server = TestServer::spawn(TestResponse::no_content()).await;
        let client = FleetHttpClient::new(Duration::from_secs(2));

        let task = client
            .poll_task(&server.base_url, "poll-token")
            .await
            .expect("poll_task should succeed");

        let captured = server.capture_request().await;
        assert_eq!(captured.method, Method::GET);
        assert_eq!(captured.path, TASK_PATH);
        assert_eq!(captured.authorization.as_deref(), Some("Bearer poll-token"));
        assert!(captured.body.is_empty());
        assert_eq!(task, None);
    }

    #[tokio::test]
    async fn worker_status_reads_json_response_with_bearer_auth() {
        let response = sample_worker_status();
        let mut server = TestServer::spawn(TestResponse::json(
            serde_json::to_value(&response).expect("status should serialize"),
        ))
        .await;
        let client = FleetHttpClient::new(Duration::from_secs(2));

        let status = client
            .worker_status(&server.base_url, "status-token")
            .await
            .expect("worker_status should succeed");

        let captured = server.capture_request().await;
        assert_eq!(captured.method, Method::GET);
        assert_eq!(captured.path, STATUS_PATH);
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer status-token")
        );
        assert!(captured.body.is_empty());
        assert_eq!(status, response);
    }

    #[tokio::test]
    async fn register_sends_json_to_register_endpoint() {
        let request = sample_registration_request();
        let response = FleetRegistrationResponse {
            node_id: "node-123".to_string(),
            accepted: true,
            message: "registered".to_string(),
        };
        let mut server = TestServer::spawn(TestResponse::json(
            serde_json::to_value(&response).expect("response should serialize"),
        ))
        .await;
        let client = FleetHttpClient::new(Duration::from_secs(2));

        let registered = client
            .register(&server.base_url, &request)
            .await
            .expect("register should succeed");

        let captured = server.capture_request().await;
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, REGISTER_PATH);
        assert_eq!(captured.authorization, None);
        assert_eq!(
            captured.json_body(),
            serde_json::to_value(&request).unwrap()
        );
        assert_eq!(registered, response);
    }

    #[tokio::test]
    async fn heartbeat_sends_json_and_bearer_auth() {
        let heartbeat = sample_heartbeat();
        let mut server = TestServer::spawn(TestResponse::no_content()).await;
        let client = FleetHttpClient::new(Duration::from_secs(2));

        client
            .heartbeat(&server.base_url, "heartbeat-token", &heartbeat)
            .await
            .expect("heartbeat should succeed");

        let captured = server.capture_request().await;
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, HEARTBEAT_PATH);
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer heartbeat-token")
        );
        assert_eq!(
            captured.json_body(),
            serde_json::to_value(&heartbeat).unwrap()
        );
    }

    #[tokio::test]
    async fn submit_result_sends_json_and_bearer_auth() {
        let result = sample_task_result();
        let mut server = TestServer::spawn(TestResponse::no_content()).await;
        let client = FleetHttpClient::new(Duration::from_secs(2));

        client
            .submit_result(&server.base_url, "result-token", &result)
            .await
            .expect("submit_result should succeed");

        let captured = server.capture_request().await;
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, RESULT_PATH);
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer result-token")
        );
        assert_eq!(captured.json_body(), serde_json::to_value(&result).unwrap());
    }

    #[tokio::test]
    async fn non_success_responses_include_status_and_body() {
        let request = sample_task_request();
        let mut server = TestServer::spawn(TestResponse::text(
            StatusCode::BAD_REQUEST,
            "missing required scope",
        ))
        .await;
        let client = FleetHttpClient::new(Duration::from_secs(2));

        let error = client
            .send_task(&server.base_url, "dispatch-token", &request)
            .await
            .expect_err("send_task should fail");

        let captured = server.capture_request().await;
        assert_eq!(captured.path, TASK_PATH);
        let message = http_error_message(error);
        assert!(message.contains("task dispatch failed with HTTP 400"));
        assert!(message.contains("missing required scope"));
    }
}
