//! fx-orchestrator — distributed task coordination for Fawx.
//!
//! Routes tasks to nodes, manages retries/timeouts, and delivers
//! responses back through originating channels. Pure synchronous
//! state machine — no async, no networking.

use fx_core::channel::{Channel, ResponseContext};
use fx_core::types::InputSource;
use fx_fleet::{NodeRegistry, NodeStatus, RoutingDecision, TaskRequirements, TaskRouter};
use std::collections::HashMap;
use std::sync::Arc;

/// A task to be routed and executed.
#[derive(Debug, Clone)]
pub struct Task {
    /// Unique task identifier.
    pub task_id: String,
    /// The message/prompt to process.
    pub message: String,
    /// Which channel this task originated from.
    pub source: InputSource,
    /// Required node capabilities for this task.
    pub requirements: TaskRequirements,
    /// Maximum retries on failure (0 = no retry).
    pub max_retries: u32,
    /// Task timeout in milliseconds. 0 = no timeout.
    pub timeout_ms: u64,
    /// When this task was submitted (unix ms). Set by submit().
    pub submitted_at_ms: u64,
    /// How many retries have been attempted so far.
    pub retries_attempted: u32,
}

/// Result of task execution.
#[derive(Debug, Clone)]
pub enum TaskResult {
    /// Task completed with a response.
    Completed {
        task_id: String,
        response: String,
        node_id: String,
    },
    /// Task failed.
    Failed {
        task_id: String,
        error: String,
        node_id: Option<String>,
    },
    /// No node available to handle this task.
    NoNode { task_id: String, reason: String },
}

impl TaskResult {
    pub fn task_id(&self) -> &str {
        match self {
            Self::Completed { task_id, .. }
            | Self::Failed { task_id, .. }
            | Self::NoNode { task_id, .. } => task_id,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }
}

/// What happened when complete() was called.
#[derive(Debug, Clone)]
pub enum CompletionOutcome {
    /// Response delivered to channel.
    Delivered(String),
    /// Task failed but retried on a new node.
    Retried(RoutingDecision),
    /// All retries exhausted, task abandoned.
    Exhausted(String, String),
}

/// Errors from orchestrator operations.
#[derive(Debug, Clone)]
pub enum OrchestratorError {
    /// No node available for the task requirements.
    NoNodeAvailable(String),
    /// Channel not found for response delivery.
    ChannelNotFound(String),
    /// Task routing failed.
    RoutingFailed(String),
    /// Pending task capacity exceeded.
    CapacityExceeded(String),
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoNodeAvailable(msg) => write!(f, "no node available: {msg}"),
            Self::ChannelNotFound(msg) => write!(f, "channel not found: {msg}"),
            Self::RoutingFailed(msg) => write!(f, "routing failed: {msg}"),
            Self::CapacityExceeded(msg) => write!(f, "capacity exceeded: {msg}"),
        }
    }
}

impl std::error::Error for OrchestratorError {}

/// Summary of fleet health.
#[derive(Debug, Clone)]
pub struct FleetStatus {
    pub online: usize,
    pub stale: usize,
    pub offline: usize,
    pub busy: usize,
    pub total: usize,
}

/// Coordinates task routing, node management, and response delivery.
pub struct Orchestrator {
    registry: NodeRegistry,
    channels: HashMap<String, Arc<dyn Channel>>,
    pending: HashMap<String, Task>,
    is_coordinator: bool,
    max_pending_tasks: usize,
}

impl Orchestrator {
    /// Create a new orchestrator with default settings.
    pub fn new() -> Self {
        Self {
            registry: NodeRegistry::new(),
            channels: HashMap::new(),
            pending: HashMap::new(),
            is_coordinator: true,
            max_pending_tasks: 100,
        }
    }

    /// Create a new orchestrator with the given capacity limit.
    pub fn with_max_pending(max_pending_tasks: usize) -> Self {
        Self {
            max_pending_tasks,
            ..Self::new()
        }
    }

