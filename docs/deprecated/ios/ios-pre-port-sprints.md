# iOS Pre-Port Sprint Plan

Status: Proposed  
Owner: iOS + Core  
Date: 2026-02-24

## Context

This plan defines iOS work that can begin now before full Android loop-port parity.

Source docs:
- `docs/specs/ios-spec.md`
- `docs/specs/ios-agent-architecture-draft.md`
- `docs/specs/ios-sensor-provider.md`
- `docs/specs/ios-testing-infrastructure.md`

This plan explicitly respects Android stabilization gates in `ios-spec.md`:
- `G1`: loop contract freeze
- `G2`: safety/policy contract freeze
- `G3`: durability baseline validated

## Objectives

1. Deliver low-regret iOS foundation work immediately.
2. Keep all high-cost loop-port work gated on Android contract stability.
3. Produce measurable, CI-enforced progress each sprint.

## Prerequisites and Assumptions

1. iOS module skeletons exist and build in CI (Agent core, policy engine, adapters, and test targets).
2. Referenced iOS specs are stable enough for pre-port work and can be treated as implementation contracts.
3. Sprint duration estimates (2 weeks each) assume 1-2 iOS engineers at near full-time capacity.
4. Android gate milestones (`G1`/`G2`/`G3`) continue to be the source of truth for full loop-port timing.

## Sprint 1: Contracts + Policy Foundation (2 weeks)

### Goal
Establish stable Swift contracts and safety policy semantics independent of Android loop internals.

### Scope
1. Create core model contracts in `AgentCoreSwift`:
   - `ToolDefinition`, `ToolCall`, `ToolResult`, `ChatResponse`, `TokenUsage`
2. Create policy contracts in `PolicyEngineSwift`:
   - risk tiers, execution channels, approval requirements, policy decisions
3. Implement default matrix and policy evaluator:
   - enforce precedence: `DENY > RATE_LIMIT > CONFIRM > ALLOW`
4. Implement budget/rate-limit guard abstractions for policy checks.
5. Add unit tests for every registered v1 tool decision path.

### Deliverables
1. `AgentCoreSwift` contracts module (compilable).
2. `PolicyEngineSwift` with default matrix + evaluator.
3. Test suite proving policy precedence and escalation behavior.

### Dependencies
1. None outside current specs and iOS module skeleton.

### Exit Criteria
1. 100% of registered v1 tools have allow/confirm/deny test coverage.
2. Policy precedence tests pass in CI.
3. Budget/rate-limit scenarios have deterministic unit tests.

## Sprint 2: Adapter + Approval Shell (2 weeks)

### Goal
Implement execution adapter interfaces and human approval shell with deterministic mocked behavior.

### Scope
1. Build `AutomationAdaptersIOS` interfaces + router:
   - AppIntent adapter
   - Shortcut adapter
   - DeepLink adapter
   - Notification adapter
2. Build `ApprovalGate` flow:
   - one-tap approval (T2)
   - strong approval path scaffold (T3)
3. Wire policy decisions to adapter routing.
4. Add mock adapters and scripted approval gates for integration testing.
5. Add UI state shell for status timeline + pending approvals.
6. Keep Live Activity progress updates in the UI/status surface layer for this sprint (not a standalone execution adapter).

### Deliverables
1. Adapter router with structured `ToolResult` outputs.
2. Approval shell and state transitions for required approvals.
3. Integration tests for mock end-to-end flows.

### Dependencies
1. Sprint 1 policy contracts + evaluator.

### Exit Criteria
1. Simulated multi-step flow completes through mocked adapters.
2. Approval-required actions cannot execute without approval token.
3. Integration suite for adapter/policy/approval is green in CI.

## Sprint 3: Persistence + Audit + Harness (2 weeks)

### Goal
Make iOS execution state durable and testable under interruption/resume conditions.

### Scope
1. Implement resumable task state store:
   - persisted tool step timeline
   - pending approvals
   - resumable execution cursor
2. Implement audit log model:
   - tool call
   - policy decision
   - approval event
   - execution result
3. Implement deterministic test harness scenarios (see `docs/specs/ios-testing-infrastructure.md`):
   - pause/resume
   - cancellation
   - denied-action fallback
   - approval timeout behavior
4. Implement sensor-provider protocol and fake provider for tests.
   - runtime sensor integration deferred until `G2`

### Deliverables
1. Persisted task-state manager for iOS orchestrator shell.
2. Audit/event schema and storage API.
3. Deterministic integration harness integrated into CI workflows.

### Dependencies
1. Sprint 2 adapter + approval shell.

### Exit Criteria
1. Lifecycle interruption tests pass consistently.
2. Every executed/simulated step emits an audit entry.
3. CI runs unit + integration suites without flake for harness scenarios.

## Deferred Until Android Gates

The following items are out of scope for pre-port sprints and blocked by gate criteria:

1. Full `AgentExecutor` semantic port and boundary-check parity (`G1`).
2. Final `ActionVerifier` parity implementation (`G1`/`G2`).
3. Runtime sensor-provider behavior tied to finalized safety policy (`G2`).
4. Full reliability tuning based on service-resident execution assumptions (`G3`).

## Cross-Sprint Milestones

1. End of Sprint 1: policy-driven tool decisions are production-shaped and test-complete.
2. End of Sprint 2: adapter/approval shell demonstrates supervised autopilot flows.
3. End of Sprint 3: durable, auditable, resumable iOS orchestration shell with deterministic harness.

## Risks and Mitigations

1. Risk: implementing loop semantics before Android freeze creates rework.
   - Mitigation: strict gate policy for full loop port (`G1`/`G2`/`G3`).
2. Risk: adapter behavior drifts from policy expectations.
   - Mitigation: central policy router integration tests using mocked adapters.
3. Risk: lifecycle-driven flakiness hides regressions.
   - Mitigation: deterministic harness with scripted pauses/resumes and CI enforcement.

## Tracking

Recommended issue labels:
- `ios-preport`
- `policy-engine`
- `adapter-shell`
- `approval-flow`
- `task-state`
- `audit`
- `harness`

Recommended board columns:
1. Planned
2. In Progress
3. Blocked on G1/G2/G3
4. Ready for Review
5. Done
