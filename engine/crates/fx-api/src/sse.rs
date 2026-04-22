use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap};
use axum::response::Response;
use futures::stream;
use fx_kernel::{StreamCallback, StreamEvent};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tokio::time::{sleep_until, Instant};

pub const SSE_CHANNEL_CAPACITY: usize = 64;
pub const SSE_CLIENT_GRACE_TIMEOUT: Duration = Duration::from_secs(2);
pub const SSE_PING_INTERVAL: Duration = Duration::from_secs(15);
pub const SSE_PING_FRAME: &str = ": ping\n\n";
const SSE_REQUIRED_OVERFLOW_CAPACITY: usize = SSE_CHANNEL_CAPACITY;

/// Delivery contract enforced by SSE backpressure handling.
///
/// This is intentionally not a severity scale. Required frames all share the
/// same backpressure path: deliver them to the output stream or disconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseFrameDelivery {
    Coalescible,
    Lossless,
    Required,
}

impl SseFrameDelivery {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Coalescible => "coalescible",
            Self::Lossless => "lossless",
            Self::Required => "required",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseEventKind {
    TextPreviewDelta,
    WorkingNarrationDelta,
    TextReset,
    TextDelta,
    FinalAnswerDelta,
    Progress,
    Notification,
    ActivityStart,
    ActivityEnd,
    ActivityToolCallStart,
    ToolCallStart,
    ActivityToolCallComplete,
    ToolCallComplete,
    ActivityToolResult,
    ToolResult,
    ToolProgress,
    CompletedSummary,
    ToolError,
    PermissionPrompt,
    Phase,
    TranscriptPhaseBoundary,
    Done,
    EngineError,
    TransportError,
    ContextCompacted,
}

impl SseEventKind {
    const fn event_name(self) -> &'static str {
        match self {
            Self::TextPreviewDelta => "text_preview_delta",
            Self::WorkingNarrationDelta => "working_narration_delta",
            Self::TextReset => "text_reset",
            Self::TextDelta => "text_delta",
            Self::FinalAnswerDelta => "final_answer_delta",
            Self::Progress => "progress",
            Self::Notification => "notification",
            Self::ActivityStart => "activity_start",
            Self::ActivityEnd => "activity_end",
            Self::ActivityToolCallStart => "activity_tool_call_start",
            Self::ToolCallStart => "tool_call_start",
            Self::ActivityToolCallComplete => "activity_tool_call_complete",
            Self::ToolCallComplete => "tool_call_complete",
            Self::ActivityToolResult => "activity_tool_result",
            Self::ToolResult => "tool_result",
            Self::ToolProgress => "tool_progress",
            Self::CompletedSummary => "completed_summary",
            Self::ToolError => "tool_error",
            Self::PermissionPrompt => "permission_prompt",
            Self::Phase => "phase",
            Self::TranscriptPhaseBoundary => "phase_boundary",
            Self::Done => "done",
            Self::EngineError => "engine_error",
            Self::TransportError => "error",
            Self::ContextCompacted => "context_compacted",
        }
    }

