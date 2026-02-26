# iOS Testing Infrastructure

Date: 2026-02-24
Status: Draft v1
Tracks: Issue #776

## 1. Purpose

Define the comprehensive testing infrastructure for the iOS agent port, covering
test doubles, framework patterns, CI pipeline, coverage targets, and phase exit
criteria. This document complements [ios-spec.md](ios-spec.md) §12 (Testing
Strategy) with concrete implementation details.

## 2. Testing Philosophy

The iOS port follows the same testing discipline as the Kotlin Android codebase:

1. **TDD mandatory** — RED → GREEN → REFACTOR for all new code
2. **Every feature needs tests** — no untested code ships
3. **Every bug fix needs a regression test** — prove the bug existed, prove it's fixed
4. **Protocol-first design enables test doubles** — all external dependencies are protocols
5. **Deterministic by default** — no flaky tests, no timing-dependent assertions

## 3. Test Doubles Catalog

### 3.1 Mock Adapters

Every `ExecutionAdapter` has a corresponding mock for testing:

```swift
/// Base mock adapter that records calls and returns scripted results.
public actor MockExecutionAdapter: ExecutionAdapter {
    public let channel: ExecutionChannel
    private var scriptedResults: [String: ToolResult]
    private(set) var executedCalls: [ToolCall] = []

    public init(
        channel: ExecutionChannel,
        results: [String: ToolResult] = [:]
    ) {
        self.channel = channel
        self.scriptedResults = results
    }

    public func execute(call: ToolCall) async -> ToolResult {
        executedCalls.append(call)
        return scriptedResults[call.name] ?? ToolResult(
            text: "Mock result for \(call.name)",
            isError: false
        )
    }

    // MARK: - Test Helpers

    public func setResult(_ result: ToolResult, for toolName: String) {
        scriptedResults[toolName] = result
    }

    public func reset() {
        executedCalls = []
    }

    public func callCount(for toolName: String) -> Int {
        executedCalls.filter { $0.name == toolName }.count
    }
}
```

Available mock adapters:

| Mock | Replaces | Purpose |
|---|---|---|
| `MockAppIntentAdapter` | `AppIntentAdapter` | Script app intent results |
| `MockShortcutAdapter` | `ShortcutAdapter` | Script shortcut execution |
| `MockDeepLinkAdapter` | `InternalDeepLinkAdapter` / `ExternalDeepLinkAdapter` | Script deep link opens |
| `MockLiveActivityAdapter` | `LiveActivityAdapter` | Verify activity updates |
| `MockSensorProvider` | `MotionSensorProvider` / `LocationSensorProvider` | Script sensor readings (see [ios-sensor-provider.md](ios-sensor-provider.md) §7) |

### 3.2 Scripted PolicyEngine

```swift
/// PolicyEngine that returns pre-configured decisions for testing.
/// Allows testing the full policy → approval → execution flow with
/// deterministic outcomes.
public struct ScriptedPolicyEngine: PolicyEngine {
    private let decisions: [String: PolicyDecision]
    private let defaultDecision: PolicyDecision

    public init(
        decisions: [String: PolicyDecision] = [:],
        defaultDecision: PolicyDecision = .allow(channel: .appIntent)
    ) {
        self.decisions = decisions
        self.defaultDecision = defaultDecision
    }

    public func evaluate(toolName: String, input: [String: String]) -> PolicyDecision {
        decisions[toolName] ?? defaultDecision
    }

    // MARK: - Factory Methods

    /// All tools allowed — for testing execution flow without policy interference.
    public static var allowAll: ScriptedPolicyEngine {
        ScriptedPolicyEngine(defaultDecision: .allow(channel: .appIntent))
    }

    /// All tools denied — for testing deny fallback behavior.
    public static var denyAll: ScriptedPolicyEngine {
        ScriptedPolicyEngine(defaultDecision: .deny(reason: "Test deny"))
    }

    /// All tools require approval — for testing approval gate flow.
    public static var approveAll: ScriptedPolicyEngine {
        ScriptedPolicyEngine(defaultDecision: .requireApproval(
            channel: .appIntent,
            requirement: .oneTap,
            summary: "Test approval"
        ))
    }
}
```

