use super::{
    current_time_ms, loop_error, truncate_prompt_text, DecomposeToolArguments, LlmProvider,
    LoopEngine, DECOMPOSITION_DEPTH_LIMIT_RESPONSE, DECOMPOSITION_RESULTS_PREFIX, MAX_SUB_GOALS,
};
use crate::act::{ActionContinuation, ActionNextStep, ActionResult, TokenUsage};
use crate::budget::{
    effective_max_depth, estimate_complexity, ActionCost, AllocationMode, AllocationPlan,
    BudgetAllocator, BudgetConfig, BudgetRemaining, BudgetState, DepthMode,
    DEFAULT_LLM_CALL_COST_CENTS, DEFAULT_TOOL_INVOCATION_COST_CENTS,
};
use crate::decide::Decision;
use crate::signals::{LoopStep, SignalKind};
use crate::types::LoopError;
use fx_decompose::{
    AggregationStrategy, ComplexityHint, DecompositionPlan, SubGoal, SubGoalOutcome, SubGoalResult,
};
use fx_llm::Message;

impl LoopEngine {
    pub(super) async fn execute_decomposition(
        &mut self,
        decision: &Decision,
        plan: &DecompositionPlan,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Result<ActionResult, LoopError> {
        if self.budget.state() == BudgetState::Low {
            return Ok(self.budget_low_blocked_result(decision, "decomposition", &[]));
        }
        let timestamp_ms = current_time_ms();
        let effective_cap =
            self.effective_decomposition_depth_cap(&self.budget.remaining(timestamp_ms));
        if self.decomposition_depth_limited(effective_cap) {
            return Ok(self.depth_limited_decomposition_result(decision));
        }
        self.emit_decomposition_truncation(plan);
        let allocation = self.prepare_allocation_plan(plan, timestamp_ms, effective_cap);
        let results = self
            .execute_allocated_sub_goals(plan, &allocation, llm, context_messages)
            .await;
        Ok(build_decomposition_action(
            decision,
            aggregate_sub_goal_results(&results),
        ))
    }

    fn emit_decomposition_truncation(&mut self, plan: &DecompositionPlan) {
        if let Some(original_sub_goals) = plan.truncated_from {
            self.emit_decomposition_truncation_signal(original_sub_goals, plan.sub_goals.len());
        }
    }

    fn prepare_allocation_plan(
        &self,
        plan: &DecompositionPlan,
        timestamp_ms: u64,
        effective_cap: u32,
    ) -> AllocationPlan {
        let mode = allocation_mode_for_strategy(&plan.strategy);
        let mut allocation =
            BudgetAllocator::new().allocate(&self.budget, &plan.sub_goals, mode, timestamp_ms);
        self.apply_effective_depth_cap(&mut allocation.sub_goal_budgets, effective_cap);
        allocation
    }

    pub(super) fn decomposition_depth_limited(&self, effective_cap: u32) -> bool {
        self.budget.depth() >= effective_cap
    }

    pub(super) fn effective_decomposition_depth_cap(&self, remaining: &BudgetRemaining) -> u32 {
        let config = self.budget.config();
        match config.decompose_depth_mode {
            DepthMode::Static => config.max_recursion_depth,
            DepthMode::Adaptive => config
                .max_recursion_depth
                .min(effective_max_depth(remaining)),
        }
    }

    pub(super) fn apply_effective_depth_cap(
        &self,
        sub_goal_budgets: &mut [BudgetConfig],
        effective_cap: u32,
    ) {
        for budget in sub_goal_budgets {
            budget.max_recursion_depth = budget.max_recursion_depth.min(effective_cap);
        }
    }

    pub(super) fn zero_sub_goal_budget(&self) -> BudgetConfig {
        let template = self.budget.config();
        BudgetConfig {
            max_llm_calls: 0,
            max_tool_invocations: 0,
            max_tokens: 0,
            max_cost_cents: 0,
            max_wall_time_ms: 0,
            max_recursion_depth: template.max_recursion_depth,
            decompose_depth_mode: template.decompose_depth_mode,
            soft_ceiling_percent: template.soft_ceiling_percent,
            max_fan_out: template.max_fan_out,
            max_tool_result_bytes: template.max_tool_result_bytes,
            max_aggregate_result_bytes: template.max_aggregate_result_bytes,
            max_synthesis_tokens: template.max_synthesis_tokens,
            max_consecutive_failures: template.max_consecutive_failures,
            max_cycle_failures: template.max_cycle_failures,
            max_no_progress: template.max_no_progress,
            max_tool_retries: template.max_tool_retries,
            termination: template.termination.clone(),
        }
    }

    fn depth_limited_decomposition_result(&mut self, decision: &Decision) -> ActionResult {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            "task decomposition blocked by recursion depth",
            serde_json::json!({"reason": "max recursion depth reached"}),
        );
        self.text_action_result(decision, DECOMPOSITION_DEPTH_LIMIT_RESPONSE)
    }
}

