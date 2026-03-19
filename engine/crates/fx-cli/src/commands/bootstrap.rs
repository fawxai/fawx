use super::setup::{load_config_document, random_hex, set_bool, set_integer, set_string};
use crate::auth_store::{open_auth_store_with_recovery, AuthStore};
use crate::startup::fawx_data_dir;
use std::{
    fs,
    path::{Path, PathBuf},
};
use toml_edit::DocumentMut;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT_START: u16 = 8400;
const DEFAULT_PORT_END: u16 = 8410;
const HTTP_BEARER_PROVIDER: &str = "http_bearer";

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct BootstrapOutput {
    port: u16,
    host: String,
    bearer_token: String,
    data_dir: String,
    config_path: String,
    created: bool,
}

#[derive(serde::Serialize)]
struct BootstrapError {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    port_range: Option<[u16; 2]>,
}

#[derive(Debug, PartialEq, Eq)]
enum BootstrapFailure {
    Message(String),
    PortRangeExhausted { start: u16, end: u16 },
}

impl BootstrapFailure {
    fn message(&self) -> String {
        match self {
            Self::Message(message) => message.clone(),
            Self::PortRangeExhausted { start, end } => {
                format!("All ports {start}-{end} are in use")
            }
        }
    }

    fn port_range(&self) -> Option<[u16; 2]> {
        match self {
            Self::PortRangeExhausted { start, end } => Some([*start, *end]),
            Self::Message(_) => None,
        }
    }
}

pub async fn run(
    json: bool,
    port_override: Option<u16>,
    data_dir_override: Option<PathBuf>,
) -> anyhow::Result<i32> {
    let data_dir = data_dir_override.unwrap_or_else(fawx_data_dir);
    match bootstrap(&data_dir, port_override) {
        Ok(output) => {
            print_success(&output, json)?;
            Ok(0)
        }
        Err(error) => {
            print_failure(&error, json)?;
            Ok(1)
        }
    }
}

fn bootstrap(
    data_dir: &Path,
    port_override: Option<u16>,
) -> Result<BootstrapOutput, BootstrapFailure> {
    create_data_dir(data_dir)?;
    let config_path = data_dir.join("config.toml");
    let document = load_document(&config_path)?;
    if let Some(output) = existing_output(&document, data_dir, &config_path) {
        return Ok(output);
    }
    let auth_store = open_auth_store(data_dir)?;
    write_bootstrap(document, &auth_store, data_dir, &config_path, port_override)
}

fn create_data_dir(data_dir: &Path) -> Result<(), BootstrapFailure> {
    fs::create_dir_all(data_dir).map_err(|error| {
        BootstrapFailure::Message(format!(
            "failed to create data directory {}: {error}",
            data_dir.display()
        ))
    })
}

fn load_document(config_path: &Path) -> Result<DocumentMut, BootstrapFailure> {
    load_config_document(config_path).map_err(|error| BootstrapFailure::Message(error.to_string()))
}

fn existing_output(
    document: &DocumentMut,
    data_dir: &Path,
    config_path: &Path,
) -> Option<BootstrapOutput> {
    let (token, port) = existing_http_config(document);
    let token = token.and_then(|value| normalized_token(&value));
    let port = port.and_then(normalized_port);
    match (token, port) {
        (Some(token), Some(port)) => Some(build_output(token, port, data_dir, config_path, false)),
        _ => None,
    }
}

fn open_auth_store(data_dir: &Path) -> Result<AuthStore, BootstrapFailure> {
    open_auth_store_with_recovery(data_dir)
        .map(|recovered| recovered.store)
        .map_err(BootstrapFailure::Message)
}

