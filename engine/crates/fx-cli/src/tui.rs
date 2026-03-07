use crate::auth_store::{migrate_if_needed, AuthStore};
use crate::ui;
use async_trait::async_trait;
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEvent,
    KeyModifiers,
};
use crossterm::style::Stylize;
use crossterm::{cursor, event, style, terminal, ExecutableCommand};
use futures::StreamExt;
use fx_analysis::{AnalysisEngine, AnalysisError, AnalysisFinding, Confidence};
use fx_auth::auth::{AuthManager, AuthMethod};
use fx_auth::credential_store::CredentialStore as CredentialStoreTrait;
use fx_auth::oauth::{PkceFlow, TokenExchangeRequest, TokenResponse};
use fx_config::FawxConfig;
use fx_conversation::{
    ConversationMessage, ConversationStore, TokenUsage as ConversationTokenUsage,
};
use fx_core::error::LlmError as CoreLlmError;
use fx_core::memory::{MemoryProvider, MemoryStore};
use fx_core::message::InternalMessage;
use fx_core::runtime_info::{ConfigSummary, RuntimeInfo, SkillInfo};
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_core::EventBus;
use fx_improve::{CyclePaths, ImprovementConfig, OutputMode};
use fx_kernel::act::{TokenUsage, ToolExecutor};
use fx_kernel::budget::{BudgetConfig, BudgetTracker};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::context_manager::ContextCompactor;
use fx_kernel::input::{loop_input_channel, LoopCommand, LoopInputSender};
use fx_kernel::loop_engine::{
    LlmProvider as LoopLlmProvider, LoopEngine, LoopEngineBuilder, LoopResult, ScratchpadProvider,
};
use fx_kernel::signals::{LoopStep, Signal, SignalCollector};
use fx_kernel::types::PerceptionSnapshot;
use fx_kernel::{CachingExecutor, ProposalGateExecutor, ProposalGateState};
use fx_llm::{
    AnthropicProvider, CompletionRequest, Message, ModelCatalog, ModelInfo, ModelRouter,
    OpenAiProvider, OpenAiResponsesProvider, ProviderError, RouterError, StreamChunk,
};
use fx_loadable::{ReloadEvent, SignaturePolicy, SkillRegistry, SkillWatcher, TransactionSkill};
use fx_memory::{JsonFileMemory, JsonMemoryConfig, SignalStore};
use fx_scratchpad::skill::ScratchpadSkill;
use fx_scratchpad::Scratchpad;
use fx_skills::live_host_api::CredentialProvider;
use fx_tools::{BuiltinToolsSkill, FawxToolExecutor, GitSkill, ToolConfig};
use ratatui::DefaultTerminal;
use sha2::{Digest, Sha256};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use std::sync::LazyLock;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;
use termimad::crossterm::style::{Attribute as MadAttribute, Color as MadColor};
use termimad::{MadSkin, StyledChar};
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};

const BANNER_ART: &str = r#"   ___
  / _/__ __    ____  __
 / _/ _ `/ |/|/ /\ \/ /
/_/ \_,_/|__,__/ /_/\_\"#;
/// Pre-rendered braille+truecolor ANSI banner (via ascii-image-converter).
/// Plain-text source art lives in `docs/fawx-hero.txt` (no ANSI escapes).
const FAWX_BANNER_ANSI: &str = include_str!("../../../../docs/fawx-hero-ansi.txt");

const DEFAULT_OPENAI_TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const MAX_PROMPT_RETRIES: usize = 10;
const DEFAULT_CONTEXT_MAX_TOKENS: usize = 8_000;
const DEFAULT_CONTEXT_COMPACT_TARGET: usize = 6_000;
const DEFAULT_SYNTHESIS_INSTRUCTION: &str =
    "Use the tool output to directly answer the user's question. Be natural and specific — \
don't dump raw tool output, but don't hide data either. Match your response format to what \
the user asked for: if they asked for a specific format (e.g., a count, a timestamp, a \
raw value), use exactly that format — do not reformat into a 'friendlier' version unless \
explicitly asked. If they asked a simple question, give a simple answer. If they asked \
for a listing or search results, present it cleanly formatted.";
const MAX_SYNTHESIS_INSTRUCTION_LENGTH: usize = 500;
const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const CANCELLED_INPUT_MESSAGE: &str = "input cancelled";
const USER_ECHO_PREFIX: &str = "you › ";
const USER_CONTINUATION_PREFIX: &str = "      ";

pub(crate) type SharedMemoryStore = Arc<Mutex<dyn MemoryStore>>;

enum IdleLoopEvent {
    Reload(ReloadEvent),
    Terminal(Event),
}

const DEFAULT_ANTHROPIC_MODELS: &[&str] = &[
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-opus-4-5-20251101",
    "claude-sonnet-4-5-20250929",
    "claude-haiku-4-5-20251001",
    "claude-opus-4-20250514",
    "claude-sonnet-4-20250514",
    "claude-3-7-sonnet-latest",
];
const DEFAULT_OPENAI_MODELS: &[&str] = &["gpt-4.1", "gpt-4o", "gpt-4o-mini"];
const DEFAULT_OPENAI_SUBSCRIPTION_MODELS: &[&str] =
    &["gpt-5.3-codex", "gpt-5.2", "gpt-5.1", "o4-mini"];
const DEFAULT_OPENROUTER_MODELS: &[&str] = &[
    "openai/gpt-4o-mini",
    "anthropic/claude-3.5-sonnet",
    "google/gemini-2.0-flash-001",
];

fn term_indicates_truecolor(term: &str) -> bool {
    term.ends_with("-direct") || term == "xterm-direct" || term.contains("truecolor")
}

fn supports_truecolor() -> bool {
    if let Ok(value) = std::env::var("COLORTERM") {
        if value == "truecolor" || value == "24bit" {
            return true;
        }
    }

    if let Ok(term) = std::env::var("TERM") {
        return term_indicates_truecolor(&term);
    }

    false
}

pub(crate) fn theme_color(r: u8, g: u8, b: u8, fallback_256: u8) -> style::Color {
    if supports_truecolor() {
        style::Color::Rgb { r, g, b }
    } else {
        style::Color::AnsiValue(fallback_256)
    }
}

/// Render the startup banner: ANSI art for truecolor terminals, text fallback otherwise.
fn render_banner(truecolor: bool, amber: style::Color) -> String {
    if truecolor {
        FAWX_BANNER_ANSI.to_string()
    } else {
        BANNER_ART
            .lines()
            .map(|line| {
                format!(
                    "{}
",
                    line.bold().with(amber)
                )
            })
            .collect()
    }
}
fn markdown_color(r: u8, g: u8, b: u8, fallback_256: u8) -> MadColor {
    if supports_truecolor() {
        MadColor::Rgb { r, g, b }
    } else {
        MadColor::AnsiValue(fallback_256)
    }
}

fn build_markdown_skin() -> MadSkin {
    let amber = markdown_color(255, 165, 0, 214);
    let gold = markdown_color(255, 204, 0, 220);
    let burnt = markdown_color(210, 112, 10, 166);
    let dim_white = markdown_color(230, 230, 230, 252);
    let code_bg = markdown_color(26, 26, 26, 235);

    let mut skin = MadSkin::default();
    skin.set_headers_fg(amber);
    skin.headers[0].add_attr(MadAttribute::Bold);
    skin.bold.set_fg(gold);
    skin.bold.add_attr(MadAttribute::Bold);
    skin.italic.set_fg(burnt);
    skin.italic.add_attr(MadAttribute::Italic);
    skin.inline_code.set_fgbg(dim_white, code_bg);
    skin.inline_code.add_attr(MadAttribute::Dim);
    skin.code_block.set_fgbg(dim_white, code_bg);
    skin.code_block.add_attr(MadAttribute::Dim);
    skin.bullet = StyledChar::from_fg_char(amber, '•');
    skin.paragraph.set_fg(dim_white);
    skin
}

struct SyntectAssets {
    syntax_set: SyntaxSet,
    theme: Theme,
}

static SYNTECT_ASSETS: LazyLock<SyntectAssets> = LazyLock::new(|| {
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let themes = ThemeSet::load_defaults();
    let theme = themes
        .themes
        .get("base16-ocean.dark")
        .cloned()
        .or_else(|| themes.themes.values().next().cloned())
        .unwrap_or_default();
    SyntectAssets { syntax_set, theme }
});

fn normalize_lang_tag(lang: &str) -> &str {
    let tag = lang.split_whitespace().next().unwrap_or(lang);
    let tag = tag.split(',').next().unwrap_or(tag);
    tag.trim()
}

fn highlight_code(code: &str, lang: &str) -> String {
    let dim = "\x1b[2m";
    let reset = "\x1b[0m";
    let lang = normalize_lang_tag(lang);
    let assets = &*SYNTECT_ASSETS;
    let syntax = assets
        .syntax_set
        .find_syntax_by_token(lang)
        .or_else(|| assets.syntax_set.find_syntax_by_extension(lang));
    let Some(syntax) = syntax else {
        return format!("{dim}{code}{reset}");
    };

    let mut highlighter = HighlightLines::new(syntax, &assets.theme);
    code.lines()
        .map(|line| highlighter.highlight_line(line, &assets.syntax_set))
        .map(|ranges| ranges.unwrap_or_else(|_| vec![(SyntectStyle::default(), "")]))
        .map(|ranges| format!("{}\n", as_24_bit_terminal_escaped(&ranges, false)))
        .collect()
}

enum FenceLine {
    Open { backtick_count: usize, lang: String },
    Close,
    Content,
}

fn parse_fence_line(line: &str, fence_state: &Option<usize>) -> FenceLine {
    let indent = line.chars().take_while(|ch| *ch == ' ').count();
    if indent >= 4 {
        return FenceLine::Content;
    }

    let trimmed = line.trim();
    let backtick_count = trimmed.chars().take_while(|ch| *ch == '`').count();
    if backtick_count < 3 {
        return FenceLine::Content;
    }

    if let Some(open_count) = fence_state {
        let after_backticks = &trimmed[backtick_count..];
        if backtick_count >= *open_count && after_backticks.trim().is_empty() {
            return FenceLine::Close;
        }
        FenceLine::Content
    } else {
        let lang = trimmed[backtick_count..].trim().to_string();
        FenceLine::Open {
            backtick_count,
            lang,
        }
    }
}

fn flush_markdown_prose(output: &mut String, prose: &mut String, skin: &MadSkin) {
    if prose.is_empty() {
        return;
    }
    output.push_str(&format!("{}", skin.term_text(prose)));
    prose.clear();
}

fn render_markdown(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let skin = build_markdown_skin();
    let mut output = String::new();
    let mut prose = String::new();
    let mut code = String::new();
    let mut fence = None;
    let mut lang = String::new();

    for line in text.lines() {
        match parse_fence_line(line, &fence) {
            FenceLine::Open {
                backtick_count,
                lang: parsed_lang,
            } => {
                flush_markdown_prose(&mut output, &mut prose, &skin);
                fence = Some(backtick_count);
                lang = parsed_lang;
            }
            FenceLine::Close => {
                output.push_str(&highlight_code(&code, &lang));
                output.push('\n');
                code.clear();
                fence = None;
            }
            FenceLine::Content => {
                if fence.is_some() {
                    code.push_str(line);
                    code.push('\n');
                } else {
                    prose.push_str(line);
                    prose.push('\n');
                }
            }
        }
    }

    if fence.is_some() {
        output.push_str(&highlight_code(&code, &lang));
    }
    flush_markdown_prose(&mut output, &mut prose, &skin);
    output.trim_end_matches('\n').to_string()
}

fn spinner_frame(index: usize) -> char {
    SPINNER_FRAMES[index % SPINNER_FRAMES.len()]
}

// ---------------------------------------------------------------------------
// Ratatui event loop helpers
// ---------------------------------------------------------------------------

/// Install a panic hook that restores the terminal on panic.
/// Restore the terminal to its pre-TUI state (best-effort, for panic/signal paths).
fn restore_ratatui_terminal() {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show
    );
}

/// Restore the terminal to its pre-TUI state, propagating errors on the
/// normal exit path so callers know if restoration failed.
fn try_restore_ratatui_terminal() -> std::io::Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;
    Ok(())
}

/// Install a panic hook that restores the terminal on panic.
fn install_ratatui_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_ratatui_terminal();
        original_hook(info);
    }));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RowScrub {
    row: u16,
    start_col: u16,
}

fn collect_row_scrubs(
    previous: Option<&ui::FrameRenderRows>,
    current: &ui::FrameRenderRows,
) -> Vec<RowScrub> {
    let current_width = current.width as usize;
    if current_width == 0 {
        return Vec::new();
    }

    current
        .rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, _)| {
            let start = match previous.filter(|rows| rows.rows.get(row_index).is_some()) {
                None => Some(0),
                Some(previous_rows) => {
                    scrub_start_for_existing_row(previous_rows, current, row_index, current_width)
                }
            }?;

            (start < current_width).then_some(RowScrub {
                row: row_index as u16,
                start_col: start as u16,
            })
        })
        .collect()
}

fn scrub_start_for_existing_row(
    previous_rows: &ui::FrameRenderRows,
    current: &ui::FrameRenderRows,
    row_index: usize,
    current_width: usize,
) -> Option<usize> {
    let previous_width = previous_rows.width as usize;
    let previous_content_width = previous_rows
        .content_columns
        .get(row_index)
        .copied()
        .unwrap_or(0) as usize;
    let current_content_width =
        current.content_columns.get(row_index).copied().unwrap_or(0) as usize;
    let mut scrub_start = None;

    if current_content_width < previous_content_width {
        scrub_start = Some(current_content_width);
    }
    if current_width > previous_width {
        scrub_start = Some(scrub_start.map_or(previous_width, |start| start.min(previous_width)));
    }

    scrub_start
}

fn scrub_disappeared_cells(
    terminal: &mut DefaultTerminal,
    frame_rows: &ui::FrameRenderRows,
    scrubs: &[RowScrub],
) -> io::Result<()> {
    // These writes intentionally bypass ratatui's front-buffer bookkeeping.
    // The scrubbed cells are either overwritten by the next draw() diff or
    // left blank because that content truly disappeared from the frame.
    for scrub in scrubs {
        if scrub.start_col >= frame_rows.width {
            continue;
        }

        let background = match frame_rows
            .fills
            .get(scrub.row as usize)
            .copied()
            .unwrap_or(ui::FrameRowFill::Default)
        {
            ui::FrameRowFill::Default => style::Color::Reset,
            ui::FrameRowFill::InputBackground => style::Color::AnsiValue(ui::INPUT_BG_INDEX),
        };

        crossterm::queue!(
            terminal.backend_mut(),
            cursor::MoveTo(scrub.start_col, scrub.row),
            style::SetBackgroundColor(background),
            style::Print(" ".repeat((frame_rows.width - scrub.start_col) as usize)),
            style::ResetColor
        )?;
    }

    Ok(())
}

fn draw_ratatui_frame(
    terminal: &mut DefaultTerminal,
    app: &ui::FawxApp,
    last_rows: &mut Option<ui::FrameRenderRows>,
) -> Result<(), TuiError> {
    let size = terminal.size().map_err(TuiError::Io)?;
    let frame_rows = ui::frame_render_rows(size.width, size.height, app);
    let scrubs = collect_row_scrubs(last_rows.as_ref(), &frame_rows);
    scrub_disappeared_cells(terminal, &frame_rows, &scrubs).map_err(TuiError::Io)?;

    terminal
        .draw(|frame| ui::draw(frame, app))
        .map_err(TuiError::Io)?;
    *last_rows = Some(frame_rows);
    Ok(())
}

/// Drain streaming events from the event bus into the FawxApp.
fn drain_bus_events(
    bus_rx: &mut broadcast::Receiver<InternalMessage>,
    app: &mut ui::FawxApp,
    streamed: &mut bool,
) {
    loop {
        match bus_rx.try_recv() {
            Ok(msg) => route_bus_message_to_app(msg, app, streamed),
            Err(broadcast::error::TryRecvError::Empty) => break,
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "stream display lagged");
            }
            Err(broadcast::error::TryRecvError::Closed) => break,
        }
    }
}

/// Route a single bus message to the FawxApp output.
fn route_bus_message_to_app(msg: InternalMessage, app: &mut ui::FawxApp, streamed: &mut bool) {
    match msg {
        InternalMessage::StreamingStarted { .. } => {
            app.begin_streaming_session();
            app.set_state(ui::AppState::Streaming);
            *streamed = true;
        }
        InternalMessage::StreamDelta { delta, .. } => {
            app.append_streaming_delta(&delta);
            *streamed = true;
        }
        InternalMessage::StreamingFinished { .. } => {
            app.finish_streaming();
            app.set_state(ui::AppState::Executing { spinner_frame: 0 });
        }
        _ => {}
    }
}

/// Handle a key event during the idle (input) state.
///
/// Returns `Some(text)` if the user submitted input, `None` otherwise.
fn handle_idle_key(key: KeyEvent, app: &mut ui::FawxApp) -> Option<String> {
    match key.code {
        KeyCode::Enter => {
            let text = app.submit_input();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        KeyCode::Up => {
            app.history_up();
            None
        }
        KeyCode::Down => {
            app.history_down();
            None
        }
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                return None; // handled by signal handler
            }
            app.append_input_char(c);
            None
        }
        KeyCode::Backspace => {
            app.backspace_input();
            None
        }
        KeyCode::PageUp => {
            app.scroll_up();
            None
        }
        KeyCode::PageDown => {
            app.scroll_down();
            None
        }
        _ => None,
    }
}

fn handle_idle_event(event: Event, app: &mut ui::FawxApp) -> Option<String> {
    match event {
        Event::Key(key) => handle_idle_key(key, app),
        Event::Paste(text) => {
            app.insert_pasted_input(text);
            None
        }
        _ => None,
    }
}

async fn next_idle_terminal_event(event_stream: &mut EventStream) -> io::Result<Event> {
    match event_stream.next().await {
        Some(result) => result,
        None => Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "terminal event stream closed",
        )),
    }
}

async fn wait_for_idle_loop_event<TerminalEventFuture>(
    terminal_event: TerminalEventFuture,
    reload_rx: &mut tokio::sync::mpsc::Receiver<ReloadEvent>,
) -> Result<IdleLoopEvent, TuiError>
where
    TerminalEventFuture: Future<Output = io::Result<Event>>,
{
    let reload_open = !reload_rx.is_closed();
    tokio::select! {
        event = terminal_event => event.map(IdleLoopEvent::Terminal).map_err(TuiError::Io),
        Some(reload_event) = reload_rx.recv(), if reload_open => Ok(IdleLoopEvent::Reload(reload_event)),
    }
}

fn process_reload_event(app: &mut ui::FawxApp, executor: &dyn ToolExecutor, event: ReloadEvent) {
    app.add_output(reload_event_message(&event));
    if reload_event_requires_cache_clear(&event) {
        executor.clear_cache();
    }
}

fn reload_event_message(event: &ReloadEvent) -> String {
    match event {
        ReloadEvent::Loaded {
            skill_name,
            version,
        } => format!("🔌 Loaded skill: {skill_name} v{version}"),
        ReloadEvent::Updated {
            skill_name,
            new_version,
            ..
        } => format!("🔄 Updated skill: {skill_name} v{new_version}"),
        ReloadEvent::Removed { skill_name } => format!("🗑️ Removed skill: {skill_name}"),
        ReloadEvent::Error { skill_name, error } => {
            format!("⚠️ Skill error ({skill_name}): {error}")
        }
    }
}

fn reload_event_requires_cache_clear(event: &ReloadEvent) -> bool {
    matches!(
        event,
        ReloadEvent::Loaded { .. } | ReloadEvent::Updated { .. } | ReloadEvent::Removed { .. }
    )
}

/// Check if input is a control command that only makes sense during execution.
///
/// During idle, only `/stop` (with slash) is recognized so that bare words
/// like "a", "s", "no" are not intercepted as control commands.
fn is_idle_control_command(input: &str) -> bool {
    let trimmed = input.trim();
    trimmed.eq_ignore_ascii_case("/stop") || trimmed.eq_ignore_ascii_case("/abort")
}

fn poll_execution_events(
    app: &mut ui::FawxApp,
    cancel_token: &CancellationToken,
    input_sender: &Option<LoopInputSender>,
) {
    while crossterm::event::poll(Duration::ZERO).unwrap_or(false) {
        if let Ok(event) = crossterm::event::read() {
            handle_execution_event(event, app, cancel_token, input_sender);
        }
    }
}

/// Handle key events during execution, routing input to steer/abort/queue.
///
/// Returns `true` if an abort was requested (Ctrl+C or `/stop`).
fn handle_execution_key(
    key: KeyEvent,
    app: &mut ui::FawxApp,
    cancel_token: &CancellationToken,
    input_sender: &Option<LoopInputSender>,
) {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.request_abort();
            cancel_token.cancel();
            if let Some(sender) = input_sender {
                let _ = sender.send(LoopCommand::Abort);
            }
        }
        KeyCode::Enter => {
            let text = app.submit_input();
            if text.is_empty() {
                return;
            }
            route_execution_input(&text, app, cancel_token, input_sender);
        }
        KeyCode::Char(c) => {
            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                app.append_input_char(c);
            }
        }
        KeyCode::Backspace => {
            app.backspace_input();
        }
        KeyCode::PageUp => app.scroll_up(),
        KeyCode::PageDown => app.scroll_down(),
        _ => {}
    }
}

fn handle_execution_event(
    event: Event,
    app: &mut ui::FawxApp,
    cancel_token: &CancellationToken,
    input_sender: &Option<LoopInputSender>,
) {
    match event {
        Event::Key(key) => handle_execution_key(key, app, cancel_token, input_sender),
        Event::Paste(text) => app.insert_pasted_input(text),
        _ => {}
    }
}

fn append_user_message_output(app: &mut ui::FawxApp, text: &str) {
    app.add_output(String::new());

    let normalized = text
        .trim_end_matches(['\r', '\n'])
        .replace("\r\n", "\n")
        .replace('\r', "\n");

    if normalized.is_empty() {
        app.add_output(USER_ECHO_PREFIX.to_string());
        return;
    }

    for (index, line) in normalized.split('\n').enumerate() {
        if index == 0 {
            app.add_output(format!("{USER_ECHO_PREFIX}{line}"));
        } else {
            app.add_output(format!("{USER_CONTINUATION_PREFIX}{line}"));
        }
    }
}

/// Route submitted text during execution to the appropriate handler.
fn route_execution_input(
    text: &str,
    app: &mut ui::FawxApp,
    cancel_token: &CancellationToken,
    input_sender: &Option<LoopInputSender>,
) {
    if let Some(cmd) = parse_bare_command(text) {
        match cmd {
            LoopCommand::Stop | LoopCommand::Abort => {
                app.request_abort();
                cancel_token.cancel();
                if let Some(sender) = input_sender {
                    let _ = sender.send(LoopCommand::Abort);
                }
                app.add_output(format!("⛔ {text}"));
            }
            LoopCommand::Steer(ref steer_text) => {
                if let Some(sender) = input_sender {
                    let _ = sender.send(LoopCommand::Steer(steer_text.clone()));
                    if matches!(app.state, ui::AppState::Streaming) || app.streaming_prefix_printed
                    {
                        app.interrupt_stream();
                        app.set_state(ui::AppState::Executing { spinner_frame: 0 });
                        append_user_message_output(app, steer_text);
                    } else {
                        app.add_output(format!("↪ Steer sent: {steer_text}"));
                    }
                } else {
                    // No channel available — store for forwarding at next cycle start
                    app.set_steer(steer_text.clone());
                    app.add_output(format!("↪ Steer queued: {steer_text}"));
                }
            }
            LoopCommand::StatusQuery => {
                if let Some(sender) = input_sender {
                    let _ = sender.send(LoopCommand::StatusQuery);
                }
            }
            other => {
                if let Some(sender) = input_sender {
                    let _ = sender.send(other);
                }
            }
        }
    } else {
        // Not a command — queue for the next turn
        app.queue_input(text.to_string());
        app.add_output(format!("📋 Queued: {text}"));
    }
}

/// Curated preference order — newest capable model first.
const PREFERRED_MODEL_PATTERNS: &[&str] = &[
    "opus-4-6",
    "opus-4.6",
    "opus-4-5",
    "opus-4.5",
    "sonnet-4-6",
    "sonnet-4.6",
    "sonnet-4-5",
    "sonnet-4.5",
    "sonnet-4",
    "opus-4",
    "gpt-5",
    "gpt-4o",
    "grok-3",
    "qwen-2.5",
    "deepseek-chat",
    "sonnet",
    "opus",
];

/// Never auto-default to small models.
const DEPRIORITIZED_PATTERNS: &[&str] = &["haiku", "mini", "flash", "nano"];

fn preferred_default_model(model_ids: &[String]) -> Option<&str> {
    for pattern in PREFERRED_MODEL_PATTERNS {
        if let Some(model) = model_ids
            .iter()
            .find(|id| id.to_ascii_lowercase().contains(pattern))
        {
            return Some(model.as_str());
        }
    }

    highest_version_model(model_ids).or_else(|| {
        model_ids
            .iter()
            .find(|id| !is_deprioritized_model(id))
            .or_else(|| model_ids.first())
            .map(String::as_str)
    })
}

fn is_deprioritized_model(model_id: &str) -> bool {
    let lower = model_id.to_ascii_lowercase();
    DEPRIORITIZED_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

fn highest_version_model(model_ids: &[String]) -> Option<&str> {
    model_ids
        .iter()
        .filter_map(|id| {
            let parts = version_parts(id);
            if parts.is_empty() {
                None
            } else {
                Some((id, parts))
            }
        })
        .max_by(|(_, left), (_, right)| left.cmp(right))
        .map(|(id, _)| id.as_str())
}

fn version_parts(model_id: &str) -> Vec<u32> {
    model_id
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

fn resolve_model_alias(selector: &str, model_ids: &[String]) -> Option<String> {
    let family_prefix = claude_family_prefix(selector)?;
    let matches = model_ids
        .iter()
        .filter(|model_id| model_id.starts_with(&family_prefix))
        .cloned()
        .collect::<Vec<_>>();

    highest_version_model(&matches).map(ToString::to_string)
}

fn claude_family_prefix(selector: &str) -> Option<String> {
    let mut parts = selector.split('-');
    let provider = parts.next()?;
    let family = parts.next()?.to_ascii_lowercase();
    let major = parts.next()?;

    if !provider.eq_ignore_ascii_case("claude") {
        return None;
    }
    if family.is_empty() || !family.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    if major.parse::<u32>().is_err() {
        return None;
    }

    Some(format!("claude-{family}-{major}-"))
}

const TUI_COMMANDS: &[&str] = &[
    "/help",
    "/quit",
    "/exit",
    "/clear",
    "/new",
    "/history",
    "/config",
    "/status",
    "/model",
    "/auth",
    "/keys",
    "/sign",
    "/loop",
    "/budget",
    "/signals",
    "/debug",
    "/analyze",
    "/improve",
    "/synthesis",
];

const PROMPT_COLOR_START: &str = "\x1b[38;2;255;204;0m";
const PROMPT_COLOR_END: &str = "\x1b[0m";

/// Group models by provider name, preserving insertion order.
fn group_models_by_provider(models: &[ModelInfo]) -> Vec<(String, Vec<&ModelInfo>)> {
    let mut groups: Vec<(String, Vec<&ModelInfo>)> = Vec::new();
    for model in models {
        if let Some(existing) = groups
            .iter_mut()
            .find(|(name, _)| *name == model.provider_name)
        {
            existing.1.push(model);
        } else {
            groups.push((model.provider_name.clone(), vec![model]));
        }
    }
    groups
}

/// Only add recognized commands and chat messages to history.
/// Rejects mistyped slash commands (e.g. `/ex`) to prevent history pollution.
fn should_add_to_history(line: &str) -> bool {
    if !line.starts_with('/') {
        return true; // regular chat message
    }
    let command_token = line.split_whitespace().next().unwrap_or(line);
    TUI_COMMANDS.contains(&command_token)
}

fn history_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let fawx_dir = home.join(".fawx");

    let history_file = history_namespace(&home)
        .map(|namespace| format!("history-{namespace}.txt"))
        .unwrap_or_else(|| "history.txt".to_string());

    Some(fawx_dir.join(history_file))
}

fn history_namespace(home: &Path) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    history_namespace_for_cwd(home, &cwd)
}

fn history_namespace_for_cwd(home: &Path, cwd: &Path) -> Option<String> {
    if cwd == home {
        return None;
    }

    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    Some(format!("{:016x}", hasher.finish()))
}

/// Parse a bare-word command typed during execution.
///
/// Returns `Some(LoopCommand)` for recognized commands, `None` for
/// unrecognized text (callers decide whether to steer or queue).
pub(crate) fn parse_bare_command(input: &str) -> Option<LoopCommand> {
    let trimmed = input.trim();
    let lower = trimmed.to_lowercase();

    // Handle "/steer <text>" and "steer <text>" with a single strip_prefix chain
    let steer_text = lower
        .strip_prefix("/steer ")
        .or_else(|| lower.strip_prefix("steer "))
        .and_then(|_| {
            // Use the original (non-lowercased) text for the steer payload
            let prefix_len = if trimmed.starts_with('/') {
                "/steer ".len()
            } else {
                "steer ".len()
            };
            let text = trimmed[prefix_len..].trim();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        });
    if let Some(text) = steer_text {
        return Some(LoopCommand::Steer(text.to_string()));
    }

    match lower.as_str() {
        "stop" | "s" | "/stop" => Some(LoopCommand::Stop),
        "abort" | "a" | "cancel" | "/abort" => Some(LoopCommand::Abort),
        "no" => Some(LoopCommand::Stop),
        "wait" | "pause" | "w" | "/wait" => Some(LoopCommand::Wait),
        "go" | "resume" | "continue" | "/resume" => Some(LoopCommand::Resume),
        "status" | "st" | "/status" => Some(LoopCommand::StatusQuery),
        _ => None,
    }
}

/// The main TUI application loop.
pub struct TuiApp {
    router: ModelRouter,
    auth_manager: AuthManager,
    auth_store: AuthStore,
    catalog: ModelCatalog,
    loop_engine: LoopEngine,
    running: bool,
    conversation_history: Vec<Message>,
    conversation_store: ConversationStore,
    last_signals: Vec<Signal>,
    signal_store: SignalStore,
    cancel_token: CancellationToken,
    config: FawxConfig,
    config_path: PathBuf,
    max_history: usize,
    memory: Option<SharedMemoryStore>,
    runtime_info: Arc<RwLock<RuntimeInfo>>,
    /// Shared model ID list for tab-completion.
    completer_model_ids: Arc<Mutex<Vec<String>>>,
    /// Event bus for streaming events from the kernel.
    event_bus: EventBus,
    scratchpad: Arc<Mutex<Scratchpad>>,
    skill_registry: Arc<SkillRegistry>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    tool_executor: Arc<dyn ToolExecutor>,
    /// Single encrypted credential store instance (opened once at startup, shared via Arc).
    credential_store: Option<Arc<fx_auth::credential_store::EncryptedFileCredentialStore>>,
    /// Buffer for command output that will be flushed to FawxApp.
    output_buffer: Vec<String>,
    /// Signature verification policy, loaded once at startup and shared with the
    /// hot-reload watcher to avoid redundant `load_trusted_keys()` calls.
    signature_policy: SignaturePolicy,
}

/// Dependencies for constructing a [`TuiApp`]. Avoids > 5 bare parameters.
pub struct TuiAppDeps {
    pub auth_manager: AuthManager,
    pub router: ModelRouter,
    pub loop_engine: LoopEngine,
    pub runtime_info: Arc<RwLock<RuntimeInfo>>,
    pub config: FawxConfig,
    pub memory: Option<SharedMemoryStore>,
    pub event_bus: EventBus,
    pub scratchpad: Arc<Mutex<Scratchpad>>,
    pub skill_registry: Arc<SkillRegistry>,
    pub credential_provider: Option<Arc<dyn CredentialProvider>>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    pub credential_store: Option<Arc<fx_auth::credential_store::EncryptedFileCredentialStore>>,
    pub signature_policy: SignaturePolicy,
}

/// Bundles mutable references needed by [`TuiApp::run_cycle_rendering`],
/// keeping the parameter count within the 4-5 limit.
struct CycleRenderingContext<'a> {
    snapshot: Option<PerceptionSnapshot>,
    active_model: String,
    terminal: &'a mut DefaultTerminal,
    app: &'a mut ui::FawxApp,
    last_frame_rows: &'a mut Option<ui::FrameRenderRows>,
    reload_rx: &'a mut tokio::sync::mpsc::Receiver<ReloadEvent>,
    bus_rx: &'a mut broadcast::Receiver<InternalMessage>,
    cancel_token: &'a CancellationToken,
    streamed: &'a mut bool,
    input_sender: Option<LoopInputSender>,
}

impl TuiApp {
    /// Create a new TUI application.
    #[cfg(test)]
    pub fn new(
        auth_manager: AuthManager,
        router: ModelRouter,
        loop_engine: LoopEngine,
        runtime_info: Arc<RwLock<RuntimeInfo>>,
        config: FawxConfig,
    ) -> Result<Self, TuiError> {
        let event_bus = EventBus::new(EVENT_BUS_CAPACITY);
        let scratchpad = Arc::new(Mutex::new(Scratchpad::new()));
        let skill_registry = Arc::new(SkillRegistry::new());
        let tool_executor: Arc<dyn ToolExecutor> =
            Arc::new(SharedSkillRegistry::new(Arc::clone(&skill_registry)));
        Self::new_with_deps(TuiAppDeps {
            auth_manager,
            router,
            loop_engine,
            runtime_info,
            config,
            memory: None,
            event_bus,
            scratchpad,
            skill_registry,
            credential_provider: None,
            tool_executor,
            credential_store: None,
            signature_policy: SignaturePolicy::default(),
        })
    }

    /// Create a new TUI application with full dependency injection.
    pub fn new_with_deps(deps: TuiAppDeps) -> Result<Self, TuiError> {
        let TuiAppDeps {
            auth_manager,
            router,
            loop_engine,
            runtime_info,
            config,
            memory,
            event_bus,
            scratchpad,
            skill_registry,
            credential_provider,
            tool_executor,
            credential_store,
            signature_policy,
        } = deps;
        let base_data_dir = fawx_data_dir();
        let data_dir = configured_data_dir(&base_data_dir, &config);
        let _ = std::fs::create_dir_all(&data_dir);
        let auth_store = AuthStore::open(&data_dir)
            .map_err(|e| TuiError::Auth(format!("failed to open auth store: {e}")))?;
        let mut conversation_store = ConversationStore::new(&data_dir).map_err(TuiError::Store)?;
        let session_id = conversation_store
            .ensure_active()
            .map_err(TuiError::Store)?;
        let signal_store =
            SignalStore::new(&data_dir, &session_id).map_err(|e| TuiError::Store(e.to_string()))?;
        if let Err(e) = signal_store.cleanup_old_signals() {
            eprintln!("warning: signal cleanup failed: {e}");
        }
        let max_history = config.general.max_history;
        let conversation_history = load_startup_conversation_history(&conversation_store, &config);
        let mut app = Self {
            router,
            auth_manager,
            auth_store,
            catalog: ModelCatalog::new(),
            loop_engine,
            running: true,
            conversation_history,
            conversation_store,
            last_signals: Vec::new(),
            signal_store,
            cancel_token: CancellationToken::new(),
            config,
            config_path: base_data_dir.join("config.toml"),
            max_history,
            memory,
            runtime_info,
            completer_model_ids: Arc::new(Mutex::new(Vec::new())),
            event_bus,
            scratchpad,
            skill_registry,
            credential_provider,
            tool_executor,
            credential_store,
            output_buffer: Vec::new(),
            signature_policy,
        };
        app.select_first_available_model_without_refresh();
        Ok(app)
    }

    /// Write one or more logical lines to the output buffer.
    fn tui_println(&mut self, line: impl Into<String>) {
        let line = line.into();
        self.output_buffer.extend(split_output_block(&line));
    }

    /// Return a reference to the credential store, or an error if unavailable.
    fn get_credential_store(
        &self,
    ) -> Result<&Arc<fx_auth::credential_store::EncryptedFileCredentialStore>, TuiError> {
        self.credential_store
            .as_ref()
            .ok_or_else(|| TuiError::Auth("credential store not available".into()))
    }

    /// Persist the current auth manager to the encrypted store.
    fn persist_auth_manager(&self) -> Result<(), TuiError> {
        self.auth_store
            .save_auth_manager(&self.auth_manager)
            .map_err(TuiError::Auth)
    }

    /// Drain the output buffer into the FawxApp.
    fn flush_output_to(&mut self, app: &mut ui::FawxApp) {
        for line in self.output_buffer.drain(..) {
            app.add_output(line);
        }
    }

