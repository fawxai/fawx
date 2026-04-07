use crate::{
    DecomposeError, GraphNodeId, GraphOfOperations, GraphOperation, MergeStrategy, ScoringStrategy,
    ThoughtMetadata, ThoughtPool, ThoughtState, ValidationStrategy,
};
use regex::Regex;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

mod thought_impls;
mod traits;

#[cfg(test)]
mod tests;

pub use thought_impls::{
    ConcatMerger, HeuristicThoughtScorer, LlmThoughtGenerator, LlmThoughtMerger, LlmThoughtScorer,
};
pub use traits::{
    GeneratedThoughts, MergedThought, ThoughtGenerator, ThoughtMerger, ThoughtScore, ThoughtScorer,
};

#[derive(Debug, Clone)]
pub struct GraphExecutionResult {
    /// The active thought pool after all operations complete.
    pub thoughts: Vec<ThoughtState>,
    /// The best remaining thought, if the dispatcher can determine one.
    pub best: Option<ThoughtState>,
    /// Number of model-backed calls performed by graph execution.
    pub llm_calls: usize,
    /// Number of graph nodes executed, including repeated back-edge traversals.
    pub operations_executed: usize,
    /// Whether any refine operation exhausted its configured iteration cap.
    pub refinement_capped: bool,
}

pub struct GraphDispatcher {
    generator: Arc<dyn ThoughtGenerator>,
    scorer: Arc<dyn ThoughtScorer>,
    merger: Arc<dyn ThoughtMerger>,
}

#[derive(Default)]
struct ExecutionCounters {
    llm_calls: usize,
    operations_executed: usize,
    refinement_capped: bool,
    back_edge_counts: HashMap<(GraphNodeId, GraphNodeId), usize>,
    node_visit_counts: HashMap<GraphNodeId, usize>,
}

struct ExecutionState {
    pool: ThoughtPool,
    counters: ExecutionCounters,
}

impl ExecutionState {
    fn new(initial_content: String, initial_metadata: ThoughtMetadata) -> Self {
        let mut pool = ThoughtPool::new();
        pool.create(initial_content, Vec::new(), initial_metadata);
        Self {
            pool,
            counters: ExecutionCounters::default(),
        }
    }

    fn into_result(self) -> GraphExecutionResult {
        let thoughts = pool_snapshot(&self.pool);
        let best = select_best(&thoughts);

        GraphExecutionResult {
            thoughts,
            best,
            llm_calls: self.counters.llm_calls,
            operations_executed: self.counters.operations_executed,
            refinement_capped: self.counters.refinement_capped,
        }
    }
}

struct RefineConfig<'a> {
    node_id: GraphNodeId,
    max_iterations: usize,
    target_score: f64,
    scoring: &'a ScoringStrategy,
}

impl GraphDispatcher {
    pub fn new(
        generator: Arc<dyn ThoughtGenerator>,
        scorer: Arc<dyn ThoughtScorer>,
        merger: Arc<dyn ThoughtMerger>,
    ) -> Self {
        Self {
            generator,
            scorer,
            merger,
        }
    }

    pub async fn execute(
        &self,
        graph: &GraphOfOperations,
        initial_content: String,
        initial_metadata: ThoughtMetadata,
    ) -> Result<GraphExecutionResult, DecomposeError> {
        graph.validate().map_err(|error| {
            DecomposeError::DecompositionFailed(format!("invalid graph topology: {error}"))
        })?;

        let mut state = ExecutionState::new(initial_content, initial_metadata);
        let mut current = graph.entry();

        loop {
            let Some(next) = self.execute_node(graph, current, &mut state).await? else {
                break;
            };
            current = next;
        }

        Ok(state.into_result())
    }

    async fn execute_node(
        &self,
        graph: &GraphOfOperations,
        current: GraphNodeId,
        state: &mut ExecutionState,
    ) -> Result<Option<GraphNodeId>, DecomposeError> {
        let node = graph.node(current).ok_or_else(|| {
            DecomposeError::DecompositionFailed(format!(
                "graph node {current} disappeared during execution"
            ))
        })?;
        let cycle = state
            .counters
            .node_visit_counts
            .get(&current)
            .copied()
            .unwrap_or(0);
        let span = tracing::info_span!(
            "got_operation",
            node = %current,
            op = operation_name(node.operation()),
            cycle = cycle,
        );
        let _guard = span.enter();

        state.counters.node_visit_counts.insert(current, cycle + 1);
        self.apply_operation(current, node.operation(), state)
            .await?;
        state.counters.operations_executed += 1;

        Ok(next_node(
            graph,
            current,
            &mut state.counters.back_edge_counts,
        ))
    }

