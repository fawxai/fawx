use crate::commands::slash::display_path_for_user;
use crate::startup::fawx_data_dir;
use clap::Subcommand;
use fx_fleet::{FleetError, FleetManager, NodeInfo, NodeStatus};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const FLEET_DIR_NAME: &str = "fleet";
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
        /// Node name (e.g., "macmini")
        name: String,
        /// Tailscale IP address
        #[arg(long)]
        ip: String,
        /// API port (default: 8400)
        #[arg(long, default_value = "8400")]
        port: u16,
    },
    /// Remove a worker node from the fleet
    Remove {
        /// Node name to remove
        name: String,
    },
    /// List all registered fleet nodes
    List,
}

pub fn handle_fleet_command(command: &FleetCommands) -> anyhow::Result<()> {
    let fleet_dir = default_fleet_dir();
    let mut stdout = std::io::stdout();
    execute_fleet_command(command, &fleet_dir, &mut stdout).map_err(anyhow::Error::from)
}

fn default_fleet_dir() -> PathBuf {
    fawx_data_dir().join(FLEET_DIR_NAME)
}

fn execute_fleet_command(
    command: &FleetCommands,
    fleet_dir: &Path,
    writer: &mut impl Write,
) -> Result<(), FleetError> {
    match command {
        FleetCommands::Init => run_init_command(fleet_dir, writer),
        FleetCommands::Add { name, ip, port } => {
            run_add_command(fleet_dir, name, ip, *port, writer)
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

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
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
    use fx_fleet::FleetToken;
    use serde_json::from_slice;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn fleet_init_creates_directory() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut output = Vec::new();

        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut output)
            .expect("fleet init should succeed");

        assert!(fleet_dir.is_dir());
        assert!(fleet_dir.join(FLEET_KEY_FILE).is_file());
        assert!(fleet_dir.join(NODES_FILE).is_file());
        assert!(fleet_dir.join(TOKENS_FILE).is_file());
        assert!(String::from_utf8(output)
            .expect("utf8")
            .contains("Fleet initialized at"));
    }

    #[test]
    fn fleet_add_prints_join_command() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .expect("fleet init should succeed");

        let mut output = Vec::new();
        execute_fleet_command(
            &FleetCommands::Add {
                name: "macmini".to_string(),
                ip: "100.75.191.19".to_string(),
                port: 8400,
            },
            &fleet_dir,
            &mut output,
        )
        .expect("fleet add should succeed");

        let output = String::from_utf8(output).expect("utf8");
        let tokens = read_tokens(&fleet_dir);
        let token = tokens.first().expect("token should exist");

        assert!(output.contains("✓ Node \"macmini\" registered"));
        assert!(output.contains("✓ Token generated"));
        assert!(output.contains("Join command (run on the worker):"));
        assert!(output.contains(&format!(
            "fawx fleet join 100.75.191.19:8400 --token {}",
            token.secret
        )));
    }

    #[test]
    fn fleet_add_duplicate_returns_error() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .expect("fleet init should succeed");
        let mut first_output = Vec::new();
        execute_fleet_command(
            &FleetCommands::Add {
                name: "macmini".to_string(),
                ip: "100.75.191.19".to_string(),
                port: 8400,
            },
            &fleet_dir,
            &mut first_output,
        )
        .expect("first add should succeed");

        let result = execute_fleet_command(
            &FleetCommands::Add {
                name: "macmini".to_string(),
                ip: "100.75.191.20".to_string(),
                port: 8400,
            },
            &fleet_dir,
            &mut Vec::new(),
        );

        assert!(matches!(result, Err(FleetError::DuplicateNode)));
    }

    #[test]
    fn fleet_remove_success() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut manager = FleetManager::init(&fleet_dir).expect("fleet should initialize");
        let token = manager
            .add_node("macmini", "100.75.191.19", 8400)
            .expect("node should add");

        let mut output = Vec::new();
        execute_fleet_command(
            &FleetCommands::Remove {
                name: "macmini".to_string(),
            },
            &fleet_dir,
            &mut output,
        )
        .expect("fleet remove should succeed");

        let reloaded_manager = FleetManager::load(&fleet_dir).expect("fleet should load");
        let output = String::from_utf8(output).expect("utf8");

        assert!(output.contains("✓ Node \"macmini\" removed and token revoked"));
        assert_eq!(reloaded_manager.verify_bearer(&token.secret), None);
        assert!(reloaded_manager.list_nodes().is_empty());
    }

    #[test]
    fn fleet_remove_nonexistent_returns_error() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .expect("fleet init should succeed");

        let result = execute_fleet_command(
            &FleetCommands::Remove {
                name: "missing".to_string(),
            },
            &fleet_dir,
            &mut Vec::new(),
        );

        assert!(matches!(result, Err(FleetError::NodeNotFound)));
    }

    #[test]
    fn fleet_list_empty_shows_no_nodes() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .expect("fleet init should succeed");

        let mut output = Vec::new();
        execute_fleet_command(&FleetCommands::List, &fleet_dir, &mut output)
            .expect("fleet list should succeed");

        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("Fleet Nodes:"));
        assert!(output.contains("(no nodes registered)"));
    }

    #[test]
    fn fleet_list_with_nodes_shows_table() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut init_output = Vec::new();
        execute_fleet_command(&FleetCommands::Init, &fleet_dir, &mut init_output)
            .expect("fleet init should succeed");

        let mut manager = FleetManager::load(&fleet_dir).expect("fleet should load");
        manager
            .add_node("macmini", "100.75.191.19", 8400)
            .expect("first node should add");
        manager
            .add_node("macbook", "100.75.191.20", 8400)
            .expect("second node should add");

        let now_ms = current_time_ms();
        let mut nodes = read_nodes(&fleet_dir);
        for node in &mut nodes {
            if node.name == "macbook" {
                node.status = NodeStatus::Online;
                node.last_heartbeat_ms = now_ms.saturating_sub(65_000);
            }
        }
        write_nodes(&fleet_dir, &nodes);

        let mut output = Vec::new();
        execute_fleet_command(&FleetCommands::List, &fleet_dir, &mut output)
            .expect("fleet list should succeed");

        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("Fleet Nodes:"));
        assert!(output.contains("macbook"));
        assert!(output.contains("macmini"));
        assert!(output.contains("100.75.191.19:8400"));
        assert!(output.contains("100.75.191.20:8400"));
        assert!(output.contains("online"));
        assert!(output.contains("offline"));
        assert!(output.contains("1m ago"));
        assert!(output.contains("(never seen)"));
    }

    #[test]
    fn fleet_list_does_not_leak_token_secret() {
        let temp_dir = TempDir::new().expect("tempdir should create");
        let fleet_dir = temp_dir.path().join("fleet");
        let mut manager = FleetManager::init(&fleet_dir).expect("fleet should initialize");
        let token = manager
            .add_node("macmini", "100.75.191.19", 8400)
            .expect("node should add");

        let nodes = manager.list_nodes();
        let output = render_list_output(&nodes, current_time_ms());

        assert!(output.contains("macmini"));
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
