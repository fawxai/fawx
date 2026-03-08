# Spec: Phase 0 PR 5 — ChannelRegistry + ResponseRouter Wiring

**Gap:** Channel abstraction built but never instantiated  
**Estimated size:** ~400 lines  
**Risk:** Medium — refactors message flow in HTTP server  
**Depends on:** None (parallel with PRs 1-4, but land after to reduce merge conflicts)

---

## Problem

`fx-kernel/src/channels.rs` defines:
- `ChannelRegistry` — tracks registered input/output channels (register/get/list/remove)
- `ResponseRouter` — routes kernel responses to originating channel via `InputSource`
- `TuiChannel`, `HttpChannel` — built-in channel implementations

`fx-channel-telegram/` implements `Channel` for Telegram (tested, 654+ lines).
`fx-channel-webhook/` implements `Channel` for generic webhooks.

None of these are used. Telegram is wired directly in `http_serve.rs` with 
hardcoded `Arc<TelegramChannel>` references. This means:
- Adding new channels requires modifying http_serve.rs directly
- No multi-channel routing — responses can't be directed back to origin
- WebhookChannel is dead code
- TuiChannel and HttpChannel are tested but never instantiated

## Design Correction (from pressure test)

### Channel::send_response needs routing context

The current trait signature:
```rust
fn send_response(&self, message: &str) -> Result<(), ChannelError>;
```

This works for TUI (no-op) and HTTP (single pending slot). But Telegram 
needs to know *which chat* to send the response to — multiple chats can be 
active simultaneously. The generic signature loses this routing context.

**Solution: Add `ResponseContext` parameter.**

```rust
/// Routing context for response delivery.
/// Channels extract what they need; others ignore it.
#[derive(Debug, Clone, Default)]
pub struct ResponseContext {
    /// Channel-specific routing key (e.g., Telegram chat_id as string)
    pub routing_key: Option<String>,
    /// Original message ID (for reply threading)
    pub reply_to: Option<String>,
}

pub trait Channel: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn input_source(&self) -> InputSource;
    fn is_active(&self) -> bool;
    
    /// Send a response back through this channel.
    /// `ctx` carries channel-specific routing info (chat_id, reply_to, etc.)
    fn send_response(&self, message: &str, ctx: &ResponseContext) -> Result<(), ChannelError>;
}
```

**Impact on existing implementors:**
- `TuiChannel::send_response` — ignores ctx, still no-op. 1-line signature change.
- `HttpChannel::send_response` — ignores ctx, still writes to pending slot. 1-line change.
- `TelegramChannel::send_response` — reads `ctx.routing_key` as chat_id. Buffers 
  `(chat_id, message)` pair internally. Polling task drains and sends.

**ResponseRouter update:**
```rust
pub fn route(&self, source: &InputSource, message: &str, ctx: &ResponseContext) -> Result<(), ChannelError>;
```

Callers construct `ResponseContext` from the inbound message metadata (e.g., 
Telegram update carries chat_id → stored as routing_key).

### Telegram response buffering

`TelegramChannel` gets an internal outbound buffer:
```rust
pub struct TelegramChannel {
    // ... existing fields ...
    outbound: Mutex<VecDeque<OutboundMessage>>,
}

struct OutboundMessage {
    chat_id: i64,
    text: String,
}
```

- `send_response()` parses `ctx.routing_key` as chat_id, pushes to `outbound`
- Existing polling task checks `outbound` each iteration, sends via Telegram API
- If parsing fails or routing_key is None: return `ChannelError::DeliveryFailed`

## Solution

### 1. Update Channel trait (fx-core)

Add `ResponseContext` struct. Update `Channel::send_response` signature.
Update all 4 implementors (TuiChannel, HttpChannel, TelegramChannel, WebhookChannel).

### 2. Instantiate ChannelRegistry on HTTP server startup

In `http_serve.rs`, replace direct `Arc<TelegramChannel>` with `ChannelRegistry`:

```rust
let mut channel_registry = ChannelRegistry::new();

// Always register HTTP channel
channel_registry.register(Box::new(HttpChannel::new()));

// Register Telegram if configured
if let Some(tg_config) = &config.telegram {
    if tg_config.enabled {
        let telegram = TelegramChannel::new(/* ... */);
        channel_registry.register(Box::new(telegram));
    }
}

// Register webhook channels from config
for wh_config in &config.webhook.channels {
    let webhook = WebhookChannel::new(wh_config.clone());
    channel_registry.register(Box::new(webhook));
}

let registry = Arc::new(channel_registry);
let response_router = ResponseRouter::new(Arc::clone(&registry));
```