    const fn delivery(self) -> SseFrameDelivery {
        match self {
            Self::TextDelta | Self::FinalAnswerDelta => SseFrameDelivery::Lossless,
            Self::TextPreviewDelta | Self::WorkingNarrationDelta | Self::Progress | Self::Phase => {
                SseFrameDelivery::Coalescible
            }
            Self::TextReset
            | Self::ActivityStart
            | Self::ActivityEnd
            | Self::ActivityToolCallStart
            | Self::ToolCallStart
            | Self::ActivityToolCallComplete
            | Self::ToolCallComplete
            | Self::ActivityToolResult
            | Self::ToolResult
            | Self::ToolProgress
            | Self::CompletedSummary
            | Self::TranscriptPhaseBoundary
            | Self::ContextCompacted => SseFrameDelivery::Required,
            Self::Notification
            | Self::ToolError
            | Self::PermissionPrompt
            | Self::Done
            | Self::EngineError
            | Self::TransportError => SseFrameDelivery::Required,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    kind: SseEventKind,
    body: String,
    merge_text: Option<String>,
}

impl SseFrame {
    pub fn new(kind: SseEventKind, body: String) -> Self {
        Self {
            kind,
            body,
            merge_text: None,
        }
    }

    fn text_delta(kind: SseEventKind, text: String) -> Option<Self> {
        typed_sse_body(kind, serde_json::json!({ "text": text })).map(|body| Self {
            kind,
            body,
            merge_text: Some(text),
        })
    }

    pub fn kind(&self) -> SseEventKind {
        self.kind
    }

    pub fn delivery(&self) -> SseFrameDelivery {
        self.kind.delivery()
    }

    pub fn into_body(self) -> String {
        self.body
    }

    fn try_merge_lossless_text_delta(&mut self, next: SseFrame) -> Result<(), SseFrame> {
        if self.kind != next.kind || !is_lossless_text_delta_kind(self.kind) {
            return Err(next);
        }

        let Some(existing_text) = self.merge_text.as_mut() else {
            return Err(next);
        };
        let Some(next_text) = next.merge_text else {
            return Err(SseFrame {
                kind: next.kind,
                body: next.body,
                merge_text: None,
            });
        };

        existing_text.push_str(&next_text);
        self.body = typed_sse_body(self.kind, serde_json::json!({ "text": existing_text }))
            .expect("text delta frame serializes");
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SseStreamContext {
    endpoint: &'static str,
    session_id: Option<String>,
}

impl SseStreamContext {
    pub fn new(endpoint: &'static str) -> Self {
        Self {
            endpoint,
            session_id: None,
        }
    }

    pub fn for_session(endpoint: &'static str, session_id: impl Into<String>) -> Self {
        Self {
            endpoint,
            session_id: Some(session_id.into()),
        }
    }

    fn session_id(&self) -> &str {
        self.session_id.as_deref().unwrap_or("<none>")
    }
}

/// Per-stream SSE control state.
///
/// Diagnostic counters are lifetime-scoped to this stream state. They are not
/// windowed or reset while a client connection remains alive.
#[derive(Debug)]
pub struct SseStreamState {
    disconnected: AtomicBool,
    lifetime_sent_frames: AtomicU64,
    lifetime_dropped_coalescible: AtomicU64,
    lifetime_output_backpressure_graces: AtomicU64,
    lifetime_upstream_full: AtomicU64,
    upstream_send_lock: Mutex<()>,
    upstream_required_overflow: Mutex<VecDeque<SseFrame>>,
    upstream_required_overflow_notify: Notify,
}

impl SseStreamState {
    pub fn shared() -> Arc<Self> {
        Arc::new(Self {
            disconnected: AtomicBool::new(false),
            lifetime_sent_frames: AtomicU64::new(0),
            lifetime_dropped_coalescible: AtomicU64::new(0),
            lifetime_output_backpressure_graces: AtomicU64::new(0),
            lifetime_upstream_full: AtomicU64::new(0),
            upstream_send_lock: Mutex::new(()),
            upstream_required_overflow: Mutex::new(VecDeque::new()),
            upstream_required_overflow_notify: Notify::new(),
        })
    }

    pub fn is_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::Acquire)
    }

    pub fn lifetime_sent_frames(&self) -> u64 {
        self.lifetime_sent_frames.load(Ordering::Relaxed)
    }

    pub fn lifetime_dropped_coalescible(&self) -> u64 {
        self.lifetime_dropped_coalescible.load(Ordering::Relaxed)
    }

    pub fn lifetime_output_backpressure_graces(&self) -> u64 {
        self.lifetime_output_backpressure_graces
            .load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub fn lifetime_upstream_full(&self) -> u64 {
        self.lifetime_upstream_full.load(Ordering::Relaxed)
    }

    fn disconnect(&self) {
        self.disconnected.store(true, Ordering::Release);
    }

    fn record_sent_frame(&self) -> u64 {
        self.lifetime_sent_frames.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn record_coalescible_drop(&self) -> u64 {
        self.lifetime_dropped_coalescible
            .fetch_add(1, Ordering::Relaxed)
            + 1
    }

    fn record_output_backpressure_grace(&self) -> u64 {
        self.lifetime_output_backpressure_graces
            .fetch_add(1, Ordering::Relaxed)
            + 1
    }

    fn record_upstream_full(&self) -> u64 {
        self.lifetime_upstream_full.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn enqueue_upstream_required_overflow(
        &self,
        frame: SseFrame,
    ) -> Result<RequiredOverflowPush, SseFrame> {
        let push = {
            let mut queue = self
                .upstream_required_overflow
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let frame = match try_merge_with_pending_back(&mut queue, frame) {
                Ok(()) => return Ok(RequiredOverflowPush::Coalesced),
                Err(frame) => frame,
            };
            if is_redundant_text_reset_frame(queue.back().map(SseFrame::kind), frame.kind()) {
                return Ok(RequiredOverflowPush::Coalesced);
            }
            if queue.len() >= SSE_REQUIRED_OVERFLOW_CAPACITY {
                return Err(frame);
            }
            queue.push_back(frame);
            RequiredOverflowPush::Enqueued {
                pending_required_frames: queue.len(),
            }
        };
        self.upstream_required_overflow_notify.notify_one();
        Ok(push)
    }

    fn upstream_required_overflow_len(&self) -> usize {
        self.upstream_required_overflow
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    fn enqueue_required_if_overflow_active(&self, frame: SseFrame) -> UpstreamOverflowEnqueue {
        let len = {
            let mut queue = self
                .upstream_required_overflow
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if queue.is_empty() {
                return UpstreamOverflowEnqueue::Inactive(frame);
            }
            let frame = match try_merge_with_pending_back(&mut queue, frame) {
                Ok(()) => return UpstreamOverflowEnqueue::Coalesced,
                Err(frame) => frame,
            };
            if is_redundant_text_reset_frame(queue.back().map(SseFrame::kind), frame.kind()) {
                return UpstreamOverflowEnqueue::Coalesced;
            }
            if queue.len() >= SSE_REQUIRED_OVERFLOW_CAPACITY {
                return UpstreamOverflowEnqueue::Full(frame);
            }
            queue.push_back(frame);
            queue.len()
        };
        self.upstream_required_overflow_notify.notify_one();
        UpstreamOverflowEnqueue::Buffered {
            pending_required_frames: len,
        }
    }

    fn pop_upstream_required_overflow(&self) -> Option<SseFrame> {
        self.upstream_required_overflow
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
    }
}

fn try_merge_with_pending_back(
    queue: &mut VecDeque<SseFrame>,
    frame: SseFrame,
) -> Result<(), SseFrame> {
    let Some(previous) = queue.back_mut() else {
        return Err(frame);
    };
    previous.try_merge_lossless_text_delta(frame)
}

fn is_redundant_text_reset_frame(previous: Option<SseEventKind>, next: SseEventKind) -> bool {
    // `text_reset` must be delivered if preview text may be visible, so it is a
    // required frame. Adjacent resets are idempotent, though, and can collapse
    // while a backpressure queue is already preserving required ordering.
    matches!(
        (previous, next),
        (Some(SseEventKind::TextReset), SseEventKind::TextReset)
    )
}

const fn is_lossless_text_delta_kind(kind: SseEventKind) -> bool {
    matches!(
        kind,
        SseEventKind::TextDelta | SseEventKind::FinalAnswerDelta
    )
}

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

fn typed_sse_body(kind: SseEventKind, data: serde_json::Value) -> Option<String> {
    sse_frame(kind.event_name(), data)
}

fn typed_sse_frame(kind: SseEventKind, data: serde_json::Value) -> Option<SseFrame> {
    typed_sse_body(kind, data).map(|body| SseFrame::new(kind, body))
}

#[cfg(test)]
pub fn serialize_stream_event(event: StreamEvent) -> Option<String> {
    serialize_stream_event_frame(event).map(SseFrame::into_body)
}

pub fn serialize_stream_event_frame(event: StreamEvent) -> Option<SseFrame> {
    match event {
        StreamEvent::TextPreviewDelta { text } => typed_sse_frame(
            SseEventKind::TextPreviewDelta,
            serde_json::json!({ "text": text }),
        ),
        StreamEvent::WorkingNarrationDelta {
            text,
            voiceover_suppressed,
        } => typed_sse_frame(
            SseEventKind::WorkingNarrationDelta,
            serde_json::json!({
                "text": text,
                "voiceover_suppressed": voiceover_suppressed,
            }),
        ),
        StreamEvent::TextReset => typed_sse_frame(SseEventKind::TextReset, serde_json::json!({})),
        StreamEvent::TextDelta { text } => SseFrame::text_delta(SseEventKind::TextDelta, text),
        StreamEvent::FinalAnswerDelta { text } => {
            SseFrame::text_delta(SseEventKind::FinalAnswerDelta, text)
        }
        StreamEvent::Progress { kind, message } => typed_sse_frame(
            SseEventKind::Progress,
            serde_json::json!({ "kind": kind, "message": message }),
        ),
        StreamEvent::Notification { title, body } => typed_sse_frame(
            SseEventKind::Notification,
            serde_json::json!({ "title": title, "body": body }),
        ),
        StreamEvent::ActivityStart { id, title, kind } => typed_sse_frame(
            SseEventKind::ActivityStart,
            serde_json::json!({ "id": id, "title": title, "kind": kind }),
        ),
        StreamEvent::ActivityEnd { id } => {
            typed_sse_frame(SseEventKind::ActivityEnd, serde_json::json!({ "id": id }))
        }
        StreamEvent::ActivityToolCallStart {
            activity_id,
            id,
            name,
        } => typed_sse_frame(
            SseEventKind::ActivityToolCallStart,
            serde_json::json!({ "activity_id": activity_id, "id": id, "name": name }),
        ),
        StreamEvent::ToolCallStart { id, name } => typed_sse_frame(
            SseEventKind::ToolCallStart,
            serde_json::json!({ "id": id, "name": name }),
        ),
        StreamEvent::ActivityToolCallComplete {
            activity_id,
            id,
            name,
            arguments,
        } => typed_sse_frame(
            SseEventKind::ActivityToolCallComplete,
            serde_json::json!({
                "activity_id": activity_id,
                "id": id,
                "name": name,
                "arguments": arguments,
            }),
        ),
        StreamEvent::ToolCallComplete {
            id,
            name,
            arguments,
        } => typed_sse_frame(
            SseEventKind::ToolCallComplete,
            serde_json::json!({
                "id": id,
                "name": name,
                "arguments": arguments,
            }),
        ),
        StreamEvent::ActivityToolResult {
            activity_id,
            id,
            tool_name,
            output,
            is_error,
        } => typed_sse_frame(
            SseEventKind::ActivityToolResult,
            serde_json::json!({
                "activity_id": activity_id,
                "id": id,
                "tool_name": tool_name,
                "output": output,
                "is_error": is_error,
            }),
        ),
        StreamEvent::ToolResult {
            id,
            tool_name,
            output,
            is_error,
        } => typed_sse_frame(
            SseEventKind::ToolResult,
            serde_json::json!({
                "id": id,
                "tool_name": tool_name,
                "output": output,
                "is_error": is_error,
            }),
        ),
        StreamEvent::ToolProgress {
            activity_id,
            id,
            tool_name,
            class,
            target,
            advances_slot,
            outcome,
        } => typed_sse_frame(
            SseEventKind::ToolProgress,
            serde_json::json!({
                "activity_id": activity_id,
                "id": id,
                "tool_name": tool_name,
                "class": class,
                "target": target,
                "advances_slot": advances_slot,
                "outcome": outcome,
            }),
        ),
        StreamEvent::CompletedSummary { text } => typed_sse_frame(
            SseEventKind::CompletedSummary,
            serde_json::json!({ "text": text }),
        ),
        StreamEvent::ToolError { tool_name, error } => typed_sse_frame(
            SseEventKind::ToolError,
            serde_json::json!({
                "tool_name": tool_name,
                "error": error,
            }),
        ),
        StreamEvent::PermissionPrompt(prompt) => typed_sse_frame(
            SseEventKind::PermissionPrompt,
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
            typed_sse_frame(SseEventKind::Phase, serde_json::json!({ "phase": phase }))
        }
        StreamEvent::TranscriptPhaseBoundary { phase } => typed_sse_frame(
            SseEventKind::TranscriptPhaseBoundary,
            serde_json::json!({ "phase": phase }),
        ),
        StreamEvent::Done { response } => typed_sse_frame(
            SseEventKind::Done,
            serde_json::json!({ "response": response }),
        ),
        StreamEvent::Error {
            category,
            message,
            recoverable,
        } => typed_sse_frame(
            SseEventKind::EngineError,
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
        } => typed_sse_frame(
            SseEventKind::ContextCompacted,
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

pub fn error_stream_frame(error: &str) -> SseFrame {
    let body = sse_frame("error", serde_json::json!({ "error": error }))
        .unwrap_or_else(|| "event: error\ndata: {\"error\":\"internal_error\"}\n\n".to_string());
    SseFrame::new(SseEventKind::TransportError, body)
}

/// Queue an SSE frame for the relay while the stream is connected.
///
/// Returns `true` when the stream is still alive and can continue accepting
/// future frames. It does not guarantee this specific frame was delivered:
/// coalescible frames may be dropped under backpressure and still return
/// `true`. Returns `false` only after the stream is marked disconnected.
pub fn send_sse_frame(
    sender: &mpsc::Sender<SseFrame>,
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
    frame: SseFrame,
) -> bool {
    if stream_state.is_disconnected() {
        return false;
    }

    let _send_guard = stream_state
        .upstream_send_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if stream_state.is_disconnected() {
        return false;
    }

    let frame = match try_buffer_if_upstream_overflow_active(stream_state, context, frame) {
        ActiveOverflowResult::Buffered => return true,
        ActiveOverflowResult::Disconnected => return false,
        ActiveOverflowResult::Inactive(frame) => frame,
    };

    match sender.try_send(frame) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(frame)) => match frame.delivery() {
            SseFrameDelivery::Coalescible => {
                let dropped = stream_state.record_coalescible_drop();
                if should_log_backpressure_count(dropped) {
                    tracing::warn!(
                        endpoint = context.endpoint,
                        session_id = context.session_id(),
                        event = frame.kind().event_name(),
                        delivery = frame.delivery().as_str(),
                        lifetime_sent_frames = stream_state.lifetime_sent_frames(),
                        lifetime_dropped_coalescible = dropped,
                        channel_capacity = SSE_CHANNEL_CAPACITY,
                        "dropping coalescible SSE frame before relay: client is lagging"
                    );
                }
                true
            }
            SseFrameDelivery::Lossless => {
                let upstream_full = stream_state.record_upstream_full();
                buffer_required_frame_to_overflow(stream_state, context, frame, upstream_full)
            }
            SseFrameDelivery::Required => {
                let upstream_full = stream_state.record_upstream_full();
                buffer_required_frame_to_overflow(stream_state, context, frame, upstream_full)
            }
        },
        Err(mpsc::error::TrySendError::Closed(_)) => {
            stream_state.disconnect();
            tracing::debug!(
                endpoint = context.endpoint,
                session_id = context.session_id(),
                "stopping SSE stream: client disconnected"
            );
            false
        }
    }
}

enum ActiveOverflowResult {
    Inactive(SseFrame),
    Buffered,
    Disconnected,
}

enum UpstreamOverflowEnqueue {
    Inactive(SseFrame),
    Buffered { pending_required_frames: usize },
    Coalesced,
    Full(SseFrame),
}

#[derive(Debug, PartialEq, Eq)]
enum RequiredOverflowPush {
    Enqueued { pending_required_frames: usize },
    Coalesced,
}

fn try_buffer_if_upstream_overflow_active(
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
    frame: SseFrame,
) -> ActiveOverflowResult {
    let kind = frame.kind();
    match frame.delivery() {
        SseFrameDelivery::Coalescible => {
            if stream_state.upstream_required_overflow_len() == 0 {
                ActiveOverflowResult::Inactive(frame)
            } else {
                let dropped = stream_state.record_coalescible_drop();
                if should_log_backpressure_count(dropped) {
                    tracing::warn!(
                        endpoint = context.endpoint,
                        session_id = context.session_id(),
                        event = frame.kind().event_name(),
                        delivery = frame.delivery().as_str(),
                        lifetime_sent_frames = stream_state.lifetime_sent_frames(),
                        lifetime_dropped_coalescible = dropped,
                        overflow_capacity = SSE_REQUIRED_OVERFLOW_CAPACITY,
                        "dropping coalescible SSE frame behind required overflow"
                    );
                }
                ActiveOverflowResult::Buffered
            }
        }
        SseFrameDelivery::Lossless => match stream_state.enqueue_required_if_overflow_active(frame)
        {
            UpstreamOverflowEnqueue::Buffered {
                pending_required_frames,
            } => {
                let upstream_full = stream_state.record_upstream_full();
                tracing::warn!(
                    endpoint = context.endpoint,
                    session_id = context.session_id(),
                    event = kind.event_name(),
                    delivery = SseFrameDelivery::Lossless.as_str(),
                    lifetime_sent_frames = stream_state.lifetime_sent_frames(),
                    lifetime_upstream_full = upstream_full,
                    lifetime_dropped_coalescible = stream_state.lifetime_dropped_coalescible(),
                    pending_required_frames,
                    overflow_capacity = SSE_REQUIRED_OVERFLOW_CAPACITY,
                    "buffering lossless SSE frame behind required overflow"
                );
                ActiveOverflowResult::Buffered
            }
            UpstreamOverflowEnqueue::Inactive(frame) => ActiveOverflowResult::Inactive(frame),
            UpstreamOverflowEnqueue::Coalesced => ActiveOverflowResult::Buffered,
            UpstreamOverflowEnqueue::Full(frame) => {
                let upstream_full = stream_state.record_upstream_full();
                disconnect_on_required_overflow_full(stream_state, context, frame, upstream_full);
                ActiveOverflowResult::Disconnected
            }
        },
        SseFrameDelivery::Required => match stream_state.enqueue_required_if_overflow_active(frame)
        {
            UpstreamOverflowEnqueue::Buffered {
                pending_required_frames,
            } => {
                let upstream_full = stream_state.record_upstream_full();
                tracing::warn!(
                    endpoint = context.endpoint,
                    session_id = context.session_id(),
                    event = kind.event_name(),
                    delivery = SseFrameDelivery::Required.as_str(),
                    lifetime_sent_frames = stream_state.lifetime_sent_frames(),
                    lifetime_upstream_full = upstream_full,
                    lifetime_dropped_coalescible = stream_state.lifetime_dropped_coalescible(),
                    pending_required_frames,
                    overflow_capacity = SSE_REQUIRED_OVERFLOW_CAPACITY,
                    "buffering required SSE frame behind required overflow"
                );
                ActiveOverflowResult::Buffered
            }
            UpstreamOverflowEnqueue::Inactive(frame) => ActiveOverflowResult::Inactive(frame),
            UpstreamOverflowEnqueue::Coalesced => ActiveOverflowResult::Buffered,
            UpstreamOverflowEnqueue::Full(frame) => {
                let upstream_full = stream_state.record_upstream_full();
                disconnect_on_required_overflow_full(stream_state, context, frame, upstream_full);
                ActiveOverflowResult::Disconnected
            }
        },
    }
}

fn buffer_required_frame_to_overflow(
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
    frame: SseFrame,
    upstream_full: u64,
) -> bool {
    let kind = frame.kind();
    match stream_state.enqueue_upstream_required_overflow(frame) {
        Ok(RequiredOverflowPush::Enqueued {
            pending_required_frames,
        }) => {
            tracing::warn!(
                endpoint = context.endpoint,
                session_id = context.session_id(),
                event = kind.event_name(),
                delivery = kind.delivery().as_str(),
                lifetime_sent_frames = stream_state.lifetime_sent_frames(),
                lifetime_upstream_full = upstream_full,
                lifetime_dropped_coalescible = stream_state.lifetime_dropped_coalescible(),
                pending_required_frames,
                overflow_capacity = SSE_REQUIRED_OVERFLOW_CAPACITY,
                "buffering required SSE frame: relay queue filled before required frame"
            );
            true
        }
        Ok(RequiredOverflowPush::Coalesced) => true,
        Err(frame) => {
            disconnect_on_required_overflow_full(stream_state, context, frame, upstream_full);
            false
        }
    }
}

fn disconnect_on_required_overflow_full(
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
    frame: SseFrame,
    upstream_full: u64,
) {
    stream_state.disconnect();
    tracing::warn!(
        endpoint = context.endpoint,
        session_id = context.session_id(),
        event = frame.kind().event_name(),
        delivery = frame.delivery().as_str(),
        lifetime_sent_frames = stream_state.lifetime_sent_frames(),
        lifetime_upstream_full = upstream_full,
        lifetime_dropped_coalescible = stream_state.lifetime_dropped_coalescible(),
        overflow_capacity = SSE_REQUIRED_OVERFLOW_CAPACITY,
        "stopping SSE stream: required overflow queue filled before relay"
    );
}

pub fn sse_response(
    receiver: mpsc::Receiver<SseFrame>,
    stream_state: Arc<SseStreamState>,
    context: SseStreamContext,
) -> Response {
    let (tx, rx) = mpsc::channel::<String>(SSE_CHANNEL_CAPACITY);
    tokio::spawn(ping_relay(receiver, tx, stream_state, context));
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

async fn ping_relay(
    mut data_rx: mpsc::Receiver<SseFrame>,
    tx: mpsc::Sender<String>,
    stream_state: Arc<SseStreamState>,
    context: SseStreamContext,
) {
    let mut upstream_closed = false;
    let mut pending_output = VecDeque::new();
    let mut output_grace_deadline = None;

    loop {
        if stream_state.is_disconnected() {
            break;
        }

        if !flush_pending_output(&mut pending_output, &tx, &stream_state, &context) {
            break;
        }
        if pending_output.is_empty() {
            output_grace_deadline = None;
        }

        if !upstream_closed {
            match data_rx.try_recv() {
                Ok(frame) => {
                    if !relay_frame(
                        frame,
                        &tx,
                        &stream_state,
                        &context,
                        &mut pending_output,
                        &mut output_grace_deadline,
                    ) {
                        break;
                    }
                    continue;
                }
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    upstream_closed = true;
                }
            }
        }

        if let Some(frame) = stream_state.pop_upstream_required_overflow() {
            if !relay_frame(
                frame,
                &tx,
                &stream_state,
                &context,
                &mut pending_output,
                &mut output_grace_deadline,
            ) {
                break;
            }
            continue;
        }

        if upstream_closed && pending_output.is_empty() {
            break;
        }

        if let Some(deadline) = output_grace_deadline {
            tokio::select! {
                biased;
                _ = sleep_until(deadline), if !pending_output.is_empty() => {
                    disconnect_after_output_backpressure_timeout(
                        &pending_output,
                        &stream_state,
                        &context,
                    );
                    break;
                }
                permit = tx.reserve(), if !pending_output.is_empty() => {
                    match permit {
                        Ok(permit) => {
                            let pending = pending_output
                                .pop_front()
                                .expect("pending output exists when permit is reserved");
                            permit.send(pending.into_body());
                            stream_state.record_sent_frame();
                        }
                        Err(_) => {
                            stream_state.disconnect();
                            tracing::debug!(
                                endpoint = context.endpoint,
                                session_id = context.session_id(),
                                "stopping SSE stream: client disconnected"
                            );
                            break;
                        }
                    }
                }
                frame = data_rx.recv(), if !upstream_closed => {
                    match frame {
                        Some(frame) => {
                            if !relay_frame(
                                frame,
                                &tx,
                                &stream_state,
                                &context,
                                &mut pending_output,
                                &mut output_grace_deadline,
                            ) {
                                break;
                            }
                        }
                        None => {
                            upstream_closed = true;
                        }
                    }
                }
                _ = stream_state.upstream_required_overflow_notify.notified() => {}
            }
        } else {
            tokio::select! {
                biased;
                frame = data_rx.recv(), if !upstream_closed => {
                    match frame {
                        Some(frame) => {
                            if !relay_frame(
                                frame,
                                &tx,
                                &stream_state,
                                &context,
                                &mut pending_output,
                                &mut output_grace_deadline,
                            ) {
                                break;
                            }
                        }
                        None => {
                            upstream_closed = true;
                        }
                    }
                }
                _ = stream_state.upstream_required_overflow_notify.notified() => {}
                _ = tokio::time::sleep(SSE_PING_INTERVAL) => {
                    if !relay_ping(&tx, &stream_state, &context) {
                        break;
                    }
                }
            }
        }
    }
}

fn relay_frame(
    frame: SseFrame,
    tx: &mpsc::Sender<String>,
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
    pending_output: &mut VecDeque<SseFrame>,
    output_grace_deadline: &mut Option<Instant>,
) -> bool {
    let kind = frame.kind();
    let delivery = frame.delivery();

    if !pending_output.is_empty() {
        return relay_frame_behind_pending_output(frame, stream_state, context, pending_output);
    }

    match tx.try_send(frame.body.clone()) {
        Ok(()) => {
            stream_state.record_sent_frame();
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            stream_state.disconnect();
            tracing::debug!(
                endpoint = context.endpoint,
                session_id = context.session_id(),
                "stopping SSE stream: client disconnected"
            );
            false
        }
        Err(mpsc::error::TrySendError::Full(_)) => match delivery {
            SseFrameDelivery::Coalescible => {
                let dropped = stream_state.record_coalescible_drop();
                if should_log_backpressure_count(dropped) {
                    tracing::warn!(
                        endpoint = context.endpoint,
                        session_id = context.session_id(),
                        event = kind.event_name(),
                        delivery = delivery.as_str(),
                        lifetime_sent_frames = stream_state.lifetime_sent_frames(),
                        lifetime_dropped_coalescible = dropped,
                        channel_capacity = SSE_CHANNEL_CAPACITY,
                        "dropping coalescible SSE frame: client is lagging"
                    );
                }
                true
            }
            SseFrameDelivery::Lossless => start_output_backpressure_grace(
                frame,
                stream_state,
                context,
                pending_output,
                output_grace_deadline,
            ),
            SseFrameDelivery::Required => start_output_backpressure_grace(
                frame,
                stream_state,
                context,
                pending_output,
                output_grace_deadline,
            ),
        },
    }
}

fn relay_frame_behind_pending_output(
    frame: SseFrame,
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
    pending_output: &mut VecDeque<SseFrame>,
) -> bool {
    let kind = frame.kind();
    let delivery = frame.delivery();
    match delivery {
        SseFrameDelivery::Coalescible => {
            let dropped = stream_state.record_coalescible_drop();
            if should_log_backpressure_count(dropped) {
                tracing::warn!(
                    endpoint = context.endpoint,
                    session_id = context.session_id(),
                    event = kind.event_name(),
                    delivery = delivery.as_str(),
                    lifetime_sent_frames = stream_state.lifetime_sent_frames(),
                    lifetime_dropped_coalescible = dropped,
                    pending_required_frames = pending_output.len(),
                    "dropping coalescible SSE frame behind pending required output"
                );
            }
            true
        }
        SseFrameDelivery::Lossless => {
            queue_pending_required_output(frame, stream_state, context, pending_output)
        }
        SseFrameDelivery::Required => {
            queue_pending_required_output(frame, stream_state, context, pending_output)
        }
    }
}

fn start_output_backpressure_grace(
    frame: SseFrame,
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
    pending_output: &mut VecDeque<SseFrame>,
    output_grace_deadline: &mut Option<Instant>,
) -> bool {
    let kind = frame.kind();
    let delivery = frame.delivery();
    if !queue_pending_required_output(frame, stream_state, context, pending_output) {
        return false;
    }

    let grace_count = stream_state.record_output_backpressure_grace();
    *output_grace_deadline = Some(Instant::now() + SSE_CLIENT_GRACE_TIMEOUT);
    tracing::warn!(
        endpoint = context.endpoint,
        session_id = context.session_id(),
        event = kind.event_name(),
        delivery = delivery.as_str(),
        lifetime_sent_frames = stream_state.lifetime_sent_frames(),
        lifetime_output_backpressure_graces = grace_count,
        lifetime_dropped_coalescible = stream_state.lifetime_dropped_coalescible(),
        pending_required_frames = pending_output.len(),
        timeout_ms = SSE_CLIENT_GRACE_TIMEOUT.as_millis() as u64,
        "SSE client backpressure before required frame; starting shared grace period"
    );
    true
}

fn queue_pending_required_output(
    frame: SseFrame,
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
    pending_output: &mut VecDeque<SseFrame>,
) -> bool {
    let kind = frame.kind();
    let delivery = frame.delivery();
    let frame = match try_merge_with_pending_back(pending_output, frame) {
        Ok(()) => return true,
        Err(frame) => frame,
    };
    if is_redundant_text_reset_frame(pending_output.back().map(SseFrame::kind), kind) {
        return true;
    }

    if pending_output.len() >= SSE_REQUIRED_OVERFLOW_CAPACITY {
        stream_state.disconnect();
        tracing::warn!(
            endpoint = context.endpoint,
            session_id = context.session_id(),
            event = kind.event_name(),
            delivery = delivery.as_str(),
            lifetime_sent_frames = stream_state.lifetime_sent_frames(),
            lifetime_output_backpressure_graces =
                stream_state.lifetime_output_backpressure_graces(),
            lifetime_dropped_coalescible = stream_state.lifetime_dropped_coalescible(),
            pending_capacity = SSE_REQUIRED_OVERFLOW_CAPACITY,
            "stopping SSE stream: pending required output queue filled"
        );
        return false;
    }

    pending_output.push_back(frame);
    true
}

fn flush_pending_output(
    pending_output: &mut VecDeque<SseFrame>,
    tx: &mpsc::Sender<String>,
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
) -> bool {
    while let Some(pending) = pending_output.pop_front() {
        match tx.try_send(pending.body.clone()) {
            Ok(()) => {
                stream_state.record_sent_frame();
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                pending_output.push_front(pending);
                return true;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                stream_state.disconnect();
                tracing::debug!(
                    endpoint = context.endpoint,
                    session_id = context.session_id(),
                    "stopping SSE stream: client disconnected"
                );
                return false;
            }
        }
    }
    true
}

fn disconnect_after_output_backpressure_timeout(
    pending_output: &VecDeque<SseFrame>,
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
) {
    stream_state.disconnect();
    let event = pending_output
        .front()
        .map(|pending| pending.kind().event_name())
        .unwrap_or("<none>");
    tracing::warn!(
        endpoint = context.endpoint,
        session_id = context.session_id(),
        event,
        delivery = SseFrameDelivery::Required.as_str(),
        lifetime_sent_frames = stream_state.lifetime_sent_frames(),
        lifetime_dropped_coalescible = stream_state.lifetime_dropped_coalescible(),
        lifetime_output_backpressure_graces = stream_state.lifetime_output_backpressure_graces(),
        pending_required_frames = pending_output.len(),
        timeout_ms = SSE_CLIENT_GRACE_TIMEOUT.as_millis() as u64,
        "stopping SSE stream: client remained too slow after shared grace period"
    );
}

fn relay_ping(
    tx: &mpsc::Sender<String>,
    stream_state: &Arc<SseStreamState>,
    context: &SseStreamContext,
) -> bool {
    match tx.try_send(SSE_PING_FRAME.to_string()) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::debug!(
                endpoint = context.endpoint,
                session_id = context.session_id(),
                "dropping SSE ping: client is lagging"
            );
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            stream_state.disconnect();
            tracing::debug!(
                endpoint = context.endpoint,
                session_id = context.session_id(),
                "stopping SSE stream: client disconnected"
            );
            false
        }
    }
}

const fn should_log_backpressure_count(count: u64) -> bool {
    count == 1 || count.is_multiple_of(64)
}

pub fn stream_callback(
    sender: mpsc::Sender<SseFrame>,
    stream_state: Arc<SseStreamState>,
    context: SseStreamContext,
) -> StreamCallback {
    Arc::new(move |event| {
        let Some(frame) = serialize_stream_event_frame(event) else {
            return;
        };
        let _ = send_sse_frame(&sender, &stream_state, &context, frame);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::task::yield_now;
    use tokio::time::{advance, pause};

    fn test_context() -> SseStreamContext {
        SseStreamContext::new("test")
    }

    fn text_frame(body: &str) -> SseFrame {
        SseFrame::new(SseEventKind::TextDelta, body.to_string())
    }

    fn text_preview_frame(body: &str) -> SseFrame {
        SseFrame::new(SseEventKind::TextPreviewDelta, body.to_string())
    }

    fn working_narration_frame(body: &str) -> SseFrame {
        SseFrame::new(SseEventKind::WorkingNarrationDelta, body.to_string())
    }

    fn final_answer_frame(body: &str) -> SseFrame {
        SseFrame::new(SseEventKind::FinalAnswerDelta, body.to_string())
    }

    fn text_reset_frame() -> SseFrame {
        SseFrame::new(SseEventKind::TextReset, "{}".to_string())
    }

    fn done_frame(body: &str) -> SseFrame {
        SseFrame::new(SseEventKind::Done, body.to_string())
    }

    fn tool_call_start_frame(body: &str) -> SseFrame {
        SseFrame::new(SseEventKind::ToolCallStart, body.to_string())
    }

    fn tool_call_complete_frame(body: &str) -> SseFrame {
        SseFrame::new(SseEventKind::ToolCallComplete, body.to_string())
    }

    fn activity_start_frame(body: &str) -> SseFrame {
        SseFrame::new(SseEventKind::ActivityStart, body.to_string())
    }

    #[test]
    fn delivery_contract_is_backpressure_policy_not_severity() {
        assert_eq!(
            text_frame("coalescible").delivery(),
            SseFrameDelivery::Lossless
        );
        assert_eq!(
            text_preview_frame("preview").delivery(),
            SseFrameDelivery::Coalescible
        );
        assert_eq!(
            working_narration_frame("narrating").delivery(),
            SseFrameDelivery::Coalescible
        );
        assert_eq!(
            final_answer_frame("answer").delivery(),
            SseFrameDelivery::Lossless
        );
        assert_eq!(text_reset_frame().delivery(), SseFrameDelivery::Required);
        assert_eq!(
            activity_start_frame("activity").delivery(),
            SseFrameDelivery::Required
        );
        assert_eq!(
            tool_call_start_frame("tool").delivery(),
            SseFrameDelivery::Required
        );
        assert_eq!(done_frame("done").delivery(), SseFrameDelivery::Required);
    }

    #[test]
    fn adjacent_text_resets_coalesce_inside_required_overflow_queue() {
        let stream_state = SseStreamState::shared();

        assert_eq!(
            stream_state
                .enqueue_upstream_required_overflow(text_reset_frame())
                .expect("first reset enqueued"),
            RequiredOverflowPush::Enqueued {
                pending_required_frames: 1
            }
        );
        assert_eq!(
            stream_state
                .enqueue_upstream_required_overflow(text_reset_frame())
                .expect("second reset coalesced"),
            RequiredOverflowPush::Coalesced
        );

        assert_eq!(stream_state.upstream_required_overflow_len(), 1);
    }

    #[test]
    fn adjacent_text_resets_coalesce_inside_pending_output_queue() {
        let stream_state = SseStreamState::shared();
        let mut pending_output = VecDeque::new();

        assert!(queue_pending_required_output(
            text_reset_frame(),
            &stream_state,
            &test_context(),
            &mut pending_output,
        ));
        assert!(queue_pending_required_output(
            text_reset_frame(),
            &stream_state,
            &test_context(),
            &mut pending_output,
        ));

        assert_eq!(pending_output.len(), 1);
    }

    #[tokio::test]
    async fn ping_sent_during_silence() {
        pause();
        let (_data_tx, data_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(1);
        let stream_state = SseStreamState::shared();
        tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            Arc::clone(&stream_state),
            test_context(),
        ));
        yield_now().await;

        advance(SSE_PING_INTERVAL).await;
        yield_now().await;

        assert_eq!(output_rx.recv().await, Some(SSE_PING_FRAME.to_string()));
        assert_eq!(stream_state.lifetime_sent_frames(), 0);
    }

    #[tokio::test]
    async fn ping_not_sent_while_data_flows() {
        pause();
        let (data_tx, data_rx) = mpsc::channel(4);
        let (output_tx, mut output_rx) = mpsc::channel(4);
        tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            SseStreamState::shared(),
            test_context(),
        ));
        yield_now().await;

        data_tx
            .send(text_frame("frame-1"))
            .await
            .expect("send frame-1");
        assert_eq!(output_rx.recv().await, Some("frame-1".to_string()));

        advance(Duration::from_secs(14)).await;
        yield_now().await;
        assert!(output_rx.try_recv().is_err());

        data_tx
            .send(text_frame("frame-2"))
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
        let relay = tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            SseStreamState::shared(),
            test_context(),
        ));

        drop(data_tx);
        yield_now().await;
        relay.await.expect("relay task");

        assert_eq!(output_rx.recv().await, None);
    }

    #[tokio::test]
    async fn relay_drops_coalescible_frames_when_output_is_full() {
        let (data_tx, data_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(1);
        output_tx
            .send("occupied".to_string())
            .await
            .expect("fill output channel");
        let stream_state = SseStreamState::shared();
        tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            Arc::clone(&stream_state),
            test_context(),
        ));

        data_tx
            .send(text_preview_frame("coalescible"))
            .await
            .expect("send coalescible frame");
        yield_now().await;

        assert_eq!(stream_state.lifetime_dropped_coalescible(), 1);
        assert_eq!(stream_state.lifetime_sent_frames(), 0);
        assert_eq!(output_rx.try_recv().expect("queued frame"), "occupied");
        assert!(output_rx.try_recv().is_err());
        assert!(!stream_state.is_disconnected());
    }

    #[tokio::test]
    async fn relay_merges_lossless_final_answer_deltas_when_output_is_full() {
        let (data_tx, data_rx) = mpsc::channel(4);
        let (output_tx, mut output_rx) = mpsc::channel(1);
        output_tx
            .send("occupied".to_string())
            .await
            .expect("fill output channel");
        let stream_state = SseStreamState::shared();
        tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            Arc::clone(&stream_state),
            test_context(),
        ));

