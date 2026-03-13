//! Thin wrapper around the extracted `fx-api` crate.

use crate::headless::HeadlessApp;
use fx_api::token::BearerTokenStore;
use fx_channel_telegram::TelegramChannel;
use fx_channel_webhook::WebhookChannel;
use fx_config::HttpConfig;
use std::sync::Arc;

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

    fx_api::run(
        app,
        port,
        http_config,
        auth_store
            .as_ref()
            .map(|store| store as &dyn BearerTokenStore),
        &data_dir,
        telegram,
        webhook_channels,
    )
    .await
}
