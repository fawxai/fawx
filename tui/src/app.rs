#[cfg(feature = "embedded")]
use crate::embedded_backend::EmbeddedBackend;
use crate::experiment_panel::ExperimentPanel;
use crate::fawx_backend::{
    friendly_error_message, BackendEvent, EngineBackend, EngineStatus, HttpBackend,
    TranscriptPhaseBoundary,
};
use crate::markdown_render::render_markdown_text_with_width;
use crate::render::line_utils::{line_to_static, prefix_lines};
use crate::wrapping::{adaptive_wrap_line, RtOptions};
use anyhow::Context;
use base64::Engine as _;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as CEvent, EventStream, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
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
use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Stdout, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
#[cfg(target_os = "macos")]
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use unicode_width::UnicodeWidthChar;

mod transcript_render;
use transcript_render::{
    phase_separator_line, render_activity_group_entry, render_tool_result_entry,
    render_tool_use_entry,
};

const INPUT_PLACEHOLDER: &str = "Ask Fawx anything...";
const SHORTCUT_HINT: &str =
    "Ctrl+C: cancel | Ctrl+Y: copy | Tab: activity | /clear: clear | /quit: exit";
const UNICODE_ACTIVE_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
const ASCII_ACTIVE_FRAMES: [&str; 4] = ["-", "\\", "|", "/"];
const ACTIVE_FRAME_TICKS: usize = 4;
const SYNTHETIC_ACTIVITY_GROUP_PREFIX: &str = "__fawx_tui_legacy_activity:";

type AppBackend = Arc<dyn EngineBackend>;
type SharedPanel = Arc<Mutex<ExperimentPanel>>;
type BuiltBackend = (AppBackend, String, SharedPanel);
type BackendBuildResult = anyhow::Result<BuiltBackend>;

fn select_active_spinner_frames() -> &'static [&'static str; 4] {
    let force_ascii = std::env::var("FAWX_TUI_ASCII_SPINNER").ok();
    let term = std::env::var("TERM").ok();
    let locale = std::env::var("LC_ALL")
        .ok()
        .or_else(|| std::env::var("LC_CTYPE").ok())
        .or_else(|| std::env::var("LANG").ok());
    active_spinner_frames_for_env(force_ascii.as_deref(), term.as_deref(), locale.as_deref())
}

fn active_spinner_frames_for_env(
    force_ascii: Option<&str>,
    term: Option<&str>,
    locale: Option<&str>,
) -> &'static [&'static str; 4] {
    let force_ascii = force_ascii
        .map(str::trim)
        .is_some_and(|value| !value.is_empty() && value != "0");
    if force_ascii {
        return &ASCII_ACTIVE_FRAMES;
    }

    if term
        .map(|value| matches!(value, "dumb" | "linux"))
        .unwrap_or(false)
    {
        return &ASCII_ACTIVE_FRAMES;
    }

    if locale
        .map(|value| {
            let value = value.to_ascii_uppercase();
            !(value.contains("UTF-8") || value.contains("UTF8"))
        })
        .unwrap_or(false)
    {
        return &ASCII_ACTIVE_FRAMES;
    }

    &UNICODE_ACTIVE_FRAMES
}

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
// This bounds only the TUI render buffer. Engine conversation history is owned by
// the backend/session layer and is not truncated here.
const MAX_TRANSCRIPT_ENTRIES: usize = 400;
const COPY_SELECTION_HINT: &str = "selection: Ctrl+Y copy • Esc clear";
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
static CLICKABLE_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r"https?://[^\s<>()]+") {
        Ok(regex) => regex,
        Err(error) => panic!("invalid clickable URL regex: {error}"),
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
    WorkingNarration,
    FinalAnswer,
    System,
    Error,
    ActivityGroup,
    CompletedSummary,
    ToolUse,
    // TODO(tui-transcript): keep this flat legacy role only for the
    // standalone renderer regression tests until the legacy tool renderer is
    // fully deleted in favor of normalized activity groups.
    #[allow(dead_code)]
    ToolResult,
    // TODO(tui-transcript): see ToolResult; production backend events should
    // normalize failures into TuiActivityGroup rather than constructing this.
    #[allow(dead_code)]
    ToolError,
}

// TODO(tui-transcript): retained only to exercise the legacy flat tool result
// renderer in tests while production tool events are normalized into activity
// groups. Delete with EntryRole::ToolResult/ToolError once those tests move to
// activity-group fixtures.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy)]
enum ToolOutcome {
    Success,
    Error,
}

