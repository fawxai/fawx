//! Core loop types for the Fawx kernel.
//!
//! These types model the recursive seven-step loop:
//! perceive → reason → decide → act → verify → learn → continue.

use fx_core::types::{Notification, ScreenState, SwipeDirection, UserInput};
use fx_llm::Message;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A full perception snapshot received from Kotlin/FFI each loop cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptionSnapshot {
    /// Current screen content.
    pub screen: ScreenState,
    /// Active notifications visible to the agent.
    pub notifications: Vec<Notification>,
    /// Foreground app package/activity identifier.
    pub active_app: String,
    /// Unix timestamp for the snapshot in milliseconds.
    pub timestamp_ms: u64,
    /// Optional sensor stream data for future expansion.
    pub sensor_data: Option<SensorData>,
    /// User-initiated input that triggered this cycle (if any).
    pub user_input: Option<UserInput>,
    /// Prior conversation turns retained for context-window construction.
    #[serde(default)]
    pub conversation_history: Vec<Message>,
    /// Latest user steering guidance queued for this turn.
    #[serde(default)]
    pub steer_context: Option<String>,
}

/// Sensor data captured alongside perception.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorData {
    /// Optional geolocation reading.
    pub location: Option<Location>,
    /// Optional battery percentage.
    pub battery_percent: Option<u8>,
}

/// Geographic location reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// Latitude in decimal degrees.
    pub latitude: f64,
    /// Longitude in decimal degrees.
    pub longitude: f64,
    /// Estimated horizontal accuracy in meters.
    pub accuracy_meters: f32,
}

/// Reasoning context assembled by the Perceive step and consumed by Reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningContext {
    /// Fresh perception for this loop turn.
    pub perception: PerceptionSnapshot,
    /// Short-lived key/value working memory.
    pub working_memory: Vec<WorkingMemoryEntry>,
    /// Retrieved episodic memory summaries relevant to the current goal.
    pub relevant_episodic: Vec<EpisodicMemoryRef>,
    /// Retrieved semantic facts relevant to the current goal.
    pub relevant_semantic: Vec<SemanticMemoryRef>,
    /// Active procedures loaded for this context.
    pub active_procedures: Vec<ProcedureRef>,
    /// User identity and preference context.
    pub identity_context: IdentityContext,
    /// Goal for this recursive invocation.
    pub goal: Goal,
    /// Current recursion depth.
    pub depth: u32,
    /// Parent reasoning context for recursive decomposition.
    pub parent_context: Option<Box<ReasoningContext>>,
}

impl ReasoningContext {
    /// Validate that parent chain depth matches the depth field.
    pub fn validate_depth(&self) -> bool {
        let mut chain_len: u32 = 0;
        let mut current = &self.parent_context;
        while let Some(parent) = current {
            chain_len += 1;
            current = &parent.parent_context;
        }
        chain_len == self.depth
    }
}

/// What this loop invocation is trying to achieve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    /// Human-readable objective statement.
    pub description: String,
    /// Observable criteria used to determine success.
    pub success_criteria: Vec<String>,
    /// Optional guardrail on maximum steps for this invocation.
    pub max_steps: Option<u32>,
}

impl Goal {
    /// Construct a goal instance.
    pub fn new(
        description: impl Into<String>,
        success_criteria: Vec<String>,
        max_steps: Option<u32>,
    ) -> Self {
        Self {
            description: description.into(),
            success_criteria,
            max_steps,
        }
    }

    /// Validate a goal definition.
    pub fn validate(&self) -> Result<(), String> {
        if self.description.trim().is_empty() {
            return Err("goal description must not be empty".to_owned());
        }

        if self.success_criteria.is_empty() {
            return Err("goal must include at least one success criterion".to_owned());
        }

        if self
            .success_criteria
            .iter()
            .any(|criterion| criterion.trim().is_empty())
        {
            return Err("success criteria entries must not be empty".to_owned());
        }

        if matches!(self.max_steps, Some(0)) {
            return Err("max_steps must be greater than zero when provided".to_owned());
        }

        Ok(())
    }

    /// Convenience boolean helper for quick checks.
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }
}

/// Intent produced by the Reason step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasonedIntent {
    /// Action selected for execution.
    pub action: IntendedAction,
    /// Natural language rationale explaining the plan.
    pub rationale: String,
    /// Confidence score in range 0.0..=1.0.
    pub confidence: f32,
    /// Expected result used by the Verify step.
    pub expected_outcome: Option<ExpectedOutcome>,
    /// Recursive decomposition targets.
    pub sub_goals: Vec<Goal>,
}

