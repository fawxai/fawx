use crate::engine::CycleResult;
use crate::handlers::message::{
    encoded_images_to_attachments, internal_error, validate_and_encode_images,
    validate_message_text,
};
use crate::sse::{
    error_stream_frame, send_sse_frame, sse_response, stream_callback, wants_sse,
    SSE_CHANNEL_CAPACITY,
};
use crate::state::HttpState;
use crate::types::{EncodedImage, ErrorBody, MessageRequest, MessageResponse};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use fx_core::channel::ResponseContext;
use fx_core::types::InputSource;
use fx_llm::Message;
use fx_session::{
    MessageRole, SessionConfig, SessionError, SessionInfo, SessionKey, SessionKind, SessionMessage,
    SessionRegistry, SessionStatus,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    #[serde(default)]
    pub kind: Option<SessionKind>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SessionMessagesQuery {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionInfo>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct SessionMessagesResponse {
    pub messages: Vec<SessionMessage>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct DeleteSessionResponse {
    pub deleted: bool,
    pub key: String,
}

#[derive(Debug, Serialize)]
pub struct ClearSessionResponse {
    pub cleared: bool,
    pub key: String,
}

struct StreamingSessionMessageTask {
    state: HttpState,
    registry: SessionRegistry,
    key: SessionKey,
    message: String,
    images: Vec<EncodedImage>,
    context: Vec<Message>,
    sender: mpsc::Sender<String>,
    disconnected: Arc<AtomicBool>,
}

pub async fn handle_create_session(
    State(state): State<HttpState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let model = match request.model {
        Some(model) => model,
        None => {
            let app = state.app.lock().await;
            app.active_model().to_string()
        }
    };
    let config = SessionConfig {
        label: request.label,
        model,
    };

    let info = create_session(&registry, config).map_err(internal_error)?;
    Ok((StatusCode::CREATED, Json(info)).into_response())
}

pub async fn handle_list_sessions(
    State(state): State<HttpState>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let mut sessions = registry
        .list(query.kind)
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;
    sessions.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.key.as_str().cmp(right.key.as_str()))
    });
    let total = sessions.len();
    sessions.truncate(query.limit.unwrap_or(50));

    Ok(Json(ListSessionsResponse { sessions, total }).into_response())
}

pub async fn handle_get_session(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    let info = registry
        .get_info(&key)
        .map_err(|error| map_session_error(&id, error))?;
    Ok(Json(info).into_response())
}

pub async fn handle_delete_session(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    registry
        .destroy(&key)
        .map_err(|error| map_session_error(&id, error))?;

    Ok(Json(DeleteSessionResponse {
        deleted: true,
        key: id,
    })
    .into_response())
}

pub async fn handle_clear_session(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    registry
        .clear(&key)
        .map_err(|error| map_session_error(&id, error))?;

    Ok(Json(ClearSessionResponse {
        cleared: true,
        key: id,
    })
    .into_response())
}

pub async fn handle_get_messages(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Query(query): Query<SessionMessagesQuery>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    let total = registry
        .get_info(&key)
        .map_err(|error| map_session_error(&id, error))?
        .message_count;
    let messages = if query.limit == Some(0) {
        Vec::new()
    } else {
        registry
            .history(&key, query.limit.unwrap_or(100))
            .map_err(|error| map_session_error(&id, error))?
    };

    Ok(Json(SessionMessagesResponse { messages, total }).into_response())
}

pub async fn handle_send_message(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<MessageRequest>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    handle_send_message_for_session(state, headers, id, request).await
}

pub(crate) async fn handle_send_message_for_session(
    state: HttpState,
    headers: HeaderMap,
    id: String,
    request: MessageRequest,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    let history = registry
        .history(&key, usize::MAX)
        .map_err(|error| map_session_error(&id, error))?;
    let context = session_messages_to_context(&history);

    validate_message_text(&request.message)?;
    let images = validate_and_encode_images(&request.images)?;

    if wants_sse(&headers) {
        return Ok(stream_session_message_response(
            state,
            registry,
            key,
            request.message,
            images,
            context,
        )
        .await);
    }

    let (result, response) =
        process_and_route_session_message(&state, &request.message, &images, context)
            .await
            .map_err(internal_error)?;
    record_session_turn(&registry, &key, &request.message, &response).map_err(internal_error)?;

    Ok(Json(MessageResponse {
        response,
        model: result.model,
        iterations: result.iterations,
    })
    .into_response())
}

