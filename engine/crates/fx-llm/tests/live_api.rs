//! Live API contract tests for the Anthropic provider.
//!
//! These tests hit the real Anthropic API and verify end-to-end behavior
//! through the public `complete()` and `complete_stream()` methods.
//!
//! Run with: `cargo test --test live_api -- --ignored --test-threads=1`
//! Requires: `ANTHROPIC_API_KEY` environment variable.

use futures::StreamExt;
use fx_llm::{
    AnthropicProvider, CompletionProvider, CompletionRequest, Message, StreamChunk, ThinkingConfig,
    ToolDefinition,
};
use serde_json::json;

const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";
const SONNET_MODEL: &str = "claude-sonnet-4-6";

/// Skip the test (via early return) when `ANTHROPIC_API_KEY` is not set.
macro_rules! skip_without_api_key {
    () => {
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            eprintln!("ANTHROPIC_API_KEY not set — skipping live test");
            return;
        }
    };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_provider() -> AnthropicProvider {
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap();
    AnthropicProvider::new(ANTHROPIC_BASE_URL, api_key).unwrap()
}

fn text_request(model: &str, prompt: &str) -> CompletionRequest {
    CompletionRequest {
        model: model.to_string(),
        messages: vec![Message::user(prompt)],
        tools: Vec::new(),
        temperature: Some(0.0),
        max_tokens: Some(256),
        system_prompt: None,
        thinking: None,
    }
}

fn tool_request(model: &str, prompt: &str) -> CompletionRequest {
    CompletionRequest {
        model: model.to_string(),
        messages: vec![Message::user(prompt)],
        tools: vec![read_file_tool()],
        temperature: Some(0.0),
        max_tokens: Some(256),
        system_prompt: None,
        thinking: None,
    }
}

fn thinking_text_request(prompt: &str) -> CompletionRequest {
    CompletionRequest {
        model: SONNET_MODEL.to_string(),
        messages: vec![Message::user(prompt)],
        tools: Vec::new(),
        temperature: None, // required when thinking is enabled
        max_tokens: Some(4096),
        system_prompt: None,
        thinking: Some(ThinkingConfig::Enabled {
            budget_tokens: 2000,
        }),
    }
}

fn thinking_tool_request(prompt: &str) -> CompletionRequest {
    CompletionRequest {
        model: SONNET_MODEL.to_string(),
        messages: vec![Message::user(prompt)],
        tools: vec![read_file_tool()],
        temperature: None,
        max_tokens: Some(4096),
        system_prompt: None,
        thinking: Some(ThinkingConfig::Enabled {
            budget_tokens: 2000,
        }),
    }
}

fn read_file_tool() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file from disk".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to read" }
            },
            "required": ["path"]
        }),
    }
}

/// Result of collecting a `CompletionStream` into its constituent parts.
struct StreamResult {
    chunks: Vec<StreamChunk>,
    text: String,
    stop_reason: Option<String>,
}

/// Collect a `CompletionStream` into text, tool deltas, and the final stop reason.
async fn collect_stream(provider: &AnthropicProvider, request: CompletionRequest) -> StreamResult {
    let mut stream = provider.complete_stream(request).await.unwrap();
    let mut chunks = Vec::new();
    let mut text = String::new();
    let mut stop_reason = None;

    while let Some(result) = stream.next().await {
        let chunk = result.unwrap();
        if let Some(ref delta) = chunk.delta_content {
            text.push_str(delta);
        }
        if let Some(ref reason) = chunk.stop_reason {
            stop_reason = Some(reason.clone());
        }
        chunks.push(chunk);
    }

    StreamResult {
        chunks,
        text,
        stop_reason,
    }
}

