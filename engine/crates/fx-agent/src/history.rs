//! Conversation history management.

use crate::claude::types::Message;
use std::collections::VecDeque;

/// Manages conversation history with automatic truncation.
#[derive(Debug, Clone)]
pub struct ConversationHistory {
    /// Maximum number of messages to keep.
    max_messages: usize,
    /// Message history (oldest to newest).
    messages: VecDeque<Message>,
}

impl ConversationHistory {
    /// Create a new conversation history with a maximum message count.
    pub fn new(max_messages: usize) -> Self {
        Self {
            max_messages,
            messages: VecDeque::with_capacity(max_messages),
        }
    }

    /// Add a user message to the history.
    pub fn add_user_message(&mut self, text: &str) {
        self.add_message(Message::user(text));
    }

    /// Add an assistant message to the history.
    pub fn add_assistant_message(&mut self, text: &str) {
        self.add_message(Message::assistant(text));
    }

    /// Add a tool result as an assistant message.
    pub fn add_tool_result(&mut self, tool_use_id: &str, result: &str) {
        let content = format!("Tool {} result: {}", tool_use_id, result);
        self.add_message(Message::assistant(content));
    }

    /// Get all messages in the history.
    ///
    /// Returns a slice of all messages. Note: VecDeque::make_contiguous() is called
    /// to ensure the messages are contiguous in memory for efficient slice access.
    pub fn messages(&mut self) -> &[Message] {
        self.messages.make_contiguous()
    }

    /// Truncate history to a specific number of messages.
    ///
    /// Keeps only the most recent `max_messages` by removing from the front.
    pub fn truncate_to(&mut self, max_messages: usize) {
        while self.messages.len() > max_messages {
            self.messages.pop_front();
        }
        self.max_messages = max_messages;
    }

    /// Clear all messages.
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Get the number of messages in history.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if history is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Add a message and enforce max_messages limit.
    ///
    /// Removes the oldest message if at capacity before adding the new one.
    fn add_message(&mut self, message: Message) {
        if self.messages.len() >= self.max_messages {
            self.messages.pop_front();
        }
        self.messages.push_back(message);
    }
}

impl Default for ConversationHistory {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::types::Role;

    #[test]
    fn test_new() {
        let history = ConversationHistory::new(10);
        assert_eq!(history.max_messages, 10);
        assert_eq!(history.len(), 0);
        assert!(history.is_empty());
    }

    #[test]
    fn test_add_user_message() {
        let mut history = ConversationHistory::new(10);
        history.add_user_message("Hello");

        assert_eq!(history.len(), 1);
        assert_eq!(history.messages()[0].role, Role::User);
        assert_eq!(history.messages()[0].content, "Hello");
    }

    #[test]
    fn test_add_assistant_message() {
        let mut history = ConversationHistory::new(10);
        history.add_assistant_message("Hi there!");

        assert_eq!(history.len(), 1);
        assert_eq!(history.messages()[0].role, Role::Assistant);
        assert_eq!(history.messages()[0].content, "Hi there!");
    }

    #[test]
    fn test_add_tool_result() {
        let mut history = ConversationHistory::new(10);
        history.add_tool_result("tool_123", "success");

        assert_eq!(history.len(), 1);
        assert_eq!(history.messages()[0].role, Role::Assistant);
        assert!(history.messages()[0].content.contains("tool_123"));
        assert!(history.messages()[0].content.contains("success"));
    }

    #[test]
    fn test_max_messages_enforcement() {
        let mut history = ConversationHistory::new(3);

        history.add_user_message("Message 1");
        history.add_user_message("Message 2");
        history.add_user_message("Message 3");
        assert_eq!(history.len(), 3);

        // Adding 4th message should remove the oldest
        history.add_user_message("Message 4");
        assert_eq!(history.len(), 3);
        assert_eq!(history.messages()[0].content, "Message 2");
        assert_eq!(history.messages()[2].content, "Message 4");
    }

    #[test]
    fn test_truncate_to() {
        let mut history = ConversationHistory::new(10);
        for i in 1..=5 {
            history.add_user_message(&format!("Message {}", i));
        }
        assert_eq!(history.len(), 5);

        history.truncate_to(3);
        assert_eq!(history.len(), 3);
        assert_eq!(history.max_messages, 3);
        // Should keep the most recent messages
        assert_eq!(history.messages()[0].content, "Message 3");
        assert_eq!(history.messages()[2].content, "Message 5");
    }

    #[test]
    fn test_clear() {
        let mut history = ConversationHistory::new(10);
        history.add_user_message("Message 1");
        history.add_assistant_message("Response 1");
        assert_eq!(history.len(), 2);

        history.clear();
        assert_eq!(history.len(), 0);
        assert!(history.is_empty());
    }

    #[test]
    fn test_messages() {
        let mut history = ConversationHistory::new(10);
        history.add_user_message("Hello");
        history.add_assistant_message("Hi");

        let messages = history.messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[1].role, Role::Assistant);
    }

    #[test]
    fn test_default() {
        let history = ConversationHistory::default();
        assert_eq!(history.max_messages, 100);
        assert!(history.is_empty());
    }

    #[test]
    fn test_alternating_messages() {
        let mut history = ConversationHistory::new(6);
        history.add_user_message("Q1");
        history.add_assistant_message("A1");
        history.add_user_message("Q2");
        history.add_assistant_message("A2");
        history.add_user_message("Q3");
        history.add_assistant_message("A3");

        assert_eq!(history.len(), 6);

        // Add one more to test oldest removal
        history.add_user_message("Q4");
        assert_eq!(history.len(), 6);
        assert_eq!(history.messages()[0].content, "A1");
        assert_eq!(history.messages()[5].content, "Q4");
    }
}
