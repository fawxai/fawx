# Live API Contract Tests — Specification

**Status:** Draft
**Wave:** 8, Item 3
**Issue:** #1229
**Depends on:** PR #1231 (L1 request validation), PR #1232 (L2 response fixtures)
**Scope:** fx-llm crate + CI workflow

---

## Problem

Layers 1 and 2 validate requests and responses in isolation using static fixtures. They cannot catch:
- API version changes that alter response shapes in ways our fixtures don't cover
- Authentication/header issues
- Streaming connection lifecycle bugs (SSE framing, chunked transfer)
- Model-specific behavior differences (thinking availability, tool call formatting)

The TUI smoke test catches these — but it's manual, unstructured, and depends on Joe being available. Live contract tests formalize the smoke test as repeatable, automated CI.

---

## Design

### Location

Separate integration test file: `engine/crates/fx-llm/tests/live_api.rs`

**Why not `anthropic.rs` test module:**
- L2 already covers internal type deserialization exhaustively
- L3's job is end-to-end round-trip through the **public API** (`complete()`, `complete_stream()`)
- Clean isolation — impossible to accidentally run during normal `cargo test`
- Natural home for future provider live tests (`live_openai.rs`, etc.)

### Auth

- API key from `ANTHROPIC_API_KEY` environment variable
- Tests **skip** (not fail) if key is missing — `return` early with printed message
- In CI: GitHub Actions secret

### Models

- **Haiku** (`claude-haiku-3-5-20241022`) for non-thinking tests — cheapest, fastest, same API contract
- **Sonnet** (`claude-sonnet-4-20250514`) for thinking tests — thinking not available on Haiku

### Cost

- ~$0.15-0.40 per full suite run (8 tests, short prompts)
- Sequential execution (`--test-threads=1`) to avoid rate limits

---

## Test Cases (8)

### Non-thinking tests (Haiku)

#### 1. `live_text_response`
```
Prompt: "Reply with exactly: hello world"
Verify:
  - response.text is non-empty
  - response.stop_reason == "end_turn"
  - response.usage.input_tokens > 0
  - response.usage.output_tokens > 0
```

#### 2. `live_tool_call`
```
Prompt: "Read the file at path src/main.rs"
Tools: [read_file(path: String)]
Verify:
  - response.tool_calls is non-empty
  - tool_calls[0].name == "read_file"
  - tool_calls[0].arguments contains "path" key
  - response.stop_reason == "tool_use"
```

#### 3. `live_streaming_text`
```
Prompt: "Reply with exactly: hello world"
Via: complete_stream()
Verify:
  - collected text is non-empty
  - received at least 2 chunks (proves streaming, not single response)
  - final chunk has stop_reason == "end_turn"
```

#### 4. `live_streaming_tool_call`
```
Prompt: "Read the file at path src/main.rs"
Tools: [read_file(path: String)]
Via: complete_stream()
Verify:
  - tool call assembled from deltas has name == "read_file"
  - arguments JSON is valid and contains "path"
  - final chunk has stop_reason == "tool_use"
```

### Thinking tests (Sonnet)

#### 5. `live_thinking_text`
```
Prompt: "What is 7 * 8? Think step by step."
Thinking: enabled, budget_tokens: 2000
Verify:
  - response.text is non-empty and contains "56"
  - response.stop_reason == "end_turn"
  - No thinking blocks leak into response.text
```

#### 6. `live_thinking_tool_call`
```
Prompt: "Think about what file to read, then read src/main.rs"
Tools: [read_file(path: String)]
Thinking: enabled, budget_tokens: 2000
Verify:
  - response.tool_calls is non-empty
  - tool_calls[0].name == "read_file"
  - response.stop_reason == "tool_use"
```

#### 7. `live_streaming_thinking_text`
```
Prompt: "What is 7 * 8? Think step by step."
Thinking: enabled, budget_tokens: 2000
Via: complete_stream()
Verify:
  - collected text is non-empty
  - thinking chunks are received but NOT included in assembled text
  - final chunk has stop_reason == "end_turn"
```

#### 8. `live_streaming_thinking_tool_call`
```
Prompt: "Think about what file to read, then read src/main.rs"
Tools: [read_file(path: String)]
Thinking: enabled, budget_tokens: 2000
Via: complete_stream()
Verify:
  - tool call assembled correctly
  - thinking chunks received but excluded from text
  - stop_reason == "tool_use"
```

### Multi-tool (stretch — add if time permits)

#### 9. `live_multi_tool_call` (optional)
```
Prompt: "Read src/main.rs and list the src/ directory"
Tools: [read_file(path: String), list_directory(path: String)]
Verify:
  - response.tool_calls.len() >= 2
  - Both tool names present
```

#### 10. `live_streaming_multi_tool` (optional)
```
Same as above, via complete_stream()
```

---

## Test Infrastructure

### Skip macro

```rust
macro_rules! skip_without_api_key {
    () => {
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            eprintln!("ANTHROPIC_API_KEY not set — skipping live test");
            return;
        }
    };
}
```

### Provider construction

```rust
fn make_provider(model: &str) -> AnthropicProvider {
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap();
    AnthropicProvider::new(api_key, model.to_string())
}

fn haiku_provider() -> AnthropicProvider {
    make_provider("claude-haiku-3-5-20241022")
}

fn sonnet_provider() -> AnthropicProvider {
    make_provider("claude-sonnet-4-20250514")
}
```

### Tool schema helper

```rust
fn read_file_tool() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path" }
            },
            "required": ["path"]
        }),
    }
}
```

---

## CI Workflow

File: `.github/workflows/live-contract-tests.yml`

```yaml
name: Live API Contract Tests

on:
  pull_request:
    branches: [main]    # staging→main releases only
  workflow_dispatch:      # manual trigger

jobs:
  live-tests:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Run live contract tests
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          cd engine
          cargo test --test live_api -- --ignored --test-threads=1
        timeout-minutes: 5
```

### GitHub setup required
1. Add `ANTHROPIC_API_KEY` to repo Settings → Secrets → Actions
2. Use a dedicated low-limit API key (separate from production)

---

## Scope & Constraints

### In scope
- 8 live tests (4 Haiku + 4 Sonnet)
- CI workflow (staging→main + manual dispatch)
- Skip-if-no-key macro
- Provider/tool helper functions

### Out of scope (YAGNI)
- Response content validation (we test plumbing, not model quality)
- Retry/flake tolerance (if Anthropic is down, test fails — that's correct)
- OpenAI live tests (same pattern, separate PR when needed)
- Rate limit handling (sequential execution + short prompts = no issue)
- Test result caching/memoization

---

## Implementation Plan

### Single PR: `feat/live-contract-tests`

1. Create `engine/crates/fx-llm/tests/live_api.rs`
2. Add 8 `#[ignore]` test functions + helpers
3. Create `.github/workflows/live-contract-tests.yml`
4. Update issue #1229 with PR link

### Sizing
- ~250-300 lines test code
- ~40 lines CI workflow
- ~1.5 hours implementation + review

---

## Success Criteria

1. `cargo test --test live_api -- --ignored --test-threads=1` passes with valid API key
2. `cargo test --test live_api` skips all tests (no key = no execution)
3. Normal `cargo test -p fx-llm` does NOT run live tests
4. CI workflow triggers on staging→main PRs and manual dispatch
5. All three thinking bug classes (#1226, #1227, #1228) would be caught by these tests
