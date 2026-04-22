use crate::devices::DeviceStore;
use crate::engine::{AppEngine, ConfigManagerHandle};
use crate::pairing::PairingState;
use crate::server_runtime::ServerRuntime;
use crate::types::{ModelInfoDto, ThinkingLevelDto};
use fx_channel_telegram::TelegramChannel;
use fx_channel_webhook::WebhookChannel;
use fx_core::channel::Channel;
use fx_fleet::FleetManager;
use fx_kernel::{
    loop_input_channel, CancellationToken, ChannelRegistry, HttpChannel, LoopCommand,
    LoopInputChannel, LoopInputSender, ResponseRouter, TokenUsage,
};
use fx_session::SessionKey;
use fx_session::SessionRegistry;
use fx_telemetry::{SignalCollector, TelemetryConsent};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
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
    pub token_usage: TokenUsage,
    pub max_history: usize,
}

/// Read-only state cache, updated after mutations. Handlers that only need
/// to read model/thinking/usage info use this instead of locking the app Mutex.
/// A single RwLock around the snapshot ensures all fields are consistent.
pub struct SharedReadState {
    snapshot: TokioRwLock<ReadSnapshot>,
}

impl SharedReadState {
    pub fn new(
        model: String,
        thinking: ThinkingLevelDto,
        models: Vec<ModelInfoDto>,
        max_history: usize,
    ) -> Self {
        Self {
            snapshot: TokioRwLock::new(ReadSnapshot {
                active_model: model,
                thinking_level: thinking,
                available_models: models,
                token_usage: TokenUsage::default(),
                max_history,
            }),
        }
    }

    pub fn from_app(app: &dyn AppEngine) -> Self {
        Self::new(
            app.active_model().to_owned(),
            app.thinking_level(),
            app.available_models(),
            app.max_history(),
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
        tokens: TokenUsage,
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
        max_history: usize,
    ) {
        let mut snap = self.snapshot.write().await;
        snap.active_model = model.to_owned();
        snap.thinking_level = thinking.clone();
        snap.available_models = models;
        snap.max_history = max_history;
    }

    /// Update just thinking level.
    pub async fn update_thinking(&self, thinking: &ThinkingLevelDto) {
        let mut snap = self.snapshot.write().await;
        snap.thinking_level = thinking.clone();
    }
}

#[derive(Debug)]
pub struct SessionRunPermit {
    key: SessionKey,
    run_id: u64,
    token: CancellationToken,
    cancel_state: Arc<SessionRunCancelState>,
    input_channel: Option<LoopInputChannel>,
}

impl SessionRunPermit {
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    pub fn cancel_reason(&self) -> Option<SessionRunCancelReason> {
        self.cancel_state.reason()
    }

    pub fn cancellation_message(&self) -> Option<&'static str> {
        self.cancel_reason().map(SessionRunCancelReason::message)
    }

    pub fn take_input_channel(&mut self) -> Option<LoopInputChannel> {
        self.input_channel.take()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRunCancelReason {
    StoppedByUser,
    SupersededByNewerRequest,
}

impl SessionRunCancelReason {
    fn code(self) -> u8 {
        match self {
            Self::StoppedByUser => 1,
            Self::SupersededByNewerRequest => 2,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::StoppedByUser),
            2 => Some(Self::SupersededByNewerRequest),
            _ => None,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::StoppedByUser => "Cancelled by user",
            Self::SupersededByNewerRequest => "Superseded by a newer request",
        }
    }
}

#[derive(Debug)]
struct SessionRunCancelState {
    reason: AtomicU8,
}

impl SessionRunCancelState {
    fn new() -> Self {
        Self {
            reason: AtomicU8::new(0),
        }
    }

