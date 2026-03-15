# Spec: Multi-Session Management

**Status:** Draft  
**Date:** 2026-03-08  

---

## 1. Problem

Fawx runs a single conversation. To orchestrate work — spawn subagents, delegate tasks, monitor progress — the parent session needs to track and communicate with multiple concurrent sessions.

## 2. Goals

1. **Session registry** — list active sessions with metadata (type, status, age, model)
2. **Session history** — fetch conversation history from any session
3. **Cross-session messaging** — send messages into other sessions
4. **Session lifecycle** — create, pause, resume, destroy sessions
5. **Session persistence** — sessions survive process restart

## 3. Architecture

### New crate: `fx-session`

```
engine/crates/fx-session/
├── src/
│   ├── lib.rs           # Public API
│   ├── registry.rs      # SessionRegistry — tracks all sessions
│   ├── session.rs       # Session — conversation state + metadata
│   ├── store.rs         # SessionStore — persistence layer
│   └── types.rs         # SessionKey, SessionKind, SessionStatus
└── Cargo.toml
```

### Core Types

```rust
pub struct SessionKey(pub String);  // UUID or label

pub enum SessionKind {
    Main,       // Primary user conversation
    Subagent,   // Spawned by parent
    Channel,    // Per-channel (Telegram, Matrix, etc.)
    Cron,       // Scheduled task
}

pub struct SessionInfo {
    pub key: SessionKey,
    pub kind: SessionKind,
    pub status: SessionStatus,
    pub label: Option<String>,
    pub model: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub message_count: usize,
}

pub enum SessionStatus {
    Active,
    Idle,
    Completed,
    Failed,
    Paused,
}
```

### SessionRegistry

```rust
impl SessionRegistry {
    pub fn list(&self, filter: Option<SessionKind>) -> Vec<SessionInfo>;
    pub fn get(&self, key: &SessionKey) -> Option<&Session>;
    pub fn create(&mut self, kind: SessionKind, config: SessionConfig) -> Result<SessionKey>;
    pub fn destroy(&mut self, key: &SessionKey) -> Result<()>;
    pub fn send(&self, key: &SessionKey, message: &str) -> Result<String>;
    pub fn history(&self, key: &SessionKey, limit: usize) -> Result<Vec<Message>>;
}
```

### Tools

- `session_list` — list sessions with optional filters
- `session_history` — fetch history for a session
- `session_send` — send a message to another session

## 4. Integration

- `HeadlessApp` gets a `SessionRegistry` field
- Subagent spawning (PR #1247) integrates: spawned subagents register in the registry
- Each Telegram conversation becomes a session
- Persistence via fx-storage (redb)

## 5. Testing

- Registry CRUD operations
- Cross-session send/receive
- Persistence across restart (store/load)
- Concurrent session access (Arc<RwLock>)
- Session cleanup/GC

## 6. File Touchpoints

- **New:** `engine/crates/fx-session/`
- **Modify:** `engine/crates/fx-cli/src/headless.rs` (add SessionRegistry)
- **Modify:** `engine/crates/fx-subagent/` (register spawned subagents)
- **Modify:** `engine/crates/fx-core/src/tools/` (register session tools)
