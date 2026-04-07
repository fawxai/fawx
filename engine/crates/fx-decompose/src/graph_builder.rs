use crate::{
    GraphOfOperations, GraphOperation, GraphTopologyError, MergeStrategy, ScoringStrategy,
    ValidationStrategy,
};

const DEFAULT_MAX_ITERATIONS_PER_CYCLE: usize = 1;

/// Fluent builder for constructing `GraphOfOperations`.
///
/// This is a consuming builder: each chain step takes ownership and returns the
/// updated builder, so callers keep a single linear construction path.
///
/// The builder owns topology only. It intentionally does not expose a
/// `max_tokens()` setter because GoT token budgets live at the session and
/// dispatcher layers, which are the single source of truth for execution
/// limits.
#[derive(Debug, Clone)]
pub struct GraphBuilder {
    max_iterations_per_cycle: usize,
    operations: Vec<GraphOperation>,
}

impl GraphBuilder {
    pub fn new(max_iterations_per_cycle: usize) -> Self {
        Self {
            max_iterations_per_cycle,
            operations: Vec::new(),
        }
    }

    pub fn generate(self, num_branches: usize) -> Self {
        self.operation(GraphOperation::Generate {
            num_branches,
            prompt_override: None,
        })
    }

    pub fn generate_with_prompt(self, num_branches: usize, prompt: impl Into<String>) -> Self {
        self.operation(GraphOperation::Generate {
            num_branches,
            prompt_override: Some(prompt.into()),
        })
    }

    pub fn score(self, criteria: impl Into<String>) -> Self {
        self.operation(GraphOperation::Score {
            strategy: ScoringStrategy::LlmRating {
                criteria: criteria.into(),
            },
        })
    }

    pub fn score_heuristic(self, pattern: impl Into<String>) -> Self {
        self.operation(GraphOperation::Score {
            strategy: ScoringStrategy::Heuristic {
                pattern: pattern.into(),
            },
        })
    }

    pub fn keep_best(self, n: usize) -> Self {
        self.operation(GraphOperation::KeepBest { n })
    }

    pub fn merge(self) -> Self {
        self.operation(GraphOperation::Merge {
            strategy: MergeStrategy::LlmSynthesis { instruction: None },
        })
    }

    pub fn merge_with_instruction(self, instruction: impl Into<String>) -> Self {
        self.operation(GraphOperation::Merge {
            strategy: MergeStrategy::LlmSynthesis {
                instruction: Some(instruction.into()),
            },
        })
    }

    pub fn concat(self, separator: impl Into<String>) -> Self {
        self.operation(GraphOperation::Merge {
            strategy: MergeStrategy::Concatenate {
                separator: separator.into(),
            },
        })
    }

    pub fn refine(
        self,
        max_iterations: usize,
        target_score: f64,
        criteria: impl Into<String>,
    ) -> Self {
        self.operation(GraphOperation::Refine {
            max_iterations,
            target_score,
            scoring: ScoringStrategy::LlmRating {
                criteria: criteria.into(),
            },
        })
    }

    pub fn validate_exact(self, expected: impl Into<String>) -> Self {
        self.operation(GraphOperation::Validate {
            strategy: ValidationStrategy::ExactMatch {
                expected: expected.into(),
            },
        })
    }

    pub fn validate_contains(self, expected: impl Into<String>) -> Self {
        self.operation(GraphOperation::Validate {
            strategy: ValidationStrategy::Contains {
                expected: expected.into(),
            },
        })
    }

    pub fn validate_llm(self, criteria: impl Into<String>) -> Self {
        self.operation(GraphOperation::Validate {
            strategy: ValidationStrategy::LlmJudge {
                criteria: criteria.into(),
            },
        })
    }

    pub fn operation(mut self, operation: GraphOperation) -> Self {
        self.operations.push(operation);
        self
    }