    async fn apply_operation(
        &self,
        node_id: GraphNodeId,
        operation: &GraphOperation,
        state: &mut ExecutionState,
    ) -> Result<(), DecomposeError> {
        match operation {
            GraphOperation::Generate {
                num_branches,
                prompt_override,
            } => {
                self.apply_generate(node_id, state, *num_branches, prompt_override.as_deref())
                    .await
            }
            GraphOperation::Score { strategy } => self.apply_score(strategy, state).await,
            GraphOperation::KeepBest { n } => {
                self.apply_keep_best(*n, state);
                Ok(())
            }
            GraphOperation::Merge { strategy } => self.apply_merge(node_id, strategy, state).await,
            GraphOperation::Refine {
                max_iterations,
                target_score,
                scoring,
            } => {
                let config = RefineConfig {
                    node_id,
                    max_iterations: *max_iterations,
                    target_score: *target_score,
                    scoring,
                };
                self.apply_refine(config, state).await
            }
            GraphOperation::Validate { strategy } => self.apply_validate(strategy, state).await,
        }
    }

    async fn apply_generate(
        &self,
        node_id: GraphNodeId,
        state: &mut ExecutionState,
        num_branches: usize,
        prompt_override: Option<&str>,
    ) -> Result<(), DecomposeError> {
        for parent_id in state.pool.active_ids() {
            let Some(parent) = state.pool.get(parent_id).cloned() else {
                continue;
            };
            let generation = self
                .generator
                .generate(&parent, num_branches, prompt_override)
                .await?;
            state.counters.llm_calls += generation.llm_calls;
            validate_generation_count(&generation, parent.id().value(), num_branches)?;
            replace_parent_with_children(node_id, &mut state.pool, &parent, generation.contents);
        }

        Ok(())
    }

    async fn apply_score(
        &self,
        strategy: &ScoringStrategy,
        state: &mut ExecutionState,
    ) -> Result<(), DecomposeError> {
        match strategy {
            ScoringStrategy::LlmRating { criteria } => self.apply_llm_score(criteria, state).await,
            ScoringStrategy::Heuristic { pattern } => self.apply_heuristic_score(pattern, state),
            ScoringStrategy::External => Ok(()),
        }
    }

    async fn apply_llm_score(
        &self,
        criteria: &str,
        state: &mut ExecutionState,
    ) -> Result<(), DecomposeError> {
        for thought_id in state.pool.active_ids() {
            let Some(thought) = state.pool.get(thought_id).cloned() else {
                continue;
            };
            let score = self.scorer.score(&thought, criteria).await?;
            let value = validate_score_range(score.value, "scoring")?;
            state.counters.llm_calls += score.llm_calls;
            update_score(&mut state.pool, thought_id, value);
        }

        Ok(())
    }

    fn apply_heuristic_score(
        &self,
        pattern: &str,
        state: &mut ExecutionState,
    ) -> Result<(), DecomposeError> {
        let regex = Regex::new(pattern).map_err(|error| {
            DecomposeError::DecompositionFailed(format!(
                "invalid heuristic scoring pattern {pattern:?}: {error}"
            ))
        })?;

        for thought_id in state.pool.active_ids() {
            let Some(thought) = state.pool.get(thought_id) else {
                continue;
            };
            let score = if regex.is_match(&thought.content) {
                1.0
            } else {
                0.0
            };
            update_score(&mut state.pool, thought_id, score);
        }

        Ok(())
    }

    fn apply_keep_best(&self, n: usize, state: &mut ExecutionState) {
        let keep_ids = state
            .pool
            .top_n(n)
            .into_iter()
            .map(ThoughtState::id)
            .collect::<HashSet<_>>();

        for thought_id in state.pool.active_ids() {
            if !keep_ids.contains(&thought_id) {
                state.pool.remove(thought_id);
            }
        }
    }

    async fn apply_merge(
        &self,
        node_id: GraphNodeId,
        strategy: &MergeStrategy,
        state: &mut ExecutionState,
    ) -> Result<(), DecomposeError> {
        let active = pool_snapshot(&state.pool);
        if active.is_empty() {
            return Ok(());
        }

        let refs = active.iter().collect::<Vec<_>>();
        let merged_content = match strategy {
            MergeStrategy::LlmSynthesis { instruction } => {
                let merged = self.merger.merge(&refs, instruction.as_deref()).await?;
                state.counters.llm_calls += merged.llm_calls;
                merged.content
            }
            MergeStrategy::Concatenate { separator } => join_contents(&refs, separator),
        };

        let parent_ids = active.iter().map(ThoughtState::id).collect::<Vec<_>>();
        let merged_metadata = merge_metadata(&active);
        let merged_id = state
            .pool
            .create(merged_content, parent_ids.clone(), merged_metadata);
        if let Some(merged) = state.pool.get_mut(merged_id) {
            merged.origin_operation = Some(node_id);
        }

        for parent_id in parent_ids {
            state.pool.remove(parent_id);
        }

        Ok(())
    }

