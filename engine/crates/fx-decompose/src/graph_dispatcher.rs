use crate::{
    DecomposeError, DecompositionProgressCallback, GraphNodeId, GraphOfOperations, GraphOperation,
    MergeStrategy, ScoringStrategy, ThoughtMetadata, ThoughtPool, ThoughtState, ValidationStrategy,
};
use async_trait::async_trait;
use fx_llm::{completion_text, CompletionRequest, Message, ModelRouter};
use regex::Regex;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

const DEFAULT_SCORE_FALLBACK: f64 = 0.5;
const SCORE_RESPONSE_TOKEN_LIMIT: u32 = 64;
const GENERATION_TOKEN_LIMIT: u32 = 512;
const MERGE_TOKEN_LIMIT: u32 = 1024;

/// Pluggable scoring for thoughts. Used by Score and Refine operations.
#[async_trait]
pub trait ThoughtScorer: Send + Sync {
    async fn score(&self, thought: &ThoughtState, criteria: &str) -> Result<f64, DecomposeError>;
}

/// Generates new thoughts from a parent thought.
#[async_trait]
pub trait ThoughtGenerator: Send + Sync {
    async fn generate(
        &self,
        parent: &ThoughtState,
        num_branches: usize,
        prompt_override: Option<&str>,
    ) -> Result<Vec<String>, DecomposeError>;
}

