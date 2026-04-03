//! OpenAI-compatible chat completions provider.
//!
//! Supports OpenAI and OpenRouter style APIs via configurable `base_url`.

use async_trait::async_trait;
use futures::{stream, Stream, StreamExt};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Duration;

use crate::document::document_text_fallback;
use crate::openai_common::{filter_model_ids, OpenAiModelsResponse};
use crate::provider::{
    bearer_auth_headers, insert_header_value, null_loop_harness,
    resolve_loop_harness_from_profiles, CompletionStream, LlmProvider, LoopHarness, LoopModelMatch,
    LoopModelProfile, LoopPromptOverlayContext, ProviderCapabilities, ProviderCatalogFilters,
    StaticLoopModelProfile,
};
use crate::sse::{SseFrame, SseFramer};
use crate::streaming::{collect_completion_stream, StreamCallback};
use crate::thinking::valid_thinking_levels;
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message, MessageRole,
    StreamChunk, ToolCall, ToolUseDelta, Usage,
};

const GPT_REASONING_OVERLAY: &str = "\n\nModel-family guidance for GPT-5/Codex reasoning models: \
When work clearly splits into independent streams, actually use `spawn_agent` / `subagent_status` instead of only describing a parallel plan. \
If the user names an exact command or workflow, execute that exact path before exploring alternatives unless you hit a concrete blocker. \
If you are blocked, state the blocker plainly and ask for direction rather than ending on promise language like \"Let me...\" without taking the next action.";

const GPT_TOOL_CONTINUATION_OVERLAY: &str = "\n\nModel-family guidance for GPT-5/Codex reasoning models: \
After tool calls, turn the evidence into either a direct answer or an explicit blocker. \
Do not emit planning-only text or future-tense promises unless you are also making the next tool call in the same response.";

#[derive(Debug)]
struct OpenAiChatLoopHarness {
    use_reasoning_overlays: bool,
}

impl LoopHarness for OpenAiChatLoopHarness {
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
}

static OPENAI_CHAT_LOOP_HARNESS: OpenAiChatLoopHarness = OpenAiChatLoopHarness {
    use_reasoning_overlays: false,
};

static OPENAI_REASONING_CHAT_LOOP_HARNESS: OpenAiChatLoopHarness = OpenAiChatLoopHarness {
    use_reasoning_overlays: true,
};

static OPENAI_REASONING_CHAT_LOOP_PROFILE: StaticLoopModelProfile = StaticLoopModelProfile {
    label: "openai_reasoning",
    matcher: LoopModelMatch::AnyPrefix(&["gpt-5.4", "gpt-5.2", "gpt-5", "codex-", "o1", "o3"]),
    harness: &OPENAI_REASONING_CHAT_LOOP_HARNESS,
};

static OPENAI_DEFAULT_CHAT_LOOP_PROFILE: StaticLoopModelProfile = StaticLoopModelProfile {
    label: "openai_default",
    matcher: LoopModelMatch::Any,
    harness: &OPENAI_CHAT_LOOP_HARNESS,
};

static OPENAI_CHAT_LOOP_PROFILES: [&'static dyn LoopModelProfile; 2] = [
    &OPENAI_REASONING_CHAT_LOOP_PROFILE,
    &OPENAI_DEFAULT_CHAT_LOOP_PROFILE,
];

fn openai_chat_loop_harness(model: &str) -> &'static dyn LoopHarness {
    resolve_loop_harness_from_profiles(&OPENAI_CHAT_LOOP_PROFILES, model, null_loop_harness())
}

pub(crate) const OPENAI_THINKING_LEVELS: &[&str] = &["off", "low", "high"];
const OPENROUTER_THINKING_LEVELS: &[&str] = &["off"];
pub(crate) const OPENAI_FALLBACK_MODELS: &[&str] = &[
    "gpt-5.4",
    "gpt-4.1",
    "o3",
    "o4-mini",
    "gpt-4o",
    "gpt-4o-mini",
];
const OPENROUTER_FALLBACK_MODELS: &[&str] = &[
    "anthropic/claude-sonnet-4",
    "openai/gpt-4o",
    "x-ai/grok-3",
    "qwen/qwen-2.5-72b-instruct",
    "deepseek/deepseek-chat-v3",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAiCatalogKind {
    Compatible,
    OpenAi,
    OpenRouter,
}

impl OpenAiCatalogKind {
    fn is_openrouter(self) -> bool {
        matches!(self, Self::OpenRouter)
    }
}

pub(crate) fn openai_models_endpoint(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    if base_url.ends_with("/v1") {
        format!("{base_url}/models")
    } else {
        format!("{base_url}/v1/models")
    }
}

pub(crate) fn is_openai_chat_capable(model_id: &str) -> bool {
    let id = model_id.to_ascii_lowercase();
    let includes = id.starts_with("gpt-")
        || id.starts_with("gpt-5")
        || id.starts_with("o1")
        || id.starts_with("o3")
        || id.starts_with("o4");
    let excludes = id.contains("embedding")
        || id.contains("tts")
        || id.contains("whisper")
        || id.contains("dall-e")
        || id.contains("moderation")
        || id.contains("audio")
        || id.contains("realtime")
        || id.contains("search")
        || id.contains("instruct");
    includes && !excludes
}

pub(crate) fn openai_thinking_levels(model_id: &str) -> &'static [&'static str] {
    valid_thinking_levels(model_id)
}

pub(crate) fn openai_context_window(model_id: &str) -> usize {
    let id = model_id.to_ascii_lowercase();
    if id.contains("claude-opus") || id.contains("claude-sonnet") || id.contains("claude-haiku") {
        return 200_000;
    }
    if id.contains("deepseek") {
        return 64_000;
    }
    if id.contains("gemini") {
        return 1_000_000;
    }
    128_000
}

