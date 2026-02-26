//! OpenAI-compatible chat completions provider.
//!
//! Supports OpenAI and OpenRouter style APIs via configurable `base_url`.

use async_trait::async_trait;
use bytes::Bytes;
use futures::{stream, Stream, StreamExt};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::VecDeque, time::Duration};

use crate::provider::{CompletionStream, LlmProvider};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message, MessageRole,
    StreamChunk, ToolCall, ToolDefinition, ToolUseDelta, Usage,
};

/// OpenAI-compatible provider implementation.
#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    base_url: String,
    api_key: String,
    provider_name: String,
    supported_models: Vec<String>,
    client: reqwest::Client,
}

impl OpenAiProvider {
    /// Create a new OpenAI-compatible provider.
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self, LlmError> {
        let base_url = base_url.into();
        let api_key = api_key.into();

        if base_url.trim().is_empty() {
            return Err(LlmError::Config("base_url cannot be empty".to_string()));
        }

        if api_key.trim().is_empty() {
            return Err(LlmError::Config("api_key cannot be empty".to_string()));
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| LlmError::Config(format!("failed to build HTTP client: {error}")))?;

        Ok(Self {
            base_url,
            api_key,
            provider_name: "openai-compatible".to_string(),
            supported_models: Vec::new(),
            client,
        })
    }

    /// Override provider name for logs/metrics.
    pub fn with_name(mut self, provider_name: impl Into<String>) -> Self {
        self.provider_name = provider_name.into();
        self
    }

