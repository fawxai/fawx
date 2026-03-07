//! Ratatui-based UI module for Fawx TUI.
//!
//! Provides [`FawxApp`] state management and [`draw`] rendering.
//! This module is standalone — it does not depend on the existing
//! `tui.rs` event loop or `scroll_region.rs`.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
    Frame,
};
use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

// ── Styling constants ──────────────────────────────────────────

/// Amber accent colour used for separator and prompt.
const AMBER: Color = Color::Rgb(255, 204, 0);

/// Dark-gray background for the input region.
pub const INPUT_BG_INDEX: u8 = 236;
const INPUT_BG: Color = Color::Indexed(INPUT_BG_INDEX);

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
    /// Scroll offset in visual rows from the bottom of the output.
    ///
    /// This value is unbounded — it may exceed the actual number of visual
    /// rows. [`render_output`] clamps it to `max_scroll` at render time so
    /// that out-of-range offsets are harmless. The final value is also
    /// clamped to [`u16::MAX`] before being passed to [`Paragraph::scroll`].
    pub scroll_offset: usize,
    pub input_text: String,
    pasted_line_count: Option<usize>,
    pub state: AppState,
    pub command_history: Vec<String>,
    pub history_index: Option<usize>,
    /// Whether the "fawx ›" prefix has been printed for the current stream.
    pub streaming_prefix_printed: bool,
    /// Index of the first output line that belongs to the active stream, if any.
    pub streaming_start_index: Option<usize>,
    /// Index of the current logical line receiving streamed tokens.
    pub streaming_line_index: Option<usize>,
    /// Whether streamed deltas should be ignored until the next explicit start event.
    pub suppress_stream_until_next_start: bool,
    /// Inputs queued while the agent is executing (one per future turn).
    pub pending_inputs: Vec<String>,
    /// Steer message for the running agent (last one wins).
    pub steer_message: Option<String>,
    /// Whether an abort has been requested for the current cycle.
    pub abort_requested: bool,
    /// Whether the user has manually scrolled up (disables auto-scroll to bottom).
    pub user_scrolled: bool,
}

impl FawxApp {
    /// Create a new `FawxApp` with sensible defaults.
    pub fn new() -> Self {
        Self {
            output_lines: Vec::new(),
            scroll_offset: 0,
            input_text: String::new(),
            pasted_line_count: None,
            state: AppState::Idle,
            command_history: Vec::new(),
            history_index: None,
            streaming_prefix_printed: false,
            streaming_start_index: None,
            streaming_line_index: None,
            suppress_stream_until_next_start: false,
            pending_inputs: Vec::new(),
            steer_message: None,
            abort_requested: false,
            user_scrolled: false,
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
        self.scroll_offset = self.scroll_offset.saturating_add(1);
        self.user_scrolled = true;
    }

    /// Scroll the output region down (toward newer content) by one line.
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
        self.user_scrolled = self.scroll_offset > 0;
    }

    /// Jump to the bottom of output (follow latest content).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.user_scrolled = false;
    }

    /// Take the current input text, push it to history, clear the
    /// input field, and return the submitted text.
    pub fn submit_input(&mut self) -> String {
        let text = std::mem::take(&mut self.input_text);
        self.pasted_line_count = None;
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
        self.pasted_line_count = None;
        self.input_text = self.command_history[idx].clone();
    }

    /// Navigate command history downward (newer entries).
    pub fn history_down(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        self.pasted_line_count = None;
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

    /// Insert pasted input at the active insertion point.
    ///
    /// The ratatui input bar currently only supports appending at the end of
    /// the buffer, so bracketed paste follows the same rule instead of
    /// replacing the entire input.
    pub fn insert_pasted_input(&mut self, pasted: String) {
        self.input_text.push_str(&pasted);
        self.pasted_line_count = pasted_line_count(&self.input_text);
        self.history_index = None;
    }

    /// Append a typed character, clearing any pending multiline paste preview first.
    pub fn append_input_char(&mut self, ch: char) {
        if self.pasted_line_count.is_some() {
            self.input_text.clear();
            self.pasted_line_count = None;
        }
        self.input_text.push(ch);
    }

    /// Remove one character, or clear the entire pending multiline paste preview.
    pub fn backspace_input(&mut self) {
        if self.pasted_line_count.is_some() {
            self.input_text.clear();
            self.pasted_line_count = None;
        } else {
            self.input_text.pop();
        }
    }

    fn input_display_text(&self) -> Cow<'_, str> {
        match self.pasted_line_count {
            Some(1) => Cow::Borrowed("[pasted 1 line]"),
            Some(lines) => Cow::Owned(format!("[pasted {lines} lines]")),
            None => Cow::Borrowed(self.input_text.as_str()),
        }
    }

    /// Begin streaming output: add the "fawx ›" prefix line.
    pub fn start_streaming(&mut self) {
        self.add_output(String::new());
        let prefix_index = self.output_lines.len();
        self.streaming_start_index = Some(prefix_index);
        self.streaming_line_index = Some(prefix_index);
        self.suppress_stream_until_next_start = false;
        self.add_output("fawx › ".to_string());
        self.streaming_prefix_printed = true;
        if !self.user_scrolled {
            self.scroll_to_bottom();
        }
    }

    /// Append a streaming delta to the output buffer.
    ///
    /// Handles newlines by splitting into separate output lines.
    pub fn append_streaming_delta(&mut self, delta: &str) {
        if self.suppress_stream_until_next_start {
            return;
        }
        if !self.streaming_prefix_printed {
            self.start_streaming();
        }
        for ch in delta.chars() {
            let Some(line_index) = self.streaming_line_index else {
                break;
            };
            match ch {
                '\n' => {
                    // insert() is O(n) for Vec, but streaming lines are always
                    // appended near the end of output_lines (the stream cursor
                    // tracks the last line), so the shift is bounded by the
                    // number of lines *after* the cursor — typically zero or a
                    // small constant. A VecDeque would penalise the far more
                    // frequent index-based access in rendering.
                    let next_index = line_index + 1;
                    self.output_lines.insert(next_index, String::new());
                    self.streaming_line_index = Some(next_index);
                }
                '\r' => {}
                '\t' => {
                    if let Some(line) = self.output_lines.get_mut(line_index) {
                        line.push_str("    ");
                    }
                }
                ch if ch.is_control() && ch != '\x1b' => {}
                _ => {
                    if let Some(line) = self.output_lines.get_mut(line_index) {
                        line.push(ch);
                    }
                }
            }
        }
        if !self.user_scrolled {
            self.scroll_to_bottom();
        }
    }

    /// Finish streaming: reset the streaming flag.
    pub fn finish_streaming(&mut self) {
        self.streaming_prefix_printed = false;
        self.streaming_start_index = None;
        self.streaming_line_index = None;
    }

    /// Mark the start of a fresh streaming session from the engine.
    pub fn begin_streaming_session(&mut self) {
        self.finish_streaming();
        self.suppress_stream_until_next_start = false;
    }

    /// Stop the current stream locally and ignore stale deltas until the next start event.
    pub fn interrupt_stream(&mut self) {
        self.finish_streaming();
        self.suppress_stream_until_next_start = true;
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
        let display_text = self.input_display_text();
        let content_len = PROMPT.width() + display_text.width();
        (content_len as u16).div_ceil(width).max(1)
    }
}

