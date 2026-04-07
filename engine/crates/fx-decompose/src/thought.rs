use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;

/// Unique identifier for a thought node in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ThoughtId(u64);

impl ThoughtId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Typed wrapper for a node index in the operation graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GraphNodeId(usize);

impl GraphNodeId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for GraphNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Typed metadata carried by a thought.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum ThoughtMetadata {
    /// No metadata (default for most operations).
    #[default]
    Empty,
    /// Key-value metadata produced by domain-specific operations.
    Fields(HashMap<String, ThoughtMetadataValue>),
}

/// A single metadata value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ThoughtMetadataValue {
    Text(String),
    Number(f64),
    Bool(bool),
}

/// A single thought in the graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThoughtState {
    id: ThoughtId,
    pub content: String,
    pub score: Option<f64>,
    pub metadata: ThoughtMetadata,
    pub parent_ids: Vec<ThoughtId>,
    pub origin_operation: Option<GraphNodeId>,
}

impl ThoughtState {
    pub fn new(
        id: ThoughtId,
        content: String,
        metadata: ThoughtMetadata,
        parent_ids: Vec<ThoughtId>,
        origin_operation: Option<GraphNodeId>,
    ) -> Self {
        Self {
            id,
            content,
            score: None,
            metadata,
            parent_ids,
            origin_operation,
        }
    }

    pub const fn id(&self) -> ThoughtId {
        self.id
    }
}

/// Allocator for thought IDs within a single graph execution.
#[derive(Debug, Default)]
pub struct ThoughtIdAllocator {
    next: u64,
}

impl ThoughtIdAllocator {
    /// Allocate the next monotonic ID for this graph execution.
    pub fn allocate(&mut self) -> ThoughtId {
        let id = ThoughtId::new(self.next);
        self.next = self.next.checked_add(1).expect("thought id overflow");
        id
    }

    fn advance_past(&mut self, id: ThoughtId) {
        let next_after_id = id.value().checked_add(1).expect("thought id overflow");
        self.next = self.next.max(next_after_id);
    }
}

/// Container for all thoughts in a graph execution.
#[derive(Debug, Default)]
pub struct ThoughtPool {
    thoughts: HashMap<ThoughtId, ThoughtState>,
    allocator: ThoughtIdAllocator,
}

impl ThoughtPool {
    /// Create an empty pool with a fresh monotonic allocator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a fully-constructed thought and advance allocation past its ID.
    pub fn insert(&mut self, state: ThoughtState) -> ThoughtId {
        let id = state.id();
        self.allocator.advance_past(id);
        self.thoughts.insert(id, state);
        id
    }

    /// Create a new unscored thought owned by this pool.
    pub fn create(
        &mut self,
        content: String,
        parents: Vec<ThoughtId>,
        metadata: ThoughtMetadata,
    ) -> ThoughtId {
        let thought_id = self.allocator.allocate();
        let state = ThoughtState::new(thought_id, content, metadata, parents, None);
        self.insert(state)
    }

    /// Return an immutable view of a thought by ID.
    pub fn get(&self, id: ThoughtId) -> Option<&ThoughtState> {
        self.thoughts.get(&id)
    }

    /// Return a mutable view of a thought without exposing its immutable ID.
    pub fn get_mut(&mut self, id: ThoughtId) -> Option<&mut ThoughtState> {
        self.thoughts.get_mut(&id)
    }

    /// Remove a thought from the pool, returning ownership if it existed.
    pub fn remove(&mut self, id: ThoughtId) -> Option<ThoughtState> {
        self.thoughts.remove(&id)
    }

    /// Return all scored thoughts ordered by `ThoughtId` for deterministic iteration.
    pub fn scored(&self) -> Vec<&ThoughtState> {
        let mut thoughts: Vec<&ThoughtState> = self
            .thoughts
            .values()
            .filter(|state| state.score.is_some())
            .collect();
        thoughts.sort_unstable_by_key(|state| state.id());
        thoughts
    }

    /// Return the highest-scoring thoughts in descending score order with ID tie-breaks.
    pub fn top_n(&self, n: usize) -> Vec<&ThoughtState> {
        let mut thoughts = self.scored();
        thoughts.sort_by(compare_scored_thoughts);
        thoughts.truncate(n);
        thoughts
    }

