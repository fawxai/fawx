use crate::commands::slash::display_path_for_user;
use crate::startup::fawx_data_dir;
use clap::Subcommand;
use fx_fleet::{
    current_time_ms, FleetError, FleetHttpClient, FleetIdentity, FleetManager,
    FleetRegistrationRequest, FleetRegistrationResponse, NodeInfo, NodeStatus,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) const FLEET_DIR_NAME: &str = "fleet";
pub(crate) const IDENTITY_FILE: &str = "identity.json";
#[cfg(test)]
const FLEET_KEY_FILE: &str = "fleet.key";
#[cfg(test)]
const NODES_FILE: &str = "nodes.json";
#[cfg(test)]
const TOKENS_FILE: &str = "tokens.json";

#[derive(Debug, Clone, Subcommand)]
pub enum FleetCommands {
    /// Initialize fleet on this node (primary role)
    Init,
    /// Add a worker node to the fleet
    Add {
        /// Node name (e.g., "node-alpha")
        name: String,
        /// Tailscale IP address
        #[arg(long)]
        ip: String,
        /// API port (default: 8400)
        #[arg(long, default_value = "8400")]
        port: u16,
    },
    /// Join a fleet as a worker node
    Join {
        /// Primary node endpoint (e.g., 10.0.0.1:8400)
        primary: String,
        /// Bearer token from `fawx fleet add`
        #[arg(long)]
        token: String,
    },
    /// Remove a worker node from the fleet
    Remove {
        /// Node name to remove
        name: String,
    },
    /// List all registered fleet nodes
    List,
}

pub async fn handle_fleet_command(command: &FleetCommands) -> anyhow::Result<()> {
    let fleet_dir = default_fleet_dir();
    let mut stdout = std::io::stdout();
    execute_fleet_command(command, &fleet_dir, &mut stdout)
        .await
        .map_err(anyhow::Error::from)
}

pub(crate) fn default_fleet_dir() -> PathBuf {
    fawx_data_dir().join(FLEET_DIR_NAME)
}

async fn execute_fleet_command(
    command: &FleetCommands,
    fleet_dir: &Path,
    writer: &mut impl Write,
) -> Result<(), FleetError> {
    match command {
        FleetCommands::Init => run_init_command(fleet_dir, writer),
        FleetCommands::Add { name, ip, port } => {
            run_add_command(fleet_dir, name, ip, *port, writer)
        }
        FleetCommands::Join { primary, token } => {
            run_join_command(fleet_dir, primary, token, writer).await
        }
        FleetCommands::Remove { name } => run_remove_command(fleet_dir, name, writer),
        FleetCommands::List => run_list_command(fleet_dir, writer),
    }
}

fn run_init_command(fleet_dir: &Path, writer: &mut impl Write) -> Result<(), FleetError> {
    FleetManager::init(fleet_dir)?;
    writer.write_all(render_init_output(fleet_dir).as_bytes())?;
    Ok(())
}

fn run_add_command(
    fleet_dir: &Path,
    name: &str,
    ip: &str,
    port: u16,
    writer: &mut impl Write,
) -> Result<(), FleetError> {
    let mut manager = FleetManager::load(fleet_dir)?;
    let token = manager.add_node(name, ip, port)?;
    writer.write_all(render_add_output(name, ip, port, &token.secret).as_bytes())?;
    Ok(())
}