impl Default for FawxApp {
    fn default() -> Self {
        Self::new()
    }
}

// ── Drawing ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameRenderRows {
    pub width: u16,
    pub rows: Vec<String>,
    pub fills: Vec<FrameRowFill>,
    pub content_columns: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRowFill {
    Default,
    InputBackground,
}

struct FrameSection {
    lines: Vec<Line<'static>>,
    width: usize,
    height: usize,
    fill: FrameRowFill,
}

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

/// Split lines that exceed `width` display columns into multiple lines,
/// preserving styles. Uses [`unicode_width`] so that wide characters
/// (CJK, braille, wide emoji) are measured by display columns, not char
/// count. Each output line fits within the terminal width, giving an
/// exact row count.
fn wrap_lines_to_width(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return lines;
    }
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let display_width: usize = line.spans.iter().map(|s| s.content.width()).sum();
        if display_width <= width {
            out.push(line);
        } else if line.spans.len() == 1 {
            wrap_single_span(&line.spans[0], width, &mut out);
        } else {
            wrap_multi_span(&line.spans, width, &mut out);
        }
    }
    out
}

#[derive(Debug)]
struct WrapToken {
    content: String,
    style: Style,
    whitespace: bool,
}

/// Width-aware wrapping for a single-span line.
fn wrap_single_span(span: &Span<'_>, width: usize, out: &mut Vec<Line<'static>>) {
    let tokens = tokenize_span(span);
    wrap_tokens_to_lines(tokens, width, out);
}

/// Width-aware wrapping for a multi-span line.
fn wrap_multi_span(spans: &[Span<'_>], width: usize, out: &mut Vec<Line<'static>>) {
    let tokens: Vec<WrapToken> = spans.iter().flat_map(tokenize_span).collect();
    wrap_tokens_to_lines(tokens, width, out);
}

fn push_grapheme_to_spans(spans: &mut Vec<Span<'static>>, grapheme: &str, style: Style) {
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push_str(grapheme);
            return;
        }
    }
    spans.push(Span::styled(grapheme.to_string(), style));
}

fn push_text_to_spans(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push_str(text);
            return;
        }
    }
    spans.push(Span::styled(text.to_string(), style));
}

/// Flush the accumulated graphemes into a new token.
fn flush_token(tokens: &mut Vec<WrapToken>, current: &mut String, style: Style, whitespace: bool) {
    tokens.push(WrapToken {
        content: std::mem::take(current),
        style,
        whitespace,
    });
}

/// Process a single grapheme during span tokenization, accumulating runs
/// of same-kind (whitespace vs non-whitespace) characters.
fn accumulate_grapheme(
    grapheme: &str,
    tokens: &mut Vec<WrapToken>,
    current: &mut String,
    current_whitespace: &mut Option<bool>,
    style: Style,
) {
    let whitespace = grapheme.chars().all(char::is_whitespace);
    match *current_whitespace {
        Some(kind) if kind == whitespace => current.push_str(grapheme),
        Some(kind) => {
            flush_token(tokens, current, style, kind);
            current.push_str(grapheme);
            *current_whitespace = Some(whitespace);
        }
        None => {
            current.push_str(grapheme);
            *current_whitespace = Some(whitespace);
        }
    }
}

fn tokenize_span(span: &Span<'_>) -> Vec<WrapToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_whitespace = None;

    for grapheme in span.content.graphemes(true) {
        accumulate_grapheme(
            grapheme,
            &mut tokens,
            &mut current,
            &mut current_whitespace,
            span.style,
        );
    }

    if let Some(whitespace) = current_whitespace {
        flush_token(&mut tokens, &mut current, span.style, whitespace);
    }

    tokens
}

fn token_display_width(token: &WrapToken) -> usize {
    token.content.width()
}

fn flush_row(row: &mut Vec<Span<'static>>, out: &mut Vec<Line<'static>>) {
    if !row.is_empty() {
        out.push(Line::from(std::mem::take(row)));
    }
}

fn split_token_across_rows(
    token: &WrapToken,
    width: usize,
    row: &mut Vec<Span<'static>>,
    current_width: &mut usize,
    out: &mut Vec<Line<'static>>,
) {
    for grapheme in token.content.graphemes(true) {
        let grapheme_width = grapheme.width();
        if *current_width + grapheme_width > width && !row.is_empty() {
            flush_row(row, out);
            *current_width = 0;
        }
        push_grapheme_to_spans(row, grapheme, token.style);
        *current_width += grapheme_width;
    }
}

/// Mutable state carried through the token wrapping loop.
struct WrapState<'a> {
    row: Vec<Span<'static>>,
    current_width: usize,
    pending_space: Option<WrapToken>,
    width: usize,
    out: &'a mut Vec<Line<'static>>,
}

impl<'a> WrapState<'a> {
    fn new(width: usize, out: &'a mut Vec<Line<'static>>) -> Self {
        Self {
            row: Vec::new(),
            current_width: 0,
            pending_space: None,
            width,
            out,
        }
    }

