use futures::StreamExt;
use serde_json::Value;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use crate::provider::CompletionStream;
use crate::types::{
    CompletionResponse, ContentBlock, LlmError, StreamChunk, ToolCall, ToolUseDelta, Usage,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta {
        text: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        args_delta: String,
    },
    ToolCallComplete {
        id: String,
        name: String,
        arguments: String,
    },
    Done {
        response: String,
    },
}

pub type StreamCallback = Arc<dyn Fn(StreamEvent) + Send + Sync>;

pub(crate) async fn collect_completion_stream(
    stream: &mut CompletionStream,
    callback: &StreamCallback,
) -> Result<CompletionResponse, LlmError> {
    let mut collector = StreamCollector::default();

    while let Some(chunk) = stream.next().await {
        collector.ingest(chunk?, callback);
    }

    Ok(collector.finish(callback))
}

#[cfg(test)]
pub(crate) fn collect_stream_chunks<I>(chunks: I, callback: &StreamCallback) -> CompletionResponse
where
    I: IntoIterator<Item = StreamChunk>,
{
    let mut collector = StreamCollector::default();

    for chunk in chunks {
        collector.ingest(chunk, callback);
    }

    collector.finish(callback)
}

pub fn emit_default_stream_response(response: &CompletionResponse, callback: &StreamCallback) {
    let text = completion_text(response);
    if !text.is_empty() {
        emit_event(callback, StreamEvent::TextDelta { text: text.clone() });
    }
    emit_event(callback, StreamEvent::Done { response: text });
}

pub fn completion_text(response: &CompletionResponse) -> String {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            ContentBlock::Image { .. } => None,
            ContentBlock::Document { .. } => None,
            _ => None,
        })
        .collect::<String>()
}

pub(crate) fn emit_event(callback: &StreamCallback, event: StreamEvent) {
    if catch_unwind(AssertUnwindSafe(|| (callback)(event))).is_err() {
        tracing::warn!("stream callback panicked; continuing provider stream");
    }
}

#[derive(Debug, Default)]
struct StreamCollector {
    text: String,
    tool_calls: Vec<PendingToolCall>,
    usage: Option<Usage>,
    stop_reason: Option<String>,
}

impl StreamCollector {
    fn ingest(&mut self, chunk: StreamChunk, callback: &StreamCallback) {
        self.ingest_text(chunk.delta_content, callback);
        self.ingest_tool_deltas(chunk.tool_use_deltas, callback);
        self.merge_usage(chunk.usage);
        self.merge_stop_reason(chunk.stop_reason);
    }

    fn ingest_text(&mut self, text: Option<String>, callback: &StreamCallback) {
        let Some(text) = text else {
            return;
        };
        self.text.push_str(&text);
        emit_event(callback, StreamEvent::TextDelta { text });
    }

    fn ingest_tool_deltas(&mut self, deltas: Vec<ToolUseDelta>, callback: &StreamCallback) {
        for delta in deltas {
            self.ingest_tool_delta(delta, callback);
        }
    }

    fn ingest_tool_delta(&mut self, delta: ToolUseDelta, callback: &StreamCallback) {
        let Some(index) = self.resolve_tool_index(&delta) else {
            tracing::warn!(?delta, "dropping tool delta without identifiable tool call");
            return;
        };

        let events = self.apply_tool_delta(index, delta);
        emit_events(callback, events);
    }

    fn resolve_tool_index(&mut self, delta: &ToolUseDelta) -> Option<usize> {
        match delta.id.as_deref() {
            Some(id) => Some(self.ensure_tool(id)),
            None => self.single_open_tool_index(),
        }
    }

    fn ensure_tool(&mut self, id: &str) -> usize {
        self.tool_calls
            .iter()
            .position(|call| call.id == id)
            .unwrap_or_else(|| self.push_tool(id))
    }

    fn push_tool(&mut self, id: &str) -> usize {
        self.tool_calls.push(PendingToolCall::new(id));
        self.tool_calls.len() - 1
    }

    fn single_open_tool_index(&self) -> Option<usize> {
        let mut indexes = self
            .tool_calls
            .iter()
            .enumerate()
            .filter(|(_, call)| !call.completed)
            .map(|(index, _)| index);
        let first = indexes.next()?;
        indexes.next().is_none().then_some(first)
    }

    fn apply_tool_delta(&mut self, index: usize, delta: ToolUseDelta) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        let call = &mut self.tool_calls[index];

        maybe_update_name(call, delta.name.clone());
        maybe_push_start_event(call, &mut events);
        maybe_push_argument_delta(call, delta.arguments_delta, &mut events);
        maybe_mark_complete(call, delta.arguments_done, &mut events);

