pub(crate) mod auth_store;
pub mod bundle;
pub(crate) mod config_redaction;
pub mod time_util;

pub(crate) mod devices;
pub mod engine;
pub(crate) mod error;
pub mod experiment_bridge;
pub mod experiment_registry;
pub(crate) mod handlers;
pub mod launchagent;
pub(crate) mod listener;
pub(crate) mod middleware;
pub(crate) mod pairing;
pub(crate) mod router;
pub mod server_runtime;
pub(crate) mod skill_manifests;
pub(crate) mod sse;
pub(crate) mod state;
pub mod tailscale;
pub mod telegram;
pub mod token;
pub(crate) mod types;

#[cfg(test)]
mod tests;

use crate::devices::DeviceStore;
use crate::engine::AppEngine;
use crate::listener::{
    active_tailscale_ip, bind_listeners, detect_optional_tailscale_ip, detect_tls_config,
    listen_targets, print_startup_targets, run_listeners, ServerProtocol,
};
use crate::pairing::PairingState;
use crate::router::{build_router, load_fleet_manager_if_initialized};
use crate::server_runtime::ServerRuntime;
use crate::state::{build_channel_runtime, default_telemetry, HttpState, SharedReadState};
use crate::telegram::polling::run_telegram_polling;
use crate::token::{validate_bearer_token, BearerTokenStore};
use fx_channel_telegram::TelegramChannel;
use fx_channel_webhook::WebhookChannel;
use fx_config::HttpConfig;
use fx_session::SessionRegistry;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

pub use engine::ResultKind;
pub use tailscale::is_tailscale_ip;
pub use types::{
    ApiKeyRequest, ApiKeyResponse, AuthProviderDto, ContextInfoDto, ContextInfoSnapshotLike,
    DeleteProviderResponse, ErrorBody, ErrorRecordDto, HealthResponse, MessageRequest,
    MessageResponse, ModelInfoDto, ModelSwitchDto, RecentErrorsResponse, SendToSessionRequest,
    SendToSessionResponse, ServerRestartResponse, ServerStatusResponse, ServerStopResponse,
    SetupAuthStatus, SetupStatusResponse, SetupTailscaleStatus, SetupTokenRequest,
    SetupTokenResponse, SkillSummaryDto, StatusResponse, ThinkingAdjustedDto, ThinkingLevelDto,
    VerifyRequest, VerifyResponse,
};

pub type SharedExperimentRegistry = Arc<Mutex<crate::experiment_registry::ExperimentRegistry>>;

pub struct RunConfig {
    pub port: u16,
    pub http_config: HttpConfig,
    pub data_dir: PathBuf,
    pub telegram: Option<Arc<TelegramChannel>>,
    pub webhook_channels: Vec<Arc<WebhookChannel>>,
    pub cron_store: Option<fx_cron::SharedCronStore>,
    pub improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    pub experiment_registry: Option<SharedExperimentRegistry>,
    pub ripcord: Option<Arc<fx_ripcord::RipcordJournal>>,
}