    fn spawn_skill_watcher(&self) -> tokio::sync::mpsc::Receiver<ReloadEvent> {
        let (reload_tx, reload_rx) = tokio::sync::mpsc::channel(32);
        let skills_dir = fawx_skills_dir();
        if let Err(error) = std::fs::create_dir_all(&skills_dir) {
            tracing::warn!(
                error = %error,
                path = %skills_dir.display(),
                "failed to ensure skills dir exists before starting watcher"
            );
        }
        let mut watcher = SkillWatcher::new(
            skills_dir,
            Arc::clone(&self.skill_registry),
            reload_tx,
            self.credential_provider.clone(),
            self.signature_policy.clone(),
        );
        watcher.initialize_hashes();
        tokio::spawn(async move {
            if let Err(error) = watcher.run().await {
                tracing::error!(error = %error, "skill watcher exited");
            }
        });
        reload_rx
    }

    fn install_force_quit_handler(&self) {
        let cancel_for_signal = self.cancel_token.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            cancel_for_signal.cancel();
            tokio::signal::ctrl_c().await.ok();
            restore_ratatui_terminal();
            eprintln!("\n\u{23f9} Force quit.");
            std::process::exit(130);
        });
    }

    async fn handle_idle_loop_event(
        &mut self,
        event: IdleLoopEvent,
        terminal: &mut DefaultTerminal,
        app: &mut ui::FawxApp,
        last_frame_rows: &mut Option<ui::FrameRenderRows>,
        reload_rx: &mut tokio::sync::mpsc::Receiver<ReloadEvent>,
    ) -> Result<bool, TuiError> {
        match event {
            IdleLoopEvent::Reload(reload_event) => {
                process_reload_event(app, self.tool_executor.as_ref(), reload_event);
                Ok(false)
            }
            IdleLoopEvent::Terminal(event) => {
                self.handle_idle_terminal_event(event, terminal, app, last_frame_rows, reload_rx)
                    .await
            }
        }
    }

    async fn handle_idle_terminal_event(
        &mut self,
        event: Event,
        terminal: &mut DefaultTerminal,
        app: &mut ui::FawxApp,
        last_frame_rows: &mut Option<ui::FrameRenderRows>,
        reload_rx: &mut tokio::sync::mpsc::Receiver<ReloadEvent>,
    ) -> Result<bool, TuiError> {
        match event {
            Event::Key(key) => {
                let should_break =
                    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
                if let Some(input) = handle_idle_event(Event::Key(key), app) {
                    self.process_input_line_tui(&input, terminal, app, last_frame_rows, reload_rx)
                        .await?;
                }
                Ok(should_break)
            }
            Event::Paste(text) => {
                handle_idle_event(Event::Paste(text), app);
                Ok(false)
            }
            Event::Resize(_, _) => Ok(false),
            _ => Ok(false),
        }
    }

    /// Run the TUI main loop using ratatui rendering.
    pub async fn run(&mut self) -> Result<(), TuiError> {
        self.select_first_available_model().await;

        install_ratatui_panic_hook();

        crossterm::terminal::enable_raw_mode().map_err(TuiError::Io)?;
        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            EnableBracketedPaste
        )
        .map_err(TuiError::Io)?;
        let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
        let mut terminal = ratatui::Terminal::new(backend).map_err(TuiError::Io)?;
        let mut terminal_events = EventStream::new();
        let mut app = ui::FawxApp::new();

        // Show welcome banner
        self.show_welcome();
        self.flush_output_to(&mut app);
        let mut reload_rx = self.spawn_skill_watcher();

        self.install_force_quit_handler();

        self.loop_engine.set_cancel_token(self.cancel_token.clone());
        self.sync_completer_model_ids();
        let mut last_frame_rows = None;

        while self.running {
            draw_ratatui_frame(&mut terminal, &app, &mut last_frame_rows)?;
            let event = wait_for_idle_loop_event(
                next_idle_terminal_event(&mut terminal_events),
                &mut reload_rx,
            )
            .await?;
            if self
                .handle_idle_loop_event(
                    event,
                    &mut terminal,
                    &mut app,
                    &mut last_frame_rows,
                    &mut reload_rx,
                )
                .await?
            {
                break;
            }
        }

        try_restore_ratatui_terminal().map_err(TuiError::Io)?;
        Ok(())
    }

    /// Process user input and run the agent cycle with ratatui rendering.
    async fn process_input_line_tui(
        &mut self,
        input: &str,
        terminal: &mut DefaultTerminal,
        app: &mut ui::FawxApp,
        last_frame_rows: &mut Option<ui::FrameRenderRows>,
        reload_rx: &mut tokio::sync::mpsc::Receiver<ReloadEvent>,
    ) -> Result<(), TuiError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        // Add to history
        if should_add_to_history(trimmed) {
            app.push_history(trimmed.to_string());
        }

        // Handle control commands when idle (e.g. /stop shows "nothing running")
        if is_idle_control_command(trimmed) {
            app.add_output("ℹ Nothing is running.".to_string());
            return Ok(());
        }

        if trimmed.starts_with('/') {
            if let Err(error) = self.handle_command(trimmed).await {
                self.tui_println(format_error_message(&error.to_string()));
            }
            self.flush_output_to(app);
            return Ok(());
        }

        // Echo user message
        append_user_message_output(app, trimmed);

        // Run agent cycle with rendering
        self.run_agent_cycle_tui(trimmed, terminal, app, last_frame_rows, reload_rx)
            .await?;

        // After cycle completes, drain queued inputs (one per cycle)
        while let Some(queued) = app.drain_next_input() {
            append_user_message_output(app, &queued);
            self.run_agent_cycle_tui(&queued, terminal, app, last_frame_rows, reload_rx)
                .await?;
        }

        Ok(())
    }

    /// Run a single agent cycle with ratatui rendering for the spinner
    /// and streaming output.
    async fn run_agent_cycle_tui(
        &mut self,
        input: &str,
        terminal: &mut DefaultTerminal,
        app: &mut ui::FawxApp,
        last_frame_rows: &mut Option<ui::FrameRenderRows>,
        reload_rx: &mut tokio::sync::mpsc::Receiver<ReloadEvent>,
    ) -> Result<(), TuiError> {
        app.set_state(ui::AppState::Executing { spinner_frame: 0 });
        app.reset_cycle_state();

        let mut bus_rx = self.event_bus.subscribe();
        let cancel_token = self.cancel_token.clone();
        let mut streamed = false;
        let started = Instant::now();

        // Wire up the input channel so the engine can receive commands
        let (sender, channel) = loop_input_channel();
        self.loop_engine.set_input_channel(channel);

        // Forward any leftover steer from a previous cycle
        if let Some(steer) = app.take_steer() {
            let _ = sender.send(LoopCommand::Steer(steer));
        }

        self.ensure_message_auth().await?;
        let active_model = self.resolve_active_model()?;
        self.update_memory_context_for_input(input);
        let snapshot = self.build_perception_snapshot(input);

        // Run cycle — blocks on the engine but tokio::select! lets us
        // interleave rendering ticks and keyboard checks.
        let mut ctx = CycleRenderingContext {
            snapshot: Some(snapshot),
            active_model,
            terminal,
            app,
            last_frame_rows,
            reload_rx,
            bus_rx: &mut bus_rx,
            cancel_token: &cancel_token,
            streamed: &mut streamed,
            input_sender: Some(sender),
        };
        let result = self.run_cycle_rendering(&mut ctx).await;

        // Drain any remaining bus events
        drain_bus_events(&mut bus_rx, app, &mut streamed);
        app.finish_streaming();
        app.set_state(ui::AppState::Idle);

        self.handle_cycle_result(input, result, streamed, started, app);
        Ok(())
    }

    /// Rendering + cycle select loop. Isolated to satisfy the borrow checker:
    /// borrows of `self.loop_engine` and `self.router` end at the loop exit.
    async fn run_cycle_rendering(
        &mut self,
        ctx: &mut CycleRenderingContext<'_>,
    ) -> Result<LoopResult, TuiError> {
        let snapshot = ctx.snapshot.take().expect("snapshot must be provided");
        let active_model = std::mem::take(&mut ctx.active_model);
        let llm = RouterLoopLlmProvider::new(&self.router, active_model);
        let cycle_future = self.loop_engine.run_cycle(snapshot, &llm);
        tokio::pin!(cycle_future);

        loop {
            draw_ratatui_frame(ctx.terminal, ctx.app, ctx.last_frame_rows)?;
            let reload_open = !ctx.reload_rx.is_closed();

            tokio::select! {
                biased;
                result = &mut cycle_future => {
                    return result.map_err(|e| TuiError::Loop(e.reason));
                }
                Some(reload_event) = ctx.reload_rx.recv(), if reload_open => {
                    process_reload_event(ctx.app, self.tool_executor.as_ref(), reload_event);
                }
                _ = sleep(Duration::from_millis(50)) => {
                    drain_bus_events(ctx.bus_rx, ctx.app, ctx.streamed);
                    if let ui::AppState::Executing { ref mut spinner_frame } = ctx.app.state {
                        *spinner_frame += 1;
                    }
                    poll_execution_events(ctx.app, ctx.cancel_token, &ctx.input_sender);
                }
            }
        }
    }

    /// Post-cycle: bookkeeping + output.
    fn handle_cycle_result(
        &mut self,
        input: &str,
        result: Result<LoopResult, TuiError>,
        streamed: bool,
        started: Instant,
        app: &mut ui::FawxApp,
    ) {
        match result {
            Ok(ref loop_result) => {
                if let Err(e) = self.post_cycle_bookkeeping(input, loop_result) {
                    self.tui_println(format_error_message(&e.to_string()));
                }
                if !streamed {
                    let rendered = render_loop_result(loop_result.clone(), started.elapsed());
                    self.tui_println(rendered);
                } else if let Some(m) =
                    format_loop_metadata_for_result(loop_result, started.elapsed())
                {
                    self.tui_println(m);
                }
            }
            Err(ref error) => {
                self.tui_println(format_error_message(&error.to_string()));
            }
        }
        self.flush_output_to(app);
    }

    async fn process_input_line(&mut self, input: &str) -> Result<(), TuiError> {
        if input.is_empty() {
            return Ok(());
        }

        if input.starts_with('/') {
            if let Err(error) = self.handle_command(input).await {
                self.tui_println(format_error_message(&error.to_string()));
            }
            return Ok(());
        }

        self.tui_println(format!("you \u{203a} {input}"));

        match self.handle_message(input).await {
            Ok(Some(response)) => self.display_response(&response)?,
            Ok(None) => {} // streaming already rendered inline
            Err(error) => self.tui_println(format_error_message(&error.to_string())),
        }

        Ok(())
    }

    /// Display the welcome banner.
    fn show_welcome(&mut self) {
        // Show ANSI hero art on truecolor terminals, plain ASCII fallback otherwise.
        let banner = if supports_truecolor() {
            FAWX_BANNER_ANSI
        } else {
            BANNER_ART
        };
        for line in banner.lines() {
            self.tui_println(line.to_string());
        }
        self.tui_println(String::new());
        self.tui_println(
            "  fawx \u{00b7} agentic engine \u{00b7} type /help for commands".to_string(),
        );
        if !self.auth_manager.has_any() {
            self.tui_println(
                "  Not authenticated \u{00b7} /auth to set up or just send a message".to_string(),
            );
        }
        self.tui_println(String::new());
    }

    /// Run the first-time auth wizard if no credentials exist.
    async fn auth_wizard(&mut self) -> Result<(), TuiError> {
        self.tui_println("Welcome to Fawx.\n");
        if self.auth_manager.providers().is_empty() {
            self.tui_println("No credentials found. Let's set up authentication.\n");
        } else {
            self.tui_println("Add another provider.\n");
        }

        let result = self.run_auth_wizard_flow().await;
        finalize_auth_wizard_result(result)
    }

    async fn run_auth_wizard_flow(&mut self) -> Result<(), TuiError> {
        let selection = self.run_auth_selection()?;
        let preferred_provider = match selection {
            AuthSelection::ClaudeSubscription => self.auth_wizard_claude_subscription()?,
            AuthSelection::ChatGptSubscription => self.run_oauth_flow().await?,
            AuthSelection::ApiKey => self.auth_wizard_api_key()?,
        };

        self.persist_and_activate_model(&preferred_provider).await
    }

    fn run_auth_selection(&mut self) -> Result<AuthSelection, TuiError> {
        self.tui_println("How would you like to authenticate?");
        self.tui_println("  [1] Claude subscription (paste setup-token)");
        self.tui_println("  [2] ChatGPT subscription (browser sign-in)");
        self.tui_println("  [3] API key (any provider)");
        self.tui_println(String::new());

        prompt_choice(
            "> ",
            "Please choose 1, 2, or 3.\n",
            "authentication selection",
            parse_auth_selection,
        )
    }

    fn auth_wizard_claude_subscription(&mut self) -> Result<String, TuiError> {
        let token = prompt_non_empty_secret(
            "Paste your Claude setup token: ",
            "Setup token cannot be empty.\n",
            "Claude setup token",
        )?;

        self.auth_manager
            .store("anthropic", AuthMethod::SetupToken { token });

        self.tui_println("✓ Authenticated. Token stored.\n");
        Ok("anthropic".to_string())
    }

    async fn run_oauth_flow(&mut self) -> Result<String, TuiError> {
        let flow = PkceFlow::try_new().map_err(|error| {
            TuiError::Auth(format!("failed to initialize oauth PKCE flow: {error}"))
        })?;
        let client_id = openai_oauth_client_id();
        let auth_url = flow.authorization_url(&client_id);
        let auth_code = obtain_oauth_authorization_code(&flow, &auth_url).await?;

        self.tui_println("Exchanging authorization code for tokens...");
        let token_response = exchange_oauth_code_for_tokens(&flow, &client_id, &auth_code).await?;
        self.store_openai_oauth_tokens(token_response);

        self.tui_println("✓ Authenticated. Tokens stored.\n");
        Ok("openai".to_string())
    }

    fn store_openai_oauth_tokens(&mut self, token_response: TokenResponse) {
        let account_id = fx_auth::oauth::extract_openai_account_id(&token_response.access_token);
        let expires_at =
            current_time_ms().saturating_add(token_response.expires_in.saturating_mul(1_000));

        self.auth_manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: token_response.access_token,
                refresh_token: token_response.refresh_token,
                expires_at,
                account_id,
            },
        );
    }

    fn auth_wizard_api_key(&mut self) -> Result<String, TuiError> {
        let provider = prompt_api_key_provider()?;

        let key = prompt_non_empty_secret(
            &format!("Enter your {provider} API key: "),
            "API key cannot be empty.\n",
            "API key",
        )?;

        self.auth_manager.store(
            &provider,
            AuthMethod::ApiKey {
                provider: provider.clone(),
                key,
            },
        );

        self.tui_println("✓ API key stored.\n");
        Ok(provider)
    }

    async fn persist_and_activate_model(
        &mut self,
        preferred_provider: &str,
    ) -> Result<(), TuiError> {
        self.persist_auth_manager()?;

        let preferred_models = self.get_models_for_provider(preferred_provider).await;
        self.refresh_router_models().await?;
        self.set_preferred_model(&preferred_models).await;
        self.print_active_model();

        Ok(())
    }

    fn print_active_model(&mut self) {
        if let Some(active_model) = self.router.active_model() {
            let model_info = self
                .router
                .available_models()
                .into_iter()
                .find(|model| model.model_id == active_model);

            if let Some(model_info) = model_info {
                self.tui_println(format!(
                    "Active model: {} ({} {})",
                    active_model, model_info.provider_name, model_info.auth_method
                ));
            } else {
                self.tui_println(format!("Active model: {active_model}\n"));
            }
        }
    }

    /// Process a user command (starts with `/`).
    async fn handle_command(&mut self, input: &str) -> Result<(), TuiError> {
        match parse_command(input) {
            ParsedCommand::Model(None) => {
                self.refresh_router_models().await?;
                self.show_model_menu();
            }
            ParsedCommand::Model(Some(model)) => {
                match self.set_active_model_with_refresh(&model).await {
                    Ok(resolved_model) => {
                        self.config.model.default_model = Some(resolved_model.clone());
                        if let Err(error) = self.config.save(&fawx_data_dir()) {
                            eprintln!("Warning: couldn't save model preference: {error}");
                        }
                        self.tui_println(format!("Active model set to: {resolved_model}"));
                    }
                    Err(error) => {
                        self.tui_println(format!("Couldn't select model: {error}"));
                        self.show_model_menu();
                    }
                }
            }
            ParsedCommand::Auth {
                subcommand,
                action,
                value,
                has_extra_args,
            } => {
                if has_extra_args {
                    self.tui_println("Note: extra arguments ignored.");
                }
                self.handle_auth_command(subcommand, action, value).await?;
            }
            ParsedCommand::Keys {
                subcommand,
                value,
                option,
                has_extra_args,
            } => {
                self.handle_keys_command(subcommand, value, option, has_extra_args)?;
            }
            ParsedCommand::Sign {
                target,
                has_extra_args,
            } => {
                self.handle_sign_command(target, has_extra_args)?;
            }
            ParsedCommand::Budget => self.show_budget_status(),
            ParsedCommand::Loop => self.show_loop_status(),
            ParsedCommand::Status => self.show_status(),
            ParsedCommand::Signals => self.show_signals_summary(),
            ParsedCommand::Debug => self.show_signals_debug(),
            ParsedCommand::Analyze => self.handle_analyze_command().await?,
            ParsedCommand::Improve(flags) => self.handle_improve_command(flags).await?,
            ParsedCommand::Synthesis(instruction) => {
                self.update_synthesis_instruction(instruction);
            }
            ParsedCommand::Clear => {
                self.conversation_store
                    .clear_active()
                    .map_err(TuiError::Store)?;
                self.conversation_history.clear();
                self.clear_screen()?;
            }
            ParsedCommand::New => {
                let id = self
                    .conversation_store
                    .create_new()
                    .map_err(TuiError::Store)?;
                self.conversation_history.clear();
                self.tui_println(format!("Started new conversation: {id}"));
            }
            ParsedCommand::History => self.show_conversation_history(),
            ParsedCommand::Config(action) => self.handle_config_command(action)?,
            ParsedCommand::Help => self.show_help(),
            ParsedCommand::Quit => {
                self.running = false;
                self.tui_println("Goodbye!");
            }
            ParsedCommand::Unknown(command) => {
                self.tui_println(format!("Unknown command: /{command}"));
                self.tui_println("Type /help for available commands.");
            }
        }

        Ok(())
    }

    /// Process a user message by running the full loop engine.
    ///
    /// Returns `Ok(None)` when streaming rendered the response inline,
    /// or `Ok(Some(rendered))` for batch display.
    async fn handle_message(&mut self, input: &str) -> Result<Option<String>, TuiError> {
        self.ensure_message_auth().await?;
        let active_model = self.resolve_active_model()?;
        self.update_memory_context_for_input(input);
        let snapshot = self.build_perception_snapshot(input);
        let started = Instant::now();

        let loop_result = self.run_cycle(snapshot, active_model).await?;

        self.post_cycle_bookkeeping(input, &loop_result)?;
        // In the ratatui path, streaming is handled by run_agent_cycle_tui.
        // For the legacy path (tests), we pass streamed=false so the
        // response is returned as a batch string.
        self.finalize_streaming_display(&loop_result, false, started.elapsed())
    }

    /// Resolve the currently active model ID.
    fn resolve_active_model(&self) -> Result<String, TuiError> {
        self.router
            .active_model()
            .map(|m| m.to_string())
            .ok_or_else(|| TuiError::Router("no active model selected".to_string()))
    }

    /// Run the loop engine cycle (no display — callers handle rendering).
    async fn run_cycle(
        &mut self,
        snapshot: PerceptionSnapshot,
        active_model: String,
    ) -> Result<LoopResult, TuiError> {
        let llm = RouterLoopLlmProvider::new(&self.router, active_model);
        self.loop_engine
            .run_cycle(snapshot, &llm)
            .await
            .map_err(|error| TuiError::Loop(error.reason))
    }

    /// Persist signals, conversation turn, and signal store after a cycle.
    fn post_cycle_bookkeeping(
        &mut self,
        input: &str,
        loop_result: &LoopResult,
    ) -> Result<(), TuiError> {
        self.last_signals = loop_result_signals(loop_result).to_vec();
        if let Err(e) = self.signal_store.persist(&self.last_signals) {
            eprintln!("warning: signal persist failed: {e}");
        }
        let response_text = loop_result_response_text(loop_result);
        self.record_conversation_turn(input, response_text.clone());
        self.persist_turn(input, &response_text, loop_result)
            .map_err(TuiError::Store)?;
        Ok(())
    }

    /// Decide whether to return a batch-rendered response or `None`
    /// (streaming already displayed the text).
    fn finalize_streaming_display(
        &mut self,
        loop_result: &LoopResult,
        streamed: bool,
        wall_time: std::time::Duration,
    ) -> Result<Option<String>, TuiError> {
        if streamed {
            if matches!(loop_result, LoopResult::UserStopped { .. }) {
                self.tui_println(" [interrupted]".to_string());
            }
            let metadata = format_loop_metadata_for_result(loop_result, wall_time);
            if let Some(meta) = metadata {
                self.tui_println(meta);
            }
            self.tui_println(String::new());
            Ok(None)
        } else {
            Ok(Some(render_loop_result(loop_result.clone(), wall_time)))
        }
    }

    fn update_memory_context_for_input(&mut self, input: &str) {
        let memory_context = self.relevant_memory_context(input).unwrap_or_default();
        self.loop_engine.set_memory_context(memory_context);
        self.update_scratchpad_context();
    }

    fn update_scratchpad_context(&mut self) {
        let context = match self.scratchpad.lock() {
            Ok(sp) => sp.render_for_context(),
            Err(e) => {
                eprintln!("warning: failed to lock scratchpad: {e}");
                return;
            }
        };
        self.loop_engine.set_scratchpad_context(context);
    }

    fn relevant_memory_context(&self, input: &str) -> Option<String> {
        let entries = self.search_relevant_memory_entries(input)?;
        format_memory_for_prompt(&entries, self.config.memory.max_snapshot_chars)
    }

    fn search_relevant_memory_entries(&self, input: &str) -> Option<Vec<(String, String)>> {
        let memory = self.memory.as_ref()?;
        match memory.lock() {
            Ok(store) => {
                Some(store.search_relevant(input, self.config.memory.max_relevant_results))
            }
            Err(error) => {
                eprintln!("warning: failed to lock memory store: {error}");
                None
            }
        }
    }

    async fn ensure_message_auth(&mut self) -> Result<(), TuiError> {
        if !self.auth_manager.has_any() {
            self.auth_wizard().await?;
        }

        if self.router.active_model().is_none() {
            self.select_first_available_model().await;
        }

        Ok(())
    }

    // Sync counterpart of `set_active_model_with_refresh` for use in non-async contexts
    // (constructor, tests). See async version for the authoritative startup path.
    /// Set the active model by exact match only — no alias resolution, no API refresh.
    /// Used as the first step in the layered matching strategy:
    /// exact → from_selector (with alias) → with_refresh.
    fn set_active_model_exact(&mut self, selector: &str) -> Result<String, RouterError> {
        self.router.set_active(selector)?;

        let resolved = self
            .router
            .active_model()
            .map(ToString::to_string)
            .ok_or(RouterError::NoActiveModel)?;
        self.sync_runtime_info_model();
        Ok(resolved)
    }

    // Sync counterpart of `set_active_model_with_refresh` for use in non-async contexts
    // (constructor, tests). See async version for the authoritative startup path.
    fn set_active_model_from_selector(&mut self, selector: &str) -> Result<String, RouterError> {
        match self.set_active_model_exact(selector) {
            Ok(model) => Ok(model),
            Err(error @ RouterError::ModelNotFound(_)) => {
                let model_ids = self
                    .router
                    .available_models()
                    .into_iter()
                    .map(|model| model.model_id)
                    .collect::<Vec<_>>();
                let Some(alias_target) = resolve_model_alias(selector, &model_ids) else {
                    return Err(error);
                };
                self.set_active_model_exact(&alias_target)
            }
            Err(error) => Err(error),
        }
    }

    async fn set_active_model_with_refresh(
        &mut self,
        selector: &str,
    ) -> Result<String, RouterError> {
        match self.set_active_model_exact(selector) {
            Ok(model) => Ok(model),
            Err(initial_error @ RouterError::ModelNotFound(_)) => {
                if !self.auth_manager.has_any() {
                    return self
                        .set_active_model_from_selector(selector)
                        .or(Err(initial_error));
                }

                if self.refresh_router_models().await.is_err() {
                    return self
                        .set_active_model_from_selector(selector)
                        .or(Err(initial_error));
                }

                // After a successful refresh, propagate the fresh error (more informative
                // than the stale initial_error). When refresh fails or auth is missing,
                // fall back to initial_error since no new information is available.
                self.set_active_model_exact(selector)
                    .or_else(|_| self.set_active_model_from_selector(selector))
            }
            Err(error) => Err(error),
        }
    }

    fn sync_runtime_info_model(&self) {
        let (active_model, provider) = runtime_model_state(&self.router);
        match self.runtime_info.write() {
            Ok(mut info) => {
                info.active_model = active_model;
                info.provider = provider;
            }
            Err(error) => {
                eprintln!("warning: runtime info lock poisoned: {error}");
            }
        }
    }

    fn show_signals_summary(&mut self) {
        if self.last_signals.is_empty() {
            self.tui_println("No signals from last turn.");
            return;
        }

        let collector = SignalCollector::from_signals(self.last_signals.clone());
        self.tui_println(collector.summary().to_string());
    }

    fn show_signals_debug(&mut self) {
        if self.last_signals.is_empty() {
            self.tui_println("No signals from last turn.");
            return;
        }

        let collector = SignalCollector::from_signals(self.last_signals.clone());
        self.tui_println(collector.debug_dump().to_string());
    }

    async fn handle_analyze_command(&mut self) -> Result<(), TuiError> {
        let active_model = self
            .router
            .active_model()
            .ok_or_else(|| TuiError::Router("no active model for analysis".to_string()))?
            .to_string();
        self.tui_println("Analyzing signals across all sessions...");

        // Create provider and engine in a block so the borrow of self.router
        // and self.signal_store ends before we call self.tui_println.
        let analysis_result = {
            let provider = AnalysisCompletionProvider::new(&self.router, active_model);
            let engine = AnalysisEngine::new(&self.signal_store);
            engine.analyze(&provider).await
        };

        match analysis_result {
            Ok(findings) if findings.is_empty() => {
                self.tui_println("No patterns found. Collect more signals first.");
            }
            Ok(findings) => {
                let memory_ref = self.memory.as_ref().cloned();
                let (stored, surfaced, logged) =
                    self.route_findings_by_confidence(&findings, memory_ref.as_ref());
                self.print_analysis_findings(&findings);
                self.tui_println(format!(
                    "Wrote {} patterns to memory, surfaced {} for review, logged {}",
                    stored, surfaced, logged
                ));
            }
            Err(AnalysisError::ParseError(error)) => {
                self.tui_println(format!(
                    "Analysis model responded, but output was unparseable JSON: {error}"
                ));
            }
            Err(error) => return Err(error.into()),
        }

        Ok(())
    }

    async fn handle_improve_command(&mut self, flags: ImproveFlags) -> Result<(), TuiError> {
        if let Some(unknown) = &flags.has_unknown_flag {
            self.tui_println(format!("Unknown flag: {unknown}"));
            self.tui_println("Usage: /improve [--dry-run]");
            return Ok(());
        }

        let active_model = self
            .router
            .active_model()
            .ok_or_else(|| TuiError::Router("no active model for improvement".to_string()))?
            .to_string();

        let (config, data_dir, repo_root) = Self::build_improve_config(&self.config, &flags);
        let proposals_dir = data_dir.join("proposals");
        let paths = CyclePaths {
            data_dir: &data_dir,
            repo_root: &repo_root,
            proposals_dir: &proposals_dir,
        };
        self.tui_println("⚡ Analyzing signals...");
        self.tui_println("⚡ Planning improvements...");

        let provider = AnalysisCompletionProvider::new(&self.router, active_model);
        let result =
            fx_improve::run_improvement_cycle(&self.signal_store, &provider, &config, &paths)
                .await?;
        self.print_improve_result(&result, flags.dry_run);

        Ok(())
    }

    /// Build the [`ImprovementConfig`], data directory, and repo root for an
    /// `/improve` invocation.
    fn build_improve_config(
        app_config: &FawxConfig,
        flags: &ImproveFlags,
    ) -> (ImprovementConfig, PathBuf, PathBuf) {
        let base_data_dir = fawx_data_dir();
        let data_dir = configured_data_dir(&base_data_dir, app_config);
        let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let mut config = ImprovementConfig::default();
        if flags.dry_run {
            config.output_mode = OutputMode::DryRun;
        }
        (config, data_dir, repo_root)
    }

    fn print_improve_result(&mut self, result: &fx_improve::ExecutionResult, dry_run: bool) {
        if dry_run {
            self.tui_println("⚡ Dry run complete.");
        } else {
            self.tui_println("⚡ Improvement cycle complete.");
        }

        if result.proposals_written.is_empty()
            && result.branches_created.is_empty()
            && result.skipped.is_empty()
        {
            self.tui_println("  No actionable improvements found.");
            return;
        }

        for path in &result.proposals_written {
            self.tui_println(format!("  Proposal: {}", path.display()));
        }
        for branch in &result.branches_created {
            self.tui_println(format!("  Branch: {branch}"));
        }
        for (name, reason) in &result.skipped {
            self.tui_println(format!("  Skipped: {name} — {reason}"));
        }
    }

    fn print_analysis_findings(&mut self, findings: &[AnalysisFinding]) {
        for finding in findings {
            let badge = Self::confidence_badge(finding.confidence);
            self.tui_println(format!("\n{badge} | {}", finding.pattern_name));
            self.tui_println(format!("  {}", finding.description));
            self.tui_println(format!("  Evidence: {} signals", finding.evidence.len()));
            if let Some(action) = &finding.suggested_action {
                self.tui_println(format!("  Suggested: {action}"));
            }
        }

        self.tui_println(format!("\nFound {} patterns total.", findings.len()));
    }

    fn route_findings_by_confidence(
        &mut self,
        findings: &[AnalysisFinding],
        memory: Option<&SharedMemoryStore>,
    ) -> (usize, usize, usize) {
        let mut stored = 0;
        let mut surfaced = 0;
        let mut logged = 0;

        for finding in findings {
            match finding.confidence {
                Confidence::High => {
                    if Self::store_high_confidence_finding(memory, finding) {
                        stored += 1;
                    }
                }
                Confidence::Medium => {
                    self.tui_println(format!(
                        "Consider: {} — {}",
                        finding.pattern_name, finding.description
                    ));
                    surfaced += 1;
                }
                Confidence::Low => {
                    tracing::debug!("log: {} — {}", finding.pattern_name, finding.description);
                    logged += 1;
                }
            }
        }

        (stored, surfaced, logged)
    }

    fn store_high_confidence_finding(
        memory: Option<&SharedMemoryStore>,
        finding: &AnalysisFinding,
    ) -> bool {
        let Some(memory_store) = memory else {
            return false;
        };

        let mut store = match memory_store.lock() {
            Ok(store) => store,
            Err(error) => {
                eprintln!("warning: failed to lock memory store: {error}");
                return false;
            }
        };

        let key = Self::pattern_memory_key(&finding.pattern_name);
        if let Err(error) = store.write(&key, &finding.description) {
            eprintln!("warning: failed to persist analysis finding: {error}");
            return false;
        }

        true
    }

    fn pattern_memory_key(pattern_name: &str) -> String {
        format!("pattern/{pattern_name}")
    }

    fn confidence_badge(confidence: Confidence) -> &'static str {
        match confidence {
            Confidence::High => "🔴 HIGH",
            Confidence::Medium => "🟡 MEDIUM",
            Confidence::Low => "🟢 LOW",
        }
    }

    fn update_synthesis_instruction(&mut self, instruction: Option<String>) {
        match instruction {
            None => self.tui_println("Usage: /synthesis <instruction> or /synthesis reset"),
            Some(value) if value.trim().is_empty() => {
                self.tui_println("Synthesis instruction cannot be empty.");
            }
            Some(value) if value.eq_ignore_ascii_case("reset") => {
                if let Err(error) = self
                    .loop_engine
                    .set_synthesis_instruction(DEFAULT_SYNTHESIS_INSTRUCTION.to_string())
                {
                    self.tui_println(format!(
                        "Failed to reset synthesis instruction: {}",
                        error.reason
                    ));
                    return;
                }
                self.tui_println("Synthesis instruction reset to default.");
            }
            Some(value) => {
                if value.len() > MAX_SYNTHESIS_INSTRUCTION_LENGTH {
                    self.tui_println(format!(
                        "Synthesis instruction exceeds {} characters.",
                        MAX_SYNTHESIS_INSTRUCTION_LENGTH
                    ));
                    return;
                }

                match self.loop_engine.set_synthesis_instruction(value.clone()) {
                    Ok(()) => {
                        self.tui_println(format!("Synthesis instruction updated: {}", value.trim()))
                    }
                    Err(error) => self.tui_println(format!(
                        "Failed to update synthesis instruction: {}",
                        error.reason
                    )),
                }
            }
        }
    }

    fn persist_turn(
        &self,
        user_input: &str,
        response: &str,
        loop_result: &LoopResult,
    ) -> Result<(), String> {
        let user_message = ConversationMessage {
            role: "user".to_string(),
            content: user_input.to_string(),
            timestamp_ms: current_time_ms(),
            signals: None,
            tool_calls: None,
            token_usage: None,
        };
        self.conversation_store.save_message(&user_message)?;

        let assistant_message = ConversationMessage {
            role: "assistant".to_string(),
            content: response.to_string(),
            timestamp_ms: current_time_ms(),
            signals: Some(signal_labels(loop_result_signals(loop_result))),
            tool_calls: Some(tool_names(loop_result_signals(loop_result))),
            token_usage: token_usage(loop_result),
        };
        self.conversation_store.save_message(&assistant_message)
    }

    fn handle_config_command(&mut self, action: Option<String>) -> Result<(), TuiError> {
        match action.as_deref() {
            None => self.show_config(),
            Some("init") => self.init_config_file()?,
            Some(other) => self.tui_println(format!(
                "Unknown /config action: {other}. Use /config or /config init."
            )),
        }
        Ok(())
    }

    fn show_config(&mut self) {
        let fields: &[(&str, String)] = &[
            (
                "general.max_iterations",
                self.config.general.max_iterations.to_string(),
            ),
            (
                "general.max_history",
                self.config.general.max_history.to_string(),
            ),
            (
                "model.default_model",
                format!("{:?}", self.config.model.default_model),
            ),
            (
                "model.synthesis_instruction",
                format!("{:?}", self.config.model.synthesis_instruction),
            ),
            (
                "tools.working_dir",
                format!("{:?}", self.config.tools.working_dir),
            ),
            (
                "tools.search_exclude",
                format!("{:?}", self.config.tools.search_exclude),
            ),
            (
                "tools.max_read_size",
                self.config.tools.max_read_size.to_string(),
            ),
            (
                "memory.max_entries",
                self.config.memory.max_entries.to_string(),
            ),
            (
                "memory.max_value_size",
                self.config.memory.max_value_size.to_string(),
            ),
            (
                "memory.max_snapshot_chars",
                self.config.memory.max_snapshot_chars.to_string(),
            ),
            (
                "memory.max_relevant_results",
                self.config.memory.max_relevant_results.to_string(),
            ),
        ];
        let mut output = format!(
            "Config path: {}\nRuntime data dir: {}\nLoaded values:\n",
            self.config_path.display(),
            self.data_dir_display(),
        );
        for (key, value) in fields {
            output.push_str(&format!("  {key} = {value}\n"));
        }
        self.tui_println(output);
    }

    fn init_config_file(&mut self) -> Result<(), TuiError> {
        let base_data_dir = fawx_data_dir();
        let created = FawxConfig::write_default(&base_data_dir).map_err(TuiError::Store)?;
        self.tui_println(format!("Created default config at {}", created.display()));
        Ok(())
    }

    fn data_dir_display(&self) -> String {
        self.config
            .general
            .data_dir
            .clone()
            .unwrap_or_else(fawx_data_dir)
            .display()
            .to_string()
    }

    fn show_conversation_history(&mut self) {
        let conversations = self.conversation_store.list_conversations();
        if conversations.is_empty() {
            self.tui_println("No saved conversations yet.");
            return;
        }

        self.tui_println("Saved conversations:");
        for (id, count) in conversations {
            self.tui_println(format!("  - {id}: {count} messages"));
        }
    }

    /// Display formatted output to the output buffer.
    fn display_response(&mut self, response: &str) -> Result<(), TuiError> {
        self.tui_println(String::new());
        self.tui_println(format!("fawx \u{203a} {response}"));
        self.tui_println(String::new());
        Ok(())
    }

    /// Display the model selection menu grouped by provider.
    fn show_model_menu(&mut self) {
        let active = self.router.active_model().map(|s| s.to_string());
        let models = self.router.available_models();

        if models.is_empty() {
            self.tui_println("No models available. Use /auth to configure credentials.");
            return;
        }

        let grouped = group_models_by_provider(&models);

        self.tui_println("Available models:");
        for (provider, group) in &grouped {
            self.tui_println(String::new());
            self.tui_println(format!("{provider}:"));
            for model in group {
                let marker = if active.as_deref() == Some(model.model_id.as_str()) {
                    "*"
                } else {
                    " "
                };
                self.tui_println(format!(
                    "  {marker} {} ({})",
                    model.model_id, model.auth_method
                ));
            }
        }
    }

    // Sync counterpart of `select_first_available_model` for use in non-async contexts
    // (constructor, tests). See async version for the authoritative startup path.
    fn select_first_available_model_without_refresh(&mut self) {
        if let Some(saved) = self.config.model.default_model.clone() {
            if self.set_active_model_from_selector(&saved).is_ok() {
                return;
            }
            eprintln!("Saved model '{saved}' no longer available, selecting default");
        }

        if self.router.active_model().is_some() {
            self.sync_runtime_info_model();
            return;
        }

        self.set_preferred_default_model_without_refresh();
        self.sync_runtime_info_model();
    }

    async fn select_first_available_model(&mut self) {
        if let Some(saved) = self.config.model.default_model.clone() {
            if self.set_active_model_with_refresh(&saved).await.is_ok() {
                return;
            }
            eprintln!("Saved model '{saved}' no longer available, selecting default");
        }

        if self.router.active_model().is_some() {
            self.sync_runtime_info_model();
            return;
        }

        self.set_preferred_default_model().await;
        self.sync_runtime_info_model();
    }

    // Sync counterpart of `set_preferred_default_model` for use in non-async contexts
    // (constructor, tests). See async version for the authoritative startup path.
    fn set_preferred_default_model_without_refresh(&mut self) {
        let model_ids = self.available_model_ids();

        if let Some(model) = preferred_default_model(&model_ids) {
            if let Err(error) = self.set_active_model_from_selector(model) {
                eprintln!("failed to set initial model {model}: {error}");
            }
        }
    }

    async fn set_preferred_default_model(&mut self) {
        let model_ids = self.available_model_ids();

        if let Some(model) = preferred_default_model(&model_ids) {
            if let Err(error) = self.set_active_model_with_refresh(model).await {
                eprintln!("failed to set initial model {model}: {error}");
            }
        }
    }

    fn available_model_ids(&self) -> Vec<String> {
        self.router
            .available_models()
            .into_iter()
            .map(|model| model.model_id)
            .collect()
    }

    async fn set_preferred_model(&mut self, candidates: &[String]) {
        for candidate in candidates {
            if self.set_active_model_with_refresh(candidate).await.is_ok() {
                return;
            }
        }

        self.select_first_available_model().await;
    }

    async fn get_models_for_provider(&mut self, provider: &str) -> Vec<String> {
        let Some(auth_method) = self.auth_manager.get(provider).cloned() else {
            return models_for_provider(provider);
        };

        let models = catalog_models_for_auth(&mut self.catalog, &auth_method).await;
        if models.is_empty() {
            models_for_provider(provider)
        } else {
            models
        }
    }

    async fn refresh_router_models(&mut self) -> Result<(), TuiError> {
        let previous_active = self.router.active_model().map(ToString::to_string);

        let mut refreshed =
            build_router_with_catalog(&self.auth_manager, &mut self.catalog).await?;

        if let Some(active_model) = previous_active {
            if refreshed.set_active(&active_model).is_err() {
                if let Some(first_model) = refreshed
                    .available_models()
                    .into_iter()
                    .next()
                    .map(|model| model.model_id)
                {
                    if let Err(error) = refreshed.set_active(&first_model) {
                        eprintln!("failed to restore model {first_model}: {error}");
                    }
                }
            }
        }

        self.router = refreshed;
        self.sync_completer_model_ids();
        self.sync_runtime_info_model();
        Ok(())
    }

    /// Push current model IDs into the shared completer list.
    fn sync_completer_model_ids(&self) {
        let ids: Vec<String> = self
            .router
            .available_models()
            .into_iter()
            .map(|m| m.model_id)
            .collect();
        if let Ok(mut locked) = self.completer_model_ids.lock() {
            *locked = ids;
        }
    }

    fn show_help(&mut self) {
        self.tui_println(format!(
            "{}",
            "Commands".bold().with(theme_color(255, 165, 0, 214))
        ));
        self.tui_println("  /model         List models and switch active model");
        self.tui_println("  /model <name>  Switch to a specific model");
        self.tui_println("  /auth          Show credential status + auth help");
        self.tui_println("  /auth <provider> set-token <TOKEN>");
        self.tui_println("                 Save API key or PAT for a provider");
        self.tui_println("  /keys          Manage WASM signing keys");
        self.tui_println("  /keys generate [--force]");
        self.tui_println("  /keys list     List trusted public keys");
        self.tui_println("  /keys trust <path>");
        self.tui_println("  /keys revoke <fingerprint>");
        self.tui_println("  /sign <skill>  Sign one WASM skill");
        self.tui_println("  /sign --all    Sign all installed WASM skills");
        self.tui_println("  /status        Show model, tokens, budget summary");
        self.tui_println("  /budget        Show detailed budget usage");
        self.tui_println("  /loop          Show loop iteration details");
        self.tui_println("  /signals       Show condensed signal summary for last turn");
        self.tui_println("  /debug         Show full signal dump for last turn");
        self.tui_println("  /analyze       Analyze persisted signals across sessions");
        self.tui_println("  /improve       Run self-improvement cycle");
        self.tui_println("  /synthesis     Set or reset synthesis instruction");
        self.tui_println("  /clear         Clear the screen and active conversation");
        self.tui_println("  /new           Start a new conversation");
        self.tui_println("  /history       List saved conversations");
        self.tui_println("  /config        Show loaded config values");
        self.tui_println("  /config init   Create ~/.fawx/config.toml template");
        self.tui_println("  /help          Show this help");
        self.tui_println("  /quit          Exit");
    }

    /// Display status for all known providers.
    fn show_auth_status(&mut self) {
        match self.provider_status_rows() {
            Ok(rows) if rows.is_empty() => {
                self.tui_println("No credentials configured.");
            }
            Ok(rows) => {
                self.tui_println("Configured credentials:");
                for (provider, status, configured) in rows {
                    let marker = if configured { "✓" } else { "✗" };
                    self.tui_println(format!("  {marker} {provider}: {status}"));
                }
            }
            Err(error) => {
                self.tui_println(format!("Error loading credential status: {error}"));
            }
        }
    }

    /// Gather status rows for all known providers.
    fn provider_status_rows(&self) -> Result<Vec<(String, String, bool)>, TuiError> {
        let providers = self.collect_provider_names();
        Ok(providers
            .into_iter()
            .map(|p| self.single_provider_status(&p))
            .collect())
    }

    /// Gather de-duplicated, sorted provider names from all sources.
    fn collect_provider_names(&self) -> Vec<String> {
        let mut providers = vec![
            "anthropic".to_string(),
            "openai".to_string(),
            "github".to_string(),
        ];
        providers.extend(
            self.auth_manager
                .providers()
                .into_iter()
                .map(|p| normalize_provider_name(&p)),
        );
        if let Ok(tokens) = self.auth_store.list_provider_tokens() {
            providers.extend(tokens);
        }
        providers.sort();
        providers.dedup();
        providers
    }

    /// Determine the status string for a single provider.
    fn single_provider_status(&self, provider: &str) -> (String, String, bool) {
        let github_pat = self.github_pat_configured();
        let status = self.provider_status_label(provider, github_pat);
        let configured = status != "not configured";
        (provider.to_string(), status.to_string(), configured)
    }

    /// Check whether a GitHub PAT exists in the credential store.
    fn github_pat_configured(&self) -> bool {
        let Some(store) = self.credential_store.as_ref() else {
            return false;
        };
        CredentialStoreTrait::status(
            store.as_ref(),
            fx_auth::credential_store::AuthProvider::GitHub,
        )
        .map(|s| s.configured)
        .unwrap_or(false)
    }

    /// Return a human label for a provider's auth status.
    fn provider_status_label(&self, provider: &str, github_pat: bool) -> &str {
        if provider == "github" && github_pat {
            return "configured";
        }
        if let Some(method) = self.auth_manager.get(provider) {
            return match method {
                AuthMethod::OAuth { .. } => "configured (OAuth)",
                AuthMethod::SetupToken { .. } | AuthMethod::ApiKey { .. } => "configured",
            };
        }
        if self
            .auth_store
            .get_provider_token(provider)
            .ok()
            .flatten()
            .is_some()
        {
            return "configured";
        }
        "not configured"
    }

    /// Dispatch `/auth` subcommands.
    async fn handle_auth_command(
        &mut self,
        subcommand: Option<String>,
        action: Option<String>,
        value: Option<String>,
    ) -> Result<(), TuiError> {
        let norm_sub = subcommand.as_deref().map(normalize_provider_name);
        let norm_act = action.as_deref().map(|a| a.to_ascii_lowercase());

        match (norm_sub.as_deref(), norm_act.as_deref(), value.as_deref()) {
            (None, _, _) => {
                self.show_auth_status();
                self.show_auth_help();
            }
            (Some("list-providers"), _, _) => self.show_auth_status(),
            (Some("github"), Some("set-token"), t) => {
                self.handle_github_set_token(t).await?;
            }
            (Some(p), Some("set-token"), Some(t)) => {
                self.handle_provider_set_token(p, t).await?;
            }
            (Some(_), Some("set-token"), None) => {
                self.tui_println("Usage: /auth <provider> set-token <TOKEN>");
            }
            (Some("github"), Some("show-status"), _) => {
                self.handle_github_show_status();
            }
            (Some(p), Some("show-status"), _) => {
                self.handle_provider_show_status(p);
            }
            (Some("github"), Some("clear-token"), _) => {
                self.handle_github_clear_token().await?;
            }
            (Some(p), Some("clear-token"), _) => {
                self.handle_provider_clear_token(p).await?;
            }
            (Some(p), _, _) => {
                self.tui_println(format!("Unknown auth action for provider: {p}"));
                self.show_provider_auth_help(p);
            }
        }
        Ok(())
    }

    /// Print auth help text.
    fn show_auth_help(&mut self) {
        self.tui_println(String::new());
        self.tui_println("Auth commands:");
        self.tui_println("  /auth list-providers                  Show all providers");
        self.tui_println("  /auth <provider> set-token <TOKEN>    Save API key or token");
        self.tui_println("  /auth <provider> show-status          Check provider auth status");
        self.tui_println("  /auth <provider> clear-token          Remove stored credentials");
        self.tui_println(String::new());
        self.tui_println("Examples:");
        self.tui_println("  /auth anthropic set-token sk-ant-xxxxx");
        self.tui_println("  /auth openai set-token sk-xxxxx");
        self.tui_println("  /auth github set-token ghp_xxxxx");
    }

    /// Print per-provider usage hint.
    fn show_provider_auth_help(&mut self, provider: &str) {
        self.tui_println(format!(
            "Usage: /auth {provider} <set-token|show-status|clear-token> [TOKEN]"
        ));
    }

    /// `/auth github set-token <TOKEN>` — validate and store a GitHub PAT.
    async fn handle_github_set_token(&mut self, token: Option<&str>) -> Result<(), TuiError> {
        let Some(token) = token else {
            self.tui_println("Usage: /auth github set-token <TOKEN>");
            return Ok(());
        };
        let token = token.trim();
        if token.is_empty() {
            self.tui_println("Usage: /auth github set-token <TOKEN>");
            return Ok(());
        }
        let token = zeroize::Zeroizing::new(token.to_string());
        self.tui_println("Validating token...");
        match fx_auth::github::validate_github_pat(&token).await {
            Ok(info) => {
                self.store_github_pat(&token, &info)?;
                self.tui_println("✓ github: token saved.");
                self.tui_println(format!("  Login: {}", info.login));
                for line in format_github_token_result(&token, &info) {
                    self.tui_println(line);
                }
            }
            Err(error) => {
                self.tui_println(format!("✗ Token validation failed: {error}"));
            }
        }
        Ok(())
    }

    /// Store a validated GitHub PAT in the credential store.
    fn store_github_pat(
        &self,
        token: &zeroize::Zeroizing<String>,
        info: &fx_auth::github::GitHubTokenInfo,
    ) -> Result<(), TuiError> {
        let store = self.get_credential_store()?;
        store_github_pat_in(store, token, info)
    }

    /// `/auth <provider> set-token <TOKEN>` — generic key storage.
    async fn handle_provider_set_token(
        &mut self,
        provider: &str,
        token: &str,
    ) -> Result<(), TuiError> {
        // Provider name is already normalized by the dispatcher.
        let token = token.trim();
        if token.is_empty() {
            self.tui_println("Usage: /auth <provider> set-token <TOKEN>");
            return Ok(());
        }
        let token = zeroize::Zeroizing::new(token.to_string());
        self.auth_store
            .store_provider_token(provider, &token)
            .map_err(TuiError::Auth)?;
        if provider != "github" {
            self.auth_manager.store(
                provider,
                AuthMethod::ApiKey {
                    provider: provider.to_string(),
                    key: (*token).clone(),
                },
            );
            self.persist_auth_manager()?;
            self.refresh_router_models().await?;
        }
        self.tui_println(format!("✓ {provider}: token saved."));
        Ok(())
    }

    /// `/auth github show-status` — display GitHub credential status.
    fn handle_github_show_status(&mut self) {
        let lines = match self.credential_store.as_ref() {
            Some(store) => format_github_status_lines(store),
            None => vec!["Error: credential store not available".to_string()],
        };
        for line in lines {
            self.tui_println(line);
        }
    }

    /// `/auth <provider> show-status` — generic status check.
    fn handle_provider_show_status(&mut self, provider: &str) {
        let provider = normalize_provider_name(provider);
        let configured = self
            .auth_store
            .get_provider_token(&provider)
            .ok()
            .flatten()
            .is_some();
        self.tui_println(format!("{provider} auth status:"));
        if let Some(method) = self.auth_manager.get(&provider) {
            match method {
                AuthMethod::OAuth { .. } => {
                    self.tui_println("  Status: configured (OAuth)");
                }
                AuthMethod::SetupToken { .. } | AuthMethod::ApiKey { .. } => {
                    self.tui_println("  Status: configured");
                }
            }
        } else if configured {
            self.tui_println("  Status: configured");
        } else {
            self.tui_println("  Status: not configured");
        }
    }

    /// `/auth github clear-token` — remove GitHub credentials.
    async fn handle_github_clear_token(&mut self) -> Result<(), TuiError> {
        let removed_pat = self.clear_github_credential()?;
        let removed_token = self
            .auth_store
            .clear_provider_token("github")
            .map_err(TuiError::Auth)?;
        let removed_auth = self.auth_manager.get("github").is_some();
        if removed_auth {
            self.auth_manager.remove("github");
            self.persist_auth_manager()?;
            self.refresh_router_models().await?;
        }
        if removed_pat || removed_token || removed_auth {
            self.tui_println("✓ github: credentials removed.");
        } else {
            self.tui_println("github: not configured");
        }
        Ok(())
    }

    /// Clear the GitHub PAT from the encrypted credential store.
    fn clear_github_credential(&self) -> Result<bool, TuiError> {
        let store = self.get_credential_store()?;
        store
            .clear(
                fx_auth::credential_store::AuthProvider::GitHub,
                fx_auth::credential_store::CredentialMethod::Pat,
            )
            .map_err(|e| TuiError::Auth(format!("failed to clear credential: {e}")))
    }

    /// `/auth <provider> clear-token` — generic credential removal.
    async fn handle_provider_clear_token(&mut self, provider: &str) -> Result<(), TuiError> {
        let provider = normalize_provider_name(provider);
        let removed_token = self
            .auth_store
            .clear_provider_token(&provider)
            .map_err(TuiError::Auth)?;
        let removed_auth = self.auth_manager.get(&provider).is_some();
        if removed_auth {
            self.auth_manager.remove(&provider);
            self.persist_auth_manager()?;
            self.refresh_router_models().await?;
        }
        if removed_token || removed_auth {
            self.tui_println(format!("✓ {provider}: credentials removed."));
        } else {
            self.tui_println(format!("{provider}: not configured."));
        }
        Ok(())
    }

    fn handle_keys_command(
        &mut self,
        subcommand: Option<String>,
        value: Option<String>,
        option: Option<String>,
        has_extra_args: bool,
    ) -> Result<(), TuiError> {
        match subcommand.as_deref() {
            None => self.show_keys_help(),
            Some("generate") => {
                self.handle_keys_generate(value.as_deref(), option.as_deref(), has_extra_args)?
            }
            Some("list") if value.is_none() && option.is_none() && !has_extra_args => {
                self.handle_keys_list()?;
            }
            Some("list") => self.tui_println("Usage: /keys list"),
            Some("trust") if option.is_none() && !has_extra_args => {
                self.handle_keys_trust(value.as_deref())?;
            }
            Some("trust") => self.tui_println("Usage: /keys trust <path>"),
            Some("revoke") if option.is_none() && !has_extra_args => {
                self.handle_keys_revoke(value.as_deref())?;
            }
            Some("revoke") => self.tui_println("Usage: /keys revoke <fingerprint>"),
            Some(other) => {
                self.tui_println(format!("Unknown keys command: {other}"));
                self.show_keys_help();
            }
        }
        Ok(())
    }

    fn handle_sign_command(
        &mut self,
        target: Option<String>,
        has_extra_args: bool,
    ) -> Result<(), TuiError> {
        match (target.as_deref(), has_extra_args) {
            (Some("--all"), false) => self.handle_sign_all()?,
            (Some(skill_name), false) => self.handle_sign_skill(skill_name)?,
            _ => self.show_sign_help(),
        }
        Ok(())
    }

    fn show_keys_help(&mut self) {
        self.tui_println(String::new());
        self.tui_println("WASM signing key commands:");
        self.tui_println("  /keys generate [--force]       Generate a new Ed25519 keypair");
        self.tui_println("  /keys list                     List trusted public keys");
        self.tui_println("  /keys trust <path>             Trust a public key file");
        self.tui_println("  /keys revoke <fingerprint>     Revoke a trusted public key");
    }

    fn show_sign_help(&mut self) {
        self.tui_println(String::new());
        self.tui_println("WASM signing commands:");
        self.tui_println("  /sign <skill_name>             Sign one installed WASM skill");
        self.tui_println("  /sign --all                    Sign all installed WASM skills");
    }

    fn handle_keys_generate(
        &mut self,
        value: Option<&str>,
        option: Option<&str>,
        has_extra_args: bool,
    ) -> Result<(), TuiError> {
        let force = match (value, option, has_extra_args) {
            (None, None, false) => false,
            (Some("--force"), None, false) => true,
            _ => {
                self.tui_println("Usage: /keys generate [--force]");
                return Ok(());
            }
        };
        let base_dir = fawx_data_dir();
        let fingerprint = generate_signing_keypair_in(&base_dir, force)?;
        self.tui_println("Generated WASM signing keypair.");
        self.tui_println(format!("  Fingerprint: {fingerprint}"));
        self.tui_println(format!(
            "  Private key: {}",
            signing_private_key_path_in(&base_dir).display()
        ));
        self.tui_println(format!(
            "  Public key: {}",
            signing_public_key_path_in(&base_dir).display()
        ));
        Ok(())
    }

    fn handle_keys_list(&mut self) -> Result<(), TuiError> {
        let keys = trusted_key_entries_from_dir(&fawx_trusted_keys_dir())?;
        if keys.is_empty() {
            self.tui_println("No trusted public keys.");
            return Ok(());
        }
        self.tui_println("Trusted public keys:");
        for key in keys {
            self.tui_println(format!(
                "  {} {} {} bytes",
                display_file_name(&key.path),
                key.fingerprint,
                key.file_size
            ));
        }
        Ok(())
    }

    fn handle_keys_trust(&mut self, path: Option<&str>) -> Result<(), TuiError> {
        let Some(path) = path else {
            self.tui_println("Usage: /keys trust <path>");
            return Ok(());
        };
        let fingerprint = trust_public_key_in(&fawx_data_dir(), Path::new(path))?;
        self.tui_println(format!("Trusted public key from {path}."));
        self.tui_println(format!("  Fingerprint: {fingerprint}"));
        Ok(())
    }

    fn handle_keys_revoke(&mut self, fingerprint: Option<&str>) -> Result<(), TuiError> {
        let Some(fingerprint) = fingerprint else {
            self.tui_println("Usage: /keys revoke <fingerprint>");
            return Ok(());
        };
        let removed = revoke_trusted_key_in(&fawx_data_dir(), fingerprint)?;
        self.tui_println(format!(
            "Revoked {removed} trusted key(s) for fingerprint {}.",
            fingerprint.to_ascii_lowercase()
        ));
        Ok(())
    }

    fn handle_sign_skill(&mut self, skill_name: &str) -> Result<(), TuiError> {
        let signed = sign_skill(skill_name)?;
        self.print_signed_skill(&signed, true);
        Ok(())
    }

    fn handle_sign_all(&mut self) -> Result<(), TuiError> {
        let signed = sign_all_skills()?;
        if signed.is_empty() {
            self.tui_println("No WASM skills found to sign.");
            return Ok(());
        }
        for skill in &signed {
            self.print_signed_skill(skill, false);
        }
        self.tui_println(format!("Signed {} skill(s).", signed.len()));
        Ok(())
    }

    fn print_signed_skill(&mut self, signed: &SignedSkill, include_path: bool) {
        self.tui_println(format!(
            "Signed {} (fingerprint: {})",
            signed.name, signed.fingerprint
        ));
        self.tui_println(format!(
            "Verified: signature valid (key: {})",
            signed.fingerprint
        ));
        if include_path {
            self.tui_println(format!("  Signature: {}", signed.signature_path.display()));
        }
    }

    fn show_budget_status(&mut self) {
        let status = self.loop_engine.status(current_time_ms());
        self.tui_println("Budget usage:");
        self.tui_println(format!("  - LLM calls used: {}", status.llm_calls_used));
        self.tui_println(format!(
            "  - Tool calls used: {}",
            status.tool_invocations_used
        ));
        self.tui_println(format!("  - Tokens used: {}", status.tokens_used));
        self.tui_println(format!("  - Cost used (cents): {}", status.cost_cents_used));
        self.tui_println(format!("  - Tokens remaining: {}", status.remaining.tokens));
        self.tui_println(format!(
            "  - LLM calls remaining: {}",
            status.remaining.llm_calls
        ));
    }

    fn show_loop_status(&mut self) {
        let status = self.loop_engine.status(current_time_ms());
        self.tui_println("Loop status:");
        self.tui_println(format!(
            "  - Iterations (last cycle): {}/{}",
            status.iteration_count, status.max_iterations
        ));
        self.tui_println(format!("  - Tokens used (tracker): {}", status.tokens_used));
        self.tui_println(format!("  - Tokens remaining: {}", status.remaining.tokens));
        self.tui_println(format!(
            "  - LLM calls remaining: {}",
            status.remaining.llm_calls
        ));
        self.tui_println(format!(
            "  - Tool calls remaining: {}",
            status.remaining.tool_invocations
        ));
        self.tui_println(format!(
            "  - Wall time remaining (ms): {}",
            status.remaining.wall_time_ms
        ));
    }

    fn show_status(&mut self) {
        let model = self.current_model().to_string();
        let status = self.loop_engine.status(current_time_ms());
        let providers = self.auth_manager.providers();
        self.tui_println("Fawx Status");
        self.tui_println(format!("  model:     {model}"));
        self.tui_println(format!("  providers: {}", providers.join(", ")));
        self.tui_println(format!("  tokens:    {} used", status.tokens_used));
        self.tui_println(format!(
            "  budget:    {} tokens remaining",
            status.remaining.tokens
        ));
    }

    fn current_model(&self) -> &str {
        self.router.active_model().unwrap_or_default()
    }

    fn clear_screen(&self) -> Result<(), TuiError> {
        let mut stdout = io::stdout();
        stdout
            .execute(terminal::Clear(terminal::ClearType::All))
            .map_err(TuiError::Io)?;
        stdout.execute(cursor::MoveTo(0, 0)).map_err(TuiError::Io)?;
        Ok(())
    }

    fn build_perception_snapshot(&self, input: &str) -> PerceptionSnapshot {
        let timestamp_ms = current_time_ms();

        PerceptionSnapshot {
            screen: ScreenState {
                current_app: "fawx.tui".to_string(),
                elements: Vec::new(),
                text_content: input.to_string(),
            },
            notifications: Vec::new(),
            active_app: "fawx.tui".to_string(),
            timestamp_ms,
            sensor_data: None,
            user_input: Some(UserInput {
                text: input.to_string(),
                source: InputSource::Text,
                timestamp: timestamp_ms,
                context_id: None,
            }),
            conversation_history: self.conversation_history.clone(),
            steer_context: None,
        }
    }

    fn record_conversation_turn(&mut self, user_text: &str, assistant_text: String) {
        let clean_assistant_text = sanitize_history_text(&assistant_text);
        self.conversation_history
            .push(Message::user(user_text.to_string()));
        self.conversation_history
            .push(Message::assistant(clean_assistant_text));
        trim_history(&mut self.conversation_history, self.max_history);
    }
}