    /// Build a graph from the appended operations.
    ///
    /// Non-empty builders are topologically valid by construction because they
    /// always compile to a simple forward-linked chain. `Result` is preserved so
    /// callers get a recoverable error for empty builders and for any future
    /// validation rules added to `GraphOfOperations`.
    pub fn build(self) -> Result<GraphOfOperations, GraphTopologyError> {
        let mut graph = GraphOfOperations::new(self.max_iterations_per_cycle);
        let mut previous_node = None;

        for operation in self.operations {
            let label = operation.to_string();
            let node_id = graph.add_node(operation, Some(label));
            if let Some(previous_node_id) = previous_node {
                graph.add_edge(previous_node_id, node_id)?;
            }
            previous_node = Some(node_id);
        }

        graph.validate()?;
        Ok(graph)
    }

    pub fn chain_of_thought(
        criteria: impl Into<String>,
    ) -> Result<GraphOfOperations, GraphTopologyError> {
        Self::new(DEFAULT_MAX_ITERATIONS_PER_CYCLE)
            .generate(1)
            .score(criteria)
            .build()
    }

    pub fn tree_of_thought(
        branches: usize,
        criteria: impl Into<String>,
    ) -> Result<GraphOfOperations, GraphTopologyError> {
        Self::new(DEFAULT_MAX_ITERATIONS_PER_CYCLE)
            .generate(branches)
            .score(criteria)
            .keep_best(1)
            .build()
    }

    pub fn graph_of_thought(
        branches: usize,
        keep: usize,
        refine_iterations: usize,
        target_score: f64,
        criteria: impl Into<String>,
    ) -> Result<GraphOfOperations, GraphTopologyError> {
        let criteria = criteria.into();

        Self::new(refine_iterations.max(DEFAULT_MAX_ITERATIONS_PER_CYCLE))
            .generate(branches)
            .score(criteria.clone())
            .keep_best(keep)
            .merge()
            .refine(refine_iterations, target_score, criteria)
            .operation(GraphOperation::Validate {
                strategy: ValidationStrategy::AlwaysPass,
            })
            .build()
    }

    pub fn consensus(
        branches: usize,
        criteria: impl Into<String>,
    ) -> Result<GraphOfOperations, GraphTopologyError> {
        Self::new(DEFAULT_MAX_ITERATIONS_PER_CYCLE)
            .generate(branches)
            .score(criteria)
            .merge()
            .build()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        GraphBuilder, GraphNodeId, GraphOfOperations, GraphOperation, GraphTopologyError,
        MergeStrategy, ScoringStrategy, ValidationStrategy,
    };

    fn operations(graph: &GraphOfOperations) -> Vec<GraphOperation> {
        graph
            .nodes()
            .iter()
            .map(|node| node.operation().clone())
            .collect()
    }

    #[test]
    fn build_rejects_empty_graph() {
        let result = GraphBuilder::new(3).build();

        assert!(matches!(result, Err(GraphTopologyError::EmptyGraph)));
    }

    #[test]
    fn builder_auto_wires_linear_operations() {
        let graph = GraphBuilder::new(3)
            .generate(2)
            .score("quality")
            .keep_best(1)
            .merge()
            .validate_exact("done")
            .build()
            .unwrap();

        assert_eq!(graph.entry(), GraphNodeId::new(0));
        assert_eq!(graph.len(), 5);
        assert_eq!(
            graph.successors(GraphNodeId::new(0)),
            vec![GraphNodeId::new(1)]
        );
        assert_eq!(
            graph.successors(GraphNodeId::new(1)),
            vec![GraphNodeId::new(2)]
        );
        assert_eq!(
            graph.successors(GraphNodeId::new(2)),
            vec![GraphNodeId::new(3)]
        );
        assert_eq!(
            graph.successors(GraphNodeId::new(3)),
            vec![GraphNodeId::new(4)]
        );
        assert_eq!(graph.terminal_nodes(), vec![GraphNodeId::new(4)]);
        assert!(matches!(
            operations(&graph).as_slice(),
            [
                GraphOperation::Generate {
                    num_branches: 2,
                    prompt_override: None,
                },
                GraphOperation::Score {
                    strategy: ScoringStrategy::LlmRating { criteria },
                },
                GraphOperation::KeepBest { n: 1 },
                GraphOperation::Merge {
                    strategy: MergeStrategy::LlmSynthesis { instruction: None },
                },
                GraphOperation::Validate {
                    strategy: ValidationStrategy::ExactMatch { expected },
                },
            ] if criteria == "quality" && expected == "done"
        ));
    }

