# iOS MVP Specification

Status: Draft v1  
Owner: Citros Core Team  
Date: 2026-02-24

## 1. Purpose

Define a practical iOS MVP that delivers a strong "agentic assistant" experience while staying App-Store-safe.

This spec answers:
- Where to start now (before Android reaches full maturity)
- What to port from Kotlin
- When each port should happen

## 2. Product Goal

Ship an iOS app that can reliably execute supervised multi-step tasks (e.g., trip planning) using:
- LEAP function-calling orchestration
- App Intents, Shortcuts, deep links, notifications, and Live Activities
- Human approval for risky actions

## 3. Non-Goals (MVP)

- Generic cross-app UI puppeteering (tap/swipe/type anywhere)
- Unattended high-risk execution (payments, sensitive data sends)
- Full parity with rooted Android Accessibility control

## 4. Platform Constraints (Design Inputs)

1. iOS background execution is constrained; no Android-style always-on foreground daemon.
2. Execution must route through public APIs/surfaces.
3. Risky actions require explicit user approval.
4. Reliability must come from resumable state + deterministic loop semantics, not perpetual process lifetime.

## 5. Success Criteria (MVP)

1. Complete supervised trip task flow with approval checkpoints:
   - Find flight/hotel options
   - Add itinerary to calendar
   - Open booking links to finalize
   - Stage ride request handoff
2. Resume interrupted tasks safely (app background/foreground transitions).
3. Enforce policy precedence (`DENY > RATE_LIMIT > CONFIRM > ALLOW`) with audit trail.
4. Zero "hidden" execution for risky actions.

## 6. Start Now vs Start Later

### 6.1 Start Now (parallel to Android work)

These are low-regret and do not depend on Android loop internals changing.

1. iOS tool and policy contract package
   - Define tool schema contract and risk tiers in Swift.
   - Build policy matrix and approval requirements per tool.
2. Execution adapter layer
   - Implement adapter interfaces for App Intents, Shortcuts, deep links, notifications, Live Activities.
   - Return structured results even when execution is a handoff.
3. Approval UX shell
   - One-tap confirmation for T2 actions.
   - Strong approval (Face ID + explicit summary) for T3.
4. Task state persistence
   - Build resumable task state machine and durable storage.
   - Support pause/resume after lifecycle interruptions.
5. Audit/event model
   - Define per-step execution logs and policy decisions.

### 6.2 Wait Until Android Gates Are Hit

Full loop port should wait for Android contract stability.

Gate G1 (Loop Contract Freeze):
- `send -> continueAfterTools -> tool_result` semantics stable
- boundary-check precedence stable
- compaction behavior stable

Gate G2 (Safety Contract Freeze):
- policy engine behavior and precedence finalized
- confirmation semantics finalized
- risk taxonomy finalized

Gate G3 (Durability Baseline):
- service-resident Android execution validated in real lifecycle disruptions
- deterministic recovery patterns established

## 7. Kotlin -> iOS Port Plan (What, How, When)

| Kotlin source | Port target | Port type | When |
|---|---|---|---|
| `android/core/.../ToolUse.kt` | `AgentCoreSwift/Models.swift` | Direct concept port (types/schema) | Now |
| `android/core/.../PhoneTools.kt` | `AgentCoreSwift/ToolRegistry.swift` | Port + prune (iOS-safe tools only) | Now |
| `android/core/.../ToolCategory.kt` | `PolicyEngineSwift/RiskTaxonomy.swift` | Direct concept port | Now |
| `android/core/.../BudgetGuard.kt` | `PolicyEngineSwift/BudgetGuard.swift` | Direct concept port | Now |
| `android/core/.../ContextCompactor.kt` | `AgentCoreSwift/ContextCompactor.swift` | Protocol port (compaction strategy) | After G1 |
| `android/core/.../BoundaryCheck.kt` | `AgentCoreSwift/BoundaryChecks.swift` | Near 1:1 port | After G1 |
| `android/core/.../AgentExecutor.kt` | `AgentCoreSwift/AgentExecutor.swift` | Near 1:1 port (actor-based) | After G1 |
| `android/core/.../ActionVerifier.kt` | `AgentCoreSwift/ActionVerifier.swift` | Redesign for iOS surfaces | After G1 |
| `android/core/.../PhoneAgentApi.kt` | `AssistantCoordinator.swift` | Split/compose into coordinator + policy + router | After G2 |
| `android/chat/.../ChatViewModel.kt` | `AssistantUI/AssistantStore.swift` | Concept port (state/events), no Android dependencies | After G1 |
| `android/chat/.../OverlayService.kt` | `AutomationAdaptersIOS` + `LiveActivity` + notif flows | Redesign (not portable as-is) | Now |
| `android/chat/.../AndroidSensorProvider.kt` | `IOSSensorProvider.swift` | Redesign (iOS permission model) — see [ios-sensor-provider.md](ios-sensor-provider.md) | After G2 |
| `android/chat/.../ServiceToolDelegate.kt` | `AdapterRouter + ToolDelegate` | Concept port | After G1 |

## 8. iOS Architecture (MVP)

```text
AssistantUI (SwiftUI)
  -> AssistantStore (state/event orchestration)
     -> AgentExecutor (loop kernel)
        -> PolicyEngine (risk + approvals + budgets)
           -> AdapterRouter
              -> AppIntentAdapter
              -> ShortcutAdapter
              -> DeepLinkAdapter
              -> NotificationAdapter
              -> LiveActivityAdapter
```