fn write_bootstrap(
    mut document: DocumentMut,
    auth_store: &AuthStore,
    data_dir: &Path,
    config_path: &Path,
    port_override: Option<u16>,
) -> Result<BootstrapOutput, BootstrapFailure> {
    let (config_token, config_port) = existing_http_config(&document);
    let bearer_token = resolve_bearer_token(config_token, auth_store)?;
    let port = resolve_port(config_port, port_override)?;
    store_bearer_token(auth_store, &bearer_token)?;
    write_http_config(&mut document, port, &bearer_token)?;
    save_document(config_path, &document)?;
    Ok(build_output(
        bearer_token,
        port,
        data_dir,
        config_path,
        true,
    ))
}

fn resolve_bearer_token(
    config_token: Option<String>,
    auth_store: &AuthStore,
) -> Result<String, BootstrapFailure> {
    if let Some(token) = config_token.and_then(|value| normalized_token(&value)) {
        return Ok(token);
    }
    if let Some(token) = stored_bearer_token(auth_store)? {
        return Ok(token);
    }
    random_hex(32).map_err(|error| BootstrapFailure::Message(error.to_string()))
}

fn stored_bearer_token(auth_store: &AuthStore) -> Result<Option<String>, BootstrapFailure> {
    auth_store
        .get_provider_token(HTTP_BEARER_PROVIDER)
        .map(|token| token.and_then(|token| normalized_token(token.as_str())))
        .map_err(BootstrapFailure::Message)
}

fn resolve_port(
    config_port: Option<i64>,
    port_override: Option<u16>,
) -> Result<u16, BootstrapFailure> {
    if let Some(port) = config_port.and_then(normalized_port) {
        return Ok(port);
    }
    match port_override {
        Some(port) => ensure_port_available(port),
        None => find_available_port(DEFAULT_PORT_START, DEFAULT_PORT_END).ok_or(
            BootstrapFailure::PortRangeExhausted {
                start: DEFAULT_PORT_START,
                end: DEFAULT_PORT_END,
            },
        ),
    }
}

fn ensure_port_available(port: u16) -> Result<u16, BootstrapFailure> {
    std::net::TcpListener::bind((DEFAULT_HOST, port))
        .map(|_| port)
        .map_err(|error| BootstrapFailure::Message(format!("port {port} is unavailable: {error}")))
}

fn write_http_config(
    document: &mut DocumentMut,
    port: u16,
    bearer_token: &str,
) -> Result<(), BootstrapFailure> {
    set_bool(document, &["http"], "enabled", true)
        .map_err(|error| BootstrapFailure::Message(error.to_string()))?;
    set_integer(document, &["http"], "port", i64::from(port))
        .map_err(|error| BootstrapFailure::Message(error.to_string()))?;
    set_string(document, &["http"], "bearer_token", bearer_token)
        .map_err(|error| BootstrapFailure::Message(error.to_string()))
}

fn store_bearer_token(auth_store: &AuthStore, bearer_token: &str) -> Result<(), BootstrapFailure> {
    auth_store
        .store_provider_token(HTTP_BEARER_PROVIDER, bearer_token)
        .map_err(BootstrapFailure::Message)
}

fn save_document(config_path: &Path, document: &DocumentMut) -> Result<(), BootstrapFailure> {
    fs::write(config_path, document.to_string()).map_err(|error| {
        BootstrapFailure::Message(format!(
            "failed to write {}: {error}",
            config_path.display()
        ))
    })?;
    restrict_config_permissions(config_path);
    Ok(())
}

#[cfg(unix)]
fn restrict_config_permissions(config_path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    if let Err(error) = fs::set_permissions(config_path, permissions) {
        tracing::warn!(
            "could not restrict permissions on {}: {error}",
            config_path.display()
        );
    }
}

#[cfg(not(unix))]
fn restrict_config_permissions(_config_path: &Path) {
    // File permissions are not applicable on non-Unix platforms
}

fn build_output(
    bearer_token: String,
    port: u16,
    data_dir: &Path,
    config_path: &Path,
    created: bool,
) -> BootstrapOutput {
    BootstrapOutput {
        port,
        host: DEFAULT_HOST.to_string(),
        bearer_token,
        data_dir: data_dir.display().to_string(),
        config_path: config_path.display().to_string(),
        created,
    }
}