fn is_openrouter_chat_capable(model_id: &str) -> bool {
    let id = model_id.to_ascii_lowercase();
    id.contains("claude")
        || id.contains("gpt-")
        || id.contains("o4")
        || id.contains("grok")
        || id.contains("qwen")
        || id.contains("minimax")
        || id.contains("liquidai")
        || id.contains("lfm")
        || id.contains("deepseek")
}

/// OpenAI-compatible provider implementation.
#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    base_url: String,
    models_endpoint: String,
    api_key: String,
    catalog_kind: OpenAiCatalogKind,
    auth_method: &'static str,
    provider_name: String,
    supported_models: Vec<String>,
    /// ChatGPT account ID for subscription OAuth (sent as `chatgpt-account-id` header).
    account_id: Option<String>,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub const fn default_base_url() -> &'static str {
        "https://api.openai.com"
    }

    pub const fn openrouter_base_url() -> &'static str {
        "https://openrouter.ai/api"
    }

    /// Create a new OpenAI-compatible provider.
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self, LlmError> {
        Self::compatible(base_url, api_key, "openai-compatible")
    }

    pub fn compatible(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        provider_name: impl Into<String>,
    ) -> Result<Self, LlmError> {
        Self::build(
            base_url.into(),
            api_key.into(),
            OpenAiCatalogKind::Compatible,
            provider_name.into(),
        )
    }

    pub fn openai(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, LlmError> {
        Self::build(
            base_url.into(),
            api_key.into(),
            OpenAiCatalogKind::OpenAi,
            "openai".to_string(),
        )
    }

    pub fn openrouter(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, LlmError> {
        Self::build(
            base_url.into(),
            api_key.into(),
            OpenAiCatalogKind::OpenRouter,
            "openrouter".to_string(),
        )
    }

    fn build(
        base_url: String,
        api_key: String,
        catalog_kind: OpenAiCatalogKind,
        provider_name: String,
    ) -> Result<Self, LlmError> {
        if base_url.trim().is_empty() {
            return Err(LlmError::Config("base_url cannot be empty".to_string()));
        }

        if api_key.trim().is_empty() {
            return Err(LlmError::Config("api_key cannot be empty".to_string()));
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1800))
            .build()
            .map_err(|error| LlmError::Config(format!("failed to build HTTP client: {error}")))?;
        let models_endpoint = openai_models_endpoint(&base_url);

        Ok(Self {
            base_url,
            models_endpoint,
            api_key,
            catalog_kind,
            auth_method: "api_key",
            provider_name,
            supported_models: Vec::new(),
            account_id: None,
            client,
        })
    }

    /// Override provider name for logs/metrics without changing provider behavior.
    pub fn with_name(mut self, provider_name: impl Into<String>) -> Self {
        self.provider_name = provider_name.into();
        self
    }

    pub fn with_auth_method(mut self, auth_method: &'static str) -> Self {
        self.auth_method = auth_method;
        self
    }

    /// Set explicit supported models list.
    pub fn with_supported_models(mut self, supported_models: Vec<String>) -> Self {
        self.supported_models = supported_models;
        self
    }

    /// Set ChatGPT account ID for subscription OAuth (sent as `chatgpt-account-id` header).
    pub fn with_account_id(mut self, account_id: impl Into<String>) -> Self {
        self.account_id = Some(account_id.into());
        self
    }

    fn endpoint(&self) -> String {
        let base_url = self.base_url.trim_end_matches('/');
        if base_url.ends_with("/v1") {
            format!("{base_url}/chat/completions")
        } else {
            format!("{base_url}/v1/chat/completions")
        }
    }

    async fn fetch_models(&self) -> Result<Vec<String>, LlmError> {
        let response = self
            .client
            .get(&self.models_endpoint)
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        parse_model_response(response, self, &self.supported_models).await
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
    ) -> Result<OpenAiRequestBody, LlmError> {
        self.ensure_supported_model(&request.model)?;

        let mut messages = map_messages_to_openai(&request.messages)?;

        if let Some(system_prompt) = &request.system_prompt {
            messages.insert(
                0,
                OpenAiMessage {
                    role: "system".to_string(),
                    content: Some(OpenAiMessageContent::Text(system_prompt.clone())),
                    tool_calls: None,
                    tool_call_id: None,
                },
            );
        }

        let tools = request
            .tools
            .iter()
            .map(|tool| OpenAiTool {
                tool_type: "function".to_string(),
                function: OpenAiToolFunction {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.parameters.clone(),
                },
            })
            .collect::<Vec<_>>();

        Ok(OpenAiRequestBody {
            model: request.model.clone(),
            messages,
            tools,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream,
        })
    }

    fn parse_completion_response(body: OpenAiResponseBody) -> Result<CompletionResponse, LlmError> {
        let choice = body
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::InvalidResponse("missing choices".to_string()))?;

        let mut content = Vec::new();
        let mut tool_calls = Vec::new();

        if let Some(OpenAiMessageContent::Text(text)) = choice.message.content {
            if !text.is_empty() {
                content.push(ContentBlock::Text { text });
            }
        }

        if let Some(calls) = choice.message.tool_calls {
            for call in calls {
                let arguments = parse_json_or_string(&call.function.arguments);
                content.push(ContentBlock::ToolUse {
                    id: call.id.clone(),
                    provider_id: None,
                    name: call.function.name.clone(),
                    input: arguments.clone(),
                });
                tool_calls.push(ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments,
                });
            }
        }

        Ok(CompletionResponse {
            content,
            tool_calls,
            usage: body.usage.map(|usage| Usage {
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
            }),
            stop_reason: choice.finish_reason,
        })
    }

    #[allow(dead_code)]
    fn parse_sse_payload(payload: &str) -> Result<Vec<StreamChunk>, LlmError> {
        let mut framer = SseFramer::default();
        let mut chunks = Vec::new();
        let mut tool_calls_by_index = HashMap::new();

        for line in payload.lines() {
            let mut framed = framer.push_bytes(format!("{line}\n").as_bytes())?;
            let mut parsed = Self::map_sse_frames(&mut framed, &mut tool_calls_by_index)?;
            chunks.append(&mut parsed);
        }

        let mut final_frames = framer.finish()?;
        let mut parsed = Self::map_sse_frames(&mut final_frames, &mut tool_calls_by_index)?;
        chunks.append(&mut parsed);
        Ok(chunks)
    }

    fn parse_sse_data(
        data: &str,
        tool_calls_by_index: &mut HashMap<usize, OpenAiToolStreamState>,
    ) -> Result<Vec<StreamChunk>, LlmError> {
        let envelope: OpenAiStreamEnvelope = serde_json::from_str(data)
            .map_err(|error| LlmError::Streaming(format!("invalid SSE JSON: {error}")))?;
        let mut chunks = Vec::new();

        maybe_push_usage_chunk(&mut chunks, envelope.usage);
        for choice in envelope.choices {
            maybe_push_text_chunk(&mut chunks, choice.delta.content);
            maybe_push_tool_chunk(&mut chunks, choice.delta.tool_calls, tool_calls_by_index);
            maybe_push_stop_chunk(&mut chunks, choice.finish_reason);
        }

        Ok(chunks)
    }

    fn stream_from_sse(
        response: reqwest::Response,
    ) -> impl Stream<Item = Result<StreamChunk, LlmError>> + Send {
        stream::unfold(
            OpenAiSseState::new(response.bytes_stream()),
            |mut state| async move {
                loop {
                    if let Some(chunk) = state.pending_chunks.pop_front() {
                        return Some((chunk, state));
                    }

                    if state.finished {
                        return None;
                    }

                    match state.bytes_stream.as_mut().next().await {
                        Some(Ok(bytes)) => {
                            let mut frames = match state.framer.push_bytes(&bytes) {
                                Ok(frames) => frames,
                                Err(error) => {
                                    state.finished = true;
                                    return Some((Err(error), state));
                                }
                            };

                            match Self::map_sse_frames(&mut frames, &mut state.tool_calls_by_index)
                            {
                                Ok(chunks) => {
                                    state.pending_chunks.extend(chunks.into_iter().map(Ok))
                                }
                                Err(error) => {
                                    state.finished = true;
                                    return Some((Err(error), state));
                                }
                            }
                        }
                        Some(Err(error)) => {
                            state.finished = true;
                            return Some((Err(LlmError::Streaming(error.to_string())), state));
                        }
                        None => {
                            let mut frames = match state.framer.finish() {
                                Ok(frames) => frames,
                                Err(error) => {
                                    state.finished = true;
                                    return Some((Err(error), state));
                                }
                            };

                            match Self::map_sse_frames(&mut frames, &mut state.tool_calls_by_index)
                            {
                                Ok(chunks) => {
                                    state.pending_chunks.extend(chunks.into_iter().map(Ok))
                                }
                                Err(error) => {
                                    state.finished = true;
                                    return Some((Err(error), state));
                                }
                            }

                            state.finished = true;
                        }
                    }
                }
            },
        )
    }

    fn map_sse_frames(
        frames: &mut Vec<SseFrame>,
        tool_calls_by_index: &mut HashMap<usize, OpenAiToolStreamState>,
    ) -> Result<Vec<StreamChunk>, LlmError> {
        let mut chunks = Vec::new();
        for frame in frames.drain(..) {
            match frame {
                SseFrame::Data(data) => {
                    let mut parsed = Self::parse_sse_data(&data, tool_calls_by_index)?;
                    chunks.append(&mut parsed);
                }
                SseFrame::Done => break,
            }
        }
        Ok(chunks)
    }

    fn update_tool_state(
        tool_calls_by_index: &mut HashMap<usize, OpenAiToolStreamState>,
        delta: &OpenAiToolCallDelta,
    ) -> OpenAiToolStreamState {
        let state = if let Some(index) = delta.index {
            tool_calls_by_index.entry(index).or_default()
        } else {
            tracing::warn!(?delta.index, "openai tool delta missing index; state may be incomplete");
            tool_calls_by_index.entry(usize::MAX).or_default()
        };

        if let Some(id) = &delta.id {
            state.id = Some(id.clone());
        }
        if let Some(function) = &delta.function {
            if let Some(name) = &function.name {
                state.name = Some(name.clone());
            }
        }

        state.clone()
    }

    fn map_tool_delta(
        delta: OpenAiToolCallDelta,
        tool_calls_by_index: &mut HashMap<usize, OpenAiToolStreamState>,
    ) -> ToolUseDelta {
        let state = Self::update_tool_state(tool_calls_by_index, &delta);
        let arguments_delta = delta.function.and_then(|function| function.arguments);

        ToolUseDelta {
            id: state.id,
            provider_id: None,
            name: state.name,
            arguments_delta,
            arguments_done: false,
        }
    }

    fn map_tool_deltas(
        tool_calls: Vec<OpenAiToolCallDelta>,
        tool_calls_by_index: &mut HashMap<usize, OpenAiToolStreamState>,
    ) -> Vec<ToolUseDelta> {
        tool_calls
            .into_iter()
            .map(|tool_call| Self::map_tool_delta(tool_call, tool_calls_by_index))
            .collect()
    }

    fn map_http_error(status: StatusCode, body: String) -> LlmError {
        match status.as_u16() {
            401 | 403 => LlmError::Authentication(body),
            429 => LlmError::RateLimited(body),
            400..=499 => LlmError::Request(format!("client error {}: {body}", status.as_u16())),
            500..=599 => LlmError::Provider(format!("server error {}: {body}", status.as_u16())),
            _ => LlmError::Request(format!("http {}: {body}", status.as_u16())),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let body = self.build_request_body(&request, false)?;

        let mut builder = self.client.post(self.endpoint()).bearer_auth(&self.api_key);
        if let Some(ref account_id) = self.account_id {
            builder = builder.header("chatgpt-account-id", account_id);
        }
        let response = builder.json(&body).send().await?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("unable to read error body: {error}"));
            return Err(Self::map_http_error(status, body));
        }

        let parsed = response
            .json::<OpenAiResponseBody>()
            .await
            .map_err(|error| LlmError::InvalidResponse(error.to_string()))?;

        Self::parse_completion_response(parsed)
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, LlmError> {
        let body = self.build_request_body(&request, true)?;

        let mut builder = self.client.post(self.endpoint()).bearer_auth(&self.api_key);
        if let Some(ref account_id) = self.account_id {
            builder = builder.header("chatgpt-account-id", account_id);
        }
        let response = builder.json(&body).send().await?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("unable to read error body: {error}"));
            return Err(Self::map_http_error(status, body));
        }

        Ok(Box::pin(Self::stream_from_sse(response)))
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        callback: StreamCallback,
    ) -> Result<CompletionResponse, LlmError> {
        let mut stream = self.complete_stream(request).await?;
        collect_completion_stream(&mut stream, &callback).await
    }

    fn name(&self) -> &str {
        &self.provider_name
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported_models.clone()
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        if self.api_key.trim().is_empty() {
            return Ok(self.supported_models());
        }

        match self.fetch_models().await {
            Ok(models) if !models.is_empty() => Ok(models),
            Ok(_) => {
                tracing::warn!(provider = %self.provider_name, "openai models response was empty; using static fallback");
                Ok(self.supported_models())
            }
            Err(error) => {
                tracing::warn!(provider = %self.provider_name, error = %error, "failed to fetch openai models; using static fallback");
                Ok(self.supported_models())
            }
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_temperature: true,
            requires_streaming: false,
        }
    }

    fn supported_thinking_levels(&self) -> &'static [&'static str] {
        if self.catalog_kind.is_openrouter() {
            OPENROUTER_THINKING_LEVELS
        } else {
            OPENAI_THINKING_LEVELS
        }
    }

    fn thinking_levels(&self, model: &str) -> &'static [&'static str] {
        if self.catalog_kind.is_openrouter() {
            OPENROUTER_THINKING_LEVELS
        } else {
            openai_thinking_levels(model)
        }
    }

    fn models_endpoint(&self) -> Option<&str> {
        Some(&self.models_endpoint)
    }

    fn auth_method(&self) -> &'static str {
        self.auth_method
    }

    fn catalog_auth_headers(
        &self,
        api_key: &str,
        _auth_mode: &str,
    ) -> Result<reqwest::header::HeaderMap, String> {
        let mut headers = bearer_auth_headers(api_key)?;
        if let Some(account_id) = &self.account_id {
            insert_header_value(&mut headers, "chatgpt-account-id", account_id, "account id")?;
        }
        Ok(headers)
    }

    fn is_chat_capable(&self, model_id: &str) -> bool {
        if self.catalog_kind.is_openrouter() {
            is_openrouter_chat_capable(model_id)
        } else {
            is_openai_chat_capable(model_id)
        }
    }

    fn fallback_models(&self) -> Vec<&'static str> {
        if self.catalog_kind.is_openrouter() {
            OPENROUTER_FALLBACK_MODELS.to_vec()
        } else {
            OPENAI_FALLBACK_MODELS.to_vec()
        }
    }

    fn catalog_filters(&self) -> ProviderCatalogFilters {
        ProviderCatalogFilters {
            apply_recency_and_price_floor: self.catalog_kind.is_openrouter(),
        }
    }

    fn context_window(&self, model: &str) -> usize {
        openai_context_window(model)
    }

    fn loop_harness(&self, model: &str) -> &'static dyn LoopHarness {
        openai_chat_loop_harness(model)
    }
}

