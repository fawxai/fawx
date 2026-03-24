use crate::context::DecompositionContext;
use crate::error::DecomposeError;
use crate::{ComplexityHint, DecompositionPlan, SubGoal};
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
        context: &DecompositionContext,
    ) -> Result<DecompositionPlan, DecomposeError> {
        Err(DecomposeError::DecompositionFailed(format!(
            "LlmDecomposer requires a ModelRouter (not yet wired): signal={}, model={}, max_sub_goals={}, max_complexity_weight={}",
            signal.message, self.model, context.max_sub_goals, context.max_complexity_weight
        )))
    }
}

/// Validate and potentially truncate a decomposition plan against budget constraints.
/// Truncation happens BEFORE weight check so plans with many goals but acceptable
/// post-truncation weight are not incorrectly rejected.
pub fn validate_plan(
    mut plan: DecompositionPlan,
    context: &DecompositionContext,
) -> Result<DecompositionPlan, DecomposeError> {
    context.validate()?;
    if plan.sub_goals.len() > context.max_sub_goals {
        let original = plan.sub_goals.len();
        plan.sub_goals.truncate(context.max_sub_goals);
        plan.truncated_from = Some(original);
    }
    let total_weight = total_complexity_weight(&plan.sub_goals);
    if total_weight > context.max_complexity_weight {
        return Err(DecomposeError::BudgetExceeded(format!(
            "total complexity weight {total_weight} exceeds max {}",
            context.max_complexity_weight
        )));
    }
    Ok(plan)
}

/// Computes total complexity weight for a set of sub-goals.
/// Goals without a complexity hint default to `Moderate` (weight 2).
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

/// Format fitness context for inclusion in LLM decomposition prompts.
pub fn format_fitness_context(fitness: &crate::context::FitnessContext) -> String {
    if fitness.prior_attempts.is_empty() {
        return "No prior decomposition attempts for this signal.".to_owned();
    }
    let mut lines = Vec::new();
    lines.push("Previous decomposition attempts for this signal:".to_owned());
    for (index, attempt) in fitness.prior_attempts.iter().enumerate() {
        lines.push(format!(
            "- Attempt {} ({}): {}, score {:.2}",
            index + 1,
            attempt.timestamp.format("%Y-%m-%d"),
            attempt.decision,
            attempt.best_score
        ));
        for sub_goal in &attempt.sub_goals {
            let score_str = sub_goal
                .score
                .map(|score| format!(" (score {score:.2})"))
                .unwrap_or_default();
            let fail_str = sub_goal
                .failure_reason
                .as_deref()
                .map(|reason| format!(" — {reason}"))
                .unwrap_or_default();
            lines.push(format!(
                "  - \"{}\": {}{}{}",
                sub_goal.description, sub_goal.outcome, score_str, fail_str
            ));
        }
    }
    push_fitness_stats(&mut lines, &fitness.stats);
    lines.join("\n")
}

fn push_fitness_stats(lines: &mut Vec<String>, stats: &crate::context::FitnessStats) {
    lines.push(format!(
        "\nStatistics: {} attempts, {} accepts, {} rejects, avg score {:.2}",
        stats.total_attempts, stats.accepts, stats.rejects, stats.avg_best_score
    ));
    if !stats.common_failures.is_empty() {
        let failures = stats
            .common_failures
            .iter()
            .take(5)
            .map(|(reason, count)| format!("\"{reason}\" ({count}x)"))
            .collect::<Vec<_>>();
        lines.push(format!("Common failures: {}", failures.join(", ")));
    }
    if !stats.successful_approaches.is_empty() {
        let approaches = stats
            .successful_approaches
            .iter()
            .take(10)
            .map(String::as_str)
            .collect::<Vec<_>>();
        lines.push(format!("Successful approaches: {}", approaches.join(", ")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AggregationStrategy;
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
    fn validate_plan_truncates_before_weight_check() {
        // 12 Moderate goals = weight 24, but after truncation to 5 = weight 10
        let plan = simple_plan(12);
        let context = DecompositionContext {
            max_sub_goals: 5,
            max_complexity_weight: 12,
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

    #[test]
    fn format_empty_fitness_context() {
        let fitness = crate::context::FitnessContext::default();
        let text = format_fitness_context(&fitness);
        assert!(text.contains("No prior"));
    }

    #[test]
    fn format_fitness_context_with_attempts() {
        let fitness = crate::context::FitnessContext {
            prior_attempts: vec![crate::context::DecompositionAttempt {
                timestamp: chrono::Utc::now(),
                sub_goals: vec![crate::context::SubGoalAttempt {
                    description: "optimize hot path".to_owned(),
                    outcome: crate::context::SubGoalAttemptOutcome::Completed,
                    score: Some(0.7),
                    failure_reason: None,
                }],
                decision: crate::context::AttemptDecision::Reject,
                best_score: 0.5,
            }],
            stats: crate::context::FitnessStats {
                total_attempts: 1,
                accepts: 0,
                rejects: 1,
                avg_best_score: 0.5,
                common_failures: vec![("build error".to_owned(), 1)],
                successful_approaches: vec!["optimize hot path".to_owned()],
            },
        };
        let text = format_fitness_context(&fitness);
        assert!(text.contains("Attempt 1"));
        assert!(text.contains("optimize hot path"));
        assert!(text.contains("Common failures"));
        assert!(text.contains("Successful approaches"));
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
