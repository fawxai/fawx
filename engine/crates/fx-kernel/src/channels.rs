//! Channel registry, response router, and built-in channel implementations.
//!
//! The [`ChannelRegistry`] tracks all registered input/output channels.
//! The [`ResponseRouter`] routes kernel responses back to the originating channel.

use fx_core::channel::{Channel, ChannelError, ResponseContext};
use fx_core::types::InputSource;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// ChannelRegistry
// ---------------------------------------------------------------------------

/// Registry of active input/output channels.
///
/// Tracks which channels are connected and provides lookup by id.
pub struct ChannelRegistry {
    channels: Vec<Arc<dyn Channel>>,
}

impl std::fmt::Debug for ChannelRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<&str> = self.channels.iter().map(|channel| channel.id()).collect();
        f.debug_struct("ChannelRegistry")
            .field("channels", &ids)
            .finish()
    }
}

impl ChannelRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
        }
    }

    /// Register a new channel.
    ///
    /// If a channel with the same id already exists, it is replaced.
    pub fn register(&mut self, channel: Arc<dyn Channel>) {
        let id = channel.id().to_string();
        self.channels.retain(|existing| existing.id() != id);
        self.channels.push(channel);
    }

    /// Remove a channel by id. Returns `true` if a channel was removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.channels.len();
        self.channels.retain(|channel| channel.id() != id);
        self.channels.len() < before
    }

    /// Get a channel by id.
    pub fn get(&self, id: &str) -> Option<&dyn Channel> {
        self.channels
            .iter()
            .find(|channel| channel.id() == id)
            .map(Arc::as_ref)
    }

    /// List all registered channels.
    pub fn list(&self) -> Vec<&dyn Channel> {
        self.channels.iter().map(Arc::as_ref).collect()
    }

    /// List only active channels.
    pub fn active(&self) -> Vec<&dyn Channel> {
        self.channels
            .iter()
            .filter(|channel| channel.is_active())
            .map(Arc::as_ref)
            .collect()
    }

    /// Number of registered channels.
    pub fn count(&self) -> usize {
        self.channels.len()
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ResponseRouter
// ---------------------------------------------------------------------------

/// Routes responses back to originating channels.
///
/// Given an [`InputSource`], the router looks up the corresponding channel
/// in the registry and calls [`Channel::send_response()`].
pub struct ResponseRouter {
    registry: Arc<ChannelRegistry>,
}

impl ResponseRouter {
    /// Create a new router backed by the given registry.
    pub fn new(registry: Arc<ChannelRegistry>) -> Self {
        Self { registry }
    }

    /// Route a response to the channel that originated the request.
    ///
    /// - `InputSource::Text` -> TUI channel (no-op send_response)
    /// - `InputSource::Http` -> HTTP channel
    /// - `InputSource::Channel(id)` -> channel with matching id
    /// - Other variants -> `ChannelError::NotFound`
    pub fn route(
        &self,
        source: &InputSource,
        message: &str,
        context: &ResponseContext,
    ) -> Result<(), ChannelError> {
        let channel = self.find_channel_for_source(source)?;
        channel.send_response(message, context)
    }

    fn find_channel_for_source(&self, source: &InputSource) -> Result<&dyn Channel, ChannelError> {
        match source {
            InputSource::Text => self
                .registry
                .get("tui")
                .ok_or_else(|| ChannelError::NotFound("tui".to_string())),
            InputSource::Http => self
                .registry
                .get("http")
                .ok_or_else(|| ChannelError::NotFound("http".to_string())),
            InputSource::Channel(id) => self
                .registry
                .get(id)
                .ok_or_else(|| ChannelError::NotFound(id.clone())),
            other => Err(ChannelError::NotFound(format!("{other:?}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Built-in channel implementations
// ---------------------------------------------------------------------------

/// The TUI channel -- terminal input, handles its own output.
///
/// `send_response()` is a no-op because the TUI renders responses directly.
pub struct TuiChannel {
    active: AtomicBool,
}

impl TuiChannel {
    /// Create a new TUI channel (active by default).
    pub fn new() -> Self {
        Self {
            active: AtomicBool::new(true),
        }
    }

    /// Set the active state.
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }
}

impl Default for TuiChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl Channel for TuiChannel {
    fn id(&self) -> &str {
        "tui"
    }

    fn name(&self) -> &str {
        "Terminal UI"
    }

    fn input_source(&self) -> InputSource {
        InputSource::Text
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn send_response(
        &self,
        _message: &str,
        _context: &ResponseContext,
    ) -> Result<(), ChannelError> {
        Ok(())
    }
}

/// The HTTP API channel -- stores responses for retrieval by the HTTP handler.
pub struct HttpChannel {
    active: AtomicBool,
    /// Pending response slot -- set by `send_response`, read by HTTP handler.
    pending_response: Mutex<Option<String>>,
}

impl HttpChannel {
    /// Create a new HTTP channel (active by default).
    pub fn new() -> Self {
        Self {
            active: AtomicBool::new(true),
            pending_response: Mutex::new(None),
        }
    }

    /// Set the active state.
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }

    /// Take the pending response (if any). Clears the slot.
    pub fn take_response(&self) -> Option<String> {
        self.pending_response
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }
}

impl Default for HttpChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl Channel for HttpChannel {
    fn id(&self) -> &str {
        "http"
    }

    fn name(&self) -> &str {
        "HTTP API"
    }

    fn input_source(&self) -> InputSource {
        InputSource::Http
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn send_response(&self, message: &str, _context: &ResponseContext) -> Result<(), ChannelError> {
        let mut slot = self
            .pending_response
            .lock()
            .map_err(|error| ChannelError::DeliveryFailed(error.to_string()))?;
        *slot = Some(message.to_string());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// Test helper: a mock channel with configurable id, active state, and
    /// response capture.
    struct MockChannel {
        id: String,
        channel_name: String,
        source: InputSource,
        active: AtomicBool,
        last_response: Arc<Mutex<Option<(String, ResponseContext)>>>,
    }

    impl MockChannel {
        fn new(id: &str, source: InputSource, active: bool) -> Self {
            Self {
                id: id.to_string(),
                channel_name: format!("Mock {id}"),
                source,
                active: AtomicBool::new(active),
                last_response: Arc::new(Mutex::new(None)),
            }
        }

        fn response_slot(&self) -> Arc<Mutex<Option<(String, ResponseContext)>>> {
            Arc::clone(&self.last_response)
        }
    }

    impl Channel for MockChannel {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            &self.channel_name
        }

        fn input_source(&self) -> InputSource {
            self.source.clone()
        }

        fn is_active(&self) -> bool {
            self.active.load(Ordering::Relaxed)
        }

        fn send_response(
            &self,
            message: &str,
            context: &ResponseContext,
        ) -> Result<(), ChannelError> {
            let mut slot = self.last_response.lock().expect("response slot");
            *slot = Some((message.to_string(), context.clone()));
            Ok(())
        }
    }

    #[test]
    fn register_channel() {
        let mut registry = ChannelRegistry::new();
        assert_eq!(registry.count(), 0);

        registry.register(Arc::new(TuiChannel::new()));
        assert_eq!(registry.count(), 1);

        registry.register(Arc::new(HttpChannel::new()));
        assert_eq!(registry.count(), 2);
    }

    #[test]
    fn remove_channel() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(TuiChannel::new()));
        registry.register(Arc::new(HttpChannel::new()));
        assert_eq!(registry.count(), 2);

        assert!(registry.remove("tui"));
        assert_eq!(registry.count(), 1);
        assert!(registry.get("tui").is_none());
        assert!(!registry.remove("nonexistent"));
    }

    #[test]
    fn get_channel_by_id() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(TuiChannel::new()));
        registry.register(Arc::new(HttpChannel::new()));

        let tui = registry.get("tui").expect("tui should exist");
        assert_eq!(tui.id(), "tui");
        assert_eq!(tui.name(), "Terminal UI");

        let http = registry.get("http").expect("http should exist");
        assert_eq!(http.id(), "http");
        assert_eq!(http.name(), "HTTP API");

        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn list_active_channels() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(MockChannel::new(
            "active1",
            InputSource::Channel("active1".to_string()),
            true,
        )));
        registry.register(Arc::new(MockChannel::new(
            "inactive",
            InputSource::Channel("inactive".to_string()),
            false,
        )));
        registry.register(Arc::new(MockChannel::new(
            "active2",
            InputSource::Channel("active2".to_string()),
            true,
        )));

        let all = registry.list();
        assert_eq!(all.len(), 3);

        let active = registry.active();
        assert_eq!(active.len(), 2);
        assert!(active.iter().all(|channel| channel.is_active()));
    }

    #[test]
    fn duplicate_id_handling() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(MockChannel::new(
            "dup",
            InputSource::Channel("dup".to_string()),
            true,
        )));
        assert_eq!(registry.count(), 1);

        registry.register(Arc::new(MockChannel::new(
            "dup",
            InputSource::Channel("dup".to_string()),
            false,
        )));
        assert_eq!(registry.count(), 1);

        let channel = registry.get("dup").expect("dup should exist");
        assert!(!channel.is_active());
    }

    #[test]
    fn input_source_channel_variant() {
        let source = InputSource::Channel("telegram".to_string());
        let json = serde_json::to_string(&source).expect("serialize");
        let deserialized: InputSource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(source, deserialized);

        let http = InputSource::Http;
        let json = serde_json::to_string(&http).expect("serialize");
        let deserialized: InputSource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(http, deserialized);
    }

    #[test]
    fn response_router_delivers_to_channel_with_context() {
        let mock = Arc::new(MockChannel::new(
            "test-ch",
            InputSource::Channel("test-ch".to_string()),
            true,
        ));
        let slot = mock.response_slot();

        let mut registry = ChannelRegistry::new();
        registry.register(mock);

        let router = ResponseRouter::new(Arc::new(registry));
        let source = InputSource::Channel("test-ch".to_string());
        let context = ResponseContext {
            routing_key: Some("chat-42".to_string()),
            reply_to: Some("99".to_string()),
        };

        let result = router.route(&source, "hello from router", &context);
        assert!(result.is_ok());

        let delivered = slot.lock().expect("delivered").clone();
        assert_eq!(
            delivered,
            Some(("hello from router".to_string(), context.clone()))
        );

        let missing = InputSource::Channel("missing".to_string());
        let err = router.route(&missing, "should fail", &ResponseContext::default());
        assert!(matches!(err, Err(ChannelError::NotFound(_))));
    }

    #[test]
    fn response_router_noop_for_tui() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(TuiChannel::new()));

        let router = ResponseRouter::new(Arc::new(registry));
        let result = router.route(&InputSource::Text, "hello tui", &ResponseContext::default());
        assert!(result.is_ok());
    }
}
