# Spec: API Sprint 1 — Session Management + Chat Enrichment

**Status**: Ready for implementation
**Crates touched**: `fx-api` (new handlers + router extension), `fx-session` (new `clear` method), `fx-cli` (AppEngine extension)
**Estimated scope**: ~300 lines production code + ~200 lines tests
**Depends on**: PR #1392 (fx-api extraction) — merged ✅

---

## Problem

The HTTP API has one implicit conversation with no persistence across
restarts. A GUI client needs to:
1. Create and switch between multiple conversations
2. Retrieve conversation history (scroll back)
3. Send images alongside text
4. Clear or delete conversations

The `fx-session` crate already provides `SessionRegistry` with full
CRUD + history + persistence (redb). The work is **wiring** these into
HTTP endpoints in `fx-api` and connecting session-scoped message processing
through the `AppEngine` trait.

---

## Design

### Current architecture (after Sprint 0)

```
POST /message → fx-api handler → AppEngine::process_message()
                                    └── HeadlessApp.conversation_history (single implicit session)
```

### Target architecture

```
POST   /v1/sessions                → SessionRegistry::create()
GET    /v1/sessions                → SessionRegistry::list()
GET    /v1/sessions/{id}           → SessionRegistry::get_info()
DELETE /v1/sessions/{id}           → SessionRegistry::destroy()
GET    /v1/sessions/{id}/messages  → SessionRegistry::history()
POST   /v1/sessions/{id}/messages  → AppEngine::process_with_context() → record in session
POST   /v1/sessions/{id}/clear     → SessionRegistry::clear()

POST   /message                    → extended with optional images + session_id
```

All new endpoints live under `/v1/` prefix. The existing `/message`
endpoint continues to work unchanged (backwards compatible — uses the
implicit "default" session).

### Why /v1/ prefix

We're building an API that a Swift app will depend on. Versioning now
avoids breaking changes later. The existing unversioned endpoints
(`/message`, `/status`, `/config`) remain as-is for backwards
compatibility.

---

## Implementation

### 1. Add `SessionRegistry` to `HttpState` (`fx-api/src/state.rs`)

```rust
use fx_session::SessionRegistry;

pub struct HttpState {
    pub(crate) app: Arc<Mutex<dyn AppEngine>>,
    pub(crate) session_registry: Option<SessionRegistry>,  // NEW
    pub(crate) start_time: Instant,
    pub(crate) bearer_token: String,
    pub(crate) channels: ChannelRuntime,
    pub(crate) data_dir: PathBuf,
}
```

Initialize from the same redb `Storage` that fx-session already uses.
If session store initialization fails, log a warning and set to `None`
(graceful degradation — `/message` still works without sessions).

The `SessionRegistry` is created in `fx-api/src/lib.rs` during startup
and passed into `HttpState`. The `fx-api::run()` function (or a new
`ApiConfig` field) accepts an optional `SessionRegistry`.

### 2. Extend `AppEngine` trait (`fx-api/src/engine.rs`)

Add a method for session-scoped message processing:

```rust
#[async_trait]
pub trait AppEngine: Send + Sync {
    // ... existing methods ...

    /// Process a message with an externally-provided conversation context.
    /// Returns the result plus updated conversation history.
    async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        context: Vec<Message>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<(CycleResult, Vec<Message>), anyhow::Error>;
}
```

This lets session-scoped handlers pass in the session's history without
mutating the engine's internal conversation state. The implementation
in `HeadlessApp` (`fx-cli/src/headless.rs`) temporarily swaps the
conversation history, runs the cycle, and returns the updated history.

**Option A**: ~~Swap HeadlessApp's internal history~~ — fragile, not concurrent-safe.
**Option B**: **New method that accepts external context.** ✅ CHOSEN.

### 3. Create session handlers (`fx-api/src/handlers/sessions.rs`)

New file with 7 handler functions:

#### `POST /v1/sessions` — Create session

Request:
```json
{
    "label": "optional human name",
    "model": "optional model override"
}
```

Response (201):
```json
{
    "key": "sess-a1b2c3d4",
    "kind": "main",
    "status": "idle",
    "label": "optional human name",
    "model": "gpt-4",
    "created_at": 1710300000,
    "updated_at": 1710300000,
    "message_count": 0
}
```

Implementation:
- Generate UUID-based key: `sess-{uuid4_short}`
- Kind: `SessionKind::Main`
- Model: use provided model or fall back to `AppEngine::active_model()`
- Return `SessionInfo` as JSON

#### `GET /v1/sessions` — List sessions

Query params:
- `kind` (optional): filter by session kind
- `limit` (optional, default 50): max results

Response (200):
```json
{
    "sessions": [SessionInfo, ...],
    "total": 42
}
```

#### `GET /v1/sessions/{id}` — Get session info

Response (200): `SessionInfo` JSON
Response (404): `{ "error": "session not found: {id}" }`

