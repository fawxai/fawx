# Wave 7 Item #5: Task Router (fx-fleet)

## Overview

The task router lives inside fx-fleet (not a new crate). It selects the best node for a given task based on required capabilities, node status, and load.

Pure logic — no networking, no async. Given a task's requirements and the current registry state, it returns the best node (or an error explaining why none qualify).

## 1. TaskRequirements

```rust
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
```

### Builder

```rust
impl TaskRequirements {
    pub fn new(capabilities: Vec<NodeCapability>) -> Self;
    pub fn prefer_idle(mut self, prefer: bool) -> Self;
    pub fn preferred_node(mut self, node_id: String) -> Self;
}
```

## 2. RoutingDecision

```rust
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
    pub fn node(&self) -> Option<&NodeInfo>;
    
    /// Returns true if a node was selected.
    pub fn is_routed(&self) -> bool;
}
```

## 3. TaskRouter

```rust
/// Selects the best node for a task from the registry.
pub struct TaskRouter;
```

This is a stateless selector — all state lives in `NodeRegistry`.

```rust
impl TaskRouter {
    /// Select the best node for the given requirements.
    ///
    /// Selection logic:
    /// 1. Filter to Online nodes only (exclude Stale, Offline, Busy unless no Online nodes)
    /// 2. Filter to nodes with ALL required capabilities
    /// 3. If preferred_node is set and qualifies, return it
    /// 4. If prefer_idle, sort Online before Busy
    /// 5. Among remaining candidates, pick the one with most recent heartbeat
    ///    (most recently confirmed alive)
    pub fn select(
        registry: &NodeRegistry,
        requirements: &TaskRequirements,
    ) -> RoutingDecision;
}
```

### Selection Algorithm (detail)

```
fn select(registry, requirements) -> RoutingDecision:
    candidates = registry.online()  // Online nodes
    
    // Filter by capabilities
    candidates = candidates.filter(|n| 
        requirements.capabilities.iter().all(|c| n.capabilities.contains(c))
    )
    
    if candidates.is_empty():
        // Try Busy nodes as fallback
        candidates = registry.all().filter(|n| 
            n.status == Busy 
            && requirements.capabilities.iter().all(|c| n.capabilities.contains(c))
        )
    
    if candidates.is_empty():
        return NoNodeAvailable("no nodes with required capabilities: [...]")
    
    // Preferred node shortcut
    if let Some(preferred) = requirements.preferred_node:
        if let Some(node) = candidates.find(|n| n.node_id == preferred):
            return Routed(node)
    
    // Sort: Online before Busy (if prefer_idle), then by most recent heartbeat
    candidates.sort(...)
    
    return Routed(candidates.first())
```

## 4. Add to NodeRegistry

Add a convenience method:

```rust
impl NodeRegistry {
    /// Get all registered nodes (regardless of status).
    pub fn all(&self) -> Vec<NodeInfo>;
}
```

This may already exist — check first. If `online()` exists, `all()` should too.

## 5. Tests (8 required)

1. `route_to_capable_node` — single node with matching capability, gets routed
2. `route_no_capable_node` — no node has required capability, NoNodeAvailable
3. `route_prefers_online_over_busy` — Online node chosen over Busy when prefer_idle=true
4. `route_falls_back_to_busy` — no Online nodes, Busy node with capability selected
5. `route_excludes_offline_and_stale` — Offline/Stale nodes never selected
6. `route_preferred_node_wins` — preferred_node matches a candidate, selected even if not most recent heartbeat
7. `route_preferred_node_skipped_if_unqualified` — preferred_node exists but lacks capability, another node selected
8. `route_multiple_capabilities_required` — node must have ALL required capabilities, not just one

## 6. File Changes

All changes in `engine/crates/fx-fleet/src/lib.rs` — add TaskRequirements, RoutingDecision, TaskRouter, and the `all()` method to NodeRegistry. No new files.

## 7. What This Does NOT Do

- No async, no networking — pure selection logic
- No task queuing or load balancing — that's the orchestrator's job (item #6)
- No task state tracking — router is stateless, called per-task
- No retry logic — caller decides what to do with NoNodeAvailable
