use super::thought_impls::parse_llm_score;
use super::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

struct MockGenerator {
    suffix: &'static str,
    llm_calls: usize,
}

#[async_trait]
impl ThoughtGenerator for MockGenerator {
    async fn generate(
        &self,
        parent: &ThoughtState,
        num_branches: usize,
        _prompt_override: Option<&str>,
    ) -> Result<GeneratedThoughts, DecomposeError> {
        let contents = (0..num_branches)
            .map(|index| format!("{}{}-{index}", parent.content, self.suffix))
            .collect();
        Ok(GeneratedThoughts::new(contents, self.llm_calls))
    }
}

struct ImprovingGenerator {
    llm_calls: usize,
}

#[async_trait]
impl ThoughtGenerator for ImprovingGenerator {
    async fn generate(
        &self,
        parent: &ThoughtState,
        num_branches: usize,
        _prompt_override: Option<&str>,
    ) -> Result<GeneratedThoughts, DecomposeError> {
        let contents = (0..num_branches)
            .map(|_| format!("{}-improved", parent.content))
            .collect();
        Ok(GeneratedThoughts::new(contents, self.llm_calls))
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
    ) -> Result<GeneratedThoughts, DecomposeError> {
        Err(DecomposeError::DecompositionFailed(
            "simulated generation failure".to_string(),
        ))
    }
}

struct ShortGeneratingMock;

#[async_trait]
impl ThoughtGenerator for ShortGeneratingMock {
    async fn generate(
        &self,
        parent: &ThoughtState,
        num_branches: usize,
        _prompt_override: Option<&str>,
    ) -> Result<GeneratedThoughts, DecomposeError> {
        let contents = (0..num_branches.saturating_sub(1))
            .map(|index| format!("{}-short-{index}", parent.content))
            .collect();
        Ok(GeneratedThoughts::new(contents, 1))
    }
}

struct FixedScorer {
    scores: HashMap<String, f64>,
    default_score: f64,
    llm_calls: usize,
}

#[async_trait]
impl ThoughtScorer for FixedScorer {
    async fn score(
        &self,
        thought: &ThoughtState,
        _criteria: &str,
    ) -> Result<ThoughtScore, DecomposeError> {
        let value = *self
            .scores
            .get(&thought.content)
            .unwrap_or(&self.default_score);
        Ok(ThoughtScore::new(value, self.llm_calls))
    }
}

struct JoiningMerger;

#[async_trait]
impl ThoughtMerger for JoiningMerger {
    async fn merge(
        &self,
        thoughts: &[&ThoughtState],
        _instruction: Option<&str>,
    ) -> Result<MergedThought, DecomposeError> {
        Ok(MergedThought::new(
            thoughts
                .iter()
                .map(|thought| thought.content.clone())
                .collect::<Vec<_>>()
                .join(" + "),
            0,
        ))
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

#[test]
fn refine_prompt_describes_heuristic_matchers_explicitly() {
    let thought = ThoughtState::new(
        crate::ThoughtId::new(1),
        "draft".to_string(),
        ThoughtMetadata::Empty,
        Vec::new(),
        None,
    );
    let prompt = build_refine_prompt(
        &thought,
        0,
        0.9,
        &ScoringStrategy::Heuristic {
            pattern: r"\d+\.\d+".to_string(),
        },
    );

    assert!(prompt.contains("regular expression"));
    assert!(prompt.contains(r"\d+\.\d+"));
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
        Arc::new(MockGenerator {
            suffix: "-branch",
            llm_calls: 3,
        }),
        Arc::new(FixedScorer {
            scores: HashMap::from([
                ("seed-branch-0".to_string(), 0.1),
                ("seed-branch-1".to_string(), 0.9),
                ("seed-branch-2".to_string(), 0.5),
            ]),
            default_score: 0.0,
            llm_calls: 1,
        }),
        Arc::new(JoiningMerger),
    );

    let result = dispatcher
        .execute(&graph, "seed".to_string(), ThoughtMetadata::Empty)
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
        Arc::new(MockGenerator {
            suffix: "-branch",
            llm_calls: 0,
        }),
        Arc::new(FixedScorer {
            scores: HashMap::new(),
            default_score: 0.0,
            llm_calls: 0,
        }),
        Arc::new(JoiningMerger),
    );