#### `DELETE /v1/sessions/{id}` — Delete session

Response (200): `{ "deleted": true, "key": "{id}" }`
Response (404): `{ "error": "session not found: {id}" }`

#### `POST /v1/sessions/{id}/clear` — Clear conversation

Clears message history but keeps the session metadata.

Response (200): `{ "cleared": true, "key": "{id}" }`

#### `GET /v1/sessions/{id}/messages` — Get history

Query params:
- `limit` (optional, default 100): max messages

Response (200):
```json
{
    "messages": [
        {
            "role": "user",
            "content": "hello",
            "timestamp": 1710300000
        },
        {
            "role": "assistant",
            "content": "Hi! How can I help?",
            "timestamp": 1710300001
        }
    ],
    "total": 42
}
```

#### `POST /v1/sessions/{id}/messages` — Send message in session

Request:
```json
{
    "message": "What's the weather like?",
    "images": [
        {
            "data": "base64-encoded-image-data",
            "media_type": "image/jpeg"
        }
    ]
}
```

SSE streaming: If the client sends `Accept: text/event-stream`, stream
the response via SSE (same as current `/message` behavior).

Implementation:
1. Look up session by key from registry (404 if not found)
2. Load session's conversation history via `registry.history()`
3. Convert `Vec<SessionMessage>` → `Vec<Message>` (type bridge)
4. Call `AppEngine::process_message_with_context()` with the session's history
5. Record both user message and assistant response in the session via `registry.send()`
6. Persist automatically (registry persists on each operation)
7. Return response (JSON or SSE stream)

### 4. Extend `/message` endpoint (`fx-api/src/handlers/message.rs`)

Extend `MessageRequest` in `fx-api/src/types.rs`:

```rust
#[derive(Deserialize)]
pub(crate) struct MessageRequest {
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) images: Vec<ImagePayload>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
}
```

`ImagePayload` already exists in `fx-api/src/types.rs` (added during
Sprint 0, currently has `#[allow(dead_code)]`). Remove that allow and
wire it into the message handler.

If `session_id` is provided, route through the session-scoped path.
If not, use the existing implicit conversation (backwards compatible).

### 5. Wire routes into router (`fx-api/src/router.rs`)

Add the `/v1/` routes to the authenticated router:

```rust
let v1 = Router::new()
    .route("/sessions", post(handle_create_session).get(handle_list_sessions))
    .route("/sessions/{id}", get(handle_get_session).delete(handle_delete_session))
    .route("/sessions/{id}/clear", post(handle_clear_session))
    .route("/sessions/{id}/messages", get(handle_get_messages).post(handle_send_message));

let authenticated = Router::new()
    .route("/message", post(handle_message))
    // ... existing routes ...
    .nest("/v1", v1);
```

### 6. Add `clear()` to `SessionRegistry` (`fx-session/src/registry.rs`)

New method:
```rust
pub fn clear(&self, key: &SessionKey) -> Result<()> {
    let snapshot = {
        let mut map = self.write()?;
        let session = map
            .get_mut(key)
            .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
        session.clear_messages();
        session.clone()
    };
    self.store.save(&snapshot)?;
    Ok(())
}
```

And `clear_messages()` on `Session` (`fx-session/src/session.rs`):
```rust
pub fn clear_messages(&mut self) {
    self.messages.clear();
    self.updated_at = now_epoch_secs();
}
```

### 7. Add `fx-session` dependency to `fx-api`

In `engine/crates/fx-api/Cargo.toml`:
```toml
fx-session = { path = "../fx-session" }
```

Also add `uuid` for session key generation:
```toml
uuid = { version = "1", features = ["v4"] }
```

---

## Type Bridge: SessionMessage ↔ Message

`fx-session` uses `SessionMessage` (role + content + timestamp).
`fx-kernel` uses `Message` (role + content blocks + metadata).

The handler needs to convert between them:

```rust
fn session_messages_to_context(messages: &[SessionMessage]) -> Vec<Message> {
    messages.iter().map(|m| Message {
        role: match m.role {
            MessageRole::User => Role::User,
            MessageRole::Assistant => Role::Assistant,
            MessageRole::System => Role::System,
        },
        content: vec![ContentBlock::Text { text: m.content.clone() }],
        // ... default metadata
    }).collect()
}
```

This conversion lives in `fx-api/src/handlers/sessions.rs` as a
private helper.

---

## Edge Cases (MUST handle)

### Session registry unavailable
All `/v1/sessions*` endpoints must check `state.session_registry.is_some()`.
If `None`, return:
```
503 Service Unavailable
{ "error": "session storage not available" }
```

### Empty message body
`POST /v1/sessions/{id}/messages` must validate that `message` is not
empty or whitespace-only, same as `/message` does:
```
400 Bad Request
{ "error": "message must not be empty" }
```