fn build_decomposition_action(decision: &Decision, aggregate: String) -> ActionResult {
    ActionResult {
        decision: decision.clone(),
        tool_results: Vec::new(),
        response_text: aggregate.clone(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Continue(ActionContinuation::new(None, Some(aggregate))),
    }
}

pub(super) fn aggregate_sub_goal_results(results: &[SubGoalResult]) -> String {
    if results.is_empty() {
        return "Task decomposition contained no sub-goals.".to_string();
    }
    let mut lines = Vec::with_capacity(results.len() + 1);
    lines.push(DECOMPOSITION_RESULTS_PREFIX.to_string());
    for (index, result) in results.iter().enumerate() {
        lines.push(format_sub_goal_line(index + 1, result));
    }
    lines.join("\n")
}

pub(super) fn is_decomposition_results_message(text: &str) -> bool {
    text.trim_start().starts_with(DECOMPOSITION_RESULTS_PREFIX)
}

pub(super) fn decomposition_results_all_skipped(text: &str) -> bool {
    is_decomposition_results_message(text)
        && text
            .lines()
            .skip(1)
            .all(|line| line.contains("=> skipped (below floor)"))
}

fn format_sub_goal_line(index: usize, result: &SubGoalResult) -> String {
    format!(
        "{index}. {} => {}",
        result.goal.description,
        format_sub_goal_outcome(&result.outcome)
    )
}

fn format_sub_goal_outcome(outcome: &SubGoalOutcome) -> String {
    match outcome {
        SubGoalOutcome::Completed(response) => format!("completed: {response}"),
        SubGoalOutcome::Incomplete(message) => format!("incomplete: {message}"),
        SubGoalOutcome::Failed(message) => format!("failed: {message}"),
        SubGoalOutcome::BudgetExhausted { partial_response } => partial_response
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| {
                format!(
                    "budget exhausted after partial: {}",
                    truncate_prompt_text(text, 240)
                )
            })
            .unwrap_or_else(|| "budget exhausted".to_string()),
        SubGoalOutcome::Skipped => "skipped (below floor)".to_string(),
    }
}

fn allocation_mode_for_strategy(strategy: &AggregationStrategy) -> AllocationMode {
    match strategy {
        AggregationStrategy::Sequential => AllocationMode::Sequential,
        AggregationStrategy::Parallel => AllocationMode::Concurrent,
        AggregationStrategy::Custom(strategy) => {
            unreachable!("custom strategy '{strategy}' should be rejected during parsing")
        }
    }
}

pub(super) fn parse_decomposition_plan(
    arguments: &serde_json::Value,
) -> Result<DecompositionPlan, LoopError> {
    let parsed = parse_decompose_arguments(arguments)?;
    reject_custom_strategy(parsed.strategy.as_ref())?;
    ensure_sub_goals_present(&parsed)?;
    let (sub_goals, truncated_from) = parsed_sub_goals(parsed.sub_goals);
    Ok(DecompositionPlan {
        sub_goals,
        strategy: parsed.strategy.unwrap_or(AggregationStrategy::Sequential),
        truncated_from,
    })
}

