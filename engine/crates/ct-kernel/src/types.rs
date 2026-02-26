//! Core loop types for the Citros kernel.
//!
//! These types model the recursive seven-step loop:
//! perceive → reason → decide → act → verify → learn → continue.

use ct_core::types::{Notification, ScreenState, SwipeDirection, UserInput};
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

/// Result produced by Verify.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationResult {
    /// Verification succeeded with evidence strings.
    Confirmed { evidence: Vec<String> },
    /// Verification failed and recommends recovery.
    Failed {
        reason: String,
        recovery: RecoveryStrategy,
    },
    /// Verification could not determine success/failure.
    Inconclusive { reason: String },
}

/// Recovery strategy for failed verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryStrategy {
    /// Retry the same action.
    Retry,
    /// Re-run planning/reasoning.
    Replan,
    /// Gather additional evidence with a focused goal.
    GatherEvidence(Goal),
    /// Escalate to user/system with context.
    Escalate(String),
    /// Abort execution with explicit reason.
    Abort(String),
}

/// Output produced by Learn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningOutcome {
    /// Episodic memories to persist.
    pub episodic_entries: Vec<EpisodicEntry>,
    /// Proposed semantic facts for consolidation.
    pub semantic_proposals: Vec<SemanticProposal>,
    /// Proposed reusable procedures.
    pub procedural_proposals: Vec<ProceduralProposal>,
}

/// Decision emitted by Continue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContinuationDecision {
    /// Goal completed with evidence.
    Complete(LoopEvidence),
    /// Continue the loop, optionally refreshing perception.
    Continue { next_perception: bool },
    /// Loop failed with unrecoverable error.
    Failed(LoopError),
    /// User input is required before proceeding.
    NeedsUser(EscalationContext),
    /// Recursion depth exceeded safety threshold.
    DepthExceeded,
    /// Budget has been exhausted.
    BudgetExhausted,
}

/// Loop completion evidence returned to callers/audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopEvidence {
    /// Goal that was completed.
    pub goal: Goal,
    /// Optional final perception captured at completion.
    pub final_perception: Option<PerceptionSnapshot>,
    /// Verification result supporting completion.
    pub verification: Option<VerificationResult>,
    /// Additional evidence notes/artifacts.
    pub evidence: Vec<String>,
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

/// Context used when escalation to the user is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationContext {
    /// Prompt/question for the user.
    pub prompt: String,
    /// Optional associated goal.
    pub goal: Option<Goal>,
    /// Optional planned intent awaiting confirmation/input.
    pub proposed_intent: Option<ReasonedIntent>,
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
    /// Key/value preference pairs.
    pub preferences: Vec<(String, String)>,
    /// Personality or style traits relevant to behavior.
    pub personality_traits: Vec<String>,
}

/// Episodic memory candidate extracted during Learn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicEntry {
    /// One-line summary of the experience.
    pub summary: String,
    /// Importance score in range 0.0..=1.0.
    pub importance: f32,
    /// Entities involved in the episode.
    pub entities: Vec<String>,
    /// Action taken by the agent.
    pub action_taken: String,
    /// Outcome after execution.
    pub outcome: String,
}

/// Semantic fact candidate proposed by Learn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticProposal {
    /// Candidate fact.
    pub fact: String,
    /// Confidence score in the fact.
    pub confidence: f32,
    /// Source tag (e.g. "inferred", "user_stated").
    pub source: String,
}

/// Procedure candidate proposed by Learn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralProposal {
    /// Procedure name.
    pub name: String,
    /// Trigger condition.
    pub trigger: String,
    /// Step-by-step procedure text.
    pub steps: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_core::types::InputSource;

    #[test]
    fn perception_snapshot_serialization_roundtrip() {
        let snapshot = PerceptionSnapshot {
            screen: ScreenState {
                current_app: "com.example.app".to_owned(),
                elements: vec![ct_core::types::UiElement {
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
            }),
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
            vec!["Messages app is foreground".to_owned(), "Reply sent".to_owned()],
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
    fn verification_and_recovery_patterns() {
        let gather_goal = Goal::new(
            "Collect more UI evidence",
            vec!["Have at least one screenshot".to_owned()],
            Some(2),
        );

        let failed = VerificationResult::Failed {
            reason: "Expected element not visible".to_owned(),
            recovery: RecoveryStrategy::GatherEvidence(gather_goal.clone()),
        };

        match failed {
            VerificationResult::Failed { reason, recovery } => {
                assert_eq!(reason, "Expected element not visible");
                match recovery {
                    RecoveryStrategy::GatherEvidence(goal) => {
                        assert_eq!(goal.description, gather_goal.description);
                    }
                    _ => panic!("expected gather-evidence recovery"),
                }
            }
            _ => panic!("expected failed verification"),
        }

        let confirmed = VerificationResult::Confirmed {
            evidence: vec!["Element 'Send' is visible".to_owned()],
        };
        assert!(matches!(
            confirmed,
            VerificationResult::Confirmed { evidence } if evidence.len() == 1
        ));
    }

    #[test]
    fn continuation_decision_variants_are_constructible() {
        let goal = Goal::new("Complete task", vec!["Task complete".to_owned()], Some(3));

        let complete = ContinuationDecision::Complete(LoopEvidence {
            goal: goal.clone(),
            final_perception: None,
            verification: Some(VerificationResult::Confirmed {
                evidence: vec!["Task marker detected".to_owned()],
            }),
            evidence: vec!["marker:task_complete".to_owned()],
        });
        assert!(matches!(complete, ContinuationDecision::Complete(_)));

        let continue_decision = ContinuationDecision::Continue {
            next_perception: true,
        };
        assert!(matches!(
            continue_decision,
            ContinuationDecision::Continue {
                next_perception: true
            }
        ));

        let failed = ContinuationDecision::Failed(LoopError {
            stage: "act".to_owned(),
            reason: "executor unavailable".to_owned(),
            recoverable: false,
        });
        assert!(matches!(failed, ContinuationDecision::Failed(_)));

        let needs_user = ContinuationDecision::NeedsUser(EscalationContext {
            prompt: "Should I proceed with deleting this draft?".to_owned(),
            goal: Some(goal),
            proposed_intent: Some(ReasonedIntent {
                action: IntendedAction::Respond {
                    text: "Awaiting confirmation".to_owned(),
                },
                rationale: "destructive action requires approval".to_owned(),
                confidence: 0.92,
                expected_outcome: None,
                sub_goals: vec![],
            }),
        });
        assert!(matches!(needs_user, ContinuationDecision::NeedsUser(_)));

        assert!(matches!(
            ContinuationDecision::DepthExceeded,
            ContinuationDecision::DepthExceeded
        ));
        assert!(matches!(
            ContinuationDecision::BudgetExhausted,
            ContinuationDecision::BudgetExhausted
        ));
    }
}