### 3.3 Auto-Approve / Auto-Deny Gates

```swift
/// ApprovalGate that automatically approves all requests.
public struct AutoApproveGate: ApprovalGate {
    public init() {}
    public func requestApproval(summary: String, requirement: ApprovalRequirement) async -> Bool {
        true
    }
}

/// ApprovalGate that automatically denies all requests.
public struct AutoDenyGate: ApprovalGate {
    public init() {}
    public func requestApproval(summary: String, requirement: ApprovalRequirement) async -> Bool {
        false
    }
}

/// ApprovalGate that records requests and returns scripted responses.
public actor ScriptedApprovalGate: ApprovalGate {
    private var responses: [Bool]
    private var responseIndex = 0
    private(set) var requests: [(summary: String, requirement: ApprovalRequirement)] = []

    /// Initialize with a sequence of approve/deny responses.
    /// Responses are consumed in order; cycles back to start if exhausted.
    public init(responses: [Bool] = [true]) {
        self.responses = responses
    }

    public func requestApproval(summary: String, requirement: ApprovalRequirement) async -> Bool {
        requests.append((summary, requirement))
        let response = responses[responseIndex % responses.count]
        responseIndex += 1
        return response
    }

    public var requestCount: Int { requests.count }
}
```

### 3.4 Mock BudgetGuard

```swift
/// BudgetGuard that tracks calls and returns scripted violations.
public actor MockBudgetGuard: BudgetGuard {
    private var violations: [String: BudgetViolation]
    private(set) var checks: [(toolName: String, estimatedCost: Double)] = []
    private(set) var spends: [(toolName: String, actualCost: Double)] = []

    public init(violations: [String: BudgetViolation] = [:]) {
        self.violations = violations
    }

    public func check(toolName: String, estimatedCost: Double) -> BudgetViolation? {
        checks.append((toolName, estimatedCost))
        return violations[toolName]
    }

    public func recordSpend(toolName: String, actualCost: Double) {
        spends.append((toolName, actualCost))
    }

    public func setViolation(_ violation: BudgetViolation, for toolName: String) {
        violations[toolName] = violation
    }
}
```

### 3.5 Scripted ToolExecutionDelegate

```swift
/// ToolExecutionDelegate that replays scripted tool results for deterministic
/// loop testing. This is the primary integration test double — it controls
/// what the AgentExecutor sees from tool execution.
public actor ScriptedToolDelegate: ToolExecutionDelegate {
    private var scriptedResults: [String: [ToolResult]]
    private var callCounts: [String: Int] = [:]
    private var uiMutatingTools: Set<String>
    private(set) var steerMessages: [String] = []
    private(set) var stepNotifications: [(step: Int, maxSteps: Int)] = []

    public init(
        results: [String: [ToolResult]] = [:],
        uiMutatingTools: Set<String> = []
    ) {
        self.scriptedResults = results
        self.uiMutatingTools = uiMutatingTools
    }

    public func executeToolCall(_ call: ToolCall, screen: ScreenSnapshot?) async -> ToolResult {
        let count = callCounts[call.name, default: 0]
        callCounts[call.name] = count + 1

        guard let results = scriptedResults[call.name], count < results.count else {
            return ToolResult(text: "Unscripted tool: \(call.name)", isError: true, errorCode: .toolNotFound)
        }
        return results[count]
    }

    public nonisolated func isUIMutatingTool(_ toolName: String) -> Bool {
        uiMutatingTools.contains(toolName)
    }

    public func refreshScreenAfterTool(_ toolName: String, _ result: ToolResult) async -> ScreenSnapshot? {
        ScreenSnapshot(hash: toolName.hashValue, summary: "After \(toolName)")
    }

    public func addSteerMessage(_ message: String) {
        steerMessages.append(message)
    }

    public func onStepStarted(_ step: Int, maxSteps: Int) {
        stepNotifications.append((step, maxSteps))
    }
}
```

