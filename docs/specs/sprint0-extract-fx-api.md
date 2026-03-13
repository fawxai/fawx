# Spec: Sprint 0 — Extract `fx-api` Crate

**Status**: Ready for implementation
**Type**: Pure refactor — zero behavior changes
**Source**: `fx-cli/src/http_serve.rs` (3,482 lines, 40 tests) + `fx-cli/src/fleet_endpoints.rs` (444 lines, 2 tests)
**Target**: New crate `engine/crates/fx-api/`
**Validation**: Every existing test passes. Every endpoint returns identical responses. No new features.

---

## Problem

All HTTP API logic lives in `fx-cli`, the CLI binary crate. This is wrong:

1. **`fx-cli` is a 3,500-line monolith** mixing CLI concerns (TUI launch,
   daemon start/stop, setup wizard) with API concerns (route handlers,
   request/response types, SSE streaming, auth middleware).
2. **The API is becoming a first-class product surface.** A Swift app will
   depend on these endpoints. API contract types and handlers belong in a
   library crate, not a binary's internal module.
3. **Tests are untestable in isolation.** API handler tests require
   importing `fx-cli` internals. A library crate exposes proper test APIs.
4. **Future consumers** (fleet workers, desktop apps, web clients) should
   import `fx-api` without pulling in CLI dependencies.

---

## Goal

Move all HTTP API code from `fx-cli` into a new `fx-api` library crate.
`fx-cli` becomes a thin shell: reads config, builds app state, calls
`fx_api::run()`.

**The diff must be a pure refactor.** If any endpoint behaves differently
after extraction, it's a bug.

---

## Target Structure

```
engine/crates/fx-api/
├── Cargo.toml
└── src/
    ├── lib.rs              — public API: run(), re-exports
    ├── state.rs            — HttpState, ChannelRuntime
    ├── types.rs            — request/response structs (MessageRequest,
    │                         MessageResponse, HealthResponse, StatusResponse,
    │                         ErrorBody, EncodedImage, ImagePayload)
    ├── router.rs           — build_router(), route tree assembly
    ├── middleware.rs        — auth_middleware(), verify_token()
    ├── sse.rs              — wants_sse(), serialize_stream_event(),
    │                         sse_frame(), error_stream_frame(),
    │                         sse_response(), stream_callback()
    ├── tailscale.rs        — is_tailscale_ip(), detect_tailscale_ip(),
    │                         detect_via_tailscale_cli(), detect_via_cgnat_scan()
    ├── listener.rs         — ListenTarget, ListenPlan, BoundListener,
    │                         BoundListeners, bind/serve/run functions
    ├── error.rs            — HttpError enum
    ├── handlers/
    │   ├── mod.rs
    │   ├── message.rs      — handle_message(), stream_message_response(),
    │   │                     run_streaming_message_task(),
    │   │                     process_and_route_message(), run_message_cycle()
    │   ├── health.rs       — handle_health(), handle_status()
    │   ├── config.rs       — handle_config_get(), handle_config_set(),
    │   │                     sanitize_config(), sanitized_status_config()
    │   ├── webhook.rs      — handle_webhook()
    │   └── fleet.rs        — fleet_router(), handle_fleet_register(),
    │                         handle_fleet_heartbeat(), handle_fleet_result()
    │                         (moved from fleet_endpoints.rs)
    ├── telegram/
    │   ├── mod.rs
    │   ├── polling.rs      — run_telegram_polling(), handle_telegram_update()
    │   ├── webhook.rs      — handle_telegram_webhook()
    │   └── helpers.rs      — encode_photos(), download_and_encode_photo(),
    │                         media_inbound_dir(), telegram_context(),
    │                         queue_telegram_error(), flush_telegram_outbound()
    └── token.rs            — validate_bearer_token()
```

---

## What Moves Where

### From `fx-cli/src/http_serve.rs` (3,482 lines)

