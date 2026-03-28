//! OpenAI Responses API provider for ChatGPT subscription (OAuth) auth.
//!
//! Uses the `chatgpt.com/backend-api/codex/responses` endpoint with
//! `Authorization: Bearer` + `chatgpt-account-id` headers.
//! This is the wire format used by ChatGPT Plus/Pro subscriptions via OAuth tokens.

use async_trait::async_trait;
use futures::{stream, SinkExt, StreamExt};
use http::{header::HeaderValue, Request};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashSet;
use tokio_tungstenite::tungstenite::{
    self,
    client::IntoClientRequest,
    protocol::{frame::coding::CloseCode, CloseFrame},
    Message as WsMessage,
};

use crate::document::document_text_fallback;
use crate::openai::{
    is_openai_chat_capable, openai_context_window, openai_models_endpoint, openai_thinking_levels,
    OPENAI_FALLBACK_MODELS, OPENAI_THINKING_LEVELS,
};
use crate::openai_common::{filter_model_ids, OpenAiModelsResponse};
use crate::provider::{
    bearer_auth_headers, insert_header_value, null_loop_harness,
    resolve_loop_harness_from_profiles, CompletionStream, LlmProvider,
    LoopBufferedCompletionStrategy, LoopHarness, LoopModelMatch, LoopModelProfile,
    LoopPromptOverlayContext, LoopStreamingRecoveryStrategy, ProviderCapabilities,
    StaticLoopModelProfile,
};
use crate::sse::{SseFrame, SseFramer};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message, MessageRole,
    StreamChunk, ThinkingConfig, ToolCall, ToolUseDelta, Usage,
};
use crate::validation::validate_tool_message_sequence;

const DEFAULT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api";
const WS_POLICY_CLOSE_PREFIX: &str = "websocket policy close (1008)";
const STREAM_REQUIRED_DETAIL: &str = "Stream must be set to true";
const GPT_REASONING_OVERLAY: &str = "\n\nModel-family guidance for GPT-5/Codex reasoning models: \
When work clearly splits into independent streams, actually use `spawn_agent` / `subagent_status` instead of only describing a parallel plan. \
If the user names an exact command or workflow, execute that exact path before exploring alternatives unless you hit a concrete blocker. \
If you are blocked, state the blocker plainly and ask for direction rather than ending on promise language like \"Let me...\" without taking the next action.";

const GPT_TOOL_CONTINUATION_OVERLAY: &str = "\n\nModel-family guidance for GPT-5/Codex reasoning models: \
After tool calls, turn the evidence into either a direct answer or an explicit blocker. \
Do not emit planning-only text or future-tense promises unless you are also making the next tool call in the same response.";

#[derive(Debug)]
struct OpenAiResponsesLoopHarness {
    use_reasoning_overlays: bool,
}

impl LoopHarness for OpenAiResponsesLoopHarness {
    fn buffered_completion_strategy(&self) -> LoopBufferedCompletionStrategy {
        LoopBufferedCompletionStrategy::SingleResponse
    }

    fn prompt_overlay(&self, context: LoopPromptOverlayContext) -> Option<&'static str> {
        if !self.use_reasoning_overlays {
            return None;
        }

        match context {
            LoopPromptOverlayContext::Reasoning => Some(GPT_REASONING_OVERLAY),
            LoopPromptOverlayContext::ToolContinuation => Some(GPT_TOOL_CONTINUATION_OVERLAY),
        }
    }

    fn is_truncated(&self, stop_reason: Option<&str>) -> bool {
        matches!(
            stop_reason
                .map(|reason| reason.trim().to_ascii_lowercase())
                .as_deref(),
            Some("length" | "incomplete")
        )
    }

    fn streaming_recovery(
        &self,
        _error: &LlmError,
        emitted_text: bool,
    ) -> LoopStreamingRecoveryStrategy {
        if emitted_text {
            LoopStreamingRecoveryStrategy::Fail
        } else {
            LoopStreamingRecoveryStrategy::RetryWithSingleResponse
        }
    }
}

static OPENAI_RESPONSES_LOOP_HARNESS: OpenAiResponsesLoopHarness = OpenAiResponsesLoopHarness {
    use_reasoning_overlays: false,
};

static OPENAI_REASONING_RESPONSES_LOOP_HARNESS: OpenAiResponsesLoopHarness =
    OpenAiResponsesLoopHarness {
        use_reasoning_overlays: true,
    };

static OPENAI_REASONING_RESPONSES_LOOP_PROFILE: StaticLoopModelProfile = StaticLoopModelProfile {
    label: "openai_responses_reasoning",
    matcher: LoopModelMatch::AnyPrefix(&["gpt-5.4", "gpt-5.2", "gpt-5", "codex-", "o1", "o3"]),
    harness: &OPENAI_REASONING_RESPONSES_LOOP_HARNESS,
};

static OPENAI_DEFAULT_RESPONSES_LOOP_PROFILE: StaticLoopModelProfile = StaticLoopModelProfile {
    label: "openai_responses_default",
    matcher: LoopModelMatch::Any,
    harness: &OPENAI_RESPONSES_LOOP_HARNESS,
};

static OPENAI_RESPONSES_LOOP_PROFILES: [&'static dyn LoopModelProfile; 2] = [
    &OPENAI_REASONING_RESPONSES_LOOP_PROFILE,
    &OPENAI_DEFAULT_RESPONSES_LOOP_PROFILE,
];

fn openai_responses_loop_harness(model: &str) -> &'static dyn LoopHarness {
    resolve_loop_harness_from_profiles(&OPENAI_RESPONSES_LOOP_PROFILES, model, null_loop_harness())
}

fn responses_models_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.contains("chatgpt.com") {
        return openai_models_endpoint("https://api.openai.com");
    }
    if base.ends_with("/responses") {
        return format!(
            "{}/models",
            base.trim_end_matches("/responses")
                .trim_end_matches("/codex")
        );
    }
    openai_models_endpoint(base)
}

/// OpenAI Responses API provider for ChatGPT subscription auth.
#[derive(Debug, Clone)]
pub struct OpenAiResponsesProvider {
    base_url: String,
    models_endpoint: String,
    access_token: String,
    account_id: String,
    provider_name: String,
    supported_models: Vec<String>,
    client: reqwest::Client,
}

impl OpenAiResponsesProvider {
    /// Create a new Responses API provider.
    ///
    /// `access_token` is the OAuth Bearer token.
    /// `account_id` is the ChatGPT account ID extracted from the JWT.
    pub fn new(
        access_token: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Result<Self, LlmError> {
        let access_token = access_token.into();
        let account_id = account_id.into();

        if access_token.trim().is_empty() {
            return Err(LlmError::Config("access_token cannot be empty".to_string()));
        }

        if account_id.trim().is_empty() {
            return Err(LlmError::Config("account_id cannot be empty".to_string()));
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(1800))
            .build()
            .map_err(|error| LlmError::Config(format!("failed to build HTTP client: {error}")))?;
        let models_endpoint = responses_models_endpoint(DEFAULT_CODEX_BASE_URL);

        Ok(Self {
            base_url: DEFAULT_CODEX_BASE_URL.to_string(),
            models_endpoint,
            access_token,
            account_id,
            provider_name: "openai".to_string(),
            supported_models: Vec::new(),
            client,
        })
    }

    /// Override the base URL (for testing or alternative endpoints).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self.models_endpoint = responses_models_endpoint(&self.base_url);
        self
    }

