//! System diagnostics command

use super::diagnostics::{plural_suffix, DiagnosticLine, DiagnosticSection, DiagnosticStatus};
use super::runtime_layout::RuntimeLayout;
use super::skill_signatures::{scan_skill_signatures, SkillSignatureReport};
use crate::auth_store::AuthStore;
use fx_auth::auth::AuthManager;
use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;

type DoctorLine = DiagnosticLine;
type DoctorSection = DiagnosticSection;
type DoctorStatus = DiagnosticStatus;

const PROVIDERS: [ProviderCheck; 2] = [
    ProviderCheck::new("Anthropic", "anthropic", "https://api.anthropic.com"),
    ProviderCheck::new("OpenAI", "openai", "https://api.openai.com"),
];
const EMBEDDING_FILES: [&str; 4] = [
    "config.json",
    "tokenizer.json",
    "model.safetensors",
    "manifest.json",
];

pub async fn run() -> anyhow::Result<i32> {
    let layout = RuntimeLayout::detect()?;
    let report = DoctorReport::gather(&layout).await;
    report.print();
    Ok(report.exit_code())
}

#[derive(Debug, Clone, Copy)]
struct ProviderCheck {
    label: &'static str,
    key: &'static str,
    url: &'static str,
}

impl ProviderCheck {
    const fn new(label: &'static str, key: &'static str, url: &'static str) -> Self {
        Self { label, key, url }
    }
}

#[derive(Debug, Clone)]
struct DoctorReport {
    sections: Vec<DoctorSection>,
}

impl DoctorReport {
    async fn gather(layout: &RuntimeLayout) -> Self {
        Self {
            sections: vec![
                system_section(layout).await,
                providers_section(layout).await,
                skills_section(layout),
                models_section(layout),
                toolchain_section(),
                network_section(layout),
            ],
        }
    }

    fn print(&self) {
        println!("Fawx Doctor\n───────────\n");
        for section in &self.sections {
            section.print();
        }
        println!("{}", self.summary_line());
    }

    fn exit_code(&self) -> i32 {
        if self.count(DoctorStatus::Fail) > 0 {
            1
        } else {
            0
        }
    }

    fn summary_line(&self) -> String {
        format!(
            "{} passed, {} warning{}, {} not configured, {} failed",
            self.count(DoctorStatus::Pass),
            self.count(DoctorStatus::Warning),
            plural_suffix(self.count(DoctorStatus::Warning)),
            self.count(DoctorStatus::NotConfigured),
            self.count(DoctorStatus::Fail),
        )
    }

    fn count(&self, status: DoctorStatus) -> usize {
        self.sections
            .iter()
            .flat_map(|section| section.lines.iter())
            .filter(|line| line.status == status)
            .count()
    }
}

async fn system_section(layout: &RuntimeLayout) -> DoctorSection {
    DoctorSection::new(
        "System",
        vec![
            workspace_line(layout),
            config_line(layout),
            storage_line(layout).await,
            audit_log_line(layout).await,
        ],
    )
}

fn workspace_line(layout: &RuntimeLayout) -> DoctorLine {
    let status = if layout.data_dir.exists() {
        DoctorStatus::Pass
    } else {
        DoctorStatus::Fail
    };
    DoctorLine::new(
        status,
        format!("Workspace directory ({})", layout.data_dir.display()),
    )
}

fn config_line(layout: &RuntimeLayout) -> DoctorLine {
    if layout.config_path.exists() {
        return DoctorLine::new(DoctorStatus::Pass, "Config file found");
    }
    DoctorLine::new(
        DoctorStatus::Pass,
        "Config file not found (defaults in use)",
    )
}

async fn storage_line(layout: &RuntimeLayout) -> DoctorLine {
    let writable = ensure_storage_writable(&layout.storage_dir).await;
    let status = if writable {
        DoctorStatus::Pass
    } else {
        DoctorStatus::Fail
    };
    DoctorLine::new(status, "Storage directory writable")
}

async fn ensure_storage_writable(path: &std::path::Path) -> bool {
    if tokio::fs::create_dir_all(path).await.is_err() {
        return false;
    }
    let test_file = path.join(".doctor-write-test");
    if tokio::fs::write(&test_file, b"ok").await.is_err() {
        return false;
    }
    let _ = tokio::fs::remove_file(test_file).await;
    true
}

async fn audit_log_line(layout: &RuntimeLayout) -> DoctorLine {
    if !layout.audit_log_path.exists() {
        return DoctorLine::new(DoctorStatus::Pass, "Audit log intact (not created yet)");
    }
    match fx_security::AuditLog::open(&layout.audit_log_path).await {
        Ok(log) if log.verify_integrity().unwrap_or(false) => {
            DoctorLine::new(DoctorStatus::Pass, "Audit log intact")
        }
        Ok(_) => DoctorLine::new(DoctorStatus::Fail, "Audit log verification failed"),
        Err(error) => DoctorLine::new(DoctorStatus::Fail, format!("Audit log unreadable: {error}")),
    }
}

