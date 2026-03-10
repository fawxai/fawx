# TUI Shell Spec — PR 9.5

## Goal
Transform the current minimal TUI into a polished, modern CLI agent interface on par with OpenClaw / Claude Code / Codex CLI.

---

## 1. Hero Banner

ASCII art displayed on startup. Placeholder for now — Joe to finalize design.

```
 ██████╗██╗████████╗██████╗  ██████╗ ███████╗
██╔════╝██║╚══██╔══╝██╔══██╗██╔═══██╗██╔════╝
██║     ██║   ██║   ██████╔╝██║   ██║███████╗
██║     ██║   ██║   ██╔══██╗██║   ██║╚════██║
╚██████╗██║   ██║   ██║  ██║╚██████╔╝███████║
 ╚═════╝╚═╝   ╚═╝   ╚═╝  ╚═╝ ╚═════╝ ╚══════╝
```

Below the hero:
```
  v0.9.0 | model: gpt-5.3-codex | provider: openai (subscription)
  Type /help for commands.
```

> **Note**: If we rename to fawx or something else, the ASCII art changes. Keep it in a single const so it's one-line swap.

---

## 2. Prompt

```
you › what's the capital of france?
```

- Colored prompt label (`you ›` in green)
- Input on same line
- Multi-line input support (paste detection or shift+enter if terminal supports it)

---

## 3. Response Rendering

### Assistant output
```
assistant › Paris is the capital of France.
```

- `assistant ›` label in cyan
- **Markdown rendering in terminal**:
  - **Bold**, *italic* via ANSI
  - `inline code` with background highlight
  - Code blocks with syntax highlighting (via `syntect` or similar)
  - Bulleted/numbered lists
  - Headers rendered with bold + color

### Streaming
- Tokens appear character-by-character after the `assistant ›` label
- Cursor stays at end of stream
- Newline after stream completes

### Thinking indicator
```
assistant › ⠋ thinking...
```
- Spinner while waiting for first token
- Replaced by streaming text once tokens arrive

---

## 4. Loop Metadata

After each response, show a subtle info line:

```
  ↳ 1 iteration · 153 in / 102 out tokens · 1.2s
```

- Gray/dim color
- Shows: iterations, token usage, wall time
- Only shown when response completes successfully

---

## 5. Tool Call Visualization (future, stub for now)

When tools are invoked (PR 10+):
```
  ⚡ using tool: read_file("src/main.rs")
  ✓ tool returned 42 lines
```

- Tool name + args in yellow
- Result summary in dim
- Errors in red

---

## 6. Error Display

```
  ✗ Error: budget exhausted after 3 iterations
```

- Red `✗` prefix
- Error message in red
- No stack traces in normal mode (behind `--verbose` flag)

---

## 7. Status Bar (bottom)

Optional persistent status bar (if terminal supports it):
```
 model: gpt-5.3-codex │ tokens: 255/250,000 │ budget: 98% │ /help for commands
```

**Decision needed**: persistent status bar vs. on-demand via `/status` command. Start with `/status` command — simpler, no terminal compatibility issues.

---

## 8. Commands

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/model` | List and switch models |
| `/auth` | Show credentials / run auth wizard |
| `/budget` | Show budget status |
| `/status` | Show model, tokens used, session info |
| `/clear` | Clear screen |
| `/quit` | Exit |

---

## 9. Color Palette

| Element | Color |
|---------|-------|
| User prompt label (`you ›`) | Green |
| Assistant label (`assistant ›`) | Cyan |
| System messages | Yellow |
| Errors | Red |
| Loop metadata | Dim/Gray |
| Tool calls | Yellow |
| Tool results | Dim |
| Code blocks | Syntax highlighted |
| Inline code | White on dark gray bg |
| Commands in `/help` | Bold white |

---

## 10. Rust Crates

| Crate | Purpose |
|-------|---------|
| `crossterm` | Already used — terminal control, colors, cursor |
| `syntect` | Syntax highlighting for code blocks |
| `textwrap` | Line wrapping respecting terminal width |
| `indicatif` | Spinner for thinking state |
| `unicode-width` | Correct width calculation for CJK/emoji |

Evaluate: `termimad` (markdown-to-terminal) vs manual ANSI rendering. `termimad` is simpler but less control. Manual gives us exact OpenClaw-style rendering.

---

## 11. Implementation Plan

1. **Hero banner** — const string, printed on startup with version/model info
2. **Prompt labels** — `you ›` / `assistant ›` with colors
3. **Spinner** — show while waiting for first token, replace with stream
4. **Loop metadata line** — iteration count, tokens, wall time
5. **Markdown rendering** — start with bold/italic/code, add syntax highlighting
6. **`/status` and `/clear` commands**
7. **Error formatting** — red prefix, clean messages

Steps 1-4 are quick wins. Step 5 is the big one.

---

## 12. What We're NOT Doing (yet)

- No persistent bottom status bar (use `/status` instead)
- No mouse support
- No split panes
- No conversation history scrollback
- No file picker / autocomplete
- No themes / config file for colors

These are all post-launch polish. Ship the basics first.

---

*This spec lives at `docs/TUI-SPEC.md`. Update as decisions are made.*
