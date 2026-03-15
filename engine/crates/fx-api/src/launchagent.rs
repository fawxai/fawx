use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const LABEL: &str = "ai.fawx.server";

#[derive(Debug, Clone)]
pub struct LaunchAgentConfig {
    pub server_binary_path: PathBuf,
    pub port: u16,
    pub data_dir: PathBuf,
    pub log_path: PathBuf,
    pub auto_start: bool,
}

#[derive(Debug)]
pub enum LaunchAgentError {
    HomeDirNotFound,
    PlistWriteFailed(io::Error),
    LaunchctlFailed {
        command: String,
        stderr: String,
        exit_code: i32,
    },
    NotInstalled,
    NotSupported,
}

impl fmt::Display for LaunchAgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeDirNotFound => write!(f, "HOME directory not found"),
            Self::PlistWriteFailed(e) => write!(f, "failed to write plist: {e}"),
            Self::LaunchctlFailed {
                command,
                stderr,
                exit_code,
            } => {
                write!(f, "launchctl {command} failed (exit {exit_code}): {stderr}")
            }
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
    pub auto_start_enabled: bool,
    pub pid: Option<u32>,
}

pub fn plist_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library/LaunchAgents/ai.fawx.server.plist"))
}

pub fn generate_plist(config: &LaunchAgentConfig) -> String {
    let binary = config.server_binary_path.display();
    let port = config.port;
    let data_dir = config.data_dir.display();
    let log = config.log_path.display();
    let run_at_load = if config.auto_start { "true" } else { "false" };

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
        <string>--port</string>
        <string>{port}</string>
        <string>--data-dir</string>
        <string>{data_dir}</string>
    </array>
    <key>RunAtLoad</key>
    <{run_at_load}/>
    <key>KeepAlive</key>
    <true/>
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(LaunchAgentError::PlistWriteFailed)?;
    }
    let content = generate_plist(config);
    fs::write(&path, content).map_err(LaunchAgentError::PlistWriteFailed)?;
    run_launchctl("bootstrap", &format!("gui/{}", current_uid()), &path)
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<(), LaunchAgentError> {
    let path = plist_path().ok_or(LaunchAgentError::HomeDirNotFound)?;
    if !path.exists() {
        return Err(LaunchAgentError::NotInstalled);
    }
    let _ = run_launchctl("bootout", &format!("gui/{}", current_uid()), &path);
    fs::remove_file(&path).map_err(LaunchAgentError::PlistWriteFailed)
}

#[cfg(target_os = "macos")]
pub fn reload(config: &LaunchAgentConfig) -> Result<(), LaunchAgentError> {
    let path = plist_path().ok_or(LaunchAgentError::HomeDirNotFound)?;
    let _ = run_launchctl("bootout", &format!("gui/{}", current_uid()), &path);
    let content = generate_plist(config);
    fs::write(&path, content).map_err(LaunchAgentError::PlistWriteFailed)?;
    run_launchctl("bootstrap", &format!("gui/{}", current_uid()), &path)
}

#[cfg(target_os = "macos")]
pub fn status() -> LaunchAgentStatus {
    let installed = plist_path().map_or(false, |p| p.exists());
    LaunchAgentStatus {
        installed,
        loaded: installed && is_loaded(),
        auto_start_enabled: installed,
        pid: None,
    }
}

#[cfg(target_os = "macos")]
fn is_loaded() -> bool {
    Command::new("launchctl")
        .args(["list", LABEL])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(target_os = "macos")]
fn run_launchctl(subcommand: &str, domain: &str, path: &Path) -> Result<(), LaunchAgentError> {
    let output = Command::new("launchctl")
        .args([subcommand, domain, &path.display().to_string()])
        .output()
        .map_err(|e| LaunchAgentError::PlistWriteFailed(e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(LaunchAgentError::LaunchctlFailed {
            command: subcommand.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install(_config: &LaunchAgentConfig) -> Result<(), LaunchAgentError> {
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
        auto_start_enabled: false,
        pid: None,
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
    fn generate_plist_run_at_load_true() {
        let mut config = test_config();
        config.auto_start = true;
        let plist = generate_plist(&config);
        assert!(plist.contains("<true/>"));
    }

    #[test]
    fn generate_plist_run_at_load_false() {
        let mut config = test_config();
        config.auto_start = false;
        let plist = generate_plist(&config);
        assert!(plist.contains("<false/>"));
    }

    #[test]
    fn plist_path_returns_some_when_home_set() {
        std::env::set_var("HOME", "/Users/testuser");
        let path = plist_path().expect("should return path");
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

    fn test_config() -> LaunchAgentConfig {
        LaunchAgentConfig {
            server_binary_path: PathBuf::from("/usr/local/bin/fawx"),
            port: 8400,
            data_dir: PathBuf::from("/Users/testuser/.fawx"),
            log_path: PathBuf::from("/Users/testuser/Library/Logs/Fawx/server.log"),
            auto_start: true,
        }
    }
}