async fn parse_model_response(
    response: reqwest::Response,
    provider: &OpenAiProvider,
    supported_models: &[String],
) -> Result<Vec<String>, LlmError> {
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|error| format!("unable to read error body: {error}"));
        return Err(OpenAiProvider::map_http_error(status, body));
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

fn maybe_push_usage_chunk(chunks: &mut Vec<StreamChunk>, usage: Option<OpenAiUsage>) {
    let Some(usage) = usage else {
        return;
    };
    chunks.push(StreamChunk {
        delta_content: None,
        tool_use_deltas: Vec::new(),
        usage: Some(Usage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
        }),
        stop_reason: None,
    });
}

fn maybe_push_text_chunk(chunks: &mut Vec<StreamChunk>, content: Option<String>) {
    let Some(content) = content else {
        return;
    };
    chunks.push(StreamChunk {
        delta_content: Some(content),
        tool_use_deltas: Vec::new(),
        usage: None,
        stop_reason: None,
    });
}

fn maybe_push_tool_chunk(
    chunks: &mut Vec<StreamChunk>,
    tool_calls: Option<Vec<OpenAiToolCallDelta>>,
    tool_calls_by_index: &mut HashMap<usize, OpenAiToolStreamState>,
) {
    let Some(tool_calls) = tool_calls else {
        return;
    };
    let deltas = OpenAiProvider::map_tool_deltas(tool_calls, tool_calls_by_index);
    if deltas.is_empty() {
        return;
    }

    chunks.push(StreamChunk {
        delta_content: None,
        tool_use_deltas: deltas,
        usage: None,
        stop_reason: None,
    });
}

