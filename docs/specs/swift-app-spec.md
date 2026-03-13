# Fawx Native App — Swift UI Specification

**Status:** Draft v1
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
- Handle `[DONE]` sentinel for stream completion
- Support cancel (drop the URLSession task)

### 2.3 Data Flow

```
FawxClient (HTTP/SSE) → ViewModel (@Published) → SwiftUI View
                                ↑
                          User interaction
```

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
3. **Navigation items** (bottom):
   - Skills
   - Settings

**Keyboard shortcuts:**
| Shortcut | Action |
|----------|--------|
| `⌘N` | New session |
| `⌘K` | Quick session switcher (spotlight-style) |
| `⌘,` | Settings |
| `⌘⏎` | Send message (when in input) |
| `⌘⇧⌫` | Clear session history |
| `⌘1-9` | Switch to nth session |
| `Esc` | Cancel streaming response |
| `⌘/` | Focus input bar |

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
| Tool call | Collapsible card: tool name as header, arguments as code, result as expandable body |
| Error | Red-tinted card with error category and message |
| System | Centered, muted text (session created, model changed, etc.) |

**Markdown rendering must support:**
- Headings (H1-H4)
- Bold, italic, strikethrough
- Inline code and fenced code blocks with syntax highlighting
- Bullet and numbered lists (nested)
- Links (tappable, open in browser)
- Tables (basic)
- Blockquotes
- Horizontal rules

**Code blocks:**
- Monospace font (SF Mono / Menlo)
- Language label in top-right corner
- Copy button on hover (macOS) or long-press (iOS)
- Syntax highlighting (basic — at minimum distinguish keywords, strings, comments)
- Horizontal scroll for long lines, no wrapping

**Input bar:**
- Multi-line text field, auto-grows up to `inputBarHeight` max
- **Left controls:** attachment button (V2, disabled/hidden for V1)
- **Right controls:** Send button (accent color when text present, muted when empty)
- **Below input (inline or dropdown):**
  - Model picker — dropdown showing available models from `GET /v1/models`
  - Thinking level picker — dropdown (off, low, medium, high, extra high) from `GET /v1/thinking`
- `⌘⏎` to send (macOS), standard send button (iOS)
- `Esc` cancels active stream

**Input bar state machine (Steer pattern, inspired by Codex):**

The input bar changes behavior based on streaming state:

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
│ ┌─ Steer ──────────────────────── [×] ┐             │
│ │ "Actually, use a different approach" │             │
│ └─────────────────────────────────────┘             │
│ [+]  [ Actually, use a dif...  ]  [Model] [Send]   │
│                                    [Think]          │
└─────────────────────────────────────────────────────┘
```

- **Idle:** Normal input, accent-colored Send button when text present, muted when empty
- **Streaming + input empty:** Send button becomes **Stop** button (red/danger style). Tap cancels the active stream.
- **Streaming + user types:** Stop transitions to **Send**. Sending creates a **steer chip** above the input bar — a dismissable tab showing the queued message. The steer message is delivered to the model as a follow-up/interrupt.
- **Steer chip:** Shows queued text (truncated), mode label, dismiss (×) button. Only one message can be queued at a time (sending another replaces it).
- After stream completes: if a message is queued, it auto-sends as the next user message.

**Steer chip modes (V1 → V2 evolution):**

```
V1:  ┌─ Queued ──────────────────────────── [×] ┐
     │ "Use async/await instead of callbacks"    │
     └───────────────────────────────────────────┘
     (static label, always queues for after completion)

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
- On `[DONE]`: streaming indicator removed, message finalized. If steer is queued, auto-send it.
- On error: error card appears below the partial response (if any)
- On cancel: partial response stays, marked as "(interrupted)"

**API calls:**
- Send: `POST /v1/sessions/{id}/messages` with `{"content": "...", "images": []}`, `Accept: text/event-stream`
- History: `GET /v1/sessions/{id}/messages` on session open
- Model switch: `PUT /v1/model` with `{"model": "..."}`
- Thinking switch: `PUT /v1/thinking` with `{"level": "..."}`
- Context window: `GET /v1/sessions/{id}/context` (**NEW — needs backend endpoint**)

### 5.3 Session List

**Sidebar (macOS) / Main list (iOS chat tab)**

**Each session row shows:**
- Session title (first message truncated, or "New Session" if empty)
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

**Search:** Search bar at top filters sessions by title/content (client-side filter on loaded sessions)

**API:** `GET /v1/sessions` — returns list with metadata (id, created_at, updated_at, message_count)

### 5.4 Skills View

**Layout:** Grid of skill cards (2-column on macOS, 1-column on iOS)