async fn run_join_command(
    fleet_dir: &Path,
    primary: &str,
    token: &str,
    writer: &mut impl Write,
) -> Result<(), FleetError> {
    let join_request = build_join_request(token)?;
    let primary_endpoint = primary_endpoint(primary);
    let client = FleetHttpClient::new(Duration::from_secs(10));
    let response = client
        .register(&primary_endpoint, &join_request.request)
        .await?;
    ensure_registration_accepted(&response)?;

    let identity = FleetIdentity {
        node_id: response.node_id.clone(),
        primary_endpoint: primary_endpoint.clone(),
        bearer_token: token.to_string(),
        registered_at_ms: current_time_ms(),
    };
    let identity_path = identity_path(fleet_dir);
    identity.save(&identity_path)?;

    writer.write_all(
        render_join_output(&join_request.summary, primary, &response, &identity_path).as_bytes(),
    )?;
    Ok(())
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

fn run_remove_command(
    fleet_dir: &Path,
    name: &str,
    writer: &mut impl Write,
) -> Result<(), FleetError> {
    let mut manager = FleetManager::load(fleet_dir)?;
    manager.remove_node(name)?;
    writer.write_all(render_remove_output(name).as_bytes())?;
    Ok(())
}

fn run_list_command(fleet_dir: &Path, writer: &mut impl Write) -> Result<(), FleetError> {
    let manager = FleetManager::load(fleet_dir)?;
    let nodes = manager.list_nodes();
    let output = render_list_output(&nodes, current_time_ms());
    writer.write_all(output.as_bytes())?;
    Ok(())
}

fn build_join_request(token: &str) -> Result<JoinRequest, FleetError> {
    let summary = detect_capability_summary()?;
    let request = registration_request(token, &summary);
    Ok(JoinRequest { request, summary })
}

pub(crate) fn build_registration_request(
    token: &str,
) -> Result<FleetRegistrationRequest, FleetError> {
    let summary = detect_capability_summary()?;
    Ok(registration_request(token, &summary))
}

fn registration_request(token: &str, summary: &CapabilitySummary) -> FleetRegistrationRequest {
    FleetRegistrationRequest {
        node_name: summary.node_name.clone(),
        bearer_token: token.to_string(),
        capabilities: vec!["agentic_loop".to_string(), summary.platform.clone()],
        rust_version: None,
        os: Some(std::env::consts::OS.to_string()),
        cpus: Some(summary.cpus),
        ram_gb: None,
    }
}

fn detect_capability_summary() -> Result<CapabilitySummary, FleetError> {
    Ok(CapabilitySummary {
        node_name: detected_node_name(),
        cpus: detected_cpus()?,
        platform: detected_platform(),
    })
}

fn detected_node_name() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|output| parsed_hostname(&output.stdout))
        .unwrap_or_else(|| "unknown".to_string())
}

fn parsed_hostname(output: &[u8]) -> Option<String> {
    std::str::from_utf8(output)
        .ok()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn detected_cpus() -> Result<u32, FleetError> {
    std::thread::available_parallelism()
        .map(|parallelism| u32::try_from(parallelism.get()).unwrap_or(u32::MAX))
        .map_err(FleetError::from)
}

fn detected_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn primary_endpoint(primary: &str) -> String {
    if primary.starts_with("http://") || primary.starts_with("https://") {
        primary.to_string()
    } else {
        format!("http://{primary}")
    }
}

pub(crate) fn identity_path(fleet_dir: &Path) -> PathBuf {
    fleet_dir.join(IDENTITY_FILE)
}

fn render_init_output(fleet_dir: &Path) -> String {
    let fleet_dir = display_fleet_dir(fleet_dir);
    format!(
        "✓ Fleet initialized at {fleet_dir}\n✓ Signing key generated\n✓ Ready to add nodes with: fawx fleet add <name> --ip <ip>\n"
    )
}

fn render_add_output(name: &str, ip: &str, port: u16, secret: &str) -> String {
    format!(
        "✓ Node \"{name}\" registered\n✓ Token generated\n\n  Join command (run on the worker):\n  fawx fleet join {ip}:{port} --token {secret}\n"
    )
}

fn render_join_output(
    summary: &CapabilitySummary,
    primary: &str,
    response: &FleetRegistrationResponse,
    identity_path: &Path,
) -> String {
    format!(
        "✓ Connected to primary at {primary}\n✓ Registered as node \"{}\"\n✓ Capabilities: {} CPUs, {}\n✓ Identity saved to {}\n\n  Start the fleet worker with:\n  fawx serve --fleet\n",
        response.node_id,
        summary.cpus,
        summary.platform,
        display_path_for_user(identity_path),
    )
}

fn render_remove_output(name: &str) -> String {
    format!("✓ Node \"{name}\" removed and token revoked\n")
}

fn render_list_output(nodes: &[&NodeInfo], now_ms: u64) -> String {
    if nodes.is_empty() {
        return "Fleet Nodes:\n  (no nodes registered)\n".to_string();
    }

    let mut sorted = nodes.to_vec();
    sorted.sort_by(|left, right| left.name.cmp(&right.name));
    let widths = table_widths(&sorted);
    let rows = sorted
        .into_iter()
        .map(|node| render_node_row(node, &widths, now_ms))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Fleet Nodes:\n{rows}\n")
}

fn render_node_row(node: &NodeInfo, widths: &TableWidths, now_ms: u64) -> String {
    let endpoint = endpoint_for_display(&node.endpoint);
    let status = status_label(&node.status);
    let last_seen = format_last_seen(now_ms, node.last_heartbeat_ms);
    format!(
        "  {name:<name_width$}  {endpoint:<endpoint_width$}  {status:<status_width$}  {last_seen}",
        name = node.name,
        endpoint = endpoint,
        status = status,
        name_width = widths.name,
        endpoint_width = widths.endpoint,
        status_width = widths.status,
    )
}

fn display_fleet_dir(fleet_dir: &Path) -> String {
    let mut display = display_path_for_user(fleet_dir);
    if !display.ends_with('/') {
        display.push('/');
    }
    display
}

fn endpoint_for_display(endpoint: &str) -> String {
    endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
        .unwrap_or(endpoint)
        .to_string()
}

fn status_label(status: &NodeStatus) -> &'static str {
    match status {
        NodeStatus::Online => "online",
        NodeStatus::Stale => "stale",
        NodeStatus::Offline => "offline",
        NodeStatus::Busy => "busy",
    }
}

