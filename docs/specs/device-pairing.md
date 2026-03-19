# Device Pairing Flow

**Status:** Specifying  
**Branch:** `feat/device-pairing`  
**PR target:** `dev`

---

## Problem

The Fawx HTTP API requires a bearer token for authentication. Currently the token is encrypted into `auth.db` during `fawx setup`, making it impossible to retrieve after the fact. The Swift app needs this token to connect, but the user has no way to get it.

Asking users to copy-paste bearer tokens is bad UX. We need a pairing flow.

---

## Design

### User Experience

**Server side (terminal):**
```
$ fawx pair
╭─────────────────────────────────╮
│  Pairing code:  A7K-M2X         │
│  Expires in 5 minutes           │
│  Waiting for device to pair...  │
╰─────────────────────────────────╯
✓ Device "Joe's MacBook" paired successfully.
```

**App side (SwiftUI onboarding):**
1. Screen 1: Enter server URL (`http://100.123.20.63:8400`)
2. Screen 2: Enter pairing code (`A7K-M2X`)
3. App calls `POST /v1/pair` with code → gets bearer token
4. Token stored in Keychain → connected

### Pairing Code

- Format: `XXX-XXX` (6 alphanumeric chars, case-insensitive, hyphen for readability)
- Character set: `ABCDEFGHJKLMNPQRSTUVWXYZ2345679` (no 0/O/1/I/l confusion)
- Entropy: 30^6 ≈ 729 million combinations (plenty for a 5-minute window on a private network)
- Single-use: consumed on first successful exchange
- TTL: 5 minutes from generation
- Max attempts: 5 wrong codes per IP per code (prevent brute force, though Tailscale makes this unlikely)

### API

#### Generate Pairing Code (CLI → running server)

The `fawx pair` command talks to the running `fawx serve` instance via its HTTP API. This requires the CLI to have local access (localhost or Tailscale).

**Internal endpoint** (authenticated — requires existing bearer token or localhost):
```
POST /v1/pair/generate
Body: { "device_name": null }  // optional, set by app later
Response: {
  "code": "A7K-M2X",
  "expires_at": 1773436000,
  "ttl_seconds": 300
}
```

Alternatively, `fawx pair` can communicate via a Unix socket or shared file to avoid the auth chicken-and-egg. Simplest approach: **localhost requests bypass bearer auth** (already the case for health checks in many systems).

#### Exchange Pairing Code for Token (unauthenticated)

```
POST /v1/pair
Body: {
  "code": "A7KM2X",      // stripped of hyphens, uppercased
  "device_name": "Joe's MacBook Pro"
}
Response (success): {
  "token": "fawx_pat_...",
  "device_id": "dev-a1b2c3d4",
  "expires_at": null       // persistent token, no expiry
}
Response (failure): {
  "error": "invalid_code",
  "message": "Pairing code is invalid or expired."
}
```

**This endpoint is unauthenticated** — it's the bootstrap mechanism. Security relies on:
- Short-lived codes (5 min)
- Single-use (consumed on exchange)
- Tailscale network (not public internet)
- Rate limiting (5 attempts per code)

### Token Generation

The exchanged token is a new concept: **device token** (prefix `fawx_pat_`).

```rust
/// A paired device token.
pub struct DeviceToken {
    pub id: String,           // "dev-{uuid}"
    pub token_hash: String,   // SHA-256 hash of the raw token (we never store raw)
    pub device_name: String,  // "Joe's MacBook Pro"
    pub created_at: u64,
    pub last_used_at: u64,
}
```

- Raw token format: `fawx_pat_{32-char-random}` (e.g., `fawx_pat_a7b2c9d4e5f6...`)
- Server stores only the SHA-256 hash in `devices.json` or redb
- On each API request: hash the incoming bearer token, compare to stored hashes
- Multiple devices can be paired simultaneously (each gets its own token)

### Pairing State

