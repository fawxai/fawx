//! OpenAI Responses API provider for ChatGPT subscription (OAuth) auth.
//!
//! Uses the `chatgpt.com/backend-api/codex/responses` endpoint with
//! `Authorization: Bearer` + `chatgpt-account-id` headers.
//! This is the wire format used by ChatGPT Plus/Pro subscriptions via OAuth tokens.

use async_trait::async_trait;
use futures::{stream, SinkExt, StreamExt};
use http::{header::HeaderValue, Request};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest, Message as WsMessage};

use crate::provider::{CompletionStream, LlmProvider, ProviderCapabilities};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, Message, MessageRole,
    StreamChunk, ToolCall, ToolUseDelta, Usage,
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

        Ok(Self {
            base_url: DEFAULT_CODEX_BASE_URL.to_string(),
            access_token,
            account_id,
            provider_name: "openai".to_string(),
            supported_models: Vec::new(),
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
        if base.ends_with("/responses") {
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
            map_message_to_responses_input(&mut input, message)?;
        }

        let tools = request
            .tools
            .iter()
            .map(|tool| ResponsesTool {
                r#type: "function".to_string(),
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            })
            .collect();

        Ok(ResponsesRequestBody {
            model: request.model.clone(),
            instructions: request.system_prompt.clone(),
            input,
            tools,
            stream,
            store: false,
            temperature: request.temperature,
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

    #[cfg(test)]
    fn parse_response(body: ResponsesResponseBody) -> CompletionResponse {
        let response_text = collect_output_text(&body.output);
        let tool_calls = collect_tool_calls(&body.output);

        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: response_text,
            }],
            usage: body.usage.map(|u| Usage {
                input_tokens: u.input_tokens.unwrap_or(0) as u32,
                output_tokens: u.output_tokens.unwrap_or(0) as u32,
            }),
            stop_reason: body.status,
            tool_calls,
        }
    }

    async fn complete_via_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let mut stream = self.complete_stream(request).await?;
        let mut text = String::new();
        let mut usage = None;
        let mut stop_reason = None;
        let mut pending_tool_calls = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            text.push_str(chunk.delta_content.as_deref().unwrap_or_default());
            usage = chunk.usage.or(usage);
            stop_reason = chunk.stop_reason.or(stop_reason);
            merge_tool_use_deltas(&mut pending_tool_calls, chunk.tool_use_deltas);
        }

        Ok(CompletionResponse {
            content: vec![ContentBlock::Text { text }],
            tool_calls: finalize_tool_calls(pending_tool_calls),
            usage,
            stop_reason,
        })
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
            "response.output_item.added" | "response.output_item.done" => {
                tool_delta_from_output_item_event(data).map(chunk_from_tool_delta)
            }
            "response.completed" | "response.done" => usage_chunk_from_done_event(data),
            _ => None,
        }
    }

    fn stream_from_ws<S>(
        read: futures::stream::SplitStream<S>,
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
                    Some(Ok(WsMessage::Close(_))) | None => return None,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        let message = format!("websocket receive failed: {error}");
                        return Some((Err(LlmError::Provider(message)), (read, true)));
                    }
                }
            }
        })
    }
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
    let id = parsed.item_id.or(parsed.call_id);

    if id.is_none() && parsed.name.is_none() && arguments_delta.is_none() {
        return None;
    }

    Some(ToolUseDelta {
        id,
        name: parsed.name,
        arguments_delta,
    })
}

fn tool_delta_from_output_item_event(data: &str) -> Option<ToolUseDelta> {
    let parsed = serde_json::from_str::<SseOutputItemEvent>(data).ok()?;
    let item = parsed.item?;
    if item.r#type.as_deref() != Some("function_call") {
        return None;
    }

    let arguments_delta = item.arguments.filter(|arguments| !arguments.is_empty());
    let id = item.id.or(item.call_id).or(parsed.item_id);
    let name = item.name;
    if id.is_none() && name.is_none() && arguments_delta.is_none() {
        return None;
    }

    Some(ToolUseDelta {
        id,
        name,
        arguments_delta,
    })
}