| Lines (approx) | Content | Target |
|----------------|---------|--------|
| 1-40 | Imports | Distributed to each module |
| 48-82 | Request/response types | `types.rs` |
| 84-210 | SSE helpers + streaming | `sse.rs` |
| 211-260 | Stream message response | `handlers/message.rs` |
| 261-300 | HttpState, ChannelRuntime, ListenTarget, etc. | `state.rs`, `listener.rs` |
| 302-366 | Token verification + auth middleware | `middleware.rs` |
| 368-466 | Tailscale detection | `tailscale.rs` |
| 468-494 | HttpError | `error.rs` |
| 496-530 | Token validation | `token.rs` |
| 531-620 | Channel runtime, EncodedImage, process helpers | `state.rs`, `types.rs`, `handlers/message.rs` |
| 623-662 | Router construction | `router.rs` |
| 665-850 | Handlers (message, webhook, health, config, status) | `handlers/*.rs` |
| 852-960 | Telegram photo helpers | `telegram/helpers.rs` |
| 961-1070 | Telegram polling | `telegram/polling.rs` |
| 1071-1125 | Telegram webhook handler | `telegram/webhook.rs` |
| 1126-1405 | Public run(), listener binding, startup | `lib.rs`, `listener.rs` |
| 1408-3482 | Tests (~2,074 lines, 40 tests) | Move with their modules |

### From `fx-cli/src/fleet_endpoints.rs` (444 lines)

All content moves to `fx-api/src/handlers/fleet.rs`. This file is deleted
from `fx-cli`.

---

## What Stays in `fx-cli`

After extraction, `fx-cli` retains:

```rust
// fx-cli/src/http_serve.rs (reduced to ~30 lines)

use fx_api::{ApiConfig, run};

pub async fn run(config: HttpConfig, app: HeadlessApp) -> anyhow::Result<()> {
    let api_config = ApiConfig::from_http_config(config, app);
    fx_api::run(api_config).await
}
```

Everything else — handlers, types, middleware, SSE, Telegram, fleet
endpoints, tests — lives in `fx-api`.

---

## `fx-api` Public API

The crate exposes a minimal public surface:

```rust
// fx-api/src/lib.rs

/// Configuration for starting the HTTP API server.
pub struct ApiConfig { ... }

/// Start the HTTP API server. Blocking until shutdown.
pub async fn run(config: ApiConfig) -> anyhow::Result<()> { ... }

// Re-exports for types that external code needs
pub use types::{MessageRequest, MessageResponse, HealthResponse, StatusResponse, ErrorBody};
pub use state::HttpState;
pub use tailscale::is_tailscale_ip;
```

---

## Cargo.toml Dependencies

`fx-api` takes the HTTP-related dependencies from `fx-cli`:

```toml
[package]
name = "fx-api"
version = "0.1.0"
edition = "2021"

[dependencies]
fx-kernel = { path = "../fx-kernel" }
fx-core = { path = "../fx-core" }
fx-config = { path = "../fx-config" }
fx-fleet = { path = "../fx-fleet" }
fx-llm = { path = "../fx-llm" }
fx-tools = { path = "../fx-tools" }
fx-channel-telegram = { path = "../fx-channel-telegram" }
fx-channel-webhook = { path = "../fx-channel-webhook" }
fx-storage = { path = "../fx-storage" }

axum = { version = "0.7", features = ["macros"] }
base64 = "0.22"
futures = "0.3"
http-body-util = "0.1"
hyper = "1"
reqwest = { version = "0.12", features = ["json"] }
ring = "0.17"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "net", "sync", "macros"] }
tower = "0.5"
tracing = "0.1"

[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
```

`fx-cli` removes these deps and adds:
```toml
fx-api = { path = "../fx-api" }
```

---

## HeadlessApp Dependency

The handlers need `HeadlessApp` to process messages. Currently `HeadlessApp`
lives in `fx-cli/src/headless.rs`.

