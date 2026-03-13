use async_trait::async_trait;
use fx_config::manager::ConfigManager;
use fx_core::types::InputSource;
use fx_kernel::StreamCallback;
use fx_llm::ImageAttachment;
use std::sync::{Arc, Mutex};

pub type ConfigManagerHandle = Arc<Mutex<ConfigManager>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleResult {
    pub response: String,
    pub model: String,
    pub iterations: u32,
}

#[async_trait]
pub trait AppEngine: Send + Sync {
    async fn process_message(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<CycleResult, anyhow::Error>;

    fn active_model(&self) -> &str;

    fn config_manager(&self) -> Option<ConfigManagerHandle>;
}
