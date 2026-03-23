//! Anthropic Messages API provider.

use async_trait::async_trait;
use futures::{stream, Stream, StreamExt};
use reqwest::StatusCode;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::time::Duration;

use crate::provider::{CompletionStream, LlmProvider, ProviderCapabilities};
use crate::sse::{SseFrame, SseFramer};
use crate::streaming::{collect_completion_stream, StreamCallback};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message, MessageRole,
    StreamChunk, ThinkingConfig, ToolCall, ToolUseDelta, Usage,
};
use crate::validation::validate_tool_message_sequence;

/// Maximum allowed thinking budget tokens — prevents token exhaustion attacks.
/// Even if a caller constructs ThinkingConfig::Enabled with an arbitrary budget,
/// the wire request is capped at this ceiling.
const MAX_THINKING_BUDGET: u32 = 32_000;

/// Minimum token headroom reserved for the model's response output
/// when thinking is enabled. Ensures max_tokens > budget_tokens.
const MIN_RESPONSE_TOKENS: u32 = 1024;
const VALID_ANTHROPIC_EFFORTS: [&str; 4] = ["low", "medium", "high", "max"];
const CLAUDE_CODE_SYSTEM_IDENTITY: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

/// Anthropic auth mode — determines how credentials are sent.
#[derive(Clone)]
pub enum AnthropicAuthMode {
    /// Standard API key: sent as `x-api-key` header.
    ApiKey(String),
    /// OAuth/setup-token (`sk-ant-oat...`): sent as `Authorization: Bearer` with
    /// Claude Code identity headers. Matches OpenClaw's behavior.
    SetupToken(String),
}

impl fmt::Debug for AnthropicAuthMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = match self {
            Self::ApiKey(_) => "ApiKey",
            Self::SetupToken(_) => "SetupToken",
        };
        write!(f, "{variant}(<redacted>)")
    }
}

impl AnthropicAuthMode {
    /// Detect auth mode from a credential string.
    /// Tokens starting with `sk-ant-oat` use Bearer auth; everything else uses x-api-key.
    pub fn detect(credential: impl Into<String>) -> Self {
        let cred = credential.into();
        if cred.starts_with("sk-ant-oat") {
            Self::SetupToken(cred)
        } else {
            Self::ApiKey(cred)
        }
    }
}

/// Anthropic API provider implementation.
#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    base_url: String,
    auth_mode: AnthropicAuthMode,
    api_version: String,
    supported_models: Vec<String>,
    client: reqwest::Client,
}

fn is_claude_4_6(model: &str) -> bool {
    let normalized = model.split('/').next_back().unwrap_or(model);
    normalized.contains("opus-4-6") || normalized.contains("sonnet-4-6")
}

fn is_valid_anthropic_effort(effort: &str) -> bool {
    VALID_ANTHROPIC_EFFORTS.contains(&effort)
}

fn build_output_config(effort: &str) -> Option<AnthropicOutputConfig> {
    is_valid_anthropic_effort(effort).then(|| AnthropicOutputConfig {
        effort: effort.to_string(),
    })
}

fn build_anthropic_thinking(
    model: &str,
    config: &Option<ThinkingConfig>,
) -> (Option<AnthropicThinking>, Option<AnthropicOutputConfig>) {
    match config {
        Some(ThinkingConfig::Adaptive { effort }) => {
            if !is_claude_4_6(model) {
                tracing::warn!(
                    model,
                    "adaptive thinking requested for non-Claude 4.6 model"
                );
            }
            // output_config.effort must be a valid Anthropic value.
            // If the effort is "adaptive" or not a recognized value, omit
            // output_config entirely and let the API use its default.
            let output_config = build_output_config(effort);
            (
                Some(AnthropicThinking::Adaptive {
                    thinking_type: "adaptive".to_string(),
                }),
                output_config,
            )
        }
        Some(ThinkingConfig::Enabled { budget_tokens }) => {
            let capped = (*budget_tokens).min(MAX_THINKING_BUDGET);
            if *budget_tokens > MAX_THINKING_BUDGET {
                tracing::warn!(
                    requested = budget_tokens,
                    capped,
                    "thinking budget exceeds maximum, capping at {MAX_THINKING_BUDGET}"
                );
            }
            (
                Some(AnthropicThinking::Manual {
                    thinking_type: "enabled".to_string(),
                    budget_tokens: capped,
                }),
                None,
            )
        }
        Some(ThinkingConfig::Off) | None => (None, None),
        Some(ThinkingConfig::Reasoning { .. }) => (None, None),
    }
}