        data_tx
            .send(
                serialize_stream_event_frame(StreamEvent::FinalAnswerDelta {
                    text: "Hel".to_string(),
                })
                .expect("first final delta"),
            )
            .await
            .expect("send first final delta");
        data_tx
            .send(
                serialize_stream_event_frame(StreamEvent::FinalAnswerDelta {
                    text: "lo".to_string(),
                })
                .expect("second final delta"),
            )
            .await
            .expect("send second final delta");
        yield_now().await;
        yield_now().await;

        assert_eq!(stream_state.lifetime_output_backpressure_graces(), 1);
        assert_eq!(stream_state.lifetime_dropped_coalescible(), 0);
        assert_eq!(output_rx.recv().await, Some("occupied".to_string()));
        assert_eq!(
            output_rx.recv().await,
            Some("event: final_answer_delta\ndata: {\"text\":\"Hello\"}\n\n".to_string())
        );
        assert_eq!(stream_state.lifetime_sent_frames(), 1);
        assert!(!stream_state.is_disconnected());
    }

    #[tokio::test]
    async fn send_sse_frame_merges_lossless_final_answer_deltas_when_upstream_is_full() {
        let (data_tx, data_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(4);
        let stream_state = SseStreamState::shared();
        let context = test_context();

        assert!(send_sse_frame(
            &data_tx,
            &stream_state,
            &context,
            serialize_stream_event_frame(StreamEvent::FinalAnswerDelta {
                text: "Hel".to_string(),
            })
            .expect("first final delta"),
        ));
        assert!(send_sse_frame(
            &data_tx,
            &stream_state,
            &context,
            serialize_stream_event_frame(StreamEvent::FinalAnswerDelta {
                text: "lo".to_string(),
            })
            .expect("second final delta"),
        ));
        assert!(send_sse_frame(
            &data_tx,
            &stream_state,
            &context,
            serialize_stream_event_frame(StreamEvent::FinalAnswerDelta {
                text: "!".to_string(),
            })
            .expect("third final delta"),
        ));

        tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            Arc::clone(&stream_state),
            context,
        ));

        assert_eq!(
            output_rx.recv().await,
            Some("event: final_answer_delta\ndata: {\"text\":\"Hel\"}\n\n".to_string())
        );
        assert_eq!(
            output_rx.recv().await,
            Some("event: final_answer_delta\ndata: {\"text\":\"lo!\"}\n\n".to_string())
        );
        assert_eq!(stream_state.lifetime_upstream_full(), 1);
        assert_eq!(stream_state.lifetime_dropped_coalescible(), 0);
        assert!(!stream_state.is_disconnected());
    }

    #[tokio::test]
    async fn relay_waits_for_required_frames_when_output_is_full() {
        let (data_tx, data_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(1);
        output_tx
            .send("occupied".to_string())
            .await
            .expect("fill output channel");
        let stream_state = SseStreamState::shared();
        tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            Arc::clone(&stream_state),
            test_context(),
        ));

        data_tx
            .send(done_frame("required"))
            .await
            .expect("send required frame");
        yield_now().await;

        assert_eq!(stream_state.lifetime_output_backpressure_graces(), 1);
        assert_eq!(output_rx.recv().await, Some("occupied".to_string()));
        assert_eq!(output_rx.recv().await, Some("required".to_string()));
        assert_eq!(stream_state.lifetime_sent_frames(), 1);
        assert!(!stream_state.is_disconnected());
    }

    #[tokio::test]
    async fn relay_required_burst_shares_one_output_grace_period() {
        let (data_tx, data_rx) = mpsc::channel(4);
        let (output_tx, mut output_rx) = mpsc::channel(1);
        output_tx
            .send("occupied".to_string())
            .await
            .expect("fill output channel");
        let stream_state = SseStreamState::shared();
        tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            Arc::clone(&stream_state),
            test_context(),
        ));

        data_tx
            .send(tool_call_start_frame("start"))
            .await
            .expect("send start");
        data_tx
            .send(tool_call_complete_frame("complete"))
            .await
            .expect("send complete");
        data_tx.send(done_frame("done")).await.expect("send done");
        yield_now().await;
        yield_now().await;

        assert_eq!(stream_state.lifetime_output_backpressure_graces(), 1);
        assert_eq!(output_rx.recv().await, Some("occupied".to_string()));
        assert_eq!(output_rx.recv().await, Some("start".to_string()));
        assert_eq!(output_rx.recv().await, Some("complete".to_string()));
        assert_eq!(output_rx.recv().await, Some("done".to_string()));
        assert_eq!(stream_state.lifetime_output_backpressure_graces(), 1);
        assert_eq!(stream_state.lifetime_sent_frames(), 3);
        assert!(!stream_state.is_disconnected());
    }

    #[tokio::test]
    async fn send_sse_frame_buffers_required_upstream_overflow_for_relay() {
        let (data_tx, data_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(4);
        let stream_state = SseStreamState::shared();
        let context = test_context();

        assert!(send_sse_frame(
            &data_tx,
            &stream_state,
            &context,
            text_frame("first")
        ));
        assert!(send_sse_frame(
            &data_tx,
            &stream_state,
            &context,
            done_frame("required-overflow")
        ));
        assert!(!stream_state.is_disconnected());
        assert_eq!(stream_state.lifetime_upstream_full(), 1);

        tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            Arc::clone(&stream_state),
            context,
        ));

        assert_eq!(output_rx.recv().await, Some("first".to_string()));
        assert_eq!(
            output_rx.recv().await,
            Some("required-overflow".to_string())
        );
        assert_eq!(stream_state.lifetime_sent_frames(), 2);
        assert!(!stream_state.is_disconnected());
    }

    #[test]
    fn send_sse_frame_disconnects_when_required_upstream_overflow_capacity_is_exceeded() {
        let (data_tx, _data_rx) = mpsc::channel(1);
        let stream_state = SseStreamState::shared();
        let context = test_context();

        assert!(send_sse_frame(
            &data_tx,
            &stream_state,
            &context,
            text_frame("first")
        ));

        for index in 0..SSE_REQUIRED_OVERFLOW_CAPACITY {
            assert!(send_sse_frame(
                &data_tx,
                &stream_state,
                &context,
                done_frame(&format!("required-{index}"))
            ));
        }
        assert!(!stream_state.is_disconnected());

        assert!(!send_sse_frame(
            &data_tx,
            &stream_state,
            &context,
            done_frame("required-overflow-capacity")
        ));
        assert!(stream_state.is_disconnected());
        assert_eq!(
            stream_state.lifetime_upstream_full(),
            (SSE_REQUIRED_OVERFLOW_CAPACITY + 1) as u64
        );
    }

    #[test]
    fn pending_required_output_capacity_disconnects_without_counting_grace() {
        let stream_state = SseStreamState::shared();
        let context = test_context();
        let mut pending_output = (0..SSE_REQUIRED_OVERFLOW_CAPACITY)
            .map(|index| done_frame(&format!("pending-{index}")))
            .collect::<VecDeque<_>>();
        let mut output_grace_deadline = None;

        assert!(!start_output_backpressure_grace(
            done_frame("overflow"),
            &stream_state,
            &context,
            &mut pending_output,
            &mut output_grace_deadline,
        ));

        assert!(stream_state.is_disconnected());
        assert_eq!(stream_state.lifetime_output_backpressure_graces(), 0);
        assert!(output_grace_deadline.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn relay_disconnects_required_frame_after_shared_grace_timeout() {
        let (data_tx, data_rx) = mpsc::channel(1);
        let (output_tx, mut output_rx) = mpsc::channel(1);
        output_tx
            .send("occupied".to_string())
            .await
            .expect("fill output channel");
        let stream_state = SseStreamState::shared();
        let relay = tokio::spawn(ping_relay(
            data_rx,
            output_tx,
            Arc::clone(&stream_state),
            test_context(),
        ));

        data_tx
            .send(done_frame("required"))
            .await
            .expect("send required frame");
        yield_now().await;
        assert_eq!(stream_state.lifetime_output_backpressure_graces(), 1);

        advance(SSE_CLIENT_GRACE_TIMEOUT).await;
        yield_now().await;

        relay.await.expect("relay task");
        assert!(stream_state.is_disconnected());
        assert_eq!(stream_state.lifetime_sent_frames(), 0);
        assert_eq!(output_rx.try_recv().expect("queued frame"), "occupied");
        assert!(output_rx.try_recv().is_err());
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
