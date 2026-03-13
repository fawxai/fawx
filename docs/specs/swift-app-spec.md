# Fawx Native App — Swift UI Specification

**Status:** APPROVED (v5 — R1–R7, 42+ findings resolved, Codex R7 APPROVE)
**Target:** macOS (primary) + iOS (secondary, shared codebase)
**Framework:** SwiftUI multiplatform
**Design Reference:** Codex macOS app (screenshots on file)
**API Surface:** 21 HTTP endpoints on `fawx serve --http` (port 8400)

---

## 1. Product Vision

Fawx is a TUI-first agentic engine. This native app is its first graphical shell — a daily-driver interface that proves the GUI↔API architecture works better than Telegram, and establishes the design language for a future native OS.

### What This App Is
- A **client** to a running Fawx server (`fawx serve --http`)
- Chat-centric with a dashboard secondary view
- Power-user tool, not a consumer chatbot wrapper
- The foundation for all future Fawx GUI surfaces (web, mobile, OS)

### What This App Is NOT
- Not a standalone LLM client (requires a Fawx server)
- Not a code editor or IDE (Fawx handles execution; app shows results)
- Not a Telegram replacement (it's a replacement for the *need* for Telegram)

### Known V1 Limitations (Documented)
- **Model/thinking is global, not per-session.** `PUT /v1/model` and `PUT /v1/thinking` change the server-wide model/thinking level. Switching model in Session A affects Session B. The UI should make this clear (e.g., status bar shows "Server model" not "Session model"). Per-session model is a V2 backend feature.
- **No pagination on session list.** Server returns up to 50 sessions (hardcoded `truncate(limit.unwrap_or(50))`). For V1 this is acceptable. V2 adds cursor-based pagination.
- **No compaction awareness.** When the server compacts a conversation (drops old messages), the app's cached history becomes stale. V1 mitigation: re-fetch full history when switching back to a session. V2: SSE `compaction` event to trigger client-side refresh.
- **No mid-stream message injection.** Queue is client-side only in V1. See Queued message chip section for V2 evolution.

### V1 Scope
- Connect to a Fawx server over the network (Tailscale or LAN)
- Full conversation management (create, list, chat, delete, clear)
- SSE streaming responses with markdown rendering
- Model and thinking level switching
- Skills browser
- Auth status overview
- Light/dark/system theme
- macOS: sidebar navigation, keyboard shortcuts
- iOS: tab bar navigation, swipe gestures

### V2 (Future — Out of Scope for This Spec)
- Multi-window on macOS (pop-out conversations)
- Fleet dashboard (node status, task dispatch)
- Experiment monitor (scores, chains, tournament view)
- Journal/memory browser
- Image/file attachments in messages
- Push notifications
- Auth credential setup flow (add/remove providers)

---

## 2. Architecture

### 2.1 Project Structure

```
Fawx/
├── Fawx.xcodeproj
├── Shared/                    ← Cross-platform code (~85%)
│   ├── FawxApp.swift          ← App entry point
│   ├── Models/
│   │   ├── Session.swift      ← Conversation session model
│   │   ├── Message.swift      ← Chat message model
│   │   ├── ModelInfo.swift    ← LLM model metadata
│   │   ├── ThinkingLevel.swift
│   │   ├── Skill.swift
│   │   ├── AuthProvider.swift
│   │   └── ServerStatus.swift
│   ├── Networking/
│   │   ├── FawxClient.swift   ← HTTP API client
│   │   ├── SSEStream.swift    ← Server-Sent Events streaming
│   │   ├── Endpoints.swift    ← Endpoint definitions
│   │   └── AuthToken.swift    ← Bearer token management
│   ├── ViewModels/
│   │   ├── ChatViewModel.swift
│   │   ├── SessionListViewModel.swift
│   │   ├── SettingsViewModel.swift
│   │   └── SkillsViewModel.swift
│   ├── Views/
│   │   ├── Chat/
│   │   │   ├── ChatView.swift
│   │   │   ├── MessageBubble.swift
│   │   │   ├── StreamingIndicator.swift
│   │   │   ├── InputBar.swift
│   │   │   └── MarkdownRenderer.swift
│   │   ├── Sessions/
│   │   │   ├── SessionListView.swift
│   │   │   └── SessionRow.swift
│   │   ├── Skills/
│   │   │   ├── SkillsView.swift
│   │   │   └── SkillCard.swift
│   │   ├── Settings/
│   │   │   ├── SettingsView.swift
│   │   │   ├── ConnectionSettings.swift
│   │   │   ├── AppearanceSettings.swift
│   │   │   └── AuthStatusView.swift
│   │   └── Components/
│   │       ├── ModelPicker.swift
│   │       ├── ThinkingPicker.swift
│   │       └── StatusBar.swift
│   └── Theme/
│       ├── FawxTheme.swift    ← Colors, typography, spacing
│       └── Assets.xcassets
├── macOS/
│   ├── MainWindow.swift       ← NavigationSplitView layout
│   ├── SidebarView.swift
│   └── KeyboardShortcuts.swift
├── iOS/
│   ├── MainTabView.swift      ← TabView layout
│   └── NavigationAdapter.swift
└── Tests/
```

### 2.2 Networking Layer

All communication with the Fawx server goes through `FawxClient`, a thin async/await wrapper over URLSession.

**Connection model:**
- Server URL stored in UserDefaults (e.g., `http://100.123.20.63:8400`)
- Bearer token for authentication (stored in Keychain)
- Connection health check via `GET /health` on app launch + periodic heartbeat
- Graceful offline state when server unreachable

**SSE Streaming:**
- `POST /v1/sessions/{id}/messages` with `Accept: text/event-stream`
- Parse `data:` lines incrementally, update message content in real-time
- Handle `event: done` for stream completion (contains full response text)
- Handle `event: engine_error` for recoverable errors and `event: error` for fatal errors
- Support cancel (drop the URLSession task)

### 2.3 Data Flow

```
FawxClient (HTTP/SSE) → ViewModel (@Observable) → SwiftUI View
                                ↑
                          User interaction
```
Uses Swift Observation framework (`@Observable` macro, iOS 17+ / macOS 14+), NOT the older `@Published` / `ObservableObject` pattern.

- **No local database in V1.** All state lives on the Fawx server. The app is a pure client.
- ViewModels hold transient UI state (selected session, draft message, streaming state)
- Session list and history fetched on demand and cached in memory
- Pull-to-refresh on iOS, periodic refresh on macOS

---

## 3. Design Language

### 3.1 Theme

**Aesthetic:** Minimal, functional, power-tool feel. Not playful, not corporate. Think terminal-meets-native.

**Modes:** Light / Dark / System (follows OS preference)

**Color Palette:**

| Token | Dark Mode | Light Mode | Usage |
|-------|-----------|------------|-------|
| `background` | `#1A1A1A` | `#FFFFFF` | Main background |
| `surface` | `#242424` | `#F5F5F5` | Cards, sidebar, input bar |
| `surfaceHover` | `#2E2E2E` | `#EBEBEB` | Hover states |
| `surfaceActive` | `#383838` | `#E0E0E0` | Selected/active items |
| `text` | `#E8E8E8` | `#1A1A1A` | Primary text |
| `textSecondary` | `#999999` | `#666666` | Secondary/muted text |
| `accent` | `#E8711A` | `#D45E14` | Fawx orange — matched to logo |
| `accentSubtle` | `#E8711A20` | `#D45E1415` | Accent backgrounds |
| `success` | `#4ADE80` | `#22C55E` | Online, connected, passed |
| `warning` | `#FBBF24` | `#D97706` | Warnings, caution |
| `error` | `#F87171` | `#DC2626` | Errors, disconnected |
| `border` | `#333333` | `#E5E5E5` | Dividers, borders |
| `code` | `#2D2D2D` | `#F0F0F0` | Code block background |

**Typography:**

| Element | Font | Size | Weight |
|---------|------|------|--------|
| Sidebar title | System (SF Pro) | 13pt | Semibold |
| Sidebar item | System | 13pt | Regular |
| Chat message | System | 14pt | Regular |
| Code blocks | SF Mono / Menlo | 13pt | Regular |
| Input bar | System | 14pt | Regular |
| Status bar | System | 11pt | Regular |
| Heading (H1) | System | 18pt | Bold |
| Heading (H2) | System | 16pt | Semibold |

**Spacing & Layout:**

| Token | Value |
|-------|-------|
| `paddingXS` | 4pt |
| `paddingSM` | 8pt |
| `paddingMD` | 12pt |
| `paddingLG` | 16pt |
| `paddingXL` | 24pt |
| `cornerRadius` | 8pt |
| `cornerRadiusSM` | 4pt |
| `sidebarWidth` | 260pt (macOS) |
| `maxMessageWidth` | 720pt |
| `inputBarHeight` | min 48pt, max 200pt (auto-grow) |

---

## 4. Navigation Structure

### 4.1 macOS — NavigationSplitView

```
┌──────────────────────────────────────────────────────┐
│ ● ● ●              Fawx              ■ □ ▢           │  ← Window chrome
├────────────┬─────────────────────────────────────────┤
│            │                                         │
│  Sessions  │          Chat / Content Area            │
│  ────────  │                                         │
│  ▸ New     │   Messages...                           │
│            │                                         │
│  Today     │                                         │
│   Session1 │                                         │
│   Session2 │                                         │
│            │                                         │
│  Yesterday │                                         │
│   Session3 │                                         │
│            │                                         │
│  ────────  │                                         │
│  Skills    │  ┌─────────────────────────────────┐    │
│  Settings  │  │ Input bar        [Model] [Think]│    │
│            │  └─────────────────────────────────┘    │
├────────────┴─────────────────────────────────────────┤
│ ● Connected │ Power User │ claude-sonnet-4-6 │ ██░░ 62% ctx │
└──────────────────────────────────────────────────────┘
```

**Sidebar sections:**
1. **New Session** button (top, prominent)
2. **Session list** — grouped by date (Today, Yesterday, Previous 7 Days, Older)
3. **Navigation items** (bottom, pinned):
   - Skills
   - Settings

**Selection model:**
The sidebar content pane uses a single selection binding for the detail area:
```swift
enum SidebarSelection: Hashable, RawRepresentable {
    case session(String)  // session key
    case skills

    // RawRepresentable conformance so @SceneStorage can persist this enum.
    // SceneStorage only accepts built-in types or RawRepresentable with
    // Int/String raw value. Associated-value enums are NOT automatically
    // RawRepresentable, so we provide an explicit String mapping.
    //
    // Prefix-encoding guarantees round-trip correctness for ALL session keys,
    // including adversarial ones. No sentinel collision is possible because
    // every case has a distinct prefix.
    private static let sessionPrefix = "session:"
    private static let skillsLiteral = "nav:skills"

    init?(rawValue: String) {
        if rawValue == Self.skillsLiteral {
            self = .skills
        } else if rawValue.hasPrefix(Self.sessionPrefix) {
            self = .session(String(rawValue.dropFirst(Self.sessionPrefix.count)))
        } else {
            return nil  // unknown format → treat as no selection
        }
    }

    var rawValue: String {
        switch self {
        case .skills: return Self.skillsLiteral
        case .session(let key): return Self.sessionPrefix + key
        }
    }
}
// Settings is NOT in this enum — it opens a separate Settings window via ⌘,
```
- `NavigationSplitView` binds to `@SceneStorage("sidebar_selection") var selection: SidebarSelection?`
- This compiles because `SidebarSelection` now conforms to `RawRepresentable` with `String` raw value, which `@SceneStorage` accepts.
- `selection == nil` → detail pane shows New Session / Empty State. There is no separate `.newSession` case — nil IS the new-session state.
- The "New Session" sidebar button sets `selection = nil`.
- **On delete selected session:** Set `selection = nil`.
- **Settings sidebar item (macOS):** Tapping it calls `NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)` or equivalent to open the `Settings { }` scene. It does NOT set `selection` — the sidebar selection stays on the current session/skills while the Settings window opens separately.
- **Sidebar collapse (macOS):** Standard `NavigationSplitView` collapse via toolbar button. Detail pane takes full width. Selection persists across launches via `@SceneStorage` (works because `SidebarSelection` is `RawRepresentable`).
- **iPad regular width:** Same `NavigationSplitView` as macOS. Sidebar is always visible in regular horizontal size class. In compact size class (iPhone, iPad slide-over), sidebar becomes a navigation stack.

**Keyboard shortcuts:**
| Shortcut | Action | Implementation |
|----------|--------|---------------|
| `⌘N` | New session | SwiftUI `Commands { }` — `CommandGroup(after: .newItem)` |
| `⌘⇧O` | Quick session switcher (see below) | SwiftUI `Commands { }` — custom `CommandMenu("Navigate")` |
| `⌘,` | Open Settings | Automatic from `Settings { }` scene (SwiftUI provides this free) |
| `⌘⏎` | Send message | `.keyboardShortcut(.return, modifiers: .command)` on Send button |
| `⌘⇧⌫` | Clear session history | SwiftUI `Commands { }` — custom `CommandMenu("Session")` |
| `⌘1-9` | Switch to nth session | SwiftUI `Commands { }` — dynamic `CommandMenu("Navigate")` items |
| `Esc` | Context-dependent (see below) | `.keyboardShortcut(.escape)` on views with priority chain |
| `⌘/` | Focus input bar | SwiftUI `Commands { }` — custom `CommandMenu("Navigate")` |

**Implementation rules:**
- **App-level actions** (`⌘N`, `⌘⇧O`, `⌘1-9`, `⌘⇧⌫`, `⌘/`) go in SwiftUI `Commands { }` blocks in the `App` body. NOT `.keyboardShortcut()` on views. This is how macOS menu bar items + shortcuts are properly wired.
- **View-level actions** (`⌘⏎`, `Esc`) use `.keyboardShortcut()` on the specific view/button.
- `⌘,` is automatic — SwiftUI creates it when you declare a `Settings { }` scene.
- **`⌘⇧O`** (Shift-Command-O) mirrors Xcode's "Open Quickly" pattern. No conflict with standard macOS shortcuts (`⌘O` = Open File, which we don't use).