Execution model:
1. Model proposes tool call.
2. Policy evaluates tool call.
3. If allowed, adapter executes.
4. If confirmation required, present approval UI/notification.
5. Persist result + next loop state.
6. Continue or pause.

## 9. Phased Timeline

### Phase A (Weeks 1-3, start now)

- Create iOS module boundaries and contracts.
- Implement policy matrix, approval flows, adapter skeletons.
- Implement durable task state + audit logging.

Exit:
- End-to-end simulated loop in test harness with mocked tool calls and policy decisions.

### Phase B (Weeks 4-7, after G1)

- Port loop kernel from Kotlin semantics:
  - boundary checks
  - steer/inject/stop handling
  - tool-result continuation flow
- Port ChatViewModel event model into `AssistantStore`.

Exit:
- Reliable deterministic loop behavior under lifecycle pause/resume in integration tests.

### Phase C (Weeks 8-11, after G2)

- Integrate finalized policy semantics from Android.
- Implement trip vertical actions:
  - search options
  - add calendar events
  - booking handoff links
  - ride handoff
- Add approval UX polish and strong-auth paths.

Exit:
- Supervised trip scenario works with clear checkpoints and auditability.

### Phase D (Weeks 12+, after G3)

- Reliability hardening and playbook strategy.
- Add replay templates for common successful task patterns.
- Optimize latency/cost and improve fallback behavior.

## 10. Feature Mapping for iOS MVP

| Capability | MVP channel | Notes |
|---|---|---|
| Plan and reason multi-step tasks | LEAP loop + executor | Core value driver |
| Calendar write | App Intent / EventKit-backed intent | Strong iOS fit |
| Messaging actions | App intent / shortcut / handoff | Confirmation required |
| Flight/hotel discovery | Search/API connectors | Pure tool execution |
| Booking completion | Deep link handoff + approval | Final submit often user-mediated |
| Ride scheduling | Intent/shortcut/handoff | Provider-specific capability limits |
| Progress visibility | Live Activity | Mandatory for trust |
| Approvals | Actionable notifications + in-app | Tiered by risk |

## 11. Risks and Mitigations

1. Risk: Premature full port while Android semantics still moving.
   - Mitigation: Gate on G1/G2/G3 before high-cost loop port.
2. Risk: Over-promising automation depth.
   - Mitigation: Explicitly design for supervised autopilot + handoff model.
3. Risk: Fragmented behavior across adapters.
   - Mitigation: Unified `ToolResult` contract and centralized policy routing.
4. Risk: App Store review rejection for agentic automation features.
   - Mitigation: All automation routes through public APIs (App Intents, Shortcuts, EventKit, URL schemes). No private API usage. Transparent approval UX for all side-effecting actions. Clear user consent flows. Provide reviewer documentation demonstrating user control at every step.

## 12. Testing Strategy

> **Comprehensive testing infrastructure is documented in
> [ios-testing-infrastructure.md](ios-testing-infrastructure.md).** This section
> provides a summary; see that document for test doubles catalog, code examples,
> CI pipeline design, coverage targets, and detailed phase exit criteria.

### Unit Tests

- **PolicyEngine:** Verify tier classification, approval requirements, channel selection, and deny fallbacks for all registered tools and edge cases (unknown tools, T4 tools).
- **BoundaryChecks:** Test each check in isolation (cancellation, step limit, action verification, steer injection) with controlled `LoopState` inputs.
- **BudgetGuard:** Test spend tracking, violation detection, and concurrent access safety.
- **Adapters:** Test each adapter returns structured `ToolResult` for success and error cases.
- **SensorProvider:** Test permission flow, graceful degradation for unavailable sensors, and registry routing (see [ios-sensor-provider.md](ios-sensor-provider.md) §7).

### Integration Tests (Test Harness)

- **Mock adapters:** No-op/scripted adapters that return deterministic `ToolResult` values for reproducible loop testing.
- **Mock PolicyEngine:** Configurable to return any `PolicyDecision` for controlled approval/deny flow testing (`ScriptedPolicyEngine`).
- **Mock ApprovalGate:** Auto-approve or auto-deny for testing both paths without UI (`AutoApproveGate`, `AutoDenyGate`, `ScriptedApprovalGate`).
- **Mock SensorProvider:** Scripted sensor readings and permission states for deterministic sensor testing.
- **Simulated loop:** End-to-end test with mock `continueAfterTools` responses that exercise multi-step tool chains, boundary check triggering, and loop exit conditions.

### Phase Exit Criteria

- **Phase A exit:** Simulated loop test passes with mocked tool calls, policy decisions, and approval gates. All boundary checks verified. Sensor provider protocol compiles with mock tests passing.
- **Phase B exit:** Deterministic loop behavior under lifecycle pause/resume in integration tests. Coverage thresholds met per [ios-testing-infrastructure.md](ios-testing-infrastructure.md) §7.
- **Phase C exit:** Supervised trip scenario test with real adapter integrations (calendar, deep link). Sensor provider reads device data on physical device.

## 13. Implementation Order (Immediate)

1. Create iOS contract package (`Models`, `PolicyTypes`, `ExecutionAdapter`).
2. Implement policy matrix and approval state machine.
3. Implement adapter router + no-op/mock adapters for deterministic tests.
4. Implement persistent task state and replayable execution log.
5. Build a single vertical demo flow: "plan trip" with staged approvals.

This gives real progress now without coupling to unstable Android internals.
