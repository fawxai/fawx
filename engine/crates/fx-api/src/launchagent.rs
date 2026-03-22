use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use std::{fs, process::Command};

const LABEL: &str = "ai.fawx.server";

#[derive(Debug, Clone)]
pub struct LaunchAgentConfig {
    pub server_binary_path: PathBuf,
    pub port: u16,
    pub data_dir: PathBuf,
    pub log_path: PathBuf,
    pub auto_start: bool,
    pub keep_alive: bool,
}

impl Default for LaunchAgentConfig {
    fn default() -> Self {
        Self {
            server_binary_path: PathBuf::new(),
            port: 0,
            data_dir: PathBuf::new(),
            log_path: PathBuf::new(),
            auto_start: false,
            keep_alive: true,
        }
    }
}

#[derive(Debug)]
pub enum LaunchAgentError {
    HomeDirNotFound,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    UidParseFailed,
    PlistWriteFailed(io::Error),
    LaunchctlSpawnFailed(io::Error),
    LaunchctlFailed {
        command: String,
        stderr: String,
        exit_code: Option<i32>,
    },
    NotInstalled,
    NotSupported,
}

impl fmt::Display for LaunchAgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeDirNotFound => write!(f, "HOME directory not found"),
            Self::UidParseFailed => write!(f, "failed to parse current user ID"),
            Self::PlistWriteFailed(e) => write!(f, "failed to write plist: {e}"),
            Self::LaunchctlSpawnFailed(e) => write!(f, "failed to spawn launchctl: {e}"),
            Self::LaunchctlFailed {
                command,
                stderr,
                exit_code,
            } => match exit_code {
                Some(code) => {
                    write!(f, "launchctl {command} failed (exit {code}): {stderr}")
                }
                None => write!(f, "launchctl {command} failed (exit unknown): {stderr}"),
            },
            Self::NotInstalled => write!(f, "LaunchAgent is not installed"),
            Self::NotSupported => write!(f, "LaunchAgent is only supported on macOS"),
        }
    }
}

impl std::error::Error for LaunchAgentError {}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LaunchAgentStatus {
    pub installed: bool,
    pub loaded: bool,
}

pub fn plist_path_with_home(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents/ai.fawx.server.plist")
}

pub fn plist_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(plist_path_with_home(Path::new(&home)))
}

fn plist_bool(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn generate_plist(config: &LaunchAgentConfig) -> String {
    // Keep this LaunchAgent template in sync with
    // app/Fawx/Services/LocalBootstrapService.swift::generatePlist.
    // Swift duplicates it during first-launch bootstrap before this API is available.
    let binary = xml_escape(&config.server_binary_path.display().to_string());
    let port = config.port;
    let data_dir = xml_escape(&config.data_dir.display().to_string());
    let log = xml_escape(&config.log_path.display().to_string());
    let run_at_load = plist_bool(config.auto_start);
    let keep_alive = plist_bool(config.keep_alive);

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>serve</string>
        <string>--http</string>
        <string>--port</string>
        <string>{port}</string>
        <string>--data-dir</string>
        <string>{data_dir}</string>
    </array>
    <key>RunAtLoad</key>
    <{run_at_load}/>
    <key>KeepAlive</key>
    <{keep_alive}/>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#
    )
}

#[cfg(target_os = "macos")]
pub fn install(config: &LaunchAgentConfig) -> Result<(), LaunchAgentError> {
    let path = plist_path().ok_or(LaunchAgentError::HomeDirNotFound)?;
    let domain = launchctl_domain()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(LaunchAgentError::PlistWriteFailed)?;
    }
    let content = generate_plist(config);
    fs::write(&path, content).map_err(LaunchAgentError::PlistWriteFailed)?;
    run_launchctl("bootstrap", &domain, &path)
}

#[cfg(target_os = "macos")]
pub fn stop_service() -> Result<(), LaunchAgentError> {
    let path = installed_plist_path()?;
    let domain = launchctl_domain()?;
    run_launchctl("bootout", &domain, &path)
}

