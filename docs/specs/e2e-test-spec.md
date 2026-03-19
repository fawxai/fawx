# Fawx Native App — E2E Test Suite Specification

**Status:** Ready for implementation  
**Framework:** XCUITest (Xcode UI Testing)  
**Target:** macOS (primary) + iOS (secondary)  
**Prerequisites:** Running `fawx serve --http` on Mac Mini (`http://100.123.20.63:8400`)

---

## 1. Architecture

### 1.1 Test Target

```
Fawx/
├── FawxUITests/
│   ├── Helpers/
│   │   ├── FawxTestApp.swift          ← XCUIApplication wrapper + launch helpers
│   │   ├── ServerFixture.swift        ← Server health check, reset, seed data
│   │   ├── AccessibilityIDs.swift     ← Centralized identifier constants
│   │   ├── Timeouts.swift             ← Shared timeout constants
│   │   └── Assertions.swift           ← Custom XCTest assertions (e.g., waitForText)
│   ├── Flows/
│   │   ├── PairingFlowTests.swift     ← Device pairing onboarding
│   │   ├── SessionFlowTests.swift     ← Session CRUD lifecycle
│   │   ├── ChatFlowTests.swift        ← Message send + SSE streaming
│   │   ├── ModelSwitchTests.swift     ← Model/thinking picker
│   │   ├── SkillsBrowserTests.swift   ← Skills list view
│   │   ├── SettingsTests.swift        ← Settings screens
│   │   └── ErrorStateTests.swift      ← Offline, auth failure, server errors
│   ├── Platform/
│   │   ├── MacOSTests.swift           ← macOS-specific: keyboard shortcuts, sidebar, multi-window
│   │   └── IOSTests.swift             ← iOS-specific: tab bar, gestures, safe area, keyboard
│   └── Resilience/
│       ├── ReconnectionTests.swift    ← Connection drop/restore
│       └── StreamInterruptTests.swift ← Mid-stream cancel, disconnect
```

### 1.2 Accessibility Identifiers

Every interactive element needs a stable accessibility identifier for XCUITest targeting. These MUST be added to the SwiftUI views during the next Codex build pass.

```swift
// AccessibilityIDs.swift — single source of truth
enum AID {
    // Onboarding / Pairing
    static let serverURLField = "server_url_field"
    static let pairingCodeField = "pairing_code_field"
    static let connectButton = "connect_button"
    static let pairButton = "pair_button"
    static let skipPairingButton = "skip_pairing_button"

    // Session List
    static let sessionList = "session_list"
    static let newSessionButton = "new_session_button"
    static let sessionRow = "session_row"          // + _\(sessionId) suffix
    static let deleteSessionButton = "delete_session"

    // Chat
    static let chatView = "chat_view"
    static let messageInput = "message_input"
    static let sendButton = "send_button"
    static let messageBubble = "message_bubble"    // + _\(index) suffix
    static let streamingIndicator = "streaming_indicator"
    static let stopStreamingButton = "stop_streaming"
    static let queuedChip = "queued_message_chip"

    // Tool Calls
    static let toolCallCard = "tool_call_card"     // + _\(index) suffix
    static let toolCallName = "tool_call_name"
    static let toolCallStatus = "tool_call_status" // running/complete/error

    // Model / Thinking Pickers
    static let modelPicker = "model_picker"
    static let modelBadge = "model_badge"
    static let thinkingPicker = "thinking_picker"
    static let thinkingBadge = "thinking_badge"

    // Status Bar
    static let statusBar = "status_bar"
    static let connectionIndicator = "connection_indicator"
    static let contextProgress = "context_progress"

    // Settings
    static let settingsView = "settings_view"
    static let serverURLSetting = "settings_server_url"
    static let themePicker = "settings_theme_picker"
    static let testConnectionButton = "test_connection_button"
    static let authStatusList = "auth_status_list"

    // Skills
    static let skillsList = "skills_list"
    static let skillCard = "skill_card"            // + _\(skillName) suffix

    // Empty States
    static let emptySessionList = "empty_session_list"
    static let emptyChatView = "empty_chat_view"
    static let offlineBanner = "offline_banner"
    static let authErrorBanner = "auth_error_banner"
}
```

