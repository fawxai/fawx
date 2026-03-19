# App Lock Refactor — Production Fix for Server Responsiveness

**Ship blocker:** Server becomes unresponsive during any long operation (experiment, streaming response) because all HTTP handlers share a single `Arc<Mutex<dyn AppEngine>>`.

---

## Root Cause

`HttpState.app` is `Arc<Mutex<dyn AppEngine>>`. Every handler that needs any app state — even read-only data like `active_model()` — must `state.app.lock().await`. When `process_message` holds the lock for 30+ seconds during an LLM response or minutes during an experiment, every other endpoint blocks.

## Production Fix: Shared Read State

### New type: `SharedReadState`

```rust
/// Frequently-read state extracted from HeadlessApp, updated after each mutation.
/// Readers use this without locking the app Mutex.
pub struct SharedReadState {
    pub active_model: RwLock<String>,
    pub thinking_level: RwLock<ThinkingLevelDto>,
    pub uptime_start: Instant,
    pub tailscale_ip: Option<String>,
}
```

Add to `HttpState`:
```rust
pub shared: Arc<SharedReadState>,
```

### Update pattern

After any operation that changes app state (model switch, thinking change, cycle completion):
```rust
// Inside the locked section, update shared state:
shared.active_model.write().await = app.active_model().to_string();
shared.thinking_level.write().await = app.thinking_level();
```

### Read-only handlers use shared state

Health, status, and other read-only handlers read from `shared` instead of locking app:
```rust
pub async fn handle_health(State(state): State<HttpState>) -> Json<HealthResponse> {
    let model = state.shared.active_model.read().await.clone();
    let uptime = state.shared.uptime_start.elapsed().as_secs();
    Json(HealthResponse { status: "ok", model, uptime_seconds: uptime, skills_loaded: 0 })
}
```

### Which handlers move to shared state

| Handler | Currently locks app for | Can use shared? |
|---------|------------------------|-----------------|
| health | active_model | ✅ Yes |
| status | active_model + config | ✅ model from shared, config snapshot |
| get_thinking | thinking_level | ✅ Yes |
| get_model | active_model | ✅ Yes |
| available_models | model list | ✅ Yes (snapshot on startup + model switch) |
| usage | token counts | ✅ Yes (updated after each cycle) |
| settings reads | various | Most can use snapshots |

### Which handlers still need the app lock

| Handler | Why |
|---------|-----|
| process_message (sessions) | Mutates conversation history, runs cycle |
| set_model | Mutates active model |
| set_thinking | Mutates thinking level |
| config writes | Mutates config |

### Session-level locking (future)

For true multi-session support, `process_message` should lock per-session, not globally. But that's a bigger refactor for post-ship. The shared read state fix unblocks the GUI immediately.

---

## Implementation

1. Define `SharedReadState` in `fx-api/src/state.rs`
2. Initialize from HeadlessApp at startup
3. Update after model switch, thinking change, and cycle completion in session handlers
4. Migrate health, status, get_thinking, get_model, usage handlers to use shared state
5. Keep process_message and mutation handlers on app lock

Estimated: ~200 lines changed across 5-6 handler files + state.rs.
