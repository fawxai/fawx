# Cross-Session Messaging

**Status:** Specifying  
**Branch:** `feat/session-bus`  
**PR target:** `dev`

---

## Problem

Sessions in Fawx are isolated. A main session can spawn subagents and send them messages (parent→child), but:

1. **No child→parent messaging** — subagents return a final result string, but can't send interim updates or requests back to the parent session
2. **No peer-to-peer messaging** — session A can't send a message to session B
3. **No offline delivery** — if the target session isn't running, the message is lost
4. **No external injection** — no HTTP endpoint to inject a message into a session from outside

This blocks:
- Fleet task execution (primary dispatches task → worker, worker reports result → primary)
- Cron/scheduled triggers (daemon injects wake message into a session)
- Multi-agent orchestration (orchestrator steers multiple agents, agents report back)
- Subagent progress reporting (child sends status updates to parent during long tasks)

---

## Design

### New crate: `fx-bus`

A lightweight session-to-session message router. Lives alongside `fx-session` and `fx-subagent`.

```
engine/crates/fx-bus/
├── Cargo.toml
├── src/
│   ├── lib.rs          — public API: SessionBus, Envelope, BusError
│   ├── envelope.rs     — message envelope type
│   ├── router.rs       — in-memory routing + subscriber management
│   └── store.rs        — persistent queue for offline delivery (redb)
```

### Core Types

```rust
/// A message envelope routed between sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Unique message ID.
    pub id: String,
    /// Sender session key. None for system-generated messages (cron, fleet).
    pub from: Option<SessionKey>,
    /// Target session key.
    pub to: SessionKey,
    /// Message payload.
    pub payload: Payload,
    /// Unix epoch millis when the message was created.
    pub created_at: u64,
}

/// What the message carries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Payload {
    /// Plain text message to be processed by the target session's agent loop.
    Text(String),
    /// Structured result from a completed task (subagent result, fleet task result).
    TaskResult {
        task_id: String,
        success: bool,
        output: String,
    },
    /// Status update from a long-running task.
    StatusUpdate {
        task_id: String,
        progress: String,
    },
    /// System event (cron trigger, health alert, fleet event).
    System(String),
}
```

### SessionBus

```rust
/// Session-to-session message router.
///
/// Thread-safe, cloneable. One instance per Fawx server.
#[derive(Clone)]
pub struct SessionBus {
    /// Active subscribers: session key → mpsc sender.
    subscribers: Arc<RwLock<HashMap<SessionKey, mpsc::Sender<Envelope>>>>,
    /// Persistent queue for offline delivery.
    store: BusStore,
}

impl SessionBus {
    /// Create a new bus backed by persistent storage.
    pub fn new(store: BusStore) -> Self;

    /// Subscribe a session to receive messages.
    /// Returns an mpsc::Receiver the session polls for incoming envelopes.
    /// Any queued (offline) messages are drained into the channel immediately.
    pub fn subscribe(&self, key: &SessionKey) -> mpsc::Receiver<Envelope>;

    /// Unsubscribe a session (e.g., on session close/destroy).
    pub fn unsubscribe(&self, key: &SessionKey);

    /// Send a message to a session.
    /// - If the target is subscribed (online): deliver via mpsc channel.
    /// - If the target is offline: persist to the store for later delivery.
    /// Returns the envelope ID.
    pub async fn send(&self, envelope: Envelope) -> Result<String, BusError>;

    /// Drain any persisted (offline) messages for a session.
    /// Called internally by `subscribe()`.
    fn drain_offline(&self, key: &SessionKey, tx: &mpsc::Sender<Envelope>) -> Result<(), BusError>;
}
```

### How It Plugs In

**1. Server startup** (`fx-cli/src/startup.rs`):
```rust
let bus_store = BusStore::new(storage.clone());
let session_bus = SessionBus::new(bus_store);
// Pass to HeadlessApp and HTTP state
```

**2. HeadlessApp** (`fx-cli/src/headless.rs`):
- On session activation: `bus.subscribe(&session_key)` → get receiver
- In the message processing loop: `tokio::select!` between user input and bus receiver
- When an envelope arrives: convert to user-role or system-role message depending on payload type, feed into `process_message`