    /// Whether this orchestrator is the coordinator.
    pub fn is_coordinator(&self) -> bool {
        self.is_coordinator
    }

    /// Set the coordinator flag.
    pub fn set_coordinator(&mut self, is_coordinator: bool) {
        self.is_coordinator = is_coordinator;
    }

    /// Register a channel for response routing.
    pub fn register_channel(&mut self, channel: Arc<dyn Channel>) {
        self.channels.insert(channel.id().to_string(), channel);
    }

    /// Remove a channel by id.
    pub fn remove_channel(&mut self, channel_id: &str) {
        self.channels.remove(channel_id);
    }

    /// Get a reference to the node registry.
    pub fn registry(&self) -> &NodeRegistry {
        &self.registry
    }

    /// Get a mutable reference to the node registry.
    pub fn registry_mut(&mut self) -> &mut NodeRegistry {
        &mut self.registry
    }

    /// Submit a task for routing. Sets `submitted_at_ms` to `now_ms`.
    pub fn submit(
        &mut self,
        mut task: Task,
        now_ms: u64,
    ) -> Result<RoutingDecision, OrchestratorError> {
        if self.pending.len() >= self.max_pending_tasks {
            return Err(OrchestratorError::CapacityExceeded(format!(
                "pending task limit reached ({})",
                self.max_pending_tasks,
            )));
        }

        task.submitted_at_ms = now_ms;
        let decision = TaskRouter::select(&self.registry, &task.requirements);
        match &decision {
            RoutingDecision::Routed(_) => {
                self.pending.insert(task.task_id.clone(), task);
                Ok(decision)
            }
            RoutingDecision::NoNodeAvailable(reason) => {
                Err(OrchestratorError::NoNodeAvailable(reason.clone()))
            }
        }
    }

    /// Record a task result and route response to the originating channel.
    /// `now_ms` is used to reset the submission timestamp on retries.
    pub fn complete(
        &mut self,
        result: TaskResult,
        now_ms: u64,
    ) -> Result<CompletionOutcome, OrchestratorError> {
        let task_id = result.task_id().to_string();
        match result {
            TaskResult::Completed { response, .. } => self.deliver_response(&task_id, &response),
            TaskResult::Failed { error, .. } => self.handle_failure(&task_id, &error, now_ms),
            TaskResult::NoNode { reason, .. } => self.handle_failure(&task_id, &reason, now_ms),
        }
    }

    /// Get a pending task by id.
    pub fn pending(&self, task_id: &str) -> Option<&Task> {
        self.pending.get(task_id)
    }

    /// Number of pending tasks.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Run stale detection on the registry.
    pub fn check_stale(&mut self, now_ms: u64) -> Vec<String> {
        self.registry.mark_stale(now_ms)
    }

