use crate::context::DecompositionContext;
use crate::error::DecomposeError;
use crate::{AggregationStrategy, ComplexityHint, DecompositionPlan, SubGoal};
use fx_core::signals::Signal;

#[async_trait::async_trait]
pub trait Decomposer: Send + Sync {
    async fn decompose(
        &self,
        signal: &Signal,
        context: &DecompositionContext,
    ) -> Result<DecompositionPlan, DecomposeError>;
}

/// LLM-driven decomposer. Sends signal + context to a model, parses JSON response.
pub struct LlmDecomposer {
    model: String,
}

impl LlmDecomposer {
    pub fn new(model: String) -> Self {
        Self { model }
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

#[async_trait::async_trait]
impl Decomposer for LlmDecomposer {
    async fn decompose(
        &self,
        signal: &Signal,
        _context: &DecompositionContext,
    ) -> Result<DecompositionPlan, DecomposeError> {
        Err(DecomposeError::DecompositionFailed(format!(
            "LlmDecomposer requires a ModelRouter (not yet wired): signal={}, model={}",
            signal.message, self.model
        )))
    }
}

/// Validate and potentially truncate a decomposition plan against budget constraints.
pub fn validate_plan(
    mut plan: DecompositionPlan,
    context: &DecompositionContext,
) -> Result<DecompositionPlan, DecomposeError> {
    let total_weight = total_complexity_weight(&plan.sub_goals);
    if total_weight > context.max_complexity_weight {
        return Err(DecomposeError::BudgetExceeded(format!(
            "total complexity weight {total_weight} exceeds max {}",
            context.max_complexity_weight
        )));
    }
    if plan.sub_goals.len() > context.max_sub_goals {
        let original = plan.sub_goals.len();
        plan.sub_goals.truncate(context.max_sub_goals);
        plan.truncated_from = Some(original);
    }
    Ok(plan)
}

fn total_complexity_weight(goals: &[SubGoal]) -> u32 {
    goals
        .iter()
        .map(|goal| {
            goal.complexity_hint
                .unwrap_or(ComplexityHint::Moderate)
                .weight()
        })
        .sum()
}

/// Parse a decomposition plan from JSON text.
pub fn parse_plan_json(text: &str) -> Result<DecompositionPlan, DecomposeError> {
    serde_json::from_str(text).map_err(|error| {
        DecomposeError::DecompositionFailed(format!("failed to parse plan JSON: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::signals::{LoopStep, SignalKind};

    fn simple_plan(count: usize) -> DecompositionPlan {
        DecompositionPlan {
            sub_goals: (0..count)
                .map(|index| SubGoal {
                    description: format!("Goal {index}"),
                    required_tools: vec![],
                    expected_output: None,
                    complexity_hint: Some(ComplexityHint::Moderate),
                })
                .collect(),
            strategy: AggregationStrategy::Sequential,
            truncated_from: None,
        }
    }

    fn sample_signal() -> Signal {
        Signal {
            step: LoopStep::Act,
            kind: SignalKind::Success,
            message: "test signal".to_owned(),
            metadata: serde_json::json!({"source": "test"}),
            timestamp_ms: 42,
        }
    }

    #[test]
    fn validate_plan_passes_within_budget() {
        let plan = simple_plan(3);
        let context = DecompositionContext::default();
        let result = validate_plan(plan, &context).unwrap();
        assert_eq!(result.sub_goals.len(), 3);
        assert!(result.truncated_from.is_none());
    }

    #[test]
    fn validate_plan_truncates_excess_goals() {
        let plan = simple_plan(12);
        let context = DecompositionContext {
            max_sub_goals: 5,
            max_complexity_weight: 100,
            ..DecompositionContext::default()
        };
        let result = validate_plan(plan, &context).unwrap();
        assert_eq!(result.sub_goals.len(), 5);
        assert_eq!(result.truncated_from, Some(12));
    }

    #[test]
    fn validate_plan_rejects_over_weight() {
        let mut plan = simple_plan(3);
        for goal in &mut plan.sub_goals {
            goal.complexity_hint = Some(ComplexityHint::Complex);
        }
        let context = DecompositionContext {
            max_complexity_weight: 10,
            ..DecompositionContext::default()
        };
        let error = validate_plan(plan, &context).unwrap_err();
        assert!(error.to_string().contains("complexity weight"));
    }

    #[test]
    fn parse_plan_json_valid() {
        let json = r#"{
            "sub_goals": [
                {"description": "Fix bug", "required_tools": ["read_file"]}
            ],
            "strategy": "Sequential"
        }"#;
        let plan = parse_plan_json(json).unwrap();
        assert_eq!(plan.sub_goals.len(), 1);
        assert_eq!(plan.sub_goals[0].description, "Fix bug");
    }

    #[test]
    fn parse_plan_json_invalid() {
        let error = parse_plan_json("not json").unwrap_err();
        assert!(error.to_string().contains("failed to parse"));
    }

    #[tokio::test]
    async fn llm_decomposer_returns_not_wired_error() {
        let context = DecompositionContext::default();
        let decomposer = LlmDecomposer::new("test-model".to_owned());

        let error = decomposer
            .decompose(&sample_signal(), &context)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not yet wired"));
    }
}
