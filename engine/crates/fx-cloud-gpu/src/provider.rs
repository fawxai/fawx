use crate::{ExecResult, GpuError, Pod, PodConfig};
use async_trait::async_trait;
use std::path::Path;

#[async_trait]
pub trait CloudGpuProvider: Send + Sync + std::fmt::Debug {
    fn provider_name(&self) -> &str;

    async fn create_pod(&self, config: PodConfig) -> Result<Pod, GpuError>;
    async fn list_pods(&self) -> Result<Vec<Pod>, GpuError>;
    async fn pod_status(&self, pod_id: &str) -> Result<Pod, GpuError>;
    async fn stop_pod(&self, pod_id: &str) -> Result<(), GpuError>;
    async fn destroy_pod(&self, pod_id: &str) -> Result<(), GpuError>;

    async fn exec(
        &self,
        pod_id: &str,
        command: &str,
        timeout_seconds: u32,
    ) -> Result<ExecResult, GpuError>;

    async fn upload(
        &self,
        pod_id: &str,
        local_path: &Path,
        remote_path: &str,
    ) -> Result<(), GpuError>;

    async fn download(
        &self,
        pod_id: &str,
        remote_path: &str,
        local_path: &Path,
    ) -> Result<(), GpuError>;
}
