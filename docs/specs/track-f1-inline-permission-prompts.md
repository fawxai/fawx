# Track F-1: Inline Permission Prompts

**Status:** SPEC
**Priority:** Highest Phase 5 remaining — centerpiece safety feature
**Complexity:** High — touches kernel, SSE, HTTP

---

## Problem

The current ProposalGateExecutor has two outcomes: PassThrough (execute) or Block/Propose (deny with proposal file). There's no middle ground where the loop **pauses, asks the user, and resumes based on their answer.**

Phase 5 requires lightweight inline permission prompts: the agent wants to run a tool, the permission config says "ask", the loop pauses, the client shows a card with Allow/Deny/Allow-Session, and the loop resumes with the decision.

---

## Architecture

### Flow

1. Agent calls a tool (e.g., `web_search`)
2. ProposalGateExecutor classifies → new outcome: `AskPermission`
3. Executor generates a `PermissionPrompt` struct with unique ID
4. Executor emits SSE `permission_prompt` event via StreamCallback
5. Executor **waits** on a `tokio::sync::oneshot` channel for the response
6. Client shows the prompt inline in chat
7. User taps Allow/Deny/Allow-Session
8. Client calls `POST /v1/permissions/prompts/{id}/respond`
9. Handler sends the response through the oneshot channel
10. Executor receives response → either execute the tool or return a denied result
11. If prompt expires (5 min TTL), auto-deny and continue

### Key design decision: Where does the pause happen?

**Option A:** Pause inside ProposalGateExecutor::execute_tools()
- Pro: Clean — executor owns the decision
- Con: Requires async channel inside a sync Mutex context

**Option B:** Pause in loop_engine before calling execute_tools
- Pro: Loop already has async context, SSE stream, cancellation
- Con: Splits classification from execution

**Chosen: Option A with async support.** The ProposalGateExecutor is already async (implements `#[async_trait] ToolExecutor`). The `execute_tools` method is `async fn`. We can `await` a oneshot channel inside it.

---

## Implementation Plan (3 PRs)

### PR 1: Permission prompt types + SSE event + respond endpoint

**New types** (in fx-kernel or new fx-permissions crate):
```rust
pub struct PermissionPrompt {
    pub id: String,
    pub tool: String,
    pub title: String,
    pub reason: String,
    pub request_summary: String,
    pub expires_at: u64,
}

pub enum PermissionDecision {
    Allow,
    Deny,
    AllowSession,  // Allow + remember for this session
}

pub struct PermissionPromptState {
    pending: HashMap<String, oneshot::Sender<PermissionDecision>>,
    session_overrides: HashSet<String>,  // tool names allowed for this session
}
```

**New SSE event:**
```rust
StreamEvent::PermissionPrompt {
    id: String,
    tool: String,
    title: String,
    reason: String,
    request_summary: String,
    session_scoped_allow_available: bool,
    expires_at: u64,
}
```

**New HTTP endpoint:**
`POST /v1/permissions/prompts/{id}/respond`
```json
{
  "decision": "allow",  // "allow" | "deny"
  "scope": "session"    // "once" | "session"
}
```

### PR 2: Wire into ProposalGateExecutor

Add a new `GateDecision::AskPermission` variant. When PermissionsConfig says an action is `proposal_required` AND the action is a lightweight tool action (not a file write), use AskPermission instead of Propose.

In `execute_with_decisions`, handle AskPermission:
1. Create PermissionPrompt with unique ID
2. Create oneshot channel
3. Register in PermissionPromptState
4. Emit SSE event via a new callback mechanism
5. Await oneshot receiver with 5-minute timeout
6. On Allow → execute tool via inner executor
7. On Deny/Timeout → return denied ToolResult

### PR 3: Session-scoped overrides

When user picks "Allow Always in This Session":
- Store tool name in session_overrides set
- Future AskPermission checks for same tool skip the prompt
- Overrides cleared when session ends

---

## Scope for This Sprint

Given complexity, **only PR 1 is in scope now** — the types, SSE event, and respond endpoint. This gives the Swift app everything it needs to build the UI. PR 2 (kernel wiring) and PR 3 (session overrides) can follow.

PR 1 deliverables:
1. PermissionPrompt + PermissionDecision types in fx-kernel
2. StreamEvent::PermissionPrompt variant + SSE serialization
3. PermissionPromptState with register/resolve/expire methods
4. POST /v1/permissions/prompts/{id}/respond handler
5. Tests for all of the above

---

## Files to Create/Modify

### PR 1:
1. **MODIFY: `engine/crates/fx-kernel/src/streaming.rs`** — add PermissionPrompt StreamEvent variant
2. **NEW: `engine/crates/fx-kernel/src/permission_prompt.rs`** — types + PermissionPromptState
3. **MODIFY: `engine/crates/fx-kernel/src/lib.rs`** — export new module
4. **MODIFY: `engine/crates/fx-api/src/sse.rs`** — serialize new SSE event
5. **NEW: `engine/crates/fx-api/src/handlers/permission_prompts.rs`** — respond handler
6. **MODIFY: `engine/crates/fx-api/src/handlers/mod.rs`** — add module
7. **MODIFY: `engine/crates/fx-api/src/router.rs`** — add route
8. **MODIFY: `engine/crates/fx-api/src/state.rs`** — add PermissionPromptState to HttpState

---

## Tests Required

1. `permission_prompt_serializes` — PermissionPrompt JSON round-trip
2. `permission_decision_from_string` — parse "allow"/"deny"/"allow_session"
3. `sse_permission_prompt_event` — SSE frame format matches spec
4. `prompt_state_register_and_resolve` — register prompt, resolve it
5. `prompt_state_resolve_unknown_id_returns_error` — 404 for bad ID
6. `prompt_state_expire_removes_stale` — expired prompts cleaned up
7. `prompt_state_expired_respond_returns_409` — late response rejected
8. `respond_endpoint_returns_200` — happy path
9. `respond_endpoint_returns_404_unknown` — bad prompt ID
10. `respond_endpoint_returns_409_expired` — expired prompt

---

## Acceptance Criteria (PR 1)

- PermissionPrompt and PermissionDecision types exist
- StreamEvent::PermissionPrompt serializes to correct SSE format
- PermissionPromptState manages prompt lifecycle (register, resolve, expire)
- POST /v1/permissions/prompts/{id}/respond works
- All tests pass, clippy clean
- Swift app can build the prompt UI against these contracts