    /// Return all currently tracked thought IDs in ascending order.
    pub fn active_ids(&self) -> Vec<ThoughtId> {
        let mut ids: Vec<ThoughtId> = self.thoughts.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Return the number of thoughts currently stored in the pool.
    pub fn len(&self) -> usize {
        self.thoughts.len()
    }

    /// Return `true` when the pool has no tracked thoughts.
    pub fn is_empty(&self) -> bool {
        self.thoughts.is_empty()
    }
}

fn compare_scored_thoughts(left: &&ThoughtState, right: &&ThoughtState) -> Ordering {
    let left_score = left
        .score
        .expect("scored thoughts always carry a score before sorting");
    let right_score = right
        .score
        .expect("scored thoughts always carry a score before sorting");

    right_score
        .total_cmp(&left_score)
        .then_with(|| left.id().cmp(&right.id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields_metadata<'a>(
        entries: impl IntoIterator<Item = (&'a str, ThoughtMetadataValue)>,
    ) -> ThoughtMetadata {
        ThoughtMetadata::Fields(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    #[test]
    fn thought_metadata_defaults_to_empty() {
        assert_eq!(ThoughtMetadata::default(), ThoughtMetadata::Empty);
    }

    #[test]
    fn graph_node_id_displays_as_raw_index() {
        assert_eq!(GraphNodeId::new(3).to_string(), "3");
    }

    #[test]
    fn thought_metadata_fields_roundtrip_serde() {
        let metadata = fields_metadata([
            ("summary", ThoughtMetadataValue::Text("draft".to_string())),
            ("confidence", ThoughtMetadataValue::Number(0.82)),
            ("accepted", ThoughtMetadataValue::Bool(true)),
        ]);

        let encoded = serde_json::to_string(&metadata).expect("serialize metadata");
        let decoded: ThoughtMetadata =
            serde_json::from_str(&encoded).expect("deserialize metadata");

        assert_eq!(decoded, metadata);
    }

    #[test]
    fn insert_returns_existing_id_and_advances_allocator() {
        let mut pool = ThoughtPool::new();
        let mut state = ThoughtState::new(
            ThoughtId::new(4),
            "manual seed".to_string(),
            ThoughtMetadata::Empty,
            Vec::new(),
            Some(GraphNodeId::new(2)),
        );
        state.score = Some(0.3);
        let inserted_id = pool.insert(state);

        assert_eq!(inserted_id, ThoughtId::new(4));
        assert_eq!(
            pool.get(inserted_id).map(|state| state.content.as_str()),
            Some("manual seed")
        );

        let next_id = pool.create("generated".to_string(), Vec::new(), ThoughtMetadata::Empty);
        assert_eq!(next_id, ThoughtId::new(5));
    }

    #[test]
    fn create_allocates_sequential_ids_and_stores_state() {
        let mut pool = ThoughtPool::new();
        let root_id = pool.create(
            "root".to_string(),
            Vec::new(),
            fields_metadata([
                ("label", ThoughtMetadataValue::Text("seed".to_string())),
                ("score", ThoughtMetadataValue::Number(1.0)),
                ("viable", ThoughtMetadataValue::Bool(true)),
            ]),
        );
        let child_id = pool.create("child".to_string(), vec![root_id], ThoughtMetadata::Empty);

        assert_eq!(root_id, ThoughtId::new(0));
        assert_eq!(child_id, ThoughtId::new(1));

        let child = pool.get(child_id).expect("child thought");
        assert_eq!(child.parent_ids, vec![root_id]);
        assert_eq!(child.score, None);
        assert_eq!(child.origin_operation, None);
    }

    #[test]
    fn remove_returns_state_and_deletes_it_from_pool() {
        let mut pool = ThoughtPool::new();
        let thought_id = pool.create("temporary".to_string(), Vec::new(), ThoughtMetadata::Empty);

        let removed = pool.remove(thought_id).expect("removed thought");

        assert_eq!(removed.id(), thought_id);
        assert!(pool.get(thought_id).is_none());
        assert!(pool.is_empty());
    }

    #[test]
    fn scored_returns_only_thoughts_with_scores() {
        let mut pool = ThoughtPool::new();
        let low_id = pool.create("low".to_string(), Vec::new(), ThoughtMetadata::Empty);
        let unscored_id = pool.create("pending".to_string(), Vec::new(), ThoughtMetadata::Empty);
        let high_id = pool.create("high".to_string(), Vec::new(), ThoughtMetadata::Empty);

        pool.get_mut(low_id).expect("low").score = Some(0.2);
        pool.get_mut(high_id).expect("high").score = Some(0.9);

        let scored_ids: Vec<ThoughtId> =
            pool.scored().into_iter().map(|state| state.id()).collect();

        assert_eq!(scored_ids, vec![low_id, high_id]);
        assert!(pool.get(unscored_id).expect("pending").score.is_none());
    }

    #[test]
    fn top_n_orders_by_score_descending_with_stable_tie_breaks() {
        let mut pool = ThoughtPool::new();
        let first_id = pool.create("first".to_string(), Vec::new(), ThoughtMetadata::Empty);
        let second_id = pool.create("second".to_string(), Vec::new(), ThoughtMetadata::Empty);
        let third_id = pool.create("third".to_string(), Vec::new(), ThoughtMetadata::Empty);
        let unscored_id = pool.create("pending".to_string(), Vec::new(), ThoughtMetadata::Empty);

        pool.get_mut(first_id).expect("first").score = Some(0.4);
        pool.get_mut(second_id).expect("second").score = Some(0.9);
        pool.get_mut(third_id).expect("third").score = Some(0.9);

        let top_ids: Vec<ThoughtId> = pool.top_n(3).into_iter().map(|state| state.id()).collect();

        assert_eq!(top_ids, vec![second_id, third_id, first_id]);
        assert!(pool.get(unscored_id).expect("pending").score.is_none());
    }
}
