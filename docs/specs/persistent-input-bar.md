# Persistent Input Bar (#985 — revised)

## Problem (revised understanding)

The original #985 spec was too narrow. A dimmed prompt during the spinner phase is insufficient because:
1. The spinner phase is 100-200ms — nobody sees it
2. During streaming (the long phase), the dimmed prompt gets eaten by incoming tokens
3. Typed characters during streaming get overwritten by output

The real solution is a **persistent input bar** that streaming cannot affect — like Claude Code, Hermes, OpenClaw TUI, etc.

## Solution: Fixed input bar at terminal bottom

### Architecture

Use ANSI terminal scroll region (DECSTBM) to split the terminal into two zones:
1. **Scroll region** (top) — all output (spinner, streaming, tool results, response) scrolls here
2. **Input bar** (bottom 1-2 lines) — fixed, never scrolls, always visible

### ANSI escape sequences

```
\x1b[1;{height-2}r    — Set scroll region to rows 1 through (height-2)
\x1b[{height-1};1H    — Position cursor at the input bar row
\x1b[{height};1H      — Position cursor at the status bar row (optional)
```

### Input bar content

During different phases:
- **Idle (readline active):** `you ›` with cursor, full input editing
- **Thinking (spinner):** `you ›` (dimmed) — spinner runs in scroll region above
- **Streaming:** `you ›` (dimmed) — tokens stream in scroll region above
- **Tool execution:** `you ›` (dimmed) — tool output in scroll region above

### Key behaviors

1. Input bar NEVER moves — it's pinned to terminal bottom
2. All output (spinner, streaming, tool results) renders in the scroll region
3. When readline is active, cursor is in the input bar
4. Terminal resize: re-calculate scroll region, re-render input bar

### Implementation phases

**Phase 1 (this PR):** 
- Set scroll region on TUI start
- Move all output (eprint/eprintln) to render in scroll region
- Pin `you ›` at bottom
- Handle terminal resize (SIGWINCH)
- Streaming renderer outputs to scroll region only

**Phase 2 (#930):**
- Accept input during execution (background crossterm reader)
- Steer/abort via input bar during streaming

## Also in this PR

### Rename 'assistant' → 'fawx'

Change the `assistant ›` output prefix to `fawx ›` throughout the TUI.

### New hero art

Replace the current startup hero/logo with the new Braille fox art from `docs/fawx-hero.txt`. The RTF source with color data is preserved for future color rendering.

## Files to modify

| File | Changes |
|---|---|
| `engine/crates/fx-cli/src/tui.rs` | Scroll region setup, input bar pinning, output routing, resize handler, assistant→fawx rename |
| `docs/fawx-hero.txt` | New hero art (already saved) |

## Branch

Reuse `feat/prompt-rerender-985` branch. Force-push the new approach.

## References

- Claude Code: uses alternate screen with persistent input
- OpenClaw TUI: similar fixed input bar
- crossterm: `terminal::size()`, `SetScrollRegion`, cursor positioning