    #[test]
    fn custom_generation_and_scoring_helpers_use_requested_configuration() {
        let graph = GraphBuilder::new(3)
            .generate_with_prompt(3, "be bold")
            .score_heuristic("pass")
            .build()
            .unwrap();

        assert!(matches!(
            operations(&graph).as_slice(),
            [
                GraphOperation::Generate {
                    num_branches: 3,
                    prompt_override: Some(prompt),
                },
                GraphOperation::Score {
                    strategy: ScoringStrategy::Heuristic { pattern },
                },
            ] if prompt == "be bold" && pattern == "pass"
        ));
    }

    #[test]
    fn merge_helpers_select_requested_strategy() {
        let instructed = GraphBuilder::new(3)
            .merge_with_instruction("combine the strongest evidence")
            .build()
            .unwrap();
        let concatenated = GraphBuilder::new(3).concat("\n---\n").build().unwrap();

        assert!(matches!(
            operations(&instructed).as_slice(),
            [GraphOperation::Merge {
                strategy: MergeStrategy::LlmSynthesis { instruction: Some(instruction) },
            }] if instruction == "combine the strongest evidence"
        ));
        assert!(matches!(
            operations(&concatenated).as_slice(),
            [GraphOperation::Merge {
                strategy: MergeStrategy::Concatenate { separator },
            }] if separator == "\n---\n"
        ));
    }

    #[test]
    fn validate_helpers_select_requested_strategy() {
        let contains = GraphBuilder::new(3)
            .validate_contains("needle")
            .build()
            .unwrap();
        let llm = GraphBuilder::new(3)
            .validate_llm("factual accuracy")
            .build()
            .unwrap();

        assert!(matches!(
            operations(&contains).as_slice(),
            [GraphOperation::Validate {
                strategy: ValidationStrategy::Contains { expected },
            }] if expected == "needle"
        ));
        assert!(matches!(
            operations(&llm).as_slice(),
            [GraphOperation::Validate {
                strategy: ValidationStrategy::LlmJudge { criteria },
            }] if criteria == "factual accuracy"
        ));
    }

    #[test]
    fn operation_escape_hatch_appends_raw_operation() {
        let graph = GraphBuilder::new(3)
            .operation(GraphOperation::Score {
                strategy: ScoringStrategy::External,
            })
            .build()
            .unwrap();

        assert!(matches!(
            operations(&graph).as_slice(),
            [GraphOperation::Score {
                strategy: ScoringStrategy::External,
            }]
        ));
    }

    #[test]
    fn refine_followed_by_validate_wires_from_refine_node() {
        let graph = GraphBuilder::new(3)
            .generate(2)
            .refine(2, 0.9, "quality")
            .validate_exact("expected answer")
            .build()
            .unwrap();

        assert_eq!(graph.len(), 3);
        assert_eq!(
            graph.successors(GraphNodeId::new(1)),
            vec![GraphNodeId::new(2)]
        );
        assert_eq!(graph.terminal_nodes(), vec![GraphNodeId::new(2)]);
        assert!(matches!(
            operations(&graph).as_slice(),
            [
                GraphOperation::Generate {
                    num_branches: 2,
                    prompt_override: None,
                },
                GraphOperation::Refine {
                    max_iterations: 2,
                    target_score,
                    scoring: ScoringStrategy::LlmRating { criteria },
                },
                GraphOperation::Validate {
                    strategy: ValidationStrategy::ExactMatch { expected },
                },
            ] if (target_score - 0.9).abs() < f64::EPSILON
                && criteria == "quality"
                && expected == "expected answer"
        ));
    }

