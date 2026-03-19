# Track H-1: Fleet Dashboard HTTP Endpoints

**Status:** SPEC
**Priority:** High — unblocks Swift fleet dashboard screen (Phase 5.5)
**Endpoints:** GET `/v1/fleet/overview`, GET/DELETE `/v1/fleet/nodes`, GET `/v1/fleet/nodes/{id}`, POST `/v1/fleet/nodes/{id}/tasks`

---

## Overview

Add five endpoints to fx-api that expose fleet state for the Swift app's fleet dashboard. The engine already has a mature fleet layer: `FleetManager` handles node registration/removal/persistence, `NodeRegistry` tracks liveness and capabilities, and `TaskRouter` selects nodes for dispatch. The existing `fleet_router` in fx-api handles worker-facing endpoints (`/fleet/register`, `/fleet/heartbeat`, `/fleet/result`). This track adds the **client-facing** `/v1/fleet/*` endpoints that the Swift app consumes.

The new endpoints are authenticated via the standard `auth_middleware` (bearer token), unlike the existing worker-facing fleet endpoints which use per-node bearer tokens.

---

## Endpoints

### GET /v1/fleet/overview

Returns aggregate fleet metrics for the dashboard summary card.

Response 200:
```json
{
  "total_nodes": 4,
  "healthy_nodes": 3,
  "degraded_nodes": 1,
  "offline_nodes": 0,
  "active_tasks": 7,
  "queued_tasks": 2,
  "updated_at": 1741977600
}
```

When no fleet is initialized (no `fleet.key` exists):
Response 503:
```json
{
  "error": "fleet not initialized"
}
```

**Implementation notes:**
- `healthy_nodes` = nodes with `NodeStatus::Online`
- `degraded_nodes` = nodes with `NodeStatus::Stale` or `NodeStatus::Busy`
- `offline_nodes` = nodes with `NodeStatus::Offline`
- `active_tasks` and `queued_tasks`: initially return 0 since task tracking is not yet implemented in `FleetManager`. Phase 2 will add task state tracking. The fields are included now to stabilize the API contract.
- `updated_at` = current unix timestamp (seconds)
- Must call `NodeRegistry::mark_stale()` before computing counts to ensure freshness.

### GET /v1/fleet/nodes

Returns all registered nodes with health and capability info.

Response 200:
```json
{
  "nodes": [
    {
      "id": "mac-mini-1a2b3c4d",
      "name": "Joe's Mac Mini",
      "status": "healthy",
      "last_seen_at": 1741977590,
      "active_tasks": 2,
      "capabilities": ["agentic_loop", "skill_build", "network"]
    },
    {
      "id": "vps-primary-5e6f7g8h",
      "name": "Primary VPS",
      "status": "degraded",
      "last_seen_at": 1741977500,
      "active_tasks": 0,
      "capabilities": ["agentic_loop", "network"]
    }
  ],
  "total": 2
}
```

When fleet not initialized:
Response 503:
```json
{
  "error": "fleet not initialized"
}
```

**Implementation notes:**
- `status` mapping from `NodeStatus`:
  - `Online` → `"healthy"`
  - `Busy` → `"healthy"` (busy is still healthy, just working)
  - `Stale` → `"degraded"`
  - `Offline` → `"offline"`
- `last_seen_at` = `NodeInfo::last_heartbeat_ms / 1000` (convert ms to seconds for API consistency)
- `capabilities` = serialized `NodeCapability` variants as snake_case strings. `Custom(s)` serializes as the custom string value.
- `active_tasks` = 0 for now (task tracking not yet implemented). Field included for API stability.
- Nodes sorted by name alphabetically for stable ordering.

### GET /v1/fleet/nodes/{id}

Returns detailed information for a single node.

Response 200:
```json
{
  "id": "mac-mini-1a2b3c4d",
  "name": "Joe's Mac Mini",
  "status": "healthy",
  "last_seen_at": 1741977590,
  "registered_at": 1741900000,
  "endpoint": "https://100.75.191.19:8400",
  "active_tasks": 2,
  "queued_tasks": 1,
  "capabilities": ["agentic_loop", "skill_build", "network"],
  "recent_tasks": []
}
```

Response 404:
```json
{
  "error": "node not found"
}
```

Response 503:
```json
{
  "error": "fleet not initialized"
}
```

