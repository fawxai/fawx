use super::tool_call_normalization::{
    normalize_completion_response as apply_tool_call_normalization,
    CompletionResponseNormalization, ToolCallNormalizationOutcome,
    ToolCallNormalizationSignalMetadata,
};
use super::{loop_error, merge_usage, signal_metadata_value, CycleStream, LlmProvider, LoopEngine};
use crate::signals::{ControlPlaneDecisionKind, LoopStep, SignalKind};
use crate::streaming::TranscriptTurnPhase;
use crate::streaming::{ErrorCategory, StreamCallback, StreamEvent};
use crate::types::LoopError;
use futures_util::StreamExt;
use fx_core::message::{InternalMessage, StreamPhase};
use fx_llm::{
    CompletionRequest, CompletionResponse, CompletionStream, ContentBlock, ProviderError,
    StreamCallback as ProviderStreamCallback, StreamChunk, StreamEvent as ProviderStreamEvent,
    ToolCall, ToolDefinition, ToolUseDelta, Usage,
};
use std::collections::HashMap;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

pub(super) type StreamCallbackRef<'a> = Option<&'a StreamCallback>;
type SharedBufferedDeltas = Arc<Mutex<Vec<String>>>;
type SharedPreviewTextSent = Arc<AtomicBool>;
const DEFAULT_PROVIDER_STREAM_CHUNK_IDLE_TIMEOUT: Duration = Duration::from_secs(180);
const PROVIDER_STREAM_CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(250);
const PROVIDER_STREAM_CHUNK_IDLE_TIMEOUT_ENV: &str = "FAWX_PROVIDER_STREAM_CHUNK_IDLE_TIMEOUT_SECS";

#[derive(Clone, Copy)]
struct StreamingCompletionContext<'a> {
    buffered_deltas: Option<&'a SharedBufferedDeltas>,
    preview_text_sent: Option<&'a SharedPreviewTextSent>,
    callback: &'a StreamCallback,
    event_bus: Option<&'a fx_core::EventBus>,
    request: StreamingRequestContext,
}

impl StreamingCompletionContext<'_> {
    fn stream_context(&self) -> StreamConsumeContext<'_> {
        StreamConsumeContext {
            event_bus: self.event_bus,
            step: self.request.step,
            phase: self.request.phase,
            text_visibility: self.request.text_visibility,
        }
    }
}

#[derive(Clone, Copy)]
struct StreamConsumeContext<'a> {
    event_bus: Option<&'a fx_core::EventBus>,
    step: LoopStep,
    phase: StreamPhase,
    text_visibility: TextStreamVisibility,
}

#[derive(Debug, Default)]
struct StreamConsumptionState {
    response: StreamResponseState,
    buffered_deltas: Vec<String>,
    should_buffer_deltas: bool,
    preview_text_sent: bool,
}