/// Merges multiple thoughts into one.
#[async_trait]
pub trait ThoughtMerger: Send + Sync {
    async fn merge(
        &self,
        thoughts: &[&ThoughtState],
        instruction: Option<&str>,
    ) -> Result<String, DecomposeError>;
}

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
        _progress: Option<&DecompositionProgressCallback>,
    ) -> Result<GraphExecutionResult, DecomposeError> {
        graph.validate().map_err(|error| {
            DecomposeError::DecompositionFailed(format!("invalid graph topology: {error}"))
        })?;

        let mut pool = ThoughtPool::new();
        pool.create(initial_content, Vec::new(), initial_metadata);

        let mut counters = ExecutionCounters::default();
        let mut current = graph.entry();

        loop {
            let node = graph.node(current).ok_or_else(|| {
                DecomposeError::DecompositionFailed(format!(
                    "graph node {current} disappeared during execution"
                ))
            })?;
            let cycle = counters
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

            counters.node_visit_counts.insert(current, cycle + 1);
            self.apply_operation(current, node.operation(), &mut pool, &mut counters)
                .await?;
            counters.operations_executed += 1;

            let Some(next) = next_node(graph, current, &mut counters.back_edge_counts) else {
                break;
            };
            current = next;
        }

        let thoughts = pool_snapshot(&pool);
        let best = select_best(&thoughts);

        Ok(GraphExecutionResult {
            thoughts,
            best,
            llm_calls: counters.llm_calls,
            operations_executed: counters.operations_executed,
            refinement_capped: counters.refinement_capped,
        })
    }

    async fn apply_operation(
        &self,
        node_id: GraphNodeId,
        operation: &GraphOperation,
        pool: &mut ThoughtPool,
        counters: &mut ExecutionCounters,
    ) -> Result<(), DecomposeError> {
        match operation {
            GraphOperation::Generate {
                num_branches,
                prompt_override,
            } => {
                self.apply_generate(
                    node_id,
                    pool,
                    *num_branches,
                    prompt_override.as_deref(),
                    &mut counters.llm_calls,
                )
                .await
            }
            GraphOperation::Score { strategy } => self.apply_score(strategy, pool, counters).await,
            GraphOperation::KeepBest { n } => {
                self.apply_keep_best(*n, pool);
                Ok(())
            }
            GraphOperation::Merge { strategy } => {
                self.apply_merge(node_id, strategy, pool, counters).await
            }
            GraphOperation::Refine {
                max_iterations,
                target_score,
                scoring,
            } => {
                self.apply_refine(
                    node_id,
                    *max_iterations,
                    *target_score,
                    scoring,
                    pool,
                    counters,
                )
                .await
            }
            GraphOperation::Validate { strategy } => {
                self.apply_validate(strategy, pool, counters).await
            }
        }
    }

    async fn apply_generate(
        &self,
        node_id: GraphNodeId,
        pool: &mut ThoughtPool,
        num_branches: usize,
        prompt_override: Option<&str>,
        llm_calls: &mut usize,
    ) -> Result<(), DecomposeError> {
        for parent_id in pool.active_ids() {
            let Some(parent) = pool.get(parent_id).cloned() else {
                continue;
            };
            let generated = self
                .generator
                .generate(&parent, num_branches, prompt_override)
                .await?;
            *llm_calls += num_branches;

            for content in generated {
                let child_id = pool.create(content, vec![parent.id()], parent.metadata.clone());
                if let Some(child) = pool.get_mut(child_id) {
                    child.origin_operation = Some(node_id);
                }
            }
            pool.remove(parent_id);
        }

        Ok(())
    }

    async fn apply_score(
        &self,
        strategy: &ScoringStrategy,
        pool: &mut ThoughtPool,
        counters: &mut ExecutionCounters,
    ) -> Result<(), DecomposeError> {
        for thought_id in pool.active_ids() {
            let Some(thought) = pool.get(thought_id).cloned() else {
                continue;
            };
            let Some(score) = self
                .score_for_strategy(&thought, strategy, counters)
                .await?
            else {
                continue;
            };
            if let Some(state) = pool.get_mut(thought_id) {
                state.score = Some(score);
            }
        }

        Ok(())
    }

    fn apply_keep_best(&self, n: usize, pool: &mut ThoughtPool) {
        let keep_ids = pool
            .top_n(n)
            .into_iter()
            .map(ThoughtState::id)
            .collect::<HashSet<_>>();

        for thought_id in pool.active_ids() {
            if !keep_ids.contains(&thought_id) {
                pool.remove(thought_id);
            }
        }
    }

    async fn apply_merge(
        &self,
        node_id: GraphNodeId,
        strategy: &MergeStrategy,
        pool: &mut ThoughtPool,
        counters: &mut ExecutionCounters,
    ) -> Result<(), DecomposeError> {
        let active = pool_snapshot(pool);
        if active.is_empty() {
            return Ok(());
        }

        let refs = active.iter().collect::<Vec<_>>();
        let merged_content = match strategy {
            MergeStrategy::LlmSynthesis { instruction } => {
                let content = self.merger.merge(&refs, instruction.as_deref()).await?;
                counters.llm_calls += 1;
                content
            }
            MergeStrategy::Concatenate { separator } => join_contents(&refs, separator),
        };

        let parent_ids = active.iter().map(ThoughtState::id).collect::<Vec<_>>();
        let merged_metadata = merge_metadata(&active);
        let merged_id = pool.create(merged_content, parent_ids.clone(), merged_metadata);
        if let Some(merged) = pool.get_mut(merged_id) {
            merged.origin_operation = Some(node_id);
        }

        for parent_id in parent_ids {
            pool.remove(parent_id);
        }

        Ok(())
    }

    async fn apply_refine(
        &self,
        node_id: GraphNodeId,
        max_iterations: usize,
        target_score: f64,
        scoring: &ScoringStrategy,
        pool: &mut ThoughtPool,
        counters: &mut ExecutionCounters,
    ) -> Result<(), DecomposeError> {
        if max_iterations == 0 {
            counters.refinement_capped = true;
            return Ok(());
        }

        for iteration in 0..max_iterations {
            self.apply_score(scoring, pool, counters).await?;
            if current_top_score(pool).is_some_and(|score| score >= target_score) {
                return Ok(());
            }

            for parent_id in pool.active_ids() {
                let Some(parent) = pool.get(parent_id).cloned() else {
                    continue;
                };
                let prompt = build_refine_prompt(&parent, iteration, target_score, scoring);
                let generated = self
                    .generator
                    .generate(&parent, 1, Some(prompt.as_str()))
                    .await?;
                counters.llm_calls += 1;

                for content in generated {
                    let child_id = pool.create(content, vec![parent.id()], parent.metadata.clone());
                    if let Some(child) = pool.get_mut(child_id) {
                        child.origin_operation = Some(node_id);
                    }
                }

                pool.remove(parent_id);
            }

            if iteration + 1 == max_iterations {
                counters.refinement_capped = true;
            }
        }

        Ok(())
    }

    async fn apply_validate(
        &self,
        strategy: &ValidationStrategy,
        pool: &mut ThoughtPool,
        counters: &mut ExecutionCounters,
    ) -> Result<(), DecomposeError> {
        for thought_id in pool.active_ids() {
            let Some(thought) = pool.get(thought_id).cloned() else {
                continue;
            };
            let score = match strategy {
                ValidationStrategy::ExactMatch { expected } => {
                    if thought.content.trim() == expected.trim() {
                        1.0
                    } else {
                        0.0
                    }
                }
                ValidationStrategy::Contains { expected } => {
                    if thought.content.contains(expected) {
                        1.0
                    } else {
                        0.0
                    }
                }
                ValidationStrategy::LlmJudge { criteria } => {
                    let score = self.scorer.score(&thought, criteria).await?;
                    counters.llm_calls += 1;
                    if validate_score_range(score, "validation")? >= 0.5 {
                        1.0
                    } else {
                        0.0
                    }
                }
                ValidationStrategy::AlwaysPass => 1.0,
            };

            if let Some(state) = pool.get_mut(thought_id) {
                state.score = Some(score);
            }
        }

        Ok(())
    }

    async fn score_for_strategy(
        &self,
        thought: &ThoughtState,
        strategy: &ScoringStrategy,
        counters: &mut ExecutionCounters,
    ) -> Result<Option<f64>, DecomposeError> {
        match strategy {
            ScoringStrategy::LlmRating { criteria } => {
                let score = self.scorer.score(thought, criteria).await?;
                counters.llm_calls += 1;
                Ok(Some(validate_score_range(score, "scoring")?))
            }
            ScoringStrategy::Heuristic { pattern } => {
                let regex = Regex::new(pattern).map_err(|error| {
                    DecomposeError::DecompositionFailed(format!(
                        "invalid heuristic scoring pattern {pattern:?}: {error}"
                    ))
                })?;
                Ok(Some(if regex.is_match(&thought.content) {
                    1.0
                } else {
                    0.0
                }))
            }
            ScoringStrategy::External => Ok(None),
        }
    }
}

