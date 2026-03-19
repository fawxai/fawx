# Sprint 3: Backend Enhancements for Swift App

**Status:** Implementing  
**Branch:** `feat/api-sprint3-enhancements`  
**PR target:** `dev`

---

## Overview

Four backend enhancements required before the Swift app can be built. All are in `fx-api` and `fx-session` crates.

---

## 1. SSE Keep-Alive Pings (Highest Priority)

**Problem:** During tool execution, the SSE stream goes silent for 10-60+ seconds. The Swift app cannot distinguish "thinking/tool running" from "dead connection." Browsers and HTTP clients may time out.

**Solution:** Send SSE comment pings (`: ping\n\n`) every 15 seconds while the stream is alive but no data events have been sent recently.

### Implementation

**File: `engine/crates/fx-api/src/sse.rs`**

Add a new function that wraps an `mpsc::Receiver<String>` with a ping-injecting layer:

```rust
/// Interval between keep-alive pings when no data is flowing.
pub const SSE_PING_INTERVAL: Duration = Duration::from_secs(15);

/// SSE comment ping (not an event — clients ignore it, but it keeps the connection alive).
pub const SSE_PING_FRAME: &str = ": ping\n\n";
```

Modify `sse_response()` to use a `tokio::select!` loop that:
1. Waits for the next data frame from the receiver
2. If no frame arrives within `SSE_PING_INTERVAL`, sends `SSE_PING_FRAME`
3. If the receiver closes (sender dropped), ends the stream

The ping is an SSE **comment** (starts with `:`), not an event. Per the SSE spec, clients MUST ignore comments. This is the standard keep-alive mechanism.

**Key constraint:** The ping timer resets every time a real data frame is sent. We only ping during silence.

### Approach

Replace the `stream::unfold` in `sse_response()` with a spawned task that reads from the data receiver and writes to an output channel, inserting pings on timeout:

```rust
pub fn sse_response(receiver: mpsc::Receiver<String>) -> Response {
    let (tx, rx) = mpsc::channel::<String>(SSE_CHANNEL_CAPACITY);
    tokio::spawn(ping_relay(receiver, tx));
    // Build response from rx (same as before but using rx)
    ...
}

async fn ping_relay(mut data_rx: mpsc::Receiver<String>, tx: mpsc::Sender<String>) {
    loop {
        tokio::select! {
            frame = data_rx.recv() => {
                match frame {
                    Some(frame) => { if tx.send(frame).await.is_err() { break; } }
                    None => break, // stream ended
                }
            }
            _ = tokio::time::sleep(SSE_PING_INTERVAL) => {
                if tx.send(SSE_PING_FRAME.to_string()).await.is_err() { break; }
            }
        }
    }
}
```

### Tests

1. `ping_sent_during_silence` — Create a sender, don't send data, verify ping arrives within ~15s (use `tokio::time::pause()` for instant test).
2. `ping_not_sent_while_data_flows` — Send data rapidly, verify no pings interleaved.
3. `stream_ends_when_sender_drops` — Drop the data sender, verify the relay task exits and the output stream closes.

---

## 2. GET /v1/sessions/{id}/context — Context Window Endpoint

**Problem:** The Swift app needs to show a context window usage indicator (status bar). The data exists server-side in `ConversationBudget` but isn't exposed via HTTP.

### API Contract

```
GET /v1/sessions/{id}/context
```

Response:
```json
{
  "used_tokens": 4200,
  "max_tokens": 16384,
  "percentage": 25.6,
  "compaction_threshold": 0.8
}
```

### Implementation

**File: `engine/crates/fx-api/src/engine.rs`** — Add to `AppEngine` trait:

```rust
fn context_info(&self) -> ContextInfoDto;
```

**File: `engine/crates/fx-api/src/types.rs`** — Add DTO:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ContextInfoDto {
    pub used_tokens: usize,
    pub max_tokens: usize,
    pub percentage: f32,
    pub compaction_threshold: f32,
}
```

**File: `engine/crates/fx-cli/src/headless.rs`** — Implement on `HeadlessApp`:

`HeadlessApp` has:
- `conversation_history: Vec<Message>` — current conversation messages
- `loop_engine: LoopEngine` — which has `conversation_budget: ConversationBudget`

Implementation:
1. Get token count from `ConversationBudget::estimate_tokens(&self.conversation_history)`
2. Get max from `self.loop_engine.conversation_budget().conversation_budget()` (the available conversation budget after reserves)
3. Get model limit from `self.loop_engine.conversation_budget().model_context_limit()`
4. Compute percentage: `(used_tokens as f32 / max_tokens as f32) * 100.0`

The `ConversationBudget` fields are currently private. Add public accessors:

In `engine/crates/fx-kernel/src/conversation_compactor.rs`, add to `impl ConversationBudget`:
```rust
pub fn model_context_limit(&self) -> usize { self.model_context_limit }
pub fn compaction_threshold_value(&self) -> f32 { self.compaction_threshold }
```

In `engine/crates/fx-kernel/src/loop_engine.rs`, add to `impl LoopEngine`:
```rust
pub fn conversation_budget(&self) -> &ConversationBudget { &self.conversation_budget }
```

**Note:** The context endpoint is "global" (not truly per-session) since the loop engine has one conversation budget. The session ID in the URL is for API consistency and future per-session budgets. For now, return the global budget regardless of session ID (but validate the session exists).

**File: `engine/crates/fx-api/src/handlers/sessions.rs`** — Add handler:

```rust
pub async fn handle_get_context(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorBody>)> {
    let registry = require_session_registry(&state)?;
    let key = session_key(&id)?;
    // Validate session exists
    registry.get_info(&key).map_err(|e| map_session_error(&id, e))?;
    let app = state.app.lock().await;
    Ok(Json(app.context_info()).into_response())
}
```

**File: `engine/crates/fx-api/src/router.rs`** — Add route:

```rust
.route("/v1/sessions/{id}/context", get(handle_get_context))
```

### Tests

1. `context_endpoint_returns_budget_info` — Verify response shape with mock AppEngine.
2. `context_endpoint_rejects_unknown_session` — 404 for nonexistent session ID.

---

## 3. SessionInfo Title + Preview

**Problem:** The Swift app sidebar needs to show a title and preview for each session without making N+1 API calls to fetch message history for every session.

### Implementation

**File: `engine/crates/fx-session/src/types.rs`** — Add fields to `SessionInfo`:

```rust
pub struct SessionInfo {
    // ... existing fields ...
    /// Title derived from the first user message (truncated).
    pub title: Option<String>,
    /// Preview of the most recent message (truncated).
    pub preview: Option<String>,
}
```

**File: `engine/crates/fx-session/src/session.rs`** — Update `Session::info()`:

```rust
pub fn info(&self) -> SessionInfo {
    SessionInfo {
        // ... existing fields ...
        title: self.compute_title(),
        preview: self.compute_preview(),
    }
}

