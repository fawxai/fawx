use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A registered Fawx node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Unique node identifier.
    pub node_id: String,
    /// Human-readable name.
    pub name: String,
    /// HTTP API endpoint (e.g., "https://100.64.1.5:8400").
    pub endpoint: String,
    /// Bearer token for authenticating with this node.
    pub auth_token: Option<String>,
    /// What this node can do.
    pub capabilities: Vec<NodeCapability>,
    /// Current status.
    pub status: NodeStatus,
    /// Last heartbeat timestamp (unix ms).
    pub last_heartbeat_ms: u64,
    /// When this node registered (unix ms).
    pub registered_at_ms: u64,
}

/// What a node can do.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeCapability {
    /// Can run the agentic loop (has LLM credentials).
    AgenticLoop,
    /// Can compile WASM skills (has Rust + wasm32-wasip1).
    SkillBuild,
    /// Can run WASM skills.
    SkillExecute,
    /// Has GPU compute available.
    GpuCompute,
    /// Has internet access (not airgapped).
    Network,
    /// Custom capability (for user-defined specializations).
    Custom(String),
}

/// Node liveness status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// Online and accepting tasks.
    Online,
    /// Registered but not responding to heartbeats.
    Stale,
    /// Explicitly marked offline.
    Offline,
    /// Busy with a task.
    Busy,
}

/// Default stale threshold: 60 seconds in milliseconds.
const DEFAULT_STALE_THRESHOLD_MS: u64 = 60_000;