    /// Set explicit supported models list.
    pub fn with_supported_models(mut self, supported_models: Vec<String>) -> Self {
        self.supported_models = supported_models;
        self
    }

    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/responses") {
            base.to_string()
        } else if base.ends_with("/codex") {
            format!("{base}/responses")
        } else {
            format!("{base}/codex/responses")
        }
    }

    async fn fetch_models(&self) -> Result<Vec<String>, LlmError> {
        let response = self
            .client
            .get(&self.models_endpoint)
            .bearer_auth(&self.access_token)
            .header("chatgpt-account-id", &self.account_id)
            .send()
            .await?;
        parse_model_response(response, self, &self.supported_models).await
    }

    /// Validate the OAuth token by performing a live model-catalog fetch.
    pub async fn verify_credentials(&self) -> Result<usize, LlmError> {
        Ok(self.fetch_models().await?.len())
    }

    fn ensure_supported_model(&self, model: &str) -> Result<(), LlmError> {
        if self.supported_models.is_empty() || self.supported_models.iter().any(|m| m == model) {
            return Ok(());
        }
        Err(LlmError::UnsupportedModel(model.to_string()))
    }

    fn build_request_body(
        &self,
        request: &CompletionRequest,
        stream: bool,
    ) -> Result<ResponsesRequestBody, LlmError> {
        self.ensure_supported_model(&request.model)?;
        validate_tool_message_sequence(&request.messages)?;

        let mut input = Vec::new();
        for message in &request.messages {
            map_message_to_responses_input(&mut input, message)?;
        }
        validate_responses_input_sequence(&input)?;

        let tools: Vec<ResponsesTool> = request
            .tools
            .iter()
            .map(|tool| ResponsesTool {
                r#type: "function".to_string(),
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            })
            .collect();

        let reasoning = match &request.thinking {
            Some(ThinkingConfig::Reasoning { effort }) => Some(ReasoningConfig {
                effort: effort.clone(),
            }),
            _ => None,
        };

        Ok(ResponsesRequestBody {
            model: request.model.clone(),
            instructions: request.system_prompt.clone(),
            input,
            tools,
            stream,
            store: false,
            temperature: request.temperature,
            tool_choice: None,
            reasoning,
        })
    }

    fn ws_endpoint(&self) -> String {
        let http_url = self.endpoint();
        if let Some(stripped) = http_url.strip_prefix("https://") {
            format!("wss://{stripped}")
        } else if let Some(stripped) = http_url.strip_prefix("http://") {
            format!("ws://{stripped}")
        } else {
            http_url
        }
    }

    fn build_ws_create_payload(&self, request: &CompletionRequest) -> Result<Value, LlmError> {
        let body = self.build_request_body(request, false)?;
        let mut payload = serde_json::to_value(body).map_err(|error| {
            LlmError::InvalidResponse(format!("failed to serialize request: {error}"))
        })?;
        let payload_object = payload.as_object_mut().ok_or_else(|| {
            LlmError::InvalidResponse("request body is not an object".to_string())
        })?;
        payload_object.insert(
            "type".to_string(),
            Value::String("response.create".to_string()),
        );
        payload_object.remove("stream");
        Ok(payload)
    }

    fn build_ws_request(&self, ws_url: &str) -> Result<Request<()>, LlmError> {
        let mut request = ws_url.into_client_request().map_err(|error| {
            LlmError::Config(format!("failed to build WebSocket request: {error}"))
        })?;
        let headers = request.headers_mut();
        insert_header(
            headers,
            "authorization",
            &format!("Bearer {}", self.access_token),
        )?;
        insert_header(headers, "chatgpt-account-id", &self.account_id)?;
        headers.insert(
            "openai-beta",
            HeaderValue::from_static("responses=experimental"),
        );
        headers.insert("originator", HeaderValue::from_static("fawx"));
        insert_header(headers, "host", &extract_host_from_url(ws_url))?;
        Ok(request)
    }

    fn parse_response(body: ResponsesResponseBody) -> CompletionResponse {
        let response_text = collect_output_text(&body.output);
        let tool_calls = collect_tool_calls(&body.output);
        let tool_use_blocks = collect_tool_use_blocks(&body.output);
        let mut content = Vec::new();
        if !response_text.is_empty() {
            content.push(ContentBlock::Text {
                text: response_text,
            });
        }
        content.extend(tool_use_blocks);

        CompletionResponse {
            content,
            usage: body.usage.map(|u| Usage {
                input_tokens: u.input_tokens.unwrap_or(0) as u32,
                output_tokens: u.output_tokens.unwrap_or(0) as u32,
            }),
            stop_reason: normalize_responses_stop_reason(body.status),
            tool_calls,
        }
    }

    fn http_request_builder(&self, body: &ResponsesRequestBody) -> reqwest::RequestBuilder {
        self.client
            .post(self.endpoint())
            .bearer_auth(&self.access_token)
            .header("chatgpt-account-id", &self.account_id)
            .header("openai-beta", "responses=experimental")
            .json(body)
    }

    async fn complete_with_policy_fallback(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        match self.complete_via_stream(request.clone()).await {
            Ok(response) => Ok(response),
            Err(error) if should_retry_http_after_ws_error(&error) => {
                self.complete_via_http_fallback(request).await
            }
            Err(error) => Err(error),
        }
    }

    async fn complete_via_http_fallback(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        if self.capabilities().requires_streaming {
            return self.complete_via_http_stream(request).await;
        }

        match self.complete_via_http(request.clone()).await {
            Err(error) if should_retry_http_with_streaming(&error) => {
                self.complete_via_http_stream(request).await
            }
            result => result,
        }
    }

    async fn complete_via_http(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let body = self.build_request_body(&request, false)?;
        let response = self.http_request_builder(&body).send().await?;
        let status = response.status();

        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("unable to read error body: {error}"));
            return Err(Self::map_http_error(status, body));
        }

        let parsed = response
            .json::<ResponsesResponseBody>()
            .await
            .map_err(|error| LlmError::InvalidResponse(error.to_string()))?;

        Ok(Self::parse_response(parsed))
    }

    async fn complete_via_http_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let body = self.build_request_body(&request, true)?;
        let response = self.http_request_builder(&body).send().await?;
        let status = response.status();

        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("unable to read error body: {error}"));
            return Err(Self::map_http_error(status, body));
        }

        Self::collect_http_stream_response(response).await
    }

    async fn collect_http_stream_response(
        response: reqwest::Response,
    ) -> Result<CompletionResponse, LlmError> {
        let mut bytes_stream = response.bytes_stream();
        let mut framer = SseFramer::default();
        let mut accumulator = StreamAccumulator::default();

        while let Some(bytes) = bytes_stream.next().await {
            let bytes = bytes.map_err(|error| LlmError::Streaming(error.to_string()))?;
            let mut frames = framer.push_bytes(&bytes)?;
            apply_sse_frames(&mut accumulator, &mut frames)?;

            if accumulator.terminal_seen {
                break;
            }
        }

        if !accumulator.terminal_seen {
            let mut frames = framer.finish()?;
            apply_sse_frames(&mut accumulator, &mut frames)?;
        }

        if !accumulator.terminal_seen {
            return Err(LlmError::Provider(
                "http stream ended before terminal response event".to_string(),
            ));
        }

        Ok(completion_from_accumulator(accumulator))
    }

    fn map_http_error(status: StatusCode, body: String) -> LlmError {
        if status.as_u16() == 400 && Self::mentions_reasoning_rejection(&body) {
            tracing::warn!("provider rejected reasoning config — check effort level: {body}");
        }
        match status.as_u16() {
            401 | 403 => LlmError::Authentication(body),
            429 => LlmError::RateLimited(body),
            400..=499 => LlmError::Request(format!("client error {}: {body}", status.as_u16())),
            500..=599 => LlmError::Provider(format!("server error {}: {body}", status.as_u16())),
            _ => LlmError::Request(format!("http {}: {body}", status.as_u16())),
        }
    }

    fn mentions_reasoning_rejection(body: &str) -> bool {
        let lowered = body.to_ascii_lowercase();
        ["reasoning", "effort", "supported values", "must be one of"]
            .iter()
            .any(|term| lowered.contains(term))
    }

    async fn complete_via_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let mut stream = self.complete_stream(request).await?;
        Self::aggregate_completion_stream(&mut stream).await
    }

    async fn aggregate_completion_stream(
        stream: &mut CompletionStream,
    ) -> Result<CompletionResponse, LlmError> {
        let mut accumulator = StreamAccumulator::default();

        while let Some(chunk) = stream.next().await {
            accumulate_stream_chunk(&mut accumulator, chunk?);
        }

        Ok(completion_from_accumulator(accumulator))
    }

    fn chunk_from_event(event_type: &str, data: &str) -> Option<StreamChunk> {
        match event_type {
            "response.output_text.delta" => {
                serde_json::from_str::<SseTextDelta>(data)
                    .ok()
                    .map(|parsed| StreamChunk {
                        delta_content: Some(parsed.delta),
                        ..Default::default()
                    })
            }
            "response.function_call_arguments.delta" => {
                tool_delta_from_arguments_event(data, false).map(chunk_from_tool_delta)
            }
            "response.function_call_arguments.done" => {
                tool_delta_from_arguments_event(data, true).map(chunk_from_tool_delta)
            }
            "response.output_item.added" => {
                tool_delta_from_output_item_event(data, false).map(chunk_from_tool_delta)
            }
            "response.output_item.done" => {
                tool_delta_from_output_item_event(data, true).map(chunk_from_tool_delta)
            }
            "response.completed" | "response.done" => usage_chunk_from_done_event(data),
            _ => None,
        }
    }

    fn stream_from_ws<S>(
        read: S,
    ) -> impl futures::Stream<Item = Result<StreamChunk, LlmError>> + Send
    where
        S: futures::Stream<Item = Result<WsMessage, tungstenite::Error>> + Unpin + Send + 'static,
    {
        stream::unfold((read, false), |(mut read, terminal_seen)| async move {
            if terminal_seen {
                return None;
            }

            loop {
                match read.next().await {
                    Some(Ok(WsMessage::Text(text))) => {
                        let frame = text.to_string();
                        let Some(parsed) = parse_ws_frame(&frame) else {
                            continue;
                        };
                        let Some(event_type) = parsed.get("type").and_then(Value::as_str) else {
                            continue;
                        };

                        if event_type == "error" || event_type == "response.failed" {
                            return Some((
                                Err(LlmError::Provider(ws_error_message(&parsed))),
                                (read, true),
                            ));
                        }

                        let is_terminal = is_terminal_ws_event(event_type);
                        if let Some(chunk) = Self::chunk_from_event(event_type, &frame) {
                            return Some((Ok(chunk), (read, is_terminal)));
                        }

                        if is_terminal {
                            return None;
                        }
                    }
                    Some(Ok(WsMessage::Close(frame))) => {
                        return Some((Err(ws_close_error(frame)), (read, true)));
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        let message = format!("websocket receive failed: {error}");
                        return Some((Err(LlmError::Provider(message)), (read, true)));
                    }
                    None => {
                        return Some((
                            Err(LlmError::Provider(
                                "websocket stream ended before terminal response event".to_string(),
                            )),
                            (read, true),
                        ));
                    }
                }
            }
        })
    }
}

#[derive(Default)]
struct StreamAccumulator {
    text: String,
    usage: Option<Usage>,
    stop_reason: Option<String>,
    pending_tool_calls: Vec<PendingToolCall>,
    terminal_seen: bool,
}

fn should_retry_http_with_streaming(error: &LlmError) -> bool {
    matches!(
        error,
        LlmError::Request(message)
            if message.contains("client error 400")
                && message.contains(STREAM_REQUIRED_DETAIL)
    )
}

fn accumulate_stream_chunk(accumulator: &mut StreamAccumulator, chunk: StreamChunk) {
    accumulator
        .text
        .push_str(chunk.delta_content.as_deref().unwrap_or_default());
    accumulator.usage = chunk.usage.or(accumulator.usage);
    accumulator.stop_reason = chunk.stop_reason.or(accumulator.stop_reason.take());
    merge_tool_use_deltas(&mut accumulator.pending_tool_calls, chunk.tool_use_deltas);
}

fn completion_from_accumulator(accumulator: StreamAccumulator) -> CompletionResponse {
    let mut content = Vec::new();
    if !accumulator.text.is_empty() {
        content.push(ContentBlock::Text {
            text: accumulator.text.clone(),
        });
    }
    content.extend(tool_use_blocks_from_pending(
        &accumulator.pending_tool_calls,
    ));

    CompletionResponse {
        content,
        tool_calls: finalize_tool_calls(accumulator.pending_tool_calls),
        usage: accumulator.usage,
        stop_reason: accumulator.stop_reason,
    }
}

fn apply_sse_frames(
    accumulator: &mut StreamAccumulator,
    frames: &mut Vec<SseFrame>,
) -> Result<(), LlmError> {
    for frame in frames.drain(..) {
        if let SseFrame::Data(data) = frame {
            apply_sse_data_frame(accumulator, &data)?;
        }
    }

    Ok(())
}

