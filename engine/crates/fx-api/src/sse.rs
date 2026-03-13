use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap};
use axum::response::Response;
use futures::stream;
use fx_kernel::{StreamCallback, StreamEvent};
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

pub const SSE_CHANNEL_CAPACITY: usize = 64;

pub fn wants_sse(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|part| part.trim().starts_with("text/event-stream"))
        })
}

pub fn sse_frame(event: &str, data: serde_json::Value) -> Option<String> {
    serde_json::to_string(&data)
        .ok()
        .map(|data| format!("event: {event}\ndata: {data}\n\n"))
}

pub fn serialize_stream_event(event: StreamEvent) -> Option<String> {
    match event {
        StreamEvent::TextDelta { text } => {
            sse_frame("text_delta", serde_json::json!({ "text": text }))
        }
        StreamEvent::ToolCallStart { id, name } => sse_frame(
            "tool_call_start",
            serde_json::json!({ "id": id, "name": name }),
        ),
        StreamEvent::ToolCallComplete {
            id,
            name,
            arguments,
        } => sse_frame(
            "tool_call_complete",
            serde_json::json!({
                "id": id,
                "name": name,
                "arguments": arguments,
            }),
        ),
        StreamEvent::ToolResult {
            id,
            output,
            is_error,
        } => sse_frame(
            "tool_result",
            serde_json::json!({
                "id": id,
                "output": output,
                "is_error": is_error,
            }),
        ),
        StreamEvent::PhaseChange { phase } => {
            sse_frame("phase", serde_json::json!({ "phase": phase }))
        }
        StreamEvent::Done { response } => {
            sse_frame("done", serde_json::json!({ "response": response }))
        }
        StreamEvent::Error {
            category,
            message,
            recoverable,
        } => sse_frame(
            "engine_error",
            serde_json::json!({
                "category": category,
                "message": message,
                "recoverable": recoverable,
            }),
        ),
    }
}

pub fn error_stream_frame(error: &str) -> String {
    sse_frame("error", serde_json::json!({ "error": error }))
        .unwrap_or_else(|| "event: error\ndata: {\"error\":\"internal_error\"}\n\n".to_string())
}

pub fn send_sse_frame(
    sender: &mpsc::Sender<String>,
    disconnected: &Arc<AtomicBool>,
    frame: String,
) -> bool {
    if disconnected.load(Ordering::Relaxed) {
        return false;
    }

    match sender.try_send(frame) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            disconnected.store(true, Ordering::Relaxed);
            tracing::warn!("stopping SSE stream: client is too slow");
            false
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            disconnected.store(true, Ordering::Relaxed);
            tracing::debug!("stopping SSE stream: client disconnected");
            false
        }
    }
}

pub fn sse_response(receiver: mpsc::Receiver<String>) -> Response {
    let body_stream = stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|chunk| {
            let bytes = Ok::<Bytes, Infallible>(Bytes::from(chunk));
            (bytes, receiver)
        })
    });
    let mut response = Response::new(Body::from_stream(body_stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/event-stream"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache"),
    );
    response.headers_mut().insert(
        header::CONNECTION,
        header::HeaderValue::from_static("keep-alive"),
    );
    response
}

pub fn stream_callback(
    sender: mpsc::Sender<String>,
    disconnected: Arc<AtomicBool>,
) -> StreamCallback {
    Arc::new(move |event| {
        let Some(frame) = serialize_stream_event(event) else {
            return;
        };
        let _ = send_sse_frame(&sender, &disconnected, frame);
    })
}
