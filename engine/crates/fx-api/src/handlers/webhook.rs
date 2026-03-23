use crate::handlers::message::{internal_error, process_and_route_message};
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use fx_channel_webhook::{WebhookMessage, WebhookResponse};
use fx_core::channel::ResponseContext;
use fx_core::types::InputSource;

pub async fn handle_webhook(
    State(state): State<HttpState>,
    Path(channel_id): Path<String>,
    Json(request): Json<WebhookMessage>,
) -> Result<Json<WebhookResponse>, (StatusCode, Json<ErrorBody>)> {
    let Some(channel) = state.channels.webhooks.get(&channel_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "webhook channel not found".to_string(),
            }),
        ));
    };
    if request.text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "message must not be empty".to_string(),
            }),
        ));
    }

    let source = InputSource::Channel(channel_id.clone());
    let result = process_and_route_message(
        &state.app,
        state.channels.router.as_ref(),
        &request.text,
        Vec::new(),
        Vec::new(),
        source,
        ResponseContext::default(),
    )
    .await
    .map_err(internal_error)?;
    let text = channel
        .take_response()
        .unwrap_or_else(|| result.response.clone());

    Ok(Json(WebhookResponse {
        text,
        channel_id,
        complete: true,
    }))
}