    #[test]
    fn all_presets_produce_valid_graphs() {
        let chain = GraphBuilder::chain_of_thought("quality").unwrap();
        let tree = GraphBuilder::tree_of_thought(4, "correctness").unwrap();
        let graph = GraphBuilder::graph_of_thought(4, 2, 3, 0.8, "test").unwrap();
        let consensus = GraphBuilder::consensus(3, "factual accuracy").unwrap();

        assert!(chain.validate().is_ok());
        assert!(tree.validate().is_ok());
        assert!(graph.validate().is_ok());
        assert!(consensus.validate().is_ok());
    }

    #[test]
    fn chain_of_thought_preset_uses_expected_operation_sequence() {
        let graph = GraphBuilder::chain_of_thought("quality").unwrap();

        assert!(matches!(
            operations(&graph).as_slice(),
            [
                GraphOperation::Generate {
                    num_branches: 1,
                    prompt_override: None,
                },
                GraphOperation::Score {
                    strategy: ScoringStrategy::LlmRating { criteria },
                },
            ] if criteria == "quality"
        ));
    }

    #[test]
    fn tree_of_thought_preset_uses_expected_operation_sequence() {
        let graph = GraphBuilder::tree_of_thought(4, "correctness").unwrap();

        assert!(matches!(
            operations(&graph).as_slice(),
            [
                GraphOperation::Generate {
                    num_branches: 4,
                    prompt_override: None,
                },
                GraphOperation::Score {
                    strategy: ScoringStrategy::LlmRating { criteria },
                },
                GraphOperation::KeepBest { n: 1 },
            ] if criteria == "correctness"
        ));
    }

    #[test]
    fn consensus_preset_uses_expected_operation_sequence() {
        let graph = GraphBuilder::consensus(3, "factual accuracy").unwrap();

        assert!(matches!(
            operations(&graph).as_slice(),
            [
                GraphOperation::Generate {
                    num_branches: 3,
                    prompt_override: None,
                },
                GraphOperation::Score {
                    strategy: ScoringStrategy::LlmRating { criteria },
                },
                GraphOperation::Merge {
                    strategy: MergeStrategy::LlmSynthesis { instruction: None },
                },
            ] if criteria == "factual accuracy"
        ));
    }

    #[test]
    fn graph_of_thought_preset_uses_expected_operation_sequence() {
        let graph = GraphBuilder::graph_of_thought(4, 2, 3, 0.8, "math").unwrap();

        assert!(matches!(
            operations(&graph).as_slice(),
            [
                GraphOperation::Generate {
                    num_branches: 4,
                    prompt_override: None,
                },
                GraphOperation::Score {
                    strategy: ScoringStrategy::LlmRating { criteria: score_criteria },
                },
                GraphOperation::KeepBest { n: 2 },
                GraphOperation::Merge {
                    strategy: MergeStrategy::LlmSynthesis { instruction: None },
                },
                GraphOperation::Refine {
                    max_iterations: 3,
                    target_score,
                    scoring: ScoringStrategy::LlmRating { criteria: refine_criteria },
                },
                GraphOperation::Validate {
                    strategy: ValidationStrategy::AlwaysPass,
                },
            ] if (target_score - 0.8).abs() < f64::EPSILON
                && score_criteria == "math"
                && refine_criteria == "math"
        ));
    }

    #[test]
    fn non_empty_builds_are_valid_by_construction() {
        let builds = vec![
            GraphBuilder::new(3).generate(1).build(),
            GraphBuilder::new(3)
                .score_heuristic("pass")
                .keep_best(1)
                .build(),
            GraphBuilder::new(3)
                .generate(2)
                .merge()
                .refine(2, 0.8, "quality")
                .validate_llm("final answer quality")
                .build(),
        ];

        assert!(builds.into_iter().all(|result| result.is_ok()));
    }
}
