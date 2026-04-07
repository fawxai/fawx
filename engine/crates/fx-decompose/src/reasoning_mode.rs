use crate::{GraphBuilder, GraphNodeId, GraphOfOperations, GraphOperation, GraphTopologyError};
use serde::{Deserialize, Serialize};

pub(crate) const DEFAULT_PRESET_BRANCHES: usize = 3;
pub(crate) const DEFAULT_GRAPH_KEEP: usize = 2;
pub(crate) const DEFAULT_GRAPH_REFINE_ITERATIONS: usize = 2;
pub(crate) const DEFAULT_GRAPH_TARGET_SCORE: f64 = 0.95;

/// How the agent should reason through a decomposition request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum ReasoningMode {
    /// Existing decomposition behavior.
    #[default]
    Standard,
    /// Graph of Thoughts reasoning using a serializable graph specification.
    GraphOfThoughts { graph: GraphOfOperationsSpec },
}

impl ReasoningMode {
    pub const fn is_standard(&self) -> bool {
        matches!(self, Self::Standard)
    }

    pub fn estimated_llm_calls_lower_bound(&self) -> u32 {
        match self {
            Self::Standard => 0,
            Self::GraphOfThoughts { graph } => graph.estimated_llm_calls_lower_bound(),
        }
    }
}

/// Serializable specification for a Graph of Thoughts execution graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GraphOfOperationsSpec {
    /// Build a graph from a named preset.
    Preset {
        name: GoTPreset,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branches: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refine_iterations: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_score: Option<f64>,
        criteria: String,
    },
    /// Build a graph from explicit operations and edges.
    ///
    /// This variant is reserved for programmatic callers and tests; the kernel
    /// tool boundary currently exposes only named presets.
    Custom {
        operations: Vec<GraphOperation>,
        #[serde(default)]
        edges: Vec<EdgeSpec>,
        max_iterations_per_cycle: usize,
    },
}

impl GraphOfOperationsSpec {
    pub fn build(&self) -> Result<GraphOfOperations, GraphTopologyError> {
        match self {
            Self::Preset {
                name,
                branches,
                keep,
                refine_iterations,
                target_score,
                criteria,
            } => build_preset_graph(
                *name,
                *branches,
                *keep,
                *refine_iterations,
                *target_score,
                criteria,
            ),
            Self::Custom {
                operations,
                edges,
                max_iterations_per_cycle,
            } => build_custom_graph(operations, edges, *max_iterations_per_cycle),
        }
    }

    pub fn estimated_llm_calls_lower_bound(&self) -> u32 {
        match self {
            Self::Preset { name, branches, .. } => {
                preset_llm_calls_lower_bound(*name, branches.unwrap_or(DEFAULT_PRESET_BRANCHES))
            }
            Self::Custom { operations, .. } => custom_llm_calls_lower_bound(operations),
        }
    }
}

/// A single edge in a custom graph specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EdgeSpec {
    pub from: GraphNodeId,
    pub to: GraphNodeId,
    pub is_back_edge: bool,
}

/// Named Graph of Thoughts presets exposed at the tool boundary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GoTPreset {
    ChainOfThought,
    TreeOfThought,
    GraphOfThought,
    Consensus,
}

fn preset_llm_calls_lower_bound(preset: GoTPreset, branches: usize) -> u32 {
    let branches = saturating_u32(branches);
    match preset {
        GoTPreset::ChainOfThought => 2,
        GoTPreset::TreeOfThought => branches.saturating_mul(2),
        GoTPreset::GraphOfThought => branches.saturating_mul(2).saturating_add(2),
        GoTPreset::Consensus => branches.saturating_mul(2).saturating_add(1),
    }
}

fn custom_llm_calls_lower_bound(operations: &[GraphOperation]) -> u32 {
    let mut active_thoughts = 1_u32;
    let mut llm_calls = 0_u32;

    for operation in operations {
        match operation {
            GraphOperation::Generate { num_branches, .. } => {
                let branches = saturating_u32(*num_branches);
                llm_calls = llm_calls.saturating_add(active_thoughts.saturating_mul(branches));
                active_thoughts = active_thoughts.saturating_mul(branches);
            }
            GraphOperation::Score { strategy } => {
                if uses_llm_scoring(strategy) {
                    llm_calls = llm_calls.saturating_add(active_thoughts);
                }
            }
            GraphOperation::KeepBest { n } => {
                active_thoughts = active_thoughts.min(saturating_u32(*n));
            }
            GraphOperation::Merge { strategy } => {
                if active_thoughts > 0 {
                    if matches!(strategy, crate::MergeStrategy::LlmSynthesis { .. }) {
                        llm_calls = llm_calls.saturating_add(1);
                    }
                    active_thoughts = 1;
                }
            }
            GraphOperation::Refine {
                max_iterations,
                scoring,
                ..
            } => {
                if *max_iterations > 0 && uses_llm_scoring(scoring) {
                    // Lower bound: refine always scores once before it can stop.
                    llm_calls = llm_calls.saturating_add(active_thoughts);
                }
            }
            GraphOperation::Validate { strategy } => {
                if matches!(strategy, crate::ValidationStrategy::LlmJudge { .. }) {
                    llm_calls = llm_calls.saturating_add(active_thoughts);
                }
            }
        }
    }

    llm_calls
}

