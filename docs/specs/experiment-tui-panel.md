# TUI Side Panel for Experiment Progress

## Problem

When running experiments from the TUI (`/experiment run ...`), there's no visual feedback. Users stare at a blank chat area for minutes.

## Solution

A side panel that auto-shows when an experiment starts and auto-hides when it completes. Uses the `ProgressCallback` from the verbose feature to receive events.

## Design

### Panel layout

```
┌─────────────────────────────┬──────────────────────┐
│                             │  ⚗ Experiment         │
│  Chat area                  │  Round 1/5            │
│                             │                      │
│                             │  ▸ node-0 generating │
│                             │  ✓ Baseline: 71 (2s) │
│                             │                      │
│                             │  Scores:             │
│                             │  (pending...)        │
│                             │                      │
│                             │  ──────────────      │
│                             │  Chain: 0 entries    │
│                             │  Signal: missing     │
│                             │   tests              │
├─────────────────────────────┴──────────────────────┤
│ > /experiment run ...                               │
└─────────────────────────────────────────────────────┘
```

### Behavior

- **Auto-show**: panel appears when experiment starts (first `RoundStarted` event)
- **Auto-hide**: panel disappears 3 seconds after final event (`RoundComplete` with terminal decision)
- **Toggle**: `/experiment` command toggles panel visibility manually
- **Width**: fixed 24 columns, right-aligned
- **Scrollback**: panel keeps last ~20 lines of progress, auto-scrolls

### Widget

New `ExperimentPanel` widget in `tui/src/`:

```rust
pub struct ExperimentPanel {
    lines: Vec<StyledLine>,
    visible: bool,
    auto_hide_at: Option<Instant>,
}

impl ExperimentPanel {
    pub fn on_progress(&mut self, event: &ProgressEvent);
    pub fn toggle(&mut self);
    pub fn should_hide(&self) -> bool;
}
```

### Integration

The TUI's `FawxApp` gets an `experiment_panel: ExperimentPanel` field. When running an experiment via the tool executor:

1. Tool executor creates a `ProgressCallback` that sends events through a `tokio::sync::mpsc` channel
2. TUI event loop polls the channel and calls `experiment_panel.on_progress(event)`
3. Panel renders in `draw()` if visible

### Event rendering

Uses the same `format_progress_event` from the verbose feature (format.rs), adapted for ratatui styled spans with colors.

## Files to modify

1. `tui/src/experiment_panel.rs` — new file, widget + state
2. `tui/src/app.rs` — add panel field, wire draw(), handle `/experiment` toggle
3. `tui/src/embedded_backend.rs` — layout split for panel

## Scope

~200 lines new code. Single PR. Depends on #1368 (verbose/progress callbacks).
