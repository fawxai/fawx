use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod provider;
mod skill;

pub use provider::CloudGpuProvider;
pub use skill::CloudGpuSkill;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodConfig {
    pub name: String,
    pub gpu: GpuType,
    pub gpu_count: u32,
    pub image: String,
    pub disk_gb: u32,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GpuType {
    Rtx3090,
    Rtx4090,
    #[serde(rename = "A100_80gb")]
    A100_80Gb,
    #[serde(rename = "H100_80gb")]
    H100_80Gb,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pod {
    pub id: String,
    pub status: PodStatus,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub gpu: GpuType,
    pub cost_per_hour: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PodStatus {
    Creating,
    Running,
    Stopped,
    Terminated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("authentication failed: {0}")]
    Authentication(String),
    #[error("pod not found: {0}")]
    PodNotFound(String),
    #[error("rate limited: retry after {retry_after_seconds}s")]
    RateLimited { retry_after_seconds: u32 },
    #[error("timeout after {0}s")]
    Timeout(u32),
    #[error("ssh error: {0}")]
    Ssh(String),
    #[error("transfer error: {0}")]
    Transfer(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pod_config_defaults_env_when_omitted() {
        let config: PodConfig = serde_json::from_value(json!({
            "name": "trainer",
            "gpu": "Rtx4090",
            "gpu_count": 1,
            "image": "nvidia/cuda:12.0.0-runtime-ubuntu22.04",
            "disk_gb": 200,
        }))
        .expect("pod config without env should deserialize");

        assert!(config.env.is_empty());
    }

    #[test]
    fn gpu_type_uses_legacy_wire_names_for_80gb_variants() {
        let a100 = serde_json::to_string(&GpuType::A100_80Gb).expect("serialize A100");
        let h100 = serde_json::to_string(&GpuType::H100_80Gb).expect("serialize H100");

        assert_eq!(a100, "\"A100_80gb\"");
        assert_eq!(h100, "\"H100_80gb\"");
        assert!(matches!(
            serde_json::from_str::<GpuType>("\"A100_80gb\""),
            Ok(GpuType::A100_80Gb)
        ));
        assert!(matches!(
            serde_json::from_str::<GpuType>("\"H100_80gb\""),
            Ok(GpuType::H100_80Gb)
        ));
    }
}
