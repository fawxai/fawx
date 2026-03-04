# Jarvis Sprint Task Spec — H2 Features + Bug Fixes + UX Polish

*Created 2026-02-16. Assigned to Jarvis (UI features track).*

---

## Priority 1: H2 Sprint Features

### 1A. Progressive Status Updates (tool name surfacing)

**Goal:** Show the user what the agent is currently doing during a tool loop, not just a spinner.

**Current state:**
- `LoopProgressListener.onToolResult()` fires AFTER each tool completes
- `ToolExecutionDelegate.onStepStarted(step, maxSteps)` fires at the start of each step
- No callback fires BEFORE a tool executes (the user doesn't know what's happening until it's done)

**What to build:**

1. **Add `onToolStarted(toolName: String, toolIndex: Int, batchSize: Int)` to `LoopProgressListener`**
   - File: `AgentExecutor.kt` (`:core` module — coordinate with Clawdio on this interface change)
   - Called right before `delegate.executeToolCall()` at ~line 215
   - `toolIndex` and `batchSize` let UI show "Running tool 2/3"

2. **Surface tool name in overlay status**
   - File: `ChatViewModel.kt` — implement the new listener method
   - Update the overlay status text: "🔧 Opening Gmail..." / "🔧 Tapping element 5..."
   - Use `ToolCategory` from OutputClassifier for user-friendly labels:
     - MECHANICAL: "Interacting..." (don't show individual taps)
     - PROMINENT: "Opening Gmail..." (show the app/action name)
     - RESEARCH: "Searching the web..." / "Fetching page..."
     - REASONING: "Thinking..."
   - Clear status text when tool completes (onToolResult fires)

3. **Show step counter in overlay**
   - "Step 3/25" or similar, using `onStepStarted(step, maxSteps)`
   - Subtle/dimmed — not the main focus, just orientation

**Files to modify:**
- `AgentExecutor.kt` lines ~215, ~437-441 (add to interface) — ⚠️ Clawdio's file, coordinate
- `ChatViewModel.kt` — implement new listener method
- `OverlayPortedScreen.kt` or overlay layout — render status text

**Tests:**
- Unit test: `onToolStarted` fires before `executeToolCall` in AgentExecutorTest
- Unit test: mechanical tools show generic "Interacting..." not tool name
- Unit test: prominent tools show specific action

---

### 1B. Conversation Lifecycle (idle timeout, daily reset)

**Goal:** Automatically manage conversation context to prevent stale/bloated sessions.

**What to build:**

1. **Idle timeout auto-clear**
   - Track `lastActivityTimestamp` in `ChatViewModel` (updated on every user message and agent response)
   - On app resume (`onResume` in `ChatActivity`), check if idle time exceeds threshold (default: 30 minutes)
   - If exceeded: auto-clear conversation, show system message "Session cleared after inactivity"
   - Make threshold configurable (Settings → "Auto-clear after" → 15min / 30min / 1hr / Never)

2. **Daily reset**
   - On first message of a new calendar day, auto-clear previous conversation
   - Track `lastConversationDate` in SharedPreferences
   - Show system message: "New day, fresh start 🌅"
   - Optional: summarize previous session before clearing (deferred — just clear for now)

3. **Wire into existing `clearConversation()`**
   - `ChatViewModel.clearConversation()` already exists at line ~1006
   - Idle timeout and daily reset both call this
   - Preserve memory (SqliteMemoryProvider data survives clears)

**Files to modify:**
- `ChatViewModel.kt` — add `lastActivityTimestamp`, idle check logic
- `ChatActivity.kt` — call idle check in `onResume()`
- `SettingsScreen.kt` or settings — add idle timeout preference
- SharedPreferences — store `lastConversationDate`

**Tests:**
- Unit test: idle timeout triggers clearConversation after threshold
- Unit test: daily reset triggers on date change
- Unit test: activity within threshold does NOT clear
- Unit test: "Never" setting disables auto-clear

---

## Priority 2: Bug Fixes

> **Note:** #437, #447, #471 transferred to Clawdio (behavioral bugs touching shared/core files).
> Jarvis keeps the visual/layout fixes below.

### 2A. #450 — Show most recent messages when keyboard is up (fullscreen)

**Bug:** In fullscreen mode, when keyboard appears, the message list doesn't scroll to bottom. User can't see the most recent messages.

**Fix:** Add `WindowInsets` listener for keyboard appearance → scroll to bottom of LazyColumn.

**Files:** `ChatActivity.kt` or fullscreen Compose layout

---

### 2B. #467 — Onboarding screens not centered

**Bug:** Onboarding content isn't vertically/horizontally centered on screen.

**Fix:** Check `Arrangement.Center` / `Alignment.CenterHorizontally` in onboarding Compose layouts.

**Files:** Onboarding Compose files

---

### 2C. #469 — Onboarding wrong colors in light mode

**Bug:** Onboarding screens use wrong colors when device is in light mode.

**Fix:** Check `MaterialTheme.colorScheme` usage. May be hardcoded dark theme colors instead of theme-aware colors.

**Files:** Onboarding Compose files, theme definitions

---

## Priority 3: UX Polish

### 3A. #456 — Curate chat model list

**Goal:** Only offer models that work well. Current list includes models that are too weak or unavailable.

**What to do:** Hardcode a curated list per provider:
- Anthropic: claude-opus-4-0, claude-sonnet-4-0, claude-haiku-3-5
- OpenAI: gpt-4o, gpt-4o-mini
- OpenRouter: top 5-6 models

Remove the "fetch all models" approach for now (#391 can do dynamic discovery later).

**Files:** Model list definitions (likely in settings or provider config)

---

### 3B. #470 — Redesign onboarding (conversational setup)

**Goal:** Replace the current static onboarding screens with a conversational flow. Deferred for now — just fix #467 and #469 first, then tackle this as a separate effort.

---

### 3C. #472 — Full-screen chat should offer switch to overlay mode

**Goal:** Add a button/menu item in the fullscreen chat to switch to overlay mode without going to settings.

**Files:** `ChatActivity.kt` toolbar/menu

---

### 3D. #406 — Default overlay mode setting

**Goal:** Let user choose whether overlay defaults to mini-chat or bubble after tool execution.

**Files:** Settings, `OverlayController.kt`

---

## Branch Convention

- Branch from `feat/android-mvp`
- Name: `jarvis/feature-name` (e.g., `jarvis/progressive-status`, `jarvis/conversation-lifecycle`)
- PR title prefix: `[Jarvis]`
- Request review: `@claude review this PR`
- When approved + CI green: `@abbudjoe ready for merge`

## File Ownership Reminder

Jarvis owns: `ChatActivity.kt`, `OverlayController.kt`, `OverlayPortedScreen.kt`, overlay/settings/layouts, onboarding

Shared (coordinate with Clawdio): `ChatViewModel.kt`, `AgentExecutor.kt` (interface changes only)

Clawdio owns: `AgentExecutor.kt` (implementation), `BoundaryCheck.kt`, `OutputClassifier.kt`, core tests

## Suggested Order

1. **Progressive Status Updates** (1A) — highest user-visible impact
2. **Conversation Lifecycle** (1B) — needs Settings UI
3. **Bug fixes** (#450, #467, #469) — visual/layout quick wins
4. **UX polish** (#456, #472, #406) — nice-to-haves
