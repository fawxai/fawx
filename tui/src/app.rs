#[cfg(feature = "embedded")]
use crate::embedded_backend::EmbeddedBackend;
use crate::experiment_panel::ExperimentPanel;
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
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use regex_lite::Regex;
use serde_json::Value;
use sparx::{render_file, RenderConfig};
use std::cmp::min;
use std::fmt;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

const INPUT_PLACEHOLDER: &str = "Ask Fawx anything...";
const SHORTCUT_HINT: &str =
    "Ctrl+C: cancel | /help: commands | /clear: clear transcript | /quit: exit";
const THINKING_FRAMES: [&str; 3] = [".", "..", "..."];

type AppBackend = Arc<dyn EngineBackend>;
type SharedPanel = Arc<Mutex<ExperimentPanel>>;
type BuiltBackend = (AppBackend, String, SharedPanel);
type BackendBuildResult = anyhow::Result<BuiltBackend>;

const WIDE_WELCOME_BREAKPOINT: usize = 100;
const MEDIUM_WELCOME_BREAKPOINT: usize = 60;
const WELCOME_COLUMN_GAP: usize = 3;
const WELCOME_LEFT_WIDTH: usize = 30;
const WELCOME_COMMAND_WIDTH: usize = 28;
const MAX_VISIBLE_SKILLS: usize = 8;
const VERSION_LABEL: &str = concat!("Fawx v", env!("CARGO_PKG_VERSION"));
const EMPTY_SKILLS_MESSAGE: &str =
    "No local skills installed. Run /skills for workflow help, fawx skill build <project> for local dev, or fawx skill install <path> for prebuilt artifacts.";
