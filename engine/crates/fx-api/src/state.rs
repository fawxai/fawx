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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock as TokioRwLock};

/// Atomic snapshot of frequently-read state. All fields are updated together
/// so concurrent readers never see inconsistent combinations (e.g., new model
/// with old thinking level).
#[derive(Debug, Clone)]
pub struct ReadSnapshot {
    pub active_model: String,
    pub thinking_level: ThinkingLevelDto,
    pub available_models: Vec<ModelInfoDto>,
    pub token_usage: (u64, u64),
}

/// Read-only state cache, updated after mutations. Handlers that only need
/// to read model/thinking/usage info use this instead of locking the app Mutex.
/// A single RwLock around the snapshot ensures all fields are consistent.
pub struct SharedReadState {
    snapshot: TokioRwLock<ReadSnapshot>,
}

impl SharedReadState {
    pub fn new(model: String, thinking: ThinkingLevelDto, models: Vec<ModelInfoDto>) -> Self {
        Self {
            snapshot: TokioRwLock::new(ReadSnapshot {
                active_model: model,
                thinking_level: thinking,
                available_models: models,
                token_usage: (0, 0),
            }),
        }
    }

    pub fn from_app(app: &dyn AppEngine) -> Self {
        Self::new(
            app.active_model().to_owned(),
            app.thinking_level(),
            app.available_models(),
        )
    }

    /// Read the current snapshot. Returns a clone — readers never block writers.
    pub async fn read(&self) -> ReadSnapshot {
        self.snapshot.read().await.clone()
    }

    /// Update all fields atomically after a cycle completes.
    pub async fn update_after_cycle(
        &self,
        model: &str,
        thinking: &ThinkingLevelDto,
        tokens: (u64, u64),
    ) {
        let mut snap = self.snapshot.write().await;
        snap.active_model = model.to_owned();
        snap.thinking_level = thinking.clone();
        snap.token_usage = tokens;
    }

    /// Update model-related fields atomically after a model switch.
    pub async fn update_model(
        &self,
        model: &str,
        thinking: &ThinkingLevelDto,
        models: Vec<ModelInfoDto>,
    ) {
        let mut snap = self.snapshot.write().await;
        snap.active_model = model.to_owned();
        snap.thinking_level = thinking.clone();
        snap.available_models = models;
    }

    /// Update just thinking level.
    pub async fn update_thinking(&self, thinking: &ThinkingLevelDto) {
        let mut snap = self.snapshot.write().await;
        snap.thinking_level = thinking.clone();
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
    pub ripcord: Option<Arc<fx_ripcord::RipcordJournal>>,
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

pub fn default_telemetry(data_dir: &Path) -> Arc<SignalCollector> {
    let consent = TelemetryConsent::load(data_dir);
    Arc::new(SignalCollector::new_with_persistence(
        consent,
        data_dir.to_path_buf(),
    ))
}

#[cfg(test)]
pub fn in_memory_telemetry() -> Arc<SignalCollector> {
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
