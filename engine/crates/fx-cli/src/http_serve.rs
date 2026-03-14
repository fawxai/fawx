//! Thin wrapper around the extracted `fx-api` crate.

use crate::headless::HeadlessApp;
use fx_api::token::BearerTokenStore;
use fx_channel_telegram::TelegramChannel;
use fx_channel_webhook::WebhookChannel;
use fx_config::HttpConfig;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub async fn run(
    app: HeadlessApp,
    port: u16,
    http_config: &HttpConfig,
    telegram: Option<Arc<TelegramChannel>>,
    webhook_channels: Vec<Arc<WebhookChannel>>,
) -> anyhow::Result<i32> {
    let data_dir = crate::startup::fawx_data_dir();
    let auth_store = match crate::auth_store::AuthStore::open(&data_dir) {
        Ok(store) => Some(store),
        Err(error) => {
            tracing::warn!(
                error = %error,
                "could not open credential store; falling back to config-only bearer token"
            );
            None
        }
    };

    let cron_store = match fx_cron::CronStore::open(&data_dir.join("cron.redb")) {
        Ok(store) => Some(Arc::new(tokio::sync::Mutex::new(store))),
        Err(error) => {
            tracing::warn!(error = %error, "cron store unavailable");
            None
        }
    };
    let scheduler_handle = start_scheduler_if_possible(&app, cron_store.as_ref());

    let result = fx_api::run(
        app,
        auth_store
            .as_ref()
            .map(|store| store as &dyn BearerTokenStore),
        fx_api::RunConfig {
            port,
            http_config: http_config.clone(),
            data_dir,
            telegram,
            webhook_channels,
            cron_store,
        },
    )
    .await;

    drop(scheduler_handle);
    result
}

fn start_scheduler_if_possible(
    app: &HeadlessApp,
    cron_store: Option<&Arc<tokio::sync::Mutex<fx_cron::CronStore>>>,
) -> Option<tokio::task::JoinHandle<()>> {
    let store = cron_store.cloned()?;
    let Some(bus) = app.session_bus().cloned() else {
        tracing::warn!("session bus unavailable; cron scheduler not started");
        return None;
    };
    let scheduler = fx_cron::Scheduler::new(store, bus, CancellationToken::new());
    Some(scheduler.start())
}