#[derive(Clone)]
struct Entry {
    role: EntryRole,
    text: String,
    tool_name: Option<String>,
    tool_arguments: Option<Value>,
    activity_group: Option<TuiActivityGroup>,
    render_phase: Option<TuiRenderPhase>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TuiActivityGroup {
    id: String,
    title: Option<String>,
    kind: Option<String>,
    narration: Option<String>,
    tool_calls: Vec<TuiToolCall>,
    is_live: bool,
    collapsed: bool,
    synthetic: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TuiToolCall {
    id: String,
    name: String,
    arguments: Option<Value>,
    result: Option<String>,
    success: Option<bool>,
    progress: Option<TuiToolProgress>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TuiToolProgress {
    category: String,
    target: Option<String>,
    advances_slot: Option<String>,
    outcome: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TuiRenderPhase {
    Working,
    Activity,
    Summary,
    Response,
}

impl TuiRenderPhase {
    fn from_boundary(boundary: &TranscriptPhaseBoundary) -> Option<Self> {
        match boundary {
            TranscriptPhaseBoundary::CollectingWork => Some(Self::Working),
            TranscriptPhaseBoundary::ExecutingTools => Some(Self::Activity),
            TranscriptPhaseBoundary::Summarizing => Some(Self::Summary),
            TranscriptPhaseBoundary::Finalizing => Some(Self::Response),
            TranscriptPhaseBoundary::Completed | TranscriptPhaseBoundary::Other(_) => None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct TuiTranscriptRenderModel {
    turns: Vec<TuiTranscriptRenderTurn>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct TuiTranscriptRenderTurn {
    sections: Vec<TuiTranscriptRenderSection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TuiTranscriptRenderSection {
    phase: Option<TuiRenderPhase>,
    entry_indices: Vec<usize>,
}

impl TuiTranscriptRenderModel {
    fn reduce(entries: &[Entry]) -> Self {
        let mut model = Self::default();
        let mut turn = TuiTranscriptRenderTurn::default();

        for (index, entry) in entries.iter().enumerate() {
            if matches!(entry.role, EntryRole::User) && !turn.is_empty() {
                model.turns.push(turn);
                turn = TuiTranscriptRenderTurn::default();
            }
            turn.push(index, entry.render_phase);
        }

        if !turn.is_empty() {
            model.turns.push(turn);
        }

        model
    }
}

impl TuiTranscriptRenderTurn {
    fn is_empty(&self) -> bool {
        self.sections
            .iter()
            .all(|section| section.entry_indices.is_empty())
    }

    fn push(&mut self, entry_index: usize, phase: Option<TuiRenderPhase>) {
        if let Some(section) = self
            .sections
            .last_mut()
            .filter(|section| section.phase == phase)
        {
            section.entry_indices.push(entry_index);
            return;
        }

        self.sections.push(TuiTranscriptRenderSection {
            phase,
            entry_indices: vec![entry_index],
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TranscriptPoint {
    line: usize,
    column: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TranscriptSelection {
    anchor: TranscriptPoint,
    focus: TranscriptPoint,
    dragging: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TranscriptSelectionRange {
    start: TranscriptPoint,
    end: TranscriptPoint,
}

impl TranscriptSelection {
    fn new(point: TranscriptPoint) -> Self {
        Self {
            anchor: point,
            focus: point,
            dragging: true,
        }
    }

    fn range(self) -> Option<TranscriptSelectionRange> {
        if self.anchor == self.focus {
            return None;
        }

        let (start, end) = if transcript_point_leq(self.anchor, self.focus) {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        };
        Some(TranscriptSelectionRange { start, end })
    }
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
            Self::WorkingNarration => "working_narration",
            Self::FinalAnswer => "final_answer",
            Self::System => "system",
            Self::Error => "error",
            Self::ActivityGroup => "activity_group",
            Self::CompletedSummary => "completed_summary",
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
            activity_group: None,
            render_phase: None,
        }
    }

    fn with_render_phase(mut self, phase: Option<TuiRenderPhase>) -> Self {
        self.render_phase = phase;
        self
    }

    fn working_narration(text: impl Into<String>) -> Self {
        Self::plain(EntryRole::WorkingNarration, text)
    }

    fn final_answer(text: impl Into<String>) -> Self {
        Self::plain(EntryRole::FinalAnswer, text)
    }

    fn completed_summary(text: impl Into<String>) -> Self {
        Self::plain(EntryRole::CompletedSummary, text)
    }

    fn activity_group(group: TuiActivityGroup) -> Self {
        Self {
            role: EntryRole::ActivityGroup,
            text: group.title.clone().unwrap_or_default(),
            tool_name: None,
            tool_arguments: None,
            activity_group: Some(group),
            render_phase: None,
        }
    }

    // TODO(tui-transcript): test-only fixture constructor for the legacy flat
    // tool-use renderer. Runtime events should go through TuiActivityGroup.
    #[cfg_attr(not(test), allow(dead_code))]
    fn tool_use(name: String, arguments: Value) -> Self {
        Self {
            role: EntryRole::ToolUse,
            text: name.clone(),
            tool_name: Some(name),
            tool_arguments: normalize_tool_arguments(arguments),
            activity_group: None,
            render_phase: None,
        }
    }

    // TODO(tui-transcript): test-only fixture constructor for legacy flat tool
    // results. Runtime success/failure state belongs on TuiToolCall.
    #[cfg_attr(not(test), allow(dead_code))]
    fn tool_result(outcome: ToolOutcome, name: Option<String>, content: String) -> Self {
        Self {
            role: outcome.role(),
            text: content,
            tool_name: name,
            tool_arguments: None,
            activity_group: None,
            render_phase: None,
        }
    }
}

impl TuiActivityGroup {
    fn new(
        id: String,
        title: Option<String>,
        kind: Option<String>,
        narration: Option<String>,
    ) -> Self {
        Self {
            id,
            title: title.and_then(non_empty_trimmed),
            kind: kind.and_then(non_empty_trimmed),
            narration: narration.and_then(non_empty_trimmed),
            tool_calls: Vec::new(),
            is_live: true,
            collapsed: true,
            synthetic: false,
        }
    }

    fn synthetic(id: String, narration: Option<String>) -> Self {
        Self {
            id,
            title: Some("Tool activity".to_string()),
            kind: Some("legacy_tool_events".to_string()),
            narration: narration.and_then(non_empty_trimmed),
            tool_calls: Vec::new(),
            is_live: true,
            collapsed: true,
            synthetic: true,
        }
    }

    fn error_count(&self) -> usize {
        self.tool_calls
            .iter()
            .filter(|call| call.success == Some(false))
            .count()
    }

    fn running_count(&self) -> usize {
        self.tool_calls
            .iter()
            .filter(|call| call.success.is_none())
            .count()
    }
}

impl TuiToolCall {
    fn new(id: Option<String>, name: Option<String>) -> Self {
        let name = name
            .and_then(non_empty_trimmed)
            .unwrap_or_else(|| "tool".to_string());
        let id = id
            .and_then(non_empty_trimmed)
            .unwrap_or_else(|| format!("{}:pending", name));
        Self {
            id,
            name,
            arguments: None,
            result: None,
            success: None,
            progress: None,
        }
    }
}

impl ToolOutcome {
    // TODO(tui-transcript): remove with Entry::tool_result once the final flat
    // tool-result renderer tests are migrated to activity-group fixtures.
    #[cfg_attr(not(test), allow(dead_code))]
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
    response_preview_text: String,
    final_answer_text: Option<String>,
    transcript_phase: Option<TranscriptPhaseBoundary>,
    logo_art: String,
    installed_skills: Vec<LocalSkillSummary>,
    pending_request: bool,
    awaiting_stream_start: bool,
    follow_output: bool,
    scroll: u16,
    input_scroll: u16,
    spinner_frame: usize,
    spinner_frames: &'static [&'static str; 4],
    last_meta: Option<String>,
    last_tokens: Option<TokenUsageSummary>,
    active_tool_name: Option<String>,
    active_legacy_activity_id: Option<String>,
    focused_activity_group_id: Option<String>,
    expanded_activity_group_ids: BTreeSet<String>,
    next_synthetic_activity_id: usize,
    should_quit: bool,
    transcript_area: Rect,
    input_area: Rect,
    transcript_selection: Option<TranscriptSelection>,
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
            response_preview_text: String::new(),
            final_answer_text: None,
            transcript_phase: None,
            logo_art: String::new(),
            installed_skills: discover_installed_skills(),
            pending_request: false,
            awaiting_stream_start: false,
            follow_output: true,
            scroll: 0,
            input_scroll: 0,
            spinner_frame: 0,
            spinner_frames: select_active_spinner_frames(),
            last_meta: None,
            last_tokens: None,
            active_tool_name: None,
            active_legacy_activity_id: None,
            focused_activity_group_id: None,
            expanded_activity_group_ids: BTreeSet::new(),
            next_synthetic_activity_id: 0,
            should_quit: false,
            transcript_area: Rect::default(),
            input_area: Rect::default(),
            transcript_selection: None,
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
        if self.pending_request {
            self.spinner_frame =
                (self.spinner_frame + 1) % (self.spinner_frames.len() * ACTIVE_FRAME_TICKS);
        }
    }

    fn active_spinner(&self) -> &'static str {
        self.spinner_frames[(self.spinner_frame / ACTIVE_FRAME_TICKS) % self.spinner_frames.len()]
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
            CEvent::Paste(text) => self.insert_pasted_text(&text),
            CEvent::Mouse(mouse) => self.handle_mouse_event(mouse),
            CEvent::Resize(_, _) => self.follow_output = true,
            _ => {}
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                if self.transcript_selection.is_some() {
                    self.transcript_selection = None;
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.copy_transcript_selection();
            }
            KeyCode::Tab if self.input.is_empty() => {
                self.focus_activity_group(false);
            }
            KeyCode::BackTab if self.input.is_empty() => {
                self.focus_activity_group(true);
            }
            KeyCode::Enter if self.input.is_empty() && self.toggle_focused_activity_group() => {}
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

    fn insert_pasted_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.input.push_str(&normalize_pasted_text(text));
        self.scroll_input_to_bottom();
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(point) = self.transcript_point_for_mouse(mouse) {
                    if self.toggle_activity_group_at_transcript_line(point.line) {
                        self.transcript_selection = None;
                        return;
                    }
                    self.transcript_selection = Some(TranscriptSelection::new(point));
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(point) = self.transcript_point_for_mouse(mouse) {
                    if let Some(selection) = &mut self.transcript_selection {
                        selection.focus = point;
                        selection.dragging = true;
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let mut clicked_point = None;
                if let Some(point) = self.transcript_point_for_mouse(mouse) {
                    clicked_point = Some(point);
                    if let Some(selection) = &mut self.transcript_selection {
                        selection.focus = point;
                        selection.dragging = false;
                    }
                }
                if self
                    .transcript_selection
                    .and_then(TranscriptSelection::range)
                    .is_none()
                {
                    if let Some(point) = clicked_point {
                        if self.open_transcript_url_at_point(point) {
                            self.transcript_selection = None;
                            return;
                        }
                    }
                    self.transcript_selection = None;
                }
            }
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

    fn transcript_point_for_mouse(&self, mouse: MouseEvent) -> Option<TranscriptPoint> {
        let inner = transcript_inner_area(self.transcript_area);
        if !rect_contains(inner, mouse.column, mouse.row) {
            return None;
        }

        let row = mouse.row.saturating_sub(inner.y) as usize;
        let line = self.scroll as usize + row;
        let column = mouse.column.saturating_sub(inner.x) as usize;
        Some(TranscriptPoint { line, column })
    }

    fn toggle_activity_group_at_transcript_line(&mut self, line: usize) -> bool {
        let width = transcript_inner_width(self.transcript_area);
        let Some(group_id) = self.activity_group_header_id_at_line(line, width) else {
            return false;
        };
        self.focused_activity_group_id = Some(group_id.clone());
        self.toggle_activity_group_by_id(&group_id);
        self.follow_output = false;
        true
    }

    fn open_transcript_url_at_point(&mut self, point: TranscriptPoint) -> bool {
        let Some(url) = self.url_at_transcript_point(point) else {
            return false;
        };
        if let Err(error) = open_url(&url) {
            self.push_error(format!("Open link failed: {error}"));
        }
        true
    }

    fn url_at_transcript_point(&self, point: TranscriptPoint) -> Option<String> {
        let width = transcript_inner_width(self.transcript_area);
        let lines = self.rendered_transcript_lines(width);
        let line = lines.get(point.line)?;
        url_at_display_column(&line_text(line), point.column)
    }

    fn toggle_focused_activity_group(&mut self) -> bool {
        self.normalize_activity_group_focus();
        let Some(group_id) = self.focused_activity_group_id.clone() else {
            return false;
        };
        self.toggle_activity_group_by_id(&group_id);
        self.follow_output = false;
        true
    }

    fn toggle_activity_group_by_id(&mut self, group_id: &str) {
        let Some(is_expanded) = self.activity_group_mut(group_id).map(|group| {
            group.collapsed = !group.collapsed;
            !group.collapsed
        }) else {
            return;
        };

        if is_expanded {
            self.expanded_activity_group_ids
                .insert(group_id.to_string());
        } else {
            self.expanded_activity_group_ids.remove(group_id);
        }
    }

    fn focus_activity_group(&mut self, reverse: bool) -> bool {
        let ids = self.toggleable_activity_group_ids();
        if ids.is_empty() {
            self.focused_activity_group_id = None;
            return false;
        }

        let next_index = self
            .focused_activity_group_id
            .as_deref()
            .and_then(|focused_id| ids.iter().position(|id| id == focused_id))
            .map(|index| {
                if reverse {
                    index.checked_sub(1).unwrap_or(ids.len() - 1)
                } else {
                    (index + 1) % ids.len()
                }
            })
            .unwrap_or_else(|| if reverse { ids.len() - 1 } else { 0 });

        self.focused_activity_group_id = Some(ids[next_index].clone());
        self.follow_output = false;
        true
    }

    fn normalize_activity_group_focus(&mut self) {
        let Some(focused_id) = self.focused_activity_group_id.as_deref() else {
            return;
        };
        if !self
            .toggleable_activity_group_ids()
            .iter()
            .any(|id| id == focused_id)
        {
            self.focused_activity_group_id = None;
        }
    }

    fn toggleable_activity_group_ids(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter_map(|entry| entry.activity_group.as_ref())
            .filter(|group| !group.is_live && !group.tool_calls.is_empty())
            .map(|group| group.id.clone())
            .collect()
    }

    fn active_render_phase(&self) -> Option<TuiRenderPhase> {
        self.transcript_phase
            .as_ref()
            .and_then(TuiRenderPhase::from_boundary)
    }

    fn push_entry_with_phase(&mut self, entry: Entry, phase: Option<TuiRenderPhase>) {
        self.entries.push(entry.with_render_phase(phase));
    }

    fn activity_group_header_id_at_line(&self, line: usize, width: usize) -> Option<String> {
        self.rendered_transcript_lines_with_activity_headers(width)
            .1
            .get(line)
            .cloned()
            .flatten()
    }

    fn submit_input(&mut self) {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return;
        }
        self.transcript_selection = None;

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

        self.push_entry_with_phase(Entry::plain(EntryRole::User, input.clone()), None);
        self.prune_transcript_entries();
        self.streaming_text = Some(String::new());
        self.response_preview_text.clear();
        self.final_answer_text = None;
        self.transcript_phase = None;
        self.pending_request = true;
        self.awaiting_stream_start = true;
        self.follow_output = true;
        self.spinner_frame = 0;
        self.last_meta = None;
        self.active_tool_name = None;
        self.active_legacy_activity_id = None;
        self.focused_activity_group_id = None;
        self.expanded_activity_group_ids.clear();
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
        self.response_preview_text.clear();
        self.final_answer_text = None;
        self.transcript_phase = None;
        self.pending_request = false;
        self.awaiting_stream_start = false;
        self.last_meta = None;
        self.last_tokens = None;
        self.active_tool_name = None;
        self.active_legacy_activity_id = None;
        self.focused_activity_group_id = None;
        self.expanded_activity_group_ids.clear();
        self.next_synthetic_activity_id = 0;
        self.transcript_selection = None;
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
            BackendEvent::WorkingNarrationDelta {
                text,
                voiceover_suppressed,
            } => {
                self.awaiting_stream_start = false;
                if !voiceover_suppressed {
                    self.append_working_narration(text);
                }
                self.follow_output = true;
            }
            BackendEvent::TextPreviewDelta(delta) => {
                self.awaiting_stream_start = false;
                self.append_response_preview_text(delta);
                self.follow_output = true;
            }
            BackendEvent::TextReset => {
                self.reset_response_preview_text();
            }
            BackendEvent::TextDelta(delta) => {
                self.awaiting_stream_start = false;
                self.streaming_text
                    .get_or_insert_with(String::new)
                    .push_str(&delta);
                self.follow_output = true;
            }
            BackendEvent::FinalAnswerDelta(delta) => {
                self.awaiting_stream_start = false;
                if !self.response_preview_text.is_empty()
                    && self
                        .streaming_text
                        .as_deref()
                        .map(|text| {
                            text.ends_with(&self.response_preview_text)
                                || self.response_preview_text.starts_with(&delta)
                        })
                        .unwrap_or(false)
                {
                    self.streaming_text = None;
                }
                self.response_preview_text.clear();
                self.final_answer_text
                    .get_or_insert_with(String::new)
                    .push_str(&delta);
                self.follow_output = true;
            }
            BackendEvent::TranscriptPhaseBoundary(phase) => {
                self.transcript_phase = Some(phase.clone());
                match phase {
                    TranscriptPhaseBoundary::Finalizing | TranscriptPhaseBoundary::Completed => {
                        self.settle_current_turn_activity();
                    }
                    TranscriptPhaseBoundary::CollectingWork
                    | TranscriptPhaseBoundary::ExecutingTools
                    | TranscriptPhaseBoundary::Summarizing
                    | TranscriptPhaseBoundary::Other(_) => {}
                }
            }
            BackendEvent::CompletedSummary(summary) => {
                self.settle_current_turn_activity();
                self.push_entry_with_phase(
                    Entry::completed_summary(summary),
                    Some(TuiRenderPhase::Summary),
                );
                self.follow_output = true;
            }
            BackendEvent::ToolUse { name, arguments } => self.push_tool_use(name, arguments),
            BackendEvent::ActivityStart { id, title, kind } => {
                self.begin_activity_group(id, title, kind)
            }
            BackendEvent::ActivityEnd { id } => self.end_activity_group(&id),
            BackendEvent::ActivityToolCallStart {
                activity_id,
                id,
                name,
            } => self.begin_activity_tool_call(&activity_id, id, name),
            BackendEvent::ActivityToolCallComplete {
                activity_id,
                id,
                name,
                arguments,
            } => self.complete_activity_tool_call(&activity_id, id, name, arguments),
            BackendEvent::ActivityToolResult {
                activity_id,
                id,
                tool_name,
                success,
                content,
            } => self.finish_activity_tool_call(&activity_id, id, tool_name, success, content),
            BackendEvent::ToolProgress {
                activity_id,
                id,
                tool_name,
                category,
                target,
                advances_slot,
                outcome,
            } => self.update_activity_tool_progress(
                activity_id,
                id,
                tool_name,
                TuiToolProgress {
                    category,
                    target,
                    advances_slot,
                    outcome,
                },
            ),
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
                self.response_preview_text.clear();
                if let Some(text) = self.streaming_text.take() {
                    if !text.trim().is_empty() {
                        self.push_entry_with_phase(
                            Entry::plain(EntryRole::Assistant, text),
                            self.active_render_phase().or(Some(TuiRenderPhase::Working)),
                        );
                    }
                }
                self.settle_current_turn_activity();
                if let Some(text) = self.final_answer_text.take() {
                    if !text.trim().is_empty() {
                        self.push_entry_with_phase(
                            Entry::final_answer(text),
                            Some(TuiRenderPhase::Response),
                        );
                    }
                }
                self.transcript_phase = Some(TranscriptPhaseBoundary::Completed);
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
                self.response_preview_text.clear();
                if let Some(text) = self.streaming_text.take() {
                    if !text.is_empty() {
                        self.push_entry_with_phase(
                            Entry::plain(EntryRole::Assistant, text),
                            self.active_render_phase().or(Some(TuiRenderPhase::Working)),
                        );
                    }
                }
                if let Some(text) = self.final_answer_text.take() {
                    if !text.is_empty() {
                        self.push_entry_with_phase(
                            Entry::final_answer(text),
                            Some(TuiRenderPhase::Response),
                        );
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
        self.prune_transcript_entries();
    }

    fn push_tool_use(&mut self, name: String, arguments: Value) {
        self.awaiting_stream_start = false;
        self.active_tool_name = Some(name.clone());
        let Some(group) = self.ensure_legacy_activity_group() else {
            tracing::warn!(
                tool_name = %name,
                "failed to create synthetic TUI activity group for legacy tool_use"
            );
            return;
        };
        let arguments = normalize_tool_arguments(arguments);
        let index = arguments
            .as_ref()
            .and_then(|_| {
                group
                    .tool_calls
                    .iter()
                    .rposition(|call| call.name == name && call.arguments.is_none())
            })
            .unwrap_or_else(|| {
                let call_id = legacy_tool_call_id(&name, group.tool_calls.len());
                group
                    .tool_calls
                    .push(TuiToolCall::new(Some(call_id), Some(name.clone())));
                group.tool_calls.len() - 1
            });
        let call = &mut group.tool_calls[index];
        call.name = name;
        call.arguments = arguments;
        self.follow_output = true;
    }

    fn push_tool_result(&mut self, name: Option<String>, success: bool, content: String) {
        self.awaiting_stream_start = false;
        let resolved_name = self
            .active_tool_name
            .take()
            .or(name.filter(|value| !value.is_empty()));
        let Some(group) = self.ensure_legacy_activity_group() else {
            tracing::warn!("failed to create synthetic TUI activity group for legacy tool_result");
            return;
        };
        let index = resolved_name
            .as_deref()
            .and_then(|tool_name| {
                activity_tool_call_index(
                    group,
                    &normalized_tool_call_id(None, Some(tool_name)),
                    Some(tool_name),
                )
            })
            .or_else(|| {
                group
                    .tool_calls
                    .iter()
                    .rposition(|call| call.success.is_none())
            })
            .unwrap_or_else(|| {
                let call_id = legacy_tool_call_id(
                    resolved_name.as_deref().unwrap_or("tool"),
                    group.tool_calls.len(),
                );
                group
                    .tool_calls
                    .push(TuiToolCall::new(Some(call_id), resolved_name.clone()));
                group.tool_calls.len() - 1
            });
        let call = &mut group.tool_calls[index];
        if let Some(resolved_name) = resolved_name.and_then(non_empty_trimmed) {
            call.name = resolved_name;
        }
        call.success = Some(success);
        call.result = Some(content);
        self.follow_output = true;
    }

    fn append_working_narration(&mut self, delta: String) {
        if delta.is_empty() {
            return;
        }
        if self.append_working_narration_to_active_group(&delta) {
            return;
        }
        if let Some(entry) = self.entries.last_mut() {
            if matches!(entry.role, EntryRole::WorkingNarration) {
                entry.text.push_str(&delta);
                return;
            }
        }
        self.push_entry_with_phase(
            Entry::working_narration(delta),
            Some(TuiRenderPhase::Working),
        );
    }

    fn append_response_preview_text(&mut self, delta: String) {
        if delta.is_empty() {
            return;
        }
        self.response_preview_text.push_str(&delta);
        self.streaming_text
            .get_or_insert_with(String::new)
            .push_str(&delta);
    }

    fn reset_response_preview_text(&mut self) {
        let preview = std::mem::take(&mut self.response_preview_text);
        if self.final_answer_text.is_none() {
            self.streaming_text = Some(String::new());
        }
        if let Some(preview) = non_empty_trimmed(preview) {
            self.append_working_narration(preview);
        }
        self.follow_output = true;
    }

    fn append_working_narration_to_active_group(&mut self, delta: &str) -> bool {
        let turn_start = self
            .entries
            .iter()
            .rposition(|entry| matches!(entry.role, EntryRole::User))
            .map(|index| index + 1)
            .unwrap_or(0);
        let Some(group) = self.entries[turn_start..]
            .iter_mut()
            .rev()
            .find_map(|entry| entry.activity_group.as_mut().filter(|group| group.is_live))
        else {
            return false;
        };

        append_activity_group_narration(group, delta);
        true
    }

    fn begin_activity_group(&mut self, id: String, title: Option<String>, kind: Option<String>) {
        self.awaiting_stream_start = false;
        let id = normalized_backend_activity_group_id(&id);
        let narration = self.take_pending_working_narration();
        if let Some(group) = self.activity_group_mut(&id) {
            if group.title.is_none() {
                group.title = title.and_then(non_empty_trimmed);
            }
            if group.kind.is_none() {
                group.kind = kind.and_then(non_empty_trimmed);
            }
            if group.narration.is_none() {
                group.narration = narration;
            }
        } else {
            let mut group = TuiActivityGroup::new(id, title, kind, narration);
            self.apply_activity_group_display_state(&mut group);
            self.push_entry_with_phase(
                Entry::activity_group(group),
                Some(TuiRenderPhase::Activity),
            );
        }
        self.follow_output = true;
    }

    fn end_activity_group(&mut self, id: &str) {
        let id = normalized_backend_activity_group_id(id);
        let should_expand = self.expanded_activity_group_ids.contains(&id);
        if let Some(group) = self.activity_group_mut(&id) {
            group.is_live = false;
            group.collapsed = !should_expand;
        } else {
            tracing::warn!(
                activity_id = %id,
                "received TUI activity_end without a matching activity_start"
            );
        }
        self.follow_output = true;
    }

    fn begin_activity_tool_call(
        &mut self,
        activity_id: &str,
        id: Option<String>,
        name: Option<String>,
    ) {
        self.awaiting_stream_start = false;
        let activity_id = normalized_backend_activity_group_id(activity_id);
        let Some(group) = self.ensure_activity_group(&activity_id) else {
            tracing::warn!(
                activity_id = %activity_id,
                "failed to create TUI activity group for tool call start"
            );
            return;
        };
        let call_id = normalized_tool_call_id(id.as_deref(), name.as_deref());
        if group.tool_calls.iter().any(|call| call.id == call_id) {
            return;
        }
        group.tool_calls.push(TuiToolCall::new(Some(call_id), name));
        self.follow_output = true;
    }

    fn complete_activity_tool_call(
        &mut self,
        activity_id: &str,
        id: Option<String>,
        name: Option<String>,
        arguments: Value,
    ) {
        self.awaiting_stream_start = false;
        let activity_id = normalized_backend_activity_group_id(activity_id);
        let Some(group) = self.ensure_activity_group(&activity_id) else {
            tracing::warn!(
                activity_id = %activity_id,
                "failed to create TUI activity group for tool call completion"
            );
            return;
        };
        let call_id = normalized_tool_call_id(id.as_deref(), name.as_deref());
        let index =
            activity_tool_call_index(group, &call_id, name.as_deref()).unwrap_or_else(|| {
                group
                    .tool_calls
                    .push(TuiToolCall::new(Some(call_id.clone()), name.clone()));
                group.tool_calls.len() - 1
            });
        let call = &mut group.tool_calls[index];
        if let Some(name) = name.and_then(non_empty_trimmed) {
            call.name = name;
        }
        call.arguments = normalize_tool_arguments(arguments);
        self.follow_output = true;
    }

    fn finish_activity_tool_call(
        &mut self,
        activity_id: &str,
        id: Option<String>,
        tool_name: Option<String>,
        success: bool,
        content: String,
    ) {
        self.awaiting_stream_start = false;
        let activity_id = normalized_backend_activity_group_id(activity_id);
        let Some(group) = self.ensure_activity_group(&activity_id) else {
            tracing::warn!(
                activity_id = %activity_id,
                "failed to create TUI activity group for tool result"
            );
            return;
        };
        let call_id = normalized_tool_call_id(id.as_deref(), tool_name.as_deref());
        let index =
            activity_tool_call_index(group, &call_id, tool_name.as_deref()).unwrap_or_else(|| {
                group
                    .tool_calls
                    .push(TuiToolCall::new(Some(call_id.clone()), tool_name.clone()));
                group.tool_calls.len() - 1
            });
        let call = &mut group.tool_calls[index];
        if let Some(tool_name) = tool_name.and_then(non_empty_trimmed) {
            call.name = tool_name;
        }
        call.success = Some(success);
        call.result = Some(content);
        self.follow_output = true;
    }

    fn update_activity_tool_progress(
        &mut self,
        activity_id: Option<String>,
        id: Option<String>,
        tool_name: Option<String>,
        progress: TuiToolProgress,
    ) {
        let Some(activity_id) = activity_id.as_deref().and_then(non_empty_trimmed_str) else {
            return;
        };
        let activity_id = normalized_backend_activity_group_id(&activity_id);
        let Some(group) = self.ensure_activity_group(&activity_id) else {
            tracing::warn!(
                activity_id = %activity_id,
                "failed to create TUI activity group for tool progress"
            );
            return;
        };
        let call_id = normalized_tool_call_id(id.as_deref(), tool_name.as_deref());
        let index =
            activity_tool_call_index(group, &call_id, tool_name.as_deref()).unwrap_or_else(|| {
                group
                    .tool_calls
                    .push(TuiToolCall::new(Some(call_id.clone()), tool_name.clone()));
                group.tool_calls.len() - 1
            });
        let call = &mut group.tool_calls[index];
        call.progress = Some(progress);
        self.follow_output = true;
    }

    fn ensure_activity_group(&mut self, id: &str) -> Option<&mut TuiActivityGroup> {
        let id = normalized_backend_activity_group_id(id);
        if let Some(index) = self.activity_group_index(&id) {
            return self
                .entries
                .get_mut(index)
                .and_then(|entry| entry.activity_group.as_mut());
        }

        let narration = self.take_pending_working_narration();
        let mut group = TuiActivityGroup::new(id, None, None, narration);
        self.apply_activity_group_display_state(&mut group);
        self.push_entry_with_phase(Entry::activity_group(group), Some(TuiRenderPhase::Activity));
        self.entries
            .last_mut()
            .and_then(|entry| entry.activity_group.as_mut())
    }

    fn ensure_legacy_activity_group(&mut self) -> Option<&mut TuiActivityGroup> {
        if let Some(id) = self.active_legacy_activity_id.as_deref() {
            if let Some(index) = self.activity_group_index(id) {
                return self
                    .entries
                    .get_mut(index)
                    .and_then(|entry| entry.activity_group.as_mut());
            }
            self.active_legacy_activity_id = None;
        }

        let id = self.next_unique_synthetic_activity_group_id();
        let narration = self.take_pending_working_narration();
        let mut group = TuiActivityGroup::synthetic(id.clone(), narration);
        self.apply_activity_group_display_state(&mut group);
        self.push_entry_with_phase(Entry::activity_group(group), Some(TuiRenderPhase::Activity));
        self.active_legacy_activity_id = Some(id);
        self.entries
            .last_mut()
            .and_then(|entry| entry.activity_group.as_mut())
    }

    fn next_unique_synthetic_activity_group_id(&mut self) -> String {
        loop {
            let id = format!(
                "{SYNTHETIC_ACTIVITY_GROUP_PREFIX}{}",
                self.next_synthetic_activity_id
            );
            self.next_synthetic_activity_id = self.next_synthetic_activity_id.saturating_add(1);
            if self.activity_group_index(&id).is_none() {
                return id;
            }
        }
    }

    fn apply_activity_group_display_state(&self, group: &mut TuiActivityGroup) {
        if self.expanded_activity_group_ids.contains(&group.id) {
            group.collapsed = false;
        }
    }

    fn settle_current_turn_activity(&mut self) {
        let turn_start = self
            .entries
            .iter()
            .rposition(|entry| matches!(entry.role, EntryRole::User))
            .map(|index| index + 1)
            .unwrap_or(0);
        let expanded_activity_group_ids = self.expanded_activity_group_ids.clone();
        for entry in self.entries[turn_start..].iter_mut() {
            if let Some(group) = entry.activity_group.as_mut() {
                group.is_live = false;
                group.collapsed = !expanded_activity_group_ids.contains(&group.id);
            }
        }
        self.active_tool_name = None;
        self.active_legacy_activity_id = None;
    }

    fn activity_group_index(&self, id: &str) -> Option<usize> {
        self.entries.iter().position(|entry| {
            entry
                .activity_group
                .as_ref()
                .map(|group| group.id == id)
                .unwrap_or(false)
        })
    }

    fn activity_group_mut(&mut self, id: &str) -> Option<&mut TuiActivityGroup> {
        self.entries
            .iter_mut()
            .find_map(|entry| entry.activity_group.as_mut().filter(|group| group.id == id))
    }

    fn take_pending_working_narration(&mut self) -> Option<String> {
        let is_pending = self
            .entries
            .last()
            .map(|entry| matches!(entry.role, EntryRole::WorkingNarration))
            .unwrap_or(false);
        if !is_pending {
            return None;
        }
        self.entries
            .pop()
            .and_then(|entry| non_empty_trimmed(entry.text))
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
        self.prune_transcript_entries();
        self.follow_output = true;
    }

    fn push_error(&mut self, message: impl Into<String>) {
        self.entries.push(Entry::plain(EntryRole::Error, message));
        self.prune_transcript_entries();
        self.follow_output = true;
    }

    fn prune_transcript_entries(&mut self) {
        let overflow = self.entries.len().saturating_sub(MAX_TRANSCRIPT_ENTRIES);
        if overflow == 0 {
            return;
        }

        self.entries.drain(0..overflow);
        self.scroll = self
            .scroll
            .saturating_sub(overflow.min(u16::MAX as usize) as u16);
        self.transcript_selection = None;
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
        if self.pending_request {
            let status_text = if self.awaiting_stream_start {
                format!("{} Fawx is thinking", self.active_spinner())
            } else {
                format!("{} Fawx is working", self.active_spinner())
            };
            details.push(status_text);
        } else if self.transcript_selection.is_some() {
            details.push(COPY_SELECTION_HINT.to_string());
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
        let mut lines = self.transcript_lines_for_area(area);
        let scroll = self.sync_transcript_scroll(area, transcript_content_line_count(&lines));
        apply_transcript_selection(&mut lines, self.transcript_selection);
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

    fn copy_transcript_selection(&mut self) {
        let Some(text) = self.selected_transcript_text() else {
            self.push_system("No transcript selection to copy.");
            return;
        };

        match copy_text_to_clipboard(&text) {
            Ok(()) => self.push_system("Copied selected transcript text."),
            Err(error) => self.push_error(format!("Copy failed: {error}")),
        }
    }

    fn selected_transcript_text(&self) -> Option<String> {
        let range = self
            .transcript_selection
            .and_then(TranscriptSelection::range)?;
        let lines = self.rendered_transcript_lines(transcript_inner_width(self.transcript_area));
        selected_text_from_lines(&lines, range)
    }

    fn rendered_transcript_lines(&self, width: usize) -> Vec<Line<'static>> {
        self.rendered_transcript_lines_with_activity_headers(width)
            .0
    }

    fn rendered_transcript_lines_with_activity_headers(
        &self,
        width: usize,
    ) -> (Vec<Line<'static>>, Vec<Option<String>>) {
        let mut out = Vec::new();
        let mut activity_header_ids = Vec::new();
        let mut render_phase = None;
        let render_model = TuiTranscriptRenderModel::reduce(&self.entries);
        for turn in &render_model.turns {
            for section in &turn.sections {
                for entry_index in &section.entry_indices {
                    let Some(entry) = self.entries.get(*entry_index) else {
                        continue;
                    };
                    self.push_rendered_entry_lines(
                        entry,
                        width,
                        section.phase,
                        &mut render_phase,
                        &mut out,
                        &mut activity_header_ids,
                    );
                }
            }
        }
        if let Some(text) = &self.streaming_text {
            if !text.is_empty() {
                let entry = Entry::plain(EntryRole::Assistant, text.clone());
                self.push_rendered_entry_lines(
                    &entry,
                    width,
                    self.active_render_phase().or(Some(TuiRenderPhase::Working)),
                    &mut render_phase,
                    &mut out,
                    &mut activity_header_ids,
                );
            }
        }
        if let Some(text) = &self.final_answer_text {
            if !text.is_empty() {
                let entry = Entry::final_answer(text.clone());
                self.push_rendered_entry_lines(
                    &entry,
                    width,
                    Some(TuiRenderPhase::Response),
                    &mut render_phase,
                    &mut out,
                    &mut activity_header_ids,
                );
            }
        }
        (out, activity_header_ids)
    }

    fn push_rendered_entry_lines(
        &self,
        entry: &Entry,
        width: usize,
        entry_phase: Option<TuiRenderPhase>,
        render_phase: &mut Option<TuiRenderPhase>,
        out: &mut Vec<Line<'static>>,
        activity_header_ids: &mut Vec<Option<String>>,
    ) {
        if !out.is_empty() {
            out.push(Line::default());
            activity_header_ids.push(None);
        }
        if let Some(entry_phase) = entry_phase {
            if *render_phase != Some(entry_phase) {
                out.push(phase_separator_line(entry_phase));
                activity_header_ids.push(None);
                *render_phase = Some(entry_phase);
            }
        } else {
            *render_phase = None;
        }

        let start = out.len();
        self.render_entry(entry, width, out);
        let group_header_id = entry.activity_group.as_ref().and_then(|group| {
            (!group.is_live && !group.tool_calls.is_empty()).then(|| group.id.clone())
        });
        for index in start..out.len() {
            activity_header_ids.push(if index == start {
                group_header_id.clone()
            } else {
                None
            });
        }
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
            EntryRole::WorkingNarration => {
                let text = sanitize_terminal_text(&entry.text);
                let rendered =
                    render_markdown_text_with_width(&text, Some(width.saturating_sub(7)));
                let muted = rendered
                    .lines
                    .into_iter()
                    .map(|line| line.patch_style(Style::default().fg(Color::Gray)))
                    .collect::<Vec<_>>();
                out.extend(prefix_lines(
                    muted,
                    Span::styled("work › ", Style::default().fg(Color::DarkGray)),
                    Span::raw("       "),
                ));
            }
            EntryRole::FinalAnswer => {
                let text = sanitize_terminal_text(&entry.text);
                let rendered =
                    render_markdown_text_with_width(&text, Some(width.saturating_sub(7)));
                out.extend(prefix_lines(
                    rendered.lines,
                    Span::styled(
                        "done › ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("       "),
                ));
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
            EntryRole::ActivityGroup => {
                if let Some(group) = &entry.activity_group {
                    out.extend(render_activity_group_entry(
                        group,
                        width,
                        self.activity_group_is_expanded(group),
                        self.activity_group_is_focused(group),
                        group.is_live.then(|| self.active_spinner()),
                    ));
                }
            }
            EntryRole::CompletedSummary => {
                let text = sanitize_terminal_text(&entry.text);
                out.extend(prefix_wrapped_lines(
                    &text,
                    width,
                    "sum  › ",
                    "       ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
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

    fn activity_group_is_expanded(&self, group: &TuiActivityGroup) -> bool {
        group.is_live || !group.collapsed
    }

    fn activity_group_is_focused(&self, group: &TuiActivityGroup) -> bool {
        !group.is_live
            && self
                .focused_activity_group_id
                .as_deref()
                .is_some_and(|focused_id| focused_id == group.id)
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

fn transcript_point_leq(left: TranscriptPoint, right: TranscriptPoint) -> bool {
    left.line < right.line || (left.line == right.line && left.column <= right.column)
}

fn apply_transcript_selection(lines: &mut [Line<'static>], selection: Option<TranscriptSelection>) {
    let Some(range) = selection.and_then(TranscriptSelection::range) else {
        return;
    };

    for (line_index, line) in lines.iter_mut().enumerate() {
        let Some((start, end)) = selection_columns_for_line(&range, line_index, line.width())
        else {
            continue;
        };
        *line = highlight_line_range(line, start, end);
    }
}

fn selection_columns_for_line(
    range: &TranscriptSelectionRange,
    line_index: usize,
    line_width: usize,
) -> Option<(usize, usize)> {
    if line_index < range.start.line || line_index > range.end.line || line_width == 0 {
        return None;
    }

    let start = if line_index == range.start.line {
        range.start.column.min(line_width)
    } else {
        0
    };
    let end = if line_index == range.end.line {
        range.end.column.min(line_width)
    } else {
        line_width
    };

    (end > start).then_some((start, end))
}

fn highlight_line_range(line: &Line<'static>, start: usize, end: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let mut column = 0;
    for span in &line.spans {
        spans.extend(highlight_span_range(span, start, end, &mut column));
    }
    Line::from(spans)
}

fn highlight_span_range(
    span: &Span<'static>,
    start: usize,
    end: usize,
    column: &mut usize,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buffer = String::new();
    let mut buffer_selected: Option<bool> = None;
    for ch in span.content.as_ref().chars() {
        let width = char_display_width(ch);
        let ch_start = *column;
        let ch_end = ch_start.saturating_add(width);
        let selected = ch_end > start && ch_start < end;
        if buffer_selected != Some(selected) {
            flush_selection_segment(&mut spans, &mut buffer, span.style, buffer_selected);
            buffer_selected = Some(selected);
        }
        buffer.push(ch);
        *column = ch_end;
    }
    flush_selection_segment(&mut spans, &mut buffer, span.style, buffer_selected);
    spans
}

fn flush_selection_segment(
    spans: &mut Vec<Span<'static>>,
    buffer: &mut String,
    base_style: Style,
    selected: Option<bool>,
) {
    if buffer.is_empty() {
        return;
    }
    let style = if selected.unwrap_or(false) {
        Style::default().fg(Color::Black).bg(Color::White)
    } else {
        base_style
    };
    spans.push(Span::styled(std::mem::take(buffer), style));
}

fn selected_text_from_lines(lines: &[Line<'_>], range: TranscriptSelectionRange) -> Option<String> {
    if lines.is_empty() || range.start.line >= lines.len() {
        return None;
    }

    let end_line = range.end.line.min(lines.len().saturating_sub(1));
    let mut selected = Vec::new();
    for (line_index, line) in lines
        .iter()
        .enumerate()
        .take(end_line + 1)
        .skip(range.start.line)
    {
        let text = line_text(line);
        let line_width = text_display_width(&text);
        let Some((start, end)) = selection_columns_for_line(&range, line_index, line_width) else {
            selected.push(String::new());
            continue;
        };
        selected.push(display_slice(&text, start, end));
    }

    let text = selected.join("\n");
    (!text.is_empty()).then_some(text)
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn display_slice(text: &str, start: usize, end: usize) -> String {
    let mut out = String::new();
    let mut column: usize = 0;
    for ch in text.chars() {
        let width = char_display_width(ch);
        let ch_start = column;
        let ch_end = ch_start.saturating_add(width);
        if ch_end > start && ch_start < end {
            out.push(ch);
        }
        column = ch_end;
    }
    out
}

fn url_at_display_column(text: &str, column: usize) -> Option<String> {
    CLICKABLE_URL_RE.find_iter(text).find_map(|match_| {
        let url = trim_clickable_url(match_.as_str());
        if url.is_empty() {
            return None;
        }
        let start = text_display_width(&text[..match_.start()]);
        let end = start.saturating_add(text_display_width(url));
        (column >= start && column <= end).then(|| url.to_string())
    })
}

fn trim_clickable_url(url: &str) -> &str {
    url.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']'])
}

fn text_display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

fn char_display_width(ch: char) -> usize {
    ch.width().unwrap_or(0).max(1)
}

fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        copy_text_to_macos_clipboard(text)
    }
    #[cfg(not(target_os = "macos"))]
    {
        copy_text_to_terminal_clipboard(text)
    }
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";

    let status = ProcessCommand::new(opener)
        .arg(url)
        .status()
        .map_err(|error| format!("failed to launch {opener}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{opener} exited with status {status}"))
    }
}

#[cfg(target_os = "macos")]
fn copy_text_to_macos_clipboard(text: &str) -> Result<(), String> {
    let mut child = ProcessCommand::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to launch pbcopy: {error}"))?;
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| "failed to open pbcopy stdin".to_string())?;
    stdin
        .write_all(text.as_bytes())
        .map_err(|error| format!("failed to write selection to pbcopy: {error}"))?;
    let status = child
        .wait()
        .map_err(|error| format!("failed waiting for pbcopy: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("pbcopy exited with status {status}"))
    }
}

#[cfg(not(target_os = "macos"))]
fn copy_text_to_terminal_clipboard(text: &str) -> Result<(), String> {
    let sequence = osc52_sequence(text, std::env::var_os("TMUX").is_some());
    let mut stdout = io::stdout();
    stdout
        .write_all(sequence.as_bytes())
        .map_err(|error| format!("failed to write OSC 52 clipboard sequence: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("failed to flush OSC 52 clipboard sequence: {error}"))
}

#[cfg_attr(all(target_os = "macos", not(test)), allow(dead_code))]
fn osc52_sequence(text: &str, tmux: bool) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    if tmux {
        format!("\x1bPtmux;\x1b{sequence}\x1b\\")
    } else {
        sequence
    }
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
    skills.sort_by_key(|skill| skill.name.to_lowercase());
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
    skills.sort_by_key(|skill| skill.name.to_lowercase());
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

fn legacy_tool_call_id(name: &str, ordinal: usize) -> String {
    let slug = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("legacy-tool-{}-{}", ordinal.saturating_add(1), slug)
}

fn normalize_pasted_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn non_empty_trimmed(text: String) -> Option<String> {
    non_empty_trimmed_str(&text)
}

fn non_empty_trimmed_str(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn normalized_tool_call_id(id: Option<&str>, name: Option<&str>) -> String {
    id.and_then(non_empty_trimmed_str)
        .or_else(|| name.and_then(non_empty_trimmed_str))
        .unwrap_or_else(|| "tool".to_string())
}

fn normalized_backend_activity_group_id(id: &str) -> String {
    let normalized = non_empty_trimmed_str(id).unwrap_or_else(|| "activity".to_string());
    if normalized.starts_with(SYNTHETIC_ACTIVITY_GROUP_PREFIX) {
        // Keep the TUI's generated legacy namespace private. If a backend ever
        // emits the same prefix, render it as a distinct real backend group
        // rather than merging it with synthetic compatibility entries.
        format!("backend:{normalized}")
    } else {
        normalized
    }
}

fn activity_tool_call_index(
    group: &TuiActivityGroup,
    id: &str,
    name: Option<&str>,
) -> Option<usize> {
    if let Some(index) = group.tool_calls.iter().position(|call| call.id == id) {
        return Some(index);
    }
    let Some(name) = name.and_then(non_empty_trimmed_str) else {
        return group
            .tool_calls
            .iter()
            .rposition(|call| call.success.is_none());
    };
    group
        .tool_calls
        .iter()
        .rposition(|call| call.name == name && call.success.is_none())
}

fn append_activity_group_narration(group: &mut TuiActivityGroup, delta: &str) {
    if group.narration.is_none() && delta.trim().is_empty() {
        return;
    }
    group
        .narration
        .get_or_insert_with(String::new)
        .push_str(delta);
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
        EnableBracketedPaste,
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
        DisableBracketedPaste,
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
        app.spinner_frames = &UNICODE_ACTIVE_FRAMES;
        app
    }

    fn live_like_test_app() -> App {
        let backend: Arc<dyn EngineBackend> = Arc::new(TestBackend::default());
        let (tx, rx) = unbounded_channel();
        let mut app = App::new(
            backend,
            crate::DEFAULT_ENGINE_URL.to_string(),
            Arc::new(Mutex::new(ExperimentPanel::new())),
            tx,
            rx,
        );
        app.spinner_frames = &UNICODE_ACTIVE_FRAMES;
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

        assert_eq!(app.entries.len(), 1);
        let group = app.entries[0]
            .activity_group
            .as_ref()
            .expect("legacy tool events should normalize into an activity group");
        assert!(group.synthetic);
        assert_eq!(group.tool_calls.len(), 1);
        assert_eq!(group.tool_calls[0].name, "run_experiment");
        assert!(group.tool_calls[0].arguments.is_some());
        assert_eq!(group.tool_calls[0].success, Some(true));
    }

    #[test]
    fn repeated_legacy_tool_use_events_get_distinct_call_slots() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::ToolUse {
            name: "run_command".to_string(),
            arguments: json!({ "command": "echo first" }),
        });
        app.handle_backend_event(BackendEvent::ToolUse {
            name: "run_command".to_string(),
            arguments: json!({ "command": "echo second" }),
        });
        app.handle_backend_event(BackendEvent::ToolResult {
            name: None,
            success: true,
            content: "second".to_string(),
        });
        app.handle_backend_event(BackendEvent::ToolResult {
            name: None,
            success: true,
            content: "first".to_string(),
        });

        let group = app.entries[0]
            .activity_group
            .as_ref()
            .expect("legacy activity group");
        assert!(group.synthetic);
        assert_eq!(group.tool_calls.len(), 2);
        assert_ne!(group.tool_calls[0].id, group.tool_calls[1].id);
        assert_eq!(group.tool_calls[0].result.as_deref(), Some("first"));
        assert_eq!(group.tool_calls[1].result.as_deref(), Some("second"));
    }

    #[test]
    fn backend_activity_ids_cannot_collide_with_synthetic_legacy_namespace() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::ToolUse {
            name: "run_command".to_string(),
            arguments: json!({ "command": "legacy" }),
        });
        let synthetic_id = app.entries[0]
            .activity_group
            .as_ref()
            .expect("synthetic group")
            .id
            .clone();

        app.handle_backend_event(BackendEvent::ActivityStart {
            id: synthetic_id.clone(),
            title: Some("Backend group".to_string()),
            kind: None,
        });
        app.handle_backend_event(BackendEvent::ActivityToolResult {
            activity_id: synthetic_id.clone(),
            id: Some("tool-1".to_string()),
            tool_name: Some("read_file".to_string()),
            success: true,
            content: "backend result".to_string(),
        });

        let groups = app
            .entries
            .iter()
            .filter_map(|entry| entry.activity_group.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].id, synthetic_id);
        assert_eq!(groups[1].id, format!("backend:{synthetic_id}"));
        assert!(!groups[1].synthetic);
        assert_eq!(
            groups[1].tool_calls[0].result.as_deref(),
            Some("backend result")
        );
    }

    #[test]
    fn legacy_tool_events_finish_as_collapsed_activity_before_final_answer() {
        let mut app = test_app();
        app.entries
            .push(Entry::plain(EntryRole::User, "inspect the TUI"));
        app.handle_backend_event(BackendEvent::WorkingNarrationDelta {
            text: "I'm locating the transcript components first.".to_string(),
            voiceover_suppressed: false,
        });
        app.handle_backend_event(BackendEvent::ToolUse {
            name: "read_file".to_string(),
            arguments: json!({ "path": "tui/src/app.rs" }),
        });
        app.handle_backend_event(BackendEvent::ToolResult {
            name: None,
            success: true,
            content: "fn rendered_transcript_lines() {}".to_string(),
        });
        app.handle_backend_event(BackendEvent::TranscriptPhaseBoundary(
            TranscriptPhaseBoundary::Summarizing,
        ));
        app.handle_backend_event(BackendEvent::CompletedSummary(
            "Worked this turn: 1 file read.".to_string(),
        ));
        app.handle_backend_event(BackendEvent::TranscriptPhaseBoundary(
            TranscriptPhaseBoundary::Finalizing,
        ));
        app.handle_backend_event(BackendEvent::FinalAnswerDelta(
            "The TUI has a normalized activity path now.".to_string(),
        ));
        app.handle_backend_event(BackendEvent::Done {
            model: None,
            iterations: Some(1),
            input_tokens: None,
            output_tokens: None,
        });

        assert!(matches!(app.entries[1].role, EntryRole::ActivityGroup));
        let group = app.entries[1]
            .activity_group
            .as_ref()
            .expect("synthetic activity group");
        assert!(group.synthetic);
        assert!(!group.is_live);
        assert_eq!(
            group.narration.as_deref(),
            Some("I'm locating the transcript components first.")
        );
        let group_id = group.id.clone();
        assert!(matches!(app.entries[2].role, EntryRole::CompletedSummary));
        assert_eq!(app.entries[2].text, "Worked this turn: 1 file read.");
        assert!(matches!(app.entries[3].role, EntryRole::FinalAnswer));

        let text = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(text.contains("work · ▸ Tool activity · 1 file read"));
        assert!(text.contains("read_file"));
        assert!(text.contains("tui/src/app.rs"));
        assert!(!text.contains("I'm locating the transcript components first."));
        assert!(!text.contains("fn rendered_transcript_lines()"));

        app.toggle_activity_group_by_id(&group_id);
        let expanded_text = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(expanded_text.contains("work · ▾ Tool activity · 1 file read"));
        assert!(expanded_text.contains("I'm locating the transcript components first."));
        assert!(expanded_text.contains("fn rendered_transcript_lines()"));
    }

    #[test]
    fn typed_activity_stream_renders_group_summary_then_final_answer() {
        let mut app = test_app();
        app.entries
            .push(Entry::plain(EntryRole::User, "inspect the TUI"));

        app.handle_backend_event(BackendEvent::WorkingNarrationDelta {
            text: "I'm locating the transcript code first.".to_string(),
            voiceover_suppressed: false,
        });
        app.handle_backend_event(BackendEvent::ActivityStart {
            id: "act-1".to_string(),
            title: Some("Read TUI files".to_string()),
            kind: None,
        });
        app.handle_backend_event(BackendEvent::ActivityToolCallStart {
            activity_id: "act-1".to_string(),
            id: Some("tool-1".to_string()),
            name: Some("read_file".to_string()),
        });
        app.handle_backend_event(BackendEvent::ActivityToolCallComplete {
            activity_id: "act-1".to_string(),
            id: Some("tool-1".to_string()),
            name: Some("read_file".to_string()),
            arguments: json!({ "path": "tui/src/app.rs" }),
        });
        app.handle_backend_event(BackendEvent::ActivityToolResult {
            activity_id: "act-1".to_string(),
            id: Some("tool-1".to_string()),
            tool_name: Some("read_file".to_string()),
            success: true,
            content: "fn rendered_transcript_lines() {}".to_string(),
        });
        app.handle_backend_event(BackendEvent::ActivityEnd {
            id: "act-1".to_string(),
        });
        app.handle_backend_event(BackendEvent::TranscriptPhaseBoundary(
            TranscriptPhaseBoundary::Summarizing,
        ));
        app.handle_backend_event(BackendEvent::CompletedSummary(
            "Worked this turn: 1 file read.".to_string(),
        ));
        app.handle_backend_event(BackendEvent::TranscriptPhaseBoundary(
            TranscriptPhaseBoundary::Finalizing,
        ));
        app.handle_backend_event(BackendEvent::FinalAnswerDelta(
            "The TUI has a typed activity path now.".to_string(),
        ));
        app.handle_backend_event(BackendEvent::Done {
            model: None,
            iterations: Some(1),
            input_tokens: None,
            output_tokens: None,
        });

        assert!(matches!(app.entries[1].role, EntryRole::ActivityGroup));
        assert!(matches!(app.entries[2].role, EntryRole::CompletedSummary));
        assert!(matches!(app.entries[3].role, EntryRole::FinalAnswer));

        let lines = app.rendered_transcript_lines(96);
        let text = rendered_text(&lines).join("\n");
        assert!(text.contains("work · ▸ Read TUI files"));
        assert!(text.contains("read_file"));
        assert!(text.contains("tui/src/app.rs"));
        assert!(!text.contains("I'm locating the transcript code first."));
        assert!(!text.contains("fn rendered_transcript_lines()"));
        assert!(text.contains("sum  › Worked this turn: 1 file read."));
        assert!(text.contains("done › The TUI has a typed activity path now."));

        app.toggle_activity_group_by_id("act-1");
        let expanded_text = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(expanded_text.contains("work · ▾ Read TUI files"));
        assert!(expanded_text.contains("I'm locating the transcript code first."));
        assert!(expanded_text.contains("✓ read_file"));
        assert!(expanded_text.contains("fn rendered_transcript_lines()"));
    }

    #[test]
    fn executing_tools_boundary_activates_activity_phase() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::TranscriptPhaseBoundary(
            TranscriptPhaseBoundary::ExecutingTools,
        ));

        assert_eq!(app.active_render_phase(), Some(TuiRenderPhase::Activity));

        app.handle_backend_event(BackendEvent::ActivityStart {
            id: "act-1".to_string(),
            title: Some("Inspect files".to_string()),
            kind: None,
        });

        assert!(matches!(
            app.entries.last().and_then(|entry| entry.render_phase),
            Some(TuiRenderPhase::Activity)
        ));
    }

    #[test]
    fn transcript_render_model_groups_entries_into_turn_phase_sections() {
        let entries = vec![
            Entry::plain(EntryRole::User, "inspect"),
            Entry::working_narration("planning".to_string())
                .with_render_phase(Some(TuiRenderPhase::Working)),
            Entry::activity_group(TuiActivityGroup::new(
                "act-1".to_string(),
                Some("Read files".to_string()),
                None,
                None,
            ))
            .with_render_phase(Some(TuiRenderPhase::Activity)),
            Entry::completed_summary("Worked this turn: 1 file read.".to_string())
                .with_render_phase(Some(TuiRenderPhase::Summary)),
            Entry::final_answer("Done.".to_string())
                .with_render_phase(Some(TuiRenderPhase::Response)),
        ];

        let model = TuiTranscriptRenderModel::reduce(&entries);

        assert_eq!(model.turns.len(), 1);
        let phases = model.turns[0]
            .sections
            .iter()
            .map(|section| section.phase)
            .collect::<Vec<_>>();
        assert_eq!(
            phases,
            vec![
                None,
                Some(TuiRenderPhase::Working),
                Some(TuiRenderPhase::Activity),
                Some(TuiRenderPhase::Summary),
                Some(TuiRenderPhase::Response),
            ]
        );
    }

    #[test]
    fn working_narration_after_activity_start_attaches_to_live_group() {
        let mut app = test_app();
        app.entries
            .push(Entry::plain(EntryRole::User, "inspect the TUI"));
        app.handle_backend_event(BackendEvent::ActivityStart {
            id: "act-1".to_string(),
            title: Some("Read TUI files".to_string()),
            kind: None,
        });
        app.handle_backend_event(BackendEvent::WorkingNarrationDelta {
            text: "I found the group; now I'm reading the reducer.".to_string(),
            voiceover_suppressed: false,
        });

        assert_eq!(app.entries.len(), 2);
        assert!(matches!(app.entries[1].role, EntryRole::ActivityGroup));
        let group = app.entries[1]
            .activity_group
            .as_ref()
            .expect("activity group");
        assert_eq!(
            group.narration.as_deref(),
            Some("I found the group; now I'm reading the reducer.")
        );
        assert!(
            !app.entries
                .iter()
                .any(|entry| matches!(entry.role, EntryRole::WorkingNarration)),
            "late narration should not drift away from the active tool group"
        );
    }

    #[test]
    fn completed_activity_group_toggles_details_from_header_click() {
        let mut app = test_app();
        let mut group = TuiActivityGroup::new(
            "act-1".to_string(),
            Some("Read TUI files".to_string()),
            None,
            Some("I checked the transcript reducer.".to_string()),
        );
        group.is_live = false;
        let mut call = TuiToolCall::new(Some("tool-1".to_string()), Some("read_file".to_string()));
        call.success = Some(true);
        call.result = Some("expanded evidence payload from the tool".to_string());
        group.tool_calls.push(call);
        app.push_entry_with_phase(Entry::activity_group(group), Some(TuiRenderPhase::Activity));
        app.transcript_area = Rect::new(0, 0, 100, 12);

        let collapsed = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(collapsed.contains("work · ▸ Read TUI files · 1 file read"));
        assert!(!collapsed.contains("expanded evidence payload"));

        let inner = transcript_inner_area(app.transcript_area);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: inner.x.saturating_add(1),
            row: inner.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        });

        let expanded = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(expanded.contains("work · ▾ Read TUI files · 1 file read"));
        assert!(expanded.contains("expanded evidence payload"));

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: inner.x.saturating_add(1),
            row: inner.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        });

        let collapsed_again = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(collapsed_again.contains("work · ▸ Read TUI files · 1 file read"));
        assert!(!collapsed_again.contains("expanded evidence payload"));
    }

    #[test]
    fn keyboard_focus_toggles_completed_activity_group_details() {
        let mut app = test_app();
        let mut group = TuiActivityGroup::new(
            "act-1".to_string(),
            Some("Read TUI files".to_string()),
            None,
            None,
        );
        group.is_live = false;
        let mut call = TuiToolCall::new(Some("tool-1".to_string()), Some("read_file".to_string()));
        call.success = Some(true);
        call.result = Some("expanded evidence payload from the tool".to_string());
        group.tool_calls.push(call);
        app.push_entry_with_phase(Entry::activity_group(group), Some(TuiRenderPhase::Activity));

        let collapsed = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(collapsed.contains("work · ▸ Read TUI files · 1 file read"));
        assert!(!collapsed.contains("expanded evidence payload"));

        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focused_activity_group_id.as_deref(), Some("act-1"));

        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let expanded = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(expanded.contains("work · ▾ Read TUI files · 1 file read"));
        assert!(expanded.contains("expanded evidence payload"));

        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let collapsed_again = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(collapsed_again.contains("work · ▸ Read TUI files · 1 file read"));
        assert!(!collapsed_again.contains("expanded evidence payload"));
    }

    #[test]
    fn collapsed_activity_headers_hide_raw_run_command_json() {
        let mut app = test_app();
        let mut group = TuiActivityGroup::new(
            "act-1".to_string(),
            Some("Ran 1 tool".to_string()),
            None,
            None,
        );
        group.is_live = false;
        let mut call =
            TuiToolCall::new(Some("tool-1".to_string()), Some("run_command".to_string()));
        call.arguments = Some(json!({
            "argv": [
                "gh",
                "pr",
                "comment",
                "1858",
                "--repo",
                "fawxai/fawx",
                "--body",
                "Review posted"
            ]
        }));
        call.progress = Some(TuiToolProgress {
            category: "mutation".to_string(),
            target: Some("run_command:{\"argv\":[\"gh\",\"pr\",\"comment\",\"1858\"]}".to_string()),
            advances_slot: None,
            outcome: "advanced".to_string(),
        });
        call.success = Some(true);
        group.tool_calls.push(call);
        app.push_entry_with_phase(Entry::activity_group(group), Some(TuiRenderPhase::Activity));

        let collapsed = rendered_text(&app.rendered_transcript_lines(120)).join("\n");

        assert!(collapsed.contains("gh pr comment 1858 --repo fawxai/fawx --body Review posted"));
        assert!(!collapsed.contains("{\"argv\""));
        assert!(!collapsed.contains("run_command:{"));
    }

    #[test]
    fn live_activity_group_header_surfaces_latest_tool_progress() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::ToolProgress {
            activity_id: Some("act-1".to_string()),
            id: Some("tool-1".to_string()),
            tool_name: Some("read_file".to_string()),
            category: "file".to_string(),
            target: Some("src/app.rs".to_string()),
            advances_slot: None,
            outcome: "reading".to_string(),
        });

        let text = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(text.contains("work · ◐ Working · reading src/app.rs"));
        assert!(text.contains("1 file read, 1 running"));
    }

    #[test]
    fn active_spinner_advances_during_live_activity() {
        let mut app = test_app();
        app.pending_request = true;
        app.handle_backend_event(BackendEvent::ActivityStart {
            id: "act-1".to_string(),
            title: None,
            kind: None,
        });

        let first = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(first.contains("work · ◐ Working"));

        for _ in 0..4 {
            app.advance_spinner();
        }

        let second = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(second.contains("work · ◓ Working"));
    }

    #[test]
    fn spinner_uses_ascii_fallback_for_limited_terminals() {
        assert_eq!(
            active_spinner_frames_for_env(None, Some("linux"), Some("en_US.UTF-8")),
            &ASCII_ACTIVE_FRAMES
        );
        assert_eq!(
            active_spinner_frames_for_env(None, Some("xterm-256color"), Some("C")),
            &ASCII_ACTIVE_FRAMES
        );
        assert_eq!(
            active_spinner_frames_for_env(Some("1"), Some("xterm-256color"), Some("en_US.UTF-8")),
            &ASCII_ACTIVE_FRAMES
        );
        assert_eq!(
            active_spinner_frames_for_env(None, Some("xterm-256color"), Some("en_US.UTF-8")),
            &UNICODE_ACTIVE_FRAMES
        );
    }

    #[test]
    fn expanded_activity_group_state_survives_recreated_entries() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::ActivityStart {
            id: "act-1".to_string(),
            title: Some("Read files".to_string()),
            kind: None,
        });
        app.handle_backend_event(BackendEvent::ActivityToolResult {
            activity_id: "act-1".to_string(),
            id: Some("tool-1".to_string()),
            tool_name: Some("read_file".to_string()),
            success: true,
            content: "first payload".to_string(),
        });
        app.handle_backend_event(BackendEvent::ActivityEnd {
            id: "act-1".to_string(),
        });
        app.toggle_activity_group_by_id("act-1");

        assert!(
            !app.entries[0]
                .activity_group
                .as_ref()
                .expect("activity group")
                .collapsed
        );

        app.entries.clear();
        app.handle_backend_event(BackendEvent::ActivityStart {
            id: "act-1".to_string(),
            title: Some("Read files".to_string()),
            kind: None,
        });
        app.handle_backend_event(BackendEvent::ActivityToolResult {
            activity_id: "act-1".to_string(),
            id: Some("tool-1".to_string()),
            tool_name: Some("read_file".to_string()),
            success: true,
            content: "rebuilt payload".to_string(),
        });
        app.handle_backend_event(BackendEvent::ActivityEnd {
            id: "act-1".to_string(),
        });

        let group = app.entries[0]
            .activity_group
            .as_ref()
            .expect("recreated activity group");
        assert!(!group.collapsed);
        let text = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(text.contains("rebuilt payload"));
    }

    #[test]
    fn transcript_renders_phase_separators_and_live_final_answer() {
        let mut app = test_app();
        app.entries.push(Entry::plain(EntryRole::User, "inspect"));
        app.handle_backend_event(BackendEvent::WorkingNarrationDelta {
            text: "I'm checking the reducer.".to_string(),
            voiceover_suppressed: false,
        });
        app.handle_backend_event(BackendEvent::ActivityStart {
            id: "act-1".to_string(),
            title: Some("Read TUI files".to_string()),
            kind: None,
        });
        app.handle_backend_event(BackendEvent::ActivityToolResult {
            activity_id: "act-1".to_string(),
            id: Some("tool-1".to_string()),
            tool_name: Some("read_file".to_string()),
            success: true,
            content: "payload".to_string(),
        });
        app.handle_backend_event(BackendEvent::ActivityEnd {
            id: "act-1".to_string(),
        });
        app.handle_backend_event(BackendEvent::TranscriptPhaseBoundary(
            TranscriptPhaseBoundary::Summarizing,
        ));
        app.handle_backend_event(BackendEvent::CompletedSummary(
            "Worked this turn: 1 file read.".to_string(),
        ));
        app.handle_backend_event(BackendEvent::TranscriptPhaseBoundary(
            TranscriptPhaseBoundary::Finalizing,
        ));
        app.handle_backend_event(BackendEvent::FinalAnswerDelta("Here is ".to_string()));

        let streaming = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(streaming.contains("╌╌ Activity"));
        assert!(streaming.contains("━━ Completed work"));
        assert!(streaming.contains("══ Response"));
        assert!(!streaming.contains("I'm checking the reducer."));
        assert!(streaming.contains("done › Here is"));

        app.toggle_activity_group_by_id("act-1");
        let expanded_streaming = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(expanded_streaming.contains("I'm checking the reducer."));

        app.handle_backend_event(BackendEvent::FinalAnswerDelta("the answer.".to_string()));
        let updated = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(updated.contains("done › Here is the answer."));
    }

    #[test]
    fn working_narration_without_activity_stays_muted_and_separate() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::WorkingNarrationDelta {
            text: "I'm thinking through the request.".to_string(),
            voiceover_suppressed: false,
        });

        assert!(matches!(app.entries[0].role, EntryRole::WorkingNarration));
        let text = rendered_text(&app.rendered_transcript_lines(80)).join("\n");
        assert!(text.contains("work › I'm thinking through the request."));
    }

    #[test]
    fn suppressed_working_narration_is_not_preserved_as_voiceover() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::WorkingNarrationDelta {
            text: "I'm reading app.".to_string(),
            voiceover_suppressed: true,
        });

        assert!(
            app.entries.is_empty(),
            "tool-progress narration should be represented by the tool card, not duplicated"
        );
    }

    #[test]
    fn text_preview_streams_as_candidate_answer_until_reset() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::TextPreviewDelta(
            "Current Architecture Assessment".to_string(),
        ));

        let text = rendered_text(&app.rendered_transcript_lines(96)).join("\n");
        assert!(text.contains("fawx › Current Architecture Assessment"));
        assert!(app.entries.is_empty());
    }

    #[test]
    fn text_reset_demotes_preview_to_working_narration() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::TextPreviewDelta(
            "I'm locating the transcript components first.".to_string(),
        ));
        app.handle_backend_event(BackendEvent::TextReset);