/// Action schema consumed by the Act step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntendedAction {
    /// Tap a target element, with optional fallback selector.
    Tap {
        target: String,
        fallback: Option<String>,
    },
    /// Type text into a target element.
    Type { text: String, target: String },
    /// Swipe relative to a target (or globally when target is None).
    Swipe {
        direction: SwipeDirection,
        target: Option<String>,
    },
    /// Launch an app by package identifier.
    LaunchApp { package: String },
    /// Navigate toward a semantic destination.
    Navigate { destination: String },
    /// Wait for a condition with timeout.
    Wait { condition: String, timeout_ms: u64 },
    /// Respond to the user via voice/display surface.
    Respond { text: String },
    /// Delegate execution to another skill with parameter map.
    Delegate {
        skill_id: String,
        params: HashMap<String, String>,
    },
    /// Composite action set for multi-step execution.
    Composite(Vec<IntendedAction>),
}

/// Expected output contract for Verify.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedOutcome {
    /// Human-readable expectation summary.
    pub description: String,
    /// Artifact checks to execute for verification.
    pub artifact_checks: Vec<ArtifactCheck>,
}

/// Artifact-level checks used during verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArtifactCheck {
    /// Verify that visible screen text includes a string.
    ScreenContains(String),
    /// Verify that the screen changed from prior state.
    ScreenChanged,
    /// Verify foreground app package/activity.
    AppInForeground(String),
    /// Verify element is visible.
    ElementVisible(String),
    /// Verify element disappeared.
    ElementGone(String),
    /// Custom check expression for specialized validators.
    Custom(String),
}

/// Combined result of the Decide step's three gates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionGateResult {
    /// Policy gate decision.
    pub policy_result: GateResult,
    /// Budget gate decision.
    pub budget_result: GateResult,
    /// Permission gate decision.
    pub permission_result: GateResult,
}

/// Outcome from a single decision gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GateResult {
    /// Gate approved the action.
    Approved,
    /// Gate denied the action with reason.
    Denied { reason: String },
    /// Gate requires explicit user confirmation.
    NeedsConfirmation { prompt: String },
}

/// Error describing an unrecoverable loop failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopError {
    /// Loop stage where the error originated.
    pub stage: String,
    /// Human-readable failure reason.
    pub reason: String,
    /// Whether the error may be recoverable in a future attempt.
    pub recoverable: bool,
}

/// Working-memory reference entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingMemoryEntry {
    /// Memory key.
    pub key: String,
    /// Memory value summary.
    pub value: String,
    /// Relevance score for current context.
    pub relevance: f32,
}

/// Lightweight episodic memory summary/reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicMemoryRef {
    /// Stable episodic memory identifier.
    pub id: u64,
    /// Human-readable episode summary.
    pub summary: String,
    /// Relevance score for current context.
    pub relevance: f32,
    /// Episode timestamp in milliseconds.
    pub timestamp_ms: u64,
}

/// Lightweight semantic memory summary/reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMemoryRef {
    /// Stable semantic memory identifier.
    pub id: u64,
    /// Fact text.
    pub fact: String,
    /// Confidence score for the fact.
    pub confidence: f32,
}

/// Reference to an active/loadable procedure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureRef {
    /// Procedure identifier.
    pub id: String,
    /// Human-readable procedure name.
    pub name: String,
    /// Procedure version.
    pub version: u32,
}

