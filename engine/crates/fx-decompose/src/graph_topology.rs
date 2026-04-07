use crate::{GraphNodeId, GraphOperation};
use std::collections::HashSet;

/// A node in the operation graph.
#[derive(Debug, Clone)]
pub struct GraphNode {
    /// Typed index of this node in the graph's node list.
    pub id: GraphNodeId,
    /// The operation this node performs.
    pub operation: GraphOperation,
    /// Human-readable label for debugging/logging.
    pub label: Option<String>,
}

/// A directed edge in the operation graph.
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: GraphNodeId,
    pub to: GraphNodeId,
    /// If true, this is a back-edge (cycle). Subject to iteration limits.
    pub is_back_edge: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum GraphTopologyError {
    #[error("node {0:?} out of bounds (graph has {1} nodes)")]
    NodeOutOfBounds(GraphNodeId, usize),

    #[error(
        "forward edge from {0:?} to {1:?} is invalid (target must be > source for forward edges)"
    )]
    InvalidForwardEdge(GraphNodeId, GraphNodeId),

    #[error("back-edge from {0:?} to {1:?} is invalid (target must be <= source for back-edges)")]
    InvalidBackEdge(GraphNodeId, GraphNodeId),

    #[error("empty graph (no nodes)")]
    EmptyGraph,

    #[error("entry node {0:?} does not exist")]
    InvalidEntry(GraphNodeId),

    #[error("duplicate edge from {0:?} to {1:?}")]
    DuplicateEdge(GraphNodeId, GraphNodeId),
}

/// A directed graph of reasoning operations.
///
/// Unlike `ExecutionDag` (which is strictly level-based and acyclic), this
/// supports arbitrary edges including back-edges for refinement loops.
///
/// Back-edges are bounded by `max_iterations_per_cycle` to prevent runaway execution.
///
/// Invariant: nodes must be added in topological order of the forward-edge
/// subgraph. The builder will guarantee this for callers that do not construct
/// the graph directly.
#[derive(Debug, Clone)]
pub struct GraphOfOperations {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    entry: GraphNodeId,
    max_iterations_per_cycle: usize,
}