impl StreamConsumptionState {
    fn new(phase: StreamPhase) -> Self {
        Self {
            response: StreamResponseState::default(),
            buffered_deltas: Vec::new(),
            should_buffer_deltas: buffer_phase_text_until_response(phase),
            preview_text_sent: false,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct StreamingRequestContext {
    step: LoopStep,
    phase: StreamPhase,
    text_visibility: TextStreamVisibility,
    commit_preview_as_final: bool,
}

impl StreamingRequestContext {
    pub(super) fn new(
        step: LoopStep,
        phase: StreamPhase,
        text_visibility: TextStreamVisibility,
    ) -> Self {
        Self {
            step,
            phase,
            text_visibility,
            commit_preview_as_final: true,
        }
    }

    pub(super) fn with_preview_final_commit(mut self, enabled: bool) -> Self {
        self.commit_preview_as_final = enabled;
        self
    }

    pub(super) fn stage(self) -> &'static str {
        self.step.to_label()
    }
}

#[derive(serde::Serialize)]
struct CostSignalMetadata<'a> {
    stage: &'static str,
    model: &'a str,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cache_creation_input_tokens: u64,
    total_tokens: u64,
}

pub(super) fn buffer_phase_text_until_response(phase: StreamPhase) -> bool {
    matches!(phase, StreamPhase::Reason | StreamPhase::Synthesize)
}

fn shared_buffered_deltas(phase: StreamPhase) -> Option<SharedBufferedDeltas> {
    buffer_phase_text_until_response(phase).then(|| Arc::new(Mutex::new(Vec::new())))
}

fn shared_preview_text_sent(visibility: TextStreamVisibility) -> Option<SharedPreviewTextSent> {
    matches!(visibility, TextStreamVisibility::Preview).then(|| Arc::new(AtomicBool::new(false)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TextStreamVisibility {
    Preview,
    Public,
    Hidden,
}

enum StreamRead {
    Chunk(Result<StreamChunk, ProviderError>),
    End,
    Cancelled(CompletionResponse),
}

fn emit_phase_text_delta(
    callback: StreamCallbackRef<'_>,
    event_bus: Option<&fx_core::EventBus>,
    visibility: TextStreamVisibility,
    phase: StreamPhase,
    text: String,
) {
    if matches!(visibility, TextStreamVisibility::Hidden) {
        return;
    }
    let event = match visibility {
        TextStreamVisibility::Preview => StreamEvent::TextPreviewDelta { text: text.clone() },
        TextStreamVisibility::Public => StreamEvent::FinalAnswerDelta { text: text.clone() },
        TextStreamVisibility::Hidden => return,
    };
    if let Some(bus) = event_bus {
        let _ = bus.publish(InternalMessage::StreamDelta {
            delta: text.clone(),
            phase,
        });
    }
    if let Some(callback) = callback {
        callback(event);
    }
}

fn reset_preview_text(callback: StreamCallbackRef<'_>) {
    if let Some(callback) = callback {
        callback(StreamEvent::TextReset);
    }
}

fn reset_preview_text_if_sent(
    callback: StreamCallbackRef<'_>,
    preview_text_sent: Option<&SharedPreviewTextSent>,
) {
    if preview_text_sent.is_some_and(|sent| sent.load(Ordering::Acquire)) {
        reset_preview_text(callback);
    }
}

fn preview_text_must_be_cleared(normalized: &CompletionResponseNormalization) -> bool {
    normalized.outcome.suppresses_buffered_text() || !normalized.response.tool_calls.is_empty()
}

fn preview_text_can_commit_as_final_answer(
    normalized: &CompletionResponseNormalization,
    request: StreamingRequestContext,
) -> bool {
    normalized.response.tool_calls.is_empty()
        && request.commit_preview_as_final
        && matches!(request.text_visibility, TextStreamVisibility::Preview)
        && matches!(request.step, LoopStep::Reason)
}

fn flush_phase_text_deltas(
    buffered_deltas: &mut Vec<String>,
    callback: StreamCallbackRef<'_>,
    event_bus: Option<&fx_core::EventBus>,
    visibility: TextStreamVisibility,
    phase: StreamPhase,
) {
    for delta in buffered_deltas.drain(..) {
        emit_phase_text_delta(callback, event_bus, visibility, phase, delta);
    }
}

fn flush_shared_phase_text_deltas(
    buffered_deltas: &SharedBufferedDeltas,
    callback: StreamCallbackRef<'_>,
    event_bus: Option<&fx_core::EventBus>,
    visibility: TextStreamVisibility,
    phase: StreamPhase,
) {
    let mut deltas = {
        let mut guard = buffered_deltas
            .lock()
            .expect("buffered stream deltas lock poisoned");
        std::mem::take(&mut *guard)
    };
    flush_phase_text_deltas(&mut deltas, callback, event_bus, visibility, phase);
}

fn emit_final_answer_text(
    response: &CompletionResponse,
    callback: StreamCallbackRef<'_>,
    event_bus: Option<&fx_core::EventBus>,
    phase: StreamPhase,
) {
    let Some(text) = completion_response_text(response) else {
        return;
    };
    emit_phase_text_delta(
        callback,
        event_bus,
        TextStreamVisibility::Public,
        phase,
        text,
    );
}

fn completion_response_text(response: &CompletionResponse) -> Option<String> {
    let text = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            ContentBlock::ToolUse { .. }
            | ContentBlock::ToolResult { .. }
            | ContentBlock::Image { .. }
            | ContentBlock::Document { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return None;
    }
    Some(text)
}

fn provider_stream_bridge(
    callback: StreamCallback,
    event_bus: Option<fx_core::EventBus>,
    visibility: TextStreamVisibility,
    phase: StreamPhase,
    buffered_deltas: Option<SharedBufferedDeltas>,
    preview_text_sent: Option<SharedPreviewTextSent>,
) -> ProviderStreamCallback {
    Arc::new(move |event| {
        if let ProviderStreamEvent::TextDelta { text } = event {
            if let Some(buffered_deltas) = &buffered_deltas {
                // Preview text is emitted immediately for responsiveness, but
                // kept buffered until normalization proves it is safe to
                // commit. Tool-intent text clears the preview instead.
                buffered_deltas
                    .lock()
                    .expect("buffered stream deltas lock poisoned")
                    .push(text.clone());
                if matches!(visibility, TextStreamVisibility::Preview) {
                    if !text.is_empty() {
                        if let Some(sent) = &preview_text_sent {
                            sent.store(true, Ordering::Release);
                        }
                    }
                    emit_phase_text_delta(
                        Some(&callback),
                        event_bus.as_ref(),
                        visibility,
                        phase,
                        text,
                    );
                }
            } else {
                emit_phase_text_delta(Some(&callback), event_bus.as_ref(), visibility, phase, text);
            }
        }
    })
}

#[derive(Debug, Clone, Default)]
pub(super) struct StreamToolCallState {
    pub(super) id: Option<String>,
    pub(super) provider_id: Option<String>,
    pub(super) name: Option<String>,
    pub(super) arguments: String,
    pub(super) arguments_done: bool,
}

#[derive(Debug, Default)]
pub(super) struct StreamResponseState {
    text: String,
    usage: Option<Usage>,
    stop_reason: Option<String>,
    tool_calls_by_index: HashMap<usize, StreamToolCallState>,
    id_to_index: HashMap<String, usize>,
}

impl StreamResponseState {
    pub(super) fn apply_chunk(&mut self, chunk: StreamChunk) {
        if let Some(delta) = chunk.delta_content {
            self.text.push_str(&delta);
        }
        self.usage = merge_usage(self.usage, chunk.usage);
        self.stop_reason = chunk.stop_reason.or(self.stop_reason.take());
        self.apply_tool_deltas(chunk.tool_use_deltas);
    }

    fn apply_tool_deltas(&mut self, deltas: Vec<ToolUseDelta>) {
        for (chunk_index, delta) in deltas.into_iter().enumerate() {
            let index = stream_tool_index(
                chunk_index,
                &delta,
                &self.tool_calls_by_index,
                &self.id_to_index,
            );
            let entry = self.tool_calls_by_index.entry(index).or_default();
            merge_stream_tool_delta(entry, delta, &mut self.id_to_index, index);
        }
    }

    pub(super) fn into_response(self) -> CompletionResponse {
        let finalized_tools = finalize_stream_tool_payloads(self.tool_calls_by_index);
        let mut content = Vec::with_capacity(
            usize::from(!self.text.is_empty()).saturating_add(finalized_tools.len()),
        );
        if !self.text.is_empty() {
            content.push(ContentBlock::Text { text: self.text });
        }
        content.extend(finalized_tools.iter().map(|tool| ContentBlock::ToolUse {
            id: tool.call.id.clone(),
            provider_id: tool.provider_id.clone(),
            name: tool.call.name.clone(),
            input: tool.call.arguments.clone(),
        }));
        CompletionResponse {
            content,
            tool_calls: finalized_tools.into_iter().map(|tool| tool.call).collect(),
            usage: self.usage,
            stop_reason: self.stop_reason,
        }
    }

    fn into_cancelled_response(self) -> CompletionResponse {
        let content = if self.text.is_empty() {
            Vec::new()
        } else {
            vec![ContentBlock::Text { text: self.text }]
        };
        CompletionResponse {
            content,
            tool_calls: Vec::new(),
            usage: self.usage,
            stop_reason: Some("cancelled".to_string()),
        }
    }
}

impl LoopEngine {
    pub(super) async fn request_completion(
        &mut self,
        llm: &dyn LlmProvider,
        mut request: CompletionRequest,
        context: StreamingRequestContext,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        self.apply_turn_steer_to_request(&mut request, context);
        match context.text_visibility {
            TextStreamVisibility::Preview => {
                self.emit_transcript_phase_boundary(stream, TranscriptTurnPhase::CollectingWork)
            }
            TextStreamVisibility::Hidden if context.step == LoopStep::Synthesize => {
                self.emit_transcript_phase_boundary(stream, TranscriptTurnPhase::Summarizing)
            }
            TextStreamVisibility::Public => {
                self.emit_transcript_phase_boundary(stream, TranscriptTurnPhase::Finalizing)
            }
            TextStreamVisibility::Hidden => {}
        }
        let allowed_tools = request.tools.clone();
        let response = match stream.callback {
            Some(callback) => {
                self.request_streaming_completion(llm, request, &allowed_tools, context, callback)
                    .await
            }
            None => {
                self.request_buffered_completion(llm, request, &allowed_tools, context)
                    .await
            }
        }?;
        self.emit_cost_signal_if_available(llm.model_name(), &response, context);
        Ok(response)
    }

    fn emit_cost_signal_if_available(
        &mut self,
        model_name: &str,
        response: &CompletionResponse,
        context: StreamingRequestContext,
    ) {
        let Some(usage) = response.usage else {
            return;
        };

        self.emit_signal(
            context.step,
            SignalKind::Cost,
            "LLM usage observed",
            signal_metadata_value(CostSignalMetadata {
                stage: context.stage(),
                model: model_name,
                input_tokens: u64::from(usage.input_tokens),
                output_tokens: u64::from(usage.output_tokens),
                cached_input_tokens: u64::from(usage.cached_input_tokens),
                cache_creation_input_tokens: u64::from(usage.cache_creation_input_tokens),
                total_tokens: u64::from(usage.input_tokens)
                    .saturating_add(u64::from(usage.output_tokens)),
            }),
        );
    }

    async fn request_buffered_completion(
        &mut self,
        llm: &dyn LlmProvider,
        request: CompletionRequest,
        allowed_tools: &[ToolDefinition],
        context: StreamingRequestContext,
    ) -> Result<CompletionResponse, LoopError> {
        let mut stream = llm.complete_stream(request).await.map_err(|error| {
            self.emit_background_error(
                ErrorCategory::Provider,
                format!("LLM request failed: {error}"),
                false,
            );
            loop_error(
                context.stage(),
                &format!("completion failed: {error}"),
                true,
            )
        })?;
        self.publish_stream_started(context.phase);
        self.consume_stream_with_events(
            &mut stream,
            allowed_tools,
            context.step,
            context.phase,
            context.text_visibility,
        )
        .await
    }

    pub(super) async fn request_streaming_completion(
        &mut self,
        llm: &dyn LlmProvider,
        request: CompletionRequest,
        allowed_tools: &[ToolDefinition],
        context: StreamingRequestContext,
        callback: &StreamCallback,
    ) -> Result<CompletionResponse, LoopError> {
        self.publish_stream_started(context.phase);
        let event_bus = self.public_event_bus_clone();
        let buffered_deltas = shared_buffered_deltas(context.phase);
        let preview_text_sent = shared_preview_text_sent(context.text_visibility);
        let bridge = provider_stream_bridge(
            callback.clone(),
            event_bus.clone(),
            context.text_visibility,
            context.phase,
            buffered_deltas.clone(),
            preview_text_sent.clone(),
        );
        let completion_context = StreamingCompletionContext {
            buffered_deltas: buffered_deltas.as_ref(),
            preview_text_sent: preview_text_sent.as_ref(),
            callback,
            event_bus: event_bus.as_ref(),
            request: context,
        };
        self.finish_streaming_completion(
            llm.stream(request, bridge).await,
            allowed_tools,
            completion_context,
        )
    }

    fn finish_streaming_completion(
        &mut self,
        response: Result<CompletionResponse, ProviderError>,
        allowed_tools: &[ToolDefinition],
        context: StreamingCompletionContext<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        match response {
            Ok(response) => Ok(self.handle_streaming_success(response, allowed_tools, context)),
            Err(error) => Err(self.handle_streaming_failure(error, context)),
        }
    }

    fn handle_streaming_success(
        &mut self,
        response: CompletionResponse,
        allowed_tools: &[ToolDefinition],
        context: StreamingCompletionContext<'_>,
    ) -> CompletionResponse {
        let normalized =
            self.normalize_completion_response(response, allowed_tools, context.request.step);
        if preview_text_must_be_cleared(&normalized) {
            if matches!(
                context.request.text_visibility,
                TextStreamVisibility::Preview
            ) {
                reset_preview_text_if_sent(Some(context.callback), context.preview_text_sent);
            }
            self.clear_shared_stream_deltas(context.buffered_deltas);
        } else if normalized.response.tool_calls.is_empty()
            && !matches!(
                context.request.text_visibility,
                TextStreamVisibility::Preview
            )
        {
            self.flush_shared_stream_deltas(
                context.buffered_deltas,
                Some(context.callback),
                context.stream_context(),
            );
            if matches!(
                context.request.text_visibility,
                TextStreamVisibility::Public
            ) && completion_response_text(&normalized.response).is_some()
            {
                self.mark_final_answer_streamed();
            }
        } else if preview_text_can_commit_as_final_answer(&normalized, context.request) {
            emit_final_answer_text(
                &normalized.response,
                Some(context.callback),
                context.event_bus,
                context.request.phase,
            );
            if completion_response_text(&normalized.response).is_some() {
                self.mark_final_answer_streamed();
            }
        }
        self.publish_stream_finished(context.request.phase);
        normalized.response
    }

    fn handle_streaming_failure(
        &self,
        error: ProviderError,
        context: StreamingCompletionContext<'_>,
    ) -> LoopError {
        if matches!(
            context.request.text_visibility,
            TextStreamVisibility::Preview
        ) {
            reset_preview_text_if_sent(Some(context.callback), context.preview_text_sent);
        }
        self.flush_shared_stream_deltas(
            context.buffered_deltas,
            Some(context.callback),
            context.stream_context(),
        );
        (context.callback)(StreamEvent::Error {
            category: ErrorCategory::Provider,
            message: format!("LLM streaming failed: {error}"),
            recoverable: false,
        });
        self.publish_stream_finished(context.request.phase);
        loop_error(
            context.request.stage(),
            &format!("completion failed: {error}"),
            true,
        )
    }

    fn flush_shared_stream_deltas(
        &self,
        buffered_deltas: Option<&SharedBufferedDeltas>,
        callback: StreamCallbackRef<'_>,
        context: StreamConsumeContext<'_>,
    ) {
        if let Some(buffered_deltas) = buffered_deltas {
            if matches!(context.text_visibility, TextStreamVisibility::Preview) {
                buffered_deltas
                    .lock()
                    .expect("buffered stream deltas lock poisoned")
                    .clear();
                return;
            }
            flush_shared_phase_text_deltas(
                buffered_deltas,
                callback,
                context.event_bus,
                context.text_visibility,
                context.phase,
            );
        }
    }

    fn clear_shared_stream_deltas(&self, buffered_deltas: Option<&SharedBufferedDeltas>) {
        let Some(buffered_deltas) = buffered_deltas else {
            return;
        };
        buffered_deltas
            .lock()
            .expect("buffered stream deltas lock poisoned")
            .clear();
    }

    pub(super) fn publish_stream_started(&self, phase: StreamPhase) {
        if let Some(bus) = self.public_event_bus() {
            let _ = bus.publish(InternalMessage::StreamingStarted { phase });
        }
    }

    pub(super) fn publish_stream_finished(&self, phase: StreamPhase) {
        if let Some(bus) = self.public_event_bus() {
            let _ = bus.publish(InternalMessage::StreamingFinished { phase });
        }
    }

    fn stream_cancel_requested(&mut self) -> bool {
        if self.user_stop_requested || self.cancellation_token_triggered() {
            return true;
        }

        if self.consume_stop_or_abort_command() {
            self.user_stop_requested = true;
            return true;
        }

        false
    }

    /// Consume a completion stream, publishing delta/finished events.
    ///
    /// `StreamingFinished` is always published by this method on all exit
    /// paths (success, cancellation, error). Callers must NOT publish
    /// `StreamingFinished` themselves — doing so would produce duplicates.
    pub(super) async fn consume_stream_with_events(
        &mut self,
        stream: &mut CompletionStream,
        allowed_tools: &[ToolDefinition],
        step: LoopStep,
        phase: StreamPhase,
        text_visibility: TextStreamVisibility,
    ) -> Result<CompletionResponse, LoopError> {
        let event_bus = self.public_event_bus_clone();
        let context = StreamConsumeContext {
            event_bus: event_bus.as_ref(),
            step,
            phase,
            text_visibility,
        };
        let mut state = StreamConsumptionState::new(phase);

        loop {
            match self.next_stream_read(stream, &mut state, context).await? {
                StreamRead::Chunk(chunk_result) => {
                    if let Some(response) =
                        self.consume_stream_iteration(&mut state, chunk_result, context)?
                    {
                        return Ok(response);
                    }
                }
                StreamRead::End => break,
                StreamRead::Cancelled(response) => return Ok(response),
            }
        }

        Ok(self.finish_stream_response(state, allowed_tools, context))
    }

    async fn next_stream_read(
        &mut self,
        stream: &mut CompletionStream,
        state: &mut StreamConsumptionState,
        context: StreamConsumeContext<'_>,
    ) -> Result<StreamRead, LoopError> {
        let next_chunk = stream.next();
        tokio::pin!(next_chunk);
        let idle_timeout = provider_stream_chunk_idle_timeout();
        let idle_deadline = sleep(idle_timeout);
        tokio::pin!(idle_deadline);

        loop {
            tokio::select! {
                chunk = &mut next_chunk => {
                    return Ok(match chunk {
                        Some(chunk) => StreamRead::Chunk(chunk),
                        None => StreamRead::End,
                    });
                }
                _ = sleep(PROVIDER_STREAM_CANCEL_POLL_INTERVAL) => {
                    if let Some(response) = self.cancelled_stream_response(state, context) {
                        return Ok(StreamRead::Cancelled(response));
                    }
                }
                _ = &mut idle_deadline => {
                    let error = ProviderError::Streaming(format!(
                        "provider stream was idle for {} seconds",
                        idle_timeout.as_secs()
                    ));
                    self.fail_stream_consumption(error, state, context)?;
                    unreachable!("fail_stream_consumption always returns Err");
                }
            }
        }
    }

    fn consume_stream_iteration(
        &mut self,
        state: &mut StreamConsumptionState,
        chunk_result: Result<StreamChunk, ProviderError>,
        context: StreamConsumeContext<'_>,
    ) -> Result<Option<CompletionResponse>, LoopError> {
        if let Some(response) = self.cancelled_stream_response(state, context) {
            return Ok(Some(response));
        }

        let chunk = self.stream_chunk_or_error(chunk_result, state, context)?;
        self.capture_stream_text_delta(chunk.delta_content.clone(), state, context);
        state.response.apply_chunk(chunk);
        Ok(self.cancelled_stream_response(state, context))
    }

    fn cancelled_stream_response(
        &mut self,
        state: &mut StreamConsumptionState,
        context: StreamConsumeContext<'_>,
    ) -> Option<CompletionResponse> {
        if self.stream_cancel_requested() {
            return Some(self.finish_cancelled_stream(state, context));
        }

        None
    }

    fn stream_chunk_or_error(
        &mut self,
        chunk_result: Result<StreamChunk, ProviderError>,
        state: &mut StreamConsumptionState,
        context: StreamConsumeContext<'_>,
    ) -> Result<StreamChunk, LoopError> {
        match chunk_result {
            Ok(chunk) => Ok(chunk),
            Err(error) => self.fail_stream_consumption(error, state, context),
        }
    }

    fn finish_cancelled_stream(
        &self,
        state: &mut StreamConsumptionState,
        context: StreamConsumeContext<'_>,
    ) -> CompletionResponse {
        self.flush_local_stream_deltas(state, context);
        self.publish_stream_finished(context.phase);
        std::mem::take(&mut state.response).into_cancelled_response()
    }

    fn fail_stream_consumption(
        &mut self,
        error: ProviderError,
        state: &mut StreamConsumptionState,
        context: StreamConsumeContext<'_>,
    ) -> Result<StreamChunk, LoopError> {
        self.flush_local_stream_deltas(state, context);
        self.publish_stream_finished(context.phase);
        self.emit_background_error(
            ErrorCategory::Provider,
            format!("LLM stream error: {error}"),
            false,
        );
        Err(loop_error(
            phase_stage(context.phase),
            &format!("stream consumption failed: {error}"),
            true,
        ))
    }

    fn capture_stream_text_delta(
        &self,
        delta: Option<String>,
        state: &mut StreamConsumptionState,
        context: StreamConsumeContext<'_>,
    ) {
        let Some(delta) = delta else {
            return;
        };

        if state.should_buffer_deltas {
            state.buffered_deltas.push(delta.clone());
            if matches!(context.text_visibility, TextStreamVisibility::Preview) {
                // Preview duplicates the buffered delta intentionally: the UI
                // can show speculative final text now, while the buffer is
                // either discarded for tool calls or withheld from commit.
                if !delta.is_empty() {
                    state.preview_text_sent = true;
                }
                emit_phase_text_delta(
                    None,
                    context.event_bus,
                    context.text_visibility,
                    context.phase,
                    delta,
                );
            }
            return;
        }

        emit_phase_text_delta(
            None,
            context.event_bus,
            context.text_visibility,
            context.phase,
            delta,
        );
    }

    fn finish_stream_response(
        &mut self,
        mut state: StreamConsumptionState,
        allowed_tools: &[ToolDefinition],
        context: StreamConsumeContext<'_>,
    ) -> CompletionResponse {
        let normalized = self.normalize_completion_response(
            state.response.into_response(),
            allowed_tools,
            context.step,
        );
        if preview_text_must_be_cleared(&normalized) {
            if matches!(context.text_visibility, TextStreamVisibility::Preview)
                && state.preview_text_sent
            {
                reset_preview_text(None);
            }
            state.buffered_deltas.clear();
        } else if state.should_buffer_deltas
            && normalized.response.tool_calls.is_empty()
            && !matches!(context.text_visibility, TextStreamVisibility::Preview)
        {
            flush_phase_text_deltas(
                &mut state.buffered_deltas,
                None,
                context.event_bus,
                context.text_visibility,
                context.phase,
            );
            if completion_response_text(&normalized.response).is_some() {
                self.mark_final_answer_streamed();
            }
        }
        self.publish_stream_finished(context.phase);
        normalized.response
    }

    fn flush_local_stream_deltas(
        &self,
        state: &mut StreamConsumptionState,
        context: StreamConsumeContext<'_>,
    ) {
        if state.should_buffer_deltas {
            if matches!(context.text_visibility, TextStreamVisibility::Preview) {
                state.buffered_deltas.clear();
                return;
            }
            flush_phase_text_deltas(
                &mut state.buffered_deltas,
                None,
                context.event_bus,
                context.text_visibility,
                context.phase,
            );
        }
    }

    fn normalize_completion_response(
        &mut self,
        response: CompletionResponse,
        allowed_tools: &[ToolDefinition],
        step: LoopStep,
    ) -> CompletionResponseNormalization {
        // Emitting normalization trace signals makes the streaming intake path
        // mutable even though most stream consumption remains read-only.
        let normalized = apply_tool_call_normalization(response, allowed_tools);

        match &normalized.outcome {
            ToolCallNormalizationOutcome::None => {}
            ToolCallNormalizationOutcome::Normalized { source, tool_names } => {
                self.emit_signal(
                    step,
                    SignalKind::Trace,
                    "normalized malformed tool-call markup",
                    signal_metadata_value(ToolCallNormalizationSignalMetadata {
                        decision_kind: ControlPlaneDecisionKind::ToolCallNormalization,
                        decision: "normalized",
                        outcome: "normalized",
                        source: *source,
                        tool_names: tool_names.clone(),
                        reason: None,
                    }),
                );
            }
            ToolCallNormalizationOutcome::Rejected { source, reason } => {
                self.emit_signal(
                    step,
                    SignalKind::Trace,
                    "rejected malformed tool-call markup",
                    signal_metadata_value(ToolCallNormalizationSignalMetadata {
                        decision_kind: ControlPlaneDecisionKind::ToolCallNormalization,
                        decision: "rejected",
                        outcome: "rejected",
                        source: *source,
                        tool_names: Vec::new(),
                        reason: Some(*reason),
                    }),
                );
            }
        }

        normalized
    }
}

fn provider_stream_chunk_idle_timeout() -> Duration {
    env::var(PROVIDER_STREAM_CHUNK_IDLE_TIMEOUT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_PROVIDER_STREAM_CHUNK_IDLE_TIMEOUT)
}

fn phase_stage(phase: StreamPhase) -> &'static str {
    match phase {
        StreamPhase::Reason => "reason",
        StreamPhase::Synthesize => "act",
    }
}

fn stream_tool_index(
    chunk_index: usize,
    delta: &ToolUseDelta,
    tool_calls_by_index: &HashMap<usize, StreamToolCallState>,
    id_to_index: &HashMap<String, usize>,
) -> usize {
    for identifier in [delta.id.as_deref(), delta.provider_id.as_deref()]
        .into_iter()
        .flatten()
    {
        if let Some(index) = id_to_index.get(identifier).copied() {
            return index;
        }
    }

    let Some(identifier) = delta.id.as_deref().or(delta.provider_id.as_deref()) else {
        return chunk_index;
    };

    if chunk_index_usable_for_identifier(chunk_index, identifier, tool_calls_by_index) {
        return chunk_index;
    }

    next_stream_tool_index(tool_calls_by_index)
}

fn chunk_index_usable_for_identifier(
    chunk_index: usize,
    identifier: &str,
    tool_calls_by_index: &HashMap<usize, StreamToolCallState>,
) -> bool {
    match tool_calls_by_index.get(&chunk_index) {
        None => true,
        Some(state) => match (state.id.as_deref(), state.provider_id.as_deref()) {
            (None, None) => true,
            (Some(existing_id), _) if existing_id == identifier => true,
            (_, Some(existing_provider_id)) if existing_provider_id == identifier => true,
            _ => false,
        },
    }
}

fn next_stream_tool_index(tool_calls_by_index: &HashMap<usize, StreamToolCallState>) -> usize {
    tool_calls_by_index
        .keys()
        .copied()
        .max()
        .map(|index| index.saturating_add(1))
        .unwrap_or(0)
}

fn merge_stream_tool_delta(
    entry: &mut StreamToolCallState,
    delta: ToolUseDelta,
    id_to_index: &mut HashMap<String, usize>,
    index: usize,
) {
    let ToolUseDelta {
        id,
        provider_id,
        name,
        arguments_delta,
        arguments_done,
    } = delta;

    reconcile_stream_tool_id(entry, id, provider_id.as_deref());
    if entry.provider_id.is_none() {
        entry.provider_id = provider_id;
    }
    if entry.name.is_none() {
        entry.name = name;
    }
    register_stream_tool_identifiers(entry, id_to_index, index);
    if let Some(arguments_delta) = arguments_delta {
        merge_stream_arguments(&mut entry.arguments, &arguments_delta, arguments_done);
    }
    entry.arguments_done |= arguments_done;
}

fn reconcile_stream_tool_id(
    entry: &mut StreamToolCallState,
    incoming_id: Option<String>,
    provider_id: Option<&str>,
) {
    let Some(incoming_id) = incoming_id else {
        return;
    };

    match entry.id.as_deref() {
        None => entry.id = Some(incoming_id),
        Some(current_id) if current_id == incoming_id => {}
        Some(current_id) if provider_id.is_some_and(|provider_id| provider_id == current_id) => {
            entry.id = Some(incoming_id);
        }
        Some(_) => {
            if entry.provider_id.is_none() {
                entry.provider_id = Some(incoming_id);
            }
        }
    }
}

fn register_stream_tool_identifiers(
    entry: &StreamToolCallState,
    id_to_index: &mut HashMap<String, usize>,
    index: usize,
) {
    if let Some(id) = entry.id.clone() {
        id_to_index.insert(id, index);
    }
    if let Some(provider_id) = entry.provider_id.clone() {
        id_to_index.insert(provider_id, index);
    }
}

fn merge_stream_arguments(arguments: &mut String, arguments_delta: &str, arguments_done: bool) {
    if arguments_delta.is_empty() {
        return;
    }

    let done_payload_is_complete = arguments_done
        && !arguments.is_empty()
        && serde_json::from_str::<serde_json::Value>(arguments_delta).is_ok();
    if done_payload_is_complete {
        arguments.clear();
    }

    arguments.push_str(arguments_delta);
}

#[cfg(test)]
pub(super) fn finalize_stream_tool_calls(
    by_index: HashMap<usize, StreamToolCallState>,
) -> Vec<ToolCall> {
    finalize_stream_tool_payloads(by_index)
        .into_iter()
        .map(|tool| tool.call)
        .collect()
}

#[derive(Debug)]
struct FinalizedStreamToolCall {
    call: ToolCall,
    provider_id: Option<String>,
}

struct FinalizedStreamToolIdentity {
    id: String,
    name: String,
    provider_id: Option<String>,
}

fn finalize_stream_tool_payloads(
    by_index: HashMap<usize, StreamToolCallState>,
) -> Vec<FinalizedStreamToolCall> {
    let mut indexed_calls = by_index.into_iter().collect::<Vec<_>>();
    indexed_calls.sort_by_key(|(index, _)| *index);
    indexed_calls
        .into_iter()
        .filter_map(|(_, state)| finalized_stream_tool_call_from_state(state))
        .collect()
}

#[cfg(test)]
pub(super) fn stream_tool_call_from_state(state: StreamToolCallState) -> Option<ToolCall> {
    finalized_stream_tool_call_from_state(state).map(|tool| tool.call)
}

fn finalized_stream_tool_call_from_state(
    state: StreamToolCallState,
) -> Option<FinalizedStreamToolCall> {
    if !state.arguments_done {
        return None;
    }

    let identity = finalized_stream_tool_identity(&state)?;
    let arguments = parse_stream_tool_arguments(&state.arguments, &identity.id, &identity.name);
    Some(FinalizedStreamToolCall {
        provider_id: identity.provider_id,
        call: ToolCall {
            id: identity.id,
            name: identity.name,
            arguments,
        },
    })
}

fn finalized_stream_tool_identity(
    state: &StreamToolCallState,
) -> Option<FinalizedStreamToolIdentity> {
    let id = state.id.as_deref().or(state.provider_id.as_deref())?;
    let name = state.name.as_deref()?;
    let id = id.trim().to_string();
    let name = name.trim().to_string();
    if id.is_empty() || name.is_empty() {
        return None;
    }

    Some(FinalizedStreamToolIdentity {
        provider_id: normalized_provider_id(state.provider_id.as_deref(), &id),
        id,
        name,
    })
}

fn normalized_provider_id(provider_id: Option<&str>, id: &str) -> Option<String> {
    provider_id.and_then(|provider_id| {
        let trimmed = provider_id.trim();
        (!trimmed.is_empty() && trimmed != id).then(|| trimmed.to_string())
    })
}

fn parse_stream_tool_arguments(raw_arguments: &str, id: &str, name: &str) -> serde_json::Value {
    let raw_arguments = if raw_arguments.trim().is_empty() {
        "{}"
    } else {
        raw_arguments
    };

    let arguments = fx_llm::parse_tool_arguments_object(raw_arguments);
    if let Some(details) = fx_llm::malformed_tool_arguments(&arguments) {
        tracing::warn!(
            tool_id = %id,
            tool_name = %name,
            raw_arguments = %details.raw,
            error = %details.error,
            "preserving malformed tool call for actionable rejection"
        );
    }

    arguments
}