pub struct LlmThoughtScorer {
    router: Arc<ModelRouter>,
    model: String,
}

impl LlmThoughtScorer {
    pub fn new(router: Arc<ModelRouter>, model: impl Into<String>) -> Self {
        Self {
            router,
            model: model.into(),
        }
    }
}

#[async_trait]
impl ThoughtScorer for LlmThoughtScorer {
    async fn score(&self, thought: &ThoughtState, criteria: &str) -> Result<f64, DecomposeError> {
        let prompt = format!(
            "Rate the following reasoning on a scale of 0.0 to 1.0 based on this criteria:\n\
             {criteria}\n\nReasoning:\n{}\n\nRespond with only a number between 0.0 and 1.0.",
            thought.content
        );
        let response = complete_text_response(
            &self.router,
            &self.model,
            prompt,
            SCORE_RESPONSE_TOKEN_LIMIT,
        )
        .await?;
        Ok(parse_llm_score(&response))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicThoughtScorer;

impl HeuristicThoughtScorer {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ThoughtScorer for HeuristicThoughtScorer {
    async fn score(&self, thought: &ThoughtState, criteria: &str) -> Result<f64, DecomposeError> {
        let regex = Regex::new(criteria).map_err(|error| {
            DecomposeError::DecompositionFailed(format!(
                "invalid heuristic scoring pattern {criteria:?}: {error}"
            ))
        })?;
        Ok(if regex.is_match(&thought.content) {
            1.0
        } else {
            0.0
        })
    }
}

pub struct LlmThoughtGenerator {
    router: Arc<ModelRouter>,
    model: String,
}

impl LlmThoughtGenerator {
    pub fn new(router: Arc<ModelRouter>, model: impl Into<String>) -> Self {
        Self {
            router,
            model: model.into(),
        }
    }
}

#[async_trait]
impl ThoughtGenerator for LlmThoughtGenerator {
    async fn generate(
        &self,
        parent: &ThoughtState,
        num_branches: usize,
        prompt_override: Option<&str>,
    ) -> Result<Vec<String>, DecomposeError> {
        let mut branches = Vec::with_capacity(num_branches);
        let base_prompt = prompt_override
            .unwrap_or("Generate an alternative reasoning branch for the following thought.");

        for branch_index in 0..num_branches {
            let prompt = format!(
                "{base_prompt}\n\nParent reasoning:\n{}\n\nProduce alternative {}/{} as plain text only.",
                parent.content,
                branch_index + 1,
                num_branches
            );
            branches.push(
                complete_text_response(&self.router, &self.model, prompt, GENERATION_TOKEN_LIMIT)
                    .await?,
            );
        }

        Ok(branches)
    }
}

pub struct LlmThoughtMerger {
    router: Arc<ModelRouter>,
    model: String,
}

impl LlmThoughtMerger {
    pub fn new(router: Arc<ModelRouter>, model: impl Into<String>) -> Self {
        Self {
            router,
            model: model.into(),
        }
    }
}

#[async_trait]
impl ThoughtMerger for LlmThoughtMerger {
    async fn merge(
        &self,
        thoughts: &[&ThoughtState],
        instruction: Option<&str>,
    ) -> Result<String, DecomposeError> {
        let numbered = thoughts
            .iter()
            .enumerate()
            .map(|(index, thought)| format!("Thought {}:\n{}", index + 1, thought.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        let merge_instruction =
            instruction.unwrap_or("Synthesize the strongest ideas into one concise thought.");
        let prompt = format!(
            "{merge_instruction}\n\nMerge the following reasoning paths into one improved thought:\n\n{numbered}"
        );
        complete_text_response(&self.router, &self.model, prompt, MERGE_TOKEN_LIMIT).await
    }
}

#[derive(Debug, Clone)]
pub struct ConcatMerger {
    separator: String,
}

impl ConcatMerger {
    pub fn new(separator: impl Into<String>) -> Self {
        Self {
            separator: separator.into(),
        }
    }
}

#[async_trait]
impl ThoughtMerger for ConcatMerger {
    async fn merge(
        &self,
        thoughts: &[&ThoughtState],
        instruction: Option<&str>,
    ) -> Result<String, DecomposeError> {
        let separator = instruction.unwrap_or(self.separator.as_str());
        Ok(join_contents(thoughts, separator))
    }
}

pub fn parse_llm_score(response: &str) -> f64 {
    static SCORE_REGEX: OnceLock<Regex> = OnceLock::new();
    let regex = SCORE_REGEX.get_or_init(|| {
        Regex::new(r"(1(?:\.0+)?|0(?:\.\d+)?)").expect("score extraction regex is valid")
    });

    let Some(capture) = regex.find(response) else {
        tracing::warn!(
            response,
            "unable to parse llm score; using midpoint fallback"
        );
        return DEFAULT_SCORE_FALLBACK;
    };

    match capture.as_str().parse::<f64>() {
        Ok(score) => score,
        Err(error) => {
            tracing::warn!(
                response,
                %error,
                "llm score looked numeric but failed to parse; using midpoint fallback"
            );
            DEFAULT_SCORE_FALLBACK
        }
    }
}

async fn complete_text_response(
    router: &Arc<ModelRouter>,
    model: &str,
    prompt: String,
    max_tokens: u32,
) -> Result<String, DecomposeError> {
    let request = CompletionRequest {
        model: model.to_string(),
        messages: vec![Message::user(prompt)],
        tools: Vec::new(),
        temperature: None,
        max_tokens: Some(max_tokens),
        system_prompt: None,
        thinking: None,
    };
    let response = router.complete(request).await.map_err(|error| {
        DecomposeError::DecompositionFailed(format!("thought-model request failed: {error}"))
    })?;
    Ok(completion_text(&response))
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

    // A loop body may expose both a back-edge and a forward exit from the same
    // node. Prefer the back-edge while budget remains so refinement cycles are
    // explicit in topology rather than hidden in edge insertion order.
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

fn select_best(thoughts: &[ThoughtState]) -> Option<ThoughtState> {
    let best_scored = thoughts
        .iter()
        .filter(|thought| thought.score.is_some())
        .max_by(compare_scored_thoughts)
        .cloned();

    if best_scored.is_some() {
        return best_scored;
    }

    (thoughts.len() == 1).then(|| thoughts[0].clone())
}

fn compare_scored_thoughts(left: &&ThoughtState, right: &&ThoughtState) -> Ordering {
    let left_score = left
        .score
        .expect("scored thought comparison only runs on scored thoughts");
    let right_score = right
        .score
        .expect("scored thought comparison only runs on scored thoughts");

    left_score
        .total_cmp(&right_score)
        .then_with(|| right.id().cmp(&left.id()))
}

fn current_top_score(pool: &ThoughtPool) -> Option<f64> {
    pool.top_n(1)
        .into_iter()
        .next()
        .and_then(|thought| thought.score)
}

fn join_contents(thoughts: &[&ThoughtState], separator: &str) -> String {
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
        first.metadata.clone()
    } else {
        ThoughtMetadata::Empty
    }
}

fn build_refine_prompt(
    thought: &ThoughtState,
    iteration: usize,
    target_score: f64,
    scoring: &ScoringStrategy,
) -> String {
    let criteria = match scoring {
        ScoringStrategy::LlmRating { criteria } => criteria.as_str(),
        ScoringStrategy::Heuristic { pattern } => pattern.as_str(),
        ScoringStrategy::External => "the active external evaluation criteria",
    };
    let current_score = thought
        .score
        .map(|score| score.to_string())
        .unwrap_or_else(|| "unscored".to_string());

    format!(
        "Improve the following reasoning.\n\nIteration: {}\nCurrent score: {}\nTarget score: {}\nEvaluation criteria: {}\n\nReasoning:\n{}",
        iteration + 1,
        current_score,
        target_score,
        criteria,
        thought.content
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockGenerator {
        suffix: &'static str,
    }

    #[async_trait]
    impl ThoughtGenerator for MockGenerator {
        async fn generate(
            &self,
            parent: &ThoughtState,
            num_branches: usize,
            _prompt_override: Option<&str>,
        ) -> Result<Vec<String>, DecomposeError> {
            Ok((0..num_branches)
                .map(|index| format!("{}{}-{index}", parent.content, self.suffix))
                .collect())
        }
    }

    struct ImprovingGenerator;

    #[async_trait]
    impl ThoughtGenerator for ImprovingGenerator {
        async fn generate(
            &self,
            parent: &ThoughtState,
            num_branches: usize,
            _prompt_override: Option<&str>,
        ) -> Result<Vec<String>, DecomposeError> {
            Ok((0..num_branches)
                .map(|_| format!("{}-improved", parent.content))
                .collect())
        }
    }

    struct FailingGenerator;

    #[async_trait]
    impl ThoughtGenerator for FailingGenerator {
        async fn generate(
            &self,
            _parent: &ThoughtState,
            _num_branches: usize,
            _prompt_override: Option<&str>,
        ) -> Result<Vec<String>, DecomposeError> {
            Err(DecomposeError::DecompositionFailed(
                "simulated generation failure".to_string(),
            ))
        }
    }

    struct FixedScorer {
        scores: HashMap<String, f64>,
        default_score: f64,
    }

    #[async_trait]
    impl ThoughtScorer for FixedScorer {
        async fn score(
            &self,
            thought: &ThoughtState,
            _criteria: &str,
        ) -> Result<f64, DecomposeError> {
            Ok(*self
                .scores
                .get(&thought.content)
                .unwrap_or(&self.default_score))
        }
    }

    struct JoiningMerger;

    #[async_trait]
    impl ThoughtMerger for JoiningMerger {
        async fn merge(
            &self,
            thoughts: &[&ThoughtState],
            _instruction: Option<&str>,
        ) -> Result<String, DecomposeError> {
            Ok(thoughts
                .iter()
                .map(|thought| thought.content.clone())
                .collect::<Vec<_>>()
                .join(" + "))
        }
    }

    fn linear_graph(operations: Vec<GraphOperation>) -> GraphOfOperations {
        let mut graph = GraphOfOperations::new(2);
        let mut previous = None;
        for operation in operations {
            let node = graph.add_node(operation, None);
            if let Some(previous) = previous {
                graph.add_edge(previous, node).unwrap();
            }
            previous = Some(node);
        }
        graph
    }

    #[test]
    fn parse_llm_score_extracts_wrapped_numeric_response() {
        assert_eq!(
            parse_llm_score("I'd rate this 0.7 because it's mostly correct"),
            0.7
        );
        assert_eq!(parse_llm_score("0.85"), 0.85);
        assert_eq!(parse_llm_score("Score: 0.6/1.0"), 0.6);
        assert_eq!(parse_llm_score("The quality is moderate"), 0.5);
    }

    #[tokio::test]
    async fn generate_score_keep_best_and_merge_produce_single_best_thought() {
        let graph = linear_graph(vec![
            GraphOperation::Generate {
                num_branches: 3,
                prompt_override: None,
            },
            GraphOperation::Score {
                strategy: ScoringStrategy::LlmRating {
                    criteria: "quality".to_string(),
                },
            },
            GraphOperation::KeepBest { n: 2 },
            GraphOperation::Merge {
                strategy: MergeStrategy::Concatenate {
                    separator: " + ".to_string(),
                },
            },
        ]);

        let dispatcher = GraphDispatcher::new(
            Arc::new(MockGenerator { suffix: "-branch" }),
            Arc::new(FixedScorer {
                scores: HashMap::from([
                    ("seed-branch-0".to_string(), 0.1),
                    ("seed-branch-1".to_string(), 0.9),
                    ("seed-branch-2".to_string(), 0.5),
                ]),
                default_score: 0.0,
            }),
            Arc::new(JoiningMerger),
        );

        let result = dispatcher
            .execute(&graph, "seed".to_string(), ThoughtMetadata::Empty, None)
            .await
            .unwrap();

        assert_eq!(result.operations_executed, 4);
        assert_eq!(result.llm_calls, 6);
        assert_eq!(result.thoughts.len(), 1);
        assert_eq!(
            result.thoughts[0].content,
            "seed-branch-1 + seed-branch-2".to_string()
        );
        assert_eq!(
            result.best.as_ref().map(|thought| thought.content.as_str()),
            Some("seed-branch-1 + seed-branch-2")
        );
    }

    #[tokio::test]
    async fn validate_assigns_pass_fail_scores() {
        let graph = linear_graph(vec![GraphOperation::Validate {
            strategy: ValidationStrategy::ExactMatch {
                expected: "answer".to_string(),
            },
        }]);

        let dispatcher = GraphDispatcher::new(
            Arc::new(MockGenerator { suffix: "-branch" }),
            Arc::new(FixedScorer {
                scores: HashMap::new(),
                default_score: 0.0,
            }),
            Arc::new(JoiningMerger),
        );

        let result = dispatcher
            .execute(&graph, " answer ".to_string(), ThoughtMetadata::Empty, None)
            .await
            .unwrap();

        assert_eq!(result.operations_executed, 1);
        assert_eq!(result.llm_calls, 0);
        assert_eq!(result.thoughts.len(), 1);
        assert_eq!(result.thoughts[0].score, Some(1.0));
    }

    #[tokio::test]
    async fn refine_exits_early_when_target_score_is_reached() {
        let graph = linear_graph(vec![GraphOperation::Refine {
            max_iterations: 3,
            target_score: 0.9,
            scoring: ScoringStrategy::LlmRating {
                criteria: "quality".to_string(),
            },
        }]);

        let dispatcher = GraphDispatcher::new(
            Arc::new(ImprovingGenerator),
            Arc::new(FixedScorer {
                scores: HashMap::from([
                    ("draft".to_string(), 0.4),
                    ("draft-improved".to_string(), 0.95),
                ]),
                default_score: 0.1,
            }),
            Arc::new(JoiningMerger),
        );

        let result = dispatcher
            .execute(&graph, "draft".to_string(), ThoughtMetadata::Empty, None)
            .await
            .unwrap();

        assert!(!result.refinement_capped);
        assert_eq!(result.operations_executed, 1);
        assert_eq!(result.llm_calls, 3);
        assert_eq!(result.thoughts.len(), 1);
        assert_eq!(result.thoughts[0].content, "draft-improved");
        assert_eq!(result.thoughts[0].score, Some(0.95));
    }

    #[tokio::test]
    async fn refine_marks_result_when_iteration_cap_is_hit() {
        let graph = linear_graph(vec![GraphOperation::Refine {
            max_iterations: 2,
            target_score: 0.9,
            scoring: ScoringStrategy::LlmRating {
                criteria: "quality".to_string(),
            },
        }]);

        let dispatcher = GraphDispatcher::new(
            Arc::new(ImprovingGenerator),
            Arc::new(FixedScorer {
                scores: HashMap::from([
                    ("draft".to_string(), 0.2),
                    ("draft-improved".to_string(), 0.3),
                ]),
                default_score: 0.3,
            }),
            Arc::new(JoiningMerger),
        );

        let result = dispatcher
            .execute(&graph, "draft".to_string(), ThoughtMetadata::Empty, None)
            .await
            .unwrap();

        assert!(result.refinement_capped);
        assert_eq!(result.operations_executed, 1);
        assert_eq!(result.llm_calls, 4);
        assert_eq!(result.thoughts.len(), 1);
        assert_eq!(result.thoughts[0].content, "draft-improved-improved");
    }

    #[tokio::test]
    async fn back_edges_repeat_until_iteration_budget_then_fall_through() {
        let mut graph = GraphOfOperations::new(2);
        let score = graph.add_node(
            GraphOperation::Score {
                strategy: ScoringStrategy::LlmRating {
                    criteria: "quality".to_string(),
                },
            },
            Some("score".to_string()),
        );
        let improve = graph.add_node(
            GraphOperation::Generate {
                num_branches: 1,
                prompt_override: Some("improve".to_string()),
            },
            Some("improve".to_string()),
        );
        let validate = graph.add_node(
            GraphOperation::Validate {
                strategy: ValidationStrategy::AlwaysPass,
            },
            Some("validate".to_string()),
        );

        graph.add_edge(score, improve).unwrap();
        graph.add_back_edge(improve, score).unwrap();
        graph.add_edge(improve, validate).unwrap();

        let dispatcher = GraphDispatcher::new(
            Arc::new(MockGenerator { suffix: "-x" }),
            Arc::new(FixedScorer {
                scores: HashMap::new(),
                default_score: 0.3,
            }),
            Arc::new(JoiningMerger),
        );

        let result = dispatcher
            .execute(&graph, "seed".to_string(), ThoughtMetadata::Empty, None)
            .await
            .unwrap();

        assert_eq!(result.operations_executed, 7);
        assert_eq!(result.llm_calls, 6);
        assert_eq!(result.thoughts.len(), 1);
        assert_eq!(result.thoughts[0].content, "seed-x-0-x-0-x-0");
        assert_eq!(result.thoughts[0].score, Some(1.0));
    }

    #[tokio::test]
    async fn generate_failure_preserves_parent_until_a_complete_result_exists() {
        let dispatcher = GraphDispatcher::new(
            Arc::new(FailingGenerator),
            Arc::new(FixedScorer {
                scores: HashMap::new(),
                default_score: 0.0,
            }),
            Arc::new(JoiningMerger),
        );
        let mut pool = ThoughtPool::new();
        pool.create("seed".to_string(), Vec::new(), ThoughtMetadata::Empty);
        let mut llm_calls = 0usize;

        let error = dispatcher
            .apply_generate(GraphNodeId::new(4), &mut pool, 3, None, &mut llm_calls)
            .await
            .unwrap_err();

        assert!(matches!(error, DecomposeError::DecompositionFailed(_)));
        assert_eq!(llm_calls, 0);
        assert_eq!(pool.len(), 1);
        assert_eq!(pool.active_ids().len(), 1);
        let only_thought = pool.get(pool.active_ids()[0]).unwrap();
        assert_eq!(only_thought.content, "seed");
        assert_eq!(only_thought.origin_operation, None);
    }
}
