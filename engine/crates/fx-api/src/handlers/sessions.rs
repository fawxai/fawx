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
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{TimeZone, Utc};
use fx_bus::{Envelope, Payload, SessionBus};
use fx_core::channel::ResponseContext;
use fx_core::types::InputSource;
use fx_kernel::StreamCallback;
use fx_llm::{trim_conversation_history, Message};
use fx_session::{
    prune_unresolved_tool_history, render_content_blocks_with_options, validate_tool_message_order,
    ContentRenderOptions, SessionArchiveFilter, SessionConfig, SessionError, SessionHistoryError,
    SessionInfo, SessionKey, SessionKind, SessionMemory, SessionMessage, SessionRegistry,
    SessionStatus,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use uuid::Uuid;

// Coarse API validation gate. Session-level dynamic caps enforce the real limit.
const SESSION_MEMORY_MAX_ITEMS: usize = 80;
const SESSION_MEMORY_MAX_TOKENS: usize = 8_000;

struct TurnInput<'a> {
    message: Cow<'a, str>,
    images: Cow<'a, [EncodedImage]>,
    documents: Cow<'a, [EncodedDocument]>,
    context: Vec<Message>,
}

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
    #[serde(default)]
    pub archived: Option<String>,
}

impl ListSessionsQuery {
    fn archive_filter(&self) -> Result<SessionArchiveFilter, (StatusCode, Json<ErrorBody>)> {
        ArchivedQueryValue::parse(self.archived.as_deref()).map(SessionArchiveFilter::from)
    }
}