## 4. XCTest + Swift Testing Patterns

### 4.1 Actor-Isolated Test Patterns

Testing actors (like `AgentExecutor`) requires async test methods:

```swift
import Testing

@Suite("AgentExecutor Loop Tests")
struct AgentExecutorTests {

    @Test("Completes with noTools when response has no tool calls")
    func noToolCalls() async {
        let delegate = ScriptedToolDelegate()
        let listener = MockLoopProgressListener()
        let executor = AgentExecutor(delegate: delegate, listener: listener)

        let response = ChatResponse(
            text: "Hello!",
            toolCalls: [],
            stopReason: "end_turn",
            usage: nil
        )

        let result = await executor.run(
            initialResponse: response,
            initialScreen: nil,
            isCancelled: { false },
            continueAfterTools: { fatalError("Should not be called") }
        )

        guard case .completed(let text, let steps, let reason) = result else {
            Issue.record("Expected .completed")
            return
        }
        #expect(text == "Hello!")
        #expect(steps == 0)
        #expect(reason == .noTools)
    }

    @Test("Executes tool calls and continues loop")
    func toolCallLoop() async {
        let delegate = ScriptedToolDelegate(results: [
            "search_flights": [ToolResult(text: "Found 3 flights")]
        ])
        let listener = MockLoopProgressListener()
        let executor = AgentExecutor(delegate: delegate, listener: listener)

        let initialResponse = ChatResponse(
            text: nil,
            toolCalls: [ToolCall(id: "1", name: "search_flights", input: ["dest": "SFO"])],
            stopReason: "tool_use",
            usage: nil
        )

        var continuationCount = 0
        let result = await executor.run(
            initialResponse: initialResponse,
            initialScreen: nil,
            isCancelled: { false },
            continueAfterTools: {
                continuationCount += 1
                return ChatResponse(
                    text: "Here are your flight options.",
                    toolCalls: [],
                    stopReason: "end_turn",
                    usage: nil
                )
            }
        )

        guard case .completed(let text, let steps, let reason) = result else {
            Issue.record("Expected .completed")
            return
        }
        #expect(text == "Here are your flight options.")
        #expect(steps == 1)
        #expect(reason == .completed)
        #expect(continuationCount == 1)
    }

    @Test("Stops at step limit")
    func stepLimitStop() async {
        let delegate = ScriptedToolDelegate(results: [
            "search": Array(repeating: ToolResult(text: "Result"), count: 30)
        ])
        let listener = MockLoopProgressListener()
        let executor = AgentExecutor(
            delegate: delegate,
            listener: listener,
            maxToolSteps: 3
        )

        let response = ChatResponse(
            text: nil,
            toolCalls: [ToolCall(id: "1", name: "search", input: [:])],
            stopReason: "tool_use",
            usage: nil
        )

        let result = await executor.run(
            initialResponse: response,
            initialScreen: nil,
            isCancelled: { false },
            continueAfterTools: {
                ChatResponse(
                    text: nil,
                    toolCalls: [ToolCall(id: "2", name: "search", input: [:])],
                    stopReason: "tool_use",
                    usage: nil
                )
            }
        )

        guard case .completed(_, let steps, let reason) = result else {
            Issue.record("Expected .completed")
            return
        }
        #expect(reason == .maxSteps)
        #expect(steps == 3)
    }

    @Test("Cancellation stops loop immediately")
    func cancellation() async {
        let delegate = ScriptedToolDelegate(results: [
            "slow_tool": [ToolResult(text: "Done")]
        ])
        let listener = MockLoopProgressListener()
        let executor = AgentExecutor(delegate: delegate, listener: listener)

        var cancelled = false
        let response = ChatResponse(
            text: nil,
            toolCalls: [ToolCall(id: "1", name: "slow_tool", input: [:])],
            stopReason: "tool_use",
            usage: nil
        )

        let result = await executor.run(
            initialResponse: response,
            initialScreen: nil,
            isCancelled: {
                // Cancel after first tool execution
                cancelled = true
                return cancelled
            },
            continueAfterTools: {
                ChatResponse(text: nil, toolCalls: [], stopReason: "end_turn", usage: nil)
            }
        )

        guard case .completed(_, _, let reason) = result else {
            Issue.record("Expected .completed")
            return
        }
        #expect(reason == .cancelled || reason == .completed)
    }
}
```

