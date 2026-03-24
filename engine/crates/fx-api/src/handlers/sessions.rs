use crate::engine::CycleResult;
use crate::handlers::message::{
    encoded_documents_to_attachments, encoded_images_to_attachments, internal_error,
    validate_and_encode_documents, validate_and_encode_images, validate_message_request,
    validate_message_text,
};
use crate::sse::{
    error_stream_frame, send_sse_frame, sse_response, stream_callback, wants_sse,
    SSE_CHANNEL_CAPACITY,
};
use crate::state::HttpState;
use crate::types::{
    EncodedDocument, EncodedImage, ErrorBody, MessageRequest, MessageResponse,
    SendToSessionRequest, SendToSessionResponse,
};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use fx_bus::{Envelope, Payload, SessionBus};
use fx_core::channel::ResponseContext;
use fx_core::types::InputSource;
use fx_kernel::StreamCallback;
use fx_llm::{trim_conversation_history, ContentBlock, Message};
use fx_session::{
    SessionConfig, SessionError, SessionInfo, SessionKey, SessionKind, SessionMemory,
    SessionMessage, SessionRegistry, SessionStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use uuid::Uuid;

const SESSION_MEMORY_MAX_ITEMS: usize = 20;
const SESSION_MEMORY_MAX_TOKENS: usize = 2_000;

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
    documents: Vec<EncodedDocument>,
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

pub async fn handle_get_context(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    registry
        .get_info(&key)
        .map_err(|error| map_session_error(&id, error))?;
    let history = registry
        .history(&key, usize::MAX)
        .map_err(|error| map_session_error(&id, error))?;
    let context = session_messages_to_context(&history);
    let app = state.app.lock().await;
    Ok(Json(app.context_info_for_messages(&context)).into_response())
}

pub async fn handle_get_session_memory(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    let memory = registry
        .memory(&key)
        .map_err(|error| map_session_error(&id, error))?;
    Ok(Json(memory).into_response())
}

pub async fn handle_update_session_memory(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Json(memory): Json<SessionMemory>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    let memory = validate_session_memory(memory)?;
    registry
        .record_turn(&key, Vec::new(), memory.clone())
        .map_err(|error| map_session_error(&id, error))?;

    let mut app = state.app.lock().await;
    if app.loaded_session_key().as_ref() == Some(&key) {
        let _ = app.replace_session_memory(memory.clone());
    }

    Ok(Json(memory).into_response())
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

pub async fn handle_send_to_session(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Json(request): Json<SendToSessionRequest>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let target = target_session_key(&id)?;
    let payload = send_request_payload(request)?;
    let bus = load_session_bus(&state).await?;
    let result = bus
        .send(Envelope::new(None, target, payload))
        .await
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;

    Ok(Json(SendToSessionResponse {
        envelope_id: result.envelope_id,
        delivered: result.delivered,
    })
    .into_response())
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
    let mut context = session_messages_to_context(&history);
    let max_history = {
        let app = state.app.lock().await;
        app.max_history()
    };
    trim_conversation_history(&mut context, max_history);

    validate_message_request(
        &request.message,
        request.images.len(),
        request.documents.len(),
    )?;
    let images = validate_and_encode_images(&request.images)?;
    let documents = validate_and_encode_documents(&request.documents)?;

    if wants_sse(&headers) {
        return Ok(stream_session_message_response(
            state,
            registry,
            key,
            request.message,
            images,
            documents,
            context,
        )
        .await);
    }

    let (result, response, session_messages, session_memory) = process_and_route_session_message(
        &state,
        &registry,
        &key,
        &request.message,
        &images,
        &documents,
        context,
    )
    .await
    .map_err(internal_error)?;
    persist_session_turn(&registry, &key, session_messages, session_memory)
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;

    Ok(Json(MessageResponse {
        response,
        model: result.model,
        iterations: result.iterations,
        result_kind: result.result_kind,
    })
    .into_response())
}

pub(crate) fn session_messages_to_context(messages: &[SessionMessage]) -> Vec<Message> {
    let context = messages
        .iter()
        .map(SessionMessage::to_llm_message)
        .collect();
    prune_unresolved_tool_context(context)
}

fn prune_unresolved_tool_context(messages: Vec<Message>) -> Vec<Message> {
    let mut tool_use_ids = HashSet::new();
    let mut tool_result_ids = HashSet::new();

    for message in &messages {
        for block in &message.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    tool_use_ids.insert(id.clone());
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    tool_result_ids.insert(tool_use_id.clone());
                }
                ContentBlock::Text { .. }
                | ContentBlock::Image { .. }
                | ContentBlock::Document { .. } => {}
            }
        }
    }

    let unresolved_tool_use_ids = tool_use_ids
        .iter()
        .filter(|id| !tool_result_ids.contains(*id))
        .cloned()
        .collect::<HashSet<_>>();

    messages
        .into_iter()
        .filter_map(|mut message| {
            message.content.retain(|block| match block {
                ContentBlock::ToolUse { id, .. } => !unresolved_tool_use_ids.contains(id),
                ContentBlock::ToolResult { tool_use_id, .. } => tool_use_ids.contains(tool_use_id),
                ContentBlock::Text { .. }
                | ContentBlock::Image { .. }
                | ContentBlock::Document { .. } => true,
            });
            (!message.content.is_empty()).then_some(message)
        })
        .collect()
}