/// Identity and preference context for reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityContext {
    /// Optional preferred name of the user.
    pub user_name: Option<String>,
    /// Key/value preference map.
    pub preferences: HashMap<String, String>,
    /// Personality or style traits relevant to behavior.
    pub personality_traits: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::types::InputSource;

    #[test]
    fn perception_snapshot_serialization_roundtrip() {
        let snapshot = PerceptionSnapshot {
            screen: ScreenState {
                current_app: "com.example.app".to_owned(),
                elements: vec![fx_core::types::UiElement {
                    id: "button_ok".to_owned(),
                    element_type: "button".to_owned(),
                    text: "OK".to_owned(),
                    clickable: true,
                }],
                text_content: "Welcome".to_owned(),
            },
            notifications: vec![Notification {
                id: "notif-1".to_owned(),
                app: "com.chat".to_owned(),
                title: "New message".to_owned(),
                content: "Hey there".to_owned(),
                timestamp: 1_700_000_000_000,
            }],
            active_app: "com.example.app".to_owned(),
            timestamp_ms: 1_700_000_000_123,
            sensor_data: Some(SensorData {
                location: Some(Location {
                    latitude: 40.7128,
                    longitude: -74.0060,
                    accuracy_meters: 3.5,
                }),
                battery_percent: Some(87),
            }),
            user_input: Some(UserInput {
                text: "open messages".to_owned(),
                source: InputSource::Voice,
                timestamp: 1_700_000_000_122,
                context_id: Some("ctx-123".to_owned()),
                images: Vec::new(),
                documents: Vec::new(),
            }),
            conversation_history: Vec::new(),
            steer_context: None,
        };

        let encoded = serde_json::to_string(&snapshot).expect("serialize snapshot");
        let decoded: PerceptionSnapshot =
            serde_json::from_str(&encoded).expect("deserialize snapshot");

        assert_eq!(decoded.active_app, snapshot.active_app);
        assert_eq!(decoded.timestamp_ms, snapshot.timestamp_ms);
        assert_eq!(decoded.screen.current_app, snapshot.screen.current_app);
        assert_eq!(decoded.screen.text_content, snapshot.screen.text_content);
        assert_eq!(decoded.notifications.len(), 1);
        assert_eq!(decoded.notifications[0].title, "New message");
        assert_eq!(
            decoded
                .sensor_data
                .as_ref()
                .and_then(|sensor| sensor.battery_percent),
            Some(87)
        );
        assert_eq!(
            decoded.user_input.as_ref().map(|input| input.text.as_str()),
            Some("open messages")
        );
    }

    #[test]
    fn goal_creation_and_validation() {
        let goal = Goal::new(
            "Respond to latest unread message",
            vec![
                "Messages app is foreground".to_owned(),
                "Reply sent".to_owned(),
            ],
            Some(5),
        );

        assert!(goal.is_valid());
        assert!(goal.validate().is_ok());

        let invalid_description = Goal::new("   ", vec!["something".to_owned()], Some(2));
        assert!(invalid_description.validate().is_err());

        let invalid_steps = Goal::new("Do thing", vec!["done".to_owned()], Some(0));
        assert!(invalid_steps.validate().is_err());

        let invalid_criteria = Goal::new("Do thing", vec![" ".to_owned()], Some(1));
        assert!(invalid_criteria.validate().is_err());
    }

    #[test]
    fn reasoning_context_depth_validation_and_roundtrip() {
        let mut parent_preferences = HashMap::new();
        parent_preferences.insert("lang".to_owned(), "en".to_owned());

        let parent_context = ReasoningContext {
            perception: PerceptionSnapshot {
                screen: ScreenState {
                    current_app: "com.example.mail".to_owned(),
                    elements: vec![],
                    text_content: "Inbox".to_owned(),
                },
                notifications: vec![],
                active_app: "com.example.mail".to_owned(),
                timestamp_ms: 1_700_000_000_001,
                sensor_data: None,
                user_input: None,
                conversation_history: Vec::new(),
                steer_context: None,
            },
            working_memory: vec![WorkingMemoryEntry {
                key: "thread_id".to_owned(),
                value: "42".to_owned(),
                relevance: 0.8,
            }],
            relevant_episodic: vec![],
            relevant_semantic: vec![],
            active_procedures: vec![],
            identity_context: IdentityContext {
                user_name: Some("Example User".to_owned()),
                preferences: parent_preferences,
                personality_traits: vec!["concise".to_owned()],
            },
            goal: Goal::new(
                "Open latest unread email",
                vec!["Unread thread is visible".to_owned()],
                Some(3),
            ),
            depth: 0,
            parent_context: None,
        };

        let mut child_preferences = HashMap::new();
        child_preferences.insert("theme".to_owned(), "dark".to_owned());

        let context = ReasoningContext {
            perception: PerceptionSnapshot {
                screen: ScreenState {
                    current_app: "com.example.mail".to_owned(),
                    elements: vec![],
                    text_content: "Messages".to_owned(),
                },
                notifications: vec![],
                active_app: "com.example.mail".to_owned(),
                timestamp_ms: 1_700_000_000_123,
                sensor_data: None,
                user_input: None,
                conversation_history: Vec::new(),
                steer_context: None,
            },
            working_memory: vec![],
            relevant_episodic: vec![],
            relevant_semantic: vec![],
            active_procedures: vec![],
            identity_context: IdentityContext {
                user_name: Some("Example User".to_owned()),
                preferences: child_preferences,
                personality_traits: vec!["focused".to_owned()],
            },
            goal: Goal::new(
                "Summarize message and draft reply",
                vec!["Draft reply is prepared".to_owned()],
                Some(5),
            ),
            depth: 1,
            parent_context: Some(Box::new(parent_context)),
        };

        assert!(context.validate_depth());

        let encoded = serde_json::to_string(&context).expect("serialize reasoning context");
        let decoded: ReasoningContext =
            serde_json::from_str(&encoded).expect("deserialize reasoning context");

        assert_eq!(decoded.depth, 1);
        assert!(decoded.validate_depth());
        assert_eq!(
            decoded.goal.description,
            "Summarize message and draft reply"
        );
        assert_eq!(
            decoded
                .identity_context
                .preferences
                .get("theme")
                .map(String::as_str),
            Some("dark")
        );

        let parent = decoded
            .parent_context
            .as_ref()
            .expect("expected one parent context");
        assert_eq!(parent.goal.description, "Open latest unread email");
        assert_eq!(
            parent
                .identity_context
                .preferences
                .get("lang")
                .map(String::as_str),
            Some("en")
        );

        let mut mismatched_depth = decoded.clone();
        mismatched_depth.depth = 2;
        assert!(!mismatched_depth.validate_depth());
    }

    #[test]
    fn intended_action_delegate_roundtrip_preserves_params() {
        let mut params = HashMap::new();
        params.insert("query".to_owned(), "best ramen nearby".to_owned());
        params.insert("radius_m".to_owned(), "1500".to_owned());

        let action = IntendedAction::Delegate {
            skill_id: "local-search".to_owned(),
            params: params.clone(),
        };

        let encoded = serde_json::to_string(&action).expect("serialize delegate action");
        let decoded: IntendedAction =
            serde_json::from_str(&encoded).expect("deserialize delegate action");

        match decoded {
            IntendedAction::Delegate {
                skill_id,
                params: decoded_params,
            } => {
                assert_eq!(skill_id, "local-search");
                assert_eq!(decoded_params, params);
            }
            _ => panic!("expected delegate action"),
        }
    }

    #[test]
    fn reasoned_intent_roundtrip_with_expected_outcome_and_sub_goals() {
        let intent = ReasonedIntent {
            action: IntendedAction::Navigate {
                destination: "123 Main St".to_owned(),
            },
            rationale: "Navigate to the confirmed meeting location".to_owned(),
            confidence: 0.88,
            expected_outcome: Some(ExpectedOutcome {
                description: "Maps app shows route ETA".to_owned(),
                artifact_checks: vec![
                    ArtifactCheck::AppInForeground("com.maps.app".to_owned()),
                    ArtifactCheck::ScreenContains("ETA".to_owned()),
                ],
            }),
            sub_goals: vec![
                Goal::new(
                    "Open maps app",
                    vec!["Maps app is in foreground".to_owned()],
                    Some(2),
                ),
                Goal::new(
                    "Start route guidance",
                    vec!["Turn-by-turn guidance is active".to_owned()],
                    Some(3),
                ),
            ],
        };

        let encoded = serde_json::to_string(&intent).expect("serialize reasoned intent");
        let decoded: ReasonedIntent =
            serde_json::from_str(&encoded).expect("deserialize reasoned intent");

        assert_eq!(decoded.rationale, intent.rationale);
        assert_eq!(decoded.confidence, intent.confidence);
        assert_eq!(decoded.sub_goals.len(), 2);
        assert_eq!(decoded.sub_goals[0].description, "Open maps app");
        assert_eq!(decoded.sub_goals[1].description, "Start route guidance");

        let expected = decoded
            .expected_outcome
            .as_ref()
            .expect("expected expected_outcome to be present");
        assert_eq!(expected.description, "Maps app shows route ETA");
        assert_eq!(expected.artifact_checks.len(), 2);
        assert!(matches!(
            expected.artifact_checks[0],
            ArtifactCheck::AppInForeground(ref app) if app == "com.maps.app"
        ));
    }

    #[test]
    fn identity_context_preferences_serialize_as_object() {
        let mut preferences = HashMap::new();
        preferences.insert("theme".to_owned(), "dark".to_owned());
        preferences.insert("lang".to_owned(), "en".to_owned());

        let identity = IdentityContext {
            user_name: Some("Example User".to_owned()),
            preferences,
            personality_traits: vec!["friendly".to_owned()],
        };

        let encoded = serde_json::to_value(&identity).expect("serialize identity context");
        let preferences_value = encoded
            .get("preferences")
            .and_then(serde_json::Value::as_object)
            .expect("preferences should serialize as a JSON object");

        assert_eq!(
            preferences_value
                .get("theme")
                .and_then(serde_json::Value::as_str),
            Some("dark")
        );
        assert_eq!(
            preferences_value
                .get("lang")
                .and_then(serde_json::Value::as_str),
            Some("en")
        );
    }
}