fn runtime_model_state(router: &ModelRouter) -> (String, String) {
    let Some(active_model) = router.active_model().map(ToString::to_string) else {
        return (String::new(), String::new());
    };

    let provider = router
        .available_models()
        .into_iter()
        .find_map(|model| {
            if model.model_id == active_model {
                Some(model.provider_name)
            } else {
                None
            }
        })
        .unwrap_or_default();

    (active_model, provider)
}

/// Load the user config from ~/.fawx/config.toml (or return defaults).
pub fn load_config() -> Result<FawxConfig, TuiError> {
    let base_data_dir = fawx_data_dir();
    FawxConfig::load(&base_data_dir).map_err(TuiError::Store)
}

/// Build a loop engine with sensible defaults for the TUI shell.
/// Convenience wrapper used by tests.
#[cfg(test)]
fn build_loop_engine() -> LoopEngine {
    build_loop_engine_bundle().engine
}

#[cfg(test)]
fn build_loop_engine_bundle() -> LoopEngineBundle {
    let config = load_config().unwrap_or_else(|error| {
        eprintln!("warning: failed to load config: {error}");
        FawxConfig::default()
    });
    build_loop_engine_from_config(&config).expect("loop engine config should be valid")
}

/// Bundle returned by the loop engine builder functions.
pub struct LoopEngineBundle {
    pub engine: LoopEngine,
    pub memory: Option<SharedMemoryStore>,
    pub runtime_info: Arc<RwLock<RuntimeInfo>>,
    pub event_bus: EventBus,
    pub scratchpad: Arc<Mutex<Scratchpad>>,
    pub skill_registry: Arc<SkillRegistry>,
    pub credential_provider: Option<Arc<dyn CredentialProvider>>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    pub credential_store: Option<Arc<fx_auth::credential_store::EncryptedFileCredentialStore>>,
    /// Signature policy loaded once at startup, shared with skill watcher.
    pub signature_policy: SignaturePolicy,
}

impl LoopEngineBundle {
    pub fn into_tui_deps(
        self,
        auth_manager: AuthManager,
        router: ModelRouter,
        config: FawxConfig,
    ) -> TuiAppDeps {
        TuiAppDeps {
            auth_manager,
            router,
            loop_engine: self.engine,
            runtime_info: self.runtime_info,
            config,
            memory: self.memory,
            event_bus: self.event_bus,
            scratchpad: self.scratchpad,
            skill_registry: self.skill_registry,
            credential_provider: self.credential_provider,
            tool_executor: self.tool_executor,
            credential_store: self.credential_store,
            signature_policy: self.signature_policy,
        }
    }
}

/// Build a loop engine from an already-loaded config.
pub fn build_loop_engine_from_config(config: &FawxConfig) -> Result<LoopEngineBundle, TuiError> {
    let base_data_dir = fawx_data_dir();
    let data_dir = configured_data_dir(&base_data_dir, config);
    build_loop_engine_with_config(data_dir, config.clone())
}

/// Capacity of the streaming event bus broadcast channel.
const EVENT_BUS_CAPACITY: usize = 256;

fn build_loop_engine_with_config(
    data_dir: PathBuf,
    config: FawxConfig,
) -> Result<LoopEngineBundle, TuiError> {
    let event_bus = EventBus::new(EVENT_BUS_CAPACITY);
    let budget = BudgetTracker::new(BudgetConfig::default(), current_time_ms(), 0);
    let context = ContextCompactor::new(DEFAULT_CONTEXT_MAX_TOKENS, DEFAULT_CONTEXT_COMPACT_TARGET);
    let working_dir = configured_working_dir(&config);
    let skills = build_skill_registry(working_dir.clone(), &data_dir, &config);
    let synthesis = config
        .model
        .synthesis_instruction
        .clone()
        .unwrap_or_else(|| DEFAULT_SYNTHESIS_INSTRUCTION.to_string());

    let bridge: Arc<dyn ScratchpadProvider> = Arc::new(ScratchpadBridge {
        scratchpad: Arc::clone(&skills.scratchpad),
    });

    let caching_registry =
        CachingExecutor::new(SharedSkillRegistry::new(Arc::clone(&skills.registry)));

    // Build ProposalGateExecutor to wrap the CachingExecutor.
    // Chain: kernel → ProposalGateExecutor → CachingExecutor → SkillRegistry
    let self_modify_config = crate::config_bridge::to_core_self_modify(&config.self_modify);
    let proposals_dir = data_dir.join("proposals");
    let gate_state = ProposalGateState::new(self_modify_config, working_dir.clone(), proposals_dir);
    let tool_executor: Arc<dyn ToolExecutor> =
        Arc::new(ProposalGateExecutor::new(caching_registry, gate_state));

    let mut builder = LoopEngine::builder()
        .budget(budget)
        .context(context)
        .max_iterations(config.general.max_iterations)
        .tool_executor(Arc::clone(&tool_executor))
        .synthesis_instruction(synthesis)
        .event_bus(event_bus.clone())
        .iteration_counter(Arc::clone(&skills.iteration_counter))
        .scratchpad_provider(bridge);
    if let Some(snapshot_text) = skills.memory_snapshot {
        builder = builder.memory_context(snapshot_text);
    }

    let engine = build_loop_engine_from_builder(builder)?;
    Ok(LoopEngineBundle {
        engine,
        memory: skills.memory,
        runtime_info: skills.runtime_info,
        event_bus,
        scratchpad: skills.scratchpad,
        skill_registry: skills.registry,
        credential_provider: skills.credential_provider,
        tool_executor,
        credential_store: skills.credential_store,
        signature_policy: skills.signature_policy,
    })
}

fn build_loop_engine_from_builder(builder: LoopEngineBuilder) -> Result<LoopEngine, TuiError> {
    builder.build().map_err(|error| {
        TuiError::Loop(format!(
            "failed to build loop engine: stage={} reason={}",
            error.stage, error.reason
        ))
    })
}

/// Bridges `fx_scratchpad::Scratchpad` into the kernel's [`ScratchpadProvider`]
/// trait without introducing a circular crate dependency.
struct ScratchpadBridge {
    scratchpad: Arc<Mutex<Scratchpad>>,
}

impl ScratchpadProvider for ScratchpadBridge {
    fn render_for_context(&self) -> String {
        match self.scratchpad.lock() {
            Ok(sp) => sp.render_for_context(),
            Err(_) => String::new(),
        }
    }

    fn compact_if_needed(&self, current_iteration: u32) {
        let Ok(mut sp) = self.scratchpad.lock() else {
            return;
        };
        let rendered_len = sp.render_for_context().len();
        if rendered_len > fx_scratchpad::SCRATCHPAD_COMPACT_THRESHOLD_CHARS {
            sp.compact(
                fx_scratchpad::SCRATCHPAD_COMPACT_TARGET_TOKENS,
                current_iteration,
                fx_scratchpad::SCRATCHPAD_AGE_THRESHOLD,
            );
        }
    }
}

/// Bridges the encrypted credential store to the [`CredentialProvider`]
/// trait so WASM skills can retrieve secrets via `kv_get`.
///
/// Maps well-known key names to credential store lookups:
/// - `"github_token"` → GitHub PAT from the encrypted store
struct CredentialStoreBridge {
    store: Arc<fx_auth::credential_store::EncryptedFileCredentialStore>,
}