### 3. Update HttpState

```rust
struct HttpState {
    app: Arc<Mutex<HeadlessApp>>,
    start_time: Instant,
    tailscale_ip: IpAddr,
    bearer_token: String,
    registry: Arc<ChannelRegistry>,     // replaces telegram: Option<Arc<TelegramChannel>>
    router: Arc<ResponseRouter>,        // new
}
```

### 4. Refactor Telegram-specific code

Current `http_serve.rs` has:
- `handle_telegram_webhook()` — resolve channel from registry instead of direct Arc
- `run_telegram_polling()` — get channel from registry, tag inbound messages with 
  `InputSource::Channel("telegram")` + store chat_id for ResponseContext
- Direct `telegram.send_message()` calls → `router.route()` with ResponseContext

**Inbound flow (polling):**
```
Telegram API → polling task → extract chat_id + message text
  → create ResponseContext { routing_key: Some(chat_id.to_string()) }
  → feed to loop engine with InputSource::Channel("telegram")
  → store (message_id → ResponseContext) mapping for response routing
```

**Outbound flow (after loop engine completes):**
```
Loop engine → LoopResult with response
  → look up stored ResponseContext for this message
  → router.route(&InputSource::Channel("telegram"), response, &ctx)
  → TelegramChannel::send_response buffers (chat_id, message)
  → polling task drains buffer → Telegram API send_message
```

### 5. Webhook endpoint

Add generic webhook endpoint:
```
POST /webhook/:channel_id
```

Looks up channel in registry, routes inbound message. Same ResponseContext
pattern — webhook channels store callback URLs or response slots.

### 6. TUI wiring

In `run_tui()`: register `TuiChannel` in a `ChannelRegistry`. Not strictly 
needed for Phase 0 (TUI renders directly), but establishes the pattern. 
Can be a follow-up if it adds complexity.

## Implementation Gates

### Gate 1: Concurrent message tracking
The inbound→outbound ResponseContext mapping needs to handle concurrent 
messages from different Telegram chats. If the loop engine processes messages 
sequentially (single HeadlessApp behind Mutex), a simple "last context" slot 
works. If concurrent processing is possible, need a `HashMap<message_id, ResponseContext>`.

**Rule:** Check how `HeadlessApp` handles concurrent `/message` requests. If 
it's behind `Arc<Mutex<>>` (sequential), use simple last-context slot. If 
there's any concurrency, use a map. **Stop and report** if the concurrency 
model is unclear.

### Gate 2: TelegramChannel constructor dependencies
`TelegramChannel::new()` — check what it needs (bot token, reqwest client, 
config). These are currently constructed directly in `http_serve.rs`. Moving 
construction to startup means these deps need to be available earlier. If 
TelegramChannel needs things that are only available after server starts 
(e.g., webhook URL), **stop and report**.

## Files touched

| File | Change |
|------|--------|
| `fx-core/src/channel.rs` | Add `ResponseContext`, update `Channel` trait signature |
| `fx-kernel/src/channels.rs` | Update TuiChannel, HttpChannel, ResponseRouter for new signature |
| `fx-channel-telegram/src/lib.rs` | Add outbound buffer, update send_response with ctx |
| `fx-channel-webhook/src/lib.rs` | Update send_response with ctx |
| `fx-cli/src/http_serve.rs` | Replace direct Telegram with ChannelRegistry, ResponseRouter, add webhook endpoint |
| `fx-cli/src/main.rs` | Build channels from config, pass to HTTP server |
| Tests | ResponseContext routing, outbound buffer drain, concurrent chat isolation, webhook endpoint |

## Security

- Webhook endpoint requires bearer auth (same as /message)
- Channel IDs are from config — agent cannot register arbitrary channels
- ResponseContext routing_key is set by the server from inbound metadata — 
  agent cannot forge a routing_key to send to arbitrary chats
- Response routing is based on source channel tag (no cross-channel leaks)
- Outbound buffer is bounded (max 100 pending messages, oldest dropped)