**Quick session switcher (`⌘⇧O`):**
- Presented as a **sheet** (`.sheet` modifier), NOT a popover or custom overlay. Sheets have well-defined Esc dismissal behavior on both macOS and iOS.
- Contains a search field (auto-focused) + filtered session list.
- Typing filters sessions by label/key. Arrow keys navigate. Return selects. Esc dismisses.
- On selection: set `SidebarSelection.session(key)` and dismiss the sheet.

**`Esc` priority chain** (evaluated in order):
1. If a sheet (including session switcher) / popover / menu is open → dismiss it (handled by SwiftUI, highest priority)
2. If an SSE stream is active → cancel the stream
3. If input bar is focused → blur the input bar
4. Otherwise → no-op

### 4.2 iOS — TabView

```
┌──────────────────────────────┐
│ Sessions              ✏️ New │  ← Navigation bar
├──────────────────────────────┤
│                              │
│  Search sessions...          │
│                              │
│  Today                       │
│  ┌──────────────────────┐    │
│  │ Session 1        3m  │    │
│  │ Last message prev... │    │
│  └──────────────────────┘    │
│  ┌──────────────────────┐    │
│  │ Session 2       12m  │    │
│  │ Analyzing the cod... │    │
│  └──────────────────────┘    │
│                              │
├──────────────────────────────┤
│  💬 Chat  |  🧩 Skills  |  ⚙️ │  ← Tab bar
└──────────────────────────────┘
```

**Tab bar items:**
1. **Chat** — Session list → tap to open conversation
2. **Skills** — Installed skills browser
3. **Settings** — App and server configuration

---

## 5. Screen Specifications

### 5.0 First-Run Onboarding

**When:** App launches with no stored server URL (first ever launch, or after reset).

**Flow:**
```
┌──────────────────────────────────────┐
│                                      │
│          🦊 Welcome to Fawx          │
│                                      │
│   Connect to your Fawx server to     │
│   get started.                       │
│                                      │
│   Server URL                         │
│   ┌────────────────────────────────┐ │
│   │ http://100.123.20.63:8400      │ │
│   └────────────────────────────────┘ │
│                                      │
│   Bearer Token                       │
│   ┌────────────────────────────────┐ │
│   │ ●●●●●●●●●●●●●●●●             │ │
│   └────────────────────────────────┘ │
│   Token is generated by `fawx setup` │
│   and stored in your Fawx config.    │
│                                      │
│   ┌────────────────────────────────┐ │
│   │      Test Connection           │ │
│   └────────────────────────────────┘ │
│                                      │
│   ● Connected — claude-sonnet-4-6    │
│                                      │
│   ┌────────────────────────────────┐ │
│   │        Continue →              │ │
│   └────────────────────────────────┘ │
│                                      │
└──────────────────────────────────────┘
```

