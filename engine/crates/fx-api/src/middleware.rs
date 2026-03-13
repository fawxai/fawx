use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::Json;
use ring::hmac;

pub fn verify_token(expected: &str, provided: &str) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, expected.as_bytes());
    let tag = hmac::sign(&key, expected.as_bytes());
    hmac::verify(&key, provided.as_bytes(), tag.as_ref()).is_ok()
}

pub async fn auth_middleware(
    State(state): State<HttpState>,
    request: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> axum::response::Response {
    let unauthorized = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".to_string(),
            }),
        )
            .into_response()
    };

    let path = request.uri().path().to_string();
    tracing::info!(endpoint = %path, "HTTP request");

    let header = match request.headers().get("authorization") {
        Some(h) => h,
        None => return unauthorized(),
    };

    let header_str = match header.to_str() {
        Ok(s) => s,
        Err(_) => return unauthorized(),
    };

    let token = match header_str.strip_prefix("Bearer ") {
        Some(t) => t,
        None => return unauthorized(),
    };

    if !verify_token(&state.bearer_token, token) {
        return unauthorized();
    }

    next.run(request).await
}
