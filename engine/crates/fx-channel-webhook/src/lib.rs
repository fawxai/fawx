//! Generic webhook channel implementation.
//!
//! Provides a [`Channel`] implementation that receives messages via HTTP POST
//! and queues responses for retrieval. This crate handles message parsing and
//! response formatting only — no networking (no axum, no reqwest). The actual
//! HTTP server lives in fx-cli.

use fx_core::channel::{Channel, ChannelError, ResponseContext};
use fx_core::types::InputSource;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// WebhookChannel
// ---------------------------------------------------------------------------

/// A webhook channel that receives messages via HTTP POST
/// and queues responses for retrieval.
pub struct WebhookChannel {
    channel_id: String,
    display_name: String,
    callback_url: Option<String>,
    active: AtomicBool,
    pending_response: Mutex<Option<String>>,
}

impl WebhookChannel {
    /// Create a new webhook channel.
    pub fn new(channel_id: String, display_name: String, callback_url: Option<String>) -> Self {
        Self {
            channel_id,
            display_name,
            callback_url,
            active: AtomicBool::new(true),
            pending_response: Mutex::new(None),
        }
    }

    /// Take the pending response (if any). Returns `None` if no response queued.
    ///
    /// A poisoned mutex is treated as "no response available." This is
    /// deliberate: the response slot is ephemeral, so if another thread
    /// panicked while holding the lock the pending value is unreliable and
    /// returning `None` is the safest default.
    pub fn take_response(&self) -> Option<String> {
        self.pending_response
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    /// Set the active state.
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }

    /// The configured callback URL (if any).
    ///
    /// This is a hint for the HTTP layer — this crate does not perform HTTP.
    pub fn callback_url(&self) -> Option<&str> {
        self.callback_url.as_deref()
    }
}

impl Channel for WebhookChannel {
    fn id(&self) -> &str {
        &self.channel_id
    }

    fn name(&self) -> &str {
        &self.display_name
    }

    fn input_source(&self) -> InputSource {
        InputSource::Channel(self.channel_id.clone())
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn send_response(&self, message: &str, _context: &ResponseContext) -> Result<(), ChannelError> {
        let mut slot = self
            .pending_response
            .lock()
            .map_err(|e| ChannelError::DeliveryFailed(e.to_string()))?;
        *slot = Some(message.to_string());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// Inbound webhook message (deserialized from POST body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookMessage {
    /// The message text.
    pub text: String,
    /// Optional sender identifier.
    #[serde(default)]
    pub sender_id: Option<String>,
    /// Optional callback URL for response delivery.
    #[serde(default)]
    pub callback_url: Option<String>,
    /// Optional metadata (arbitrary JSON).
    #[serde(default)]
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_channel() -> WebhookChannel {
        WebhookChannel::new(
            "webhook-test".to_string(),
            "Test Webhook".to_string(),
            Some("https://example.com/callback".to_string()),
        )
    }

    #[test]
    fn webhook_channel_implements_channel() {
        let ch = make_channel();
        assert_eq!(ch.id(), "webhook-test");
        assert_eq!(ch.name(), "Test Webhook");
        assert!(ch.is_active());
        assert_eq!(ch.callback_url(), Some("https://example.com/callback"));
    }

    #[test]
    fn send_response_stores_pending() {
        let ch = make_channel();
        ch.send_response("hello webhook", &ResponseContext::default())
            .unwrap();
        assert_eq!(ch.take_response().as_deref(), Some("hello webhook"));
    }

    #[test]
    fn take_response_clears_slot() {
        let ch = make_channel();
        ch.send_response("first", &ResponseContext::default())
            .unwrap();
        assert!(ch.take_response().is_some());
        assert!(ch.take_response().is_none());
    }

    #[test]
    fn set_active_toggles_state() {
        let ch = make_channel();
        assert!(ch.is_active());
        ch.set_active(false);
        assert!(!ch.is_active());
        ch.set_active(true);
        assert!(ch.is_active());
    }

    #[test]
    fn webhook_message_deserialize() {
        let json = r#"{
            "text": "hello",
            "sender_id": "user-1",
            "callback_url": "https://example.com/cb",
            "metadata": {"key": "value"}
        }"#;
        let msg: WebhookMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.sender_id.as_deref(), Some("user-1"));
        assert_eq!(msg.callback_url.as_deref(), Some("https://example.com/cb"));
        assert!(msg.metadata.is_some());

        // Round-trip
        let serialized = serde_json::to_string(&msg).unwrap();
        let roundtrip: WebhookMessage = serde_json::from_str(&serialized).unwrap();
        assert_eq!(roundtrip.text, "hello");
    }

    #[test]
    fn webhook_response_serialize() {
        let resp = WebhookResponse {
            text: "answer".to_string(),
            channel_id: "wh-1".to_string(),
            complete: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""text":"answer""#));
        assert!(json.contains(r#""channel_id":"wh-1""#));
        assert!(json.contains(r#""complete":true"#));

        // Round-trip
        let roundtrip: WebhookResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.text, "answer");
        assert_eq!(roundtrip.channel_id, "wh-1");
        assert!(roundtrip.complete);
    }

    #[test]
    fn webhook_message_optional_fields() {
        let json = r#"{"text": "minimal"}"#;
        let msg: WebhookMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.text, "minimal");
        assert!(msg.sender_id.is_none());
        assert!(msg.callback_url.is_none());
        assert!(msg.metadata.is_none());
    }

    #[test]
    fn input_source_is_channel_variant() {
        let ch = make_channel();
        let source = ch.input_source();
        assert_eq!(source, InputSource::Channel("webhook-test".to_string()));
    }
}
