pub(crate) mod fs_utils;
pub mod http;
pub mod identity;
pub mod manager;
pub mod ssh;
pub mod token;
pub mod transport;

pub use http::{
    FleetHeartbeat, FleetHttpClient, FleetRegistrationRequest, FleetRegistrationResponse,
    FleetTaskRequest, FleetTaskResult, FleetTaskStatus, FleetTaskType, FleetWorkerStatus,
    WorkerState,
};
pub use identity::FleetIdentity;
pub use manager::FleetManager;
pub use ssh::SshTransport;
pub use token::{FleetError, FleetKey, FleetToken};
pub use transport::{CommandResult, NodeTransport, TransportError};

use fx_config::NodeConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// A registered Fawx node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Unique node identifier.
    pub node_id: String,
    /// Human-readable name.
    pub name: String,
    /// HTTP API endpoint (e.g., "https://203.0.113.5:8400").
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
    /// SSH address (IP or hostname) for SSH transport.
    pub address: Option<String>,
    /// SSH username for SSH transport.
    pub ssh_user: Option<String>,
    /// SSH key path override (uses transport default if `None`).
    pub ssh_key: Option<String>,
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

impl From<&str> for NodeCapability {
    fn from(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "agentic_loop" => Self::AgenticLoop,
            "skill_build" => Self::SkillBuild,
            "skill_execute" => Self::SkillExecute,
            "gpu_compute" => Self::GpuCompute,
            "network" => Self::Network,
            _ => Self::Custom(value.to_string()),
        }
    }
}

impl From<&NodeConfig> for NodeInfo {
    fn from(config: &NodeConfig) -> Self {
        Self {
            node_id: config.id.clone(),
            name: config.name.clone(),
            endpoint: config.endpoint.clone().unwrap_or_default(),
            auth_token: config.auth_token.clone(),
            capabilities: config
                .capabilities
                .iter()
                .map(|capability| NodeCapability::from(capability.as_str()))
                .collect(),
            status: NodeStatus::Online,
            last_heartbeat_ms: 0,
            registered_at_ms: current_time_ms(),
            address: config.address.clone(),
            ssh_user: config.user.clone(),
            ssh_key: config.ssh_key.clone(),
        }
    }
}

/// Returns the current Unix timestamp in milliseconds.
pub fn current_time_ms() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

/// Default stale threshold: 60 seconds in milliseconds.
pub const DEFAULT_STALE_THRESHOLD_MS: u64 = 60_000;

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

    pub(crate) fn get_mut(&mut self, node_id: &str) -> Option<&mut NodeInfo> {
        self.nodes.get_mut(node_id)
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

    /// Get all registered nodes (regardless of status).
    pub fn all(&self) -> Vec<&NodeInfo> {
        self.nodes.values().collect()
    }

    /// Number of registered nodes.
    pub fn count(&self) -> usize {
        self.nodes.len()
    }
}

// ── Task Router ─────────────────────────────────────────────────────

/// What a task needs from a node.
#[derive(Debug, Clone)]
pub struct TaskRequirements {
    /// Required capabilities — node must have ALL of these.
    pub capabilities: Vec<NodeCapability>,
    /// Prefer nodes that aren't busy (soft preference, not hard filter).
    pub prefer_idle: bool,
    /// Optional: prefer a specific node by ID (sticky routing).
    pub preferred_node: Option<String>,
}

impl TaskRequirements {
    pub fn new(capabilities: Vec<NodeCapability>) -> Self {
        Self {
            capabilities,
            prefer_idle: false,
            preferred_node: None,
        }
    }

    pub fn prefer_idle(mut self, prefer: bool) -> Self {
        self.prefer_idle = prefer;
        self
    }

    pub fn preferred_node(mut self, node_id: String) -> Self {
        self.preferred_node = Some(node_id);
        self
    }
}

/// Result of task routing.
#[derive(Debug, Clone)]
pub enum RoutingDecision {
    /// Route to this node.
    Routed(NodeInfo),
    /// No node available — reason included.
    NoNodeAvailable(String),
}

impl RoutingDecision {
    /// Returns the routed node, or None.
    pub fn node(&self) -> Option<&NodeInfo> {
        match self {
            Self::Routed(n) => Some(n),
            Self::NoNodeAvailable(_) => None,
        }
    }

    /// Returns true if a node was selected.
    pub fn is_routed(&self) -> bool {
        matches!(self, Self::Routed(_))
    }
}

/// Selects the best node for a task from the registry.
pub struct TaskRouter;