async fn stream_session_message_response(
    state: HttpState,
    registry: SessionRegistry,
    key: SessionKey,
    message: String,
    images: Vec<EncodedImage>,
    documents: Vec<EncodedDocument>,
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
            documents,
            context,
            sender,
            disconnected,
        },
    ));
    sse_response(receiver)
}

async fn run_streaming_session_message_task(task: StreamingSessionMessageTask) {
    let callback = stream_callback(task.sender.clone(), Arc::clone(&task.disconnected));
    let result = execute_session_turn(
        &task.state,
        &task.registry,
        &task.key,
        &task.message,
        &task.images,
        &task.documents,
        task.context,
        Some(callback),
    )
    .await;

    match result {
        Ok((_result, session_messages, session_memory)) => {
            if let Err(error) =
                persist_session_turn(&task.registry, &task.key, session_messages, session_memory)
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
    registry: &SessionRegistry,
    key: &SessionKey,
    message: &str,
    images: &[EncodedImage],
    documents: &[EncodedDocument],
    context: Vec<Message>,
) -> Result<(CycleResult, String, Vec<SessionMessage>, SessionMemory), anyhow::Error> {
    let (result, session_messages, session_memory) = execute_session_turn(
        state, registry, key, message, images, documents, context, None,
    )
    .await?;

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
    Ok((result, response, session_messages, session_memory))
}

#[allow(clippy::too_many_arguments)]
async fn execute_session_turn(
    state: &HttpState,
    registry: &SessionRegistry,
    key: &SessionKey,
    message: &str,
    images: &[EncodedImage],
    documents: &[EncodedDocument],
    context: Vec<Message>,
    callback: Option<StreamCallback>,
) -> Result<(CycleResult, Vec<SessionMessage>, SessionMemory), anyhow::Error> {
    let loaded_memory = registry.memory(key).map_err(anyhow::Error::new)?;
    let mut app = state.app.lock().await;
    let previous_memory = app.replace_session_memory(loaded_memory);
    let outcome = app
        .process_message_with_context(
            message,
            encoded_images_to_attachments(images),
            encoded_documents_to_attachments(documents),
            context,
            InputSource::Http,
            callback,
        )
        .await;
    state
        .shared
        .update_after_cycle(
            app.active_model(),
            &app.thinking_level(),
            app.session_token_usage(),
        )
        .await;
    let result = match outcome {
        Ok((result, _)) => {
            let session_messages = app.take_last_session_messages();
            let session_memory = app.session_memory();
            Ok((result, session_messages, session_memory))
        }
        Err(error) => Err(error),
    };
    app.replace_session_memory(previous_memory);
    result
}

fn persist_session_turn(
    registry: &SessionRegistry,
    key: &SessionKey,
    session_messages: Vec<SessionMessage>,
    session_memory: SessionMemory,
) -> Result<(), SessionError> {
    registry.record_turn(key, session_messages, session_memory)
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

async fn load_session_bus(state: &HttpState) -> Result<SessionBus, (StatusCode, Json<ErrorBody>)> {
    let app = state.app.lock().await;
    app.session_bus()
        .cloned()
        .ok_or_else(session_bus_unavailable)
}

fn send_request_payload(
    request: SendToSessionRequest,
) -> Result<Payload, (StatusCode, Json<ErrorBody>)> {
    match (request.text, request.payload) {
        (Some(text), None) => {
            validate_message_text(&text)?;
            Ok(Payload::Text(text))
        }
        (None, Some(payload)) => serde_json::from_value(payload).map_err(invalid_payload),
        (Some(_), Some(_)) => Err(bad_request("provide either text or payload, not both")),
        (None, None) => Err(bad_request("request body must include text or payload")),
    }
}

fn target_session_key(id: &str) -> Result<SessionKey, (StatusCode, Json<ErrorBody>)> {
    SessionKey::new(id.to_string()).map_err(|_| bad_request("session id must not be empty"))
}

fn invalid_payload(error: serde_json::Error) -> (StatusCode, Json<ErrorBody>) {
    bad_request(&format!("invalid payload: {error}"))
}

fn validate_session_memory(
    mut memory: SessionMemory,
) -> Result<SessionMemory, (StatusCode, Json<ErrorBody>)> {
    if memory.key_decisions.len() > SESSION_MEMORY_MAX_ITEMS {
        return Err(bad_request(&format!(
            "key_decisions must contain at most {SESSION_MEMORY_MAX_ITEMS} items"
        )));
    }

    if memory.custom_context.len() > SESSION_MEMORY_MAX_ITEMS {
        return Err(bad_request(&format!(
            "custom_context must contain at most {SESSION_MEMORY_MAX_ITEMS} items"
        )));
    }

    if memory.active_files.len() > SESSION_MEMORY_MAX_ITEMS {
        return Err(bad_request(&format!(
            "active_files must contain at most {SESSION_MEMORY_MAX_ITEMS} items"
        )));
    }

    let estimated_tokens = memory.estimated_tokens();
    if estimated_tokens > SESSION_MEMORY_MAX_TOKENS {
        return Err(bad_request(&format!(
            "session memory exceeds the {SESSION_MEMORY_MAX_TOKENS} token cap ({estimated_tokens} estimated)"
        )));
    }

    memory.last_updated = current_epoch_secs();
    Ok(memory)
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn bad_request(message: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            error: message.to_string(),
        }),
    )
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

fn session_bus_unavailable() -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorBody {
            error: "session bus not available".to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_session::{MessageRole as SessionMessageRole, SessionContentBlock};

    #[test]
    fn session_messages_to_context_drops_unresolved_tool_use_messages() {
        let messages = vec![
            SessionMessage::text(SessionMessageRole::User, "first", 1),
            SessionMessage::structured(
                SessionMessageRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call_good".to_string(),
                    provider_id: Some("fc_good".to_string()),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "good.txt"}),
                }],
                2,
                None,
            ),
            SessionMessage::structured(
                SessionMessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_good".to_string(),
                    content: serde_json::json!("ok"),
                    is_error: Some(false),
                }],
                3,
                None,
            ),
            SessionMessage::structured(
                SessionMessageRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call_bad".to_string(),
                    provider_id: Some("fc_bad".to_string()),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "bad.txt"}),
                }],
                4,
                None,
            ),
        ];

        let context = session_messages_to_context(&messages);

        assert_eq!(context.len(), 3);
        assert!(context
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| {
                matches!(
                    block,
                    ContentBlock::ToolUse { id, .. } if id == "call_good"
                )
            }));
        assert!(!context
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| {
                matches!(
                    block,
                    ContentBlock::ToolUse { id, .. } if id == "call_bad"
                )
            }));
    }

    #[test]
    fn validate_session_memory_rejects_too_many_active_files() {
        let memory = SessionMemory {
            active_files: (0..=SESSION_MEMORY_MAX_ITEMS)
                .map(|index| format!("file-{index}.rs"))
                .collect(),
            ..SessionMemory::default()
        };

        let error = validate_session_memory(memory).expect_err("validation should fail");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(
            error.1 .0.error,
            format!("active_files must contain at most {SESSION_MEMORY_MAX_ITEMS} items")
        );
    }
}
