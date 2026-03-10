#[cfg(feature = "embedded")]
use crate::embedded_backend::EmbeddedBackend;
use crate::fawx_backend::{
    friendly_error_message, BackendEvent, EngineBackend, EngineStatus, HttpBackend,
};
use crate::markdown_render::render_markdown_text_with_width;
use crate::render::line_utils::{line_to_static, prefix_lines};
use crate::wrapping::{adaptive_wrap_line, RtOptions};
use anyhow::Context;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event as CEvent, EventStream, KeyCode, KeyEvent,
    KeyModifiers, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
};
use crossterm::Command;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use sparx::{render_file, RenderConfig};
use std::cmp::min;
use std::fmt;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

const HERO: &str = r#"███████  █████  ██     ██ ██   ██
██      ██   ██ ██     ██  ██ ██
█████   ███████ ██  █  ██   ███
██      ██   ██ ██ ███ ██  ██ ██
██      ██   ██  ███ ███  ██   ██"#;

const INPUT_PLACEHOLDER: &str = "Ask Fawx anything...";
const SHORTCUT_HINT: &str =
    "Ctrl+C: cancel | /help: commands | /clear: clear transcript | /quit: exit";
const THINKING_FRAMES: [&str; 3] = [".", "..", "..."];

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub embedded: bool,
    pub host: String,
}

pub async fn run_tui(options: RunOptions) -> anyhow::Result<()> {
    let (backend, connection_target) = build_backend(&options)?;
    let (tx, rx) = unbounded_channel();
    let mut app = App::new(backend, connection_target, tx.clone(), rx);
    app.spawn_bootstrap();

    let mut terminal = init_terminal()?;
    let result = app.run(&mut terminal).await;
    restore_terminal(&mut terminal)?;
    result
}

fn build_backend(options: &RunOptions) -> anyhow::Result<(Arc<dyn EngineBackend>, String)> {
    if options.embedded {
        return build_embedded_backend();
    }
    Ok(build_http_backend(&options.host))
}

fn build_http_backend(host: &str) -> (Arc<dyn EngineBackend>, String) {
    let backend = HttpBackend::new(host);
    let target = backend.base_url().to_string();
    (Arc::new(backend), target)
}

#[cfg(feature = "embedded")]
fn build_embedded_backend() -> anyhow::Result<(Arc<dyn EngineBackend>, String)> {
    let app = fx_cli::build_headless_app(None)?;
    Ok((
        Arc::new(EmbeddedBackend::new(app)),
        "embedded engine".to_string(),
    ))
}

#[cfg(not(feature = "embedded"))]
fn build_embedded_backend() -> anyhow::Result<(Arc<dyn EngineBackend>, String)> {
    Err(anyhow::anyhow!(
        "Embedded mode requires the 'embedded' feature. Build with: cargo build -p fawx-tui --features embedded"
    ))
}

#[derive(Clone, Copy)]
enum EntryRole {
    Hero,
    User,
    Assistant,
    System,
    Error,
    ToolUse,
    ToolResult,
    ToolError,
}

struct Entry {
    role: EntryRole,
    text: String,
}

impl fmt::Display for EntryRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Hero => "hero",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Error => "error",
            Self::ToolUse => "tool_use",
            Self::ToolResult => "tool_result",
            Self::ToolError => "tool_error",
        };
        f.write_str(label)
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.role, self.text)
    }
}

#[derive(Clone, Copy)]
struct TokenUsageSummary {
    input: u64,
    output: u64,
}