**Skill card:**
- Icon (placeholder icon for V1, skills don't ship icons yet)
- Name (bold)
- Description (2 lines max, truncated)
- Status indicator (loaded / available)

**Sections:**
- **Loaded** — skills currently active on the server
- Skills are read-only in V1 (no install/remove from GUI)

**API:** `GET /v1/skills` — returns list with name, description, tool count

### 5.5 Settings

**Navigation:** Left sidebar on macOS (within settings panel), list → detail on iOS

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
- **Default model** — picker populated from `GET /v1/models`
- **Default thinking level** — picker (off, low, medium, high, extra high)
- Note: these are per-session overrides; changing here sets the default for new sessions

#### Auth Status
- **Provider list** from `GET /v1/auth`
- Each provider shows: name, status (authenticated/not configured), model access
- Read-only in V1 (credential management stays in CLI/config)

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
3. **Active model** — current model name, tappable to open model picker
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

**One new endpoint needed** (`/v1/sessions/{id}/context`) for context window display. All other screens are fully backed by the existing 21-endpoint API surface. Every screen is fully backed by the existing 21-endpoint API surface shipped in Sprint 1 + Sprint 2.

---

## 7. State Management

### 7.1 App State

```swift
@Observable
class AppState {
    var connectionStatus: ConnectionStatus = .disconnected
    var serverURL: URL?
    var sessions: [Session] = []
    var selectedSessionId: String?
    var activeModel: ModelInfo?
    var thinkingLevel: ThinkingLevel = .off
    var availableModels: [ModelInfo] = []
    var skills: [Skill] = []
    var authProviders: [AuthProvider] = []
    var theme: AppTheme = .system
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
- On disconnect: show reconnecting UI, queue outgoing messages

### 7.3 Streaming States (per session)

```
Idle → Sending → WaitingForFirstToken → Streaming → Complete
                                              │
                                          Cancelled
                                              │
                                           Error
```

---

## 8. Error Handling

### 8.1 Connection Errors
- **Server unreachable:** Banner at top: "Cannot connect to Fawx server at {url}" with Retry button
- **Auth failure (401):** Banner: "Authentication failed. Check your bearer token in Settings."
- **Timeout:** Same as unreachable, with "Connection timed out" message

### 8.2 API Errors
- **400 Bad Request:** Show server error message in chat as error card
- **404 Session Not Found:** Remove from sidebar, show "Session no longer exists"
- **429 Rate Limited:** "Rate limited by LLM provider. Waiting..." with auto-retry
- **500 Server Error:** Error card in chat with raw error (useful for debugging)

### 8.3 Streaming Errors
- **Stream interrupted:** Keep partial response, append "(interrupted)" marker
- **SSE parse error:** Log silently, continue consuming stream
- **Network drop during stream:** Reconnecting banner + retry from last known state

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

---

## 10. Performance Requirements

- **App launch → ready:** < 1 second (connection attempt is async, app usable immediately)
- **Message send → first token visible:** Bounded by server/LLM, but UI response must be < 100ms (immediate input clear + placeholder)
- **Session list load:** < 500ms for up to 100 sessions
- **Streaming render:** 60fps smooth scrolling during token streaming
- **Memory:** < 150MB for typical usage (10 sessions, 1000 messages loaded)

---

## 11. Security

- **Bearer token:** Stored in iOS/macOS Keychain, never in UserDefaults or plain files
- **Network:** HTTPS recommended for production; HTTP allowed for Tailscale/LAN
- **No local message storage:** All data on server. App is stateless (aside from connection config + theme pref)
- **Token display:** Masked in settings (●●●●●●), with reveal toggle
- **Clipboard:** Sensitive data (token) never auto-copied without explicit user action

---

## 12. Build & Distribution

### 12.1 Project Setup
- **Xcode 16+**, Swift 6, SwiftUI
- **Minimum deployment:** macOS 14 (Sonoma), iOS 17
- **No third-party dependencies in V1** — URLSession for networking, native SwiftUI for markdown (or minimal AttributedString rendering)
- If markdown rendering proves insufficient with native tools, consider single dependency: [swift-markdown-ui](https://github.com/gonzalezreal/swift-markdown-ui)

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
8. Code block rendering with copy + syntax highlight

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

### Session
```json
{
  "id": "uuid-string",
  "created_at": "2026-03-13T12:00:00Z",
  "updated_at": "2026-03-13T12:30:00Z",
  "message_count": 12
}
```

### Message
```json
{
  "role": "user" | "assistant" | "system" | "tool",
  "content": "markdown string",
  "timestamp": "2026-03-13T12:00:00Z",
  "tool_calls": [
    {
      "name": "tool_name",
      "arguments": { ... },
      "result": "string or null"
    }
  ]
}
```

### SSE Stream Events
```
data: {"type": "content", "text": "Hello"}
data: {"type": "content", "text": " world"}
data: {"type": "tool_call", "name": "search", "arguments": {...}}
data: {"type": "tool_result", "name": "search", "result": "..."}
data: {"type": "error", "category": "provider", "message": "Rate limited"}
data: [DONE]
```

### Model Info
```json
{
  "id": "claude-sonnet-4-6",
  "provider": "anthropic",
  "name": "Claude Sonnet 4.6",
  "is_active": true
}
```

### Thinking Level
```json
{
  "level": "high",
  "available": ["off", "low", "medium", "high", "extra_high"]
}
```

### Skill
```json
{
  "name": "brave-search",
  "description": "Web search via Brave Search API",
  "tools": ["web_search"],
  "loaded": true
}
```

### Auth Provider
```json
{
  "provider": "anthropic",
  "status": "authenticated",
  "models": ["claude-sonnet-4-6", "claude-opus-4"]
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
