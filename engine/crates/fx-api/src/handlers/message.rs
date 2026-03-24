use crate::engine::{AppEngine, CycleResult};
use crate::handlers::sessions::handle_send_message_for_session;
use crate::sse::{
    error_stream_frame, send_sse_frame, sse_response, stream_callback, wants_sse,
    SSE_CHANNEL_CAPACITY,
};
use crate::state::HttpState;
use crate::types::{
    DocumentPayload, EncodedDocument, EncodedImage, ErrorBody, ImagePayload, MessageRequest,
    MessageResponse,
};
use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use fx_core::channel::ResponseContext;
use fx_core::types::InputSource;
use fx_kernel::ResponseRouter;
use fx_llm::{DocumentAttachment, ImageAttachment};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

const SUPPORTED_IMAGE_MEDIA_TYPES: &[&str] =
    &["image/jpeg", "image/png", "image/gif", "image/webp"];
const SUPPORTED_DOCUMENT_MEDIA_TYPES: &[&str] = &["application/pdf"];
const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;

pub async fn stream_message_response(
    state: HttpState,
    message: String,
    images: Vec<EncodedImage>,
    documents: Vec<EncodedDocument>,
) -> Response {
    let (sender, receiver) = mpsc::channel(SSE_CHANNEL_CAPACITY);
    let disconnected = Arc::new(AtomicBool::new(false));
    tokio::spawn(run_streaming_message_task(
        state,
        message,
        images,
        documents,
        sender,
        disconnected,
    ));
    sse_response(receiver)
}

pub async fn run_streaming_message_task(
    state: HttpState,
    message: String,
    images: Vec<EncodedImage>,
    documents: Vec<EncodedDocument>,
    sender: mpsc::Sender<String>,
    disconnected: Arc<AtomicBool>,
) {
    let callback = stream_callback(sender.clone(), Arc::clone(&disconnected));
    let result = {
        let mut app = state.app.lock().await;
        app.process_message(
            &message,
            encoded_images_to_attachments(&images),
            encoded_documents_to_attachments(&documents),
            InputSource::Http,
            Some(callback),
        )
        .await
    };
    if let Err(error) = result {
        let _ = send_sse_frame(
            &sender,
            &disconnected,
            error_stream_frame(&error.to_string()),
        );
    }
}

pub async fn process_and_route_message(
    app: &Arc<Mutex<dyn AppEngine>>,
    router: &ResponseRouter,
    text: &str,
    images: Vec<EncodedImage>,
    documents: Vec<EncodedDocument>,
    source: InputSource,
    context: ResponseContext,
) -> Result<CycleResult, anyhow::Error> {
    let mut guard = app.lock().await;
    let result = run_message_cycle(&mut *guard, text, &images, &documents, &source).await?;
    router
        .route(&source, &result.response, &context)
        .map_err(|error| anyhow::anyhow!("response routing failed: {error}"))?;
    Ok(result)
}

pub async fn run_message_cycle(
    app: &mut dyn AppEngine,
    text: &str,
    images: &[EncodedImage],
    documents: &[EncodedDocument],
    source: &InputSource,
) -> Result<CycleResult, anyhow::Error> {
    app.process_message(
        text,
        encoded_images_to_attachments(images),
        encoded_documents_to_attachments(documents),
        source.clone(),
        None,
    )
    .await
}

pub(crate) fn encoded_images_to_attachments(images: &[EncodedImage]) -> Vec<ImageAttachment> {
    images
        .iter()
        .map(|image| ImageAttachment {
            media_type: image.media_type.clone(),
            data: image.base64_data.clone(),
        })
        .collect()
}

pub(crate) fn encoded_documents_to_attachments(
    documents: &[EncodedDocument],
) -> Vec<DocumentAttachment> {
    documents
        .iter()
        .map(|document| DocumentAttachment {
            media_type: document.media_type.clone(),
            data: document.base64_data.clone(),
            filename: document.filename.clone(),
        })
        .collect()
}

pub(crate) fn validate_message_text(message: &str) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    if message.trim().is_empty() {
        return Err(bad_request("message must not be empty"));
    }
    Ok(())
}

pub(crate) fn validate_message_request(
    message: &str,
    image_count: usize,
    document_count: usize,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    if message.trim().is_empty() && image_count == 0 && document_count == 0 {
        return Err(bad_request("message must not be empty"));
    }

    Ok(())
}

pub(crate) fn validate_and_encode_images(
    images: &[ImagePayload],
) -> Result<Vec<EncodedImage>, (StatusCode, Json<ErrorBody>)> {
    images
        .iter()
        .enumerate()
        .map(|(index, image)| {
            let media_type = image.media_type.trim();
            if !SUPPORTED_IMAGE_MEDIA_TYPES.contains(&media_type) {
                return Err(bad_request(format!(
                    "unsupported image media type: {media_type}"
                )));
            }
            let base64_data = image.data.trim();
            base64::engine::general_purpose::STANDARD
                .decode(base64_data)
                .map_err(|_| bad_request(format!("invalid base64 in image at index {index}")))?;

            Ok(EncodedImage {
                media_type: media_type.to_string(),
                base64_data: base64_data.to_string(),
            })
        })
        .collect()
}

pub(crate) fn validate_and_encode_documents(
    documents: &[DocumentPayload],
) -> Result<Vec<EncodedDocument>, (StatusCode, Json<ErrorBody>)> {
    documents
        .iter()
        .enumerate()
        .map(|(index, document)| {
            let media_type = document.media_type.trim();
            if !SUPPORTED_DOCUMENT_MEDIA_TYPES.contains(&media_type) {
                return Err(bad_request(format!(
                    "unsupported document media type: {media_type}"
                )));
            }

            let base64_data = document.data.trim();
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(base64_data)
                .map_err(|_| bad_request(format!("invalid base64 in document at index {index}")))?;
            if decoded.len() > MAX_DOCUMENT_BYTES {
                return Err(bad_request(format!(
                    "document at index {index} exceeds the 10MB limit"
                )));
            }

            Ok(EncodedDocument {
                media_type: media_type.to_string(),
                base64_data: base64_data.to_string(),
                filename: document
                    .filename
                    .as_ref()
                    .map(|filename| filename.trim().to_string())
                    .filter(|filename| !filename.is_empty()),
            })
        })
        .collect()
}

pub async fn handle_message(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(request): Json<MessageRequest>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    if let Some(session_id) = request.session_id.clone() {
        return handle_send_message_for_session(state, headers, session_id, request).await;
    }

    validate_message_request(
        &request.message,
        request.images.len(),
        request.documents.len(),
    )?;
    let images = validate_and_encode_images(&request.images)?;
    let documents = validate_and_encode_documents(&request.documents)?;

    if wants_sse(&headers) {
        return Ok(stream_message_response(state, request.message, images, documents).await);
    }

    let result = process_and_route_message(
        &state.app,
        state.channels.router.as_ref(),
        &request.message,
        images,
        documents,
        InputSource::Http,
        ResponseContext::default(),
    )
    .await
    .map_err(internal_error)?;
    let response = state
        .channels
        .http
        .take_response()
        .unwrap_or_else(|| result.response.clone());

    Ok(Json(MessageResponse {
        response,
        model: result.model,
        iterations: result.iterations,
        result_kind: result.result_kind,
    })
    .into_response())
}

pub(crate) fn internal_error(error: anyhow::Error) -> (StatusCode, Json<ErrorBody>) {
    tracing::error!(error = %error, "message processing failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            error: "internal_error".to_string(),
        }),
    )
}

fn bad_request(error: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            error: error.into(),
        }),
    )
}
