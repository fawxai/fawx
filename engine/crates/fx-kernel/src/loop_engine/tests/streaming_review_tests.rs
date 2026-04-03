use super::*;
use async_trait::async_trait;
use fx_llm::{CompletionResponse, CompletionStream, ContentBlock, ProviderError, StreamChunk};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
struct NoopToolExecutor;

#[async_trait]
impl ToolExecutor for NoopToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }
}

fn engine_with_bus(bus: &fx_core::EventBus) -> LoopEngine {
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            0,
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(NoopToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");
    engine.set_event_bus(bus.clone());
    engine
}

fn base_engine() -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(
            crate::budget::BudgetConfig::default(),
            0,
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(NoopToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

// -- Finding NB1: stream_tool_call_from_state drops malformed JSON --

#[test]
fn stream_tool_call_from_state_drops_malformed_json_arguments() {
    let state = StreamToolCallState {
        id: Some("call-1".to_string()),
        provider_id: None,
        name: Some("read_file".to_string()),
        arguments: "not valid json {{{".to_string(),
        arguments_done: true,
    };
    let result = stream_tool_call_from_state(state);
    assert!(
        result.is_none(),
        "malformed JSON arguments should cause the tool call to be dropped"
    );
}

#[test]
fn stream_tool_call_from_state_accepts_valid_json_arguments() {
    let state = StreamToolCallState {
        id: Some("call-1".to_string()),
        provider_id: Some("fc-1".to_string()),
        name: Some("read_file".to_string()),
        arguments: r#"{"path":"README.md"}"#.to_string(),
        arguments_done: true,
    };
    let result = stream_tool_call_from_state(state);
    assert!(result.is_some(), "valid JSON arguments should be accepted");
    let call = result.expect("tool call");
    assert_eq!(call.id, "call-1");
    assert_eq!(call.name, "read_file");
    assert_eq!(call.arguments, serde_json::json!({"path": "README.md"}));
}

// -- Regression tests for #1118: empty args for zero-param tools --

#[test]
fn stream_tool_call_from_state_normalizes_empty_arguments_to_empty_object() {
    let state = StreamToolCallState {
        id: Some("call-1".to_string()),
        provider_id: None,
        name: Some("git_status".to_string()),
        arguments: String::new(),
        arguments_done: true,
    };
    let result = stream_tool_call_from_state(state);
    assert!(
        result.is_some(),
        "empty arguments should be normalized to {{}}, not dropped"
    );
    let call = result.expect("tool call");
    assert_eq!(call.id, "call-1");
    assert_eq!(call.name, "git_status");
    assert_eq!(call.arguments, serde_json::json!({}));
}

#[test]
fn stream_tool_call_from_state_normalizes_whitespace_arguments_to_empty_object() {
    let state = StreamToolCallState {
        id: Some("call-1".to_string()),
        provider_id: None,
        name: Some("current_time".to_string()),
        arguments: "   \n\t  ".to_string(),
        arguments_done: true,
    };
    let result = stream_tool_call_from_state(state);
    assert!(
        result.is_some(),
        "whitespace-only arguments should be normalized to {{}}, not dropped"
    );
    let call = result.expect("tool call");
    assert_eq!(call.arguments, serde_json::json!({}));
}

#[test]
fn finalize_stream_tool_calls_preserves_zero_param_tool_calls() {
    let mut by_index = HashMap::new();
    by_index.insert(
        0,
        StreamToolCallState {
            id: Some("call-zero".to_string()),
            provider_id: None,
            name: Some("memory_list".to_string()),
            arguments: String::new(),
            arguments_done: true,
        },
    );
    by_index.insert(
        1,
        StreamToolCallState {
            id: Some("call-with-args".to_string()),
            provider_id: None,
            name: Some("read_file".to_string()),
            arguments: r#"{"path":"test.rs"}"#.to_string(),
            arguments_done: true,
        },
    );
    let calls = finalize_stream_tool_calls(by_index);
    assert_eq!(
        calls.len(),
        2,
        "both zero-param and parameterized tool calls should be preserved"
    );
    assert_eq!(calls[0].name, "memory_list");
    assert_eq!(calls[0].arguments, serde_json::json!({}));
    assert_eq!(calls[1].name, "read_file");
    assert_eq!(calls[1].arguments, serde_json::json!({"path": "test.rs"}));
}

#[test]
fn finalize_stream_tool_calls_filters_out_malformed_arguments() {
    let mut by_index = HashMap::new();
    by_index.insert(
        0,
        StreamToolCallState {
            id: Some("call-good".to_string()),
            provider_id: None,
            name: Some("read_file".to_string()),
            arguments: r#"{"path":"a.txt"}"#.to_string(),
            arguments_done: true,
        },
    );
    by_index.insert(
        1,
        StreamToolCallState {
            id: Some("call-bad".to_string()),
            provider_id: None,
            name: Some("write_file".to_string()),
            arguments: "truncated json {".to_string(),
            arguments_done: true,
        },
    );
    let calls = finalize_stream_tool_calls(by_index);
    assert_eq!(calls.len(), 1, "only the valid tool call should survive");
    assert_eq!(calls[0].id, "call-good");
}

// -- Finding NB2: StreamingFinished exactly once for all paths --

fn count_streaming_finished(
    receiver: &mut tokio::sync::broadcast::Receiver<fx_core::message::InternalMessage>,
) -> usize {
    let mut count = 0;
    while let Ok(msg) = receiver.try_recv() {
        if matches!(msg, InternalMessage::StreamingFinished { .. }) {
            count += 1;
        }
    }
    count
}

#[tokio::test]
async fn consume_stream_publishes_exactly_one_finished_on_success() {
    let bus = fx_core::EventBus::new(16);
    let mut receiver = bus.subscribe();
    let mut engine = engine_with_bus(&bus);

    let mut stream: CompletionStream =
        Box::pin(futures_util::stream::iter(vec![Ok(StreamChunk {
            delta_content: Some("hello".to_string()),
            tool_use_deltas: Vec::new(),
            usage: None,
            stop_reason: Some("stop".to_string()),
        })]));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Reason,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");

    assert_eq!(extract_response_text(&response), "hello");
    assert_eq!(
        count_streaming_finished(&mut receiver),
        1,
        "exactly one StreamingFinished on success path"
    );
}

#[tokio::test]
async fn consume_stream_publishes_exactly_one_finished_on_cancel() {
    let bus = fx_core::EventBus::new(16);
    let mut receiver = bus.subscribe();
    let mut engine = engine_with_bus(&bus);
    let token = CancellationToken::new();
    engine.set_cancel_token(token.clone());

    let cancel_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        token.cancel();
    });

    let delayed = futures_util::stream::iter(vec![
        StreamChunk {
            delta_content: Some("first".to_string()),
            tool_use_deltas: Vec::new(),
            usage: None,
            stop_reason: None,
        },
        StreamChunk {
            delta_content: Some("second".to_string()),
            tool_use_deltas: Vec::new(),
            usage: None,
            stop_reason: Some("stop".to_string()),
        },
    ])
    .enumerate()
    .then(|(index, chunk)| async move {
        if index == 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<StreamChunk, ProviderError>(chunk)
    });
    let mut stream: CompletionStream = Box::pin(delayed);

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Reason,
            TextStreamVisibility::Public,
        )
        .await
        .expect("stream consumed");
    cancel_task.await.expect("cancel task");

    assert_eq!(response.stop_reason.as_deref(), Some("cancelled"));
    assert_eq!(
        count_streaming_finished(&mut receiver),
        1,
        "exactly one StreamingFinished on cancel path"
    );
}

