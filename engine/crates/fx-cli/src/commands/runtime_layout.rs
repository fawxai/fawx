use crate::repo_root;
use crate::startup;
use fx_config::FawxConfig;
use std::path::PathBuf;

const DEFAULT_HTTP_PORT: u16 = 8400;
const DEFAULT_LOG_DIR_SUFFIX: &str = ".fawx/logs";
const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text-v1.5";

#[derive(Debug, Clone)]
pub struct RuntimeLayout {
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
    pub storage_dir: PathBuf,
    pub audit_log_path: PathBuf,
    pub auth_db_path: PathBuf,
    pub logs_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub trusted_keys_dir: PathBuf,
    pub embedding_model_dir: PathBuf,
    pub pid_file: PathBuf,
    pub memory_json_path: PathBuf,
    pub sessions_dir: PathBuf,
    pub security_baseline_path: PathBuf,
    pub repo_root: PathBuf,
    pub http_port: u16,
    pub config: FawxConfig,
}

impl RuntimeLayout {
    pub fn detect() -> anyhow::Result<Self> {
        let base_dir = startup::fawx_data_dir();
        let config = startup::load_config().unwrap_or_default();
        Self::from_parts(base_dir, config)
    }

    fn from_parts(base_dir: PathBuf, config: FawxConfig) -> anyhow::Result<Self> {
        let data_dir = startup::configured_data_dir(&base_dir, &config);
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let logs_dir = configured_logs_dir(&config, &home);
        let embedding_model_dir = data_dir.join("models").join(DEFAULT_EMBEDDING_MODEL);
        Ok(Self {
            config_path: base_dir.join("config.toml"),
            storage_dir: data_dir.join("storage"),
            audit_log_path: data_dir.join("audit.log"),
            auth_db_path: data_dir.join("auth.db"),
            skills_dir: data_dir.join("skills"),
            trusted_keys_dir: data_dir.join("trusted_keys"),
            pid_file: data_dir.join("fawx.pid"),
            memory_json_path: data_dir.join("memory").join("memory.json"),
            sessions_dir: data_dir.join("signals"),
            security_baseline_path: data_dir.join("security-baseline.json"),
            repo_root: repo_root::detect_repo_root()?,
            http_port: DEFAULT_HTTP_PORT,
            data_dir,
            logs_dir,
            embedding_model_dir,
            config,
        })
    }
}

fn configured_logs_dir(config: &FawxConfig, home: &std::path::Path) -> PathBuf {
    config
        .logging
        .log_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(DEFAULT_LOG_DIR_SUFFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_points_at_workspace_root() {
        let root = repo_root::detect_repo_root().expect("repo root");
        assert!(root.join("engine").is_dir());
        assert!(root.join(".github").exists());
    }

    #[test]
    fn runtime_layout_uses_default_http_port() {
        let layout = RuntimeLayout::detect().expect("layout");
        assert_eq!(layout.http_port, DEFAULT_HTTP_PORT);
    }
}