enum ConnectionState {
    Connecting,
    Connected(EngineStatus),
    Error(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EnableAlternateScroll;

impl Command for EnableAlternateScroll {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1007h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute EnableAlternateScroll using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DisableAlternateScroll;

impl Command for DisableAlternateScroll {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1007l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute DisableAlternateScroll using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

struct App {
    backend: Arc<dyn EngineBackend>,
    connection_target: String,
    tx: UnboundedSender<BackendEvent>,
    rx: UnboundedReceiver<BackendEvent>,
    entries: Vec<Entry>,
    input: String,
    connection: ConnectionState,
    streaming_text: Option<String>,
    logo_art: String,
    logo_width: Option<u32>,
    pending_request: bool,
    awaiting_stream_start: bool,
    follow_output: bool,
    scroll: u16,
    input_scroll: u16,
    spinner_frame: usize,
    last_meta: Option<String>,
    last_tokens: Option<TokenUsageSummary>,
    should_quit: bool,
    transcript_area: Rect,
    input_area: Rect,
}

impl App {
    fn new(
        backend: Arc<dyn EngineBackend>,
        connection_target: String,
        tx: UnboundedSender<BackendEvent>,
        rx: UnboundedReceiver<BackendEvent>,
    ) -> Self {
        Self {
            backend,
            connection_target,
            tx,
            rx,
            entries: initial_entries(HERO),
            input: String::new(),
            connection: ConnectionState::Connecting,
            streaming_text: None,
            logo_art: HERO.to_string(),
            logo_width: None,
            pending_request: false,
            awaiting_stream_start: false,
            follow_output: true,
            scroll: 0,
            input_scroll: 0,
            spinner_frame: 0,
            last_meta: None,
            last_tokens: None,
            should_quit: false,
            transcript_area: Rect::default(),
            input_area: Rect::default(),
        }
    }

    fn spawn_bootstrap(&self) {
        let backend = Arc::clone(&self.backend);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            backend.check_health(tx).await;
        });
    }

    async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> anyhow::Result<()> {
        let mut events = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(33));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame))?;

