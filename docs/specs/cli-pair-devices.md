# CLI Commands: `fawx pair` + `fawx devices`

**Status:** Ready for implementation  
**Branch:** `feat/cli-pair-devices`  
**PR target:** `dev`  
**Depends on:** PR #1397 (device pairing backend) — merged to dev

---

## Overview

Two CLI commands that complete the device pairing story:
1. `fawx pair` — generate a pairing code on a running server, display it, wait for exchange
2. `fawx devices` — list and revoke paired devices

Both talk to the running `fawx serve --http` instance over localhost.

---

## `fawx pair`

### Usage

```
fawx pair [--ttl <seconds>] [--json]
```

### Behavior

1. Read `RuntimeLayout` to get HTTP port and bearer token
2. Call `POST http://127.0.0.1:{port}/v1/pair/generate` with bearer auth
3. Display the pairing code with a countdown timer
4. Poll `GET /health` to keep the display alive (optional — the code is single-use, consumed on exchange)
5. Exit with success message when the user presses Ctrl+C or the code expires

### Display

```
╭───────────────────────────────────╮
│                                   │
│   Pairing code:  A7K-M2X         │
│   Expires in 4:52                 │
│                                   │
│   Enter this code in the Fawx     │
│   app to connect this device.     │
│                                   │
╰───────────────────────────────────╯

Waiting for device to pair... (Ctrl+C to cancel)
```

The countdown should update every second (overwrite the line). When the code expires:

```
Code expired. Run `fawx pair` again to generate a new code.
```

### JSON mode

```
fawx pair --json
```

Output:
```json
{
  "code": "A7K-M2X",
  "expires_at": 1773436000,
  "ttl_seconds": 300
}
```

No countdown, no interactive display. For scripting.

### Error cases

- Server not running → "Fawx server is not running. Start it with `fawx serve --http`"
- No bearer token configured → "No authentication configured. Run `fawx setup` first."
- Server returns error → print the error message

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--ttl` | 300 | Code TTL in seconds (passed to generate endpoint) |
| `--json` | false | Machine-readable JSON output |

---

## `fawx devices`

### Usage

```
fawx devices [list|revoke <device-id>] [--json]
```

Default subcommand is `list`.

### `fawx devices list`

Calls a new `GET /v1/devices` endpoint (needs to be added — reads `DeviceStore` and returns device list).

```
Paired Devices:

  ID                              Name              Paired          Last Used
  dev-a1b2c3d4e5f6...             Joe's MacBook     2 hours ago     5 min ago
  dev-f7e8d9c0b1a2...             Joe's iPhone      3 days ago      1 day ago

2 devices paired.
```

### `fawx devices revoke`

Calls a new `DELETE /v1/devices/{device-id}` endpoint.

```
$ fawx devices revoke dev-a1b2c3d4e5f6
✓ Device "Joe's MacBook" revoked. Token is no longer valid.
```

### JSON mode

```json
{
  "devices": [
    {
      "id": "dev-a1b2c3d4e5f6...",
      "device_name": "Joe's MacBook",
      "created_at": 1773400000,
      "last_used_at": 1773435000
    }
  ]
}
```

---

## Backend additions needed

Two new endpoints in `fx-api`:

### `GET /v1/devices` (authenticated)

Returns the device list from `DeviceStore`. Response:
```json
{
  "devices": [
    {
      "id": "dev-...",
      "device_name": "...",
      "created_at": 1773400000,
      "last_used_at": 1773435000
    }
  ]
}
```

Note: `token_hash` is NEVER included in the response.

### `DELETE /v1/devices/{id}` (authenticated)

Revokes a device token. Response:
```json
{ "revoked": true, "device_id": "dev-..." }
```

404 if device not found.

---

## Files

### New files
| File | Purpose |
|------|---------|
| `engine/crates/fx-cli/src/commands/pair.rs` | `fawx pair` command |
| `engine/crates/fx-cli/src/commands/devices.rs` | `fawx devices` command |
| `engine/crates/fx-api/src/handlers/devices.rs` | `GET /v1/devices` + `DELETE /v1/devices/{id}` handlers |

### Modified files
| File | Change |
|------|--------|
| `engine/crates/fx-cli/src/main.rs` | Add `Pair` and `Devices` to `Commands` enum, wire dispatch |
| `engine/crates/fx-cli/src/commands/mod.rs` | Add `pub mod pair; pub mod devices;` |
| `engine/crates/fx-api/src/router.rs` | Add device list/revoke routes |
| `engine/crates/fx-api/src/handlers/mod.rs` | Add `pub mod devices;` |
| `engine/crates/fx-api/src/devices.rs` | Add `DeviceInfo` (serializable without token_hash) and `list_device_info()` method |

### Estimated size
~300 lines production code, ~100 lines tests.

---

## Tests

### CLI tests (fx-cli)
- `pair_json_output_format` — verify JSON mode output structure
- `pair_requires_running_server` — error message when server down
- `devices_list_json_format` — verify JSON device list structure

### API tests (fx-api)
- `get_devices_requires_auth` — 401 without token
- `get_devices_returns_device_list` — list devices after pairing
- `get_devices_excludes_token_hash` — token_hash never in response
- `delete_device_revokes_token` — revoke and verify auth fails
- `delete_device_not_found` — 404 for unknown device ID
- `delete_device_requires_auth` — 401 without token

---

## Implementation notes

- Reuse `http_client()` and `RuntimeLayout` patterns from `status.rs`
- Countdown timer: `tokio::time::interval(Duration::from_secs(1))` loop with `\r` overwrite
- For the box drawing, use simple `println!` with Unicode box chars (no external TUI dep)
- `DeviceInfo` is a separate struct from `DeviceToken` — never exposes `token_hash`
- Bearer token for localhost comes from `layout.config.http.bearer_token`
