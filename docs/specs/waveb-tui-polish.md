# Spec: Wave B — TUI Polish (Welcome Screen + Memory Count)

**Phase:** 3c (Polish)  
**Status:** Ready  
**Date:** 2026-03-10

---

## Scope

This work item bundles the two TUI-facing Wave B tasks that touch the same surface area:

1. **Welcome screen redesign**
2. **`mem 0` status-bar bug fix**

Use `docs/specs/tui-welcome-screen.md` as the baseline design doc. This spec adds implementation constraints and the memory-count requirement.

---

## Goals

### 1) Welcome screen redesign
Implement the welcome screen described in `docs/specs/tui-welcome-screen.md`.

Key requirements:
- Replace the current startup hero/system-message presentation with the new responsive welcome layout.
- Preserve graceful behavior across narrow, medium, and wide terminal widths.
- Show installed skills dynamically.
- Keep the welcome screen as normal transcript content that scrolls away naturally after conversation begins.
- Do **not** add new crates.

### 2) Fix `mem 0` in the header/status bar
The TUI header currently shows `mem 0` even when memory is working.

Fix requirement:
- The header must show the real memory entry count in embedded mode, not a hardcoded zero.
- Reuse existing counting logic/patterns where possible instead of inventing a second memory model.
- If memory storage is missing/unavailable, fall back cleanly to `0` without erroring.

---

## File boundaries

Expected primary files:
- `tui/src/app.rs`
- `tui/src/embedded_backend.rs`
- `tui/src/fawx_backend.rs` only if needed for small supporting changes

Avoid broad unrelated cleanup.

---

## Design constraints

### Welcome layout
Follow `docs/specs/tui-welcome-screen.md` for:
- wide / medium / narrow responsive behavior
- command list content
- skill list behavior
- version display
- color/styling intent

Pragmatic rule: if the exact visual treatment needs small adjustment to fit the current TUI architecture cleanly, prefer the simpler implementation that still matches the spirit of the spec.

### Skill discovery
- Read installed skills from the real Fawx skills directory.
- Avoid blocking/failing the TUI when the skills directory is absent or malformed.
- If manifests/icons are missing, degrade gracefully.

### Memory count
- The embedded backend must report a real `memory_entries` value to the TUI.
- Prefer sharing or mirroring the same filesystem-based count semantics already used by CLI status rather than introducing a separate in-memory counter.

---

## Tests

Required regression coverage:

### Welcome screen
- wide layout renders expected sections
- medium layout stacks appropriately
- narrow layout omits the mascot/art path if required by width
- empty skills directory shows placeholder text
- long skills list truncates and shows overflow indicator if specified by implementation

### Memory count bug
- embedded backend health/status reports a non-zero count when memory storage contains entries
- missing memory storage reports zero without error

If snapshot tests are a good fit for layout rendering, use them. If not, assert on stable text fragments/sections.

---

## Done criteria

This task is done when:
- the new welcome screen is visible on startup
- the layout adapts across terminal widths
- installed skills appear (or a sensible empty state appears)
- embedded-mode header no longer lies with `mem 0` when memory exists
- tests cover both the redesign and the regression bug
