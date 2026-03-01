use crate::auth_store::{migrate_if_needed, AuthStore};
use crate::config::FawxConfig;
use crate::conversation_store::{
    ConversationMessage, ConversationStore, TokenUsage as ConversationTokenUsage,
};
use crate::git_skill::GitSkill;
use crate::json_memory::{JsonFileMemory, JsonMemoryConfig};
use crate::signal_store::SignalStore;
use crate::skill_bridge::BuiltinToolsSkill;
use crate::tools::{FawxToolExecutor, ToolConfig};
use async_trait::async_trait;
use crossterm::style::Stylize;
use crossterm::{cursor, event, style, terminal, ExecutableCommand};
use futures::StreamExt;
use fx_core::error::LlmError as CoreLlmError;
use fx_core::memory::MemoryProvider;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_kernel::act::TokenUsage;
use fx_kernel::auth::{AuthManager, AuthMethod};
use fx_kernel::budget::{BudgetConfig, BudgetTracker};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::context_manager::ContextCompactor;
use fx_kernel::input::LoopCommand;
use fx_kernel::loop_engine::{LlmProvider as LoopLlmProvider, LoopEngine, LoopResult};
use fx_kernel::oauth::{PkceFlow, TokenExchangeRequest, TokenResponse};
use fx_kernel::signals::{LoopStep, Signal, SignalCollector};
use fx_kernel::types::PerceptionSnapshot;
use fx_llm::{
    AnthropicProvider, CompletionRequest, Message, ModelCatalog, ModelInfo, ModelRouter,
    OpenAiProvider, OpenAiResponsesProvider, ProviderError, RouterError, StreamChunk,
};
use fx_loadable::SkillRegistry;
use rustyline::completion::{Completer, Pair};
use rustyline::config::CompletionType;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;
use termimad::crossterm::style::{Attribute as MadAttribute, Color as MadColor};
use termimad::{MadSkin, StyledChar};
use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};

