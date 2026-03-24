use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Sandbox configuration derived from capability presets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxConfig {
    /// Whether OS-level sandboxing is enabled.
    pub enabled: bool,
    /// Filesystem paths the agent can access.
    pub filesystem_allow: Vec<SandboxPath>,
    /// Network access policy.
    pub network: NetworkCapability,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            filesystem_allow: Vec::new(),
            network: NetworkCapability::Full,
        }
    }
}

/// A filesystem path with access mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxPath {
    pub path: PathBuf,
    pub mode: PathMode,
}

/// Filesystem access mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PathMode {
    ReadOnly,
    ReadWrite,
}

/// Network access policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkCapability {
    /// No network restrictions.
    Full,
    /// Only localhost connections allowed.
    LocalhostOnly,
    /// No network access.
    None,
    /// Specific hosts allowed.
    AllowList(Vec<String>),
}

/// Status of applied sandbox layers.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxStatus {
    pub landlock: bool,
    pub seccomp: bool,
    pub network: bool,
}

impl SandboxStatus {
    /// Whether any sandbox layer is active.
    pub fn any_active(&self) -> bool {
        self.landlock || self.seccomp || self.network
    }
}

/// Build a SandboxConfig from permission preset capabilities.
pub fn sandbox_config_from_preset(
    preset: &str,
    working_dir: &Path,
    data_dir: &Path,
) -> SandboxConfig {
    match preset {
        "open" | "experimental" => config_with_network(
            read_write_paths(working_dir, data_dir),
            NetworkCapability::Full,
        ),
        "standard" | "power" => config_with_network(
            read_write_paths(working_dir, data_dir),
            NetworkCapability::LocalhostOnly,
        ),
        "restricted" | "cautious" => config_with_network(
            restricted_paths(working_dir, data_dir),
            NetworkCapability::None,
        ),
        _ => SandboxConfig::default(),
    }
}

fn config_with_network(
    filesystem_allow: Vec<SandboxPath>,
    network: NetworkCapability,
) -> SandboxConfig {
    SandboxConfig {
        enabled: true,
        filesystem_allow,
        network,
    }
}

fn read_write_paths(working_dir: &Path, data_dir: &Path) -> Vec<SandboxPath> {
    vec![
        sandbox_path(working_dir, PathMode::ReadWrite),
        sandbox_path(data_dir, PathMode::ReadWrite),
        sandbox_path(Path::new("/tmp"), PathMode::ReadWrite),
        sandbox_path(Path::new("/usr"), PathMode::ReadOnly),
        sandbox_path(Path::new("/lib"), PathMode::ReadOnly),
        sandbox_path(Path::new("/etc"), PathMode::ReadOnly),
    ]
}

fn restricted_paths(working_dir: &Path, data_dir: &Path) -> Vec<SandboxPath> {
    vec![
        sandbox_path(working_dir, PathMode::ReadOnly),
        sandbox_path(data_dir, PathMode::ReadWrite),
        sandbox_path(Path::new("/tmp"), PathMode::ReadWrite),
        sandbox_path(Path::new("/usr"), PathMode::ReadOnly),
        sandbox_path(Path::new("/lib"), PathMode::ReadOnly),
        sandbox_path(Path::new("/etc"), PathMode::ReadOnly),
    ]
}

fn sandbox_path(path: &Path, mode: PathMode) -> SandboxPath {
    SandboxPath {
        path: path.to_path_buf(),
        mode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sandbox_config_is_disabled() {
        let config = SandboxConfig::default();

        assert!(!config.enabled);
        assert!(config.filesystem_allow.is_empty());
        assert_eq!(config.network, NetworkCapability::Full);
    }

    #[test]
    fn sandbox_status_any_active() {
        let inactive = SandboxStatus::default();
        let active = SandboxStatus {
            landlock: true,
            seccomp: false,
            network: false,
        };

        assert!(!inactive.any_active());
        assert!(active.any_active());
    }

    #[test]
    fn sandbox_config_from_preset_standard() {
        let config = sandbox_config_from_preset("standard", Path::new("/work"), Path::new("/data"));

        assert!(config.enabled);
        assert_eq!(config.network, NetworkCapability::LocalhostOnly);
        assert_eq!(
            config.filesystem_allow[0],
            sandbox_path(Path::new("/work"), PathMode::ReadWrite)
        );
        assert_eq!(
            config.filesystem_allow[1],
            sandbox_path(Path::new("/data"), PathMode::ReadWrite)
        );
    }

    #[test]
    fn sandbox_config_from_preset_restricted() {
        let config =
            sandbox_config_from_preset("restricted", Path::new("/work"), Path::new("/data"));

        assert!(config.enabled);
        assert_eq!(config.network, NetworkCapability::None);
        assert_eq!(
            config.filesystem_allow[0],
            sandbox_path(Path::new("/work"), PathMode::ReadOnly)
        );
        assert_eq!(
            config.filesystem_allow[1],
            sandbox_path(Path::new("/data"), PathMode::ReadWrite)
        );
        assert!(config
            .filesystem_allow
            .contains(&sandbox_path(Path::new("/etc"), PathMode::ReadOnly,)));
    }

    #[test]
    fn sandbox_config_from_preset_open() {
        let config = sandbox_config_from_preset("open", Path::new("/work"), Path::new("/data"));

        assert!(config.enabled);
        assert_eq!(config.network, NetworkCapability::Full);
        assert_eq!(config.filesystem_allow.len(), 6);
    }

    #[test]
    fn sandbox_config_from_unknown_preset() {
        let config = sandbox_config_from_preset("unknown", Path::new("/work"), Path::new("/data"));

        assert_eq!(config, SandboxConfig::default());
    }

    #[test]
    fn sandbox_config_serde_round_trip() {
        let config = sandbox_config_from_preset("power", Path::new("/work"), Path::new("/data"));

        let json = serde_json::to_string(&config).expect("serialize sandbox config");
        let decoded: SandboxConfig =
            serde_json::from_str(&json).expect("deserialize sandbox config");

        assert_eq!(decoded, config);
    }
}