/// Registry of known Fawx nodes.
pub struct NodeRegistry {
    nodes: HashMap<String, NodeInfo>,
    /// How long before a node is considered stale (ms).
    stale_threshold_ms: u64,
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeRegistry {
    /// Create a new registry with the default stale threshold (60s).
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            stale_threshold_ms: DEFAULT_STALE_THRESHOLD_MS,
        }
    }

    /// Create with a custom stale threshold in milliseconds.
    pub fn with_stale_threshold(threshold_ms: u64) -> Self {
        Self {
            nodes: HashMap::new(),
            stale_threshold_ms: threshold_ms,
        }
    }

    /// Register or update a node.
    pub fn register(&mut self, node: NodeInfo) {
        self.nodes.insert(node.node_id.clone(), node);
    }

    /// Remove a node by id.
    pub fn remove(&mut self, node_id: &str) -> Option<NodeInfo> {
        self.nodes.remove(node_id)
    }

    /// Record a heartbeat from a node. Returns `false` if node is unknown.
    pub fn heartbeat(&mut self, node_id: &str, now_ms: u64) -> bool {
        let Some(node) = self.nodes.get_mut(node_id) else {
            return false;
        };
        node.last_heartbeat_ms = now_ms;
        if node.status == NodeStatus::Stale {
            node.status = NodeStatus::Online;
        }
        true
    }

    /// Get a node by id.
    pub fn get(&self, node_id: &str) -> Option<&NodeInfo> {
        self.nodes.get(node_id)
    }

    /// List all nodes.
    pub fn list(&self) -> Vec<&NodeInfo> {
        self.nodes.values().collect()
    }

    /// List nodes with a specific capability.
    pub fn with_capability(&self, cap: &NodeCapability) -> Vec<&NodeInfo> {
        self.nodes
            .values()
            .filter(|n| n.capabilities.contains(cap))
            .collect()
    }

    /// List online nodes only (excludes Stale and Offline).
    pub fn online(&self) -> Vec<&NodeInfo> {
        self.nodes
            .values()
            .filter(|n| n.status == NodeStatus::Online || n.status == NodeStatus::Busy)
            .collect()
    }

    /// Mark stale nodes (no heartbeat within threshold). Returns ids of newly stale nodes.
    pub fn mark_stale(&mut self, now_ms: u64) -> Vec<String> {
        let mut stale_ids = Vec::new();
        for node in self.nodes.values_mut() {
            if (node.status == NodeStatus::Online || node.status == NodeStatus::Busy)
                && now_ms.saturating_sub(node.last_heartbeat_ms) > self.stale_threshold_ms
            {
                node.status = NodeStatus::Stale;
                stale_ids.push(node.node_id.clone());
            }
        }
        stale_ids
    }

    /// Number of registered nodes.
    pub fn count(&self) -> usize {
        self.nodes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, capabilities: Vec<NodeCapability>) -> NodeInfo {
        NodeInfo {
            node_id: id.to_string(),
            name: format!("Node {id}"),
            endpoint: format!("https://{id}.example.com:8400"),
            auth_token: None,
            capabilities,
            status: NodeStatus::Online,
            last_heartbeat_ms: 1000,
            registered_at_ms: 1000,
        }
    }

    #[test]
    fn register_node() {
        let mut registry = NodeRegistry::new();
        let node = make_node("n1", vec![NodeCapability::AgenticLoop]);

        registry.register(node);

        assert_eq!(registry.count(), 1);
        let retrieved = registry.get("n1").expect("node should exist");
        assert_eq!(retrieved.name, "Node n1");
        assert_eq!(retrieved.capabilities, vec![NodeCapability::AgenticLoop]);
    }

    #[test]
    fn remove_node() {
        let mut registry = NodeRegistry::new();
        registry.register(make_node("n1", vec![]));
        assert_eq!(registry.count(), 1);

        let removed = registry.remove("n1");
        assert!(removed.is_some());
        assert_eq!(registry.count(), 0);
        assert!(registry.get("n1").is_none());
    }

    #[test]
    fn heartbeat_updates_timestamp() {
        let mut registry = NodeRegistry::new();
        registry.register(make_node("n1", vec![]));

        let result = registry.heartbeat("n1", 5000);
        assert!(result);

        let node = registry.get("n1").expect("node should exist");
        assert_eq!(node.last_heartbeat_ms, 5000);
    }

    #[test]
    fn heartbeat_unknown_node_returns_false() {
        let mut registry = NodeRegistry::new();
        assert!(!registry.heartbeat("nonexistent", 5000));
    }

    #[test]
    fn with_capability_filters() {
        let mut registry = NodeRegistry::new();
        registry.register(make_node(
            "builder",
            vec![NodeCapability::SkillBuild, NodeCapability::SkillExecute],
        ));
        registry.register(make_node("runner", vec![NodeCapability::SkillExecute]));
        registry.register(make_node("gpu", vec![NodeCapability::GpuCompute]));

        let builders = registry.with_capability(&NodeCapability::SkillBuild);
        assert_eq!(builders.len(), 1);
        assert_eq!(builders[0].node_id, "builder");

        let executors = registry.with_capability(&NodeCapability::SkillExecute);
        assert_eq!(executors.len(), 2);
    }

    #[test]
    fn mark_stale_detects_timeout() {
        let mut registry = NodeRegistry::with_stale_threshold(5000);
        registry.register(make_node("n1", vec![])); // last_heartbeat_ms = 1000

        let stale = registry.mark_stale(7000); // 7000 - 1000 = 6000 > 5000
        assert_eq!(stale, vec!["n1"]);

        let node = registry.get("n1").expect("node should exist");
        assert_eq!(node.status, NodeStatus::Stale);

        // Second call should return empty — already stale (idempotent)
        let stale_again = registry.mark_stale(8000);
        assert!(stale_again.is_empty());
    }

    #[test]
    fn mark_stale_preserves_recent() {
        let mut registry = NodeRegistry::with_stale_threshold(5000);
        registry.register(make_node("n1", vec![])); // last_heartbeat_ms = 1000

        let stale = registry.mark_stale(4000); // 4000 - 1000 = 3000 < 5000
        assert!(stale.is_empty());

        let node = registry.get("n1").expect("node should exist");
        assert_eq!(node.status, NodeStatus::Online);
    }

    #[test]
    fn online_excludes_stale_and_offline() {
        let mut registry = NodeRegistry::new();

        let mut online_node = make_node("online", vec![]);
        online_node.status = NodeStatus::Online;
        registry.register(online_node);

        let mut busy_node = make_node("busy", vec![]);
        busy_node.status = NodeStatus::Busy;
        registry.register(busy_node);

        let mut stale_node = make_node("stale", vec![]);
        stale_node.status = NodeStatus::Stale;
        registry.register(stale_node);

        let mut offline_node = make_node("offline", vec![]);
        offline_node.status = NodeStatus::Offline;
        registry.register(offline_node);

        let online = registry.online();
        assert_eq!(online.len(), 2);

        let online_ids: Vec<&str> = online.iter().map(|n| n.node_id.as_str()).collect();
        assert!(online_ids.contains(&"online"));
        assert!(online_ids.contains(&"busy"));
        assert!(!online_ids.contains(&"stale"));
        assert!(!online_ids.contains(&"offline"));
    }
}
