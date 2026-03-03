//! Event bus for asynchronous communication between Fawx components.
//!
//! Uses `tokio::sync::broadcast` for multi-producer, multi-consumer event distribution.

use crate::error::Result;
use crate::message::InternalMessage;
use tokio::sync::broadcast;

/// Event bus for distributing events across Fawx components.
///
/// Uses a broadcast channel to allow multiple subscribers to receive
/// the same events.
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<InternalMessage>,
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus").finish()
    }
}

impl EventBus {
    /// Create a new event bus with the specified capacity.
    ///
    /// # Arguments
    /// * `capacity` - Maximum number of messages to buffer
    ///
    /// # Returns
    /// A new `EventBus` instance
    ///
    /// # Example
    /// ```
    /// use fx_core::EventBus;
    /// let bus = EventBus::new(100);
    /// ```
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Subscribe to events on this bus.
    ///
    /// Returns a receiver that will receive all events published after subscription.
    ///
    /// # Returns
    /// A `broadcast::Receiver` for receiving events
    pub fn subscribe(&self) -> broadcast::Receiver<InternalMessage> {
        self.sender.subscribe()
    }

    /// Publish an event to all subscribers.
    ///
    /// Returns the number of receivers that received the message.
    /// If no subscribers are active, returns `Ok(0)` — this is expected
    /// during startup or when components haven't subscribed yet.
    ///
    /// # Arguments
    /// * `message` - The message to publish
    ///
    /// # Returns
    /// `Ok(usize)` - Number of receivers that received the message
    pub fn publish(&self, message: InternalMessage) -> Result<usize> {
        match self.sender.send(message) {
            Ok(count) => Ok(count),
            Err(_) => Ok(0), // No active receivers — not an error
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus_publish_receive() {
        let bus = EventBus::new(10);
        let mut receiver = bus.subscribe();

        let message = InternalMessage::SystemStatus {
            message: "Test message".to_string(),
        };

        // Publish event
        let result = bus.publish(message.clone());
        assert!(result.is_ok());

        // Receive event
        let received = receiver.recv().await.unwrap();
        match received {
            InternalMessage::SystemStatus { message } => {
                assert_eq!(message, "Test message");
            }
            _ => panic!("Unexpected message type"),
        }
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new(10);
        let mut receiver1 = bus.subscribe();
        let mut receiver2 = bus.subscribe();

        let message = InternalMessage::SystemStatus {
            message: "Broadcast test".to_string(),
        };

        bus.publish(message).unwrap();

        // Both receivers should get the message
        let msg1 = receiver1.recv().await.unwrap();
        let msg2 = receiver2.recv().await.unwrap();

        match (msg1, msg2) {
            (
                InternalMessage::SystemStatus { message: m1 },
                InternalMessage::SystemStatus { message: m2 },
            ) => {
                assert_eq!(m1, "Broadcast test");
                assert_eq!(m2, "Broadcast test");
            }
            _ => panic!("Unexpected message types"),
        }
    }
}
