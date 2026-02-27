# Codex OAuth Bridge API Spec

Status: Draft (implemented by Fawx Android client)  
Last updated: 2026-02-11

## 1. Purpose

This document defines the HTTP API contract between:

- Fawx Android client (`chat` module)
- A local/remote OAuth bridge service that starts OpenAI browser login and exchanges callback codes for OAuth tokens

The bridge exists because Android client code should not embed provider OAuth client secrets or custom app-server logic.

## 2. Base URL

Bridge base URL is configurable in the Fawx UI.

- Default: `http://127.0.0.1:4318`
- Android app expects JSON over HTTP(S)

Reference implementation is available in this repo via:

- `cargo run -p ct-cli -- oauth-bridge`
- source: `crates/ct-cli/src/commands/oauth_bridge.rs`

Configuration (CLI flag or env var):

- `--auth-url` or `FAWX_OPENAI_AUTH_URL`
- `--token-url` or `FAWX_OPENAI_TOKEN_URL`
- `--client-id` or `FAWX_OPENAI_CLIENT_ID`
- `--client-secret` or `FAWX_OPENAI_CLIENT_SECRET` (optional)
- `--scope` or `FAWX_OPENAI_SCOPE` (optional)

## 3. Endpoint Discovery Order

Fawx tries endpoints in this exact order and stops on first success:

### 3.1 Start Login

1. `POST /oauth/codex/start`
2. `POST /oauth/start`
3. `POST /account/login/start`

### 3.2 Exchange Code

1. `POST /oauth/codex/exchange`
2. `POST /oauth/exchange`
3. `POST /account/login/exchange`

Any non-2xx response is treated as a failed attempt and the next path is tried.

## 4. Start Login API

### 4.1 Request

`POST {baseUrl}/...start` with JSON body:

```json
{
  "redirect_uri": "fawx://oauth/callback",
  "redirectUri": "fawx://oauth/callback",
  "state": "random-state"
}
```

Notes:

- `redirect_uri` and `redirectUri` are both sent for compatibility.
- `state` is generated client-side and validated on callback.

### 4.2 Response

The response MUST contain an auth URL in any accepted key alias:

- `authUrl`, `auth_url`, `url`, `loginUrl`, `login_url`

Optional fields:

- `loginId`, `login_id`, `requestId`, `request_id`, `id`
- `codeVerifier`, `code_verifier`

Wrapper objects are supported:

- top-level payload
- `result.{...}`
- `data.{...}`
- `payload.{...}`
- `session.{...}`

Example:

```json
{
  "result": {
    "auth_url": "https://auth.openai.com/oauth2/authorize?...",
    "login_id": "req_123",
    "code_verifier": "pkce_verifier_here"
  }
}
```

## 5. Exchange Code API

### 5.1 Request

`POST {baseUrl}/...exchange` with JSON body:

```json
{
  "code": "authorization_code_from_callback",
  "state": "same-state-from-start",
  "login_id": "req_123",
  "loginId": "req_123",
  "code_verifier": "pkce_verifier_here",
  "codeVerifier": "pkce_verifier_here"
}
```

Only `code` is always required by client logic. Other fields are sent when available.

### 5.2 Response

Response MUST contain token in any accepted key alias:

- `accessToken`, `access_token`, `token`, `oauthToken`, `oauth_token`

Same wrapper objects as section 4.2 are supported.

Example:

```json
{
  "data": {
    "access_token": "sess-..."
  }
}
```

## 6. OAuth Callback Contract

Fawx Android deep link:

- `fawx://oauth/callback`

Bridge/provider redirect should include:

- `state` (required for CSRF validation)
- either:
  - `code` (preferred, exchanged via bridge endpoint), or
  - direct token (`token`, `access_token`, `accessToken`, `oauth_token`, `oauthToken`)

Error callback handling:

- `error`
- optional `error_description`

Fragment parameters (`#access_token=...`) and query parameters are both supported by client parsing.

## 7. Error Semantics

- Non-2xx: attempt failure, client tries next path
- 2xx with empty or invalid JSON: attempt failure
- 2xx without required response key (`authUrl` for start, token for exchange): treated as failure
- If all paths fail, client surfaces aggregated error context to UI

## 8. Security Requirements

Bridge implementation SHOULD:

- enforce one-time authorization code usage
- enforce short code/token TTL
- validate redirect URI allowlist
- bind `state` to login session and verify on exchange
- use PKCE when upstream provider supports it
- avoid logging full tokens/codes in plaintext
- use HTTPS for non-local deployments

Fawx client currently validates `state` on callback and stores resulting token in app prefs.

## 9. Field Naming Convention

**Canonical casing:** camelCase (e.g., `authUrl`, `loginId`, `codeVerifier`)

**Legacy support:** The client also sends/accepts snake_case aliases (e.g., `auth_url`, `login_id`, `code_verifier`) for backward compatibility with bridge implementations that use snake_case.

**Implementation note:** When sending dual-cased fields (both `loginId` and `login_id`), the client includes both for maximum compatibility. Bridge implementations should accept either, but are encouraged to use camelCase in responses.

## 10. Compatibility Matrix

| Area | Primary | Accepted aliases |
|---|---|---|
| Start URL | `authUrl` | `auth_url`, `url`, `loginUrl`, `login_url` |
| Login ID | `loginId` | `login_id`, `requestId`, `request_id`, `id` |
| PKCE verifier | `codeVerifier` | `code_verifier` |
| Exchange token | `accessToken` | `access_token`, `token`, `oauthToken`, `oauth_token` |
| Response wrapper | top-level | `result`, `data`, `payload`, `session` |

## 11. Reference

Client implementation:

- `android/core/src/main/kotlin/ai/fawx/core/CodexOauthBridgeClient.kt`
- `android/chat/src/main/kotlin/ai/fawx/chat/ChatActivity.kt`