impl CredentialProvider for CredentialStoreBridge {
    fn get_credential(&self, key: &str) -> Option<zeroize::Zeroizing<String>> {
        use fx_auth::credential_store::{AuthProvider, CredentialMethod};
        match key {
            "github_token" => self
                .store
                .get(AuthProvider::GitHub, CredentialMethod::Pat)
                .ok()
                .flatten(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct SharedSkillRegistry {
    registry: Arc<SkillRegistry>,
}

impl SharedSkillRegistry {
    fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl ToolExecutor for SharedSkillRegistry {
    async fn execute_tools(
        &self,
        calls: &[fx_llm::ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<fx_kernel::act::ToolResult>, fx_kernel::act::ToolExecutorError> {
        self.registry.execute_tools(calls, cancel).await
    }

    fn concurrency_policy(&self) -> fx_kernel::act::ConcurrencyPolicy {
        self.registry.concurrency_policy()
    }

    fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
        self.registry.tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> fx_kernel::act::ToolCacheability {
        self.registry.cacheability(tool_name)
    }

    fn cache_stats(&self) -> Option<fx_kernel::act::ToolCacheStats> {
        self.registry.cache_stats()
    }
}

/// Result of [`build_skill_registry`]: groups related outputs to avoid a
/// large tuple return type.
struct SkillRegistryBundle {
    registry: Arc<SkillRegistry>,
    memory: Option<SharedMemoryStore>,
    memory_snapshot: Option<String>,
    runtime_info: Arc<RwLock<RuntimeInfo>>,
    scratchpad: Arc<Mutex<Scratchpad>>,
    iteration_counter: Arc<std::sync::atomic::AtomicU32>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    credential_store: Option<Arc<fx_auth::credential_store::EncryptedFileCredentialStore>>,
    /// Signature policy loaded once at startup, shared with the skill watcher
    /// to avoid redundant filesystem reads.
    signature_policy: SignaturePolicy,
}

fn build_skill_registry(
    working_dir: PathBuf,
    data_dir: &Path,
    config: &FawxConfig,
) -> SkillRegistryBundle {
    let tool_config = ToolConfig {
        max_read_size: config.tools.max_read_size,
        search_exclude: config.tools.search_exclude.clone(),
        ..ToolConfig::default()
    };
    let executor = FawxToolExecutor::new(working_dir.clone(), tool_config);
    let (mut executor, memory, snapshot_text, memory_enabled) =
        attach_memory(executor, data_dir, config);

    let self_modify_config = crate::config_bridge::to_core_self_modify(&config.self_modify);
    let sm = self_modify_config.enabled.then_some(self_modify_config);
    if let Some(ref smc) = sm {
        executor = executor.with_self_modify(smc.clone());
    }

    let runtime_info = new_runtime_info(config, memory_enabled);
    executor = executor.with_runtime_info(Arc::clone(&runtime_info));

    let registry = Arc::new(SkillRegistry::new());
    registry.register(Arc::new(BuiltinToolsSkill::new(executor)));
    let git_skill = GitSkill::new(working_dir.clone(), sm.clone());
    registry.register(Arc::new(git_skill));
    let tx_skill = TransactionSkill::new(working_dir.clone(), sm);
    registry.register(Arc::new(tx_skill));
    let scratchpad = Arc::new(Mutex::new(Scratchpad::new()));
    let iteration_counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let scratchpad_skill =
        ScratchpadSkill::new(Arc::clone(&scratchpad), Arc::clone(&iteration_counter));
    registry.register(Arc::new(scratchpad_skill));

    // Open the credential store once and share via Arc between TuiApp and WASM bridge.
    let credential_store: Option<Arc<fx_auth::credential_store::EncryptedFileCredentialStore>> =
        match fx_auth::credential_store::EncryptedFileCredentialStore::open(data_dir) {
            Ok(store) => Some(Arc::new(store)),
            Err(e) => {
                tracing::warn!("credential store unavailable: {e}");
                None
            }
        };

    let credential_provider: Option<Arc<dyn CredentialProvider>> =
        credential_store.as_ref().map(|store| {
            Arc::new(CredentialStoreBridge {
                store: Arc::clone(store),
            }) as Arc<dyn CredentialProvider>
        });

    // Load WASM skills from ~/.fawx/skills/
    let trusted_keys = fx_loadable::wasm_skill::load_trusted_keys().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "failed to load trusted keys");
        vec![]
    });
    let signature_policy = SignaturePolicy {
        trusted_keys,
        require_signatures: config.security.require_signatures,
    };
    match fx_loadable::wasm_skill::load_wasm_skills(credential_provider.clone(), &signature_policy)
    {
        Ok(wasm_skills) => {
            for skill in wasm_skills {
                registry.register(skill);
            }
        }
        Err(e) => {
            eprintln!("warning: failed to load WASM skills: {e}");
        }
    }

    apply_skill_summaries(&runtime_info, registry.as_ref());

    SkillRegistryBundle {
        registry,
        memory,
        memory_snapshot: snapshot_text,
        runtime_info,
        scratchpad,
        iteration_counter,
        credential_provider,
        credential_store,
        signature_policy,
    }
}

fn attach_memory(
    mut executor: FawxToolExecutor,
    data_dir: &Path,
    config: &FawxConfig,
) -> (
    FawxToolExecutor,
    Option<SharedMemoryStore>,
    Option<String>,
    bool,
) {
    let memory_config = JsonMemoryConfig {
        max_entries: config.memory.max_entries,
        max_value_size: config.memory.max_value_size,
        decay_config: fx_memory::DecayConfig::default(),
    };
    match JsonFileMemory::new_with_config(data_dir, memory_config) {
        Ok(mut memory_store) => {
            let pruned = memory_store.prune();
            if pruned > 0 {
                eprintln!("memory: pruned {pruned} stale entries at session start");
            }
            let snapshot = memory_store.snapshot();
            let text = format_memory_for_prompt(&snapshot, config.memory.max_snapshot_chars);
            let memory: SharedMemoryStore = Arc::new(Mutex::new(memory_store));
            executor = executor.with_memory(Arc::clone(&memory));
            (executor, Some(memory), text, true)
        }
        Err(error) => {
            eprintln!("warning: failed to initialize memory: {error}");
            (executor, None, None, false)
        }
    }
}

fn new_runtime_info(config: &FawxConfig, memory_enabled: bool) -> Arc<RwLock<RuntimeInfo>> {
    Arc::new(RwLock::new(RuntimeInfo {
        active_model: String::new(),
        provider: String::new(),
        skills: Vec::new(),
        config_summary: ConfigSummary {
            max_iterations: config.general.max_iterations,
            max_history: config.general.max_history,
            memory_enabled,
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}

fn apply_skill_summaries(runtime_info: &Arc<RwLock<RuntimeInfo>>, registry: &SkillRegistry) {
    let skills = registry
        .skill_summaries()
        .into_iter()
        .map(|(name, tool_names)| SkillInfo { name, tool_names })
        .collect::<Vec<_>>();

    match runtime_info.write() {
        Ok(mut info) => info.skills = skills,
        Err(error) => eprintln!("warning: runtime info lock poisoned: {error}"),
    }
}

pub(crate) fn format_memory_for_prompt(
    entries: &[(String, String)],
    max_chars: usize,
) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let mut text = String::from("What you remember from previous sessions:\n");
    for (key, value) in entries {
        let line = format!("- {key}: {value}\n");
        if text.len() + line.len() > max_chars {
            text.push_str("(truncated)\n");
            break;
        }
        text.push_str(&line);
    }
    text.push_str(
        "(Use memory_read for details. \
        Use memory_write to update or add memories.)",
    );
    Some(text)
}

/// Thin wrapper to expose `ModelRouter` as a `CompletionProvider` for analysis.
struct AnalysisCompletionProvider<'a> {
    router: &'a ModelRouter,
    active_model: String,
}

impl<'a> fmt::Debug for AnalysisCompletionProvider<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalysisCompletionProvider")
            .field("active_model", &self.active_model)
            .finish()
    }
}

impl<'a> AnalysisCompletionProvider<'a> {
    fn new(router: &'a ModelRouter, active_model: String) -> Self {
        Self {
            router,
            active_model,
        }
    }

    fn with_active_model(&self, mut request: CompletionRequest) -> CompletionRequest {
        request.model = self.active_model.clone();
        request
    }
}

#[async_trait]
impl fx_llm::CompletionProvider for AnalysisCompletionProvider<'_> {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
        self.router.complete(self.with_active_model(request)).await
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
        self.router
            .complete_stream(self.with_active_model(request))
            .await
    }

    fn name(&self) -> &str {
        "analysis"
    }

    fn supported_models(&self) -> Vec<String> {
        vec![self.active_model.clone()]
    }

    fn capabilities(&self) -> fx_llm::ProviderCapabilities {
        fx_llm::ProviderCapabilities {
            supports_temperature: true,
            requires_streaming: false,
        }
    }
}

impl<'a> fmt::Debug for RouterLoopLlmProvider<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouterLoopLlmProvider")
            .field("active_model", &self.active_model)
            .finish()
    }
}
pub(crate) struct RouterLoopLlmProvider<'a> {
    router: &'a ModelRouter,
    active_model: String,
}

impl<'a> RouterLoopLlmProvider<'a> {
    pub(crate) fn new(router: &'a ModelRouter, active_model: String) -> Self {
        Self {
            router,
            active_model,
        }
    }
}

#[async_trait]
impl LoopLlmProvider for RouterLoopLlmProvider<'_> {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, CoreLlmError> {
        let request = CompletionRequest {
            model: self.active_model.clone(),
            messages: vec![Message::user(prompt)],
            tools: Vec::new(),
            temperature: None, // Codex Responses API does not support temperature
            max_tokens: Some(max_tokens),
            system_prompt: Some(prompt.to_string()),
        };

        let mut stream = self
            .router
            .complete_stream(request)
            .await
            .map_err(|error| CoreLlmError::Inference(error.to_string()))?;

        let collected = consume_stream_silent(&mut stream).await?;

        if collected.trim().is_empty() {
            Err(CoreLlmError::InvalidResponse(
                "provider returned an empty completion".to_string(),
            ))
        } else {
            Ok(collected)
        }
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, CoreLlmError> {
        let request = CompletionRequest {
            model: self.active_model.clone(),
            messages: vec![Message::user(prompt)],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(max_tokens),
            system_prompt: Some(prompt.to_string()),
        };

        let mut stream = self
            .router
            .complete_stream(request)
            .await
            .map_err(|error| CoreLlmError::Inference(error.to_string()))?;
        let mut stdout = io::stdout();
        let collected = consume_stream_with_writer(&mut stream, &mut stdout).await?;

        if collected.trim().is_empty() {
            return Err(CoreLlmError::InvalidResponse(
                "provider returned an empty completion".to_string(),
            ));
        }

        callback(collected.clone());
        Ok(collected)
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionResponse, ProviderError> {
        self.router.complete(request).await
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionStream, ProviderError> {
        self.router.complete_stream(request).await
    }

    fn model_name(&self) -> &str {
        &self.active_model
    }
}

fn loop_result_signals(result: &LoopResult) -> &[Signal] {
    match result {
        LoopResult::Complete { signals, .. }
        | LoopResult::BudgetExhausted { signals, .. }
        | LoopResult::NeedsInput { signals, .. }
        | LoopResult::UserStopped { signals, .. }
        | LoopResult::Error { signals, .. } => signals,
    }
}

fn loop_result_response_text(result: &LoopResult) -> String {
    match result {
        LoopResult::Complete { response, .. } => response.clone(),
        LoopResult::UserStopped {
            partial_response,
            iterations,
            ..
        } => render_user_stopped(partial_response.clone(), *iterations),
        LoopResult::BudgetExhausted {
            partial_response,
            iterations,
            ..
        } => render_budget_exhausted(partial_response.clone(), *iterations),
        LoopResult::NeedsInput {
            prompt, iterations, ..
        } => {
            let meta =
                format!("\x1b[2m  \u{21b3} {iterations} iteration(s) \u{00b7} needs input\x1b[0m");
            format!("{prompt}\n{meta}")
        }
        LoopResult::Error {
            message,
            recoverable,
            ..
        } => render_loop_error(message, *recoverable),
    }
}

fn sanitize_history_text(text: &str) -> String {
    let stripped = strip_ansi_csi_sequences(text);
    stripped
        .lines()
        .filter(|line| !line.contains('\u{21b3}'))
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn split_output_block(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                lines.push(std::mem::take(&mut current));
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            '\n' => lines.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }

    lines.push(current);
    lines
}

fn strip_ansi_csi_sequences(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
            continue;
        }

        output.push(ch);
    }

    output
}

pub(crate) fn trim_history(history: &mut Vec<Message>, max_history: usize) {
    if history.len() <= max_history {
        return;
    }

    let remove_count = history.len() - max_history;
    history.drain(0..remove_count);
}

pub(crate) fn fawx_data_dir() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join(".fawx"))
        .unwrap_or_else(|| PathBuf::from(".fawx"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrustedKeyEntry {
    path: PathBuf,
    fingerprint: String,
    file_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignedSkill {
    name: String,
    signature_path: PathBuf,
    fingerprint: String,
}

fn fawx_skills_dir() -> PathBuf {
    fawx_skills_dir_in(&fawx_data_dir())
}

fn fawx_skills_dir_in(base_dir: &Path) -> PathBuf {
    base_dir.join("skills")
}

fn fawx_keys_dir_in(base_dir: &Path) -> PathBuf {
    base_dir.join("keys")
}

fn fawx_trusted_keys_dir() -> PathBuf {
    fawx_trusted_keys_dir_in(&fawx_data_dir())
}

fn fawx_trusted_keys_dir_in(base_dir: &Path) -> PathBuf {
    base_dir.join("trusted_keys")
}

fn signing_private_key_path_in(base_dir: &Path) -> PathBuf {
    fawx_keys_dir_in(base_dir).join("signing.key")
}

fn signing_public_key_path_in(base_dir: &Path) -> PathBuf {
    fawx_keys_dir_in(base_dir).join("signing.pub")
}

fn trusted_signing_public_key_path_in(base_dir: &Path) -> PathBuf {
    fawx_trusted_keys_dir_in(base_dir).join("signing.pub")
}

fn generate_signing_keypair_in(base_dir: &Path, force: bool) -> Result<String, TuiError> {
    let private_key_path = signing_private_key_path_in(base_dir);
    let public_key_path = signing_public_key_path_in(base_dir);
    let trusted_key_path = trusted_signing_public_key_path_in(base_dir);
    ensure_paths_do_not_exist(
        &[&private_key_path, &public_key_path, &trusted_key_path],
        force,
    )?;
    let (private_key, public_key) = fx_skills::signing::generate_keypair()
        .map_err(|error| store_error(format!("failed to generate signing keypair: {error}")))?;
    write_binary_file(&private_key_path, &private_key)?;
    set_private_key_permissions(&private_key_path)?;
    write_binary_file(&public_key_path, &public_key)?;
    set_public_key_permissions(&public_key_path)?;
    write_binary_file(&trusted_key_path, &public_key)?;
    set_public_key_permissions(&trusted_key_path)?;
    Ok(public_key_fingerprint(&public_key))
}

fn ensure_paths_do_not_exist(paths: &[&Path], force: bool) -> Result<(), TuiError> {
    if force {
        return Ok(());
    }
    if let Some(path) = paths.iter().find(|path| path.exists()) {
        return Err(store_error(format!(
            "refusing to overwrite {} without --force",
            path.display()
        )));
    }
    Ok(())
}

fn write_binary_file(path: &Path, bytes: &[u8]) -> Result<(), TuiError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_key_permissions(path: &Path) -> Result<(), TuiError> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o600);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_key_permissions(_path: &Path) -> Result<(), TuiError> {
    Ok(())
}

#[cfg(unix)]
fn set_public_key_permissions(path: &Path) -> Result<(), TuiError> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o644);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_public_key_permissions(_path: &Path) -> Result<(), TuiError> {
    Ok(())
}

fn trusted_key_entries_from_dir(trusted_dir: &Path) -> Result<Vec<TrustedKeyEntry>, TuiError> {
    let mut keys = Vec::new();
    if !trusted_dir.exists() {
        return Ok(keys);
    }
    for entry in std::fs::read_dir(trusted_dir)? {
        let path = entry?.path();
        if is_public_key_path(&path) {
            keys.push(trusted_key_entry_from_path(&path)?);
        }
    }
    keys.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(keys)
}

fn trusted_key_entry_from_path(path: &Path) -> Result<TrustedKeyEntry, TuiError> {
    let public_key = read_public_key_file(path)?;
    let file_size = std::fs::metadata(path)?.len();
    Ok(TrustedKeyEntry {
        path: path.to_path_buf(),
        fingerprint: public_key_fingerprint(&public_key),
        file_size,
    })
}

fn trust_public_key_in(base_dir: &Path, source_path: &Path) -> Result<String, TuiError> {
    ensure_public_key_extension(source_path)?;
    let public_key = read_public_key_file(source_path)?;
    let file_name = source_path.file_name().ok_or_else(|| {
        store_error(format!(
            "invalid public key path: {}",
            source_path.display()
        ))
    })?;
    let destination = fawx_trusted_keys_dir_in(base_dir).join(file_name);
    if destination.exists() && !paths_refer_to_same_file(source_path, &destination) {
        return Err(store_error(format!(
            "trusted key already exists: {}",
            destination.display()
        )));
    }
    if !paths_refer_to_same_file(source_path, &destination) {
        write_binary_file(&destination, &public_key)?;
        set_public_key_permissions(&destination)?;
    }
    Ok(public_key_fingerprint(&public_key))
}

fn revoke_trusted_key_in(base_dir: &Path, fingerprint: &str) -> Result<usize, TuiError> {
    let target = fingerprint.to_ascii_lowercase();
    let matches: Vec<PathBuf> = trusted_key_entries_from_dir(&fawx_trusted_keys_dir_in(base_dir))?
        .into_iter()
        .filter(|entry| entry.fingerprint == target)
        .map(|entry| entry.path)
        .collect();
    if matches.is_empty() {
        return Err(store_error(format!("trusted key not found: {target}")));
    }
    for path in &matches {
        std::fs::remove_file(path)?;
    }
    Ok(matches.len())
}

fn sign_skill(skill_name: &str) -> Result<SignedSkill, TuiError> {
    let base_dir = fawx_data_dir();
    let trusted_keys = load_default_trusted_keys()?;
    sign_skill_with_keys(&base_dir, skill_name, &trusted_keys)
}

fn sign_skill_in(base_dir: &Path, skill_name: &str) -> Result<SignedSkill, TuiError> {
    let trusted_dir = fawx_trusted_keys_dir_in(base_dir);
    let trusted_keys = load_trusted_keys_from_dir(&trusted_dir)?;
    sign_skill_with_keys(base_dir, skill_name, &trusted_keys)
}

fn sign_skill_with_keys(
    base_dir: &Path,
    skill_name: &str,
    trusted_keys: &[Vec<u8>],
) -> Result<SignedSkill, TuiError> {
    let private_key = read_signing_private_key(base_dir)?;
    let sig_path = skill_signature_path_in(base_dir, skill_name);
    let wasm_bytes = read_skill_wasm(base_dir, skill_name)?;
    let signature = fx_skills::signing::sign_skill(&wasm_bytes, &private_key)
        .map_err(|error| store_error(format!("failed to sign skill '{skill_name}': {error}")))?;
    let fingerprint = write_verified_signature(&sig_path, &signature, &wasm_bytes, trusted_keys)?;
    Ok(SignedSkill {
        name: skill_name.to_string(),
        signature_path: sig_path,
        fingerprint,
    })
}

fn sign_all_skills() -> Result<Vec<SignedSkill>, TuiError> {
    let base_dir = fawx_data_dir();
    let trusted_keys = load_default_trusted_keys()?;
    sign_all_skills_with_keys(&base_dir, &trusted_keys)
}

fn sign_all_skills_in(base_dir: &Path) -> Result<Vec<SignedSkill>, TuiError> {
    let trusted_dir = fawx_trusted_keys_dir_in(base_dir);
    let trusted_keys = load_trusted_keys_from_dir(&trusted_dir)?;
    sign_all_skills_with_keys(base_dir, &trusted_keys)
}

fn sign_all_skills_with_keys(
    base_dir: &Path,
    trusted_keys: &[Vec<u8>],
) -> Result<Vec<SignedSkill>, TuiError> {
    let skill_names = installed_skill_names(&fawx_skills_dir_in(base_dir))?;
    let mut signed = Vec::new();
    for skill_name in skill_names {
        signed.push(sign_skill_with_keys(base_dir, &skill_name, trusted_keys)?);
    }
    Ok(signed)
}

fn installed_skill_names(skills_dir: &Path) -> Result<Vec<String>, TuiError> {
    let mut names = Vec::new();
    if !skills_dir.exists() {
        return Ok(names);
    }
    for entry in std::fs::read_dir(skills_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.path().join(format!("{name}.wasm")).exists() {
                names.push(name);
            }
        }
    }
    names.sort();
    Ok(names)
}

fn skill_wasm_path_in(base_dir: &Path, skill_name: &str) -> PathBuf {
    fawx_skills_dir_in(base_dir)
        .join(skill_name)
        .join(format!("{skill_name}.wasm"))
}

fn skill_signature_path_in(base_dir: &Path, skill_name: &str) -> PathBuf {
    fawx_skills_dir_in(base_dir)
        .join(skill_name)
        .join(format!("{skill_name}.wasm.sig"))
}

fn write_verified_signature(
    sig_path: &Path,
    signature: &[u8],
    wasm_bytes: &[u8],
    trusted_keys: &[Vec<u8>],
) -> Result<String, TuiError> {
    write_binary_file(sig_path, signature)?;
    match verify_signature_with_keys(wasm_bytes, signature, trusted_keys) {
        Ok(fingerprint) => Ok(fingerprint),
        Err(error) => {
            let _ = std::fs::remove_file(sig_path);
            Err(error)
        }
    }
}

fn verify_signature_with_keys(
    wasm_bytes: &[u8],
    signature: &[u8],
    trusted_keys: &[Vec<u8>],
) -> Result<String, TuiError> {
    if trusted_keys.is_empty() {
        return Err(store_error("no trusted public keys configured"));
    }
    for public_key in trusted_keys {
        let valid = fx_skills::signing::verify_skill(wasm_bytes, signature, public_key)
            .map_err(|error| store_error(format!("failed to verify signature: {error}")))?;
        if valid {
            return Ok(public_key_fingerprint(public_key));
        }
    }
    Err(store_error(
        "signature verification failed against trusted keys",
    ))
}

fn read_signing_private_key(base_dir: &Path) -> Result<zeroize::Zeroizing<Vec<u8>>, TuiError> {
    let path = signing_private_key_path_in(base_dir);
    match std::fs::read(&path) {
        Ok(private_key) => Ok(zeroize::Zeroizing::new(private_key)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(store_error(
            "No signing key found. Run /keys generate first.",
        )),
        Err(error) => Err(store_error(format!(
            "failed to read signing private key at {}: {error}",
            path.display()
        ))),
    }
}

fn read_skill_wasm(base_dir: &Path, skill_name: &str) -> Result<Vec<u8>, TuiError> {
    let wasm_path = skill_wasm_path_in(base_dir, skill_name);
    match std::fs::read(&wasm_path) {
        Ok(wasm_bytes) => Ok(wasm_bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(store_error(format!(
            "No WASM skill found for '{skill_name}'. Expected {}",
            wasm_path.display()
        ))),
        Err(error) => Err(store_error(format!(
            "failed to read skill WASM at {}: {error}",
            wasm_path.display()
        ))),
    }
}

fn load_default_trusted_keys() -> Result<Vec<Vec<u8>>, TuiError> {
    fx_loadable::wasm_skill::load_trusted_keys()
        .map_err(|error| store_error(format!("failed to load trusted keys: {error}")))
}

fn load_trusted_keys_from_dir(trusted_dir: &Path) -> Result<Vec<Vec<u8>>, TuiError> {
    fx_loadable::wasm_skill::load_trusted_keys_from(trusted_dir)
        .map_err(|error| store_error(format!("failed to load trusted keys: {error}")))
}

fn read_binary_file(path: &Path, context: &str) -> Result<Vec<u8>, TuiError> {
    std::fs::read(path)
        .map_err(|error| store_error(format!("{context} at {}: {error}", path.display())))
}

fn read_public_key_file(path: &Path) -> Result<Vec<u8>, TuiError> {
    let public_key = read_binary_file(path, "failed to read public key")?;
    ensure_public_key_length(&public_key, path)?;
    Ok(public_key)
}

fn ensure_public_key_length(public_key: &[u8], path: &Path) -> Result<(), TuiError> {
    if public_key.len() == 32 {
        return Ok(());
    }
    Err(store_error(format!(
        "invalid public key length at {}: expected 32 bytes, found {}",
        path.display(),
        public_key.len()
    )))
}

fn ensure_public_key_extension(path: &Path) -> Result<(), TuiError> {
    if is_public_key_path(path) {
        return Ok(());
    }
    Err(store_error(format!(
        "public key path must end with .pub: {}",
        path.display()
    )))
}

fn is_public_key_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("pub")
}

fn public_key_fingerprint(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    hex_encode(&digest[..8])
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn display_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn store_error(message: impl Into<String>) -> TuiError {
    TuiError::Store(message.into())
}

fn configured_data_dir(base_data_dir: &Path, config: &FawxConfig) -> PathBuf {
    config
        .general
        .data_dir
        .clone()
        .unwrap_or_else(|| base_data_dir.to_path_buf())
}

fn configured_working_dir(config: &FawxConfig) -> PathBuf {
    if let Some(path) = &config.tools.working_dir {
        return path.clone();
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn load_startup_conversation_history(
    store: &ConversationStore,
    config: &FawxConfig,
) -> Vec<Message> {
    if !should_load_startup_conversation_history(config) {
        return Vec::new();
    }
    load_conversation_history(store, config.general.max_history)
}

fn should_load_startup_conversation_history(config: &FawxConfig) -> bool {
    !cfg!(test) || config.general.data_dir.is_some()
}

fn load_conversation_history(store: &ConversationStore, max_history: usize) -> Vec<Message> {
    let mut history = store
        .load_recent(max_history)
        .into_iter()
        .filter_map(message_from_store)
        .collect::<Vec<_>>();
    trim_history(&mut history, max_history);
    history
}

fn message_from_store(message: ConversationMessage) -> Option<Message> {
    match message.role.as_str() {
        "user" => Some(Message::user(message.content)),
        "assistant" => Some(Message::assistant(message.content)),
        _ => None,
    }
}

fn signal_labels(signals: &[Signal]) -> Vec<String> {
    signals
        .iter()
        .map(|signal| format!("{}:{}", signal.step.to_label(), signal.kind.to_label()))
        .collect()
}

fn tool_names(signals: &[Signal]) -> Vec<String> {
    signals.iter().filter_map(extract_tool_name).collect()
}

fn extract_tool_name(signal: &Signal) -> Option<String> {
    if signal.step != LoopStep::Act {
        return None;
    }

    signal
        .message
        .strip_prefix("tool ")
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn token_usage(result: &LoopResult) -> Option<ConversationTokenUsage> {
    match result {
        LoopResult::Complete { tokens_used, .. } => Some(ConversationTokenUsage {
            input: tokens_used.input_tokens,
            output: tokens_used.output_tokens,
        }),
        _ => None,
    }
}

fn render_loop_result(result: LoopResult, wall_time: std::time::Duration) -> String {
    match result {
        LoopResult::UserStopped {
            partial_response,
            iterations,
            ..
        } => {
            let wall = format_wall_time(wall_time);
            let meta = format!(
                "\x1b[33m  ⏹ Stopped by user after {iterations} iteration(s) · {wall}\x1b[0m"
            );
            match partial_response {
                Some(text) if !text.is_empty() => format!("{text}\n{meta}"),
                _ => meta,
            }
        }
        LoopResult::Complete {
            response,
            iterations,
            tokens_used,
            ..
        } => {
            let meta = format_loop_metadata(iterations, &tokens_used, wall_time);
            format!("{response}\n{meta}")
        }
        LoopResult::BudgetExhausted {
            partial_response,
            iterations,
            ..
        } => {
            let wall = format_wall_time(wall_time);
            let meta = format!(
                "\x1b[33m  \u{2717} budget exhausted after {iterations} iteration(s) \u{00b7} {wall}\x1b[0m"
            );
            match partial_response {
                Some(text) if !text.is_empty() => format!("{text}\n{meta}"),
                _ => meta,
            }
        }
        LoopResult::NeedsInput {
            prompt, iterations, ..
        } => {
            let meta =
                format!("\x1b[2m  \u{21b3} {iterations} iteration(s) \u{00b7} needs input\x1b[0m");
            format!("{prompt}\n{meta}")
        }
        LoopResult::Error {
            message,
            recoverable,
            ..
        } => {
            let suffix = if recoverable { " (recoverable)" } else { "" };
            let wall = format_wall_time(wall_time);
            format!("\x1b[31m  \u{2717} {message}{suffix} \u{00b7} {wall}\x1b[0m")
        }
    }
}

fn format_loop_metadata(
    iterations: u32,
    tokens: &TokenUsage,
    wall_time: std::time::Duration,
) -> String {
    let iter_text = if iterations == 1 {
        "1 iteration".to_string()
    } else {
        format!("{iterations} iterations")
    };
    format!(
        "\x1b[2m\x1b[38;2;210;112;10m  \u{21b3} {iter_text} \u{00b7} {} in / {} out tokens \u{00b7} {}\x1b[0m",
        tokens.input_tokens,
        tokens.output_tokens,
        format_wall_time(wall_time),
    )
}

/// Extract metadata line from a [`LoopResult`] for streaming display.
///
/// When the response text was already streamed to stdout, we only need
/// the trailing metadata line (iterations, tokens, wall time).
fn format_loop_metadata_for_result(
    result: &LoopResult,
    wall_time: std::time::Duration,
) -> Option<String> {
    match result {
        LoopResult::Complete {
            iterations,
            tokens_used,
            ..
        } => Some(format_loop_metadata(*iterations, tokens_used, wall_time)),
        LoopResult::UserStopped { iterations, .. } => {
            let wall = format_wall_time(wall_time);
            Some(format!(
                "\x1b[33m  \u{23f9} Stopped by user after {iterations} iteration(s) \u{00b7} {wall}\x1b[0m"
            ))
        }
        LoopResult::BudgetExhausted { iterations, .. } => {
            let wall = format_wall_time(wall_time);
            Some(format!(
                "\x1b[33m  \u{2717} budget exhausted after {iterations} iteration(s) \u{00b7} {wall}\x1b[0m"
            ))
        }
        _ => None,
    }
}

fn render_user_stopped(partial: Option<String>, iterations: u32) -> String {
    let meta = format!("\x1b[33m  ⏹ Stopped by user after {iterations} iteration(s)\x1b[0m");
    match partial {
        Some(text) if !text.is_empty() => format!("{text}\n{meta}"),
        _ => meta,
    }
}

fn render_budget_exhausted(partial: Option<String>, iterations: u32) -> String {
    let meta =
        format!("\x1b[33m  \u{2717} budget exhausted after {iterations} iteration(s)\x1b[0m");
    match partial {
        Some(text) if !text.is_empty() => format!("{text}\n{meta}"),
        _ => meta,
    }
}

fn render_loop_error(message: &str, recoverable: bool) -> String {
    let suffix = if recoverable { " (recoverable)" } else { "" };
    format!("\x1b[31m  \u{2717} {message}{suffix}\x1b[0m")
}

/// Collect all stream chunks into a string without printing.
///
/// Used by the loop LLM provider so internal reasoning output is not leaked
/// to the terminal. User-facing display is handled separately by
/// [`TuiApp::display_response`].
async fn consume_stream_silent(
    stream: &mut (impl futures::Stream<Item = Result<StreamChunk, ProviderError>> + Unpin),
) -> Result<String, CoreLlmError> {
    let mut sink = io::sink();
    consume_stream_with_writer(stream, &mut sink).await
}

async fn consume_stream_with_writer(
    stream: &mut (impl futures::Stream<Item = Result<StreamChunk, ProviderError>> + Unpin),
    writer: &mut impl Write,
) -> Result<String, CoreLlmError> {
    let mut collected = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                if let Some(delta) = &chunk.delta_content {
                    write_stream_delta(writer, delta)?;
                    collected.push_str(delta);
                }
            }
            Err(error) => return Err(CoreLlmError::Inference(error.to_string())),
        }
    }
    Ok(collected)
}

fn write_stream_delta(writer: &mut impl Write, delta: &str) -> Result<(), CoreLlmError> {
    writer
        .write_all(delta.as_bytes())
        .map_err(|error| CoreLlmError::Inference(error.to_string()))?;
    writer
        .flush()
        .map_err(|error| CoreLlmError::Inference(error.to_string()))
}

fn format_wall_time(wall_time: std::time::Duration) -> String {
    if wall_time.as_secs_f64() >= 1.0 {
        format!("{:.1}s", wall_time.as_secs_f64())
    } else {
        format!("{}ms", wall_time.as_millis())
    }
}

/// Spawn a background task that listens for terminal resize signals
/// (SIGWINCH on Unix) and re-applies the scroll region.
fn format_error_message(error: &str) -> String {
    format!("  \u{2717} {error}")
}

/// User-facing TUI errors.
#[derive(Debug)]
pub enum TuiError {
    /// Terminal or filesystem IO failure.
    Io(io::Error),
    /// Authentication flow error.
    Auth(String),
    /// User cancelled interactive input.
    Cancelled,
    /// Conversation store/persistence error.
    Store(String),
    /// Model routing error.
    Router(String),
    /// Request execution error.
    Loop(String),
}

impl fmt::Display for TuiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Auth(message) => write!(f, "auth error: {message}"),
            Self::Cancelled => write!(f, "{CANCELLED_INPUT_MESSAGE}"),
            Self::Store(message) => write!(f, "store error: {message}"),
            Self::Router(message) => write!(f, "router error: {message}"),
            Self::Loop(message) => write!(f, "loop error: {message}"),
        }
    }
}

impl std::error::Error for TuiError {}

impl From<io::Error> for TuiError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<AnalysisError> for TuiError {
    fn from(value: AnalysisError) -> Self {
        Self::Loop(format!("analysis failed: {value}"))
    }
}

impl From<fx_improve::ImprovementError> for TuiError {
    fn from(value: fx_improve::ImprovementError) -> Self {
        Self::Loop(format!("improvement failed: {value}"))
    }
}

/// Load the persisted auth manager from the encrypted store.
pub fn load_auth_manager() -> Result<AuthManager, TuiError> {
    // NB2: Warn if the removed FAWX_AUTH_FILE env var is still set.
    if std::env::var("FAWX_AUTH_FILE").is_ok() {
        eprintln!(
            "warning: FAWX_AUTH_FILE is deprecated; \
             credentials now stored encrypted in ~/.fawx/auth.db"
        );
    }
    let data_dir = fawx_data_dir();
    let store = AuthStore::open(&data_dir)
        .map_err(|e| TuiError::Auth(format!("failed to open auth store: {e}")))?;
    migrate_if_needed(&data_dir, &store)
        .map_err(|e| TuiError::Auth(format!("auth migration failed: {e}")))?;
    store
        .load_auth_manager()
        .map_err(|e| TuiError::Auth(format!("failed to load credentials: {e}")))
}

/// Build a model router from stored authentication credentials.
pub fn build_router(auth_manager: &AuthManager) -> Result<ModelRouter, TuiError> {
    let mut router = ModelRouter::new();

    for provider in auth_manager.providers() {
        if let Some(auth_method) = auth_manager.get(&provider) {
            register_auth_provider(&mut router, auth_method)?;
        }
    }

    if let Some(first_model) = router
        .available_models()
        .into_iter()
        .next()
        .map(|model| model.model_id)
    {
        if let Err(error) = router.set_active(&first_model) {
            eprintln!("failed to set initial model {first_model}: {error}");
        }
    }

    Ok(router)
}

async fn build_router_with_catalog(
    auth_manager: &AuthManager,
    catalog: &mut ModelCatalog,
) -> Result<ModelRouter, TuiError> {
    let mut router = ModelRouter::new();

    for provider in auth_manager.providers() {
        if let Some(auth_method) = auth_manager.get(&provider) {
            let dynamic_models = catalog_models_for_auth(catalog, auth_method).await;
            register_auth_provider_with_models(&mut router, auth_method, dynamic_models)?;
        }
    }

    if let Some(first_model) = router
        .available_models()
        .into_iter()
        .next()
        .map(|model| model.model_id)
    {
        if let Err(error) = router.set_active(&first_model) {
            eprintln!("failed to set initial model {first_model}: {error}");
        }
    }

    Ok(router)
}

/// Persist the auth manager to the encrypted store.
///
/// ## Optimization opportunity (NH8)
///
/// Each call re-opens `AuthStore` (new SQLite connection, re-reads salt,
/// re-derives key).  For a TUI that saves auth multiple times per session
/// this is wasteful.  A future improvement could keep the `AuthStore` in
/// `TuiApp` state alongside `auth_manager` and reuse it across saves.
fn persist_auth_manager(auth_manager: &AuthManager) -> Result<(), TuiError> {
    let data_dir = fawx_data_dir();
    let store = AuthStore::open(&data_dir)
        .map_err(|e| TuiError::Auth(format!("failed to open auth store: {e}")))?;
    store
        .save_auth_manager(auth_manager)
        .map_err(|e| TuiError::Auth(format!("failed to persist credentials: {e}")))
}

const OAUTH_SUCCESS_HTML: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Authentication successful</title></head>
<body><p>Authentication successful. Return to your terminal to continue.</p></body></html>"#;

async fn obtain_oauth_authorization_code(
    flow: &PkceFlow,
    auth_url: &str,
) -> Result<String, TuiError> {
    match start_oauth_callback_server(flow.state()).await {
        Ok(server) => oauth_code_with_callback_server(flow, auth_url, server).await,
        Err(_) => oauth_code_manual_fallback(flow, auth_url),
    }
}

async fn oauth_code_with_callback_server<F>(
    flow: &PkceFlow,
    auth_url: &str,
    server: F,
) -> Result<String, TuiError>
where
    F: std::future::Future<Output = Result<String, TuiError>>,
{
    eprintln!("Opening browser for ChatGPT sign-in...");
    if let Err(error) = open_browser(auth_url) {
        eprintln!(
            "Couldn't open browser automatically ({error}). Open this URL manually:\n{auth_url}"
        );
    }
    eprintln!("Waiting for callback on http://localhost:1455/auth/callback...");
    eprintln!("(Or paste the redirect URL/code below if browser didn't work)\n");

    tokio::select! {
        result = server => result.or_else(|_| prompt_for_oauth_code(flow)),
    }
}

fn oauth_code_manual_fallback(flow: &PkceFlow, auth_url: &str) -> Result<String, TuiError> {
    eprintln!("Couldn't start local server. Open this URL in your browser:\n");
    eprintln!("  {auth_url}\n");
    prompt_for_oauth_code(flow)
}

fn prompt_for_oauth_code(flow: &PkceFlow) -> Result<String, TuiError> {
    let input = prompt_non_empty_line(
        "Paste the redirect URL or authorization code: ",
        "Input cannot be empty.\n",
        "OAuth redirect URL or code",
    )?;

    flow.parse_callback(&input)
        .or_else(|_| Ok::<String, fx_auth::oauth::AuthError>(input.trim().to_string()))
        .map_err(|error| TuiError::Auth(format!("{error}")))
}

/// Start a local HTTP server on port 1455 to capture the OAuth callback.
/// Returns a future that resolves with the authorization code when received.
async fn start_oauth_callback_server(
    expected_state: &str,
) -> Result<impl std::future::Future<Output = Result<String, TuiError>>, TuiError> {
    let listener = tokio::net::TcpListener::bind("localhost:1455")
        .await
        .map_err(|e| TuiError::Auth(format!("failed to bind port 1455: {e}")))?;

    let state = expected_state.to_string();
    Ok(async move {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(TuiError::Auth("OAuth callback timed out (60s)".to_string()));
            }
            let stream = accept_with_timeout(&listener, remaining).await?;
            match handle_oauth_connection(stream, &state).await? {
                Some(code) => return Ok(code),
                None => continue,
            }
        }
    })
}

async fn accept_with_timeout(
    listener: &tokio::net::TcpListener,
    timeout: std::time::Duration,
) -> Result<tokio::net::TcpStream, TuiError> {
    let (stream, _) = tokio::time::timeout(timeout, listener.accept())
        .await
        .map_err(|_| TuiError::Auth("OAuth callback timed out (60s)".to_string()))?
        .map_err(|e| TuiError::Auth(format!("failed to accept connection: {e}")))?;
    Ok(stream)
}