### 1.3 Server Fixture

Tests run against a real Fawx server. A `ServerFixture` helper manages test isolation:

```swift
class ServerFixture {
    let serverURL: URL  // e.g., http://100.123.20.63:8400

    /// Verify server is reachable before test suite runs
    func waitForServer(timeout: TimeInterval = 10) async throws

    /// Create a fresh session for test isolation
    func createSession(title: String?) async throws -> String  // returns session ID

    /// Delete a session (cleanup)
    func deleteSession(id: String) async throws

    /// Get current model/thinking state (for assertions)
    func getActiveModel() async throws -> String
    func getThinkingLevel() async throws -> String

    /// Generate a pairing code (for pairing tests)
    func generatePairingCode() async throws -> String

    /// Send a message via API (to set up test state)
    func sendMessage(sessionId: String, message: String) async throws

    /// Check auth status
    func checkHealth() async throws -> Bool
}
```

### 1.4 Timeout Constants

```swift
enum TestTimeout {
    static let serverReady: TimeInterval = 15
    static let elementAppear: TimeInterval = 5
    static let sseStream: TimeInterval = 30     // LLM responses can take time
    static let pairingExchange: TimeInterval = 10
    static let navigation: TimeInterval = 3
    static let animation: TimeInterval = 1
    static let reconnection: TimeInterval = 20
}
```

---

## 2. Test Suites

### 2.1 Pairing Flow (`PairingFlowTests.swift`)

The pairing flow is the app's first-run experience. Tests validate the complete onboarding from launch to connected state.

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testFirstLaunchShowsOnboarding` | Fresh install shows pairing screen | Launch app with no stored credentials | Onboarding view visible, server URL field present, no session list |
| `testServerURLValidation` | Invalid URLs show error | Enter `not-a-url`, tap Connect | Error message appears, Connect button stays enabled |
| `testServerURLHealthCheck` | Valid URL triggers health check | Enter server URL, tap Connect | Loading indicator → success indicator → advance to code screen |
| `testServerUnreachable` | Unreachable server shows error | Enter `http://192.168.99.99:8400`, tap Connect | Error: "Cannot reach server" with retry option |
| `testPairingCodeEntry` | Code entry accepts formatted input | On code screen, type `A7K-M2X` | Code field shows formatted code, Pair button enabled |
| `testPairingCodeExchange` | Valid code connects successfully | Generate code via fixture, enter in app, tap Pair | Success animation → session list appears → connection indicator green |
| `testInvalidPairingCode` | Wrong code shows error | Enter `ZZZ-999`, tap Pair | Error: "Invalid pairing code", field clears, can retry |
| `testExpiredPairingCode` | Expired code shows specific error | Wait 5+ min (or use short-TTL test code), tap Pair | Error: "Code expired", prompt to generate new code |
| `testPairedTokenPersists` | Token survives app restart | Complete pairing, quit app, relaunch | App launches directly to session list (skips onboarding) |
| `testManualTokenEntry` | Bearer token can be entered manually | In settings/onboarding, enter raw `fawx_pat_...` token | Connection established, session list loads |

### 2.2 Session Flow (`SessionFlowTests.swift`)

