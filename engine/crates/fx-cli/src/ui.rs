//! Ratatui-based UI module for Fawx TUI.
//!
//! Provides [`FawxApp`] state management and [`draw`] rendering.
//! This module is standalone — it does not depend on the existing
//! `tui.rs` event loop or `scroll_region.rs`.

use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
    Frame,
};

// ── Styling constants ──────────────────────────────────────────

/// Amber accent colour used for separator and prompt.
const AMBER: Color = Color::Rgb(255, 204, 0);

/// Dark-gray background for the input region.
const INPUT_BG: Color = Color::Indexed(236);

/// Prompt prefix shown in the input bar.
const PROMPT: &str = "you › ";

// ── AppState ───────────────────────────────────────────────────

/// High-level state of the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    /// Waiting for user input.
    Idle,
    /// Agent is executing a tool / thinking. Spinner rendered in output.
    Executing { spinner_frame: usize },
    /// Streaming tokens from the model.
    Streaming,
}

// ── FawxApp ────────────────────────────────────────────────────

/// Core application state for the ratatui TUI.
pub struct FawxApp {
    pub output_lines: Vec<String>,
    pub scroll_offset: usize,
    pub input_text: String,
    pub state: AppState,
    pub command_history: Vec<String>,
    pub history_index: Option<usize>,
    /// Whether the "fawx ›" prefix has been printed for the current stream.
    pub streaming_prefix_printed: bool,
    /// Inputs queued while the agent is executing (one per future turn).
    pub pending_inputs: Vec<String>,
    /// Steer message for the running agent (last one wins).
    pub steer_message: Option<String>,
    /// Whether an abort has been requested for the current cycle.
    pub abort_requested: bool,
}

impl FawxApp {
    /// Create a new `FawxApp` with sensible defaults.
    pub fn new() -> Self {
        Self {
            output_lines: Vec::new(),
            scroll_offset: 0,
            input_text: String::new(),
            state: AppState::Idle,
            command_history: Vec::new(),
            history_index: None,
            streaming_prefix_printed: false,
            pending_inputs: Vec::new(),
            steer_message: None,
            abort_requested: false,
        }
    }

    /// Append a single line to the output buffer.
    pub fn add_output(&mut self, line: String) {
        self.output_lines.push(line);
    }

    /// Batch-append lines to the output buffer.
    #[allow(dead_code)] // TODO(#1148): Phase 3 will wire batch output into ratatui
    pub fn add_output_lines(&mut self, lines: Vec<String>) {
        self.output_lines.extend(lines);
    }

    /// Transition to a new [`AppState`].
    pub fn set_state(&mut self, state: AppState) {
        self.state = state;
    }

    /// Scroll the output region up (toward older content) by one line.
    pub fn scroll_up(&mut self) {
        let max = self.output_lines.len();
        if self.scroll_offset < max {
            self.scroll_offset += 1;
        }
    }

    /// Scroll the output region down (toward newer content) by one line.
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Jump to the bottom of output (follow latest content).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Take the current input text, push it to history, clear the
    /// input field, and return the submitted text.
    pub fn submit_input(&mut self) -> String {
        let text = std::mem::take(&mut self.input_text);
        if !text.is_empty() {
            self.command_history.push(text.clone());
        }
        self.history_index = None;
        text
    }

    /// Navigate command history upward (older entries).
    pub fn history_up(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        let idx = match self.history_index {
            Some(i) => i.saturating_sub(1),
            None => self.command_history.len() - 1,
        };
        self.history_index = Some(idx);
        self.input_text = self.command_history[idx].clone();
    }

    /// Navigate command history downward (newer entries).
    pub fn history_down(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        if idx + 1 >= self.command_history.len() {
            self.history_index = None;
            self.input_text.clear();
        } else {
            let next = idx + 1;
            self.history_index = Some(next);
            self.input_text = self.command_history[next].clone();
        }
    }

    /// Add a command to the history buffer.
    pub fn push_history(&mut self, entry: String) {
        self.command_history.push(entry);
        self.history_index = None;
    }

    /// Begin streaming output: add the "fawx ›" prefix line.
    pub fn start_streaming(&mut self) {
        self.add_output(String::new());
        self.add_output("fawx › ".to_string());
        self.streaming_prefix_printed = true;
        self.scroll_to_bottom();
    }