**Implementation notes:**
- `registered_at` = `NodeInfo::registered_at_ms / 1000`
- `endpoint` = `NodeInfo::endpoint` (the node's HTTP API URL)
- `recent_tasks` = empty array for now (task history not yet tracked). Included for API contract stability.
- `active_tasks` and `queued_tasks` = 0 for now.

### DELETE /v1/fleet/nodes/{id}

Remove a node from the fleet. Revokes its tokens and deregisters it.

Response 200:
```json
{
  "id": "mac-mini-1a2b3c4d",
  "removed": true
}
```

Response 404:
```json
{
  "error": "node not found"
}
```

Response 503:
```json
{
  "error": "fleet not initialized"
}
```

**Implementation notes:**
- `FleetManager::remove_node()` works by name, but the API takes node ID. Need to look up the node by ID first to get its name, then call `remove_node(name)`.
- **Decision needed:** `FleetManager::remove_node()` takes a name, not an ID. Either:
  - (a) Add `FleetManager::remove_node_by_id(&mut self, id: &str)` method, or
  - (b) Look up the node by ID via the registry, extract the name, call `remove_node(name)`.
  - Preferred: (a) — cleaner API, avoids TOCTOU between lookup and removal.

### POST /v1/fleet/nodes/{id}/tasks

Dispatch a task to a specific node.

Request:
```json
{
  "task": "Run proof-of-fitness tournament on experiment exp_42",
  "priority": "normal"
}
```

Response 200:
```json
{
  "accepted": true,
  "task_id": "task_1741977600_a1b2",
  "node_id": "mac-mini-1a2b3c4d",
  "status": "queued"
}
```

Response 404:
```json
{
  "error": "node not found"
}
```

Response 422 (node not available):
```json
{
  "error": "node is offline and cannot accept tasks"
}
```

Response 503:
```json
{
  "error": "fleet not initialized"
}
```

**Implementation notes:**
- `priority` values: `"low"` | `"normal"` | `"high"`. Default: `"normal"`.
- `task_id` is generated server-side: `task_{timestamp_ms}_{random_4hex}`.
- For Phase 5.5, this endpoint creates a `FleetTaskRequest` and dispatches it via `FleetHttpClient::send_task()` to the target node.
- The node must be `Online` or `Busy` to accept tasks. `Stale` and `Offline` nodes return 422.
- **Open question:** Should this endpoint accept a structured task type (`FleetTaskType`) or a freeform string `task` field? The Appendix C schema uses a freeform string, but the existing `FleetTaskRequest` uses typed `FleetTaskType`. Recommend starting with the freeform string per the spec and mapping to `FleetTaskType::GenerateAndEvaluate` as default, with a future `type` field for explicit task typing.

---

## Response Types

New serde types for the API responses. All live in the handler module.

```rust
#[derive(Serialize)]
pub struct FleetOverviewResponse {
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub degraded_nodes: usize,
    pub offline_nodes: usize,
    pub active_tasks: usize,
    pub queued_tasks: usize,
    pub updated_at: u64,
}

#[derive(Serialize)]
pub struct FleetNodeSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub last_seen_at: u64,
    pub active_tasks: usize,
    pub capabilities: Vec<String>,
}

#[derive(Serialize)]
pub struct FleetNodeListResponse {
    pub nodes: Vec<FleetNodeSummary>,
    pub total: usize,
}

#[derive(Serialize)]
pub struct FleetNodeDetailResponse {
    pub id: String,
    pub name: String,
    pub status: String,
    pub last_seen_at: u64,
    pub registered_at: u64,
    pub endpoint: String,
    pub active_tasks: usize,
    pub queued_tasks: usize,
    pub capabilities: Vec<String>,
    pub recent_tasks: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct FleetNodeRemovedResponse {
    pub id: String,
    pub removed: bool,
}

#[derive(Deserialize)]
pub struct DispatchTaskRequest {
    pub task: String,
    #[serde(default = "default_priority")]
    pub priority: String,
}

#[derive(Serialize)]
pub struct DispatchTaskResponse {
    pub accepted: bool,
    pub task_id: String,
    pub node_id: String,
    pub status: String,
}
```

---

## State Management

The fleet dashboard endpoints need access to `FleetManager`. The existing pattern passes `Arc<Mutex<FleetManager>>` through `fleet_router()`. The new v1 endpoints need the same access.

**Approach:** Add `fleet_manager: Option<Arc<Mutex<FleetManager>>>` to `HttpState`. This is already available in `build_router()` — just store it in the state struct instead of only passing it to `fleet_router()`.

This avoids creating a separate state struct for fleet v1 endpoints and keeps them consistent with all other v1 endpoints that use `HttpState`.

---

## Files to Create/Modify

1. **NEW: `engine/crates/fx-api/src/handlers/fleet_dashboard.rs`** — all five handler functions + response types
2. **MODIFY: `engine/crates/fx-api/src/handlers/mod.rs`** — add `pub mod fleet_dashboard;`
3. **MODIFY: `engine/crates/fx-api/src/router.rs`** — add routes to `v1_router`:
   ```rust
   .route("/fleet/overview", get(handlers::fleet_dashboard::handle_fleet_overview))
   .route("/fleet/nodes", get(handlers::fleet_dashboard::handle_list_nodes))
   .route("/fleet/nodes/{id}", get(handlers::fleet_dashboard::handle_get_node)
       .delete(handlers::fleet_dashboard::handle_remove_node))
   .route("/fleet/nodes/{id}/tasks", post(handlers::fleet_dashboard::handle_dispatch_task))
   ```
4. **MODIFY: `engine/crates/fx-api/src/state.rs`** — add `pub fleet_manager: Option<Arc<Mutex<FleetManager>>>` to `HttpState`
5. **MODIFY: `engine/crates/fx-api/src/lib.rs`** (or wherever `HttpState` is constructed) — pass `fleet_manager` into state
6. **MODIFY: `engine/crates/fx-fleet/src/manager.rs`** — add `remove_node_by_id(&mut self, id: &str)` method

---

## Helper Functions

A `node_status_label()` function to map `NodeStatus` → API string:

```rust
fn node_status_label(status: &NodeStatus) -> &'static str {
    match status {
        NodeStatus::Online | NodeStatus::Busy => "healthy",
        NodeStatus::Stale => "degraded",
        NodeStatus::Offline => "offline",
    }
}
```

A `capability_label()` function to serialize `NodeCapability`:

```rust
fn capability_label(cap: &NodeCapability) -> String {
    match cap {
        NodeCapability::AgenticLoop => "agentic_loop".to_string(),
        NodeCapability::SkillBuild => "skill_build".to_string(),
        NodeCapability::SkillExecute => "skill_execute".to_string(),
        NodeCapability::GpuCompute => "gpu_compute".to_string(),
        NodeCapability::Network => "network".to_string(),
        NodeCapability::Custom(s) => s.clone(),
    }
}
```

---

## Tests Required

### Handler tests (in `fleet_dashboard.rs`)

1. `overview_returns_aggregate_counts` — GET /v1/fleet/overview with mixed node statuses returns correct counts
2. `overview_returns_503_when_fleet_not_initialized` — GET /v1/fleet/overview when no fleet returns 503
3. `list_nodes_returns_all_registered_nodes` — GET /v1/fleet/nodes returns node summaries
4. `list_nodes_returns_empty_when_no_nodes` — GET /v1/fleet/nodes with empty registry returns empty list
5. `list_nodes_returns_503_when_fleet_not_initialized` — GET /v1/fleet/nodes when no fleet returns 503
6. `list_nodes_sorts_by_name` — nodes returned in alphabetical name order
7. `get_node_returns_detail` — GET /v1/fleet/nodes/{id} returns full node detail
8. `get_node_returns_404_for_unknown_id` — GET /v1/fleet/nodes/{id} with bad ID returns 404
9. `get_node_returns_503_when_fleet_not_initialized` — 503 when no fleet
10. `remove_node_succeeds` — DELETE /v1/fleet/nodes/{id} removes and returns confirmation
11. `remove_node_returns_404_for_unknown_id` — DELETE with bad ID returns 404
12. `remove_node_returns_503_when_fleet_not_initialized` — 503 when no fleet
13. `dispatch_task_succeeds_for_online_node` — POST /v1/fleet/nodes/{id}/tasks returns accepted
14. `dispatch_task_rejects_offline_node` — POST to offline node returns 422
15. `dispatch_task_returns_404_for_unknown_node` — POST to unknown node returns 404
16. `dispatch_task_returns_503_when_fleet_not_initialized` — 503 when no fleet
17. `dispatch_task_validates_request_body` — missing `task` field returns 422
18. `node_status_label_maps_correctly` — unit test for status label helper
19. `capability_label_maps_known_and_custom` — unit test for capability label helper
20. `overview_marks_stale_before_counting` — verifies stale detection runs before count

### FleetManager tests (in `manager.rs`)

21. `remove_node_by_id_succeeds` — removes node by ID and revokes tokens
22. `remove_node_by_id_returns_error_for_unknown_id` — returns `NodeNotFound` for bad ID

---

## Acceptance Criteria

- `GET /v1/fleet/overview` returns correct aggregate metrics matching node registry state
- `GET /v1/fleet/nodes` returns all nodes with status, capabilities, and last-seen timestamps
- `GET /v1/fleet/nodes/{id}` returns detailed node information including endpoint and registration time
- `DELETE /v1/fleet/nodes/{id}` removes the node and revokes its tokens
- `POST /v1/fleet/nodes/{id}/tasks` dispatches a task to an available node
- All endpoints return 503 when fleet is not initialized
- All endpoints return standard error format `{"error": "<message>"}`
- Authenticated via standard `auth_middleware` (not per-node bearer tokens)
- All existing tests pass, clippy clean

---

## Open Questions

1. **Task tracking:** `active_tasks`, `queued_tasks`, and `recent_tasks` are stub zeros/empty arrays. A future track should add task state tracking to `FleetManager`. Should we add a TODO comment in the handler, or create a follow-up spec?

2. **Node removal by ID vs name:** `FleetManager::remove_node()` takes a name. We need `remove_node_by_id()`. This is a clean addition — the ID lookup and removal happen atomically inside the manager.

3. **Task dispatch payload:** The Appendix C schema uses freeform `task` string + `priority`. The existing `FleetTaskRequest` is much more structured (task_type, repo_url, branch, signal, config, etc.). For Phase 5.5 MVP, should the v1 endpoint accept the simple form and internally construct a `FleetTaskRequest`, or should it expose the full structured form? Recommend: simple form for MVP, full form in a follow-up.

4. **Stale threshold configuration:** The `NodeRegistry` default stale threshold is 60s. Should the fleet overview endpoint be configurable, or is 60s acceptable for the dashboard?