    fn cancel(&self, reason: SessionRunCancelReason) -> bool {
        self.reason
            .compare_exchange(0, reason.code(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn reason(&self) -> Option<SessionRunCancelReason> {
        SessionRunCancelReason::from_code(self.reason.load(Ordering::Acquire))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSessionRunOutcome {
    Stopped,
    NoActiveRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerSessionRunOutcome {
    Steered,
    NoActiveRun,
}

pub type SessionEngineHandle = Arc<Mutex<Box<dyn AppEngine>>>;

#[derive(Clone, Default)]
pub struct SessionEnginePool {
    engines: Arc<TokioRwLock<HashMap<SessionKey, SessionEngineHandle>>>,
}

impl SessionEnginePool {
    pub async fn get_or_spawn(
        &self,
        app: &Arc<Mutex<dyn AppEngine>>,
        key: &SessionKey,
        execution_root: PathBuf,
    ) -> Result<Option<SessionEngineHandle>, anyhow::Error> {
        if let Some(engine) = self.engines.read().await.get(key).cloned() {
            return Ok(Some(engine));
        }

        let spawned = {
            let app = app.lock().await;
            app.spawn_session_engine(key, execution_root)?
        };
        let Some(engine) = spawned else {
            return Ok(None);
        };

        let handle = Arc::new(Mutex::new(engine));
        let mut engines = self.engines.write().await;
        let handle = engines
            .entry(key.clone())
            .or_insert_with(|| Arc::clone(&handle))
            .clone();
        Ok(Some(handle))
    }

    pub async fn remove(&self, key: &SessionKey) {
        self.engines.write().await.remove(key);
    }

    pub async fn clear(&self) {
        self.engines.write().await.clear();
    }
}

#[derive(Debug, Clone)]
struct SessionRunEntry {
    run_id: u64,
    token: CancellationToken,
    cancel_state: Arc<SessionRunCancelState>,
    input_sender: LoopInputSender,
}

impl SessionRunEntry {
    fn cancel(&self, reason: SessionRunCancelReason) -> bool {
        let did_cancel = self.cancel_state.cancel(reason);
        self.token.cancel();
        did_cancel
    }
}

#[derive(Debug, Clone)]
pub struct SessionRunRegistry {
    next_run_id: Arc<AtomicU64>,
    active_runs: Arc<Mutex<HashMap<SessionKey, SessionRunEntry>>>,
}

impl Default for SessionRunRegistry {
    fn default() -> Self {
        Self {
            next_run_id: Arc::new(AtomicU64::new(1)),
            active_runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SessionRunRegistry {
    pub async fn begin(&self, key: &SessionKey) -> SessionRunPermit {
        let run_id = self.next_run_id.fetch_add(1, Ordering::Relaxed);
        let token = CancellationToken::new();
        let cancel_state = Arc::new(SessionRunCancelState::new());
        let (input_sender, input_channel) = loop_input_channel();
        let previous = self.active_runs.lock().await.insert(
            key.clone(),
            SessionRunEntry {
                run_id,
                token: token.clone(),
                cancel_state: Arc::clone(&cancel_state),
                input_sender,
            },
        );
        if let Some(previous) = previous {
            previous.cancel(SessionRunCancelReason::SupersededByNewerRequest);
            tracing::warn!(
                session_id = %key.as_str(),
                previous_run_id = previous.run_id,
                run_id,
                "cancelled previous session run before starting replacement"
            );
        }
        SessionRunPermit {
            key: key.clone(),
            run_id,
            token,
            cancel_state,
            input_channel: Some(input_channel),
        }
    }

    pub async fn stop(&self, key: &SessionKey) -> StopSessionRunOutcome {
        let entry = self.active_runs.lock().await.get(key).cloned();
        match entry {
            Some(entry) if entry.cancel(SessionRunCancelReason::StoppedByUser) => {
                StopSessionRunOutcome::Stopped
            }
            Some(_) => StopSessionRunOutcome::NoActiveRun,
            None => StopSessionRunOutcome::NoActiveRun,
        }
    }

    pub async fn steer(&self, key: &SessionKey, text: String) -> SteerSessionRunOutcome {
        let entry = self.active_runs.lock().await.get(key).cloned();
        match entry {
            Some(entry) if !entry.token.is_cancelled() => {
                if entry.input_sender.send(LoopCommand::Steer(text)).is_ok() {
                    SteerSessionRunOutcome::Steered
                } else {
                    SteerSessionRunOutcome::NoActiveRun
                }
            }
            _ => SteerSessionRunOutcome::NoActiveRun,
        }
    }

    pub async fn finish(&self, permit: &SessionRunPermit) {
        let mut active_runs = self.active_runs.lock().await;
        if active_runs
            .get(&permit.key)
            .is_some_and(|entry| entry.run_id == permit.run_id)
        {
            // A cancelled permit may already have been replaced by `begin()`.
            // Only the current owner for this session is allowed to clear the
            // active slot, so a superseded run cannot remove its replacement.
            active_runs.remove(&permit.key);
        }
    }
}

#[derive(Clone)]
pub struct HttpState {
    pub app: Arc<Mutex<dyn AppEngine>>,
    pub shared: Arc<SharedReadState>,
    pub config_manager: Option<ConfigManagerHandle>,
    pub session_registry: Option<SessionRegistry>,
    pub session_runs: SessionRunRegistry,
    pub session_engines: SessionEnginePool,
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