fn compute_title(&self) -> Option<String> {
    self.messages
        .iter()
        .find(|m| m.role == MessageRole::User)
        .map(|m| truncate_text(&m.content, 80))
}

fn compute_preview(&self) -> Option<String> {
    self.messages
        .last()
        .map(|m| truncate_text(&m.content, 120))
}
```

Add a helper:
```rust
fn truncate_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max_chars {
        trimmed.to_string()
    } else {
        // Truncate at char boundary
        let mut end = max_chars;
        while !trimmed.is_char_boundary(end) { end -= 1; }
        format!("{}…", &trimmed[..end])
    }
}
```

### Tests

1. `session_info_title_from_first_user_message` — Title is first user message, truncated.
2. `session_info_preview_from_last_message` — Preview is last message.
3. `session_info_no_title_when_no_user_messages` — Title is None for empty session.
4. `truncate_text_handles_multibyte` — Unicode safety.
5. Update existing `session_info_serializes_to_json` test to include new fields.

---

## 4. SkillSummaryDto Description Field

**Problem:** The Swift app skills grid needs a description for each skill. The WASM manifest has a `description: String` field but it's not exposed through the API.

### Implementation

**File: `engine/crates/fx-loadable/src/registry.rs`** — Change `skill_summaries()` return type:

```rust
/// Return (name, description, tool_names) for each registered skill.
pub fn skill_summaries(&self) -> Vec<(String, String, Vec<String>)> {
    let skills = self.skills.read().unwrap_or_else(|p| p.into_inner());
    skills
        .iter()
        .map(|skill| {
            let tools = skill.tool_definitions().into_iter().map(|d| d.name).collect();
            (skill.name().to_string(), skill.description().to_string(), tools)
        })
        .collect()
}
```

This requires a `description()` method on the `Skill` trait. Check if it exists; if not, add it.

**File: `engine/crates/fx-skills/src/lib.rs`** (or wherever `Skill` trait is defined) — Add if missing:

```rust
fn description(&self) -> &str { "" }  // default impl for backward compat
```

**File: `engine/crates/fx-api/src/types.rs`** — Update DTO:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SkillSummaryDto {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
}

impl From<(String, String, Vec<String>)> for SkillSummaryDto {
    fn from((name, description, tools): (String, String, Vec<String>)) -> Self {
        Self { name, description, tools }
    }
}
```

**File: `engine/crates/fx-cli/src/headless.rs`** — Update the `skill_summaries()` impl to pass through the new tuple shape. The `From` impl handles conversion.

### Tests

1. `skill_summary_includes_description` — Verify description appears in API response.
2. Update existing `skill_summaries_returns_skill_names_and_tools` test for new tuple.

---

## File Change Summary

| File | Changes |
|------|---------|
| `fx-api/src/sse.rs` | Ping relay, constants |
| `fx-api/src/engine.rs` | `context_info()` trait method |
| `fx-api/src/types.rs` | `ContextInfoDto`, `SkillSummaryDto` description field |
| `fx-api/src/handlers/sessions.rs` | `handle_get_context` |
| `fx-api/src/router.rs` | Context route |
| `fx-api/src/tests.rs` | Mock updates, new tests |
| `fx-session/src/types.rs` | `title`, `preview` fields on `SessionInfo` |
| `fx-session/src/session.rs` | `compute_title`, `compute_preview`, `truncate_text` |
| `fx-kernel/src/conversation_compactor.rs` | Public accessors on `ConversationBudget` |
| `fx-kernel/src/loop_engine.rs` | Public accessor for `conversation_budget` |
| `fx-loadable/src/registry.rs` | Updated `skill_summaries()` return type |
| `fx-cli/src/headless.rs` | `context_info()` impl, updated skill summaries |

Estimated: ~250 lines production code, ~150 lines tests.
