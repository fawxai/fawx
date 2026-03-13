# Codex Task: Build the Fawx Native App (SwiftUI)

## Overview

Build a SwiftUI multiplatform app (macOS primary, iOS secondary) for the Fawx agentic engine. The app connects to a Fawx server over HTTP/SSE and provides a native chat interface with session management, skills browsing, and settings.

**This is a greenfield build.** Create the Xcode project from scratch in the `app/` directory at the repo root.

---

## Reference Materials (all in this repo)

### Source of Truth
- **Spec:** `docs/specs/swift-app-spec.md` — APPROVED (v5, R7 APPROVE). This is the definitive reference for all behavior, architecture, API contracts, data models, and edge cases. Read it fully before writing any code.

### Visual Reference (pixel-accurate mockups)
- **Screenshots:** `docs/design/screenshots/` — 50 PNG files (25 screens × dark/light)
- **Interactive HTML:** `docs/design/fawx-mockups.html` — all screens with dark/light toggle

Screenshot naming convention:
```
01-onboarding-{dark,light}.png         — First-run connection flow
02-empty-state-{dark,light}.png        — New session, empty chat
03-active-chat-{dark,light}.png        — Chat with streaming response
04-queued-msg-{dark,light}.png         — Queued message chip
05-tool-call-{dark,light}.png          — Tool call card (collapsible)
06-skills-grid-{dark,light}.png        — Skills browser
07-settings-{dark,light}.png           — Settings overview
07a-settings-connection-{dark,light}.png
07b-settings-model-{dark,light}.png
07c-settings-appearance-{dark,light}.png
08-ios-sessions-{dark,light}.png       — iOS session list
09-ios-chat-{dark,light}.png           — iOS chat view
10-empty-states-{dark,light}.png       — Empty states overview
10a-empty-no-sessions-{dark,light}.png
10b-empty-no-results-{dark,light}.png
10c-empty-load-failed-{dark,light}.png
10d-empty-skills-empty-{dark,light}.png
11-ios-settings-{dark,light}.png       — iOS settings list
11b-ios-model-detail-{dark,light}.png  — iOS settings drill-in
12-error-states-{dark,light}.png       — Error states overview
12a-error-reconnecting-{dark,light}.png
12b-error-disconnected-{dark,light}.png
12c-error-interrupted-{dark,light}.png
12d-error-rate-limited-{dark,light}.png
```

