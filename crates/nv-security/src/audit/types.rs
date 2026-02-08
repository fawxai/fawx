//! Audit event types for security and compliance tracking.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An audit event recording a security-relevant action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique identifier for this event (UUID v4)
    pub id: String,

    /// Unix timestamp in milliseconds
    pub timestamp: u64,

    /// Type of event
    pub event_type: AuditEventType,

    /// Actor who triggered this event (e.g., "agent", "user", "skill:camera")
    pub actor: String,

    /// Human-readable description
    pub description: String,

    /// Additional context as key-value pairs
    pub metadata: HashMap<String, String>,
}

/// Categories of auditable events
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuditEventType {
    // Agent actions
    /// An action was successfully executed
    ActionExecuted,

    /// An action was denied by policy
    ActionDenied,

    /// User confirmed a high-risk action
    ActionConfirmed,

    // Policy events
    /// Policy engine evaluated a request
    PolicyEvaluated,

    /// Policy violation detected
    PolicyViolation,

    // Skill events
    /// A skill was invoked
    SkillInvoked,

    /// A skill was installed
    SkillInstalled,

    /// A skill was removed
    SkillRemoved,

    // Security events
    /// Authentication attempt (success or failure)
    AuthAttempt,

    /// Credential access (encrypted store)
    CredentialAccess,

    // System events
    /// Nova agent started
    SystemStartup,

    /// Nova agent stopped
    SystemShutdown,

    /// Configuration was modified
    ConfigChanged,
}

impl AuditEvent {
    /// Create a new audit event with a generated UUID and current timestamp
    pub fn new(
        event_type: AuditEventType,
        actor: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("System time before Unix epoch")
                .as_millis() as u64,
            event_type,
            actor: actor.into(),
            description: description.into(),
            metadata: HashMap::new(),
        }
    }

    /// Create an event with metadata
    pub fn with_metadata(
        event_type: AuditEventType,
        actor: impl Into<String>,
        description: impl Into<String>,
        metadata: HashMap<String, String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("System time before Unix epoch")
                .as_millis() as u64,
            event_type,
            actor: actor.into(),
            description: description.into(),
            metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_creation() {
        let event = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Sent SMS message");

        assert_eq!(event.event_type, AuditEventType::ActionExecuted);
        assert_eq!(event.actor, "agent");
        assert_eq!(event.description, "Sent SMS message");
        assert!(!event.id.is_empty());
        assert!(event.timestamp > 0);
        assert!(event.metadata.is_empty());
    }

    #[test]
    fn test_audit_event_with_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert("recipient".to_string(), "+1234567890".to_string());
        metadata.insert("app".to_string(), "messages".to_string());

        let event = AuditEvent::with_metadata(
            AuditEventType::ActionExecuted,
            "agent",
            "Sent SMS message",
            metadata.clone(),
        );

        assert_eq!(event.metadata.len(), 2);
        assert_eq!(
            event.metadata.get("recipient"),
            Some(&"+1234567890".to_string())
        );
        assert_eq!(event.metadata.get("app"), Some(&"messages".to_string()));
    }

    #[test]
    fn test_audit_event_type_serialization() {
        let types = vec![
            AuditEventType::ActionExecuted,
            AuditEventType::ActionDenied,
            AuditEventType::ActionConfirmed,
            AuditEventType::PolicyEvaluated,
            AuditEventType::PolicyViolation,
            AuditEventType::SkillInvoked,
            AuditEventType::SkillInstalled,
            AuditEventType::SkillRemoved,
            AuditEventType::AuthAttempt,
            AuditEventType::CredentialAccess,
            AuditEventType::SystemStartup,
            AuditEventType::SystemShutdown,
            AuditEventType::ConfigChanged,
        ];

        for event_type in types {
            let json = serde_json::to_string(&event_type).expect("Failed to serialize");
            let deserialized: AuditEventType =
                serde_json::from_str(&json).expect("Failed to deserialize");
            assert_eq!(event_type, deserialized);
        }
    }

    #[test]
    fn test_audit_event_serialization_roundtrip() {
        let event = AuditEvent::new(
            AuditEventType::SkillInvoked,
            "skill:camera",
            "Captured photo",
        );

        let json = serde_json::to_string(&event).expect("Failed to serialize");
        let deserialized: AuditEvent = serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(event.id, deserialized.id);
        assert_eq!(event.timestamp, deserialized.timestamp);
        assert_eq!(event.event_type, deserialized.event_type);
        assert_eq!(event.actor, deserialized.actor);
        assert_eq!(event.description, deserialized.description);
    }
}
