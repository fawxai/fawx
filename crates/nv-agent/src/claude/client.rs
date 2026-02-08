//! Claude API client implementation.

use super::config::ClaudeConfig;
use super::error::{AgentError, Result};
use super::types::{
    CompletionResponse, ContentBlock, Message, StopReason, StreamEvent, Tool, Usage,
};
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

/// HTTP client for Claude API.
pub struct ClaudeClient {
    config: ClaudeConfig,
    client: reqwest::Client,
}

impl ClaudeClient {
    /// Create a new Claude API client.
    pub fn new(config: ClaudeConfig) -> Result<Self> {
        config.validate()?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| AgentError::ApiRequest(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }

    /// Send a completion request to Claude API.
    pub async fn complete(
        &self,
        messages: &[Message],
        tools: Option<&[Tool]>,
    ) -> Result<CompletionResponse> {
        let request_body = self.build_request_body(messages, tools, false)?;
        let response = self.send_request(&request_body).await?;
        self.parse_response(response).await
    }

    /// Send a streaming completion request to Claude API.
    pub async fn complete_streaming<F>(
        &self,
        messages: &[Message],
        tools: Option<&[Tool]>,
        mut callback: F,
    ) -> Result<CompletionResponse>
    where
        F: FnMut(StreamEvent) + Send,
    {
        let request_body = self.build_request_body(messages, tools, true)?;
        let response = self.send_request(&request_body).await?;

        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let mut stream = response.bytes_stream();
        let mut accumulated_text = String::new();
        let mut content_blocks: Vec<ContentBlock> = Vec::new();
        let mut usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
        };
        let mut stop_reason = StopReason::EndTurn;

        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| AgentError::ApiRequest(format!("Stream error: {}", e)))?;

            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data.trim() == "[DONE]" {
                        callback(StreamEvent::MessageStop);
                        continue;
                    }

                    let event: StreamEventData = serde_json::from_str(data).map_err(|e| {
                        AgentError::InvalidResponse(format!("Failed to parse SSE: {}", e))
                    })?;

                    match event.event_type.as_str() {
                        "content_block_start" => {
                            // Handle ToolUse content blocks
                            if let Some(block) = event.content_block {
                                if matches!(block, ContentBlock::ToolUse { .. }) {
                                    content_blocks.push(block);
                                }
                            }
                            callback(StreamEvent::ContentBlockStart);
                        }
                        "content_block_delta" => {
                            if let Some(delta) = event.delta {
                                if let Some(text) = delta.text {
                                    accumulated_text.push_str(&text);
                                    callback(StreamEvent::ContentBlockDelta(text));
                                }
                            }
                        }
                        "content_block_stop" => {
                            if !accumulated_text.is_empty() {
                                content_blocks.push(ContentBlock::Text {
                                    text: accumulated_text.clone(),
                                });
                                accumulated_text.clear();
                            }
                            callback(StreamEvent::ContentBlockStop);
                        }
                        "message_delta" => {
                            if let Some(msg_usage) = event.usage {
                                usage.output_tokens = msg_usage.output_tokens;
                            }
                            if let Some(reason) = event.stop_reason {
                                stop_reason = reason;
                            }
                        }
                        "message_start" => {
                            if let Some(message) = event.message {
                                if let Some(msg_usage) = message.usage {
                                    usage.input_tokens = msg_usage.input_tokens;
                                }
                            }
                        }
                        "message_stop" => {
                            callback(StreamEvent::MessageStop);
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(CompletionResponse {
            content: content_blocks,
            stop_reason,
            usage,
        })
    }

    /// Build the request body for the API call.
    fn build_request_body(
        &self,
        messages: &[Message],
        tools: Option<&[Tool]>,
        stream: bool,
    ) -> Result<Value> {
        let mut body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "messages": messages,
            "stream": stream,
        });

        if let Some(tools) = tools {
            if !tools.is_empty() {
                body["tools"] = serde_json::to_value(tools)?;
            }
        }