// ---------------------------------------------------------------------------
// Haiku tests (non-thinking)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "live API test — see #1229"]
async fn live_text_response() {
    skip_without_api_key!();

    let provider = make_provider();
    let request = text_request(HAIKU_MODEL, "Reply with exactly: hello world");
    let response = provider.complete(request).await.unwrap();

    // Non-empty text
    let text: String = response
        .content
        .iter()
        .filter_map(|b| match b {
            fx_llm::ContentBlock::Text { text } => Some(text.as_str()),
            fx_llm::ContentBlock::Image { .. } => None,
            _ => None,
        })
        .collect();
    assert!(!text.is_empty(), "response text must be non-empty");

    // Stop reason
    assert_eq!(
        response.stop_reason.as_deref(),
        Some("end_turn"),
        "stop_reason must be end_turn"
    );

    // Usage
    let usage = response.usage.expect("usage must be present");
    assert!(usage.input_tokens > 0, "input_tokens must be > 0");
    assert!(usage.output_tokens > 0, "output_tokens must be > 0");
}

#[tokio::test]
#[ignore = "live API test — see #1229"]
async fn live_tool_call() {
    skip_without_api_key!();

    let provider = make_provider();
    let request = tool_request(HAIKU_MODEL, "Read the file at path src/main.rs");
    let response = provider.complete(request).await.unwrap();

    // Tool calls populated
    assert!(
        !response.tool_calls.is_empty(),
        "tool_calls must be non-empty"
    );
    assert_eq!(
        response.tool_calls[0].name, "read_file",
        "tool name must be read_file"
    );
    assert!(
        response.tool_calls[0].arguments.get("path").is_some(),
        "tool arguments must contain 'path' key"
    );

    // Stop reason
    assert_eq!(
        response.stop_reason.as_deref(),
        Some("tool_use"),
        "stop_reason must be tool_use"
    );
}

#[tokio::test]
#[ignore = "live API test — see #1229"]
async fn live_streaming_text() {
    skip_without_api_key!();

    let provider = make_provider();
    let request = text_request(HAIKU_MODEL, "Reply with exactly: hello world");
    let result = collect_stream(&provider, request).await;

    assert!(!result.text.is_empty(), "assembled text must be non-empty");
    assert!(
        result.chunks.len() >= 2,
        "must receive at least 2 chunks to prove streaming, got {}",
        result.chunks.len()
    );
    assert_eq!(
        result.stop_reason.as_deref(),
        Some("end_turn"),
        "final stop_reason must be end_turn"
    );
}

#[tokio::test]
#[ignore = "live API test — see #1229"]
async fn live_streaming_tool_call() {
    skip_without_api_key!();

    let provider = make_provider();
    let request = tool_request(HAIKU_MODEL, "Read the file at path src/main.rs");
    let result = collect_stream(&provider, request).await;

    // Find tool-use deltas
    let tool_start = result
        .chunks
        .iter()
        .flat_map(|c| &c.tool_use_deltas)
        .find(|d| d.name.is_some());
    assert!(
        tool_start.is_some(),
        "must receive a tool_use start delta with name"
    );
    assert_eq!(
        tool_start.unwrap().name.as_deref(),
        Some("read_file"),
        "tool name must be read_file"
    );

    // Assemble arguments from deltas
    let args_json: String = result
        .chunks
        .iter()
        .flat_map(|c| &c.tool_use_deltas)
        .filter_map(|d| d.arguments_delta.as_deref())
        .collect();
    assert!(!args_json.is_empty(), "tool arguments must be non-empty");
    let parsed: serde_json::Value =
        serde_json::from_str(&args_json).expect("assembled arguments must be valid JSON");
    assert!(
        parsed.get("path").is_some(),
        "arguments must contain 'path' key"
    );

    assert_eq!(
        result.stop_reason.as_deref(),
        Some("tool_use"),
        "stop_reason must be tool_use"
    );
}