    /// Scan pending tasks for timeouts.
    pub fn check_timeouts(&mut self, now_ms: u64) -> Vec<(String, CompletionOutcome)> {
        let expired: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, task)| is_expired(task, now_ms))
            .map(|(id, _)| id.clone())
            .collect();

        let mut outcomes = Vec::new();
        for task_id in expired {
            let timeout_msg = format!(
                "task timed out after {}ms",
                self.pending.get(&task_id).map_or(0, |t| t.timeout_ms)
            );
            let outcome = self.handle_failure(&task_id, &timeout_msg, now_ms);
            match outcome {
                Ok(o) => outcomes.push((task_id, o)),
                Err(e) => outcomes.push((
                    task_id.clone(),
                    CompletionOutcome::Exhausted(task_id, format!("{e}")),
                )),
            }
        }
        outcomes
    }

    /// Get a summary of fleet status.
    pub fn fleet_status(&self) -> FleetStatus {
        let nodes = self.registry.all();
        let mut status = FleetStatus {
            online: 0,
            stale: 0,
            offline: 0,
            busy: 0,
            total: nodes.len(),
        };
        for node in &nodes {
            match node.status {
                NodeStatus::Online => status.online += 1,
                NodeStatus::Stale => status.stale += 1,
                NodeStatus::Offline => status.offline += 1,
                NodeStatus::Busy => status.busy += 1,
            }
        }
        status
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Private helpers ─────────────────────────────────────────────────

fn is_expired(task: &Task, now_ms: u64) -> bool {
    task.timeout_ms > 0 && now_ms.saturating_sub(task.submitted_at_ms) > task.timeout_ms
}

/// Extract channel id from an InputSource.
fn channel_id_for_source(source: &InputSource) -> String {
    match source {
        InputSource::Channel(id) => id.clone(),
        InputSource::Http => "http".to_string(),
        InputSource::Voice => "voice".to_string(),
        InputSource::Text => "text".to_string(),
        InputSource::Notification => "notification".to_string(),
        InputSource::Scheduled => "scheduled".to_string(),
    }
}

impl Orchestrator {
    fn deliver_response(
        &mut self,
        task_id: &str,
        response: &str,
    ) -> Result<CompletionOutcome, OrchestratorError> {
        let task = self
            .pending
            .remove(task_id)
            .ok_or_else(|| OrchestratorError::RoutingFailed(format!("unknown task: {task_id}")))?;

        let ch_id = channel_id_for_source(&task.source);
        let channel = self
            .channels
            .get(&ch_id)
            .ok_or_else(|| OrchestratorError::ChannelNotFound(ch_id.clone()))?;

        channel
            .send_response(response, &ResponseContext::default())
            .map_err(|e| OrchestratorError::RoutingFailed(format!("{e}")))?;

        Ok(CompletionOutcome::Delivered(ch_id))
    }

    fn handle_failure(
        &mut self,
        task_id: &str,
        error: &str,
        now_ms: u64,
    ) -> Result<CompletionOutcome, OrchestratorError> {
        let task = self
            .pending
            .remove(task_id)
            .ok_or_else(|| OrchestratorError::RoutingFailed(format!("unknown task: {task_id}")))?;

        if task.retries_attempted < task.max_retries {
            return self.retry_task(task, now_ms);
        }

        Ok(CompletionOutcome::Exhausted(
            task.task_id,
            error.to_string(),
        ))
    }

    fn retry_task(
        &mut self,
        mut task: Task,
        now_ms: u64,
    ) -> Result<CompletionOutcome, OrchestratorError> {
        task.retries_attempted += 1;
        task.submitted_at_ms = now_ms;
        let decision = TaskRouter::select(&self.registry, &task.requirements);
        match &decision {
            RoutingDecision::Routed(_) => {
                self.pending.insert(task.task_id.clone(), task);
                Ok(CompletionOutcome::Retried(decision))
            }
            RoutingDecision::NoNodeAvailable(reason) => {
                Err(OrchestratorError::NoNodeAvailable(reason.clone()))
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use fx_fleet::{NodeCapability, NodeInfo, NodeStatus};
    use std::sync::Mutex;

    /// Mock channel that records sent responses.
    struct MockChannel {
        id: String,
        responses: Mutex<Vec<String>>,
    }

    impl MockChannel {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                responses: Mutex::new(Vec::new()),
            }
        }

        fn sent(&self) -> Vec<String> {
            self.responses.lock().unwrap().clone()
        }
    }

    impl Channel for MockChannel {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            "Mock Channel"
        }

        fn input_source(&self) -> InputSource {
            InputSource::Channel(self.id.clone())
        }

        fn is_active(&self) -> bool {
            true
        }

        fn send_response(
            &self,
            message: &str,
            _context: &ResponseContext,
        ) -> Result<(), fx_core::channel::ChannelError> {
            self.responses.lock().unwrap().push(message.to_string());
            Ok(())
        }
    }

    fn make_node(id: &str, caps: Vec<NodeCapability>, status: NodeStatus) -> NodeInfo {
        NodeInfo {
            node_id: id.to_string(),
            name: format!("Node {id}"),
            endpoint: format!("https://{id}.example.com:8400"),
            auth_token: None,
            capabilities: caps,
            status,
            last_heartbeat_ms: 1000,
            registered_at_ms: 1000,
            address: None,
            ssh_user: None,
            ssh_key: None,
        }
    }

    fn make_task(id: &str, channel_id: &str) -> Task {
        Task {
            task_id: id.to_string(),
            message: "hello".to_string(),
            source: InputSource::Channel(channel_id.to_string()),
            requirements: TaskRequirements::new(vec![NodeCapability::AgenticLoop]),
            max_retries: 0,
            timeout_ms: 0,
            submitted_at_ms: 1000,
            retries_attempted: 0,
        }
    }

    #[test]
    fn submit_routes_to_capable_node() {
        let mut orch = Orchestrator::new();
        orch.registry_mut().register(make_node(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));

        let task = make_task("t1", "ch1");
        let decision = orch.submit(task, 1000).unwrap();

        assert!(decision.is_routed());
        let node = decision.node().unwrap();
        assert_eq!(node.node_id, "n1");
        assert_eq!(orch.pending_count(), 1);
    }

    #[test]
    fn submit_no_node_returns_error() {
        let mut orch = Orchestrator::new();
        let task = make_task("t1", "ch1");
        let err = orch.submit(task, 1000).unwrap_err();

        assert!(matches!(err, OrchestratorError::NoNodeAvailable(_)));
    }

    #[test]
    fn submit_capacity_exceeded() {
        let mut orch = Orchestrator::with_max_pending(1);
        orch.registry_mut().register(make_node(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));

        let t1 = make_task("t1", "ch1");
        orch.submit(t1, 1000).unwrap();

        let t2 = make_task("t2", "ch1");
        let err = orch.submit(t2, 2000).unwrap_err();
        assert!(matches!(err, OrchestratorError::CapacityExceeded(_)));
    }

    #[test]
    fn submit_sets_submitted_at_ms() {
        let mut orch = Orchestrator::new();
        orch.registry_mut().register(make_node(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));

        let task = make_task("t1", "ch1");
        orch.submit(task, 5000).unwrap();
        assert_eq!(orch.pending("t1").unwrap().submitted_at_ms, 5000);
    }

    #[test]
    fn complete_delivers_to_channel() {
        let mut orch = Orchestrator::new();
        orch.registry_mut().register(make_node(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));

        let channel = Arc::new(MockChannel::new("ch1"));
        orch.register_channel(channel.clone());

        let task = make_task("t1", "ch1");
        orch.submit(task, 1000).unwrap();

        let result = TaskResult::Completed {
            task_id: "t1".to_string(),
            response: "world".to_string(),
            node_id: "n1".to_string(),
        };
        let outcome = orch.complete(result, 2000).unwrap();

        assert!(matches!(outcome, CompletionOutcome::Delivered(ref id) if id == "ch1"));
        assert_eq!(channel.sent(), vec!["world"]);
        assert_eq!(orch.pending_count(), 0);
    }

    #[test]
    fn complete_unknown_task_returns_error() {
        let mut orch = Orchestrator::new();

        let result = TaskResult::Completed {
            task_id: "nonexistent".to_string(),
            response: "hello".to_string(),
            node_id: "n1".to_string(),
        };
        let err = orch.complete(result, 1000).unwrap_err();

        assert!(matches!(err, OrchestratorError::RoutingFailed(_)));
    }

    #[test]
    fn complete_retries_on_failure() {
        let mut orch = Orchestrator::new();
        orch.registry_mut().register(make_node(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));
        orch.registry_mut().register(make_node(
            "n2",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));

        let mut task = make_task("t1", "ch1");
        task.max_retries = 2;
        orch.submit(task, 1000).unwrap();

        let result = TaskResult::Failed {
            task_id: "t1".to_string(),
            error: "oops".to_string(),
            node_id: Some("n1".to_string()),
        };
        let outcome = orch.complete(result, 2000).unwrap();

        assert!(matches!(outcome, CompletionOutcome::Retried(_)));
        let pending = orch.pending("t1").unwrap();
        assert_eq!(pending.retries_attempted, 1);
        assert_eq!(pending.submitted_at_ms, 2000);
    }

    #[test]
    fn complete_exhausted_after_max_retries() {
        let mut orch = Orchestrator::new();
        orch.registry_mut().register(make_node(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));

        let mut task = make_task("t1", "ch1");
        task.max_retries = 1;
        orch.submit(task, 1000).unwrap();

        // First failure — retries
        let result1 = TaskResult::Failed {
            task_id: "t1".to_string(),
            error: "fail1".to_string(),
            node_id: Some("n1".to_string()),
        };
        let outcome1 = orch.complete(result1, 2000).unwrap();
        assert!(matches!(outcome1, CompletionOutcome::Retried(_)));

        // Second failure — exhausted
        let result2 = TaskResult::Failed {
            task_id: "t1".to_string(),
            error: "fail2".to_string(),
            node_id: Some("n1".to_string()),
        };
        let outcome2 = orch.complete(result2, 3000).unwrap();
        assert!(matches!(outcome2, CompletionOutcome::Exhausted(ref id, _) if id == "t1"));
        assert_eq!(orch.pending_count(), 0);
    }

    #[test]
    fn check_timeouts_marks_expired() {
        let mut orch = Orchestrator::new();
        orch.registry_mut().register(make_node(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));

        let mut task = make_task("t1", "ch1");
        task.timeout_ms = 1000;
        orch.submit(task, 1000).unwrap();

        // Advance 2000ms past submission
        let outcomes = orch.check_timeouts(3001);

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].0, "t1");
        assert!(matches!(outcomes[0].1, CompletionOutcome::Exhausted(_, _)));
        assert_eq!(orch.pending_count(), 0);
    }

    #[test]
    fn check_timeouts_retries_if_allowed() {
        let mut orch = Orchestrator::new();
        orch.registry_mut().register(make_node(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));

        let mut task = make_task("t1", "ch1");
        task.timeout_ms = 1000;
        task.max_retries = 1;
        orch.submit(task, 1000).unwrap();

        let outcomes = orch.check_timeouts(3001);

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].0, "t1");
        assert!(matches!(outcomes[0].1, CompletionOutcome::Retried(_)));
        // Task should still be pending (retried) with reset timestamp
        assert_eq!(orch.pending_count(), 1);
        assert_eq!(orch.pending("t1").unwrap().submitted_at_ms, 3001);
    }

    #[test]
    fn check_stale_delegates_to_registry() {
        let mut orch = Orchestrator::new();
        orch.registry_mut().register(make_node(
            "n1",
            vec![NodeCapability::AgenticLoop],
            NodeStatus::Online,
        ));

        // Default stale threshold is 60s, so advance past that
        let stale = orch.check_stale(62_000);

        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0], "n1");
    }

    #[test]
    fn fleet_status_counts_correctly() {
        let mut orch = Orchestrator::new();
        orch.registry_mut()
            .register(make_node("online1", vec![], NodeStatus::Online));
        orch.registry_mut()
            .register(make_node("online2", vec![], NodeStatus::Online));
        orch.registry_mut()
            .register(make_node("stale1", vec![], NodeStatus::Stale));
        orch.registry_mut()
            .register(make_node("offline1", vec![], NodeStatus::Offline));
        orch.registry_mut()
            .register(make_node("busy1", vec![], NodeStatus::Busy));

        let status = orch.fleet_status();

        assert_eq!(status.online, 2);
        assert_eq!(status.stale, 1);
        assert_eq!(status.offline, 1);
        assert_eq!(status.busy, 1);
        assert_eq!(status.total, 5);
    }
}