            tokio::select! {
                _ = tick.tick() => self.handle_tick(),
                maybe_event = events.next() => {
                    if let Some(Ok(event)) = maybe_event {
                        self.handle_terminal_event(event);
                    }
                }
                maybe_backend = self.rx.recv() => {
                    if let Some(event) = maybe_backend {
                        self.handle_backend_event(event);
                    }
                }
            }
        }

        Ok(())
    }

    fn handle_tick(&mut self) {
        if self.pending_request && self.awaiting_stream_start {
            self.spinner_frame = (self.spinner_frame + 1) % (THINKING_FRAMES.len() * 4);
        }
    }

    fn handle_terminal_event(&mut self, event: CEvent) {
        match event {
            CEvent::Key(key) => self.handle_key_event(key),
            CEvent::Mouse(mouse) => self.handle_mouse_event(mouse),
            CEvent::Resize(_, _) => self.follow_output = true,
            _ => {}
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Enter => self.submit_input(),
            KeyCode::Backspace => {
                self.input.pop();
                self.scroll_input_to_bottom();
            }
            KeyCode::Up => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.input_scroll = self.input_scroll.saturating_sub(1);
                } else {
                    self.follow_output = false;
                    self.scroll = self.scroll.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.input_scroll = self.input_scroll.saturating_add(1);
                } else {
                    self.follow_output = false;
                    self.scroll = self.scroll.saturating_add(1);
                }
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.push(ch);
                self.scroll_input_to_bottom();
            }
            _ => {}
        }
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.event_is_over_input(mouse) {
                    self.input_scroll = self.input_scroll.saturating_sub(3);
                } else {
                    self.follow_output = false;
                    self.scroll = self.scroll.saturating_sub(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.event_is_over_input(mouse) {
                    self.input_scroll = self.input_scroll.saturating_add(3);
                } else {
                    self.follow_output = false;
                    self.scroll = self.scroll.saturating_add(3);
                }
            }
            _ => {}
        }
    }

    fn event_is_over_input(&self, mouse: MouseEvent) -> bool {
        rect_contains(self.input_area, mouse.column, mouse.row)
    }

    fn submit_input(&mut self) {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return;
        }

        if self.handle_local_command(&input) {
            self.input.clear();
            self.follow_output = true;
            return;
        }

        if self.pending_request {
            self.push_system("A response is already in progress.");
            self.input.clear();
            return;
        }

        self.entries.push(Entry {
            role: EntryRole::User,
            text: input.clone(),
        });
        self.streaming_text = Some(String::new());
        self.pending_request = true;
        self.awaiting_stream_start = true;
        self.follow_output = true;
        self.spinner_frame = 0;
        self.last_meta = None;
        self.input.clear();
        self.input_scroll = 0;

        let backend = Arc::clone(&self.backend);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            backend.stream_message(input, tx).await;
        });
    }

    fn handle_local_command(&mut self, input: &str) -> bool {
        match input.split_whitespace().next() {
            Some("/clear") => {
                self.clear_transcript();
                true
            }
            Some("/quit") | Some("/exit") => {
                self.should_quit = true;
                true
            }
            _ => false,
        }
    }

    fn clear_transcript(&mut self) {
        self.entries = initial_entries(&self.logo_art);
        self.streaming_text = None;
        self.pending_request = false;
        self.awaiting_stream_start = false;
        self.last_meta = None;
        self.last_tokens = None;
        self.input_scroll = 0;
        self.push_system("Transcript cleared.");
    }

    fn handle_backend_event(&mut self, event: BackendEvent) {
        match event {
            BackendEvent::Connected(status) => {
                self.push_system(format!(
                    "Connected to Fawx on {} using model {}.",
                    self.connection_target, status.model
                ));
                self.connection = ConnectionState::Connected(status);
            }
            BackendEvent::ConnectionError(error) => {
                let friendly = friendly_error_message(&error);
                self.push_error(format!("Connection failed: {friendly}"));
                self.connection = ConnectionState::Error(friendly);
            }
            BackendEvent::TextDelta(delta) => {
                self.awaiting_stream_start = false;
                self.streaming_text
                    .get_or_insert_with(String::new)
                    .push_str(&delta);
                self.follow_output = true;
            }
            BackendEvent::ToolUse { name, arguments } => {
                self.awaiting_stream_start = false;
                let text = match serde_json::to_string_pretty(&arguments) {
                    Ok(arguments) if arguments != "null" && arguments != "{}" => {
                        format!("{name}\n{arguments}")
                    }
                    _ => name,
                };
                self.entries.push(Entry {
                    role: EntryRole::ToolUse,
                    text,
                });
                self.follow_output = true;
            }
            BackendEvent::ToolResult {
                name,
                success,
                content,
            } => {
                self.awaiting_stream_start = false;
                let text = match name {
                    Some(name) if !name.is_empty() => format!("{name}\n{content}"),
                    _ => content,
                };
                self.entries.push(Entry {
                    role: if success {
                        EntryRole::ToolResult
                    } else {
                        EntryRole::ToolError
                    },
                    text,
                });
                self.follow_output = true;
            }
            BackendEvent::Done {
                model,
                iterations,
                input_tokens,
                output_tokens,
            } => {
                if let Some(text) = self.streaming_text.take() {
                    self.entries.push(Entry {
                        role: EntryRole::Assistant,
                        text,
                    });
                }
                self.pending_request = false;
                self.awaiting_stream_start = false;
                self.follow_output = true;
                self.last_meta = Some(match iterations {
                    Some(1) => "1 iter".to_string(),
                    Some(value) => format!("{value} iters"),
                    None => "ready".to_string(),
                });
                self.last_tokens = match (input_tokens, output_tokens) {
                    (Some(input), Some(output)) => Some(TokenUsageSummary { input, output }),
                    _ => None,
                };
                if let Some(model) = model {
                    if let ConnectionState::Connected(status) = &mut self.connection {
                        status.model = model;
                    }
                }
            }
            BackendEvent::StreamError(error) => {
                if let Some(text) = self.streaming_text.take() {
                    if !text.is_empty() {
                        self.entries.push(Entry {
                            role: EntryRole::Assistant,
                            text,
                        });
                    }
                }
                self.pending_request = false;
                self.awaiting_stream_start = false;
                self.push_error(format!(
                    "Request failed: {}",
                    friendly_error_message(&error)
                ));
            }
        }
    }

    fn push_system(&mut self, message: impl Into<String>) {
        self.entries.push(Entry {
            role: EntryRole::System,
            text: message.into(),
        });
        self.follow_output = true;
    }

    fn push_error(&mut self, message: impl Into<String>) {
        self.entries.push(Entry {
            role: EntryRole::Error,
            text: message.into(),
        });
        self.follow_output = true;
    }

    fn draw(&mut self, frame: &mut Frame<'_>) {
        let size = frame.area();
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(4),
            ])
            .split(size);
        self.transcript_area = layout[1];
        self.input_area = layout[2];
        self.sync_logo_art(layout[1].width);

        frame.render_widget(self.render_header(), layout[0]);
        frame.render_widget(self.render_transcript(layout[1]), layout[1]);
        self.render_input(frame, layout[2]);
    }

    /// Re-render the ASCII logo art when the terminal width changes.
    ///
    /// Cached by `logo_width`: only re-renders when the computed target width
    /// differs from the last render. The `sparx::render_file` call is the
    /// expensive part; on steady-state frames this is a no-op comparison.
    fn sync_logo_art(&mut self, area_width: u16) {
        let desired_width = logo_target_width(area_width);
        if self.logo_width == Some(desired_width) {
            return;
        }

        self.logo_width = Some(desired_width);
        self.logo_art = render_logo_art(desired_width).unwrap_or_else(|_| HERO.to_string());

        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| matches!(entry.role, EntryRole::Hero))
        {
            entry.text = self.logo_art.clone();
        }
    }

    fn render_header(&self) -> Paragraph<'static> {
        let (mut status, status_style) = match &self.connection {
            ConnectionState::Connecting => (
                "connecting to the local Fawx engine...".to_string(),
                Style::default().fg(Color::Gray),
            ),
            ConnectionState::Connected(status) => (
                format!("🦊 {} • mem {}", status.model, status.memory_entries),
                Style::default().fg(Color::Gray),
            ),
            ConnectionState::Error(error) => (
                format!("engine error • {error}"),
                Style::default().fg(Color::Red),
            ),
        };
        let mut details = Vec::new();
        if self.pending_request && self.awaiting_stream_start {
            details.push(format!(
                "Fawx is thinking{}",
                THINKING_FRAMES[(self.spinner_frame / 4) % THINKING_FRAMES.len()]
            ));
        } else if let Some(meta) = &self.last_meta {
            details.push(meta.clone());
        }
        if !self.pending_request {
            if let Some(tokens) = self.last_tokens {
                details.push(format_token_usage(tokens));
            }
        }
        if !details.is_empty() {
            status.push_str(" • ");
            status.push_str(&details.join(" • "));
        }

        Paragraph::new(vec![Line::from(vec![Span::styled(status, status_style)])]).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(fawx_amber()))
                .title(Line::from(Span::styled(
                    "Fawx",
                    Style::default()
                        .fg(fawx_amber())
                        .add_modifier(Modifier::BOLD),
                ))),
        )
    }

    fn render_transcript(&mut self, area: Rect) -> Paragraph<'static> {
        let scroll = self.sync_transcript_scroll(area);
        self.transcript_widget(area).scroll((scroll, 0))
    }

    fn transcript_widget(&self, area: Rect) -> Paragraph<'static> {
        let inner_width = area.width.saturating_sub(4) as usize;
        let lines = self.rendered_transcript_lines(inner_width.max(20));
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Conversation"))
    }

    fn transcript_max_scroll(&self, area: Rect) -> u16 {
        let inner_width = area.width.saturating_sub(4) as usize;
        let lines = self.rendered_transcript_lines(inner_width.max(20));
        let total_lines = lines.len().saturating_add(2);
        let bottom = total_lines.saturating_sub(area.height as usize);
        bottom.min(u16::MAX as usize) as u16
    }

    fn sync_transcript_scroll(&mut self, area: Rect) -> u16 {
        let bottom = self.transcript_max_scroll(area);
        let scroll = if self.follow_output {
            bottom
        } else {
            min(self.scroll, bottom)
        };
        self.scroll = scroll;
        self.follow_output = scroll >= bottom;
        scroll
    }

    fn render_input(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let (widget, cursor) = self.input_widget(area);
        frame.render_widget(widget, area);
        if let Some((x, y)) = cursor {
            frame.set_cursor_position((x, y));
        }
    }

    fn input_widget(&mut self, area: Rect) -> (Paragraph<'static>, Option<(u16, u16)>) {
        let lines = self.rendered_input_lines(area.width.saturating_sub(2) as usize);
        let scroll = self.sync_input_scroll(area, lines.len());
        let cursor = self.input_cursor(area, &lines, scroll);
        let title = if self.pending_request {
            "Message (waiting for response)"
        } else {
            "Message"
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.pending_request {
                fawx_amber()
            } else {
                Color::DarkGray
            }))
            .title(title);
        let widget = Paragraph::new(lines).scroll((scroll, 0)).block(block);
        (widget, cursor)
    }

    fn rendered_input_lines(&self, width: usize) -> Vec<Line<'static>> {
        if self.input.is_empty() {
            return vec![
                Line::from(vec![Span::styled(
                    INPUT_PLACEHOLDER,
                    Style::default().fg(Color::DarkGray),
                )]),
                Line::from(vec![Span::styled(
                    SHORTCUT_HINT,
                    Style::default().fg(Color::DarkGray),
                )]),
            ];
        }

        wrap_plain_text(&self.input, width.max(1))
    }

    fn input_max_scroll(&self, area: Rect, total_lines: usize) -> u16 {
        let total_lines = total_lines.saturating_add(2);
        total_lines
            .saturating_sub(area.height as usize)
            .min(u16::MAX as usize) as u16
    }

    fn sync_input_scroll(&mut self, area: Rect, total_lines: usize) -> u16 {
        let bottom = self.input_max_scroll(area, total_lines);
        self.input_scroll = min(self.input_scroll, bottom);
        self.input_scroll
    }

    fn input_cursor(&self, area: Rect, lines: &[Line<'static>], scroll: u16) -> Option<(u16, u16)> {
        if self.input.is_empty() {
            return Some((area.x.saturating_add(1), area.y.saturating_add(1)));
        }

        let cursor_line = lines.len().saturating_sub(1);
        if cursor_line < scroll as usize {
            return None;
        }

        let visible_row = cursor_line - scroll as usize;
        let inner_height = area.height.saturating_sub(2) as usize;
        if visible_row >= inner_height {
            return None;
        }

        let x = area
            .x
            .saturating_add(1)
            .saturating_add(lines[cursor_line].width() as u16);
        let max_x = area.x.saturating_add(area.width.saturating_sub(2));
        Some((x.min(max_x), area.y.saturating_add(1 + visible_row as u16)))
    }

    fn scroll_input_to_bottom(&mut self) {
        let width = self.input_area.width.saturating_sub(2).max(1) as usize;
        let total_lines = wrap_plain_text(&self.input, width).len();
        self.input_scroll = self.input_max_scroll(self.input_area, total_lines);
    }

    fn rendered_transcript_lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        for entry in &self.entries {
            if matches!(entry.role, EntryRole::Hero) && !self.should_show_logo_art() {
                continue;
            }
            self.render_entry(entry, width, &mut out);
            out.push(Line::default());
        }
        if let Some(text) = &self.streaming_text {
            self.render_entry(
                &Entry {
                    role: EntryRole::Assistant,
                    text: text.clone(),
                },
                width,
                &mut out,
            );
        }
        out
    }

    fn should_show_logo_art(&self) -> bool {
        self.input.trim().is_empty()
    }

    fn render_entry(&self, entry: &Entry, width: usize, out: &mut Vec<Line<'static>>) {
        match entry.role {
            EntryRole::Hero => {
                out.extend(
                    entry
                        .text
                        .lines()
                        .map(|line| {
                            Line::from(vec![Span::styled(
                                line.to_string(),
                                Style::default()
                                    .fg(fawx_amber())
                                    .add_modifier(Modifier::BOLD),
                            )])
                        })
                        .collect::<Vec<_>>(),
                );
            }
            EntryRole::Assistant => {
                let rendered =
                    render_markdown_text_with_width(&entry.text, Some(width.saturating_sub(7)));
                let prefixed = prefix_lines(
                    rendered.lines,
                    Span::styled("fawx › ", Style::default().fg(fawx_amber())),
                    Span::raw("       "),
                );
                out.extend(prefixed);
            }
            EntryRole::User => {
                out.extend(prefix_wrapped_lines(
                    &entry.text,
                    width,
                    "you  › ",
                    "       ",
                    Style::default().fg(Color::Cyan),
                ));
            }
            EntryRole::System => {
                out.extend(prefix_wrapped_lines(
                    &entry.text,
                    width,
                    "info › ",
                    "       ",
                    Style::default().fg(Color::Gray),
                ));
            }
            EntryRole::Error => {
                out.extend(prefix_wrapped_lines(
                    &entry.text,
                    width,
                    "error! ",
                    "       ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ));
            }
            EntryRole::ToolUse => {
                out.extend(prefix_wrapped_lines(
                    &entry.text,
                    width,
                    "tool › ",
                    "       ",
                    Style::default().fg(Color::Magenta),
                ));
            }
            EntryRole::ToolResult => {
                out.extend(prefix_wrapped_lines(
                    &entry.text,
                    width,
                    "tool · ",
                    "       ",
                    Style::default().fg(Color::Green),
                ));
            }
            EntryRole::ToolError => {
                out.extend(prefix_wrapped_lines(
                    &entry.text,
                    width,
                    "tool ! ",
                    "       ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ));
            }
        }
    }
}

fn fawx_amber() -> Color {
    Color::Rgb(255, 140, 0)
}

fn initial_entries(logo_art: &str) -> Vec<Entry> {
    vec![
        Entry {
            role: EntryRole::Hero,
            text: logo_art.to_string(),
        },
        Entry {
            role: EntryRole::System,
            text: "connecting to Fawx...".to_string(),
        },
        Entry {
            role: EntryRole::System,
            text: "Use /help to see available commands.".to_string(),
        },
    ]
}

fn render_logo_art(width: u32) -> anyhow::Result<String> {
    let image_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("fawx.png");
    let config = RenderConfig {
        width: Some(width),
        color: false,
        ..Default::default()
    };
    render_file(&image_path.to_string_lossy(), &config)
        .with_context(|| format!("render splash art {}", image_path.display()))
}

fn logo_target_width(area_width: u16) -> u32 {
    u32::from(area_width.saturating_sub(4).clamp(24, 110))
}

fn format_token_usage(tokens: TokenUsageSummary) -> String {
    format!(
        "↑{} ↓{} tokens",
        format_token_count(tokens.input),
        format_token_count(tokens.output)
    )
}

fn format_token_count(value: u64) -> String {
    if value == 0 {
        "—".to_string()
    } else if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn prefix_wrapped_lines(
    text: &str,
    width: usize,
    initial: &str,
    subsequent: &str,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let content = Line::from(raw_line.to_string());
        let wrapped = adaptive_wrap_line(
            &content,
            RtOptions::new(width)
                .initial_indent(Line::from(vec![Span::styled(
                    initial.to_string(),
                    prefix_style,
                )]))
                .subsequent_indent(Line::from(subsequent.to_string())),
        );
        out.extend(wrapped.iter().map(line_to_static));
    }
    if text.ends_with('\n') {
        out.push(Line::from(vec![Span::styled(
            initial.to_string(),
            prefix_style,
        )]));
    }
    if out.is_empty() {
        out.push(Line::from(vec![Span::styled(
            initial.to_string(),
            prefix_style,
        )]));
    }
    out
}

fn wrap_plain_text(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let content = Line::from(raw_line.to_string());
        let wrapped = adaptive_wrap_line(&content, RtOptions::new(width));
        out.extend(wrapped.iter().map(line_to_static));
    }
    if text.ends_with('\n') {
        out.push(Line::default());
    }
    if out.is_empty() {
        out.push(Line::default());
    }
    out
}

fn rect_contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn init_terminal() -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableAlternateScroll,
        EnableMouseCapture,
        SetTitle("Fawx")
    )
    .context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    terminal.clear().context("clear alternate screen")?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> anyhow::Result<()> {
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableAlternateScroll,
        LeaveAlternateScreen
    )
    .context("leave alt screen")?;
    disable_raw_mode().context("disable raw mode")?;
    terminal.show_cursor().context("show cursor")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::OnceLock;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn test_app() -> App {
        let backend: Arc<dyn EngineBackend> = Arc::new(HttpBackend::from_env());
        let (tx, rx) = unbounded_channel();
        let mut app = App::new(backend, crate::DEFAULT_ENGINE_URL.to_string(), tx, rx);
        app.entries.clear();
        app
    }

    #[derive(Default)]
    struct TestBackend {
        health_checks: AtomicUsize,
    }

    #[async_trait]
    impl EngineBackend for TestBackend {
        async fn stream_message(&self, _message: String, _tx: UnboundedSender<BackendEvent>) {}

        async fn check_health(&self, tx: UnboundedSender<BackendEvent>) {
            self.health_checks.fetch_add(1, Ordering::SeqCst);
            let _ = tx.send(BackendEvent::Connected(EngineStatus {
                status: "running".to_string(),
                model: "test-model".to_string(),
                memory_entries: 0,
            }));
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn entry_texts(app: &App) -> Vec<&str> {
        app.entries
            .iter()
            .map(|entry| entry.text.as_str())
            .collect()
    }

    fn last_entry_text(app: &App) -> &str {
        app.entries
            .last()
            .map(|entry| entry.text.as_str())
            .expect("expected at least one entry")
    }

    fn fill_transcript(app: &mut App, count: usize) {
        for index in 0..count {
            app.entries.push(Entry {
                role: EntryRole::Assistant,
                text: format!("line {index}: this is a deliberately long transcript entry to force wrapping in a narrow viewport"),
            });
        }
    }

    fn env_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    struct BaseUrlGuard {
        previous: Option<String>,
    }

    impl BaseUrlGuard {
        async fn set(base_url: &str) -> (tokio::sync::MutexGuard<'static, ()>, Self) {
            let guard = env_lock().lock().await;
            let previous = std::env::var("FAWX_TUI_BASE_URL").ok();
            std::env::set_var("FAWX_TUI_BASE_URL", base_url);
            (guard, Self { previous })
        }
    }

    impl Drop for BaseUrlGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("FAWX_TUI_BASE_URL", value),
                None => std::env::remove_var("FAWX_TUI_BASE_URL"),
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_bootstrap_uses_backend_trait_health_check() {
        let backend = Arc::new(TestBackend::default());
        let app_backend: Arc<dyn EngineBackend> = backend.clone();
        let (tx, rx) = unbounded_channel();
        let mut app = App::new(app_backend, "embedded engine".to_string(), tx, rx);
        app.entries.clear();

        app.spawn_bootstrap();
        let event = app.rx.recv().await.expect("bootstrap event");
        app.handle_backend_event(event);

        assert_eq!(backend.health_checks.load(Ordering::SeqCst), 1);
        assert_eq!(
            last_entry_text(&app),
            "Connected to Fawx on embedded engine using model test-model."
        );
    }

    async fn spawn_message_server(
        expected_message: &str,
        body: serde_json::Value,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("listener addr");
        let expected_body = format!(
            "{{\"message\":{}}}",
            serde_json::to_string(expected_message).expect("serialize message")
        );
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept connection");
            let mut request = vec![0_u8; 4096];
            let read = stream.read(&mut request).await.expect("read request");
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("POST /message "));
            assert!(request.contains(&expected_body));

            let payload = body.to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn follow_output_stays_pinned_to_bottom_as_transcript_grows() {
        let area = Rect::new(0, 0, 40, 8);
        let mut app = test_app();
        fill_transcript(&mut app, 8);

        let initial_bottom = app.transcript_max_scroll(area);
        let initial_scroll = app.sync_transcript_scroll(area);
        assert_eq!(initial_scroll, initial_bottom);
        assert_eq!(app.scroll, initial_bottom);
        assert!(app.follow_output);

        app.entries.push(Entry {
            role: EntryRole::ToolResult,
            text: "tool output\nwith enough detail to extend the transcript and move the viewport down"
                .to_string(),
        });

        let grown_bottom = app.transcript_max_scroll(area);
        let grown_scroll = app.sync_transcript_scroll(area);
        assert!(grown_bottom > initial_bottom);
        assert_eq!(grown_scroll, grown_bottom);
        assert_eq!(app.scroll, grown_bottom);
        assert!(app.follow_output);
    }

    #[test]
    fn scrolling_up_from_follow_mode_starts_from_current_bottom() {
        let area = Rect::new(0, 0, 40, 8);
        let mut app = test_app();
        fill_transcript(&mut app, 8);

        let bottom = app.sync_transcript_scroll(area);
        assert!(bottom > 0);

        app.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert!(!app.follow_output);
        assert_eq!(app.scroll, bottom.saturating_sub(1));
        assert_eq!(app.sync_transcript_scroll(area), bottom.saturating_sub(1));
    }

    #[test]
    fn alternate_scroll_commands_emit_expected_sequences() {
        let mut enable = String::new();
        EnableAlternateScroll.write_ansi(&mut enable).unwrap();
        assert_eq!(enable, "\x1b[?1007h");

        let mut disable = String::new();
        DisableAlternateScroll.write_ansi(&mut disable).unwrap();
        assert_eq!(disable, "\x1b[?1007l");
    }

    #[test]
    fn empty_input_shows_placeholder_and_shortcuts() {
        let app = test_app();
        let lines = app.rendered_input_lines(80);

        assert_eq!(line_text(&lines[0]), INPUT_PLACEHOLDER);
        assert_eq!(line_text(&lines[1]), SHORTCUT_HINT);
    }

    #[test]
    fn token_usage_is_formatted_compactly() {
        assert_eq!(
            format_token_usage(TokenUsageSummary {
                input: 1_200,
                output: 3_400,
            }),
            "↑1.2k ↓3.4k tokens"
        );
        assert_eq!(
            format_token_usage(TokenUsageSummary {
                input: 0,
                output: 0,
            }),
            "↑— ↓— tokens"
        );
    }

    #[test]
    fn initial_entries_point_users_to_server_help() {
        let entries = initial_entries("fox art");
        assert!(matches!(entries[0].role, EntryRole::Hero));
        assert_eq!(entries[0].text, "fox art");
        assert!(entries
            .iter()
            .any(|entry| entry.text == "Use /help to see available commands."));
    }

    #[test]
    fn handle_local_command_returns_false_for_unknown_command() {
        let mut app = test_app();

        assert!(!app.handle_local_command("/definitely-unknown"));
        assert!(app.entries.is_empty());
    }

    #[test]
    fn only_clear_and_exit_commands_are_handled_locally() {
        let mut app = test_app();

        for command in [
            "/help",
            "/auth",
            "/model",
            "/config",
            "/memory",
            "/approvals",
            "/diff",
            "/search",
            "/agents",
            "/voice",
        ] {
            assert!(
                !app.handle_local_command(command),
                "{command} should be delegated to the server"
            );
        }
        assert!(app.entries.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn help_command_uses_message_endpoint() {
        let (base_url, server) = spawn_message_server(
            "/help",
            json!({
                "response": "server help",
                "model": "claude-opus-4-6",
                "iterations": 1
            }),
        )
        .await;
        let (_env_guard, _base_url_guard) = BaseUrlGuard::set(&base_url).await;
        let mut app = test_app();
        app.input = "/help".to_string();

        app.submit_input();
        let text_delta = app.rx.recv().await.expect("text delta event");
        app.handle_backend_event(text_delta);
        let done = app.rx.recv().await.expect("done event");
        app.handle_backend_event(done);
        server.await.expect("join message server");

        assert_eq!(app.entries[0].text, "/help");
        assert_eq!(last_entry_text(&app), "server help");
    }

    #[test]
    fn clear_command_resets_transcript_state_and_keeps_clear_notice() {
        let mut app = test_app();
        app.entries.push(Entry {
            role: EntryRole::Assistant,
            text: "stale".to_string(),
        });
        app.streaming_text = Some("partial".to_string());
        app.pending_request = true;
        app.awaiting_stream_start = true;
        app.last_meta = Some("meta".to_string());
        app.last_tokens = Some(TokenUsageSummary {
            input: 1,
            output: 2,
        });
        app.input_scroll = 4;

        assert!(app.handle_local_command("/clear"));

        assert!(entry_texts(&app).contains(&"Transcript cleared."));
        assert!(app.streaming_text.is_none());
        assert!(!app.pending_request);
        assert!(!app.awaiting_stream_start);
        assert!(app.last_meta.is_none());
        assert!(app.last_tokens.is_none());
        assert_eq!(app.input_scroll, 0);
    }

    #[test]
    fn quit_and_exit_commands_set_should_quit() {
        let mut quit_app = test_app();
        assert!(quit_app.handle_local_command("/quit"));
        assert!(quit_app.should_quit);

        let mut exit_app = test_app();
        assert!(exit_app.handle_local_command("/exit"));
        assert!(exit_app.should_quit);
    }
}