    /// Set explicit supported models list.
    pub fn with_supported_models(mut self, supported_models: Vec<String>) -> Self {
        self.supported_models = supported_models;
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
                    content: Some(system_prompt.clone()),
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

        if let Some(text) = choice.message.content {
            if !text.is_empty() {
                content.push(ContentBlock::Text { text });
            }
        }

        if let Some(calls) = choice.message.tool_calls {
            for call in calls {
                let arguments = parse_json_or_string(&call.function.arguments);
                content.push(ContentBlock::ToolUse {
                    id: call.id.clone(),
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

    fn parse_sse_payload(payload: &str) -> Result<Vec<StreamChunk>, LlmError> {
        let mut chunks = Vec::new();

        for line in payload.lines() {
            match Self::parse_sse_line(line)? {
                ParsedSseLine::Ignore => {}
                ParsedSseLine::Done => break,
                ParsedSseLine::Chunks(mut parsed_chunks) => chunks.append(&mut parsed_chunks),
            }
        }

        Ok(chunks)
    }

    fn parse_sse_line(line: &str) -> Result<ParsedSseLine, LlmError> {
        let line = line.trim_start().trim_end_matches('\r');
        let Some(data) = line.strip_prefix("data:") else {
            return Ok(ParsedSseLine::Ignore);
        };

        let data = data.trim_start();
        if data.is_empty() {
            return Ok(ParsedSseLine::Ignore);
        }

        if data == "[DONE]" {
            return Ok(ParsedSseLine::Done);
        }

        Ok(ParsedSseLine::Chunks(Self::parse_sse_data(data)?))
    }

    fn parse_sse_data(data: &str) -> Result<Vec<StreamChunk>, LlmError> {
        let envelope: OpenAiStreamEnvelope = serde_json::from_str(data)
            .map_err(|error| LlmError::Streaming(format!("invalid SSE JSON: {error}")))?;

        let mut chunks = Vec::new();

        if let Some(usage) = envelope.usage {
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

        for choice in envelope.choices {
            if let Some(content) = choice.delta.content {
                chunks.push(StreamChunk {
                    delta_content: Some(content),
                    tool_use_deltas: Vec::new(),
                    usage: None,
                    stop_reason: None,
                });
            }

            if let Some(tool_calls) = choice.delta.tool_calls {
                let deltas = tool_calls
                    .into_iter()
                    .map(|tool_call| {
                        let (name, arguments_delta) = match tool_call.function {
                            Some(function) => (function.name, function.arguments),
                            None => (None, None),
                        };

                        ToolUseDelta {
                            id: tool_call.id,
                            name,
                            arguments_delta,
                        }
                    })
                    .collect::<Vec<_>>();

                if !deltas.is_empty() {
                    chunks.push(StreamChunk {
                        delta_content: None,
                        tool_use_deltas: deltas,
                        usage: None,
                        stop_reason: None,
                    });
                }
            }

            if let Some(stop_reason) = choice.finish_reason {
                chunks.push(StreamChunk {
                    delta_content: None,
                    tool_use_deltas: Vec::new(),
                    usage: None,
                    stop_reason: Some(stop_reason),
                });
            }
        }

        Ok(chunks)
    }

    fn stream_from_sse(response: reqwest::Response) -> impl Stream<Item = Result<StreamChunk, LlmError>> + Send {
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
                            state.buffer.extend_from_slice(&bytes);

                            if let Err(error) = Self::drain_buffered_lines(&mut state) {
                                state.finished = true;
                                return Some((Err(error), state));
                            }
                        }
                        Some(Err(error)) => {
                            state.finished = true;
                            return Some((Err(LlmError::Streaming(error.to_string())), state));
                        }
                        None => {
                            if !state.buffer.is_empty() {
                                let remaining = std::mem::take(&mut state.buffer);
                                let line = match std::str::from_utf8(&remaining) {
                                    Ok(line) => line,
                                    Err(error) => {
                                        state.finished = true;
                                        return Some((
                                            Err(LlmError::Streaming(format!(
                                                "stream was not valid UTF-8: {error}"
                                            ))),
                                            state,
                                        ));
                                    }
                                };

                                if let Err(error) = Self::enqueue_parsed_line(
                                    line,
                                    &mut state.pending_chunks,
                                    &mut state.finished,
                                ) {
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

    fn drain_buffered_lines(state: &mut OpenAiSseState) -> Result<(), LlmError> {
        while let Some(newline_index) = state.buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes = state.buffer.drain(..=newline_index).collect::<Vec<_>>();
            let line_bytes = &line_bytes[..line_bytes.len().saturating_sub(1)];

            let line = std::str::from_utf8(line_bytes)
                .map_err(|error| LlmError::Streaming(format!("stream was not valid UTF-8: {error}")))?;

            Self::enqueue_parsed_line(line, &mut state.pending_chunks, &mut state.finished)?;

            if state.finished {
                state.buffer.clear();
                break;
            }
        }

        Ok(())
    }

    fn enqueue_parsed_line(
        line: &str,
        pending_chunks: &mut VecDeque<Result<StreamChunk, LlmError>>,
        finished: &mut bool,
    ) -> Result<(), LlmError> {
        match Self::parse_sse_line(line)? {
            ParsedSseLine::Ignore => {}
            ParsedSseLine::Done => {
                *finished = true;
            }
            ParsedSseLine::Chunks(chunks) => {
                pending_chunks.extend(chunks.into_iter().map(Ok));
            }
        }

        Ok(())
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

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

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

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

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
        &self.provider_name
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported_models.clone()
    }
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
                    content: Some(extract_text(&message.content)),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            MessageRole::Assistant => {
                let text = extract_text(&message.content);
                let tool_calls = extract_tool_calls(&message.content)?;

                mapped.push(OpenAiMessage {
                    role: "assistant".to_string(),
                    content: if text.is_empty() { None } else { Some(text) },
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
                        content: Some(extract_text(&message.content)),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                } else {
                    for (tool_call_id, content) in tool_results {
                        mapped.push(OpenAiMessage {
                            role: "tool".to_string(),
                            content: Some(value_to_openai_content(content)),
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
            ContentBlock::ToolUse { id, name, input } => Some((id, name, input)),
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
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_json_or_string(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

fn value_to_openai_content(value: Value) -> String {
    match value {
        Value::String(content) => content,
        other => other.to_string(),
    }
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
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
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
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiToolFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

enum ParsedSseLine {
    Ignore,
    Done,
    Chunks(Vec<StreamChunk>),
}

struct OpenAiSseState {
    bytes_stream: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    buffer: Vec<u8>,
    pending_chunks: VecDeque<Result<StreamChunk, LlmError>>,
    finished: bool,
}

impl OpenAiSseState {
    fn new<S>(bytes_stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    {
        Self {
            bytes_stream: Box::pin(bytes_stream),
            buffer: Vec::new(),
            pending_chunks: VecDeque::new(),
            finished: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_build_request_body_maps_messages_tools_and_system() {
        let provider = OpenAiProvider::new("http://localhost:8080", "test-key")
            .unwrap()
            .with_name("openrouter")
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
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        assert_eq!(serialized["model"], "gpt-4o-mini");
        assert_eq!(serialized["stream"], false);
        assert_eq!(serialized["messages"][0]["role"], "system");
        assert_eq!(serialized["messages"][0]["content"], "Be concise");
        assert_eq!(serialized["messages"][1]["role"], "user");
        assert_eq!(serialized["tools"].as_array().unwrap().len(), 1);
        assert_eq!(serialized["tools"][0]["function"]["name"], "lookup");
    }

    #[test]
    fn test_parse_completion_response_maps_text_and_tool_calls() {
        let body = OpenAiResponseBody {
            choices: vec![OpenAiChoice {
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some("I can call a tool".to_string()),
                    tool_calls: Some(vec![OpenAiToolCall {
                        id: "call_1".to_string(),
                        call_type: "function".to_string(),
                        function: OpenAiFunctionCall {
                            name: "lookup".to_string(),
                            arguments: "{\"q\":\"citros\"}".to_string(),
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
        assert_eq!(mapped.tool_calls[0].arguments["q"], "citros");
        assert_eq!(mapped.stop_reason.as_deref(), Some("tool_calls"));

        let usage = mapped.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
    }

    #[test]
    fn test_parse_sse_payload_maps_text_tool_and_stop_chunks() {
        let payload = r#"
            data: {"choices":[{"delta":{"content":"hel"},"finish_reason":null}]}
            data: {"choices":[{"delta":{"tool_calls":[{"id":"call_1","function":{"name":"lookup","arguments":"{\"q\":\"ci"}}]},"finish_reason":null}]}
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
        let client_error = OpenAiProvider::map_http_error(StatusCode::BAD_REQUEST, "bad".to_string());
        assert!(matches!(client_error, LlmError::Request(message) if message.contains("client error 400")));

        let server_error = OpenAiProvider::map_http_error(StatusCode::INTERNAL_SERVER_ERROR, "oops".to_string());
        assert!(matches!(server_error, LlmError::Provider(message) if message.contains("server error 500")));
    }

    #[test]
    fn test_endpoint_avoids_duplicate_v1_segment() {
        let with_v1 = OpenAiProvider::new("https://api.openai.com/v1", "test-key").unwrap();
        assert_eq!(with_v1.endpoint(), "https://api.openai.com/v1/chat/completions");

        let without_v1 = OpenAiProvider::new("https://api.openai.com", "test-key").unwrap();
        assert_eq!(without_v1.endpoint(), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn test_map_messages_to_openai_serializes_structured_tool_result_content() {
        let messages = vec![Message {
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: json!({"ok": true, "items": [1, 2]}),
            }],
        }];

        let mapped = map_messages_to_openai(&messages).unwrap();
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].role, "tool");
        assert_eq!(mapped[0].tool_call_id.as_deref(), Some("call_1"));

        let expected: serde_json::Value =
            serde_json::from_str(r#"{"ok": true, "items": [1,2]}"#).unwrap();
        let actual_content = mapped[0].content.as_ref().unwrap();
        let actual: serde_json::Value = serde_json::from_str(actual_content).unwrap();
        assert_eq!(expected, actual);
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
        };

        let result = provider.build_request_body(&request, false);
        assert!(matches!(result, Err(LlmError::UnsupportedModel(_))));
    }
}