#[cfg(target_os = "macos")]
pub fn start_service() -> Result<(), LaunchAgentError> {
    let path = installed_plist_path()?;
    let domain = launchctl_domain()?;
    run_launchctl("bootstrap", &domain, &path)
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<(), LaunchAgentError> {
    let path = installed_plist_path()?;
    if let Err(error) = stop_service() {
        tracing::warn!("bootout failed (may be expected): {error}");
    }
    fs::remove_file(&path).map_err(LaunchAgentError::PlistWriteFailed)
}

#[cfg(target_os = "macos")]
pub fn reload(config: &LaunchAgentConfig) -> Result<(), LaunchAgentError> {
    let path = plist_path().ok_or(LaunchAgentError::HomeDirNotFound)?;
    let domain = launchctl_domain()?;
    if let Err(error) = run_launchctl("bootout", &domain, &path) {
        tracing::warn!("bootout failed (may be expected): {error}");
    }
    let content = generate_plist(config);
    fs::write(&path, content).map_err(LaunchAgentError::PlistWriteFailed)?;
    run_launchctl("bootstrap", &domain, &path)
}

#[cfg(target_os = "macos")]
pub fn status() -> LaunchAgentStatus {
    let installed = plist_path().is_some_and(|path| path.exists());
    LaunchAgentStatus {
        installed,
        loaded: installed && is_loaded(),
    }
}

#[cfg(target_os = "macos")]
fn is_loaded() -> bool {
    Command::new("launchctl")
        .args(["list", LABEL])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn parse_current_uid(stdout: &[u8]) -> Result<u32, LaunchAgentError> {
    String::from_utf8_lossy(stdout)
        .trim()
        .parse()
        .map_err(|_| LaunchAgentError::UidParseFailed)
}

#[cfg(target_os = "macos")]
fn installed_plist_path() -> Result<PathBuf, LaunchAgentError> {
    let path = plist_path().ok_or(LaunchAgentError::HomeDirNotFound)?;
    if path.exists() {
        Ok(path)
    } else {
        Err(LaunchAgentError::NotInstalled)
    }
}

#[cfg(target_os = "macos")]
fn launchctl_domain() -> Result<String, LaunchAgentError> {
    Ok(format!("gui/{}", current_uid()?))
}

#[cfg(target_os = "macos")]
fn current_uid() -> Result<u32, LaunchAgentError> {
    let output = Command::new("id")
        .args(["-u"])
        .output()
        .map_err(LaunchAgentError::LaunchctlSpawnFailed)?;
    parse_current_uid(&output.stdout)
}

#[cfg(target_os = "macos")]
fn run_launchctl(subcommand: &str, domain: &str, path: &Path) -> Result<(), LaunchAgentError> {
    let plist_path = path.display().to_string();
    let output = Command::new("launchctl")
        .args([subcommand, domain, &plist_path])
        .output()
        .map_err(LaunchAgentError::LaunchctlSpawnFailed)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(LaunchAgentError::LaunchctlFailed {
            command: subcommand.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
        })
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install(_config: &LaunchAgentConfig) -> Result<(), LaunchAgentError> {
    Err(LaunchAgentError::NotSupported)
}

#[cfg(not(target_os = "macos"))]
pub fn stop_service() -> Result<(), LaunchAgentError> {
    Err(LaunchAgentError::NotSupported)
}

#[cfg(not(target_os = "macos"))]
pub fn start_service() -> Result<(), LaunchAgentError> {
    Err(LaunchAgentError::NotSupported)
}

#[cfg(not(target_os = "macos"))]
pub fn uninstall() -> Result<(), LaunchAgentError> {
    Err(LaunchAgentError::NotSupported)
}

#[cfg(not(target_os = "macos"))]
pub fn reload(_config: &LaunchAgentConfig) -> Result<(), LaunchAgentError> {
    Err(LaunchAgentError::NotSupported)
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> LaunchAgentStatus {
    LaunchAgentStatus {
        installed: false,
        loaded: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_plist_contains_label() {
        let config = test_config();
        let plist = generate_plist(&config);
        assert!(plist.contains("<string>ai.fawx.server</string>"));
    }

    #[test]
    fn generate_plist_contains_port() {
        let config = test_config();
        let plist = generate_plist(&config);
        assert!(plist.contains("<string>8400</string>"));
    }

    #[test]
    fn generate_plist_contains_binary_path() {
        let config = test_config();
        let plist = generate_plist(&config);
        assert!(plist.contains("<string>/usr/local/bin/fawx</string>"));
    }

    #[test]
    fn generate_plist_contains_http_flag() {
        let config = test_config();
        let plist = generate_plist(&config);
        assert!(
            plist.contains("<string>--http</string>"),
            "plist must include --http so the server starts in HTTP mode"
        );
    }

    #[test]
    fn generate_plist_escapes_xml_sensitive_paths() {
        let config = LaunchAgentConfig {
            server_binary_path: PathBuf::from("/Applications/Fawx & Friends/fawx<beta>"),
            data_dir: PathBuf::from("/Users/testuser/.fawx<&>"),
            log_path: PathBuf::from("/Users/testuser/Logs/fawx>&.log"),
            ..test_config()
        };

        let plist = generate_plist(&config);

        assert!(plist.contains("/Applications/Fawx &amp; Friends/fawx&lt;beta&gt;"));
        assert!(plist.contains("/Users/testuser/.fawx&lt;&amp;&gt;"));
        assert!(plist.contains("/Users/testuser/Logs/fawx&gt;&amp;.log"));
    }

    #[test]
    fn generate_plist_run_at_load_true() {
        let mut config = test_config();
        config.auto_start = true;
        let plist = generate_plist(&config);
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
    }

    #[test]
    fn generate_plist_run_at_load_false() {
        let mut config = test_config();
        config.auto_start = false;
        let plist = generate_plist(&config);
        assert!(plist.contains("<key>RunAtLoad</key>\n    <false/>"));
    }

    #[test]
    fn generate_plist_keep_alive_false() {
        let mut config = test_config();
        config.keep_alive = false;
        let plist = generate_plist(&config);
        assert!(plist.contains("<key>KeepAlive</key>\n    <false/>"));
    }

    #[test]
    fn generate_plist_has_balanced_tags() {
        let config = test_config();
        let plist = generate_plist(&config);
        assert!(xml_tags_are_balanced(&plist));
    }

    #[test]
    fn plist_path_with_home_builds_launchagent_path() {
        let path = plist_path_with_home(Path::new("/Users/testuser"));
        assert_eq!(
            path,
            PathBuf::from("/Users/testuser/Library/LaunchAgents/ai.fawx.server.plist")
        );
    }

    #[test]
    fn error_display_not_supported() {
        let err = LaunchAgentError::NotSupported;
        assert_eq!(err.to_string(), "LaunchAgent is only supported on macOS");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn stop_service_returns_not_supported_on_non_macos() {
        assert!(matches!(
            stop_service(),
            Err(LaunchAgentError::NotSupported)
        ));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn start_service_returns_not_supported_on_non_macos() {
        assert!(matches!(
            start_service(),
            Err(LaunchAgentError::NotSupported)
        ));
    }

    #[test]
    fn error_display_uid_parse_failed() {
        let err = LaunchAgentError::UidParseFailed;
        assert_eq!(err.to_string(), "failed to parse current user ID");
    }

    #[test]
    fn error_display_launchctl_spawn_failed() {
        let err = LaunchAgentError::LaunchctlSpawnFailed(io::Error::from(io::ErrorKind::Other));
        assert_eq!(err.to_string(), "failed to spawn launchctl: other error");
    }

    #[test]
    fn error_display_launchctl_failed_without_exit_code() {
        let err = LaunchAgentError::LaunchctlFailed {
            command: "bootstrap".to_string(),
            stderr: "signal lost".to_string(),
            exit_code: None,
        };
        assert_eq!(
            err.to_string(),
            "launchctl bootstrap failed (exit unknown): signal lost"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_current_uid_parses_valid_uid() {
        assert_eq!(parse_current_uid(b"501\n").unwrap(), 501);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_current_uid_returns_uid_parse_failed_for_invalid_uid() {
        let err = parse_current_uid(b"not-a-uid").unwrap_err();
        assert!(matches!(err, LaunchAgentError::UidParseFailed));
    }

    fn xml_tags_are_balanced(xml: &str) -> bool {
        let mut stack = Vec::new();
        let mut remaining = xml;

        while let Some(start) = remaining.find('<') {
            remaining = &remaining[start + 1..];
            let Some(end) = remaining.find('>') else {
                return false;
            };
            let tag = &remaining[..end];
            remaining = &remaining[end + 1..];

            if tag.starts_with('?') || tag.starts_with('!') || tag.ends_with('/') {
                continue;
            }

            let name = tag
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or_default();
            if name.is_empty() {
                return false;
            }

            if tag.starts_with('/') {
                if stack.pop().as_deref() != Some(name) {
                    return false;
                }
            } else {
                stack.push(name.to_string());
            }
        }

        stack.is_empty()
    }

    fn test_config() -> LaunchAgentConfig {
        LaunchAgentConfig {
            server_binary_path: PathBuf::from("/usr/local/bin/fawx"),
            port: 8400,
            data_dir: PathBuf::from("/Users/testuser/.fawx"),
            log_path: PathBuf::from("/Users/testuser/Library/Logs/Fawx/server.log"),
            auto_start: true,
            keep_alive: true,
        }
    }
}