        events
    }

    fn merge_usage(&mut self, usage: Option<Usage>) {
        if usage.is_some() {
            self.usage = usage;
        }
    }

    fn merge_stop_reason(&mut self, stop_reason: Option<String>) {
        if stop_reason.is_some() {
            self.stop_reason = stop_reason;
        }
    }

    fn finish(mut self, callback: &StreamCallback) -> CompletionResponse {
        emit_events(callback, finish_tool_calls(&mut self.tool_calls));
        let response = build_completion_response(self);
        let done = completion_text(&response);
        emit_event(callback, StreamEvent::Done { response: done });
        response
    }
}

#[derive(Debug, Clone)]
struct PendingToolCall {
    id: String,
    name: Option<String>,
    arguments: String,
    started: bool,
    completed: bool,
}

impl PendingToolCall {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: None,
            arguments: String::new(),
            started: false,
            completed: false,
        }
    }
}

fn maybe_update_name(call: &mut PendingToolCall, name: Option<String>) {
    if let Some(name) = name {
        call.name = Some(name);
    }
}

fn maybe_push_start_event(call: &mut PendingToolCall, events: &mut Vec<StreamEvent>) {
    let Some(name) = call.name.clone() else {
        return;
    };
    if call.started {
        return;
    }

    call.started = true;
    events.push(StreamEvent::ToolCallStart {
        id: call.id.clone(),
        name,
    });
}

fn maybe_push_argument_delta(
    call: &mut PendingToolCall,
    arguments_delta: Option<String>,
    events: &mut Vec<StreamEvent>,
) {
    let Some(arguments_delta) = arguments_delta else {
        return;
    };
    call.arguments.push_str(&arguments_delta);
    events.push(StreamEvent::ToolCallDelta {
        id: call.id.clone(),
        args_delta: arguments_delta,
    });
}

fn maybe_mark_complete(
    call: &mut PendingToolCall,
    arguments_done: bool,
    events: &mut Vec<StreamEvent>,
) {
    if !arguments_done || call.completed {
        return;
    }

    events.push(complete_tool_call(call));
}

fn finish_tool_calls(tool_calls: &mut [PendingToolCall]) -> Vec<StreamEvent> {
    tool_calls
        .iter_mut()
        .filter(|call| !call.completed)
        .map(complete_tool_call)
        .collect()
}

fn complete_tool_call(call: &mut PendingToolCall) -> StreamEvent {
    call.completed = true;
    StreamEvent::ToolCallComplete {
        id: call.id.clone(),
        name: call.name.clone().unwrap_or_default(),
        arguments: call.arguments.clone(),
    }
}

fn build_completion_response(collector: StreamCollector) -> CompletionResponse {
    let tool_calls = build_tool_calls(&collector.tool_calls);
    let content = build_content_blocks(&collector.text, &tool_calls);

    CompletionResponse {
        content,
        tool_calls,
        usage: collector.usage,
        stop_reason: collector.stop_reason,
    }
}

fn build_tool_calls(tool_calls: &[PendingToolCall]) -> Vec<ToolCall> {
    tool_calls
        .iter()
        .map(|call| ToolCall {
            id: call.id.clone(),
            name: call.name.clone().unwrap_or_default(),
            arguments: parse_tool_arguments(&call.arguments),
        })
        .collect()
}

fn build_content_blocks(text: &str, tool_calls: &[ToolCall]) -> Vec<ContentBlock> {
    let mut content = Vec::new();

    if !text.is_empty() {
        content.push(ContentBlock::Text {
            text: text.to_string(),
        });
    }

    content.extend(tool_calls.iter().map(tool_call_content_block));
    content
}

fn tool_call_content_block(tool_call: &ToolCall) -> ContentBlock {
    ContentBlock::ToolUse {
        id: tool_call.id.clone(),
        provider_id: None,
        name: tool_call.name.clone(),
        input: tool_call.arguments.clone(),
    }
}

fn parse_tool_arguments(arguments: &str) -> Value {
    crate::parse_tool_arguments_object(arguments)
}