fn apply_sse_data_frame(accumulator: &mut StreamAccumulator, data: &str) -> Result<(), LlmError> {
    let Some(parsed) = parse_ws_frame(data) else {
        return Ok(());
    };

    let Some(event_type) = parsed.get("type").and_then(Value::as_str) else {
        return Ok(());
    };

    if event_type == "error" || event_type == "response.failed" {
        return Err(LlmError::Provider(ws_error_message(&parsed)));
    }

    if let Some(chunk) = OpenAiResponsesProvider::chunk_from_event(event_type, data) {
        accumulate_stream_chunk(accumulator, chunk);
    }

    if is_terminal_ws_event(event_type) {
        accumulator.terminal_seen = true;
    }

    Ok(())
}

fn chunk_from_tool_delta(delta: ToolUseDelta) -> StreamChunk {
    StreamChunk {
        tool_use_deltas: vec![delta],
        ..Default::default()
    }
}

fn tool_delta_from_arguments_event(data: &str, is_done_event: bool) -> Option<ToolUseDelta> {
    let parsed = serde_json::from_str::<SseFunctionCallArgsEvent>(data).ok()?;
    let arguments_delta = if is_done_event {
        parsed.arguments
    } else {
        parsed.delta
    };
    let provider_id = parsed.item_id;
    let id = parsed.call_id.or(provider_id.clone());

    if id.is_none() && parsed.name.is_none() && arguments_delta.is_none() {
        return None;
    }

    Some(ToolUseDelta {
        id,
        provider_id,
        name: parsed.name,
        arguments_delta,
        arguments_done: is_done_event,
    })
}

fn tool_delta_from_output_item_event(data: &str, is_done_event: bool) -> Option<ToolUseDelta> {
    let parsed = serde_json::from_str::<SseOutputItemEvent>(data).ok()?;
    let item = parsed.item?;
    if item.r#type.as_deref() != Some("function_call") {
        return None;
    }

    let arguments_delta = item.arguments.filter(|arguments| !arguments.is_empty());
    let provider_id = item.id.or(parsed.item_id);
    let id = item.call_id.or(provider_id.clone());
    let name = item.name;
    if id.is_none() && name.is_none() && arguments_delta.is_none() {
        return None;
    }

    Some(ToolUseDelta {
        id,
        provider_id,
        name,
        arguments_delta,
        arguments_done: is_done_event,
    })
}

fn usage_chunk_from_done_event(data: &str) -> Option<StreamChunk> {
    let parsed = serde_json::from_str::<SseResponseDone>(data).ok()?;
    let SseResponseDone { response, usage } = parsed;
    let (response_usage, response_status) = response
        .map(|response| (response.usage, response.status))
        .unwrap_or((None, None));
    let usage = response_usage.or(usage)?;
    let stop_reason =
        normalize_responses_stop_reason(response_status).unwrap_or_else(|| "stop".to_string());

    Some(StreamChunk {
        usage: Some(responses_usage_to_usage(usage)),
        stop_reason: Some(stop_reason),
        ..Default::default()
    })
}

fn normalize_responses_stop_reason(status: Option<String>) -> Option<String> {
    status.map(|value| {
        if value.eq_ignore_ascii_case("incomplete") {
            "max_tokens".to_string()
        } else {
            value
        }
    })
}

fn responses_usage_to_usage(usage: ResponsesUsage) -> Usage {
    Usage {
        input_tokens: usage.input_tokens.unwrap_or(0) as u32,
        output_tokens: usage.output_tokens.unwrap_or(0) as u32,
    }
}

fn insert_header(
    headers: &mut http::HeaderMap,
    key: &'static str,
    value: &str,
) -> Result<(), LlmError> {
    let header_value = HeaderValue::from_str(value)
        .map_err(|error| LlmError::Config(format!("invalid {key} header: {error}")))?;
    headers.insert(key, header_value);
    Ok(())
}

fn extract_host_from_url(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("chatgpt.com")
        .to_string()
}

fn parse_ws_frame(frame: &str) -> Option<Value> {
    serde_json::from_str::<Value>(frame).ok()
}

fn ws_error_message(frame: &Value) -> String {
    frame
        .pointer("/error/message")
        .or_else(|| frame.pointer("/response/error/message"))
        .and_then(Value::as_str)
        .unwrap_or("websocket error")
        .to_string()
}

fn should_retry_http_after_ws_error(error: &LlmError) -> bool {
    matches!(error,
        LlmError::Provider(message) if message.starts_with(WS_POLICY_CLOSE_PREFIX)
    )
}

fn ws_close_error(frame: Option<CloseFrame>) -> LlmError {
    let Some(frame) = frame else {
        return LlmError::Provider("websocket closed before response completed".to_string());
    };

    if frame.code == CloseCode::Policy {
        return LlmError::Provider(format!(
            "{WS_POLICY_CLOSE_PREFIX}: {}",
            close_reason_or_default(frame.reason.as_ref())
        ));
    }

    LlmError::Provider(format!(
        "websocket closed with code {}: {}",
        u16::from(frame.code),
        close_reason_or_default(frame.reason.as_ref())
    ))
}

fn close_reason_or_default(reason: &str) -> &str {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        "no reason provided"
    } else {
        trimmed
    }
}

fn is_terminal_ws_event(event_type: &str) -> bool {
    matches!(event_type, "response.completed" | "response.done")
}

fn map_message_to_responses_input(
    input: &mut Vec<Value>,
    message: &Message,
) -> Result<(), LlmError> {
    match message.role {
        MessageRole::System => {
            // System messages go into instructions, skip here.
        }
        MessageRole::User => push_text_input(input, "user", &message.content),
        MessageRole::Assistant => push_assistant_input(input, &message.content)?,
        MessageRole::Tool => push_tool_result_input(input, &message.content),
    }

    Ok(())
}

fn validate_responses_input_sequence(input: &[Value]) -> Result<(), LlmError> {
    let mut seen_tool_calls = HashSet::new();

    for (item_index, item) in input.iter().enumerate() {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                if let Some(call_id) = item.get("call_id").and_then(Value::as_str) {
                    let trimmed = call_id.trim();
                    if !trimmed.is_empty() {
                        seen_tool_calls.insert(trimmed.to_string());
                    }
                }
            }
            Some("function_call_output") => {
                let Some(call_id) = item.get("call_id").and_then(Value::as_str) else {
                    continue;
                };
                let trimmed = call_id.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if !seen_tool_calls.contains(trimmed) {
                    return Err(LlmError::Request(format!(
                        "invalid responses input: function_call_output '{}' at item {} has no matching earlier function_call; tail={}",
                        trimmed,
                        item_index,
                        summarize_input_tail(input),
                    )));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn summarize_input_tail(input: &[Value]) -> String {
    let start = input.len().saturating_sub(8);
    input[start..]
        .iter()
        .enumerate()
        .map(|(offset, item)| summarize_input_item(start + offset, item))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn summarize_input_item(index: usize, item: &Value) -> String {
    if let Some(item_type) = item.get("type").and_then(Value::as_str) {
        return match item_type {
            "function_call" => format!(
                "{index}:function_call:{}:{}",
                item.get("call_id").and_then(Value::as_str).unwrap_or("-"),
                item.get("id").and_then(Value::as_str).unwrap_or("-")
            ),
            "function_call_output" => format!(
                "{index}:function_call_output:{}",
                item.get("call_id").and_then(Value::as_str).unwrap_or("-")
            ),
            other => format!("{index}:{other}"),
        };
    }

    if let Some(role) = item.get("role").and_then(Value::as_str) {
        return format!("{index}:role:{role}");
    }

    format!("{index}:unknown")
}

/// Normalize a tool call ID for OpenAI Responses API compatibility.
/// OpenAI requires call_id to match `^fc_`. If the ID already starts
/// with `fc_`, return as-is. Otherwise, strip known prefixes and add `fc_`.
fn normalize_call_id(id: &str) -> String {
    if id.starts_with("fc_") {
        return id.to_string();
    }

    let stripped = id
        .strip_prefix("toolu_")
        .or_else(|| id.strip_prefix("call_"))
        .unwrap_or(id);

    format!("fc_{stripped}")
}

fn push_text_input(input: &mut Vec<Value>, role: &str, blocks: &[ContentBlock]) {
    let content = response_input_content(role, blocks);
    if content.is_empty() {
        return;
    }

    input.push(json!({"role": role, "content": content}));
}

fn response_input_content(role: &str, blocks: &[ContentBlock]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| response_input_block(role, block))
        .collect()
}

fn response_input_block(role: &str, block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Text { text } => {
            let block_type = match role {
                "assistant" => "output_text",
                _ => "input_text",
            };
            Some(json!({"type": block_type, "text": text}))
        }
        ContentBlock::Image { media_type, data } if role == "user" => Some(json!({
            "type": "input_image",
            "image_url": format!("data:{media_type};base64,{data}")
        })),
        ContentBlock::Document {
            media_type,
            data,
            filename,
        } if role == "user" => Some(json!({
            "type": "input_text",
            "text": document_text_fallback(media_type, data, filename.as_deref())
        })),
        ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. } => None,
        ContentBlock::Image { .. } | ContentBlock::Document { .. } => None,
    }
}

fn push_assistant_input(input: &mut Vec<Value>, blocks: &[ContentBlock]) -> Result<(), LlmError> {
    push_text_input(input, "assistant", blocks);

    for block in blocks {
        if let ContentBlock::ToolUse {
            id,
            provider_id,
            name,
            input: tool_input,
        } = block
        {
            let arguments = serde_json::to_string(tool_input)
                .map_err(|error| LlmError::Serialization(error.to_string()))?;
            let normalized_id = normalize_call_id(provider_id.as_deref().unwrap_or(id));
            let normalized_call_id = normalize_call_id(id);
            input.push(json!({
                "type": "function_call",
                "id": normalized_id,
                "call_id": normalized_call_id,
                "name": name,
                "arguments": arguments,
            }));
        }
    }

    Ok(())
}

fn push_tool_result_input(input: &mut Vec<Value>, blocks: &[ContentBlock]) {
    let fallback_text = extract_text(blocks);
    let mut appended_tool_result = false;

    for block in blocks {
        if let ContentBlock::ToolResult {
            tool_use_id,
            content,
        } = block
        {
            if tool_use_id.trim().is_empty() {
                continue;
            }

            appended_tool_result = true;
            let normalized_call_id = normalize_call_id(tool_use_id);
            input.push(json!({
                "type": "function_call_output",
                "call_id": normalized_call_id,
                "output": tool_result_output(content, &fallback_text),
            }));
        }
    }

    if !appended_tool_result {
        push_text_input(input, "user", blocks);
    }
}

fn tool_result_output(content: &Value, fallback_text: &str) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Object(object) => object
            .get("output")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                if fallback_text.is_empty() {
                    None
                } else {
                    Some(fallback_text.to_string())
                }
            })
            .unwrap_or_else(|| Value::Object(object.clone()).to_string()),
        other => {
            if fallback_text.is_empty() {
                other.to_string()
            } else {
                fallback_text.to_string()
            }
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiResponsesProvider {
    fn name(&self) -> &str {
        &self.provider_name
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.complete_with_policy_fallback(request).await
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, LlmError> {
        let ws_payload = self.build_ws_create_payload(&request)?;
        let ws_request = self.build_ws_request(&self.ws_endpoint())?;
        let (ws_stream, _) = tokio_tungstenite::connect_async(ws_request)
            .await
            .map_err(|error| LlmError::Provider(format!("websocket connection failed: {error}")))?;
        let (mut write, read) = ws_stream.split();
        let ws_message = serde_json::to_string(&ws_payload).map_err(|error| {
            LlmError::InvalidResponse(format!("failed to serialize message: {error}"))
        })?;
        write
            .send(WsMessage::Text(ws_message.into()))
            .await
            .map_err(|error| LlmError::Provider(format!("websocket send failed: {error}")))?;

        Ok(Box::pin(Self::stream_from_ws(read)))
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported_models.clone()
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        if self.access_token.trim().is_empty() {
            return Ok(self.supported_models());
        }

        match self.fetch_models().await {
            Ok(models) if !models.is_empty() => Ok(models),
            Ok(_) => Ok(self.supported_models()),
            Err(error) => {
                tracing::warn!(provider = %self.provider_name, error = %error, "failed to fetch openai responses models; using static fallback");
                Ok(self.supported_models())
            }
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_temperature: false,
            requires_streaming: true,
        }
    }

    fn supported_thinking_levels(&self) -> &'static [&'static str] {
        OPENAI_THINKING_LEVELS
    }

    fn thinking_levels(&self, model: &str) -> &'static [&'static str] {
        openai_thinking_levels(model)
    }

    fn models_endpoint(&self) -> Option<&str> {
        Some(&self.models_endpoint)
    }

    fn auth_method(&self) -> &'static str {
        "subscription"
    }

    fn catalog_auth_headers(
        &self,
        api_key: &str,
        _auth_mode: &str,
    ) -> Result<reqwest::header::HeaderMap, String> {
        let mut headers = bearer_auth_headers(api_key)?;
        insert_header_value(
            &mut headers,
            "chatgpt-account-id",
            &self.account_id,
            "account id",
        )?;
        Ok(headers)
    }

    fn is_chat_capable(&self, model_id: &str) -> bool {
        is_openai_chat_capable(model_id)
    }

    fn fallback_models(&self) -> Vec<&'static str> {
        OPENAI_FALLBACK_MODELS.to_vec()
    }

    fn context_window(&self, model: &str) -> usize {
        openai_context_window(model)
    }

    fn loop_harness(&self, model: &str) -> &'static dyn LoopHarness {
        openai_responses_loop_harness(model)
    }
}