**3. SubagentManager** (`fx-subagent/src/manager.rs`):
- Subagent instances get a `SessionBus` handle
- Child can call `bus.send(Envelope { to: parent_key, payload: StatusUpdate { ... } })`
- Parent receives it via its subscription

**4. HTTP API** (`fx-api/src/handlers/sessions.rs`):
```
POST /v1/sessions/{id}/send
Body: { "text": "..." } or { "payload": { ... } }
Response: { "envelope_id": "..." }
```
External orchestrators (fleet primary, cron daemon, CLI tools) use this to inject messages.

**5. Fleet integration** (future):
- Fleet primary uses `POST /v1/sessions/{id}/send` to dispatch tasks to worker sessions
- Workers use `bus.send()` to report results back to the primary's session

### Persistence (BusStore)

Uses redb (already a dependency via `fx-storage`). One table:

```
Table: "bus_queue"
Key: (target_session_key, message_id) → serialized Envelope
```

Messages are written when target is offline, deleted when drained on subscribe. TTL cleanup (optional): messages older than 24h are dropped on startup.

### Channel capacity

- `mpsc::channel(256)` per subscriber — large enough for burst delivery
- If channel full: persist to store instead of dropping (back-pressure → offline path)

---

## File Changes

| File | Change |
|------|--------|
| `engine/crates/fx-bus/` (NEW) | New crate: `Envelope`, `Payload`, `SessionBus`, `BusStore`, `BusError` |
| `engine/Cargo.toml` | Add `fx-bus` to workspace members |
| `engine/crates/fx-api/Cargo.toml` | Add `fx-bus` dependency |
| `engine/crates/fx-api/src/engine.rs` | Add `session_bus()` to `AppEngine` trait |
| `engine/crates/fx-api/src/handlers/sessions.rs` | Add `handle_send_to_session` handler |
| `engine/crates/fx-api/src/router.rs` | Add `/v1/sessions/{id}/send` route |
| `engine/crates/fx-api/src/types.rs` | Add `SendToSessionRequest`, `SendToSessionResponse` DTOs |
| `engine/crates/fx-cli/src/headless.rs` | Subscribe to bus, select on bus receiver in message loop |
| `engine/crates/fx-cli/src/startup.rs` | Create `SessionBus`, wire into `HeadlessApp` |
| `engine/crates/fx-subagent/Cargo.toml` | Add `fx-bus` dependency |
| `engine/crates/fx-subagent/src/instance.rs` | Pass bus handle to subagent, enable child→parent sends |

Estimated: ~400 lines new crate + ~100 lines integration = ~500 lines total.

---

## Tests

### fx-bus unit tests
1. `send_to_online_session_delivers_immediately` — subscribe, send, recv
2. `send_to_offline_session_persists_to_store` — send without subscribe, verify in store
3. `subscribe_drains_offline_messages` — send offline, then subscribe, verify delivery
4. `unsubscribe_removes_subscriber` — unsubscribe, send, verify goes to store
5. `multiple_subscribers_independent` — two sessions, messages don't cross
6. `envelope_roundtrip_serde` — serialize/deserialize all Payload variants
7. `channel_full_falls_back_to_store` — fill channel, verify overflow goes to store
8. `concurrent_send_does_not_panic` — multiple senders, one receiver, no deadlock

### fx-api integration tests
9. `send_to_session_endpoint_returns_envelope_id` — HTTP POST, verify response
10. `send_to_nonexistent_session_queues_for_later` — POST to unknown key, verify 200 (queued)

### fx-subagent integration tests
11. `subagent_sends_status_to_parent` — spawn subagent with bus, verify parent receives update

---

## Non-Goals (V1)

- **Message acknowledgment** — fire-and-forget. No delivery receipts.
- **Message ordering guarantees** — mpsc preserves order per sender, but cross-sender ordering is best-effort.
- **Encryption** — messages are local (same process or same Tailscale fleet). No encryption needed.
- **Rate limiting** — trust boundary is the HTTP auth layer, not the bus.
- **Message expiry/TTL** — V1 keeps messages in the store until drained. TTL cleanup is V2.