    async fn apply_refine(
        &self,
        config: RefineConfig<'_>,
        state: &mut ExecutionState,
    ) -> Result<(), DecomposeError> {
        if config.max_iterations == 0 {
            state.counters.refinement_capped = true;
            return Ok(());
        }

        for iteration in 0..config.max_iterations {
            self.apply_score(config.scoring, state).await?;
            if current_top_score(&state.pool).is_some_and(|score| score >= config.target_score) {
                return Ok(());
            }

            self.refine_active_thoughts(iteration, &config, state)
                .await?;
            if iteration + 1 == config.max_iterations {
                state.counters.refinement_capped = true;
            }
        }

        Ok(())
    }

    async fn refine_active_thoughts(
        &self,
        iteration: usize,
        config: &RefineConfig<'_>,
        state: &mut ExecutionState,
    ) -> Result<(), DecomposeError> {
        for parent_id in state.pool.active_ids() {
            let Some(parent) = state.pool.get(parent_id).cloned() else {
                continue;
            };
            let prompt =
                build_refine_prompt(&parent, iteration, config.target_score, config.scoring);
            let generation = self
                .generator
                .generate(&parent, 1, Some(prompt.as_str()))
                .await?;
            state.counters.llm_calls += generation.llm_calls;
            validate_generation_count(&generation, parent.id().value(), 1)?;
            replace_parent_with_children(
                config.node_id,
                &mut state.pool,
                &parent,
                generation.contents,
            );
        }

        Ok(())
    }

    async fn apply_validate(
        &self,
        strategy: &ValidationStrategy,
        state: &mut ExecutionState,
    ) -> Result<(), DecomposeError> {
        for thought_id in state.pool.active_ids() {
            let Some(thought) = state.pool.get(thought_id).cloned() else {
                continue;
            };
            let score = self
                .validate_thought(strategy, &thought, &mut state.counters.llm_calls)
                .await?;
            update_score(&mut state.pool, thought_id, score);
        }

        Ok(())
    }

    async fn validate_thought(
        &self,
        strategy: &ValidationStrategy,
        thought: &ThoughtState,
        llm_calls: &mut usize,
    ) -> Result<f64, DecomposeError> {
        match strategy {
            ValidationStrategy::ExactMatch { expected } => {
                Ok(if thought.content.trim() == expected.trim() {
                    1.0
                } else {
                    0.0
                })
            }
            ValidationStrategy::Contains { expected } => {
                Ok(if thought.content.contains(expected) {
                    1.0
                } else {
                    0.0
                })
            }
            ValidationStrategy::LlmJudge { criteria } => {
                let score = self.scorer.score(thought, criteria).await?;
                let value = validate_score_range(score.value, "validation")?;
                *llm_calls += score.llm_calls;
                Ok(if value >= 0.5 { 1.0 } else { 0.0 })
            }
            ValidationStrategy::AlwaysPass => Ok(1.0),
        }
    }
}

fn replace_parent_with_children(
    node_id: GraphNodeId,
    pool: &mut ThoughtPool,
    parent: &ThoughtState,
    contents: Vec<String>,
) {
    for content in contents {
        let child_id = pool.create(content, vec![parent.id()], parent.metadata.clone());
        if let Some(child) = pool.get_mut(child_id) {
            child.origin_operation = Some(node_id);
        }
    }
    pool.remove(parent.id());
}

fn update_score(pool: &mut ThoughtPool, thought_id: crate::ThoughtId, score: f64) {
    if let Some(state) = pool.get_mut(thought_id) {
        state.score = Some(score);
    }
}

fn validate_generation_count(
    generation: &GeneratedThoughts,
    parent_id: u64,
    expected_branches: usize,
) -> Result<(), DecomposeError> {
    if generation.contents.len() == expected_branches {
        return Ok(());
    }

    Err(DecomposeError::DecompositionFailed(format!(
        "generator for parent thought {parent_id} returned {} branches, expected {expected_branches}",
        generation.contents.len()
    )))
}