async fn parse_model_response(
    response: reqwest::Response,
    provider: &OpenAiResponsesProvider,
    supported_models: &[String],
) -> Result<Vec<String>, LlmError> {
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|error| format!("unable to read error body: {error}"));
        return Err(LlmError::Provider(format!(
            "model list request failed ({status}): {body}"
        )));
    }

    let parsed = response
        .json::<OpenAiModelsResponse>()
        .await
        .map_err(|error| LlmError::InvalidResponse(error.to_string()))?;
    Ok(filter_model_ids(
        parsed.data,
        supported_models,
        |model_id| provider.is_chat_capable(model_id),
    ))
}

// =====================================================================
// Request/Response types
// =====================================================================

#[derive(Serialize)]
struct ResponsesRequestBody {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ResponsesTool>,
    stream: bool,
    store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
}

#[derive(Debug, Serialize)]
struct ReasoningConfig {
    effort: String,
}

/// Maps a Fawx ToolDefinition to the OpenAI Responses API function tool format.
/// Note: the Responses API also accepts a `strict` field for strict JSON schema
/// validation, but we omit it — emit_intent uses dynamic sub-schemas (tool lists
/// vary at runtime) which are not compatible with strict mode.
#[derive(Serialize)]
struct ResponsesTool {
    r#type: String,
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Deserialize)]
struct ResponsesResponseBody {
    #[serde(default)]
    output: Vec<ResponsesOutputItem>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Deserialize)]
struct ResponsesOutputItem {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
    #[serde(default)]
    content: Option<Vec<ResponsesContentPart>>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct ResponsesContentPart {
    r#type: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct ResponsesUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

// SSE event types
#[derive(Deserialize)]
struct SseTextDelta {
    delta: String,
}

#[derive(Deserialize)]
struct SseResponseDone {
    #[serde(default)]
    response: Option<SseResponseBody>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Deserialize)]
struct SseResponseBody {
    #[serde(default)]
    usage: Option<ResponsesUsage>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct SseFunctionCallArgsEvent {
    #[serde(default)]
    item_id: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct SseOutputItemEvent {
    #[serde(default)]
    item_id: Option<String>,
    #[serde(default)]
    item: Option<SseOutputItem>,
}

#[derive(Deserialize)]
struct SseOutputItem {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

// =====================================================================
// Helpers
// =====================================================================

fn extract_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(Cow::Borrowed(text.as_str())),
            ContentBlock::Image { .. } => None,
            ContentBlock::Document {
                media_type,
                data,
                filename,
            } => Some(Cow::Owned(document_text_fallback(
                media_type,
                data,
                filename.as_deref(),
            ))),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn collect_output_text(output: &[ResponsesOutputItem]) -> String {
    let mut text_parts = Vec::new();

    for item in output {
        if let Some(content) = &item.content {
            for part in content {
                if part.r#type == "output_text" {
                    if let Some(text) = &part.text {
                        text_parts.push(text.clone());
                    }
                }
            }
        }
        if let Some(text) = &item.text {
            text_parts.push(text.clone());
        }
    }

    text_parts.join("")
}

fn collect_tool_calls(output: &[ResponsesOutputItem]) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();

    for item in output {
        if item.r#type.as_deref() != Some("function_call") {
            continue;
        }

        let Some(id) = item.call_id.as_ref().or(item.id.as_ref()) else {
            continue;
        };
        if let (Some(name), Some(arguments)) = (&item.name, &item.arguments) {
            let arguments_value = crate::parse_tool_arguments_object(arguments);

            tool_calls.push(ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments_value,
            });
        }
    }

    tool_calls
}

fn collect_tool_use_blocks(output: &[ResponsesOutputItem]) -> Vec<ContentBlock> {
    output.iter().filter_map(tool_use_block_from_item).collect()
}

fn tool_use_block_from_item(item: &ResponsesOutputItem) -> Option<ContentBlock> {
    if item.r#type.as_deref() != Some("function_call") {
        return None;
    }

    let id = item.call_id.as_ref().or(item.id.as_ref())?.clone();
    let provider_id = item
        .id
        .as_ref()
        .filter(|provider_id| provider_id.as_str() != id.as_str())
        .cloned();
    let name = item.name.as_ref()?.clone();
    let arguments = item.arguments.as_ref()?;
    let input = crate::parse_tool_arguments_object(arguments);

    Some(ContentBlock::ToolUse {
        id,
        provider_id,
        name,
        input,
    })
}

#[derive(Default)]
struct PendingToolCall {
    id: Option<String>,
    provider_id: Option<String>,
    name: Option<String>,
    arguments: String,
}

fn merge_tool_use_deltas(pending: &mut Vec<PendingToolCall>, deltas: Vec<ToolUseDelta>) {
    for delta in deltas {
        let index = pending_tool_call_index(pending, &delta);
        apply_tool_use_delta(&mut pending[index], delta);
    }
}

fn pending_tool_call_index(pending: &mut Vec<PendingToolCall>, delta: &ToolUseDelta) -> usize {
    if let Some(index) = pending_index_by_id(pending, delta) {
        return index;
    }

    if let Some(index) = pending_index_without_id(pending, delta) {
        return index;
    }

    pending.push(PendingToolCall::default());
    pending.len() - 1
}

fn pending_index_by_id(pending: &[PendingToolCall], delta: &ToolUseDelta) -> Option<usize> {
    pending
        .iter()
        .position(|call| identifiers_overlap(call, delta))
}

fn identifiers_overlap(call: &PendingToolCall, delta: &ToolUseDelta) -> bool {
    let call_identifiers = [call.id.as_deref(), call.provider_id.as_deref()];
    let delta_identifiers = [delta.id.as_deref(), delta.provider_id.as_deref()];

    call_identifiers.into_iter().flatten().any(|left| {
        delta_identifiers
            .into_iter()
            .flatten()
            .any(|right| left == right)
    })
}

fn pending_index_without_id(pending: &[PendingToolCall], delta: &ToolUseDelta) -> Option<usize> {
    if delta.id.is_some() || delta.provider_id.is_some() {
        return pending.iter().rposition(|call| {
            call.id.is_none()
                && call.provider_id.is_none()
                && same_or_unknown_name(call.name.as_deref(), delta.name.as_deref())
        });
    }

    if let Some(name) = delta.name.as_deref() {
        return pending.iter().rposition(|call| {
            call.id.is_none()
                && call.provider_id.is_none()
                && same_or_unknown_name(call.name.as_deref(), Some(name))
        });
    }

    pending
        .iter()
        .enumerate()
        .filter(|(_, call)| call.id.is_none() || call.provider_id.is_none() || call.name.is_none())
        .map(|(index, _)| index)
        // Use next_back() instead of last() to avoid needless full iteration
        // on this DoubleEndedIterator (clippy::double_ended_iterator_last).
        .next_back()
}

