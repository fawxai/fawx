# Tier 3 Pressure Test: Overlay Auto-Hide, Screen Content, Output Classification

*Lighter-touch audit — 2026-02-16*

## Context

These three areas are Fawx-specific patterns with no direct OpenClaw equivalent. OpenClaw is a desktop CLI/web tool — no overlays, no Android accessibility APIs, no phone-specific screen reading. Pressure-tested against Android platform best practices and general architecture principles.

---

## 1. Overlay Auto-Hide Hook Pattern

### How It Works
- `ScreenReader` (`:core` singleton) exposes 4 nullable suspend lambda hooks:
  - `toolLoopOverlayHideHook` / `toolLoopOverlayRestoreHook` — hide overlay for entire tool loop duration
  - `screenshotOverlayHook` / `screenshotOverlayRestoreHook` — hide overlay during screenshot capture
- `ChatActivity` sets all 4 in `onCreate()`, clears in `onDestroy()`
- `ChatViewModel` invokes tool loop hooks around `AgentExecutor.run()`
- `ScreenReader.takeScreenshot()` invokes screenshot hooks around capture
- `OverlayService` uses `View.INVISIBLE` (not GONE) to preserve layout position
- Double-hide guard via `savedVisibility` sentinel in OverlayService

### Findings

**Architectural pattern: acceptable**
The hook-based decoupling between `:core` (no Android UI deps) and `:chat` (UI) is intentional and correct. `:core` can't reference OverlayService directly. The nullable lambda pattern is lightweight and functional.

**Issue 1: Global mutable state — LOW**
Four `var` properties on a singleton. Any code can overwrite them. In a single-Activity app this is fine, but it's fragile if the architecture ever grows. Not worth fixing now — the cost of an interface/DI layer exceeds the risk.