pub(crate) fn session_messages_to_context(messages: &[SessionMessage]) -> Vec<Message> {
    messages
        .iter()
        .map(|message| match message.role {
            MessageRole::User => Message::user(message.content.clone()),
            MessageRole::Assistant => Message::assistant(message.content.clone()),
            MessageRole::System => Message::system(message.content.clone()),
        })
        .collect()
}

async fn stream_session_message_response(
    state: HttpState,
    registry: SessionRegistry,
    key: SessionKey,
    message: String,
    images: Vec<EncodedImage>,
    context: Vec<Message>,
) -> Response {
    let (sender, receiver) = mpsc::channel(SSE_CHANNEL_CAPACITY);
    let disconnected = Arc::new(AtomicBool::new(false));
    tokio::spawn(run_streaming_session_message_task(
        StreamingSessionMessageTask {
            state,
            registry,
            key,
            message,
            images,
            context,
            sender,
            disconnected,
        },
    ));
    sse_response(receiver)
}

async fn run_streaming_session_message_task(task: StreamingSessionMessageTask) {
    let callback = stream_callback(task.sender.clone(), Arc::clone(&task.disconnected));
    let result = {
        let mut app = task.state.app.lock().await;
        app.process_message_with_context(
            &task.message,
            encoded_images_to_attachments(&task.images),
            task.context,
            InputSource::Http,
            Some(callback),
        )
        .await
    };

    match result {
        Ok((result, _)) => {
            if let Err(error) =
                record_session_turn(&task.registry, &task.key, &task.message, &result.response)
            {
                let _ = send_sse_frame(
                    &task.sender,
                    &task.disconnected,
                    error_stream_frame(&error.to_string()),
                );
            }
        }
        Err(error) => {
            let _ = send_sse_frame(
                &task.sender,
                &task.disconnected,
                error_stream_frame(&error.to_string()),
            );
        }
    }
}

async fn process_and_route_session_message(
    state: &HttpState,
    message: &str,
    images: &[EncodedImage],
    context: Vec<Message>,
) -> Result<(CycleResult, String), anyhow::Error> {
    let result = {
        let mut app = state.app.lock().await;
        let (result, _) = app
            .process_message_with_context(
                message,
                encoded_images_to_attachments(images),
                context,
                InputSource::Http,
                None,
            )
            .await?;
        result
    };

    state
        .channels
        .router
        .route(
            &InputSource::Http,
            &result.response,
            &ResponseContext::default(),
        )
        .map_err(|error| anyhow::anyhow!("response routing failed: {error}"))?;
    let response = state
        .channels
        .http
        .take_response()
        .unwrap_or_else(|| result.response.clone());
    Ok((result, response))
}

fn create_session(
    registry: &SessionRegistry,
    config: SessionConfig,
) -> anyhow::Result<SessionInfo> {
    for _ in 0..5 {
        let key = generate_session_key()?;
        match registry.create(key.clone(), SessionKind::Main, config.clone()) {
            Ok(_) => {
                registry.set_status(&key, SessionStatus::Idle)?;
                return registry.get_info(&key).map_err(anyhow::Error::new);
            }
            Err(SessionError::AlreadyExists(_)) => continue,
            Err(error) => return Err(anyhow::Error::new(error)),
        }
    }

    Err(anyhow::anyhow!("failed to generate a unique session key"))
}

fn generate_session_key() -> anyhow::Result<SessionKey> {
    let uuid = Uuid::new_v4().simple().to_string();
    SessionKey::new(format!("sess-{}", &uuid[..8])).map_err(anyhow::Error::new)
}

fn record_session_turn(
    registry: &SessionRegistry,
    key: &SessionKey,
    user_message: &str,
    assistant_message: &str,
) -> anyhow::Result<()> {
    registry.record_message(key, MessageRole::User, user_message)?;
    registry.record_message(key, MessageRole::Assistant, assistant_message)?;
    Ok(())
}

fn require_session_registry(
    state: &HttpState,
) -> Result<SessionRegistry, (StatusCode, Json<ErrorBody>)> {
    state
        .session_registry
        .clone()
        .ok_or_else(session_storage_unavailable)
}

fn session_key(id: &str) -> Result<SessionKey, (StatusCode, Json<ErrorBody>)> {
    SessionKey::new(id.to_string()).map_err(|_| session_not_found(id))
}

fn map_session_error(id: &str, error: SessionError) -> (StatusCode, Json<ErrorBody>) {
    match error {
        SessionError::NotFound(_) => session_not_found(id),
        other => internal_error(anyhow::Error::new(other)),
    }
}

fn session_not_found(id: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("session not found: {id}"),
        }),
    )
}

fn session_storage_unavailable() -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorBody {
            error: "session storage not available".to_string(),
        }),
    )
}