impl AnthropicProvider {
    /// Create a new Anthropic provider. Auto-detects auth mode from the credential.
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self, LlmError> {
        let base_url = base_url.into();
        let api_key = api_key.into();

        if base_url.trim().is_empty() {
            return Err(LlmError::Config("base_url cannot be empty".to_string()));
        }

        if api_key.trim().is_empty() {
            return Err(LlmError::Config("api_key cannot be empty".to_string()));
        }

        let auth_mode = AnthropicAuthMode::detect(&api_key);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1800))
            .build()
            .map_err(|error| LlmError::Config(format!("failed to build HTTP client: {error}")))?;

        Ok(Self {
            base_url,
            auth_mode,
            api_version: "2023-06-01".to_string(),
            supported_models: Vec::new(),
            client,
        })
    }

    /// Set Anthropic API version header.
    pub fn with_api_version(mut self, api_version: impl Into<String>) -> Self {
        self.api_version = api_version.into();
        self
    }

    /// Set explicit supported models list.
    pub fn with_supported_models(mut self, supported_models: Vec<String>) -> Self {
        self.supported_models = supported_models;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url.trim_end_matches('/'))
    }

    fn models_endpoint(&self) -> String {
        format!("{}/v1/models", self.base_url.trim_end_matches('/'))
    }

    async fn fetch_models(&self) -> Result<Vec<String>, LlmError> {
        let mut url = Url::parse(&self.models_endpoint())
            .map_err(|error| LlmError::Config(format!("invalid anthropic models url: {error}")))?;
        let mut model_ids = Vec::new();

        loop {
            let response = self.fetch_models_page(url.clone()).await?;
            model_ids.extend(filter_model_ids(response.data));

            if !response.has_more {
                return Ok(model_ids);
            }

            update_pagination_cursor(&mut url, response.last_id)?;
        }
    }

    async fn fetch_models_page(&self, url: Url) -> Result<AnthropicModelsResponse, LlmError> {
        let request_builder = self
            .client
            .get(url)
            .header("anthropic-version", &self.api_version);
        let response = self.apply_auth(request_builder).send().await?;
        parse_model_response(response).await
    }

    /// Apply auth headers to a request builder based on auth mode.
    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth_mode {
            AnthropicAuthMode::ApiKey(key) => builder.header("x-api-key", key),
            AnthropicAuthMode::SetupToken(token) => builder
                .header("Authorization", format!("Bearer {token}"))
                .header(
                    "anthropic-beta",
                    "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14",
                )
                .header("user-agent", "claude-cli/2.1.75")
                .header("x-app", "cli")
                .header("accept", "application/json")
                // Required by Anthropic's API for OAuth/setup-token auth.
                // Despite the name, this does not enable browser access — it's
                // an Anthropic SDK misnomer for non-browser clients using Bearer auth.
                .header("anthropic-dangerous-direct-browser-access", "true"),
        }
    }

    fn has_configured_auth(&self) -> bool {
        match &self.auth_mode {
            AnthropicAuthMode::ApiKey(key) | AnthropicAuthMode::SetupToken(key) => {
                !key.trim().is_empty()
            }
        }
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
    ) -> Result<AnthropicRequestBody, LlmError> {
        self.ensure_supported_model(&request.model)?;

        let mut system_prompt = request.system_prompt.clone();

        let mut messages = Vec::new();

        for message in &request.messages {
            if matches!(message.role, MessageRole::System) {
                let extracted = extract_text(&message.content);
                if !extracted.is_empty() {
                    let merged = match system_prompt.take() {
                        Some(existing) if !existing.is_empty() => {
                            format!("{existing}\n{extracted}")
                        }
                        _ => extracted,
                    };
                    system_prompt = Some(merged);
                }
                continue;
            }

            messages.push(self.map_message_to_anthropic(message)?);
        }

        let tools = request
            .tools
            .iter()
            .map(|tool| AnthropicTool {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: tool.parameters.clone(),
            })
            .collect::<Vec<_>>();

        let (thinking, output_config) = build_anthropic_thinking(&request.model, &request.thinking);

        let body = AnthropicRequestBody {
            model: request.model.clone(),
            messages,
            tools,
            // When thinking is enabled, Anthropic requires temperature=1 (or omitted)
            temperature: if thinking.is_some() {
                None
            } else {
                request.temperature
            },
            max_tokens: match &thinking {
                Some(AnthropicThinking::Manual { budget_tokens, .. }) => request
                    .max_tokens
                    .unwrap_or(4096)
                    .max(budget_tokens + MIN_RESPONSE_TOKENS),
                Some(AnthropicThinking::Adaptive { .. }) => request.max_tokens.unwrap_or(16_000),
                None => request.max_tokens.unwrap_or(4096),
            },
            system: self.build_system_value(system_prompt),
            stream,
            thinking,
            output_config,
        };

        #[cfg(debug_assertions)]
        validate_request(&body);

        Ok(body)
    }

    /// Build the `system` field value.
    ///
    /// For setup tokens, Anthropic requires the Claude Code identity as the
    /// first content block in an array format. For API keys, a plain string.
    fn build_system_value(&self, prompt: Option<String>) -> Option<serde_json::Value> {
        if matches!(self.auth_mode, AnthropicAuthMode::SetupToken(_)) {
            let mut blocks =
                vec![serde_json::json!({"type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY})];
            if let Some(text) = prompt {
                if !text.is_empty() {
                    blocks.push(serde_json::json!({"type": "text", "text": text}));
                }
            }
            Some(serde_json::Value::Array(blocks))
        } else {
            prompt.map(serde_json::Value::String)
        }
    }

    fn map_message_to_anthropic(&self, message: &Message) -> Result<AnthropicMessage, LlmError> {
        let role = match message.role {
            MessageRole::Assistant => "assistant",
            MessageRole::User | MessageRole::Tool => "user",
            MessageRole::System => {
                return Err(LlmError::Config(
                    "system messages are mapped to top-level system prompt".to_string(),
                ));
            }
        }
        .to_string();

        let content = message
            .content
            .iter()
            .map(map_content_to_anthropic)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(AnthropicMessage { role, content })
    }

    fn parse_completion_response(body: AnthropicResponseBody) -> CompletionResponse {
        let mut content = Vec::with_capacity(body.content.len());
        let mut tool_calls = Vec::new();

        for block in body.content {
            match block {
                AnthropicContentBlock::Text { text } => {
                    content.push(ContentBlock::Text { text });
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    content.push(ContentBlock::ToolUse {
                        id: id.clone(),
                        provider_id: None,
                        name: name.clone(),
                        input: input.clone(),
                    });
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: input,
                    });
                }
                AnthropicContentBlock::ToolResult {
                    tool_use_id,
                    content: result,
                } => {
                    content.push(ContentBlock::ToolResult {
                        tool_use_id,
                        content: result,
                    });
                }
                AnthropicContentBlock::Image { source } => {
                    content.push(ContentBlock::Image {
                        media_type: source.media_type,
                        data: source.data,
                    });
                }
                AnthropicContentBlock::Document { source, title } => {
                    content.push(ContentBlock::Document {
                        media_type: source.media_type,
                        data: source.data,
                        filename: title,
                    });
                }
                AnthropicContentBlock::Thinking { .. } => {
                    // Extended thinking block — skip (not surfaced to user)
                }
                AnthropicContentBlock::RedactedThinking { .. } => {
                    // Redacted thinking block — skip (content-policy redaction)
                }
            }
        }

        CompletionResponse {
            content,
            tool_calls,
            usage: body.usage.map(|usage| Usage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
            }),
            stop_reason: body.stop_reason,
        }
    }

    #[allow(dead_code)]
    fn parse_sse_payload(payload: &str) -> Result<Vec<StreamChunk>, LlmError> {
        let mut framer = SseFramer::default();
        let mut chunks = Vec::new();
        let mut tool_ids_by_index: HashMap<usize, String> = HashMap::new();

        for line in payload.lines() {
            let mut framed = framer.push_bytes(format!("{line}\n").as_bytes())?;
            let mut frame_chunks = Self::map_sse_frames(&mut framed, &mut tool_ids_by_index)?;
            chunks.append(&mut frame_chunks);
        }

        let mut final_frames = framer.finish()?;
        let mut final_chunks = Self::map_sse_frames(&mut final_frames, &mut tool_ids_by_index)?;
        chunks.append(&mut final_chunks);
        Ok(chunks)
    }

    /// Handle a `content_block_start` SSE event. If the block is a
    /// `tool_use`, record the `index → id` mapping and emit the
    /// start-of-tool-call chunk.
    fn handle_block_start(
        event: AnthropicStreamEvent,
        tool_ids_by_index: &mut HashMap<usize, String>,
    ) -> Vec<StreamChunk> {
        let Some(AnthropicContentBlock::ToolUse { id, name, .. }) = event.content_block else {
            return Vec::new();
        };
        if let Some(index) = event.index {
            tool_ids_by_index.insert(index, id.clone());
        }
        vec![StreamChunk {
            delta_content: None,
            tool_use_deltas: vec![ToolUseDelta {
                id: Some(id),
                provider_id: None,
                name: Some(name),
                arguments_delta: None,
                arguments_done: false,
            }],
            usage: None,
            stop_reason: None,
        }]
    }

    /// Handle a `content_block_delta` SSE event. For `input_json_delta`
    /// payloads, resolve the tool-call ID from the event's `index` field
    /// using the `tool_ids_by_index` map.
    fn handle_block_delta(
        event: AnthropicStreamEvent,
        tool_ids_by_index: &HashMap<usize, String>,
    ) -> Vec<StreamChunk> {
        let Some(delta) = event.delta else {
            return Vec::new();
        };
        let mut chunks = Vec::new();
        if let Some(text) = delta.text {
            chunks.push(StreamChunk {
                delta_content: Some(text),
                tool_use_deltas: Vec::new(),
                usage: None,
                stop_reason: None,
            });
        }
        if let Some(partial_json) = delta.partial_json {
            let tool_id = event
                .index
                .and_then(|idx| tool_ids_by_index.get(&idx).cloned());
            chunks.push(StreamChunk {
                delta_content: None,
                tool_use_deltas: vec![ToolUseDelta {
                    id: tool_id,
                    provider_id: None,
                    name: None,
                    arguments_delta: Some(partial_json),
                    arguments_done: false,
                }],
                usage: None,
                stop_reason: None,
            });
        }
        chunks
    }

    /// Handle a `content_block_stop` SSE event. Only emits a done-marker
    /// chunk when the stopped block was a tool-use block (tracked in
    /// `tool_ids_by_index`). Text-block stops produce no output.
    fn handle_block_stop(
        event: AnthropicStreamEvent,
        tool_ids_by_index: &mut HashMap<usize, String>,
    ) -> Vec<StreamChunk> {
        let Some(tool_id) = event.index.and_then(|idx| tool_ids_by_index.remove(&idx)) else {
            return Vec::new();
        };
        vec![StreamChunk {
            delta_content: None,
            tool_use_deltas: vec![ToolUseDelta {
                id: Some(tool_id),
                provider_id: None,
                name: None,
                arguments_delta: None,
                arguments_done: true,
            }],
            usage: None,
            stop_reason: None,
        }]
    }

    /// Parse a single SSE `data:` payload into zero or more stream chunks.
    ///
    /// `tool_ids_by_index` maps Anthropic content-block indices to
    /// tool-use IDs, enabling correct routing of interleaved deltas
    /// across concurrent tool-call blocks.
    fn parse_sse_data(
        data: &str,
        tool_ids_by_index: &mut HashMap<usize, String>,
    ) -> Result<Vec<StreamChunk>, LlmError> {
        let event: AnthropicStreamEvent = serde_json::from_str(data)
            .map_err(|error| LlmError::Streaming(format!("invalid SSE JSON: {error}")))?;

        match event.event_type.as_str() {
            "content_block_start" => Ok(Self::handle_block_start(event, tool_ids_by_index)),
            "content_block_delta" => Ok(Self::handle_block_delta(event, tool_ids_by_index)),
            "content_block_stop" => Ok(Self::handle_block_stop(event, tool_ids_by_index)),
            "message_start" => Ok(Self::handle_message_start(event)),
            "message_delta" => Ok(Self::handle_message_delta(event)),
            _ => Ok(Vec::new()),
        }
    }

    fn handle_message_start(event: AnthropicStreamEvent) -> Vec<StreamChunk> {
        let Some(usage) = event.message.and_then(|m| m.usage) else {
            return Vec::new();
        };
        vec![StreamChunk {
            delta_content: None,
            tool_use_deltas: Vec::new(),
            usage: Some(Usage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
            }),
            stop_reason: None,
        }]
    }

    fn handle_message_delta(event: AnthropicStreamEvent) -> Vec<StreamChunk> {
        let usage = event.usage.map(|u| Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        });
        let stop_reason = event.delta.and_then(|d| d.stop_reason);
        if usage.is_none() && stop_reason.is_none() {
            return Vec::new();
        }
        vec![StreamChunk {
            delta_content: None,
            tool_use_deltas: Vec::new(),
            usage,
            stop_reason,
        }]
    }

    fn stream_from_sse(
        response: reqwest::Response,
    ) -> impl Stream<Item = Result<StreamChunk, LlmError>> + Send {
        stream::unfold(
            AnthropicSseState::new(response.bytes_stream()),
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

                            match Self::map_sse_frames(&mut frames, &mut state.tool_ids_by_index) {
                                Ok(chunks) => {
                                    state.pending_chunks.extend(chunks.into_iter().map(Ok));
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

                            match Self::map_sse_frames(&mut frames, &mut state.tool_ids_by_index) {
                                Ok(chunks) => {
                                    state.pending_chunks.extend(chunks.into_iter().map(Ok));
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
        tool_ids_by_index: &mut HashMap<usize, String>,
    ) -> Result<Vec<StreamChunk>, LlmError> {
        let mut chunks = Vec::new();
        for frame in frames.drain(..) {
            match frame {
                SseFrame::Data(data) => {
                    chunks.append(&mut Self::parse_sse_data(&data, tool_ids_by_index)?);
                }
                SseFrame::Done => break,
            }
        }
        Ok(chunks)
    }

    fn map_http_error(status: StatusCode, body: String) -> LlmError {
        if status.as_u16() == 400 && Self::mentions_thinking_rejection(&body) {
            tracing::warn!(
                "provider rejected thinking config — check effort/budget_tokens: {body}"
            );
        }
        match status.as_u16() {
            401 | 403 => LlmError::Authentication(body),
            429 => LlmError::RateLimited(body),
            400..=499 => LlmError::Request(format!("client error {}: {body}", status.as_u16())),
            500..=599 => LlmError::Provider(format!("server error {}: {body}", status.as_u16())),
            _ => LlmError::Request(format!("http {}: {body}", status.as_u16())),
        }
    }

    fn mentions_thinking_rejection(body: &str) -> bool {
        let lowered = body.to_ascii_lowercase();
        ["thinking", "effort", "budget_tokens", "output_config"]
            .iter()
            .any(|term| lowered.contains(term))
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        validate_tool_message_sequence(&request.messages)?;
        let body = self.build_request_body(&request, false)?;

        let request_builder = self
            .client
            .post(self.endpoint())
            .header("anthropic-version", &self.api_version);
        let response = self.apply_auth(request_builder).json(&body).send().await?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("unable to read error body: {error}"));
            return Err(Self::map_http_error(status, body));
        }

        let parsed = response
            .json::<AnthropicResponseBody>()
            .await
            .map_err(|error| LlmError::InvalidResponse(error.to_string()))?;

        Ok(Self::parse_completion_response(parsed))
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, LlmError> {
        let body = self.build_request_body(&request, true)?;
        let endpoint = self.endpoint();

        tracing::debug!(
            endpoint = %endpoint,
            auth_mode = ?self.auth_mode,
            "anthropic streaming request"
        );
        tracing::trace!(
            endpoint = %endpoint,
            body = %serde_json::to_string(&body).unwrap_or_default(),
            "anthropic streaming request body"
        );

        let request_builder = self
            .client
            .post(endpoint)
            .header("anthropic-version", &self.api_version);
        let response = self.apply_auth(request_builder).json(&body).send().await?;

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
        "anthropic"
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported_models.clone()
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        if !self.has_configured_auth() {
            return Ok(self.supported_models());
        }

        match self.fetch_models().await {
            Ok(models) if !models.is_empty() => Ok(models),
            Ok(_) => {
                tracing::warn!("anthropic models response was empty; using static fallback");
                Ok(self.supported_models())
            }
            Err(error) => {
                tracing::warn!(error = %error, "failed to fetch anthropic models; using static fallback");
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
}

async fn parse_model_response(
    response: reqwest::Response,
) -> Result<AnthropicModelsResponse, LlmError> {
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|error| format!("unable to read error body: {error}"));
        return Err(AnthropicProvider::map_http_error(status, body));
    }

    response
        .json::<AnthropicModelsResponse>()
        .await
        .map_err(|error| LlmError::InvalidResponse(error.to_string()))
}

fn update_pagination_cursor(url: &mut Url, last_id: Option<String>) -> Result<(), LlmError> {
    let cursor = last_id.ok_or_else(|| {
        LlmError::InvalidResponse(
            "anthropic models response set has_more without last_id".to_string(),
        )
    })?;
    url.set_query(Some(&format!("after_id={cursor}")));
    Ok(())
}

fn filter_model_ids(models: Vec<AnthropicModel>) -> Vec<String> {
    models
        .into_iter()
        .filter(|model| model.model_type == "model")
        .map(|model| model.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn map_content_to_anthropic(block: &ContentBlock) -> Result<AnthropicContentBlock, LlmError> {
    match block {
        ContentBlock::Text { text } => Ok(AnthropicContentBlock::Text { text: text.clone() }),
        ContentBlock::ToolUse {
            id, name, input, ..
        } => Ok(AnthropicContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
        } => Ok(AnthropicContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
        }),
        ContentBlock::Image { media_type, data } => Ok(AnthropicContentBlock::Image {
            source: AnthropicImageSource {
                source_type: "base64".to_string(),
                media_type: media_type.clone(),
                data: data.clone(),
            },
        }),
        ContentBlock::Document {
            media_type,
            data,
            filename,
        } => Ok(AnthropicContentBlock::Document {
            source: AnthropicDocumentSource {
                source_type: "base64".to_string(),
                media_type: media_type.clone(),
                data: data.clone(),
            },
            title: filename.clone(),
        }),
    }
}

fn extract_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            ContentBlock::Image { .. } => None,
            ContentBlock::Document { .. } => None,
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Serialize)]
struct AnthropicRequestBody {
    model: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<AnthropicTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<serde_json::Value>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<AnthropicThinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<AnthropicOutputConfig>,
}

/// Anthropic `output_config` parameter for effort control.
#[derive(Debug, Serialize)]
struct AnthropicOutputConfig {
    effort: String,
}

/// Anthropic extended thinking parameter.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicThinking {
    Adaptive {
        #[serde(rename = "type")]
        thinking_type: String,
    },
    Manual {
        #[serde(rename = "type")]
        thinking_type: String,
        budget_tokens: u32,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: Value,
    },
    Image {
        source: AnthropicImageSource,
    },
    Document {
        source: AnthropicDocumentSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    Thinking {
        thinking: String,
    },
    RedactedThinking {
        data: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicDocumentSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicModelsResponse {
    #[serde(default)]
    data: Vec<AnthropicModel>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    last_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicModel {
    id: String,
    #[serde(rename = "type")]
    model_type: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponseBody {
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    /// Content block index — present on all `content_block_*` events.
    /// Used to route deltas to the correct tool-use block when multiple
    /// tool calls are streamed concurrently.
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    delta: Option<AnthropicStreamDelta>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    message: Option<AnthropicStreamMessage>,
    #[serde(default)]
    content_block: Option<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamDelta {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    /// Captures `thinking_delta` content; intentionally unused (thinking
    /// blocks are silently skipped, but the field must exist so serde
    /// does not reject the payload).
    #[serde(default, rename = "thinking")]
    _thinking: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamMessage {
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

struct AnthropicSseState {
    bytes_stream:
        std::pin::Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    framer: SseFramer,
    pending_chunks: std::collections::VecDeque<Result<StreamChunk, LlmError>>,
    finished: bool,
    /// Maps Anthropic content-block index → tool-use ID for correct
    /// routing of interleaved streaming deltas.
    tool_ids_by_index: HashMap<usize, String>,
}

impl AnthropicSseState {
    fn new<S>(bytes_stream: S) -> Self
    where
        S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    {
        Self {
            bytes_stream: Box::pin(bytes_stream),
            framer: SseFramer::default(),
            pending_chunks: std::collections::VecDeque::new(),
            finished: false,
            tool_ids_by_index: HashMap::new(),
        }
    }
}

/// Validates an Anthropic request body against known API constraints.
/// Debug-only: panics on violation with a clear message.
#[cfg(debug_assertions)]
fn validate_request(body: &AnthropicRequestBody) {
    assert!(body.max_tokens > 0, "max_tokens must be > 0");
    assert!(!body.model.is_empty(), "model must be non-empty");
    assert!(!body.messages.is_empty(), "messages must be non-empty");

    if let Some(thinking) = &body.thinking {
        assert!(
            body.temperature.is_none(),
            "temperature must be None when thinking is enabled \
             (Anthropic requires temperature=1 or omitted)"
        );
        match thinking {
            AnthropicThinking::Adaptive { .. } => {
                if let Some(output_config) = &body.output_config {
                    assert!(
                        is_valid_anthropic_effort(&output_config.effort),
                        "adaptive thinking effort must be a valid Anthropic value"
                    );
                }
            }
            AnthropicThinking::Manual { budget_tokens, .. } => {
                assert!(
                    *budget_tokens > 0,
                    "thinking.budget_tokens must be > 0 when thinking is enabled"
                );
                assert!(
                    *budget_tokens <= MAX_THINKING_BUDGET,
                    "thinking.budget_tokens ({}) must be <= MAX_THINKING_BUDGET ({MAX_THINKING_BUDGET})",
                    budget_tokens
                );
                assert!(
                    body.max_tokens > *budget_tokens,
                    "max_tokens must be greater than thinking.budget_tokens \
                     ({} <= {})",
                    body.max_tokens,
                    budget_tokens
                );
                assert!(
                    body.output_config.is_none(),
                    "manual thinking must not include effort"
                );
            }
        }
    } else {
        assert!(
            body.output_config.is_none(),
            "effort must be None when thinking is disabled"
        );
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn anthropic_list_models_parses_response() {
        let base_url = spawn_json_server(
            "200 OK",
            r#"{"data":[{"id":"claude-3-5-haiku-20241022","type":"model"},{"id":"claude-3-7-sonnet-latest","type":"alias"},{"id":"claude-sonnet-4-20250514","type":"model"}]}"#,
        )
        .await;
        let provider = AnthropicProvider::new(base_url, "test-key")
            .expect("provider")
            .with_supported_models(vec!["claude-static".to_string()]);

        let models = provider.list_models().await.expect("list models");

        assert_eq!(
            models,
            vec![
                "claude-3-5-haiku-20241022".to_string(),
                "claude-sonnet-4-20250514".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn anthropic_list_models_skips_fetch_when_auth_is_empty() {
        let mut provider = AnthropicProvider::new("http://127.0.0.1:1", "test-key")
            .expect("provider")
            .with_supported_models(vec!["claude-opus-4-1-20250805".to_string()]);
        provider.auth_mode = AnthropicAuthMode::ApiKey("   ".to_string());

        let models = provider.list_models().await.expect("list models");

        assert_eq!(models, vec!["claude-opus-4-1-20250805".to_string()]);
    }

    #[tokio::test]
    async fn anthropic_list_models_follows_pagination() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let first = r#"{"data":[{"id":"claude-3-5-haiku-20241022","type":"model"}],"has_more":true,"last_id":"page-1"}"#;
            let second = r#"{"data":[{"id":"claude-sonnet-4-20250514","type":"model"}],"has_more":false,"last_id":"page-2"}"#;
            for expected_after_id in [None, Some("page-1")] {
                let (mut socket, _) = listener.accept().await.expect("accept connection");
                let mut buffer = [0_u8; 2048];
                let read = socket.read(&mut buffer).await.expect("read request");
                let request = String::from_utf8_lossy(&buffer[..read]);
                match expected_after_id {
                    None => assert!(request.starts_with("GET /v1/models HTTP/1.1")),
                    Some(after_id) => assert!(request
                        .starts_with(&format!("GET /v1/models?after_id={after_id} HTTP/1.1"))),
                }
                let body = if expected_after_id.is_none() {
                    first
                } else {
                    second
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response");
            }
        });

        let provider =
            AnthropicProvider::new(format!("http://{address}"), "test-key").expect("provider");

        let models = provider.list_models().await.expect("list models");

        assert_eq!(
            models,
            vec![
                "claude-3-5-haiku-20241022".to_string(),
                "claude-sonnet-4-20250514".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn anthropic_list_models_falls_back_on_error() {
        let provider = AnthropicProvider::new("http://127.0.0.1:1", "test-key")
            .expect("provider")
            .with_supported_models(vec!["claude-opus-4-1-20250805".to_string()]);

        let models = provider.list_models().await.expect("list models");

        assert_eq!(models, vec!["claude-opus-4-1-20250805".to_string()]);
    }

    #[test]
    fn test_build_request_body_maps_system_tools_and_content() {
        let provider = AnthropicProvider::new("http://localhost:9999", "test-key")
            .unwrap()
            .with_supported_models(vec!["claude-3-7-sonnet".to_string()]);

        let request = CompletionRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: vec![ContentBlock::Text {
                        text: "Follow policy".to_string(),
                    }],
                },
                Message::user("hello"),
            ],
            tools: vec![ToolDefinition {
                name: "search".to_string(),
                description: "Search docs".to_string(),
                parameters: json!({"type":"object","properties":{"query":{"type":"string"}}}),
            }],
            temperature: Some(0.2),
            max_tokens: Some(256),
            system_prompt: Some("System prelude".to_string()),
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        assert_eq!(serialized["model"], "claude-3-7-sonnet");
        assert_eq!(serialized["stream"], false);
        assert_eq!(serialized["max_tokens"], 256);
        assert_eq!(serialized["messages"].as_array().unwrap().len(), 1);
        assert_eq!(serialized["system"], "System prelude\nFollow policy");
        assert_eq!(serialized["tools"].as_array().unwrap().len(), 1);
        assert_eq!(serialized["tools"][0]["input_schema"]["type"], "object");
    }

    #[tokio::test]
    async fn anthropic_rejects_orphaned_tool_result() {
        let provider = AnthropicProvider::new("http://127.0.0.1:1", "test-key")
            .expect("provider")
            .with_supported_models(vec!["claude-3-7-sonnet".to_string()]);
        let request = CompletionRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![
                Message::user("Find weather"),
                Message {
                    role: MessageRole::Tool,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: serde_json::json!("first result"),
                    }],
                },
            ],
            tools: vec![],
            temperature: None,
            max_tokens: Some(256),
            system_prompt: None,
            thinking: None,
        };

        let error = provider.complete(request).await.expect_err("should reject");

        assert!(
            error
                .to_string()
                .contains("invalid tool continuation messages"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_parse_completion_response_maps_text_and_tool_calls() {
        let response = AnthropicResponseBody {
            content: vec![
                AnthropicContentBlock::Text {
                    text: "Thinking...".to_string(),
                },
                AnthropicContentBlock::ToolUse {
                    id: "toolu_01".to_string(),
                    name: "search".to_string(),
                    input: json!({"query":"fawx"}),
                },
            ],
            stop_reason: Some("tool_use".to_string()),
            usage: Some(AnthropicUsage {
                input_tokens: 12,
                output_tokens: 34,
            }),
        };

        let mapped = AnthropicProvider::parse_completion_response(response);

        assert_eq!(mapped.content.len(), 2);
        assert_eq!(mapped.tool_calls.len(), 1);
        assert_eq!(mapped.tool_calls[0].name, "search");
        assert_eq!(mapped.tool_calls[0].arguments["query"], "fawx");
        assert_eq!(mapped.stop_reason.as_deref(), Some("tool_use"));

        let usage = mapped.usage.unwrap();
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 34);
    }

    #[test]
    fn test_parse_sse_payload_maps_text_tool_and_usage_chunks() {
        let payload = r#"
            data: {"type":"content_block_delta","index":0,"delta":{"text":"hel"}}

            data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_01","name":"search","input":{}}}

            data: {"type":"content_block_delta","index":1,"delta":{"partial_json":"{\"query\":\"cit"}}

            data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}

            data: [DONE]
        "#;

        let chunks = AnthropicProvider::parse_sse_payload(payload).unwrap();

        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].delta_content.as_deref(), Some("hel"));

        assert_eq!(chunks[1].tool_use_deltas.len(), 1);
        assert_eq!(chunks[1].tool_use_deltas[0].id.as_deref(), Some("toolu_01"));

        assert_eq!(chunks[2].tool_use_deltas.len(), 1);
        assert!(chunks[2].tool_use_deltas[0]
            .arguments_delta
            .as_deref()
            .unwrap()
            .contains("query"));
        assert_eq!(
            chunks[2].tool_use_deltas[0].id.as_deref(),
            Some("toolu_01"),
            "delta must carry the tool ID resolved from index"
        );

        assert_eq!(chunks[3].usage.unwrap().output_tokens, 9);
        assert_eq!(chunks[3].stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn anthropic_stream_collection_emits_text_and_tool_events() {
        let payload = r#"
            data: {"type":"content_block_delta","index":0,"delta":{"text":"Hi"}}

            data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_01","name":"search","input":{}}}

            data: {"type":"content_block_delta","index":1,"delta":{"partial_json":"{\"query\":\"fawx\"}"}}

            data: {"type":"content_block_stop","index":1}

            data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":4}}

            data: [DONE]
        "#;
        let chunks = AnthropicProvider::parse_sse_payload(payload).unwrap();
        let (callback, events) = callback_events();

        let response = collect_stream_chunks(chunks, &callback);

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "search");
        assert_eq!(response.tool_calls[0].arguments["query"], "fawx");
        assert_eq!(response.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(
            read_events(events),
            vec![
                StreamEvent::TextDelta {
                    text: "Hi".to_string()
                },
                StreamEvent::ToolCallStart {
                    id: "toolu_01".to_string(),
                    name: "search".to_string()
                },
                StreamEvent::ToolCallDelta {
                    id: "toolu_01".to_string(),
                    args_delta: "{\"query\":\"fawx\"}".to_string()
                },
                StreamEvent::ToolCallComplete {
                    id: "toolu_01".to_string(),
                    name: "search".to_string(),
                    arguments: "{\"query\":\"fawx\"}".to_string()
                },
                StreamEvent::Done {
                    response: "Hi".to_string()
                }
            ]
        );
    }

    #[test]
    fn anthropic_stream_collection_matches_final_completion_response() {
        let response = AnthropicResponseBody {
            content: vec![
                AnthropicContentBlock::Text {
                    text: "I'll search".to_string(),
                },
                AnthropicContentBlock::ToolUse {
                    id: "toolu_01".to_string(),
                    name: "search".to_string(),
                    input: json!({"query":"fawx"}),
                },
            ],
            stop_reason: Some("tool_use".to_string()),
            usage: Some(AnthropicUsage {
                input_tokens: 10,
                output_tokens: 11,
            }),
        };
        let payload = r#"
            data: {"type":"content_block_delta","index":0,"delta":{"text":"I'll search"}}

            data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_01","name":"search","input":{}}}

            data: {"type":"content_block_delta","index":1,"delta":{"partial_json":"{\"query\":\"fawx\"}"}}

            data: {"type":"content_block_stop","index":1}

            data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":10,"output_tokens":11}}

            data: [DONE]
        "#;
        let chunks = AnthropicProvider::parse_sse_payload(payload).unwrap();
        let (callback, _) = callback_events();

        let streamed = collect_stream_chunks(chunks, &callback);
        let expected = AnthropicProvider::parse_completion_response(response);

        assert_eq!(streamed, expected);
    }

    #[test]
    fn test_parse_completion_response_preserves_structured_tool_result_content() {
        let response = AnthropicResponseBody {
            content: vec![AnthropicContentBlock::ToolResult {
                tool_use_id: "toolu_01".to_string(),
                content: json!({"status": "ok", "rows": [1, 2, 3]}),
            }],
            stop_reason: None,
            usage: None,
        };

        let mapped = AnthropicProvider::parse_completion_response(response);
        assert_eq!(mapped.content.len(), 1);
        match &mapped.content[0] {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "toolu_01");
                assert_eq!(content["status"], "ok");
                assert_eq!(content["rows"], json!([1, 2, 3]));
            }
            other => panic!("expected tool result block, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_sse_payload_malformed_data_cases() {
        let incomplete_json = "data: {\"type\":\"content_block_delta\",\"delta\":{";
        let result = AnthropicProvider::parse_sse_payload(incomplete_json);
        assert!(matches!(result, Err(LlmError::Streaming(_))));

        let missing_data_prefix = "event: content_block_delta\nretry: 1000";
        let result = AnthropicProvider::parse_sse_payload(missing_data_prefix).unwrap();
        assert!(result.is_empty());

        let unexpected_format = "data: not-json";
        let result = AnthropicProvider::parse_sse_payload(unexpected_format);
        assert!(matches!(result, Err(LlmError::Streaming(_))));
    }

    #[test]
    fn test_map_http_error_maps_client_and_server_statuses() {
        let client_error =
            AnthropicProvider::map_http_error(StatusCode::BAD_REQUEST, "bad".to_string());
        assert!(
            matches!(client_error, LlmError::Request(message) if message.contains("client error 400"))
        );

        let server_error = AnthropicProvider::map_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "oops".to_string(),
        );
        assert!(
            matches!(server_error, LlmError::Provider(message) if message.contains("server error 500"))
        );
    }

    #[test]
    fn image_content_block_serializes_for_anthropic() {
        let source = AnthropicImageSource {
            source_type: "base64".to_string(),
            media_type: "image/png".to_string(),
            data: "abc123".to_string(),
        };
        let block = AnthropicContentBlock::Image {
            source: source.clone(),
        };

        let serialized = serde_json::to_value(&block).unwrap();

        assert_eq!(serialized["type"], "image");
        assert_eq!(serialized["source"]["type"], "base64");
        assert_eq!(serialized["source"]["media_type"], "image/png");
        assert_eq!(serialized["source"]["data"], "abc123");
    }

    #[test]
    fn map_content_image_round_trips() {
        let block = ContentBlock::Image {
            media_type: "image/jpeg".to_string(),
            data: "xyz789".to_string(),
        };

        let mapped = map_content_to_anthropic(&block).unwrap();

        match mapped {
            AnthropicContentBlock::Image { source } => {
                assert_eq!(source.source_type, "base64");
                assert_eq!(source.media_type, "image/jpeg");
                assert_eq!(source.data, "xyz789");
            }
            other => panic!("expected image, got {other:?}"),
        }
    }

    #[test]
    fn document_content_block_serializes_for_anthropic() {
        let block = ContentBlock::Document {
            media_type: "application/pdf".to_string(),
            data: base64::engine::general_purpose::STANDARD
                .encode(simple_pdf_with_text("Hello PDF")),
            filename: Some("brief.pdf".to_string()),
        };

        let mapped = map_content_to_anthropic(&block).expect("mapped block");
        let serialized = serde_json::to_value(&mapped).expect("serialized block");

        assert_eq!(serialized["type"], "document");
        assert_eq!(serialized["source"]["type"], "base64");
        assert_eq!(serialized["source"]["media_type"], "application/pdf");
        assert_eq!(serialized["title"], "brief.pdf");
        assert!(serialized["source"]["data"]
            .as_str()
            .expect("base64 data")
            .starts_with("JVBERi0"));
    }

    #[test]
    fn test_map_content_to_anthropic_preserves_structured_tool_result_content() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_01".to_string(),
            content: json!({"result": true}),
        };

        let mapped = map_content_to_anthropic(&block).unwrap();
        match mapped {
            AnthropicContentBlock::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "toolu_01");
                assert_eq!(content, json!({"result": true}));
            }
            other => panic!("expected tool result, got {other:?}"),
        }
    }

    #[test]
    fn test_build_request_rejects_unsupported_model() {
        let provider = AnthropicProvider::new("http://localhost:9999", "test-key")
            .unwrap()
            .with_supported_models(vec!["claude-3-5-sonnet".to_string()]);

        let request = CompletionRequest {
            model: "claude-3-haiku".to_string(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(64),
            system_prompt: None,
            thinking: None,
        };

        let result = provider.build_request_body(&request, false);
        assert!(matches!(result, Err(LlmError::UnsupportedModel(_))));
    }

    #[test]
    fn content_block_stop_marks_tool_arguments_done() {
        let payload = r#"
            data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_abc","name":"list_directory","input":{}}}

            data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"/tmp\"}"}}

            data: {"type":"content_block_stop","index":1}

            data: [DONE]
        "#;

        let chunks = AnthropicProvider::parse_sse_payload(payload).unwrap();

        // content_block_start (tool id+name), delta (args), stop (done marker)
        assert!(
            chunks.len() >= 3,
            "expected at least 3 chunks, got {}",
            chunks.len()
        );

        let done_chunk = chunks
            .iter()
            .find(|c| c.tool_use_deltas.first().is_some_and(|d| d.arguments_done));
        assert!(
            done_chunk.is_some(),
            "content_block_stop must emit a ToolUseDelta with arguments_done=true"
        );

        // The done marker should carry no argument data — it's purely a
        // completion signal, not a delta with an empty string.
        let done_delta = &done_chunk.unwrap().tool_use_deltas[0];
        assert!(
            done_delta.arguments_delta.is_none(),
            "content_block_stop done marker must have arguments_delta=None, not Some(\"\")"
        );
    }

    /// Regression test for #1106: interleaved deltas across concurrent
    /// tool-use blocks must route to the correct tool ID via the `index`
    /// field, not a single `current_tool_use_id`.
    #[test]
    fn interleaved_tool_deltas_route_to_correct_tool_id() {
        // Simulates: tool A starts, tool A delta, tool B starts (would
        // overwrite single ID), tool A delta again → must still carry
        // tool A's ID, not tool B's.
        let payload = r#"
            data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_A","name":"search","input":{}}}

            data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"q\":"}}

            data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_B","name":"write","input":{}}}

            data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"hello\"}"}}

            data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"data\":\"world\"}"}}

            data: {"type":"content_block_stop","index":0}

            data: {"type":"content_block_stop","index":1}

            data: [DONE]
        "#;

        let chunks = AnthropicProvider::parse_sse_payload(payload).unwrap();

        let arg_deltas: Vec<_> = chunks
            .iter()
            .flat_map(|c| &c.tool_use_deltas)
            .filter(|d| d.arguments_delta.is_some())
            .collect();

        assert_eq!(arg_deltas.len(), 3, "expected 3 argument deltas");

        // First two deltas belong to tool A (index 0)
        assert_eq!(arg_deltas[0].id.as_deref(), Some("toolu_A"));
        assert_eq!(arg_deltas[1].id.as_deref(), Some("toolu_A"));
        // Third delta belongs to tool B (index 1)
        assert_eq!(arg_deltas[2].id.as_deref(), Some("toolu_B"));

        // Concatenated args for tool A must form valid JSON (no `}{`)
        let tool_a_args: String = arg_deltas
            .iter()
            .filter(|d| d.id.as_deref() == Some("toolu_A"))
            .filter_map(|d| d.arguments_delta.as_deref())
            .collect();
        assert!(
            !tool_a_args.contains("}{"),
            "tool A arguments must not contain '}}{{': {tool_a_args}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&tool_a_args)
            .expect("tool A concatenated arguments must be valid JSON");
        assert_eq!(parsed["q"], "hello");
    }

    /// Regression test for #1106: 4+ concurrent tool blocks must all
    /// have their arguments correctly routed via per-index tracking.
    #[test]
    fn four_concurrent_tool_blocks_all_arguments_clean() {
        let payload = r#"
            data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t0","name":"a","input":{}}}

            data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"b","input":{}}}

            data: {"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"t2","name":"c","input":{}}}

            data: {"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"t3","name":"d","input":{}}}

            data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"x\":2}"}}

            data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"x\":0}"}}

            data: {"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"x\":3}"}}

            data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"x\":1}"}}

            data: {"type":"content_block_stop","index":0}

            data: {"type":"content_block_stop","index":1}

            data: {"type":"content_block_stop","index":2}

            data: {"type":"content_block_stop","index":3}

            data: [DONE]
        "#;

        let chunks = AnthropicProvider::parse_sse_payload(payload).unwrap();

        // Verify each tool delta carries the correct ID
        let arg_deltas: Vec<_> = chunks
            .iter()
            .flat_map(|c| &c.tool_use_deltas)
            .filter(|d| d.arguments_delta.is_some())
            .collect();

        assert_eq!(arg_deltas.len(), 4, "expected 4 argument deltas");

        // Deltas arrive in order: index 2, 0, 3, 1
        assert_eq!(arg_deltas[0].id.as_deref(), Some("t2"));
        assert_eq!(arg_deltas[1].id.as_deref(), Some("t0"));
        assert_eq!(arg_deltas[2].id.as_deref(), Some("t3"));
        assert_eq!(arg_deltas[3].id.as_deref(), Some("t1"));

        // Each argument parses as valid JSON
        for delta in &arg_deltas {
            let json_str = delta.arguments_delta.as_deref().unwrap();
            assert!(
                !json_str.contains("}{"),
                "arguments must not contain '}}{{': {json_str}"
            );
            serde_json::from_str::<serde_json::Value>(json_str)
                .expect("each tool's arguments must be valid JSON");
        }

        // Verify done markers carry correct tool IDs
        let done_deltas: Vec<_> = chunks
            .iter()
            .flat_map(|c| &c.tool_use_deltas)
            .filter(|d| d.arguments_done)
            .collect();
        assert_eq!(done_deltas.len(), 4, "expected 4 done markers");
        let done_ids: Vec<_> = done_deltas.iter().filter_map(|d| d.id.as_deref()).collect();
        assert!(done_ids.contains(&"t0"));
        assert!(done_ids.contains(&"t1"));
        assert!(done_ids.contains(&"t2"));
        assert!(done_ids.contains(&"t3"));
    }

    #[test]
    fn anthropic_auth_mode_debug_redacts_credentials() {
        let api_key_debug = format!(
            "{:?}",
            AnthropicAuthMode::ApiKey("sk-ant-api03-secret".to_string())
        );
        assert_eq!(api_key_debug, "ApiKey(<redacted>)");
        assert!(
            !api_key_debug.contains("sk-ant-api03-secret"),
            "API key must be redacted in Debug output"
        );

        let setup_token_debug = format!(
            "{:?}",
            AnthropicAuthMode::SetupToken("sk-ant-oat01-secret".to_string())
        );
        assert_eq!(setup_token_debug, "SetupToken(<redacted>)");
        assert!(
            !setup_token_debug.contains("sk-ant-oat01-secret"),
            "setup token must be redacted in Debug output"
        );
    }

    #[test]
    fn build_system_value_uses_claude_code_identity_blocks_for_setup_tokens() {
        let provider = AnthropicProvider::new("http://localhost:9999", "sk-ant-oat01-test-token")
            .expect("provider");

        let system = provider
            .build_system_value(Some("Follow the user's instructions.".to_string()))
            .expect("system");

        assert_eq!(
            system,
            json!([
                {"type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY},
                {"type": "text", "text": "Follow the user's instructions."}
            ])
        );
    }

    #[test]
    fn build_system_value_uses_plain_string_for_api_keys() {
        let provider =
            AnthropicProvider::new("http://localhost:9999", "test-key").expect("provider");

        let system = provider
            .build_system_value(Some("Follow the user's instructions.".to_string()))
            .expect("system");

        assert_eq!(system, json!("Follow the user's instructions."));
    }

    #[test]
    fn build_system_value_setup_token_none_prompt_returns_identity_only() {
        let provider = AnthropicProvider::new("http://localhost:9999", "sk-ant-oat01-test-token")
            .expect("provider");

        let system = provider.build_system_value(None).expect("system");
        assert_eq!(
            system,
            json!([{"type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY}])
        );
    }

    #[test]
    fn build_system_value_setup_token_empty_prompt_returns_identity_only() {
        let provider = AnthropicProvider::new("http://localhost:9999", "sk-ant-oat01-test-token")
            .expect("provider");

        let system = provider
            .build_system_value(Some(String::new()))
            .expect("system");
        assert_eq!(
            system,
            json!([{"type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY}])
        );
    }

    #[test]
    fn build_system_value_api_key_none_returns_none() {
        let provider =
            AnthropicProvider::new("http://localhost:9999", "test-key").expect("provider");

        assert!(provider.build_system_value(None).is_none());
    }

    #[test]
    fn build_anthropic_thinking_omits_output_config_for_invalid_effort() {
        let (thinking, output_config) = build_anthropic_thinking(
            "claude-opus-4-6-20250301",
            &Some(ThinkingConfig::Adaptive {
                effort: "adaptive".to_string(),
            }),
        );

        assert!(matches!(thinking, Some(AnthropicThinking::Adaptive { .. })));
        assert!(
            output_config.is_none(),
            "invalid effort must omit output_config"
        );
    }

    #[test]
    fn build_anthropic_thinking_keeps_output_config_for_valid_efforts() {
        for effort in VALID_ANTHROPIC_EFFORTS {
            let (thinking, output_config) = build_anthropic_thinking(
                "claude-opus-4-6-20250301",
                &Some(ThinkingConfig::Adaptive {
                    effort: effort.to_string(),
                }),
            );

            assert!(matches!(thinking, Some(AnthropicThinking::Adaptive { .. })));
            assert_eq!(
                output_config
                    .expect("valid effort must produce output_config")
                    .effort,
                effort
            );
        }
    }

    #[test]
    fn build_request_body_omits_output_config_for_invalid_adaptive_effort() {
        let provider = AnthropicProvider::new("http://localhost:9999", "test-key")
            .expect("provider")
            .with_supported_models(vec!["claude-opus-4-6-20250301".to_string()]);
        let request = CompletionRequest {
            model: "claude-opus-4-6-20250301".to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(1024),
            system_prompt: None,
            thinking: Some(ThinkingConfig::Adaptive {
                effort: "adaptive".to_string(),
            }),
        };

        let body = provider.build_request_body(&request, false).expect("body");

        assert!(body.output_config.is_none());
    }

    /// Regression test: when thinking is enabled, Anthropic requires
    /// temperature=1 or omitted. Verify we strip the caller's temperature.
    #[test]
    fn thinking_enabled_strips_temperature() {
        let provider = AnthropicProvider::new("http://localhost:9999", "test-key")
            .unwrap()
            .with_supported_models(vec!["claude-sonnet-4-20250514".to_string()]);

        let request = CompletionRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: Some(0.7),
            max_tokens: Some(1024),
            system_prompt: None,
            thinking: Some(ThinkingConfig::Enabled {
                budget_tokens: 5000,
            }),
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        // temperature must be absent (None serializes to null, but
        // skip_serializing_if = "Option::is_none" omits it entirely)
        assert!(
            serialized.get("temperature").is_none(),
            "temperature must be omitted when thinking is enabled, got: {serialized}"
        );
        // thinking must still be present
        assert_eq!(serialized["thinking"]["type"], "enabled");
        assert_eq!(serialized["thinking"]["budget_tokens"], 5000);
    }

    /// Verify temperature is preserved when thinking is NOT enabled.
    #[test]
    fn thinking_disabled_preserves_temperature() {
        let provider = AnthropicProvider::new("http://localhost:9999", "test-key")
            .unwrap()
            .with_supported_models(vec!["claude-sonnet-4-20250514".to_string()]);

        let request = CompletionRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: Some(0.7),
            max_tokens: Some(1024),
            system_prompt: None,
            thinking: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        let temp = serialized["temperature"]
            .as_f64()
            .expect("temperature must be present when thinking is disabled");
        assert!(
            (temp - 0.7).abs() < 0.001,
            "temperature must be ~0.7, got {temp}"
        );
    }

    #[test]
    fn multi_tool_streaming_preserves_separate_arguments() {
        let payload = r#"
            data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_01","name":"read_file","input":{}}}

            data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"/tmp/a.txt\"}"}}

            data: {"type":"content_block_stop","index":0}

            data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_02","name":"read_file","input":{}}}

            data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"/tmp/b.txt\"}"}}

            data: {"type":"content_block_stop","index":1}

            data: [DONE]
        "#;

        let chunks = AnthropicProvider::parse_sse_payload(payload).unwrap();

        let tool_deltas: Vec<_> = chunks
            .iter()
            .flat_map(|c| &c.tool_use_deltas)
            .filter(|d| d.arguments_delta.as_ref().is_some_and(|s| !s.is_empty()))
            .collect();

        assert_eq!(tool_deltas.len(), 2, "expected 2 argument deltas");
        assert_eq!(
            tool_deltas[0].id.as_deref(),
            Some("toolu_01"),
            "first argument delta must carry toolu_01 id"
        );
        assert_eq!(
            tool_deltas[1].id.as_deref(),
            Some("toolu_02"),
            "second argument delta must carry toolu_02 id"
        );
    }

    /// Regression test: when thinking budget exceeds default max_tokens,
    /// max_tokens must be raised to at least budget_tokens + 1024.
    #[test]
    fn thinking_budget_increases_max_tokens() {
        let provider = AnthropicProvider::new("http://localhost:9999", "test-key")
            .unwrap()
            .with_supported_models(vec!["claude-sonnet-4-20250514".to_string()]);

        let request = CompletionRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None, // defaults to 4096, which is < 10000
            system_prompt: None,
            thinking: Some(ThinkingConfig::Enabled {
                budget_tokens: 10000,
            }),
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        let max_tokens = serialized["max_tokens"]
            .as_u64()
            .expect("max_tokens must be present");
        assert!(
            max_tokens >= 10000 + MIN_RESPONSE_TOKENS as u64,
            "max_tokens must be at least budget_tokens + MIN_RESPONSE_TOKENS, got {max_tokens}"
        );
    }

    /// Regression test: thinking budget_tokens must be capped at MAX_THINKING_BUDGET
    /// to prevent token exhaustion attacks via absurdly high values.
    #[test]
    fn thinking_budget_capped_at_maximum() {
        let provider = AnthropicProvider::new("http://localhost:9999", "test-key")
            .unwrap()
            .with_supported_models(vec!["claude-sonnet-4-20250514".to_string()]);

        let request = CompletionRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(8192),
            system_prompt: None,
            thinking: Some(ThinkingConfig::Enabled {
                budget_tokens: 500_000,
            }),
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        // budget_tokens must be capped at MAX_THINKING_BUDGET
        assert_eq!(
            serialized["thinking"]["budget_tokens"], MAX_THINKING_BUDGET,
            "budget_tokens must be capped at {MAX_THINKING_BUDGET}, got {}",
            serialized["thinking"]["budget_tokens"]
        );

        // max_tokens must be based on the capped budget, not the original 500_000
        let max_tokens = serialized["max_tokens"]
            .as_u64()
            .expect("max_tokens must be present");
        assert_eq!(
            max_tokens,
            (MAX_THINKING_BUDGET + MIN_RESPONSE_TOKENS) as u64,
            "max_tokens must be based on capped budget ({MAX_THINKING_BUDGET} + {MIN_RESPONSE_TOKENS}), got {max_tokens}",
        );
    }

    /// Verify max_tokens is NOT increased when it already exceeds the thinking budget.
    #[test]
    fn thinking_budget_preserves_sufficient_max_tokens() {
        let provider = AnthropicProvider::new("http://localhost:9999", "test-key")
            .unwrap()
            .with_supported_models(vec!["claude-sonnet-4-20250514".to_string()]);

        let request = CompletionRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(20000),
            system_prompt: None,
            thinking: Some(ThinkingConfig::Enabled {
                budget_tokens: 5000,
            }),
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        let max_tokens = serialized["max_tokens"]
            .as_u64()
            .expect("max_tokens must be present");
        assert_eq!(
            max_tokens, 20000,
            "max_tokens must stay at explicitly set value when it already exceeds budget"
        );
    }

    #[test]
    fn thinking_block_skipped_in_response() {
        let response = AnthropicResponseBody {
            content: vec![
                AnthropicContentBlock::Thinking {
                    thinking: "Let me reason about this...".to_string(),
                },
                AnthropicContentBlock::Text {
                    text: "The answer is 42.".to_string(),
                },
            ],
            stop_reason: Some("end_turn".to_string()),
            usage: Some(AnthropicUsage {
                input_tokens: 10,
                output_tokens: 20,
            }),
        };

        let mapped = AnthropicProvider::parse_completion_response(response);

        assert_eq!(mapped.content.len(), 1, "thinking block must be skipped");
        assert!(
            matches!(&mapped.content[0], ContentBlock::Text { text } if text == "The answer is 42."),
            "only the text block should remain"
        );
        assert!(mapped.tool_calls.is_empty());
    }

    #[test]
    fn redacted_thinking_block_skipped_in_response() {
        let response = AnthropicResponseBody {
            content: vec![
                AnthropicContentBlock::RedactedThinking {
                    data: "base64-redacted-content".to_string(),
                },
                AnthropicContentBlock::Text {
                    text: "The answer is 42.".to_string(),
                },
            ],
            stop_reason: Some("end_turn".to_string()),
            usage: Some(AnthropicUsage {
                input_tokens: 10,
                output_tokens: 20,
            }),
        };

        let mapped = AnthropicProvider::parse_completion_response(response);

        assert_eq!(
            mapped.content.len(),
            1,
            "redacted thinking block must be skipped"
        );
        assert!(
            matches!(&mapped.content[0], ContentBlock::Text { text } if text == "The answer is 42."),
            "only the text block should remain"
        );
        assert!(mapped.tool_calls.is_empty());
    }

    #[test]
    fn thinking_block_skipped_in_stream() {
        let payload = r#"
            data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}

            data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me reason..."}}

            data: {"type":"content_block_stop","index":0}

            data: {"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"base64-redacted"}}

            data: {"type":"content_block_stop","index":1}

            data: {"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}}

            data: {"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"Hello!"}}

            data: {"type":"content_block_stop","index":2}

            data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}

            data: [DONE]
        "#;

        let chunks = AnthropicProvider::parse_sse_payload(payload).unwrap();

        // Only the text delta and the message_delta (stop_reason) should produce chunks.
        // Thinking start/delta/stop must produce nothing.
        let text_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| c.delta_content.is_some())
            .collect();
        assert_eq!(text_chunks.len(), 1);
        assert_eq!(text_chunks[0].delta_content.as_deref(), Some("Hello!"));

        // No tool calls from thinking blocks
        assert!(
            chunks.iter().all(|c| c.tool_use_deltas.is_empty()),
            "thinking blocks must not produce tool use deltas"
        );
    }

    // --- Contract tests: Layer 1 request validation ---

    /// Helper to build a minimal valid `AnthropicRequestBody` for validation tests.
    fn valid_request_body(thinking: Option<AnthropicThinking>) -> AnthropicRequestBody {
        let max_tokens = match &thinking {
            Some(AnthropicThinking::Manual { budget_tokens, .. }) => {
                budget_tokens + MIN_RESPONSE_TOKENS
            }
            Some(AnthropicThinking::Adaptive { .. }) => 16_000,
            None => 4096,
        };
        AnthropicRequestBody {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: vec![AnthropicContentBlock::Text {
                    text: "hello".to_string(),
                }],
            }],
            tools: Vec::new(),
            temperature: if thinking.is_some() { None } else { Some(0.7) },
            max_tokens,
            system: None,
            stream: false,
            thinking,
            output_config: None,
        }
    }

    #[test]
    #[should_panic(expected = "temperature must be None")]
    fn validate_request_rejects_temperature_with_thinking() {
        let mut body = valid_request_body(Some(AnthropicThinking::Manual {
            thinking_type: "enabled".to_string(),
            budget_tokens: 5000,
        }));
        body.temperature = Some(0.7); // violates constraint
        validate_request(&body);
    }

    #[test]
    #[should_panic(expected = "max_tokens must be greater")]
    fn validate_request_rejects_low_max_tokens() {
        let mut body = valid_request_body(Some(AnthropicThinking::Manual {
            thinking_type: "enabled".to_string(),
            budget_tokens: 5000,
        }));
        body.max_tokens = 5000; // equal, not greater — violates constraint
        validate_request(&body);
    }

    #[test]
    fn validate_request_accepts_valid_thinking_request() {
        let body = valid_request_body(Some(AnthropicThinking::Manual {
            thinking_type: "enabled".to_string(),
            budget_tokens: 5000,
        }));
        validate_request(&body); // should not panic
    }

    #[test]
    fn validate_request_accepts_valid_non_thinking_request() {
        let body = valid_request_body(None);
        validate_request(&body); // should not panic
    }

    #[test]
    #[should_panic(expected = "model must be non-empty")]
    fn validate_request_rejects_empty_model() {
        let mut body = valid_request_body(None);
        body.model = String::new();
        validate_request(&body);
    }

    #[test]
    #[should_panic(expected = "messages must be non-empty")]
    fn validate_request_rejects_empty_messages() {
        let mut body = valid_request_body(None);
        body.messages = Vec::new();
        validate_request(&body);
    }

    #[test]
    #[should_panic(expected = "max_tokens must be > 0")]
    fn validate_request_rejects_zero_max_tokens() {
        let mut body = valid_request_body(None);
        body.max_tokens = 0;
        validate_request(&body);
    }

    #[test]
    #[should_panic(expected = "budget_tokens must be > 0")]
    fn validate_request_rejects_zero_budget() {
        let mut body = valid_request_body(Some(AnthropicThinking::Manual {
            thinking_type: "enabled".to_string(),
            budget_tokens: 0,
        }));
        body.max_tokens = 4096;
        validate_request(&body);
    }

    #[test]
    #[should_panic(expected = "must be <= MAX_THINKING_BUDGET")]
    fn validate_request_rejects_budget_over_cap() {
        let mut body = valid_request_body(Some(AnthropicThinking::Manual {
            thinking_type: "enabled".to_string(),
            budget_tokens: MAX_THINKING_BUDGET + 1,
        }));
        body.max_tokens = MAX_THINKING_BUDGET + 1 + MIN_RESPONSE_TOKENS;
        validate_request(&body);
    }

    // --- Contract tests: Layer 2 response fixtures ---

    #[test]
    fn fixture_response_text() {
        let json = include_str!("../tests/fixtures/anthropic/response_text.json");
        let body: AnthropicResponseBody =
            serde_json::from_str(json).expect("response_text.json must deserialize");
        assert_eq!(body.content.len(), 1);
        assert!(
            matches!(&body.content[0], AnthropicContentBlock::Text { text } if text == "Hello, world!")
        );
        assert_eq!(body.stop_reason.as_deref(), Some("end_turn"));
        let usage = body.usage.expect("usage must be present");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
    }

    #[test]
    fn fixture_response_thinking() {
        let json = include_str!("../tests/fixtures/anthropic/response_thinking.json");
        let body: AnthropicResponseBody =
            serde_json::from_str(json).expect("response_thinking.json must deserialize");
        assert_eq!(body.content.len(), 2);
        assert!(
            matches!(&body.content[0], AnthropicContentBlock::Thinking { thinking } if thinking == "Let me reason about this...")
        );
        assert!(
            matches!(&body.content[1], AnthropicContentBlock::Text { text } if text == "The answer is 42.")
        );
        assert_eq!(body.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn fixture_response_tool_call() {
        let json = include_str!("../tests/fixtures/anthropic/response_tool_call.json");
        let body: AnthropicResponseBody =
            serde_json::from_str(json).expect("response_tool_call.json must deserialize");
        assert_eq!(body.content.len(), 2);
        assert!(
            matches!(&body.content[0], AnthropicContentBlock::Text { text } if text == "I'll search for that.")
        );
        match &body.content[1] {
            AnthropicContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_01ABC");
                assert_eq!(name, "search_text");
                assert_eq!(input["query"], "hello world");
                assert_eq!(input["path"], ".");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
        assert_eq!(body.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn fixture_response_multi_tool() {
        let json = include_str!("../tests/fixtures/anthropic/response_multi_tool.json");
        let body: AnthropicResponseBody =
            serde_json::from_str(json).expect("response_multi_tool.json must deserialize");
        assert_eq!(body.content.len(), 3);
        assert!(
            matches!(&body.content[0], AnthropicContentBlock::Text { text } if text == "Let me check both.")
        );
        match &body.content[1] {
            AnthropicContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "toolu_01ABC");
                assert_eq!(name, "read_file");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
        match &body.content[2] {
            AnthropicContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "toolu_02DEF");
                assert_eq!(name, "list_directory");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
        assert_eq!(body.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn fixture_response_thinking_tool() {
        let json = include_str!("../tests/fixtures/anthropic/response_thinking_tool.json");
        let body: AnthropicResponseBody =
            serde_json::from_str(json).expect("response_thinking_tool.json must deserialize");
        assert_eq!(body.content.len(), 3);
        assert!(
            matches!(&body.content[0], AnthropicContentBlock::Thinking { thinking } if thinking == "I should read the file to understand the structure.")
        );
        assert!(
            matches!(&body.content[1], AnthropicContentBlock::Text { text } if text == "Let me check that file.")
        );
        match &body.content[2] {
            AnthropicContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_01XYZ");
                assert_eq!(name, "read_file");
                assert_eq!(input["path"], "Cargo.toml");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
        assert_eq!(body.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn fixture_response_redacted_thinking() {
        let json = include_str!("../tests/fixtures/anthropic/response_redacted_thinking.json");
        let body: AnthropicResponseBody =
            serde_json::from_str(json).expect("response_redacted_thinking.json must deserialize");
        assert_eq!(body.content.len(), 2);
        match &body.content[0] {
            AnthropicContentBlock::RedactedThinking { data } => {
                assert_eq!(data, "base64-redacted-content");
            }
            other => panic!("expected RedactedThinking, got {other:?}"),
        }
        assert!(
            matches!(&body.content[1], AnthropicContentBlock::Text { text } if text == "I can't share my internal reasoning, but here's the result.")
        );
        assert_eq!(body.stop_reason.as_deref(), Some("end_turn"));
        let usage = body.usage.expect("usage must be present");
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 9);
    }

    #[test]
    fn fixture_stream_text() {
        let sse = include_str!("../tests/fixtures/anthropic/stream_text.sse");
        let chunks = AnthropicProvider::parse_sse_payload(sse).expect("stream_text.sse must parse");
        let text: String = chunks
            .iter()
            .filter_map(|c| c.delta_content.as_deref())
            .collect();
        assert_eq!(text, "Hello, world!");
        let stop = chunks
            .iter()
            .find_map(|c| c.stop_reason.as_deref())
            .expect("stop_reason must be present");
        assert_eq!(stop, "end_turn");
    }

    #[test]
    fn fixture_stream_thinking() {
        let sse = include_str!("../tests/fixtures/anthropic/stream_thinking.sse");
        let chunks =
            AnthropicProvider::parse_sse_payload(sse).expect("stream_thinking.sse must parse");
        // Thinking deltas are silently skipped — only text should come through
        let text: String = chunks
            .iter()
            .filter_map(|c| c.delta_content.as_deref())
            .collect();
        assert_eq!(text, "The answer is 42.");
        let stop = chunks
            .iter()
            .find_map(|c| c.stop_reason.as_deref())
            .expect("stop_reason must be present");
        assert_eq!(stop, "end_turn");
    }

    #[test]
    fn fixture_stream_tool_call() {
        let sse = include_str!("../tests/fixtures/anthropic/stream_tool_call.sse");
        let chunks =
            AnthropicProvider::parse_sse_payload(sse).expect("stream_tool_call.sse must parse");
        // Text delta
        let text: String = chunks
            .iter()
            .filter_map(|c| c.delta_content.as_deref())
            .collect();
        assert_eq!(text, "I'll search.");
        // Tool use start
        let tool_start = chunks
            .iter()
            .flat_map(|c| &c.tool_use_deltas)
            .find(|d| d.name.is_some())
            .expect("tool start must be present");
        assert_eq!(tool_start.id.as_deref(), Some("toolu_01ABC"));
        assert_eq!(tool_start.name.as_deref(), Some("search_text"));
        // Tool argument deltas
        let args: String = chunks
            .iter()
            .flat_map(|c| &c.tool_use_deltas)
            .filter_map(|d| d.arguments_delta.as_deref())
            .collect();
        assert_eq!(args, r#"{"query":"hello"}"#);
        // Stop reason
        let stop = chunks
            .iter()
            .find_map(|c| c.stop_reason.as_deref())
            .expect("stop_reason must be present");
        assert_eq!(stop, "tool_use");
    }

    #[test]
    fn fixture_stream_multi_tool() {
        let sse = include_str!("../tests/fixtures/anthropic/stream_multi_tool.sse");
        let chunks =
            AnthropicProvider::parse_sse_payload(sse).expect("stream_multi_tool.sse must parse");
        // Text delta
        let text: String = chunks
            .iter()
            .filter_map(|c| c.delta_content.as_deref())
            .collect();
        assert_eq!(text, "Let me check both.");
        // Two tool starts
        let tool_starts: Vec<_> = chunks
            .iter()
            .flat_map(|c| &c.tool_use_deltas)
            .filter(|d| d.name.is_some())
            .collect();
        assert_eq!(tool_starts.len(), 2);
        assert_eq!(tool_starts[0].name.as_deref(), Some("read_file"));
        assert_eq!(tool_starts[1].name.as_deref(), Some("list_directory"));
    }

    #[test]
    fn fixture_error_invalid_request() {
        let json = include_str!("../tests/fixtures/anthropic/error_invalid_request.json");
        let value: serde_json::Value =
            serde_json::from_str(json).expect("error_invalid_request.json must be valid JSON");
        assert_eq!(value["type"], "error");
        assert_eq!(value["error"]["type"], "invalid_request_error");
        assert!(value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("max_tokens"));
    }
}