    /// Place a whitespace token at the start of a row or defer it.
    fn place_whitespace(&mut self, token: WrapToken, token_width: usize) {
        if self.row.is_empty() {
            if token_width <= self.width {
                push_text_to_spans(&mut self.row, &token.content, token.style);
                self.current_width += token_width;
            } else {
                split_token_across_rows(
                    &token,
                    self.width,
                    &mut self.row,
                    &mut self.current_width,
                    self.out,
                );
            }
        } else {
            self.pending_space = Some(token);
        }
    }

    /// Place a word token, wrapping or splitting as needed.
    fn place_word(&mut self, token: &WrapToken, token_width: usize) {
        let ps_w = self
            .pending_space
            .as_ref()
            .map(token_display_width)
            .unwrap_or(0);
        if !self.row.is_empty() && self.current_width + ps_w + token_width > self.width {
            if self.pending_space.is_some() {
                flush_row(&mut self.row, self.out);
                self.current_width = 0;
                self.pending_space = None;
            } else {
                split_token_across_rows(
                    token,
                    self.width,
                    &mut self.row,
                    &mut self.current_width,
                    self.out,
                );
                return;
            }
        }

        if self.row.is_empty() && token_width > self.width {
            split_token_across_rows(
                token,
                self.width,
                &mut self.row,
                &mut self.current_width,
                self.out,
            );
            return;
        }

        self.emit_pending_space();
        push_text_to_spans(&mut self.row, &token.content, token.style);
        self.current_width += token_width;
    }

    /// Flush any deferred whitespace token into the current row.
    fn emit_pending_space(&mut self) {
        if let Some(space) = self.pending_space.take() {
            if !self.row.is_empty() {
                push_text_to_spans(&mut self.row, &space.content, space.style);
                self.current_width += token_display_width(&space);
            }
        }
    }

    /// Flush the final row, if non-empty.
    fn finish(mut self) {
        flush_row(&mut self.row, self.out);
    }
}

fn wrap_tokens_to_lines(tokens: Vec<WrapToken>, width: usize, out: &mut Vec<Line<'static>>) {
    let mut state = WrapState::new(width, out);

    for token in tokens {
        let token_width = token_display_width(&token);
        if token.whitespace {
            state.place_whitespace(token, token_width);
        } else {
            state.place_word(&token, token_width);
        }
    }

    state.finish();
}

fn build_output_lines(app: &FawxApp) -> Vec<Line<'static>> {
    app.output_lines
        .iter()
        .enumerate()
        .map(|(idx, line)| renderable_output_line(app, idx, line))
        .collect()
}

fn renderable_output_line(app: &FawxApp, index: usize, line: &str) -> Line<'static> {
    let streaming_line = app.streaming_prefix_printed
        && app
            .streaming_start_index
            .is_some_and(|start_index| index >= start_index);

    // Streaming lines skip ANSI parsing because they may contain incomplete
    // escape sequences mid-delta — parsing partial ANSI would produce garbled
    // output. ANSI is only applied to finalized (non-streaming) lines.
    if !streaming_line && crate::ansi::contains_ansi(line) {
        crate::ansi::ansi_to_line(line)
    } else {
        Line::from(line.to_string())
    }
}

fn prepare_output_lines_for_render(app: &FawxApp, width: usize) -> (Vec<Line<'static>>, usize) {
    let render_lines = wrap_lines_to_width(build_output_lines(app), width);
    let total_visual_rows = render_lines.len();
    (render_lines, total_visual_rows)
}

pub fn frame_render_rows(
    terminal_width: u16,
    terminal_height: u16,
    app: &FawxApp,
) -> FrameRenderRows {
    let area = Rect::new(0, 0, terminal_width, terminal_height);
    let input_height = app.calculate_input_height(terminal_width);
    let layout = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(input_height),
    ])
    .split(area);
    let mut rows = Vec::new();
    let mut fills = Vec::new();
    let mut content_columns = Vec::new();

    for section in build_frame_sections(app, &layout) {
        append_frame_section(
            &mut rows,
            &mut fills,
            &mut content_columns,
            section.lines,
            section.width,
            section.height,
            section.fill,
        );
    }

    FrameRenderRows {
        width: terminal_width,
        rows,
        fills,
        content_columns,
    }
}

fn build_frame_sections(app: &FawxApp, layout: &[Rect]) -> [FrameSection; 3] {
    let output_width = layout[0].width as usize;
    let output_height = layout[0].height as usize;
    let (output_lines, _) = visible_output_lines_for_render(app, output_width, output_height);

    [
        FrameSection {
            lines: output_lines,
            width: output_width,
            height: output_height,
            fill: FrameRowFill::Default,
        },
        FrameSection {
            lines: vec![build_separator_line(
                layout[1].width as usize,
                &build_status_indicators(app),
            )],
            width: layout[1].width as usize,
            height: layout[1].height as usize,
            fill: FrameRowFill::Default,
        },
        FrameSection {
            lines: build_input_lines(
                app,
                layout[2].width as usize,
                Style::default(),
                Style::default(),
            ),
            width: layout[2].width as usize,
            height: layout[2].height as usize,
            fill: FrameRowFill::InputBackground,
        },
    ]
}

fn visible_output_lines_for_render(
    app: &FawxApp,
    width: usize,
    height: usize,
) -> (Vec<Line<'static>>, usize) {
    let (render_lines, total_visual_rows) = prepare_output_lines_for_render(app, width);
    let max_scroll = total_visual_rows.saturating_sub(height);
    let clamped_offset = app.scroll_offset.min(max_scroll);
    let start = max_scroll.saturating_sub(clamped_offset);
    let visible_lines = render_lines.into_iter().skip(start).take(height).collect();
    (visible_lines, total_visual_rows)
}

fn line_display_width(line: &Line<'_>) -> usize {
    line.spans.iter().map(|span| span.content.width()).sum()
}

fn line_to_string(line: &Line<'_>) -> String {
    let mut rendered = String::new();

    for span in &line.spans {
        for grapheme in span.content.graphemes(true) {
            rendered.push_str(grapheme);
            let filler_width = grapheme.width().saturating_sub(1);
            if filler_width > 0 {
                rendered.push_str(&" ".repeat(filler_width));
            }
        }
    }

    rendered
}