**Steps:**
1. **Server URL** — text field, placeholder shows example URL format. Validated on "Test Connection".
2. **Bearer Token** — secure text field with reveal toggle. The token is generated during `fawx setup` and stored in `~/.fawx/config.toml` under `[http] bearer_token`. User copies it from there (or from the setup wizard output).
3. **Test Connection** — hits `GET /health` (unauthenticated endpoint) to verify server is reachable, then hits `GET /v1/models` (authenticated) to verify the token works. Shows result inline:
   - ✅ "Connected — {model_name}" (both checks pass)
   - ⚠️ "Server reachable but authentication failed" (health OK, models 401)
   - ❌ "Cannot reach server at {url}" (health fails)
4. **Continue** — enabled only after successful connection. Saves URL + token and transitions to main app.

**After onboarding:** App opens to New Session / Empty State (5.1).

**Re-entry:** Settings → Connection allows changing URL/token later. Same validation flow.

**Bearer token context:** `fawx setup` auto-generates a random 32-byte hex token and stores it in both the config file and the encrypted credential store. The API enforces bearer auth on all endpoints except `/health` and `/telegram/webhook`. This is not optional — the app cannot function without a valid token.

### 5.1 New Session / Empty State

**When:** App opens with no session selected, or user taps "New Session"

**Layout:**
- Centered content area
- Fawx logo/icon (subtle, small)
- Tagline: "What are you working on?" (or configurable greeting)
- Input bar at bottom (same as chat input)
- Optional: suggestion chips for common actions

