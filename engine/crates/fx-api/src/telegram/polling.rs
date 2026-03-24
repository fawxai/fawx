use crate::engine::AppEngine;
use crate::handlers::message::process_and_route_message;
use crate::telegram::helpers::{
    encode_photos, flush_telegram_outbound, queue_telegram_error, telegram_context,
};
use fx_channel_telegram::TelegramChannel;
use fx_core::types::InputSource;
use fx_kernel::ResponseRouter;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn handle_telegram_update(
    telegram: &TelegramChannel,
    app: &Arc<Mutex<dyn AppEngine>>,
    router: &ResponseRouter,
    raw_update: &serde_json::Value,
    data_dir: &std::path::Path,
) {
    let payload = raw_update.to_string();
    let mut incoming = match telegram.parse_update(&payload) {
        Ok(Some(message)) => message,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!("Telegram poll parse error: {error}");
            return;
        }
    };

    tracing::info!(
        chat_id = incoming.chat_id,
        from = ?incoming.from_name,
        photos = incoming.photos.len(),
        "Telegram poll: message received"
    );
    let _ = telegram.send_typing(incoming.chat_id).await;

    let images = encode_photos(telegram, &mut incoming, data_dir).await;
    let source = InputSource::Channel("telegram".to_string());
    let context = telegram_context(&incoming);
    if let Err(error) = process_and_route_message(
        app,
        router,
        &incoming.text,
        images,
        Vec::new(),
        source,
        context,
    )
    .await
    {
        tracing::error!("Telegram poll loop error: {error}");
        queue_telegram_error(telegram, &incoming, &error);
    }
    flush_telegram_outbound(telegram).await;
}

pub async fn run_telegram_polling(
    telegram: Arc<TelegramChannel>,
    app: Arc<Mutex<dyn AppEngine>>,
    router: Arc<ResponseRouter>,
    data_dir: PathBuf,
) {
    if let Err(error) = telegram.delete_webhook().await {
        tracing::error!("Telegram poll: failed to delete webhook: {error}");
    }

    let mut offset: i64 = 0;
    loop {
        flush_telegram_outbound(&telegram).await;
        match telegram.get_updates(offset, 30).await {
            Ok((updates, next_offset)) => {
                for update in &updates {
                    handle_telegram_update(&telegram, &app, router.as_ref(), update, &data_dir)
                        .await;
                }
                offset = next_offset;
            }
            Err(error) => {
                tracing::error!("Telegram poll error: {error}");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}