```rust
/// Ephemeral pairing state (in-memory only, not persisted).
pub struct PairingState {
    /// Active pairing codes: code → metadata
    codes: HashMap<String, PendingPair>,
}

pub struct PendingPair {
    pub code: String,
    pub expires_at: Instant,
    pub attempts: u32,
    pub max_attempts: u32,
}
```

- Stored in-memory only (lost on restart — codes are ephemeral)
- Cleanup: expired codes removed on each generate/exchange call
- No persistence needed — if server restarts, user just runs `fawx pair` again

### Auth Flow (updated)

Current: single bearer token checked against encrypted credential store.

New: **multi-source auth**:
1. Check incoming `Authorization: Bearer <token>` against:
   a. Legacy bearer token from credential store (backward compat)
   b. Device token hashes from `devices.json`/redb
2. If either matches → authenticated
3. Unauthenticated endpoints: `GET /health`, `POST /v1/pair`

### CLI: `fawx pair`

```
fawx pair [--ttl <seconds>] [--json]
```

- Connects to the running `fawx serve` instance
- Generates a pairing code via `POST /v1/pair/generate` (localhost, bypasses auth)
- Displays the code with countdown timer
- Polls for pairing completion (or uses SSE/websocket for instant feedback)
- On success: prints device name
- On timeout: prints "Code expired. Run `fawx pair` again."

### CLI: `fawx devices`

```
fawx devices list              # List paired devices
fawx devices revoke <device-id> # Revoke a device token
```

---

## Implementation

### Files

| File | Change |
|------|--------|
| `engine/crates/fx-api/src/handlers/pairing.rs` (NEW) | Pairing handlers: generate + exchange |
| `engine/crates/fx-api/src/pairing.rs` (NEW) | `PairingState`, `PendingPair`, code generation |
| `engine/crates/fx-api/src/devices.rs` (NEW) | `DeviceToken`, `DeviceStore`, token hashing |
| `engine/crates/fx-api/src/router.rs` | Add pairing routes |
| `engine/crates/fx-api/src/middleware.rs` | Update auth middleware for multi-source |
| `engine/crates/fx-cli/src/commands/pair.rs` (NEW) | `fawx pair` CLI command |
| `engine/crates/fx-cli/src/commands/devices.rs` (NEW) | `fawx devices` CLI command |
| `engine/crates/fx-cli/src/commands/mod.rs` | Register new commands |

Estimated: ~500 lines production code.

### Tests

1. `generate_code_format_valid` — code matches XXX-XXX pattern, valid charset
2. `exchange_valid_code_returns_token` — happy path
3. `exchange_expired_code_fails` — TTL enforcement
4. `exchange_consumed_code_fails` — single-use enforcement
5. `exchange_wrong_code_increments_attempts` — brute force protection
6. `exchange_max_attempts_locks_code` — rate limiting
7. `device_token_auth_succeeds` — token hash comparison works
8. `legacy_token_still_works` — backward compatibility
9. `revoke_device_invalidates_token` — device revocation
10. `concurrent_pairing_independent` — multiple codes simultaneously

---

## Swift App Changes

The onboarding flow changes from:
```
Server URL → Bearer Token → Test Connection → Done
```
To:
```
Server URL → Health Check → Pairing Code → Exchange → Done
```

1. User enters server URL
2. App calls `GET /health` to verify server is reachable
3. App shows "Enter pairing code" screen with instructions: "Run `fawx pair` on your server"
4. User enters 6-char code
5. App calls `POST /v1/pair` with code + device name (from `UIDevice.current.name` or `Host.current().localizedName`)
6. On success: store token in Keychain, navigate to main app
7. On failure: show error, let user retry

The Settings → Connection screen also changes:
- Shows "Paired as: Joe's MacBook Pro" instead of a bearer token field
- "Unpair" button (deletes local token, returns to onboarding)

---

## Non-Goals (V1)

- **QR code pairing** — nice to have, but manual code entry works fine for V1
- **mDNS discovery** — auto-finding the server on the network. V2.
- **Token refresh/rotation** — device tokens don't expire in V1
- **Mutual TLS** — Tailscale handles transport security