    let result = dispatcher
        .execute(&graph, " answer ".to_string(), ThoughtMetadata::Empty)
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
        Arc::new(ImprovingGenerator { llm_calls: 1 }),
        Arc::new(FixedScorer {
            scores: HashMap::from([
                ("draft".to_string(), 0.4),
                ("draft-improved".to_string(), 0.95),
            ]),
            default_score: 0.1,
            llm_calls: 1,
        }),
        Arc::new(JoiningMerger),
    );

    let result = dispatcher
        .execute(&graph, "draft".to_string(), ThoughtMetadata::Empty)
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
        Arc::new(ImprovingGenerator { llm_calls: 1 }),
        Arc::new(FixedScorer {
            scores: HashMap::from([
                ("draft".to_string(), 0.2),
                ("draft-improved".to_string(), 0.3),
            ]),
            default_score: 0.3,
            llm_calls: 1,
        }),
        Arc::new(JoiningMerger),
    );

    let result = dispatcher
        .execute(&graph, "draft".to_string(), ThoughtMetadata::Empty)
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
        Arc::new(MockGenerator {
            suffix: "-x",
            llm_calls: 1,
        }),
        Arc::new(FixedScorer {
            scores: HashMap::new(),
            default_score: 0.3,
            llm_calls: 1,
        }),
        Arc::new(JoiningMerger),
    );

    let result = dispatcher
        .execute(&graph, "seed".to_string(), ThoughtMetadata::Empty)
        .await
        .unwrap();

    assert_eq!(result.operations_executed, 7);
    assert_eq!(result.llm_calls, 6);
    assert_eq!(result.thoughts.len(), 1);
    assert_eq!(result.thoughts[0].content, "seed-x-0-x-0-x-0");
    assert_eq!(result.thoughts[0].score, Some(1.0));
}

#[tokio::test]
async fn generate_error_preserves_parent_thought() {
    let dispatcher = GraphDispatcher::new(
        Arc::new(FailingGenerator),
        Arc::new(FixedScorer {
            scores: HashMap::new(),
            default_score: 0.0,
            llm_calls: 0,
        }),
        Arc::new(JoiningMerger),
    );
    let mut state = ExecutionState::new("seed".to_string(), ThoughtMetadata::Empty);

    let error = dispatcher
        .apply_generate(GraphNodeId::new(4), &mut state, 3, None)
        .await
        .unwrap_err();

    assert!(matches!(error, DecomposeError::DecompositionFailed(_)));
    assert_eq!(state.counters.llm_calls, 0);
    assert_eq!(state.pool.len(), 1);
    let only_thought = state.pool.get(state.pool.active_ids()[0]).unwrap();
    assert_eq!(only_thought.content, "seed");
    assert_eq!(only_thought.origin_operation, None);
}

#[tokio::test]
async fn generate_short_count_preserves_parent_and_fails_loudly() {
    let dispatcher = GraphDispatcher::new(
        Arc::new(ShortGeneratingMock),
        Arc::new(FixedScorer {
            scores: HashMap::new(),
            default_score: 0.0,
            llm_calls: 0,
        }),
        Arc::new(JoiningMerger),
    );
    let mut state = ExecutionState::new("seed".to_string(), ThoughtMetadata::Empty);

    let error = dispatcher
        .apply_generate(GraphNodeId::new(7), &mut state, 3, None)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("expected 3"));
    assert_eq!(state.pool.len(), 1);
    let only_thought = state.pool.get(state.pool.active_ids()[0]).unwrap();
    assert_eq!(only_thought.content, "seed");
}