fn same_or_unknown_name(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

fn apply_tool_use_delta(call: &mut PendingToolCall, delta: ToolUseDelta) {
    if let Some(incoming_id) = delta.id.clone() {
        match call.id.as_deref() {
            None => call.id = Some(incoming_id),
            Some(current_id) if current_id == incoming_id => {}
            Some(current_id)
                if delta
                    .provider_id
                    .as_deref()
                    .is_some_and(|provider_id| provider_id == current_id) =>
            {
                call.id = Some(incoming_id);
            }
            Some(_) => {
                if call.provider_id.is_none() {
                    call.provider_id = Some(incoming_id);
                }
            }
        }
    }

    if call.provider_id.is_none() {
        call.provider_id = delta.provider_id;
    }

    if call.name.is_none() {
        call.name = delta.name;
    }

    if let Some(arguments_delta) = delta.arguments_delta {
        merge_arguments(&mut call.arguments, &arguments_delta, delta.arguments_done);
    }
}

fn merge_arguments(arguments: &mut String, incoming: &str, is_done_event: bool) {
    if incoming.is_empty() {
        return;
    }

    if is_done_event && !arguments.is_empty() {
        return;
    }

    arguments.push_str(incoming);
}

fn finalize_tool_calls(pending: Vec<PendingToolCall>) -> Vec<ToolCall> {
    pending
        .into_iter()
        .filter_map(|call| {
            let id = call.id.or(call.provider_id)?.trim().to_string();
            let name = call.name?.trim().to_string();
            if id.is_empty() || name.is_empty() {
                return None;
            }
            let arguments = crate::parse_tool_arguments_object(&call.arguments);

            Some(ToolCall {
                id,
                name,
                arguments,
            })
        })
        .collect()
}

fn tool_use_blocks_from_pending(pending: &[PendingToolCall]) -> Vec<ContentBlock> {
    pending
        .iter()
        .filter_map(|call| {
            let id = call.id.as_ref()?.trim();
            let name = call.name.as_ref()?.trim();
            if id.is_empty() || name.is_empty() {
                return None;
            }

            let provider_id = call
                .provider_id
                .as_deref()
                .filter(|provider_id| *provider_id != id)
                .map(ToString::to_string);
            let input = crate::parse_tool_arguments_object(&call.arguments);

            Some(ContentBlock::ToolUse {
                id: id.to_string(),
                provider_id,
                name: name.to_string(),
                input,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{simple_pdf_with_text, spawn_json_server};
    use base64::Engine;
    use futures::{pin_mut, stream, StreamExt};

    #[test]
    fn endpoint_resolves_correctly() {
        let provider = OpenAiResponsesProvider::new("test-token", "test-account").unwrap();
        assert_eq!(
            provider.endpoint(),
            "https://chatgpt.com/backend-api/codex/responses"
        );

        let custom = provider.clone().with_base_url("https://example.com/api");
        assert_eq!(custom.endpoint(), "https://example.com/api/codex/responses");

        let already_full = provider
            .clone()
            .with_base_url("https://example.com/codex/responses");
        assert_eq!(
            already_full.endpoint(),
            "https://example.com/codex/responses"
        );
    }

    #[tokio::test]
    async fn list_models_fetches_dynamic_openai_catalog() {
        let base_url = spawn_json_server(
            "200 OK",
            r#"{"data":[{"id":"gpt-4.1"},{"id":"text-embedding-3-small"},{"id":"o3-mini"}]}"#,
        )
        .await;
        let provider = OpenAiResponsesProvider::new("test-token", "test-account")
            .expect("provider")
            .with_base_url(base_url)
            .with_supported_models(vec!["gpt-4o-mini".to_string()]);

        let models = provider.list_models().await.expect("list models");

        assert_eq!(models, vec!["gpt-4.1".to_string(), "o3-mini".to_string()]);
    }

    #[tokio::test]
    async fn list_models_falls_back_when_dynamic_fetch_fails() {
        let base_url = spawn_json_server("500 Internal Server Error", r#"{"error":"nope"}"#).await;
        let provider = OpenAiResponsesProvider::new("test-token", "test-account")
            .expect("provider")
            .with_base_url(base_url)
            .with_supported_models(vec!["gpt-4o-mini".to_string()]);

        let models = provider.list_models().await.expect("list models");

        assert_eq!(models, vec!["gpt-4o-mini".to_string()]);
    }

    #[tokio::test]
    async fn list_models_returns_supported_models_when_token_is_empty() {
        let mut provider = OpenAiResponsesProvider::new("test-token", "test-account")
            .expect("provider")
            .with_supported_models(vec!["gpt-4o-mini".to_string()]);
        provider.access_token.clear();

        let models = provider.list_models().await.expect("list models");

        assert_eq!(models, vec!["gpt-4o-mini".to_string()]);
    }

    #[test]
    fn responses_catalog_metadata_matches_expected_contract() {
        let provider = OpenAiResponsesProvider::new("test-token", "acct_123").unwrap();

        assert_eq!(
            provider.supported_thinking_levels(),
            &["off", "low", "high"]
        );
        assert_eq!(
            provider.thinking_levels("gpt-5.4"),
            &["none", "low", "medium", "high", "xhigh"]
        );
        assert_eq!(
            provider.models_endpoint(),
            Some("https://api.openai.com/v1/models")
        );
        assert!(provider.is_chat_capable("gpt-4o"));
        assert!(!provider.is_chat_capable("text-embedding-3-small"));
        assert_eq!(provider.fallback_models(), OPENAI_FALLBACK_MODELS);
        assert_eq!(provider.auth_method(), "subscription");
        assert_eq!(provider.context_window("gemini-2.5-pro"), 1_000_000);
    }

    #[test]
    fn responses_catalog_auth_headers_include_account_context() {
        let provider = OpenAiResponsesProvider::new("test-token", "acct_123").unwrap();

        let headers = provider
            .catalog_auth_headers("oauth-token-123", "oauth")
            .expect("headers");

        assert_eq!(
            headers.get(reqwest::header::AUTHORIZATION).unwrap(),
            "Bearer oauth-token-123"
        );
        assert_eq!(headers.get("chatgpt-account-id").unwrap(), "acct_123");
    }

    #[test]
    fn parse_response_extracts_text() {
        let body = ResponsesResponseBody {
            output: vec![ResponsesOutputItem {
                r#type: None,
                id: None,
                call_id: None,
                name: None,
                arguments: None,
                content: Some(vec![ResponsesContentPart {
                    r#type: "output_text".to_string(),
                    text: Some("Hello from ChatGPT!".to_string()),
                }]),
                text: None,
            }],
            status: Some("completed".to_string()),
            usage: Some(ResponsesUsage {
                input_tokens: Some(10),
                output_tokens: Some(5),
            }),
        };

        let response = OpenAiResponsesProvider::parse_response(body);
        assert_eq!(
            response.content,
            vec![ContentBlock::Text {
                text: "Hello from ChatGPT!".to_string()
            }]
        );
        let usage = response.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
    }

    #[test]
    fn document_content_block_falls_back_to_text_for_openai_responses() {
        let document = ContentBlock::Document {
            media_type: "application/pdf".to_string(),
            data: base64::engine::general_purpose::STANDARD
                .encode(simple_pdf_with_text("Hello PDF")),
            filename: Some("brief.pdf".to_string()),
        };

        let mapped = response_input_block("user", &document).expect("mapped block");

        assert_eq!(mapped["type"], "input_text");
        let text = mapped["text"].as_str().expect("text payload");
        assert!(text.contains("[file: brief.pdf]"));
        assert!(text.contains("Hello PDF"));
    }

    fn continuation_assistant_message() -> crate::types::Message {
        crate::types::Message {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: Some("fc_1".to_string()),
                    name: "lookup".to_string(),
                    input: serde_json::json!({"q": "first"}),
                },
                ContentBlock::ToolUse {
                    id: "call_2".to_string(),
                    provider_id: Some("fc_2".to_string()),
                    name: "lookup".to_string(),
                    input: serde_json::json!({"q": "second"}),
                },
            ],
        }
    }

    fn continuation_tool_message() -> crate::types::Message {
        crate::types::Message {
            role: MessageRole::Tool,
            content: vec![
                ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: Value::String("first result".to_string()),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "call_2".to_string(),
                    content: serde_json::json!({"output": "second result"}),
                },
            ],
        }
    }

    fn continuation_request() -> CompletionRequest {
        CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![
                crate::types::Message::user("Find weather"),
                continuation_assistant_message(),
                continuation_tool_message(),
            ],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        }
    }

    fn single_tool_continuation_request(tool_output: Value) -> CompletionRequest {
        CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![
                crate::types::Message {
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        provider_id: None,
                        name: "lookup".to_string(),
                        input: serde_json::json!({"q": "weather"}),
                    }],
                },
                crate::types::Message {
                    role: MessageRole::Tool,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: tool_output,
                    }],
                },
            ],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        }
    }

    #[test]
    fn normalize_call_id_passes_fc_through() {
        assert_eq!(normalize_call_id("fc_123"), "fc_123");
    }

    #[test]
    fn normalize_call_id_remaps_toolu_prefix() {
        assert_eq!(normalize_call_id("toolu_011VJLabcdef"), "fc_011VJLabcdef");
    }

    #[test]
    fn normalize_call_id_remaps_call_prefix() {
        assert_eq!(normalize_call_id("call_abc123"), "fc_abc123");
    }

    #[test]
    fn normalize_call_id_remaps_unknown_prefix() {
        assert_eq!(normalize_call_id("xyz789"), "fc_xyz789");
    }

    #[test]
    fn build_request_body_normalizes_anthropic_tool_ids_for_openai_responses() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![
                crate::types::Message {
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id: "toolu_011VJLabcdef".to_string(),
                        provider_id: None,
                        name: "lookup".to_string(),
                        input: serde_json::json!({"q": "weather"}),
                    }],
                },
                crate::types::Message {
                    role: MessageRole::Tool,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "toolu_011VJLabcdef".to_string(),
                        content: Value::String("sunny".to_string()),
                    }],
                },
            ],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();
        let function_call_call_id = input[0]["call_id"].as_str().unwrap();
        let function_call_output_call_id = input[1]["call_id"].as_str().unwrap();

        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[1]["type"], "function_call_output");
        assert!(function_call_call_id.starts_with("fc_"));
        assert!(function_call_output_call_id.starts_with("fc_"));
        assert_eq!(function_call_call_id, function_call_output_call_id);
    }

    #[test]
    fn build_request_body_maps_messages() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();

        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![crate::types::Message {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".to_string(),
                }],
            }],
            system_prompt: Some("You are helpful.".to_string()),
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(body.model, "gpt-4.1");
        assert_eq!(body.instructions, Some("You are helpful.".to_string()));
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(
            input[0]["content"][0],
            serde_json::json!({"type": "input_text", "text": "Hello"})
        );
        assert!(!body.stream);
    }

    #[test]
    fn build_request_body_includes_reasoning_effort() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-5.4".to_string(),
            messages: vec![crate::types::Message::user("Hello")],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: Some(ThinkingConfig::Reasoning {
                effort: "xhigh".to_string(),
            }),
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();

        assert_eq!(
            serialized["reasoning"],
            serde_json::json!({"effort": "xhigh"})
        );
    }

    #[test]
    fn build_request_body_omits_reasoning_when_disabled() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-5.4".to_string(),
            messages: vec![crate::types::Message::user("Hello")],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: Some(ThinkingConfig::Off),
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();

        assert!(serialized.get("reasoning").is_none());
    }

    #[test]
    fn build_request_body_maps_user_images_for_responses_api() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![crate::types::Message::user_with_images(
                "describe this",
                vec![crate::types::ImageAttachment {
                    media_type: "image/png".to_string(),
                    data: "abc123".to_string(),
                }],
            )],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();
        let content = input[0]["content"].as_array().unwrap();

        assert_eq!(
            content[0],
            serde_json::json!({
                "type": "input_image",
                "image_url": "data:image/png;base64,abc123"
            })
        );
        assert_eq!(
            content[1],
            serde_json::json!({
                "type": "input_text",
                "text": "describe this"
            })
        );
    }

    #[test]
    fn build_request_body_maps_assistant_text_as_output_text() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![
                crate::types::Message::user("Hello"),
                crate::types::Message::assistant("Hi there"),
            ],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(
            input[1],
            serde_json::json!({
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hi there"}]
            })
        );
    }

    #[test]
    fn build_request_body_drops_image_on_assistant_message() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![
                crate::types::Message::user("Hello"),
                crate::types::Message {
                    role: MessageRole::Assistant,
                    content: vec![
                        ContentBlock::Image {
                            media_type: "image/png".to_string(),
                            data: "abc123".to_string(),
                        },
                        ContentBlock::Text {
                            text: "Hi there".to_string(),
                        },
                    ],
                },
            ],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(
            input[1],
            serde_json::json!({
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hi there"}]
            })
        );
    }

    #[test]
    fn test_build_request_body_maps_tool_continuation_to_function_call_output() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let body = provider
            .build_request_body(&continuation_request(), false)
            .unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(input.len(), 5);
        assert_eq!(
            input[0],
            serde_json::json!({
                "role": "user",
                "content": [{"type": "input_text", "text": "Find weather"}]
            })
        );
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["id"], "fc_1");
        assert_eq!(input[1]["call_id"], "fc_1");
        assert_eq!(input[1]["name"], "lookup");
        assert_eq!(input[1]["arguments"], "{\"q\":\"first\"}");
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["id"], "fc_2");
        assert_eq!(input[2]["call_id"], "fc_2");
        assert_eq!(input[2]["name"], "lookup");
        assert_eq!(input[2]["arguments"], "{\"q\":\"second\"}");
        assert_eq!(
            input[3],
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "fc_1",
                "output": "first result"
            })
        );
        assert_eq!(
            input[4],
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "fc_2",
                "output": "second result"
            })
        );
    }

    #[test]
    fn build_tool_continuation_request_omits_tool_choice_with_tools_available() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let mut request = continuation_request();
        request.tools = vec![crate::types::ToolDefinition {
            name: "lookup".to_string(),
            description: "look something up".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[4]["type"], "function_call_output");
        assert_eq!(serialized["tools"][0]["name"], "lookup");
        assert!(serialized.get("tool_choice").is_none());
    }

    #[test]
    fn build_request_body_rejects_tool_result_without_matching_assistant_tool_use() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![
                crate::types::Message::user("Find weather"),
                crate::types::Message {
                    role: MessageRole::Tool,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: serde_json::json!("first result"),
                    }],
                },
            ],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let error = match provider.build_request_body(&request, false) {
            Ok(_) => panic!("should reject orphan tool result"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("invalid tool continuation messages"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn validate_responses_input_sequence_rejects_orphan_function_call_output() {
        let input = vec![
            serde_json::json!({
                "role": "user",
                "content": [{"type": "input_text", "text": "Find weather"}]
            }),
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "first result"
            }),
        ];

        let error = match validate_responses_input_sequence(&input) {
            Ok(_) => panic!("should reject orphan function call output"),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("invalid responses input"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_build_request_body_maps_tool_result_string_content() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = single_tool_continuation_request(Value::String("tool output".to_string()));

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[1]["type"], "function_call_output");
        assert_eq!(input[1]["call_id"], "fc_1");
        assert_eq!(input[1]["output"], "tool output");
    }

    #[test]
    fn test_build_request_body_maps_tool_result_error_prefix() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = single_tool_continuation_request(Value::String(
            "[ERROR] permission denied".to_string(),
        ));

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[1]["type"], "function_call_output");
        assert_eq!(input[1]["call_id"], "fc_1");
        assert_eq!(input[1]["output"], "[ERROR] permission denied");
    }

    #[test]
    fn build_request_body_includes_tools() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();

        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![crate::types::Message::user("hi")],
            system_prompt: None,
            tools: vec![crate::types::ToolDefinition {
                name: "emit_intent".to_string(),
                description: "Emit intent".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        assert_eq!(serialized["tools"][0]["type"], "function");
        assert_eq!(serialized["tools"][0]["name"], "emit_intent");
    }

    #[test]
    fn build_request_body_omits_empty_tools() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();

        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![crate::types::Message::user("hi")],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        assert!(serialized.get("tools").is_none());
    }

    #[test]
    fn build_request_body_omits_tool_choice_when_tools_present() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![],
            system_prompt: None,
            tools: vec![crate::types::ToolDefinition {
                name: "test_tool".to_string(),
                description: "a test tool".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        assert!(serialized.get("tool_choice").is_none());
    }

    #[test]
    fn build_request_body_omits_tool_choice_when_no_tools() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        assert!(serialized.get("tool_choice").is_none());
    }

    #[test]
    fn test_ws_endpoint_converts_https_to_wss() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        assert_eq!(
            provider.ws_endpoint(),
            "wss://chatgpt.com/backend-api/codex/responses"
        );
    }

    #[test]
    fn test_ws_endpoint_converts_http_to_ws() {
        let provider = OpenAiResponsesProvider::new("token", "account")
            .unwrap()
            .with_base_url("http://localhost:8080/responses");
        assert_eq!(provider.ws_endpoint(), "ws://localhost:8080/responses");
    }

    #[test]
    fn test_extract_host_from_url() {
        assert_eq!(
            extract_host_from_url("wss://chatgpt.com/backend-api/codex/responses"),
            "chatgpt.com"
        );
        assert_eq!(
            extract_host_from_url("ws://localhost:8080/responses"),
            "localhost:8080"
        );
        assert_eq!(extract_host_from_url("chatgpt.com/path"), "chatgpt.com");
    }

    #[test]
    fn test_ws_request_body_wraps_in_response_create() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-5.3-codex".to_string(),
            messages: vec![crate::types::Message::user("hi")],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let payload = provider.build_ws_create_payload(&request).unwrap();

        assert_eq!(payload["type"], "response.create");
        assert!(payload.get("stream").is_none());
    }

    #[test]
    fn parse_response_extracts_tool_calls() {
        let body = ResponsesResponseBody {
            output: vec![ResponsesOutputItem {
                r#type: Some("function_call".to_string()),
                id: Some("call_123".to_string()),
                call_id: None,
                name: Some("emit_intent".to_string()),
                arguments: Some("{\"intent\":\"open\"}".to_string()),
                content: None,
                text: None,
            }],
            status: Some("completed".to_string()),
            usage: None,
        };

        let response = OpenAiResponsesProvider::parse_response(body);
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "call_123");
        assert_eq!(response.tool_calls[0].name, "emit_intent");
        assert_eq!(response.tool_calls[0].arguments["intent"], "open");
    }

    #[test]
    fn openai_responses_incomplete_status_maps_to_max_tokens() {
        let body = ResponsesResponseBody {
            output: Vec::new(),
            status: Some("incomplete".to_string()),
            usage: None,
        };

        let response = OpenAiResponsesProvider::parse_response(body);
        assert_eq!(response.stop_reason.as_deref(), Some("max_tokens"));
    }

    #[test]
    fn openai_responses_done_event_incomplete_maps_to_max_tokens() {
        let done_event = r#"{"type":"response.done","response":{"status":"incomplete","usage":{"input_tokens":5,"output_tokens":2}}}"#;
        let chunk = OpenAiResponsesProvider::chunk_from_event("response.done", done_event)
            .expect("done event should produce terminal chunk");

        let usage = chunk.usage.expect("done event should include usage");
        assert_eq!(usage.input_tokens, 5);
        assert_eq!(usage.output_tokens, 2);
        assert_eq!(chunk.stop_reason.as_deref(), Some("max_tokens"));
    }

    #[test]
    fn empty_credentials_rejected() {
        assert!(OpenAiResponsesProvider::new("", "account").is_err());
        assert!(OpenAiResponsesProvider::new("token", "").is_err());
    }

    #[test]
    fn test_chunk_from_event_handles_ws_frame_with_type_field() {
        let text_delta = r#"{"type":"response.output_text.delta","delta":"hello"}"#;
        let text_chunk =
            OpenAiResponsesProvider::chunk_from_event("response.output_text.delta", text_delta)
                .unwrap();
        assert_eq!(text_chunk.delta_content.as_deref(), Some("hello"));

        let args_delta = r#"{"type":"response.function_call_arguments.delta","item_id":"call_1","name":"emit_intent","delta":"{\"intent\":\"op"}"#;
        let args_chunk = OpenAiResponsesProvider::chunk_from_event(
            "response.function_call_arguments.delta",
            args_delta,
        )
        .unwrap();
        assert_eq!(args_chunk.tool_use_deltas.len(), 1);
        assert_eq!(args_chunk.tool_use_deltas[0].id.as_deref(), Some("call_1"));

        let done_event =
            r#"{"type":"response.done","response":{"usage":{"input_tokens":5,"output_tokens":2}}}"#;
        let done_chunk =
            OpenAiResponsesProvider::chunk_from_event("response.done", done_event).unwrap();
        let usage = done_chunk.usage.unwrap();
        assert_eq!(usage.input_tokens, 5);
        assert_eq!(usage.output_tokens, 2);

        let completed_event =
            r#"{"type":"response.completed","usage":{"input_tokens":3,"output_tokens":1}}"#;
        let completed_chunk =
            OpenAiResponsesProvider::chunk_from_event("response.completed", completed_event)
                .unwrap();
        let completed_usage = completed_chunk.usage.unwrap();
        assert_eq!(completed_usage.input_tokens, 3);
        assert_eq!(completed_usage.output_tokens, 1);
    }

    #[test]
    fn capabilities_report_temperature_unsupported() {
        let provider = OpenAiResponsesProvider::new("token", "acct_123").unwrap();
        let capabilities = provider.capabilities();

        assert!(!capabilities.supports_temperature);
        assert!(capabilities.requires_streaming);
    }

    #[test]
    fn policy_close_error_is_classified_for_http_retry() {
        let error = ws_close_error(Some(CloseFrame {
            code: CloseCode::Policy,
            reason: "".into(),
        }));

        assert!(should_retry_http_after_ws_error(&error));
        assert!(matches!(
            error,
            LlmError::Provider(message) if message.contains("1008")
        ));
    }

    #[test]
    fn non_policy_close_error_does_not_trigger_http_retry() {
        let error = ws_close_error(Some(CloseFrame {
            code: CloseCode::Away,
            reason: "bye".into(),
        }));

        assert!(!should_retry_http_after_ws_error(&error));
    }

    #[test]
    fn stream_required_http_error_triggers_streaming_retry() {
        let error = OpenAiResponsesProvider::map_http_error(
            StatusCode::BAD_REQUEST,
            r#"{"detail":"Stream must be set to true"}"#.to_string(),
        );

        assert!(should_retry_http_with_streaming(&error));
    }

    #[test]
    fn stream_required_http_retry_ignores_other_client_errors() {
        let error = OpenAiResponsesProvider::map_http_error(
            StatusCode::BAD_REQUEST,
            r#"{"detail":"different error"}"#.to_string(),
        );

        assert!(!should_retry_http_with_streaming(&error));
    }

    #[test]
    fn sse_frames_are_assembled_into_completion_response() {
        let mut accumulator = StreamAccumulator::default();
        let mut frames = vec![
            SseFrame::Data(r#"{"type":"response.output_text.delta","delta":"Hey"}"#.to_string()),
            SseFrame::Data(
                r#"{"type":"response.done","response":{"usage":{"input_tokens":7,"output_tokens":3}}}"#
                    .to_string(),
            ),
            SseFrame::Done,
        ];

        apply_sse_frames(&mut accumulator, &mut frames).expect("sse frames should parse");

        let response = completion_from_accumulator(accumulator);
        assert_eq!(
            response.content,
            vec![ContentBlock::Text {
                text: "Hey".to_string()
            }]
        );
        assert_eq!(response.stop_reason.as_deref(), Some("stop"));
        assert_eq!(
            response.usage,
            Some(Usage {
                input_tokens: 7,
                output_tokens: 3,
            })
        );
    }

    #[tokio::test]
    async fn stream_from_ws_policy_close_emits_error_instead_of_empty_stream() {
        let frames = stream::iter(vec![Ok::<WsMessage, tungstenite::Error>(WsMessage::Close(
            Some(CloseFrame {
                code: CloseCode::Policy,
                reason: "".into(),
            }),
        ))]);
        let ws_stream = OpenAiResponsesProvider::stream_from_ws(frames);
        pin_mut!(ws_stream);

        let first = ws_stream
            .next()
            .await
            .expect("policy close should emit an error item");

        assert!(matches!(
            first,
            Err(LlmError::Provider(message)) if message.contains("1008")
        ));
        assert!(ws_stream.next().await.is_none());
    }

    #[tokio::test]
    async fn stream_from_ws_end_before_terminal_emits_error() {
        let frames = stream::empty::<Result<WsMessage, tungstenite::Error>>();
        let ws_stream = OpenAiResponsesProvider::stream_from_ws(frames);
        pin_mut!(ws_stream);

        let first = ws_stream
            .next()
            .await
            .expect("early socket end should emit an error item");

        assert!(matches!(
            first,
            Err(LlmError::Provider(message)) if message.contains("terminal")
        ));
        assert!(ws_stream.next().await.is_none());
    }

    fn reconstructed_tool_calls(events: &[(&str, &str)]) -> Vec<ToolCall> {
        let mut pending = Vec::new();
        for (event_type, frame) in events {
            if let Some(chunk) = OpenAiResponsesProvider::chunk_from_event(event_type, frame) {
                merge_tool_use_deltas(&mut pending, chunk.tool_use_deltas);
            }
        }
        finalize_tool_calls(pending)
    }

    #[test]
    fn chunk_from_event_maps_function_call_output_item_done() {
        let item_done = r#"{"type":"response.output_item.done","item":{"type":"function_call","id":"call_99","name":"lookup","arguments":"{\"q\":\"weather\"}"}}"#;
        let chunk =
            OpenAiResponsesProvider::chunk_from_event("response.output_item.done", item_done)
                .unwrap();

        assert_eq!(chunk.tool_use_deltas.len(), 1);
        assert_eq!(chunk.tool_use_deltas[0].id.as_deref(), Some("call_99"));
        assert_eq!(
            chunk.tool_use_deltas[0].provider_id.as_deref(),
            Some("call_99")
        );
        assert_eq!(chunk.tool_use_deltas[0].name.as_deref(), Some("lookup"));
    }

    #[test]
    fn chunk_from_event_maps_function_call_output_item_call_id() {
        let item_done = r#"{"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_77","name":"lookup","arguments":"{\"q\":\"weather\"}"}}"#;
        let chunk =
            OpenAiResponsesProvider::chunk_from_event("response.output_item.done", item_done)
                .unwrap();

        assert_eq!(chunk.tool_use_deltas.len(), 1);
        assert_eq!(chunk.tool_use_deltas[0].id.as_deref(), Some("call_77"));
        assert!(chunk.tool_use_deltas[0].provider_id.is_none());
        assert_eq!(chunk.tool_use_deltas[0].name.as_deref(), Some("lookup"));
    }

    #[test]
    fn chunk_from_event_prefers_call_id_when_both_identifiers_exist() {
        let item_done = r#"{"type":"response.output_item.done","item":{"type":"function_call","id":"fc_123","call_id":"call_123","name":"lookup","arguments":"{\"q\":\"weather\"}"}}"#;
        let chunk =
            OpenAiResponsesProvider::chunk_from_event("response.output_item.done", item_done)
                .unwrap();

        assert_eq!(chunk.tool_use_deltas.len(), 1);
        assert_eq!(chunk.tool_use_deltas[0].id.as_deref(), Some("call_123"));
        assert_eq!(
            chunk.tool_use_deltas[0].provider_id.as_deref(),
            Some("fc_123")
        );
    }

    #[test]
    fn tool_required_stream_events_preserve_tool_call_for_continuation() {
        let events = vec![
            (
                "response.output_item.added",
                r#"{"type":"response.output_item.added","item":{"type":"function_call","id":"call_1","name":"emit_intent","arguments":""}}"#,
            ),
            (
                "response.function_call_arguments.delta",
                r#"{"type":"response.function_call_arguments.delta","item_id":"call_1","delta":"{\"intent\":\"op"}"#,
            ),
            (
                "response.function_call_arguments.delta",
                r#"{"type":"response.function_call_arguments.delta","item_id":"call_1","delta":"en\"}"}"#,
            ),
            (
                "response.function_call_arguments.done",
                r#"{"type":"response.function_call_arguments.done","item_id":"call_1","arguments":"{\"intent\":\"open\"}"}"#,
            ),
        ];

        let tool_calls = reconstructed_tool_calls(&events);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "emit_intent");
        assert_eq!(tool_calls[0].arguments["intent"], "open");
    }

    #[test]
    fn merge_tool_use_deltas_handles_id_arriving_after_arguments() {
        let events = vec![
            (
                "response.function_call_arguments.delta",
                r#"{"type":"response.function_call_arguments.delta","name":"emit_intent","delta":"{\"intent\":\"o"}"#,
            ),
            (
                "response.output_item.added",
                r#"{"type":"response.output_item.added","item":{"type":"function_call","id":"call_2","name":"emit_intent","arguments":""}}"#,
            ),
            (
                "response.function_call_arguments.delta",
                r#"{"type":"response.function_call_arguments.delta","item_id":"call_2","delta":"pen\"}"}"#,
            ),
            (
                "response.function_call_arguments.done",
                r#"{"type":"response.function_call_arguments.done","item_id":"call_2","arguments":"{\"intent\":\"open\"}"}"#,
            ),
        ];

        let tool_calls = reconstructed_tool_calls(&events);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_2");
        assert_eq!(tool_calls[0].name, "emit_intent");
        assert_eq!(tool_calls[0].arguments["intent"], "open");
    }

    #[test]
    fn merge_tool_use_deltas_accepts_call_id_argument_events() {
        let events = vec![
            (
                "response.output_item.added",
                r#"{"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_3","name":"emit_intent","arguments":""}}"#,
            ),
            (
                "response.function_call_arguments.done",
                r#"{"type":"response.function_call_arguments.done","call_id":"call_3","arguments":"{\"intent\":\"open\"}"}"#,
            ),
        ];

        let tool_calls = reconstructed_tool_calls(&events);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_3");
        assert_eq!(tool_calls[0].name, "emit_intent");
        assert_eq!(tool_calls[0].arguments["intent"], "open");
    }

    #[test]
    fn merge_tool_use_deltas_done_event_with_full_arguments_does_not_double_append() {
        let events = vec![
            (
                "response.output_item.added",
                r#"{"type":"response.output_item.added","item":{"type":"function_call","id":"call_4","name":"emit_intent","arguments":""}}"#,
            ),
            (
                "response.function_call_arguments.delta",
                r#"{"type":"response.function_call_arguments.delta","item_id":"call_4","delta":"{\"intent\":\"op"}"#,
            ),
            (
                "response.function_call_arguments.delta",
                r#"{"type":"response.function_call_arguments.delta","item_id":"call_4","delta":"en\"}"}"#,
            ),
            (
                "response.function_call_arguments.done",
                r#"{"type":"response.function_call_arguments.done","item_id":"call_4","arguments":"{\"intent\":\"open\"}"}"#,
            ),
        ];

        let tool_calls = reconstructed_tool_calls(&events);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_4");
        assert_eq!(tool_calls[0].name, "emit_intent");
        assert_eq!(tool_calls[0].arguments["intent"], "open");
        let expected = serde_json::from_str::<Value>(r#"{"intent":"open"}"#).unwrap();
        assert_eq!(tool_calls[0].arguments, expected);
    }

    #[test]
    fn output_item_done_does_not_double_append_arguments() {
        let full_args = r#"{"intent":"open"}"#;
        let events = vec![
            (
                "response.output_item.added",
                r#"{"type":"response.output_item.added","item":{"type":"function_call","id":"call_5","name":"emit_intent","arguments":""}}"#,
            ),
            (
                "response.function_call_arguments.delta",
                r#"{"type":"response.function_call_arguments.delta","item_id":"call_5","delta":"{\"intent\":\"op"}"#,
            ),
            (
                "response.function_call_arguments.delta",
                r#"{"type":"response.function_call_arguments.delta","item_id":"call_5","delta":"en\"}"}"#,
            ),
            (
                "response.function_call_arguments.done",
                r#"{"type":"response.function_call_arguments.done","item_id":"call_5","arguments":"{\"intent\":\"open\"}"}"#,
            ),
            (
                "response.output_item.done",
                r#"{"type":"response.output_item.done","item":{"type":"function_call","id":"call_5","name":"emit_intent","arguments":"{\"intent\":\"open\"}"}}"#,
            ),
        ];

        let tool_calls = reconstructed_tool_calls(&events);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_5");
        assert_eq!(tool_calls[0].name, "emit_intent");
        let expected = serde_json::from_str::<Value>(full_args).unwrap();
        assert_eq!(tool_calls[0].arguments, expected);
    }

    #[test]
    fn build_request_body_skips_empty_tool_result_call_id() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![crate::types::Message {
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "".to_string(),
                    content: Value::String("tool output".to_string()),
                }],
            }],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();
        assert!(input.is_empty());
    }

    #[test]
    fn merge_tool_use_deltas_collects_complete_tool_call() {
        let mut pending = Vec::new();
        merge_tool_use_deltas(
            &mut pending,
            vec![ToolUseDelta {
                id: Some("call_1".to_string()),
                provider_id: None,
                name: Some("emit_intent".to_string()),
                arguments_delta: Some("{\"intent\":\"op".to_string()),
                arguments_done: false,
            }],
        );
        merge_tool_use_deltas(
            &mut pending,
            vec![ToolUseDelta {
                id: Some("call_1".to_string()),
                provider_id: None,
                name: Some("emit_intent".to_string()),
                arguments_delta: Some("en\"}".to_string()),
                arguments_done: false,
            }],
        );

        let tool_calls = finalize_tool_calls(pending);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "emit_intent");
        assert_eq!(tool_calls[0].arguments["intent"], "open");
    }

    #[test]
    fn merge_tool_use_deltas_merges_when_name_arrives_later() {
        let mut pending = Vec::new();
        merge_tool_use_deltas(
            &mut pending,
            vec![ToolUseDelta {
                id: Some("call_1".to_string()),
                provider_id: None,
                name: None,
                arguments_delta: Some("{\"intent\":\"op".to_string()),
                arguments_done: false,
            }],
        );
        merge_tool_use_deltas(
            &mut pending,
            vec![ToolUseDelta {
                id: Some("call_1".to_string()),
                provider_id: None,
                name: Some("emit_intent".to_string()),
                arguments_delta: Some("en\"}".to_string()),
                arguments_done: false,
            }],
        );

        let tool_calls = finalize_tool_calls(pending);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "emit_intent");
        assert_eq!(tool_calls[0].arguments["intent"], "open");
    }

    #[test]
    fn merge_arguments_concatenates_without_dedup() {
        let mut arguments = String::new();
        merge_arguments(&mut arguments, "{\"findings\":[{", false);
        merge_arguments(&mut arguments, "\"confidence\":\"high\"", false);
        merge_arguments(&mut arguments, "}]}", false);

        assert_eq!(arguments, "{\"findings\":[{\"confidence\":\"high\"}]}");
        let parsed = serde_json::from_str::<Value>(&arguments).expect("valid json");
        assert_eq!(parsed["findings"][0]["confidence"], "high");
    }

    #[test]
    fn merge_arguments_handles_overlapping_like_chunks() {
        let mut arguments = String::new();
        merge_arguments(&mut arguments, "{\"a\":", false);
        merge_arguments(&mut arguments, "{\"a\":\"b\"}", false);

        assert_eq!(arguments, "{\"a\":{\"a\":\"b\"}");
    }

    // -- Regression tests for #1118: empty args for zero-param tools --

    #[test]
    fn finalize_tool_calls_normalizes_empty_arguments_to_empty_object() {
        let pending = vec![PendingToolCall {
            id: Some("call_1".to_string()),
            provider_id: None,
            name: Some("git_status".to_string()),
            arguments: String::new(),
        }];
        let tool_calls = finalize_tool_calls(pending);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "git_status");
        assert_eq!(
            tool_calls[0].arguments,
            Value::Object(serde_json::Map::new()),
            "empty arguments should be normalized to {{}}"
        );
    }

    #[test]
    fn finalize_tool_calls_normalizes_whitespace_arguments_to_empty_object() {
        let pending = vec![PendingToolCall {
            id: Some("call_1".to_string()),
            provider_id: None,
            name: Some("current_time".to_string()),
            arguments: "  \t\n  ".to_string(),
        }];
        let tool_calls = finalize_tool_calls(pending);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(
            tool_calls[0].arguments,
            Value::Object(serde_json::Map::new()),
            "whitespace-only arguments should be normalized to {{}}"
        );
    }

    #[test]
    fn collect_tool_calls_normalizes_empty_arguments_to_empty_object() {
        let output = vec![ResponsesOutputItem {
            r#type: Some("function_call".to_string()),
            id: Some("call_abc".to_string()),
            call_id: None,
            name: Some("get_weather".to_string()),
            arguments: Some("".to_string()),
            content: None,
            text: None,
        }];
        let tool_calls = collect_tool_calls(&output);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "get_weather");
        assert_eq!(
            tool_calls[0].arguments,
            Value::Object(serde_json::Map::new()),
            "empty arguments in collect_tool_calls should be normalized to {{}}"
        );
    }

    #[test]
    fn collect_tool_calls_preserves_valid_json_arguments() {
        let output = vec![ResponsesOutputItem {
            r#type: Some("function_call".to_string()),
            id: Some("call_def".to_string()),
            call_id: None,
            name: Some("search".to_string()),
            arguments: Some(r#"{"query":"rust"}"#.to_string()),
            content: None,
            text: None,
        }];
        let tool_calls = collect_tool_calls(&output);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(
            tool_calls[0].arguments,
            serde_json::json!({"query": "rust"}),
            "valid JSON arguments should be parsed as-is"
        );
    }

    #[test]
    fn merge_tool_use_deltas_promotes_call_id_over_provider_item_id() {
        let mut pending = Vec::new();
        merge_tool_use_deltas(
            &mut pending,
            vec![ToolUseDelta {
                id: Some("fc_123".to_string()),
                provider_id: Some("fc_123".to_string()),
                name: Some("emit_intent".to_string()),
                arguments_delta: Some("{\"intent\":\"op".to_string()),
                arguments_done: false,
            }],
        );
        merge_tool_use_deltas(
            &mut pending,
            vec![ToolUseDelta {
                id: Some("call_123".to_string()),
                provider_id: Some("fc_123".to_string()),
                name: Some("emit_intent".to_string()),
                arguments_delta: Some("en\"}".to_string()),
                arguments_done: false,
            }],
        );

        let tool_calls = finalize_tool_calls(pending);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_123");
        assert_eq!(tool_calls[0].arguments["intent"], "open");
    }

    #[test]
    fn collect_tool_calls_prefers_call_id_when_present() {
        let output = vec![ResponsesOutputItem {
            r#type: Some("function_call".to_string()),
            id: Some("fc_123".to_string()),
            call_id: Some("call_123".to_string()),
            name: Some("search".to_string()),
            arguments: Some(r#"{"query":"rust"}"#.to_string()),
            content: None,
            text: None,
        }];

        let tool_calls = collect_tool_calls(&output);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_123");
    }

    #[test]
    fn build_request_normalizes_cross_provider_tool_ids() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![
                crate::types::Message {
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id: "toolu_011VJLabcdef".to_string(),
                        provider_id: None,
                        name: "read_file".to_string(),
                        input: serde_json::json!({"path": "/tmp/a"}),
                    }],
                },
                crate::types::Message {
                    role: MessageRole::Tool,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "toolu_011VJLabcdef".to_string(),
                        content: serde_json::json!("file contents"),
                    }],
                },
            ],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(input[0]["call_id"], "fc_011VJLabcdef");
        assert_eq!(input[1]["call_id"], "fc_011VJLabcdef");
    }
}
