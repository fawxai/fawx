pub mod aggregator;
pub mod context;
pub mod dag;
pub mod dispatcher;
pub mod engine;
pub mod error;

use fx_core::signals::Signal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ComplexityHint {
    Trivial,
    Moderate,
    Complex,
}

impl ComplexityHint {
    pub const fn weight(self) -> u32 {
        match self {
            Self::Trivial => 1,
            Self::Moderate => 2,
            Self::Complex => 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubGoal {
    pub description: String,
    pub required_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity_hint: Option<ComplexityHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecompositionPlan {
    pub sub_goals: Vec<SubGoal>,
    pub strategy: AggregationStrategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated_from: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AggregationStrategy {
    Sequential,
    Parallel,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubGoalResult {
    pub goal: SubGoal,
    pub outcome: SubGoalOutcome,
    pub signals: Vec<Signal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubGoalOutcome {
    Completed(String),
    Failed(String),
    BudgetExhausted,
    Skipped,
}

pub use aggregator::{
    AggregatedResult, BuildVerifyAggregator, DefaultWorkspaceProvider, MergeResult,
    MergeTestResult, PatchApplyError, ResultAggregator, SimpleAggregator, TempWorkspace,
    WorkspaceProvider,
};
pub use context::{
    AttemptDecision, ChainEntry, DecompositionAttempt, DecompositionContext, Experiment,
    FitnessContext, FitnessStats, PathPattern, SubGoalAttempt, SubGoalAttemptOutcome,
};
pub use dag::ExecutionDag;
#[cfg(any(test, feature = "test-support"))]
pub use dispatcher::MockSubGoalExecutor;
pub use dispatcher::{
    DagDispatcher, DecompositionEvent, DecompositionProgressCallback, ParallelDispatcher,
    SequentialDispatcher, SubGoalDispatcher, SubGoalExecutor,
};
pub use engine::{
    format_fitness_context, parse_plan_json, validate_plan, Decomposer, LlmDecomposer,
};
pub use error::DecomposeError;

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::signals::{LoopStep, SignalKind};

    fn sample_sub_goal() -> SubGoal {
        SubGoal {
            description: "Summarize issue history".to_string(),
            required_tools: vec!["gh".to_string(), "read_file".to_string()],
            expected_output: Some("Summary of issue events".to_string()),
            complexity_hint: Some(ComplexityHint::Moderate),
        }
    }

    fn sample_signal() -> Signal {
        Signal {
            step: LoopStep::Act,
            kind: SignalKind::Success,
            message: "sub-goal completed".to_string(),
            metadata: serde_json::json!({"test": true}),
            timestamp_ms: 42,
        }
    }

    #[test]
    fn sub_goal_roundtrip_serde() {
        let original = sample_sub_goal();
        let encoded = serde_json::to_string(&original).expect("serialize sub-goal");
        let decoded: SubGoal = serde_json::from_str(&encoded).expect("deserialize sub-goal");
        assert_eq!(decoded, original);
    }

    #[test]
    fn decomposition_plan_roundtrip_serde() {
        let original = DecompositionPlan {
            sub_goals: vec![sample_sub_goal()],
            strategy: AggregationStrategy::Sequential,
            truncated_from: None,
        };

        let encoded = serde_json::to_string(&original).expect("serialize plan");
        let decoded: DecompositionPlan = serde_json::from_str(&encoded).expect("deserialize plan");
        assert_eq!(decoded, original);
    }

    #[test]
    fn sub_goal_result_roundtrip_serde() {
        let original = SubGoalResult {
            goal: sample_sub_goal(),
            outcome: SubGoalOutcome::Completed("done".to_string()),
            signals: vec![sample_signal()],
        };

        let encoded = serde_json::to_string(&original).expect("serialize result");
        let decoded: SubGoalResult =
            serde_json::from_str(&encoded).expect("deserialize sub-goal result");
        assert_eq!(decoded, original);
    }

    #[test]
    fn aggregation_strategy_variants_serialize_correctly() {
        let sequential = serde_json::to_value(AggregationStrategy::Sequential).expect("seq");
        let parallel = serde_json::to_value(AggregationStrategy::Parallel).expect("par");
        let custom = serde_json::to_value(AggregationStrategy::Custom("fan-in".to_string()))
            .expect("custom");

        assert_eq!(sequential, serde_json::json!("Sequential"));
        assert_eq!(parallel, serde_json::json!("Parallel"));
        assert_eq!(custom, serde_json::json!({"Custom": "fan-in"}));
    }

    #[test]
    fn sub_goal_outcome_variants_cover_all_cases() {
        let completed = SubGoalOutcome::Completed("ok".to_string());
        let failed = SubGoalOutcome::Failed("boom".to_string());
        let exhausted = SubGoalOutcome::BudgetExhausted;
        let skipped = SubGoalOutcome::Skipped;

        assert!(matches!(completed, SubGoalOutcome::Completed(text) if text == "ok"));
        assert!(matches!(failed, SubGoalOutcome::Failed(text) if text == "boom"));
        assert!(matches!(exhausted, SubGoalOutcome::BudgetExhausted));
        assert!(matches!(skipped, SubGoalOutcome::Skipped));
    }

    #[test]
    fn empty_sub_goals_list_roundtrip_serde() {
        let plan = DecompositionPlan {
            sub_goals: Vec::new(),
            strategy: AggregationStrategy::Sequential,
            truncated_from: None,
        };

        let encoded = serde_json::to_string(&plan).expect("serialize empty plan");
        let decoded: DecompositionPlan =
            serde_json::from_str(&encoded).expect("deserialize empty plan");
        assert!(decoded.sub_goals.is_empty());
    }

    #[test]
    fn required_tools_empty_vec_roundtrip_serde() {
        let goal = SubGoal {
            description: "No tool task".to_string(),
            required_tools: Vec::new(),
            expected_output: Some("Plain text".to_string()),
            complexity_hint: None,
        };

        let encoded = serde_json::to_string(&goal).expect("serialize goal");
        let decoded: SubGoal = serde_json::from_str(&encoded).expect("deserialize goal");
        assert!(decoded.required_tools.is_empty());
    }

    #[test]
    fn expected_output_missing_deserializes_to_none() {
        let encoded = serde_json::json!({
            "description": "Summarize findings",
            "required_tools": ["read_file"]
        });

        let decoded: SubGoal = serde_json::from_value(encoded).expect("deserialize goal");

        assert_eq!(decoded.expected_output, None);
    }

    #[test]
    fn expected_output_none_is_omitted_from_serialization() {
        let goal = SubGoal {
            description: "Summarize findings".to_string(),
            required_tools: Vec::new(),
            expected_output: None,
            complexity_hint: None,
        };

        let encoded = serde_json::to_value(&goal).expect("serialize goal");

        assert!(encoded.get("expected_output").is_none());
    }

    #[test]
    fn sub_goal_with_complexity_hint_roundtrip_serde() {
        let goal = SubGoal {
            description: "Implement adaptive budget allocator".to_string(),
            required_tools: vec!["read_file".to_string()],
            expected_output: Some("patch".to_string()),
            complexity_hint: Some(ComplexityHint::Complex),
        };

        let encoded = serde_json::to_string(&goal).expect("serialize sub-goal");
        let decoded: SubGoal = serde_json::from_str(&encoded).expect("deserialize sub-goal");

        assert_eq!(decoded.complexity_hint, Some(ComplexityHint::Complex));
        assert_eq!(decoded, goal);
    }

    #[test]
    fn complexity_hint_weight_values_are_stable() {
        assert_eq!(ComplexityHint::Trivial.weight(), 1);
        assert_eq!(ComplexityHint::Moderate.weight(), 2);
        assert_eq!(ComplexityHint::Complex.weight(), 4);
    }

    #[test]
    fn sub_goal_outcome_skipped_roundtrip_serde() {
        let encoded = serde_json::to_string(&SubGoalOutcome::Skipped).expect("serialize skipped");
        let decoded: SubGoalOutcome = serde_json::from_str(&encoded).expect("deserialize skipped");
        assert_eq!(decoded, SubGoalOutcome::Skipped);
    }
}