#[tokio::test]
async fn consume_stream_publishes_exactly_one_finished_on_error() {
    let bus = fx_core::EventBus::new(16);
    let mut receiver = bus.subscribe();
    let mut engine = engine_with_bus(&bus);

    let chunks = vec![
        Ok(StreamChunk {
            delta_content: Some("partial".to_string()),
            tool_use_deltas: Vec::new(),
            usage: None,
            stop_reason: None,
        }),
        Err(ProviderError::Streaming(
            "simulated stream failure".to_string(),
        )),
    ];
    let mut stream: CompletionStream = Box::pin(futures_util::stream::iter(chunks));

    let error = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Reason,
            TextStreamVisibility::Public,
        )
        .await
        .expect_err("stream should fail");
    assert!(error.reason.contains("stream consumption failed"));

    assert_eq!(
        count_streaming_finished(&mut receiver),
        1,
        "exactly one StreamingFinished on error path"
    );
}

// -- Nice-to-have 1: response_to_chunk multi-text-block test --

#[test]
fn response_to_chunk_joins_multiple_text_blocks_with_newline() {
    let response = CompletionResponse {
        content: vec![
            ContentBlock::Text {
                text: "first paragraph".to_string(),
            },
            ContentBlock::Text {
                text: "second paragraph".to_string(),
            },
            ContentBlock::Text {
                text: "third paragraph".to_string(),
            },
        ],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    };

    let chunk = response_to_chunk(response);
    assert_eq!(
        chunk.delta_content.as_deref(),
        Some("first paragraph\nsecond paragraph\nthird paragraph"),
        "multiple text blocks should be joined with newlines"
    );
}

#[test]
fn response_to_chunk_skips_non_text_blocks_in_join() {
    let response = CompletionResponse {
        content: vec![
            ContentBlock::Text {
                text: "before".to_string(),
            },
            ContentBlock::ToolUse {
                id: "t1".to_string(),
                provider_id: None,
                name: "read_file".to_string(),
                input: serde_json::json!({}),
            },
            ContentBlock::Text {
                text: "after".to_string(),
            },
        ],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    };

    let chunk = response_to_chunk(response);
    assert_eq!(
        chunk.delta_content.as_deref(),
        Some("before\nafter"),
        "non-text blocks should be skipped in the join"
    );
}