#[derive(Debug, Deserialize)]
pub struct SessionMessagesQuery {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SessionExportQuery {
    #[serde(default)]
    pub format: Option<String>,
}

impl SessionExportQuery {
    fn export_format(&self) -> Result<SessionExportFormat, (StatusCode, Json<ErrorBody>)> {
        SessionExportFormat::parse(self.format.as_deref())
    }
}

#[derive(Debug, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionSummaryResponse>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct SessionMessagesResponse {
    pub messages: Vec<SessionMessage>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct SessionExportResponse {
    pub key: String,
    pub session: SessionExportSessionMetadata,
    pub archive: SessionArchiveMetadata,
    pub messages: Vec<SessionMessage>,
    pub total_messages: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionArchiveMetadata {
    pub archived: bool,
    pub archived_at: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct SessionSummaryResponse {
    pub key: String,
    pub kind: SessionKind,
    pub status: SessionStatus,
    pub label: Option<String>,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub model: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub message_count: usize,
    #[serde(flatten)]
    pub archive: SessionArchiveMetadata,
}

#[derive(Debug, Serialize)]
pub struct SessionExportSessionMetadata {
    pub kind: SessionKind,
    pub status: SessionStatus,
    pub label: Option<String>,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub model: String,
    pub created_at: u64,
    pub updated_at: u64,
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

struct SessionExportData {
    info: SessionInfo,
    messages: Vec<SessionMessage>,
}

#[derive(Debug, Clone, Copy)]
enum TimestampDisplay {
    Minute,
    Second,
}

#[derive(Debug, Clone, Copy)]
enum ArchivedQueryValue {
    Active,
    All,
    Only,
}

#[derive(Debug, Clone, Copy)]
enum SessionExportFormat {
    Text,
    Json,
}

impl SessionExportFormat {
    fn parse(value: Option<&str>) -> Result<Self, (StatusCode, Json<ErrorBody>)> {
        match value.unwrap_or("text") {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            other => Err(invalid_export_format(other)),
        }
    }
}

impl ArchivedQueryValue {
    fn parse(value: Option<&str>) -> Result<Self, (StatusCode, Json<ErrorBody>)> {
        match value.unwrap_or("active") {
            "active" => Ok(Self::Active),
            "all" => Ok(Self::All),
            "only" => Ok(Self::Only),
            other => Err(invalid_archive_filter(other)),
        }
    }
}

impl From<ArchivedQueryValue> for SessionArchiveFilter {
    fn from(value: ArchivedQueryValue) -> Self {
        match value {
            ArchivedQueryValue::Active => Self::ActiveOnly,
            ArchivedQueryValue::All => Self::All,
            ArchivedQueryValue::Only => Self::ArchivedOnly,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ArchiveRouteOperation {
    Archive,
    Unarchive,
}

impl ArchiveRouteOperation {
    fn apply(self, registry: &SessionRegistry, key: &SessionKey) -> Result<(), SessionError> {
        match self {
            Self::Archive => registry.archive(key),
            Self::Unarchive => registry.unarchive(key),
        }
    }
}

impl From<&SessionInfo> for SessionArchiveMetadata {
    fn from(info: &SessionInfo) -> Self {
        Self {
            archived: info.is_archived(),
            archived_at: info.archived_at,
        }
    }
}

impl From<SessionInfo> for SessionSummaryResponse {
    fn from(info: SessionInfo) -> Self {
        let archive = SessionArchiveMetadata::from(&info);
        Self {
            key: info.key.to_string(),
            kind: info.kind,
            status: info.status,
            label: info.label,
            title: info.title,
            preview: info.preview,
            model: info.model,
            created_at: info.created_at,
            updated_at: info.updated_at,
            message_count: info.message_count,
            archive,
        }
    }
}

impl From<&SessionInfo> for SessionExportSessionMetadata {
    fn from(info: &SessionInfo) -> Self {
        Self {
            kind: info.kind,
            status: info.status,
            label: info.label.clone(),
            title: info.title.clone(),
            preview: info.preview.clone(),
            model: info.model.clone(),
            created_at: info.created_at,
            updated_at: info.updated_at,
        }
    }
}

impl SessionExportData {
    fn into_json_payload(self) -> SessionExportResponse {
        let total_messages = self.messages.len();
        SessionExportResponse {
            key: self.info.key.to_string(),
            session: SessionExportSessionMetadata::from(&self.info),
            archive: SessionArchiveMetadata::from(&self.info),
            messages: self.messages,
            total_messages,
        }
    }
}

struct StreamingSessionMessageTask {
    state: HttpState,
    registry: SessionRegistry,
    key: SessionKey,
    input: TurnInput<'static>,
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
    Ok((
        StatusCode::CREATED,
        Json(SessionSummaryResponse::from(info)),
    )
        .into_response())
}

pub async fn handle_list_sessions(
    State(state): State<HttpState>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let archive_filter = query.archive_filter()?;
    let mut sessions = registry
        .list_with_archive_filter(query.kind, archive_filter)
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;
    sessions.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.key.as_str().cmp(right.key.as_str()))
    });
    let total = sessions.len();
    sessions.truncate(query.limit.unwrap_or(50));
    let sessions = sessions
        .into_iter()
        .map(SessionSummaryResponse::from)
        .collect();

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
    Ok(Json(SessionSummaryResponse::from(info)).into_response())
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
    let context = session_messages_to_context(&history)
        .map_err(|error| map_session_history_error(&id, error))?;
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

pub async fn handle_archive_session(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    update_session_archive_state(state, id, ArchiveRouteOperation::Archive).await
}

pub async fn handle_unarchive_session(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    update_session_archive_state(state, id, ArchiveRouteOperation::Unarchive).await
}

pub async fn handle_export_session(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Query(query): Query<SessionExportQuery>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let export = load_session_export(&registry, &id)?;
    Ok(render_session_export_response(
        export,
        query.export_format()?,
    ))
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
    let mut context = session_messages_to_context(&history)
        .map_err(|error| map_session_history_error(&id, error))?;
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
            TurnInput {
                message: Cow::Owned(request.message),
                images: Cow::Owned(images),
                documents: Cow::Owned(documents),
                context,
            },
        )
        .await);
    }

    let (result, response, session_messages, session_memory) = process_and_route_session_message(
        &state,
        &registry,
        &key,
        TurnInput {
            message: Cow::Borrowed(request.message.as_str()),
            images: Cow::Borrowed(&images),
            documents: Cow::Borrowed(&documents),
            context,
        },
    )
    .await
    .map_err(internal_error)?;
    persist_session_turn(&registry, &key, session_messages, session_memory)
        .map_err(|error| map_session_error(&id, error))?;

    Ok(Json(MessageResponse {
        response,
        model: result.model,
        iterations: result.iterations,
        result_kind: result.result_kind,
    })
    .into_response())
}

pub(crate) fn session_messages_to_context(
    messages: &[SessionMessage],
) -> Result<Vec<Message>, SessionHistoryError> {
    validate_tool_message_order(messages)?;
    let replay_safe = prune_unresolved_tool_history(messages);
    let context = replay_safe
        .iter()
        .map(SessionMessage::to_llm_message)
        .collect();
    Ok(context)
}

async fn stream_session_message_response(
    state: HttpState,
    registry: SessionRegistry,
    key: SessionKey,
    input: TurnInput<'static>,
) -> Response {
    let (sender, receiver) = mpsc::channel(SSE_CHANNEL_CAPACITY);
    let disconnected = Arc::new(AtomicBool::new(false));
    tokio::spawn(run_streaming_session_message_task(
        StreamingSessionMessageTask {
            state,
            registry,
            key,
            input,
            sender,
            disconnected,
        },
    ));
    sse_response(receiver)
}

async fn run_streaming_session_message_task(task: StreamingSessionMessageTask) {
    let StreamingSessionMessageTask {
        state,
        registry,
        key,
        input,
        sender,
        disconnected,
    } = task;
    let callback = stream_callback(sender.clone(), Arc::clone(&disconnected));
    let result = execute_session_turn(&state, &registry, &key, input, Some(callback)).await;

    match result {
        Ok((_result, session_messages, session_memory)) => {
            if let Err(error) =
                persist_session_turn(&registry, &key, session_messages, session_memory)
            {
                let _ = send_sse_frame(
                    &sender,
                    &disconnected,
                    error_stream_frame(&error.to_string()),
                );
            }
        }
        Err(error) => {
            let _ = send_sse_frame(
                &sender,
                &disconnected,
                error_stream_frame(&error.to_string()),
            );
        }
    }
}

async fn process_and_route_session_message(
    state: &HttpState,
    registry: &SessionRegistry,
    key: &SessionKey,
    input: TurnInput<'_>,
) -> Result<(CycleResult, String, Vec<SessionMessage>, SessionMemory), anyhow::Error> {
    let (result, session_messages, session_memory) =
        execute_session_turn(state, registry, key, input, None).await?;

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

async fn execute_session_turn(
    state: &HttpState,
    registry: &SessionRegistry,
    key: &SessionKey,
    input: TurnInput<'_>,
    callback: Option<StreamCallback>,
) -> Result<(CycleResult, Vec<SessionMessage>, SessionMemory), anyhow::Error> {
    let loaded_memory = registry.memory(key).map_err(anyhow::Error::new)?;
    let mut app = state.app.lock().await;
    let previous_memory = app.replace_session_memory(loaded_memory);
    let outcome = app
        .process_message_with_context(
            input.message.as_ref(),
            encoded_images_to_attachments(input.images.as_ref()),
            encoded_documents_to_attachments(input.documents.as_ref()),
            input.context,
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

async fn update_session_archive_state(
    state: HttpState,
    id: String,
    operation: ArchiveRouteOperation,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    operation
        .apply(&registry, &key)
        .map_err(|error| map_session_error(&id, error))?;
    let info = registry
        .get_info(&key)
        .map_err(|error| map_session_error(&id, error))?;
    Ok(Json(SessionSummaryResponse::from(info)).into_response())
}

fn load_session_export(
    registry: &SessionRegistry,
    id: &str,
) -> Result<SessionExportData, (StatusCode, Json<ErrorBody>)> {
    let key = session_key(id)?;
    let info = registry
        .get_info(&key)
        .map_err(|error| map_session_error(id, error))?;
    let messages = registry
        .history(&key, info.message_count)
        .map_err(|error| map_session_error(id, error))?;
    Ok(SessionExportData { info, messages })
}

fn render_session_export_response(
    export: SessionExportData,
    format: SessionExportFormat,
) -> Response {
    match format {
        SessionExportFormat::Json => Json(export.into_json_payload()).into_response(),
        SessionExportFormat::Text => text_export_response(render_session_export_text(&export)),
    }
}

fn text_export_response(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

fn render_session_export_text(export: &SessionExportData) -> String {
    let mut output = format!(
        "Session: {}\nKind: {} | Status: {} | Model: {}\nCreated: {} | Updated: {}\n{}\nMessages: {}\n---\n",
        export.info.key,
        export.info.kind,
        export.info.status,
        export.info.model,
        format_export_timestamp(export.info.created_at, TimestampDisplay::Minute),
        format_export_timestamp(export.info.updated_at, TimestampDisplay::Minute),
        format_archive_line(&export.info),
        export.info.message_count,
    );
    if export.messages.is_empty() {
        return output;
    }
    let blocks = export
        .messages
        .iter()
        .map(format_export_message)
        .collect::<Vec<_>>()
        .join("\n\n");
    output.push('\n');
    output.push_str(&blocks);
    output.push('\n');
    output
}

fn format_archive_line(info: &SessionInfo) -> String {
    match info.archived_at {
        Some(timestamp) => {
            format!(
                "Archived: yes | Archived at: {}",
                format_export_timestamp(timestamp, TimestampDisplay::Minute)
            )
        }
        None => "Archived: no".to_string(),
    }
}

fn format_export_message(message: &SessionMessage) -> String {
    format!(
        "[{}] {}{}\n{}",
        message.role,
        format_export_timestamp(message.timestamp, TimestampDisplay::Second),
        format_export_token_suffix(message),
        render_content_blocks_with_options(
            &message.content,
            ContentRenderOptions {
                include_tool_use_id: true,
            },
        )
    )
}

fn format_export_token_suffix(message: &SessionMessage) -> String {
    match (
        message.total_token_count(),
        message.input_token_count,
        message.output_token_count,
    ) {
        (Some(total), Some(input), Some(output)) => {
            format!(" | {total} tokens ({input} in / {output} out)")
        }
        (Some(total), _, _) => format!(" | {total} tokens"),
        (None, _, _) => String::new(),
    }
}

fn format_export_timestamp(timestamp: u64, display: TimestampDisplay) -> String {
    let (pattern, fallback) = match display {
        TimestampDisplay::Minute => ("%Y-%m-%d %H:%M", "1970-01-01 00:00"),
        TimestampDisplay::Second => ("%Y-%m-%d %H:%M:%S", "1970-01-01 00:00:00"),
    };
    format_timestamp(timestamp, pattern, fallback)
}

fn format_timestamp(timestamp: u64, pattern: &str, fallback: &str) -> String {
    match Utc.timestamp_opt(timestamp as i64, 0).single() {
        Some(value) => value.format(pattern).to_string(),
        None => fallback.to_string(),
    }
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

fn invalid_archive_filter(value: &str) -> (StatusCode, Json<ErrorBody>) {
    bad_request(&format!(
        "invalid archived filter '{value}'; expected one of: active, all, only"
    ))
}

fn invalid_export_format(value: &str) -> (StatusCode, Json<ErrorBody>) {
    bad_request(&format!(
        "invalid export format '{value}'; expected one of: text, json"
    ))
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
        SessionError::Corrupted { source, .. } => corrupted_session(id, &source),
        SessionError::InvalidHistory(source) => corrupted_session(id, &source),
        other => internal_error(anyhow::Error::new(other)),
    }
}

fn map_session_history_error(
    id: &str,
    error: SessionHistoryError,
) -> (StatusCode, Json<ErrorBody>) {
    corrupted_session(id, &error)
}

fn session_not_found(id: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("session not found: {id}"),
        }),
    )
}

fn corrupted_session(id: &str, error: &SessionHistoryError) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::CONFLICT,
        Json(ErrorBody {
            error: format!("corrupted session '{id}': {error}"),
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

        let context = session_messages_to_context(&messages).expect("valid context");

        assert_eq!(context.len(), 3);
        assert!(context
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| {
                matches!(
                    block,
                    fx_llm::ContentBlock::ToolUse { id, .. } if id == "call_good"
                )
            }));
        assert!(!context
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| {
                matches!(
                    block,
                    fx_llm::ContentBlock::ToolUse { id, .. } if id == "call_bad"
                )
            }));
    }

    #[test]
    fn session_messages_to_context_rejects_poisoned_tool_ordering() {
        let messages = vec![
            SessionMessage::text(SessionMessageRole::User, "first", 1),
            SessionMessage::structured(
                SessionMessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_bad".to_string(),
                    content: serde_json::json!("bad"),
                    is_error: Some(false),
                }],
                2,
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
                3,
                None,
            ),
            SessionMessage::structured(
                SessionMessageRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call_good".to_string(),
                    provider_id: Some("fc_good".to_string()),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "good.txt"}),
                }],
                4,
                None,
            ),
            SessionMessage::structured(
                SessionMessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_good".to_string(),
                    content: serde_json::json!("ok"),
                    is_error: Some(false),
                }],
                5,
                None,
            ),
        ];

        assert_eq!(
            session_messages_to_context(&messages),
            Err(SessionHistoryError::ToolResultBeforeToolUse {
                tool_use_id: "call_bad".to_string(),
                message_index: 1,
                block_index: 0,
            })
        );
    }

    #[test]
    fn validate_session_memory_accepts_maximum_dynamic_item_cap() {
        let mut memory = SessionMemory::default();
        memory.active_files = (0..SESSION_MEMORY_MAX_ITEMS)
            .map(|index| format!("file-{index}.rs"))
            .collect();

        let validated = validate_session_memory(memory).expect("validation should pass");

        assert_eq!(validated.active_files.len(), SESSION_MEMORY_MAX_ITEMS);
    }

    #[test]
    fn validate_session_memory_accepts_maximum_dynamic_token_cap() {
        let mut memory = SessionMemory::default();
        memory.project = Some("a ".repeat(7_900).trim_end().to_string());

        let estimated_tokens = memory.estimated_tokens();
        assert!(estimated_tokens > 4_000);
        assert!(estimated_tokens <= SESSION_MEMORY_MAX_TOKENS);

        let validated = validate_session_memory(memory).expect("validation should pass");

        assert_eq!(validated.estimated_tokens(), estimated_tokens);
    }

    #[test]
    fn validate_session_memory_rejects_too_many_active_files() {
        let mut memory = SessionMemory::default();
        memory.active_files = (0..=SESSION_MEMORY_MAX_ITEMS)
            .map(|index| format!("file-{index}.rs"))
            .collect();

        let error = validate_session_memory(memory).expect_err("validation should fail");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(
            error.1 .0.error,
            format!("active_files must contain at most {SESSION_MEMORY_MAX_ITEMS} items")
        );
    }
}