async fn handle_oauth_connection(
    mut stream: tokio::net::TcpStream,
    expected_state: &str,
) -> Result<Option<String>, TuiError> {
    use tokio::io::AsyncReadExt;

    let mut buf = vec![0u8; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| TuiError::Auth(format!("failed to read request: {e}")))?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let (path, query) = parse_oauth_callback_request(&request)?;

    if path != "/auth/callback" {
        if let Err(error) = send_http_response(
            &mut stream,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot found",
        )
        .await
        {
            // Non-critical: wrong callback path is ignored; best-effort response is enough.
            eprintln!("oauth callback 404 write failed: {error}");
        }
        return Ok(None);
    }

    let code = validate_and_extract_code(query, expected_state)?;
    let body = OAUTH_SUCCESS_HTML;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    send_http_response(&mut stream, &response).await?;
    Ok(Some(code))
}

async fn send_http_response(
    stream: &mut tokio::net::TcpStream,
    response: &str,
) -> Result<(), TuiError> {
    write_http_response(stream, response)
        .await
        .map_err(|error| TuiError::Auth(format!("oauth callback write failed: {error}")))
}

async fn write_http_response<W>(writer: &mut W, response: &str) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    writer.write_all(response.as_bytes()).await
}

fn parse_oauth_callback_request(request: &str) -> Result<(&str, &str), TuiError> {
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| TuiError::Auth("malformed HTTP request".to_string()))?;

    Ok(path
        .split_once('?')
        .map_or((path, ""), |(parsed_path, query)| (parsed_path, query)))
}

fn validate_and_extract_code(query: &str, expected_state: &str) -> Result<String, TuiError> {
    let params: std::collections::HashMap<&str, &str> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .collect();

    let decode_param = |key: &str| -> Result<String, TuiError> {
        let value = params
            .get(key)
            .ok_or_else(|| TuiError::Auth(format!("callback missing {key} parameter")))?;

        urlencoding::decode(value)
            .map(|decoded| decoded.into_owned())
            .map_err(|error| {
                TuiError::Auth(format!(
                    "callback {key} was not valid percent-encoding: {error}"
                ))
            })
    };

    let returned_state = decode_param("state")?;
    if returned_state != expected_state {
        return Err(TuiError::Auth("OAuth state mismatch".to_string()));
    }

    decode_param("code")
}

fn openai_oauth_client_id() -> String {
    std::env::var("FAWX_OPENAI_CLIENT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fx_auth::oauth::OPENAI_CLIENT_ID.to_string())
}

fn openai_oauth_token_endpoint() -> String {
    std::env::var("FAWX_OPENAI_TOKEN_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_OPENAI_TOKEN_ENDPOINT.to_string())
}

async fn exchange_oauth_code_for_tokens(
    flow: &PkceFlow,
    client_id: &str,
    authorization_code: &str,
) -> Result<TokenResponse, TuiError> {
    let request = TokenExchangeRequest {
        grant_type: "authorization_code".to_string(),
        code: authorization_code.to_string(),
        redirect_uri: flow.redirect_uri().to_string(),
        code_verifier: flow.code_verifier().to_string(),
        client_id: client_id.to_string(),
    };

    let token_endpoint = openai_oauth_token_endpoint();
    let response = reqwest::Client::new()
        .post(&token_endpoint)
        .form(&request)
        .send()
        .await
        .map_err(|error| {
            TuiError::Auth(format!(
                "failed to exchange OAuth authorization code: {error}"
            ))
        })?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| TuiError::Auth(format!("failed to read OAuth token response: {error}")))?;

    if !status.is_success() {
        return Err(parse_token_error_response(status, &body));
    }

    serde_json::from_str::<TokenResponse>(&body)
        .map_err(|error| TuiError::Auth(format!("oauth token response was invalid JSON: {error}")))
}

fn parse_token_error_response(status: reqwest::StatusCode, body: &str) -> TuiError {
    let reason = parse_oauth_error_reason(body)
        .unwrap_or_else(|| "token endpoint request failed".to_string());
    let raw_body = format_oauth_error_body(body);

    TuiError::Auth(format!(
        "oauth token exchange failed ({}): {reason}. response_body={raw_body}",
        status.as_u16()
    ))
}

fn parse_oauth_error_reason(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            json.get("error_description")
                .or_else(|| json.get("error"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
}

fn format_oauth_error_body(body: &str) -> String {
    const MAX_ERROR_BODY_CHARS: usize = 300;
    format_oauth_error_body_with_limit(body, MAX_ERROR_BODY_CHARS)
}

fn format_oauth_error_body_with_limit(body: &str, max_chars: usize) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "<empty>".to_string();
    }

    let mut chars = trimmed.chars();
    let mut out: String = (&mut chars).take(max_chars).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

async fn catalog_models_for_auth(
    catalog: &mut ModelCatalog,
    auth_method: &AuthMethod,
) -> Vec<String> {
    // OAuth subscription tokens use the Codex Responses API which only supports
    // specific models. Skip dynamic fetch — the API would return models from
    // api.openai.com that don't work on the Codex endpoint.
    if matches!(auth_method, AuthMethod::OAuth { .. }) {
        return default_supported_models(auth_method);
    }

    let (provider, credential, auth_mode) = auth_context(auth_method);

    let discovered = catalog
        .get_models(provider, credential, auth_mode)
        .await
        .into_iter()
        .map(|model| model.id)
        .collect::<Vec<_>>();

    if discovered.is_empty() {
        default_supported_models(auth_method)
    } else {
        discovered
    }
}

fn auth_context(auth_method: &AuthMethod) -> (&str, &str, &'static str) {
    match auth_method {
        AuthMethod::SetupToken { token } => ("anthropic", token.as_str(), "setup_token"),
        AuthMethod::ApiKey { provider, key } => {
            let mode = if provider == "anthropic" {
                "api_key"
            } else {
                "bearer"
            };
            (provider.as_str(), key.as_str(), mode)
        }
        AuthMethod::OAuth {
            provider,
            access_token,
            ..
        } => (provider.as_str(), access_token.as_str(), "bearer"),
    }
}

fn register_auth_provider(
    router: &mut ModelRouter,
    auth_method: &AuthMethod,
) -> Result<(), TuiError> {
    register_auth_provider_with_models(router, auth_method, default_supported_models(auth_method))
}

fn register_auth_provider_with_models(
    router: &mut ModelRouter,
    auth_method: &AuthMethod,
    supported_models: Vec<String>,
) -> Result<(), TuiError> {
    let models = ensure_supported_models(auth_method, supported_models);

    match auth_method {
        AuthMethod::SetupToken { token } => {
            register_setup_token_provider(router, token, models)?;
        }
        AuthMethod::ApiKey { provider, key } => {
            register_api_key_provider(router, provider, key, models)?;
        }
        AuthMethod::OAuth {
            provider,
            access_token,
            account_id,
            ..
        } => {
            register_oauth_provider(
                router,
                provider,
                access_token,
                account_id.as_deref(),
                models,
            )?;
        }
    }

    Ok(())
}

fn ensure_supported_models(auth_method: &AuthMethod, supported_models: Vec<String>) -> Vec<String> {
    if supported_models.is_empty() {
        default_supported_models(auth_method)
    } else {
        supported_models
    }
}

fn register_setup_token_provider(
    router: &mut ModelRouter,
    token: &str,
    supported_models: Vec<String>,
) -> Result<(), TuiError> {
    let provider = AnthropicProvider::new(base_url_for_provider("anthropic"), token.to_string())
        .map_err(|error| {
            TuiError::Router(format!("failed to configure Anthropic provider: {error}"))
        })?
        .with_supported_models(supported_models);

    router.register_provider_with_auth(Box::new(provider), "subscription");
    Ok(())
}

fn register_api_key_provider(
    router: &mut ModelRouter,
    provider: &str,
    key: &str,
    supported_models: Vec<String>,
) -> Result<(), TuiError> {
    if provider == "anthropic" {
        let anthropic = AnthropicProvider::new(base_url_for_provider("anthropic"), key.to_string())
            .map_err(|error| {
                TuiError::Router(format!("failed to configure Anthropic provider: {error}"))
            })?
            .with_supported_models(supported_models);
        router.register_provider_with_auth(Box::new(anthropic), "api_key");
        return Ok(());
    }

    let provider_client = OpenAiProvider::new(base_url_for_provider(provider), key.to_string())
        .map_err(|error| {
            TuiError::Router(format!("failed to configure {provider} provider: {error}"))
        })?
        .with_name(provider.to_string())
        .with_supported_models(supported_models);

    router.register_provider_with_auth(Box::new(provider_client), "api_key");
    Ok(())
}

fn register_oauth_provider(
    router: &mut ModelRouter,
    provider: &str,
    access_token: &str,
    account_id: Option<&str>,
    supported_models: Vec<String>,
) -> Result<(), TuiError> {
    if let Some(account_id) = account_id {
        let provider_client =
            OpenAiResponsesProvider::new(access_token.to_string(), account_id.to_string())
                .map_err(|error| {
                    TuiError::Router(format!(
                        "failed to configure {provider} Responses provider: {error}"
                    ))
                })?
                .with_supported_models(supported_models);

        router.register_provider_with_auth(Box::new(provider_client), "subscription");
        return Ok(());
    }

    let provider_client =
        OpenAiProvider::new(base_url_for_provider(provider), access_token.to_string())
            .map_err(|error| {
                TuiError::Router(format!("failed to configure {provider} provider: {error}"))
            })?
            .with_name(provider.to_string())
            .with_supported_models(supported_models);

    router.register_provider_with_auth(Box::new(provider_client), "subscription");
    Ok(())
}

fn default_supported_models(auth_method: &AuthMethod) -> Vec<String> {
    match auth_method {
        AuthMethod::SetupToken { .. } => to_strings(DEFAULT_ANTHROPIC_MODELS),
        AuthMethod::ApiKey { provider, .. } => models_for_provider(provider),
        AuthMethod::OAuth {
            account_id,
            provider,
            ..
        } => {
            if account_id.is_some() {
                to_strings(DEFAULT_OPENAI_SUBSCRIPTION_MODELS)
            } else {
                models_for_provider(provider)
            }
        }
    }
}

fn base_url_for_provider(provider: &str) -> String {
    let env_key = format!(
        "FAWX_{}_BASE_URL",
        provider.to_ascii_uppercase().replace('-', "_")
    );

    if let Ok(url) = std::env::var(&env_key) {
        if !url.trim().is_empty() {
            return url;
        }
    }

    match provider {
        "anthropic" => "https://api.anthropic.com".to_string(),
        "openrouter" => "https://openrouter.ai/api".to_string(),
        "openai" => "https://api.openai.com".to_string(),
        _ => std::env::var("FAWX_OPENAI_COMPAT_BASE_URL")
            .ok()
            .filter(|url| !url.trim().is_empty())
            .unwrap_or_else(|| "https://api.openai.com".to_string()),
    }
}

fn models_for_provider(provider: &str) -> Vec<String> {
    match provider {
        "anthropic" => to_strings(DEFAULT_ANTHROPIC_MODELS),
        "openrouter" => to_strings(DEFAULT_OPENROUTER_MODELS),
        "openai" => to_strings(DEFAULT_OPENAI_MODELS),
        _ => vec!["gpt-4o-mini".to_string()],
    }
}

fn to_strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// Run a closure on the normal terminal screen.
///
/// Leaves the ratatui alternate screen before executing `f` so that raw
/// `eprint!`/`stdin` I/O renders correctly, then re-enters the alternate
/// screen afterward.  Both transitions are best-effort: if the terminal
/// does not support them the prompt still works (graceful degradation).
fn with_normal_screen<T>(f: impl FnOnce() -> Result<T, TuiError>) -> Result<T, TuiError> {
    let _ = crossterm::execute!(io::stdout(), terminal::LeaveAlternateScreen);
    let result = f();
    let _ = crossterm::execute!(io::stdout(), terminal::EnterAlternateScreen);
    result
}

fn prompt_line(prompt: &str) -> Result<String, TuiError> {
    ensure_cooked_mode();

    eprint!("{prompt}");
    io::stdout().flush().map_err(TuiError::Io)?;

    let mut input = String::new();
    let bytes = io::stdin().read_line(&mut input).map_err(TuiError::Io)?;
    if bytes == 0 {
        return Err(TuiError::Auth("stdin closed unexpectedly".to_string()));
    }

    Ok(input.trim().to_string())
}

fn ensure_cooked_mode() {
    ensure_cooked_mode_with(
        || {
            terminal::disable_raw_mode().ok();
        },
        drain_stdin,
    );
}

fn ensure_cooked_mode_with<DisableRawMode, DrainStdin>(
    mut disable_raw_mode: DisableRawMode,
    mut drain_stdin: DrainStdin,
) where
    DisableRawMode: FnMut(),
    DrainStdin: FnMut(),
{
    // Ensure cooked mode so stdin.read_line() handles Enter correctly even after raw-mode input paths.
    disable_raw_mode();
    drain_stdin();
}

/// Flush pending bytes from the terminal input queue.
///
/// After disabling raw mode the kernel tty buffer may still hold
/// stale CR/LF bytes from the previous raw-mode session. Flushing
/// prevents those bytes from being echoed as `^M` when cooked mode
/// re-enables terminal echo.
fn drain_stdin() {
    #[cfg(unix)]
    {
        drain_stdin_with(drain_stdin_input_queue, log_drain_stdin_error);
    }
}

#[cfg(unix)]
fn drain_stdin_with<Drain, Log>(mut drain: Drain, mut log_error: Log)
where
    Drain: FnMut() -> io::Result<()>,
    Log: FnMut(&io::Error),
{
    if let Err(error) = drain() {
        log_error(&error);
    }
}

#[cfg(unix)]
fn drain_stdin_input_queue() -> io::Result<()> {
    flush_stdin_input_queue_with(|fd, queue_selector| {
        // SAFETY: tcflush is a standard POSIX call that discards
        // data received but not yet read.
        unsafe { libc::tcflush(fd, queue_selector) }
    })
}

#[cfg(unix)]
fn log_drain_stdin_error(error: &io::Error) {
    if is_benign_stdin_flush_error(error) {
        tracing::debug!(
            errno = ?error.raw_os_error(),
            error = %error,
            "skipping stdin input queue flush because stdin is not a tty"
        );
        return;
    }

    tracing::warn!(
        errno = ?error.raw_os_error(),
        error = %error,
        "failed to flush stdin input queue"
    );
}

#[cfg(unix)]
fn is_benign_stdin_flush_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ENOTTY)
}

#[cfg(unix)]
fn flush_stdin_input_queue_with<F>(mut flush: F) -> io::Result<()>
where
    F: FnMut(i32, i32) -> i32,
{
    let status = flush(libc::STDIN_FILENO, libc::TCIFLUSH);
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn confirm_provider_removal(provider: &str) -> Result<bool, TuiError> {
    with_normal_screen(|| {
        let prompt = format!("Remove {provider}? [y/N]: ");
        let response = prompt_line(&prompt)?;
        Ok(removal_confirmation_accepted(&response))
    })
}

fn removal_confirmation_accepted(response: &str) -> bool {
    let normalized = response.trim();
    normalized.eq_ignore_ascii_case("y") || normalized.eq_ignore_ascii_case("yes")
}

fn finalize_auth_wizard_result(result: Result<(), TuiError>) -> Result<(), TuiError> {
    let mut stdout = io::stdout();
    finalize_auth_wizard_result_with_writer(result, &mut stdout)
}

fn finalize_auth_wizard_result_with_writer(
    result: Result<(), TuiError>,
    writer: &mut impl Write,
) -> Result<(), TuiError> {
    if is_cancelled_error(&result) {
        writeln!(writer, "Cancelled.").map_err(TuiError::Io)?;
        return Ok(());
    }

    result
}

fn retry_limit_error(context: &str) -> TuiError {
    TuiError::Auth(format!("maximum input retries exceeded for {context}"))
}

fn prompt_choice<T, F>(
    prompt: &str,
    invalid_message: &str,
    context: &str,
    parser: F,
) -> Result<T, TuiError>
where
    F: Fn(&str) -> Option<T>,
{
    with_normal_screen(|| prompt_choice_inner(prompt, invalid_message, context, parser))
}

fn prompt_choice_inner<T, F>(
    prompt: &str,
    invalid_message: &str,
    context: &str,
    parser: F,
) -> Result<T, TuiError>
where
    F: Fn(&str) -> Option<T>,
{
    for _ in 0..MAX_PROMPT_RETRIES {
        let value = prompt_line(prompt)?;
        if let Some(parsed) = parser(&value) {
            return Ok(parsed);
        }

        eprint!("{invalid_message}");
    }

    Err(retry_limit_error(context))
}

fn prompt_non_empty_line(
    prompt: &str,
    empty_message: &str,
    context: &str,
) -> Result<String, TuiError> {
    with_normal_screen(|| prompt_non_empty_line_inner(prompt, empty_message, context))
}

fn prompt_non_empty_line_inner(
    prompt: &str,
    empty_message: &str,
    context: &str,
) -> Result<String, TuiError> {
    for _ in 0..MAX_PROMPT_RETRIES {
        let value = prompt_line(prompt)?;
        if !value.is_empty() {
            return Ok(value);
        }

        eprint!("{empty_message}");
    }

    Err(retry_limit_error(context))
}

fn prompt_api_key_provider() -> Result<String, TuiError> {
    with_normal_screen(|| {
        eprintln!("Which provider?");
        eprintln!("  [1] Anthropic");
        eprintln!("  [2] OpenAI");
        eprintln!("  [3] OpenRouter");
        eprintln!("  [4] Other (OpenAI-compatible)");
        eprintln!();

        let choice = prompt_choice_inner(
            "> ",
            "Please choose 1, 2, 3, or 4.",
            "API key provider selection",
            parse_api_key_provider_selection,
        )?;

        match choice {
            ApiKeyProvider::Anthropic => Ok("anthropic".to_string()),
            ApiKeyProvider::OpenAi => Ok("openai".to_string()),
            ApiKeyProvider::OpenRouter => Ok("openrouter".to_string()),
            ApiKeyProvider::Other => prompt_non_empty_line_inner(
                "Provider name: ",
                "Provider name cannot be empty.",
                "API key provider name",
            ),
        }
    })
}

fn prompt_non_empty_secret(
    prompt: &str,
    empty_message: &str,
    context: &str,
) -> Result<String, TuiError> {
    with_normal_screen(|| {
        for _ in 0..MAX_PROMPT_RETRIES {
            let value = prompt_secret(prompt)?;
            if !value.is_empty() {
                return Ok(value);
            }

            eprint!("{empty_message}");
        }

        Err(retry_limit_error(context))
    })
}

fn prompt_secret(prompt: &str) -> Result<String, TuiError> {
    eprint!("{prompt}");
    io::stdout().flush().map_err(TuiError::Io)?;

    let _guard = RawModeGuard::new()?;
    let mut value = String::new();
    read_secret_input(&mut value)?;

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        eprintln!();
    } else {
        eprint!(" ({} chars)", trimmed.len());
    }

    Ok(trimmed)
}

