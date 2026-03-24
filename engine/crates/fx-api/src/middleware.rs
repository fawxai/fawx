use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware;
use axum::response::IntoResponse;
use axum::Json;
use ring::digest;
use subtle::ConstantTimeEq;

/// Constant-time comparison of two token strings.
///
/// Hashes both tokens with SHA-256 before comparing to prevent
/// length-based timing side-channels. Returns `false` for empty
/// tokens as a defense-in-depth measure.
#[must_use]
pub fn verify_token(expected: &str, provided: &str) -> bool {
    if expected.is_empty() || provided.is_empty() {
        return false;
    }

    let hash_expected = digest::digest(&digest::SHA256, expected.as_bytes());
    let hash_provided = digest::digest(&digest::SHA256, provided.as_bytes());

    hash_expected.as_ref().ct_eq(hash_provided.as_ref()).into()
}

pub async fn auth_middleware(
    State(state): State<HttpState>,
    request: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> axum::response::Response {
    let path = request.uri().path().to_string();
    tracing::info!(endpoint = %path, "HTTP request");

    let Some(token) = extract_bearer_token(request.headers()) else {
        return unauthorized_response();
    };
    if verify_token(&state.bearer_token, token) {
        return next.run(request).await;
    }

    match authenticate_device(&state, token).await {
        Some(device_id) => {
            tracing::info!(endpoint = %path, device_id = %device_id, "HTTP request authenticated via device token");
            next.run(request).await
        }
        None => unauthorized_response(),
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let header = headers.get("authorization")?;
    let header_str = header.to_str().ok()?;
    header_str.strip_prefix("Bearer ")
}

async fn authenticate_device(state: &HttpState, token: &str) -> Option<String> {
    let mut devices = state.devices.lock().await;
    let device_id = devices.authenticate(token)?;
    crate::devices::persist_devices(&devices, state.devices_path.as_deref());
    Some(device_id)
}

fn unauthorized_response() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorBody {
            error: "unauthorized".to_string(),
        }),
    )
        .into_response()
}
