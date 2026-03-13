# Codex Task: Fawx Swift App — Phase 2 (Core Experience)

Phase 1 is complete — Xcode project, networking, SSE parser, onboarding, basic chat all working. Now build the core experience.

---

## Reference Materials (all in this repo)

- **Spec:** `docs/specs/swift-app-spec.md` — the definitive reference. Read Sections 3–6 carefully.
- **Screenshots:** `docs/design/screenshots/` — pixel reference for every screen
- **Phase 1 code:** `app/` — your existing work. Build on top of it.
- **Full build prompt (for context):** `docs/specs/codex-swift-build-prompt.md`

---

## What to Build (Phase 2)

### 1. Markdown Rendering

Add `swift-markdown-ui` (https://github.com/gonzalezreal/swift-markdown-ui) via SPM if not already added.

- Assistant messages render as markdown via MarkdownUI
- User messages render as plain text (no markdown)
- **Code blocks are monochrome** — single text color on code background. NO syntax highlighting in V1. This is intentional per spec.
- Code blocks get a "Copy" button (top-right corner, appears on hover on macOS, always visible on iOS)
- Inline code uses the code background color
- Links are styled in accent color, open in default browser
- See screenshots `03-active-chat-dark.png` and `03-active-chat-light.png` for reference

### 2. Session Management (Sidebar)

Reference: Screenshots `02-*`, `08-*` (iOS)

**macOS sidebar:**
- List all sessions from `GET /v1/sessions`
- Group by date: "Today", "Yesterday", "Previous 7 Days", "Older" (use `updated_at` timestamp)
- Each row shows: `title` (bold, fall back to "New Session" if nil), `preview` below (secondary text, fall back to empty), relative timestamp on the right
- Active session highlighted with accent background
- "New Session" button at top (+ icon)
- Right-click context menu: "Delete Session" with confirmation
- Swipe-to-delete on iOS session list

**Session CRUD:**
- Create: `POST /v1/sessions` with empty body → navigate to new session
- Delete: `DELETE /v1/sessions/{id}` → remove from list, switch to next session or empty state
- Switch: tap session → load messages via `GET /v1/sessions/{id}/messages` → display in chat
- Clear: menu item → `POST /v1/sessions/{id}/clear` → clear local messages, confirm via sheet

**Polling:**
- Refresh session list every 30 seconds (lightweight poll) to catch title/preview updates
- Also refresh on app foreground (`.scenePhase` change)

### 3. Model & Thinking Pickers

Reference: Screenshots `07b-*`, `11b-*` (iOS drill-in)

**Model picker** (in input bar area or settings):
- Fetch available models from `GET /v1/models`
- Current model from `GET /v1/status` (field: `model`)
- Switch via `PUT /v1/model` with body `{ "model": "<model-name>" }`
- Display model name without provider prefix: `anthropic/claude-sonnet-4-6` → `claude-sonnet-4-6`
- On iOS, truncate further to 15 chars with `.lineLimit(1)`
- **Model/thinking is server-global, not per-session.** Make this clear in the UI (e.g., label says "Server Model")

**Thinking picker:**
- Thinking levels: `off`, `low`, `adaptive`, `high` — exactly 4 segments
- Current level from `GET /v1/thinking`
- Switch via `PUT /v1/thinking` with body `{ "level": "<level>" }`
- macOS: segmented control in settings (Screen 07b)
- iOS: segmented control in Model & Thinking drill-in (Screen 11b)

### 4. Tool Call Cards

Reference: Screenshots `05-tool-call-dark.png`, `05-tool-call-light.png`

Tool calls arrive via SSE events during streaming:
- `tool_call_start` — tool name, start of arguments
- `tool_call_delta` — streaming argument chunks
- `tool_call_complete` — final arguments
- `tool_result` — execution result

**Card design:**
- Collapsible card with tool name as header (e.g., "🔧 web_search")
- Collapsed by default after completion
- Expanded shows: arguments (as monospace JSON) + result (as monospace text)
- While executing: show spinner + "Running..." label
- Card background: surface color, rounded corners, subtle border
- See spec Section 5.3 for full behavior

### 5. Queued Message Chip

Reference: Screenshots `04-queued-msg-dark.png`, `04-queued-msg-light.png`

When user sends a message while the assistant is still streaming:
- Don't interrupt the stream
- Show a "queued" chip below the input bar: the message text with a clock icon
- When the current stream finishes, automatically send the queued message
- Only one queued message at a time (subsequent sends replace the queued one)
- Chip is dismissible (X button to cancel the queued message)

### 6. Status Bar (macOS)

Reference: Screenshots `03-active-chat-dark.png` — bottom of chat area

A thin bar at the bottom of the chat view showing:
- **Connection dot**: green (connected), yellow (reconnecting), red (disconnected)
- **Plan**: "Power User" (or whatever auth returns)
- **Model**: abbreviated name (no provider prefix)
- **Context**: percentage from `GET /v1/sessions/{id}/context` — show as "62% ctx" with a tiny progress bar
- Refresh context on each message send/receive
- **Status bar text is NOT interactive** — display only. Pickers live in Settings.
- Font size: 11pt, secondary text color

---

## Implementation Notes

1. **Don't break Phase 1.** Onboarding, basic chat, SSE streaming must continue working.
2. **Test with a real Fawx server.** The server is at `http://100.123.20.63:8400`. Use an existing bearer token.
3. **Markdown in streaming.** As `text_delta` events arrive, accumulate the text and re-render markdown progressively. MarkdownUI handles partial markdown gracefully.
4. **Session list refresh.** When a new message is sent/received, also update the session's `preview` locally (optimistic update) so the sidebar reflects the latest message immediately.
5. **No auto-retry on POST.** Failed message sends show an error + Retry button. Only GET requests auto-retry.
6. **`@AppStorage` bridging.** If you need persistent settings (like preferred theme), use the bridging pattern from Phase 1 — don't put `@AppStorage` inside `@Observable`.
7. **iOS considerations.** Session list is a full-screen tab (not a sidebar). Model/thinking are in Settings tab drill-in. See iOS screenshots for layout.

---

## What Success Looks Like

After Phase 2, the app should:
- ✅ Render assistant markdown with code blocks (monochrome, copy button)
- ✅ Show sessions in sidebar grouped by date, with title/preview
- ✅ Create, switch, delete sessions
- ✅ Pick model and thinking level (4-segment: off/low/adaptive/high)
- ✅ Display tool calls as collapsible cards with args + results
- ✅ Queue messages during streaming, auto-send on completion
- ✅ Show status bar with connection, model, context percentage
- ✅ Work on both macOS and iOS targets