// ---------------------------------------------------------------------------
// Sonnet thinking tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "live API test — see #1229"]
async fn live_thinking_text() {
    skip_without_api_key!();

    let provider = make_provider();
    let request = thinking_text_request("What is 7 * 8? Think step by step.");
    let response = provider.complete(request).await.unwrap();

    let text: String = response
        .content
        .iter()
        .filter_map(|b| match b {
            fx_llm::ContentBlock::Text { text } => Some(text.as_str()),
            fx_llm::ContentBlock::Image { .. } => None,
            _ => None,
        })
        .collect();

    assert!(!text.is_empty(), "response text must be non-empty");
    assert!(text.contains("56"), "response must contain '56'");
    assert_eq!(
        response.stop_reason.as_deref(),
        Some("end_turn"),
        "stop_reason must be end_turn"
    );

    // Thinking blocks must not leak into content
    // (AnthropicProvider skips Thinking/RedactedThinking blocks)
    for block in &response.content {
        assert!(
            matches!(block, fx_llm::ContentBlock::Text { .. }),
            "only text blocks expected in response content, got: {block:?}"
        );
    }
}

#[tokio::test]
#[ignore = "live API test — see #1229"]
async fn live_thinking_tool_call() {
    skip_without_api_key!();

    let provider = make_provider();
    let request = thinking_tool_request("Think about what file to read, then read src/main.rs");
    let response = provider.complete(request).await.unwrap();

    assert!(
        !response.tool_calls.is_empty(),
        "tool_calls must be non-empty"
    );
    assert_eq!(
        response.tool_calls[0].name, "read_file",
        "tool name must be read_file"
    );
    assert_eq!(
        response.stop_reason.as_deref(),
        Some("tool_use"),
        "stop_reason must be tool_use"
    );
}

#[tokio::test]
#[ignore = "live API test — see #1229"]
async fn live_streaming_thinking_text() {
    skip_without_api_key!();

    let provider = make_provider();
    let request = thinking_text_request("What is 7 * 8? Think step by step.");
    let result = collect_stream(&provider, request).await;

    assert!(!result.text.is_empty(), "assembled text must be non-empty");
    assert!(result.text.contains("56"), "response must contain '56'");

    // Verify thinking blocks don't leak into assembled text output
    let text_lower = result.text.to_lowercase();
    assert!(
        !text_lower.contains("<thinking>"),
        "thinking tags should not appear in text"
    );
    assert!(
        !text_lower.contains("</thinking>"),
        "thinking tags should not appear in text"
    );

    assert!(
        result.chunks.len() >= 2,
        "must receive multiple chunks, got {}",
        result.chunks.len()
    );
    assert_eq!(
        result.stop_reason.as_deref(),
        Some("end_turn"),
        "final stop_reason must be end_turn"
    );
}

#[tokio::test]
#[ignore = "live API test — see #1229"]
async fn live_streaming_thinking_tool_call() {
    skip_without_api_key!();

    let provider = make_provider();
    let request = thinking_tool_request("Think about what file to read, then read src/main.rs");
    let result = collect_stream(&provider, request).await;

    // Tool call assembled from deltas
    let tool_start = result
        .chunks
        .iter()
        .flat_map(|c| &c.tool_use_deltas)
        .find(|d| d.name.is_some());
    assert!(
        tool_start.is_some(),
        "must receive a tool_use start delta with name"
    );
    assert_eq!(
        tool_start.unwrap().name.as_deref(),
        Some("read_file"),
        "tool name must be read_file"
    );

    // Assemble and validate tool arguments from deltas
    let args_json: String = result
        .chunks
        .iter()
        .flat_map(|c| &c.tool_use_deltas)
        .filter_map(|d| d.arguments_delta.as_deref())
        .collect();
    assert!(!args_json.is_empty(), "tool arguments must be non-empty");
    let parsed: serde_json::Value =
        serde_json::from_str(&args_json).expect("assembled arguments must be valid JSON");
    assert!(
        parsed.get("path").is_some(),
        "arguments must contain 'path' key"
    );

    assert_eq!(
        result.stop_reason.as_deref(),
        Some("tool_use"),
        "stop_reason must be tool_use"
    );
}
