//! Local HTTP + SSE adapter for Fawx headless mode.
//!
//! Exposes a lightweight localhost-only API intended for the Phase 1 TUI
//! fork. Unlike the older Tailscale-only server, this binds to 127.0.0.1,
//! requires no bearer token, and streams assistant text deltas as SSE.

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use futures::stream;
use fx_config::FawxConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};

use crate::headless::{HeadlessApp, HeadlessStreamEvent};

const MAX_REQUEST_BYTES: usize = 1_048_576;

#[derive(Clone)]
struct HttpState {
    app: Arc<Mutex<HeadlessApp>>,
}

#[derive(Debug, Deserialize)]
struct MessageRequest {
    message: Option<String>,
    content: Option<String>,
}

impl MessageRequest {
    fn into_message(self) -> Option<String> {
        self.message
            .or(self.content)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    status: &'static str,
    model: String,
    memory_entries: usize,
    tools: Vec<String>,
    config: Value,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

pub async fn run(mut app: HeadlessApp, port: u16) -> anyhow::Result<i32> {
    app.apply_http_defaults();
    let state = HttpState {
        app: Arc::new(Mutex::new(app)),
    };
    let router = build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await?;

    eprintln!("fawx serve --http");
    eprintln!("local API: http://127.0.0.1:{port}");
    eprintln!("POST /message streams SSE text deltas");

    axum::serve(listener, router).await?;
    Ok(0)
}

fn build_router(state: HttpState) -> Router {
    Router::new()
        .route("/message", post(handle_message))
        .route("/health", get(handle_health))
        .route("/status", get(handle_status))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state)
}

async fn handle_message(
    State(state): State<HttpState>,
    Json(request): Json<MessageRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorBody>)> {
    let Some(message) = request.into_message() else {
        return Err(bad_request("message must not be empty"));
    };

    let (tx, rx) = mpsc::channel::<HeadlessStreamEvent>(64);
    let app = Arc::clone(&state.app);
    tokio::spawn(async move {
        let mut app = app.lock().await;
        if let Err(error) = app.process_message_streaming(&message, &tx).await {
            let _ = tx
                .send(HeadlessStreamEvent::Error {
                    error: error.to_string(),
                })
                .await;
        }
    });

    let stream = stream::unfold(rx, |mut rx| async {
        rx.recv()
            .await
            .map(|payload| (Ok::<Event, Infallible>(sse_event(payload)), rx))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn handle_status(
    State(state): State<HttpState>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ErrorBody>)> {
    let app = state.app.lock().await;
    Ok(Json(StatusResponse {
        status: "ok",
        model: app.active_model().to_string(),
        memory_entries: app.memory_entry_count(),
        tools: Vec::new(),
        config: sanitize_config(app.config()),
    }))
}

fn sanitize_config(config: &FawxConfig) -> Value {
    let mut value = serde_json::to_value(config).unwrap_or(Value::Null);
    redact_sensitive_config_fields(&mut value);
    value
}

fn redact_sensitive_config_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map.iter_mut() {
                if is_sensitive_config_key(key) {
                    *nested = Value::String("<redacted>".to_string());
                } else {
                    redact_sensitive_config_fields(nested);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_sensitive_config_fields(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn is_sensitive_config_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("token") || normalized.contains("secret")
}

fn bad_request(message: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            error: message.to_string(),
        }),
    )
}

fn sse_event(payload: HeadlessStreamEvent) -> Event {
    match serde_json::to_string(&payload) {
        Ok(data) => Event::default().data(data),
        Err(error) => {
            let fallback = format!(r#"{{"type":"error","error":"{error}"}}"#);
            Event::default().data(fallback)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_request_accepts_message_field() {
        let message = MessageRequest {
            message: Some(" hello ".to_string()),
            content: None,
        }
        .into_message();

        assert_eq!(message.as_deref(), Some("hello"));
    }

    #[test]
    fn message_request_accepts_content_field() {
        let message = MessageRequest {
            message: None,
            content: Some(" hi ".to_string()),
        }
        .into_message();

        assert_eq!(message.as_deref(), Some("hi"));
    }

    #[test]
    fn message_request_rejects_blank_input() {
        let message = MessageRequest {
            message: Some("   ".to_string()),
            content: None,
        }
        .into_message();

        assert!(message.is_none());
    }

    #[test]
    fn sanitize_config_redacts_sensitive_fields() {
        let mut config = FawxConfig::default();
        config.http.bearer_token = Some("secret-token".to_string());
        config.telegram.bot_token = Some("bot-token".to_string());
        config.telegram.webhook_secret = Some("webhook-secret".to_string());
        config.fleet.nodes.push(fx_config::NodeConfig {
            name: "node-a".to_string(),
            endpoint: "http://127.0.0.1:9999".to_string(),
            auth_token: Some("node-auth".to_string()),
            capabilities: vec!["agentic_loop".to_string()],
        });
        config.model.default_model = Some("claude-opus-4-6".to_string());

        let sanitized = sanitize_config(&config);

        assert_eq!(sanitized["http"]["bearer_token"], "<redacted>");
        assert_eq!(sanitized["telegram"]["bot_token"], "<redacted>");
        assert_eq!(sanitized["telegram"]["webhook_secret"], "<redacted>");
        assert_eq!(sanitized["fleet"]["nodes"][0]["auth_token"], "<redacted>");
        assert_eq!(sanitized["model"]["default_model"], "claude-opus-4-6");
    }
}
