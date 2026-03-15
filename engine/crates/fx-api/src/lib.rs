pub(crate) mod auth_store;
pub(crate) mod config_redaction;
pub mod time_util;

pub(crate) mod devices;
pub mod engine;
pub(crate) mod error;
pub(crate) mod handlers;
pub mod launchagent;
pub(crate) mod listener;
pub(crate) mod middleware;
pub(crate) mod pairing;
pub(crate) mod router;
pub mod server_runtime;
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
    active_tailscale_ip, bind_listeners, detect_optional_tailscale_ip, listen_targets,
    print_startup_targets, run_listeners,
};
use crate::pairing::PairingState;
use crate::router::{build_router, load_fleet_manager_if_initialized};
use crate::server_runtime::ServerRuntime;
use crate::state::{build_channel_runtime, HttpState};
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

pub use tailscale::is_tailscale_ip;
pub use types::{
    ApiKeyRequest, ApiKeyResponse, AuthProviderDto, ContextInfoDto, ContextInfoSnapshotLike,
    DeleteProviderResponse, ErrorBody, ErrorRecordDto, HealthResponse, MessageRequest,
    MessageResponse, ModelInfoDto, ModelSwitchDto, RecentErrorsResponse, SendToSessionRequest,
    SendToSessionResponse, ServerRestartResponse, ServerStatusResponse, SetupAuthStatus,
    SetupStatusResponse, SetupTailscaleStatus, SetupTokenRequest, SetupTokenResponse,
    SkillSummaryDto, StatusResponse, ThinkingAdjustedDto, ThinkingLevelDto, VerifyRequest,
    VerifyResponse,
};

pub struct RunConfig {
    pub port: u16,
    pub http_config: HttpConfig,
    pub data_dir: PathBuf,
    pub telegram: Option<Arc<TelegramChannel>>,
    pub webhook_channels: Vec<Arc<WebhookChannel>>,
    pub cron_store: Option<fx_cron::SharedCronStore>,
}

pub async fn run(
    app: impl AppEngine + 'static,
    auth_store: Option<&dyn BearerTokenStore>,
    config: RunConfig,
) -> anyhow::Result<i32> {
    let bearer_token = validate_bearer_token(&config.http_config, auth_store)
        .map_err(|error| anyhow::anyhow!("{error}"))?;

    let listen_plan = listen_targets(config.port, detect_optional_tailscale_ip());
    let listeners = bind_listeners(listen_plan).await?;
    let shared_app: Arc<Mutex<dyn AppEngine>> = Arc::new(Mutex::new(app));
    let channels = build_channel_runtime(config.telegram.clone(), config.webhook_channels);
    let session_registry = init_session_registry(&config.data_dir);
    let devices_path = config.data_dir.join("devices.json");
    let devices = DeviceStore::load(&devices_path);
    let server_runtime = ServerRuntime::local(config.port);
    let state = HttpState {
        app: Arc::clone(&shared_app),
        session_registry,
        start_time: Instant::now(),
        server_runtime,
        tailscale_ip: active_tailscale_ip(&listeners),
        bearer_token,
        pairing: Arc::new(Mutex::new(PairingState::new())),
        devices: Arc::new(Mutex::new(devices)),
        devices_path: Some(devices_path),
        channels: channels.clone(),
        data_dir: config.data_dir.clone(),
        cron_store: config.cron_store.clone(),
    };
    let fleet_manager = load_fleet_manager_if_initialized(&config.data_dir)?;
    let router = build_router(state, fleet_manager);

    print_startup_targets(&listeners);
    eprintln!("Bearer token authentication: enabled");
    validate_telegram_startup(channels.telegram.as_ref()).await;
    start_telegram_polling(&channels, &shared_app, &config.data_dir);

    run_listeners(router, listeners).await?;
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