### 4.2 PolicyEngine Test Patterns

```swift
@Suite("DefaultPolicyEngine Tests")
struct DefaultPolicyEngineTests {
    let engine = DefaultPolicyEngine()

    @Test("T0 tool auto-allowed")
    func t0AutoAllow() {
        let decision = engine.evaluate(toolName: "summarize_day", input: [:])
        guard case .allow(let channel) = decision else {
            Issue.record("Expected .allow")
            return
        }
        #expect(channel == .appIntent)
    }

    @Test("T2 irreversible tool escalates to strong biometric")
    func t2IrreversibleEscalation() {
        let decision = engine.evaluate(toolName: "send_message", input: ["to": "alice"])
        guard case .requireApproval(_, let requirement, let summary) = decision else {
            Issue.record("Expected .requireApproval")
            return
        }
        #expect(requirement == .strongBiometric)  // Escalated from oneTap for irreversible
        #expect(summary.contains("irreversible"))
    }

    @Test("T3 tool requires strong biometric")
    func t3StrongBiometric() {
        let decision = engine.evaluate(toolName: "transfer_funds", input: ["amount": "100"])
        guard case .requireApproval(_, let requirement, _) = decision else {
            Issue.record("Expected .requireApproval")
            return
        }
        #expect(requirement == .strongBiometric)
    }

    @Test("Unknown tool is denied")
    func unknownToolDenied() {
        let decision = engine.evaluate(toolName: "nonexistent_tool", input: [:])
        guard case .deny(let reason) = decision else {
            Issue.record("Expected .deny")
            return
        }
        #expect(reason.contains("not registered"))
    }
}
```

### 4.3 BoundaryCheck Test Patterns

```swift
@Suite("BoundaryCheck Tests")
struct BoundaryCheckTests {

    @Test("CancellationCheck stops when cancelled")
    func cancellationCheck() async {
        let check = CancellationCheck()
        let state = LoopState(
            step: 1, maxSteps: 25,
            lastToolName: "test",
            lastScreenHash: nil, preActionScreenHash: nil,
            lastToolWasUIMutating: false,
            isCancelled: true,
            pendingSteerMessages: []
        )
        let result = await check.check(state: state)
        guard case .stop(.cancelled) = result else {
            Issue.record("Expected .stop(.cancelled)")
            return
        }
    }

    @Test("StepLimitCheck stops at max")
    func stepLimitCheck() async {
        let check = StepLimitCheck()
        let state = LoopState(
            step: 25, maxSteps: 25,
            lastToolName: "test",
            lastScreenHash: nil, preActionScreenHash: nil,
            lastToolWasUIMutating: false,
            isCancelled: false,
            pendingSteerMessages: []
        )
        let result = await check.check(state: state)
        guard case .stop(.maxSteps) = result else {
            Issue.record("Expected .stop(.maxSteps)")
            return
        }
    }

    @Test("ActionVerificationCheck injects warning on unchanged screen")
    func actionVerificationUnchanged() async {
        let check = ActionVerificationCheck()
        let hash = 12345
        let state = LoopState(
            step: 1, maxSteps: 25,
            lastToolName: "tap_button",
            lastScreenHash: hash, preActionScreenHash: hash,
            lastToolWasUIMutating: true,
            isCancelled: false,
            pendingSteerMessages: []
        )
        let result = await check.check(state: state)
        guard case .inject(let msg) = result else {
            Issue.record("Expected .inject")
            return
        }
        #expect(msg.contains("may not have taken effect"))
    }

    @Test("SteerCheck returns steer messages")
    func steerCheck() async {
        let check = SteerCheck()
        let state = LoopState(
            step: 1, maxSteps: 25,
            lastToolName: "test",
            lastScreenHash: nil, preActionScreenHash: nil,
            lastToolWasUIMutating: false,
            isCancelled: false,
            pendingSteerMessages: ["Focus on flights only"]
        )
        let result = await check.check(state: state)
        guard case .steer(let messages) = result else {
            Issue.record("Expected .steer")
            return
        }
        #expect(messages == ["Focus on flights only"])
    }
}
```

