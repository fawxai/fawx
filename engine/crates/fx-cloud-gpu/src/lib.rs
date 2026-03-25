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
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GpuType {
    Rtx3090,
    Rtx4090,
    A100_80gb,
    H100_80gb,
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
    #[error("pod not found: {0}")]
    PodNotFound(String),
    #[error("timeout after {0}s")]
    Timeout(u32),
    #[error("ssh error: {0}")]
    Ssh(String),
    #[error("transfer error: {0}")]
    Transfer(String),
}
