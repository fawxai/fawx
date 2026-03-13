mod config_redaction;

pub mod engine;
pub mod error;
pub mod handlers;
pub mod listener;
pub mod middleware;
pub mod router;
pub mod sse;
pub mod state;
pub mod tailscale;
pub mod telegram;
pub mod token;
pub mod types;

#[cfg(test)]
mod tests;

use crate::engine::AppEngine;
use crate::listener::{
    active_tailscale_ip, bind_listeners, detect_optional_tailscale_ip, listen_targets,
    print_startup_targets, run_listeners,
};
use crate::router::{build_router, load_fleet_manager_if_initialized};
use crate::state::{build_channel_runtime, HttpState};
use crate::telegram::polling::run_telegram_polling;
use crate::token::{validate_bearer_token, BearerTokenStore};
use fx_channel_telegram::TelegramChannel;
use fx_channel_webhook::WebhookChannel;
use fx_config::HttpConfig;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

pub use tailscale::is_tailscale_ip;
pub use types::{ErrorBody, HealthResponse, MessageRequest, MessageResponse, StatusResponse};

pub async fn run(
    app: impl AppEngine + 'static,
    port: u16,
    http_config: &HttpConfig,
    auth_store: Option<&dyn BearerTokenStore>,
    data_dir: &Path,
    telegram: Option<Arc<TelegramChannel>>,
    webhook_channels: Vec<Arc<WebhookChannel>>,
) -> anyhow::Result<i32> {
    let bearer_token = validate_bearer_token(http_config, auth_store)
        .map_err(|error| anyhow::anyhow!("{error}"))?;

    let listen_plan = listen_targets(port, detect_optional_tailscale_ip());
    let listeners = bind_listeners(listen_plan).await?;
    let shared_app: Arc<Mutex<dyn AppEngine>> = Arc::new(Mutex::new(app));
    let channels = build_channel_runtime(telegram.clone(), webhook_channels);
    let state = HttpState {
        app: Arc::clone(&shared_app),
        start_time: Instant::now(),
        tailscale_ip: active_tailscale_ip(&listeners),
        bearer_token,
        channels: channels.clone(),
        data_dir: data_dir.to_path_buf(),
    };
    let fleet_manager = load_fleet_manager_if_initialized(data_dir)?;
    let router = build_router(state, fleet_manager);

    print_startup_targets(&listeners);
    eprintln!("Bearer token authentication: enabled");
    validate_telegram_startup(channels.telegram.as_ref()).await;
    start_telegram_polling(&channels, &shared_app, data_dir);

    run_listeners(router, listeners).await?;
    Ok(0)
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
