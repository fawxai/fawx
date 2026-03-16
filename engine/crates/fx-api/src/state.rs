use crate::devices::DeviceStore;
use crate::engine::{AppEngine, ConfigManagerHandle};
use crate::pairing::PairingState;
use crate::server_runtime::ServerRuntime;
use crate::types::{ModelInfoDto, ThinkingLevelDto};
use fx_channel_telegram::TelegramChannel;
use fx_channel_webhook::WebhookChannel;
use fx_core::channel::Channel;
use fx_fleet::FleetManager;
use fx_kernel::{ChannelRegistry, HttpChannel, ResponseRouter};
use fx_session::SessionRegistry;
use fx_telemetry::{SignalCollector, TelemetryConsent};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock as TokioRwLock};

/// Read-only state cache, updated after mutations. Handlers that only need
/// to read model/thinking/usage info use this instead of locking the app Mutex.
pub struct SharedReadState {
    pub active_model: TokioRwLock<String>,
    pub thinking_level: TokioRwLock<ThinkingLevelDto>,
    pub available_models: TokioRwLock<Vec<ModelInfoDto>>,
    pub token_usage: TokioRwLock<(u64, u64)>,
}

impl SharedReadState {
    pub fn new(model: String, thinking: ThinkingLevelDto, models: Vec<ModelInfoDto>) -> Self {
        Self {
            active_model: TokioRwLock::new(model),
            thinking_level: TokioRwLock::new(thinking),
            available_models: TokioRwLock::new(models),
            token_usage: TokioRwLock::new((0, 0)),
        }
    }

    pub fn from_app(app: &dyn AppEngine) -> Self {
        Self::new(
            app.active_model().to_owned(),
            app.thinking_level(),
            app.available_models(),
        )
    }

    pub async fn update_after_cycle(
        &self,
        model: &str,
        thinking: &ThinkingLevelDto,
        tokens: (u64, u64),
    ) {
        *self.active_model.write().await = model.to_owned();
        *self.thinking_level.write().await = thinking.clone();
        *self.token_usage.write().await = tokens;
    }

    pub async fn update_model(
        &self,
        model: &str,
        thinking: &ThinkingLevelDto,
        models: Vec<ModelInfoDto>,
    ) {
        *self.active_model.write().await = model.to_owned();
        *self.thinking_level.write().await = thinking.clone();
        *self.available_models.write().await = models;
    }
}

#[derive(Clone)]
pub struct HttpState {
    pub app: Arc<Mutex<dyn AppEngine>>,
    pub shared: Arc<SharedReadState>,
    pub config_manager: Option<ConfigManagerHandle>,
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
    pub fleet_manager: Option<Arc<Mutex<FleetManager>>>,
    pub cron_store: Option<fx_cron::SharedCronStore>,
    pub experiment_registry:
        Arc<tokio::sync::Mutex<crate::experiment_registry::ExperimentRegistry>>,
    pub improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    pub telemetry: Arc<SignalCollector>,
}

#[derive(Clone)]
pub struct ChannelRuntime {
    pub router: Arc<ResponseRouter>,
    pub http: Arc<HttpChannel>,
    pub telegram: Option<Arc<TelegramChannel>>,
    pub webhooks: Arc<HashMap<String, Arc<WebhookChannel>>>,
}

pub fn default_telemetry() -> Arc<SignalCollector> {
    Arc::new(SignalCollector::new(TelemetryConsent::default()))
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