### Invalid base64 image data
If `images[].data` contains invalid base64, return:
```
400 Bad Request
{ "error": "invalid base64 in image at index {i}" }
```
Validate BEFORE processing the message, not during.

### Invalid or unsupported image media_type
Accept only: `image/jpeg`, `image/png`, `image/gif`, `image/webp`.
If unsupported:
```
400 Bad Request
{ "error": "unsupported image media type: {type}" }
```

### Session not found (all single-session endpoints)
`GET/DELETE /v1/sessions/{id}`, `POST /v1/sessions/{id}/clear`,
`GET/POST /v1/sessions/{id}/messages` — all return 404 if session
doesn't exist:
```
404 Not Found
{ "error": "session not found: {id}" }
```

### History with limit=0
Return empty messages array with correct total count:
```json
{ "messages": [], "total": 42 }
```

### List sessions ordering
Return sessions sorted by `updated_at` descending (most recent first).

### Concurrent message sends to same session
`SessionRegistry` uses `RwLock` internally. The handler acquires the
lock for history read, releases, processes the message (which can take
seconds), then re-acquires to record the response. This means two
concurrent sends to the same session could interleave. This is
**acceptable for Sprint 1** — document it as a known limitation.
True session locking is a follow-up.

### Large image payloads
No size limit enforcement in Sprint 1. The HTTP server's body size
limit (if configured) provides a natural ceiling. Document as follow-up.

---

## What NOT to build in Sprint 1

- **Model switching endpoint** (`PUT /model`) — Sprint 2
- **Thinking toggle** (`PUT /thinking`) — Sprint 2
- **Auth management** (`/auth/*`) — Sprint 2
- **Compaction integration** — sessions use their own history, compaction
  applies to HeadlessApp's internal history. Session compaction is a
  follow-up.
- **Session-scoped tool state** — tools operate on global state.
- **WebSocket** — SSE is sufficient for the Swift app.
- **Pagination cursors** — simple `limit` is enough for now.

---

## Testing

### Unit tests in `fx-api` (new test module or extend `tests.rs`)

1. `create_session_returns_201` — POST /v1/sessions returns 201 with
   valid SessionInfo JSON.
2. `list_sessions_returns_array` — GET /v1/sessions returns sessions
   array after creating two.
3. `get_session_returns_info` — GET /v1/sessions/{id} returns correct
   session.
4. `get_nonexistent_session_returns_404` — GET with bad id → 404.
5. `delete_session_removes_it` — DELETE then GET → 404.
6. `clear_session_empties_history` — Send message, clear, get history
   → empty.
7. `session_message_records_history` — POST message, GET history →
   contains user + assistant messages.
8. `session_message_streams_sse` — POST with Accept: text/event-stream
   returns SSE response.
9. `message_with_images_accepted` — POST /message with images array
   doesn't error.
10. `message_with_session_id_routes_to_session` — POST /message with
    session_id records in the correct session.
11. `sessions_require_auth` — All /v1/ endpoints return 401 without
    bearer token.

### Unit test in `fx-session`

12. `session_clear_empties_messages_and_persists` — New method test for
    SessionRegistry::clear().

---

## File changes summary

| File | Change |
|------|--------|
| `fx-api/src/handlers/sessions.rs` | NEW — 7 session endpoint handlers + type bridge helper |
| `fx-api/src/handlers/mod.rs` | Add `pub(crate) mod sessions;` |
| `fx-api/src/handlers/message.rs` | Extend to handle images + session_id routing |
| `fx-api/src/types.rs` | Extend MessageRequest with images + session_id, remove dead_code allow on ImagePayload |
| `fx-api/src/engine.rs` | Add `process_message_with_context()` to AppEngine trait |
| `fx-api/src/state.rs` | Add `session_registry: Option<SessionRegistry>` to HttpState |
| `fx-api/src/router.rs` | Add `/v1/` route nest with session endpoints |
| `fx-api/src/lib.rs` | Initialize SessionRegistry during startup |
| `fx-api/Cargo.toml` | Add fx-session + uuid dependencies |
| `fx-api/src/tests.rs` | Add 11 session endpoint tests |
| `fx-cli/src/headless.rs` | Implement `process_message_with_context()` for HeadlessApp |
| `fx-session/src/registry.rs` | Add `clear()` method |
| `fx-session/src/session.rs` | Add `clear_messages()` method |

---

## Route summary

```
POST   /v1/sessions                → handle_create_session
GET    /v1/sessions                → handle_list_sessions
GET    /v1/sessions/{id}           → handle_get_session
DELETE /v1/sessions/{id}           → handle_delete_session
POST   /v1/sessions/{id}/clear     → handle_clear_session
GET    /v1/sessions/{id}/messages  → handle_get_messages
POST   /v1/sessions/{id}/messages  → handle_send_message
```

Plus extended: `POST /message` gains optional `images` + `session_id`.
