# Wave 7 Item #2: Webhook Channel (fx-channel-webhook)

## Overview

A generic HTTP webhook channel that implements the `Channel` trait. Receives messages via HTTP POST, routes them through the kernel, and delivers responses back to configurable callback URLs.

This is NOT a Telegram/Discord-specific channel — it's a generic webhook bridge. Specific integrations come later.

## 1. Crate Structure

```
engine/crates/fx-channel-webhook/
├── Cargo.toml
└── src/
    └── lib.rs
```

Dependencies: `serde`, `serde_json`, `fx-core` (for Channel, ChannelError, InputSource).
NO networking dependencies (no axum, no reqwest). This crate handles message parsing and response formatting only. The actual HTTP server lives in fx-cli's existing http_serve.rs.

## 2. WebhookChannel

```rust
use fx_core::channel::{Channel, ChannelError};
use fx_core::types::InputSource;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// A webhook channel that receives messages via HTTP POST
/// and queues responses for retrieval.
pub struct WebhookChannel {
    channel_id: String,
    display_name: String,
    callback_url: Option<String>,
    active: AtomicBool,
    pending_response: Mutex<Option<String>>,
}
```

### Constructor

```rust
impl WebhookChannel {
    pub fn new(channel_id: String, display_name: String, callback_url: Option<String>) -> Self;
    
    /// Take the pending response (if any). Returns None if no response queued.
    /// Deliberately treats mutex poison as "no response" (documented choice).
    pub fn take_response(&self) -> Option<String>;
    
    /// Set active state.
    pub fn set_active(&self, active: bool);
}
```

### Channel impl

```rust
impl Channel for WebhookChannel {
    fn id(&self) -> &str { &self.channel_id }
    fn name(&self) -> &str { &self.display_name }
    fn input_source(&self) -> InputSource { InputSource::Channel(self.channel_id.clone()) }
    fn is_active(&self) -> bool { self.active.load(Ordering::Relaxed) }
    fn send_response(&self, message: &str) -> Result<(), ChannelError> {
        // Store response in pending slot for retrieval
        // If callback_url is set, that's a hint for the HTTP layer — but
        // this crate doesn't do HTTP. It just queues the response.
        let mut slot = self.pending_response.lock()
            .map_err(|e| ChannelError::DeliveryFailed(e.to_string()))?;
        *slot = Some(message.to_string());
        Ok(())
    }
}
```

## 3. Webhook Message Types

```rust
/// Inbound webhook message (deserialized from POST body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookMessage {
    /// The message text.
    pub text: String,
    /// Optional sender identifier.
    pub sender_id: Option<String>,
    /// Optional callback URL for response delivery.
    pub callback_url: Option<String>,
    /// Optional metadata (arbitrary JSON).
    pub metadata: Option<serde_json::Value>,
}

/// Outbound webhook response (serialized as JSON response body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookResponse {
    /// Response text from the agent.
    pub text: String,
    /// Channel that produced this response.
    pub channel_id: String,
    /// Whether the response is complete (vs streaming partial).
    pub complete: bool,
}
```

## 4. WebhookConfig (add to fx-config)

```toml
[webhook]
enabled = false
channels = []  # list of { id, name, callback_url }
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebhookConfig {
    pub enabled: bool,
    pub channels: Vec<WebhookChannelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookChannelConfig {
    pub id: String,
    pub name: String,
    pub callback_url: Option<String>,
}
```

## 5. Tests (8 required)

1. `webhook_channel_implements_channel` — id, name, input_source, is_active correct
2. `send_response_stores_pending` — send_response queues, take_response retrieves
3. `take_response_clears_slot` — second take_response returns None
4. `set_active_toggles_state` — active starts true, toggle to false
5. `webhook_message_deserialize` — JSON round-trip for WebhookMessage
6. `webhook_response_serialize` — WebhookResponse serializes correctly
7. `webhook_message_optional_fields` — sender_id, callback_url, metadata all optional
8. `input_source_is_channel_variant` — InputSource::Channel(id) matches

## 6. What This Does NOT Do

- No HTTP server (that's fx-cli's job)
- No actual HTTP callback delivery (future work)
- No authentication (bearer token is in fx-cli's http_serve.rs)
- No streaming (complete responses only for now)

This crate is the data model + Channel impl. The HTTP layer in fx-cli routes requests to it.