fn print_success(output: &BootstrapOutput, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(output)?);
        return Ok(());
    }
    print_human_success(output);
    Ok(())
}

fn print_human_success(output: &BootstrapOutput) {
    let token_status = if output.created { "generated" } else { "found" };
    let config_status = if output.created { "written" } else { "ready" };
    println!("✓ Data directory: {}", output.data_dir);
    println!("✓ Bearer token {token_status} (encrypted)");
    println!("✓ Port selected: {}", output.port);
    println!("✓ Config {config_status}: {}", output.config_path);
}

fn print_failure(error: &BootstrapFailure, json: bool) -> anyhow::Result<()> {
    if json {
        let payload = BootstrapError {
            error: error.message(),
            port_range: error.port_range(),
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    eprintln!("Error: {}", error.message());
    Ok(())
}

fn normalized_token(token: &str) -> Option<String> {
    let trimmed = token.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn normalized_port(port: i64) -> Option<u16> {
    u16::try_from(port).ok().filter(|port| *port > 0)
}

fn find_available_port(start: u16, end: u16) -> Option<u16> {
    (start..=end).find(|&port| std::net::TcpListener::bind((DEFAULT_HOST, port)).is_ok())
}

fn existing_http_config(doc: &DocumentMut) -> (Option<String>, Option<i64>) {
    let http = doc.get("http");
    let token = http
        .and_then(|section| section.get("bearer_token"))
        .and_then(|value| value.as_str())
        .map(String::from);
    let port = http
        .and_then(|section| section.get("port"))
        .and_then(|value| value.as_integer());
    (token, port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use tempfile::TempDir;

    #[test]
    fn find_available_port_returns_first_free() {
        let (start, listeners) = bind_contiguous_range(1);
        drop(listeners);

        let port = find_available_port(start, start);

        assert_eq!(port, Some(start));
    }

    #[test]
    fn find_available_port_returns_none_when_exhausted() {
        let (start, _listeners) = bind_contiguous_range(3);

        let port = find_available_port(start, start + 2);

        assert_eq!(port, None);
    }

    #[test]
    fn bootstrap_creates_config_on_fresh_directory() {
        let temp_dir = TempDir::new().expect("temp dir");
        let port = free_port();

        let output = bootstrap(temp_dir.path(), Some(port)).expect("bootstrap should succeed");
        let document = load_test_config(temp_dir.path());
        let (token, configured_port) = existing_http_config(&document);

        assert!(temp_dir.path().join("config.toml").exists());
        assert!(temp_dir.path().join("auth.db").exists());
        assert!(temp_dir.path().join(".auth-salt").exists());
        assert!(token.as_deref().and_then(normalized_token).is_some());
        assert_eq!(configured_port.and_then(normalized_port), Some(port));
        assert!(output.created);
    }

    #[test]
    fn bootstrap_is_idempotent() {
        let temp_dir = TempDir::new().expect("temp dir");
        let first_port = free_port();
        let second_port = free_port();

        let first = bootstrap(temp_dir.path(), Some(first_port)).expect("first bootstrap");
        let second = bootstrap(temp_dir.path(), Some(second_port)).expect("second bootstrap");

        assert!(first.created);
        assert!(!second.created);
        assert_eq!(second.port, first.port);
        assert_eq!(second.bearer_token, first.bearer_token);
    }

    #[test]
    fn bootstrap_preserves_existing_config() {
        let temp_dir = TempDir::new().expect("temp dir");
        let port = free_port();
        write_config(
            temp_dir.path(),
            "[model]\ndefault_model = \"gpt-4o-mini\"\n\n[http]\nbearer_token = \"preserve-me\"\n",
        );

        let output = bootstrap(temp_dir.path(), Some(port)).expect("bootstrap should succeed");
        let document = load_test_config(temp_dir.path());

        assert!(output.created);
        assert_eq!(
            document["model"]["default_model"].as_str(),
            Some("gpt-4o-mini")
        );
        assert_eq!(
            document["http"]["bearer_token"].as_str(),
            Some("preserve-me")
        );
        assert_eq!(document["http"]["port"].as_integer(), Some(i64::from(port)));
    }

    #[test]
    fn find_available_port_skips_occupied_ports() {
        let (range_start, _listeners) = bind_contiguous_range(3);
        // range_start through range_start+2 are occupied; search range_start..range_start+5
        let result = find_available_port(range_start, range_start + 5);
        assert!(result.is_some(), "should find a port after occupied range");
        assert!(
            result.unwrap() >= range_start + 3,
            "should skip the 3 occupied ports"
        );
    }

    #[test]
    fn find_available_port_returns_none_when_range_fully_occupied() {
        let (range_start, _listeners) = bind_contiguous_range(5);
        let range_end = range_start + 4;
        let result = find_available_port(range_start, range_end);
        assert!(
            result.is_none(),
            "should return None when all ports in range are occupied"
        );
    }

    #[test]
    fn bootstrap_json_output_is_valid() {
        let output = BootstrapOutput {
            port: 8400,
            host: DEFAULT_HOST.to_string(),
            bearer_token: "secret-token".to_string(),
            data_dir: "/tmp/.fawx".to_string(),
            config_path: "/tmp/.fawx/config.toml".to_string(),
            created: true,
        };

        let json = serde_json::to_string(&output).expect("serialize output");
        let parsed: BootstrapOutput = serde_json::from_str(&json).expect("parse output json");

        assert_eq!(parsed, output);
    }

    fn free_port() -> u16 {
        TcpListener::bind((DEFAULT_HOST, 0))
            .expect("bind free port")
            .local_addr()
            .expect("local addr")
            .port()
    }

    fn load_test_config(data_dir: &Path) -> DocumentMut {
        let config_path = data_dir.join("config.toml");
        load_config_document(&config_path).expect("load config")
    }

    fn write_config(data_dir: &Path, content: &str) {
        fs::create_dir_all(data_dir).expect("create data dir");
        fs::write(data_dir.join("config.toml"), content).expect("write config");
    }

    #[test]
    fn bootstrap_fails_when_override_port_occupied() {
        let temp_dir = TempDir::new().expect("temp dir");
        let (port, _listener) = bind_contiguous_range(1);
        let error = bootstrap(temp_dir.path(), Some(port))
            .expect_err("bootstrap should fail with occupied port");
        assert!(
            error.message().contains("unavailable"),
            "error should mention port unavailability: {}",
            error.message()
        );
    }

    #[test]
    fn port_range_exhaustion_error_has_correct_fields() {
        let failure = BootstrapFailure::PortRangeExhausted {
            start: 8400,
            end: 8410,
        };
        assert_eq!(failure.port_range(), Some([8400, 8410]));
        assert_eq!(failure.message(), "All ports 8400-8410 are in use");
    }

    #[cfg(unix)]
    #[test]
    fn bootstrap_sets_config_permissions_to_0600() {
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = TempDir::new().expect("temp dir");
        let port = free_port();
        bootstrap(temp_dir.path(), Some(port)).expect("bootstrap should succeed");
        let metadata = fs::metadata(temp_dir.path().join("config.toml")).expect("config metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config.toml should be owner-only read/write");
    }

    fn bind_contiguous_range(size: u16) -> (u16, Vec<TcpListener>) {
        for start in 30_000..60_000 {
            if let Some(listeners) = try_bind_range(start, start + size - 1) {
                return (start, listeners);
            }
        }
        panic!("no contiguous free port range available for test");
    }

    fn try_bind_range(start: u16, end: u16) -> Option<Vec<TcpListener>> {
        let mut listeners = Vec::new();
        for port in start..=end {
            match TcpListener::bind((DEFAULT_HOST, port)) {
                Ok(listener) => listeners.push(listener),
                Err(_) => return None,
            }
        }
        Some(listeners)
    }
}
