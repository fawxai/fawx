# Spec: API Sprint 1 — Session Management + Chat Enrichment

**Status**: Ready for implementation
**Crates touched**: `fx-cli` (http_serve.rs, headless.rs), `fx-session`
**Estimated scope**: ~250 lines production code + ~150 lines tests
**Depends on**: PR #1391 (error surfacing) — merge first to avoid conflicts

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
HTTP endpoints and connecting session-scoped message processing.

---

## Design

### Current architecture

```
POST /message → HeadlessApp::process_message()
                  └── uses self.conversation_history: Vec<Message>
                       (single implicit conversation, in-memory only)
```

### Target architecture

```
POST /v1/sessions                  → SessionRegistry::create()
GET  /v1/sessions                  → SessionRegistry::list()
GET  /v1/sessions/{id}             → SessionRegistry::get_info()
DELETE /v1/sessions/{id}           → SessionRegistry::destroy()
GET  /v1/sessions/{id}/messages    → SessionRegistry::history()
POST /v1/sessions/{id}/messages    → process + record in session
POST /v1/sessions/{id}/clear       → clear conversation history
```

All new endpoints live under `/v1/` prefix. The existing `/message`
endpoint continues to work unchanged (backwards compatible — uses the
implicit "default" session).

### Why /v1/ prefix

We're building an API that external clients (Swift app) will depend on.
Versioning now avoids breaking changes later. The existing unversioned
endpoints (`/message`, `/status`, `/config`) remain as-is for backwards
compatibility.

---

## Implementation

### 1. Add `SessionRegistry` to `HttpState`

```rust
// In http_serve.rs
struct HttpState {
    app: Arc<Mutex<HeadlessApp>>,
    session_registry: Option<SessionRegistry>,  // NEW
    start_time: Instant,
    // ... existing fields
}
```

Initialize from the same redb `Storage` that fx-session already uses.
If session store initialization fails, log a warning and set to `None`
(graceful degradation — `/message` still works without sessions).

### 2. Session CRUD endpoints

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
- Model: use provided model or fall back to `HeadlessApp::active_model()`
- Return `SessionInfo` as JSON

#### `GET /v1/sessions` — List sessions

Query params:
- `kind` (optional): filter by session kind
- `limit` (optional, default 50): max results
- `offset` (optional, default 0): pagination

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

Implementation: Add a `clear()` method to `SessionRegistry` that empties
the session's message vec and persists. This is a new method — the
registry currently has no clear operation.

Response (200): `{ "cleared": true, "key": "{id}" }`

### 3. Session-scoped messaging

#### `GET /v1/sessions/{id}/messages` — Get history

Query params:
- `limit` (optional, default 100): max messages
- `before` (optional): pagination cursor (message index)

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

The `images` array is optional. Each entry has `data` (base64) and
`media_type` (MIME type).

SSE streaming: If the client sends `Accept: text/event-stream`, stream
the response via SSE (same as current `/message` behavior).

Implementation:
1. Look up session by key from registry (404 if not found)
2. Load session's conversation history into a temporary `Vec<Message>`
3. Call `HeadlessApp::process_message_with_images()` with the session's
   history context
4. Record both user message and assistant response in the session
5. Persist via `SessionStore::save()`
6. Return response (JSON or SSE stream)

**Key design decision**: The session's `Vec<SessionMessage>` is the
source of truth for history. But `HeadlessApp::process_message()` uses
its own `conversation_history: Vec<Message>`. We need a bridge:

Option A: Before each session-scoped call, swap HeadlessApp's
conversation_history with the session's history. After the call, save
the updated history back to the session.

Option B: Add a `process_message_with_context()` method that accepts
an external history vec instead of using the internal one.

**Recommended: Option B.** It's cleaner, doesn't mutate shared state,
and works correctly with concurrent sessions. Add to HeadlessApp:

```rust
pub async fn process_message_with_context(
    &mut self,
    input: &str,
    images: Vec<EncodedImage>,
    context: Vec<Message>,
    callback: Option<StreamCallback>,
) -> Result<(CycleResult, Vec<Message>), anyhow::Error>
```

Returns the result plus the updated conversation history (with the new
user + assistant messages appended). The caller (HTTP handler) persists
this back to the session.

### 4. Image support on existing `/message` endpoint

Extend `MessageRequest` to accept optional images:

```rust
#[derive(Deserialize)]
struct MessageRequest {
    message: String,
    #[serde(default)]
    images: Vec<ImagePayload>,
    #[serde(default)]
    session_id: Option<String>,  // optional: route to a session
}

#[derive(Deserialize)]
struct ImagePayload {
    data: String,      // base64-encoded
    media_type: String, // e.g. "image/jpeg"
}
```

If `session_id` is provided, route through the session-scoped path.
If not, use the existing implicit conversation (backwards compatible).

This means the existing `/message` endpoint gets image support AND
optional session routing without breaking any current clients.

---

## What NOT to build in Sprint 1

- **Model switching endpoint** (`PUT /model`) — Sprint 2
- **Thinking toggle** (`PUT /thinking`) — Sprint 2
- **Auth management** (`/auth/*`) — Sprint 2
- **Compaction integration** — sessions use their own history, compaction
  applies to HeadlessApp's internal history. Session compaction is a
  follow-up (sessions can grow unbounded for now; `limit` on history
  queries prevents huge responses).
- **Session-scoped tool state** — tools currently operate on global state.
  Session isolation for tools is a future concern.
- **WebSocket** — SSE is sufficient for the Swift app. URLSession handles
  SSE natively.

---

## Testing

### Unit tests (`fx-cli`, http_serve.rs)

1. `create_session_returns_info` — POST /v1/sessions returns 201 with
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

### Integration test (`fx-session`)

11. `session_clear_empties_messages_and_persists` — New method test for
    SessionRegistry::clear().

---

## File changes summary

| File | Change |
|------|--------|
| `fx-cli/src/http_serve.rs` | Add 7 new route handlers, SessionRegistry in HttpState, ImagePayload type, session_id on MessageRequest |
| `fx-cli/src/headless.rs` | Add `process_message_with_context()` method |
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