        Ok(body)
    }

    /// Send the HTTP request to Claude API.
    async fn send_request(&self, body: &Value) -> Result<reqwest::Response> {
        let url = format!("{}/v1/messages", self.config.base_url);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.config.api_key)
                .map_err(|e| AgentError::Config(format!("Invalid API key: {}", e)))?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_str(&self.config.api_version)
                .map_err(|e| AgentError::Config(format!("Invalid API version: {}", e)))?,
        );

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    AgentError::Timeout(format!("Request timed out: {}", e))
                } else {
                    AgentError::ApiRequest(format!("Request failed: {}", e))
                }
            })?;

        Ok(response)
    }

    /// Parse a successful response.
    async fn parse_response(&self, response: reqwest::Response) -> Result<CompletionResponse> {
        if !response.status().is_success() {
            return Err(self.handle_error_response(response).await);
        }

        let body: ApiResponse = response
            .json()
            .await
            .map_err(|e| AgentError::InvalidResponse(format!("Failed to parse JSON: {}", e)))?;

        Ok(CompletionResponse {
            content: body.content,
            stop_reason: body.stop_reason,
            usage: body.usage,
        })
    }

    /// Handle error responses from the API.
    async fn handle_error_response(&self, response: reqwest::Response) -> AgentError {
        let status = response.status();
        let status_code = status.as_u16();

        let body = response
            .text()
            .await
            .unwrap_or_else(|e| format!("Failed to read error body: {}", e));

        match status_code {
            401 => AgentError::Auth(format!("Authentication failed: {}", body)),
            400 => AgentError::BadRequest(format!("Bad request: {}", body)),
            429 => AgentError::RateLimit(format!("Rate limit exceeded: {}", body)),
            500..=599 => {
                AgentError::ServerError(format!("Server error ({}): {}", status_code, body))
            }
            _ => AgentError::ApiRequest(format!("HTTP {}: {}", status_code, body)),
        }
    }
}

/// Internal API response structure.
#[derive(Debug, Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    stop_reason: StopReason,
    usage: Usage,
}

/// Internal streaming event data.
#[derive(Debug, Deserialize)]
struct StreamEventData {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<StreamDelta>,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    message: Option<StreamMessage>,
    #[serde(default)]
    stop_reason: Option<StopReason>,
    #[serde(default)]
    content_block: Option<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamMessage {
    #[serde(default)]
    usage: Option<Usage>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_client() {
        let config = ClaudeConfig::new("test-api-key").unwrap();
        let client = ClaudeClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_new_client_invalid_config() {
        let config = ClaudeConfig::default(); // Empty API key
        let client = ClaudeClient::new(config);
        assert!(client.is_err());
    }

    #[test]
    fn test_build_request_body_simple() {
        let config = ClaudeConfig::new("test-key").unwrap();
        let client = ClaudeClient::new(config).unwrap();

        let messages = vec![Message::user("Hello")];
        let body = client.build_request_body(&messages, None, false).unwrap();

        assert_eq!(body["model"], "claude-sonnet-4-5");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["stream"], false);
        assert!(body["messages"].is_array());
    }

    #[test]
    fn test_build_request_body_with_tools() {
        let config = ClaudeConfig::new("test-key").unwrap();
        let client = ClaudeClient::new(config).unwrap();

        let messages = vec![Message::user("Hello")];
        let tools = vec![Tool::new("test_tool", "A test tool", serde_json::json!({}))];
        let body = client
            .build_request_body(&messages, Some(&tools), false)
            .unwrap();

        assert!(body["tools"].is_array());
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_build_request_body_streaming() {
        let config = ClaudeConfig::new("test-key").unwrap();
        let client = ClaudeClient::new(config).unwrap();

        let messages = vec![Message::user("Hello")];
        let body = client.build_request_body(&messages, None, true).unwrap();

        assert_eq!(body["stream"], true);
    }
}
