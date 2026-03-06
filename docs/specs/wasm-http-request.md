# WASM Host API: http_request (#1138)

## Problem

The `HostApi` trait has no `http_request` method. Skills declaring `capabilities = ["network"]` import `host_api_v1::http_request` via FFI but the host never provides it. This blocks GitHubSkill and any network-capable WASM skill.

## FFI Contract (already in GitHubSkill)

```rust
#[link(wasm_import_module = "host_api_v1")]
extern "C" {
    #[link_name = "http_request"]
    fn host_http_request(
        method_ptr: *const u8, method_len: u32,
        url_ptr: *const u8, url_len: u32,
        headers_ptr: *const u8, headers_len: u32,
        body_ptr: *const u8, body_len: u32,
    ) -> u32; // returns ptr to NUL-terminated response string, 0 on failure
}
```

## Changes Required

### 1. `engine/crates/fx-skills/src/host_api.rs`

Add to `HostApi` trait:
```rust
fn http_request(&self, method: &str, url: &str, headers: &str, body: &str) -> Option<String>;
```

Add to `MockHostApi`:
- Store a `Vec<(String, String)>` of canned responses (URL pattern → response)
- Return matching response or None

### 2. `engine/crates/fx-skills/Cargo.toml`

Add `ureq` dependency (lightweight, blocking HTTP — no tokio needed):
```toml
ureq = { version = "2", features = ["json"] }
```

### 3. `engine/crates/fx-loadable/src/wasm_host.rs`

Implement `http_request` in `LiveHostApi`:
- Parse headers JSON string into header map
- Make HTTP request via `ureq`
- Timeout: 30 seconds
- Max response body: 1MB
- Return response body as String, or None on error
- Log request method+url at info level, errors at error level

### 4. `engine/crates/fx-skills/src/runtime.rs`

Add to `link_host_functions()`:
```rust
// host_api_v1::http_request(method_ptr, method_len, url_ptr, url_len, 
//                           headers_ptr, headers_len, body_ptr, body_len) -> u32
linker.func_wrap(
    "host_api_v1",
    "http_request",
    |mut caller: Caller<'_, HostState>,
     method_ptr: u32, method_len: u32,
     url_ptr: u32, url_len: u32,
     headers_ptr: u32, headers_len: u32,
     body_ptr: u32, body_len: u32| -> u32 {
        // Read strings from WASM memory
        // Call host_api.http_request(method, url, headers, body)
        // Write response to WASM memory, return pointer
        // Return 0 on failure
    },
)?;
```

Follow the same pattern as `kv_get` for reading strings from WASM memory and writing response back (allocate in WASM linear memory, write NUL-terminated string, return pointer).

### 5. Capability gating

In `LiveHostApi::http_request`, check that the skill has `Capability::Network`. If not, log an error and return None. This requires storing the skill's capabilities in `LiveHostApi` (add a `capabilities: Vec<Capability>` field, passed in via `LiveHostApiConfig`).

## Tests Required

1. `MockHostApi` returns canned response for matching URL
2. `MockHostApi` returns None for unmatched URL  
3. Trait method exists and compiles (compile-time check)
4. WASM integration test: skill calling http_request gets response
5. Capability gating: skill without network capability gets None
6. Response size limit: truncate at 1MB
7. Header parsing: valid JSON headers → request headers
8. Header parsing: invalid JSON → error/None

## Security

- HTTPS only for V1 (reject http:// URLs)
- 30s timeout
- 1MB response limit
- No capability = no access
- Log all requests at info level for audit trail

## Branch

`feat/wasm-http-request-1138` from `origin/staging`