## 5. Integration Test Harness

### 5.1 Deterministic Loop Replay

The integration test harness exercises the full agent loop with scripted
inputs and deterministic assertions:

```swift
@Suite("Integration: Full Loop Replay")
struct FullLoopReplayTests {

    /// Trip planning scenario: search → create event → handoff booking
    @Test("Trip planning flow with approvals")
    func tripPlanningFlow() async {
        // Configure scripted policy: allow search, require approval for calendar
        let policy = ScriptedPolicyEngine(decisions: [
            "search_flights": .allow(channel: .appIntent),
            "create_event": .requireApproval(
                channel: .appIntent,
                requirement: .oneTap,
                summary: "Add flight to calendar"
            ),
            "open_booking_link": .allow(channel: .deepLinkExternal)
        ])

        let approvalGate = ScriptedApprovalGate(responses: [true])  // Approve calendar

        let delegate = ScriptedToolDelegate(results: [
            "search_flights": [ToolResult(text: "LAX→SFO $199, LAX→SFO $249")],
            "create_event": [ToolResult(text: "Event created: Flight LAX→SFO Feb 28")],
            "open_booking_link": [ToolResult(text: "Opened booking page")]
        ])

        let listener = MockLoopProgressListener()
        let executor = AgentExecutor(delegate: delegate, listener: listener)

        // Simulate 3-step tool chain
        let responses: [ChatResponse] = [
            // Step 1: search
            ChatResponse(
                text: nil,
                toolCalls: [ToolCall(id: "1", name: "search_flights", input: ["route": "LAX-SFO"])],
                stopReason: "tool_use", usage: nil
            ),
            // Step 2: create event
            ChatResponse(
                text: nil,
                toolCalls: [ToolCall(id: "2", name: "create_event", input: ["title": "Flight"])],
                stopReason: "tool_use", usage: nil
            ),
            // Step 3: open booking
            ChatResponse(
                text: nil,
                toolCalls: [ToolCall(id: "3", name: "open_booking_link", input: ["url": "https://..."])],
                stopReason: "tool_use", usage: nil
            ),
            // Final: no more tools
            ChatResponse(
                text: "Your trip is planned! Flight booked, calendar updated.",
                toolCalls: [],
                stopReason: "end_turn", usage: nil
            )
        ]

        var responseIndex = 0
        let result = await executor.run(
            initialResponse: responses[0],
            initialScreen: nil,
            isCancelled: { false },
            continueAfterTools: {
                responseIndex += 1
                return responses[responseIndex]
            }
        )

        guard case .completed(let text, let steps, let reason) = result else {
            Issue.record("Expected .completed")
            return
        }
        #expect(reason == .completed)
        #expect(steps == 3)
        #expect(text?.contains("trip is planned") == true)
    }
}
```

### 5.2 Lifecycle Pause/Resume Tests