Session lifecycle: create, list, select, delete, clear.

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testEmptySessionList` | No sessions shows empty state | Launch with no sessions | Empty state view with "Start a conversation" prompt visible |
| `testCreateNewSession` | New session via button | Tap new session button | New session appears in sidebar, chat view opens with empty state |
| `testCreateSessionByTyping` | First message creates session | Type message in empty state, send | Session created, message appears, sidebar shows new session |
| `testSessionListPopulates` | Sessions load on launch | Pre-seed 3 sessions via fixture | All 3 sessions visible in sidebar/list |
| `testSessionListDateGrouping` | Sessions grouped by date | Pre-seed sessions across multiple days | "Today", "Yesterday", "Previous 7 Days" section headers |
| `testSelectSession` | Tapping session loads history | Pre-seed session with messages, tap it | Chat view shows message history, input bar focused |
| `testDeleteSession` | Delete removes session | Pre-seed session, swipe-to-delete (iOS) or right-click (macOS) | Confirmation dialog → session removed from list |
| `testDeleteActiveSession` | Deleting active session clears chat | Delete currently-selected session | Chat view returns to empty state |
| `testClearSessionHistory` | Clear removes messages but keeps session | Open session with messages, clear history | Messages gone, session still in sidebar, empty chat state |
| `testSessionPreview` | Sidebar shows message preview | Send a message in session | Session row shows truncated last message |
| `testSessionOrderByRecency` | Most recent session is first | Send message in older session | That session moves to top of list |

### 2.3 Chat Flow (`ChatFlowTests.swift`)

Core chat: send messages, receive SSE streaming responses, markdown rendering.

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testSendPlainMessage` | Basic message send | Type "Hello", tap send | User message bubble appears, streaming indicator shows, assistant response streams in |
| `testSendWithEnterKey` | Enter key sends (macOS) | Type message, press Enter | Message sends (same as tap send) |
| `testShiftEnterNewline` | Shift+Enter adds newline (macOS) | Type text, Shift+Enter, type more | Multiline message in input, not sent |
| `testStreamingResponse` | SSE tokens stream incrementally | Send message | Streaming indicator visible → text appears incrementally → indicator disappears on completion |
| `testStreamingCancel` | Stop button cancels stream | Send message, tap stop while streaming | Streaming stops, partial response visible, input re-enabled |
| `testMarkdownRendering` | Markdown renders correctly | Trigger response with code block | Code block rendered with monospace font, proper formatting |
| `testToolCallCard` | Tool calls show as cards | Trigger tool-using response (e.g., "search for...") | Tool call card appears with tool name, status transitions (running → complete) |
| `testMultipleToolCalls` | Multiple tool calls render | Trigger multi-tool response | Each tool call gets its own card, shown in order |
| `testLongConversation` | Scroll behavior in long chats | Send 10+ message exchanges | Auto-scroll to bottom on new message, can scroll up to see history |
| `testScrollToBottomButton` | Quick-scroll after scrolling up | Scroll up in long conversation | "Jump to bottom" button appears, tap it → scrolls to latest message |
| `testQueuedMessage` | Message queued during streaming | Send message, type and send another while streaming | Queued chip shows second message, sent automatically after first response completes |
| `testEmptyMessageNotSent` | Cannot send empty message | Tap send with empty input | Nothing happens, send button disabled |
| `testMessagePersistence` | Messages survive session switch | Send message, switch session, switch back | Original messages still visible |
| `testInputBarRetainsText` | Unsent text stays on session switch | Type text (don't send), switch session, switch back | Draft text preserved in input bar |
| `testSSEPingHandling` | SSE pings don't produce visible output | Server sends `: ping\n\n` during response | No visual artifact, streaming indicator stays active |

### 2.4 Model & Thinking Switch (`ModelSwitchTests.swift`)

Model and thinking level controls — global server setting, not per-session.

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testModelPickerPopulates` | Model picker shows available models | Open model picker in input bar | Dropdown shows models from `GET /v1/models`, current model highlighted |
| `testSwitchModel` | Model change updates server | Select different model from picker | `PUT /v1/model` called, model badge updates, status bar updates |
| `testThinkingPickerSegments` | Thinking picker shows 4 levels | Open thinking picker | Shows Off / Low / Adaptive / High segments |
| `testSwitchThinking` | Thinking change updates server | Select different thinking level | `PUT /v1/thinking` called, thinking badge updates |
| `testModelPickerDisabledDuringStreaming` | Can't change model while streaming | Start streaming, try to open model picker | Picker is disabled/grayed out |
| `testThinkingPickerDisabledDuringStreaming` | Can't change thinking while streaming | Start streaming, try to change thinking | Picker is disabled |
| `testModelPersistsAcrossSessionSwitch` | Model stays after switching sessions | Change model, switch session | Model badge still shows new model |
| `testModelBadgeShowsCurrentModel` | Input bar badge reflects server state | Load app | Model badge matches `GET /v1/models` active model |
| `testThinkingBadgeShowsCurrentLevel` | Thinking badge reflects server state | Load app | Thinking badge matches `GET /v1/thinking` level |
| `testSettingsModelPicker` | Settings model picker works | Open Settings → Model & Thinking → change model | Same effect as input bar picker |
| `testModelUnavailableState` | Graceful handling when models fail | (Simulate `/v1/models` failure) | Model picker shows "Unavailable", disabled |

### 2.5 Skills Browser (`SkillsBrowserTests.swift`)

Skills list view — read-only in V1.

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testSkillsListLoads` | Skills page shows loaded skills | Navigate to Skills view | List of skills visible with names and tool counts |
| `testSkillCardContent` | Each skill card shows details | View skill card | Skill name, description (if available), tool list |
| `testEmptySkillsList` | No skills shows empty state | (Server with no skills loaded) | Empty state: "No skills loaded" with guidance |
| `testSkillsRefresh` | Pull-to-refresh updates skills | Pull to refresh (iOS) or refresh action (macOS) | Skills list re-fetches from server |

### 2.6 Settings (`SettingsTests.swift`)

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testSettingsOpens` | Settings screen accessible | ⌘, on macOS / tap Settings tab on iOS | Settings view visible with all sections |
| `testConnectionSection` | Connection settings display | Open Connection section | Server URL, token status, connection indicator visible |
| `testTestConnection` | Test Connection button works | Tap "Test Connection" | Loading → success/failure indicator |
| `testThemeSwitching` | Theme picker changes appearance | Switch between Light / Dark / System | App appearance changes immediately |
| `testAuthStatusSection` | Auth providers listed | Open Auth Status section | Provider list with names and status |
| `testAboutSection` | About shows version info | Open About section | App version and server version visible |
| `testServerURLChange` | Changing server URL reconnects | Enter new URL in settings | Health check runs, connection status updates |
| `testTokenRevocation` | Can enter new token | Clear token, enter new one | Re-authenticates with new token |

### 2.7 Error States (`ErrorStateTests.swift`)

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testOfflineBanner` | Disconnected state shown | Launch with server unreachable | Offline banner visible, session list disabled or shows cached |
| `testAuthFailure` | Invalid token handled | Launch with bad token | Auth error banner, prompt to re-pair or enter token |
| `testServerErrorOnSend` | Server error during send | (Simulate 500 on message send) | Error message in chat, input re-enabled, can retry |
| `testSSEStreamError` | Stream error during response | (Simulate stream disconnect) | Error indicator in chat, partial response preserved |
| `testSessionNotFound` | Deleted session handled | Select session deleted server-side | Error message, redirect to session list |
| `testNetworkTimeout` | Slow network handled | (Simulate high latency) | Loading indicators visible, eventual timeout message |
| `testReconnectionAfterDrop` | Auto-reconnect on network restore | Disconnect then reconnect network | Banner shows "Reconnecting..." → "Connected", state refreshes |

---

## 3. Platform-Specific Tests

### 3.1 macOS (`MacOSTests.swift`)

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testSidebarNavigation` | Sidebar shows sessions | Launch app | NavigationSplitView with sidebar visible |
| `testSidebarCollapse` | Sidebar can be collapsed | Toggle sidebar | Sidebar collapses/expands |
| `testKeyboardShortcutNewSession` | ⌘N creates new session | Press ⌘N | New session created and selected |
| `testKeyboardShortcutSettings` | ⌘, opens settings | Press ⌘, | Settings window opens |
| `testKeyboardShortcutDeleteSession` | ⌘⌫ deletes session | Select session, press ⌘⌫ | Confirmation → session deleted |
| `testRightClickContextMenu` | Right-click session shows options | Right-click session in sidebar | Context menu: Rename, Delete, Clear History |
| `testWindowMinimumSize` | Window respects min size | Resize window very small | Window stops at minimum dimensions (800×500) |
| `testFocusInputOnSessionSelect` | Input bar focuses automatically | Click session in sidebar | Message input field has keyboard focus |

### 3.2 iOS (`IOSTests.swift`)

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testTabBarNavigation` | Tab bar with all tabs | Launch app | Chat, Skills, Settings tabs visible |
| `testSwipeToDelete` | Swipe to delete session | Swipe left on session row | Delete action appears, confirm deletes |
| `testPullToRefresh` | Pull to refresh session list | Pull down on session list | Refresh indicator, list updates |
| `testKeyboardAvoidance` | Input bar above keyboard | Tap message input | Keyboard appears, input bar moves above it |
| `testKeyboardDismiss` | Keyboard dismisses on scroll | Open keyboard, scroll up in chat | Keyboard dismisses |
| `testSafeAreaRespected` | Content within safe area | View on device with notch/island | No content clipped by system UI |
| `testLandscapeLayout` | Landscape mode works | Rotate to landscape | Layout adapts, sidebar may split (iPad) |
| `testBackNavigation` | Back button in chat | Open session, tap back | Returns to session list |

---

## 4. Resilience Tests

### 4.1 Reconnection (`ReconnectionTests.swift`)

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testReconnectAfterServerRestart` | App recovers from server restart | (Restart server during session) | Banner shows disconnect → reconnect → state restored |
| `testReconnectRefreshesState` | State refreshed on reconnect | Change model on server while disconnected, reconnect | App shows updated model |
| `testHeartbeatDetectsDisconnect` | Periodic health check catches drop | (Server goes down between heartbeats) | Within heartbeat interval, banner shows disconnected |

### 4.2 Stream Interruption (`StreamInterruptTests.swift`)

| Test | Description | Steps | Assertions |
|------|-------------|-------|------------|
| `testCancelMidStream` | User cancel during streaming | Tap stop button mid-response | Partial text preserved, input re-enabled, no crash |
| `testNetworkDropMidStream` | Network dies during stream | (Simulate disconnect during SSE) | Error shown, partial response preserved, can retry |
| `testServerErrorMidStream` | Server error during stream | (Simulate engine_error SSE event) | Error card in chat, can send new message |
| `testRapidSendCancel` | Rapid send-cancel cycles | Send, cancel, send, cancel rapidly | No crash, no orphaned state, UI stays responsive |

---

## 5. Test Data & Fixtures

### 5.1 Server-Side Setup

Before the test suite runs, the server should be in a known state:

```
1. Server is running: GET /health returns 200
2. Auth is configured: bearer token available
3. No leftover sessions (or test creates its own)
4. A known model is active (e.g., claude-sonnet-4-6)
5. Thinking level is known (e.g., adaptive)
```

### 5.2 Test Isolation

Each test case should:
- Create its own session(s) via the API fixture (not share sessions between tests)
- Clean up created sessions in `tearDown`
- Not depend on execution order
- Not depend on specific LLM output content (assert structure, not exact text)

### 5.3 Assertion Patterns

```swift
// Wait for element with timeout
func waitForElement(_ id: String, timeout: TimeInterval = TestTimeout.elementAppear) {
    let element = app.descendants(matching: .any)[id]
    XCTAssertTrue(element.waitForExistence(timeout: timeout), "\(id) did not appear within \(timeout)s")
}

// Wait for text content (streaming responses)
func waitForTextInElement(_ id: String, containing text: String, timeout: TimeInterval = TestTimeout.sseStream) {
    let element = app.descendants(matching: .any)[id]
    let predicate = NSPredicate(format: "label CONTAINS %@", text)
    let expectation = XCTNSPredicateExpectation(predicate: predicate, object: element)
    let result = XCTWaiter.wait(for: [expectation], timeout: timeout)
    XCTAssertEqual(result, .completed, "Expected text '\(text)' not found in \(id)")
}

// Assert element not present
func assertNotExists(_ id: String) {
    let element = app.descendants(matching: .any)[id]
    XCTAssertFalse(element.exists, "\(id) should not exist")
}

// Assert streaming indicator lifecycle
func assertStreamingLifecycle(after sendAction: () -> Void) {
    sendAction()
    waitForElement(AID.streamingIndicator)
    // Wait for streaming to complete (indicator disappears)
    let indicator = app.descendants(matching: .any)[AID.streamingIndicator]
    let gone = NSPredicate(format: "exists == false")
    let expectation = XCTNSPredicateExpectation(predicate: gone, object: indicator)
    XCTWaiter.wait(for: [expectation], timeout: TestTimeout.sseStream)
}
```

### 5.4 LLM Response Strategy

E2E tests hit a real LLM, so response content is non-deterministic. Tests should:
- **Assert structure, not content**: "assistant message bubble exists" not "response contains 'Hello'"
- **Use simple prompts**: "Say hello" or "What is 2+2?" for predictable-ish responses
- **Assert timing**: streaming indicator appears, then disappears within timeout
- **Assert tool calls by name**: if testing tool rendering, use a prompt that reliably triggers a known tool
- For tool call tests, use prompts like "Use the brave_search tool to search for 'test'" — explicit tool requests are more deterministic

---

## 6. CI / Automation Notes

### 6.1 Running Locally

```bash
# macOS tests
xcodebuild test \
  -project Fawx.xcodeproj \
  -scheme FawxUITests \
  -destination 'platform=macOS' \
  FAWX_SERVER_URL=http://100.123.20.63:8400 \
  FAWX_BEARER_TOKEN=fawx_pat_...

# iOS tests (simulator)
xcodebuild test \
  -project Fawx.xcodeproj \
  -scheme FawxUITests \
  -destination 'platform=iOS Simulator,name=iPhone 16' \
  FAWX_SERVER_URL=http://100.123.20.63:8400 \
  FAWX_BEARER_TOKEN=fawx_pat_...
```

### 6.2 Environment Variables

Tests read server config from environment or `ProcessInfo`:

```swift
let serverURL = ProcessInfo.processInfo.environment["FAWX_SERVER_URL"]
    ?? "http://100.123.20.63:8400"
let bearerToken = ProcessInfo.processInfo.environment["FAWX_BEARER_TOKEN"]
    ?? ""  // fails tests if not set — intentional
```

### 6.3 Test Ordering

Tests are independent and can run in any order. However, for efficiency:
1. `PairingFlowTests` first (validates connectivity)
2. `SessionFlowTests` (validates CRUD)
3. `ChatFlowTests` (validates core UX)
4. `ModelSwitchTests`, `SkillsBrowserTests`, `SettingsTests` (feature tests)
5. `ErrorStateTests`, `ReconnectionTests`, `StreamInterruptTests` (resilience — last, may be slow)
6. `MacOSTests` / `IOSTests` (platform-specific, run only on matching platform)

### 6.4 Expected Test Count

| Suite | Tests | Critical |
|-------|-------|----------|
| Pairing Flow | 10 | ✅ Must pass |
| Session Flow | 11 | ✅ Must pass |
| Chat Flow | 15 | ✅ Must pass |
| Model Switch | 11 | ✅ Must pass |
| Skills Browser | 4 | Should pass |
| Settings | 8 | Should pass |
| Error States | 7 | Should pass |
| macOS Platform | 8 | macOS only |
| iOS Platform | 8 | iOS only |
| Reconnection | 3 | Best effort |
| Stream Interruption | 4 | Best effort |
| **Total** | **89** | |

---

## 7. Accessibility Identifier Contract

For these tests to work, the app MUST have accessibility identifiers on all interactive and assertable elements. The identifiers listed in Section 1.2 (`AccessibilityIDs.swift`) are the contract between the app code and the test suite.

**Rule:** Every new UI element that a test needs to find MUST have an `accessibilityIdentifier` set in the SwiftUI view. No test should rely on text matching for element location — text changes, identifiers don't.

```swift
// In the app code:
TextField("Server URL", text: $serverURL)
    .accessibilityIdentifier(AID.serverURLField)

Button("Connect") { ... }
    .accessibilityIdentifier(AID.connectButton)

ForEach(sessions) { session in
    SessionRow(session: session)
        .accessibilityIdentifier("\(AID.sessionRow)_\(session.id)")
}
```

---

*This spec defines the what. Codex implements the how.*