impl TaskRouter {
    /// Select the best node for the given requirements.
    ///
    /// Selection logic:
    /// 1. Filter to Online nodes with ALL required capabilities
    /// 2. Fall back to Busy nodes if no Online nodes qualify
    /// 3. Never selects Offline or Stale nodes
    /// 4. Preferred node shortcut if it qualifies
    /// 5. Sort: Online before Busy (if prefer_idle), then most recent heartbeat
    pub fn select(registry: &NodeRegistry, requirements: &TaskRequirements) -> RoutingDecision {
        let online_capable = Self::filter_capable(
            &registry.all(),
            &requirements.capabilities,
            &NodeStatus::Online,
        );

        let candidates = if online_capable.is_empty() {
            Self::filter_capable(
                &registry.all(),
                &requirements.capabilities,
                &NodeStatus::Busy,
            )
        } else {
            online_capable
        };

        if candidates.is_empty() {
            return Self::no_node_available(&requirements.capabilities);
        }

        if let Some(node) = Self::try_preferred(&candidates, &requirements.preferred_node) {
            return RoutingDecision::Routed(node.clone());
        }

        let best = Self::pick_best(&candidates, requirements.prefer_idle);
        RoutingDecision::Routed(best.clone())
    }

    fn filter_capable<'a>(
        nodes: &[&'a NodeInfo],
        required: &[NodeCapability],
        status: &NodeStatus,
    ) -> Vec<&'a NodeInfo> {
        nodes
            .iter()
            .filter(|n| n.status == *status)
            .filter(|n| required.iter().all(|c| n.capabilities.contains(c)))
            .copied()
            .collect()
    }

    fn try_preferred<'a>(
        candidates: &[&'a NodeInfo],
        preferred: &Option<String>,
    ) -> Option<&'a NodeInfo> {
        let pref_id = preferred.as_ref()?;
        candidates.iter().find(|n| n.node_id == *pref_id).copied()
    }

    fn pick_best<'a>(candidates: &[&'a NodeInfo], prefer_idle: bool) -> &'a NodeInfo {
        let mut sorted: Vec<&NodeInfo> = candidates.to_vec();
        sorted.sort_by(|a, b| {
            if prefer_idle {
                let a_online = a.status == NodeStatus::Online;
                let b_online = b.status == NodeStatus::Online;
                if a_online != b_online {
                    return b_online.cmp(&a_online);
                }
            }
            b.last_heartbeat_ms.cmp(&a.last_heartbeat_ms)
        });
        sorted[0]
    }

    fn no_node_available(capabilities: &[NodeCapability]) -> RoutingDecision {
        RoutingDecision::NoNodeAvailable(format!(
            "no nodes with required capabilities: {capabilities:?}"
        ))
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
            address: None,
            ssh_user: None,
            ssh_key: None,
        }
    }

    #[test]
    fn capability_strings_map_known_and_custom_values() {
        assert_eq!(
            NodeCapability::from("agentic_loop"),
            NodeCapability::AgenticLoop
        );
        assert_eq!(
            NodeCapability::from("skill_build"),
            NodeCapability::SkillBuild
        );
        assert_eq!(
            NodeCapability::from("skill_execute"),
            NodeCapability::SkillExecute
        );
        assert_eq!(
            NodeCapability::from("gpu_compute"),
            NodeCapability::GpuCompute
        );
        assert_eq!(NodeCapability::from("network"), NodeCapability::Network);
        assert_eq!(
            NodeCapability::from("Agentic_Loop"),
            NodeCapability::AgenticLoop
        );
        assert_eq!(
            NodeCapability::from("SKILL_BUILD"),
            NodeCapability::SkillBuild
        );
        assert_eq!(
            NodeCapability::from("GPU_COMPUTE"),
            NodeCapability::GpuCompute
        );
        assert_eq!(
            NodeCapability::from("test"),
            NodeCapability::Custom("test".to_string())
        );
    }

    #[test]
    fn node_info_from_config_maps_fleet_fields() {
        let config = NodeConfig {
            id: "mac-mini".to_string(),
            name: "Worker Node A".to_string(),
            endpoint: Some("https://10.0.0.5:8400".to_string()),
            auth_token: Some("token".to_string()),
            capabilities: vec!["agentic_loop".to_string(), "test".to_string()],
            address: Some("10.0.0.5".to_string()),
            user: Some("builder".to_string()),
            ssh_key: Some("~/.ssh/id_ed25519".to_string()),
        };

        let node = NodeInfo::from(&config);

        assert_eq!(node.node_id, "mac-mini");
        assert_eq!(node.name, "Worker Node A");
        assert_eq!(node.endpoint, "https://10.0.0.5:8400");
        assert_eq!(node.auth_token.as_deref(), Some("token"));
        assert_eq!(
            node.capabilities,
            vec![
                NodeCapability::AgenticLoop,
                NodeCapability::Custom("test".to_string()),
            ]
        );
        assert_eq!(node.status, NodeStatus::Online);
        assert_eq!(node.last_heartbeat_ms, 0);
        assert!(node.registered_at_ms > 0);
        assert_eq!(node.address.as_deref(), Some("10.0.0.5"));
        assert_eq!(node.ssh_user.as_deref(), Some("builder"));
        assert_eq!(node.ssh_key.as_deref(), Some("~/.ssh/id_ed25519"));
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

    // ── Task Router Tests ────────────────────────────────────────

    fn make_node_with_status(
        id: &str,
        caps: Vec<NodeCapability>,
        status: NodeStatus,
        heartbeat: u64,
    ) -> NodeInfo {
        NodeInfo {
            node_id: id.to_string(),
            name: format!("Node {id}"),
            endpoint: format!("https://{id}.example.com:8400"),
            auth_token: None,
            capabilities: caps,
            status,
            last_heartbeat_ms: heartbeat,
            registered_at_ms: 1000,
            address: None,
            ssh_user: None,
            ssh_key: None,
        }
    }

    #[test]
    fn route_to_capable_node() {
        let mut reg = NodeRegistry::new();
        reg.register(make_node_with_status(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
            5000,
        ));

        let reqs = TaskRequirements::new(vec![NodeCapability::AgenticLoop]);
        let decision = TaskRouter::select(&reg, &reqs);

        let node = decision.node().expect("should route");
        assert_eq!(node.node_id, "n1");
        assert!(decision.is_routed());
    }

    #[test]
    fn route_no_capable_node() {
        let mut reg = NodeRegistry::new();
        reg.register(make_node_with_status(
            "n1",
            vec![NodeCapability::Network],
            NodeStatus::Online,
            5000,
        ));

        let reqs = TaskRequirements::new(vec![NodeCapability::GpuCompute]);
        let decision = TaskRouter::select(&reg, &reqs);

        assert!(!decision.is_routed());
        assert!(decision.node().is_none());
    }

    #[test]
    fn route_prefers_online_over_busy() {
        let mut reg = NodeRegistry::new();
        reg.register(make_node_with_status(
            "busy1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Busy,
            9000, // more recent heartbeat
        ));
        reg.register(make_node_with_status(
            "online1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
            5000,
        ));

        let reqs = TaskRequirements::new(vec![NodeCapability::AgenticLoop]).prefer_idle(true);
        let decision = TaskRouter::select(&reg, &reqs);

        // Online node is selected even though busy has more recent heartbeat
        let node = decision.node().expect("should route");
        assert_eq!(node.node_id, "online1");
    }

    #[test]
    fn route_falls_back_to_busy() {
        let mut reg = NodeRegistry::new();
        reg.register(make_node_with_status(
            "busy1",
            vec![NodeCapability::SkillExecute],
            NodeStatus::Busy,
            5000,
        ));
        // No Online nodes with matching capability

        let reqs = TaskRequirements::new(vec![NodeCapability::SkillExecute]);
        let decision = TaskRouter::select(&reg, &reqs);

        let node = decision.node().expect("should fall back to busy");
        assert_eq!(node.node_id, "busy1");
    }

    #[test]
    fn route_excludes_offline_and_stale() {
        let mut reg = NodeRegistry::new();
        reg.register(make_node_with_status(
            "offline1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Offline,
            9000,
        ));
        reg.register(make_node_with_status(
            "stale1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Stale,
            8000,
        ));

        let reqs = TaskRequirements::new(vec![NodeCapability::AgenticLoop]);
        let decision = TaskRouter::select(&reg, &reqs);

        assert!(!decision.is_routed());
    }

    #[test]
    fn route_preferred_node_wins() {
        let mut reg = NodeRegistry::new();
        reg.register(make_node_with_status(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
            9000, // more recent
        ));
        reg.register(make_node_with_status(
            "n2",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
            5000, // less recent but preferred
        ));

        let reqs = TaskRequirements::new(vec![NodeCapability::AgenticLoop])
            .preferred_node("n2".to_string());
        let decision = TaskRouter::select(&reg, &reqs);

        let node = decision.node().expect("should route to preferred");
        assert_eq!(node.node_id, "n2");
    }

    #[test]
    fn route_preferred_node_skipped_if_unqualified() {
        let mut reg = NodeRegistry::new();
        reg.register(make_node_with_status(
            "preferred",
            vec![NodeCapability::Network], // lacks required capability
            NodeStatus::Online,
            9000,
        ));
        reg.register(make_node_with_status(
            "capable",
            vec![NodeCapability::GpuCompute],
            NodeStatus::Online,
            5000,
        ));

        let reqs = TaskRequirements::new(vec![NodeCapability::GpuCompute])
            .preferred_node("preferred".to_string());
        let decision = TaskRouter::select(&reg, &reqs);

        let node = decision.node().expect("should route to capable node");
        assert_eq!(node.node_id, "capable");
    }

    #[test]
    fn route_multiple_capabilities_required() {
        let mut reg = NodeRegistry::new();
        reg.register(make_node_with_status(
            "partial",
            vec![NodeCapability::SkillBuild], // only one cap
            NodeStatus::Online,
            9000,
        ));
        reg.register(make_node_with_status(
            "full",
            vec![NodeCapability::SkillBuild, NodeCapability::SkillExecute],
            NodeStatus::Online,
            5000,
        ));

        let reqs = TaskRequirements::new(vec![
            NodeCapability::SkillBuild,
            NodeCapability::SkillExecute,
        ]);
        let decision = TaskRouter::select(&reg, &reqs);

        let node = decision.node().expect("should route to node with all caps");
        assert_eq!(node.node_id, "full");
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