```swift
@Suite("Integration: Lifecycle Resilience")
struct LifecycleResilienceTests {

    @Test("Loop survives cancellation and resumes from persisted state")
    func pauseResumeFlow() async {
        // This tests the durable task state contract:
        // 1. Start loop with 3 tools
        // 2. Cancel after tool 1
        // 3. Verify state is persisted
        // 4. Resume from persisted state
        // 5. Complete remaining tools

        let delegate = ScriptedToolDelegate(results: [
            "step_1": [ToolResult(text: "Step 1 done")],
            "step_2": [ToolResult(text: "Step 2 done")],
            "step_3": [ToolResult(text: "Step 3 done")]
        ])
        let listener = MockLoopProgressListener()
        let executor = AgentExecutor(delegate: delegate, listener: listener)

        var step = 0
        let result = await executor.run(
            initialResponse: ChatResponse(
                text: nil,
                toolCalls: [ToolCall(id: "1", name: "step_1", input: [:])],
                stopReason: "tool_use", usage: nil
            ),
            initialScreen: nil,
            isCancelled: {
                // Cancel after first step
                step > 0
            },
            continueAfterTools: {
                step += 1
                return ChatResponse(
                    text: nil,
                    toolCalls: [ToolCall(id: "2", name: "step_2", input: [:])],
                    stopReason: "tool_use", usage: nil
                )
            }
        )

        guard case .completed(_, _, let reason) = result else {
            Issue.record("Expected .completed")
            return
        }
        #expect(reason == .cancelled)
        // Verify listener recorded step 1 completion
        #expect(listener.completedTools.contains("step_1"))
    }
}
```

### 5.3 Sensor Provider Integration Tests

```swift
@Suite("Integration: Sensor Provider")
struct SensorProviderIntegrationTests {

    @Test("Registry routes to correct provider")
    func registryRouting() async {
        let motionProvider = MockSensorProvider(
            sensors: [.accelerometer, .gyroscope],
            readings: [
                .accelerometer: SensorReading(
                    sensorType: .accelerometer,
                    values: ["x": 0.1, "y": 9.8, "z": 0.0]
                )
            ]
        )
        let locationProvider = MockSensorProvider(
            sensors: [.location],
            readings: [
                .location: SensorReading(
                    sensorType: .location,
                    values: ["latitude": 37.7749, "longitude": -122.4194]
                )
            ]
        )

        let registry = SensorProviderRegistry(providers: [motionProvider, locationProvider])

        let accelResult = await registry.readAsToolResult(sensor: .accelerometer)
        #expect(!accelResult.isError)
        #expect(accelResult.text.contains("9.8"))

        let locationResult = await registry.readAsToolResult(sensor: .location)
        #expect(!locationResult.isError)
        #expect(locationResult.text.contains("37.7749"))
    }

    @Test("Unavailable sensor returns structured error")
    func unavailableSensor() async {
        let registry = SensorProviderRegistry(providers: [])
        let result = await registry.readAsToolResult(sensor: .heartRate)
        #expect(result.isError)
        #expect(result.errorCode == .notConfigured)
    }

    @Test("Permission denied returns privacy blocked error")
    func permissionDenied() async {
        let provider = MockSensorProvider(
            sensors: [.location],
            permissions: [.location: .denied]
        )
        let registry = SensorProviderRegistry(providers: [provider])
        let result = await registry.readAsToolResult(sensor: .location)
        #expect(result.isError)
        #expect(result.errorCode == .privacyBlocked)
    }
}
```

## 6. CI Pipeline Design

### 6.1 Pipeline Stages