**API:** `POST /v1/sessions` on first message send (lazy creation — don't create empty sessions)

**Behavior:**
- User types message → create session via API → immediately begin SSE stream for response
- Session appears in sidebar once created
- Focus is on the input bar by default

**First-message failure handling:**
The new session flow has two sequential API calls: `POST /v1/sessions` (create) then `POST /v1/sessions/{id}/messages` (send + stream). Failures at each stage:

1. **Session creation fails (POST /v1/sessions returns error):** Show error inline ("Failed to create session — Retry?"). Preserve the user's draft text in the input bar. Do NOT navigate away from the empty state.
2. **Session created but message send fails (POST /v1/sessions/{id}/messages returns error or stream drops immediately):** The empty session now exists on the server. The client must:
   - Add the session to the sidebar (it exists server-side).
   - Show the user's message as a sent message (it was the intent).
   - Show an error card: "Failed to send — Retry?" with a button that resends the same message.
   - Do NOT auto-delete the empty session. The user may retry, or they can manually delete it.
3. **Session created, stream starts, then drops mid-response:** Follow the standard stream drop recovery protocol (Section 8.3).

### 5.2 Chat View

**The primary screen. This is where users spend 90% of their time.**

**Layout (top to bottom):**
1. **Header bar** — Session title (editable? V2), model badge, thinking badge, connection indicator
2. **Message list** — Scrollable, auto-scroll to bottom on new content, scroll-to-bottom button when scrolled up
3. **Input bar** — Multi-line text input with controls

**Message types:**

| Type | Rendering |
|------|-----------|
| User message | Right-aligned (or left with user icon), plain text or markdown |
| Assistant text | Left-aligned, full markdown rendering (headings, lists, code blocks, bold, italic, links) |
| Assistant streaming | Same as above but tokens appear incrementally, blinking cursor at end |
| Tool call (live) | Collapsible card: tool name as header, arguments as code, result as expandable body. Built from `tool_call_start` → `tool_call_complete` → `tool_result` SSE events during streaming. |
| Tool call (history) | Tool calls in loaded history appear as plain text in assistant messages (the API does not structure them separately). Render as-is — no collapsible cards for historical tool calls in V1. |
| Error | Red-tinted card with error category and message |
| System | Centered, muted text (session created, model changed, etc.) |

**Markdown rendering must support:**
- Headings (H1-H4)
- Bold, italic, strikethrough
- Inline code and fenced code blocks (monochrome in V1 — see code blocks section below)
- Bullet and numbered lists (nested)
- Links (tappable, open in browser)
- Tables (basic)
- Blockquotes
- Horizontal rules

**Code blocks:**
- Monospace font (SF Mono / Menlo)
- Language label in top-right corner
- Copy button on hover (macOS) or long-press (iOS)
- **Syntax highlighting: V1 uses monochrome code blocks (no highlighting).** MarkdownUI renders fenced code blocks with language labels and monospace font, but does NOT include a syntax highlighter. Adding one requires either a second dependency ([Splash](https://github.com/JohnSundell/Splash) for Swift-only, or [Highlightr](https://github.com/nicklama/Highlightr) for multi-language via highlight.js) or a custom implementation. **V1 ships without syntax highlighting.** Code blocks are monospace, dark background, language label, copy button — but all text is one color. Syntax highlighting is a V2 enhancement.
- Horizontal scroll for long lines, no wrapping

**Input bar:**
- Multi-line text field, auto-grows up to `inputBarHeight` max
- **Left controls:** attachment button (V2, disabled/hidden for V1)
- **Right controls:** Send button (accent color when text present, muted when empty)
- **Below input (inline or dropdown):**
  - Model picker — dropdown showing available models from `GET /v1/models`
  - Thinking level picker — dropdown (off, low, adaptive, high) from `GET /v1/thinking`
- `⌘⏎` to send (macOS), standard send button (iOS)
- `Esc` cancels active stream

**Input bar state machine (Queue pattern, inspired by Codex):**

The input bar changes behavior based on streaming state. In V1, the chip is always labeled "Queued" (queue-after-completion). V2 evolution is documented in the Queued message chip modes subsection below.

```
┌─────────────────────────────────────────────────────┐
│ IDLE STATE                                          │
│ [+]  [ Type a message...          ]  [Model] [Send] │
│                                       [Think]       │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ STREAMING STATE (input empty)                       │
│ [+]  [ ...                         ]  [Model] [■ Stop]│
│                                       [Think]       │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ STREAMING STATE (user typing)                       │
│ ┌─ Queued ─────────────────────── [×] ┐             │
│ │ "Actually, use a different approach" │             │
│ └─────────────────────────────────────┘             │
│ [+]  [ Actually, use a dif...  ]  [Model] [Send]   │
│                                    [Think]          │
└─────────────────────────────────────────────────────┘
```

- **Idle:** Normal input, accent-colored Send button when text present, muted when empty
- **Streaming + input empty:** Send button becomes **Stop** button (red/danger style). Tap cancels the active stream.
- **Streaming + user types:** Stop transitions to **Send**. Sending creates a **queued message chip** above the input bar — a dismissable banner showing the queued message. The queued message is delivered to the model as a follow-up after the current stream completes.
- **Queued message chip:** Shows queued text (truncated), "Queued" label, dismiss (×) button. Only one message can be queued at a time (sending another replaces it).
- After stream completes: if a message is queued, it auto-sends as the next user message.

**Queued message chip modes (V1 → V2 evolution):**

```
V1:  ┌─ Queued ─────────────────────────── [×] ┐
     │ "Use async/await instead of callbacks"   │
     └──────────────────────────────────────────┘
     (static label "Queued", always queues for after completion)

V2:  ┌─ Queue ▾ ────────────────────────── [×] ┐
     │ "Use async/await instead of callbacks"   │
     └──────────────────────────────────────────┘
              │ tap ▾ dropdown
              ├── Queue   (send after response completes)
              └── Steer   (redirect model mid-stream)
```

- **V1:** Chip shows "Queued" as a static label. Behavior is always queue-and-send-after-completion. Non-interactive mode selector.
- **V2:** Chip label becomes a tappable dropdown toggling between "Queue" and "Steer". "Queue" waits for completion then sends as next turn. "Steer" interrupts the active stream and injects the message as a mid-response redirect — the model receives it immediately and shifts its response. Requires backend support for mid-stream message injection (`POST /v1/sessions/{id}/messages` with `"mode": "steer"` while a stream is active).

**API requirements:**
- **V1:** `POST /v1/sessions/{id}/messages` sent during an active stream returns 409 (conflict) or queues server-side. Client handles queuing locally.
- **V2:** Same endpoint accepts optional `"mode": "steer"` field. When `mode=steer` and a stream is active, the server injects the message into the active turn (partial response preserved, model receives new input). When `mode=queue` or omitted, behavior matches V1.

**Streaming behavior:**
- On send: input clears, user message appears immediately, assistant message placeholder appears with streaming indicator
- SSE tokens append to assistant message in real-time
- Typing indicator (three dots or pulsing cursor) while waiting for first token
- On `done` event: streaming indicator removed, message finalized with full response from `done.response`. If a queued message exists, auto-send it.
- On error: error card appears below the partial response (if any)
- On cancel: partial response stays, marked as "(interrupted)"

**API calls:**
- Send: `POST /v1/sessions/{id}/messages` with `{"message": "...", "images": []}`, `Accept: text/event-stream` (⚠️ field is `message` not `content`)
- History: `GET /v1/sessions/{id}/messages?limit=200` on session open
- Model switch: `PUT /v1/model` with `{"model": "..."}`
- Thinking switch: `PUT /v1/thinking` with `{"level": "..."}`
- Context window: `GET /v1/sessions/{id}/context` (**NEW — needs backend endpoint**)

### 5.3 Session List

**Sidebar (macOS) / Main list (iOS chat tab)**

**Each session row shows:**
- Session title — derived from `label` (if set) or first user message (truncated to 60 chars), or "New Session" if empty
- Relative timestamp ("3m", "2h", "Yesterday")
- Last message preview (one line, truncated)
- Active indicator (dot) if currently streaming

**Grouping:** Today, Yesterday, Previous 7 Days, Older

**Actions:**
- Tap/click to open session
- Swipe left (iOS) or right-click (macOS) for context menu:
  - Clear history
  - Delete session
- Pull-to-refresh (iOS), auto-refresh on focus (macOS)

**Destructive actions during streaming:**
- **Delete/clear the currently streaming session:** Show a confirmation alert: "This session is actively streaming. Stop and {delete/clear}?" On confirm: cancel the stream task (`ChatViewModel.stopStreaming()`), then execute the API call (`DELETE` or `POST .../clear`), then set `selection = nil`.
- **Delete/clear a non-active session while another is streaming:** Allowed without confirmation — it doesn't affect the active stream.
- **`⌘⇧⌫` (clear):** Disabled when the selected session is streaming. Enabled otherwise.
- **Session switch while streaming:** Cancel the active stream first (`cleanup()`), then load the new session. No confirmation dialog — session switching should feel instant. **Partial response handling:** The server may or may not have persisted the partial assistant output at the time of cancellation — this is NOT guaranteed. When the user switches back to the old session, the client re-fetches history via `GET /v1/sessions/{id}/messages` (per Section 7.6 rule #1). If the server has the complete response, it will appear. If the server has only a partial or no assistant response, the user sees whatever the server recorded. The client does NOT cache the partial `streamingText` across session switches — it is discarded on `cleanup()`. This is acceptable: the server is the single source of truth, and re-fetch on session select is the one and only history loading rule.

**Search:** Search bar at top filters sessions by `label` and `key` fields only (these are the only text fields available from `GET /v1/sessions` without the backend `title`/`preview` enhancement). If backend enhancement #2 ships, also filter by `title` and `preview`. Do NOT attempt to filter by message content — that would require loading all messages for all sessions.

**Empty states:**
- **No sessions exist:** Centered message: "No conversations yet. Start a new one!" with New Session button.
- **Search returns no results:** "No sessions matching '{query}'" with clear-search button.
- **Session list loading failed:** "Could not load sessions" with Retry button.

**API:** `GET /v1/sessions` — returns `SessionInfo[]` with: `key`, `kind`, `status`, `label`, `model`, `created_at`, `updated_at`, `message_count`

**⚠️ Backend enhancement needed: session title + preview.**
The current `SessionInfo` struct has a `label` field (optional, set at creation) but no `title` or `last_message_preview`. Two options:
- **Option A (recommended):** Add `title: Option<String>` and `preview: Option<String>` to `SessionInfo`. The server derives `title` from the first user message (truncated) when label is null, and `preview` from the last message. Computed on list, not stored.
- **Option B (client-side):** App fetches `GET /v1/sessions/{id}/messages?limit=1` for each session to get the first message. Causes N+1 API calls — unacceptable for >10 sessions.
- **V1 minimum:** If backend change isn't ready, the app uses `label` for title (falls back to "Session {key}") and shows no preview. Functional but ugly.

### 5.4 Skills View

**Layout:** Grid of skill cards (2-column on macOS, 1-column on iOS)

**Skill card:**
- Icon (placeholder icon for V1, skills don't ship icons yet)
- Name (bold)
- Tool list (e.g., "web_search, web_fetch")
- Description (2 lines max, truncated) — ⚠️ requires backend enhancement #3, otherwise omit

**Sections:**
- **Loaded** — skills currently active on the server
- Skills are read-only in V1 (no install/remove from GUI)

**API:** `GET /v1/skills` — returns `{"skills": [{"name", "tools"}], "total": N}`. No description field yet (see backend work item #3).

### 5.5 Settings

**Presentation:**
- **macOS:** Standard SwiftUI `Settings { }` scene, opened via `⌘,`. This creates a separate Settings window (standard macOS pattern). NOT an in-app navigation destination — the sidebar "Settings" item opens this same window.
- **iOS:** Settings is a tab in the TabView. Tapping the ⚙️ tab shows a `List` with section headers → tap for detail views via `NavigationStack`.
- **Settings sections (left sidebar on macOS window, list on iOS):**

**Settings sections:**

#### Connection
- **Server URL** — text field, validated with `/health` ping
- **Bearer token** — secure text field, stored in Keychain
- **Connection status** — live indicator (connected/disconnected/connecting)
- **Test Connection** button

#### Appearance
- **Theme** — picker: Light / Dark / System
- **Font size** — slider or stepper (small / medium / large)
- **Code font** — picker: SF Mono, Menlo, Fira Code (if installed)

#### Model & Thinking
- **Active model** — picker populated from `GET /v1/models`, shows current `active_model`
- **Thinking level** — picker (off, low, adaptive, high)
- ⚠️ These are **server-wide settings**, not per-session. Changing the model here changes it for all sessions. Label clearly as "Server Model" / "Server Thinking Level" in the UI.

**Model/thinking control locations and conflict rules:**
The same server-global model/thinking can be changed from multiple places. These rules prevent confusion:

| Location | What it shows | Can change? | When disabled? |
|----------|--------------|-------------|---------------|
| Input bar (model badge) | Current `active_model` | Yes (dropdown) | During streaming |
| Input bar (thinking badge) | Current thinking level | Yes (dropdown) | During streaming |
| Status bar (model text) | Current `active_model` | **Read-only display only** — not tappable, not interactive | Never (always shows current state) |
| Settings → Model & Thinking | Current settings | Yes (pickers) | During streaming (show "Cannot change while streaming") |

- **During streaming:** All model/thinking pickers are **disabled**. Changing the model mid-stream could cause undefined behavior on the server. Show a tooltip/message: "Cannot change model while a response is streaming."
- **Remote change propagation:** The app refreshes model/thinking state by calling `GET /v1/models` and `GET /v1/thinking` on: (a) app launch, (b) session select, (c) after each stream completes. This is NOT tied to the health heartbeat — `/health` only checks connectivity and does not return model/thinking state. All UI locations read from `AppState.activeModel` and `AppState.thinkingLevel` (single source of truth).
- **Queued messages and model changes:** A queued message always uses whatever model is active when it actually sends (after the current stream completes). The model shown in the input bar at queue time is NOT locked in.
- **Empty/error states:** If `GET /v1/models` fails or returns an empty list, the model picker shows "Unavailable" (disabled, grayed out). If `GET /v1/thinking` fails, `AppState.thinkingLevel` stays `nil` and the thinking badge shows "—" (unknown) — it does NOT default to "off" because that would misrepresent server state.

#### Auth Status
- **Provider list** from `GET /v1/auth`
- Each provider shows: name, status (authenticated/not configured), `model_count` (integer — the API does NOT return a list of model names per provider, only a count), auth methods
- Read-only in V1 (credential management stays in CLI/config)
- **Empty state:** If no providers are configured: "No authentication configured. Run `fawx setup` on your server."

#### About
- App version
- Server version (from `/health` or `/status`)
- Fawx logo + link to website

---

### 5.6 Status Bar

**Persistent bar at the bottom of the window (macOS) or as a header element (iOS).**

```
● Connected  │  Power User  │  claude-sonnet-4-6  │  ██░░ 62% ctx
```

**Segments (left to right):**
1. **Connection indicator** — green dot + "Connected" / yellow dot + "Reconnecting..." / red dot + "Disconnected"
2. **Permission preset** — "Power User" / "Cautious" / "Experimental" (from server config, read-only in V1)
3. **Active model** — current model name, **read-only text display** (not tappable — model is changed via the input bar dropdown or Settings, not the status bar)
4. **Context window** — progress bar + percentage showing context usage for the active session

**Context window indicator:**
- Shows `used_tokens / max_tokens` as a mini progress bar + percentage
- Color coding: green (< 60%), yellow (60-85%), red (> 85%)
- Tooltip (macOS hover) or tap (iOS) shows exact numbers: "24,576 / 40,000 tokens"
- Updates after each message exchange (polled from `GET /v1/sessions/{id}/context`)
- When no session is selected: shows "—" or hides

**API requirement:** `GET /v1/sessions/{id}/context` — **new endpoint needed on backend**
```json
{
  "used_tokens": 24576,
  "max_tokens": 40000,
  "percentage": 61.4,
  "compaction_threshold": 80.0
}
```
This data already exists server-side in the `SlidingWindow` compaction system — just needs an HTTP endpoint to expose it.

---

## 6. API Integration Map

Every screen's data source, exhaustively mapped to existing endpoints.

| Screen | Endpoint | Method | Notes |
|--------|----------|--------|-------|
| App launch | `/health` | GET | Connection check |
| Session list | `/v1/sessions` | GET | List all sessions |
| New session | `/v1/sessions` | POST | Create on first message |
| Open session | `/v1/sessions/{id}` | GET | Session metadata |
| Chat history | `/v1/sessions/{id}/messages` | GET | Message history |
| Send message | `/v1/sessions/{id}/messages` | POST | SSE streaming response |
| Delete session | `/v1/sessions/{id}` | DELETE | Remove session |
| Clear history | `/v1/sessions/{id}/clear` | POST | Clear messages |
| Model list | `/v1/models` | GET | Available models |
| Switch model | `/v1/model` | PUT | Change active model |
| Thinking level | `/v1/thinking` | GET | Current thinking config |
| Set thinking | `/v1/thinking` | PUT | Change thinking level |
| Skills list | `/v1/skills` | GET | Loaded skills |
| Auth status | `/v1/auth` | GET | Provider auth status |
| Server status | `/status` | GET | Server health + version |
| Server config | `/config` | GET | Read-only config view |
| Context window | `/v1/sessions/{id}/context` | GET | Token usage (⚠️ NEW — needs backend) |

**Backend work needed before V1 ships:**
1. `GET /v1/sessions/{id}/context` — **new endpoint** for context window display (data exists in SlidingWindow, needs HTTP exposure)
2. `SessionInfo` enhancement — add `title: Option<String>` and `preview: Option<String>` fields computed from first/last messages (avoids N+1 sidebar problem)
3. `SkillSummaryDto` enhancement — add `description: Option<String>` (data exists in WASM skill manifests, just not exposed via API)
4. **SSE keep-alive pings** — emit `: ping\n\n` comment lines every 15s during tool execution or any period with no SSE events. Without this, the client cannot distinguish "server is thinking" from "connection is dead." This is the highest-priority backend item — the app cannot ship without it.

All other screens are fully backed by the existing 21-endpoint API surface shipped in Sprint 1 + Sprint 2.

---

## 7. State Management

### 7.1 App State

```swift
@MainActor @Observable
final class AppState {
    var connectionStatus: ConnectionStatus = .disconnected
    var serverURLString: String = ""         // ⚠️ NOT @AppStorage — see bridging note below
    var serverURL: URL? { URL(string: serverURLString) }  // computed, not stored
    var sessions: [Session] = []
    // NOTE: selection lives in @SceneStorage on the view, NOT here.
    // AppState holds server-derived data only. See SidebarSelection enum.
    var activeModel: ModelInfo?
    var thinkingLevel: ThinkingLevel?        // nil = unknown/fetch failed (NOT defaulting to .off)
    var availableModels: [ModelInfo] = []
    var skills: [Skill] = []
    var authProviders: [AuthProvider] = []
    var theme: AppTheme = .system
}
```

**⚠️ @AppStorage / @Observable bridging (CRITICAL):**

`@AppStorage` is a SwiftUI property wrapper that combines UserDefaults persistence with SwiftUI view invalidation. It **cannot** be placed inside an `@Observable` class — the Observation macro generates backing storage that conflicts with `@AppStorage`'s property wrapper storage, causing a compile error.

**The correct pattern:** `@AppStorage` lives in the **View or App layer**, and the View bridges values into `AppState` on launch and on change:

```swift
// In FawxApp.swift or the root ContentView:
struct ContentView: View {
    @AppStorage("server_url") private var storedServerURL: String = ""
    @AppStorage("theme") private var storedTheme: String = "system"
    @Environment(AppState.self) private var appState

    var body: some View {
        MainView()
            .onAppear {
                // Bridge persisted values → AppState on launch
                appState.serverURLString = storedServerURL
                appState.theme = AppTheme(rawValue: storedTheme) ?? .system
            }
            .onChange(of: appState.serverURLString) { _, newValue in
                // Bridge AppState → UserDefaults on change
                storedServerURL = newValue
            }
            .onChange(of: appState.theme) { _, newValue in
                storedTheme = newValue.rawValue
            }
    }
}
```

Alternatively, `AppState` can read/write `UserDefaults.standard` directly (no `@AppStorage`):
```swift
// In AppState:
func loadPersistedSettings() {
    serverURLString = UserDefaults.standard.string(forKey: "server_url") ?? ""
}
func persistServerURL(_ url: String) {
    serverURLString = url
    UserDefaults.standard.set(url, forKey: "server_url")
}
```
Either approach compiles. The key rule: **`@AppStorage` never appears inside `@Observable`.**

**Swift 6 strict concurrency rules:**
- `AppState` and all ViewModels (`ChatViewModel`, `SessionListViewModel`, etc.) MUST be `@MainActor`. All UI state mutation happens on the main actor.
- `FawxClient` is a `final class` isolated to a **custom global actor** (`@FawxClientActor`) or implemented as a plain `actor`. It holds the `URLSession` and server URL. It is NOT `@unchecked Sendable` — use proper actor isolation to avoid strict-concurrency fights.
- All model types (`Session`, `Message`, `ModelInfo`, `ThinkingLevel`, `Skill`, `AuthProvider`, `ServerStatus`) must be `struct` (not class) and conform to `Sendable`.
- The handoff pattern: `FawxClient` methods are `async` and return `Sendable` value types → ViewModel (on `@MainActor`) awaits and assigns to `@Observable` properties → SwiftUI reacts.
- SSE streaming uses `AsyncThrowingStream<SSEEvent, Error>` where `SSEEvent` is a `Sendable` enum. The ViewModel consumes this stream in a `Task { }` on the main actor.
- **Cancellation propagation (CRITICAL):** The `Task` that consumes the SSE stream must be stored on the ViewModel. When the user taps Stop, switches sessions, or the view tears down, cancel the task via `task.cancel()`. Inside `FawxClient`, the `AsyncThrowingStream` must be created with an `onTermination` handler that calls `urlSessionTask.cancel()` on the underlying `URLSessionDataTask`. Without this, orphaned URLSession tasks will leak after every Stop/session-switch.
- Never capture a `@MainActor`-isolated reference in a `nonisolated` closure. Pass only `Sendable` values across the boundary.

```swift
// Example: correct SSE consumption + cancellation pattern
@MainActor @Observable
final class ChatViewModel {
    var messages: [Message] = []
    var streamingText: String = ""
    private var streamTask: Task<Void, Never>?  // stored for cancellation
    
    func sendMessage(_ text: String, sessionId: String) {
        let client = self.client // FawxClient is an actor, called cross-actor
        streamTask = Task {
            do {
                let stream = try await client.sendMessage(text, sessionId: sessionId)
                for try await event in stream { // AsyncThrowingStream<SSEEvent, Error>
                    switch event {
                    case .textDelta(let text):
                        self.streamingText += text // safe: @MainActor
                    case .done(let response):
                        self.messages.append(Message(role: .assistant, content: response))
                        self.streamingText = ""
                    // ...
                    }
                }
            } catch is CancellationError {
                // Stream was cancelled (Stop, session switch, view teardown)
                if !self.streamingText.isEmpty {
                    self.messages.append(Message(role: .assistant,
                        content: self.streamingText + "\n\n*(interrupted)*"))
                    self.streamingText = ""
                }
            } catch { /* handle other errors */ }
        }
    }
    
    func stopStreaming() {
        streamTask?.cancel()  // triggers onTermination in FawxClient → URLSessionTask.cancel()
        streamTask = nil
    }
    
    // Called on session switch or view teardown
    func cleanup() {
        stopStreaming()
    }
}
```

### 7.2 Connection States

```
Disconnected → Connecting → Connected
      ↑                         │
      └────── Reconnecting ─────┘
                    │
              Disconnected (after max retries)
```

- On app launch: attempt connection to stored server URL
- On connection failure: retry with exponential backoff (1s, 2s, 4s, 8s, max 30s)
- Health check heartbeat every 30s while connected
- On disconnect: show reconnecting UI. **Do NOT queue outgoing message sends.** A queued `POST /v1/sessions/{id}/messages` that fires after reconnection risks double-sending if the original request was partially processed. Instead, disable the send button and show "Reconnecting..." in the input bar. The user can send after connection is restored. (Safe to queue: GET requests like session list refresh.)

### 7.3 Streaming States (per session)

```
Idle → Sending → WaitingForFirstToken → Streaming → Complete
                                              │
                                          Cancelled
                                              │
                                           Error
```

---

### 7.4 Network Timeout Configuration

URLSession must be configured with different timeouts for REST vs SSE:

| Request Type | `timeoutIntervalForRequest` | `timeoutIntervalForResource` | Notes |
|-------------|----------------------------|------------------------------|-------|
| REST API calls | 15s | 30s | Standard CRUD operations |
| SSE streams | 0 (no timeout) | 0 (no timeout) | Stream lives until `done`/`error` event or client cancels. **Both timeouts must be 0** — tool executions can take 5-10+ minutes with no data on the wire. Any non-zero request timeout will kill valid turns. |
| Health check | 5s | 10s | Fast-fail for connection status |

**⚠️ SSE keep-alive (CRITICAL backend work item #4):**
The server does NOT currently send heartbeat/ping frames during long tool executions. With both client timeouts set to 0, a dropped TCP connection is invisible — the client will hang forever waiting for data that will never arrive.

**Required fix before V1 ships:** Add server-side SSE keep-alive. The server must send a comment line (`: ping\n\n`) every 15 seconds during tool execution or any period where no SSE event is emitted. This is standard SSE practice.

**Client-side dead connection detection:**
- Track `lastEventReceivedAt` timestamp. If no SSE event (including pings) arrives within 45 seconds, treat the connection as dead.
- On dead connection: follow the stream drop recovery protocol (Section 8.3). Do NOT auto-retry the POST.
- This replaces the previous "300s timeout + treat silence as thinking" approach, which was not implementable.

### 7.5 Loading States

Every data fetch should have an explicit loading state visible to the user:

| Context | Loading State | Shown Where |
|---------|--------------|-------------|
| Initial connection | Spinner + "Connecting to {url}..." | Center of main area |
| Session list loading | Skeleton rows (3-4 placeholder rows with shimmer) | Sidebar |
| Message history loading | Spinner + "Loading conversation..." | Center of chat area |
| Sending message | User message appears immediately, assistant placeholder with streaming dots | Chat area |
| Model list loading | Spinner in dropdown | Model picker |
| Reconnecting | Yellow banner: "Reconnecting..." with animated dots | Top of window |

**Rule:** Never show a blank screen. Every fetch-in-progress state has a corresponding visual indicator. If data fails to load, show the error inline (not a blank area).

### 7.6 Session History Freshness

To handle server-side compaction and external changes (e.g., TUI and GUI both talking to same server):

**History loading rules (single source of truth):**

**The one and only history loading rule:**

`GET /v1/sessions/{id}/messages?limit=200` — every time, everywhere. 200 is the V1 ceiling.

1. **On session select:** Fetch `?limit=200`. Replace any in-memory cache with the response.
2. **On app foreground (iOS) / window focus (macOS):** Re-fetch `?limit=200` for the active session. Replace cache.
3. **During streaming:** Do NOT re-fetch. Trust the local SSE-assembled message.
4. **After stream completes:** The `done` event contains the full response. Append it to the local cache. The server also persists it, so next re-fetch will include it.
5. **On session list refresh:** Fetch `GET /v1/sessions` only. Do NOT fetch messages for every session.
6. **If `total` > 200:** The client cannot access older messages in V1. No "Load earlier" button, no scroll-to-load, no pagination. The user sees the most recent 200 messages and that's it. This is an acceptable limitation — compaction typically runs well before 200 messages.

**V2 backend work:** Add `?offset=N` or cursor-based pagination to `GET /v1/sessions/{id}/messages`.

---

## 8. Error Handling

### 8.1 Connection Errors
- **Server unreachable:** Banner at top: "Cannot connect to Fawx server at {url}" with Retry button
- **Auth failure (401):** Banner: "Authentication failed. Check your bearer token in Settings."
- **Timeout:** Same as unreachable, with "Connection timed out" message

### 8.2 API Errors
- **400 Bad Request:** Show server error message in chat as error card
- **404 Session Not Found:** Remove from sidebar, show "Session no longer exists"
- **429 Rate Limited:** "Rate limited by LLM provider." Show error card in chat. **Do NOT auto-retry message sends** (same no-retry-POST rule as stream drops — without an idempotency key, auto-retry risks double-sending). Show a "Retry" button that the user taps explicitly. Safe to auto-retry: GET requests (session list, models, etc.).
- **500 Server Error:** Error card in chat with raw error (useful for debugging)

### 8.3 Streaming Errors

**⚠️ CRITICAL: Never auto-retry a POST request.** `POST /v1/sessions/{id}/messages` is side-effecting (sends the message to the LLM). There is no idempotency key, cursor, or resume token. Any automatic retry risks double-sending the user's turn or forking the conversation.

**Stream drop recovery protocol:**
1. Mark the current stream as `interrupted`. Keep any partial assistant response visible.
2. Append "(interrupted)" marker to the partial response.
3. Re-fetch history: `GET /v1/sessions/{id}/messages` to check if the server recorded the full turn.
4. **If the server has the complete response** (assistant message exists after the user's): replace the partial response with the full one. Done.
5. **If the server has no assistant response** (turn was lost): show a "Response interrupted — Retry?" button. User must explicitly tap to resend.
6. **Never silently resend the user's message.** Always require user confirmation for retry.

**V2 backend work:** Add `X-Idempotency-Key` header support to `POST /v1/sessions/{id}/messages`. Server deduplicates by key and returns the existing response if the turn was already processed. This enables safe automatic retry.

**Other streaming errors:**
- **SSE parse error:** Log to console, continue consuming stream (don't kill a valid stream over one malformed frame)
- **`engine_error` event (recoverable: true):** Show error card inline but keep the stream alive — the engine may continue.
- **`error` event (fatal):** Show error card, mark stream as terminated. Follow recovery protocol above.

---

## 9. Platform-Specific Behavior

### 9.1 macOS

- **Window management:** Standard macOS window with min size 800×500
- **Title bar:** Transparent, content extends to top
- **Sidebar:** Resizable (200-400pt), collapsible via toolbar button
- **Focus management:** `⌘/` focuses input, `Esc` blurs
- **Text selection:** Full text selection in messages (native macOS behavior)
- **Copy:** `⌘C` copies selected text, copy button on code blocks
- **Drag & drop:** V2 (files into input)
- **Menu bar:** Standard macOS menu with app-specific items (New Session, Settings, etc.)
- **Toolbar:** Optional toolbar items for quick actions (New Session, Model picker)

### 9.2 iOS

- **Navigation:** Tab bar (Chat, Skills, Settings), NavigationStack within each tab
- **Safe areas:** Respect notch, home indicator, keyboard
- **Keyboard:** Input bar moves with keyboard, scroll to bottom on keyboard appear
- **Haptics:** Light haptic on send, on session switch
- **Swipe gestures:** Swipe left on session row for delete/clear, swipe back for navigation
- **Dynamic Type:** Support system font size preferences
- **iPad:** Sidebar layout (same as macOS) via NavigationSplitView when horizontal size class is regular

**iOS background / suspension handling (CRITICAL):**

URLSession bytes-based streaming does NOT survive iOS app suspension. The system will kill the connection when the app moves to background. This is not optional behavior — it is an iOS platform constraint.

**Lifecycle protocol:**
1. **`scenePhase == .inactive`:** No action. User may return immediately (e.g., notification center pull-down).
2. **`scenePhase == .background`:** Mark the active SSE stream as `suspended`. Do NOT attempt to keep it alive. Record `lastStreamedContent` (partial response text assembled so far) and `streamSessionId`.
3. **On return to `.active`:**
   - Re-check connection via `GET /health`.
   - Re-fetch history for `streamSessionId` via `GET /v1/sessions/{id}/messages`.
   - **If server has complete response:** Replace partial `lastStreamedContent` with the full response. Clear streaming state.
   - **If server has no assistant response:** Show partial content (if any) with "(interrupted)" marker + "Retry?" button.
   - **If a queued message exists:** Do NOT auto-send. Show the queued chip and let the user decide.
4. **Health check heartbeat:** Pause while backgrounded, resume on foreground.

**Do NOT use `URLSessionConfiguration.background`** — background URLSession is for downloads/uploads, not SSE streaming. It will not work for our use case.

### 9.3 App Transport Security (iOS + macOS)

The spec allows plain HTTP for Tailscale/LAN connections. Both iOS and macOS enforce ATS on URL Loading System traffic (URLSession), so both platforms need configuration.

**The problem:** Tailscale uses CGNAT addresses (100.64.0.0/10). These are NOT local network addresses — `NSAllowsLocalNetworking` does NOT cover them. There is no ATS exception target for IP address ranges.

**V1 approach: require `NSAllowsArbitraryLoads` + recommend HTTPS for production.**

```xml
<key>NSAppTransportSecurity</key>
<dict>
  <key>NSAllowsArbitraryLoads</key>
  <true/>
</dict>
```

This allows HTTP to any address. It is necessary for V1 because:
- Users connect via raw IP addresses (no domain names to create per-domain exceptions)
- Tailscale CGNAT IPs are not covered by `NSAllowsLocalNetworking`
- LAN addresses (192.168.x.x) are similarly not covered

**Production hardening (document in README):**
- **Recommended:** Enable Tailscale HTTPS on the server (`tailscale cert <hostname>`, configure `fawx serve --http` to bind with TLS). Then the app connects via `https://machinename.tail-net.ts.net:8400` and ATS is satisfied without any exceptions.
- **If HTTPS is not feasible:** `NSAllowsArbitraryLoads` is acceptable for a power-user tool distributed outside the App Store (direct download / TestFlight). App Store review may push back — cross that bridge in V2 when we add Tailscale HTTPS as the default.

**Both platforms:** This `Info.plist` entry is needed in BOTH the macOS and iOS targets. macOS does enforce ATS on URLSession traffic despite common misconceptions.

---

## 10. Performance Requirements

- **App launch → ready:** < 1 second (connection attempt is async, app usable immediately)
- **Message send → first token visible:** Bounded by server/LLM, but UI response must be < 100ms (immediate input clear + placeholder)
- **Session list load:** < 500ms for up to 100 sessions
- **Streaming render:** 60fps smooth scrolling during token streaming
- **Memory:** < 150MB for typical usage (10 sessions, 1000 messages loaded)

---

## 11. Security

### 11.1 Bearer Token Storage

- **Storage:** iOS/macOS Keychain via `SecItemAdd`/`SecItemCopyMatching`. Never in UserDefaults, files, or `@AppStorage`.
- **Keychain item attributes:**
  - `kSecClass`: `kSecClassGenericPassword`
  - `kSecAttrService`: `"ai.fawx.app"` (fixed, identifies our app)
  - `kSecAttrAccount`: The server URL string (e.g., `"http://100.123.20.63:8400"`) — this keys the token per-server, so connecting to a different server gets its own credential.
  - `kSecAttrAccessible`: `kSecAttrAccessibleWhenUnlocked`
- **Multi-server support:** Each server URL gets its own Keychain entry. Changing the URL in settings does NOT delete the old token — if the user switches back, the old token is still there.
- **No Keychain Sharing entitlement.** macOS and iOS apps do NOT share a single Keychain item across devices. Each device stores its own token independently. This is fine — the user copies the bearer token from their `~/.fawx/config.toml` once per device.
- **Token display:** Masked in settings (●●●●●●), with reveal toggle (eye icon)
- **Clipboard:** Token is never auto-copied. Reveal → manual select → copy is the only path.

### 11.2 Server URL Storage

- Stored in `UserDefaults` under key `"server_url"` as a **`String`**, not `URL`. Accessed via `@AppStorage("server_url")` in the View layer or `UserDefaults.standard` in AppState (see Section 7.1 bridging pattern). `@AppStorage` does not natively support `URL?`, so the raw string is stored and `URL` is computed.
- **Canonical format for Keychain keying:** The stored URL string is normalized before storage AND before Keychain lookup using this algorithm:
  ```swift
  func canonicalizeServerURL(_ input: String) -> String? {
      var normalized = input.trimmingCharacters(in: .whitespacesAndNewlines)
      guard !normalized.isEmpty else { return nil }

      // Reject double-scheme input (e.g., "http://http://example.com").
      // URLComponents silently mangles this into garbage like "http://http//example.com"
      // which would persist a bad URL and a bad Keychain account key.
      let schemePattern = #/^[a-zA-Z][a-zA-Z0-9+\-.]*:\/\//#
      if let match = normalized.firstMatch(of: schemePattern) {
          let afterScheme = normalized[match.range.upperBound...]
          if afterScheme.firstMatch(of: schemePattern) != nil {
              return nil  // double scheme → reject
          }
      }

      // URLComponents(string:) fails on scheme-less input:
      //   "100.93.251.101:8400" → nil (colon parsed as scheme separator)
      //   "myhost:8400" → scheme="myhost", path="8400" (misparsed)
      // Fix: detect missing scheme and prepend "http://" before parsing.
      if !normalized.contains("://") {
          normalized = "http://" + normalized
      }

      guard var components = URLComponents(string: normalized) else { return nil }
      components.scheme = components.scheme?.lowercased()
      components.host = components.host?.lowercased()
      // Reject if host is still nil after normalization
      guard let host = components.host, !host.isEmpty else { return nil }
      components.path = components.path == "/" ? "" : components.path  // strip trailing slash
      // Do NOT strip port — 8400 is not a default port for any scheme
      return components.string
  }
  // "100.93.251.101:8400"        → "http://100.93.251.101:8400"
  // "myhost:8400"                → "http://myhost:8400"
  // "HTTP://MyHost:8400/"        → "http://myhost:8400"
  // "http://100.93.251.101:8400" → "http://100.93.251.101:8400"
  // "http://http://example.com"  → nil (double scheme rejected)
  // ""                           → nil
  // "   "                        → nil
  ```
  **Onboarding UX note:** The Server URL text field placeholder should show `http://100.123.20.63:8400` (with scheme). If the user omits the scheme, canonicalization adds `http://`. The onboarding validation step ("Test Connection") runs canonicalization first, so typos like trailing slashes or missing schemes are auto-corrected before the first request.
  The output of this function is stored in `@AppStorage("server_url")` AND used as `kSecAttrAccount`. This prevents duplicate credentials from trailing slashes, scheme casing, or hostname casing.
- On URL change: test connection with new URL before overwriting the stored value. The old URL's Keychain entry is NOT deleted (in case the user switches back).

### 11.3 Network Security

- **HTTPS recommended** for production and public networks.
- **HTTP allowed** for Tailscale/LAN (both networks provide their own encryption layer).
- **ATS (both platforms):** Requires `NSAllowsArbitraryLoads` in `Info.plist` — `NSAllowsLocalNetworking` does NOT cover Tailscale CGNAT addresses (100.64.0.0/10) or raw IP connections. See Section 9.3 for full rationale and production hardening with Tailscale HTTPS.
- **No local message storage.** All conversation data lives on the server. The app caches messages in memory only for the duration of the session.
- **No analytics, telemetry, or crash reporting** in V1.

---

## 12. Build & Distribution

### 12.1 Project Setup
- **Xcode 16+**, Swift 6, SwiftUI
- **Minimum deployment:** macOS 14 (Sonoma), iOS 17
- **One approved third-party dependency:** [swift-markdown-ui](https://github.com/gonzalezreal/swift-markdown-ui) (MarkdownUI) for markdown rendering. Native `AttributedString` markdown support in SwiftUI cannot handle tables, fenced code blocks with language labels, or nested lists. MarkdownUI handles all of these. **Note:** MarkdownUI does NOT provide syntax highlighting — V1 ships with monochrome code blocks. If MarkdownUI proves to be in maintenance-mode or incompatible with our needs at build time, the fallback is Apple's `swift-markdown` package for parsing + custom SwiftUI views for rendering.
- All other networking and UI is native — URLSession, SwiftUI, Keychain Services. No other third-party packages.

### 12.2 Distribution
- **macOS:** Direct download (.dmg or .app zip) for now, Mac App Store later
- **iOS:** TestFlight for testing, App Store later
- **Signing:** Developer ID for macOS, standard iOS provisioning

---

## 13. Implementation Phases

### Phase 1: Foundation (Build First)
1. Project setup (Xcode multiplatform, shared/macOS/iOS targets)
2. `FawxClient` + `SSEStream` networking layer
3. Connection flow (settings → URL + token → health check → connected)
4. Basic chat: send message, receive SSE stream, render plain text

### Phase 2: Core Experience
5. Markdown rendering in messages
6. Session management (create, list, switch, delete, clear)
7. Model and thinking pickers (read + switch)
8. Code block rendering with copy button (monochrome — syntax highlighting is V2)

### Phase 3: Polish
9. Skills browser
10. Auth status view
11. Appearance settings (light/dark/system, font size)
12. Keyboard shortcuts (macOS)
13. Error handling + reconnection logic
14. iOS-specific adaptations (tab bar, swipe, keyboard handling)

### Phase 4: Ship
15. App icon + branding
16. TestFlight distribution
17. README + setup guide

---

## Appendix A: API Response Shapes

These are the JSON shapes the app must parse from the Fawx API.

### SessionInfo (verified against `fx-session/src/types.rs`)
```json
{
  "key": "sess-a1b2c3d4",
  "kind": "main",
  "status": "idle",
  "label": null,
  "model": "claude-sonnet-4-6",
  "created_at": 1741862400,
  "updated_at": 1741864200,
  "message_count": 12
}
```
**Notes:**
- `key` is the session identifier (not `id`). Use this in all `/v1/sessions/{id}` paths.
- `created_at` and `updated_at` are **Unix epoch seconds** (u64), NOT ISO 8601 strings.
- `kind`: `"main"` | `"subagent"` | `"channel"` | `"cron"`
- `status`: `"active"` | `"idle"` | `"completed"` | `"failed"` | `"paused"`
- `label` is optional (null if not set at creation)

### SessionMessage (verified against `fx-session/src/session.rs`)
```json
{
  "role": "user",
  "content": "Show me the streaming implementation",
  "timestamp": 1741862400
}
```
**Notes:**
- `role`: `"user"` | `"assistant"` | `"system"` (no `"tool"` role — tool calls are embedded in assistant content)
- `timestamp` is **Unix epoch seconds** (u64)
- No `tool_calls` field — tool invocations appear as text content in assistant messages. The client should detect and render tool call patterns if desired, but the API does not structure them separately in history.

### Messages List Response
```json
{
  "messages": [ /* SessionMessage[] */ ],
  "total": 24
}
```

### Health Response (unauthenticated — `GET /health`)
```json
{
  "status": "ok",
  "model": "claude-sonnet-4-6",
  "uptime_seconds": 3600,
  "skills_loaded": 0
}
```

### SSE Stream Events (verified against `fx-api/src/sse.rs`)

The server uses **named SSE events** (`event:` + `data:` lines), NOT the `{"type": ...}` pattern. The client MUST parse the `event:` field to determine the event type.

```
event: text_delta
data: {"text": "Hello"}

event: text_delta
data: {"text": " world"}

event: tool_call_start
data: {"id": "call_1", "name": "web_search"}

event: tool_call_complete
data: {"id": "call_1", "name": "web_search", "arguments": "{\"query\": \"SwiftUI\"}"}

event: tool_result
data: {"id": "call_1", "output": "...", "is_error": false}

event: phase
data: {"phase": "thinking"}

event: engine_error
data: {"category": "provider", "message": "Rate limited", "recoverable": true}

event: done
data: {"response": "Full response text here"}

event: error
data: {"error": "session storage not available"}
```

**Event types:**
| Event | Fields | When |
|-------|--------|------|
| `text_delta` | `text` | Each token/chunk of assistant response |
| `tool_call_start` | `id`, `name` | Model begins a tool call |
| `tool_call_complete` | `id`, `name`, `arguments` | Tool call arguments finalized |
| `tool_result` | `id`, `output`, `is_error` | Tool execution result |
| `phase` | `phase` | Processing phase change (thinking, executing, etc.) |
| `engine_error` | `category`, `message`, `recoverable` | Non-fatal engine error (from `StreamEvent::Error`) |
| `done` | `response` | Stream complete, full response text |
| `error` | `error` | Fatal error (stream terminates) |

**Client parsing rules:**
1. **`event:` line** → set the current event type
2. **`data:` line** → parse JSON payload, dispatch with the current event type
3. **`:` line (comment/ping)** → no event dispatch, but **MUST reset `lastEventReceivedAt` timestamp** for dead-connection detection. The server sends `: ping` comments every 15s during long tool executions where no real events are emitted. Ignoring these lines means the client will falsely declare the connection dead after 45s even though the server is alive and working.
4. **Empty line** → event boundary (standard SSE spec)
5. The `done` event replaces the `[DONE]` sentinel from the original spec. The `response` field in `done` contains the full assembled response text.

### Models List — `GET /v1/models` (verified against `settings.rs`)
```json
{
  "active_model": "claude-sonnet-4-6",
  "models": [
    {
      "model_id": "claude-sonnet-4-6",
      "provider": "anthropic",
      "auth_method": "api_key"
    },
    {
      "model_id": "gpt-5.4",
      "provider": "openai",
      "auth_method": "oauth"
    }
  ]
}
```
**Notes:**
- Field is `model_id`, NOT `id` or `name`.
- No `is_active` per-model — use `active_model` at top level to determine which is selected.
- `auth_method` tells you how the provider is authenticated (useful for auth status display).
- No human-readable display name — derive from `model_id` in the client if needed.

### Set Model — `PUT /v1/model`
Request: `{"model": "gpt-5.4"}`
Response:
```json
{
  "previous_model": "claude-sonnet-4-6",
  "active_model": "gpt-5.4"
}
```

### Thinking Level — `GET /v1/thinking` (verified against `types.rs`)
```json
{
  "level": "high",
  "budget_tokens": 10000
}
```
**Notes:**
- No `available` array — the client must hardcode the available levels for Anthropic models: `["off", "low", "adaptive", "high"]`
- **V2 note:** Thinking levels are provider-specific. OpenAI/Codex models use `["low", "medium", "high", "extra_high"]`. When multi-provider thinking support lands, the backend should return available levels per provider so the client doesn't hardcode.
- `budget_tokens` is `null` when level is `"off"`

### Set Thinking — `PUT /v1/thinking`
Request: `{"level": "extra_high"}`
Response:
```json
{
  "previous_level": "high",
  "level": "extra_high",
  "budget_tokens": 32000
}
```

### Skills List — `GET /v1/skills` (verified against `types.rs`)
```json
{
  "skills": [
    {
      "name": "brave-search",
      "tools": ["web_search"]
    }
  ],
  "total": 4
}
```
**Notes:**
- ⚠️ No `description` or `loaded` field exists in `SkillSummaryDto`. Only `name` and `tools`.
- The client cannot show skill descriptions unless the backend is enhanced.
- **Backend enhancement needed:** Add `description: Option<String>` to `SkillSummaryDto` (the WASM skill manifest has descriptions, just not exposed via API yet).

### Auth Providers — `GET /v1/auth` (verified against `types.rs`)
```json
{
  "providers": [
    {
      "provider": "anthropic",
      "auth_methods": ["api_key", "oauth"],
      "model_count": 3,
      "status": "authenticated"
    }
  ]
}
```
**Notes:**
- No `models` array — only `model_count` (integer). Client cannot list specific models per provider from this endpoint alone.
- `auth_methods` is an array of strings, not a single string.
- `status` values: `"authenticated"` | `"not_configured"` (verify in implementation)

### Create Session — `POST /v1/sessions`
Request:
```json
{
  "label": "Sprint 2 discussion",
  "model": "claude-sonnet-4-6"
}
```
**Notes:** Both fields optional. `label` defaults to null, `model` defaults to server's active model.

Response: `SessionInfo` (same shape as list, HTTP 201)

### Send Message — `POST /v1/sessions/{id}/messages`
Request:
```json
{
  "message": "Show me the streaming implementation",
  "images": [
    {
      "data": "base64-encoded-image-data",
      "media_type": "image/png"
    }
  ]
}
```
**⚠️ CRITICAL:** Field name is `message`, NOT `content`. Using `content` will cause a 422/deserialization error. `images` is an array of `{data, media_type}` objects, not URLs or paths.

### Error Response (all error endpoints)
```json
{
  "error": "session not found: sess-xyz"
}
```

---

## Appendix B: Design Reference

**Primary reference:** Codex macOS app (6 screenshots on file, 2026-03-13)
- Screenshot 1: New thread view — sidebar + centered empty state + suggestion cards + input bar
- Screenshot 2: Sidebar expanded — thread list with diff stats + time
- Screenshot 3: Active thread — conversation with terminal output, code changes, file diffs
- Screenshot 4: Skills page — grid layout with installed/recommended sections
- Screenshot 5: Automations page — categorized cards
- Screenshot 6: Settings — left sidebar settings navigation with General panel

**Key design borrowings from Codex:**
1. Sidebar-centric navigation with session list grouped by time
2. Input bar at bottom with model + thinking controls inline
3. Status bar at bottom (connection, permission level, active model)
4. Skills as a grid of cards with icon + name + description
5. Settings as a dedicated panel (not modal) with left-side section nav (macOS)
6. Minimal chrome — content takes priority

**Key differentiators from Codex:**
1. No git integration in V1 (no commit button, no diff view, no branch selector)
2. Fleet/node awareness in V2 (Codex has nothing equivalent)
3. Experiment monitoring in V2 (unique to Fawx)
4. Auth is multi-provider (Codex is OpenAI-only)
5. Permission presets (Power/Cautious/Experimental) instead of Codex's approval modes
6. Light + dark + system theme (Codex appears dark-only)
