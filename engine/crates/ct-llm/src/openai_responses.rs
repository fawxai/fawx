//! OpenAI Responses API provider for ChatGPT subscription (OAuth) auth.
//!
//! Uses the `chatgpt.com/backend-api/codex/responses` endpoint with
//! `Authorization: Bearer` + `chatgpt-account-id` headers.
//! This is the wire format used by ChatGPT Plus/Pro subscriptions via OAuth tokens.

use async_trait::async_trait;
use futures::{stream, StreamExt};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::VecDeque, time::Duration};

use crate::provider::{CompletionStream, LlmProvider};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, MessageRole, StreamChunk, Usage,
};

const DEFAULT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api";

/// OpenAI Responses API provider for ChatGPT subscription auth.
#[derive(Debug, Clone)]
pub struct OpenAiResponsesProvider {
    base_url: String,
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
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| LlmError::Config(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            base_url: DEFAULT_CODEX_BASE_URL.to_string(),
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
        self
    }

    /// Set explicit supported models list.
    pub fn with_supported_models(mut self, supported_models: Vec<String>) -> Self {
        self.supported_models = supported_models;
        self
    }

    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/codex/responses") {
            base.to_string()
        } else if base.ends_with("/codex") {
            format!("{base}/responses")
        } else {
            format!("{base}/codex/responses")
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
    ) -> Result<ResponsesRequestBody, LlmError> {
        self.ensure_supported_model(&request.model)?;

        let mut input = Vec::new();
        for message in &request.messages {
            match message.role {
                MessageRole::System => {
                    // System messages go into instructions, skip here
                }
                MessageRole::User | MessageRole::Tool => {
                    let text = extract_text(&message.content);
                    if !text.is_empty() {
                        input.push(ResponsesMessage {
                            role: "user".to_string(),
                            content: text,
                        });
                    }
                }
                MessageRole::Assistant => {
                    let text = extract_text(&message.content);
                    if !text.is_empty() {
                        input.push(ResponsesMessage {
                            role: "assistant".to_string(),
                            content: text,
                        });
                    }
                }
            }
        }

        Ok(ResponsesRequestBody {
            model: request.model.clone(),
            instructions: request.system_prompt.clone(),
            input,
            stream,
            store: false,
            temperature: None, // Codex Responses API does not support temperature
        })
    }

    fn build_headers(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::HeaderValue;
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&self.account_id) {
            headers.insert("chatgpt-account-id", val);
        }
        headers.insert(
            "openai-beta",
            HeaderValue::from_static("responses=experimental"),
        );
        headers.insert("originator", HeaderValue::from_static("citros"));
        headers
    }

    fn map_http_error(status: StatusCode, body: String) -> LlmError {
        match status.as_u16() {
            401 => LlmError::Authentication(format!("authentication failed: {body}")),
            403 => LlmError::Authentication(format!("forbidden: {body}")),
            404 => LlmError::Config(format!("endpoint not found: {body}")),
            429 => LlmError::RateLimited(format!("rate limited: {body}")),
            500..=599 => LlmError::Provider(format!("server error {}: {body}", status.as_u16())),
            _ => LlmError::Request(format!("http {}: {body}", status.as_u16())),
        }
    }

    fn parse_response(body: ResponsesResponseBody) -> CompletionResponse {
        let mut text_parts = Vec::new();

        for item in &body.output {
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

        let response_text = text_parts.join("");

        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: response_text,
            }],
            usage: body.usage.map(|u| Usage {
                input_tokens: u.input_tokens.unwrap_or(0) as u32,
                output_tokens: u.output_tokens.unwrap_or(0) as u32,
            }),
            stop_reason: body.status,
            tool_calls: Vec::new(),
        }
    }

    fn stream_from_sse(
        response: reqwest::Response,
    ) -> impl futures::Stream<Item = Result<StreamChunk, LlmError>> {
        let byte_stream = response.bytes_stream();
        let buffer = String::new();
        let pending: VecDeque<Result<StreamChunk, LlmError>> = VecDeque::new();
        let done = false;

        stream::unfold(
            (byte_stream, buffer, pending, done),
            move |(mut byte_stream, mut buffer, mut pending, mut done)| async move {
                loop {
                    if let Some(item) = pending.pop_front() {
                        return Some((item, (byte_stream, buffer, pending, done)));
                    }

                    if done {
                        return None;
                    }

                    match byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));

                            while let Some(event_end) = buffer.find("\n\n") {
                                let event_str = buffer[..event_end].to_string();
                                buffer = buffer[event_end + 2..].to_string();

                                let mut event_type = String::new();
                                let mut data = String::new();
                                for line in event_str.lines() {
                                    if let Some(val) = line.strip_prefix("event:") {
                                        event_type = val.trim().to_string();
                                    } else if let Some(val) = line.strip_prefix("data:") {
                                        data = val.trim().to_string();
                                    }
                                }

                                if data == "[DONE]" {
                                    done = true;
                                    break;
                                }

                                if data.is_empty() {
                                    continue;
                                }

                                match event_type.as_str() {
                                    "response.output_text.delta" => {
                                        if let Ok(parsed) =
                                            serde_json::from_str::<SseTextDelta>(&data)
                                        {
                                            pending.push_back(Ok(StreamChunk {
                                                delta_content: Some(parsed.delta),
                                                ..Default::default()
                                            }));
                                        }
                                    }
                                    "response.completed" | "response.done" => {
                                        if let Ok(parsed) =
                                            serde_json::from_str::<SseResponseDone>(&data)
                                        {
                                            if let Some(usage) =
                                                parsed.response.and_then(|r| r.usage)
                                            {
                                                pending.push_back(Ok(StreamChunk {
                                                    usage: Some(Usage {
                                                        input_tokens: usage
                                                            .input_tokens
                                                            .unwrap_or(0)
                                                            as u32,
                                                        output_tokens: usage
                                                            .output_tokens
                                                            .unwrap_or(0)
                                                            as u32,
                                                    }),
                                                    stop_reason: Some("stop".to_string()),
                                                    ..Default::default()
                                                }));
                                            }
                                        }
                                        done = true;
                                    }
                                    "response.failed" => {
                                        if let Ok(parsed) = serde_json::from_str::<Value>(&data) {
                                            let msg = parsed
                                                .pointer("/response/error/message")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("response failed")
                                                .to_string();
                                            pending.push_back(Err(LlmError::Provider(msg)));
                                        }
                                        done = true;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Err(e)) => {
                            pending.push_back(Err(LlmError::Request(e.to_string())));
                            done = true;
                        }
                        None => {
                            done = true;
                        }
                    }
                }
            },
        )
    }
}

