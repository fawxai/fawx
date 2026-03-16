use crate::format::ModelFormat;
use crate::ForgeError;
use std::path::Path;

/// Backend for serving models with optional LoRA adapters.
#[async_trait::async_trait]
pub trait ServingBackend: Send + Sync {
    async fn load_model(&self, model_path: &Path, format: &ModelFormat) -> Result<(), ForgeError>;
    async fn attach_adapter(&self, adapter_path: &Path) -> Result<(), ForgeError>;
    async fn detach_adapter(&self) -> Result<(), ForgeError>;
    fn active_adapter(&self) -> Option<&Path>;
    async fn health(&self) -> Result<bool, ForgeError>;
}
