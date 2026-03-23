use crate::handlers::message::process_and_route_message;
use crate::state::HttpState;
use crate::telegram::helpers::{
    encode_photos, flush_telegram_outbound, queue_telegram_error, telegram_context,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use fx_core::types::InputSource;
use std::sync::Arc;

pub async fn handle_telegram_webhook(
    State(state): State<HttpState>,
    headers: axum::http::HeaderMap,
    body: String,
) -> impl IntoResponse {
    let telegram = match &state.channels.telegram {
        Some(channel) => Arc::clone(channel),
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let secret_header = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok());
    if !telegram.validate_webhook_secret(secret_header) {
        tracing::warn!("Telegram webhook: invalid or missing secret token");
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let mut incoming = match telegram.parse_update(&body) {
        Ok(Some(message)) => message,
        Ok(None) => return StatusCode::OK.into_response(),
        Err(error) => {
            tracing::warn!("Telegram parse error: {error:?}");
            return StatusCode::OK.into_response();
        }
    };

    tracing::info!(
        chat_id = incoming.chat_id,
        from = ?incoming.from_name,
        photos = incoming.photos.len(),
        "Telegram message received"
    );
    let _ = telegram.send_typing(incoming.chat_id).await;

    let images = encode_photos(&telegram, &mut incoming, &state.data_dir).await;
    let source = InputSource::Channel("telegram".to_string());
    let context = telegram_context(&incoming);
    if let Err(error) = process_and_route_message(
        &state.app,
        state.channels.router.as_ref(),
        &incoming.text,
        images,
        Vec::new(),
        source,
        context,
    )
    .await
    {
        tracing::error!("Telegram loop error: {error:?}");
        queue_telegram_error(&telegram, &incoming, &error);
    }
    flush_telegram_outbound(&telegram).await;
    StatusCode::OK.into_response()
}
