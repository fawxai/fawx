# Spec: HTTP Server Startup Without Configured Providers

## Problem

On a fresh install, no AI provider is configured. `fawx serve --http` calls
`HeadlessApp::new()` which calls `resolve_active_model()` → `first_runtime_model()`
→ returns `Err("no models available in router")`. The `?` propagates the error,
the server exits, launchd restarts it, and it crash-loops forever.

The Swift app cannot connect (nothing listening on port 8400), so the user sees
"Server disconnected" and cannot complete the setup wizard to add credentials.

## Root Cause

`headless.rs` line ~320:
```rust
let active_model = resolve_active_model(&deps.router, &deps.config)?;
```

The `?` makes this a hard failure. This check is correct for `fawx serve`
(stdin/stdout chat mode — you need a model to chat), but wrong for
`fawx serve --http` where the server must start to serve the HTTP API
(health, setup, credential management, bootstrap) before any provider exists.

## Required Fix

### 1. Make `active_model` gracefully degrade to empty string

In `HeadlessApp::new()`, change:
```rust
let active_model = resolve_active_model(&deps.router, &deps.config)?;
```
to:
```rust
let active_model = resolve_active_model(&deps.router, &deps.config)
    .unwrap_or_default();
```

`String::default()` is `""`, which is safe — no downstream code panics on
an empty `active_model`. The chat paths (`process_message`, `run_cycle`)
already go through `RouterLoopLlmProvider` which will error naturally when
the empty model string doesn't match any registered provider.

### 2. Guard the `seed_runtime_info` path

`seed_runtime_info()` calls `self.router.provider_for_model(&self.active_model)`.
With an empty model, this returns `None` / `""` — already handled via
`.unwrap_or("")`. No change needed.

### 3. Guard `apply_http_defaults`

`apply_http_defaults()` is called after `new()` in the HTTP path. It calls
`router.set_active(selector)` which already handles failures with a warning log.
It then updates `self.active_model` only if `router.active_model()` returns
`Some(...)`. With no providers, it returns `None` and the method is a no-op.
No change needed.

### 4. Keep non-HTTP serve path strict

`run_headless()` (the stdin/stdout path) should still fail fast if no model is
available. The change in `HeadlessApp::new()` affects both paths, so add a
post-construction check in `run_headless()`:

```rust
if app.active_model().is_empty() {
    return Err(no_headless_models_available().into());
}
```

This preserves the existing behavior: `fawx serve` (no --http) still fails
immediately with a clear error message.

### 5. Add a startup warning for HTTP mode

When the server starts with no providers, log a WARN and add it to
`startup_warnings` so the API can surface it:

```rust
if active_model.is_empty() {
    tracing::warn!(
        "no AI providers configured; HTTP API is available but chat is disabled until a provider is added"
    );
}
```

## What must NOT change

- `fawx serve` (non-HTTP) must still fail fast with the existing error message
- If a provider IS configured at startup, `active_model` must resolve to a real
  model exactly as before — the `unwrap_or_default()` only fires when the
  `Result` is `Err`
- `reload_config()` / `sync_headless_model_from_config()` already handle
  updating `active_model` when credentials are added later via SIGHUP — no
  changes needed there
- Chat endpoints should return a proper error (not panic) when invoked with
  no configured provider

## Files to Modify

1. `engine/crates/fx-cli/src/headless.rs` — `HeadlessApp::new()`, add warning
2. `engine/crates/fx-cli/src/main.rs` — add guard in `run_headless()` after construction

## Tests

1. **Regression test**: `HeadlessApp::new()` succeeds with an empty router (no
   providers). Verify `active_model()` returns `""`.
2. **Regression test**: `HeadlessApp::new()` succeeds with a configured provider.
   Verify `active_model()` returns the expected model ID (existing behavior).
3. **Unit test for `run_headless` guard**: verify that the non-HTTP path still
   rejects empty routers.

## Verification

```bash
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

After push, verify: `git log --oneline origin/<branch> -3`
If your commit is not visible, the push failed — do not report success.