fn format_last_seen(now_ms: u64, last_heartbeat_ms: u64) -> String {
    if last_heartbeat_ms == 0 {
        return "(never seen)".to_string();
    }
    format_relative_age(now_ms.saturating_sub(last_heartbeat_ms))
}

fn format_relative_age(age_ms: u64) -> String {
    let age_secs = age_ms / 1_000;
    if age_secs < 60 {
        return format!("{}s ago", age_secs.max(1));
    }
    if age_secs < 3_600 {
        return format!("{}m ago", age_secs / 60);
    }
    if age_secs < 86_400 {
        return format!("{}h ago", age_secs / 3_600);
    }
    format!("{}d ago", age_secs / 86_400)
}

fn table_widths(nodes: &[&NodeInfo]) -> TableWidths {
    let mut widths = TableWidths::default();
    for node in nodes {
        widths.name = widths.name.max(node.name.len());
        widths.endpoint = widths
            .endpoint
            .max(endpoint_for_display(&node.endpoint).len());
        widths.status = widths.status.max(status_label(&node.status).len());
    }
    widths
}

#[derive(Debug, Clone)]
struct JoinRequest {
    request: FleetRegistrationRequest,
    summary: CapabilitySummary,
}

#[derive(Debug, Clone)]
struct CapabilitySummary {
    node_name: String,
    cpus: u32,
    platform: String,
}

#[derive(Debug, Clone, Copy)]
struct TableWidths {
    name: usize,
    endpoint: usize,
    status: usize,
}

impl Default for TableWidths {
    fn default() -> Self {
        Self {
            name: 4,
            endpoint: 8,
            status: 7,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Bytes,
        extract::State,
        http::{header, HeaderMap, Method, StatusCode, Uri},
        response::{IntoResponse, Response},
        routing::post,
        Json, Router,
    };
    use fx_fleet::FleetToken;
    use serde_json::from_slice;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::{
        sync::{oneshot, Mutex},
        task::JoinHandle,
        time::timeout,
    };

    #[derive(Debug, Clone)]
    struct TestRegisterResponse {
        status: StatusCode,
        body: FleetRegistrationResponse,
    }

    #[derive(Debug)]
    struct CapturedRegistration {
        authorization: Option<String>,
        json: FleetRegistrationRequest,
        method: Method,
        path: String,
    }

    #[derive(Clone)]
    struct TestServerState {
        sender: Arc<Mutex<Option<oneshot::Sender<CapturedRegistration>>>>,
        response: TestRegisterResponse,
    }

    struct TestRegisterServer {
        base_url: String,
        receiver: Option<oneshot::Receiver<CapturedRegistration>>,
        handle: JoinHandle<()>,
    }

    impl TestRegisterServer {
        async fn spawn(response: TestRegisterResponse) -> Self {
            let (sender, receiver) = oneshot::channel();
            let app = Router::new()
                .route("/fleet/register", post(capture_registration))
                .with_state(TestServerState {
                    sender: Arc::new(Mutex::new(Some(sender))),
                    response,
                });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("test server should bind");
            let address = listener.local_addr().expect("local address should exist");
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

        async fn captured(&mut self) -> CapturedRegistration {
            let receiver = self.receiver.take().expect("receiver should be available");
            timeout(Duration::from_secs(2), receiver)
                .await
                .expect("request should arrive")
                .expect("request should be captured")
        }
    }

    impl Drop for TestRegisterServer {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }

    async fn capture_registration(
        State(state): State<TestServerState>,
        method: Method,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let captured = CapturedRegistration {
            authorization: headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            json: serde_json::from_slice(&body).expect("request should decode"),
            method,
            path: uri.path().to_string(),
        };
        if let Some(sender) = state.sender.lock().await.take() {
            let _ = sender.send(captured);
        }
        Json(state.response.body)
            .into_response()
            .with_status(state.response.status)
    }

    trait ResponseStatusExt {
        fn with_status(self, status: StatusCode) -> Response;
    }

    impl ResponseStatusExt for Response {
        fn with_status(mut self, status: StatusCode) -> Response {
            *self.status_mut() = status;
            self
        }
    }

    #[test]
    fn parsed_hostname_trims_trailing_newline() {
        assert_eq!(parsed_hostname(b"macmini\n"), Some("macmini".to_string()));
    }

    #[test]
    fn parsed_hostname_rejects_blank_output() {
        assert_eq!(parsed_hostname(b"  \n\t"), None);
    }

    #[test]
    fn parsed_hostname_rejects_invalid_utf8() {
        assert_eq!(parsed_hostname(&[0xff, 0xfe]), None);
    }

    #[tokio::test]
    async fn fleet_init_creates_directory() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut output = Vec::new();

        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut output)
            .await
            .expect("fleet init should succeed");

        assert!(fleet_dir.is_dir());
        assert!(fleet_dir.join(FLEET_KEY_FILE).is_file());
        assert!(fleet_dir.join(NODES_FILE).is_file());
        assert!(fleet_dir.join(TOKENS_FILE).is_file());
        assert!(String::from_utf8(output)
            .expect("utf8")
            .contains("Fleet initialized at"));
    }