fn uses_llm_scoring(strategy: &crate::ScoringStrategy) -> bool {
    matches!(strategy, crate::ScoringStrategy::LlmRating { .. })
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn build_preset_graph(
    preset: GoTPreset,
    branches: Option<usize>,
    keep: Option<usize>,
    refine_iterations: Option<usize>,
    target_score: Option<f64>,
    criteria: &str,
) -> Result<GraphOfOperations, GraphTopologyError> {
    let branches = branches.unwrap_or(DEFAULT_PRESET_BRANCHES);
    match preset {
        GoTPreset::ChainOfThought => GraphBuilder::chain_of_thought(criteria),
        GoTPreset::TreeOfThought => GraphBuilder::tree_of_thought(branches, criteria),
        GoTPreset::GraphOfThought => GraphBuilder::graph_of_thought(
            branches,
            keep.unwrap_or(DEFAULT_GRAPH_KEEP),
            refine_iterations.unwrap_or(DEFAULT_GRAPH_REFINE_ITERATIONS),
            target_score.unwrap_or(DEFAULT_GRAPH_TARGET_SCORE),
            criteria,
        ),
        GoTPreset::Consensus => GraphBuilder::consensus(branches, criteria),
    }
}

fn build_custom_graph(
    operations: &[GraphOperation],
    edges: &[EdgeSpec],
    max_iterations_per_cycle: usize,
) -> Result<GraphOfOperations, GraphTopologyError> {
    let mut graph = GraphOfOperations::new(max_iterations_per_cycle);
    let mut node_ids = Vec::with_capacity(operations.len());

    for operation in operations {
        node_ids.push(graph.add_node(operation.clone(), Some(operation.to_string())));
    }

    if edges.is_empty() {
        for window in node_ids.windows(2) {
            graph.add_edge(window[0], window[1])?;
        }
    } else {
        for edge in edges {
            if edge.is_back_edge {
                graph.add_back_edge(edge.from, edge.to)?;
            } else {
                graph.add_edge(edge.from, edge.to)?;
            }
        }
    }

    graph.validate()?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MergeStrategy, ScoringStrategy, ValidationStrategy};

    #[test]
    fn reasoning_mode_defaults_to_standard() {
        assert_eq!(ReasoningMode::default(), ReasoningMode::Standard);
        assert!(ReasoningMode::default().is_standard());
    }

    #[test]
    fn preset_graph_specs_build_expected_topologies() {
        let chain = GraphOfOperationsSpec::Preset {
            name: GoTPreset::ChainOfThought,
            branches: None,
            keep: None,
            refine_iterations: None,
            target_score: None,
            criteria: "focus".to_string(),
        }
        .build()
        .expect("chain preset");
        let tree = GraphOfOperationsSpec::Preset {
            name: GoTPreset::TreeOfThought,
            branches: Some(4),
            keep: None,
            refine_iterations: None,
            target_score: None,
            criteria: "correctness".to_string(),
        }
        .build()
        .expect("tree preset");
        let graph = GraphOfOperationsSpec::Preset {
            name: GoTPreset::GraphOfThought,
            branches: Some(3),
            keep: None,
            refine_iterations: None,
            target_score: None,
            criteria: "quality".to_string(),
        }
        .build()
        .expect("graph preset");

        assert_eq!(chain.len(), 2);
        assert_eq!(tree.len(), 3);
        assert_eq!(graph.len(), 6);
        assert!(matches!(
            chain.nodes()[0].operation(),
            GraphOperation::Generate {
                num_branches: 1,
                prompt_override: None
            }
        ));
        assert!(matches!(
            graph.nodes()[4].operation(),
            GraphOperation::Refine {
                max_iterations: 2,
                target_score,
                scoring: ScoringStrategy::LlmRating { criteria },
            } if *target_score == DEFAULT_GRAPH_TARGET_SCORE && criteria == "quality"
        ));
    }

    #[test]
    fn custom_spec_auto_wires_linearly_when_edges_are_omitted() {
        let graph = GraphOfOperationsSpec::Custom {
            operations: vec![
                GraphOperation::Generate {
                    num_branches: 2,
                    prompt_override: None,
                },
                GraphOperation::Score {
                    strategy: ScoringStrategy::LlmRating {
                        criteria: "quality".to_string(),
                    },
                },
                GraphOperation::Merge {
                    strategy: MergeStrategy::Concatenate {
                        separator: " + ".to_string(),
                    },
                },
                GraphOperation::Validate {
                    strategy: ValidationStrategy::AlwaysPass,
                },
            ],
            edges: Vec::new(),
            max_iterations_per_cycle: 2,
        }
        .build()
        .expect("custom graph");

        assert_eq!(graph.entry(), GraphNodeId::new(0));
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
    }

    #[test]
    fn llm_call_lower_bound_matches_presets() {
        let chain = GraphOfOperationsSpec::Preset {
            name: GoTPreset::ChainOfThought,
            branches: None,
            keep: None,
            refine_iterations: None,
            target_score: None,
            criteria: "clarity".to_string(),
        };
        let graph = GraphOfOperationsSpec::Preset {
            name: GoTPreset::GraphOfThought,
            branches: Some(3),
            keep: None,
            refine_iterations: None,
            target_score: None,
            criteria: "quality".to_string(),
        };

        assert_eq!(chain.estimated_llm_calls_lower_bound(), 2);
        assert_eq!(graph.estimated_llm_calls_lower_bound(), 8);
    }
}