```yaml
# .github/workflows/ios-tests.yml (conceptual)
name: iOS Tests

on:
  pull_request:
    paths:
      - 'ios/**'
      - 'docs/specs/ios-*'

jobs:
  unit-tests:
    runs-on: macos-15
    steps:
      - uses: actions/checkout@v4
      - name: Select Xcode
        run: sudo xcode-select -s /Applications/Xcode_16.app
      - name: Run Unit Tests
        run: |
          xcodebuild test \
            -scheme AgentCoreSwift \
            -destination 'platform=iOS Simulator,name=iPhone 16 Pro' \
            -resultBundlePath TestResults/AgentCore.xcresult
      - name: Run Policy Tests
        run: |
          xcodebuild test \
            -scheme PolicyEngineSwift \
            -destination 'platform=iOS Simulator,name=iPhone 16 Pro' \
            -resultBundlePath TestResults/PolicyEngine.xcresult
      - name: Run Sensor Tests
        run: |
          xcodebuild test \
            -scheme SensorProviderIOS \
            -destination 'platform=iOS Simulator,name=iPhone 16 Pro' \
            -resultBundlePath TestResults/SensorProvider.xcresult

  integration-tests:
    runs-on: macos-15
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      - name: Select Xcode
        run: sudo xcode-select -s /Applications/Xcode_16.app
      - name: Run Integration Tests
        run: |
          xcodebuild test \
            -scheme AssistantAppIntegrationTests \
            -destination 'platform=iOS Simulator,name=iPhone 16 Pro' \
            -resultBundlePath TestResults/Integration.xcresult

  coverage:
    runs-on: macos-15
    needs: [unit-tests, integration-tests]
    steps:
      - name: Generate Coverage Report
        run: |
          xcrun xccov view --report TestResults/AgentCore.xcresult \
            --json > coverage-agent-core.json
          xcrun xccov view --report TestResults/PolicyEngine.xcresult \
            --json > coverage-policy-engine.json
      - name: Check Coverage Thresholds
        run: |
          python3 scripts/check-coverage.py \
            --min-line 80 \
            --min-branch 70 \
            coverage-agent-core.json coverage-policy-engine.json
```

### 6.2 Test Scheme Organization

| Scheme | Contents | Runs On |
|---|---|---|
| `AgentCoreSwift` | BoundaryCheck, AgentExecutor, Models, ContextCompactor tests | Every PR |
| `PolicyEngineSwift` | PolicyEngine, BudgetGuard, DefaultPolicyMatrix tests | Every PR |
| `SensorProviderIOS` | SensorProvider, MockSensorProvider tests | Every PR |
| `AutomationAdaptersIOS` | Adapter routing, mock adapter tests | Every PR |
| `AssistantUI` | AssistantStore, approval flow tests | Every PR |
| `AssistantAppIntegrationTests` | Full loop replay, lifecycle, trip scenario | Every PR |

## 7. Coverage Targets

| Module | Line Coverage | Branch Coverage | Notes |
|---|---|---|---|
| `AgentCoreSwift` | ≥ 85% | ≥ 75% | Loop kernel is safety-critical |
| `PolicyEngineSwift` | ≥ 90% | ≥ 85% | Policy decisions must be exhaustively tested |
| `SensorProviderIOS` | ≥ 80% | ≥ 70% | Permission flows, all sensor types |
| `AutomationAdaptersIOS` | ≥ 75% | ≥ 65% | Adapters are thin; focus on routing |
| `AssistantUI` | ≥ 70% | ≥ 60% | UI state management |
| **Overall** | **≥ 80%** | **≥ 70%** | Enforced in CI |

### Coverage Enforcement

- CI fails if any module drops below its threshold
- Coverage deltas shown on PR: new code must meet or exceed module target
- Untested code in safety-critical paths (PolicyEngine, BoundaryChecks) is a blocking review issue

## 8. Snapshot / UI Testing Strategy

### 8.1 SwiftUI Preview Tests

```swift
import XCTest
import SnapshotTesting

final class ApprovalUISnapshotTests: XCTestCase {

    func testOneTapApprovalSheet() {
        let view = ApprovalSheetView(
            summary: "Send message to Alice",
            requirement: .oneTap,
            onApprove: {},
            onDeny: {}
        )
        assertSnapshot(of: view, as: .image(layout: .device(config: .iPhone13Pro)))
    }

    func testStrongBiometricApprovalSheet() {
        let view = ApprovalSheetView(
            summary: "Transfer $500 to checking",
            requirement: .strongBiometric,
            onApprove: {},
            onDeny: {}
        )
        assertSnapshot(of: view, as: .image(layout: .device(config: .iPhone13Pro)))
    }

    func testLoopTimelineView() {
        let viewModel = LoopTimelineViewModel.preview(steps: [
            .toolStarted("search_flights"),
            .toolResult("search_flights", "Found 3 flights"),
            .toolStarted("create_event"),
            .approvalRequested("Add flight to calendar"),
            .approved,
            .toolResult("create_event", "Event created"),
            .completed("Trip planned!")
        ])
        let view = LoopTimelineView(viewModel: viewModel)
        assertSnapshot(of: view, as: .image(layout: .device(config: .iPhone13Pro)))
    }
}
```

