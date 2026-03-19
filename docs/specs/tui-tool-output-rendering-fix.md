# TUI Tool Output Rendering Fix

**Status:** Ready for implementation  
**Priority:** High — breaks usability when experiments run  
**Repo:** `abbudjoe/fawx`, branch from `dev`

---

## Bug

When the TUI renders tool call output (especially from `run_experiment`), the text overflows, overlaps, and corrupts the display. This is a recurring ratatui rendering bug.

**Symptoms (visible in attached screen recording):**
1. Tool call argument JSON renders as a massive unbroken block
2. Long lines overflow the terminal width and overlap with adjacent content
3. Subagent internal monologue (multi-paragraph reasoning) floods the transcript
4. Text from the tool output area bleeds into the experiment side panel
5. After enough tool output, the entire display becomes garbled/unreadable

**When it happens:**
- `run_experiment` tool is called from chat
- Tool arguments are large (hypothesis, signal description, scope paths)
- Tool output includes the subagent's full reasoning and code analysis
- Any tool call with verbose output can trigger this

---

## Root Cause

The transcript rendering in `tui/src/app.rs` does not properly constrain tool call entries to the available width. Specifically:

1. **Tool call arguments** are rendered as raw JSON — no truncation, no wrapping
2. **Tool output** (the result text) is rendered as a single block — long lines are not wrapped to the `Rect` width
3. The ratatui `Paragraph` widget needs explicit `Wrap { trim: false }` and the text must be pre-split into lines that fit the available width
4. When the experiment side panel is visible (70/30 split), the transcript area is narrower, making overflow worse

---

## Files to Change

### Primary: `tui/src/app.rs`
- `render_transcript()` — where entries are rendered into `Line`/`Span` elements
- Tool call entries (ToolUse, ToolResult) need width-aware rendering
- Look for where `BackendEvent::ToolUse` and `BackendEvent::ToolResult` are converted to `Entry` items

### Secondary: `tui/src/embedded_backend.rs`
- `handle_stream_event()` — where streaming events become `BackendEvent`s
- Tool call arguments could be truncated/summarized here before reaching the transcript

### Reference: `tui/src/experiment_panel.rs`
- This module already handles wrapping correctly (it was built with width-awareness)
- Use the same patterns for tool output in the transcript

---

## Expected Behavior

### Tool Call Display (ToolUse entry)
```
▶ run_experiment
  signal: "Low success-to-decision ratio..."
  hypothesis: "Retry storms from run_command..."
  nodes: 2, mode: subagent
```
- Show tool name prominently
- Summarize key arguments (first ~80 chars of each string value)
- Truncate with `...` if over limit
- Max 4-5 lines total for the tool call header

### Tool Output Display (ToolResult entry)
```
✓ run_experiment (success)
  Round 1/2 complete — score: 0.85
  Decision: ACCEPT
  [full output: 247 lines — scroll in panel →]
```
- Show success/failure status
- For experiment results: extract the decision and score
- For all tool output: wrap to available width
- If output exceeds ~20 lines, truncate with a note pointing to the side panel or offering scroll

### General Rules
- ALL text in the transcript must respect the `Rect` width
- Use `textwrap` or manual line splitting — never rely on ratatui to wrap raw strings
- The `Wrap { trim: false }` widget option helps but is not sufficient for pre-formatted text with ANSI or mixed content
- Test at multiple terminal widths: 80, 120, 200 columns
- Test with the experiment panel visible (narrower transcript area)

---

## Implementation Notes

### How transcript entries work
1. `BackendEvent`s arrive from the backend (streaming or complete)
2. They're converted to `Entry` structs and pushed to `self.entries`
3. `render_transcript()` iterates entries and builds `Vec<Line>` for a `Paragraph`
4. The `Paragraph` is rendered into the transcript `Rect`

### The width problem
The `Rect` width is known at render time but NOT at entry creation time. So either:
- **Option A:** Store raw text in entries, wrap at render time (using `self.transcript_area.width`)
- **Option B:** Pre-wrap when creating entries, re-wrap on terminal resize

Option A is cleaner. The `render_transcript()` function already has access to the area width.

### Key function to modify
Look for the function that converts an `Entry` to `Vec<Line>` — this is where width-aware wrapping needs to happen. If tool output is currently rendered as a single `Line` with one giant `Span`, it needs to be split into multiple `Line`s, each fitting within `width - 2` (accounting for borders/padding).

### What NOT to do
- Don't hide tool calls entirely — users need to see what tools ran
- Don't add horizontal scrolling — ratatui support is limited and UX is bad
- Don't truncate silently — always indicate when content is truncated
- Don't break the non-experiment tool rendering (read, write, exec, etc.)

---

## Testing

1. Run `fawx chat` with `--features embedded`
2. Ask it to run an experiment: "run an experiment on fx-consensus scoring"
3. Verify: tool call arguments are summarized, not raw JSON
4. Verify: tool output wraps cleanly within the transcript area
5. Verify: no text bleeds into the side panel
6. Resize terminal during experiment — verify re-render is clean
7. Test at 80 columns (minimum) — everything must still be readable
8. Test without experiment (normal tool calls like `read`, `exec`) — still renders correctly

---

## Constraints

- Follow ENGINEERING.md: no functions >40 lines, no .unwrap() outside tests, tests for new rendering logic
- Don't touch the experiment panel — it already works correctly
- Don't change BackendEvent types — fix is in the rendering layer
- Keep the fix in the TUI crate — don't change engine crates for a display bug