const DEFAULT_SKILL_ICON: &str = "🧩";
const ASCII_LOGO_ART: &str = r#"    ___                  
   / __\__ ___      ___ __
  / _\/ _` \ \ /\ / \ \/ /
 / / | (_| |\ V  V / >  < 
 \/   \__,_| \_/\_/ /_/\_\
"#;
const WELCOME_COMMANDS: [(&str, &str); 6] = [
    ("/help", "overview"),
    ("/model", "switch LLM"),
    ("/skills", "local skill state"),
    ("/clear", "clear chat"),
    ("/status", "engine info"),
    ("/quit", "exit"),
];
const EXPERIMENT_PANEL_TITLE: &str = "Experiment";
const TRANSCRIPT_WIDTH_PERCENT: u16 = 70;
const EXPERIMENT_PANEL_WIDTH_PERCENT: u16 = 30;
const PANEL_BORDER_LINES: usize = 2;
const TOOL_USE_PREFIX: &str = "tool › ";
const TOOL_RESULT_PREFIX: &str = "tool · ";
const TOOL_ERROR_PREFIX: &str = "tool ! ";
const TOOL_PREFIX_DISPLAY_WIDTH: usize = 7;
const TOOL_USE_FIELD_LIMIT: usize = 3;
const TOOL_USE_MAX_LINES: usize = 5;
const TOOL_RESULT_MAX_LINES: usize = 20;
const TOOL_VALUE_PREVIEW_CHARS: usize = 80;
static ANSI_CSI_RE: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]") {
        Ok(regex) => regex,
        Err(error) => panic!("invalid ANSI CSI regex: {error}"),
    });
static ANSI_OSC_RE: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)") {
        Ok(regex) => regex,
        Err(error) => panic!("invalid ANSI OSC regex: {error}"),
    });
static ANSI_ESC_RE: LazyLock<Regex> = LazyLock::new(|| match Regex::new(r"\x1b[@-_]") {
    Ok(regex) => regex,
    Err(error) => panic!("invalid ANSI escape regex: {error}"),
});

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub embedded: bool,
    pub host: String,
}

pub async fn run_tui(options: RunOptions) -> anyhow::Result<()> {
    let (backend, connection_target, experiment_panel) = build_backend(&options)?;
    let (tx, rx) = unbounded_channel();
    let mut app = App::new(backend, connection_target, experiment_panel, tx.clone(), rx);
    app.spawn_bootstrap();

    let mut terminal = init_terminal()?;
    let result = app.run(&mut terminal).await;
    restore_terminal(&mut terminal)?;
    result
}

fn build_backend(options: &RunOptions) -> BackendBuildResult {
    if options.embedded {
        build_embedded_backend()
    } else {
        build_http_backend(&options.host)
    }
}

fn build_http_backend(host: &str) -> BackendBuildResult {
    let backend = HttpBackend::new(host);
    let target = backend.base_url().to_string();
    Ok((
        Arc::new(backend),
        target,
        // HTTP mode: panel stays empty (no embedded progress callback), but kept for uniform API
        Arc::new(Mutex::new(ExperimentPanel::new())),
    ))
}

#[cfg(feature = "embedded")]
fn build_embedded_backend() -> BackendBuildResult {
    let (backend, experiment_panel) = EmbeddedBackend::build()?;
    Ok((
        Arc::new(backend),
        "embedded engine".to_string(),
        experiment_panel,
    ))
}

#[cfg(not(feature = "embedded"))]
fn build_embedded_backend() -> BackendBuildResult {
    Err(anyhow::anyhow!(
        "Embedded mode requires the 'embedded' feature. Build with: cargo build -p fawx-tui --features embedded"
    ))
}

#[derive(Clone, Copy)]
enum EntryRole {
    Welcome,
    User,
    Assistant,
    System,
    Error,
    ToolUse,
    ToolResult,
    ToolError,
}

#[derive(Clone, Copy)]
enum ToolOutcome {
    Success,
    Error,
}

struct Entry {
    role: EntryRole,
    text: String,
    tool_name: Option<String>,
    tool_arguments: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocalSkillSummary {
    icon: String,
    name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalSkillState {
    BuiltLocally,
    InstalledLocally,
}

impl LocalSkillState {
    fn description(self) -> &'static str {
        match self {
            Self::BuiltLocally => "Built locally: artifact exists in the repo/build tree",
            Self::InstalledLocally => "Installed locally: skill exists in ~/.fawx/skills",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::BuiltLocally => "Built locally (repo artifact found):",
            Self::InstalledLocally => "Installed locally (~/.fawx/skills):",
        }
    }

    fn marker(self) -> char {
        match self {
            Self::BuiltLocally => '○',
            Self::InstalledLocally => '✓',
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WelcomeLayout {
    Wide,
    Medium,
    Narrow,
}

impl WelcomeLayout {
    fn for_width(width: usize) -> Self {
        if width > WIDE_WELCOME_BREAKPOINT {
            Self::Wide
        } else if width >= MEDIUM_WELCOME_BREAKPOINT {
            Self::Medium
        } else {
            Self::Narrow
        }
    }
}

impl fmt::Display for EntryRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Welcome => "welcome",
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

impl Entry {
    fn plain(role: EntryRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
            tool_name: None,
            tool_arguments: None,
        }
    }

    fn tool_use(name: String, arguments: Value) -> Self {
        Self {
            role: EntryRole::ToolUse,
            text: name.clone(),
            tool_name: Some(name),
            tool_arguments: normalize_tool_arguments(arguments),
        }
    }

    fn tool_result(outcome: ToolOutcome, name: Option<String>, content: String) -> Self {
        Self {
            role: outcome.role(),
            text: content,
            tool_name: name,
            tool_arguments: None,
        }
    }
}

impl ToolOutcome {
    fn role(self) -> EntryRole {
        match self {
            Self::Success => EntryRole::ToolResult,
            Self::Error => EntryRole::ToolError,
        }
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
    backend: AppBackend,
    connection_target: String,
    experiment_panel: SharedPanel,
    tx: UnboundedSender<BackendEvent>,
    rx: UnboundedReceiver<BackendEvent>,
    entries: Vec<Entry>,
    input: String,
    connection: ConnectionState,
    streaming_text: Option<String>,
    logo_art: String,
    installed_skills: Vec<LocalSkillSummary>,
    pending_request: bool,
    awaiting_stream_start: bool,
    follow_output: bool,
    scroll: u16,
    input_scroll: u16,
    spinner_frame: usize,
    last_meta: Option<String>,
    last_tokens: Option<TokenUsageSummary>,
    active_tool_name: Option<String>,
    should_quit: bool,
    transcript_area: Rect,
    input_area: Rect,
}

impl App {
    fn new(
        backend: AppBackend,
        connection_target: String,
        experiment_panel: SharedPanel,
        tx: UnboundedSender<BackendEvent>,
        rx: UnboundedReceiver<BackendEvent>,
    ) -> Self {
        Self {
            backend,
            connection_target,
            experiment_panel,
            tx,
            rx,
            entries: initial_entries(),
            input: String::new(),
            connection: ConnectionState::Connecting,
            streaming_text: None,
            logo_art: String::new(),
            installed_skills: discover_installed_skills(),
            pending_request: false,
            awaiting_stream_start: false,
            follow_output: true,
            scroll: 0,
            input_scroll: 0,
            spinner_frame: 0,
            last_meta: None,
            last_tokens: None,
            active_tool_name: None,
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
        self.advance_spinner();
        self.update_experiment_panel();
    }

    fn advance_spinner(&mut self) {
        if self.pending_request && self.awaiting_stream_start {
            self.spinner_frame = (self.spinner_frame + 1) % (THINKING_FRAMES.len() * 4);
        }
    }

    fn update_experiment_panel(&self) {
        let Ok(mut panel) = self.experiment_panel.lock() else {
            return;
        };
        let _ = panel.check_auto_hide();
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

        self.dismiss_welcome_entry(&input);
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

        self.entries
            .push(Entry::plain(EntryRole::User, input.clone()));
        self.streaming_text = Some(String::new());
        self.pending_request = true;
        self.awaiting_stream_start = true;
        self.follow_output = true;
        self.spinner_frame = 0;
        self.last_meta = None;
        self.active_tool_name = None;
        self.input.clear();
        self.input_scroll = 0;

        let backend = Arc::clone(&self.backend);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            backend.stream_message(input, tx).await;
        });
    }

    fn dismiss_welcome_entry(&mut self, input: &str) {
        if should_dismiss_welcome(&self.entries, input) {
            self.entries.remove(0);
        }
    }

    fn handle_local_command(&mut self, input: &str) -> bool {
        match input.split_whitespace().next() {
            Some("/clear") => {
                self.clear_transcript();
                true
            }
            Some("/skills") => {
                self.show_skills_list();
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
        self.entries = initial_entries();
        self.installed_skills = discover_installed_skills();
        self.streaming_text = None;
        self.pending_request = false;
        self.awaiting_stream_start = false;
        self.last_meta = None;
        self.last_tokens = None;
        self.active_tool_name = None;
        self.input_scroll = 0;
        self.clear_experiment_panel();
        self.push_system("Transcript cleared.");
    }

    fn clear_experiment_panel(&self) {
        let Ok(mut panel) = self.experiment_panel.lock() else {
            return;
        };
        panel.clear();
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
            BackendEvent::ToolUse { name, arguments } => self.push_tool_use(name, arguments),
            BackendEvent::ToolResult {
                name,
                success,
                content,
            } => self.push_tool_result(name, success, content),
            BackendEvent::Done {
                model,
                iterations,
                input_tokens,
                output_tokens,
            } => {
                if let Some(text) = self.streaming_text.take() {
                    self.entries.push(Entry::plain(EntryRole::Assistant, text));
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
                        self.entries.push(Entry::plain(EntryRole::Assistant, text));
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

    fn push_tool_use(&mut self, name: String, arguments: Value) {
        self.awaiting_stream_start = false;
        self.active_tool_name = Some(name.clone());
        if let Some(entry) = self.entries.last_mut() {
            if should_update_tool_use(entry, &name) {
                *entry = Entry::tool_use(name, arguments);
                self.follow_output = true;
                return;
            }
        }
        self.entries.push(Entry::tool_use(name, arguments));
        self.follow_output = true;
    }

    fn push_tool_result(&mut self, name: Option<String>, success: bool, content: String) {
        self.awaiting_stream_start = false;
        let outcome = if success {
            ToolOutcome::Success
        } else {
            ToolOutcome::Error
        };
        let resolved_name = self
            .active_tool_name
            .take()
            .or(name.filter(|value| !value.is_empty()));
        self.entries
            .push(Entry::tool_result(outcome, resolved_name, content));
        self.follow_output = true;
    }

    fn show_skills_list(&mut self) {
        let installed = discover_installed_skills();
        let available = discover_built_skills(&installed);
        let message = format_skills_message(&installed, &available);
        self.installed_skills = installed;
        self.push_system(message);
    }

    fn push_system(&mut self, message: impl Into<String>) {
        self.entries.push(Entry::plain(EntryRole::System, message));
        self.follow_output = true;
    }

    fn push_error(&mut self, message: impl Into<String>) {
        self.entries.push(Entry::plain(EntryRole::Error, message));
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
        let (transcript_area, experiment_area) = transcript_layout(layout[1], self.panel_visible());
        self.transcript_area = transcript_area;
        self.input_area = layout[2];
        self.sync_logo_art();

        frame.render_widget(self.render_header(), layout[0]);
        frame.render_widget(Clear, transcript_area);
        frame.render_widget(self.render_transcript(transcript_area), transcript_area);
        if let Some(experiment_area) = experiment_area {
            frame.render_widget(Clear, experiment_area);
            frame.render_widget(
                self.render_experiment_panel(experiment_area),
                experiment_area,
            );
        }
        self.render_input(frame, layout[2]);
    }

    fn panel_visible(&self) -> bool {
        let Ok(panel) = self.experiment_panel.lock() else {
            return false;
        };
        panel.is_visible()
    }

    fn render_experiment_panel(&self, area: Rect) -> Paragraph<'static> {
        let inner_width = area.width.saturating_sub(2) as usize;
        let lines = self.experiment_panel_lines(inner_width.max(1));
        let scroll = panel_scroll(lines.len(), area.height);
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(EXPERIMENT_PANEL_TITLE),
            )
    }

    fn experiment_panel_lines(&self, width: usize) -> Vec<Line<'static>> {
        let Ok(panel) = self.experiment_panel.lock() else {
            return vec![Line::default()];
        };
        render_panel_lines(panel.lines(), width)
    }

    /// Render the mascot art once for the fixed welcome banner layout.
    fn sync_logo_art(&mut self) {
        if !self.logo_art.is_empty() {
            return;
        }

        self.logo_art = render_logo_art(LOGO_RENDER_WIDTH).unwrap_or_else(|_| "🦊".to_string());
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
        let lines = self.transcript_lines_for_area(area);
        let scroll = self.sync_transcript_scroll(area, transcript_content_line_count(&lines));
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .block(transcript_block())
    }

    #[cfg(test)]
    fn transcript_max_scroll(&self, area: Rect) -> u16 {
        let lines = self.transcript_lines_for_area(area);
        transcript_scroll_limit(transcript_content_line_count(&lines), area)
    }

    fn sync_transcript_scroll(&mut self, area: Rect, total_lines: usize) -> u16 {
        let bottom = transcript_scroll_limit(total_lines, area);
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
        frame.render_widget(Clear, area);
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
            if !out.is_empty() {
                out.push(Line::default());
            }
            self.render_entry(entry, width, &mut out);
        }
        if let Some(text) = &self.streaming_text {
            if !out.is_empty() {
                out.push(Line::default());
            }
            self.render_entry(
                &Entry::plain(EntryRole::Assistant, text.clone()),
                width,
                &mut out,
            );
        }
        out
    }

    fn render_entry(&self, entry: &Entry, width: usize, out: &mut Vec<Line<'static>>) {
        match entry.role {
            EntryRole::Welcome => {
                out.extend(render_welcome_screen(
                    width,
                    &self.logo_art,
                    &self.installed_skills,
                ));
            }
            EntryRole::Assistant => {
                let text = sanitize_terminal_text(&entry.text);
                let rendered =
                    render_markdown_text_with_width(&text, Some(width.saturating_sub(7)));
                let prefixed = prefix_lines(
                    rendered.lines,
                    Span::styled("fawx › ", Style::default().fg(fawx_amber())),
                    Span::raw("       "),
                );
                out.extend(prefixed);
            }
            EntryRole::User => {
                let text = sanitize_terminal_text(&entry.text);
                out.extend(prefix_wrapped_lines(
                    &text,
                    width,
                    "you  › ",
                    "       ",
                    Style::default().fg(Color::Cyan),
                ));
            }
            EntryRole::System => {
                let text = sanitize_terminal_text(&entry.text);
                out.extend(prefix_wrapped_lines(
                    &text,
                    width,
                    "info › ",
                    "       ",
                    Style::default().fg(Color::Gray),
                ));
            }
            EntryRole::Error => {
                let text = sanitize_terminal_text(&entry.text);
                out.extend(prefix_wrapped_lines(
                    &text,
                    width,
                    "error! ",
                    "       ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ));
            }
            EntryRole::ToolUse => {
                out.extend(render_tool_use_entry(entry, width));
            }
            EntryRole::ToolResult => {
                out.extend(render_tool_result_entry(entry, width, self.panel_visible()));
            }
            EntryRole::ToolError => {
                out.extend(render_tool_result_entry(entry, width, self.panel_visible()));
            }
        }
    }

    fn transcript_lines_for_area(&self, area: Rect) -> Vec<Line<'static>> {
        self.rendered_transcript_lines(transcript_inner_width(area))
    }
}

fn transcript_layout(area: Rect, show_panel: bool) -> (Rect, Option<Rect>) {
    if !show_panel {
        return (area, None);
    }
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(TRANSCRIPT_WIDTH_PERCENT),
            Constraint::Percentage(EXPERIMENT_PANEL_WIDTH_PERCENT),
        ])
        .split(area);
    (layout[0], Some(layout[1]))
}

fn transcript_block() -> Block<'static> {
    Block::default().borders(Borders::ALL).title("Conversation")
}

fn transcript_inner_area(area: Rect) -> Rect {
    transcript_block().inner(area)
}

fn transcript_inner_width(area: Rect) -> usize {
    transcript_inner_area(area).width.max(1) as usize
}

fn transcript_scroll_limit(total_lines: usize, area: Rect) -> u16 {
    total_lines
        .saturating_sub(transcript_inner_area(area).height as usize)
        .min(u16::MAX as usize) as u16
}

fn transcript_content_line_count(lines: &[Line<'_>]) -> usize {
    lines
        .iter()
        .rposition(|line| transcript_line_has_content(line))
        .map_or(0, |index| index + 1)
}

fn transcript_line_has_content(line: &Line<'_>) -> bool {
    line.spans
        .iter()
        .any(|span| !span.content.trim().is_empty())
}

fn should_dismiss_welcome(entries: &[Entry], input: &str) -> bool {
    matches!(
        entries.first().map(|entry| entry.role),
        Some(EntryRole::Welcome)
    ) && !matches!(
        input.split_whitespace().next(),
        Some("/clear") | Some("/quit") | Some("/exit")
    )
}

fn sanitize_terminal_text(text: &str) -> String {
    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ");
    let without_osc = ANSI_OSC_RE.replace_all(&normalized, "");
    let without_csi = ANSI_CSI_RE.replace_all(&without_osc, "");
    let without_esc = ANSI_ESC_RE.replace_all(&without_csi, "");
    without_esc
        .chars()
        .filter(|ch| *ch == '\n' || !ch.is_control())
        .collect()
}

fn render_panel_lines(lines: &[String], width: usize) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    for line in lines {
        rendered.extend(wrap_plain_text(&sanitize_terminal_text(line), width));
    }
    if rendered.is_empty() {
        rendered.push(Line::default());
    }
    rendered
}

fn panel_scroll(total_lines: usize, area_height: u16) -> u16 {
    total_lines
        .saturating_add(PANEL_BORDER_LINES)
        .saturating_sub(area_height as usize)
        .min(u16::MAX as usize) as u16
}

fn fawx_amber() -> Color {
    Color::Rgb(255, 140, 0)
}

fn initial_entries() -> Vec<Entry> {
    vec![Entry::plain(EntryRole::Welcome, String::new())]
}

const LOGO_RENDER_WIDTH: u32 = 40;
const LOGO_RENDER_THRESHOLD: u8 = 35;

fn render_logo_art(width: u32) -> anyhow::Result<String> {
    let config = RenderConfig {
        width: Some(width),
        threshold: LOGO_RENDER_THRESHOLD,
        color: false,
        ..Default::default()
    };
    render_logo_variant("fawx-new.png", &config)
        .and_then(validate_logo_art)
        .or_else(|_| Ok(ASCII_LOGO_ART.to_string()))
}

fn render_logo_variant(name: &str, config: &RenderConfig) -> anyhow::Result<String> {
    let image_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(name);
    render_file(&image_path.to_string_lossy(), config)
        .with_context(|| format!("render splash art {}", image_path.display()))
}

fn validate_logo_art(art: String) -> anyhow::Result<String> {
    if logo_art_looks_garbled(&art) {
        anyhow::bail!("rendered logo art looked garbled")
    }
    Ok(art)
}

fn logo_art_looks_garbled(art: &str) -> bool {
    let visible = art.chars().filter(|ch| !ch.is_whitespace()).count();
    if visible == 0 {
        return true;
    }
    let noise = art.chars().filter(|ch| "⣿⣷⣶⣤⣀⠿⡿".contains(*ch)).count();
    noise * 2 >= visible
}

fn discover_installed_skills() -> Vec<LocalSkillSummary> {
    home_skills_dir()
        .map(|path| discover_installed_skills_from(&path))
        .unwrap_or_default()
}

fn discover_built_skills(installed: &[LocalSkillSummary]) -> Vec<LocalSkillSummary> {
    let Some(root) = repo_root_from_manifest_dir() else {
        return Vec::new();
    };
    let skills_dir = root.join("skills");
    let entries = match std::fs::read_dir(skills_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let installed = installed
        .iter()
        .map(|skill| skill.name.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    let mut skills = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| read_built_skill(&entry.path(), &installed))
        .collect::<Vec<_>>();
    skills.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    skills
}

fn home_skills_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".fawx").join("skills"))
}

fn discover_installed_skills_from(path: &Path) -> Vec<LocalSkillSummary> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut skills = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| read_skill_manifest(&entry.path()))
        .collect::<Vec<_>>();
    skills.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    skills
}

fn repo_root_from_manifest_dir() -> Option<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
}

fn read_built_skill(
    path: &Path,
    installed: &std::collections::BTreeSet<String>,
) -> Option<LocalSkillSummary> {
    if !built_skill_artifact_exists(path) {
        return None;
    }
    let skill = read_skill_manifest(path);
    (!installed.contains(&skill.name.to_lowercase())).then_some(skill)
}

fn built_skill_artifact_exists(path: &Path) -> bool {
    let package_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if package_name.is_empty() {
        return false;
    }
    wasm_artifact_names(package_name)
        .iter()
        .any(|artifact_name| {
            let target_wasm = path
                .join("target")
                .join("wasm32-wasip1")
                .join("release")
                .join(artifact_name);
            let packaged_wasm = path.join("pkg").join(artifact_name);
            target_wasm.exists() || packaged_wasm.exists()
        })
}

fn wasm_artifact_names(package_name: &str) -> [String; 2] {
    [
        format!("{package_name}.wasm"),
        format!("{}.wasm", package_name.replace('-', "_")),
    ]
}

fn read_skill_manifest(path: &Path) -> LocalSkillSummary {
    let fallback_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_string();
    let manifest_path = path.join("manifest.toml");
    let content = std::fs::read_to_string(&manifest_path).ok();
    let name = content
        .as_deref()
        .and_then(|value| parse_manifest_string(value, "name"))
        .unwrap_or_else(|| fallback_name.clone());
    let icon = content
        .as_deref()
        .and_then(|value| parse_manifest_string(value, "icon"))
        .unwrap_or_else(|| default_skill_icon(&name).to_string());
    LocalSkillSummary { icon, name }
}

fn parse_manifest_string(content: &str, field: &str) -> Option<String> {
    let prefix = format!("{field} =");
    for line in content.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some(rest) = line.strip_prefix(&prefix) else {
            continue;
        };
        let value = rest.trim();
        let parsed = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            });
        if let Some(parsed) = parsed.filter(|parsed| !parsed.trim().is_empty()) {
            return Some(parsed.trim().to_string());
        }
    }
    None
}

fn default_skill_icon(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "weather" => "🌤",
        "vision" => "👁",
        "tts" => "🔊",
        "browser" => "🌐",
        "canvas" => "🖼",
        "stt" => "🎤",
        "github" => "🐙",
        "calculator" => "🧮",
        _ => DEFAULT_SKILL_ICON,
    }
}

fn format_skills_message(
    installed: &[LocalSkillSummary],
    available: &[LocalSkillSummary],
) -> String {
    let mut lines = vec![
        "Local skill state (/skills):".to_string(),
        LocalSkillState::BuiltLocally.description().to_string(),
        LocalSkillState::InstalledLocally.description().to_string(),
        "Loaded on server: running server reports it via /v1/skills".to_string(),
        String::new(),
        "Recommended workflows:".to_string(),
        "  Local dev: fawx skill build <project>".to_string(),
        "  Prebuilt artifact: fawx skill install <path>".to_string(),
        "  Built-in repo skills: skills/build.sh --install".to_string(),
    ];
    let sections = skill_sections(installed, available);
    lines.push(String::new());
    if sections.is_empty() {
        lines.push("No local built or installed skills found.".to_string());
    } else {
        lines.extend(sections);
    }
    lines.push(String::new());
    lines.push(
        "/skills does not verify loaded-on-server state. The Swift Skills UI and /v1/skills show only skills the running server has loaded.".to_string(),
    );
    lines.join("\n")
}

fn skill_sections(installed: &[LocalSkillSummary], available: &[LocalSkillSummary]) -> Vec<String> {
    let mut sections = Vec::new();
    if !installed.is_empty() {
        sections.push(format_skill_section(
            LocalSkillState::InstalledLocally,
            installed,
        ));
    }
    if !available.is_empty() {
        sections.push(format_skill_section(
            LocalSkillState::BuiltLocally,
            available,
        ));
    }
    sections
}

fn format_skill_section(state: LocalSkillState, skills: &[LocalSkillSummary]) -> String {
    let mut lines = vec![state.title().to_string()];
    lines.extend(
        skills
            .iter()
            .map(|skill| format!("{} {} {}", state.marker(), skill.icon, skill.name)),
    );
    lines.join("\n")
}

fn render_welcome_screen(
    width: usize,
    mascot_art: &str,
    skills: &[LocalSkillSummary],
) -> Vec<Line<'static>> {
    match WelcomeLayout::for_width(width) {
        WelcomeLayout::Wide => render_wide_welcome(width, mascot_art, skills),
        WelcomeLayout::Medium => render_medium_welcome(width, mascot_art, skills),
        WelcomeLayout::Narrow => render_narrow_welcome(width, skills),
    }
}

fn render_wide_welcome(
    width: usize,
    mascot_art: &str,
    skills: &[LocalSkillSummary],
) -> Vec<Line<'static>> {
    let mascot_width = width
        .saturating_sub(WELCOME_LEFT_WIDTH + WELCOME_COMMAND_WIDTH + (WELCOME_COLUMN_GAP * 2))
        .max(20);
    let left = welcome_text_column();
    let middle = welcome_commands_and_skills(WELCOME_COMMAND_WIDTH, skills);
    let right = welcome_mascot_column(mascot_art);
    let mut lines = merge_columns(vec![
        (left, WELCOME_LEFT_WIDTH),
        (middle, WELCOME_COMMAND_WIDTH),
        (right, mascot_width),
    ]);
    lines.push(blank_line());
    lines.push(styled_line(
        INPUT_PLACEHOLDER,
        Style::default().fg(Color::DarkGray),
    ));
    lines
}

fn render_medium_welcome(
    width: usize,
    mascot_art: &str,
    skills: &[LocalSkillSummary],
) -> Vec<Line<'static>> {
    let mascot_width = width
        .saturating_sub(WELCOME_LEFT_WIDTH + WELCOME_COMMAND_WIDTH + (WELCOME_COLUMN_GAP * 2))
        .max(20);
    let left = welcome_text_column();
    let middle = welcome_commands_and_skills(WELCOME_COMMAND_WIDTH, skills);
    let right = welcome_mascot_column(mascot_art);
    let mut lines = merge_columns(vec![
        (left, WELCOME_LEFT_WIDTH),
        (middle, WELCOME_COMMAND_WIDTH),
        (right, mascot_width),
    ]);
    lines.push(blank_line());
    lines.push(styled_line(
        INPUT_PLACEHOLDER,
        Style::default().fg(Color::DarkGray),
    ));
    lines
}

fn render_narrow_welcome(width: usize, skills: &[LocalSkillSummary]) -> Vec<Line<'static>> {
    let mut lines = welcome_command_section(width);
    lines.push(blank_line());
    lines.extend(welcome_skill_section(width, skills));
    lines.push(blank_line());
    lines.push(styled_line(
        VERSION_LABEL,
        Style::default().fg(Color::DarkGray),
    ));
    lines.push(styled_line(
        INPUT_PLACEHOLDER,
        Style::default().fg(Color::DarkGray),
    ));
    lines
}

fn welcome_text_column() -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = ASCII_LOGO_ART
        .lines()
        .map(|line| {
            Line::from(Span::styled(
                line.to_string(),
                Style::default()
                    .fg(fawx_amber())
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    lines.push(blank_line());
    lines.push(styled_line(
        VERSION_LABEL,
        Style::default().fg(Color::DarkGray),
    ));
    lines
}

fn welcome_mascot_column(mascot_art: &str) -> Vec<Line<'static>> {
    mascot_art
        .lines()
        .map(|line| {
            Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(fawx_amber()),
            ))
        })
        .collect()
}

fn welcome_commands_and_skills(width: usize, skills: &[LocalSkillSummary]) -> Vec<Line<'static>> {
    let mut lines = welcome_command_section(width);
    lines.push(blank_line());
    lines.extend(welcome_skill_section(width, skills));
    lines
}

fn welcome_command_section(width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![section_header("Commands")];
    for (command, description) in WELCOME_COMMANDS {
        lines.push(command_line(command, description, width));
    }
    lines
}

fn welcome_skill_section(width: usize, skills: &[LocalSkillSummary]) -> Vec<Line<'static>> {
    let mut lines = vec![section_header("Skills")];
    lines.extend(render_skill_items(width, skills));
    lines
}

fn render_skill_items(width: usize, skills: &[LocalSkillSummary]) -> Vec<Line<'static>> {
    if skills.is_empty() {
        return wrap_plain_text(EMPTY_SKILLS_MESSAGE, width.max(1))
            .into_iter()
            .map(|line| restyle_line(line, Style::default().fg(Color::Gray)))
            .collect();
    }

    let mut lines = skills
        .iter()
        .take(MAX_VISIBLE_SKILLS)
        .map(|skill| skill_line(skill, width))
        .collect::<Vec<_>>();
    let overflow = skills.len().saturating_sub(MAX_VISIBLE_SKILLS);
    if overflow > 0 {
        lines.push(styled_line(
            format!("+{overflow} more"),
            Style::default().fg(Color::Gray),
        ));
    }
    lines
}

fn section_header(title: &str) -> Line<'static> {
    styled_line(
        title,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )
}

fn command_line(command: &str, description: &str, width: usize) -> Line<'static> {
    let name_width = width.min(9);
    let command_text = truncate_text(command, name_width);
    let padding = " ".repeat(name_width.saturating_sub(command_text.len()) + 2);
    let description_width = width.saturating_sub(name_width + 2);
    let description_text = truncate_text(description, description_width);
    Line::from(vec![
        Span::styled(
            command_text,
            Style::default()
                .fg(fawx_amber())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(padding),
        Span::styled(description_text, Style::default().fg(Color::Gray)),
    ])
}

fn skill_line(skill: &LocalSkillSummary, width: usize) -> Line<'static> {
    let text = truncate_text(&format!("{}  {}", skill.icon, skill.name), width.max(1));
    styled_line(text, Style::default().fg(Color::Gray))
}

fn truncate_text(text: &str, width: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut truncated = text
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn merge_columns(columns: Vec<(Vec<Line<'static>>, usize)>) -> Vec<Line<'static>> {
    let row_count = columns
        .iter()
        .map(|(lines, _)| lines.len())
        .max()
        .unwrap_or(0);
    let mut merged = Vec::with_capacity(row_count);
    for row in 0..row_count {
        merged.push(merge_column_row(&columns, row));
    }
    merged
}

fn merge_column_row(columns: &[(Vec<Line<'static>>, usize)], row: usize) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (lines, width)) in columns.iter().enumerate() {
        let line = lines.get(row).cloned().unwrap_or_default();
        push_padded_line(&mut spans, line, *width);
        if index + 1 < columns.len() {
            spans.push(Span::raw(" ".repeat(WELCOME_COLUMN_GAP)));
        }
    }
    Line::from(spans)
}

fn push_padded_line(spans: &mut Vec<Span<'static>>, line: Line<'static>, width: usize) {
    let line_width = line.width();
    spans.extend(line.spans);
    let padding = width.saturating_sub(line_width);
    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }
}

fn styled_line(text: impl Into<String>, style: Style) -> Line<'static> {
    Line::from(vec![Span::styled(text.into(), style)])
}

fn restyle_line(line: Line<'static>, style: Style) -> Line<'static> {
    let spans = line
        .spans
        .into_iter()
        .map(|span| Span::styled(span.content.into_owned(), style))
        .collect::<Vec<_>>();
    Line::from(spans)
}

fn blank_line() -> Line<'static> {
    Line::default()
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

fn normalize_tool_arguments(arguments: Value) -> Option<Value> {
    match arguments {
        Value::Null => None,
        Value::Object(map) if map.is_empty() => None,
        other => Some(other),
    }
}

fn should_update_tool_use(entry: &Entry, name: &str) -> bool {
    matches!(entry.role, EntryRole::ToolUse)
        && entry.tool_name.as_deref() == Some(name)
        && entry.tool_arguments.is_none()
}

fn render_tool_use_entry(entry: &Entry, width: usize) -> Vec<Line<'static>> {
    let content_width = tool_content_width(width);
    let lines = wrap_tool_text_lines(&tool_use_summary_lines(entry, content_width), content_width);
    prefix_tool_lines(lines, EntryRole::ToolUse)
}

fn render_tool_result_entry(
    entry: &Entry,
    width: usize,
    panel_visible: bool,
) -> Vec<Line<'static>> {
    let content_width = tool_content_width(width);
    let text = sanitize_terminal_text(&entry.text);
    let plan = tool_result_render_plan(&text, content_width);
    let mut lines = wrap_tool_text_lines(
        &tool_result_summary_lines(entry, &text, plan.summarize),
        content_width,
    );
    let budget = TOOL_RESULT_MAX_LINES.saturating_sub(lines.len());
    lines.extend(tool_result_preview_lines(
        entry,
        content_width,
        budget,
        panel_visible,
        plan,
    ));
    prefix_tool_lines(lines, entry.role)
}

fn tool_content_width(width: usize) -> usize {
    width.saturating_sub(TOOL_PREFIX_DISPLAY_WIDTH).max(1)
}

fn prefix_tool_lines(lines: Vec<Line<'static>>, role: EntryRole) -> Vec<Line<'static>> {
    let (initial, style) = tool_prefix(role);
    prefix_lines(
        lines,
        Span::styled(initial, style),
        Span::raw(tool_continuation_prefix()),
    )
}

fn tool_continuation_prefix() -> String {
    " ".repeat(TOOL_PREFIX_DISPLAY_WIDTH)
}

fn tool_prefix(role: EntryRole) -> (&'static str, Style) {
    match role {
        EntryRole::ToolUse => (TOOL_USE_PREFIX, Style::default().fg(Color::Magenta)),
        EntryRole::ToolResult => (TOOL_RESULT_PREFIX, Style::default().fg(Color::Green)),
        EntryRole::ToolError => (
            TOOL_ERROR_PREFIX,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        _ => (TOOL_RESULT_PREFIX, Style::default()),
    }
}

fn wrap_tool_text_lines(lines: &[String], width: usize) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    for line in lines {
        rendered.extend(wrap_tool_output_text(line, width));
    }
    if rendered.is_empty() {
        rendered.push(Line::default());
    }
    rendered
}

fn tool_use_summary_lines(entry: &Entry, width: usize) -> Vec<String> {
    let mut lines = vec![format!("▶ {}", tool_label(entry.tool_name.as_deref()))];
    if let Some(arguments) = &entry.tool_arguments {
        lines.extend(tool_argument_summary_lines(
            entry.tool_name.as_deref(),
            arguments,
            width,
        ));
    }
    truncate_tool_use_summary(lines)
}

fn truncate_tool_use_summary(lines: Vec<String>) -> Vec<String> {
    if lines.len() <= TOOL_USE_MAX_LINES {
        return lines;
    }
    let mut limited = lines[..TOOL_USE_MAX_LINES - 1].to_vec();
    limited.push(format!(
        "  … {} more fields",
        lines.len() - TOOL_USE_MAX_LINES + 1
    ));
    limited
}

fn tool_argument_summary_lines(
    tool_name: Option<&str>,
    arguments: &Value,
    width: usize,
) -> Vec<String> {
    match arguments {
        Value::Object(map) => tool_object_summary_lines(tool_name, map, width),
        other => vec![format!(
            "  args: {}",
            summarize_tool_value(other, width.saturating_sub(10))
        )],
    }
}

fn tool_object_summary_lines(
    tool_name: Option<&str>,
    map: &serde_json::Map<String, Value>,
    width: usize,
) -> Vec<String> {
    let fields = prioritized_tool_fields(tool_name, map);
    let mut lines = fields
        .into_iter()
        .take(TOOL_USE_FIELD_LIMIT)
        .map(|(key, value)| {
            let available = width.saturating_sub(key.len() + 7);
            format!("  {key}: {}", summarize_tool_value(value, available))
        })
        .collect::<Vec<_>>();
    let remaining = map.len().saturating_sub(TOOL_USE_FIELD_LIMIT);
    if remaining > 0 {
        lines.push(format!("  … {remaining} more fields"));
    }
    lines
}

fn prioritized_tool_fields<'a>(
    tool_name: Option<&str>,
    map: &'a serde_json::Map<String, Value>,
) -> Vec<(&'a String, &'a Value)> {
    let mut fields = map.iter().collect::<Vec<_>>();
    if tool_name == Some("run_experiment") {
        fields.sort_by_key(|(key, _)| (experiment_tool_argument_priority(key), key.as_str()));
    }
    fields
}

fn experiment_tool_argument_priority(key: &str) -> usize {
    match key {
        "signal" => 0,
        "hypothesis" => 1,
        "scope" => 2,
        "nodes" => 3,
        "mode" => 4,
        "timeout" => 5,
        _ => 6,
    }
}

fn summarize_tool_value(value: &Value, limit: usize) -> String {
    match value {
        Value::String(text) => {
            format!("\"{}\"", preview_text(&sanitize_terminal_text(text), limit))
        }
        Value::Array(items) => format!("[{} items]", items.len()),
        Value::Object(map) => format!("{{{} keys}}", map.len()),
        other => other.to_string(),
    }
}

fn preview_text(text: &str, limit: usize) -> String {
    let preview_limit = limit.clamp(4, TOOL_VALUE_PREVIEW_CHARS);
    truncate_text(
        &text.split_whitespace().collect::<Vec<_>>().join(" "),
        preview_limit,
    )
}

fn tool_result_summary_lines(entry: &Entry, text: &str, summarize_experiment: bool) -> Vec<String> {
    let status = if matches!(entry.role, EntryRole::ToolError) {
        "failure"
    } else {
        "success"
    };
    let label = tool_label(entry.tool_name.as_deref());
    let mut lines = vec![format!(
        "{} {label} ({status})",
        tool_status_icon(entry.role)
    )];
    if summarize_experiment {
        lines.extend(experiment_result_summary_lines(text));
    }
    lines
}

fn tool_label(name: Option<&str>) -> String {
    let sanitized = sanitize_terminal_text(name.unwrap_or("tool"));
    if sanitized.trim().is_empty() {
        "tool".to_string()
    } else {
        sanitized
    }
}

fn tool_status_icon(role: EntryRole) -> &'static str {
    if matches!(role, EntryRole::ToolError) {
        "✗"
    } else {
        "✓"
    }
}

fn experiment_result_summary_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(decision) = find_prefixed_line(text, "Decision:") {
        lines.push(collapse_whitespace(decision));
    }
    if let Some(score_line) = find_experiment_score_line(text) {
        lines.push(collapse_whitespace(score_line));
    }
    if let Some(chain_entry) = find_line_containing(text, "Chain entry #") {
        lines.push(collapse_whitespace(chain_entry));
    }
    lines
}

fn find_prefixed_line<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.lines()
        .find(|line| line.trim_start().starts_with(prefix))
}

fn find_experiment_score_line(text: &str) -> Option<&str> {
    text.lines()
        .find(|line| line.contains("score:") && line.contains("WINNER"))
        .or_else(|| text.lines().find(|line| line.contains("score:")))
}

fn find_line_containing<'a>(text: &'a str, needle: &str) -> Option<&'a str> {
    text.lines().find(|line| line.contains(needle))
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct ToolResultRenderPlan {
    wrapped_output: Vec<Line<'static>>,
    summarize: bool,
}

fn tool_result_preview_lines(
    entry: &Entry,
    width: usize,
    limit: usize,
    panel_visible: bool,
    plan: ToolResultRenderPlan,
) -> Vec<Line<'static>> {
    if limit == 0 {
        return Vec::new();
    }
    let total_lines = plan.wrapped_output.len();
    if plan.summarize {
        return experiment_notice_lines(entry, width, panel_visible, limit, total_lines);
    }
    truncate_wrapped_lines(
        plan.wrapped_output,
        width,
        limit,
        tool_result_notice(entry, panel_visible, total_lines),
    )
}

fn has_experiment_summary(text: &str) -> bool {
    !experiment_result_summary_lines(text).is_empty()
}

fn tool_result_render_plan(text: &str, width: usize) -> ToolResultRenderPlan {
    let wrapped_output = wrap_tool_output_text(text, width);
    let summarize = has_experiment_summary(text) && wrapped_output.len() > TOOL_RESULT_MAX_LINES;
    ToolResultRenderPlan {
        wrapped_output,
        summarize,
    }
}

fn experiment_notice_lines(
    entry: &Entry,
    width: usize,
    panel_visible: bool,
    limit: usize,
    total_lines: usize,
) -> Vec<Line<'static>> {
    let notice = wrap_plain_text(
        &tool_result_notice(entry, panel_visible, total_lines),
        width,
    );
    notice.into_iter().take(limit).collect()
}

fn truncate_wrapped_lines(
    wrapped: Vec<Line<'static>>,
    width: usize,
    limit: usize,
    notice: String,
) -> Vec<Line<'static>> {
    if wrapped.len() <= limit {
        return wrapped;
    }
    let notice_lines = wrap_tool_output_text(&notice, width);
    let keep = limit.saturating_sub(notice_lines.len());
    let mut out = wrapped.into_iter().take(keep).collect::<Vec<_>>();
    out.extend(
        notice_lines
            .into_iter()
            .take(limit.saturating_sub(out.len())),
    );
    out
}

fn tool_result_notice(entry: &Entry, panel_visible: bool, total_lines: usize) -> String {
    if panel_visible && entry.tool_name.as_deref() == Some("run_experiment") {
        return format!("[full output: {total_lines} lines — see Experiment panel →]");
    }
    format!("[full output: {total_lines} lines — truncated in transcript]")
}

fn wrap_tool_output_text(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let options = textwrap::Options::new(width).break_words(true);
    for raw_line in text.lines() {
        out.extend(
            textwrap::wrap(raw_line, &options)
                .into_iter()
                .map(|line| Line::from(line.into_owned())),
        );
    }
    if text.ends_with('\n') {
        out.push(Line::default());
    }
    if out.is_empty() {
        out.push(Line::default());
    }
    out
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
    use ratatui::backend::TestBackend as RatatuiTestBackend;
    use ratatui::buffer::Buffer;
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TEST_HELP_TEXT: &str = concat!(
        "Commands\n",
        "  /model         List models and switch active model\n",
        "  /model <name>  Switch to a specific model\n",
        "  /auth          Show credential status + auth help\n",
        "  /auth <provider> set-token <TOKEN>\n",
        "                 Save API key or PAT for a provider\n",
        "  /keys          Manage WASM signing keys\n",
        "  /keys generate [--force]\n",
        "  /keys list     List trusted public keys\n",
        "  /keys trust <path>\n",
        "  /keys revoke <fingerprint>\n",
        "  /sign <skill>  Sign one installed WASM skill\n",
        "  /sign --all    Sign all installed WASM skills\n",
        "  /skills         Inspect local build/install state\n",
        "                 Local dev: fawx skill build <project>\n",
        "                 Prebuilt:  fawx skill install <path>\n",
        "                 Repo skills: skills/build.sh --install\n",
        "  /install <name> Install a skill from the marketplace\n",
        "  /search [query] Search the skill marketplace\n",
        "  /status        Show model, tokens, budget summary\n",
        "  /budget        Show detailed budget usage\n",
        "  /loop          Show loop iteration details\n",
        "  /signals       Show condensed signal summary for last turn\n",
        "  /debug         Show full signal dump for last turn\n",
        "  /analyze       Analyze persisted signals across sessions\n",
        "  /improve       Run self-improvement cycle\n",
        "  /proposals     List pending self-modification proposals\n",
        "  /proposals <id> Show a proposal diff preview\n",
        "  /approve       Apply a pending proposal (/approve <id> [--force])\n",
        "  /reject        Archive a pending proposal (/reject <id>)\n",
        "  /synthesis     Set or reset synthesis instruction\n",
        "  /thinking      Show or set thinking budget (high|low|adaptive|off)\n",
        "  /clear         Clear the screen and active conversation\n",
        "  /new           Start a new conversation\n",
        "  /history       List saved conversations\n",
        "  /config        Show loaded config values\n",
        "  /config init   Create ~/.fawx/config.toml template\n",
        "  /config reload Reload config.toml without restarting\n",
        "  /help          Show this help\n",
        "  /quit          Exit"
    );

    fn test_app() -> App {
        let backend: AppBackend = Arc::new(HttpBackend::from_env());
        let experiment_panel: SharedPanel = Arc::new(Mutex::new(ExperimentPanel::new()));
        let (tx, rx) = unbounded_channel();
        let mut app = App::new(
            backend,
            crate::DEFAULT_ENGINE_URL.to_string(),
            experiment_panel,
            tx,
            rx,
        );
        app.entries.clear();
        app
    }

    fn live_like_test_app() -> App {
        let backend: Arc<dyn EngineBackend> = Arc::new(TestBackend::default());
        let (tx, rx) = unbounded_channel();
        App::new(
            backend,
            crate::DEFAULT_ENGINE_URL.to_string(),
            Arc::new(Mutex::new(ExperimentPanel::new())),
            tx,
            rx,
        )
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

    fn rendered_text(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(line_text).collect()
    }

    fn visible_transcript_text(app: &mut App, area: Rect) -> Vec<String> {
        let lines = app.transcript_lines_for_area(area);
        let scroll = app.sync_transcript_scroll(area, transcript_content_line_count(&lines));
        lines
            .iter()
            .skip(scroll as usize)
            .take(transcript_inner_area(area).height as usize)
            .map(line_text)
            .collect()
    }

    fn draw_app(app: &mut App, terminal: &mut Terminal<RatatuiTestBackend>) {
        terminal
            .draw(|frame| app.draw(frame))
            .expect("draw app to test backend");
    }

    fn buffer_area_text(buffer: &Buffer, area: Rect) -> String {
        let mut out = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn buffer_area_lines(buffer: &Buffer, area: Rect) -> Vec<String> {
        (area.top()..area.bottom())
            .map(|y| {
                (area.left()..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn assert_lines_fit(lines: &[Line<'_>], width: usize) {
        for line in lines {
            assert!(
                line.width() <= width,
                "line exceeded width {width}: {}",
                line_text(line)
            );
        }
    }

    fn assert_tool_prefix_alignment(lines: &[Line<'_>]) {
        let expected = lines
            .first()
            .and_then(|line| line.spans.first())
            .map(|span| span.width())
            .expect("tool line prefix");
        for line in lines.iter().skip(1) {
            let actual = line
                .spans
                .first()
                .map(|span| span.width())
                .expect("tool continuation prefix");
            assert_eq!(
                actual,
                expected,
                "misaligned tool prefix: {}",
                line_text(line)
            );
        }
        assert_eq!(expected, TOOL_PREFIX_DISPLAY_WIDTH);
    }

    fn skill(name: &str, icon: &str) -> LocalSkillSummary {
        LocalSkillSummary {
            icon: icon.to_string(),
            name: name.to_string(),
        }
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
            app.entries.push(Entry::plain(
                EntryRole::Assistant,
                format!(
                    "line {index}: this is a deliberately long transcript entry to force wrapping in a narrow viewport"
                ),
            ));
        }
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("fawx-tui-{name}-{unique}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
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
        let app_backend: AppBackend = backend.clone();
        let experiment_panel: SharedPanel = Arc::new(Mutex::new(ExperimentPanel::new()));
        let (tx, rx) = unbounded_channel();
        let mut app = App::new(
            app_backend,
            "embedded engine".to_string(),
            experiment_panel,
            tx,
            rx,
        );
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
        let initial_lines = app.transcript_lines_for_area(area);

        let initial_bottom = app.transcript_max_scroll(area);
        let initial_scroll =
            app.sync_transcript_scroll(area, transcript_content_line_count(&initial_lines));
        assert_eq!(initial_scroll, initial_bottom);
        assert_eq!(app.scroll, initial_bottom);
        assert!(app.follow_output);

        app.entries.push(Entry::tool_result(
            ToolOutcome::Success,
            Some("run_experiment".to_string()),
            "tool output\nwith enough detail to extend the transcript and move the viewport down"
                .to_string(),
        ));

        let grown_bottom = app.transcript_max_scroll(area);
        let grown_lines = app.transcript_lines_for_area(area);
        let grown_scroll =
            app.sync_transcript_scroll(area, transcript_content_line_count(&grown_lines));
        assert!(grown_bottom > initial_bottom);
        assert_eq!(grown_scroll, grown_bottom);
        assert_eq!(app.scroll, grown_bottom);
        assert!(app.follow_output);
    }

    #[test]
    fn follow_output_keeps_latest_transcript_line_visible_with_experiment_panel() {
        let mut app = test_app();
        fill_transcript(&mut app, 6);
        app.entries.push(Entry::tool_result(
            ToolOutcome::Success,
            Some("run_experiment".to_string()),
            "summary\nLATEST MARKER".to_string(),
        ));
        app.experiment_panel
            .lock()
            .expect("experiment panel")
            .push_line("running".to_string());

        let (transcript_area, experiment_area) = transcript_layout(Rect::new(0, 0, 80, 12), true);
        assert!(experiment_area.is_some());

        let visible = visible_transcript_text(&mut app, transcript_area);
        let last_visible = visible
            .iter()
            .rev()
            .find(|line| !line.trim().is_empty())
            .expect("visible transcript content");

        assert!(last_visible.contains("LATEST MARKER"));
    }

    #[test]
    fn follow_output_does_not_leave_trailing_blank_row_after_latest_entry() {
        let area = Rect::new(0, 0, 40, 8);
        let mut app = test_app();
        fill_transcript(&mut app, 4);
        app.entries.push(Entry::tool_result(
            ToolOutcome::Success,
            Some("exec".to_string()),
            "LATEST MARKER".to_string(),
        ));

        let visible = visible_transcript_text(&mut app, area);
        let last_row = visible.last().expect("visible transcript row");

        assert!(last_row.contains("LATEST MARKER"));
    }

    #[test]
    fn rendered_buffer_keeps_latest_help_lines_visible() {
        let mut app = live_like_test_app();
        let help = TEST_HELP_TEXT.to_string();
        app.push_system("Connected to Fawx on http://127.0.0.1:8400 using model claude-opus-4-6.");
        app.entries
            .push(Entry::plain(EntryRole::Assistant, help.clone()));
        app.entries
            .push(Entry::plain(EntryRole::User, "/help".to_string()));
        app.entries
            .push(Entry::plain(EntryRole::Assistant, help.clone()));
        let area = Rect::new(0, 0, 136, 58);
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(4),
            ])
            .split(area)[1];
        let mut terminal =
            Terminal::new(RatatuiTestBackend::new(area.width, area.height)).expect("terminal");

        draw_app(&mut app, &mut terminal);

        let inner = transcript_inner_area(body);
        let actual = buffer_area_lines(terminal.backend().buffer(), inner)
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect::<Vec<_>>();
        let expected = visible_transcript_text(&mut app, body);

        assert_eq!(actual, expected);
        assert!(
            actual
                .iter()
                .any(|line| line.contains("/quit") && line.contains("Exit")),
            "visible rows:\n{}",
            actual.join("\n")
        );
    }

    #[test]
    fn sanitize_terminal_text_strips_ansi_and_control_sequences() {
        let input = "\x1b[31mred\x1b[0m\rprogress\r\nnext\x07\tok";
        let sanitized = sanitize_terminal_text(input);

        assert_eq!(sanitized, "red\nprogress\nnext    ok");
    }

    #[test]
    fn transcript_rendering_strips_terminal_control_sequences() {
        let mut app = live_like_test_app();
        app.entries.clear();
        app.entries.push(Entry::tool_result(
            ToolOutcome::Error,
            Some("run_experiment".to_string()),
            "\x1b[31mtool failed\x1b[0m\rcollecting baseline\r\nnext line".to_string(),
        ));
        app.entries.push(Entry::plain(
            EntryRole::Assistant,
            "Summary of `\x1b[36mrun_command\x1b[0m`:\rpath escapes working directory".to_string(),
        ));
        let area = Rect::new(0, 0, 100, 20);
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(4),
            ])
            .split(area)[1];
        let inner = transcript_inner_area(body);
        let mut terminal =
            Terminal::new(RatatuiTestBackend::new(area.width, area.height)).expect("terminal");

        draw_app(&mut app, &mut terminal);

        let actual = buffer_area_lines(terminal.backend().buffer(), inner).join("\n");
        assert!(!actual.contains('\u{1b}'));
        assert!(!actual.contains('\r'));
        assert!(actual.contains("tool failed"));
        assert!(actual.contains("collecting baseline"));
        assert!(actual.contains("Summary of run_command:"));
        assert!(actual.contains("path escapes working directory"));
    }

    #[test]
    fn experiment_panel_rendering_strips_terminal_control_sequences() {
        let lines = render_panel_lines(
            &[String::from(
                "\x1b[32mrun_experiment\x1b[0m\rcollecting\tbaseline\r\nnext step",
            )],
            32,
        );
        let text = rendered_text(&lines).join("\n");

        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\r'));
        assert!(text.contains("run_experiment"));
        assert!(text.contains("collecting    baseline"));
        assert!(text.contains("next step"));
    }

    #[test]
    fn trailing_blank_rows_do_not_push_latest_help_line_out_of_view() {
        let mut app = live_like_test_app();
        let help = format!("{TEST_HELP_TEXT}\n\n\n");
        app.push_system("Connected to Fawx on http://127.0.0.1:8400 using model claude-opus-4-6.");
        app.entries
            .push(Entry::plain(EntryRole::Assistant, help.clone()));
        app.entries
            .push(Entry::plain(EntryRole::User, "/help".to_string()));
        app.entries.push(Entry::plain(EntryRole::Assistant, help));
        let area = Rect::new(0, 0, 136, 58);
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(4),
            ])
            .split(area)[1];
        let inner = transcript_inner_area(body);
        let mut terminal =
            Terminal::new(RatatuiTestBackend::new(area.width, area.height)).expect("terminal");

        draw_app(&mut app, &mut terminal);

        let actual = buffer_area_lines(terminal.backend().buffer(), inner);
        let last_visible = actual
            .iter()
            .rev()
            .find(|line| !line.trim().is_empty())
            .expect("last visible transcript line");

        assert!(last_visible.contains("/quit"));
        assert!(last_visible.contains("Exit"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn first_help_turn_dismisses_welcome_screen() {
        let mut app = live_like_test_app();
        app.push_system("Connected to Fawx on embedded engine using model claude-opus-4-6.");
        app.input = "/help".to_string();

        app.submit_input();
        app.handle_backend_event(BackendEvent::TextDelta(TEST_HELP_TEXT.to_string()));
        app.handle_backend_event(BackendEvent::Done {
            model: Some("claude-opus-4-6".to_string()),
            iterations: Some(0),
            input_tokens: None,
            output_tokens: None,
        });

        let area = Rect::new(0, 0, 136, 58);
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(4),
            ])
            .split(area)[1];
        let visible = visible_transcript_text(&mut app, body);

        assert!(!app
            .entries
            .iter()
            .any(|entry| matches!(entry.role, EntryRole::Welcome)));
        assert!(visible.iter().any(|line| line.contains("you  › /help")));
        assert!(visible
            .iter()
            .any(|line| line.contains("/quit") && line.contains("Exit")));
        assert!(!visible
            .iter()
            .any(|line| line.contains("Ask Fawx anything")));
    }

    #[test]
    fn tool_use_rendering_summarizes_arguments_without_raw_json() {
        let entry = Entry::tool_use(
            "run_experiment".to_string(),
            json!({
                "signal": "Low success-to-decision ratio in the experiment runner transcript view",
                "hypothesis": "Subagent tool output should be summarized instead of rendered as a raw JSON blob",
                "nodes": 2,
                "mode": "subagent",
                "scope": "tui/src/app.rs"
            }),
        );

        for width in [42, 80, 120, 200] {
            let lines = render_tool_use_entry(&entry, width);
            let text = rendered_text(&lines).join("\n");

            assert!(text.contains("▶ run_experiment"));
            assert!(text.contains("signal:"));
            assert!(text.contains("hypothesis:"));
            assert!(text.contains("… 2 more fields"));
            assert!(!text.contains('{'));
            assert!(lines.len() <= TOOL_USE_MAX_LINES);
            assert_lines_fit(&lines, width);
            assert_tool_prefix_alignment(&lines);
        }
    }

    #[test]
    fn tool_use_rendering_sanitizes_names_and_keeps_prefixes_aligned() {
        let entry = Entry::tool_use(
            "\x1b[31mrun_experiment\x1b[0m".to_string(),
            json!({
                "signal": "alignment regression coverage for wrapped tool summaries",
            }),
        );

        let lines = render_tool_use_entry(&entry, 28);
        let text = rendered_text(&lines).join("\n");

        assert!(text.contains("▶ run_experiment"));
        assert!(!text.contains('\u{1b}'));
        assert_lines_fit(&lines, 28);
        assert_tool_prefix_alignment(&lines);
    }

    #[test]
    fn tool_results_reuse_previous_tool_name_and_render_summary() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::ToolUse {
            name: "run_experiment".to_string(),
            arguments: Value::Null,
        });
        app.handle_backend_event(BackendEvent::ToolUse {
            name: "run_experiment".to_string(),
            arguments: json!({
                "signal": "rendering overflow",
                "hypothesis": "tool call summaries should fit narrow transcript widths"
            }),
        });
        app.handle_backend_event(BackendEvent::ToolResult {
            name: None,
            success: true,
            content: "═══ Experiment Complete ═══\nDecision:      ✅ ACCEPT\nnode-0 Conservative score: 8.73 ← WINNER\nChain entry #7 recorded".to_string(),
        });

        assert_eq!(app.entries.len(), 2);
        assert_eq!(app.entries[0].tool_name.as_deref(), Some("run_experiment"));
        assert!(app.entries[0].tool_arguments.is_some());
        assert_eq!(app.entries[1].tool_name.as_deref(), Some("run_experiment"));
    }

    #[test]
    fn short_experiment_results_render_full_output_without_notice() {
        let entry = Entry::tool_result(
            ToolOutcome::Success,
            Some("run_experiment".to_string()),
            "═══ Experiment Complete ═══\nDecision:      ✅ ACCEPT\nnode-0 Conservative score: 8.73 ← WINNER\nChain entry #7 recorded".to_string(),
        );

        let lines = render_tool_result_entry(&entry, 52, true);
        let text = rendered_text(&lines).join("\n");

        assert!(text.contains("Decision:"));
        assert!(text.contains("Chain entry #7 recorded"));
        assert!(!text.contains("[full output:"));
        assert_lines_fit(&lines, 52);
    }

    #[test]
    fn tool_result_rendering_truncates_verbose_output_to_transcript_budget() {
        let content = (0..30)
            .map(|index| format!("line {index}: {}", "verbose output ".repeat(4)))
            .collect::<Vec<_>>()
            .join("\n");
        let entry = Entry::tool_result(
            ToolOutcome::Success,
            Some("run_experiment".to_string()),
            content,
        );

        for width in [40, 80, 120, 200] {
            let lines = render_tool_result_entry(&entry, width, true);
            let text = rendered_text(&lines).join("\n");

            assert!(text.contains("✓ run_experiment (success)"));
            assert!(text.contains("[full output:"));
            assert!(lines.len() <= TOOL_RESULT_MAX_LINES);
            assert_lines_fit(&lines, width);
        }
    }

    #[test]
    fn non_experiment_tool_result_truncation_has_regression_coverage() {
        let content = (0..30)
            .map(|index| format!("stdout line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let entry = Entry::tool_result(ToolOutcome::Success, Some("exec".to_string()), content);

        let lines = render_tool_result_entry(&entry, 40, false);
        let text = rendered_text(&lines).join("\n");

        assert!(text.contains("✓ exec (success)"));
        assert!(text.contains("stdout line 0"));
        assert!(text.contains("truncated in transcript"));
        assert!(!text.contains("Experiment panel"));
        assert!(lines.len() <= TOOL_RESULT_MAX_LINES);
        assert_lines_fit(&lines, 40);
    }

    #[test]
    fn tool_error_rendering_shows_failure_header_and_stays_in_bounds() {
        let content = (0..12)
            .map(|index| format!("stderr line {index}: permission denied while opening file"))
            .collect::<Vec<_>>()
            .join("\n");
        let entry = Entry::tool_result(ToolOutcome::Error, Some("read".to_string()), content);

        let lines = render_tool_result_entry(&entry, 36, false);
        let text = rendered_text(&lines).join("\n");

        assert!(text.contains("✗ read (failure)"));
        assert!(!text.contains("(success)"));
        assert_lines_fit(&lines, 36);
    }

    #[test]
    fn tool_result_rendering_wraps_long_url_like_output_to_width() {
        let entry = Entry::tool_result(
            ToolOutcome::Success,
            Some("read".to_string()),
            "https://example.com/a/really/long/tool/output/path/that/used/to/overflow/the/transcript/view".to_string(),
        );

        let lines = render_tool_result_entry(&entry, 36, false);

        assert_lines_fit(&lines, 36);
    }

    #[test]
    fn panel_render_clears_stale_tool_output_when_layout_narrows() {
        let mut app = test_app();
        app.entries.push(Entry::tool_result(
            ToolOutcome::Success,
            Some("exec".to_string()),
            "Q".repeat(160),
        ));
        let area = Rect::new(0, 0, 80, 18);
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(4),
            ])
            .split(area)[1];
        let (_, panel_area) = transcript_layout(body, true);
        let panel_area = panel_area.expect("experiment panel area");
        let mut terminal =
            Terminal::new(RatatuiTestBackend::new(area.width, area.height)).expect("terminal");

        draw_app(&mut app, &mut terminal);
        let initial = buffer_area_text(terminal.backend().buffer(), panel_area);
        assert!(initial.contains('Q'));

        app.experiment_panel
            .lock()
            .expect("experiment panel")
            .push_line("running".to_string());
        draw_app(&mut app, &mut terminal);

        let rerendered = buffer_area_text(terminal.backend().buffer(), panel_area);
        assert!(!rerendered.contains('Q'));
    }

    #[test]
    fn scrolling_up_from_follow_mode_starts_from_current_bottom() {
        let area = Rect::new(0, 0, 40, 8);
        let mut app = test_app();
        fill_transcript(&mut app, 8);
        let lines = app.transcript_lines_for_area(area);

        let bottom = app.sync_transcript_scroll(area, transcript_content_line_count(&lines));
        assert!(bottom > 0);

        app.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert!(!app.follow_output);
        assert_eq!(app.scroll, bottom.saturating_sub(1));
        let lines = app.transcript_lines_for_area(area);
        assert_eq!(
            app.sync_transcript_scroll(area, transcript_content_line_count(&lines)),
            bottom.saturating_sub(1)
        );
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
    fn input_pane_clears_stale_text_after_submit() {
        let mut app = test_app();
        app.input = "persistent memory assistant experiments or approaches might be worth exploring after reading the current memory architecture".to_string();
        let area = Rect::new(0, 0, 90, 18);
        let input_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(4),
            ])
            .split(area)[2];
        let inner = Block::default().borders(Borders::ALL).inner(input_area);
        let mut terminal =
            Terminal::new(RatatuiTestBackend::new(area.width, area.height)).expect("terminal");

        draw_app(&mut app, &mut terminal);

        app.input.clear();
        draw_app(&mut app, &mut terminal);

        let actual = buffer_area_lines(terminal.backend().buffer(), inner).join("\n");
        assert!(actual.contains(INPUT_PLACEHOLDER));
        assert!(actual.contains(SHORTCUT_HINT));
        assert!(!actual.contains("persistent memory assistant"));
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
    fn initial_entries_start_with_welcome_screen() {
        let entries = initial_entries();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].role, EntryRole::Welcome));
    }

    #[test]
    fn wide_welcome_layout_renders_text_left_and_combined_middle_column() {
        let lines = render_welcome_screen(
            120,
            "FOX\nART",
            &[skill("weather", "🌤"), skill("browser", "🌐")],
        );
        let text = rendered_text(&lines);
        let commands_index = text
            .iter()
            .position(|line| line.contains("Commands"))
            .expect("commands header");
        let skills_index = text
            .iter()
            .position(|line| line.contains("Skills"))
            .expect("skills header");

        assert!(text.iter().any(|line| line.contains("___")));
        assert!(text.iter().any(|line| line.contains("FOX")));
        assert!(skills_index > commands_index);
        assert!(text
            .iter()
            .any(|line| line.contains("/help") && line.contains("overview")));
        assert!(text.iter().any(|line| line.contains("🌤  weather")));
        assert!(text.iter().any(|line| line.contains(VERSION_LABEL)));
    }

    #[test]
    fn medium_welcome_layout_stacks_skills_below_commands() {
        let lines = render_welcome_screen(80, "FOX", &[skill("weather", "🌤")]);
        let text = rendered_text(&lines);
        let commands_index = text
            .iter()
            .position(|line| line.contains("Commands"))
            .expect("commands header");
        let skills_index = text
            .iter()
            .position(|line| line.contains("Skills"))
            .expect("skills header");

        assert!(text.iter().any(|line| line.contains("FOX")));
        assert!(skills_index > commands_index);
    }

    #[test]
    fn narrow_welcome_layout_omits_mascot_art() {
        let lines = render_welcome_screen(50, "FOX", &[skill("weather", "🌤")]);
        let text = rendered_text(&lines);

        assert!(!text.iter().any(|line| line.contains("FOX")));
        assert!(text.iter().any(|line| line.contains("Commands")));
        assert!(text.iter().any(|line| line.contains("Skills")));
    }

    #[test]
    fn welcome_screen_shows_empty_skills_placeholder() {
        let lines = render_welcome_screen(50, "FOX", &[]);
        let text = rendered_text(&lines).join("\n");

        assert!(text.contains("No local skills installed."));
        assert!(text.contains("/skills"));
        assert!(text.contains("workflow help"));
        assert!(text.contains("fawx skill build"));
        assert!(text.contains("fawx skill install"));
    }

    #[test]
    fn skills_message_for_built_only_skill_keeps_server_step_distinct() {
        let text = format_skills_message(&[], &[skill("test-built", "🧪")]);

        assert!(text.contains("Built locally: artifact exists in the repo/build tree"));
        assert!(text.contains("Built locally (repo artifact found):"));
        assert!(text.contains("○ 🧪 test-built"));
        assert!(text.contains("Loaded on server: running server reports it via /v1/skills"));
        assert!(text.contains("Recommended workflows:"));
        assert!(text.contains("Local dev: fawx skill build <project>"));
        assert!(text.contains("/skills does not verify loaded-on-server state."));
    }

    #[test]
    fn skills_message_for_installed_skill_requires_server_confirmation() {
        let text = format_skills_message(&[skill("weather", "🌤")], &[]);

        assert!(text.contains("Installed locally: skill exists in ~/.fawx/skills"));
        assert!(text.contains("Installed locally (~/.fawx/skills):"));
        assert!(text.contains("✓ 🌤 weather"));
        assert!(text.contains("Loaded on server: running server reports it via /v1/skills"));
        assert!(text.contains("Prebuilt artifact: fawx skill install <path>"));
        assert!(text.contains("/skills does not verify loaded-on-server state."));
    }

    #[test]
    fn skills_message_without_local_skills_stays_explicit_about_scope() {
        let text = format_skills_message(&[], &[]);

        assert!(text.contains("No local built or installed skills found."));
        assert!(text.contains("Built-in repo skills: skills/build.sh --install"));
        assert!(text.contains("/skills does not verify loaded-on-server state."));
        assert!(text.contains(
            "The Swift Skills UI and /v1/skills show only skills the running server has loaded."
        ));
    }

    #[test]
    fn garbled_logo_art_uses_ascii_fallback() {
        let art = validate_logo_art("⣿⣿⣿⣿\n⣷⣷⣷⣷".to_string()).unwrap_err();

        assert!(art.to_string().contains("garbled"));
        assert!(render_logo_art(LOGO_RENDER_WIDTH).is_ok());
    }

    #[test]
    fn welcome_screen_truncates_long_skill_lists() {
        let skills = (1..=10)
            .map(|index| skill(&format!("skill-{index}"), "🧩"))
            .collect::<Vec<_>>();
        let lines = render_welcome_screen(120, "FOX", &skills);
        let text = rendered_text(&lines).join("\n");

        assert!(text.contains("+2 more"));
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

    #[test]
    fn discover_built_skills_filters_against_provided_installed_skills() {
        let built_dir = repo_root_from_manifest_dir()
            .expect("repo root")
            .join("skills")
            .join("test-filtered-built-skill");
        fs::create_dir_all(built_dir.join("pkg")).expect("built dir");
        fs::write(
            built_dir.join("manifest.toml"),
            "name = \"weather\"\nicon = \"🌤\"\n",
        )
        .expect("built manifest");
        fs::write(
            built_dir.join("pkg").join("test-filtered-built-skill.wasm"),
            b"wasm",
        )
        .expect("built wasm");

        let built = discover_built_skills(&[skill("weather", "🌤")]);

        fs::remove_dir_all(&built_dir).expect("cleanup built dir");

        assert!(built.iter().all(|skill| skill.name != "weather"));
    }

    #[test]
    fn built_skill_artifact_detection_uses_wasip1_target_output() {
        let built_dir = repo_root_from_manifest_dir()
            .expect("repo root")
            .join("skills")
            .join("test-wasip1-built-skill");
        fs::create_dir_all(
            built_dir
                .join("target")
                .join("wasm32-wasip1")
                .join("release"),
        )
        .expect("built dir");
        fs::write(
            built_dir
                .join("target")
                .join("wasm32-wasip1")
                .join("release")
                .join("test_wasip1_built_skill.wasm"),
            b"wasm",
        )
        .expect("built wasm");

        assert!(built_skill_artifact_exists(&built_dir));

        fs::remove_dir_all(&built_dir).expect("cleanup built dir");
    }

    #[test]
    fn skills_command_shows_installed_and_built_skills() {
        let _guard = env_lock().blocking_lock();
        let home = temp_test_dir("home");
        let previous_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &home);

        let installed_dir = home.join(".fawx").join("skills").join("weather-skill");
        fs::create_dir_all(&installed_dir).expect("installed dir");
        fs::write(
            installed_dir.join("manifest.toml"),
            "name = \"weather\"\nicon = \"🌤\"\n",
        )
        .expect("installed manifest");

        let built_dir = repo_root_from_manifest_dir()
            .expect("repo root")
            .join("skills")
            .join("test-built-skill");
        fs::create_dir_all(built_dir.join("pkg")).expect("built dir");
        fs::write(
            built_dir.join("manifest.toml"),
            "name = \"test-built\"\nicon = \"🧪\"\n",
        )
        .expect("built manifest");
        fs::write(built_dir.join("pkg").join("test-built-skill.wasm"), b"wasm")
            .expect("built wasm");

        let mut app = test_app();
        assert!(app.handle_local_command("/skills"));
        let text = last_entry_text(&app).to_string();

        fs::remove_dir_all(&built_dir).expect("cleanup built dir");
        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        fs::remove_dir_all(&home).expect("cleanup home");

        assert!(text.contains("Installed locally (~/.fawx/skills):"));
        assert!(text.contains("✓ 🌤 weather"));
        assert!(text.contains("Built locally (repo artifact found):"));
        assert!(text.contains("○ 🧪 test-built"));
        assert!(text.contains("Recommended workflows:"));
        assert!(text.contains("Loaded on server: running server reports it via /v1/skills"));
        assert!(text.contains("/skills does not verify loaded-on-server state."));
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
        app.entries
            .push(Entry::plain(EntryRole::Assistant, "stale".to_string()));
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