#[async_trait]
impl LlmProvider for OpenAiResponsesProvider {
    fn name(&self) -> &str {
        &self.provider_name
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let body = self.build_request_body(&request, false)?;

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.access_token)
            .headers(self.build_headers())
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|e| format!("unable to read error body: {e}"));
            return Err(Self::map_http_error(status, body));
        }

        let parsed: ResponsesResponseBody = response
            .json()
            .await
            .map_err(|e| LlmError::InvalidResponse(e.to_string()))?;

        Ok(Self::parse_response(parsed))
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, LlmError> {
        let body = self.build_request_body(&request, true)?;

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.access_token)
            .headers(self.build_headers())
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|e| format!("unable to read error body: {e}"));
            return Err(Self::map_http_error(status, body));
        }

        Ok(Box::pin(Self::stream_from_sse(response)))
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported_models.clone()
    }
}

// ============================================================================
// Request/Response types
// ============================================================================

#[derive(Serialize)]
struct ResponsesRequestBody {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<ResponsesMessage>,
    stream: bool,
    store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct ResponsesMessage {
    role: String,
    content: String,
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
    response: Option<SseResponseBody>,
}

#[derive(Deserialize)]
struct SseResponseBody {
    usage: Option<ResponsesUsage>,
}

// ============================================================================
// Helpers
// ============================================================================

fn extract_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn parse_response_extracts_text() {
        let body = ResponsesResponseBody {
            output: vec![ResponsesOutputItem {
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
        };

        let body = provider.build_request_body(&request, false).unwrap();
        assert_eq!(body.model, "gpt-4.1");
        assert_eq!(body.instructions, Some("You are helpful.".to_string()));
        assert_eq!(body.input.len(), 1);
        assert_eq!(body.input[0].role, "user");
        assert!(!body.stream);
    }

    #[test]
    fn empty_credentials_rejected() {
        assert!(OpenAiResponsesProvider::new("", "account").is_err());
        assert!(OpenAiResponsesProvider::new("token", "").is_err());
    }

    #[test]
    fn headers_include_account_id_and_beta() {
        let provider = OpenAiResponsesProvider::new("token", "acct_123").unwrap();
        let headers = provider.build_headers();
        assert_eq!(headers.get("chatgpt-account-id").unwrap(), "acct_123");
        assert_eq!(
            headers.get("openai-beta").unwrap(),
            "responses=experimental"
        );
    }
}