**Options:**
1. Move `HeadlessApp` into `fx-kernel` — it's really the engine driver, not CLI-specific.
2. Keep `HeadlessApp` in `fx-cli` and pass it as `Arc<Mutex<HeadlessApp>>` into `fx-api::ApiConfig`.
3. Define a trait in `fx-api` that `HeadlessApp` implements, injected at startup.

**Recommended: Option 2** for Sprint 0. It's the least invasive — `HeadlessApp`
stays where it is, `fx-api` receives it as an opaque `Arc<Mutex<dyn AppEngine>>`
(or concrete type via generic). Moving `HeadlessApp` to `fx-kernel` is correct
architecturally but is a larger refactor that should be its own PR.

Actually, the simplest path: `fx-api` depends on `fx-cli`'s `HeadlessApp` type.
But that creates a circular dependency (fx-cli depends on fx-api, fx-api depends
on fx-cli).

**Resolution: Define a trait in fx-api.**

```rust
// fx-api/src/engine.rs

/// Trait abstracting the agentic engine for HTTP handlers.
///
/// `HeadlessApp` implements this in `fx-cli`. This breaks the circular
/// dependency: fx-api defines the interface, fx-cli provides the impl.
#[async_trait]
pub trait AppEngine: Send + Sync {
    /// Process a message and return the response.
    async fn process_message(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<CycleResult, anyhow::Error>;

    /// Get the currently active model identifier.
    fn active_model(&self) -> &str;

    /// Get the config manager, if available.
    fn config_manager(&self) -> Option<ConfigManagerHandle>;

    /// Clear conversation history.
    fn new_conversation(&mut self) -> anyhow::Result<String>;
}
```

`HttpState` holds `Arc<Mutex<dyn AppEngine>>`. `fx-cli` implements
`AppEngine` for `HeadlessApp` and passes it to `fx_api::run()`.

This is cleaner than a generic parameter on every handler and sets up
the right abstraction for Sprint 1 (session-scoped processing will add
methods to this trait).

---

## Migration Strategy

This is a **single PR** with a clean, verifiable diff:

### Step 1: Create crate skeleton
- `engine/crates/fx-api/Cargo.toml`
- `engine/crates/fx-api/src/lib.rs`
- Add to workspace `Cargo.toml`

### Step 2: Define `AppEngine` trait
- `engine/crates/fx-api/src/engine.rs`
- Methods mirror what handlers currently call on `HeadlessApp`

### Step 3: Move types and helpers (no handler logic yet)
- `types.rs`, `error.rs`, `sse.rs`, `middleware.rs`, `tailscale.rs`,
  `token.rs`, `state.rs`, `listener.rs`
- Each file compiles independently

### Step 4: Move handlers
- `handlers/message.rs`, `handlers/health.rs`, `handlers/config.rs`,
  `handlers/webhook.rs`, `handlers/fleet.rs`
- `telegram/polling.rs`, `telegram/webhook.rs`, `telegram/helpers.rs`

### Step 5: Move router and public run()
- `router.rs` — assembles routes from handlers
- `lib.rs` — public `run()` function, `ApiConfig` struct

### Step 6: Gut `fx-cli/src/http_serve.rs`
- Replace with thin wrapper calling `fx_api::run()`
- Delete `fx-cli/src/fleet_endpoints.rs`
- Implement `AppEngine` for `HeadlessApp`

### Step 7: Move tests
- Tests follow their source modules
- Verify test count matches: currently 42 tests (40 in http_serve + 2 in fleet)
- All 42 must pass after extraction

---

## Testing Requirements

### Verification (MANDATORY before merge)

1. **Test count parity**: `cargo test -p fx-api` must have ≥ 42 tests
   (40 from http_serve + 2 from fleet_endpoints).
2. **Zero test failures**: `cargo test --workspace` passes.
3. **Clippy clean**: `cargo clippy --workspace --tests -- -D warnings` passes.
4. **No behavior changes**: Same request → same response for every endpoint.
   Tests validate this implicitly (they test request/response contracts).