fn pad_lines_to_area(
    mut lines: Vec<Line<'static>>,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }
    if width == 0 {
        return lines;
    }

    for line in &mut lines {
        let padding = width.saturating_sub(line_display_width(line));
        if padding > 0 {
            line.spans.push(Span::raw(" ".repeat(padding)));
        }
    }

    while lines.len() < height {
        lines.push(Line::from(" ".repeat(width)));
    }

    lines
}

fn pad_visible_output_lines(
    lines: Vec<Line<'static>>,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    pad_lines_to_area(lines, width, height)
}

fn render_section_rows(
    lines: Vec<Line<'static>>,
    width: usize,
    height: usize,
    fill: FrameRowFill,
) -> (Vec<String>, Vec<FrameRowFill>, Vec<u16>) {
    let mut content_columns = lines
        .iter()
        .map(|line| line_display_width(line) as u16)
        .collect::<Vec<_>>();
    let rows = pad_lines_to_area(lines, width, height)
        .iter()
        .map(line_to_string)
        .collect::<Vec<_>>();
    content_columns.resize(rows.len(), 0);
    let fills = vec![fill; rows.len()];

    (rows, fills, content_columns)
}

fn append_frame_section(
    rows: &mut Vec<String>,
    fills: &mut Vec<FrameRowFill>,
    content_columns: &mut Vec<u16>,
    lines: Vec<Line<'static>>,
    width: usize,
    height: usize,
    fill: FrameRowFill,
) {
    let (section_rows, section_fills, section_columns) =
        render_section_rows(lines, width, height, fill);
    rows.extend(section_rows);
    fills.extend(section_fills);
    content_columns.extend(section_columns);
}
fn truncate_to_display_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut width = 0;

    for grapheme in text.graphemes(true) {
        let grapheme_width = grapheme.width();
        if width + grapheme_width > max_width {
            break;
        }
        out.push_str(grapheme);
        width += grapheme_width;
    }

    out
}