fn reject_custom_strategy(strategy: Option<&AggregationStrategy>) -> Result<(), LoopError> {
    if let Some(strategy) = strategy {
        if matches!(strategy, AggregationStrategy::Custom(_)) {
            return Err(loop_error(
                "decide",
                &format!("unsupported decomposition strategy: {strategy:?}"),
                false,
            ));
        }
    }
    Ok(())
}

fn ensure_sub_goals_present(parsed: &DecomposeToolArguments) -> Result<(), LoopError> {
    if parsed.sub_goals.is_empty() {
        return Err(loop_error(
            "decide",
            "decompose tool requires at least one sub_goal",
            false,
        ));
    }
    Ok(())
}

fn parsed_sub_goals(
    sub_goals: Vec<super::DecomposeSubGoalArguments>,
) -> (Vec<SubGoal>, Option<usize>) {
    let mut sub_goals: Vec<SubGoal> = sub_goals.into_iter().map(SubGoal::from).collect();
    if sub_goals.len() > MAX_SUB_GOALS {
        let original_sub_goals = sub_goals.len();
        sub_goals.truncate(MAX_SUB_GOALS);
        return (sub_goals, Some(original_sub_goals));
    }
    (sub_goals, None)
}

fn parse_decompose_arguments(
    arguments: &serde_json::Value,
) -> Result<DecomposeToolArguments, LoopError> {
    serde_json::from_value(arguments.clone()).map_err(|error| {
        loop_error(
            "decide",
            &format!("invalid decompose tool arguments: {error}"),
            false,
        )
    })
}

pub(super) fn estimate_plan_cost(plan: &DecompositionPlan) -> ActionCost {
    plan.sub_goals
        .iter()
        .fold(ActionCost::default(), |mut acc, sub_goal| {
            let llm_calls = estimated_llm_calls(sub_goal);
            let tool_invocations = sub_goal.required_tools.len() as u32;
            acc.llm_calls = acc.llm_calls.saturating_add(llm_calls);
            acc.tool_invocations = acc.tool_invocations.saturating_add(tool_invocations);
            acc.cost_cents = acc.cost_cents.saturating_add(
                u64::from(llm_calls) * DEFAULT_LLM_CALL_COST_CENTS
                    + u64::from(tool_invocations) * DEFAULT_TOOL_INVOCATION_COST_CENTS,
            );
            acc
        })
}