**Issue 2: 200ms delay is a magic number — LOW**
`SCREENSHOT_OVERLAY_HIDE_DELAY_MS = 200L` assumes WindowManager processes visibility changes within 200ms. On slow devices or under memory pressure, this may not hold. However, the consequence is just an overlay briefly visible in a screenshot — non-critical. The constant is already named and configurable (it's `internal`, not hardcoded inline).

**Issue 3: Hook clearing race — THEORETICAL**
If `ChatActivity.onDestroy()` clears hooks while a screenshot coroutine is mid-execution: the `?.invoke()` null-safe call means the restore hook just becomes a no-op. The overlay stays hidden until the service is restarted. In practice, this is unlikely because Activity destruction typically cancels the ViewModel's coroutine scope first.

**Verdict: No critical issues. No changes needed.**
The pattern works correctly for the current single-Activity architecture. The double-hide guard and try/finally blocks are solid defensive code.

---

## 2. Screen Content Model

### How It Works
- `ScreenContent(elements: List<ScreenElement>, packageName: String?)`
- `ScreenElement`: id, text, contentDescription, className, isClickable, isEditable, bounds
- `getScreenContent()`: Window-aware approach via `findAppWindowRoot()` → `pickBestWindow()`, falls back to `rootInActiveWindow`
- Self-package filtering prevents reading Fawx's own overlay
- `toPromptText()`: prioritizes interactive elements (scoring: editable +4, clickable +3, text +2, desc +1), caps at 40, restores visual order

### Findings

**Issue 1: 40-element hard cap — MEDIUM**
Not configurable. Complex apps (Settings, email clients with many items) may have 100+ actionable elements. The model can't see elements past the cap, leading to "I can't find the button" failures. The scoring heuristic helps but isn't perfect.
→ **File issue**: Make element cap configurable (constructor param or config value). Consider model-tier-aware caps (more elements for Opus, fewer for Haiku).

**Issue 2: Flat element list loses hierarchy — MEDIUM**
Elements are collected depth-first but emitted flat with sequential IDs. The model has no way to understand containment ("this button is inside a dialog" vs "this button is on the main screen"). OpenClaw's Playwright snapshots preserve tree structure (ARIA tree), giving the model much better spatial reasoning.
→ **File issue**: Consider optional hierarchy hints (depth level, parent ID) in ScreenElement. Not blocking but would significantly improve complex app navigation.

**Issue 3: No bounds/position in prompt text — LOW**
`toPromptText()` outputs text, description, clickability — but not position. The model can't reason about spatial layout ("the button at the top" vs "the button at the bottom"). This is fine for simple tasks but limits complex navigation.
→ Defer — bounds data IS captured in ScreenElement, just not surfaced in the prompt. Can be added later when needed.

**Issue 4: 50-char text truncation — LOW**
`it.take(50)` and `it.take(30)` are hardcoded. Long text fields get silently truncated. The model might not realize content continues. Low priority — most UI labels are short.

**Verdict: Two medium issues worth filing. Core architecture is sound.**
The window-aware filtering and self-package rejection are well-implemented. `pickBestWindow()` is cleanly separated for testability.

---

## 3. Output Classification

### How It Works
- `OutputClassifier` singleton with static tool categorization
- Three visibility tiers: SHOW, SHOW_DIMMED, HIDE
- Three verbosity levels: VERBOSE, NORMAL, MINIMAL
- Classification rules: errors → SHOW, mechanical tools → HIDE, prominent tools → SHOW, rest → SHOW_DIMMED
- `applyVerbosity()` overrides: VERBOSE shows all, MINIMAL hides SHOW_DIMMED
- `formatForDisplay()`: emoji prefixes (🤖 SHOW, 💭 think, ⚙️ other dimmed, null for hidden)

### Findings

**Architectural pattern: good**
Clean separation of classification logic from rendering. The three-tier model maps well to phone UX constraints (limited screen space in overlay). Verbosity override is a nice user-facing control.

**Issue 1: Static tool sets need maintenance — LOW (tracked: #491)**
`MECHANICAL_TOOLS` and `PROMINENT_TOOLS` are hardcoded sets. New tools (web_search, web_fetch, etc.) aren't classified and fall to default SHOW_DIMMED. This works but isn't intentional — web_search results should probably be SHOW.
→ Already tracked as #491 (dynamic tool descriptions). Could also be fixed by just adding new tools to the sets.

**Issue 2: Legacy string-prefix error detection — LOW**
`result.startsWith("Failed")` is fragile. Now that we have `isError` flag (PR #496), the string check is legacy compat only. Should be removed once all callers use typed ToolResult.
→ Track as part of #486 (after-tool hooks) or a separate cleanup issue.

**Issue 3: Audio/voice classification TODO — NOTED**
Code has a TODO for audio mode (ANNOUNCE/OPTIONAL/SILENT). Relevant for Voice I/O MVP on the H2 roadmap. Not an issue — just noting it's ready for expansion.

**Issue 4: web_search/web_fetch not in PROMINENT_TOOLS — LOW**
API tools (#497) added web_search and web_fetch, but OutputClassifier wasn't updated. These fall to default SHOW_DIMMED when they should arguably be SHOW (user wants to see search results).
→ **File issue**: Add web_search, web_fetch to PROMINENT_TOOLS.

**Verdict: No critical issues. One small fix worth filing (web tools classification).**
The architecture is extensible and the existing patterns are clean.

---

## Summary

| Area | Critical | High | Medium | Low | Verdict |
|------|----------|------|--------|-----|---------|
| Overlay Auto-Hide | 0 | 0 | 0 | 3 | ✅ Solid — no changes needed |
| Screen Content Model | 0 | 0 | 2 | 2 | ⚠️ Two medium issues to file |
| Output Classification | 0 | 0 | 0 | 3 | ✅ Clean — one small fix |

### Issues to File
1. **MEDIUM**: Screen element cap should be configurable (#element-cap)
2. **MEDIUM**: Screen content hierarchy info for better LLM spatial reasoning (#hierarchy-hints)
3. **LOW**: Add web_search/web_fetch to PROMINENT_TOOLS (#web-tools-classification)
4. **LOW**: Remove legacy string-prefix error detection once all callers use isError (#legacy-error-strings)