    #[tokio::test]
    async fn fleet_add_prints_join_command() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .await
            .expect("fleet init should succeed");

        let mut output = Vec::new();
        execute_fleet_command(
            &FleetCommands::Add {
                name: "node-alpha".to_string(),
                ip: "10.0.0.2".to_string(),
                port: 8400,
            },
            &fleet_dir,
            &mut output,
        )
        .await
        .expect("fleet add should succeed");

        let output = String::from_utf8(output).expect("utf8");
        let tokens = read_tokens(&fleet_dir);
        let token = tokens.first().expect("token should exist");

        assert!(output.contains("✓ Node \"node-alpha\" registered"));
        assert!(output.contains("✓ Token generated"));
        assert!(output.contains("Join command (run on the worker):"));
        assert!(output.contains(&format!(
            "fawx fleet join 10.0.0.2:8400 --token {}",
            token.secret
        )));
    }

    #[tokio::test]
    async fn fleet_add_duplicate_returns_error() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .await
            .expect("fleet init should succeed");
        let mut first_output = Vec::new();
        execute_fleet_command(
            &FleetCommands::Add {
                name: "node-alpha".to_string(),
                ip: "10.0.0.2".to_string(),
                port: 8400,
            },
            &fleet_dir,
            &mut first_output,
        )
        .await
        .expect("first add should succeed");

        let result = execute_fleet_command(
            &FleetCommands::Add {
                name: "node-alpha".to_string(),
                ip: "10.0.0.3".to_string(),
                port: 8400,
            },
            &fleet_dir,
            &mut Vec::new(),
        )
        .await;

        assert!(matches!(result, Err(FleetError::DuplicateNode)));
    }

    #[tokio::test]
    async fn fleet_join_saves_identity() {
        let mut server = TestRegisterServer::spawn(TestRegisterResponse {
            status: StatusCode::OK,
            body: FleetRegistrationResponse {
                node_id: "macmini-a1b2c3".to_string(),
                accepted: true,
                message: "registered".to_string(),
            },
        })
        .await;
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let primary = server.base_url.trim_start_matches("http://").to_string();
        let token = "tok_abc123";
        let mut output = Vec::new();

        execute_fleet_command(
            &FleetCommands::Join {
                primary: primary.clone(),
                token: token.to_string(),
            },
            &fleet_dir,
            &mut output,
        )
        .await
        .expect("fleet join should succeed");

        let identity =
            FleetIdentity::load(&fleet_dir.join(IDENTITY_FILE)).expect("identity should load");
        let captured = server.captured().await;
        let output = String::from_utf8(output).expect("utf8");

        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, "/fleet/register");
        assert!(captured.authorization.is_none());
        assert_eq!(captured.json.bearer_token, token);
        assert!(!captured.json.node_name.trim().is_empty());
        assert_eq!(captured.json.os.as_deref(), Some(std::env::consts::OS));
        assert!(captured
            .json
            .capabilities
            .contains(&"agentic_loop".to_string()));
        assert_eq!(identity.node_id, "macmini-a1b2c3");
        assert_eq!(identity.primary_endpoint, server.base_url);
        assert_eq!(identity.bearer_token, token);
        assert!(identity.registered_at_ms > 0);
        assert!(output.contains("✓ Connected to primary at"));
        assert!(output.contains("✓ Registered as node \"macmini-a1b2c3\""));
        assert!(output.contains("✓ Identity saved to"));
    }

    #[tokio::test]
    async fn fleet_remove_success() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut manager = FleetManager::init(&fleet_dir).expect("fleet should initialize");
        let token = manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("node should add");

        let mut output = Vec::new();
        execute_fleet_command(
            &FleetCommands::Remove {
                name: "node-alpha".to_string(),
            },
            &fleet_dir,
            &mut output,
        )
        .await
        .expect("fleet remove should succeed");

        let reloaded_manager = FleetManager::load(&fleet_dir).expect("fleet should load");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("✓ Node \"node-alpha\" removed and token revoked"));
        assert_eq!(reloaded_manager.verify_bearer(&token.secret), None);
        assert!(reloaded_manager.list_nodes().is_empty());
    }

    #[tokio::test]
    async fn fleet_remove_nonexistent_returns_error() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .await
            .expect("fleet init should succeed");

        let result = execute_fleet_command(
            &FleetCommands::Remove {
                name: "missing".to_string(),
            },
            &fleet_dir,
            &mut Vec::new(),
        )
        .await;

        assert!(matches!(result, Err(FleetError::NodeNotFound)));
    }

    #[tokio::test]
    async fn fleet_list_empty_shows_no_nodes() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .await
            .expect("fleet init should succeed");

        let mut output = Vec::new();
        execute_fleet_command(&FleetCommands::List, &fleet_dir, &mut output)
            .await
            .expect("fleet list should succeed");

        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("Fleet Nodes:"));
        assert!(output.contains("(no nodes registered)"));
    }

    #[tokio::test]
    async fn fleet_list_with_nodes_shows_table() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .await
            .expect("fleet init should succeed");

        let mut manager = FleetManager::load(&fleet_dir).expect("fleet should load");
        manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("first node should add");
        manager
            .add_node("node-beta", "10.0.0.3", 8400)
            .expect("second node should add");

        let now_ms = current_time_ms();
        let mut nodes = read_nodes(&fleet_dir);
        for node in &mut nodes {
            if node.name == "node-beta" {
                node.status = NodeStatus::Online;
                node.last_heartbeat_ms = now_ms.saturating_sub(65_000);
            }
        }
        write_nodes(&fleet_dir, &nodes);

        let mut output = Vec::new();
        execute_fleet_command(&FleetCommands::List, &fleet_dir, &mut output)
            .await
            .expect("fleet list should succeed");

        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("Fleet Nodes:"));
        assert!(output.contains("node-beta"));
        assert!(output.contains("node-alpha"));
        assert!(output.contains("10.0.0.2:8400"));
        assert!(output.contains("10.0.0.3:8400"));
        assert!(output.contains("online"));
        assert!(output.contains("offline"));
        assert!(output.contains("1m ago"));
        assert!(output.contains("(never seen)"));
    }

    #[tokio::test]
    async fn fleet_list_does_not_leak_token_secret() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut manager = FleetManager::init(&fleet_dir).expect("fleet should initialize");
        let token = manager
            .add_node("node-alpha", "10.0.0.2", 8400)
            .expect("node should add");

        let nodes = manager.list_nodes();
        let output = render_list_output(&nodes, current_time_ms());

        assert!(output.contains("node-alpha"));
        assert!(!output.contains(&token.secret));
    }

    fn read_tokens(fleet_dir: &Path) -> Vec<FleetToken> {
        let bytes = fs::read(fleet_dir.join(TOKENS_FILE)).expect("tokens.json should exist");
        from_slice(&bytes).expect("tokens.json should deserialize")
    }

    fn read_nodes(fleet_dir: &Path) -> Vec<NodeInfo> {
        let bytes = fs::read(fleet_dir.join(NODES_FILE)).expect("nodes.json should exist");
        from_slice(&bytes).expect("nodes.json should deserialize")
    }

    fn write_nodes(fleet_dir: &Path, nodes: &[NodeInfo]) {
        let mut json = serde_json::to_vec_pretty(nodes).expect("nodes should serialize");
        json.push(b'\n');
        fs::write(fleet_dir.join(NODES_FILE), json).expect("nodes.json should write");
    }
}