    /// Append a streaming delta to the output buffer.
    ///
    /// Handles newlines by splitting into separate output lines.
    pub fn append_streaming_delta(&mut self, delta: &str) {
        if !self.streaming_prefix_printed {
            self.start_streaming();
        }
        for ch in delta.chars() {
            if ch == '\n' {
                self.output_lines.push(String::new());
            } else if let Some(last) = self.output_lines.last_mut() {
                last.push(ch);
            }
        }
        self.scroll_to_bottom();
    }

    /// Finish streaming: reset the streaming flag.
    pub fn finish_streaming(&mut self) {
        self.streaming_prefix_printed = false;
    }

    /// Queue a user input to be executed after the current turn completes.
    pub fn queue_input(&mut self, text: String) {
        self.pending_inputs.push(text);
    }

    /// Set (or replace) the steer message. Last one wins.
    pub fn set_steer(&mut self, text: String) {
        self.steer_message = Some(text);
    }

    /// Request an abort of the running agent cycle.
    pub fn request_abort(&mut self) {
        self.abort_requested = true;
    }

    /// Drain and return the next queued input, if any.
    pub fn drain_next_input(&mut self) -> Option<String> {
        if self.pending_inputs.is_empty() {
            None
        } else {
            Some(self.pending_inputs.remove(0))
        }
    }

    /// Take the steer message, leaving `None` behind.
    pub fn take_steer(&mut self) -> Option<String> {
        self.steer_message.take()
    }

    /// Reset abort/steer/queue state for a new cycle.
    pub fn reset_cycle_state(&mut self) {
        self.abort_requested = false;
        self.steer_message = None;
        // pending_inputs are intentionally preserved across cycles
    }

    /// Calculate how many terminal rows the input bar needs given
    /// the current input text and available `width`.
    pub fn calculate_input_height(&self, width: u16) -> u16 {
        if width == 0 {
            return 1;
        }
        // Use char count (display columns) instead of byte length so
        // multi-byte characters like `›` don't inflate the calculation.
        let content_len = PROMPT.chars().count() + self.input_text.chars().count();
        (content_len as u16).div_ceil(width).max(1)
    }
}

impl Default for FawxApp {
    fn default() -> Self {
        Self::new()
    }
}

// ── Drawing ────────────────────────────────────────────────────

/// Render the entire TUI frame.
pub fn draw(frame: &mut Frame, app: &FawxApp) {
    let input_height = app.calculate_input_height(frame.area().width);

    let layout = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(input_height),
    ])
    .split(frame.area());

    render_output(frame, layout[0], app);
    render_separator(frame, layout[1], app);
    render_input(frame, layout[2], app);
}

/// Render the scrollable output region.
fn render_output(frame: &mut Frame, area: ratatui::layout::Rect, app: &FawxApp) {
    let lines: Vec<Line<'_>> = app
        .output_lines
        .iter()
        .map(|s| {
            if crate::ansi::contains_ansi(s) {
                crate::ansi::ansi_to_line(s)
            } else {
                Line::from(s.as_str())
            }
        })
        .collect();

    let total = lines.len() as u16;
    let visible = area.height;
    let scroll = if app.scroll_offset == 0 {
        total.saturating_sub(visible)
    } else {
        total
            .saturating_sub(visible)
            .saturating_sub(app.scroll_offset as u16)
    };

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}

/// Build status indicator spans for the separator line.
fn build_status_indicators(app: &FawxApp) -> Vec<Span<'_>> {
    let mut indicators: Vec<Span<'_>> = Vec::new();

    match &app.state {
        AppState::Executing { .. } => {
            if app.abort_requested {
                indicators.push(Span::styled(
                    " ⛔ Aborting... ",
                    Style::default().fg(Color::Red),
                ));
            } else {
                indicators.push(Span::styled(
                    " ⏳ Thinking... ",
                    Style::default().fg(Color::Yellow),
                ));
            }
        }
        AppState::Streaming => {
            indicators.push(Span::styled(
                " 📡 Streaming... ",
                Style::default().fg(Color::Cyan),
            ));
        }
        AppState::Idle => {}
    }

    if app.steer_message.is_some() {
        indicators.push(Span::styled(
            "↪ Steer queued ",
            Style::default().fg(Color::Magenta),
        ));
    }

    let queued = app.pending_inputs.len();
    if queued > 0 {
        indicators.push(Span::styled(
            format!("📋 {queued} queued "),
            Style::default().fg(Color::Blue),
        ));
    }

    indicators
}

