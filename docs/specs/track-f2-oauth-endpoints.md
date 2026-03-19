# Track F-2: OpenAI PKCE OAuth HTTP Endpoints

**Status:** SPEC
**Priority:** High — enables ChatGPT subscription login from Swift app
**Endpoints:** GET `/v1/auth/{provider}/oauth-start`, POST `/v1/auth/{provider}/oauth-callback`

---

## Overview

Expose the existing `PkceFlow` from fx-auth over HTTP so the Swift app can drive OpenAI PKCE OAuth natively using `ASWebAuthenticationSession`.

The CLI already has full PKCE logic. This PR wraps it in HTTP endpoints.

---

## Architecture

1. **Start:** Client calls GET `/v1/auth/openai/oauth-start`
   - Server creates PkceFlow, stores it keyed by a flow_token
   - Returns authorize_url + flow_token to client
   - Client opens authorize_url in ASWebAuthenticationSession

2. **Callback:** After user authorizes, client receives auth code via fawx-auth:// callback
   - Client calls POST `/v1/auth/openai/oauth-callback` with code + flow_token
   - Server looks up PkceFlow by flow_token
   - Server exchanges code for tokens via OpenAI token endpoint
   - Server stores access token in credential store
   - Returns success status

---

## Endpoints

### GET /v1/auth/{provider}/oauth-start

Creates a new PKCE flow. Currently only `openai` is supported.

Response 200:
```json
{
  "provider": "openai",
  "authorize_url": "https://auth.openai.com/oauth/authorize?...",
  "flow_token": "oauth_flow_abc123",
  "redirect_uri": "fawx-auth://openai/callback"
}
```

Response 400 (unsupported provider):
```json
{
  "error": "OAuth not supported for provider 'anthropic'"
}
```

Implementation:
1. Validate provider == "openai" (only supported provider for now)
2. Create PkceFlow::try_new()
3. Generate a random flow_token (hex string)
4. Override redirect_uri to "fawx-auth://openai/callback" for the Swift app
5. Store (flow_token → PkceFlow) in an in-memory map with TTL
6. Return authorize_url + flow_token

### POST /v1/auth/{provider}/oauth-callback

Completes the OAuth flow.

Request:
```json
{
  "code": "auth_code_from_redirect",
  "flow_token": "oauth_flow_abc123"
}
```

Response 200:
```json
{
  "provider": "openai",
  "status": "authenticated",
  "auth_method": "oauth",
  "verified": true
}
```

Response 400 (invalid/expired flow):
```json
{
  "error": "Invalid or expired flow token"
}
```

Response 502 (token exchange failed):
```json
{
  "error": "Token exchange failed: invalid_grant"
}
```

Implementation:
1. Look up PkceFlow by flow_token, remove from map
2. Build TokenExchangeRequest with the code + verifier
3. POST to OpenAI token endpoint (reqwest)
4. On success, store access_token in credential store under "openai"
5. Extract account_id from JWT if possible
6. Return success

---

## State Management

Add an `OAuthFlowStore` to HttpState:

```rust
pub struct OAuthFlowStore {
    flows: std::sync::Mutex<HashMap<String, StoredFlow>>,
}

struct StoredFlow {
    flow: PkceFlow,
    created_at: Instant,
}
```

- Flow tokens expire after 10 minutes
- Clean up expired flows on each lookup
- Max 10 concurrent flows (prevent DoS)

---

## Redirect URI

The redirect URI must be `fawx-auth://openai/callback` for the Swift app's `ASWebAuthenticationSession`. However, PkceFlow currently defaults to `http://localhost:1455/auth/callback`.

Two options:
1. Add a `PkceFlow::with_redirect_uri()` builder method to fx-auth
2. Build the authorize_url manually with the correct redirect_uri

**Preferred:** Option 1 — add `with_redirect_uri(uri: &str) -> Self` to PkceFlow.

---

## Files to Create/Modify

1. **NEW: `engine/crates/fx-api/src/handlers/oauth.rs`** — handlers + OAuthFlowStore
2. **MODIFY: `engine/crates/fx-api/src/handlers/mod.rs`** — add `pub mod oauth;`
3. **MODIFY: `engine/crates/fx-api/src/router.rs`** — add routes
4. **MODIFY: `engine/crates/fx-api/src/state.rs`** — add OAuthFlowStore to HttpState
5. **MODIFY: `engine/crates/fx-auth/src/oauth.rs`** — add with_redirect_uri() to PkceFlow

---

## Tests Required

1. `start_creates_flow_and_returns_url` — GET returns valid authorize_url
2. `start_rejects_unsupported_provider` — GET with "anthropic" returns 400
3. `callback_rejects_invalid_flow_token` — POST with bad token returns 400
4. `callback_rejects_expired_flow` — POST after TTL returns 400
5. `flow_store_enforces_max_concurrent` — 11th flow creation fails
6. `flow_store_cleans_expired_on_lookup` — expired flows removed
7. `pkce_flow_with_custom_redirect_uri` — PkceFlow builder test
8. Serialization tests for request/response types

---

## Acceptance Criteria

- GET /v1/auth/openai/oauth-start returns valid PKCE authorize URL
- POST /v1/auth/openai/oauth-callback exchanges code for tokens
- Flow tokens have 10-minute TTL
- Token storage uses credential store (not config file)
- No secrets in responses or logs
- All existing tests pass, clippy clean