pub async fn run(
    app: impl AppEngine + 'static,
    auth_store: Option<&dyn BearerTokenStore>,
    config: RunConfig,
) -> anyhow::Result<i32> {
    let bearer_token = validate_bearer_token(&config.http_config, auth_store)
        .map_err(|error| anyhow::anyhow!("{error}"))?;

    let tls_config = detect_tls_config(&config.data_dir);
    let tailscale_protocol = if tls_config.is_some() {
        ServerProtocol::Https
    } else {
        ServerProtocol::Http
    };
    let listen_plan = listen_targets(config.port, detect_optional_tailscale_ip());
    let listeners = bind_listeners(listen_plan, tailscale_protocol).await?;
    let tailscale_ip = active_tailscale_ip(&listeners);
    let tailscale_https_enabled =
        listeners.tailscale.is_some() && tailscale_protocol == ServerProtocol::Https;
    let server_host = if tailscale_https_enabled {
        tailscale_ip
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string())
    } else {
        "127.0.0.1".to_string()
    };
    let shared_app: Arc<Mutex<dyn AppEngine>> = Arc::new(Mutex::new(app));
    let channels = build_channel_runtime(config.telegram.clone(), config.webhook_channels);
    let session_registry = init_session_registry(&config.data_dir);
    let experiment_registry = match config.experiment_registry {
        Some(registry) => registry,
        None => {
            let registry = crate::experiment_registry::ExperimentRegistry::new(&config.data_dir)
                .map_err(|e| anyhow::anyhow!("Failed to load experiment registry: {e}"))?;
            Arc::new(Mutex::new(registry))
        }
    };
    let fleet_manager = load_fleet_manager_if_initialized(&config.data_dir)?;
    let devices_path = config.data_dir.join("devices.json");
    let devices = DeviceStore::load(&devices_path);
    let server_runtime = ServerRuntime::new(
        server_host,
        config.port,
        tailscale_https_enabled,
        crate::server_runtime::RestartController::live(),
    );
    let (shared, config_manager, has_synthesis) = {
        let app = shared_app.lock().await;
        let config_manager = app.config_manager();
        let has_synthesis = config_manager
            .as_ref()
            .and_then(|manager| manager.lock().ok())
            .map(|guard| guard.config().model.synthesis_instruction.is_some())
            .unwrap_or(false);
        (
            Arc::new(SharedReadState::from_app(&*app)),
            config_manager,
            has_synthesis,
        )
    };
    let state = HttpState {
        app: Arc::clone(&shared_app),
        shared: Arc::clone(&shared),
        config_manager,
        session_registry,
        start_time: Instant::now(),
        server_runtime,
        tailscale_ip: tailscale_ip.clone(),
        bearer_token,
        pairing: Arc::new(Mutex::new(PairingState::new())),
        devices: Arc::new(Mutex::new(devices)),
        devices_path: Some(devices_path),
        channels: channels.clone(),
        data_dir: config.data_dir.clone(),
        synthesis: Arc::new(crate::handlers::synthesis::SynthesisState::new(
            has_synthesis,
        )),
        oauth_flows: Arc::new(crate::handlers::oauth::OAuthFlowStore::new()),
        permission_prompts: Arc::new(fx_kernel::PermissionPromptState::new()),
        ripcord: config.ripcord.clone(),
        fleet_manager: fleet_manager.clone(),
        cron_store: config.cron_store.clone(),
        experiment_registry,
        improvement_provider: config.improvement_provider.clone(),
        telemetry: default_telemetry(&config.data_dir),
    };
    let router = build_router(state, fleet_manager);

    print_startup_targets(&listeners, tailscale_https_enabled);
    eprintln!("Bearer token authentication: enabled");
    validate_telegram_startup(channels.telegram.as_ref()).await;
    start_telegram_polling(&channels, &shared_app, &config.data_dir);

    run_listeners(router, listeners, tls_config).await?;
    Ok(0)
}

fn init_session_registry(data_dir: &Path) -> Option<SessionRegistry> {
    SessionRegistry::open(&data_dir.join("sessions.redb"))
}

async fn validate_telegram_startup(telegram: Option<&Arc<TelegramChannel>>) {
    let Some(tg) = telegram else {
        return;
    };

    match tg.get_me().await {
        Ok(()) => {
            eprintln!("Telegram channel: enabled (token valid, webhook at /telegram/webhook)");
            tg.register_commands().await;
        }
        Err(error) => {
            eprintln!("Warning: Telegram get_me failed: {error}");
            eprintln!(
                "Telegram channel: enabled (webhook at /telegram/webhook) — token may be invalid"
            );
        }
    }
}

fn start_telegram_polling(
    channels: &state::ChannelRuntime,
    shared_app: &Arc<Mutex<dyn AppEngine>>,
    data_dir: &Path,
) {
    let Some(telegram) = channels.telegram.as_ref() else {
        return;
    };

    tokio::spawn(run_telegram_polling(
        Arc::clone(telegram),
        Arc::clone(shared_app),
        Arc::clone(&channels.router),
        data_dir.to_path_buf(),
    ));
    eprintln!("Telegram long-polling loop: started");
}