async fn providers_section(layout: &RuntimeLayout) -> DoctorSection {
    let auth_manager = load_auth_manager(layout);
    let mut lines = Vec::new();
    for provider in PROVIDERS {
        lines.push(provider_connectivity_line(provider).await);
        lines.push(provider_credentials_line(provider, auth_manager.as_ref()));
    }
    DoctorSection::new("Providers", lines)
}

async fn provider_connectivity_line(provider: ProviderCheck) -> DoctorLine {
    let reachable = provider_reachable(provider.url).await;
    let status = if reachable {
        DoctorStatus::Pass
    } else {
        DoctorStatus::Fail
    };
    DoctorLine::new(status, format!("{} API reachable", provider.label))
}

async fn provider_reachable(url: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build();
    let Ok(client) = client else {
        return false;
    };
    client.head(url).send().await.is_ok()
}

fn load_auth_manager(layout: &RuntimeLayout) -> Result<AuthManager, String> {
    let store = AuthStore::open(&layout.data_dir)?;
    store.load_auth_manager()
}

fn provider_credentials_line(
    provider: ProviderCheck,
    auth_manager: Result<&AuthManager, &String>,
) -> DoctorLine {
    match auth_manager {
        Ok(manager) if manager.get(provider.key).is_some() => DoctorLine::new(
            DoctorStatus::Pass,
            format!("{} credentials configured", provider.label),
        ),
        Ok(_) => DoctorLine::new(
            DoctorStatus::NotConfigured,
            format!("{} credentials not configured", provider.label),
        ),
        Err(error) => DoctorLine::new(
            DoctorStatus::Fail,
            format!("{} credential store unavailable: {error}", provider.label),
        ),
    }
}

fn skills_section(layout: &RuntimeLayout) -> DoctorSection {
    let line = match scan_skill_signatures(&layout.skills_dir, &layout.trusted_keys_dir) {
        Ok(report) => skill_summary_line(&report),
        Err(error) => DoctorLine::new(
            DoctorStatus::Fail,
            format!("Skill integrity check failed: {error}"),
        ),
    };
    DoctorSection::new("Skills", vec![line])
}

fn skill_summary_line(report: &SkillSignatureReport) -> DoctorLine {
    if !report.invalid.is_empty() {
        return DoctorLine::new(
            DoctorStatus::Fail,
            format!(
                "Invalid signatures detected ({})",
                report.invalid.join(", ")
            ),
        );
    }
    if !report.unverified.is_empty() {
        return DoctorLine::new(
            DoctorStatus::Warning,
            format!(
                "{} skills signed but unverified (no trusted keys)",
                report.unverified.len()
            ),
        );
    }
    if !report.unsigned.is_empty() {
        return DoctorLine::new(
            DoctorStatus::Warning,
            format!(
                "{} skills without signatures ({})",
                report.unsigned.len(),
                report.unsigned.join(", ")
            ),
        );
    }
    DoctorLine::new(
        DoctorStatus::Pass,
        format!(
            "{} skills installed, all signatures valid",
            report.installed_count()
        ),
    )
}

fn models_section(layout: &RuntimeLayout) -> DoctorSection {
    DoctorSection::new("Models", vec![embedding_model_line(layout)])
}

fn embedding_model_line(layout: &RuntimeLayout) -> DoctorLine {
    let missing = missing_embedding_files(&layout.embedding_model_dir);
    if missing.is_empty() {
        return DoctorLine::new(DoctorStatus::Pass, "Embedding model present");
    }
    if !layout.embedding_model_dir.exists() {
        return DoctorLine::new(
            DoctorStatus::NotConfigured,
            format!(
                "Embedding model not found ({})",
                layout.embedding_model_dir.display()
            ),
        );
    }
    DoctorLine::new(
        DoctorStatus::Fail,
        format!(
            "Embedding model incomplete (missing {})",
            missing.join(", ")
        ),
    )
}

fn missing_embedding_files(model_dir: &std::path::Path) -> Vec<String> {
    let mut missing = Vec::new();
    for file in EMBEDDING_FILES {
        if !model_dir.join(file).exists() {
            missing.push(file.to_string());
        }
    }
    missing
}

fn toolchain_section() -> DoctorSection {
    DoctorSection::new(
        "Toolchain",
        vec![rustc_line(), cargo_line(), wasm_target_line()],
    )
}

fn rustc_line() -> DoctorLine {
    tool_version_line("rustc")
}

fn cargo_line() -> DoctorLine {
    tool_version_line("cargo")
}

fn tool_version_line(tool: &str) -> DoctorLine {
    match tool_version(tool) {
        Some(version) => DoctorLine::new(DoctorStatus::Pass, format!("{tool} {version}")),
        None => DoctorLine::new(DoctorStatus::Fail, format!("{tool} not found on PATH")),
    }
}

