use super::api_client::{api_error_message, bearer_token, http_client, request_error};
use super::runtime_layout::RuntimeLayout;
use anyhow::Context;
use clap::Args;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::time::Duration;

const DEFAULT_PAIR_TTL_SECONDS: u64 = 300;
const PAIR_BOX_CONTENT_WIDTH: usize = 33;

#[derive(Debug, Clone, Args)]
pub struct PairArgs {
    /// Pairing code lifetime in seconds
    #[arg(
        long,
        default_value_t = DEFAULT_PAIR_TTL_SECONDS,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub ttl: u64,

    /// Print the pairing code as JSON for scripting
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct PairCodeResponse {
    code: String,
    expires_at: u64,
    ttl_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct GeneratePairRequest {
    ttl_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct DeviceInfo {
    id: String,
    device_name: String,
    created_at: u64,
    last_used_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct DevicesResponse {
    devices: Vec<DeviceInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairWaitEvent {
    Paired,
    Tick,
    Cancelled,
}

pub async fn run(args: &PairArgs) -> anyhow::Result<i32> {
    let layout = RuntimeLayout::detect()?;
    let client = http_client()?;
    let initial_device_count = fetch_device_count(&layout, &client).await?;
    let pair = fetch_pairing_code(&layout, &client, args.ttl).await?;
    if args.json {
        println!("{}", pair_json_string(&pair)?);
        return Ok(0);
    }
    print_pair_box(&pair)?;
    wait_for_pairing_window(&layout, &client, pair.expires_at, initial_device_count).await?;
    Ok(0)
}

async fn fetch_pairing_code(
    layout: &RuntimeLayout,
    client: &reqwest::Client,
    ttl_seconds: u64,
) -> anyhow::Result<PairCodeResponse> {
    let token = bearer_token(layout)?;
    let response = client
        .post(pair_url(layout.http_port))
        .bearer_auth(token)
        .json(&GeneratePairRequest { ttl_seconds })
        .send()
        .await
        .map_err(request_error)?;
    parse_pair_response(response).await
}

async fn parse_pair_response(response: reqwest::Response) -> anyhow::Result<PairCodeResponse> {
    if response.status().is_success() {
        return response
            .json()
            .await
            .context("failed to decode pairing response");
    }
    Err(anyhow::anyhow!(api_error_message(response).await))
}

fn pair_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1/pair/generate")
}

fn pair_json_string(pair: &PairCodeResponse) -> anyhow::Result<String> {
    serde_json::to_string_pretty(pair).context("failed to encode pairing JSON")
}

fn print_pair_box(pair: &PairCodeResponse) -> anyhow::Result<()> {
    for line in pair_box_lines(pair) {
        println!("{line}");
    }
    io::stdout().flush().context("failed to flush stdout")
}

fn pair_box_lines(pair: &PairCodeResponse) -> Vec<String> {
    let remaining = format_countdown(remaining_seconds(pair.expires_at));
    vec![
        "╭───────────────────────────────────╮".to_string(),
        box_line(""),
        box_line(&format!("  Pairing code:  {}", pair.code)),
        box_line(&format!("  Expires in {remaining}")),
        box_line(""),
        box_line("  Enter this code in the Fawx"),
        box_line("  app to connect this device."),
        box_line(""),
        "╰───────────────────────────────────╯".to_string(),
        String::new(),
    ]
}

fn box_line(content: &str) -> String {
    format!("│ {:<PAIR_BOX_CONTENT_WIDTH$} │", content)
}

async fn wait_for_pairing_window(
    layout: &RuntimeLayout,
    client: &reqwest::Client,
    expires_at: u64,
    initial_device_count: usize,
) -> anyhow::Result<()> {
    loop {
        let remaining = remaining_seconds(expires_at);
        render_waiting_line(remaining)?;
        if remaining == 0 {
            return print_wait_result(
                "Code expired. Run `fawx pair` again to generate a new code.",
            );
        }
        match next_wait_event(layout, client, initial_device_count).await {
            PairWaitEvent::Paired => {
                return print_wait_result("Device paired successfully.");
            }
            PairWaitEvent::Cancelled => {
                return print_wait_result("Stopped waiting for device pairing.");
            }
            PairWaitEvent::Tick => {}
        }
    }
}

fn render_waiting_line(remaining: u64) -> anyhow::Result<()> {
    print!(
        "\rWaiting for device to pair... (Ctrl+C to cancel)  Expires in {}   ",
        format_countdown(remaining)
    );
    io::stdout().flush().context("failed to flush stdout")
}

fn print_wait_result(message: &str) -> anyhow::Result<()> {
    println!("\n{message}");
    io::stdout().flush().context("failed to flush stdout")
}

async fn next_wait_event(
    layout: &RuntimeLayout,
    client: &reqwest::Client,
    initial_device_count: usize,
) -> PairWaitEvent {
    tokio::select! {
        _ = tokio::signal::ctrl_c() => PairWaitEvent::Cancelled,
        _ = tokio::time::sleep(Duration::from_secs(1)) => {
            detect_pairing(client, layout, initial_device_count).await
        }
    }
}

async fn detect_pairing(
    client: &reqwest::Client,
    layout: &RuntimeLayout,
    initial_device_count: usize,
) -> PairWaitEvent {
    let _ = ping_health(client, layout.http_port).await;
    match fetch_device_count(layout, client).await {
        Ok(count) if count > initial_device_count => PairWaitEvent::Paired,
        _ => PairWaitEvent::Tick,
    }
}

async fn ping_health(client: &reqwest::Client, port: u16) -> anyhow::Result<()> {
    client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
        .map(|_| ())
        .map_err(anyhow::Error::new)
}

async fn fetch_device_count(
    layout: &RuntimeLayout,
    client: &reqwest::Client,
) -> anyhow::Result<usize> {
    Ok(fetch_devices(layout, client).await?.devices.len())
}

async fn fetch_devices(
    layout: &RuntimeLayout,
    client: &reqwest::Client,
) -> anyhow::Result<DevicesResponse> {
    let token = bearer_token(layout)?;
    let response = client
        .get(devices_url(layout.http_port))
        .bearer_auth(token)
        .send()
        .await
        .map_err(request_error)?;
    parse_devices_response(response).await
}

async fn parse_devices_response(response: reqwest::Response) -> anyhow::Result<DevicesResponse> {
    if response.status().is_success() {
        return response
            .json()
            .await
            .context("failed to decode device list response");
    }
    Err(anyhow::anyhow!(api_error_message(response).await))
}

fn devices_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1/devices")
}

fn remaining_seconds(expires_at: u64) -> u64 {
    expires_at.saturating_sub(super::api_client::current_unix_seconds())
}

fn format_countdown(total_seconds: u64) -> String {
    format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::api_client::server_not_running_message;
    use axum::{
        extract::State,
        http::{header, HeaderMap, StatusCode},
        routing::{get, post},
        Json, Router,
    };
    use fx_config::FawxConfig;
    use serde_json::Value;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::{
        sync::{oneshot, Mutex},
        task::JoinHandle,
        time::timeout,
    };

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedPairRequest {
        authorization: Option<String>,
        body: GeneratePairRequest,
    }

    #[derive(Clone)]
    struct TestPairState {
        sender: Arc<Mutex<Option<oneshot::Sender<CapturedPairRequest>>>>,
        pair_response: PairCodeResponse,
        device_count: usize,
    }

    struct TestPairServer {
        port: u16,
        receiver: Option<oneshot::Receiver<CapturedPairRequest>>,
        handle: JoinHandle<()>,
    }

    impl TestPairServer {
        async fn spawn(response: PairCodeResponse) -> Self {
            Self::spawn_with_device_count(response, 0).await
        }

        async fn spawn_with_device_count(response: PairCodeResponse, device_count: usize) -> Self {
            let (sender, receiver) = oneshot::channel();
            let app = Router::new()
                .route("/v1/pair/generate", post(capture_pair_request))
                .route("/v1/devices", get(list_devices))
                .route("/health", get(health_check))
                .with_state(TestPairState {
                    sender: Arc::new(Mutex::new(Some(sender))),
                    pair_response: response,
                    device_count,
                });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("test server should bind");
            let port = listener
                .local_addr()
                .expect("local address should exist")
                .port();
            let handle = tokio::spawn(async move {
                axum::serve(listener, app)
                    .await
                    .expect("test server should run");
            });
            Self {
                port,
                receiver: Some(receiver),
                handle,
            }
        }

        async fn captured(&mut self) -> CapturedPairRequest {
            let receiver = self.receiver.take().expect("receiver should be present");
            timeout(Duration::from_secs(2), receiver)
                .await
                .expect("request should arrive")
                .expect("request should be captured")
        }
    }

    impl Drop for TestPairServer {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }

    async fn capture_pair_request(
        State(state): State<TestPairState>,
        headers: HeaderMap,
        Json(body): Json<GeneratePairRequest>,
    ) -> (StatusCode, Json<PairCodeResponse>) {
        if let Some(sender) = state.sender.lock().await.take() {
            let _ = sender.send(CapturedPairRequest {
                authorization: headers
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned),
                body,
            });
        }
        (StatusCode::OK, Json(state.pair_response))
    }

    async fn list_devices(State(state): State<TestPairState>) -> Json<DevicesResponse> {
        Json(DevicesResponse {
            devices: test_devices(state.device_count),
        })
    }

    async fn health_check() -> StatusCode {
        StatusCode::OK
    }

    fn test_layout(root: &Path, port: u16, token: Option<&str>) -> RuntimeLayout {
        let mut config = FawxConfig::default();
        config.http.bearer_token = token.map(str::to_string);
        RuntimeLayout {
            data_dir: root.to_path_buf(),
            config_path: root.join("config.toml"),
            storage_dir: root.join("storage"),
            audit_log_path: root.join("audit.log"),
            auth_db_path: root.join("auth.db"),
            logs_dir: root.join("logs"),
            skills_dir: root.join("skills"),
            trusted_keys_dir: root.join("trusted_keys"),
            embedding_model_dir: root.join("models"),
            pid_file: root.join("fawx.pid"),
            memory_json_path: root.join("memory").join("memory.json"),
            sessions_dir: root.join("signals"),
            security_baseline_path: root.join("security-baseline.json"),
            repo_root: PathBuf::from("/tmp/fawx"),
            http_port: port,
            config,
        }
    }

    fn test_devices(count: usize) -> Vec<DeviceInfo> {
        (0..count)
            .map(|index| DeviceInfo {
                id: format!("dev-{index}"),
                device_name: format!("Device {index}"),
                created_at: 1_700_000_000,
                last_used_at: 1_700_000_000,
            })
            .collect()
    }

    async fn unused_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        listener
            .local_addr()
            .expect("listener should have address")
            .port()
    }

    #[tokio::test]
    async fn pair_json_output_format() {
        let temp = tempdir().expect("tempdir");
        let mut server = TestPairServer::spawn(PairCodeResponse {
            code: "A7K-M2X".to_string(),
            expires_at: 1_773_436_000,
            ttl_seconds: 123,
        })
        .await;
        let layout = test_layout(temp.path(), server.port, Some("secret-token"));
        let client = http_client().expect("client");

        let pair = fetch_pairing_code(&layout, &client, 123)
            .await
            .expect("pair request should succeed");
        let captured = server.captured().await;
        let json: Value = serde_json::from_str(&pair_json_string(&pair).expect("json string"))
            .expect("pair JSON should parse");

        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer secret-token")
        );
        assert_eq!(captured.body.ttl_seconds, 123);
        assert_eq!(json["code"], "A7K-M2X");
        assert_eq!(json["expires_at"], 1_773_436_000);
        assert_eq!(json["ttl_seconds"], 123);
    }

    #[tokio::test]
    async fn pair_requires_running_server() {
        let temp = tempdir().expect("tempdir");
        let port = unused_port().await;
        let layout = test_layout(temp.path(), port, Some("secret-token"));
        let client = http_client().expect("client");

        let error = fetch_pairing_code(&layout, &client, DEFAULT_PAIR_TTL_SECONDS)
            .await
            .expect_err("missing server should fail");

        assert_eq!(error.to_string(), server_not_running_message());
    }

    #[tokio::test]
    async fn detect_pairing_returns_paired_when_device_count_increases() {
        let temp = tempdir().expect("tempdir");
        let server = TestPairServer::spawn_with_device_count(
            PairCodeResponse {
                code: "A7K-M2X".to_string(),
                expires_at: 1_773_436_000,
                ttl_seconds: 123,
            },
            2,
        )
        .await;
        let layout = test_layout(temp.path(), server.port, Some("secret-token"));
        let client = http_client().expect("client");

        let event = detect_pairing(&client, &layout, 1).await;

        assert_eq!(event, PairWaitEvent::Paired);
    }
}
