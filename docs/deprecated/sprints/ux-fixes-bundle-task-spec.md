# UX Fixes Bundle — Task Spec for Jarvis

## Overview

Bundle of 5 UX issues to fix in a single PR or split across 2 PRs max.
All are UI-layer changes in `:chat` module (Jarvis territory).

## Issues

### 1. #474 — Text input should expand rather than scroll
**Current:** Chat text input is fixed height and scrolls internally when text is long.
**Expected:** Text input should grow vertically (up to ~4 lines max) as user types, then scroll.
**File:** `ChatActivity.kt` — find the `OutlinedTextField` / `TextField` for chat input.
**Fix:** Set `maxLines = 4` and `singleLine = false`. Remove any fixed height constraint. The Compose `TextField` with `maxLines` auto-expands.

### 2. #473 — Queue button should become send button when agent is stopped
**Current:** When agent is running, the send button shows as a "queue" action. When agent stops/finishes, the button still appears as queue instead of reverting to normal send.
**Expected:** When agent is idle (not running), the button should be the normal send button. Only show queue behavior while the agent is actively running.
**File:** `ChatActivity.kt` — look at the send/queue button composable and its state binding.
**Fix:** Check `viewModel.isAgentRunning` (or equivalent state) to toggle between send and queue modes.

### 3. #448 — Overlay response view should auto-scroll to most recent response
**Current:** In overlay mode, when agent sends a long response, user has to manually scroll down to see the latest text.
**Expected:** Auto-scroll to bottom when new content arrives.
**File:** `OverlayController.kt` — find the response text view / LazyColumn.
**Fix:** Use `LaunchedEffect` on message count or last message content to trigger `scrollToItem(lastIndex)`. Or use `reverseLayout = true` on the list.

### 4. #451 — Text input hidden by keyboard in overlay queue mode
**Current:** In overlay mode, when keyboard appears for "queue a follow up", the text input is hidden behind the keyboard.
**Expected:** Input should be visible above the keyboard.
**File:** `OverlayController.kt` — the overlay layout for queue input.
**Fix:** Use `WindowInsets.ime` padding or `android:windowSoftInputMode="adjustResize"` for the overlay window. Note: overlays use `TYPE_APPLICATION_OVERLAY` which may not get automatic keyboard adjustments — may need to listen for keyboard height via `ViewTreeObserver.OnGlobalLayoutListener` and adjust overlay position.

### 5. #475 — Agent types 'google' in Google search bar
**Current:** When agent uses `open_app` to open Google, it then types "google" into the search bar (the app name instead of the search query).
**Expected:** Agent should type the actual search query, not the app name.
**File:** This is likely a **system prompt issue**, not a code bug. The agent confuses the app name with the search text. Check `PhoneAgentPrompts.kt` or equivalent for the `open_app` / `type_text` tool guidance.
**Fix options:**
  - A) Add clarifying instruction in system prompt: "After opening an app, type the user's query — not the app name."
  - B) If `open_app` has a `search_query` parameter, make sure it's being used correctly.
  - C) This might need a `type_in_search` higher-level tool that opens app + types query.
  
  **Note:** #475 may be more of a prompt engineering issue than a code fix. If it requires core prompt changes (PhoneAgentPrompts.kt), coordinate with Clawdio — that file is in `:core`.

## Branch Convention

Branch: `ui/ux-fixes-bundle` from `feat/android-mvp`
PR title: `[Jarvis] UX fixes: input expand, send/queue button, overlay scroll, keyboard (#474 #473 #448 #451)`
Target: `feat/android-mvp`

If #475 requires a separate approach, split it into its own PR.

## File Ownership

| File | Owner | Notes |
|------|-------|-------|
| `ChatActivity.kt` | Jarvis | #474, #473 |
| `OverlayController.kt` | Jarvis | #448, #451 |
| `ChatPortedComponents.kt` | Jarvis | May need changes for input |
| `PhoneAgentPrompts.kt` | Clawdio | #475 — coordinate if needed |

## What NOT to Do

- ❌ Don't modify core module files without coordinating with Clawdio
- ❌ Don't add new dependencies for these fixes — all solvable with existing Compose/Android APIs
- ❌ Don't change overlay window type or permissions

## Testing

1. Type a multi-line message → input expands up to 4 lines, then scrolls
2. Start agent task → button shows queue mode → agent finishes → button reverts to send
3. In overlay, agent sends long response → auto-scrolls to bottom
4. In overlay queue mode, tap input → keyboard appears → input visible above keyboard
5. Ask agent to "search for weather in Denver" → agent opens Google and types "weather in Denver" (not "google")

## Priority Order

1. #474 (input expand) — easiest, biggest daily impact
2. #473 (send/queue button) — confusing UX when button state is wrong
3. #448 (overlay auto-scroll) — common annoyance
4. #451 (keyboard hides input) — overlay-specific, trickier
5. #475 (types 'google') — may need prompt changes, lowest priority in this bundle