### Backend API (already implemented)
The Fawx server (`fawx serve --http` on port 8400) exposes 21 HTTP endpoints. The spec (Section 10) documents all of them with request/response shapes. Key endpoints:

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/health` | GET | Connection health check (top-level, NOT under /v1/) |
| `/v1/sessions` | GET/POST | List/create sessions |
| `/v1/sessions/{id}` | GET/DELETE | Get/delete session |
| `/v1/sessions/{id}/messages` | GET/POST | Message history (GET) / Send message with SSE stream response (POST) |
| `/v1/sessions/{id}/clear` | POST | Clear session history |
| `/v1/sessions/{id}/context` | GET | Context window usage (used_tokens, max_tokens, percentage) |
| `/v1/models` | GET | Available models |
| `/v1/model` | PUT | Switch model (current model available via `/v1/status`) |
| `/v1/thinking` | GET/PUT | Current thinking level / switch |
| `/v1/skills` | GET | Loaded skills with descriptions |
| `/v1/auth` | GET | Auth provider status |
| `/v1/status` | GET | Server status (includes current model, session info) |
| `/v1/config` | GET/POST | Server config read/write |

**New in Sprint 3 (just merged):**
- SSE keep-alive pings: `: ping\n\n` comments every 15s during silence — your SSE parser must ignore these (they're comments, not events)
- `GET /v1/sessions/{id}/context` — powers the status bar context indicator
- `SessionInfo` now includes `title: String?` and `preview: String?` — use these for sidebar display
- `SkillSummaryDto` now includes `description: String` — use for the skills grid

---

## Project Structure

Create this directory structure:

```
app/
├── Fawx.xcodeproj/
├── Fawx/
│   ├── FawxApp.swift                    — App entry point, scenes, commands
│   ├── Info.plist                       — ATS exception for local HTTP
│   ├── Assets.xcassets/                 — App icon, accent color
│   ├── Models/                          — Data types (Session, Message, Skill, etc.)
│   │   ├── Session.swift
│   │   ├── Message.swift
│   │   ├── ModelInfo.swift
│   │   ├── ThinkingLevel.swift
│   │   ├── Skill.swift
│   │   ├── ServerStatus.swift
│   │   └── AuthProvider.swift
│   ├── Networking/                      — API client + SSE
│   │   ├── FawxClient.swift             — HTTP client (URLSession)
│   │   ├── SSEStream.swift              — Server-Sent Events parser
│   │   └── APIError.swift               — Error types
│   ├── ViewModels/                      — @Observable view models
│   │   ├── AppState.swift               — Global state (connection, model, thinking)
│   │   ├── SessionViewModel.swift       — Session list + CRUD
│   │   ├── ChatViewModel.swift          — Chat messages + streaming
│   │   └── SettingsViewModel.swift      — Settings state
│   ├── Views/                           — SwiftUI views
│   │   ├── Shared/                      — Cross-platform components
│   │   │   ├── MessageBubble.swift
│   │   │   ├── CodeBlock.swift
│   │   │   ├── ToolCallCard.swift
│   │   │   ├── StatusBar.swift
│   │   │   ├── InputBar.swift
│   │   │   ├── ModelBadge.swift
│   │   │   └── QueuedMessageChip.swift
│   │   ├── macOS/                       — macOS-specific
│   │   │   ├── ContentView.swift        — NavigationSplitView layout
│   │   │   ├── Sidebar.swift
│   │   │   └── SettingsView.swift       — Settings window
│   │   └── iOS/                         — iOS-specific
│   │       ├── TabRootView.swift        — Tab bar layout
│   │       ├── SessionListView.swift
│   │       └── iOSSettingsView.swift
│   ├── Theme/                           — Design system
│   │   ├── Colors.swift                 — All color tokens
│   │   ├── Typography.swift             — Font definitions
│   │   └── Spacing.swift                — Layout constants
│   └── Utilities/
│       ├── KeychainHelper.swift         — Token storage
│       └── Formatters.swift             — Date, model name formatting
├── FawxTests/
│   └── ...
└── Package.swift or via Xcode SPM      — MarkdownUI dependency
```

---

## Design System (exact values)

### Colors (implement as SwiftUI Color extensions)

| Token | Dark | Light |
|-------|------|-------|
| background | #1A1A1A | #FFFFFF |
| surface | #242424 | #F5F5F5 |
| surfaceHover | #2E2E2E | #EBEBEB |
| surfaceActive | #383838 | #E0E0E0 |
| text | #E8E8E8 | #1A1A1A |
| textSecondary | #999999 | #666666 |
| accent | #E8711A | #D45E14 |
| accentSubtle | #E8711A20 | #D45E1415 |
| success | #4ADE80 | #22C55E |
| warning | #FBBF24 | #D97706 |
| error | #F87171 | #DC2626 |
| border | #333333 | #E5E5E5 |
| code | #2D2D2D | #F0F0F0 |

### Typography
- UI text: System font (SF Pro via `.system()`)
- Code: `.monospaced()` / SF Mono
- Chat body: 14pt regular
- Sidebar: 13pt regular
- Status bar: 11pt regular

---

## Implementation Order

Follow the phases from spec Section 13, but build iteratively — each phase should produce a working, testable app:

### Phase 1: Foundation (do this first)
1. **Xcode project setup** — multiplatform target, add MarkdownUI via SPM
2. **Theme/Colors.swift** — all color tokens as `Color` extensions with dark/light adaptive
3. **Models/** — all Codable structs matching spec Appendix A (include new `title`/`preview` on SessionInfo, `description` on Skill)
4. **FawxClient.swift** — URLSession-based HTTP client. Bearer token auth. All 21 endpoints as async methods.
5. **SSEStream.swift** — AsyncSequence-based SSE parser. Handle `text_delta`, `tool_call_start`, `tool_call_delta`, `tool_call_complete`, `tool_result`, `done` events. **Ignore `: ping` comment lines** (keep-alive, not data).
6. **AppState.swift** — `@Observable` singleton. Connection state, active model, thinking level, server URL + token (Keychain).
7. **Onboarding view** (Screen 01) — server URL + token input, Test Connection, Continue
8. **Basic chat** — send message, stream response as plain text. macOS NavigationSplitView layout.

### Phase 2: Core Experience
9. **Markdown rendering** — MarkdownUI for assistant messages, code blocks (monochrome, no syntax highlighting), copy button
10. **Session management** — list, create, switch, delete. Sidebar with grouped dates (Today/Yesterday). Use `title` and `preview` from SessionInfo.
11. **Model/thinking pickers** — read current, switch. Thinking levels: `off, low, adaptive, high` (Anthropic only for V1).
12. **Tool call cards** (Screen 05) — collapsible, show arguments + result
13. **Queued message chip** (Screen 04) — when user sends during streaming
14. **Status bar** (macOS) — connection dot, plan, model name, context percentage from `/v1/sessions/{id}/context`

### Phase 3: Polish
15. **Skills browser** (Screen 06) — grid of skill cards with name, description, tool chips
16. **Settings** (Screens 07a-c) — Connection, Model & Thinking, Appearance (theme picker, font size)
17. **Error states** (Screens 12a-d) — reconnecting, disconnected, interrupted, rate limited
18. **Empty states** (Screens 10a-d) — no sessions, no results, load failed, skills empty
19. **iOS adaptations** — tab bar, session list, iOS settings with drill-in (Screens 08, 09, 11, 11b)
20. **Keyboard shortcuts** (macOS) — ⌘N, ⌘⇧O, ⌘,, ⌘⇧⌫, ⌘1-9, ⌘/, Esc
21. **Reconnection logic** — health check polling, auto-reconnect, state transitions

### Phase 4: Ship
22. App icon (🦊 placeholder for now), accent color in assets
23. README with setup instructions

---

## Critical Implementation Notes

1. **SSE parser must handle ping comments.** Lines starting with `:` are SSE comments — ignore them. The server sends `: ping\n\n` every 15s during tool execution. If your parser treats these as events, the app will break.

2. **Model names on iOS must be abbreviated.** Drop the provider prefix: `claude-sonnet-4-6` → `sonnet-4-6`. See screenshots 08/09 for the annotation. Apply `.lineLimit(1)` to status strips — truncate, never wrap.

3. **Thinking levels are `off, low, adaptive, high`** (4 values). NOT medium/extra_high — those are for a different provider. Hardcode these for V1.

4. **Code blocks are monochrome.** Single text color on code background. NO syntax highlighting in V1. This is intentional.

5. **`@AppStorage` cannot be inside `@Observable` classes.** Use the pattern from spec Section 8.3 — separate `AppSettings` struct with `@AppStorage` in a view, bridge to AppState via `.onChange`.

6. **No auto-retry on POST failures.** When a chat stream drops or gets 429, show error + manual Retry button. Only GET requests (session list, models, health) can auto-retry.

7. **Status bar model text is NOT interactive.** Plain text display only — the model picker is in Settings and the input bar.

8. **ATS exception required.** The app talks to `http://` (not https) local servers. Add `NSAppTransportSecurity > NSAllowsLocalNetworking = YES` to Info.plist for BOTH macOS and iOS targets.

9. **SessionInfo now includes `title` and `preview`.** Use `title` for the sidebar session name (fall back to first message content if nil). Use `preview` for the subtitle line.

10. **Context endpoint** (`GET /v1/sessions/{id}/context`) returns `{ used_tokens, max_tokens, percentage, compaction_threshold }`. Display `percentage` as "62% ctx" in the status bar with a small progress indicator.

---

## What Success Looks Like

A working macOS app that:
- Connects to a Fawx server via URL + bearer token
- Lists and manages chat sessions in a sidebar
- Streams chat responses with markdown rendering
- Shows tool call cards
- Handles queued messages during streaming
- Displays connection status, model, context usage in a status bar
- Has a Settings window with connection, model/thinking, and appearance tabs
- Handles errors gracefully (reconnecting, disconnected, interrupted, rate limited)

Plus an iOS app (shared codebase) with tab bar navigation and iOS-native settings.

The screenshots in `docs/design/screenshots/` are the pixel reference — match them.
