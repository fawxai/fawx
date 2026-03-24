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
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
) -> anyhow::Result<i32> {
    let data_dir = crate::startup::fawx_data_dir();
    let experiment_registry = app.experiment_registry().cloned();
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

    let cron_store = app.cron_store().cloned();
    let scheduler_handle = start_scheduler_if_possible(&app, cron_store.as_ref());
    let ripcord = Arc::clone(app.ripcord_journal());

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
            improvement_provider,
            experiment_registry,
            ripcord: Some(ripcord),
        },
    )
    .await;

    drop(scheduler_handle);
    result
}

fn start_scheduler_if_possible(
    app: &HeadlessApp,
    cron_store: Option<&fx_cron::SharedCronStore>,
) -> Option<tokio::task::JoinHandle<()>> {
    let store = cron_store.cloned()?;
    let Some(bus) = app.session_bus().cloned() else {
        tracing::warn!("session bus unavailable; cron scheduler not started");
        return None;
    };
    let scheduler = fx_cron::Scheduler::new(store, bus, CancellationToken::new());
    Some(scheduler.start())
}
