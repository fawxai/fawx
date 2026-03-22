# Spec: Native HTTPS via Tailscale TLS Certificates

## Problem
The Fawx server binds its Tailscale listener on plain HTTP. iOS enforces App Transport Security (ATS), which blocks cleartext HTTP to non-local addresses. Tailscale IPs (`100.x.x.x`) and MagicDNS hostnames are not considered "local" by iOS. This means the iOS app cannot connect to a remote Fawx server over Tailscale without an ATS exception (which Apple scrutinizes during App Store review).

## Solution
When TLS certificates exist at `~/.fawx/tls/{cert.pem,key.pem}`, the Tailscale listener upgrades to HTTPS automatically. Localhost stays HTTP (no cert needed for loopback). The cert generation flow already exists (`fawx setup` wizard + `tailscale cert` command). The server just needs to use the certs.

## Scope
- Rust: `fx-api` listener module + server startup
- Rust: `fx-config` for TLS config
- Swift (iOS): ATS exception for Tailscale IP ranges (fallback for HTTP)
- Swift (both): URL scheme handling for `https://`

## Design

### Behavior
1. On startup, check if `{data_dir}/tls/cert.pem` and `{data_dir}/tls/key.pem` exist
2. If both exist: bind the Tailscale listener with TLS using `axum-server` + `rustls`
3. If certs don't exist: bind Tailscale listener as plain HTTP (current behavior)
4. Localhost listener is always plain HTTP (never TLS)
5. Print startup banner showing `https://` or `http://` per listener
6. Report `https_enabled: true/false` in `/health` and `/v1/setup/status` responses

### Why This Approach
- Zero configuration: if certs exist, HTTPS activates. No config toggle needed.
- Backward compatible: missing certs means same behavior as today.
- Localhost stays HTTP: no need for certs on loopback, and the macOS app uses localhost.
- iOS ATS satisfied: valid Tailscale cert on the MagicDNS hostname passes ATS with no exceptions needed.

## Implementation

### 1. Add Dependencies (`engine/crates/fx-api/Cargo.toml`)

```toml
[dependencies]
axum-server = { version = "0.7", features = ["tls-rustls"] }
# axum-server provides axum integration with rustls for TLS termination
```

### 2. TLS Detection (`engine/crates/fx-api/src/listener.rs`)

Add a function to detect available TLS certs:

```rust
use std::path::{Path, PathBuf};

pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Check if TLS certificates exist in the data directory.
/// Returns `Some(TlsConfig)` if both cert.pem and key.pem are present.
pub fn detect_tls_config(data_dir: &Path) -> Option<TlsConfig> {
    let tls_dir = data_dir.join("tls");
    let cert_path = tls_dir.join("cert.pem");
    let key_path = tls_dir.join("key.pem");

    if cert_path.exists() && key_path.exists() {
        Some(TlsConfig { cert_path, key_path })
    } else {
        None
    }
}
```

### 3. TLS Listener (`engine/crates/fx-api/src/listener.rs`)

Add a TLS-enabled serve function alongside the existing plain one:

```rust
pub async fn serve_tls_listener(
    addr: SocketAddr,
    router: Router,
    tls_config: &TlsConfig,
    label: &'static str,
) -> anyhow::Result<()> {
    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
        &tls_config.cert_path,
        &tls_config.key_path,
    )
    .await
    .context("failed to load TLS certificates")?;

    axum_server::bind_rustls(addr, rustls_config)
        .serve(router.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .map_err(|e| anyhow::anyhow!("{label} HTTPS server error: {e}"))
}
```

Update `run_listener_pair` (or create a new variant) that uses TLS for the Tailscale listener while keeping the local listener on plain HTTP:

```rust
pub async fn run_listeners_with_tls(
    router: Router,
    listeners: BoundListeners,
    tls_config: Option<&TlsConfig>,
) -> anyhow::Result<()> {
    match (&listeners.tailscale, tls_config) {
        (Some(tailscale), Some(tls)) => {
            // Local = HTTP, Tailscale = HTTPS
            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
            let local_server = tokio::spawn(serve_listener_with_shutdown(
                listeners.local,
                router.clone(),
                shutdown_rx.clone(),
            ));
            let ts_addr = tailscale.target.addr;
            let ts_label = tailscale.target.label;
            let tls_cert = tls.cert_path.clone();
            let tls_key = tls.key_path.clone();
            let tls_conf = TlsConfig { cert_path: tls_cert, key_path: tls_key };
            // Drop the BoundListener (we rebind with TLS)
            drop(tailscale);
            let tailscale_server = tokio::spawn(async move {
                serve_tls_listener(ts_addr, router, &tls_conf, ts_label).await
            });
            // ... wait for either to exit, shutdown peer
        }
        _ => {
            // No TLS: current behavior
            run_listeners(router, listeners).await
        }
    }
}
```

Note: `axum_server::bind_rustls` creates its own listener, so you need the `SocketAddr` from the `BoundListener` but not its `TcpListener`. Either rebind with `axum_server`, or restructure to pass the address separately. The simplest approach: when TLS is enabled for Tailscale, skip binding the Tailscale `TcpListener` in `bind_listeners` and let `axum_server` handle it.