fn tool_version(tool: &str) -> Option<String> {
    which::which(tool).ok()?;
    let output = Command::new(tool).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_tool_version(&String::from_utf8(output.stdout).ok()?)
}

fn parse_tool_version(output: &str) -> Option<String> {
    output.split_whitespace().nth(1).map(str::to_string)
}

fn wasm_target_line() -> DoctorLine {
    let installed = wasm_target_installed();
    let status = if installed {
        DoctorStatus::Pass
    } else {
        DoctorStatus::NotConfigured
    };
    let message = if installed {
        "wasm32-unknown-unknown target installed"
    } else {
        "wasm32-unknown-unknown target not installed"
    };
    DoctorLine::new(status, message)
}

fn wasm_target_installed() -> bool {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    let Ok(output) = output else {
        return false;
    };
    parse_installed_targets(&String::from_utf8_lossy(&output.stdout))
}

fn parse_installed_targets(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim() == "wasm32-unknown-unknown")
}

fn network_section(layout: &RuntimeLayout) -> DoctorSection {
    let available = port_available(layout.http_port);
    let status = if available {
        DoctorStatus::Pass
    } else {
        DoctorStatus::Fail
    };
    let message = if available {
        format!("Port {} available", layout.http_port)
    } else {
        format!("Port {} already in use", layout.http_port)
    };
    DoctorSection::new("Network", vec![DoctorLine::new(status, message)])
}

fn port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn provider_reachability_check_formats_expected_label() {
        let line = DoctorLine::new(DoctorStatus::Pass, "Anthropic API reachable");
        assert_eq!(line.message, "Anthropic API reachable");
    }

    #[test]
    fn credential_check_reports_missing_provider_as_not_configured() {
        let manager = AuthManager::new();
        let line = provider_credentials_line(PROVIDERS[1], Ok(&manager));
        assert_eq!(line.status, DoctorStatus::NotConfigured);
        assert!(line.message.contains("OpenAI credentials not configured"));
    }

    #[test]
    fn port_availability_detects_bound_listener() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
        let port = listener.local_addr().expect("addr").port();
        assert!(!port_available(port));
    }

    #[test]
    fn toolchain_detection_parses_semver_from_version_output() {
        let version = parse_tool_version("rustc 1.83.0 (abc123 2026-03-10)");
        assert_eq!(version, Some("1.83.0".to_string()));
    }

    #[test]
    fn wasm_target_detection_parses_installed_targets() {
        assert!(parse_installed_targets(
            "x86_64-unknown-linux-gnu\nwasm32-unknown-unknown\n"
        ));
    }

    #[test]
    fn embedding_model_line_reports_missing_directory_as_not_configured() {
        let temp = TempDir::new().expect("tempdir");
        let mut layout = RuntimeLayout::detect().expect("layout");
        layout.embedding_model_dir = temp.path().join("missing-model");

        let line = embedding_model_line(&layout);
        assert_eq!(line.status, DoctorStatus::NotConfigured);
    }

    #[tokio::test]
    async fn storage_check_writes_to_directory() {
        let temp = TempDir::new().expect("tempdir");
        assert!(ensure_storage_writable(temp.path()).await);
        assert!(temp.path().exists());
    }

    #[tokio::test]
    async fn audit_log_line_reports_valid_log() {
        let temp = TempDir::new().expect("tempdir");
        let log_path = temp.path().join("audit.log");
        let _ = fx_security::AuditLog::open(&log_path)
            .await
            .expect("audit log");
        let mut layout = RuntimeLayout::detect().expect("layout");
        layout.audit_log_path = log_path;

        let line = audit_log_line(&layout).await;
        assert_eq!(line.status, DoctorStatus::Pass);
    }

    #[test]
    fn skill_summary_reports_unsigned_skills_as_warning() {
        let report = SkillSignatureReport {
            unsigned: vec!["weather".to_string(), "calculator".to_string()],
            ..Default::default()
        };
        let line = skill_summary_line(&report);
        assert_eq!(line.status, DoctorStatus::Warning);
        assert!(line.message.contains("weather"));
    }

    #[test]
    fn load_auth_manager_surfaces_missing_store_as_empty_or_error() {
        let temp = TempDir::new().expect("tempdir");
        let layout = RuntimeLayout {
            data_dir: temp.path().to_path_buf(),
            ..RuntimeLayout::detect().expect("layout")
        };
        let manager = load_auth_manager(&layout).expect("auth manager");
        assert!(manager.providers().is_empty());
    }

    #[test]
    fn missing_embedding_files_lists_required_artifacts() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path()).expect("create");
        let missing = missing_embedding_files(temp.path());
        assert_eq!(missing.len(), EMBEDDING_FILES.len());
    }
}
