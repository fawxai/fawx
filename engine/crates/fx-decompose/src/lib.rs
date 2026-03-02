use fx_core::signals::Signal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubGoal {
    pub description: String,
    pub required_tools: Vec<String>,
    pub expected_output: String,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::signals::{LoopStep, SignalKind};

    fn sample_sub_goal() -> SubGoal {
        SubGoal {
            description: "Summarize issue history".to_string(),
            required_tools: vec!["gh".to_string(), "read_file".to_string()],
            expected_output: "Summary of issue events".to_string(),
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

        assert!(matches!(completed, SubGoalOutcome::Completed(text) if text == "ok"));
        assert!(matches!(failed, SubGoalOutcome::Failed(text) if text == "boom"));
        assert!(matches!(exhausted, SubGoalOutcome::BudgetExhausted));
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
            expected_output: "Plain text".to_string(),
        };

        let encoded = serde_json::to_string(&goal).expect("serialize goal");
        let decoded: SubGoal = serde_json::from_str(&encoded).expect("deserialize goal");
        assert!(decoded.required_tools.is_empty());
    }
}
