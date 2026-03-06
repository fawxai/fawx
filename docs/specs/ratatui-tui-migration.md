# Spec: Ratatui TUI Migration (#1145)

**Status:** Draft
**Author:** Clawdio
**Issue:** #1145
**Branch:** `feat/ratatui-tui`

---

## 1. Problem

The current TUI uses raw ANSI escape sequences (DECSTBM scroll regions, manual cursor positioning) for layout. After 4 rounds of fixes, the persistent input bar still fails:

- Cursor escapes input bar during streaming
- Separator leaks into scroll output
- Input text invisible during execution
- Each fix introduces new escape sequence edge cases

The fundamental issue: raw ANSI escapes require every output path to maintain perfect cursor state. One missed save/restore breaks the layout. This is unsustainable.

## 2. Solution

Replace the rendering layer with [ratatui](https://ratatui.rs) — a Rust TUI framework providing frame-based rendering, declarative layout, and native resize handling. Keep all non-UI logic unchanged.

### Why ratatui
- **Frame-based rendering**: entire screen redraws each tick — no stale cursor state
- **Declarative layout**: split regions via `Layout::vertical()`, not manual row math
- **Resize handling**: built-in SIGWINCH support
- **Cursor management**: framework-owned, no manual positioning
- **Crossterm backend**: we already depend on crossterm
- **MIT licensed**, actively maintained, 10k+ GitHub stars

## 3. Architecture

### 3.1 Layout

```
┌──────────────────────────────────────┐
│                                      │
│           Output Region              │  ← Scrollable, auto-grows
│   (streaming, tool calls, results)   │
│                                      │
├──────────────────────────────────────┤  ← Separator (amber ─)
│ you ›  [input text]                  │  ← Input bar (grows with content)
└──────────────────────────────────────┘
```

Three vertical regions via `Layout::vertical()`:
1. **Output** (`Min(1)`): scrollable output area — all agent content renders here
2. **Separator** (`Length(1)`): amber `─` line, fixed height
3. **Input** (`Length(n)`): prompt + input text, grows dynamically with wrapped text

### 3.2 New module: `fx-cli/src/ui.rs`

Replaces `scroll_region.rs`. Contains:

```rust
pub struct FawxApp {
    /// All output lines (agent responses, tool calls, etc.)
    output_lines: Vec<Line<'static>>,
    /// Current scroll offset (0 = bottom/latest)
    scroll_offset: usize,
    /// Current input text (synced from readline or tui-textarea)
    input_text: String,
    /// App state: Idle | Executing | Streaming
    state: AppState,
    /// Terminal width/height (updated on resize events)
    terminal_size: (u16, u16),
}

pub enum AppState {
    /// Waiting for user input — input bar active, cursor in input
    Idle,
    /// Agent executing — input bar dimmed, spinner in output
    Executing { spinner_frame: usize },
    /// Streaming response — tokens appending to output
    Streaming,
}
```

### 3.3 Rendering (`ui.rs::draw()`)

```rust
fn draw(frame: &mut Frame, app: &FawxApp) {
    let input_height = calculate_input_height(&app.input_text, frame.area().width);
    let layout = Layout::vertical([
        Constraint::Min(1),         // output
        Constraint::Length(1),      // separator
        Constraint::Length(input_height), // input bar
    ]).split(frame.area());

    render_output(frame, layout[0], app);
    render_separator(frame, layout[1]);
    render_input(frame, layout[2], app);
}
```

Key: `draw()` is called every frame. No persistent state to corrupt. Resize = next frame draws correctly.

### 3.4 Input handling

**Option A (recommended): tui-textarea**
- Drop rustyline, use [tui-textarea](https://github.com/rhysd/tui-textarea) — a ratatui-native text input widget
- Handles multi-line, wrapping, cursor movement, history
- Renders inside the ratatui frame — no fighting between two terminal-control libraries
- ~10KB dep, MIT licensed

**Option B: rustyline hybrid**
- Keep rustyline for input, surrender terminal to rustyline during Idle state
- Reclaim terminal for ratatui rendering during Executing/Streaming
- Risk: two libraries fighting for terminal control. This is what causes the current bugs.

**Recommendation: Option A.** The core problem is two terminal controllers (rustyline + our escape sequences) fighting. Ratatui + tui-textarea = one controller.

**Feature parity note**: tui-textarea does NOT provide command completion or history hinting out of the box. These must be implemented manually:

- **History**: `Vec<String>` buffer with Up/Down arrow navigation. Persist to a history file (`~/.fawx/history`) on exit, load on startup. tui-textarea provides the text editing; we provide the history layer on top.
- **Command completion**: Implement `/` prefix matching manually. On Tab with input starting with `/`, match against known commands (`/model`, `/config`, `/stop`, `/steer`, `/help`, etc.) and cycle through matches.
- **Multi-line editing and cursor movement**: provided by tui-textarea natively.

This is custom code — budget for it in Phase 1.

### 3.5 Event loop

The event loop uses **hybrid event-driven / frame-based rendering** to avoid burning CPU when idle:

- **Idle state**: block on `crossterm::event::poll()` with no timeout (infinite wait). Only redraw when an input event, resize, or agent channel message arrives. Zero CPU when the user isn't typing and the agent isn't running.
- **Executing / Streaming states**: poll with 50ms timeout (20fps) to pick up spinner animation and streaming tokens.

```rust
loop {
    // 1. Draw frame
    terminal.draw(|frame| draw(frame, &app))?;

    // 2. Determine poll timeout based on state
    let poll_timeout = match app.state {
        AppState::Idle => Duration::from_secs(60), // effectively blocking; agent_rx wakes us
        AppState::Executing { .. } | AppState::Streaming => Duration::from_millis(50),
    };

    // 3. Poll terminal events (keyboard, resize)
    if crossterm::event::poll(poll_timeout)? {
        match crossterm::event::read()? {
            Event::Key(key) => handle_key(key, &mut app),
            Event::Resize(w, h) => app.terminal_size = (w, h),
            _ => {}
        }
    }

    // 4. Drain ALL available agent channel messages per frame.
    //    Single try_recv() per frame lags during streaming — tokens arrive
    //    faster than 20fps. Drain the entire channel each iteration.
    while let Ok(msg) = agent_rx.try_recv() {
        handle_agent_message(msg, &mut app);
    }
}
```

**Note on idle wake-up**: In Idle state with a long poll timeout, agent channel messages (e.g., background tool completion) won't render until the next terminal event. If sub-second responsiveness is needed for agent messages during idle, use a secondary waker mechanism (e.g., write to a self-pipe on channel send) or fall back to a moderate timeout (1-2s).

### 3.6 Panic hook (terminal cleanup)

Raw mode cleanup on panic is a standard ratatui requirement. If the process panics without restoring the terminal, the user's shell is left in raw mode (no echo, no line editing) — effectively bricked until they run `reset`.

Install a panic hook at startup that:
1. Disables raw mode (`crossterm::terminal::disable_raw_mode()`)
2. Shows the cursor (`crossterm::cursor::Show`)
3. Then runs the default panic handler (print backtrace + abort)

```rust
let original_hook = std::panic::take_hook();
std::panic::set_hook(Box::new(move |panic_info| {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
    original_hook(panic_info);
}));
```

Alternative: use `color-eyre` which provides this automatically along with better error reports. Either approach is fine — the requirement is that raw mode is always cleaned up.

### 3.7 Alternate screen: NO

**Decision: Do NOT use alternate screen.**

Reasoning:
- Output stays visible after exit — matches current behavior and user expectations for a chat tool
- Users can scroll back in their terminal emulator to review past agent output
- Alternate screen hides ALL output on exit — terrible UX for a conversational tool where you might want to copy/paste previous responses
- Standard for chat/REPL-style tools (Python REPL, node, irb all preserve output on exit)

This means we render directly to the main screen buffer. On exit, the last frame's content remains visible in scrollback.

### 3.8 Output rendering

Agent output is stored as `Vec<Line>` (ratatui styled text). Each output type maps to styled lines:

| Content | Style |
|---------|-------|
| Agent text | Default, markdown-rendered |
| Tool call | Dim + tool name header |
| Tool result | Indented, syntax highlighted |
| User echo | Shaded background (236) |
| Spinner | Amber, animated |
| Error | Red |

Scrollback: output region is a `Paragraph` widget with `.scroll((offset, 0))`. User can scroll with Shift+Up/Down (or Page Up/Down).

### 3.9 What stays unchanged

- **All kernel logic**: loop engine, tool execution, streaming, memory
- **All skill/plugin logic**: fx-skills, fx-loadable, fx-scratchpad
- **Agent channel protocol**: how tui.rs communicates with the agent loop
- **Markdown rendering**: `markdown.rs` output → convert to ratatui `Line`/`Span`
- **Commands**: `/model`, `/config`, `/stop`, `/steer`, `/help` — parsing stays, rendering adapts
- **Auth store, config bridge**: untouched
- **Hero art**: convert ANSI art to ratatui spans

## 4. Migration plan

### Phase 0: Pre-requisites
1. Bump workspace `crossterm` dep from 0.27 → 0.28 (ratatui 0.29 requires crossterm 0.28+)
2. Audit crossterm 0.27→0.28 API changes — update any direct crossterm usage in fx-cli
3. Verify existing TUI still builds and works with crossterm 0.28

### Phase 1: Core rendering (this PR)
1. Add `ratatui` + `tui-textarea` deps
2. Create `ui.rs` with `FawxApp`, `AppState`, `draw()`
3. Replace the main event loop in `tui.rs` to use ratatui's `Terminal::draw()`
4. Migrate output rendering (plain text + spinner)
5. Replace rustyline with tui-textarea for input
6. Delete `scroll_region.rs`
7. Verify: startup, basic chat, tool calls, streaming, resize

### Phase 2: Polish (same PR or follow-up)
1. Markdown rendering → ratatui styled spans
2. Hero art rendering
3. Shaded user echo
4. Amber separator styling
5. Scrollback navigation
6. History persistence for tui-textarea

## 5. Dependencies

```toml
# Add to fx-cli/Cargo.toml
ratatui = "0.29"           # TUI framework
tui-textarea = "0.7"       # Input widget for ratatui

# Remove
# rustyline = "15"         # Replaced by tui-textarea
```

Note: ratatui uses crossterm as backend by default — we already have crossterm. The workspace crossterm dep must be bumped from 0.27 → 0.28 first (see Phase 0).

## 6. Risks

| Risk | Mitigation |
|------|-----------|
| tui-textarea missing rustyline features | Check: history, completion, key bindings. All available. |
| Large diff on tui.rs | Scope Phase 1 to rendering replacement only. Logic stays. |
| Streaming perf with frame-based rendering | 50ms poll interval = 20fps, sufficient for token streaming |
| ratatui version compatibility | Pin version, crossterm version must match |
| Supply chain — ratatui (low-medium) | ratatui is render-only (no network/FS/unsafe beyond crossterm). Low immediate risk. Long-term: fork and maintain in-house to own the supply chain. |
| Supply chain — tui-textarea (medium) | Single maintainer, ~2k stars. Same long-term fork consideration as ratatui. Mitigation: the input widget is small enough (~1k LOC) to inline/fork if abandoned. Not a blocker — the library is stable and well-maintained today. |

## 6.1 Hero Art

The existing ANSI hero art renders as styled `Line`/`Span` entries in the output region on startup. ANSI color escapes map to ratatui `Style` attributes. Art can be shrunk (fewer rows/columns) to fit smaller terminals — ratatui's layout will clip gracefully regardless.

## 7. Definition of Done

- [ ] `scroll_region.rs` deleted — all rendering via ratatui
- [ ] Input bar works during idle AND execution (cursor stays in input)
- [ ] Terminal resize works without artifacts
- [ ] Streaming output renders in output region without leaking to input
- [ ] Long input text wraps correctly (multi-line input bar grows)
- [ ] User message echo with shaded background
- [ ] Spinner displays during execution
- [ ] `/stop`, `/steer`, `/help`, `/model`, `/config` commands work
- [ ] Hero art renders on startup
- [ ] All existing fx-cli tests pass
- [ ] New tests for FawxApp state transitions and layout calculation
- [ ] `cargo fmt && cargo clippy -D warnings` clean

## 8. Non-goals (this PR)

- Mouse support
- Split panes
- Custom themes/colors config
- Any kernel/engine changes