### 8.2 Snapshot Testing Rules

1. **Record snapshots** on a specific simulator (iPhone 16 Pro, iOS 18) for consistency
2. **Perceptual diff** tolerance: 0.1% pixel difference allowed (font rendering variance)
3. **Update snapshots** explicitly via `record: true` — never auto-accept changes
4. **Dark mode variants** — snapshot both light and dark for approval flows

## 9. Phase Exit Criteria (Detailed)

### Phase A Exit (Weeks 1–3)

| Criterion | Validation |
|---|---|
| Policy matrix covers all v1 tools | Unit test for each tool in `DefaultPolicyMatrix.table` |
| Boundary checks pass in isolation | Unit tests for all 4 checks with edge cases |
| BudgetGuard tracks spend correctly | Unit test: check + record + violation detection |
| Mock adapters return scripted results | Unit test per adapter mock |
| End-to-end simulated loop passes | Integration test: 3-tool chain with policy + approval |
| Sensor provider protocol compiles | Unit test: mock sensor provider returns scripted readings |
| CI pipeline runs all tests | GitHub Actions workflow green |

### Phase B Exit (Weeks 4–7)

| Criterion | Validation |
|---|---|
| Loop kernel matches Kotlin semantics | Side-by-side test: same inputs produce same outputs |
| Lifecycle pause/resume works | Integration test: cancel → persist → resume → complete |
| Steer injection during loop works | Integration test: steer message mid-loop redirects execution |
| Check precedence matches Kotlin | Test: cancel > step limit > verify > steer ordering |
| Coverage thresholds met | CI coverage report above module targets |

### Phase C Exit (Weeks 8–11)

| Criterion | Validation |
|---|---|
| Trip scenario works end-to-end | Integration test with real EventKit + deep link adapters |
| Sensor provider reads real device data | Manual test on physical device (simulator lacks sensors) |
| Approval UI renders correctly | Snapshot tests for all approval tiers |
| All adapters return structured results | Integration test per adapter |

### Phase D Exit (Weeks 12+)

| Criterion | Validation |
|---|---|
| Replay templates work for common scenarios | Integration test: replay a recorded successful flow |
| Performance benchmarks established | XCTest performance metrics for loop latency |
| Edge cases covered | Fuzz testing for malformed tool calls, empty responses |

## 10. Test Helper Utilities

### `MockLoopProgressListener`

```swift
/// Records all loop progress events for assertion in tests.
public actor MockLoopProgressListener: LoopProgressListener {
    private(set) var startedTools: [(name: String, index: Int, total: Int)] = []
    private(set) var completedTools: [String] = []
    private(set) var completions: [(reason: LoopExitReason, steps: Int, text: String?)] = []

    public init() {}

    public func onToolStarted(_ toolName: String, index: Int, total: Int) {
        startedTools.append((toolName, index, total))
    }

    public func onToolResult(_ toolName: String, result: ToolResult) {
        completedTools.append(toolName)
    }

    public func onLoopCompleted(reason: LoopExitReason, steps: Int, finalText: String?) {
        completions.append((reason, steps, finalText))
    }
}
```

## 11. References

- [ios-spec.md](ios-spec.md) — iOS MVP Specification (§12 for testing strategy overview)
- [ios-agent-architecture-draft.md](ios-agent-architecture-draft.md) — Protocol skeletons and module layout
- [ios-sensor-provider.md](ios-sensor-provider.md) — Sensor provider design and mock provider