### 4. Wire Into Server Startup (`engine/crates/fx-api/src/lib.rs`)

In the `run()` function, detect TLS and pass it through:

```rust
let tls_config = detect_tls_config(&config.data_dir);
// ... existing setup ...
run_listeners_with_tls(router, listeners, tls_config.as_ref()).await?;
```

### 5. Update Startup Banner (`engine/crates/fx-api/src/listener.rs`)

Update `print_startup_targets` to accept TLS state:

```rust
pub fn print_startup_targets(listeners: &BoundListeners, tls_enabled: bool) {
    eprintln!("Fawx HTTP API listening on:");
    eprintln!(
        "  http://{} ({})",
        listeners.local.target.addr, listeners.local.target.label
    );
    match &listeners.tailscale {
        Some(listener) => {
            let scheme = if tls_enabled { "https" } else { "http" };
            eprintln!(
                "  {}://{} ({})",
                scheme, listener.target.addr, listener.target.label
            );
        }
        None => {
            eprintln!("  Tailscale not detected or unavailable; serving localhost only");
        }
    }
}
```

### 6. Update ServerRuntime (`engine/crates/fx-api/src/handlers/phase4.rs`)

The `ServerRuntime` struct already has an `https_enabled` field. Set it based on TLS detection:

```rust
let server_runtime = if tls_config.is_some() {
    ServerRuntime::local_https(config.port)
} else {
    ServerRuntime::local(config.port)
};
```

If `ServerRuntime::local_https` doesn't exist, add it (or just set the `https_enabled` field directly).

### 7. Update Health Response

Add `https_enabled` to the health response so the iOS app can detect the connection type. In `engine/crates/fx-api/src/handlers/health.rs`, update `HealthResponse`:

Currently not needed for the connection to work, but useful for diagnostics. This is optional/nice-to-have.

### 8. iOS ATS Configuration (`app/Fawx/Info-iOS.plist`)

Keep `NSAllowsLocalNetworking` for the macOS-local case. No additional ATS exceptions needed when the server uses HTTPS with a valid Tailscale cert (the Tailscale CA is trusted on devices running Tailscale).

Current (keep as-is):
```xml
<key>NSAppTransportSecurity</key>
<dict>
    <key>NSAllowsLocalNetworking</key>
    <true/>
</dict>
```

**Important:** Tailscale certs are issued by a Let's Encrypt CA, which iOS trusts natively. No custom CA trust configuration needed.

### 9. iOS URL Handling

The iOS `canonicalizeServerURL` and `FawxClient` should already handle `https://` URLs. Verify that the URL input field on the onboarding screen doesn't force or strip the scheme. If the user enters a hostname without a scheme, default to `https://` (not `http://`).

Check `canonicalizeServerURL` in `SettingsViewModel.swift` and ensure:
- Input without scheme defaults to `https://`
- `https://` URLs are preserved as-is
- `http://` URLs are preserved for localhost only

## Files Changed

| File | Change |
|------|--------|
| `engine/crates/fx-api/Cargo.toml` | Add `axum-server` with `tls-rustls` feature |
| `engine/crates/fx-api/src/listener.rs` | Add `TlsConfig`, `detect_tls_config()`, `serve_tls_listener()`, update `run_listeners` and `print_startup_targets` |
| `engine/crates/fx-api/src/lib.rs` | Detect TLS config, pass to listener, update `ServerRuntime` |
| `app/Fawx/Info-iOS.plist` | Revert `NSAllowsArbitraryLoads` (keep only `NSAllowsLocalNetworking`) |
| `app/Fawx/ViewModels/SettingsViewModel.swift` | Default scheme to `https://` for non-localhost URLs |

## Testing

### Unit Tests
1. `detect_tls_config` returns `None` when certs don't exist
2. `detect_tls_config` returns `Some` when both cert.pem and key.pem exist
3. `detect_tls_config` returns `None` when only one file exists
4. `print_startup_targets` shows `https://` when TLS enabled
5. `print_startup_targets` shows `http://` when TLS disabled
6. URL canonicalization defaults to `https://` for non-localhost

### Integration Tests (Manual)
1. Start server without certs: Tailscale listener serves HTTP
2. Run `fawx cert` to generate certs, restart server: Tailscale listener serves HTTPS
3. iOS app connects via `https://joes-mac-mini-2.tail9696fb.ts.net:8400`
4. iOS app connects via `https://100.123.20.63:8400`
5. macOS app still connects via `http://127.0.0.1:8400` (unaffected)
6. Health check passes on iOS without ATS warnings

## Notes

- Tailscale certs auto-renew, but Fawx would need a restart to pick up new certs. Future improvement: watch for cert file changes and hot-reload the TLS config.
- `axum-server` is the standard choice for adding TLS to axum. It wraps `hyper` + `rustls` and provides the same `serve()` API.
- The cert files at `~/.fawx/tls/` are already generated by the setup wizard when Tailscale is detected. This spec only adds the server-side consumption of those certs.
- Port stays the same (8400). No separate HTTPS port needed.