fn truncate_spans_to_display_width(spans: &[Span<'_>], max_width: usize) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut width = 0;

    for span in spans {
        if width >= max_width {
            break;
        }

        let truncated = truncate_to_display_width(&span.content, max_width.saturating_sub(width));

        if !truncated.is_empty() {
            width += truncated.width();
            out.push(Span::styled(truncated, span.style));
        }
    }

    out
}

fn build_separator_line(area_width: usize, indicators: &[Span<'_>]) -> Line<'static> {
    if indicators.is_empty() {
        return Line::from(Span::styled(
            "─".repeat(area_width),
            Style::default().fg(AMBER),
        ));
    }

    let indicator_spans = truncate_spans_to_display_width(indicators, area_width.saturating_sub(2));
    let indicator_width = indicator_spans
        .iter()
        .map(|span| span.content.width())
        .sum::<usize>();
    let fill_len = area_width.saturating_sub(indicator_width);
    let mut line_spans = Vec::with_capacity(1 + indicator_spans.len());
    line_spans.push(Span::styled(
        "─".repeat(fill_len),
        Style::default().fg(AMBER),
    ));
    line_spans.extend(indicator_spans);

    Line::from(line_spans)
}

fn build_input_lines(
    app: &FawxApp,
    width: usize,
    prompt_style: Style,
    text_style: Style,
) -> Vec<Line<'static>> {
    // Keep the scrub snapshot and the live input widget on the same wrapping path.
    let input_display = app.input_display_text();
    let line = Line::from(vec![
        Span::styled(PROMPT, prompt_style),
        Span::styled(input_display.into_owned(), text_style),
    ]);
    wrap_lines_to_width(vec![line], width)
}

/// Render the scrollable output region.
fn render_output(frame: &mut Frame, area: ratatui::layout::Rect, app: &FawxApp) {
    let width = area.width as usize;
    let height = area.height as usize;
    let (lines, _) = visible_output_lines_for_render(app, width, height);
    let lines = pad_visible_output_lines(lines, width, height);
    let paragraph = Paragraph::new(lines);

    frame.render_widget(Clear, area);
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
    let line = Paragraph::new(build_separator_line(area.width as usize, &indicators));
    frame.render_widget(Clear, area);
    frame.render_widget(line, area);
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
    let block = Block::default().style(Style::default().bg(INPUT_BG));
    let lines = build_input_lines(app, area.width as usize, prompt_style, text_style);
    let lines = pad_lines_to_area(lines, area.width as usize, area.height as usize);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn pasted_line_count(text: &str) -> Option<usize> {
    let mut line_count = 1usize;
    let mut saw_line_break = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\n' => {
                saw_line_break = true;
                line_count += 1;
            }
            '\r' => {
                saw_line_break = true;
                line_count += 1;
                if matches!(chars.peek(), Some('\n')) {
                    chars.next();
                }
            }
            _ => {}
        }
    }

    if !saw_line_break {
        return None;
    }

    if text.ends_with('\n') || text.ends_with('\r') {
        line_count = line_count.saturating_sub(1).max(1);
    }

    Some(line_count)
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

    #[test]
    fn test_app_new_defaults() {
        let app = FawxApp::new();
        assert!(app.output_lines.is_empty());
        assert_eq!(app.scroll_offset, 0);
        assert!(app.input_text.is_empty());
        assert_eq!(app.pasted_line_count, None);
        assert_eq!(app.state, AppState::Idle);
        assert!(app.command_history.is_empty());
        assert_eq!(app.history_index, None);
        assert_eq!(app.streaming_start_index, None);
        assert_eq!(app.streaming_line_index, None);
        assert!(!app.suppress_stream_until_next_start);
        assert!(!app.user_scrolled);
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
    fn test_insert_pasted_input_shows_multiline_preview() {
        let mut app = FawxApp::new();
        app.insert_pasted_input("alpha\nbeta\ngamma".into());

        assert_eq!(app.input_text, "alpha\nbeta\ngamma");
        assert_eq!(app.pasted_line_count, Some(3));
        assert_eq!(app.input_display_text(), "[pasted 3 lines]");
    }

    #[test]
    fn test_insert_pasted_input_trailing_newline_uses_single_line_preview() {
        let mut app = FawxApp::new();
        app.insert_pasted_input("alpha\n".into());

        assert_eq!(app.pasted_line_count, Some(1));
        assert_eq!(app.input_display_text(), "[pasted 1 line]");
    }

    #[test]
    fn test_insert_pasted_input_single_line_keeps_plain_text() {
        let mut app = FawxApp::new();
        app.insert_pasted_input("alpha beta".into());

        assert_eq!(app.pasted_line_count, None);
        assert_eq!(app.input_display_text(), "alpha beta");
    }

    #[test]
    fn test_submit_input_returns_full_multiline_paste() {
        let mut app = FawxApp::new();
        app.insert_pasted_input("alpha\nbeta".into());

        let submitted = app.submit_input();

        assert_eq!(submitted, "alpha\nbeta");
        assert!(app.input_text.is_empty());
        assert_eq!(app.pasted_line_count, None);
        assert_eq!(app.command_history, vec!["alpha\nbeta".to_string()]);
    }

    #[test]
    fn test_typing_clears_multiline_paste_preview() {
        let mut app = FawxApp::new();
        app.insert_pasted_input("alpha\nbeta".into());

        app.append_input_char('x');

        assert_eq!(app.input_text, "x");
        assert_eq!(app.pasted_line_count, None);
    }

    #[test]
    fn test_backspace_clears_multiline_paste_preview() {
        let mut app = FawxApp::new();
        app.insert_pasted_input("alpha\nbeta".into());

        app.backspace_input();

        assert!(app.input_text.is_empty());
        assert_eq!(app.pasted_line_count, None);
    }

    #[test]
    fn test_insert_pasted_input_appends_to_existing_text() {
        let mut app = FawxApp::new();
        app.input_text = "hello world ".into();

        app.insert_pasted_input("AAAA".into());

        assert_eq!(app.input_text, "hello world AAAA");
        assert_eq!(app.pasted_line_count, None);
    }

    #[test]
    fn test_insert_pasted_input_appends_multiline_paste_to_existing_text() {
        let mut app = FawxApp::new();
        app.input_text = "hello world ".into();

        app.insert_pasted_input("alpha\nbeta".into());

        assert_eq!(app.input_text, "hello world alpha\nbeta");
        assert_eq!(app.pasted_line_count, Some(2));
        assert_eq!(app.input_display_text(), "[pasted 2 lines]");
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
        // No output — scroll_up still increments (render clamps)
        app.scroll_up();
        assert_eq!(app.scroll_offset, 1);

        // scroll_to_bottom resets
        app.scroll_to_bottom();
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

        // No artificial cap — render_output clamps at render time
        app.scroll_offset = 100;
        app.scroll_up();
        assert_eq!(app.scroll_offset, 101);

        // scroll_to_bottom resets
        app.scroll_to_bottom();
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn test_calculate_input_height() {
        let app = FawxApp::new();
        // PROMPT is 6 display columns. With empty input and width 80, it fits
        // on a single row.
        assert_eq!(app.calculate_input_height(80), 1);

        let mut app2 = FawxApp::new();
        // Input that forces wrapping: prompt (6 columns) + 80 chars = 86
        // display columns. At width 40, that's ceil(86/40) = 3 rows.
        app2.input_text = "x".repeat(80);
        let height = app2.calculate_input_height(40);
        assert_eq!(height, 3);

        // Width 0 edge case
        assert_eq!(app2.calculate_input_height(0), 1);
    }

    #[test]
    fn test_calculate_input_height_uses_paste_preview_text() {
        let mut app = FawxApp::new();
        app.insert_pasted_input("alpha\nbeta\ngamma".into());

        assert_eq!(app.calculate_input_height(20), 2);
    }

    #[test]
    fn test_calculate_input_height_uses_display_width() {
        let mut app = FawxApp::new();
        app.input_text = "📡📡📡".into();

        assert_eq!(app.calculate_input_height(10), 2);
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

    #[test]
    fn test_start_streaming_tracks_streaming_start_index() {
        let mut app = FawxApp::new();
        app.add_output("existing".into());
        app.start_streaming();
        assert_eq!(app.streaming_start_index, Some(2));
        assert_eq!(app.streaming_line_index, Some(2));
        assert_eq!(app.output_lines[2], "fawx › ");
    }

    #[test]
    fn test_append_streaming_delta_normalizes_carriage_return_and_tab() {
        let mut app = FawxApp::new();
        app.start_streaming();
        app.append_streaming_delta("hello\r\tworld\r\nnext");
        assert_eq!(app.output_lines[1], "fawx › hello    world");
        assert_eq!(app.output_lines[2], "next");
        assert_eq!(app.streaming_line_index, Some(2));
    }

    #[test]
    fn test_append_streaming_delta_inserts_before_non_stream_output() {
        let mut app = FawxApp::new();
        app.start_streaming();
        app.append_streaming_delta("hello");
        app.add_output("queued".into());

        app.append_streaming_delta("\nworld");

        assert_eq!(app.output_lines[1], "fawx › hello");
        assert_eq!(app.output_lines[2], "world");
        assert_eq!(app.output_lines[3], "queued");
        assert_eq!(app.streaming_line_index, Some(2));
    }

    #[test]
    fn test_build_output_lines_keeps_active_stream_raw_until_finished() {
        let mut app = FawxApp::new();
        app.add_output("\x1b[38;2;255;0;0mstable\x1b[0m".into());
        app.start_streaming();
        app.append_streaming_delta("\x1b[38;2;255");

        let lines = build_output_lines(&app);
        assert_eq!(lines[0].spans[0].content.as_ref(), "stable");
        assert_eq!(lines[2].spans.len(), 1);
        assert_eq!(lines[2].spans[0].content.as_ref(), "fawx › \x1b[38;2;255");

        app.finish_streaming();
        let lines = build_output_lines(&app);
        assert_eq!(lines[2].spans[0].content.as_ref(), "fawx › ");
    }

    #[test]
    fn test_interrupt_stream_suppresses_old_deltas_until_next_session() {
        let mut app = FawxApp::new();
        app.start_streaming();
        app.append_streaming_delta("hello");

        app.interrupt_stream();
        app.append_streaming_delta("ignored");
        assert_eq!(app.output_lines[1], "fawx › hello");
        assert!(app.streaming_line_index.is_none());
        assert!(app.suppress_stream_until_next_start);

        app.begin_streaming_session();
        app.append_streaming_delta("fresh");
        assert_eq!(app.output_lines[3], "fawx › fresh");
        assert!(!app.suppress_stream_until_next_start);
    }

    #[test]
    fn test_render_output_replaces_rows_when_scrolling_to_latest_wrap() {
        let backend = TestBackend::new(5, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = FawxApp::new();
        app.add_output("abcde".into());

        terminal
            .draw(|frame| render_output(frame, frame.area(), &app))
            .unwrap();

        app.output_lines[0].push('f');
        terminal
            .draw(|frame| render_output(frame, frame.area(), &app))
            .unwrap();

        terminal
            .backend()
            .assert_buffer(&Buffer::with_lines(["f    "]));
    }

    #[test]
    fn test_render_output_matches_visible_slice_for_long_stream() {
        let backend = TestBackend::new(24, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = FawxApp::new();
        app.start_streaming();

        let stream = "Cooperative yielding keeps evolving as the response grows beyond the viewport height and should still wrap into stable rows without scattering words across the screen.";

        for ch in stream.chars() {
            app.append_streaming_delta(&ch.to_string());
            terminal
                .draw(|frame| render_output(frame, frame.area(), &app))
                .unwrap();
            let (visible_lines, _) = visible_output_lines_for_render(&app, 24, 6);
            let mut expected_rows: Vec<String> = visible_lines
                .iter()
                .map(|line| {
                    let text: String = line
                        .spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect();
                    format!("{text:<24}")
                })
                .collect();
            expected_rows.resize(6, " ".repeat(24));

            let actual_rows: Vec<String> = (0..6)
                .map(|row| {
                    (0..24)
                        .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .collect();

            assert_eq!(actual_rows, expected_rows);
        }
    }

    #[test]
    fn test_render_output_replaces_rows_across_multi_row_scroll_shift() {
        let backend = TestBackend::new(5, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = FawxApp::new();
        app.add_output("abcde".into());
        app.add_output("vwxyz".into());
        app.add_output("12345".into());

        terminal
            .draw(|frame| render_output(frame, frame.area(), &app))
            .unwrap();

        app.add_output("f".into());
        terminal
            .draw(|frame| render_output(frame, frame.area(), &app))
            .unwrap();

        terminal
            .backend()
            .assert_buffer(&Buffer::with_lines(["vwxyz", "12345", "f    "]));
    }

    #[test]
    fn test_prepare_output_lines_for_render_counts_wrapped_stream_rows() {
        let mut app = FawxApp::new();
        app.start_streaming();
        app.append_streaming_delta("abcdef");

        let (lines, total_visual_rows) = prepare_output_lines_for_render(&app, 4);

        assert_eq!(lines.len(), 5);
        assert_eq!(total_visual_rows, 5);
        assert!(lines[0].spans.is_empty() || lines[0].spans[0].content.is_empty());
        assert_eq!(lines[1].spans[0].content.as_ref(), "fawx");
        assert_eq!(lines[2].spans[0].content.as_ref(), "›");
        assert_eq!(lines[3].spans[0].content.as_ref(), "abcd");
        assert_eq!(lines[4].spans[0].content.as_ref(), "ef");
    }

    #[test]
    fn test_prepare_output_lines_for_render_keeps_completed_lines_prewrapped() {
        let mut app = FawxApp::new();
        app.add_output("abcdefgh".into());

        let (lines, total_visual_rows) = prepare_output_lines_for_render(&app, 4);

        assert_eq!(lines.len(), 2);
        assert_eq!(total_visual_rows, 2);
        assert_eq!(lines[0].spans[0].content.as_ref(), "abcd");
        assert_eq!(lines[1].spans[0].content.as_ref(), "efgh");
    }

    #[test]
    fn test_pad_lines_to_area_fills_short_rows_and_blank_lines() {
        let padded = pad_lines_to_area(vec![Line::from("abc"), Line::from("")], 5, 4);

        let rendered: Vec<String> = padded
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        assert_eq!(rendered, vec!["abc  ", "     ", "     ", "     "]);
    }

    #[test]
    fn test_render_output_pads_visible_rows_during_long_stream_scroll() {
        let backend = TestBackend::new(24, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = FawxApp::new();
        app.start_streaming();

        let stream = "The authentication system supports three authentication methods for LLM providers.\n\n## AuthMethod Enum\n\n**1. ApiKey** - Direct API key authentication (BYO key)\n- Fields: `provider` (string), `key` (string)\n- Used for providers like OpenAI, Anthropic, OpenRouter with user-provided API keys";

        for ch in stream.chars() {
            app.append_streaming_delta(&ch.to_string());
            terminal
                .draw(|frame| render_output(frame, frame.area(), &app))
                .unwrap();
        }

        let (visible_lines, _) = visible_output_lines_for_render(&app, 24, 6);
        let padded_lines = pad_visible_output_lines(visible_lines, 24, 6);
        let expected_rows: Vec<String> = padded_lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        let actual_rows: Vec<String> = (0..6)
            .map(|row| {
                (0..24)
                    .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect();

        assert_eq!(actual_rows, expected_rows);
    }

    #[test]
    fn test_draw_clears_regions_when_layout_heights_change_between_frames() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = FawxApp::new();
        app.add_output("alpha".into());
        app.set_state(AppState::Executing { spinner_frame: 0 });
        app.input_text = "this input is long enough to wrap".into();

        terminal.draw(|frame| draw(frame, &app)).unwrap();

        app.set_state(AppState::Idle);
        app.input_text.clear();

        terminal.draw(|frame| draw(frame, &app)).unwrap();

        let actual_rows: Vec<String> = (0..5)
            .map(|row| {
                (0..20)
                    .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect();

        assert_eq!(
            actual_rows,
            vec![
                "alpha               ".to_string(),
                "                    ".to_string(),
                "                    ".to_string(),
                "────────────────────".to_string(),
                "you ›               ".to_string(),
            ]
        );
    }

    #[test]
    fn test_build_separator_line_clamps_wide_status_to_area_width() {
        let mut app = FawxApp::new();
        app.set_state(AppState::Streaming);
        app.queue_input("queued".into());

        let indicators = build_status_indicators(&app);
        let line = build_separator_line(12, &indicators);
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(line_display_width(&line), 12);
        assert!(rendered.contains("📡"));
    }

    #[test]
    fn test_build_separator_line_preserves_indicator_style_when_truncated() {
        let indicators = vec![Span::styled(
            " ⏳ Thinking... ",
            Style::default().fg(Color::Yellow),
        )];

        let line = build_separator_line(12, &indicators);

        assert!(line
            .spans
            .iter()
            .any(|span| span.style.fg == Some(Color::Yellow)));
    }

    #[test]
    fn test_truncate_to_display_width_handles_grapheme_boundaries() {
        assert_eq!(truncate_to_display_width("", 4), "");
        assert_eq!(truncate_to_display_width("alpha", 0), "");
        assert_eq!(truncate_to_display_width("alpha", 3), "alp");
        assert_eq!(truncate_to_display_width("📡ok", 1), "");
        assert_eq!(truncate_to_display_width("📡ok", 2), "📡");
        assert_eq!(truncate_to_display_width("e\u{301}x", 1), "e\u{301}");
    }

    #[test]
    fn test_frame_render_rows_matches_drawn_buffer() {
        let backend = TestBackend::new(20, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = FawxApp::new();
        app.add_output("alpha beta gamma".into());
        app.set_state(AppState::Executing { spinner_frame: 0 });
        app.queue_input("queued".into());
        app.steer_message = Some("steer".into());
        app.input_text = "this input wraps".into();

        terminal.draw(|frame| draw(frame, &app)).unwrap();

        let snapshot = frame_render_rows(20, 6, &app);
        let actual_rows: Vec<String> = (0..6)
            .map(|row| {
                (0..20)
                    .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect();

        assert_eq!(snapshot.rows, actual_rows);
        assert_eq!(
            snapshot.fills,
            vec![
                FrameRowFill::Default,
                FrameRowFill::Default,
                FrameRowFill::Default,
                FrameRowFill::Default,
                FrameRowFill::InputBackground,
                FrameRowFill::InputBackground,
            ]
        );
    }

    #[test]
    fn test_frame_render_rows_matches_drawn_buffer_with_wide_content() {
        let backend = TestBackend::new(16, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = FawxApp::new();
        app.add_output("中 alpha 📡".into());
        app.set_state(AppState::Streaming);
        app.input_text = "码🙂".into();

        terminal.draw(|frame| draw(frame, &app)).unwrap();

        let snapshot = frame_render_rows(16, 6, &app);
        let actual_rows: Vec<String> = (0..6)
            .map(|row| {
                (0..16)
                    .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect();

        assert_eq!(snapshot.rows, actual_rows);
    }

    #[test]
    fn test_render_input_matches_wrapped_lines_with_wide_chars_and_spaces() {
        let backend = TestBackend::new(10, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = FawxApp::new();
        app.input_text = "码🙂  wrap".into();

        terminal
            .draw(|frame| render_input(frame, frame.area(), &app))
            .unwrap();

        let expected_rows: Vec<String> = pad_lines_to_area(
            build_input_lines(&app, 10, Style::default().fg(AMBER), Style::default()),
            10,
            3,
        )
        .iter()
        .map(line_to_string)
        .collect();
        let actual_rows: Vec<String> = (0..3)
            .map(|row| {
                (0..10)
                    .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect();

        assert_eq!(actual_rows, expected_rows);
    }

    // ── wrap_lines_to_width tests ──────────────────────────────

    #[test]
    fn test_wrap_short_lines_noop() {
        let lines = vec![Line::from("hello"), Line::from("world")];
        let result = wrap_lines_to_width(lines, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].spans[0].content.as_ref(), "hello");
        assert_eq!(result[1].spans[0].content.as_ref(), "world");
    }

    #[test]
    fn test_wrap_single_span_exceeding_width() {
        let lines = vec![Line::from("abcdefghij")]; // 10 chars
        let result = wrap_lines_to_width(lines, 4);
        assert_eq!(result.len(), 3); // "abcd", "efgh", "ij"
        assert_eq!(result[0].spans[0].content.as_ref(), "abcd");
        assert_eq!(result[1].spans[0].content.as_ref(), "efgh");
        assert_eq!(result[2].spans[0].content.as_ref(), "ij");
    }

    #[test]
    fn test_wrap_prefers_breaking_at_spaces() {
        let lines = vec![Line::from("policy engine allows deny confirm")];
        let result = wrap_lines_to_width(lines, 12);

        let text: Vec<&str> = result
            .iter()
            .map(|line| line.spans[0].content.as_ref())
            .collect();
        assert_eq!(text, vec!["policy", "engine", "allows deny", "confirm"]);
    }

    #[test]
    fn test_wrap_multi_span_exceeding_width() {
        let red = Style::default().fg(Color::Red);
        let blue = Style::default().fg(Color::Blue);
        // "aaa" (red) + "bbb" (blue) = 6 chars, width 4
        let lines = vec![Line::from(vec![
            Span::styled("aaa", red),
            Span::styled("bbb", blue),
        ])];
        let result = wrap_lines_to_width(lines, 4);
        // First line: "aaa" red + "b" blue = 4 chars
        // Second line: "bb" blue = 2 chars
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].spans.len(), 2);
        assert_eq!(result[0].spans[0].content.as_ref(), "aaa");
        assert_eq!(result[0].spans[0].style, red);
        assert_eq!(result[0].spans[1].content.as_ref(), "b");
        assert_eq!(result[0].spans[1].style, blue);
        assert_eq!(result[1].spans[0].content.as_ref(), "bb");
        assert_eq!(result[1].spans[0].style, blue);
    }

    #[test]
    fn test_wrap_width_zero_noop() {
        let lines = vec![Line::from("anything")];
        let result = wrap_lines_to_width(lines, 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].spans[0].content.as_ref(), "anything");
    }

    #[test]
    fn test_wrap_exact_width_no_wrap() {
        // A line with exactly `width` display columns should NOT be wrapped.
        let lines = vec![Line::from("abcd")]; // 4 chars, 4 display columns
        let result = wrap_lines_to_width(lines, 4);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].spans[0].content.as_ref(), "abcd");
    }

    #[test]
    fn test_wrap_empty_line_passthrough() {
        let lines = vec![Line::from(""), Line::from("hello")];
        let result = wrap_lines_to_width(lines, 10);
        assert_eq!(result.len(), 2);
        // Empty Line::from("") has zero spans — verify it passes through intact.
        assert!(result[0].spans.is_empty());
        assert_eq!(result[1].spans[0].content.as_ref(), "hello");
    }

    #[test]
    fn test_wrap_wide_chars_cjk() {
        // Each CJK char takes 2 display columns. "你好世界" = 4 chars, 8 columns.
        // Width 5 → first line "你好" (4 cols), second line "世界" (4 cols).
        // "你" (2) + "好" (2) = 4 ≤ 5; adding "世" (2) = 6 > 5 → break.
        let lines = vec![Line::from("你好世界")];
        let result = wrap_lines_to_width(lines, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].spans[0].content.as_ref(), "你好");
        assert_eq!(result[1].spans[0].content.as_ref(), "世界");
    }

    #[test]
    fn test_wrap_wide_chars_multi_span() {
        let red = Style::default().fg(Color::Red);
        let blue = Style::default().fg(Color::Blue);
        // "你好" (red, 4 cols) + "世界" (blue, 4 cols) = 8 cols, width 5
        let lines = vec![Line::from(vec![
            Span::styled("你好", red),
            Span::styled("世界", blue),
        ])];
        let result = wrap_lines_to_width(lines, 5);
        assert_eq!(result.len(), 2);
        // First line: "你好" (red) fits in 4 cols; "世" (blue, 2 cols)
        // would exceed 6 > 5, so break after "你好".
        assert_eq!(result[0].spans[0].content.as_ref(), "你好");
        assert_eq!(result[0].spans[0].style, red);
        assert_eq!(result[1].spans[0].content.as_ref(), "世界");
        assert_eq!(result[1].spans[0].style, blue);
    }

    #[test]
    fn test_wrap_preserves_styles_across_boundary() {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        // 8 bold chars, width 3 → "abc", "def", "gh"
        let lines = vec![Line::from(Span::styled("abcdefgh", bold))];
        let result = wrap_lines_to_width(lines, 3);
        assert_eq!(result.len(), 3);
        for row in &result {
            assert_eq!(row.spans[0].style, bold);
        }
        assert_eq!(result[0].spans[0].content.as_ref(), "abc");
        assert_eq!(result[1].spans[0].content.as_ref(), "def");
        assert_eq!(result[2].spans[0].content.as_ref(), "gh");
    }

    #[test]
    fn test_wrap_keeps_combining_mark_with_base_character() {
        let lines = vec![Line::from("e\u{301}e\u{301}")];
        let result = wrap_lines_to_width(lines, 1);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].spans[0].content.as_ref(), "e\u{301}");
        assert_eq!(result[1].spans[0].content.as_ref(), "e\u{301}");
    }

    #[test]
    fn test_wrap_keeps_zwj_emoji_grapheme_together() {
        let family = "👨‍👩‍👧‍👦";
        let lines = vec![Line::from(format!("{family}{family}"))];
        let result = wrap_lines_to_width(lines, family.width());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].spans[0].content.as_ref(), family);
        assert_eq!(result[1].spans[0].content.as_ref(), family);
    }

    // ── user_scrolled tests ────────────────────────────────────

    #[test]
    fn test_user_scrolled_set_on_scroll_up_cleared_on_bottom() {
        let mut app = FawxApp::new();
        assert!(!app.user_scrolled);
        app.scroll_up();
        assert!(app.user_scrolled);
        app.scroll_to_bottom();
        assert!(!app.user_scrolled);
    }

    #[test]
    fn test_user_scrolled_cleared_when_scroll_down_reaches_zero() {
        let mut app = FawxApp::new();
        app.scroll_offset = 2;
        app.user_scrolled = true;
        app.scroll_down(); // offset → 1
        assert!(app.user_scrolled);
        app.scroll_down(); // offset → 0
        assert!(!app.user_scrolled);
    }

    #[test]
    fn test_scroll_up_no_artificial_cap() {
        let mut app = FawxApp::new();
        // With zero output lines, scroll_up should still increment
        // (render_output clamps at render time).
        app.scroll_up();
        assert_eq!(app.scroll_offset, 1);
        app.scroll_up();
        assert_eq!(app.scroll_offset, 2);
        // Even with 5 output lines, can scroll past logical count
        for i in 0..5 {
            app.add_output(format!("line {i}"));
        }
        app.scroll_offset = 10;
        app.scroll_up();
        assert_eq!(app.scroll_offset, 11);
    }

    #[test]
    fn test_start_streaming_no_scroll_when_user_scrolled() {
        let mut app = FawxApp::new();
        for i in 0..50 {
            app.add_output(format!("line {i}"));
        }
        app.scroll_offset = 10;
        app.user_scrolled = true;
        app.start_streaming();
        // scroll_offset should remain unchanged
        assert_eq!(app.scroll_offset, 10);
        assert!(app.user_scrolled);
    }

    #[test]
    fn test_append_streaming_delta_no_scroll_when_user_scrolled() {
        let mut app = FawxApp::new();
        for i in 0..50 {
            app.add_output(format!("line {i}"));
        }
        app.scroll_offset = 5;
        app.user_scrolled = true;
        app.streaming_prefix_printed = true;
        app.output_lines.push(String::new()); // current streaming line
        app.append_streaming_delta("hello");
        // scroll_offset should remain unchanged
        assert_eq!(app.scroll_offset, 5);
        assert!(app.user_scrolled);
    }
}