/// Render the amber separator line with optional status indicators.
fn render_separator(frame: &mut Frame, area: ratatui::layout::Rect, app: &FawxApp) {
    let indicators = build_status_indicators(app);

    if indicators.is_empty() {
        let sep = "─".repeat(area.width as usize);
        let line = Paragraph::new(Line::from(Span::styled(sep, Style::default().fg(AMBER))));
        frame.render_widget(line, area);
    } else {
        let indicator_text: String = indicators.iter().map(|s| s.content.as_ref()).collect();
        let indicator_char_len = indicator_text.chars().count();
        let fill_len = (area.width as usize).saturating_sub(indicator_char_len);
        let mut spans = vec![Span::styled(
            "─".repeat(fill_len),
            Style::default().fg(AMBER),
        )];
        spans.extend(indicators);
        let line = Paragraph::new(Line::from(spans));
        frame.render_widget(line, area);
    }
}

/// Render the input bar with prompt and current text.
fn render_input(frame: &mut Frame, area: ratatui::layout::Rect, app: &FawxApp) {
    let is_active = matches!(app.state, AppState::Idle);

    let prompt_style = if is_active {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(AMBER).add_modifier(Modifier::DIM)
    };

    let text_style = if is_active {
        Style::default()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };

    let line = Line::from(vec![
        Span::styled(PROMPT, prompt_style),
        Span::styled(app.input_text.as_str(), text_style),
    ]);

    let block = Block::default().style(Style::default().bg(INPUT_BG));

    let paragraph = Paragraph::new(line).block(block).wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new_defaults() {
        let app = FawxApp::new();
        assert!(app.output_lines.is_empty());
        assert_eq!(app.scroll_offset, 0);
        assert!(app.input_text.is_empty());
        assert_eq!(app.state, AppState::Idle);
        assert!(app.command_history.is_empty());
        assert_eq!(app.history_index, None);
    }

    #[test]
    fn test_add_output() {
        let mut app = FawxApp::new();
        app.add_output("line one".into());
        app.add_output("line two".into());
        assert_eq!(app.output_lines.len(), 2);
        assert_eq!(app.output_lines[0], "line one");
        assert_eq!(app.output_lines[1], "line two");
    }

    #[test]
    fn test_add_output_lines() {
        let mut app = FawxApp::new();
        app.add_output_lines(vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(app.output_lines.len(), 3);
    }

    #[test]
    fn test_submit_input() {
        let mut app = FawxApp::new();
        app.input_text = "hello world".into();
        let submitted = app.submit_input();
        assert_eq!(submitted, "hello world");
        assert!(app.input_text.is_empty());
        assert_eq!(app.command_history.len(), 1);
        assert_eq!(app.command_history[0], "hello world");
        assert_eq!(app.history_index, None);
    }

    #[test]
    fn test_submit_empty_input_no_history() {
        let mut app = FawxApp::new();
        let submitted = app.submit_input();
        assert!(submitted.is_empty());
        assert!(app.command_history.is_empty());
    }

    #[test]
    fn test_history_navigation() {
        let mut app = FawxApp::new();
        app.input_text = "first".into();
        app.submit_input();
        app.input_text = "second".into();
        app.submit_input();
        app.input_text = "third".into();
        app.submit_input();

        // Navigate up through history (most recent first)
        app.history_up();
        assert_eq!(app.input_text, "third");
        assert_eq!(app.history_index, Some(2));

        app.history_up();
        assert_eq!(app.input_text, "second");
        assert_eq!(app.history_index, Some(1));

        app.history_up();
        assert_eq!(app.input_text, "first");
        assert_eq!(app.history_index, Some(0));

        // Can't go past the oldest
        app.history_up();
        assert_eq!(app.input_text, "first");
        assert_eq!(app.history_index, Some(0));

        // Navigate back down
        app.history_down();
        assert_eq!(app.input_text, "second");
        assert_eq!(app.history_index, Some(1));

        app.history_down();
        assert_eq!(app.input_text, "third");
        assert_eq!(app.history_index, Some(2));

        // Past the end clears input
        app.history_down();
        assert!(app.input_text.is_empty());
        assert_eq!(app.history_index, None);
    }

    #[test]
    fn test_history_up_empty() {
        let mut app = FawxApp::new();
        app.history_up();
        assert!(app.input_text.is_empty());
        assert_eq!(app.history_index, None);
    }

    #[test]
    fn test_history_down_no_index() {
        let mut app = FawxApp::new();
        app.command_history.push("something".into());
        app.history_down(); // no-op when history_index is None
        assert!(app.input_text.is_empty());
    }

    #[test]
    fn test_scroll_bounds() {
        let mut app = FawxApp::new();
        // No output — scroll_up shouldn't go negative
        app.scroll_up();
        assert_eq!(app.scroll_offset, 0);

        // scroll_down at 0 stays at 0
        app.scroll_down();
        assert_eq!(app.scroll_offset, 0);

        // Add some output
        for i in 0..100 {
            app.add_output(format!("line {i}"));
        }

        // Scroll up
        app.scroll_up();
        assert_eq!(app.scroll_offset, 1);
        app.scroll_up();
        assert_eq!(app.scroll_offset, 2);

        // Can't exceed total lines
        app.scroll_offset = 100;
        app.scroll_up();
        assert_eq!(app.scroll_offset, 100);

        // scroll_to_bottom resets
        app.scroll_to_bottom();
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn test_calculate_input_height() {
        let app = FawxApp::new();
        // PROMPT is "you › " = 6 chars (byte-wise more, but char count matters
        // for terminal columns). With empty input and width 80, should be 1 row.
        assert_eq!(app.calculate_input_height(80), 1);

        let mut app2 = FawxApp::new();
        // Input that forces wrapping: prompt (6 bytes visible) + 80 chars = 86
        // At width 40, that's ceil(86/40) = 3 rows
        app2.input_text = "x".repeat(80);
        // PROMPT bytes: "you › " — the › is multi-byte UTF-8 but 1 column.
        // For simplicity the calculation uses byte length which may
        // over-count, but the result should be >= 3.
        let height = app2.calculate_input_height(40);
        assert!(height >= 2, "expected at least 2 rows, got {height}");

        // Width 0 edge case
        assert_eq!(app2.calculate_input_height(0), 1);
    }

    #[test]
    fn test_state_transitions() {
        let mut app = FawxApp::new();
        assert_eq!(app.state, AppState::Idle);

        app.set_state(AppState::Executing { spinner_frame: 0 });
        assert_eq!(app.state, AppState::Executing { spinner_frame: 0 });

        app.set_state(AppState::Streaming);
        assert_eq!(app.state, AppState::Streaming);

        app.set_state(AppState::Idle);
        assert_eq!(app.state, AppState::Idle);
    }

    #[test]
    fn test_queue_input_and_drain() {
        let mut app = FawxApp::new();
        app.queue_input("first".into());
        app.queue_input("second".into());
        app.queue_input("third".into());
        assert_eq!(app.pending_inputs.len(), 3);

        assert_eq!(app.drain_next_input(), Some("first".into()));
        assert_eq!(app.drain_next_input(), Some("second".into()));
        assert_eq!(app.drain_next_input(), Some("third".into()));
        assert_eq!(app.drain_next_input(), None);
    }

    #[test]
    fn test_set_steer_last_wins() {
        let mut app = FawxApp::new();
        app.set_steer("first steer".into());
        app.set_steer("second steer".into());
        assert_eq!(app.steer_message.as_deref(), Some("second steer"));
    }

    #[test]
    fn test_take_steer_clears() {
        let mut app = FawxApp::new();
        app.set_steer("my steer".into());
        let taken = app.take_steer();
        assert_eq!(taken.as_deref(), Some("my steer"));
        assert!(app.steer_message.is_none());
    }

    #[test]
    fn test_request_abort() {
        let mut app = FawxApp::new();
        assert!(!app.abort_requested);
        app.request_abort();
        assert!(app.abort_requested);
    }

    #[test]
    fn test_reset_cycle_state() {
        let mut app = FawxApp::new();
        app.request_abort();
        app.set_steer("steer".into());
        app.queue_input("queued".into());
        app.reset_cycle_state();
        // abort and steer are cleared
        assert!(!app.abort_requested);
        assert!(app.steer_message.is_none());
        // pending inputs are preserved across cycles
        assert_eq!(app.pending_inputs.len(), 1);
    }

    #[test]
    fn test_new_has_empty_steer_queue_state() {
        let app = FawxApp::new();
        assert!(app.pending_inputs.is_empty());
        assert!(app.steer_message.is_none());
        assert!(!app.abort_requested);
    }
}