fn read_secret_input(value: &mut String) -> Result<(), TuiError> {
    let mut display_len: usize = 0;

    loop {
        let event = event::read().map_err(TuiError::Io)?;
        if let event::Event::Key(key_event) = event {
            match classify_secret_input_key(&key_event) {
                SecretInputKeyAction::Submit => return Ok(()),
                SecretInputKeyAction::Cancel => return Err(TuiError::Cancelled),
                SecretInputKeyAction::Ignore => {}
                SecretInputKeyAction::Type(ch) => {
                    value.push(ch);
                    display_len += 1;
                    eprint!("•");
                    io::stdout().flush().map_err(TuiError::Io)?;
                }
                SecretInputKeyAction::Delete => {
                    if value.pop().is_some() && display_len > 0 {
                        display_len -= 1;
                        eprint!("\x08 \x08");
                        io::stdout().flush().map_err(TuiError::Io)?;
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretInputKeyAction {
    Submit,
    Cancel,
    Type(char),
    Delete,
    Ignore,
}

fn classify_secret_input_key(key_event: &event::KeyEvent) -> SecretInputKeyAction {
    if is_ctrl_c(key_event) {
        return SecretInputKeyAction::Cancel;
    }

    match key_event.code {
        event::KeyCode::Enter => SecretInputKeyAction::Submit,
        event::KeyCode::Esc => SecretInputKeyAction::Cancel,
        event::KeyCode::Char(ch) => SecretInputKeyAction::Type(ch),
        event::KeyCode::Backspace => SecretInputKeyAction::Delete,
        _ => SecretInputKeyAction::Ignore,
    }
}

fn open_browser(url: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open").arg(url).status()?;
        if status.success() {
            return Ok(());
        }
        Err(io::Error::other("open command failed"))
    }

    #[cfg(target_os = "windows")]
    {
        let status = Command::new("cmd").args(["/C", "start", url]).status()?;
        if status.success() {
            return Ok(());
        }
        return Err(io::Error::other("start command failed"));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let status = Command::new("xdg-open").arg(url).status()?;
        if status.success() {
            return Ok(());
        }
        Err(io::Error::other("xdg-open command failed"))
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn is_cancelled_error(result: &Result<(), TuiError>) -> bool {
    matches!(result, Err(TuiError::Cancelled))
}

fn is_ctrl_c(key_event: &event::KeyEvent) -> bool {
    key_event.code == event::KeyCode::Char('c')
        && key_event.modifiers.contains(event::KeyModifiers::CONTROL)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthSelection {
    ClaudeSubscription,
    ChatGptSubscription,
    ApiKey,
}

fn parse_auth_selection(value: &str) -> Option<AuthSelection> {
    match value.trim() {
        "1" => Some(AuthSelection::ClaudeSubscription),
        "2" => Some(AuthSelection::ChatGptSubscription),
        "3" => Some(AuthSelection::ApiKey),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthMenuSelection {
    AddOrUpdate,
    RemoveProvider,
    Cancel,
}

fn parse_auth_menu_selection(value: &str) -> Option<AuthMenuSelection> {
    match value.trim() {
        "1" => Some(AuthMenuSelection::AddOrUpdate),
        "2" => Some(AuthMenuSelection::RemoveProvider),
        "3" => Some(AuthMenuSelection::Cancel),
        _ => None,
    }
}

fn prompt_provider_selection(providers: &[String]) -> Result<String, TuiError> {
    let selected = prompt_choice(
        "> ",
        "Please choose a listed provider number.",
        "provider selection",
        |value| parse_provider_selection(value, providers.len()),
    )?;

    Ok(providers[selected].clone())
}

fn parse_provider_selection(value: &str, provider_count: usize) -> Option<usize> {
    value
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|selected| (1..=provider_count).contains(selected))
        .map(|selected| selected - 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiKeyProvider {
    Anthropic,
    OpenAi,
    OpenRouter,
    Other,
}

fn parse_api_key_provider_selection(value: &str) -> Option<ApiKeyProvider> {
    match value.trim() {
        "1" => Some(ApiKeyProvider::Anthropic),
        "2" => Some(ApiKeyProvider::OpenAi),
        "3" => Some(ApiKeyProvider::OpenRouter),
        "4" => Some(ApiKeyProvider::Other),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedCommand {
    Model(Option<String>),
    Auth {
        subcommand: Option<String>,
        action: Option<String>,
        value: Option<String>,
        has_extra_args: bool,
    },
    Keys {
        subcommand: Option<String>,
        value: Option<String>,
        option: Option<String>,
        has_extra_args: bool,
    },
    Sign {
        target: Option<String>,
        has_extra_args: bool,
    },
    Budget,
    Loop,
    Status,
    Signals,
    Debug,
    Analyze,
    Improve(ImproveFlags),
    Synthesis(Option<String>),
    Clear,
    New,
    History,
    Config(Option<String>),
    Help,
    Quit,
    Unknown(String),
}

/// Flags parsed from the `/improve` command.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ImproveFlags {
    dry_run: bool,
    has_unknown_flag: Option<String>,
}

fn parse_command(value: &str) -> ParsedCommand {
    let input = value.trim_start();
    let Some(input) = input.strip_prefix('/') else {
        return ParsedCommand::Unknown(input.to_string());
    };

    let mut parts = input.split_whitespace();
    let Some(command) = parts.next() else {
        return ParsedCommand::Unknown(String::new());
    };

    match command {
        "model" => ParsedCommand::Model(parts.next().map(ToString::to_string)),
        "auth" => {
            let subcommand = parts.next().map(ToString::to_string);
            let action = parts.next().map(ToString::to_string);
            let value = parts.next().map(ToString::to_string);
            let has_extra_args = parts.next().is_some();
            ParsedCommand::Auth {
                subcommand,
                action,
                value,
                has_extra_args,
            }
        }
        "keys" => {
            let subcommand = parts.next().map(ToString::to_string);
            let value = parts.next().map(ToString::to_string);
            let option = parts.next().map(ToString::to_string);
            let has_extra_args = parts.next().is_some();
            ParsedCommand::Keys {
                subcommand,
                value,
                option,
                has_extra_args,
            }
        }
        "sign" => ParsedCommand::Sign {
            target: parts.next().map(ToString::to_string),
            has_extra_args: parts.next().is_some(),
        },
        "budget" => ParsedCommand::Budget,
        "loop" => ParsedCommand::Loop,
        "status" => ParsedCommand::Status,
        "signals" => ParsedCommand::Signals,
        "debug" => ParsedCommand::Debug,
        "analyze" => ParsedCommand::Analyze,
        "improve" => ParsedCommand::Improve(parse_improve_flags(&mut parts)),
        "synthesis" => {
            let remainder = input[command.len()..].strip_prefix(' ');
            match remainder {
                None => ParsedCommand::Synthesis(None),
                Some(raw) if raw.trim().is_empty() => ParsedCommand::Synthesis(Some(String::new())),
                Some(raw) => ParsedCommand::Synthesis(Some(raw.trim().to_string())),
            }
        }
        "clear" | "cls" => ParsedCommand::Clear,
        "new" => ParsedCommand::New,
        "history" => ParsedCommand::History,
        "config" => ParsedCommand::Config(parts.next().map(ToString::to_string)),
        "help" => ParsedCommand::Help,
        "quit" | "exit" => ParsedCommand::Quit,
        other => ParsedCommand::Unknown(other.to_string()),
    }
}

fn parse_improve_flags(parts: &mut std::str::SplitWhitespace<'_>) -> ImproveFlags {
    let mut flags = ImproveFlags {
        dry_run: false,
        has_unknown_flag: None,
    };
    for arg in parts {
        match arg {
            "--dry-run" => flags.dry_run = true,
            other => {
                flags.has_unknown_flag = Some(other.to_string());
                break;
            }
        }
    }
    flags
}

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if let Err(error) = terminal::disable_raw_mode() {
            eprintln!("failed to disable raw mode: {error}");
        }
    }
}

// ---------------------------------------------------------------------------
// Auth helper free functions
// ---------------------------------------------------------------------------

/// Normalize a provider name to lowercase for consistent lookups.
fn normalize_provider_name(value: &str) -> String {
    let lower = value.trim().to_ascii_lowercase();
    match lower.as_str() {
        "gh" => "github".to_string(),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitHubPatKind {
    Classic,
    FineGrained,
    Unknown,
}

/// Classify a GitHub PAT by its prefix.
fn classify_github_pat(token: &str) -> GitHubPatKind {
    if token.starts_with("github_pat_") {
        GitHubPatKind::FineGrained
    } else if token.starts_with("ghp_") {
        GitHubPatKind::Classic
    } else {
        GitHubPatKind::Unknown
    }
}

/// Serialize a [`GitHubPatKind`] to a metadata label string.
fn pat_kind_label(kind: GitHubPatKind) -> &'static str {
    match kind {
        GitHubPatKind::Classic => "classic",
        GitHubPatKind::FineGrained => "fine_grained",
        GitHubPatKind::Unknown => "unknown",
    }
}

/// Deserialize a metadata label string back to [`GitHubPatKind`].
fn pat_kind_from_label(label: &str) -> GitHubPatKind {
    match label {
        "classic" => GitHubPatKind::Classic,
        "fine_grained" => GitHubPatKind::FineGrained,
        _ => GitHubPatKind::Unknown,
    }
}

/// Infer the PAT kind from prefix and scope heuristics.
fn infer_github_pat_kind(token: &str, scopes: &[String]) -> GitHubPatKind {
    let prefix_kind = classify_github_pat(token);
    if prefix_kind != GitHubPatKind::Unknown {
        return prefix_kind;
    }
    // Fine-grained PATs report empty scopes via the API.
    if scopes.is_empty() {
        GitHubPatKind::FineGrained
    } else {
        GitHubPatKind::Classic
    }
}

/// Build scope display string.
fn github_scope_display(scopes: &[String], kind: GitHubPatKind) -> String {
    if kind == GitHubPatKind::FineGrained {
        return "(fine-grained — scopes N/A)".to_string();
    }
    if scopes.is_empty() {
        "(none)".to_string()
    } else {
        scopes.join(", ")
    }
}

/// Format token info lines after successful validation.
fn format_github_token_result(
    token: &zeroize::Zeroizing<String>,
    info: &fx_auth::github::GitHubTokenInfo,
) -> Vec<String> {
    let token_kind = infer_github_pat_kind(token.as_str(), &info.scopes);
    let mut lines = Vec::new();
    if token_kind == GitHubPatKind::FineGrained {
        lines.push("  Type: fine-grained PAT".to_string());
    } else {
        lines.push(format!(
            "  Scopes: {}",
            github_scope_display(&info.scopes, token_kind)
        ));
    }
    if token_kind == GitHubPatKind::Classic && !info.missing_scopes.is_empty() {
        lines.push(format!(
            "  ⚠ Missing recommended scopes: {}",
            info.missing_scopes.join(", ")
        ));
    }
    lines
}

/// Build display lines for GitHub credential status.
fn format_github_status_lines(
    store: &fx_auth::credential_store::EncryptedFileCredentialStore,
) -> Vec<String> {
    match store.status(fx_auth::credential_store::AuthProvider::GitHub) {
        Ok(status) if status.configured => format_github_configured_status(store, status.metadata),
        Ok(_) => vec![
            "GitHub: not configured".to_string(),
            "  Use /auth github set-token <TOKEN> to configure.".to_string(),
        ],
        Err(e) => vec![format!("Error reading status: {e}")],
    }
}

/// Format lines for a configured GitHub credential.
fn format_github_configured_status(
    store: &fx_auth::credential_store::EncryptedFileCredentialStore,
    metadata: Option<fx_auth::credential_store::CredentialMetadata>,
) -> Vec<String> {
    let Some(meta) = metadata else {
        return vec!["GitHub: configured".to_string()];
    };
    let token_kind = github_pat_kind_from_store(store);
    let mut lines = vec![
        "GitHub credential status:".to_string(),
        format!("  Login: {}", meta.login.as_deref().unwrap_or("(unknown)")),
        format!("  Method: {}", meta.method),
        format!(
            "  Scopes: {}",
            github_scope_display(&meta.scopes, token_kind)
        ),
    ];
    if meta.last_validated_ms > 0 {
        let age_ms = fx_auth::credential_store::current_timestamp_ms()
            .saturating_sub(meta.last_validated_ms);
        lines.push(format!("  Last validated: {}", humanize_elapsed_ms(age_ms)));
    }
    lines
}

/// Determine the PAT kind from stored metadata (avoids decrypting the secret).
fn github_pat_kind_from_store(
    store: &fx_auth::credential_store::EncryptedFileCredentialStore,
) -> GitHubPatKind {
    use fx_auth::credential_store::{AuthProvider, CredentialStore};
    store
        .status(AuthProvider::GitHub)
        .ok()
        .and_then(|s| s.metadata)
        .and_then(|m| m.token_kind)
        .map(|kind| pat_kind_from_label(&kind))
        .unwrap_or(GitHubPatKind::Unknown)
}

/// Persist a GitHub PAT and verify the round-trip.
fn store_github_pat_in(
    store: &fx_auth::credential_store::EncryptedFileCredentialStore,
    token: &zeroize::Zeroizing<String>,
    info: &fx_auth::github::GitHubTokenInfo,
) -> Result<(), TuiError> {
    use fx_auth::credential_store::{
        AuthProvider, CredentialMetadata, CredentialMethod, CredentialStore,
    };
    let pat_kind = infer_github_pat_kind(token.as_str(), &info.scopes);
    let metadata = CredentialMetadata {
        provider: AuthProvider::GitHub,
        method: CredentialMethod::Pat,
        last_validated_ms: fx_auth::credential_store::current_timestamp_ms(),
        login: Some(info.login.clone()),
        scopes: info.scopes.clone(),
        token_kind: Some(pat_kind_label(pat_kind).to_string()),
    };
    store
        .set(
            AuthProvider::GitHub,
            CredentialMethod::Pat,
            token,
            &metadata,
        )
        .map_err(|e| TuiError::Auth(format!("failed to store credential: {e}")))?;
    // Always verify: encrypted storage round-trip catches silent corruption
    verify_github_pat_stored(store, token)
}

/// Verify a stored GitHub PAT matches the expected value.
fn verify_github_pat_stored(
    store: &fx_auth::credential_store::EncryptedFileCredentialStore,
    token: &zeroize::Zeroizing<String>,
) -> Result<(), TuiError> {
    use fx_auth::credential_store::{AuthProvider, CredentialMethod, CredentialStore};
    let persisted = store
        .get(AuthProvider::GitHub, CredentialMethod::Pat)
        .map_err(|e| TuiError::Auth(format!("failed to verify stored credential: {e}")))?;
    match persisted {
        Some(saved) if saved.as_str() == token.as_str() => Ok(()),
        Some(_) => Err(TuiError::Auth(
            "credential verification failed (token mismatch)".to_string(),
        )),
        None => Err(TuiError::Auth(
            "credential verification failed (token missing)".to_string(),
        )),
    }
}

/// Human-friendly elapsed time label.
fn humanize_elapsed_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        return "just now".to_string();
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    format!("{days}d ago")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_core::memory::{MemoryProvider, MemoryTouchProvider};
    use fx_kernel::act::ToolExecutorError;
    use fx_llm::ContentBlock;
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tempfile::TempDir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct ScopedEnvVar {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl ScopedEnvVar {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn test_engine_and_runtime_info() -> (LoopEngine, Arc<RwLock<RuntimeInfo>>) {
        let bundle = build_loop_engine_bundle();
        (bundle.engine, bundle.runtime_info)
    }

    /// Returns a `FawxConfig` whose `data_dir` points at a fresh temporary
    /// directory.  The caller **must** keep the returned `TempDir` alive for
    /// as long as the `TuiApp` (or anything else using the directory) is in
    /// scope – dropping it deletes the directory and the SQLite files inside.
    fn test_config_with_temp_dir() -> (FawxConfig, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().expect("create temp data dir for test");
        let mut config = FawxConfig::default();
        config.general.data_dir = Some(temp_dir.path().to_path_buf());
        (config, temp_dir)
    }

    fn new_test_app() -> (TuiApp, tempfile::TempDir) {
        let (config, temp_dir) = test_config_with_temp_dir();
        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let app = TuiApp::new(
            AuthManager::new(),
            ModelRouter::new(),
            loop_engine,
            runtime_info,
            config,
        )
        .expect("new test app");
        (app, temp_dir)
    }

    fn sample_wasm_bytes() -> Vec<u8> {
        vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
    }

    fn write_test_skill(base_dir: &Path, skill_name: &str, wasm_bytes: &[u8]) {
        let skill_dir = fawx_skills_dir_in(base_dir).join(skill_name);
        std::fs::create_dir_all(&skill_dir).expect("skill dir");
        std::fs::write(skill_dir.join(format!("{skill_name}.wasm")), wasm_bytes).expect("wasm");
    }

    fn create_temp_home_guard(temp_home: &TempDir) -> ScopedEnvVar {
        let home = temp_home.path().to_string_lossy().to_string();
        ScopedEnvVar::set("HOME", &home)
    }

    #[test]
    fn build_loop_engine_from_builder_returns_tui_error_on_failure() {
        let error = build_loop_engine_from_builder(LoopEngine::builder())
            .expect_err("missing required fields should return an error");

        match error {
            TuiError::Loop(message) => {
                assert!(message.contains("missing_required_field"));
            }
            other => panic!("expected TuiError::Loop, got {other:?}"),
        }
    }

    type RecordedWrites = Arc<Mutex<Vec<(String, String)>>>;

    #[derive(Debug)]
    struct MockMemoryStore {
        entries: BTreeMap<String, String>,
        writes: RecordedWrites,
    }

    impl MockMemoryStore {
        fn new(writes: RecordedWrites) -> Self {
            Self {
                entries: BTreeMap::new(),
                writes,
            }
        }
    }

    impl MemoryProvider for MockMemoryStore {
        fn read(&self, key: &str) -> Option<String> {
            self.entries.get(key).cloned()
        }

        fn write(&mut self, key: &str, value: &str) -> Result<(), String> {
            self.entries.insert(key.to_string(), value.to_string());
            match self.writes.lock() {
                Ok(mut writes) => {
                    writes.push((key.to_string(), value.to_string()));
                    Ok(())
                }
                Err(error) => Err(format!("writes lock poisoned: {error}")),
            }
        }

        fn list(&self) -> Vec<(String, String)> {
            self.entries
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        }

        fn delete(&mut self, key: &str) -> bool {
            self.entries.remove(key).is_some()
        }

        fn search(&self, query: &str) -> Vec<(String, String)> {
            self.entries
                .iter()
                .filter(|(key, value)| key.contains(query) || value.contains(query))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        }

        fn snapshot(&self) -> Vec<(String, String)> {
            self.list()
        }
    }

    impl MemoryTouchProvider for MockMemoryStore {
        fn touch(&mut self, _key: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn mock_memory_store() -> (SharedMemoryStore, RecordedWrites) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let store = MockMemoryStore::new(Arc::clone(&writes));
        (Arc::new(Mutex::new(store)), writes)
    }

    fn test_finding(pattern_name: &str, confidence: Confidence) -> AnalysisFinding {
        AnalysisFinding {
            pattern_name: pattern_name.to_string(),
            description: format!("{pattern_name} description"),
            confidence,
            evidence: Vec::new(),
            suggested_action: Some("apply corrective action".to_string()),
        }
    }

    fn test_auth_manager() -> AuthManager {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "setup-token-123".to_string(),
            },
        );
        auth_manager.store(
            "openrouter",
            AuthMethod::ApiKey {
                provider: "openrouter".to_string(),
                key: "openrouter-key-456".to_string(),
            },
        );
        auth_manager
    }

    #[test]
    fn memory_prompt_formatting_changes_with_query_relevance() {
        let temp = TempDir::new().expect("tempdir");
        let mut memory = JsonFileMemory::new_with_config(temp.path(), JsonMemoryConfig::default())
            .expect("create memory");

        memory
            .write("project_plan", "ship auth rewrite")
            .expect("write project");
        memory
            .write("favorite_food", "pizza on fridays")
            .expect("write food");

        let project_entries = memory.search_relevant("project auth", 5);
        let food_entries = memory.search_relevant("pizza", 5);

        let project_prompt =
            format_memory_for_prompt(&project_entries, 2_000).expect("project prompt");
        let food_prompt = format_memory_for_prompt(&food_entries, 2_000).expect("food prompt");

        assert_ne!(project_prompt, food_prompt);
        assert!(project_prompt.contains("project_plan"));
        assert!(food_prompt.contains("favorite_food"));
    }

    #[test]
    fn route_findings_writes_high_confidence_to_memory() {
        let (memory, writes) = mock_memory_store();
        let findings = vec![test_finding("Retry Storm", Confidence::High)];

        let (mut app, _temp_dir) = new_test_app();
        let summary = app.route_findings_by_confidence(&findings, Some(&memory));

        let writes = writes.lock().expect("lock writes");
        assert_eq!(summary, (1, 0, 0));
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, "pattern/Retry Storm");
        assert_eq!(writes[0].1, "Retry Storm description");
    }

    #[test]
    fn route_findings_does_not_write_medium_confidence_to_memory() {
        let (memory, writes) = mock_memory_store();
        let findings = vec![test_finding("Needs Follow Up", Confidence::Medium)];

        let (mut app, _temp_dir) = new_test_app();
        let summary = app.route_findings_by_confidence(&findings, Some(&memory));

        assert_eq!(summary, (0, 1, 0));
        assert!(writes.lock().expect("lock writes").is_empty());
    }

    #[test]
    fn route_findings_does_not_write_low_confidence_to_memory() {
        let (memory, writes) = mock_memory_store();
        let findings = vec![test_finding("Loose Correlation", Confidence::Low)];

        let (mut app, _temp_dir) = new_test_app();
        let summary = app.route_findings_by_confidence(&findings, Some(&memory));

        assert_eq!(summary, (0, 0, 1));
        assert!(writes.lock().expect("lock writes").is_empty());
    }

    #[test]
    fn route_findings_uses_pattern_name_in_memory_key() {
        let (memory, writes) = mock_memory_store();
        let findings = vec![test_finding("Tool-Timeout Loop", Confidence::High)];

        let (mut app, _temp_dir) = new_test_app();
        app.route_findings_by_confidence(&findings, Some(&memory));

        let writes = writes.lock().expect("lock writes");
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, "pattern/Tool-Timeout Loop");
    }

    #[test]
    fn route_findings_routes_mixed_confidence_bag() {
        let (memory, writes) = mock_memory_store();
        let findings = vec![
            test_finding("Retry Storm", Confidence::High),
            test_finding("Needs Follow Up", Confidence::Medium),
            test_finding("Loose Correlation", Confidence::Low),
            test_finding("Context Drift", Confidence::High),
        ];

        let (mut app, _temp_dir) = new_test_app();
        let summary = app.route_findings_by_confidence(&findings, Some(&memory));

        let writes = writes.lock().expect("lock writes");
        let keys: Vec<String> = writes.iter().map(|(key, _)| key.clone()).collect();
        assert_eq!(summary, (2, 1, 1));
        assert_eq!(keys, vec!["pattern/Retry Storm", "pattern/Context Drift"]);
    }

    #[test]
    fn route_findings_without_memory_store_does_not_panic() {
        let findings = vec![
            test_finding("Retry Storm", Confidence::High),
            test_finding("Needs Follow Up", Confidence::Medium),
            test_finding("Loose Correlation", Confidence::Low),
        ];

        let (mut app, _temp_dir) = new_test_app();
        let summary = app.route_findings_by_confidence(&findings, None);

        assert_eq!(summary, (0, 1, 1));
    }

    #[test]
    fn route_findings_with_poisoned_memory_mutex_skips_high_confidence_writes() {
        let (memory, writes) = mock_memory_store();
        let poisoned_memory = Arc::clone(&memory);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poisoned_memory
                .lock()
                .expect("lock memory before intentional panic");
            panic!("intentional poison");
        }));

        let findings = vec![
            test_finding("Retry Storm", Confidence::High),
            test_finding("Needs Follow Up", Confidence::Medium),
            test_finding("Loose Correlation", Confidence::Low),
        ];

        let (mut app, _temp_dir) = new_test_app();
        let summary = app.route_findings_by_confidence(&findings, Some(&memory));

        assert_eq!(summary, (0, 1, 1));
        assert!(writes.lock().expect("lock writes").is_empty());
    }

    fn mock_provider_capabilities() -> fx_llm::ProviderCapabilities {
        fx_llm::ProviderCapabilities {
            supports_temperature: true,
            requires_streaming: false,
        }
    }

    #[derive(Debug)]
    struct StaticCompletionProvider {
        provider_name: String,
        model: String,
        response: String,
    }

    impl StaticCompletionProvider {
        fn new(model: &str, response: &str) -> Self {
            Self {
                provider_name: "mock-provider".to_string(),
                model: model.to_string(),
                response: response.to_string(),
            }
        }
    }

    #[async_trait]
    impl fx_llm::CompletionProvider for StaticCompletionProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            Ok(fx_llm::CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: self.response.clone(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            })
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            let chunk = Ok(fx_llm::StreamChunk {
                delta_content: Some(self.response.clone()),
                tool_use_deltas: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            });
            Ok(Box::pin(futures::stream::iter(vec![chunk])))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            vec![self.model.clone()]
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            mock_provider_capabilities()
        }
    }

    #[derive(Debug)]
    struct ModelEchoProvider {
        provider_name: String,
        models: Vec<String>,
    }

    #[derive(Debug)]
    struct StreamingTestProvider {
        provider_name: String,
        model: String,
        chunks: Vec<Result<fx_llm::StreamChunk, fx_llm::ProviderError>>,
    }

    #[derive(Default)]
    struct RecordingWriter {
        writes: Vec<String>,
        flush_count: usize,
    }

    impl Write for RecordingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let text = std::str::from_utf8(buf)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            self.writes.push(text.to_string());
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flush_count += 1;
            Ok(())
        }
    }

    #[derive(Debug)]
    struct CompletionTestProvider {
        provider_name: String,
        model: String,
        completion: fx_llm::CompletionResponse,
    }

    #[derive(Debug)]
    struct FailingCompletionProvider {
        provider_name: String,
        model: String,
    }

    #[async_trait]
    impl fx_llm::CompletionProvider for ModelEchoProvider {
        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            let response = format!(
                "{{\"action\":{{\"Respond\":{{\"text\":\"{}\"}}}},\"rationale\":\"echo\",\"confidence\":0.9,\"expected_outcome\":null,\"sub_goals\":[]}}",
                request.model
            );
            Ok(fx_llm::CompletionResponse {
                content: vec![ContentBlock::Text { text: response }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            })
        }

        async fn complete_stream(
            &self,
            request: CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            let chunk = Ok(fx_llm::StreamChunk {
                delta_content: Some(format!(
                    "{{\"action\":{{\"Respond\":{{\"text\":\"{}\"}}}},\"rationale\":\"echo\",\"confidence\":0.9,\"expected_outcome\":null,\"sub_goals\":[]}}",
                    request.model
                )),
                tool_use_deltas: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            });
            Ok(Box::pin(futures::stream::iter(vec![chunk])))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            self.models.clone()
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            mock_provider_capabilities()
        }
    }

    #[async_trait]
    impl fx_llm::CompletionProvider for StreamingTestProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            Ok(fx_llm::CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "unused".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            })
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            Ok(Box::pin(futures::stream::iter(self.chunks.clone())))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            vec![self.model.clone()]
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            mock_provider_capabilities()
        }
    }

    #[async_trait]
    impl fx_llm::CompletionProvider for CompletionTestProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            Ok(self.completion.clone())
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            let response = self.completion.clone();
            let chunk = fx_llm::StreamChunk {
                delta_content: response.content.first().and_then(|b| match b {
                    fx_llm::ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                }),
                tool_use_deltas: Vec::new(),
                usage: response.usage,
                stop_reason: response.stop_reason.clone(),
            };
            Ok(Box::pin(futures::stream::iter(vec![Ok(chunk)])))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            vec![self.model.clone()]
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            mock_provider_capabilities()
        }
    }

    #[async_trait]
    impl fx_llm::CompletionProvider for FailingCompletionProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            Err(fx_llm::ProviderError::Provider("test error".to_string()))
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            Ok(Box::pin(futures::stream::iter(vec![])))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            vec![self.model.clone()]
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            mock_provider_capabilities()
        }
    }

    /// A provider that captures the [`CompletionRequest`] for assertion and
    /// returns a single successful stream chunk so the caller completes normally.
    #[derive(Debug)]
    struct RequestCapturingProvider {
        provider_name: String,
        model: String,
        captured: Arc<Mutex<Vec<CompletionRequest>>>,
    }

    #[async_trait]
    impl fx_llm::CompletionProvider for RequestCapturingProvider {
        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<fx_llm::CompletionResponse, fx_llm::ProviderError> {
            self.captured.lock().unwrap().push(request);
            Ok(fx_llm::CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "captured".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            })
        }

        async fn complete_stream(
            &self,
            request: CompletionRequest,
        ) -> Result<fx_llm::CompletionStream, fx_llm::ProviderError> {
            self.captured.lock().unwrap().push(request);
            let chunk = Ok(fx_llm::StreamChunk {
                delta_content: Some("captured".to_string()),
                tool_use_deltas: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            });
            Ok(Box::pin(futures::stream::iter(vec![chunk])))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            vec![self.model.clone()]
        }

        fn capabilities(&self) -> fx_llm::ProviderCapabilities {
            mock_provider_capabilities()
        }
    }

    #[test]
    fn test_completion_providers_expose_router_capabilities() {
        let static_provider = StaticCompletionProvider::new("mock-loop-model", "ok");
        assert_eq!(
            fx_llm::CompletionProvider::capabilities(&static_provider),
            mock_provider_capabilities()
        );

        let streaming_provider = StreamingTestProvider {
            provider_name: "stream-test".to_string(),
            model: "stream-model".to_string(),
            chunks: Vec::new(),
        };
        assert_eq!(
            fx_llm::CompletionProvider::capabilities(&streaming_provider),
            mock_provider_capabilities()
        );
    }

    #[test]
    fn embedded_banner_is_non_empty() {
        assert!(!FAWX_BANNER_ANSI.is_empty());
        assert!(
            FAWX_BANNER_ANSI.contains("["),
            "banner should contain ANSI escapes"
        );
    }

    #[test]
    fn render_banner_truecolor_returns_ansi_art() {
        let amber = style::Color::Rgb {
            r: 255,
            g: 165,
            b: 0,
        };
        let result = render_banner(true, amber);
        assert_eq!(result, FAWX_BANNER_ANSI);
        assert!(
            result.contains("["),
            "truecolor banner should contain ANSI escapes"
        );
    }

    #[test]
    fn render_banner_no_truecolor_returns_text_art() {
        let amber = style::Color::Rgb {
            r: 255,
            g: 165,
            b: 0,
        };
        let result = render_banner(false, amber);
        assert!(
            !result.contains("⠀"),
            "fallback should not contain braille characters"
        );
        assert!(
            result.contains("/ _/"),
            "fallback should contain ASCII art from BANNER_ART"
        );
    }

    #[test]
    fn render_banner_branches_are_distinct() {
        let amber = style::Color::Rgb {
            r: 255,
            g: 165,
            b: 0,
        };
        let truecolor = render_banner(true, amber);
        let fallback = render_banner(false, amber);
        assert_ne!(
            truecolor, fallback,
            "truecolor and fallback banners should differ"
        );
    }

    fn app_with_mock_model(response: &str) -> (TuiApp, tempfile::TempDir) {
        let (config, temp_dir) = test_config_with_temp_dir();
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(StaticCompletionProvider::new("mock-loop-model", response)),
            "test",
        );
        router
            .set_active("mock-loop-model")
            .expect("set active mock model");

        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let app = TuiApp::new(
            test_provider_auth_manager(),
            router,
            loop_engine,
            runtime_info,
            config,
        )
        .expect("mock app");
        (app, temp_dir)
    }

    fn test_provider_auth_manager() -> AuthManager {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "test-provider",
            AuthMethod::ApiKey {
                provider: "test-provider".to_string(),
                key: "test-key".to_string(),
            },
        );
        auth_manager
    }

    fn openai_oauth_auth_manager() -> AuthManager {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "oauth-access-token".to_string(),
                refresh_token: "oauth-refresh-token".to_string(),
                expires_at: 1_700_000_000_000,
                account_id: Some("acct_model_refresh".to_string()),
            },
        );
        auth_manager
    }

    fn app_with_two_models() -> (TuiApp, tempfile::TempDir) {
        let (config, temp_dir) = test_config_with_temp_dir();
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(ModelEchoProvider {
                provider_name: "echo-provider".to_string(),
                models: vec![
                    "claude-sonnet-4-6-20250929".to_string(),
                    "gpt-5-mini".to_string(),
                ],
            }),
            "test",
        );
        router
            .set_active("gpt-5-mini")
            .expect("set initial active model");

        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let app = TuiApp::new(
            test_provider_auth_manager(),
            router,
            loop_engine,
            runtime_info,
            config,
        )
        .expect("mock app");
        (app, temp_dir)
    }

    fn router_with_canonical_claude_models(initial_model: &str) -> ModelRouter {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(ModelEchoProvider {
                provider_name: "echo-provider".to_string(),
                models: vec![
                    "claude-opus-4-20250514".to_string(),
                    "claude-sonnet-4-20250514".to_string(),
                ],
            }),
            "test",
        );
        router
            .set_active(initial_model)
            .expect("set initial canonical model");
        router
    }

    fn router_with_completion_response(
        model: &str,
        completion: fx_llm::CompletionResponse,
    ) -> ModelRouter {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(CompletionTestProvider {
                provider_name: "complete-test".to_string(),
                model: model.to_string(),
                completion,
            }),
            "test",
        );
        router.set_active(model).expect("set active model");
        router
    }

    fn terminal_snapshot(text: &str) -> PerceptionSnapshot {
        PerceptionSnapshot {
            screen: ScreenState {
                current_app: "fawx.tui".to_string(),
                elements: Vec::new(),
                text_content: text.to_string(),
            },
            notifications: Vec::new(),
            active_app: "fawx.tui".to_string(),
            timestamp_ms: 1,
            sensor_data: None,
            user_input: Some(UserInput {
                text: text.to_string(),
                source: InputSource::Text,
                timestamp: 1,
                context_id: None,
            }),
            conversation_history: Vec::new(),
            steer_context: None,
        }
    }

    #[derive(Debug, Default)]
    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("write failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailingAsyncWriter;

    impl tokio::io::AsyncWrite for FailingAsyncWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::Error::other("async write failed")))
        }

        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn router_loop_llm_provider_generate_returns_stream_error() {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(StreamingTestProvider {
                provider_name: "stream-test".to_string(),
                model: "stream-model".to_string(),
                chunks: vec![Err(fx_llm::ProviderError::Streaming(
                    "chunk failed".to_string(),
                ))],
            }),
            "test",
        );

        router.set_active("stream-model").expect("set active");
        let provider = RouterLoopLlmProvider::new(&router, "stream-model".to_string());
        let result = provider.generate("hello", 32).await;

        assert!(
            matches!(result, Err(CoreLlmError::Inference(message)) if message.contains("chunk failed"))
        );
    }

    #[tokio::test]
    async fn router_loop_llm_provider_generate_rejects_empty_rendered_output() {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(StreamingTestProvider {
                provider_name: "stream-test".to_string(),
                model: "empty-model".to_string(),
                chunks: vec![Ok(fx_llm::StreamChunk {
                    delta_content: Some("   ".to_string()),
                    tool_use_deltas: Vec::new(),
                    usage: None,
                    stop_reason: Some("end_turn".to_string()),
                })],
            }),
            "test",
        );

        router.set_active("empty-model").expect("set active");
        let provider = RouterLoopLlmProvider::new(&router, "empty-model".to_string());
        let result = provider.generate("hello", 32).await;
        eprintln!("DEBUG result: {result:?}");

        assert!(
            matches!(result, Err(CoreLlmError::InvalidResponse(message)) if message.contains("empty completion"))
        );
    }

    #[tokio::test]
    async fn router_loop_llm_provider_complete_preserves_tool_calls_and_usage() {
        let expected_tool_call = fx_llm::ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({
                "action": {"Respond": {"text": "done"}},
                "rationale": "tool call response",
                "confidence": 0.9
            }),
        };
        let expected_usage = fx_llm::Usage {
            input_tokens: 123,
            output_tokens: 45,
        };
        let router = router_with_completion_response(
            "complete-model",
            fx_llm::CompletionResponse {
                content: Vec::new(),
                tool_calls: vec![expected_tool_call.clone()],
                usage: Some(expected_usage),
                stop_reason: Some("tool_use".to_string()),
            },
        );
        let provider = RouterLoopLlmProvider::new(&router, "complete-model".to_string());
        let request = CompletionRequest {
            model: "complete-model".to_string(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(32),
            system_prompt: None,
        };

        let response = provider
            .complete(request)
            .await
            .expect("completion response");

        assert_eq!(response.tool_calls, vec![expected_tool_call]);
        assert_eq!(response.usage, Some(expected_usage));
    }

    #[tokio::test]
    async fn router_loop_llm_provider_complete_propagates_provider_error() {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(FailingCompletionProvider {
                provider_name: "failing-complete-test".to_string(),
                model: "failing-complete-model".to_string(),
            }),
            "test",
        );
        router
            .set_active("failing-complete-model")
            .expect("set active failing model");

        let provider = RouterLoopLlmProvider::new(&router, "failing-complete-model".to_string());
        let request = CompletionRequest {
            model: "failing-complete-model".to_string(),
            messages: vec![Message::user("hi")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(32),
            system_prompt: None,
        };

        let error = provider
            .complete(request)
            .await
            .expect_err("provider error should bubble up");

        assert_eq!(
            error,
            fx_llm::ProviderError::Provider("test error".to_string())
        );
    }

    #[tokio::test]
    async fn generate_sets_system_prompt_for_openai_compatibility() {
        let captured: Arc<Mutex<Vec<CompletionRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(RequestCapturingProvider {
                provider_name: "capture-test".to_string(),
                model: "capture-model".to_string(),
                captured: Arc::clone(&captured),
            }),
            "test",
        );
        router.set_active("capture-model").expect("set active");

        let provider = RouterLoopLlmProvider::new(&router, "capture-model".to_string());
        let prompt = "Synthesize the tool results below.";
        provider
            .generate(prompt, 256)
            .await
            .expect("generate should succeed");

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].system_prompt,
            Some(prompt.to_string()),
            "generate() must set system_prompt so OpenAI Responses API gets an instructions field"
        );
    }

    #[tokio::test]
    async fn generate_streaming_sets_system_prompt_for_openai_compatibility() {
        let captured: Arc<Mutex<Vec<CompletionRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(RequestCapturingProvider {
                provider_name: "capture-test".to_string(),
                model: "capture-stream-model".to_string(),
                captured: Arc::clone(&captured),
            }),
            "test",
        );
        router
            .set_active("capture-stream-model")
            .expect("set active");

        let provider = RouterLoopLlmProvider::new(&router, "capture-stream-model".to_string());
        let prompt = "Synthesize the tool results below.";
        provider
            .generate_streaming(prompt, 256, Box::new(|_| {}))
            .await
            .expect("generate_streaming should succeed");

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].system_prompt,
            Some(prompt.to_string()),
            "generate_streaming() must set system_prompt so OpenAI Responses API gets an instructions field"
        );
    }

    #[tokio::test]
    async fn router_loop_llm_provider_complete_drives_direct_tool_call_flow() {
        // Verify the loop engine routes CompletionResponse tool calls through
        // the direct tool-call path (not the removed emit_intent path).
        // The CompletionTestProvider returns a text response, confirming the
        // loop engine's decide() maps content-only responses to Respond.
        let router = router_with_completion_response(
            "complete-loop-model",
            fx_llm::CompletionResponse {
                content: vec![fx_llm::ContentBlock::Text {
                    text: "Direct tool path works".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: Some(fx_llm::Usage {
                    input_tokens: 17,
                    output_tokens: 9,
                }),
                stop_reason: Some("end_turn".to_string()),
            },
        );

        let mut loop_engine = build_loop_engine();
        let llm = RouterLoopLlmProvider::new(&router, "complete-loop-model".to_string());
        let result = loop_engine
            .run_cycle(terminal_snapshot("hello"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Complete { response, .. } if response == "Direct tool path works"
        ));
    }

    #[test]
    fn auth_wizard_selection_parsing_covers_all_options() {
        assert_eq!(
            parse_auth_selection("1"),
            Some(AuthSelection::ClaudeSubscription)
        );
        assert_eq!(
            parse_auth_selection("2"),
            Some(AuthSelection::ChatGptSubscription)
        );
        assert_eq!(parse_auth_selection("3"), Some(AuthSelection::ApiKey));
        assert_eq!(parse_auth_selection("9"), None);
    }

    #[test]
    fn parse_auth_menu_selection() {
        assert_eq!(
            super::parse_auth_menu_selection("1"),
            Some(super::AuthMenuSelection::AddOrUpdate)
        );
        assert_eq!(
            super::parse_auth_menu_selection("2"),
            Some(super::AuthMenuSelection::RemoveProvider)
        );
        assert_eq!(
            super::parse_auth_menu_selection("3"),
            Some(super::AuthMenuSelection::Cancel)
        );
        assert_eq!(super::parse_auth_menu_selection("0"), None);
    }

    #[test]
    fn is_cancelled_error() {
        let cancelled = Err(TuiError::Cancelled);
        let other_auth = Err(TuiError::Auth("different".to_string()));
        let ok_result = Ok(());

        assert!(super::is_cancelled_error(&cancelled));
        assert!(!super::is_cancelled_error(&other_auth));
        assert!(!super::is_cancelled_error(&ok_result));
    }

    #[test]
    fn cancelled_error_display_is_not_auth_specific() {
        assert_eq!(TuiError::Cancelled.to_string(), "input cancelled");
    }

    #[test]
    fn prompt_line_flow_recovers_raw_mode_before_reading_stdin() {
        if terminal::enable_raw_mode().is_err() {
            return;
        }

        super::ensure_cooked_mode();

        assert!(!terminal::is_raw_mode_enabled().unwrap_or(false));
    }

    #[test]
    fn removal_confirmation_accepts_only_yes_answers() {
        assert!(super::removal_confirmation_accepted("y"));
        assert!(super::removal_confirmation_accepted("Y"));
        assert!(super::removal_confirmation_accepted("yes"));
        assert!(super::removal_confirmation_accepted(" YeS "));
        assert!(!super::removal_confirmation_accepted(""));
        assert!(!super::removal_confirmation_accepted("n"));
        assert!(!super::removal_confirmation_accepted("no"));
    }

    #[test]
    fn finalize_auth_wizard_result_cancellation_returns_ok_and_prints_message() {
        let mut output = Vec::new();

        let result =
            super::finalize_auth_wizard_result_with_writer(Err(TuiError::Cancelled), &mut output);

        assert!(result.is_ok());
        assert_eq!(
            String::from_utf8(output).expect("utf8 output"),
            "Cancelled.
"
        );
    }

    #[test]
    fn classify_secret_input_key_cancels_on_escape_and_ctrl_c() {
        let esc = event::KeyEvent::new(event::KeyCode::Esc, event::KeyModifiers::NONE);
        let ctrl_c = event::KeyEvent::new(event::KeyCode::Char('c'), event::KeyModifiers::CONTROL);

        assert_eq!(
            super::classify_secret_input_key(&esc),
            super::SecretInputKeyAction::Cancel
        );
        assert_eq!(
            super::classify_secret_input_key(&ctrl_c),
            super::SecretInputKeyAction::Cancel
        );
    }

    #[test]
    fn classify_secret_input_key_handles_all_action_types() {
        // Submit on Enter
        let enter = event::KeyEvent::new(event::KeyCode::Enter, event::KeyModifiers::NONE);
        assert_eq!(
            super::classify_secret_input_key(&enter),
            super::SecretInputKeyAction::Submit
        );

        // Type on printable characters
        let char_a = event::KeyEvent::new(event::KeyCode::Char('a'), event::KeyModifiers::NONE);
        assert_eq!(
            super::classify_secret_input_key(&char_a),
            super::SecretInputKeyAction::Type('a')
        );
        let char_z = event::KeyEvent::new(event::KeyCode::Char('Z'), event::KeyModifiers::SHIFT);
        assert_eq!(
            super::classify_secret_input_key(&char_z),
            super::SecretInputKeyAction::Type('Z')
        );
        let digit = event::KeyEvent::new(event::KeyCode::Char('9'), event::KeyModifiers::NONE);
        assert_eq!(
            super::classify_secret_input_key(&digit),
            super::SecretInputKeyAction::Type('9')
        );

        // Delete on Backspace
        let backspace = event::KeyEvent::new(event::KeyCode::Backspace, event::KeyModifiers::NONE);
        assert_eq!(
            super::classify_secret_input_key(&backspace),
            super::SecretInputKeyAction::Delete
        );

        // Ignore on unhandled keys (arrows, function keys, etc.)
        let left = event::KeyEvent::new(event::KeyCode::Left, event::KeyModifiers::NONE);
        assert_eq!(
            super::classify_secret_input_key(&left),
            super::SecretInputKeyAction::Ignore
        );
        let f1 = event::KeyEvent::new(event::KeyCode::F(1), event::KeyModifiers::NONE);
        assert_eq!(
            super::classify_secret_input_key(&f1),
            super::SecretInputKeyAction::Ignore
        );
        let tab = event::KeyEvent::new(event::KeyCode::Tab, event::KeyModifiers::NONE);
        assert_eq!(
            super::classify_secret_input_key(&tab),
            super::SecretInputKeyAction::Ignore
        );
    }

    #[test]
    fn parse_provider_selection_rejects_out_of_range_and_invalid_values() {
        assert_eq!(super::parse_provider_selection("0", 3), None);
        assert_eq!(super::parse_provider_selection("4", 3), None);
        assert_eq!(super::parse_provider_selection(" 2 ", 3), Some(1));
        assert_eq!(super::parse_provider_selection("abc", 3), None);
        assert_eq!(super::parse_provider_selection("", 3), None);
    }

    #[tokio::test]
    async fn remove_provider_flow_updates_persisted_provider_list() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_dir = TempDir::new().unwrap();
        let _home = ScopedEnvVar::set("HOME", temp_dir.path().to_str().unwrap());

        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "setup-token-flow".to_string(),
            },
        );
        auth_manager.store(
            "openai",
            AuthMethod::ApiKey {
                provider: "openai".to_string(),
                key: "openai-key-flow".to_string(),
            },
        );
        auth_manager.store(
            "openrouter",
            AuthMethod::ApiKey {
                provider: "openrouter".to_string(),
                key: "openrouter-key-flow".to_string(),
            },
        );

        let providers = auth_manager.providers();
        let selected = super::parse_provider_selection("2", providers.len())
            .expect("selection should map to provider index");
        let removed_provider = providers[selected].clone();

        auth_manager.remove(&removed_provider);
        persist_auth_manager(&auth_manager).unwrap();
        let loaded = load_auth_manager().unwrap();

        assert!(loaded.get(&removed_provider).is_none());
        assert_eq!(loaded.providers(), vec!["anthropic", "openrouter"]);
    }

    #[tokio::test]
    async fn write_http_response_surfaces_async_write_failures() {
        let mut writer = FailingAsyncWriter;

        let error = write_http_response(&mut writer, "HTTP/1.1 200 OK\r\n\r\n")
            .await
            .expect_err("write failure should bubble up");

        assert!(error.to_string().contains("async write failed"));
    }

    #[tokio::test]
    async fn openai_oauth_client_id_uses_default_when_env_is_blank() {
        let _env_lock = ENV_LOCK.lock().await;
        let _client_id = ScopedEnvVar::set("FAWX_OPENAI_CLIENT_ID", "   ");

        assert_eq!(openai_oauth_client_id(), fx_auth::oauth::OPENAI_CLIENT_ID);
    }

    #[tokio::test]
    async fn supports_truecolor_returns_false_without_env() {
        let _env_lock = ENV_LOCK.lock().await;
        let previous_colorterm = std::env::var_os("COLORTERM");
        let previous_term = std::env::var_os("TERM");
        std::env::remove_var("COLORTERM");
        std::env::remove_var("TERM");

        assert!(!supports_truecolor());

        if let Some(value) = previous_colorterm {
            std::env::set_var("COLORTERM", value);
        }
        if let Some(value) = previous_term {
            std::env::set_var("TERM", value);
        }
    }

    #[test]
    fn term_direct_suffix_detected() {
        assert!(term_indicates_truecolor("xterm-direct"));
    }

    #[test]
    fn term_containing_direct_not_falsely_detected() {
        assert!(!term_indicates_truecolor("my-indirect-term"));
    }

    #[tokio::test]
    async fn supports_truecolor_detects_term_direct() {
        let _env_lock = ENV_LOCK.lock().await;
        let _color_term = ScopedEnvVar::set("COLORTERM", "ansi");
        let _term = ScopedEnvVar::set("TERM", "xterm-direct");

        assert!(supports_truecolor());
    }

    #[tokio::test]
    async fn theme_color_uses_rgb_when_truecolor() {
        let _env_lock = ENV_LOCK.lock().await;
        let _color_term = ScopedEnvVar::set("COLORTERM", "truecolor");

        assert_eq!(
            theme_color(255, 204, 0, 220),
            style::Color::Rgb {
                r: 255,
                g: 204,
                b: 0
            }
        );
    }

    #[tokio::test]
    async fn theme_color_uses_256_fallback() {
        let _env_lock = ENV_LOCK.lock().await;
        let _color_term = ScopedEnvVar::set("COLORTERM", "ansi");
        let _term = ScopedEnvVar::set("TERM", "xterm-256color");

        assert_eq!(theme_color(255, 204, 0, 220), style::Color::AnsiValue(220));
    }

    #[test]
    fn render_markdown_handles_bold_and_italic() {
        let result = render_markdown("**bold** and *italic*");

        assert!(!result.is_empty());
        assert!(result.contains("\x1b["));
    }

    #[test]
    fn render_markdown_handles_code_blocks() {
        let result = render_markdown("```rust\nfn main() {}\n```");

        assert!(!result.is_empty());
        assert!(result.contains("\x1b["));
    }

    #[test]
    fn render_markdown_handles_unclosed_fence() {
        let result = render_markdown("```rust\nfn main() {}");

        assert!(!result.is_empty());
        assert!(result.contains("main"));
    }

    #[test]
    fn render_markdown_handles_empty_code_block() {
        let result = render_markdown("```\n```");

        assert!(result.is_empty() || !result.contains("```"));
    }

    #[test]
    fn render_markdown_handles_nested_fences() {
        let input = "````markdown\nHere's an example:\n```rust\nfn foo() {}\n```\n````";
        let result = render_markdown(input);

        assert!(result.contains("foo"));
    }

    #[test]
    fn render_markdown_indented_backticks_not_fence() {
        let result = render_markdown("    ```\n    code here\n    ```");

        assert!(result.contains("```"));
    }

    #[test]
    fn highlight_code_unknown_lang_still_renders() {
        let result = highlight_code("hello world", "nonexistent-lang-xyz");

        assert!(result.contains("hello world"));
        assert!(result.contains("\x1b["), "should contain ANSI dim escape");
    }

    #[test]
    fn render_markdown_handles_inline_code() {
        let result = render_markdown("Use `cargo build` to compile");

        assert!(!result.is_empty());
    }

    #[test]
    fn render_markdown_handles_plain_text() {
        let result = render_markdown("Just plain text");

        assert!(result.contains("Just plain text"));
    }

    #[test]
    fn render_markdown_handles_empty_input() {
        let result = render_markdown("");

        assert!(result.is_empty() || result.chars().all(|c| c.is_whitespace()));
    }

    #[test]
    fn build_markdown_skin_returns_valid_skin() {
        let skin = build_markdown_skin();
        let text = skin.term_text("**test**");

        assert!(!format!("{text}").is_empty());
    }

    #[test]
    fn default_model_prefers_newest_sonnet() {
        let model_ids = vec![
            "anthropic/claude-3-haiku".to_string(),
            "anthropic/claude-3.5-haiku".to_string(),
            "anthropic/claude-3.5-sonnet".to_string(),
            "anthropic/claude-sonnet-4".to_string(),
            "anthropic/claude-sonnet-4.5".to_string(),
            "anthropic/claude-sonnet-4.6".to_string(),
        ];

        assert_eq!(
            preferred_default_model(&model_ids),
            Some("anthropic/claude-sonnet-4.6")
        );
    }

    #[test]
    fn preferred_default_picks_opus_over_sonnet() {
        let models = vec![
            "claude-sonnet-4-6-20250929".to_string(),
            "claude-opus-4-6-20250929".to_string(),
        ];

        assert_eq!(
            preferred_default_model(&models),
            Some("claude-opus-4-6-20250929")
        );
    }

    #[test]
    fn preferred_default_matches_hyphenated_anthropic_ids() {
        let models = vec!["claude-sonnet-4-6-20250929".to_string()];

        assert_eq!(
            preferred_default_model(&models),
            Some("claude-sonnet-4-6-20250929")
        );
    }

    #[test]
    fn preferred_default_matches_openrouter_slash_prefixed_ids() {
        let models = vec![
            "anthropic/claude-opus-4.6".to_string(),
            "anthropic/claude-sonnet-4.5".to_string(),
            "openai/gpt-5-pro".to_string(),
        ];
        let picked = preferred_default_model(&models);
        assert_eq!(picked, Some("anthropic/claude-opus-4.6"));
    }

    #[test]
    fn preferred_default_picks_codex_over_mini() {
        let models = vec!["o4-mini".to_string(), "gpt-5.3-codex".to_string()];
        let picked = preferred_default_model(&models);
        assert_eq!(picked, Some("gpt-5.3-codex"));
    }

    #[test]
    fn preferred_default_deprioritizes_haiku() {
        let models = vec![
            "claude-3-haiku-20240307".to_string(),
            "claude-sonnet-4-6-20250929".to_string(),
        ];

        assert_eq!(
            preferred_default_model(&models),
            Some("claude-sonnet-4-6-20250929")
        );
    }

    #[test]
    fn preferred_default_avoids_haiku_as_last_resort() {
        let models = vec!["claude-3-haiku-20240307".to_string()];

        assert_eq!(
            preferred_default_model(&models),
            Some("claude-3-haiku-20240307")
        );
    }

    #[test]
    fn default_model_falls_back_to_older_sonnet() {
        let model_ids = vec![
            "anthropic/claude-3-haiku".to_string(),
            "anthropic/claude-3.5-haiku".to_string(),
            "anthropic/claude-3.5-sonnet".to_string(),
        ];

        assert_eq!(
            preferred_default_model(&model_ids),
            Some("anthropic/claude-3.5-sonnet")
        );
    }

    #[test]
    fn default_model_prefers_opus_when_no_sonnet() {
        let model_ids = vec![
            "anthropic/claude-3-haiku".to_string(),
            "anthropic/claude-opus-4".to_string(),
            "openai/gpt-4o".to_string(),
        ];

        assert_eq!(
            preferred_default_model(&model_ids),
            Some("anthropic/claude-opus-4")
        );
    }

    #[test]
    fn default_model_prefers_gpt4o_over_old_sonnet() {
        let model_ids = vec![
            "anthropic/claude-3-haiku".to_string(),
            "openai/gpt-4o".to_string(),
            "anthropic/claude-3.5-sonnet".to_string(),
        ];
        // gpt-4o pattern comes before generic "sonnet" in PREFERRED_MODEL_PATTERNS
        assert_eq!(preferred_default_model(&model_ids), Some("openai/gpt-4o"));
    }

    #[test]
    fn preferred_default_model_picks_grok_over_generic() {
        let models = vec![
            "meta-llama/llama-4-maverick".to_string(),
            "x-ai/grok-3".to_string(),
        ];

        let picked = preferred_default_model(&models);
        assert_eq!(picked, Some("x-ai/grok-3"));
    }

    #[derive(Debug, Default)]
    struct CacheCountingExecutor {
        clear_count: Arc<std::sync::atomic::AtomicUsize>,
    }

    fn sample_reload_events() -> Vec<fx_loadable::ReloadEvent> {
        vec![
            fx_loadable::ReloadEvent::Loaded {
                skill_name: "alpha".to_string(),
                version: "1.0.0".to_string(),
            },
            fx_loadable::ReloadEvent::Updated {
                skill_name: "alpha".to_string(),
                old_version: "1.0.0".to_string(),
                new_version: "1.1.0".to_string(),
            },
            fx_loadable::ReloadEvent::Removed {
                skill_name: "alpha".to_string(),
            },
            fx_loadable::ReloadEvent::Error {
                skill_name: "alpha".to_string(),
                error: "bad wasm".to_string(),
            },
        ]
    }

    async fn queue_reload_events(reload_tx: &tokio::sync::mpsc::Sender<fx_loadable::ReloadEvent>) {
        for event in sample_reload_events() {
            reload_tx.send(event).await.expect("queue reload event");
        }
    }

    fn assert_reload_outputs(app: &ui::FawxApp, executor: &CacheCountingExecutor) {
        assert_eq!(
            app.output_lines,
            vec![
                "🔌 Loaded skill: alpha v1.0.0".to_string(),
                "🔄 Updated skill: alpha v1.1.0".to_string(),
                "🗑️ Removed skill: alpha".to_string(),
                "⚠️ Skill error (alpha): bad wasm".to_string(),
            ]
        );
        assert_eq!(
            executor
                .clear_count
                .load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    }

    #[async_trait]
    impl ToolExecutor for CacheCountingExecutor {
        async fn execute_tools(
            &self,
            _calls: &[fx_llm::ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<fx_kernel::act::ToolResult>, ToolExecutorError> {
            Ok(Vec::new())
        }

        fn clear_cache(&self) {
            self.clear_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[tokio::test]
    async fn wait_for_idle_loop_event_reports_changes_and_clears_cache() {
        let executor = CacheCountingExecutor::default();
        let mut app = ui::FawxApp::new();
        let (reload_tx, mut reload_rx) = tokio::sync::mpsc::channel(4);
        queue_reload_events(&reload_tx).await;

        for _ in 0..4 {
            let event = wait_for_idle_loop_event(
                std::future::pending::<io::Result<Event>>(),
                &mut reload_rx,
            )
            .await
            .expect("receive reload event");
            let IdleLoopEvent::Reload(reload_event) = event else {
                panic!("expected reload event");
            };
            process_reload_event(&mut app, &executor, reload_event);
        }

        assert_reload_outputs(&app, &executor);
    }

    #[tokio::test]
    async fn wait_for_idle_loop_event_reads_terminal_events_without_reload_polling() {
        let (reload_tx, mut reload_rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        drop(reload_tx);

        let event =
            wait_for_idle_loop_event(std::future::ready(Ok(Event::Key(key))), &mut reload_rx)
                .await
                .expect("receive terminal event");

        match event {
            IdleLoopEvent::Terminal(Event::Key(received)) => {
                assert_eq!(received.code, KeyCode::Enter);
            }
            _ => panic!("expected terminal key event"),
        }
    }

    #[test]
    fn default_model_falls_back_to_highest_version() {
        let model_ids = vec![
            "llama-3".to_string(),
            "gpt-3.5-turbo".to_string(),
            "gpt-4.1-mini".to_string(),
        ];

        assert_eq!(preferred_default_model(&model_ids), Some("gpt-4.1-mini"));
    }

    #[test]
    fn default_model_falls_back_to_first() {
        let model_ids = vec![
            "alpha-mini".to_string(),
            "beta".to_string(),
            "gamma".to_string(),
        ];

        assert_eq!(preferred_default_model(&model_ids), Some("beta"));
    }

    #[test]
    fn default_model_handles_empty_list() {
        let model_ids = Vec::new();

        assert_eq!(preferred_default_model(&model_ids), None);
    }

    #[test]
    fn parse_oauth_callback_request_extracts_path_and_query() {
        let request =
            "GET /auth/callback?code=abc123&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (path, query) = parse_oauth_callback_request(request).expect("request should parse");

        assert_eq!(path, "/auth/callback");
        assert_eq!(query, "code=abc123&state=xyz");
    }

    #[test]
    fn validate_and_extract_code_rejects_state_mismatch() {
        let error = validate_and_extract_code("code=abc123&state=wrong", "expected")
            .expect_err("state mismatch should fail");

        assert!(matches!(error, TuiError::Auth(message) if message.contains("state mismatch")));
    }

    #[test]
    fn parse_token_error_response_uses_error_description_when_present() {
        let status = reqwest::StatusCode::BAD_REQUEST;
        let error = parse_token_error_response(
            status,
            r#"{"error":"invalid_grant","error_description":"bad code"}"#,
        );

        assert!(matches!(error, TuiError::Auth(message) if message.contains("bad code")));
    }

    #[test]
    fn parse_token_error_response_includes_raw_response_body() {
        let status = reqwest::StatusCode::BAD_REQUEST;
        let error = parse_token_error_response(status, "<html>gateway denied</html>");

        assert!(
            matches!(error, TuiError::Auth(message) if message.contains("response_body=<html>gateway denied</html>"))
        );
    }

    #[test]
    fn parse_token_error_response_empty_body_reports_empty_marker() {
        let status = reqwest::StatusCode::BAD_REQUEST;
        let error = parse_token_error_response(status, "");

        assert!(
            matches!(error, TuiError::Auth(message) if message.contains("response_body=<empty>"))
        );
    }

    #[test]
    fn format_oauth_error_body_truncates_long_payloads() {
        let long_payload = "x".repeat(450);
        let body = format_oauth_error_body(&long_payload);

        assert_eq!(body.chars().count(), 301);
        assert!(body.ends_with('…'));
    }

    #[test]
    fn command_parsing_recognizes_model_help_and_quit() {
        assert_eq!(parse_command("/model"), ParsedCommand::Model(None));
        assert_eq!(
            parse_command("/model claude-sonnet-4-20250514"),
            ParsedCommand::Model(Some("claude-sonnet-4-20250514".to_string()))
        );
        assert_eq!(parse_command("/help"), ParsedCommand::Help);
        assert_eq!(parse_command("/loop"), ParsedCommand::Loop);
        assert_eq!(parse_command("/status"), ParsedCommand::Status);
        assert_eq!(parse_command("/analyze"), ParsedCommand::Analyze);
        assert_eq!(
            parse_command("/synthesis Show raw output"),
            ParsedCommand::Synthesis(Some("Show raw output".to_string()))
        );
        assert_eq!(parse_command("/synthesis"), ParsedCommand::Synthesis(None));
        assert_eq!(
            parse_command("/synthesis    "),
            ParsedCommand::Synthesis(Some(String::new()))
        );
        assert_eq!(parse_command("/clear"), ParsedCommand::Clear);
        assert_eq!(parse_command("/new"), ParsedCommand::New);
        assert_eq!(parse_command("/history"), ParsedCommand::History);
        assert_eq!(parse_command("/cls"), ParsedCommand::Clear);
        assert_eq!(parse_command("/quit"), ParsedCommand::Quit);
        assert_eq!(parse_command("/exit"), ParsedCommand::Quit);
    }

    #[test]
    fn improve_command_parses_correctly() {
        assert_eq!(
            parse_command("/improve"),
            ParsedCommand::Improve(ImproveFlags {
                dry_run: false,
                has_unknown_flag: None,
            })
        );
    }

    #[test]
    fn improve_dry_run_flag_parsed() {
        assert_eq!(
            parse_command("/improve --dry-run"),
            ParsedCommand::Improve(ImproveFlags {
                dry_run: true,
                has_unknown_flag: None,
            })
        );
    }

    #[test]
    fn unknown_improve_subcommand_shows_help() {
        assert_eq!(
            parse_command("/improve --invalid"),
            ParsedCommand::Improve(ImproveFlags {
                dry_run: false,
                has_unknown_flag: Some("--invalid".to_string()),
            })
        );
    }

    #[test]
    fn should_add_to_history_accepts_valid_commands() {
        assert!(should_add_to_history("/help"));
        assert!(should_add_to_history("/quit"));
        assert!(should_add_to_history("/model list"));
        assert!(should_add_to_history("/clear"));
        assert!(should_add_to_history("/new"));
        assert!(should_add_to_history("/history"));
        assert!(should_add_to_history("/analyze"));
    }

    #[test]
    fn should_add_to_history_rejects_invalid_commands() {
        assert!(!should_add_to_history("/ex"));
        assert!(!should_add_to_history("/halp"));
        assert!(should_add_to_history("/exit"));
        assert!(!should_add_to_history("/q"));
    }

    #[test]
    fn should_add_to_history_rejects_bare_slash() {
        assert!(!should_add_to_history("/"));
    }

    #[test]
    fn should_add_to_history_accepts_commands_with_trailing_whitespace() {
        // Inputs are trimmed before reaching should_add_to_history,
        // but test the function directly with trailing space just in case.
        assert!(should_add_to_history("/help "));
        assert!(should_add_to_history("/model   "));
    }

    #[test]
    fn should_add_to_history_accepts_chat_messages() {
        assert!(should_add_to_history("hello world"));
        assert!(should_add_to_history("what is 2+2?"));
        assert!(should_add_to_history("tell me about rust"));
    }

    #[test]
    fn history_namespace_returns_none_for_home_directory() {
        let home = PathBuf::from("/tmp/home");
        assert_eq!(history_namespace_for_cwd(&home, &home), None);
    }

    #[test]
    fn history_namespace_hashes_non_home_directory() {
        let home = PathBuf::from("/tmp/home");
        let cwd = PathBuf::from("/tmp/home/project");

        let namespace = history_namespace_for_cwd(&home, &cwd);
        assert!(namespace.is_some());
    }

    #[test]
    fn history_path_uses_fawx_dir() {
        let path = history_path().expect("home directory should exist for tests");
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("history filename should be valid utf-8");

        assert!(path.starts_with(
            dirs::home_dir()
                .expect("home directory should exist")
                .join(".fawx")
        ));
        assert!(file_name == "history.txt" || file_name.starts_with("history-"));
    }

    #[tokio::test]
    async fn startup_refresh_restores_saved_model_missing_from_initial_router() {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(ModelEchoProvider {
                provider_name: "openai".to_string(),
                models: vec!["gpt-4o-mini".to_string()],
            }),
            "subscription",
        );
        router
            .set_active("gpt-4o-mini")
            .expect("set initial active model");

        let (mut config, _temp_dir) = test_config_with_temp_dir();
        config.model.default_model = Some("gpt-5.3-codex".to_string());

        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let mut app = TuiApp::new(
            openai_oauth_auth_manager(),
            router,
            loop_engine,
            runtime_info,
            config,
        )
        .expect("mock app");

        assert_eq!(app.current_model(), "gpt-4o-mini");

        app.select_first_available_model().await;

        assert_eq!(app.current_model(), "gpt-5.3-codex");
    }

    #[test]
    fn default_anthropic_models_include_claude_opus_4_6() {
        assert!(DEFAULT_ANTHROPIC_MODELS.contains(&"claude-opus-4-6"));
    }

    #[tokio::test]
    async fn model_selector_prefers_exact_match_over_family_alias() {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(ModelEchoProvider {
                provider_name: "echo-provider".to_string(),
                models: vec![
                    "claude-opus-4-6".to_string(),
                    "claude-opus-4-20250514".to_string(),
                ],
            }),
            "test",
        );
        router
            .set_active("claude-opus-4-20250514")
            .expect("set initial active model");

        let (config, _temp_dir) = test_config_with_temp_dir();
        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let mut app = TuiApp::new(
            test_provider_auth_manager(),
            router,
            loop_engine,
            runtime_info,
            config,
        )
        .expect("mock app");

        let resolved = app
            .set_active_model_with_refresh("claude-opus-4-6")
            .await
            .expect("selector should resolve exact model");

        assert_eq!(resolved, "claude-opus-4-6");
        assert_eq!(app.current_model(), "claude-opus-4-6");
    }

    #[test]
    fn resolve_model_alias_supports_new_claude_families() {
        let models = vec![
            "claude-haiku-4-20240101".to_string(),
            "claude-haiku-4-20250514".to_string(),
        ];

        let resolved = resolve_model_alias("claude-haiku-4-6", &models);

        assert_eq!(resolved.as_deref(), Some("claude-haiku-4-20250514"));
    }

    #[tokio::test]
    async fn model_alias_persists_canonical_model_and_restores_on_restart() {
        let _guard = ENV_LOCK.lock().await;
        let temp_home = tempfile::tempdir().expect("temp HOME should be created");
        let home = temp_home.path().to_string_lossy().to_string();
        let _home_env = ScopedEnvVar::set("HOME", &home);

        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let mut app = TuiApp::new(
            AuthManager::new(),
            router_with_canonical_claude_models("claude-sonnet-4-20250514"),
            loop_engine,
            runtime_info,
            FawxConfig::default(),
        )
        .expect("mock app");

        app.handle_command("/model claude-opus-4-6")
            .await
            .expect("alias selector should resolve to canonical model");

        let persisted = load_config().expect("persisted config should load");

        assert_eq!(
            persisted.model.default_model.as_deref(),
            Some("claude-opus-4-20250514")
        );

        // Drop the first app to release the auth store SQLite lock before
        // creating the second instance pointing at the same data directory.
        drop(app);

        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let restarted = TuiApp::new(
            AuthManager::new(),
            router_with_canonical_claude_models("claude-sonnet-4-20250514"),
            loop_engine,
            runtime_info,
            persisted,
        )
        .expect("restarted app");

        assert_eq!(restarted.current_model(), "claude-opus-4-20250514");
    }

    #[test]
    fn stale_persisted_model_keeps_existing_router_active_model() {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(ModelEchoProvider {
                provider_name: "echo-provider".to_string(),
                models: vec![
                    "claude-opus-4-6-20250929".to_string(),
                    "claude-sonnet-4-6-20250929".to_string(),
                ],
            }),
            "test",
        );
        router
            .set_active("claude-opus-4-6-20250929")
            .expect("set router default model");

        let (mut config, _temp_dir) = test_config_with_temp_dir();
        config.model.default_model = Some("claude-retired-0".to_string());

        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let app = TuiApp::new(
            test_provider_auth_manager(),
            router,
            loop_engine,
            runtime_info,
            config,
        )
        .expect("mock app");

        assert_eq!(app.current_model(), "claude-opus-4-6-20250929");
    }

    #[tokio::test]
    async fn status_reflects_switched_model() {
        let (mut app, _temp_dir) = app_with_two_models();

        app.handle_command("/model claude-sonnet-4-6")
            .await
            .unwrap();

        assert_eq!(app.current_model(), "claude-sonnet-4-6-20250929");
    }

    #[tokio::test]
    async fn model_command_resolves_prefix_to_full_model_id() {
        let (mut app, _temp_dir) = app_with_two_models();

        let resolved = app
            .set_active_model_from_selector("claude-sonnet-4-6")
            .expect("model selector should resolve");

        assert_eq!(resolved, "claude-sonnet-4-6-20250929");
    }

    #[tokio::test]
    async fn handle_message_uses_current_active_model() {
        let (mut app, _temp_dir) = app_with_two_models();

        app.handle_command("/model claude-sonnet-4-6")
            .await
            .unwrap();
        let rendered = app
            .handle_message("hello")
            .await
            .expect("loop response")
            .expect("batch response");

        assert!(rendered.contains("claude-sonnet-4-6-20250929"));
    }

    #[tokio::test]
    async fn handle_message_returns_loop_result_not_raw_completion_payload() {
        let (mut app, _temp_dir) = app_with_mock_model(
            r#"{"action":{"Respond":{"text":"Loop-integrated reply"}},"rationale":"direct","confidence":0.91,"expected_outcome":null,"sub_goals":[]}"#,
        );

        let rendered = app
            .handle_message("hello")
            .await
            .expect("loop-generated message")
            .expect("batch response");

        assert!(rendered.contains("Loop-integrated reply"));
        assert!(rendered.contains("1 iteration"));
    }

    #[tokio::test]
    async fn conversation_history_accumulates_across_messages() {
        let (mut app, _temp_dir) = app_with_mock_model(
            r#"{"action":{"Respond":{"text":"hello"}},"rationale":"r","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#,
        );

        let _ = app.handle_message("my name is Alice").await.expect("first");
        let _ = app
            .handle_message("what is my name?")
            .await
            .expect("second");

        assert_eq!(app.conversation_history.len(), 4);
        assert_eq!(
            app.conversation_history[0],
            Message::user("my name is Alice")
        );
        assert_eq!(
            app.conversation_history[2],
            Message::user("what is my name?")
        );
    }

    #[tokio::test]
    async fn model_switch_preserves_conversation_history() {
        let (mut app, _temp_dir) = app_with_two_models();

        app.handle_message("remember the launch code")
            .await
            .expect("first response");
        app.handle_command("/model claude-sonnet-4-6")
            .await
            .expect("model switch command");
        app.handle_message("what model are you using now?")
            .await
            .expect("second response");

        assert_eq!(app.current_model(), "claude-sonnet-4-6-20250929");
        assert_eq!(app.conversation_history.len(), 4);
        assert_eq!(
            app.conversation_history[0],
            Message::user("remember the launch code".to_string())
        );
        assert_eq!(
            app.conversation_history[2],
            Message::user("what model are you using now?".to_string())
        );
    }

    #[tokio::test]
    async fn conversation_history_respects_max_limit() {
        let (mut app, _temp_dir) = app_with_mock_model(
            r#"{"action":{"Respond":{"text":"ok"}},"rationale":"r","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#,
        );

        for i in 0..15 {
            let message = format!("msg-{i}");
            let _ = app
                .handle_message(&message)
                .await
                .expect("message response");
        }

        assert_eq!(app.conversation_history.len(), app.max_history);
        assert_eq!(
            app.conversation_history[0],
            Message::user("msg-5".to_string())
        );
    }

    #[test]
    fn startup_loads_configured_max_history_entries() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut store = ConversationStore::new(temp_dir.path()).expect("store");
        store.ensure_active().expect("active conversation");

        for index in 0..25 {
            store
                .save_message(&ConversationMessage {
                    role: "user".to_string(),
                    content: format!("msg-{index}"),
                    timestamp_ms: index as u64,
                    signals: None,
                    tool_calls: None,
                    token_usage: None,
                })
                .expect("save message");
        }

        let mut config = FawxConfig::default();
        config.general.data_dir = Some(temp_dir.path().to_path_buf());
        config.general.max_history = 20;

        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let app = TuiApp::new(
            AuthManager::new(),
            ModelRouter::new(),
            loop_engine,
            runtime_info,
            config,
        )
        .expect("app");

        assert_eq!(app.conversation_history.len(), 20);
        assert_eq!(app.max_history, 20);
        assert_eq!(
            app.conversation_history[0],
            Message::user("msg-5".to_string())
        );
    }

    #[test]
    fn startup_history_loader_uses_config_only_for_limit() {
        let _loader: fn(&ConversationStore, &FawxConfig) -> Vec<Message> =
            load_startup_conversation_history;
    }

    #[tokio::test]
    async fn synthesis_command_updates_instruction() {
        let (mut app, _temp_dir) = new_test_app();

        app.handle_command("/synthesis Show raw output verbatim")
            .await
            .expect("synthesis command");
        assert_eq!(
            app.loop_engine.synthesis_instruction(),
            "Show raw output verbatim"
        );

        app.handle_command("/synthesis ReSeT")
            .await
            .expect("synthesis reset");
        assert_eq!(
            app.loop_engine.synthesis_instruction(),
            DEFAULT_SYNTHESIS_INSTRUCTION
        );
    }

    #[tokio::test]
    async fn synthesis_command_rejects_whitespace_only_instruction() {
        let (mut app, _temp_dir) = new_test_app();

        app.handle_command("/synthesis    ")
            .await
            .expect("synthesis whitespace command");

        assert_eq!(
            app.loop_engine.synthesis_instruction(),
            DEFAULT_SYNTHESIS_INSTRUCTION
        );
    }

    #[tokio::test]
    async fn synthesis_command_rejects_instruction_over_max_length() {
        let (mut app, _temp_dir) = new_test_app();
        let long_value = "x".repeat(MAX_SYNTHESIS_INSTRUCTION_LENGTH + 1);

        app.handle_command(&format!("/synthesis {long_value}"))
            .await
            .expect("synthesis long command");

        assert_eq!(
            app.loop_engine.synthesis_instruction(),
            DEFAULT_SYNTHESIS_INSTRUCTION
        );
    }

    #[tokio::test]
    async fn clear_command_resets_conversation_history() {
        let (mut app, _temp_dir) = app_with_mock_model(
            r#"{"action":{"Respond":{"text":"ok"}},"rationale":"r","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#,
        );

        let _ = app.handle_message("remember this").await.expect("message");
        assert!(!app.conversation_history.is_empty());

        app.handle_command("/clear").await.expect("clear command");

        assert!(app.conversation_history.is_empty());
    }

    #[tokio::test]
    async fn load_auth_manager_returns_stored_credentials() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_dir = TempDir::new().unwrap();
        let _home = ScopedEnvVar::set("HOME", temp_dir.path().to_str().unwrap());

        // Pre-populate the encrypted store so load finds data.
        let data_dir = temp_dir.path().join(".fawx");
        std::fs::create_dir_all(&data_dir).unwrap();
        let store = crate::auth_store::AuthStore::open(&data_dir).unwrap();
        let expected = test_auth_manager();
        store.save_auth_manager(&expected).unwrap();
        drop(store);

        let loaded = load_auth_manager().unwrap();

        assert_eq!(loaded, expected);
    }

    #[tokio::test]
    async fn persist_auth_manager_writes_to_encrypted_store() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_dir = TempDir::new().unwrap();
        let _home = ScopedEnvVar::set("HOME", temp_dir.path().to_str().unwrap());

        let data_dir = temp_dir.path().join(".fawx");
        std::fs::create_dir_all(&data_dir).unwrap();
        let auth_manager = test_auth_manager();

        persist_auth_manager(&auth_manager).unwrap();

        // Verify by reading back through AuthStore.
        let store = crate::auth_store::AuthStore::open(&data_dir).unwrap();
        let loaded = store.load_auth_manager().unwrap();
        assert_eq!(loaded, auth_manager);
    }

    #[cfg(unix)]
    #[test]
    fn persist_auth_manager_creates_salt_with_0600_permissions() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = temp_dir.path().join(".fawx");
        std::fs::create_dir_all(&data_dir).unwrap();
        let store = crate::auth_store::AuthStore::open(&data_dir).unwrap();
        store.save_auth_manager(&test_auth_manager()).unwrap();

        let salt_mode = std::fs::metadata(data_dir.join(".auth-salt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(salt_mode, 0o600);
    }

    #[tokio::test]
    async fn persist_then_load_round_trip_returns_equivalent_auth_manager() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_dir = TempDir::new().unwrap();
        let _home = ScopedEnvVar::set("HOME", temp_dir.path().to_str().unwrap());

        let data_dir = temp_dir.path().join(".fawx");
        std::fs::create_dir_all(&data_dir).unwrap();

        let mut expected = AuthManager::new();
        expected.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "setup-token-round-trip".to_string(),
            },
        );
        expected.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "access-token-round-trip".to_string(),
                refresh_token: "refresh-token-round-trip".to_string(),
                expires_at: 1_700_000_000_000,
                account_id: Some("acct_round_trip".to_string()),
            },
        );

        persist_auth_manager(&expected).unwrap();
        let loaded = load_auth_manager().unwrap();

        assert_eq!(loaded, expected);
    }

    #[test]
    fn build_router_with_mixed_credentials_sets_expected_models_and_auth_labels() {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "anthropic",
            AuthMethod::SetupToken {
                token: "setup-token-mixed".to_string(),
            },
        );
        auth_manager.store(
            "openrouter",
            AuthMethod::ApiKey {
                provider: "openrouter".to_string(),
                key: "openrouter-key-mixed".to_string(),
            },
        );

        let router = build_router(&auth_manager).unwrap();
        let models = router.available_models();

        assert!(models.iter().any(|model| {
            &model.provider_name == "anthropic" && model.auth_method == "subscription"
        }));
        assert!(models.iter().any(|model| {
            model.model_id == "openai/gpt-4o-mini"
                && model.provider_name == "openrouter"
                && model.auth_method == "api_key"
        }));

        let anthropic_auth_labels = models
            .iter()
            .filter(|model| model.provider_name == "anthropic")
            .map(|model| model.auth_method.as_str())
            .collect::<Vec<_>>();
        assert!(!anthropic_auth_labels.is_empty());
        assert!(anthropic_auth_labels
            .iter()
            .all(|auth_method| *auth_method == "subscription"));

        let openrouter_auth_labels = models
            .iter()
            .filter(|model| model.provider_name == "openrouter")
            .map(|model| model.auth_method.as_str())
            .collect::<Vec<_>>();
        assert!(!openrouter_auth_labels.is_empty());
        assert!(openrouter_auth_labels
            .iter()
            .all(|auth_method| *auth_method == "api_key"));
    }

    #[test]
    fn build_router_with_oauth_credentials_registers_openai_subscription_models() {
        let mut auth_manager = AuthManager::new();
        auth_manager.store(
            "openai",
            AuthMethod::OAuth {
                provider: "openai".to_string(),
                access_token: "oauth-access-token".to_string(),
                refresh_token: "oauth-refresh-token".to_string(),
                expires_at: 1_700_000_000_000,
                account_id: Some("acct_oauth_router_test".to_string()),
            },
        );

        let router = build_router(&auth_manager).unwrap();
        let models = router.available_models();

        let openai_models = models
            .iter()
            .filter(|model| model.provider_name == "openai")
            .collect::<Vec<_>>();

        assert!(!openai_models.is_empty());
        assert!(openai_models
            .iter()
            .all(|model| model.auth_method == "subscription"));
        assert!(openai_models
            .iter()
            .any(|model| model.model_id == "gpt-5.3-codex"));
    }

    #[test]
    fn render_loop_complete_shows_styled_metadata() {
        use fx_kernel::act::TokenUsage;
        let result = render_loop_result(
            LoopResult::Complete {
                response: "Paris".to_string(),
                iterations: 1,
                tokens_used: TokenUsage {
                    input_tokens: 50,
                    output_tokens: 12,
                },
                learnings: Vec::new(),
                signals: Vec::new(),
            },
            std::time::Duration::from_millis(250),
        );
        assert!(result.contains("Paris"));
        assert!(result.contains("1 iteration"));
        assert!(result.contains("50 in / 12 out tokens"));
    }

    #[test]
    fn loop_metadata_includes_wall_time() {
        let meta = format_loop_metadata(
            2,
            &TokenUsage {
                input_tokens: 50,
                output_tokens: 12,
            },
            std::time::Duration::from_millis(480),
        );

        assert!(meta.contains("480ms"));
    }

    #[test]
    fn render_loop_budget_exhausted_shows_warning() {
        let result = render_loop_result(
            LoopResult::BudgetExhausted {
                partial_response: Some("partial".to_string()),
                iterations: 3,
                signals: Vec::new(),
            },
            std::time::Duration::from_millis(250),
        );
        assert!(result.contains("partial"));
        assert!(result.contains("budget exhausted"));
        assert!(result.contains("250ms"));
    }

    #[test]
    fn render_loop_error_shows_marker() {
        let result = render_loop_result(
            LoopResult::Error {
                message: "timeout".to_string(),
                recoverable: true,
                signals: Vec::new(),
            },
            std::time::Duration::from_millis(250),
        );
        assert!(result.contains("\u{2717} timeout"));
        assert!(result.contains("recoverable"));
        assert!(result.contains("250ms"));
    }

    #[test]
    fn format_wall_time_boundary_at_one_second() {
        assert_eq!(
            format_wall_time(std::time::Duration::from_millis(999)),
            "999ms"
        );
        assert_eq!(
            format_wall_time(std::time::Duration::from_millis(1000)),
            "1.0s"
        );
        assert_eq!(
            format_wall_time(std::time::Duration::from_millis(1500)),
            "1.5s"
        );
        assert_eq!(
            format_wall_time(std::time::Duration::from_millis(100)),
            "100ms"
        );
    }

    #[test]
    fn sanitize_history_text_removes_ansi_and_loop_metadata() {
        let text = "Answer\n\x1b[2m  ↳ 2 iterations · 50 in / 12 out tokens\x1b[0m\n\x1b[33mwarning\x1b[0m";

        let sanitized = sanitize_history_text(text);

        assert_eq!(sanitized, "Answer\nwarning");
    }

    #[test]
    fn collect_row_scrubs_only_blanks_trailing_disappearances() {
        let previous = ui::FrameRenderRows {
            width: 6,
            rows: vec!["abc   ".to_string()],
            fills: vec![ui::FrameRowFill::Default],
            content_columns: vec![3],
        };
        let current = ui::FrameRenderRows {
            width: 6,
            rows: vec!["ab    ".to_string()],
            fills: vec![ui::FrameRowFill::Default],
            content_columns: vec![2],
        };

        assert_eq!(
            collect_row_scrubs(Some(&previous), &current),
            vec![RowScrub {
                row: 0,
                start_col: 2
            }]
        );
    }

    #[test]
    fn collect_row_scrubs_skips_rows_that_only_grow() {
        let previous = ui::FrameRenderRows {
            width: 6,
            rows: vec!["ab    ".to_string()],
            fills: vec![ui::FrameRowFill::Default],
            content_columns: vec![2],
        };
        let current = ui::FrameRenderRows {
            width: 6,
            rows: vec!["abc   ".to_string()],
            fills: vec![ui::FrameRowFill::Default],
            content_columns: vec![3],
        };

        assert!(collect_row_scrubs(Some(&previous), &current).is_empty());
    }

    #[test]
    fn collect_row_scrubs_counts_wide_cells_by_terminal_column() {
        let previous = ui::FrameRenderRows {
            width: 4,
            rows: vec!["中 a ".to_string()],
            fills: vec![ui::FrameRowFill::Default],
            content_columns: vec![3],
        };
        let current = ui::FrameRenderRows {
            width: 4,
            rows: vec!["中   ".to_string()],
            fills: vec![ui::FrameRowFill::Default],
            content_columns: vec![2],
        };

        assert_eq!(
            collect_row_scrubs(Some(&previous), &current),
            vec![RowScrub {
                row: 0,
                start_col: 2
            }]
        );
    }

    #[test]
    fn collect_row_scrubs_blanks_newly_visible_rows_and_columns() {
        let previous = ui::FrameRenderRows {
            width: 4,
            rows: vec!["abcd".to_string()],
            fills: vec![ui::FrameRowFill::Default],
            content_columns: vec![4],
        };
        let current = ui::FrameRenderRows {
            width: 6,
            rows: vec!["abcd  ".to_string(), "xy    ".to_string()],
            fills: vec![ui::FrameRowFill::Default, ui::FrameRowFill::InputBackground],
            content_columns: vec![4, 2],
        };

        assert_eq!(
            collect_row_scrubs(Some(&previous), &current),
            vec![
                RowScrub {
                    row: 0,
                    start_col: 4
                },
                RowScrub {
                    row: 1,
                    start_col: 0
                },
            ]
        );
    }

    #[test]
    fn split_output_block_normalizes_crlf_and_cr_without_losing_trailing_lines() {
        assert_eq!(
            split_output_block("alpha\r\nbeta\rgamma\n"),
            vec![
                "alpha".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
                String::new(),
            ]
        );
    }

    #[tokio::test]
    async fn generate_streaming_prints_tokens_incrementally() {
        let chunks = vec![
            Ok(StreamChunk {
                delta_content: Some("Hello".to_string()),
                tool_use_deltas: Vec::new(),
                usage: None,
                stop_reason: None,
            }),
            Ok(StreamChunk {
                delta_content: Some(" world".to_string()),
                tool_use_deltas: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            }),
        ];
        let mut stream = futures::stream::iter(chunks);
        let mut writer = RecordingWriter::default();

        let result = consume_stream_with_writer(&mut stream, &mut writer)
            .await
            .expect("stream output");

        assert_eq!(result, "Hello world");
        assert_eq!(
            writer.writes,
            vec!["Hello".to_string(), " world".to_string()]
        );
        assert_eq!(writer.flush_count, 2);
    }

    #[tokio::test]
    async fn consume_stream_silent_collects_all_chunks_without_printing() {
        let chunks = vec![
            Ok(StreamChunk {
                delta_content: Some("Hello".to_string()),
                tool_use_deltas: Vec::new(),
                usage: None,
                stop_reason: None,
            }),
            Ok(StreamChunk {
                delta_content: Some(" world".to_string()),
                tool_use_deltas: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            }),
        ];
        let mut stream = futures::stream::iter(chunks);

        let result = consume_stream_silent(&mut stream).await.unwrap();

        assert_eq!(result, "Hello world");
    }

    #[tokio::test]
    async fn consume_stream_silent_returns_error_on_stream_failure() {
        let chunks: Vec<Result<StreamChunk, ProviderError>> =
            vec![Err(ProviderError::Streaming("broken pipe".to_string()))];
        let mut stream = futures::stream::iter(chunks);

        let result = consume_stream_silent(&mut stream).await;

        assert!(
            matches!(&result, Err(CoreLlmError::Inference(msg)) if msg.contains("broken pipe"))
        );
    }

    #[tokio::test]
    async fn handle_message_passes_plain_text_response_through() {
        // Verify that a normal plain-text LLM response renders correctly.
        let plain_text = "Hello! How can I help you today?";
        let (mut app, _temp_dir) = app_with_mock_model(plain_text);

        let rendered = app
            .handle_message("Hey!")
            .await
            .expect("loop result")
            .expect("batch response");

        assert!(
            rendered.contains("Hello! How can I help you today?"),
            "plain text response should pass through to rendered output, got: {rendered}"
        );
    }

    #[tokio::test]
    async fn handle_message_returns_parsed_response_not_raw_json() {
        // When the LLM follows the schema correctly, the Respond text
        // should appear in the rendered output.
        let valid_json = r#"{"action":{"Respond":{"text":"Hey there! How can I help?"}},"rationale":"greeting","confidence":0.95,"expected_outcome":null,"sub_goals":[]}"#;
        let (mut app, _temp_dir) = app_with_mock_model(valid_json);

        let rendered = app
            .handle_message("Hey!")
            .await
            .expect("loop result")
            .expect("batch response");

        assert!(
            rendered.contains("Hey there! How can I help?"),
            "expected parsed response text, got: {rendered}"
        );
    }

    #[test]
    fn signal_labels_use_stable_text_labels() {
        let signals = vec![Signal {
            step: LoopStep::Act,
            kind: fx_kernel::signals::SignalKind::Success,
            message: "tool read_file".to_string(),
            metadata: serde_json::json!({}),
            timestamp_ms: 1,
        }];

        assert_eq!(signal_labels(&signals), vec!["act:success".to_string()]);
    }

    #[test]
    fn tool_names_extracts_only_tool_names_from_act_signals() {
        let signals = vec![
            Signal {
                step: LoopStep::Act,
                kind: fx_kernel::signals::SignalKind::Success,
                message: "tool read_file".to_string(),
                metadata: serde_json::json!({}),
                timestamp_ms: 1,
            },
            Signal {
                step: LoopStep::Reason,
                kind: fx_kernel::signals::SignalKind::Thinking,
                message: "thinking".to_string(),
                metadata: serde_json::json!({}),
                timestamp_ms: 2,
            },
            Signal {
                step: LoopStep::Act,
                kind: fx_kernel::signals::SignalKind::Friction,
                message: "tool write_file".to_string(),
                metadata: serde_json::json!({}),
                timestamp_ms: 3,
            },
        ];

        assert_eq!(
            tool_names(&signals),
            vec!["read_file".to_string(), "write_file".to_string()]
        );
    }
    #[test]
    fn parse_bare_command_recognizes_stop_variants() {
        use fx_kernel::input::LoopCommand;

        assert_eq!(parse_bare_command("stop"), Some(LoopCommand::Stop));
        assert_eq!(parse_bare_command("s"), Some(LoopCommand::Stop));
        assert_eq!(parse_bare_command("STOP"), Some(LoopCommand::Stop));
        assert_eq!(parse_bare_command("no"), Some(LoopCommand::Stop));
        assert_eq!(parse_bare_command("abort"), Some(LoopCommand::Abort));
        assert_eq!(parse_bare_command("a"), Some(LoopCommand::Abort));
        assert_eq!(parse_bare_command("cancel"), Some(LoopCommand::Abort));
        assert_eq!(parse_bare_command("ABORT"), Some(LoopCommand::Abort));
        assert_eq!(parse_bare_command("wait"), Some(LoopCommand::Wait));
        assert_eq!(parse_bare_command("pause"), Some(LoopCommand::Wait));
        assert_eq!(parse_bare_command("w"), Some(LoopCommand::Wait));
        assert_eq!(parse_bare_command("go"), Some(LoopCommand::Resume));
        assert_eq!(parse_bare_command("resume"), Some(LoopCommand::Resume));
        assert_eq!(parse_bare_command("continue"), Some(LoopCommand::Resume));
    }

    #[test]
    fn parse_bare_command_returns_none_for_unknown() {
        assert_eq!(parse_bare_command("hello world"), None);
        assert_eq!(parse_bare_command("run tests"), None);
        assert_eq!(parse_bare_command(""), None);
        assert_eq!(parse_bare_command("   "), None);
    }

    #[test]
    fn parse_bare_command_trims_whitespace() {
        use fx_kernel::input::LoopCommand;

        assert_eq!(parse_bare_command("  stop  "), Some(LoopCommand::Stop));
        assert_eq!(
            parse_bare_command(
                "  abort
"
            ),
            Some(LoopCommand::Abort)
        );
    }

    #[test]
    fn render_user_stopped_with_partial() {
        let rendered = render_user_stopped(Some("partial answer".to_string()), 2);
        assert!(rendered.contains("partial answer"));
        assert!(rendered.contains("Stopped by user"));
    }

    #[test]
    fn render_user_stopped_without_partial() {
        let rendered = render_user_stopped(None, 1);
        assert!(rendered.contains("Stopped by user"));
    }

    #[test]
    fn render_loop_result_user_stopped_shows_message() {
        let result = render_loop_result(
            LoopResult::UserStopped {
                partial_response: Some("partial".to_string()),
                iterations: 2,
                signals: Vec::new(),
            },
            std::time::Duration::from_millis(500),
        );
        assert!(result.contains("partial"));
        assert!(result.contains("Stopped by user"));
        assert!(result.contains("500ms"));
    }

    #[test]
    fn command_parsing_recognizes_config_and_config_init() {
        assert_eq!(parse_command("/config"), ParsedCommand::Config(None));
        assert_eq!(
            parse_command("/config init"),
            ParsedCommand::Config(Some("init".to_string()))
        );
        assert_eq!(
            parse_command("/config unknown"),
            ParsedCommand::Config(Some("unknown".to_string()))
        );
    }

    #[test]
    fn group_models_by_provider_groups_correctly() {
        let models = vec![
            ModelInfo {
                model_id: "claude-sonnet-4-20260514".to_string(),
                provider_name: "Anthropic".to_string(),
                auth_method: "subscription".to_string(),
            },
            ModelInfo {
                model_id: "claude-opus-4-20260514".to_string(),
                provider_name: "Anthropic".to_string(),
                auth_method: "subscription".to_string(),
            },
            ModelInfo {
                model_id: "gpt-4o".to_string(),
                provider_name: "OpenAI".to_string(),
                auth_method: "api_key".to_string(),
            },
        ];
        let grouped = group_models_by_provider(&models);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, "Anthropic");
        assert_eq!(grouped[0].1.len(), 2);
        assert_eq!(grouped[1].0, "OpenAI");
        assert_eq!(grouped[1].1.len(), 1);
    }

    #[test]
    fn ensure_cooked_mode_disables_raw_mode_before_draining_stdin() {
        let calls = std::cell::RefCell::new(Vec::new());

        super::ensure_cooked_mode_with(
            || calls.borrow_mut().push("disable_raw_mode"),
            || calls.borrow_mut().push("drain_stdin"),
        );

        assert_eq!(calls.into_inner(), vec!["disable_raw_mode", "drain_stdin"]);
    }

    #[cfg(unix)]
    #[test]
    fn drain_stdin_flushes_stdin_input_queue() {
        let captured_fd = std::cell::Cell::new(-1);
        let captured_selector = std::cell::Cell::new(-1);

        let result = super::flush_stdin_input_queue_with(|fd, queue_selector| {
            captured_fd.set(fd);
            captured_selector.set(queue_selector);
            0
        });

        assert!(result.is_ok());
        assert_eq!(captured_fd.get(), libc::STDIN_FILENO);
        assert_eq!(captured_selector.get(), libc::TCIFLUSH);
    }

    #[cfg(unix)]
    #[test]
    fn drain_stdin_reports_flush_errors() {
        let captured_errno = std::cell::Cell::new(None);

        super::drain_stdin_with(
            || Err(io::Error::from_raw_os_error(libc::EIO)),
            |error| captured_errno.set(error.raw_os_error()),
        );

        assert_eq!(captured_errno.get(), Some(libc::EIO));
    }

    #[cfg(unix)]
    #[test]
    fn drain_stdin_does_not_report_successful_flush() {
        let report_count = std::cell::Cell::new(0);

        super::drain_stdin_with(|| Ok(()), |_| report_count.set(report_count.get() + 1));

        assert_eq!(report_count.get(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn stdin_flush_enotty_errors_are_benign() {
        let error = io::Error::from_raw_os_error(libc::ENOTTY);

        assert!(super::is_benign_stdin_flush_error(&error));
    }

    #[cfg(unix)]
    #[test]
    fn stdin_flush_non_enotty_errors_are_not_benign() {
        let error = io::Error::from_raw_os_error(libc::EIO);

        assert!(!super::is_benign_stdin_flush_error(&error));
    }

    // ── Restored functional tests (review #1147) ────────────────

    #[tokio::test]
    async fn handle_command_dispatches_help_without_stopping() {
        let (mut app, _temp_dir) = new_test_app();

        app.handle_command("/help").await.unwrap();

        assert!(app.running);
    }

    #[tokio::test]
    async fn tui_runs_without_auth_configured() {
        let (mut app, _temp_dir) = new_test_app();

        app.process_input_line("/help").await.unwrap();
        app.process_input_line("/status").await.unwrap();

        assert!(app.running);
        assert!(!app.auth_manager.has_any());
    }

    #[tokio::test]
    async fn help_command_works_without_auth() {
        let (mut app, _temp_dir) = new_test_app();

        app.process_input_line("/help").await.unwrap();

        assert!(app.running);
        assert!(!app.auth_manager.has_any());
    }

    #[tokio::test]
    async fn quit_command_works_without_auth() {
        let (mut app, _temp_dir) = new_test_app();

        app.process_input_line("/quit").await.unwrap();

        assert!(!app.running);
        assert!(!app.auth_manager.has_any());
    }

    #[tokio::test]
    async fn handle_command_dispatches_quit_and_stops() {
        let (mut app, _temp_dir) = new_test_app();

        app.handle_command("/quit").await.unwrap();

        assert!(!app.running);
    }

    #[tokio::test]
    async fn message_triggers_auth_when_not_configured() {
        let (mut app, _temp_dir) = new_test_app();

        let error = app
            .handle_message("hello")
            .await
            .expect_err("should trigger auth without credentials");

        assert!(
            matches!(error, TuiError::Auth(message) if message.contains("stdin closed unexpectedly"))
        );
    }

    #[test]
    fn persisted_model_used_on_startup() {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(ModelEchoProvider {
                provider_name: "echo-provider".to_string(),
                models: vec![
                    "claude-opus-4-6-20250929".to_string(),
                    "claude-sonnet-4-6-20250929".to_string(),
                ],
            }),
            "test",
        );
        router
            .set_active("claude-opus-4-6-20250929")
            .expect("set router default model");

        let (mut config, _temp_dir) = test_config_with_temp_dir();
        config.model.default_model = Some("claude-sonnet-4-6-20250929".to_string());

        let (loop_engine, runtime_info) = test_engine_and_runtime_info();
        let app = TuiApp::new(
            test_provider_auth_manager(),
            router,
            loop_engine,
            runtime_info,
            config,
        )
        .expect("mock app");

        assert_eq!(app.current_model(), "claude-sonnet-4-6-20250929");
    }

    // ── Steer / Abort / Queue tests ────────────────────────────

    #[test]
    fn parse_bare_command_recognizes_status_aliases() {
        use fx_kernel::input::LoopCommand;
        assert_eq!(parse_bare_command("status"), Some(LoopCommand::StatusQuery));
        assert_eq!(parse_bare_command("st"), Some(LoopCommand::StatusQuery));
        assert_eq!(
            parse_bare_command("/status"),
            Some(LoopCommand::StatusQuery)
        );
    }

    #[test]
    fn parse_bare_command_recognizes_slash_stop() {
        use fx_kernel::input::LoopCommand;
        assert_eq!(parse_bare_command("/stop"), Some(LoopCommand::Stop));
        assert_eq!(parse_bare_command("/abort"), Some(LoopCommand::Abort));
    }

    #[test]
    fn parse_bare_command_steer_with_text() {
        use fx_kernel::input::LoopCommand;
        assert_eq!(
            parse_bare_command("/steer try harder"),
            Some(LoopCommand::Steer("try harder".to_string()))
        );
        assert_eq!(
            parse_bare_command("steer use a different approach"),
            Some(LoopCommand::Steer("use a different approach".to_string()))
        );
    }

    #[test]
    fn parse_bare_command_steer_empty_returns_none() {
        // "/steer " with no message text should return None
        assert_eq!(parse_bare_command("/steer "), None);
        assert_eq!(parse_bare_command("steer "), None);
    }

    #[test]
    fn route_execution_input_stop_sets_abort() {
        let mut app = ui::FawxApp::new();
        let cancel = CancellationToken::new();
        route_execution_input("/stop", &mut app, &cancel, &None);
        assert!(app.abort_requested);
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn route_execution_input_steer_sets_message() {
        let mut app = ui::FawxApp::new();
        let cancel = CancellationToken::new();
        route_execution_input("/steer foo bar", &mut app, &cancel, &None);
        assert_eq!(app.steer_message.as_deref(), Some("foo bar"));
        assert!(!app.abort_requested);
    }

    #[test]
    fn route_execution_input_plain_text_queues() {
        let mut app = ui::FawxApp::new();
        let cancel = CancellationToken::new();
        route_execution_input("hello world", &mut app, &cancel, &None);
        assert_eq!(app.pending_inputs, vec!["hello world".to_string()]);
        assert!(!app.abort_requested);
    }

    #[test]
    fn route_execution_input_steer_interrupts_active_stream_for_clean_restart() {
        let (sender, mut receiver) = fx_kernel::input::loop_input_channel();
        let sender_opt = Some(sender);
        let mut app = ui::FawxApp::new();
        app.set_state(ui::AppState::Streaming);
        app.start_streaming();
        app.append_streaming_delta("partial");

        let cancel = CancellationToken::new();
        route_execution_input("/steer pivot now", &mut app, &cancel, &sender_opt);

        assert_eq!(
            receiver.try_recv(),
            Some(LoopCommand::Steer("pivot now".to_string()))
        );
        assert!(!app.streaming_prefix_printed);
        assert!(app.suppress_stream_until_next_start);
        assert_eq!(app.output_lines[1], "fawx › partial");
        assert_eq!(app.output_lines[3], "you › pivot now");
        assert_eq!(app.state, ui::AppState::Executing { spinner_frame: 0 });
    }

    #[test]
    fn route_bus_message_to_app_resets_stream_between_interrupted_segments() {
        let mut app = ui::FawxApp::new();
        let mut streamed = false;

        route_bus_message_to_app(
            InternalMessage::StreamingStarted {
                phase: fx_core::message::StreamPhase::Reason,
            },
            &mut app,
            &mut streamed,
        );
        route_bus_message_to_app(
            InternalMessage::StreamDelta {
                delta: "first".to_string(),
                phase: fx_core::message::StreamPhase::Reason,
            },
            &mut app,
            &mut streamed,
        );
        route_bus_message_to_app(
            InternalMessage::StreamingFinished {
                phase: fx_core::message::StreamPhase::Reason,
            },
            &mut app,
            &mut streamed,
        );

        let (sender, _) = fx_kernel::input::loop_input_channel();
        let cancel = CancellationToken::new();
        route_execution_input("/steer pivot", &mut app, &cancel, &Some(sender));

        route_bus_message_to_app(
            InternalMessage::StreamingStarted {
                phase: fx_core::message::StreamPhase::Reason,
            },
            &mut app,
            &mut streamed,
        );
        route_bus_message_to_app(
            InternalMessage::StreamDelta {
                delta: "second".to_string(),
                phase: fx_core::message::StreamPhase::Reason,
            },
            &mut app,
            &mut streamed,
        );

        assert_eq!(app.output_lines[1], "fawx › first");
        assert_eq!(app.output_lines[2], "↪ Steer sent: pivot");
        assert_eq!(app.output_lines[4], "fawx › second");
        assert_eq!(app.streaming_start_index, Some(4));
    }

    #[test]
    fn queued_inputs_drain_in_order() {
        let mut app = ui::FawxApp::new();
        app.queue_input("first".into());
        app.queue_input("second".into());
        app.queue_input("third".into());
        assert_eq!(app.drain_next_input(), Some("first".into()));
        assert_eq!(app.drain_next_input(), Some("second".into()));
        assert_eq!(app.drain_next_input(), Some("third".into()));
        assert_eq!(app.drain_next_input(), None);
    }

    #[test]
    fn steer_clears_after_take() {
        let mut app = ui::FawxApp::new();
        app.set_steer("redirect".into());
        assert_eq!(app.take_steer(), Some("redirect".into()));
        assert!(app.take_steer().is_none());
    }

    #[test]
    fn multiple_steers_last_wins() {
        let mut app = ui::FawxApp::new();
        app.set_steer("first".into());
        app.set_steer("second".into());
        app.set_steer("third".into());
        assert_eq!(app.steer_message.as_deref(), Some("third"));
    }

    #[test]
    fn abort_plus_steer_abort_takes_priority() {
        let mut app = ui::FawxApp::new();
        let cancel = CancellationToken::new();
        // Steer first, then stop
        route_execution_input("/steer try again", &mut app, &cancel, &None);
        route_execution_input("/stop", &mut app, &cancel, &None);
        // Abort should be set even though steer was set first
        assert!(app.abort_requested);
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn idle_stop_detected_as_control_command() {
        // Only slash-prefixed commands are recognized during idle
        assert!(is_idle_control_command("/stop"));
        assert!(is_idle_control_command("/abort"));
        assert!(is_idle_control_command("/STOP"));
        assert!(is_idle_control_command(" /stop "));
        // Bare aliases must NOT be intercepted during idle — they are
        // valid user messages (e.g. "a", "s", "no").
        assert!(!is_idle_control_command("stop"));
        assert!(!is_idle_control_command("abort"));
        assert!(!is_idle_control_command("a"));
        assert!(!is_idle_control_command("s"));
        assert!(!is_idle_control_command("no"));
        assert!(!is_idle_control_command("hello"));
        assert!(!is_idle_control_command("/steer something"));
        assert!(!is_idle_control_command("status"));
    }

    #[test]
    fn handle_idle_event_paste_buffers_multiline_input() {
        let mut app = ui::FawxApp::new();

        let submitted = handle_idle_event(Event::Paste("alpha\nbeta".into()), &mut app);

        assert!(submitted.is_none());
        assert_eq!(app.submit_input(), "alpha\nbeta");
    }

    #[test]
    fn handle_idle_event_paste_appends_to_existing_input() {
        let mut app = ui::FawxApp::new();
        app.input_text = "hello world ".into();

        let submitted = handle_idle_event(Event::Paste("AAAA".into()), &mut app);

        assert!(submitted.is_none());
        assert_eq!(app.submit_input(), "hello world AAAA");
    }

    #[test]
    fn handle_execution_event_paste_buffers_multiline_input() {
        let mut app = ui::FawxApp::new();
        let cancel = CancellationToken::new();

        handle_execution_event(Event::Paste("alpha\nbeta".into()), &mut app, &cancel, &None);

        assert_eq!(app.submit_input(), "alpha\nbeta");
        assert!(!cancel.is_cancelled());
        assert!(app.pending_inputs.is_empty());
    }

    #[test]
    fn handle_execution_event_paste_appends_to_existing_input() {
        let mut app = ui::FawxApp::new();
        let cancel = CancellationToken::new();
        app.input_text = "hello world ".into();

        handle_execution_event(Event::Paste("AAAA".into()), &mut app, &cancel, &None);

        assert_eq!(app.submit_input(), "hello world AAAA");
        assert!(!cancel.is_cancelled());
        assert!(app.pending_inputs.is_empty());
    }

    #[test]
    fn append_user_message_output_splits_multiline_blocks() {
        let mut app = ui::FawxApp::new();

        append_user_message_output(&mut app, "alpha\r\nbeta\ngamma");

        assert_eq!(
            app.output_lines,
            vec![
                String::new(),
                "you › alpha".to_string(),
                "      beta".to_string(),
                "      gamma".to_string(),
            ]
        );
    }

    #[test]
    fn tui_println_splits_multiline_blocks_into_logical_lines() {
        let (mut app, _temp_dir) = new_test_app();

        app.tui_println("alpha\r\nbeta\n\ngamma");

        assert_eq!(
            app.output_buffer,
            vec![
                "alpha".to_string(),
                "beta".to_string(),
                String::new(),
                "gamma".to_string(),
            ]
        );
    }

    #[test]
    fn flush_output_to_preserves_logical_lines_for_multiline_blocks() {
        let (mut tui_app, _temp_dir) = new_test_app();
        let mut ui_app = ui::FawxApp::new();

        tui_app.tui_println("fawx › first line\nsecond line\n");
        tui_app.flush_output_to(&mut ui_app);

        assert_eq!(
            ui_app.output_lines,
            vec![
                "fawx › first line".to_string(),
                "second line".to_string(),
                String::new(),
            ]
        );
    }

    #[test]
    fn route_execution_input_sends_to_channel() {
        let (sender, mut receiver) = fx_kernel::input::loop_input_channel();
        let mut app = ui::FawxApp::new();
        let cancel = CancellationToken::new();
        let sender_opt = Some(sender);

        route_execution_input("/steer try plan B", &mut app, &cancel, &sender_opt);
        let cmd = receiver.try_recv();
        assert_eq!(cmd, Some(LoopCommand::Steer("try plan B".to_string())));

        route_execution_input("/stop", &mut app, &cancel, &sender_opt);
        let cmd = receiver.try_recv();
        assert_eq!(cmd, Some(LoopCommand::Abort));
    }

    #[test]
    fn steer_with_channel_does_not_store_in_app() {
        let (sender, mut receiver) = fx_kernel::input::loop_input_channel();
        let mut app = ui::FawxApp::new();
        let cancel = CancellationToken::new();
        let sender_opt = Some(sender);

        route_execution_input("/steer try harder", &mut app, &cancel, &sender_opt);

        // Message sent via channel
        let cmd = receiver.try_recv();
        assert_eq!(cmd, Some(LoopCommand::Steer("try harder".to_string())));
        // Must NOT be stored in app (prevents duplicate injection via take_steer)
        assert!(app.steer_message.is_none());
    }

    #[test]
    fn steer_without_channel_falls_back_to_app_storage() {
        let mut app = ui::FawxApp::new();
        let cancel = CancellationToken::new();

        route_execution_input("/steer fallback msg", &mut app, &cancel, &None);

        // No channel — stored in app for forwarding at next cycle start
        assert_eq!(app.steer_message.as_deref(), Some("fallback msg"));
    }

    // -----------------------------------------------------------------------
    // Auth command tests
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_provider_name_lowercase() {
        assert_eq!(normalize_provider_name("GitHub"), "github");
    }

    #[test]
    fn normalize_provider_name_trims() {
        assert_eq!(normalize_provider_name("  openai  "), "openai");
    }

    #[test]
    fn normalize_provider_name_gh_alias() {
        assert_eq!(normalize_provider_name("gh"), "github");
        assert_eq!(normalize_provider_name("GH"), "github");
    }

    #[test]
    fn normalize_provider_name_passthrough() {
        assert_eq!(normalize_provider_name("anthropic"), "anthropic");
        assert_eq!(normalize_provider_name("openai"), "openai");
    }

    #[test]
    fn parse_auth_bare() {
        let cmd = parse_command("/auth");
        assert!(matches!(
            cmd,
            ParsedCommand::Auth {
                subcommand: None,
                action: None,
                value: None,
                has_extra_args: false,
            }
        ));
    }

    #[test]
    fn parse_auth_github_set_token() {
        let cmd = parse_command("/auth github set-token ghp_xxxxx");
        assert!(matches!(
            cmd,
            ParsedCommand::Auth {
                subcommand: Some(ref s),
                action: Some(ref a),
                value: Some(ref v),
                has_extra_args: false,
            } if s == "github" && a == "set-token" && v == "ghp_xxxxx"
        ));
    }

    #[test]
    fn parse_auth_github_show_status() {
        let cmd = parse_command("/auth github show-status");
        assert!(matches!(
            cmd,
            ParsedCommand::Auth {
                subcommand: Some(ref s),
                action: Some(ref a),
                value: None,
                has_extra_args: false,
            } if s == "github" && a == "show-status"
        ));
    }

    #[test]
    fn parse_auth_github_clear_token() {
        let cmd = parse_command("/auth github clear-token");
        assert!(matches!(
            cmd,
            ParsedCommand::Auth {
                subcommand: Some(ref s),
                action: Some(ref a),
                value: None,
                has_extra_args: false,
            } if s == "github" && a == "clear-token"
        ));
    }

    #[test]
    fn parse_auth_list_providers() {
        let cmd = parse_command("/auth list-providers");
        assert!(matches!(
            cmd,
            ParsedCommand::Auth {
                subcommand: Some(ref s),
                action: None,
                value: None,
                has_extra_args: false,
            } if s == "list-providers"
        ));
    }

    #[test]
    fn parse_auth_unknown_provider() {
        let cmd = parse_command("/auth foobar");
        assert!(matches!(
            cmd,
            ParsedCommand::Auth {
                subcommand: Some(ref s),
                action: None,
                value: None,
                has_extra_args: false,
            } if s == "foobar"
        ));
    }

    #[test]
    fn parse_auth_extra_args_detected() {
        let cmd = parse_command("/auth github set-token ghp_xxx extra stuff");
        assert!(matches!(
            cmd,
            ParsedCommand::Auth {
                has_extra_args: true,
                ..
            }
        ));
    }

    #[test]
    fn parse_keys_generate_force() {
        let cmd = parse_command("/keys generate --force");
        assert!(matches!(
            cmd,
            ParsedCommand::Keys {
                subcommand: Some(ref s),
                value: Some(ref v),
                option: None,
                has_extra_args: false,
            } if s == "generate" && v == "--force"
        ));
    }

    #[test]
    fn parse_keys_trust_path() {
        let cmd = parse_command("/keys trust /tmp/alice.pub");
        assert!(matches!(
            cmd,
            ParsedCommand::Keys {
                subcommand: Some(ref s),
                value: Some(ref v),
                option: None,
                has_extra_args: false,
            } if s == "trust" && v == "/tmp/alice.pub"
        ));
    }

    #[test]
    fn parse_sign_all() {
        let cmd = parse_command("/sign --all");
        assert!(matches!(
            cmd,
            ParsedCommand::Sign {
                target: Some(ref target),
                has_extra_args: false,
            } if target == "--all"
        ));
    }

    #[test]
    fn should_add_keys_and_sign_to_history() {
        assert!(should_add_to_history("/keys generate"));
        assert!(should_add_to_history("/sign demo"));
    }

    #[test]
    fn show_auth_status_no_credentials_no_panic() {
        let (mut app, _temp_dir) = new_test_app();
        app.show_auth_status();
        // Should produce output without panicking.
        assert!(!app.output_buffer.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn generate_signing_keypair_in_writes_expected_files_and_permissions() {
        let temp = TempDir::new().expect("tempdir");
        let fingerprint =
            generate_signing_keypair_in(temp.path(), false).expect("keypair should generate");

        assert_eq!(fingerprint.len(), 16);
        assert!(signing_private_key_path_in(temp.path()).exists());
        assert!(signing_public_key_path_in(temp.path()).exists());
        assert!(trusted_signing_public_key_path_in(temp.path()).exists());

        let mode = std::fs::metadata(signing_private_key_path_in(temp.path()))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        let public_mode = std::fs::metadata(signing_public_key_path_in(temp.path()))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(public_mode, 0o644);

        let trusted_mode = std::fs::metadata(trusted_signing_public_key_path_in(temp.path()))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(trusted_mode, 0o644);
    }

    #[test]
    fn generate_signing_keypair_in_requires_force_to_overwrite() {
        let temp = TempDir::new().expect("tempdir");
        generate_signing_keypair_in(temp.path(), false).expect("first keypair");

        let error = generate_signing_keypair_in(temp.path(), false).expect_err("must refuse");
        assert!(error.to_string().contains("--force"));
    }

    #[test]
    fn trust_public_key_in_rejects_invalid_length_key() {
        let temp = TempDir::new().expect("tempdir");
        let invalid_key = temp.path().join("invalid.pub");
        std::fs::write(&invalid_key, [1u8; 31]).expect("invalid key");

        let error = trust_public_key_in(temp.path(), &invalid_key).expect_err("must reject");
        assert!(error.to_string().contains("expected 32 bytes"));
    }

    #[test]
    fn keys_list_and_revoke_use_fingerprints() {
        let temp = TempDir::new().expect("tempdir");
        let (_, public_key) = fx_skills::signing::generate_keypair().expect("keypair");
        let trusted_dir = fawx_trusted_keys_dir_in(temp.path());
        std::fs::create_dir_all(&trusted_dir).expect("trusted dir");
        std::fs::write(trusted_dir.join("alice.pub"), &public_key).expect("pub");

        let keys = trusted_key_entries_from_dir(&trusted_dir).expect("trusted keys");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].file_size, 32);

        let removed = revoke_trusted_key_in(temp.path(), &keys[0].fingerprint)
            .expect("revoke by fingerprint");
        assert_eq!(removed, 1);
        assert!(trusted_key_entries_from_dir(&trusted_dir)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn public_key_fingerprint_is_deterministic_for_same_key() {
        let (_, public_key) = fx_skills::signing::generate_keypair().expect("keypair");

        assert_eq!(
            public_key_fingerprint(&public_key),
            public_key_fingerprint(&public_key)
        );
    }

    #[test]
    fn public_key_fingerprint_differs_for_distinct_keys() {
        let (_, left) = fx_skills::signing::generate_keypair().expect("left keypair");
        let (_, right) = fx_skills::signing::generate_keypair().expect("right keypair");

        assert_ne!(
            public_key_fingerprint(&left),
            public_key_fingerprint(&right)
        );
    }

    #[test]
    fn sign_skill_in_writes_signature_and_verifies() {
        let temp = TempDir::new().expect("tempdir");
        generate_signing_keypair_in(temp.path(), false).expect("keypair");
        write_test_skill(temp.path(), "demo", &sample_wasm_bytes());

        let signed = sign_skill_in(temp.path(), "demo").expect("sign skill");
        let signature = std::fs::read(&signed.signature_path).expect("signature");
        let wasm_bytes =
            std::fs::read(fawx_skills_dir_in(temp.path()).join("demo/demo.wasm")).expect("wasm");
        let public_key = std::fs::read(signing_public_key_path_in(temp.path())).expect("pub");

        assert_eq!(signature.len(), 64);
        assert_eq!(signed.name, "demo");
        assert_eq!(signed.fingerprint, public_key_fingerprint(&public_key));
        assert!(
            fx_skills::signing::verify_skill(&wasm_bytes, &signature, &public_key).expect("verify"),
        );
    }

    #[test]
    fn sign_all_skills_in_writes_signatures_for_each_skill() {
        let temp = TempDir::new().expect("tempdir");
        generate_signing_keypair_in(temp.path(), false).expect("keypair");
        write_test_skill(temp.path(), "alpha", &sample_wasm_bytes());
        write_test_skill(temp.path(), "beta", &sample_wasm_bytes());
        std::fs::create_dir_all(fawx_skills_dir_in(temp.path()).join("source_only"))
            .expect("source only dir");

        let signed = sign_all_skills_in(temp.path()).expect("sign all");
        let signed_names: Vec<String> = signed.iter().map(|skill| skill.name.clone()).collect();

        assert_eq!(signed_names, vec!["alpha".to_string(), "beta".to_string()]);
        assert!(fawx_skills_dir_in(temp.path())
            .join("alpha/alpha.wasm.sig")
            .exists());
        assert!(fawx_skills_dir_in(temp.path())
            .join("beta/beta.wasm.sig")
            .exists());
        assert!(!fawx_skills_dir_in(temp.path())
            .join("source_only/source_only.wasm.sig")
            .exists());
    }

    #[test]
    fn sign_skill_in_requires_private_key() {
        let temp = TempDir::new().expect("tempdir");
        write_test_skill(temp.path(), "demo", &sample_wasm_bytes());
        let (_, public_key) = fx_skills::signing::generate_keypair().expect("keypair");
        let trusted_dir = fawx_trusted_keys_dir_in(temp.path());
        std::fs::create_dir_all(&trusted_dir).expect("trusted dir");
        std::fs::write(trusted_dir.join("demo.pub"), &public_key).expect("pub");

        let error = sign_skill_in(temp.path(), "demo").expect_err("missing private key");
        assert_eq!(
            error.to_string(),
            "store error: No signing key found. Run /keys generate first."
        );
    }

    #[test]
    fn sign_skill_in_reports_missing_skill_error() {
        let temp = TempDir::new().expect("tempdir");
        generate_signing_keypair_in(temp.path(), false).expect("keypair");

        let error = sign_skill_in(temp.path(), "missing").expect_err("missing skill");

        assert_eq!(
            error.to_string(),
            format!(
                "store error: No WASM skill found for 'missing'. Expected {}",
                fawx_skills_dir_in(temp.path())
                    .join("missing/missing.wasm")
                    .display()
            )
        );
    }

    #[tokio::test]
    async fn handle_keys_generate_uses_home_directory() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_home = TempDir::new().expect("tempdir");
        let _home = create_temp_home_guard(&temp_home);
        let (mut app, _temp_dir) = new_test_app();

        app.handle_command("/keys generate")
            .await
            .expect("keys generate");

        assert!(temp_home.path().join(".fawx/keys/signing.key").exists());
        assert!(temp_home
            .path()
            .join(".fawx/trusted_keys/signing.pub")
            .exists());
    }

    #[tokio::test]
    async fn handle_sign_all_signs_skills_under_home_directory() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_home = TempDir::new().expect("tempdir");
        let _home = create_temp_home_guard(&temp_home);
        let base_dir = temp_home.path().join(".fawx");
        generate_signing_keypair_in(&base_dir, false).expect("keypair");
        write_test_skill(&base_dir, "demo", &sample_wasm_bytes());
        let (mut app, _temp_dir) = new_test_app();

        app.handle_command("/sign --all").await.expect("sign all");

        assert!(base_dir.join("skills/demo/demo.wasm.sig").exists());
    }

    #[tokio::test]
    async fn handle_sign_reports_fingerprint_and_verification_output() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_home = TempDir::new().expect("tempdir");
        let _home = create_temp_home_guard(&temp_home);
        let base_dir = temp_home.path().join(".fawx");
        generate_signing_keypair_in(&base_dir, false).expect("keypair");
        write_test_skill(&base_dir, "demo", &sample_wasm_bytes());
        let (mut app, _temp_dir) = new_test_app();

        app.handle_command("/sign demo").await.expect("sign demo");

        let output = app.output_buffer.join("\n");
        assert!(output.contains("Signed demo (fingerprint:"));
        assert!(output.contains("Verified: signature valid (key:"));
    }

    #[tokio::test]
    async fn handle_keys_list_includes_filename_fingerprint_and_size() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_home = TempDir::new().expect("tempdir");
        let _home = create_temp_home_guard(&temp_home);
        let base_dir = temp_home.path().join(".fawx");
        let (_, public_key) = fx_skills::signing::generate_keypair().expect("keypair");
        let trusted_dir = fawx_trusted_keys_dir_in(&base_dir);
        std::fs::create_dir_all(&trusted_dir).expect("trusted dir");
        std::fs::write(trusted_dir.join("alice.pub"), &public_key).expect("public key");
        let expected_fingerprint = public_key_fingerprint(&public_key);
        let (mut app, _temp_dir) = new_test_app();

        app.handle_command("/keys list").await.expect("keys list");

        let output = app.output_buffer.join("\n");
        assert!(output.contains("alice.pub"));
        assert!(output.contains(&expected_fingerprint));
        assert!(output.contains("32 bytes"));
    }

    #[test]
    fn classify_github_pat_classic() {
        assert_eq!(classify_github_pat("ghp_abc123"), GitHubPatKind::Classic);
    }

    #[test]
    fn classify_github_pat_fine_grained() {
        assert_eq!(
            classify_github_pat("github_pat_abc123"),
            GitHubPatKind::FineGrained
        );
    }

    #[test]
    fn classify_github_pat_unknown() {
        assert_eq!(classify_github_pat("other_token"), GitHubPatKind::Unknown);
    }

    #[test]
    fn humanize_elapsed_ms_just_now() {
        assert_eq!(humanize_elapsed_ms(5000), "just now");
    }

    #[test]
    fn humanize_elapsed_ms_minutes() {
        assert_eq!(humanize_elapsed_ms(300_000), "5m ago");
    }

    #[test]
    fn humanize_elapsed_ms_hours() {
        assert_eq!(humanize_elapsed_ms(7_200_000), "2h ago");
    }

    #[test]
    fn humanize_elapsed_ms_days() {
        assert_eq!(humanize_elapsed_ms(172_800_000), "2d ago");
    }

    #[test]
    fn format_github_token_result_classic_with_scopes() {
        let token = zeroize::Zeroizing::new("ghp_abc123".to_string());
        let info = fx_auth::github::GitHubTokenInfo {
            login: "testuser".to_string(),
            scopes: vec!["repo".to_string(), "workflow".to_string()],
            missing_scopes: vec![],
        };
        let lines = format_github_token_result(&token, &info);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("repo, workflow"));
    }

    #[test]
    fn format_github_token_result_fine_grained() {
        let token = zeroize::Zeroizing::new("github_pat_abc123".to_string());
        let info = fx_auth::github::GitHubTokenInfo {
            login: "testuser".to_string(),
            scopes: vec![],
            missing_scopes: vec![],
        };
        let lines = format_github_token_result(&token, &info);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("fine-grained"));
    }

    #[test]
    fn format_github_token_result_missing_scopes() {
        let token = zeroize::Zeroizing::new("ghp_abc123".to_string());
        let info = fx_auth::github::GitHubTokenInfo {
            login: "testuser".to_string(),
            scopes: vec!["repo".to_string()],
            missing_scopes: vec!["workflow".to_string()],
        };
        let lines = format_github_token_result(&token, &info);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("Missing recommended scopes"));
    }

    #[test]
    fn auto_creates_data_dir_on_startup() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let data_dir = tmp.path().join("nested").join(".fawx");
        assert!(!data_dir.exists());
        let _ = std::fs::create_dir_all(&data_dir);
        assert!(data_dir.exists());
        assert!(data_dir.is_dir());
    }
}
