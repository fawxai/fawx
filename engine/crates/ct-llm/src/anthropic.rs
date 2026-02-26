//! Anthropic Messages API provider.

use async_trait::async_trait;
use futures::stream;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

use crate::provider::{CompletionStream, LlmProvider};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message, MessageRole,
    StreamChunk, ToolCall, ToolDefinition, ToolUseDelta, Usage,
};

/// Anthropic API provider implementation.
#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    base_url: String,
    api_key: String,
    api_version: String,
    supported_models: Vec<String>,
    client: reqwest::Client,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider.
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
                        Some(existing) if !existing.is_empty() => format!("{existing}\n{extracted}"),
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

        Ok(AnthropicRequestBody {
            model: request.model.clone(),
            messages,
            tools,
            temperature: request.temperature,
            max_tokens: request.max_tokens.unwrap_or(1024),
            system: system_prompt,
            stream,
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
                        content: value_to_string(result),
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

    fn parse_sse_payload(payload: &str) -> Result<Vec<StreamChunk>, LlmError> {
        let mut chunks = Vec::new();

        for line in payload.lines() {
            let line = line.trim_start();
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };

            let trimmed = data.trim();
            if trimmed.is_empty() || trimmed == "[DONE]" {
                continue;
            }

            let event: AnthropicStreamEvent = serde_json::from_str(trimmed)
                .map_err(|error| LlmError::Streaming(format!("invalid SSE JSON: {error}")))?;

            match event.event_type.as_str() {
                "content_block_start" => {
                    if let Some(AnthropicContentBlock::ToolUse { id, name, .. }) = event.content_block
                    {
                        chunks.push(StreamChunk {
                            delta_content: None,
                            tool_use_deltas: vec![ToolUseDelta {
                                id: Some(id),
                                name: Some(name),
                                arguments_delta: None,
                            }],
                            usage: None,
                            stop_reason: None,
                        });
                    }
                }
                "content_block_delta" => {
                    if let Some(delta) = event.delta {
                        if let Some(text) = delta.text {
                            chunks.push(StreamChunk {
                                delta_content: Some(text),
                                tool_use_deltas: Vec::new(),
                                usage: None,
                                stop_reason: None,
                            });
                        }

                        if let Some(partial_json) = delta.partial_json {
                            chunks.push(StreamChunk {
                                delta_content: None,
                                tool_use_deltas: vec![ToolUseDelta {
                                    id: None,
                                    name: None,
                                    arguments_delta: Some(partial_json),
                                }],
                                usage: None,
                                stop_reason: None,
                            });
                        }
                    }
                }
                "message_start" => {
                    if let Some(usage) = event.message.and_then(|message| message.usage) {
                        chunks.push(StreamChunk {
                            delta_content: None,
                            tool_use_deltas: Vec::new(),
                            usage: Some(Usage {
                                input_tokens: usage.input_tokens,
                                output_tokens: usage.output_tokens,
                            }),
                            stop_reason: None,
                        });
                    }
                }
                "message_delta" => {
                    let usage = event.usage.map(|usage| Usage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                    });
                    let stop_reason = event.delta.and_then(|delta| delta.stop_reason);

                    if usage.is_some() || stop_reason.is_some() {
                        chunks.push(StreamChunk {
                            delta_content: None,
                            tool_use_deltas: Vec::new(),
                            usage,
                            stop_reason,
                        });
                    }
                }
                _ => {}
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

        let response = self
            .client
            .post(self.endpoint())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
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

        let response = self
            .client
            .post(self.endpoint())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
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

        // We parse SSE payload into logical stream chunks to keep provider output
        // format deterministic and provider-agnostic.
        let payload = response
            .text()
            .await
            .map_err(|error| LlmError::Streaming(error.to_string()))?;
        let chunks = Self::parse_sse_payload(&payload)?;

        Ok(Box::new(stream::iter(chunks.into_iter().map(Ok))))
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported_models.clone()
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
            content: Value::String(content.clone()),
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

fn value_to_string(value: Value) -> String {
    match value {
        Value::String(value) => value,
        other => other.to_string(),
    }
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
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: Value },
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

#[cfg(test)]
mod tests {
    use super::*;
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
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(body).unwrap();

        assert_eq!(serialized["model"], "claude-3-7-sonnet");
        assert_eq!(serialized["stream"], false);
        assert_eq!(serialized["max_tokens"], 256);
        assert_eq!(serialized["messages"].as_array().unwrap().len(), 1);
        assert_eq!(
            serialized["system"],
            "System prelude\nFollow policy"
        );
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
                    input: json!({"query":"citros"}),
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
        assert_eq!(mapped.tool_calls[0].arguments["query"], "citros");
        assert_eq!(mapped.stop_reason.as_deref(), Some("tool_use"));

        let usage = mapped.usage.unwrap();
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 34);
    }

    #[test]
    fn test_parse_sse_payload_maps_text_tool_and_usage_chunks() {
        let payload = r#"
            data: {"type":"content_block_delta","delta":{"text":"hel"}}
            data: {"type":"content_block_start","content_block":{"type":"tool_use","id":"toolu_01","name":"search","input":{}}}
            data: {"type":"content_block_delta","delta":{"partial_json":"{\"query\":\"cit"}}
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

        assert_eq!(chunks[3].usage.unwrap().output_tokens, 9);
        assert_eq!(chunks[3].stop_reason.as_deref(), Some("end_turn"));
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
        };

        let result = provider.build_request_body(&request, false);
        assert!(matches!(result, Err(LlmError::UnsupportedModel(_))));
    }
}
