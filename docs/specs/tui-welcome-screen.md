# Spec: TUI Welcome Screen Redesign

**Phase:** 3c (Polish)  
**Status:** Draft  
**Author:** Clawdio  
**Date:** 2026-03-10

---

## Problem

The current TUI welcome screen shows ASCII-rendered hero art (the old fiery Fawx logo) with a single "Use /help to see available commands" system message. It doesn't surface useful information, and the old branding is being replaced with the new fox mascot.

## Solution

Redesign the TUI welcome screen with a three-column layout: fox mascot on the left, slash commands in the center-right, installed skills on the far right.

## Layout

```
┌───────────────────────────────────────────────────────────────────────┐
│                                                                       │
│   ▄▄▄   ▄▄▄                                                          │
│  █   █ █   █       Commands               Skills                     │
│  █▀▀▀█▄█▀▀▀█       /help    overview       🌤  weather               │
│   █ ▀▀▀▀▀ █        /model   switch LLM     👁  vision                │
│    █ ● ● █         /skills  list skills     🔊  tts                  │
│     █ ▼ █          /clear   clear chat      🌐  browser              │
│      ███           /status  engine info     🖼  canvas               │
│     █████          /quit    exit            🎤  stt                  │
│                                                                       │
│  Fawx v0.1.0                                                         │
│  Ask Fawx anything...                                                │
│                                                                       │
└───────────────────────────────────────────────────────────────────────┘
```

## Design Details

### Left column: Fox mascot
- Render `tui/assets/fawx-mascot.png` (transparent background fox) as half-block pixel art using the existing `render_logo_art` approach
- Target height: ~10 terminal rows
- Target width: ~20 terminal columns
- Below the art: version string "Fawx v{VERSION}" in dim text

### Center-right column: Slash commands
- Header: "Commands" in bold/bright
- Show the 5-6 most useful commands with brief descriptions
- Format: `/command` in accent color (orange), description in dim text
- Commands to show:
  - `/help` overview
  - `/model` switch LLM
  - `/skills` list skills
  - `/clear` clear chat
  - `/status` engine info
  - `/quit` exit

### Far-right column: Installed skills
- Header: "Skills" in bold/bright
- Dynamically list installed WASM skills from `~/.fawx/skills/`
- Format: emoji + skill name
- If no skills installed, show "No skills installed. Run fawx install to browse."
- Max 8 skills shown. If more, show count: "+3 more"

### Responsive behavior
- **Wide terminal (>100 cols)**: Full three-column layout as shown
- **Medium terminal (60-100 cols)**: Two columns. Fox on left, commands + skills stacked vertically on right
- **Narrow terminal (<60 cols)**: No fox art. Just commands list, then skills list, stacked vertically

### Colors
- Fox art: rendered in true color if terminal supports it, otherwise orange/amber ANSI
- Command names: orange/amber (Fawx brand color)
- Headers: bold white
- Descriptions: dim/gray
- Version string: dim

### Scrolling behavior
- Welcome screen is the initial content in the transcript
- Scrolls up naturally as conversation begins
- Not pinned or sticky

## Implementation

### Files to modify
- `tui/src/app.rs`: Replace `initial_entries()` and `render_logo_art()` with new welcome layout
- `tui/assets/fawx-mascot.png`: New mascot image (already saved)
- `tui/assets/fawx.png`: Keep as fallback, but `fawx-mascot.png` is primary

### New function: `render_welcome_screen(width, skills)`
Returns a `Vec<Entry>` containing the formatted welcome screen. Takes terminal width for responsive layout and a list of installed skill names.

### Skill discovery
Read `~/.fawx/skills/*.toml` manifest files to get skill names and icons. The manifest already has a `name` field. Add an optional `icon` field (emoji) to manifest schema, with sensible defaults based on skill name.

### Dependencies
- No new crates needed. The existing `image` + half-block rendering handles the mascot art.
- Skill discovery uses `std::fs::read_dir` on the skills directory.

## Testing

1. **Wide layout renders three columns** at width 120
2. **Medium layout renders two columns** at width 80
3. **Narrow layout omits fox art** at width 50
4. **No skills installed shows placeholder message**
5. **Skills list truncates at 8 with "+N more" indicator**
6. **Version string matches build version**

## Not in scope
- Animated mascot (future)
- Interactive welcome screen (clicking commands)
- Custom welcome message from config
