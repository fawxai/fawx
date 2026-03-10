# Phase 1b PR 4: Embedded Mode

## Overview

Add `--embedded` flag to `fawx-tui` that starts the engine in-process instead of connecting to a remote HTTP server. This gives users a single-binary experience: `fawx-tui --embedded` = standalone Fawx with no server needed.

## Architecture

Currently:
```
fawx serve --http  →  HTTP server (port 8400)  ←  fawx-tui (HTTP client)
```

After this PR:
```
fawx-tui                →  HTTP client mode (connects to server, existing behavior)
fawx-tui --embedded     →  in-process engine (no server needed)
```

### How it works

Embedded mode reuses the same `HeadlessApp` + `process_and_route_message()` pipeline that the HTTP server uses, but calls it directly via an in-process channel instead of HTTP.

The TUI's `FawxBackend` trait gets a second implementation:
- `HttpBackend` — existing HTTP client (renamed from current `FawxBackend`)
- `EmbeddedBackend` — holds `Arc<Mutex<HeadlessApp>>`, calls engine directly

Both produce the same `BackendEvent` stream, so `app.rs` doesn't change.

### Key constraint

The `fawx-tui` crate must depend on `fx-cli` (or at least `fx-cli`'s public startup helpers) to build the engine. This means `fawx-tui --embedded` requires the `http` feature to get `HeadlessApp`, slash command routing, etc.

Alternative: extract engine startup into a shared crate (`fx-engine` or similar). But that's a larger refactor — for now, just add `fx-cli` as an optional dependency gated behind an `embedded` feature flag.

## Implementation

### 1. Add `--embedded` CLI flag

In `tui/src/main.rs`, add:
```rust
#[derive(Parser)]
struct Args {
    /// Run in embedded mode (start engine in-process, no server needed)
    #[arg(long)]
    embedded: bool,

    /// Server host URL (ignored in embedded mode)
    #[arg(long, default_value = "http://127.0.0.1:8400")]
    host: String,
}
```

### 2. Extract `BackendTrait` from `FawxBackend`

Current `FawxBackend` is a concrete struct. Extract a trait:

```rust
#[async_trait]
pub trait EngineBackend: Send + Sync {
    async fn stream_message(&self, message: String, tx: UnboundedSender<BackendEvent>);
    async fn check_health(&self, tx: UnboundedSender<BackendEvent>);
}
```

`app.rs` stores `Arc<dyn EngineBackend>` instead of `Arc<FawxBackend>`.

### 3. Implement `EmbeddedBackend`

New file: `tui/src/embedded_backend.rs`

```rust
pub struct EmbeddedBackend {
    app: Arc<Mutex<HeadlessApp>>,
    router: Arc<ResponseRouter>,  // or None — embedded doesn't need routing
}

impl EngineBackend for EmbeddedBackend {
    async fn stream_message(&self, message: String, tx: UnboundedSender<BackendEvent>) {
        let mut guard = self.app.lock().await;
        // Check for slash commands first
        if is_command_input(&message) {
            let parsed = parse_command(&message);
            // ... execute command, send BackendEvent::Done
        } else {
            // Run the loop engine
            let result = guard.process_message(&message).await;
            // Convert to BackendEvents (TextDelta, ToolUse, Done)
        }
    }
}
```

### 4. Engine initialization in embedded mode

Reuse `build_headless_startup()` from `fx-cli/src/main.rs`. This needs to be public or extracted:

```rust
// In fx-cli, make public:
pub fn build_headless_startup(...) -> Result<HeadlessStartup>
```

Or better: extract the startup logic into a builder function that both `main.rs` and `embedded_backend.rs` can call.

### 5. Streaming challenge

The HTTP backend gets SSE streaming naturally. The embedded backend needs to produce `TextDelta` events as the engine generates tokens. 

`HeadlessApp::process_message()` currently returns the full response. For streaming, we need `process_message_streaming()` that takes a callback or channel.

Check if `LoopEngine` already supports streaming internally — it likely does for the HTTP SSE path. The key is: `http_serve.rs` has streaming SSE. How does it get token-by-token output?

### 6. Feature gate

In `tui/Cargo.toml`:
```toml
[features]
default = []
embedded = ["fx-cli"]

[dependencies]
fx-cli = { workspace = true, optional = true }
```

Build: `cargo build -p fawx-tui --features embedded`

## Files to change

| File | Change |
|------|--------|
| `tui/src/main.rs` | Add `--embedded` flag, branch on mode |
| `tui/src/fawx_backend.rs` | Extract `EngineBackend` trait, rename struct to `HttpBackend` |
| `tui/src/embedded_backend.rs` | **NEW** — `EmbeddedBackend` impl |
| `tui/src/app.rs` | Use `Arc<dyn EngineBackend>` instead of `Arc<FawxBackend>` |
| `tui/src/lib.rs` | Add module |
| `tui/Cargo.toml` | Add `embedded` feature, `fx-cli` optional dep |
| `engine/crates/fx-cli/src/main.rs` | Make startup helpers public |

## Estimated size
~400-500 lines new code. No deletions (that's PR 5).

## Non-streaming MVP option

If streaming from the embedded engine is complex, ship MVP without streaming:
- Embedded mode sends the full response as a single `BackendEvent::Done`
- No `TextDelta` events (user sees response appear all at once)
- Still fully functional — just no typewriter effect
- Streaming can be added later

This dramatically simplifies the implementation since we just call `process_message()` and return the result.

## Testing

- Unit test: embedded backend processes a slash command correctly
- Unit test: backend trait dispatch (http vs embedded)  
- Integration: `fawx-tui --embedded` starts without a server running
- The TUI smoke test (manual) is the real gate