fn emit_events(callback: &StreamCallback, events: Vec<StreamEvent>) {
    for event in events {
        emit_event(callback, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    use crate::provider::{CompletionStream, LlmProvider, ProviderCapabilities};
    use crate::test_helpers::{callback_events, read_events};
    use crate::types::{CompletionRequest, Message};

    #[derive(Clone)]
    struct TestProvider {
        response: CompletionResponse,
    }

    #[async_trait]
    impl LlmProvider for TestProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Ok(self.response.clone())
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, LlmError> {
            unreachable!("default stream() should not call complete_stream()");
        }

        fn name(&self) -> &str {
            "test"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["test-model".to_string()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: true,
                requires_streaming: false,
            }
        }
    }

    fn text_request() -> CompletionRequest {
        CompletionRequest {
            model: "test-model".to_string(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(64),
            system_prompt: None,
            thinking: None,
        }
    }

    #[tokio::test]
    async fn default_stream_emits_single_text_delta_and_done() {
        let provider = TestProvider {
            response: CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "hello world".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: Some("stop".to_string()),
            },
        };
        let (callback, events) = callback_events();

        let response = provider
            .stream(text_request(), callback)
            .await
            .expect("stream result");

        assert_eq!(completion_text(&response), "hello world");
        assert_eq!(
            read_events(events),
            vec![
                StreamEvent::TextDelta {
                    text: "hello world".to_string()
                },
                StreamEvent::Done {
                    response: "hello world".to_string()
                }
            ]
        );
    }

    #[tokio::test]
    async fn default_stream_skips_empty_text_delta_for_tool_only_response() {
        let provider = TestProvider {
            response: CompletionResponse {
                content: vec![ContentBlock::ToolUse {
                    id: "tool_1".to_string(),
                    provider_id: None,
                    name: "lookup".to_string(),
                    input: serde_json::json!({"q": "fawx"}),
                }],
                tool_calls: vec![ToolCall {
                    id: "tool_1".to_string(),
                    name: "lookup".to_string(),
                    arguments: serde_json::json!({"q": "fawx"}),
                }],
                usage: None,
                stop_reason: Some("tool_calls".to_string()),
            },
        };
        let (callback, events) = callback_events();

        let response = provider
            .stream(text_request(), callback)
            .await
            .expect("stream result");

        assert!(completion_text(&response).is_empty());
        assert_eq!(
            read_events(events),
            vec![StreamEvent::Done {
                response: String::new()
            }]
        );
    }

    #[tokio::test]
    async fn callback_panics_are_caught_in_default_stream() {
        let provider = TestProvider {
            response: CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "still returns".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
        };
        let callback: StreamCallback = Arc::new(|_| {
            panic!("boom");
        });

        let response = provider
            .stream(text_request(), callback)
            .await
            .expect("stream result");

        assert_eq!(completion_text(&response), "still returns");
    }

    #[test]
    fn stream_chunk_collection_emits_tool_lifecycle_and_builds_response() {
        let chunks = vec![
            StreamChunk {
                delta_content: Some("Hello".to_string()),
                ..Default::default()
            },
            StreamChunk {
                tool_use_deltas: vec![ToolUseDelta {
                    id: Some("tool_1".to_string()),
                    provider_id: None,
                    name: Some("lookup".to_string()),
                    arguments_delta: None,
                    arguments_done: false,
                }],
                ..Default::default()
            },
            StreamChunk {
                tool_use_deltas: vec![ToolUseDelta {
                    id: Some("tool_1".to_string()),
                    provider_id: None,
                    name: None,
                    arguments_delta: Some("{\"q\":\"fawx\"}".to_string()),
                    arguments_done: true,
                }],
                stop_reason: Some("tool_calls".to_string()),
                ..Default::default()
            },
        ];
        let (callback, events) = callback_events();

        let response = collect_stream_chunks(chunks, &callback);

        assert_eq!(completion_text(&response), "Hello");
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
                    id: "tool_1".to_string(),
                    name: "lookup".to_string()
                },
                StreamEvent::ToolCallDelta {
                    id: "tool_1".to_string(),
                    args_delta: "{\"q\":\"fawx\"}".to_string()
                },
                StreamEvent::ToolCallComplete {
                    id: "tool_1".to_string(),
                    name: "lookup".to_string(),
                    arguments: "{\"q\":\"fawx\"}".to_string()
                },
                StreamEvent::Done {
                    response: "Hello".to_string()
                }
            ]
        );
    }
}

#[cfg(test)]
mod parse_tool_arguments_tests {
    use super::parse_tool_arguments;
    use serde_json::Value;

    #[test]
    fn valid_json_object_parses_normally() {
        let result = parse_tool_arguments(r#"{"path": "/tmp/test.md"}"#);
        assert!(result.is_object());
        assert_eq!(result["path"], "/tmp/test.md");
    }

    #[test]
    fn malformed_json_wraps_as_raw_object_not_string() {
        let result = parse_tool_arguments(r#"{"path": "/tmp/test.md"#);
        assert!(
            result.is_object(),
            "fallback must be an object, got: {result:?}"
        );
        assert!(
            !matches!(result, Value::String(_)),
            "must not be Value::String"
        );
        assert!(
            result.get("__fawx_raw_args").is_some(),
            "must contain __fawx_raw_args key"
        );
        assert_eq!(result["__fawx_raw_args"], r#"{"path": "/tmp/test.md"#);
    }

    #[test]
    fn empty_string_normalizes_to_empty_object() {
        let result = parse_tool_arguments("");
        assert!(result.is_object());
        assert_eq!(result, serde_json::json!({}));
    }

    #[test]
    fn whitespace_only_normalizes_to_empty_object() {
        let result = parse_tool_arguments("   ");
        assert!(result.is_object());
        assert_eq!(result, serde_json::json!({}));
    }
}