fn usage_chunk_from_done_event(data: &str) -> Option<StreamChunk> {
    let parsed = serde_json::from_str::<SseResponseDone>(data).ok()?;
    let SseResponseDone { response, usage } = parsed;
    let usage = response.and_then(|response| response.usage).or(usage)?;

    Some(StreamChunk {
        usage: Some(responses_usage_to_usage(usage)),
        stop_reason: Some("stop".to_string()),
        ..Default::default()
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

fn push_text_input(input: &mut Vec<Value>, role: &str, blocks: &[ContentBlock]) {
    let text = extract_text(blocks);
    if !text.is_empty() {
        input.push(json!({"role": role, "content": text}));
    }
}

fn push_assistant_input(input: &mut Vec<Value>, blocks: &[ContentBlock]) -> Result<(), LlmError> {
    push_text_input(input, "assistant", blocks);

    for block in blocks {
        if let ContentBlock::ToolUse {
            id,
            name,
            input: tool_input,
        } = block
        {
            let arguments = serde_json::to_string(tool_input)
                .map_err(|error| LlmError::Serialization(error.to_string()))?;
            input.push(json!({
                "type": "function_call",
                "id": id,
                "call_id": id,
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
            input.push(json!({
                "type": "function_call_output",
                "call_id": tool_use_id,
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
        self.complete_via_stream(request).await
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

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_temperature: false,
            requires_streaming: true,
        }
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
    input: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ResponsesTool>,
    stream: bool,
    store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
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
#[cfg(test)]
struct ResponsesResponseBody {
    #[serde(default)]
    output: Vec<ResponsesOutputItem>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Deserialize)]
#[cfg(test)]
struct ResponsesOutputItem {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    id: Option<String>,
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
#[cfg(test)]
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

#[cfg(test)]
fn collect_tool_calls(output: &[ResponsesOutputItem]) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();

    for item in output {
        if item.r#type.as_deref() != Some("function_call") {
            continue;
        }

        if let (Some(id), Some(name), Some(arguments)) = (&item.id, &item.name, &item.arguments) {
            let arguments_value = match serde_json::from_str::<Value>(arguments) {
                Ok(value) => value,
                Err(_) => Value::String(arguments.clone()),
            };

            tool_calls.push(ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments_value,
            });
        }
    }

    tool_calls
}

#[derive(Default)]
struct PendingToolCall {
    id: Option<String>,
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
    if let Some(index) = pending_index_by_id(pending, delta.id.as_deref()) {
        return index;
    }

    if let Some(index) = pending_index_without_id(pending, delta) {
        return index;
    }

    pending.push(PendingToolCall::default());
    pending.len() - 1
}

fn pending_index_by_id(pending: &[PendingToolCall], id: Option<&str>) -> Option<usize> {
    let id = id?;
    pending
        .iter()
        .position(|call| call.id.as_deref() == Some(id))
}

fn pending_index_without_id(pending: &[PendingToolCall], delta: &ToolUseDelta) -> Option<usize> {
    if delta.id.is_some() {
        return pending.iter().rposition(|call| {
            call.id.is_none() && same_or_unknown_name(call.name.as_deref(), delta.name.as_deref())
        });
    }

    if let Some(name) = delta.name.as_deref() {
        return pending.iter().rposition(|call| {
            call.id.is_none() && same_or_unknown_name(call.name.as_deref(), Some(name))
        });
    }

    pending
        .iter()
        .enumerate()
        .filter(|(_, call)| call.id.is_none() || call.name.is_none())
        .map(|(index, _)| index)
        .last()
}

fn same_or_unknown_name(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

fn apply_tool_use_delta(call: &mut PendingToolCall, delta: ToolUseDelta) {
    if call.id.is_none() {
        call.id = delta.id;
    }

    if call.name.is_none() {
        call.name = delta.name;
    }

    if let Some(arguments_delta) = delta.arguments_delta {
        merge_arguments(&mut call.arguments, &arguments_delta);
    }
}

fn merge_arguments(arguments: &mut String, incoming: &str) {
    if incoming.is_empty() {
        return;
    }

    if arguments.is_empty() {
        arguments.push_str(incoming);
        return;
    }

    if incoming.starts_with(arguments.as_str()) {
        *arguments = incoming.to_string();
        return;
    }

    if arguments.starts_with(incoming) {
        return;
    }

    arguments.push_str(incoming);
}

fn finalize_tool_calls(pending: Vec<PendingToolCall>) -> Vec<ToolCall> {
    pending
        .into_iter()
        .filter_map(|call| {
            let id = call.id?.trim().to_string();
            let name = call.name?.trim().to_string();
            if id.is_empty() || name.is_empty() {
                return None;
            }
            let arguments = serde_json::from_str::<Value>(&call.arguments)
                .unwrap_or(Value::String(call.arguments));

            Some(ToolCall {
                id,
                name,
                arguments,
            })
        })
        .collect()
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
                r#type: None,
                id: None,
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

    fn continuation_assistant_message() -> crate::types::Message {
        crate::types::Message {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "lookup".to_string(),
                    input: serde_json::json!({"q": "first"}),
                },
                ContentBlock::ToolUse {
                    id: "call_2".to_string(),
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
        }
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
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(body.model, "gpt-4.1");
        assert_eq!(body.instructions, Some("You are helpful.".to_string()));
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"], "Hello");
        assert!(!body.stream);
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
            serde_json::json!({"role": "user", "content": "Find weather"})
        );
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["id"], "call_1");
        assert_eq!(input[1]["call_id"], "call_1");
        assert_eq!(input[1]["name"], "lookup");
        assert_eq!(input[1]["arguments"], "{\"q\":\"first\"}");
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["id"], "call_2");
        assert_eq!(input[2]["call_id"], "call_2");
        assert_eq!(input[2]["name"], "lookup");
        assert_eq!(input[2]["arguments"], "{\"q\":\"second\"}");
        assert_eq!(
            input[3],
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "first result"
            })
        );
        assert_eq!(
            input[4],
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "call_2",
                "output": "second result"
            })
        );
    }

    #[test]
    fn test_build_request_body_maps_tool_result_string_content() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![crate::types::Message {
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: Value::String("tool output".to_string()),
                }],
            }],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[0]["output"], "tool output");
    }

    #[test]
    fn test_build_request_body_maps_tool_result_error_prefix() {
        let provider = OpenAiResponsesProvider::new("token", "account").unwrap();
        let request = CompletionRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![crate::types::Message {
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: Value::String("[ERROR] permission denied".to_string()),
                }],
            }],
            system_prompt: None,
            tools: vec![],
            temperature: None,
            max_tokens: None,
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        let input = serialized["input"].as_array().unwrap();

        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[0]["output"], "[ERROR] permission denied");
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
        };

        let body = provider.build_request_body(&request, false).unwrap();
        let serialized = serde_json::to_value(&body).unwrap();
        assert!(serialized.get("tools").is_none());
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
        assert_eq!(chunk.tool_use_deltas[0].name.as_deref(), Some("lookup"));
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
                name: Some("emit_intent".to_string()),
                arguments_delta: Some("{\"intent\":\"op".to_string()),
            }],
        );
        merge_tool_use_deltas(
            &mut pending,
            vec![ToolUseDelta {
                id: Some("call_1".to_string()),
                name: Some("emit_intent".to_string()),
                arguments_delta: Some("en\"}".to_string()),
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
                name: None,
                arguments_delta: Some("{\"intent\":\"op".to_string()),
            }],
        );
        merge_tool_use_deltas(
            &mut pending,
            vec![ToolUseDelta {
                id: Some("call_1".to_string()),
                name: Some("emit_intent".to_string()),
                arguments_delta: Some("en\"}".to_string()),
            }],
        );

        let tool_calls = finalize_tool_calls(pending);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "emit_intent");
        assert_eq!(tool_calls[0].arguments["intent"], "open");
    }
}