const BANNER_ART: &str = r#"   ___
  / _/__ __    ____  __
 / _/ _ `/ |/|/ /\ \/ /
/_/ \_,_/|__,__/ /_/\_\"#;
#[allow(dead_code)]
/// Pre-rendered braille+truecolor ANSI banner (via ascii-image-converter).
const FAWX_BANNER_ANSI: &str = include_str!("../../../../scripts/fawx-banner.ans");

const DEFAULT_OPENAI_TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const MAX_PROMPT_RETRIES: usize = 10;
/// Cap conversation history to limit context bleed between unrelated turns.
/// Higher values increase the chance of the model referencing stale context;
/// lower values lose multi-turn continuity. 10 balances both concerns.
const MAX_CONVERSATION_HISTORY: usize = 10;
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

const DEFAULT_ANTHROPIC_MODELS: &[&str] = &[
    "claude-sonnet-4-20250514",
    "claude-opus-4-20250514",
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

fn theme_color(r: u8, g: u8, b: u8, fallback_256: u8) -> style::Color {
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

fn clear_spinner_line() {
    eprint!("\r\x1b[2K");
    let _ = io::stderr().flush();
}

async fn run_thinking_spinner(mut stop_signal: oneshot::Receiver<()>) {
    let mut frame_index = 0;
    loop {
        tokio::select! {
            _ = &mut stop_signal => {
                clear_spinner_line();
                break;
            }
            _ = sleep(Duration::from_millis(80)) => {
                eprint!("\r\x1b[2K{} thinking...", spinner_frame(frame_index));
                let _ = io::stderr().flush();
                frame_index += 1;
            }
        }
    }
}

async fn run_with_thinking_spinner<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    let (stop_tx, stop_rx) = oneshot::channel();
    let spinner_task = tokio::spawn(run_thinking_spinner(stop_rx));
    let result = future.await;
    let _ = stop_tx.send(());
    let _ = spinner_task.await;
    result
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
    "/loop",
    "/budget",
    "/signals",
    "/debug",
    "/synthesis",
];

const PROMPT_COLOR_START: &str = "\x1b[38;2;255;204;0m";
const PROMPT_COLOR_END: &str = "\x1b[0m";

struct FawxReadlineHelper {
    hinter: HistoryHinter,
    model_ids: Arc<Mutex<Vec<String>>>,
}

impl Default for FawxReadlineHelper {
    fn default() -> Self {
        Self {
            hinter: HistoryHinter {},
            model_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Helper for FawxReadlineHelper {}
impl Highlighter for FawxReadlineHelper {}
impl Validator for FawxReadlineHelper {}

impl Hinter for FawxReadlineHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, context: &Context<'_>) -> Option<String> {
        if line.len() < 2 {
            return None;
        }
        self.hinter.hint(line, pos, context)
    }
}

impl Completer for FawxReadlineHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _context: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let line_end = pos.min(line.len());

        // Complete model IDs after "/model "
        if let Some(model_prefix) = line[..line_end].strip_prefix("/model ") {
            return Ok((
                "/model ".len(),
                model_completion_matches(model_prefix, &self.model_ids),
            ));
        }

        let start = token_start(line, line_end);
        if start != 0 {
            return Ok((start, Vec::new()));
        }

        let prefix = &line[start..line_end];
        if !prefix.starts_with('/') {
            return Ok((start, Vec::new()));
        }

        Ok((start, command_completion_matches(prefix)))
    }
}

fn token_start(line: &str, cursor: usize) -> usize {
    line[..cursor]
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx + ch.len_utf8()))
        .unwrap_or(0)
}

fn command_completion_matches(prefix: &str) -> Vec<Pair> {
    if !prefix.starts_with('/') {
        return Vec::new();
    }

    TUI_COMMANDS
        .iter()
        .filter(|command| command.starts_with(prefix))
        .map(|command| Pair {
            display: (*command).to_string(),
            replacement: (*command).to_string(),
        })
        .collect()
}

fn model_completion_matches(prefix: &str, model_ids: &Arc<Mutex<Vec<String>>>) -> Vec<Pair> {
    let ids = match model_ids.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            eprintln!("warning: model completion list mutex poisoned; returning no suggestions");
            return Vec::new();
        }
    };
    let normalized_prefix = prefix.to_lowercase();
    ids.iter()
        .filter(|id| id.to_lowercase().starts_with(&normalized_prefix))
        .map(|id| Pair {
            display: id.clone(),
            replacement: id.clone(),
        })
        .collect()
}

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

fn build_tui_prompt() -> String {
    format!("{PROMPT_COLOR_START}you › {PROMPT_COLOR_END}")
}

fn load_history_with_warning(
    editor: &mut Editor<FawxReadlineHelper, DefaultHistory>,
    path: &Path,
) -> bool {
    match editor.load_history(path) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "warning: failed to load command history from {}: {}",
                path.display(),
                error
            );
            false
        }
    }
}

fn configure_line_editor(
    model_ids: Arc<Mutex<Vec<String>>>,
) -> Result<Editor<FawxReadlineHelper, DefaultHistory>, TuiError> {
    let config = rustyline::Config::builder()
        .completion_type(CompletionType::List)
        .build();
    let mut editor =
        Editor::with_config(config).map_err(|error| TuiError::Auth(error.to_string()))?;
    editor.set_helper(Some(FawxReadlineHelper {
        hinter: HistoryHinter {},
        model_ids,
    }));

    if let Some(path) = history_path() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(TuiError::Io)?;
        }
        if path.exists() {
            load_history_with_warning(&mut editor, &path);
        }
    }

    Ok(editor)
}

fn save_line_editor_history(editor: &mut Editor<FawxReadlineHelper, DefaultHistory>) {
    if let Some(path) = history_path() {
        if let Err(error) = editor.save_history(&path) {
            eprintln!("failed to save command history: {error}");
        }
    }
}

/// Parse a bare-word command typed during spinner/thinking phase.
#[allow(dead_code)] // Wired into readline handoff in a follow-up.
pub(crate) fn parse_bare_command(input: &str) -> Option<LoopCommand> {
    match input.trim().to_lowercase().as_str() {
        "stop" | "s" => Some(LoopCommand::Stop),
        "abort" | "a" | "cancel" => Some(LoopCommand::Abort),
        "no" => Some(LoopCommand::Stop),
        "wait" | "pause" | "w" => Some(LoopCommand::Wait),
        "go" | "resume" | "continue" => Some(LoopCommand::Resume),
        _ => None,
    }
}

/// The main TUI application loop.
pub struct TuiApp {
    router: ModelRouter,
    auth_manager: AuthManager,
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
    /// Shared model ID list for readline tab-completion.
    completer_model_ids: Arc<Mutex<Vec<String>>>,
}

impl TuiApp {
    /// Create a new TUI application.
    pub fn new(
        auth_manager: AuthManager,
        router: ModelRouter,
        loop_engine: LoopEngine,
        config: FawxConfig,
    ) -> Result<Self, TuiError> {
        let base_data_dir = fawx_data_dir();
        let data_dir = configured_data_dir(&base_data_dir, &config);
        let mut conversation_store = ConversationStore::new(&data_dir).map_err(TuiError::Store)?;
        let session_id = conversation_store
            .ensure_active()
            .map_err(TuiError::Store)?;
        let signal_store =
            SignalStore::new(&data_dir, &session_id).map_err(|e| TuiError::Store(e.to_string()))?;
        if let Err(e) = signal_store.cleanup_old_signals() {
            eprintln!("warning: signal cleanup failed: {e}");
        }
        if config.general.max_history > MAX_CONVERSATION_HISTORY {
            eprintln!(
                "Note: conversation history capped at {MAX_CONVERSATION_HISTORY} entries (configured: {})",
                config.general.max_history
            );
        }
        let max_history = config.general.max_history.min(MAX_CONVERSATION_HISTORY);
        let conversation_history = if cfg!(test) {
            Vec::new()
        } else {
            load_conversation_history(&conversation_store, max_history)
        };

        Ok(Self {
            router,
            auth_manager,
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
            completer_model_ids: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Run the TUI main loop.
    pub async fn run(&mut self) -> Result<(), TuiError> {
        self.show_welcome();

        // Ctrl+C signals cancellation instead of killing the process.
        // First press: graceful cancel. Second press: force exit (NB1).
        let cancel_for_signal = self.cancel_token.clone();
        tokio::spawn(async move {
            // First Ctrl+C: graceful cancel
            tokio::signal::ctrl_c().await.ok();
            cancel_for_signal.cancel();
            // Second Ctrl+C: force exit
            tokio::signal::ctrl_c().await.ok();
            eprintln!("\n⏹ Force quit.");
            std::process::exit(130);
        });

        // Wire the cancellation token into the loop engine.
        self.loop_engine.set_cancel_token(self.cancel_token.clone());

        self.sync_completer_model_ids();
        let mut editor = configure_line_editor(Arc::clone(&self.completer_model_ids))?;
        let prompt = build_tui_prompt();
        while self.running {
            match editor.readline(&prompt) {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    // Only persist recognized slash commands and chat messages in history.
                    // Mistyped commands (e.g. /ex, /halp) are excluded to keep the
                    // HistoryHinter from suggesting invalid completions.
                    if should_add_to_history(trimmed) {
                        if let Err(error) = editor.add_history_entry(trimmed) {
                            eprintln!("failed to add command history entry: {error}");
                        }
                    }
                    self.process_input_line(trimmed).await?;
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                }
                Err(ReadlineError::Eof) => break,
                Err(error) => return Err(TuiError::Auth(error.to_string())),
            }
        }

        save_line_editor_history(&mut editor);
        Ok(())
    }

    async fn process_input_line(&mut self, input: &str) -> Result<(), TuiError> {
        if input.is_empty() {
            return Ok(());
        }

        if input.starts_with('/') {
            if let Err(error) = self.handle_command(input).await {
                println!("{}\n", format_error_message(&error.to_string()));
            }
            return Ok(());
        }

        match self.handle_message(input).await {
            Ok(response) => self.display_response(&response)?,
            Err(error) => println!("{}\n", format_error_message(&error.to_string())),
        }

        Ok(())
    }

    /// Display the welcome banner.
    fn show_welcome(&self) {
        let mut stdout = io::stdout();
        if let Err(error) = stdout.execute(terminal::Clear(terminal::ClearType::CurrentLine)) {
            eprintln!("failed to clear terminal line: {error}");
        }

        let amber = theme_color(255, 165, 0, 214);
        let burnt = theme_color(210, 112, 10, 166);

        println!();
        print!("{}", render_banner(supports_truecolor(), amber));
        println!();
        println!(
            "  {}",
            "fawx \u{00b7} agentic engine \u{00b7} type /help for commands"
                .with(burnt)
                .attribute(style::Attribute::Dim)
        );
        if !self.auth_manager.has_any() {
            println!(
                "  {}",
                "Not authenticated · /auth to set up or just send a message"
                    .with(burnt)
                    .attribute(style::Attribute::Dim)
            );
        }
        println!();
    }

    /// Run the first-time auth wizard if no credentials exist.
    async fn auth_wizard(&mut self) -> Result<(), TuiError> {
        println!("Welcome to Fawx.\n");
        if self.auth_manager.providers().is_empty() {
            println!("No credentials found. Let's set up authentication.\n");
        } else {
            println!("Add another provider.\n");
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

    fn run_auth_selection(&self) -> Result<AuthSelection, TuiError> {
        println!("How would you like to authenticate?");
        println!("  [1] Claude subscription (paste setup-token)");
        println!("  [2] ChatGPT subscription (browser sign-in)");
        println!("  [3] API key (any provider)");
        println!();

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

        println!("✓ Authenticated. Token stored.\n");
        Ok("anthropic".to_string())
    }

    async fn run_oauth_flow(&mut self) -> Result<String, TuiError> {
        let flow = PkceFlow::try_new().map_err(|error| {
            TuiError::Auth(format!("failed to initialize oauth PKCE flow: {error}"))
        })?;
        let client_id = openai_oauth_client_id();
        let auth_url = flow.authorization_url(&client_id);
        let auth_code = obtain_oauth_authorization_code(&flow, &auth_url).await?;

        println!("Exchanging authorization code for tokens...");
        let token_response = exchange_oauth_code_for_tokens(&flow, &client_id, &auth_code).await?;
        self.store_openai_oauth_tokens(token_response);

        println!("✓ Authenticated. Tokens stored.\n");
        Ok("openai".to_string())
    }

    fn store_openai_oauth_tokens(&mut self, token_response: TokenResponse) {
        let account_id = fx_kernel::oauth::extract_openai_account_id(&token_response.access_token);
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

        println!("✓ API key stored.\n");
        Ok(provider)
    }

    async fn persist_and_activate_model(
        &mut self,
        preferred_provider: &str,
    ) -> Result<(), TuiError> {
        persist_auth_manager(&self.auth_manager)?;

        let preferred_models = self.get_models_for_provider(preferred_provider).await;
        self.refresh_router_models().await?;
        self.set_preferred_model(&preferred_models);
        self.print_active_model();

        Ok(())
    }

    fn print_active_model(&self) {
        if let Some(active_model) = self.router.active_model() {
            let model_info = self
                .router
                .available_models()
                .into_iter()
                .find(|model| model.model_id == active_model);

            if let Some(model_info) = model_info {
                println!(
                    "Active model: {} ({} {})\n",
                    active_model, model_info.provider_name, model_info.auth_method
                );
            } else {
                println!("Active model: {active_model}\n");
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
                        println!("Active model set to: {resolved_model}");
                    }
                    Err(error) => {
                        println!("Couldn't select model: {error}");
                        self.show_model_menu();
                    }
                }
            }
            ParsedCommand::Auth => self.handle_auth_command().await?,
            ParsedCommand::Budget => self.show_budget_status(),
            ParsedCommand::Loop => self.show_loop_status(),
            ParsedCommand::Status => self.show_status(),
            ParsedCommand::Signals => self.show_signals_summary(),
            ParsedCommand::Debug => self.show_signals_debug(),
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
                println!("Started new conversation: {id}");
            }
            ParsedCommand::History => self.show_conversation_history(),
            ParsedCommand::Config(action) => self.handle_config_command(action)?,
            ParsedCommand::Help => self.show_help(),
            ParsedCommand::Quit => {
                self.running = false;
                println!("Goodbye!");
            }
            ParsedCommand::Unknown(command) => {
                println!("Unknown command: /{command}");
                println!("Type /help for available commands.");
            }
        }

        Ok(())
    }

    /// Process a user message by running the full loop engine.
    async fn handle_message(&mut self, input: &str) -> Result<String, TuiError> {
        self.ensure_message_auth().await?;

        let active_model = self
            .router
            .active_model()
            .ok_or_else(|| TuiError::Router("no active model selected".to_string()))?
            .to_string();

        let snapshot = self.build_perception_snapshot(input);
        let started = Instant::now();
        let loop_result = run_with_thinking_spinner(async {
            let router = &self.router;
            let llm = RouterLoopLlmProvider::new(router, active_model);
            let loop_engine = &mut self.loop_engine;
            loop_engine
                .run_cycle(snapshot, &llm)
                .await
                .map_err(|error| TuiError::Loop(error.reason))
        })
        .await?;

        self.last_signals = loop_result_signals(&loop_result).to_vec();
        if let Err(e) = self.signal_store.persist(&self.last_signals) {
            eprintln!("warning: signal persist failed: {e}");
        }
        let response_text = loop_result_response_text(&loop_result);
        let response = render_loop_result(loop_result.clone(), started.elapsed());
        self.record_conversation_turn(input, response_text.clone());
        self.persist_turn(input, &response_text, &loop_result)
            .map_err(TuiError::Store)?;
        Ok(response)
    }

    async fn ensure_message_auth(&mut self) -> Result<(), TuiError> {
        if !self.auth_manager.has_any() {
            self.auth_wizard().await?;
        }

        if self.router.active_model().is_none() {
            self.refresh_router_models().await?;
            self.select_first_available_model();
        }

        Ok(())
    }

    fn set_active_model_from_selector(&mut self, selector: &str) -> Result<String, RouterError> {
        self.router.set_active(selector)?;
        Ok(self.router.active_model().unwrap_or(selector).to_string())
    }

    async fn set_active_model_with_refresh(
        &mut self,
        selector: &str,
    ) -> Result<String, RouterError> {
        match self.set_active_model_from_selector(selector) {
            Ok(model) => Ok(model),
            Err(initial_error) => {
                if self.refresh_router_models().await.is_err() {
                    return Err(initial_error);
                }
                self.set_active_model_from_selector(selector)
            }
        }
    }

    fn show_signals_summary(&self) {
        if self.last_signals.is_empty() {
            println!("No signals from last turn.");
            return;
        }

        let collector = SignalCollector::from_signals(self.last_signals.clone());
        println!("{}", collector.summary());
    }

    fn show_signals_debug(&self) {
        if self.last_signals.is_empty() {
            println!("No signals from last turn.");
            return;
        }

        let collector = SignalCollector::from_signals(self.last_signals.clone());
        println!("{}", collector.debug_dump());
    }

    fn update_synthesis_instruction(&mut self, instruction: Option<String>) {
        match instruction {
            None => println!("Usage: /synthesis <instruction> or /synthesis reset"),
            Some(value) if value.trim().is_empty() => {
                println!("Synthesis instruction cannot be empty.");
            }
            Some(value) if value.eq_ignore_ascii_case("reset") => {
                if let Err(error) = self
                    .loop_engine
                    .set_synthesis_instruction(DEFAULT_SYNTHESIS_INSTRUCTION.to_string())
                {
                    println!("Failed to reset synthesis instruction: {}", error.reason);
                    return;
                }
                println!("Synthesis instruction reset to default.");
            }
            Some(value) => {
                if value.len() > MAX_SYNTHESIS_INSTRUCTION_LENGTH {
                    println!(
                        "Synthesis instruction exceeds {} characters.",
                        MAX_SYNTHESIS_INSTRUCTION_LENGTH
                    );
                    return;
                }

                match self.loop_engine.set_synthesis_instruction(value.clone()) {
                    Ok(()) => println!("Synthesis instruction updated: {}", value.trim()),
                    Err(error) => {
                        println!("Failed to update synthesis instruction: {}", error.reason)
                    }
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
            Some(other) => {
                println!("Unknown /config action: {other}. Use /config or /config init.")
            }
        }
        Ok(())
    }

    fn show_config(&self) {
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
        ];
        let mut output = format!(
            "Config path: {}\nRuntime data dir: {}\nLoaded values:\n",
            self.config_path.display(),
            self.data_dir_display(),
        );
        for (key, value) in fields {
            output.push_str(&format!("  {key} = {value}\n"));
        }
        print!("{output}");
    }

    fn init_config_file(&mut self) -> Result<(), TuiError> {
        let base_data_dir = fawx_data_dir();
        let created = FawxConfig::write_default(&base_data_dir).map_err(TuiError::Store)?;
        println!("Created default config at {}", created.display());
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

    fn show_conversation_history(&self) {
        let conversations = self.conversation_store.list_conversations();
        if conversations.is_empty() {
            println!("No saved conversations yet.");
            return;
        }

        println!("Saved conversations:");
        for (id, count) in conversations {
            println!("  - {id}: {count} messages");
        }
    }

    /// Display formatted output to the terminal.
    fn display_response(&self, response: &str) -> Result<(), TuiError> {
        let mut stdout = io::stdout();
        move_cursor_to_start(&mut stdout)?;

        println!();
        print!(
            "{} ",
            "assistant \u{203a}"
                .bold()
                .with(theme_color(255, 165, 0, 214))
        );
        let rendered = render_markdown(response);
        println!("{rendered}");
        println!();

        Ok(())
    }

    /// Display the model selection menu grouped by provider.
    fn show_model_menu(&self) {
        let active = self.router.active_model();
        let models = self.router.available_models();

        if models.is_empty() {
            println!("No models available. Use /auth to configure credentials.");
            return;
        }

        let grouped = group_models_by_provider(&models);

        println!("Available models:");
        for (provider, group) in &grouped {
            println!();
            println!("{provider}:");
            for model in group {
                let marker = if Some(model.model_id.as_str()) == active {
                    "*"
                } else {
                    " "
                };
                println!("  {marker} {} ({})", model.model_id, model.auth_method);
            }
        }
    }

    fn select_first_available_model(&mut self) {
        if self.router.active_model().is_some() {
            return;
        }

        if let Some(saved) = self.config.model.default_model.as_deref() {
            if self.router.set_active(saved).is_ok() {
                return;
            }
            eprintln!("Saved model '{saved}' no longer available, selecting default");
        }

        let model_ids = self
            .router
            .available_models()
            .into_iter()
            .map(|model| model.model_id)
            .collect::<Vec<_>>();

        if let Some(model) = preferred_default_model(&model_ids) {
            if let Err(error) = self.router.set_active(model) {
                eprintln!("failed to set initial model {model}: {error}");
            }
        }
    }

    fn set_preferred_model(&mut self, candidates: &[String]) {
        for candidate in candidates {
            if self.router.set_active(candidate).is_ok() {
                return;
            }
        }

        self.select_first_available_model();
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

    fn show_help(&self) {
        println!("{}", "Commands".bold().with(theme_color(255, 165, 0, 214)));
        println!("  /model         List models and switch active model");
        println!("  /model <name>  Switch to a specific model");
        println!("  /auth          Show credentials / run auth wizard");
        println!("  /status        Show model, tokens, budget summary");
        println!("  /budget        Show detailed budget usage");
        println!("  /loop          Show loop iteration details");
        println!("  /signals       Show condensed signal summary for last turn");
        println!("  /debug         Show full signal dump for last turn");
        println!("  /synthesis     Set or reset synthesis instruction");
        println!("  /clear         Clear the screen and active conversation");
        println!("  /new           Start a new conversation");
        println!("  /history       List saved conversations");
        println!("  /config        Show loaded config values");
        println!("  /config init   Create ~/.fawx/config.toml template");
        println!("  /help          Show this help");
        println!("  /quit          Exit");
    }

    fn show_auth_status(&self) {
        let providers = self.auth_manager.providers();
        if providers.is_empty() {
            println!("No credentials configured.");
            return;
        }

        println!("Configured credentials:");
        for provider in providers {
            if let Some(method) = self.auth_manager.get(&provider) {
                match method {
                    AuthMethod::SetupToken { .. } => {
                        println!("  - {provider}: Claude setup-token (subscription)");
                    }
                    AuthMethod::OAuth { account_id, .. } => {
                        if let Some(account_id) = account_id {
                            println!("  - {provider}: OAuth subscription ({account_id})");
                        } else {
                            println!("  - {provider}: OAuth subscription");
                        }
                    }
                    AuthMethod::ApiKey { .. } => {
                        println!("  - {provider}: API key");
                    }
                }
            }
        }
    }
    async fn handle_auth_command(&mut self) -> Result<(), TuiError> {
        self.show_auth_status();
        println!();
        println!("[1] Add/update credentials");
        println!("[2] Remove a provider");
        println!("[3] Cancel");

        let choice = prompt_choice(
            "> ",
            "Please choose 1, 2, or 3.",
            "auth menu selection",
            parse_auth_menu_selection,
        )?;

        match choice {
            AuthMenuSelection::AddOrUpdate => self.auth_wizard().await,
            AuthMenuSelection::RemoveProvider => self.remove_auth_provider().await,
            AuthMenuSelection::Cancel => Ok(()),
        }
    }

    async fn remove_auth_provider(&mut self) -> Result<(), TuiError> {
        let providers = self.auth_manager.providers();
        if providers.is_empty() {
            println!("No providers to remove.");
            return Ok(());
        }

        println!("Select a provider to remove:");
        for (idx, provider) in providers.iter().enumerate() {
            println!("  [{}] {}", idx + 1, provider);
        }

        let provider = prompt_provider_selection(&providers)?;
        if !confirm_provider_removal(&provider)? {
            println!("Removal cancelled.");
            return Ok(());
        }

        self.auth_manager.remove(&provider);
        persist_auth_manager(&self.auth_manager)?;
        self.refresh_router_models().await?;
        println!("Removed provider: {provider}");
        Ok(())
    }

    fn show_budget_status(&self) {
        let status = self.loop_engine.status(current_time_ms());
        println!("Budget usage:");
        println!("  - LLM calls used: {}", status.llm_calls_used);
        println!("  - Tool calls used: {}", status.tool_invocations_used);
        println!("  - Tokens used: {}", status.tokens_used);
        println!("  - Cost used (cents): {}", status.cost_cents_used);
        println!("  - Tokens remaining: {}", status.remaining.tokens);
        println!("  - LLM calls remaining: {}", status.remaining.llm_calls);
    }

    fn show_loop_status(&self) {
        let status = self.loop_engine.status(current_time_ms());
        println!("Loop status:");
        println!(
            "  - Iterations (last cycle): {}/{}",
            status.iteration_count, status.max_iterations
        );
        println!("  - Tokens used (tracker): {}", status.tokens_used);
        println!("  - Tokens remaining: {}", status.remaining.tokens);
        println!("  - LLM calls remaining: {}", status.remaining.llm_calls);
        println!(
            "  - Tool calls remaining: {}",
            status.remaining.tool_invocations
        );
        println!(
            "  - Wall time remaining (ms): {}",
            status.remaining.wall_time_ms
        );
    }

    fn show_status(&self) {
        let model = self.current_model();
        let status = self.loop_engine.status(current_time_ms());
        let providers = self.auth_manager.providers();
        println!(
            "{}",
            "Fawx Status".bold().with(theme_color(255, 165, 0, 214))
        );
        println!("  model:     {model}");
        println!("  providers: {}", providers.join(", "));
        println!("  tokens:    {} used", status.tokens_used);
        println!("  budget:    {} tokens remaining", status.remaining.tokens);
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

/// Load the user config from ~/.fawx/config.toml (or return defaults).
pub fn load_config() -> Result<FawxConfig, TuiError> {
    let base_data_dir = fawx_data_dir();
    FawxConfig::load(&base_data_dir).map_err(TuiError::Store)
}

/// Build a loop engine with sensible defaults for the TUI shell.
/// Convenience wrapper used by tests; production code uses
/// [`build_loop_engine_from_config`] for single-load consistency.
#[cfg(test)]
fn build_loop_engine() -> LoopEngine {
    let config = load_config().unwrap_or_else(|error| {
        eprintln!("warning: failed to load config: {error}");
        FawxConfig::default()
    });
    build_loop_engine_from_config(&config)
}

/// Build a loop engine from an already-loaded config.
pub fn build_loop_engine_from_config(config: &FawxConfig) -> LoopEngine {
    let base_data_dir = fawx_data_dir();
    let data_dir = configured_data_dir(&base_data_dir, config);
    build_loop_engine_with_config(data_dir, config.clone())
}

fn build_loop_engine_with_config(data_dir: PathBuf, config: FawxConfig) -> LoopEngine {
    let budget = BudgetTracker::new(BudgetConfig::default(), current_time_ms(), 0);
    let context = ContextCompactor::new(DEFAULT_CONTEXT_MAX_TOKENS, DEFAULT_CONTEXT_COMPACT_TARGET);
    let working_dir = configured_working_dir(&config);
    let (registry, memory_snapshot) = build_skill_registry(working_dir, &data_dir, &config);
    let synthesis = config
        .model
        .synthesis_instruction
        .clone()
        .unwrap_or_else(|| DEFAULT_SYNTHESIS_INSTRUCTION.to_string());

    let mut engine = LoopEngine::new(
        budget,
        context,
        config.general.max_iterations,
        std::sync::Arc::new(registry),
        synthesis,
    );
    if let Some(snapshot_text) = memory_snapshot {
        engine.set_memory_context(snapshot_text);
    }
    engine
}

fn build_skill_registry(
    working_dir: PathBuf,
    data_dir: &Path,
    config: &FawxConfig,
) -> (SkillRegistry, Option<String>) {
    let tool_config = ToolConfig {
        max_read_size: config.tools.max_read_size,
        search_exclude: config.tools.search_exclude.clone(),
        ..ToolConfig::default()
    };
    let mut executor = FawxToolExecutor::new(working_dir.clone(), tool_config);

    let memory_config = JsonMemoryConfig {
        max_entries: config.memory.max_entries,
        max_value_size: config.memory.max_value_size,
    };
    let snapshot_text = match JsonFileMemory::new_with_config(data_dir, memory_config) {
        Ok(memory) => {
            let snapshot = memory.snapshot();
            let text = format_memory_for_prompt(&snapshot, config.memory.max_snapshot_chars);
            let memory: Arc<Mutex<dyn fx_core::memory::MemoryProvider>> =
                Arc::new(Mutex::new(memory));
            executor = executor.with_memory(memory);
            text
        }
        Err(e) => {
            eprintln!("warning: failed to initialize memory: {e}");
            None
        }
    };
    let self_modify_config = fx_core::self_modify::SelfModifyConfig::from(&config.self_modify);
    let sm = self_modify_config.enabled.then_some(self_modify_config);
    if let Some(ref smc) = sm {
        executor = executor.with_self_modify(smc.clone());
    }
    let mut registry = SkillRegistry::new();
    registry.register(Box::new(BuiltinToolsSkill::new(executor)));
    let git_skill = GitSkill::new(working_dir.clone(), sm);
    registry.register(Box::new(git_skill));
    (registry, snapshot_text)
}

fn format_memory_for_prompt(entries: &[(String, String)], max_chars: usize) -> Option<String> {
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

impl<'a> fmt::Debug for RouterLoopLlmProvider<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouterLoopLlmProvider")
            .field("active_model", &self.active_model)
            .finish()
    }
}
struct RouterLoopLlmProvider<'a> {
    router: &'a ModelRouter,
    active_model: String,
}

impl<'a> RouterLoopLlmProvider<'a> {
    fn new(router: &'a ModelRouter, active_model: String) -> Self {
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
            system_prompt: None,
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
            system_prompt: None,
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

fn trim_history(history: &mut Vec<Message>, max_history: usize) {
    if history.len() <= max_history {
        return;
    }

    let remove_count = history.len() - max_history;
    history.drain(0..remove_count);
}

fn fawx_data_dir() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join(".fawx"))
        .unwrap_or_else(|| PathBuf::from(".fawx"))
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

fn format_error_message(error: &str) -> String {
    format!("\x1b[31m  \u{2717} {error}\x1b[0m")
}

fn move_cursor_to_start(stdout: &mut impl Write) -> Result<(), TuiError> {
    stdout
        .execute(cursor::MoveToColumn(0))
        .map(|_| ())
        .map_err(TuiError::Io)
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
    println!("Opening browser for ChatGPT sign-in...");
    if let Err(error) = open_browser(auth_url) {
        println!(
            "Couldn't open browser automatically ({error}). Open this URL manually:\n{auth_url}"
        );
    }
    println!("Waiting for callback on http://localhost:1455/auth/callback...");
    println!("(Or paste the redirect URL/code below if browser didn't work)\n");

    tokio::select! {
        result = server => result.or_else(|_| prompt_for_oauth_code(flow)),
    }
}

fn oauth_code_manual_fallback(flow: &PkceFlow, auth_url: &str) -> Result<String, TuiError> {
    println!("Couldn't start local server. Open this URL in your browser:\n");
    println!("  {auth_url}\n");
    prompt_for_oauth_code(flow)
}

fn prompt_for_oauth_code(flow: &PkceFlow) -> Result<String, TuiError> {
    let input = prompt_non_empty_line(
        "Paste the redirect URL or authorization code: ",
        "Input cannot be empty.\n",
        "OAuth redirect URL or code",
    )?;

    flow.parse_callback(&input)
        .or_else(|_| Ok::<String, fx_kernel::oauth::AuthError>(input.trim().to_string()))
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
        .unwrap_or_else(|| fx_kernel::oauth::OPENAI_CLIENT_ID.to_string())
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

fn prompt_line(prompt: &str) -> Result<String, TuiError> {
    ensure_cooked_mode();

    print!("{prompt}");
    io::stdout().flush().map_err(TuiError::Io)?;

    let mut input = String::new();
    let bytes = io::stdin().read_line(&mut input).map_err(TuiError::Io)?;
    if bytes == 0 {
        return Err(TuiError::Auth("stdin closed unexpectedly".to_string()));
    }

    Ok(input.trim().to_string())
}

fn ensure_cooked_mode() {
    // Ensure cooked mode so stdin.read_line() handles Enter correctly even after raw-mode input paths.
    terminal::disable_raw_mode().ok();
}

fn confirm_provider_removal(provider: &str) -> Result<bool, TuiError> {
    let prompt = format!("Remove {provider}? [y/N]: ");
    let response = prompt_line(&prompt)?;
    Ok(removal_confirmation_accepted(&response))
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
    for _ in 0..MAX_PROMPT_RETRIES {
        let value = prompt_line(prompt)?;
        if let Some(parsed) = parser(&value) {
            return Ok(parsed);
        }

        println!("{invalid_message}");
    }

    Err(retry_limit_error(context))
}

fn prompt_non_empty_line(
    prompt: &str,
    empty_message: &str,
    context: &str,
) -> Result<String, TuiError> {
    for _ in 0..MAX_PROMPT_RETRIES {
        let value = prompt_line(prompt)?;
        if !value.is_empty() {
            return Ok(value);
        }

        println!("{empty_message}");
    }

    Err(retry_limit_error(context))
}

fn prompt_api_key_provider() -> Result<String, TuiError> {
    println!("Which provider?");
    println!("  [1] Anthropic");
    println!("  [2] OpenAI");
    println!("  [3] OpenRouter");
    println!("  [4] Other (OpenAI-compatible)");
    println!();

    let choice = prompt_choice(
        "> ",
        "Please choose 1, 2, 3, or 4.",
        "API key provider selection",
        parse_api_key_provider_selection,
    )?;

    match choice {
        ApiKeyProvider::Anthropic => Ok("anthropic".to_string()),
        ApiKeyProvider::OpenAi => Ok("openai".to_string()),
        ApiKeyProvider::OpenRouter => Ok("openrouter".to_string()),
        ApiKeyProvider::Other => prompt_non_empty_line(
            "Provider name: ",
            "Provider name cannot be empty.",
            "API key provider name",
        ),
    }
}

fn prompt_non_empty_secret(
    prompt: &str,
    empty_message: &str,
    context: &str,
) -> Result<String, TuiError> {
    for _ in 0..MAX_PROMPT_RETRIES {
        let value = prompt_secret(prompt)?;
        if !value.is_empty() {
            return Ok(value);
        }

        println!("{empty_message}");
    }

    Err(retry_limit_error(context))
}

fn prompt_secret(prompt: &str) -> Result<String, TuiError> {
    print!("{prompt}");
    io::stdout().flush().map_err(TuiError::Io)?;

    let _guard = RawModeGuard::new()?;
    let mut value = String::new();
    read_secret_input(&mut value)?;

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        println!();
    } else {
        println!(" ({} chars)", trimmed.len());
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
                    print!("•");
                    io::stdout().flush().map_err(TuiError::Io)?;
                }
                SecretInputKeyAction::Delete => {
                    if value.pop().is_some() && display_len > 0 {
                        display_len -= 1;
                        print!("\x08 \x08");
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
    Auth,
    Budget,
    Loop,
    Status,
    Signals,
    Debug,
    Synthesis(Option<String>),
    Clear,
    New,
    History,
    Config(Option<String>),
    Help,
    Quit,
    Unknown(String),
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
        "auth" => ParsedCommand::Auth,
        "budget" => ParsedCommand::Budget,
        "loop" => ParsedCommand::Loop,
        "status" => ParsedCommand::Status,
        "signals" => ParsedCommand::Signals,
        "debug" => ParsedCommand::Debug,
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_llm::ContentBlock;
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

    fn new_test_app() -> TuiApp {
        TuiApp::new(
            AuthManager::new(),
            ModelRouter::new(),
            build_loop_engine(),
            FawxConfig::default(),
        )
        .expect("new test app")
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

    fn app_with_mock_model(response: &str) -> TuiApp {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(StaticCompletionProvider::new("mock-loop-model", response)),
            "test",
        );
        router
            .set_active("mock-loop-model")
            .expect("set active mock model");

        TuiApp::new(
            test_provider_auth_manager(),
            router,
            build_loop_engine(),
            FawxConfig::default(),
        )
        .expect("mock app")
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

    fn app_with_two_models() -> TuiApp {
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

        TuiApp::new(
            test_provider_auth_manager(),
            router,
            build_loop_engine(),
            FawxConfig::default(),
        )
        .expect("mock app")
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

        assert_eq!(openai_oauth_client_id(), fx_kernel::oauth::OPENAI_CLIENT_ID);
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

    #[tokio::test]
    async fn spinner_stops_when_signaled() {
        let (stop_tx, stop_rx) = oneshot::channel();
        let spinner = tokio::spawn(run_thinking_spinner(stop_rx));

        let _ = stop_tx.send(());

        tokio::time::timeout(Duration::from_millis(200), spinner)
            .await
            .expect("spinner should stop quickly")
            .expect("spinner task should join");
    }

    #[test]
    fn format_error_message_uses_ansi_escape_sequences() {
        let message = format_error_message("boom");

        assert!(message.contains("\x1b[31m"));
        assert!(message.contains("\u{2717} boom"));
        assert!(message.contains("\x1b[0m"));
    }

    #[test]
    fn move_cursor_to_start_returns_error_when_terminal_write_fails() {
        let mut writer = FailingWriter;

        let error = move_cursor_to_start(&mut writer).expect_err("terminal error expected");
        assert!(
            matches!(error, TuiError::Io(io_error) if io_error.to_string().contains("write failed"))
        );
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
    fn command_completer_matches_slash_commands() {
        let matches = command_completion_matches("/st");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].replacement, "/status");
    }

    #[test]
    fn command_completer_empty_for_non_slash() {
        let matches = command_completion_matches("hello");

        assert!(matches.is_empty());
    }

    #[test]
    fn readline_completer_matches_first_token_prefix() {
        let helper = FawxReadlineHelper::default();
        let history = DefaultHistory::new();
        let context = rustyline::Context::new(&history);

        let (start, matches) = helper
            .complete("/h", 2, &context)
            .expect("completion should succeed");

        assert_eq!(start, 0);
        assert!(matches.iter().any(|pair| pair.replacement == "/help"));
    }

    #[test]
    fn readline_completer_returns_no_matches_after_first_token() {
        let helper = FawxReadlineHelper::default();
        let history = DefaultHistory::new();
        let context = rustyline::Context::new(&history);
        let line = "/help arg";

        let (start, matches) = helper
            .complete(line, line.len(), &context)
            .expect("completion should succeed");

        assert_eq!(start, 6);
        assert!(matches.is_empty());
    }

    #[test]
    fn readline_completer_keeps_start_at_zero_inside_first_token() {
        let helper = FawxReadlineHelper::default();
        let history = DefaultHistory::new();
        let context = rustyline::Context::new(&history);

        let (start, matches) = helper
            .complete("/help", 2, &context)
            .expect("completion should succeed");

        assert_eq!(start, 0);
        assert!(matches.iter().any(|pair| pair.replacement == "/help"));
    }

    #[test]
    fn hinter_suppresses_hints_for_single_char_input() {
        let helper = FawxReadlineHelper::default();
        let history = DefaultHistory::new();
        let context = rustyline::Context::new(&history);

        assert!(
            helper.hint("/", 1, &context).is_none(),
            "single char should not trigger hint"
        );
        assert!(
            helper.hint("", 0, &context).is_none(),
            "empty input should not trigger hint"
        );
    }

    #[test]
    fn hinter_allows_hints_for_two_or_more_chars() {
        let helper = FawxReadlineHelper::default();
        let history = DefaultHistory::new();
        let context = rustyline::Context::new(&history);

        // With no history loaded, hint returns None regardless,
        // but the gate should not block the call.
        let _ = helper.hint("/h", 2, &context);
        let _ = helper.hint("/he", 3, &context);
        // No panic = gate allows through.
    }

    #[test]
    fn token_start_returns_zero_for_first_token() {
        assert_eq!(token_start("/help", 5), 0);
        assert_eq!(token_start("/h", 2), 0);
        assert_eq!(token_start("", 0), 0);
    }

    #[test]
    fn token_start_returns_position_after_whitespace() {
        assert_eq!(token_start("/help arg", 9), 6);
        assert_eq!(token_start("a b c", 5), 4);
        assert_eq!(token_start("hello  world", 12), 7);
    }
    #[test]
    fn should_add_to_history_accepts_valid_commands() {
        assert!(should_add_to_history("/help"));
        assert!(should_add_to_history("/quit"));
        assert!(should_add_to_history("/model list"));
        assert!(should_add_to_history("/clear"));
        assert!(should_add_to_history("/new"));
        assert!(should_add_to_history("/history"));
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

    #[test]
    fn load_history_with_warning_returns_false_for_unreadable_history_path() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let history_path = tempdir.path().join("history-as-directory");
        std::fs::create_dir(&history_path).expect("history path directory should be created");

        let mut editor = Editor::new().expect("editor should be created");
        editor.set_helper(Some(FawxReadlineHelper::default()));

        let loaded = load_history_with_warning(&mut editor, &history_path);

        assert!(!loaded);
    }

    #[test]
    fn tui_prompt_contains_ansi_color_codes() {
        let prompt = build_tui_prompt();
        assert!(
            !prompt.contains("\x01"),
            "prompt should not contain readline markers"
        );
        assert!(
            !prompt.contains("\x02"),
            "prompt should not contain readline markers"
        );
        assert!(prompt.contains(PROMPT_COLOR_START));
        assert!(prompt.contains(PROMPT_COLOR_END));
    }

    #[tokio::test]
    async fn handle_command_dispatches_help_without_stopping() {
        let mut app = new_test_app();

        app.handle_command("/help").await.unwrap();

        assert!(app.running);
    }

    #[tokio::test]
    async fn tui_runs_without_auth_configured() {
        let mut app = new_test_app();

        app.process_input_line("/help").await.unwrap();
        app.process_input_line("/status").await.unwrap();

        assert!(app.running);
        assert!(!app.auth_manager.has_any());
    }

    #[tokio::test]
    async fn help_command_works_without_auth() {
        let mut app = new_test_app();

        app.process_input_line("/help").await.unwrap();

        assert!(app.running);
        assert!(!app.auth_manager.has_any());
    }

    #[tokio::test]
    async fn quit_command_works_without_auth() {
        let mut app = new_test_app();

        app.process_input_line("/quit").await.unwrap();

        assert!(!app.running);
        assert!(!app.auth_manager.has_any());
    }

    #[tokio::test]
    async fn run_exits_immediately_when_not_running() {
        let mut app = TuiApp::new(
            test_auth_manager(),
            ModelRouter::new(),
            build_loop_engine(),
            FawxConfig::default(),
        )
        .expect("app");
        app.running = false;

        let result = app.run().await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn handle_command_dispatches_quit_and_stops() {
        let mut app = new_test_app();

        app.handle_command("/quit").await.unwrap();

        assert!(!app.running);
    }

    #[tokio::test]
    async fn message_triggers_auth_when_not_configured() {
        let mut app = new_test_app();

        let error = app
            .handle_message("hello")
            .await
            .expect_err("message should trigger auth wizard without configured credentials");

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

        let mut config = FawxConfig::default();
        config.model.default_model = Some("claude-sonnet-4-6-20250929".to_string());

        let mut app = TuiApp::new(
            test_provider_auth_manager(),
            router,
            build_loop_engine(),
            config,
        )
        .expect("mock app");

        app.select_first_available_model();

        assert_eq!(app.current_model(), "claude-sonnet-4-6-20250929");
    }

    #[tokio::test]
    async fn status_reflects_switched_model() {
        let mut app = app_with_two_models();

        app.handle_command("/model claude-sonnet-4-6")
            .await
            .unwrap();

        assert_eq!(app.current_model(), "claude-sonnet-4-6-20250929");
    }

    #[tokio::test]
    async fn model_command_resolves_prefix_to_full_model_id() {
        let mut app = app_with_two_models();

        let resolved = app
            .set_active_model_from_selector("claude-sonnet-4-6")
            .expect("model selector should resolve");

        assert_eq!(resolved, "claude-sonnet-4-6-20250929");
    }

    #[tokio::test]
    async fn handle_message_uses_current_active_model() {
        let mut app = app_with_two_models();

        app.handle_command("/model claude-sonnet-4-6")
            .await
            .unwrap();
        let rendered = app.handle_message("hello").await.expect("loop response");

        assert!(rendered.contains("claude-sonnet-4-6-20250929"));
    }

    #[tokio::test]
    async fn handle_message_returns_loop_result_not_raw_completion_payload() {
        let mut app = app_with_mock_model(
            r#"{"action":{"Respond":{"text":"Loop-integrated reply"}},"rationale":"direct","confidence":0.91,"expected_outcome":null,"sub_goals":[]}"#,
        );

        let rendered = app
            .handle_message("hello")
            .await
            .expect("loop-generated message");

        assert!(rendered.contains("Loop-integrated reply"));
        assert!(rendered.contains("1 iteration"));
    }

    #[tokio::test]
    async fn conversation_history_accumulates_across_messages() {
        let mut app = app_with_mock_model(
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
        let mut app = app_with_two_models();

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
        let mut app = app_with_mock_model(
            r#"{"action":{"Respond":{"text":"ok"}},"rationale":"r","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#,
        );

        for i in 0..15 {
            let message = format!("msg-{i}");
            let _ = app
                .handle_message(&message)
                .await
                .expect("message response");
        }

        assert_eq!(app.conversation_history.len(), MAX_CONVERSATION_HISTORY);
        assert_eq!(
            app.conversation_history[0],
            Message::user("msg-10".to_string())
        );
    }

    #[tokio::test]
    async fn synthesis_command_updates_instruction() {
        let mut app = new_test_app();

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
        let mut app = new_test_app();

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
        let mut app = new_test_app();
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
        let mut app = app_with_mock_model(
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
        let mut app = app_with_mock_model(plain_text);

        let rendered = app.handle_message("Hey!").await.expect("loop result");

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
        let mut app = app_with_mock_model(valid_json);

        let rendered = app.handle_message("Hey!").await.expect("loop result");

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
    fn completer_returns_model_matches() {
        let ids = Arc::new(Mutex::new(vec![
            "Claude-sonnet-4-20260514".to_string(),
            "Claude-opus-4-20260514".to_string(),
            "gpt-4o".to_string(),
        ]));
        let helper = FawxReadlineHelper {
            hinter: HistoryHinter {},
            model_ids: Arc::clone(&ids),
        };
        let history = DefaultHistory::new();
        let ctx = rustyline::Context::new(&history);
        let (pos, matches) = helper.complete("/model clau", 11, &ctx).unwrap();
        assert_eq!(pos, "/model ".len());
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].replacement, "Claude-sonnet-4-20260514");
        assert_eq!(matches[1].replacement, "Claude-opus-4-20260514");
    }

    #[test]
    fn completer_returns_empty_for_no_model_match() {
        let ids = Arc::new(Mutex::new(vec![
            "claude-sonnet-4-20260514".to_string(),
            "gpt-4o".to_string(),
        ]));
        let helper = FawxReadlineHelper {
            hinter: HistoryHinter {},
            model_ids: Arc::clone(&ids),
        };
        let history = DefaultHistory::new();
        let ctx = rustyline::Context::new(&history);
        let (pos, matches) = helper.complete("/model xyz", 10, &ctx).unwrap();
        assert_eq!(pos, "/model ".len());
        assert!(matches.is_empty());
    }

    #[test]
    fn completer_returns_all_models_for_empty_model_prefix() {
        let ids = Arc::new(Mutex::new(vec![
            "claude-sonnet-4-20260514".to_string(),
            "gpt-4o".to_string(),
        ]));
        let helper = FawxReadlineHelper {
            hinter: HistoryHinter {},
            model_ids: Arc::clone(&ids),
        };
        let history = DefaultHistory::new();
        let ctx = rustyline::Context::new(&history);
        let (pos, matches) = helper.complete("/model ", 7, &ctx).unwrap();
        assert_eq!(pos, "/model ".len());
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].replacement, "claude-sonnet-4-20260514");
        assert_eq!(matches[1].replacement, "gpt-4o");
    }

    #[test]
    fn completer_returns_empty_when_no_models_registered() {
        let ids = Arc::new(Mutex::new(Vec::new()));
        let helper = FawxReadlineHelper {
            hinter: HistoryHinter {},
            model_ids: Arc::clone(&ids),
        };
        let history = DefaultHistory::new();
        let ctx = rustyline::Context::new(&history);
        let (pos, matches) = helper.complete("/model ", 7, &ctx).unwrap();
        assert_eq!(pos, "/model ".len());
        assert!(matches.is_empty());
    }

    #[test]
    fn completer_still_completes_slash_commands() {
        let helper = FawxReadlineHelper::default();
        let history = DefaultHistory::new();
        let ctx = rustyline::Context::new(&history);
        let (pos, matches) = helper.complete("/mo", 3, &ctx).unwrap();
        assert_eq!(pos, 0);
        assert!(matches.iter().any(|m| m.replacement == "/model"));
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
}