fn maybe_push_stop_chunk(chunks: &mut Vec<StreamChunk>, stop_reason: Option<String>) {
    let Some(stop_reason) = stop_reason else {
        return;
    };
    chunks.push(StreamChunk {
        delta_content: None,
        tool_use_deltas: Vec::new(),
        usage: None,
        stop_reason: Some(stop_reason),
    });
}

fn map_messages_to_openai(messages: &[Message]) -> Result<Vec<OpenAiMessage>, LlmError> {
    let mut mapped = Vec::new();

    for message in messages {
        match message.role {
            MessageRole::System | MessageRole::User => {
                mapped.push(OpenAiMessage {
                    role: match message.role {
                        MessageRole::System => "system".to_string(),
                        MessageRole::User => "user".to_string(),
                        _ => unreachable!(),
                    },
                    content: map_openai_message_content(&message.content),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            MessageRole::Assistant => {
                let text = extract_text(&message.content);
                let tool_calls = extract_tool_calls(&message.content)?;

                mapped.push(OpenAiMessage {
                    role: "assistant".to_string(),
                    content: if text.is_empty() {
                        None
                    } else {
                        Some(OpenAiMessageContent::Text(text))
                    },
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    tool_call_id: None,
                });
            }
            MessageRole::Tool => {
                let tool_results = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                        } => Some((tool_use_id.clone(), content.clone())),
                        _ => None,
                    })
                    .collect::<Vec<_>>();

                if tool_results.is_empty() {
                    mapped.push(OpenAiMessage {
                        role: "tool".to_string(),
                        content: map_openai_message_content(&message.content),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                } else {
                    for (tool_call_id, content) in tool_results {
                        mapped.push(OpenAiMessage {
                            role: "tool".to_string(),
                            content: Some(OpenAiMessageContent::Text(value_to_openai_content(
                                content,
                            ))),
                            tool_calls: None,
                            tool_call_id: Some(tool_call_id),
                        });
                    }
                }
            }
        }
    }

    Ok(mapped)
}

fn extract_tool_calls(blocks: &[ContentBlock]) -> Result<Vec<OpenAiToolCall>, LlmError> {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse {
                id, name, input, ..
            } => Some((id, name, input)),
            _ => None,
        })
        .map(|(id, name, input)| {
            Ok(OpenAiToolCall {
                id: id.clone(),
                call_type: "function".to_string(),
                function: OpenAiFunctionCall {
                    name: name.clone(),
                    arguments: serde_json::to_string(input)
                        .map_err(|error| LlmError::Serialization(error.to_string()))?,
                },
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

fn extract_text(blocks: &[ContentBlock]) -> String {
    blocks
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
        .join("\n")
}

fn map_openai_message_content(blocks: &[ContentBlock]) -> Option<OpenAiMessageContent> {
    let content = blocks
        .iter()
        .filter_map(map_openai_input_block)
        .collect::<Vec<_>>();

    match content.as_slice() {
        [] => None,
        [OpenAiInputBlock::Text { text }] => Some(OpenAiMessageContent::Text(text.clone())),
        _ => Some(OpenAiMessageContent::Blocks(content)),
    }
}

fn map_openai_input_block(block: &ContentBlock) -> Option<OpenAiInputBlock> {
    match block {
        ContentBlock::Text { text } => Some(OpenAiInputBlock::Text { text: text.clone() }),
        ContentBlock::Image { media_type, data } => Some(OpenAiInputBlock::ImageUrl {
            image_url: OpenAiImageUrl {
                url: format!("data:{media_type};base64,{data}"),
            },
        }),
        ContentBlock::Document {
            media_type,
            data,
            filename,
        } => Some(OpenAiInputBlock::Text {
            text: document_text_fallback(media_type, data, filename.as_deref()),
        }),
        ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. } => None,
    }
}

fn parse_json_or_string(value: &str) -> Value {
    crate::parse_tool_arguments_object(value)
}

fn value_to_openai_content(value: Value) -> String {
    match value {
        Value::String(content) => content,
        Value::Object(object) => {
            legacy_tool_result_content(&object).unwrap_or_else(|| Value::Object(object).to_string())
        }
        other => other.to_string(),
    }
}

fn legacy_tool_result_content(object: &serde_json::Map<String, Value>) -> Option<String> {
    let output = object.get("output")?.as_str()?;
    let success = object.get("success")?.as_bool()?;
    Some(if success {
        output.to_string()
    } else {
        format!("[ERROR] {output}")
    })
}

#[derive(Debug, Serialize)]
struct OpenAiRequestBody {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAiTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<OpenAiMessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum OpenAiMessageContent {
    Text(String),
    Blocks(Vec<OpenAiInputBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAiInputBlock {
    Text { text: String },
    ImageUrl { image_url: OpenAiImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiImageUrl {
    url: String,
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiToolFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseBody {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamEnvelope {
    #[serde(default)]
    choices: Vec<OpenAiStreamChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    #[serde(default)]
    delta: OpenAiStreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct OpenAiStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCallDelta {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiToolFunctionDelta>,
}

#[derive(Debug, Clone, Default)]
struct OpenAiToolStreamState {
    id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

struct OpenAiSseState {
    bytes_stream:
        std::pin::Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    framer: SseFramer,
    pending_chunks: std::collections::VecDeque<Result<StreamChunk, LlmError>>,
    finished: bool,
    tool_calls_by_index: HashMap<usize, OpenAiToolStreamState>,
}

impl OpenAiSseState {
    fn new<S>(bytes_stream: S) -> Self
    where
        S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    {
        Self {
            bytes_stream: Box::pin(bytes_stream),
            framer: SseFramer::default(),
            pending_chunks: std::collections::VecDeque::new(),
            finished: false,
            tool_calls_by_index: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::{collect_stream_chunks, StreamEvent};
    use crate::test_helpers::{
        callback_events, read_events, simple_pdf_with_text, spawn_json_server,
    };
    use crate::types::ToolDefinition;
    use base64::Engine;
    use serde_json::json;

    #[tokio::test]
    async fn openai_list_models_parses_response() {
        let base_url = spawn_json_server(
            "200 OK",
            r#"{"data":[{"id":"gpt-4o"},{"id":"gpt-4.1-mini"},{"id":"text-embedding-3-small"},{"id":"o3-mini"}]}"#,
        )
        .await;
        let provider = OpenAiProvider::new(base_url, "test-key")
            .expect("provider")
            .with_supported_models(vec!["custom-openai-model".to_string()]);

        let models = provider.list_models().await.expect("list models");

        assert_eq!(
            models,
            vec![
                "gpt-4.1-mini".to_string(),
                "gpt-4o".to_string(),
                "o3-mini".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn openai_list_models_filters_non_chat() {
        let base_url = spawn_json_server(
            "200 OK",
            r#"{"data":[{"id":"text-embedding-ada-002"},{"id":"dall-e-3"},{"id":"gpt-4o"}]}"#,
        )
        .await;
        let provider = OpenAiProvider::new(base_url, "test-key").expect("provider");

        let models = provider.list_models().await.expect("list models");

        assert_eq!(models, vec!["gpt-4o".to_string()]);
    }

    #[tokio::test]
    async fn openai_list_models_falls_back_on_error() {
        let base_url = spawn_json_server("500 Internal Server Error", r#"{"error":"nope"}"#).await;
        let provider = OpenAiProvider::new(base_url, "test-key")
            .expect("provider")
            .with_supported_models(vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]);

        let models = provider.list_models().await.expect("list models");

        assert_eq!(
            models,
            vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]
        );
    }

    #[test]
    fn openai_catalog_metadata_matches_expected_contract() {
        let provider =
            OpenAiProvider::openai(OpenAiProvider::default_base_url(), "test-key").unwrap();

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
        assert!(!provider.catalog_filters().apply_recency_and_price_floor);
    }

    #[test]
    fn openai_catalog_auth_headers_use_bearer_auth() {
        let provider =
            OpenAiProvider::openai(OpenAiProvider::default_base_url(), "test-key").unwrap();

        let headers = provider
            .catalog_auth_headers("oauth-token-123", "oauth")
            .expect("headers");

        assert_eq!(
            headers.get(reqwest::header::AUTHORIZATION).unwrap(),
            "Bearer oauth-token-123"
        );
    }

    #[test]
    fn openrouter_catalog_metadata_uses_openrouter_contract() {
        let provider =
            OpenAiProvider::openrouter(OpenAiProvider::openrouter_base_url(), "test-key").unwrap();

        assert_eq!(provider.supported_thinking_levels(), &["off"]);
        assert_eq!(
            provider.thinking_levels("anthropic/claude-sonnet-4"),
            &["off"]
        );
        assert_eq!(
            provider.context_window("anthropic/claude-sonnet-4"),
            200_000
        );
        assert!(provider.is_chat_capable("x-ai/grok-3"));
        assert!(!provider.is_chat_capable("openai/text-embedding-3-large"));
        assert_eq!(provider.fallback_models(), OPENROUTER_FALLBACK_MODELS);
        assert!(provider.catalog_filters().apply_recency_and_price_floor);
    }

    #[test]
    fn compatible_provider_name_does_not_change_catalog_contract() {
        let provider = OpenAiProvider::new("http://localhost:8080", "test-key")
            .unwrap()
            .with_name("openrouter");

        assert_eq!(
            provider.supported_thinking_levels(),
            &["off", "low", "high"]
        );
        assert_eq!(
            provider.thinking_levels("gpt-5.4"),
            &["none", "low", "medium", "high", "xhigh"]
        );
        assert_eq!(provider.fallback_models(), OPENAI_FALLBACK_MODELS);
        assert!(!provider.catalog_filters().apply_recency_and_price_floor);
    }

    #[test]
    fn test_build_request_body_maps_messages_tools_and_system() {
        let provider = OpenAiProvider::openrouter("http://localhost:8080", "test-key")
            .unwrap()
            .with_supported_models(vec!["gpt-4o-mini".to_string()]);

        let request = CompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![Message::user("hello")],
            tools: vec![ToolDefinition {
                name: "lookup".to_string(),
                description: "Lookup docs".to_string(),
                parameters: json!({"type":"object","properties":{"q":{"type":"string"}}}),
            }],
            temperature: Some(0.1),
            max_tokens: Some(128),
            system_prompt: Some("Be concise".to_string()),
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        assert_eq!(serialized["model"], "gpt-4o-mini");
        assert_eq!(serialized["stream"], false);
        assert_eq!(serialized["messages"][0]["role"], "system");
        assert_eq!(serialized["messages"][0]["content"], "Be concise");
        assert_eq!(serialized["messages"][1]["role"], "user");
        assert_eq!(serialized["messages"][1]["content"], "hello");
        assert_eq!(serialized["tools"].as_array().unwrap().len(), 1);
        assert_eq!(serialized["tools"][0]["function"]["name"], "lookup");
    }

    #[test]
    fn image_content_block_serializes_for_openai() {
        let provider = OpenAiProvider::new("http://localhost:8080", "test-key")
            .unwrap()
            .with_supported_models(vec!["gpt-4o-mini".to_string()]);
        let request = CompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![Message::user_with_images(
                "describe this image",
                vec![crate::types::ImageAttachment {
                    media_type: "image/jpeg".to_string(),
                    data: "abc123".to_string(),
                }],
            )],
            tools: vec![],
            temperature: None,
            max_tokens: Some(128),
            system_prompt: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        assert_eq!(serialized["messages"][0]["content"][0]["type"], "image_url");
        assert_eq!(
            serialized["messages"][0]["content"][0]["image_url"]["url"],
            "data:image/jpeg;base64,abc123"
        );
        assert_eq!(serialized["messages"][0]["content"][1]["type"], "text");
        assert_eq!(
            serialized["messages"][0]["content"][1]["text"],
            "describe this image"
        );
    }

    #[test]
    fn document_content_block_falls_back_to_text_for_openai() {
        let document = ContentBlock::Document {
            media_type: "application/pdf".to_string(),
            data: base64::engine::general_purpose::STANDARD
                .encode(simple_pdf_with_text("Hello PDF")),
            filename: Some("brief.pdf".to_string()),
        };

        let mapped = map_openai_input_block(&document).expect("mapped block");

        match mapped {
            OpenAiInputBlock::Text { text } => {
                assert!(text.contains("[file: brief.pdf]"));
                assert!(text.contains("Hello PDF"));
            }
            other => panic!("expected text block, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_completion_response_maps_text_and_tool_calls() {
        let body = OpenAiResponseBody {
            choices: vec![OpenAiChoice {
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some(OpenAiMessageContent::Text("I can call a tool".to_string())),
                    tool_calls: Some(vec![OpenAiToolCall {
                        id: "call_1".to_string(),
                        call_type: "function".to_string(),
                        function: OpenAiFunctionCall {
                            name: "lookup".to_string(),
                            arguments: "{\"q\":\"fawx\"}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
            }),
        };

        let mapped = OpenAiProvider::parse_completion_response(body).unwrap();

        assert_eq!(mapped.content.len(), 2);
        assert_eq!(mapped.tool_calls.len(), 1);
        assert_eq!(mapped.tool_calls[0].name, "lookup");
        assert_eq!(mapped.tool_calls[0].arguments["q"], "fawx");
        assert_eq!(mapped.stop_reason.as_deref(), Some("tool_calls"));

        let usage = mapped.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
    }

    #[test]
    fn openai_chat_length_stop_reason_is_preserved() {
        let body = OpenAiResponseBody {
            choices: vec![OpenAiChoice {
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some(OpenAiMessageContent::Text("Partial response".to_string())),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("length".to_string()),
            }],
            usage: None,
        };

        let mapped = OpenAiProvider::parse_completion_response(body).unwrap();
        assert_eq!(mapped.stop_reason.as_deref(), Some("length"));
    }

    #[test]
    fn test_parse_sse_payload_maps_text_tool_and_stop_chunks() {
        let payload = r#"
            data: {"choices":[{"delta":{"content":"hel"},"finish_reason":null}]}

            data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"q\":\"ci"}}]},"finish_reason":null}]}

            data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":8}}

            data: [DONE]
        "#;

        let chunks = OpenAiProvider::parse_sse_payload(payload).unwrap();

        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].delta_content.as_deref(), Some("hel"));

        assert_eq!(chunks[1].tool_use_deltas.len(), 1);
        assert_eq!(chunks[1].tool_use_deltas[0].id.as_deref(), Some("call_1"));
        assert_eq!(chunks[1].tool_use_deltas[0].name.as_deref(), Some("lookup"));

        assert_eq!(chunks[2].usage.unwrap().input_tokens, 7);
        assert_eq!(chunks[2].usage.unwrap().output_tokens, 8);
        assert_eq!(chunks[3].stop_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_stream_collection_emits_text_tool_and_done_events() {
        let payload = r#"
            data: {"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}

            data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"q\":\"faw"}}]},"finish_reason":null}]}

            data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"x\"}"}}]},"finish_reason":null}]}

            data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":7,"completion_tokens":8}}

            data: [DONE]
        "#;
        let chunks = OpenAiProvider::parse_sse_payload(payload).unwrap();
        let (callback, events) = callback_events();

        let response = collect_stream_chunks(chunks, &callback);

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "lookup");
        assert_eq!(response.tool_calls[0].arguments["q"], "fawx");
        assert_eq!(response.stop_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            read_events(events),
            vec![
                StreamEvent::TextDelta {
                    text: "Hello".to_string()
                },
                StreamEvent::ToolCallStart {
                    id: "call_1".to_string(),
                    name: "lookup".to_string()
                },
                StreamEvent::ToolCallDelta {
                    id: "call_1".to_string(),
                    args_delta: "{\"q\":\"faw".to_string()
                },
                StreamEvent::ToolCallDelta {
                    id: "call_1".to_string(),
                    args_delta: "x\"}".to_string()
                },
                StreamEvent::ToolCallComplete {
                    id: "call_1".to_string(),
                    name: "lookup".to_string(),
                    arguments: "{\"q\":\"fawx\"}".to_string()
                },
                StreamEvent::Done {
                    response: "Hello".to_string()
                }
            ]
        );
    }

    #[test]
    fn openai_stream_collection_matches_final_completion_response() {
        let body = OpenAiResponseBody {
            choices: vec![OpenAiChoice {
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some(OpenAiMessageContent::Text("I can call a tool".to_string())),
                    tool_calls: Some(vec![OpenAiToolCall {
                        id: "call_1".to_string(),
                        call_type: "function".to_string(),
                        function: OpenAiFunctionCall {
                            name: "lookup".to_string(),
                            arguments: "{\"q\":\"fawx\"}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
            }),
        };
        let payload = r#"
            data: {"choices":[{"delta":{"content":"I can call a tool"},"finish_reason":null}]}

            data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"q\":\"fawx\"}"}}]},"finish_reason":null}]}

            data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":20}}

            data: [DONE]
        "#;
        let chunks = OpenAiProvider::parse_sse_payload(payload).unwrap();
        let (callback, _) = callback_events();

        let streamed = collect_stream_chunks(chunks, &callback);
        let expected = OpenAiProvider::parse_completion_response(body).unwrap();

        assert_eq!(streamed, expected);
    }

    #[test]
    fn openai_sse_preserves_tool_id_across_indexed_argument_deltas() {
        let payload = r#"
            data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"q\":\"fa"}}]},"finish_reason":null}]}

            data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"wx\"}"}}]},"finish_reason":null}]}

            data: [DONE]
        "#;

        let chunks = OpenAiProvider::parse_sse_payload(payload).unwrap();
        let deltas: Vec<_> = chunks
            .iter()
            .flat_map(|chunk| &chunk.tool_use_deltas)
            .collect();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0].id.as_deref(), Some("call_1"));
        assert_eq!(deltas[1].id.as_deref(), Some("call_1"));
        assert_eq!(deltas[1].name.as_deref(), Some("lookup"));
    }

    #[test]
    fn test_parse_sse_payload_malformed_data_cases() {
        let incomplete_json = "data: {\"choices\":[{";
        let result = OpenAiProvider::parse_sse_payload(incomplete_json);
        assert!(matches!(result, Err(LlmError::Streaming(_))));

        let missing_data_prefix = "event: message\nretry: 1000";
        let result = OpenAiProvider::parse_sse_payload(missing_data_prefix).unwrap();
        assert!(result.is_empty());

        let unexpected_format = "data: not-json";
        let result = OpenAiProvider::parse_sse_payload(unexpected_format);
        assert!(matches!(result, Err(LlmError::Streaming(_))));
    }

    #[test]
    fn test_map_http_error_maps_client_and_server_statuses() {
        let client_error =
            OpenAiProvider::map_http_error(StatusCode::BAD_REQUEST, "bad".to_string());
        assert!(
            matches!(client_error, LlmError::Request(message) if message.contains("client error 400"))
        );

        let server_error =
            OpenAiProvider::map_http_error(StatusCode::INTERNAL_SERVER_ERROR, "oops".to_string());
        assert!(
            matches!(server_error, LlmError::Provider(message) if message.contains("server error 500"))
        );
    }

    #[test]
    fn test_endpoint_avoids_duplicate_v1_segment() {
        let with_v1 = OpenAiProvider::new("https://api.openai.com/v1", "test-key").unwrap();
        assert_eq!(
            with_v1.endpoint(),
            "https://api.openai.com/v1/chat/completions"
        );

        let without_v1 = OpenAiProvider::new("https://api.openai.com", "test-key").unwrap();
        assert_eq!(
            without_v1.endpoint(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_map_messages_to_openai_preserves_string_tool_result_content() {
        let messages = vec![Message {
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: Value::String("tool output".to_string()),
            }],
        }];

        let mapped = map_messages_to_openai(&messages).unwrap();
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].role, "tool");
        assert_eq!(mapped[0].tool_call_id.as_deref(), Some("call_1"));
        assert!(matches!(
            mapped[0].content.as_ref(),
            Some(OpenAiMessageContent::Text(text)) if text == "tool output"
        ));
    }

    #[test]
    fn test_map_messages_to_openai_formats_legacy_structured_tool_result_content() {
        let messages = vec![Message {
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: json!({"output": "permission denied", "success": false}),
            }],
        }];

        let mapped = map_messages_to_openai(&messages).unwrap();
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].role, "tool");
        assert_eq!(mapped[0].tool_call_id.as_deref(), Some("call_1"));
        assert!(matches!(
            mapped[0].content.as_ref(),
            Some(OpenAiMessageContent::Text(text)) if text == "[ERROR] permission denied"
        ));
    }

    fn continuation_assistant_message() -> Message {
        Message {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: Some("fc_1".to_string()),
                    name: "lookup".to_string(),
                    input: json!({"q": "first"}),
                },
                ContentBlock::ToolUse {
                    id: "call_2".to_string(),
                    provider_id: Some("fc_2".to_string()),
                    name: "lookup".to_string(),
                    input: json!({"q": "second"}),
                },
            ],
        }
    }

    fn continuation_tool_message() -> Message {
        Message {
            role: MessageRole::Tool,
            content: vec![
                ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: Value::String("first result".to_string()),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "call_2".to_string(),
                    content: Value::String("second result".to_string()),
                },
            ],
        }
    }

    #[test]
    fn test_map_messages_handles_tool_continuation_pair() {
        let messages = vec![
            continuation_assistant_message(),
            continuation_tool_message(),
        ];
        let mapped = map_messages_to_openai(&messages).unwrap();
        let assistant_calls = mapped[0].tool_calls.as_ref().expect("assistant tool calls");
        let tool_messages = mapped[1..]
            .iter()
            .map(|message| {
                (
                    message.role.as_str(),
                    message.tool_call_id.as_deref(),
                    match message.content.as_ref() {
                        Some(OpenAiMessageContent::Text(text)) => Some(text.as_str()),
                        _ => None,
                    },
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(mapped.len(), 3);
        assert_eq!(mapped[0].role, "assistant");
        assert!(mapped[0].content.is_none());
        assert_eq!(
            assistant_calls
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            vec!["call_1", "call_2"]
        );
        assert_eq!(
            tool_messages,
            vec![
                ("tool", Some("call_1"), Some("first result")),
                ("tool", Some("call_2"), Some("second result"))
            ]
        );
    }

    #[test]
    fn test_build_request_rejects_unsupported_model() {
        let provider = OpenAiProvider::new("http://localhost:8080", "test-key")
            .unwrap()
            .with_supported_models(vec!["gpt-4o-mini".to_string()]);

        let request = CompletionRequest {
            model: "gpt-5".to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(128),
            system_prompt: None,
            thinking: None,
        };

        let result = provider.build_request_body(&request, false);
        assert!(matches!(result, Err(LlmError::UnsupportedModel(_))));
    }

    #[test]
    fn test_parse_json_or_string_normalizes_empty_to_object() {
        let result = parse_json_or_string("");
        assert_eq!(
            result,
            Value::Object(serde_json::Map::new()),
            "empty string should be normalized to {{}}"
        );
    }

    #[test]
    fn test_parse_json_or_string_normalizes_whitespace_to_object() {
        let result = parse_json_or_string("  \t\n  ");
        assert_eq!(
            result,
            Value::Object(serde_json::Map::new()),
            "whitespace-only string should be normalized to {{}}"
        );
    }

    #[test]
    fn test_parse_json_or_string_preserves_valid_json() {
        let result = parse_json_or_string(r#"{"key": "value"}"#);
        assert_eq!(
            result,
            serde_json::json!({"key": "value"}),
            "valid JSON should be parsed as-is"
        );
    }
}
