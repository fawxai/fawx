# Error Surfacing Spec

## Problem

Many async failures are silently swallowed — logged to tracing/stderr but never surfaced to the user. The `StreamEvent::Error` type and `ErrorCategory` enum exist but aren't consistently used.

A user whose memory init fails, whose embedding index is corrupt, or whose provider returns a transient error sees... nothing. The agent just doesn't respond properly, and the user has no idea why.

## Scope

### In scope
1. Audit all `tracing::warn!`, `eprintln!("warning:...)`, `.ok();`, and `let _ =` patterns in the core crates that represent user-impactful failures
2. For each, determine: should this be surfaced to the user via `StreamEvent::Error`, or is it genuinely internal?
3. Wire user-impactful errors through the SSE stream so the Swift app can display them
4. Add a `GET /v1/errors/recent` endpoint for non-streaming clients to poll for recent errors

### Out of scope
- Changing the ErrorCategory enum (it's already well-designed)
- CLI/TUI error display (already handles StreamEvent::Error at headless.rs:781)
- Retooling the entire error handling approach — this is surgical fixes to existing patterns

## Audit Results

### Must surface to user (currently silent)

| Location | Error | Current behavior | Fix |
|---|---|---|---|
| `startup.rs:990` | Memory init failed | `eprintln!` only | Emit `StreamEvent::Error { category: Memory }` on first message |
| `startup.rs:1070` | Memory embeddings init failed | `tracing::warn!` | Same |
| `startup.rs:1095` | Embedding index load failed | `tracing::warn!` | Same |
| `headless.rs:271` | Embedding save on shutdown failed | `tracing::warn!` | Log only (shutdown, no stream available) — acceptable |
| `headless.rs:1486` | Model reload failed after config change | `tracing::warn!` | Emit system error via bus |
| `headless.rs:1500` | Signal persist failed | `eprintln!` | Emit `StreamEvent::Error { category: System }` |
| `startup.rs:872` | Cron store unavailable | `tracing::warn!` | Emit system warning on first message |
| `startup.rs:769` | Experiment tool unavailable | `eprintln!` | Acceptable — power user feature |

### Acceptable as-is (internal/cleanup)

| Location | Why it's fine |
|---|---|
| `startup.rs:223,237` | Old log cleanup — housekeeping, not user-impactful |
| `startup.rs:333` | Config load warning with fallback to defaults — already prints to terminal |
| `startup.rs:841` | Trusted keys load — security log, not user error |
| `markdown.rs` `.ok()` | String builder push — infallible in practice |
| `auth_store.rs` `.ok()` | Plaintext cleanup — best-effort delete after encryption |
| `process_registry.rs` `let _ =` | Process cleanup — fire-and-forget termination |

## Implementation

### 1. Startup error accumulator
Add a `Vec<StartupWarning>` to `HeadlessApp` that collects non-fatal startup issues. On the first user message, emit them as `StreamEvent::Error` events before processing.

```rust
pub struct StartupWarning {
    pub category: ErrorCategory,
    pub message: String,
}
```

This avoids trying to emit SSE events during startup (no active stream yet).

### 2. Runtime error emission
For errors that happen during a session (model reload, signal persist), emit through the existing `StreamEvent::Error` path. The SSE serializer already handles this.

### 3. Error history endpoint
`GET /v1/errors/recent` — returns last N errors (default 20) with timestamps. Useful for the Swift app to show a notification badge or error log.

```json
{
  "errors": [
    {
      "timestamp": "2026-03-14T20:00:00Z",
      "category": "memory",
      "message": "Failed to initialize memory embeddings: file not found",
      "recoverable": true
    }
  ]
}
```

### 4. Files to modify
- `engine/crates/fx-cli/src/headless.rs` — add `startup_warnings: Vec<StartupWarning>`, emit on first message
- `engine/crates/fx-cli/src/startup.rs` — collect warnings instead of eprintln/tracing::warn for user-impactful errors
- `engine/crates/fx-api/src/handlers/` — add error history endpoint
- `engine/crates/fx-kernel/src/streaming.rs` — no changes needed (types already exist)

### 5. Tests
- Startup with broken memory path → first message includes error event
- Model reload failure → SSE stream includes error event
- `GET /v1/errors/recent` returns accumulated errors
- Error history respects limit parameter
