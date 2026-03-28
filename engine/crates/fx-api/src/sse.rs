use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap};
use axum::response::Response;
use futures::stream;
use fx_kernel::{StreamCallback, StreamEvent};
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

pub const SSE_CHANNEL_CAPACITY: usize = 64;
pub const SSE_PING_INTERVAL: Duration = Duration::from_secs(15);
pub const SSE_PING_FRAME: &str = ": ping\n\n";

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
        StreamEvent::Progress { kind, message } => sse_frame(
            "progress",
            serde_json::json!({ "kind": kind, "message": message }),
        ),
        StreamEvent::Notification { title, body } => sse_frame(
            "notification",
            serde_json::json!({ "title": title, "body": body }),
        ),
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
            tool_name,
            output,
            is_error,
        } => sse_frame(
            "tool_result",
            serde_json::json!({
                "id": id,
                "tool_name": tool_name,
                "output": output,
                "is_error": is_error,
            }),
        ),
        StreamEvent::ToolError { tool_name, error } => sse_frame(
            "tool_error",
            serde_json::json!({
                "tool_name": tool_name,
                "error": error,
            }),
        ),
        StreamEvent::PermissionPrompt(prompt) => sse_frame(
            "permission_prompt",
            serde_json::json!({
                "id": prompt.id,
                "tool": prompt.tool,
                "title": prompt.title,
                "reason": prompt.reason,
                "request_summary": prompt.request_summary,
                "session_scoped_allow_available": prompt.session_scoped_allow_available,
                "expires_at": prompt.expires_at,
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
        StreamEvent::ContextCompacted {
            tier,
            messages_removed,
            tokens_before,
            tokens_after,
            usage_ratio,
        } => sse_frame(
            "context_compacted",
            serde_json::json!({
                "tier": tier,
                "messages_removed": messages_removed,
                "tokens_before": tokens_before,
                "tokens_after": tokens_after,
                "usage_ratio": usage_ratio,
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
    let (tx, rx) = mpsc::channel::<String>(SSE_CHANNEL_CAPACITY);
    tokio::spawn(ping_relay(receiver, tx));
    let body_stream = stream::unfold(rx, |mut receiver| async move {
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

async fn ping_relay(mut data_rx: mpsc::Receiver<String>, tx: mpsc::Sender<String>) {
    loop {
        tokio::select! {
            biased;
            frame = data_rx.recv() => {
                match frame {
                    Some(frame) => {
                        if tx.send(frame).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            _ = tokio::time::sleep(SSE_PING_INTERVAL) => {
                if tx.send(SSE_PING_FRAME.to_string()).await.is_err() {
                    break;
                }
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::task::yield_now;
    use tokio::time::{advance, pause};

    #[tokio::test]
    async fn ping_sent_during_silence() {
        pause();
        let (_data_tx, data_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(1);
        tokio::spawn(ping_relay(data_rx, output_tx));
        yield_now().await;

        advance(SSE_PING_INTERVAL).await;
        yield_now().await;

        assert_eq!(output_rx.recv().await, Some(SSE_PING_FRAME.to_string()));
    }

    #[tokio::test]
    async fn ping_not_sent_while_data_flows() {
        pause();
        let (data_tx, data_rx) = mpsc::channel(4);
        let (output_tx, mut output_rx) = mpsc::channel(4);
        tokio::spawn(ping_relay(data_rx, output_tx));
        yield_now().await;

        data_tx
            .send("frame-1".to_string())
            .await
            .expect("send frame-1");
        assert_eq!(output_rx.recv().await, Some("frame-1".to_string()));

        advance(Duration::from_secs(14)).await;
        yield_now().await;
        assert!(output_rx.try_recv().is_err());

        data_tx
            .send("frame-2".to_string())
            .await
            .expect("send frame-2");
        assert_eq!(output_rx.recv().await, Some("frame-2".to_string()));

        advance(Duration::from_secs(14)).await;
        yield_now().await;
        assert!(output_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn stream_ends_when_sender_drops() {
        pause();
        let (data_tx, data_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(1);
        let relay = tokio::spawn(ping_relay(data_rx, output_tx));

        drop(data_tx);
        yield_now().await;
        relay.await.expect("relay task");

        assert_eq!(output_rx.recv().await, None);
    }

    #[test]
    fn permission_prompt_event_serializes() {
        let frame =
            serialize_stream_event(StreamEvent::PermissionPrompt(fx_kernel::PermissionPrompt {
                id: "prompt-1".to_string(),
                tool: "shell".to_string(),
                title: "Allow shell command".to_string(),
                reason: "Needed to inspect the repo".to_string(),
                request_summary: "git status --short --branch".to_string(),
                session_scoped_allow_available: true,
                expires_at: 1_742_000_000,
            }))
            .expect("permission prompt frame");

        assert_eq!(
            frame,
            "event: permission_prompt\ndata: {\"expires_at\":1742000000,\"id\":\"prompt-1\",\"reason\":\"Needed to inspect the repo\",\"request_summary\":\"git status --short --branch\",\"session_scoped_allow_available\":true,\"title\":\"Allow shell command\",\"tool\":\"shell\"}\n\n"
        );
    }

    #[test]
    fn tool_error_event_serializes() {
        let frame = serialize_stream_event(StreamEvent::ToolError {
            tool_name: "read_file".to_string(),
            error: "permission denied".to_string(),
        })
        .expect("tool error frame");

        assert_eq!(
            frame,
            "event: tool_error\ndata: {\"error\":\"permission denied\",\"tool_name\":\"read_file\"}\n\n"
        );
    }

    #[test]
    fn progress_event_serializes() {
        let frame = serialize_stream_event(StreamEvent::Progress {
            kind: fx_core::message::ProgressKind::Implementing,
            message: "Implementing the committed plan.".to_string(),
        })
        .expect("progress frame");

        assert_eq!(
            frame,
            "event: progress\ndata: {\"kind\":\"implementing\",\"message\":\"Implementing the committed plan.\"}\n\n"
        );
    }

    #[test]
    fn context_compacted_event_serializes() {
        let frame = serialize_stream_event(StreamEvent::ContextCompacted {
            tier: "slide".to_string(),
            messages_removed: 12,
            tokens_before: 5_100,
            tokens_after: 2_900,
            usage_ratio: 0.42,
        })
        .expect("context compacted frame");

        assert_eq!(
            frame,
            "event: context_compacted\ndata: {\"messages_removed\":12,\"tier\":\"slide\",\"tokens_after\":2900,\"tokens_before\":5100,\"usage_ratio\":0.42}\n\n"
        );
    }
}