5. **Mac Mini build gate**: clippy clean on macOS.

### What NOT to test in Sprint 0

- No new endpoint tests
- No new types or handlers
- No refactoring of handler logic
- No changing function signatures (beyond trait adaptation)

---

## Scope Control

This spec is ONLY about moving code. Explicitly out of scope:

- ❌ New endpoints (Sprint 1+)
- ❌ `/v1/` prefix (Sprint 1)
- ❌ Session management (Sprint 1)
- ❌ Refactoring handler internals
- ❌ Moving `HeadlessApp` to `fx-kernel`
- ❌ Changing error types or response formats
- ❌ Telegram architecture changes
- ❌ Performance optimization

If the implementer discovers something that "should" be refactored,
they must NOT do it in this PR. Note it as a follow-up and move on.
The goal is a **clean, verifiable move** with zero surprises.

---

## File changes summary

| Action | File | Notes |
|--------|------|-------|
| CREATE | `engine/crates/fx-api/Cargo.toml` | New crate manifest |
| CREATE | `engine/crates/fx-api/src/lib.rs` | Public API |
| CREATE | `engine/crates/fx-api/src/engine.rs` | AppEngine trait |
| CREATE | `engine/crates/fx-api/src/state.rs` | HttpState, ChannelRuntime |
| CREATE | `engine/crates/fx-api/src/types.rs` | Request/response types |
| CREATE | `engine/crates/fx-api/src/router.rs` | Route tree |
| CREATE | `engine/crates/fx-api/src/middleware.rs` | Auth middleware |
| CREATE | `engine/crates/fx-api/src/sse.rs` | SSE helpers |
| CREATE | `engine/crates/fx-api/src/tailscale.rs` | Tailscale detection |
| CREATE | `engine/crates/fx-api/src/listener.rs` | TCP listener binding |
| CREATE | `engine/crates/fx-api/src/error.rs` | HttpError |
| CREATE | `engine/crates/fx-api/src/token.rs` | Bearer token validation |
| CREATE | `engine/crates/fx-api/src/handlers/mod.rs` | Handler module |
| CREATE | `engine/crates/fx-api/src/handlers/message.rs` | Message handler |
| CREATE | `engine/crates/fx-api/src/handlers/health.rs` | Health/status handlers |
| CREATE | `engine/crates/fx-api/src/handlers/config.rs` | Config handlers |
| CREATE | `engine/crates/fx-api/src/handlers/webhook.rs` | Webhook handler |
| CREATE | `engine/crates/fx-api/src/handlers/fleet.rs` | Fleet handlers |
| CREATE | `engine/crates/fx-api/src/telegram/mod.rs` | Telegram module |
| CREATE | `engine/crates/fx-api/src/telegram/polling.rs` | Telegram polling |
| CREATE | `engine/crates/fx-api/src/telegram/webhook.rs` | Telegram webhook |
| CREATE | `engine/crates/fx-api/src/telegram/helpers.rs` | Telegram helpers |
| REWRITE | `fx-cli/src/http_serve.rs` | Thin wrapper (~30 lines) |
| DELETE | `fx-cli/src/fleet_endpoints.rs` | Moved to fx-api |
| MODIFY | `fx-cli/src/headless.rs` | `impl AppEngine for HeadlessApp` |
| MODIFY | `fx-cli/Cargo.toml` | Remove HTTP deps, add fx-api |
| MODIFY | workspace `Cargo.toml` | Add fx-api to members |

---

## Estimated Size

- ~3,900 lines moved (http_serve + fleet_endpoints)
- ~100 lines new (AppEngine trait, Cargo.toml, module declarations, thin wrapper)
- ~3,900 lines deleted from fx-cli
- Net new code: ~100 lines (the trait + glue)
- Net lines: approximately zero (pure move)