        assert_eq!(app.entries.len(), 1);
        assert!(matches!(app.entries[0].role, EntryRole::WorkingNarration));
        assert_eq!(
            app.entries[0].text,
            "I'm locating the transcript components first."
        );
        assert_eq!(app.streaming_text.as_deref(), Some(""));
    }

    #[test]
    fn text_reset_does_not_remove_committed_working_narration() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::WorkingNarrationDelta {
            text: "I'm checking the reducer.".to_string(),
            voiceover_suppressed: false,
        });
        app.handle_backend_event(BackendEvent::TextReset);

        assert_eq!(app.entries.len(), 1);
        assert!(matches!(app.entries[0].role, EntryRole::WorkingNarration));
        assert_eq!(app.entries[0].text, "I'm checking the reducer.");
    }

    #[test]
    fn final_answer_delta_replaces_already_rendered_preview() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::TextPreviewDelta("Done.".to_string()));
        app.handle_backend_event(BackendEvent::FinalAnswerDelta("Done.".to_string()));
        app.handle_backend_event(BackendEvent::Done {
            model: None,
            iterations: Some(1),
            input_tokens: None,
            output_tokens: None,
        });

        assert_eq!(app.entries.len(), 1);
        assert!(matches!(app.entries[0].role, EntryRole::FinalAnswer));
        assert_eq!(app.entries[0].text, "Done.");
    }

    #[test]
    fn transcript_selection_highlights_rendered_spans() {
        let mut lines = vec![Line::from("abcdef")];
        apply_transcript_selection(
            &mut lines,
            Some(TranscriptSelection {
                anchor: TranscriptPoint { line: 0, column: 1 },
                focus: TranscriptPoint { line: 0, column: 4 },
                dragging: false,
            }),
        );

        assert_eq!(
            lines[0]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>(),
            vec!["a", "bcd", "ef"]
        );
        assert_eq!(lines[0].spans[1].style.bg, Some(Color::White));
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Black));
    }

    #[test]
    fn mouse_drag_selects_visible_transcript_text() {
        let mut app = test_app();
        app.entries
            .push(Entry::plain(EntryRole::System, "hello world"));
        app.transcript_area = Rect::new(0, 0, 40, 6);
        let inner = transcript_inner_area(app.transcript_area);

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: inner.x + 7,
            row: inner.y,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: inner.x + 12,
            row: inner.y,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: inner.x + 12,
            row: inner.y,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(app.selected_transcript_text().as_deref(), Some("hello"));
    }

    #[test]
    fn esc_clears_transcript_selection_before_quitting() {
        let mut app = test_app();
        app.transcript_selection = Some(TranscriptSelection {
            anchor: TranscriptPoint { line: 0, column: 0 },
            focus: TranscriptPoint { line: 0, column: 1 },
            dragging: false,
        });

        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.transcript_selection.is_none());
        assert!(!app.should_quit);

        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn selected_text_from_lines_preserves_multiline_selection() {
        let lines = vec![
            Line::from("alpha"),
            Line::from("bravo"),
            Line::from("charlie"),
        ];
        let selected = selected_text_from_lines(
            &lines,
            TranscriptSelectionRange {
                start: TranscriptPoint { line: 0, column: 2 },
                end: TranscriptPoint { line: 2, column: 4 },
            },
        );

        assert_eq!(selected.as_deref(), Some("pha\nbravo\nchar"));
    }

    #[test]
    fn url_at_display_column_finds_visible_http_link() {
        let line = "See docs (https://example.com/path).";

        assert_eq!(
            url_at_display_column(line, 14).as_deref(),
            Some("https://example.com/path")
        );
        assert_eq!(url_at_display_column(line, 3), None);
    }

    #[test]
    fn osc52_sequence_encodes_selected_text_for_terminal_clipboard() {
        assert_eq!(osc52_sequence("hello", false), "\u{1b}]52;c;aGVsbG8=\u{7}");
        assert_eq!(
            osc52_sequence("hello", true),
            "\u{1b}Ptmux;\u{1b}\u{1b}]52;c;aGVsbG8=\u{7}\u{1b}\\"
        );
    }

    #[test]
    fn text_reset_during_final_answer_preserves_existing_output() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::WorkingNarrationDelta {
            text: "I checked the relevant files.".to_string(),
            voiceover_suppressed: false,
        });
        app.handle_backend_event(BackendEvent::FinalAnswerDelta(
            "Here is the final answer.".to_string(),
        ));
        app.handle_backend_event(BackendEvent::TextReset);
        app.handle_backend_event(BackendEvent::Done {
            model: None,
            iterations: Some(1),
            input_tokens: None,
            output_tokens: None,
        });

        assert!(matches!(app.entries[0].role, EntryRole::WorkingNarration));
        assert_eq!(app.entries[0].text, "I checked the relevant files.");
        assert!(matches!(app.entries[1].role, EntryRole::FinalAnswer));
        assert_eq!(app.entries[1].text, "Here is the final answer.");
    }

    #[test]
    fn stream_error_preserves_committed_working_narration() {
        let mut app = test_app();
        app.pending_request = true;
        app.streaming_text = Some(String::new());
        app.handle_backend_event(BackendEvent::WorkingNarrationDelta {
            text: "I started checking the workspace.".to_string(),
            voiceover_suppressed: false,
        });
        app.handle_backend_event(BackendEvent::StreamError("cancelled".to_string()));

        assert!(matches!(app.entries[0].role, EntryRole::WorkingNarration));
        assert_eq!(app.entries[0].text, "I started checking the workspace.");
        assert!(matches!(app.entries[1].role, EntryRole::Error));
        assert!(!app.pending_request);
        assert!(app.final_answer_text.is_none());
    }

    #[test]
    fn orphan_activity_end_is_ignored_without_creating_a_group() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::ActivityEnd {
            id: "missing-activity".to_string(),
        });

        assert!(app.entries.is_empty());
    }

    #[test]
    fn orphan_activity_tool_result_creates_defensive_group() {
        let mut app = test_app();
        app.handle_backend_event(BackendEvent::ActivityToolResult {
            activity_id: "late-activity".to_string(),
            id: Some("tool-1".to_string()),
            tool_name: Some("run_command".to_string()),
            success: true,
            content: "done".to_string(),
        });

        let group = app.entries[0]
            .activity_group
            .as_ref()
            .expect("late tool result should create a defensive activity group");
        assert_eq!(group.id, "late-activity");
        assert_eq!(group.tool_calls.len(), 1);
        assert_eq!(group.tool_calls[0].name, "run_command");
        assert_eq!(group.tool_calls[0].success, Some(true));
    }

    #[test]
    fn transcript_render_buffer_is_bounded() {
        let mut app = test_app();
        for index in 0..(MAX_TRANSCRIPT_ENTRIES + 12) {
            app.push_system(format!("entry {index}"));
        }

        assert_eq!(app.entries.len(), MAX_TRANSCRIPT_ENTRIES);
        assert_eq!(app.entries[0].text, "entry 12");
        let expected_last = format!("entry {}", MAX_TRANSCRIPT_ENTRIES + 11);
        assert_eq!(
            app.entries.last().map(|entry| entry.text.as_str()),
            Some(expected_last.as_str())
        );
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
    fn bracketed_multiline_paste_appends_to_input_without_submitting() {
        let mut app = test_app();

        app.handle_terminal_event(CEvent::Paste(
            "first line\r\nsecond line\nthird line".to_string(),
        ));

        assert_eq!(app.input, "first line\nsecond line\nthird line");
        assert!(app.entries.is_empty());
        assert!(!app.pending_request);
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