fn operation_name(operation: &GraphOperation) -> &'static str {
    match operation {
        GraphOperation::Generate { .. } => "generate",
        GraphOperation::Score { .. } => "score",
        GraphOperation::KeepBest { .. } => "keep_best",
        GraphOperation::Merge { .. } => "merge",
        GraphOperation::Refine { .. } => "refine",
        GraphOperation::Validate { .. } => "validate",
    }
}

fn next_node(
    graph: &GraphOfOperations,
    current: GraphNodeId,
    back_edge_counts: &mut HashMap<(GraphNodeId, GraphNodeId), usize>,
) -> Option<GraphNodeId> {
    let successors = graph.all_successors(current);

    for (target, is_back_edge) in &successors {
        if !*is_back_edge {
            continue;
        }
        let count = back_edge_counts.entry((current, *target)).or_insert(0);
        if *count >= graph.max_iterations() {
            continue;
        }
        *count += 1;
        return Some(*target);
    }

    successors
        .into_iter()
        .find_map(|(target, is_back_edge)| (!is_back_edge).then_some(target))
}

fn pool_snapshot(pool: &ThoughtPool) -> Vec<ThoughtState> {
    pool.active_ids()
        .into_iter()
        .filter_map(|thought_id| pool.get(thought_id).cloned())
        .collect()
}

/// Choose a best thought only when the result is deterministic.
///
/// If at least one thought is scored, the highest score wins with a stable
/// `ThoughtId` tie-breaker. If no thoughts are scored, we only report a best
/// thought when exactly one thought remains; otherwise we return `None` rather
/// than guessing among multiple unscored candidates.
fn select_best(thoughts: &[ThoughtState]) -> Option<ThoughtState> {
    let best_scored = thoughts
        .iter()
        .filter_map(|thought| thought.score.map(|score| (thought, score)))
        .max_by(compare_scored_candidates)
        .map(|(thought, _)| thought.clone());

    if best_scored.is_some() {
        return best_scored;
    }

    (thoughts.len() == 1).then(|| thoughts[0].clone())
}

fn compare_scored_candidates(
    left: &(&ThoughtState, f64),
    right: &(&ThoughtState, f64),
) -> Ordering {
    left.1
        .total_cmp(&right.1)
        .then_with(|| right.0.id().cmp(&left.0.id()))
}

fn current_top_score(pool: &ThoughtPool) -> Option<f64> {
    pool.active_ids()
        .into_iter()
        .filter_map(|thought_id| pool.get(thought_id).and_then(|thought| thought.score))
        .max_by(|left, right| left.total_cmp(right))
}

pub(super) fn join_contents(thoughts: &[&ThoughtState], separator: &str) -> String {
    thoughts
        .iter()
        .map(|thought| thought.content.as_str())
        .collect::<Vec<_>>()
        .join(separator)
}

fn merge_metadata(thoughts: &[ThoughtState]) -> ThoughtMetadata {
    let Some(first) = thoughts.first() else {
        return ThoughtMetadata::Empty;
    };
    if thoughts
        .iter()
        .all(|thought| thought.metadata == first.metadata)
    {
        return first.metadata.clone();
    }

    tracing::debug!(
        thought_count = thoughts.len(),
        "discarding heterogeneous thought metadata during merge"
    );
    ThoughtMetadata::Empty
}

fn build_refine_prompt(
    thought: &ThoughtState,
    iteration: usize,
    target_score: f64,
    scoring: &ScoringStrategy,
) -> String {
    let current_score = thought
        .score
        .map(|score| score.to_string())
        .unwrap_or_else(|| "unscored".to_string());

    format!(
        "Improve the following reasoning.\n\nIteration: {}\nCurrent score: {}\nTarget score: {}\n{}\n\nReasoning:\n{}",
        iteration + 1,
        current_score,
        target_score,
        refine_goal_description(scoring),
        thought.content
    )
}

fn refine_goal_description(scoring: &ScoringStrategy) -> String {
    match scoring {
        ScoringStrategy::LlmRating { criteria } => {
            format!("Evaluation criteria:\n{criteria}")
        }
        ScoringStrategy::Heuristic { pattern } => format!(
            "The scorer uses this regular expression. Improve the reasoning so the final text matches it:\n{pattern}"
        ),
        ScoringStrategy::External => {
            "Evaluation is handled externally. Improve the reasoning for the strongest final answer."
                .to_string()
        }
    }
}

fn validate_score_range(score: f64, context: &str) -> Result<f64, DecomposeError> {
    if (0.0..=1.0).contains(&score) {
        Ok(score)
    } else {
        Err(DecomposeError::DecompositionFailed(format!(
            "{context} score must be between 0.0 and 1.0, got {score}"
        )))
    }
}
