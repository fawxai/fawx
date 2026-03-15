use crate::devices::DeviceStore;
use crate::engine::AppEngine;
use crate::pairing::PairingState;
use crate::server_runtime::ServerRuntime;
use fx_channel_telegram::TelegramChannel;
use fx_channel_webhook::WebhookChannel;
use fx_core::channel::Channel;
use fx_kernel::{ChannelRegistry, HttpChannel, ResponseRouter};
use fx_session::SessionRegistry;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct HttpState {
    pub app: Arc<Mutex<dyn AppEngine>>,
    pub session_registry: Option<SessionRegistry>,
    pub start_time: Instant,
    pub server_runtime: ServerRuntime,
    pub tailscale_ip: Option<String>,
    pub bearer_token: String,
    pub pairing: Arc<Mutex<PairingState>>,
    pub devices: Arc<Mutex<DeviceStore>>,
    pub devices_path: Option<PathBuf>,
    pub channels: ChannelRuntime,
    pub data_dir: PathBuf,
    pub synthesis: Arc<crate::handlers::synthesis::SynthesisState>,
    pub oauth_flows: Arc<crate::handlers::oauth::OAuthFlowStore>,
    pub permission_prompts: Arc<fx_kernel::PermissionPromptState>,
    pub cron_store: Option<fx_cron::SharedCronStore>,
}

#[derive(Clone)]
pub struct ChannelRuntime {
    pub router: Arc<ResponseRouter>,
    pub http: Arc<HttpChannel>,
    pub telegram: Option<Arc<TelegramChannel>>,
    pub webhooks: Arc<HashMap<String, Arc<WebhookChannel>>>,
}

pub fn build_channel_runtime(
    telegram: Option<Arc<TelegramChannel>>,
    webhook_channels: Vec<Arc<WebhookChannel>>,
) -> ChannelRuntime {
    let http = Arc::new(HttpChannel::new());
    let webhooks = webhook_channels
        .into_iter()
        .fold(HashMap::new(), |mut map, channel| {
            map.insert(channel.id().to_string(), channel);
            map
        });

    let mut registry = ChannelRegistry::new();
    registry.register(http.clone());
    if let Some(channel) = &telegram {
        registry.register(channel.clone());
    }
    for channel in webhooks.values() {
        registry.register(channel.clone());
    }

    let registry = Arc::new(registry);
    ChannelRuntime {
        router: Arc::new(ResponseRouter::new(registry)),
        http,
        telegram,
        webhooks: Arc::new(webhooks),
    }
}