impl GraphOfOperations {
    /// Create a new graph. Entry defaults to node 0.
    pub fn new(max_iterations_per_cycle: usize) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            entry: GraphNodeId::new(0),
            max_iterations_per_cycle,
        }
    }

    /// Add a node and return its typed graph ID.
    ///
    /// Invariant: callers must add nodes in topological order of the forward
    /// subgraph. Forward edges may only point to later nodes.
    pub fn add_node(&mut self, operation: GraphOperation, label: Option<String>) -> GraphNodeId {
        let id = GraphNodeId::new(self.nodes.len());
        self.nodes.push(GraphNode {
            id,
            operation,
            label,
        });
        id
    }

    /// Add a forward edge (from -> to). `from` must be lower than `to`.
    pub fn add_edge(
        &mut self,
        from: GraphNodeId,
        to: GraphNodeId,
    ) -> Result<(), GraphTopologyError> {
        self.ensure_node_exists(from)?;
        self.ensure_node_exists(to)?;
        self.validate_forward_edge(from, to)?;
        self.ensure_edge_is_unique(from, to)?;
        self.edges.push(GraphEdge {
            from,
            to,
            is_back_edge: false,
        });
        Ok(())
    }

    /// Add a back-edge (from -> to where `to <= from`).
    pub fn add_back_edge(
        &mut self,
        from: GraphNodeId,
        to: GraphNodeId,
    ) -> Result<(), GraphTopologyError> {
        self.ensure_node_exists(from)?;
        self.ensure_node_exists(to)?;
        self.validate_back_edge(from, to)?;
        self.ensure_edge_is_unique(from, to)?;
        self.edges.push(GraphEdge {
            from,
            to,
            is_back_edge: true,
        });
        Ok(())
    }

    /// Set the entry node.
    pub fn set_entry(&mut self, id: GraphNodeId) -> Result<(), GraphTopologyError> {
        if self.nodes.get(id.index()).is_none() {
            return Err(GraphTopologyError::InvalidEntry(id));
        }
        self.entry = id;
        Ok(())
    }

    /// Get successors of a node (forward edges only).
    pub fn successors(&self, id: GraphNodeId) -> Vec<GraphNodeId> {
        self.edges
            .iter()
            .filter(|edge| edge.from == id && !edge.is_back_edge)
            .map(|edge| edge.to)
            .collect()
    }

    /// Get all successors including back-edges.
    pub fn all_successors(&self, id: GraphNodeId) -> Vec<(GraphNodeId, bool)> {
        self.edges
            .iter()
            .filter(|edge| edge.from == id)
            .map(|edge| (edge.to, edge.is_back_edge))
            .collect()
    }

    /// Get the node by ID.
    pub fn node(&self, id: GraphNodeId) -> Option<&GraphNode> {
        self.nodes.get(id.index())
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Maximum iterations per cycle.
    pub const fn max_iterations(&self) -> usize {
        self.max_iterations_per_cycle
    }

    /// Validate the graph: entry exists, all edge indices valid, at least one node.
    pub fn validate(&self) -> Result<(), GraphTopologyError> {
        if self.is_empty() {
            return Err(GraphTopologyError::EmptyGraph);
        }
        if self.node(self.entry).is_none() {
            return Err(GraphTopologyError::InvalidEntry(self.entry));
        }

        let mut seen_edges = HashSet::with_capacity(self.edges.len());
        for edge in &self.edges {
            self.ensure_node_exists(edge.from)?;
            self.ensure_node_exists(edge.to)?;
            if edge.is_back_edge {
                self.validate_back_edge(edge.from, edge.to)?;
            } else {
                self.validate_forward_edge(edge.from, edge.to)?;
            }
            if !seen_edges.insert((edge.from, edge.to)) {
                return Err(GraphTopologyError::DuplicateEdge(edge.from, edge.to));
            }
        }

        Ok(())
    }

    /// Return all terminal nodes (no outgoing forward edges).
    pub fn terminal_nodes(&self) -> Vec<GraphNodeId> {
        let nodes_with_forward_successors: HashSet<GraphNodeId> = self
            .edges
            .iter()
            .filter(|edge| !edge.is_back_edge)
            .map(|edge| edge.from)
            .collect();

        self.nodes
            .iter()
            .filter(|node| !nodes_with_forward_successors.contains(&node.id))
            .map(|node| node.id)
            .collect()
    }

    fn ensure_node_exists(&self, id: GraphNodeId) -> Result<(), GraphTopologyError> {
        if self.node(id).is_some() {
            Ok(())
        } else {
            Err(GraphTopologyError::NodeOutOfBounds(id, self.nodes.len()))
        }
    }

    fn validate_forward_edge(
        &self,
        from: GraphNodeId,
        to: GraphNodeId,
    ) -> Result<(), GraphTopologyError> {
        if to.index() <= from.index() {
            Err(GraphTopologyError::InvalidForwardEdge(from, to))
        } else {
            Ok(())
        }
    }

    fn validate_back_edge(
        &self,
        from: GraphNodeId,
        to: GraphNodeId,
    ) -> Result<(), GraphTopologyError> {
        if to.index() > from.index() {
            Err(GraphTopologyError::InvalidBackEdge(from, to))
        } else {
            Ok(())
        }
    }

    fn ensure_edge_is_unique(
        &self,
        from: GraphNodeId,
        to: GraphNodeId,
    ) -> Result<(), GraphTopologyError> {
        if self
            .edges
            .iter()
            .any(|edge| edge.from == from && edge.to == to)
        {
            Err(GraphTopologyError::DuplicateEdge(from, to))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScoringStrategy;

    fn score_operation(criteria: &str) -> GraphOperation {
        GraphOperation::Score {
            strategy: ScoringStrategy::LlmRating {
                criteria: criteria.to_string(),
            },
        }
    }

    #[test]
    fn graph_construction_supports_forward_and_back_edges() {
        let mut graph = GraphOfOperations::new(3);
        let start = graph.add_node(score_operation("seed"), Some("start".to_string()));
        let refine = graph.add_node(score_operation("refine"), Some("refine".to_string()));
        let validate = graph.add_node(score_operation("validate"), Some("validate".to_string()));

        graph.add_edge(start, refine).unwrap();
        graph.add_edge(refine, validate).unwrap();
        graph.add_back_edge(validate, refine).unwrap();

        assert_eq!(graph.len(), 3);
        assert!(!graph.is_empty());
        assert_eq!(graph.max_iterations(), 3);
        assert_eq!(
            graph.node(refine).map(|node| node.label.as_deref()),
            Some(Some("refine"))
        );
        assert_eq!(graph.successors(start), vec![refine]);
        assert_eq!(graph.all_successors(validate), vec![(refine, true)]);
        assert_eq!(graph.terminal_nodes(), vec![validate]);
        assert!(graph.validate().is_ok());
    }

    #[test]
    fn rejects_invalid_forward_edge_direction() {
        let mut graph = GraphOfOperations::new(2);
        let first = graph.add_node(score_operation("one"), None);
        let second = graph.add_node(score_operation("two"), None);

        let err = graph.add_edge(second, first).unwrap_err();
        assert!(matches!(
            err,
            GraphTopologyError::InvalidForwardEdge(from, to) if from == second && to == first
        ));

        let err = graph.add_edge(first, first).unwrap_err();
        assert!(matches!(
            err,
            GraphTopologyError::InvalidForwardEdge(from, to) if from == first && to == first
        ));
    }

    #[test]
    fn rejects_invalid_back_edge_direction() {
        let mut graph = GraphOfOperations::new(2);
        let first = graph.add_node(score_operation("one"), None);
        let second = graph.add_node(score_operation("two"), None);

        let err = graph.add_back_edge(first, second).unwrap_err();
        assert!(matches!(
            err,
            GraphTopologyError::InvalidBackEdge(from, to) if from == first && to == second
        ));
    }

    #[test]
    fn allows_self_loop_back_edges() {
        let mut graph = GraphOfOperations::new(2);
        let node = graph.add_node(score_operation("one"), None);

        graph.add_back_edge(node, node).unwrap();

        assert_eq!(graph.all_successors(node), vec![(node, true)]);
        assert_eq!(graph.terminal_nodes(), vec![node]);
        assert!(graph.validate().is_ok());
    }

    #[test]
    fn rejects_duplicate_edges_at_insert_time() {
        let mut graph = GraphOfOperations::new(2);
        let first = graph.add_node(score_operation("one"), None);
        let second = graph.add_node(score_operation("two"), None);

        graph.add_edge(first, second).unwrap();
        let err = graph.add_edge(first, second).unwrap_err();
        assert!(matches!(
            err,
            GraphTopologyError::DuplicateEdge(from, to) if from == first && to == second
        ));
    }

    #[test]
    fn rejects_out_of_bounds_edge_endpoints() {
        let mut graph = GraphOfOperations::new(2);
        let first = graph.add_node(score_operation("one"), None);
        let missing = GraphNodeId::new(8);

        let err = graph.add_edge(first, missing).unwrap_err();
        assert!(matches!(
            err,
            GraphTopologyError::NodeOutOfBounds(id, len) if id == missing && len == 1
        ));

        let err = graph.add_back_edge(missing, first).unwrap_err();
        assert!(matches!(
            err,
            GraphTopologyError::NodeOutOfBounds(id, len) if id == missing && len == 1
        ));
    }

    #[test]
    fn set_entry_rejects_missing_node() {
        let mut graph = GraphOfOperations::new(2);
        graph.add_node(score_operation("one"), None);

        let err = graph.set_entry(GraphNodeId::new(2)).unwrap_err();
        assert!(matches!(
            err,
            GraphTopologyError::InvalidEntry(id) if id == GraphNodeId::new(2)
        ));
    }

    #[test]
    fn validate_rejects_empty_graph() {
        let graph = GraphOfOperations::new(2);
        assert!(matches!(
            graph.validate(),
            Err(GraphTopologyError::EmptyGraph)
        ));
    }

    #[test]
    fn validate_rejects_invalid_entry() {
        let mut graph = GraphOfOperations::new(2);
        graph.add_node(score_operation("one"), None);
        graph.entry = GraphNodeId::new(3);

        assert!(matches!(
            graph.validate(),
            Err(GraphTopologyError::InvalidEntry(id)) if id == GraphNodeId::new(3)
        ));
    }

    #[test]
    fn validate_rejects_out_of_bounds_edges() {
        let mut graph = GraphOfOperations::new(2);
        let first = graph.add_node(score_operation("one"), None);
        graph.edges.push(GraphEdge {
            from: first,
            to: GraphNodeId::new(2),
            is_back_edge: false,
        });

        assert!(matches!(
            graph.validate(),
            Err(GraphTopologyError::NodeOutOfBounds(id, len)) if id == GraphNodeId::new(2) && len == 1
        ));
    }

    #[test]
    fn validate_rejects_invalid_forward_edges_inserted_out_of_band() {
        let mut graph = GraphOfOperations::new(2);
        let first = graph.add_node(score_operation("one"), None);
        let second = graph.add_node(score_operation("two"), None);
        graph.edges.push(GraphEdge {
            from: second,
            to: first,
            is_back_edge: false,
        });

        assert!(matches!(
            graph.validate(),
            Err(GraphTopologyError::InvalidForwardEdge(from, to)) if from == second && to == first
        ));
    }

    #[test]
    fn validate_rejects_invalid_back_edges_inserted_out_of_band() {
        let mut graph = GraphOfOperations::new(2);
        let first = graph.add_node(score_operation("one"), None);
        let second = graph.add_node(score_operation("two"), None);
        graph.edges.push(GraphEdge {
            from: first,
            to: second,
            is_back_edge: true,
        });

        assert!(matches!(
            graph.validate(),
            Err(GraphTopologyError::InvalidBackEdge(from, to)) if from == first && to == second
        ));
    }

    #[test]
    fn validate_rejects_duplicate_edges_inserted_out_of_band() {
        let mut graph = GraphOfOperations::new(2);
        let first = graph.add_node(score_operation("one"), None);
        let second = graph.add_node(score_operation("two"), None);
        let edge = GraphEdge {
            from: first,
            to: second,
            is_back_edge: false,
        };
        graph.edges.push(edge.clone());
        graph.edges.push(edge);

        assert!(matches!(
            graph.validate(),
            Err(GraphTopologyError::DuplicateEdge(from, to)) if from == first && to == second
        ));
    }

    #[test]
    fn terminal_nodes_ignore_back_edges() {
        let mut graph = GraphOfOperations::new(2);
        let start = graph.add_node(score_operation("one"), None);
        let middle = graph.add_node(score_operation("two"), None);
        let end = graph.add_node(score_operation("three"), None);

        graph.add_edge(start, middle).unwrap();
        graph.add_edge(middle, end).unwrap();
        graph.add_back_edge(end, middle).unwrap();

        assert_eq!(graph.terminal_nodes(), vec![end]);
    }
}
