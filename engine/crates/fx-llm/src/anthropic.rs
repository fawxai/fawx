//! Anthropic Messages API provider.

use async_trait::async_trait;
use futures::{stream, Stream, StreamExt};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

use crate::provider::{CompletionStream, LlmProvider, ProviderCapabilities};
use crate::sse::{SseFrame, SseFramer};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message, MessageRole,
    StreamChunk, ThinkingConfig, ToolCall, ToolUseDelta, Usage,
};

/// Anthropic auth mode — determines how credentials are sent.
#[derive(Debug, Clone)]
pub enum AnthropicAuthMode {
    /// Standard API key: sent as `x-api-key` header.
    ApiKey(String),
    /// OAuth/setup-token (`sk-ant-oat...`): sent as `Authorization: Bearer` with
    /// Claude Code identity headers. Matches OpenClaw's behavior.
    SetupToken(String),
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
            .timeout(Duration::from_secs(60))
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

    /// Apply auth headers to a request builder based on auth mode.
    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth_mode {
            AnthropicAuthMode::ApiKey(key) => builder.header("x-api-key", key),
            AnthropicAuthMode::SetupToken(token) => builder
                .header("Authorization", format!("Bearer {token}"))
                .header("anthropic-beta", "claude-code-20250219,oauth-2025-04-20")
                .header("x-app", "cli"),
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

        let thinking = match &request.thinking {
            Some(ThinkingConfig::Enabled { budget_tokens }) => Some(AnthropicThinking {
                thinking_type: "enabled".to_string(),
                budget_tokens: *budget_tokens,
            }),
            Some(ThinkingConfig::Off) | None => None,
        };

        Ok(AnthropicRequestBody {
            model: request.model.clone(),
            messages,
            tools,
            temperature: request.temperature,
            max_tokens: request.max_tokens.unwrap_or(4096),
            system: system_prompt,
            stream,
            thinking,
        })
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
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
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

        Ok(Box::pin(Self::stream_from_sse(response)))
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported_models.clone()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_temperature: true,
            requires_streaming: false,
        }
    }
}

fn map_content_to_anthropic(block: &ContentBlock) -> Result<AnthropicContentBlock, LlmError> {
    match block {
        ContentBlock::Text { text } => Ok(AnthropicContentBlock::Text { text: text.clone() }),
        ContentBlock::ToolUse { id, name, input } => Ok(AnthropicContentBlock::ToolUse {
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
    }
}

fn extract_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
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
    system: Option<String>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<AnthropicThinking>,
}

/// Anthropic extended thinking parameter.
#[derive(Debug, Serialize)]
struct AnthropicThinking {
    #[serde(rename = "type")]
    thinking_type: String,
    budget_tokens: u32,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolDefinition;
    use serde_json::json;

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
}