#[test]
fn response_to_chunk_preserves_tool_provider_ids() {
    let response = CompletionResponse {
        content: vec![ContentBlock::ToolUse {
            id: "call-1".to_string(),
            provider_id: Some("fc-1".to_string()),
            name: "read_file".to_string(),
            input: serde_json::json!({"path":"README.md"}),
        }],
        tool_calls: vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }],
        usage: None,
        stop_reason: Some("tool_use".to_string()),
    };

    let chunk = response_to_chunk(response);
    assert!(matches!(
        chunk.tool_use_deltas.as_slice(),
        [ToolUseDelta {
            id: Some(id),
            provider_id: Some(provider_id),
            name: Some(name),
            arguments_delta: Some(arguments),
            arguments_done: true,
        }] if id == "call-1"
            && provider_id == "fc-1"
            && name == "read_file"
            && arguments == r#"{"path":"README.md"}"#
    ));
}

// -- Nice-to-have 2: empty stream edge case test --

#[tokio::test]
async fn consume_stream_with_zero_chunks_produces_empty_response() {
    let mut engine = base_engine();

    let mut stream: CompletionStream = Box::pin(futures_util::stream::iter(Vec::<
        Result<StreamChunk, ProviderError>,
    >::new()));

    let response = engine
        .consume_stream_with_events(
            &mut stream,
            StreamPhase::Reason,
            TextStreamVisibility::Public,
        )
        .await
        .expect("empty stream consumed");

    assert_eq!(
        extract_response_text(&response),
        "",
        "zero chunks should produce empty text"
    );
    assert!(
        response.tool_calls.is_empty(),
        "zero chunks should produce no tool calls"
    );
    assert!(
        response.usage.is_none(),
        "zero chunks should produce no usage"
    );
    assert!(
        response.stop_reason.is_none(),
        "zero chunks should produce no stop reason"
    );
}

#[test]
fn default_stream_response_state_produces_empty_response() {
    let state = StreamResponseState::default();
    let response = state.into_response();

    assert_eq!(
        extract_response_text(&response),
        "",
        "default state should produce empty text"
    );
    assert!(
        response.tool_calls.is_empty(),
        "default state should produce no tool calls"
    );
    assert!(
        response.usage.is_none(),
        "default state should produce no usage"
    );
}

#[test]
fn finalize_stream_tool_calls_separates_multi_tool_arguments() {
    let mut state = StreamResponseState::default();

    // Tool 1: content_block_start with id
    state.apply_chunk(StreamChunk {
        tool_use_deltas: vec![ToolUseDelta {
            id: Some("toolu_01".to_string()),
            provider_id: None,
            name: Some("read_file".to_string()),
            arguments_delta: None,
            arguments_done: false,
        }],
        ..Default::default()
    });

    // Tool 1: argument delta (id present from provider fix)
    state.apply_chunk(StreamChunk {
        tool_use_deltas: vec![ToolUseDelta {
            id: Some("toolu_01".to_string()),
            provider_id: None,
            name: None,
            arguments_delta: Some(r#"{"path":"/tmp/a.txt"}"#.to_string()),
            arguments_done: false,
        }],
        ..Default::default()
    });

    // Tool 1: done
    state.apply_chunk(StreamChunk {
        tool_use_deltas: vec![ToolUseDelta {
            id: Some("toolu_01".to_string()),
            provider_id: None,
            name: None,
            arguments_delta: None,
            arguments_done: true,
        }],
        ..Default::default()
    });

    // Tool 2: content_block_start with id
    state.apply_chunk(StreamChunk {
        tool_use_deltas: vec![ToolUseDelta {
            id: Some("toolu_02".to_string()),
            provider_id: None,
            name: Some("read_file".to_string()),
            arguments_delta: None,
            arguments_done: false,
        }],
        ..Default::default()
    });

    // Tool 2: argument delta with id (injected by provider)
    state.apply_chunk(StreamChunk {
        tool_use_deltas: vec![ToolUseDelta {
            id: Some("toolu_02".to_string()),
            provider_id: None,
            name: None,
            arguments_delta: Some(r#"{"path":"/tmp/b.txt"}"#.to_string()),
            arguments_done: false,
        }],
        ..Default::default()
    });

    // Tool 2: done
    state.apply_chunk(StreamChunk {
        tool_use_deltas: vec![ToolUseDelta {
            id: Some("toolu_02".to_string()),
            provider_id: None,
            name: None,
            arguments_delta: None,
            arguments_done: true,
        }],
        ..Default::default()
    });

    let response = state.into_response();
    assert_eq!(
        response.tool_calls.len(),
        2,
        "expected 2 separate tool calls, got {}",
        response.tool_calls.len()
    );
    assert_eq!(response.tool_calls[0].id, "toolu_01");
    assert_eq!(
        response.tool_calls[0].arguments,
        serde_json::json!({"path": "/tmp/a.txt"})
    );
    assert_eq!(response.tool_calls[1].id, "toolu_02");
    assert_eq!(
        response.tool_calls[1].arguments,
        serde_json::json!({"path": "/tmp/b.txt"})
    );
}