fn estimated_llm_calls(sub_goal: &SubGoal) -> u32 {
    match sub_goal
        .complexity_hint
        .unwrap_or_else(|| estimate_complexity(sub_goal))
    {
        ComplexityHint::Trivial => 1,
        ComplexityHint::Moderate => 2,
        ComplexityHint::Complex => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::{ToolExecutor, ToolExecutorError, ToolResult};
    use crate::budget::{BudgetConfig, BudgetTracker};
    use crate::cancellation::CancellationToken;
    use crate::context_manager::ContextCompactor;
    use async_trait::async_trait;
    use fx_decompose::{SubGoalContract, SubGoalOutcome};
    use std::sync::Arc;

    #[test]
    fn parse_decomposition_plan_truncates_sub_goals_to_maximum() {
        let sub_goals = (0..8)
            .map(|index| serde_json::json!({"description": format!("goal-{index}")}))
            .collect::<Vec<_>>();
        let arguments = serde_json::json!({"sub_goals": sub_goals});

        let plan = parse_decomposition_plan(&arguments).expect("plan should parse");

        assert_eq!(plan.sub_goals.len(), MAX_SUB_GOALS);
        assert_eq!(plan.sub_goals[0].description, "goal-0");
        assert_eq!(plan.sub_goals[MAX_SUB_GOALS - 1].description, "goal-4");
        assert_eq!(plan.truncated_from, Some(8));
    }

    #[test]
    fn aggregate_sub_goal_results_marks_all_skipped() {
        let aggregate =
            aggregate_sub_goal_results(&[skipped_result("first"), skipped_result("second")]);

        assert!(is_decomposition_results_message(&aggregate));
        assert!(decomposition_results_all_skipped(&aggregate));
    }

    #[test]
    fn format_sub_goal_outcome_includes_skipped_variant() {
        assert_eq!(
            format_sub_goal_outcome(&SubGoalOutcome::Skipped),
            "skipped (below floor)"
        );
    }

    #[test]
    fn format_sub_goal_outcome_includes_budget_exhausted_partial_response() {
        let outcome = SubGoalOutcome::BudgetExhausted {
            partial_response: Some(
                "I have enough from the search results to write a comprehensive spec.".to_string(),
            ),
        };

        assert_eq!(
            format_sub_goal_outcome(&outcome),
            "budget exhausted after partial: I have enough from the search results to write a comprehensive spec."
        );
    }

    #[test]
    fn estimate_plan_cost_trivial_no_tools() {
        let cost = estimate_plan_cost(&plan(vec![sub_goal(
            "a",
            &[],
            Some(ComplexityHint::Trivial),
        )]));

        assert_eq!(cost.llm_calls, 1);
        assert_eq!(cost.tool_invocations, 0);
        assert_eq!(cost.cost_cents, 2);
    }

    #[test]
    fn estimate_plan_cost_complex_with_tools() {
        let cost = estimate_plan_cost(&plan(vec![sub_goal(
            "task",
            &["t1", "t2"],
            Some(ComplexityHint::Complex),
        )]));

        assert_eq!(cost.llm_calls, 4);
        assert_eq!(cost.tool_invocations, 2);
        assert_eq!(cost.cost_cents, 10);
    }

    #[test]
    fn estimate_plan_cost_accumulates_across_sub_goals() {
        let cost = estimate_plan_cost(&plan(vec![
            sub_goal("a", &["t1"], Some(ComplexityHint::Trivial)),
            sub_goal("b", &["t1", "t2"], Some(ComplexityHint::Moderate)),
        ]));

        assert_eq!(cost.llm_calls, 3);
        assert_eq!(cost.tool_invocations, 3);
        assert_eq!(cost.cost_cents, 9);
    }

    #[test]
    fn depth_limited_result_emits_blocked_signal() {
        let config = BudgetConfig {
            max_recursion_depth: 1,
            ..BudgetConfig::default()
        };
        let mut engine = build_engine_with_budget(config, 1);
        let decision = Decision::Decompose(plan(vec![sub_goal("blocked", &[], None)]));

        let result = engine.depth_limited_decomposition_result(&decision);

        assert!(result.tool_results.is_empty());
        let blocked = engine
            .signals
            .signals()
            .iter()
            .filter(|signal| signal.kind == SignalKind::Blocked)
            .collect::<Vec<_>>();
        assert_eq!(blocked.len(), 1);
        assert!(blocked[0].message.contains("recursion depth"));
    }

    fn build_engine_with_budget(config: BudgetConfig, depth: u32) -> LoopEngine {
        LoopEngine::builder()
            .budget(BudgetTracker::new(config, 0, depth))
            .context(ContextCompactor::new(2048, 256))
            .max_iterations(1)
            .tool_executor(Arc::new(PassiveToolExecutor))
            .synthesis_instruction("Summarize tool output".to_string())
            .build()
            .expect("test engine build")
    }

    fn plan(sub_goals: Vec<SubGoal>) -> DecompositionPlan {
        DecompositionPlan {
            sub_goals,
            strategy: AggregationStrategy::Parallel,
            truncated_from: None,
        }
    }

    fn skipped_result(description: &str) -> SubGoalResult {
        SubGoalResult {
            goal: sub_goal(description, &[], None),
            outcome: SubGoalOutcome::Skipped,
            signals: Vec::new(),
        }
    }

    fn sub_goal(
        description: &str,
        required_tools: &[&str],
        complexity_hint: Option<ComplexityHint>,
    ) -> SubGoal {
        SubGoal {
            description: description.to_string(),
            required_tools: required_tools
                .iter()
                .map(|tool| (*tool).to_string())
                .collect(),
            completion_contract: SubGoalContract::from_definition_of_done(None),
            complexity_hint,
        }
    }

    #[derive(Debug, Default)]
    struct PassiveToolExecutor;

    #[async_trait]
    impl ToolExecutor for PassiveToolExecutor {
        async fn execute_tools(
            &self,
            _calls: &[fx_llm::ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(Vec::new())
        }
    }
}
