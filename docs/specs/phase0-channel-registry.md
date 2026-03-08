# Spec: Phase 0 PR 5 — ChannelRegistry + ResponseRouter Wiring

**Gap:** Channel abstraction built but never instantiated  
**Estimated size:** ~400 lines  
**Risk:** Medium — refactors message flow in HTTP server  
**Depends on:** PRs 1-4 (no code dep, but should land after to reduce merge conflicts)

---

## Problem

`fx-kernel/src/channels.rs` defines:
- `ChannelRegistry` — tracks registered input/output channels
- `ResponseRouter` — routes kernel responses to originating channel

`fx-channel-telegram/` implements the Channel trait for Telegram.
`fx-channel-webhook/` implements the Channel trait for generic webhooks.

None of these are used. Telegram is wired directly in `http_serve.rs` with 
hardcoded `Arc<TelegramChannel>` references. This means:
- Adding new channels requires modifying http_serve.rs directly
- No multi-channel routing — responses can't be directed back to origin
- WebhookChannel is dead code

## Solution

### 1. Instantiate ChannelRegistry on HTTP server startup

In `http_serve.rs`, replace direct `Arc<TelegramChannel>` with `ChannelRegistry`:

```rust
let mut channel_registry = ChannelRegistry::new();

// Register Telegram if configured
if let Some(telegram) = telegram {
    channel_registry.register("telegram", telegram);
}

// Register webhook channels from config
for webhook_config in &config.webhook.channels {
    let webhook = WebhookChannel::new(webhook_config.clone());
    channel_registry.register(&webhook_config.id, Arc::new(webhook));
}

let registry = Arc::new(channel_registry);
```

### 2. Update HttpState

```rust
struct HttpState {
    app: Arc<Mutex<HeadlessApp>>,
    start_time: Instant,
    tailscale_ip: IpAddr,
    bearer_token: String,
    channels: Arc<ChannelRegistry>,  // replaces telegram: Option<Arc<TelegramChannel>>
}
```

### 3. Message routing via ResponseRouter

Create `ResponseRouter` alongside `ChannelRegistry`:
```rust
let response_router = ResponseRouter::new(Arc::clone(&registry));
```

When processing a message from a channel:
- Tag the message with its source channel ID
- After the agent responds, use `ResponseRouter` to send the response back 
  to the originating channel

### 4. Refactor Telegram-specific code

Current direct references to `TelegramChannel` in:
- `handle_telegram_webhook()` — keep as HTTP endpoint, but resolve channel from registry
- `run_telegram_polling()` — keep as background task, but get channel from registry
- `send_telegram_response()` — generalize to `send_channel_response(channel_id, response)`

### 5. Webhook endpoint

Add a generic webhook endpoint:
```
POST /webhook/:channel_id
```

Routes incoming messages to the channel registered with that ID.

### Pre-investigation needed

Before implementation:
1. Does `ChannelRegistry` currently support `get_channel(id)` lookup?
2. What's the `Channel` trait's send/receive interface?
3. Does `ResponseRouter` need any config beyond the registry?

## Files touched

| File | Change |
|------|--------|
| `http_serve.rs` | Replace direct Telegram with ChannelRegistry, add webhook endpoint |
| `main.rs` | Build channels from config, pass to HTTP server |
| Tests | Channel registration, message routing, webhook endpoint |

## Security

- Webhook endpoint requires bearer auth (same as /message)
- Channel IDs are from config — agent cannot register arbitrary channels
- Response routing is based on source channel tag (no cross-channel leaks)
- WebhookChannel callbacks validate against configured URL (no open redirect)
