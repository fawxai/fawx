//! Headless mode for Fawx — stdin/stdout REPL without the TUI.
//!
//! Provides `HeadlessApp` which drives the full agentic loop via
//! `LoopEngine::run_cycle()` while reading input from stdin and writing
//! responses to stdout. All diagnostic/error output goes to stderr so
//! downstream consumers can safely pipe stdout.

use fx_config::FawxConfig;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_kernel::loop_engine::{LoopEngine, LoopResult};
use fx_kernel::types::PerceptionSnapshot;
use fx_llm::{Message, ModelRouter};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::tui::{
    fawx_data_dir, format_memory_for_prompt, trim_history, RouterLoopLlmProvider, SharedMemoryStore,
};

// ── JSON I/O types ──────────────────────────────────────────────────────────

/// JSON-mode input envelope.
#[derive(serde::Deserialize)]
struct JsonInput {
    message: String,
}

/// JSON-mode output envelope.
#[derive(serde::Serialize)]
struct JsonOutput {
    response: String,
    model: String,
    iterations: u32,
}

// ── HeadlessApp ─────────────────────────────────────────────────────────────

/// Dependencies for constructing a [`HeadlessApp`]. Avoids > 5 bare params.
pub struct HeadlessAppDeps {
    pub loop_engine: LoopEngine,
    pub router: ModelRouter,
    pub config: FawxConfig,
    pub memory: Option<SharedMemoryStore>,
    pub system_prompt_path: Option<PathBuf>,
}

/// Headless Fawx agent: drives `LoopEngine` via stdin/stdout.
pub struct HeadlessApp {
    loop_engine: LoopEngine,
    router: ModelRouter,
    config: FawxConfig,
    memory: Option<SharedMemoryStore>,
    active_model: String,
    conversation_history: Vec<Message>,
    max_history: usize,
    custom_system_prompt: Option<String>,
}

impl HeadlessApp {
    /// Build from the standard startup bundle + router + config.
    pub fn new(deps: HeadlessAppDeps) -> Result<Self, anyhow::Error> {
        let active_model = deps
            .router
            .active_model()
            .map(ToString::to_string)
            .unwrap_or_default();
        let max_history = deps.config.general.max_history;
        let custom_system_prompt = load_system_prompt(deps.system_prompt_path.as_deref());

        Ok(Self {
            loop_engine: deps.loop_engine,
            router: deps.router,
            config: deps.config,
            memory: deps.memory,
            active_model,
            conversation_history: Vec::new(),
            max_history,
            custom_system_prompt,
        })
    }

    /// REPL mode: read lines from stdin, run the loop, print to stdout.
    pub async fn run(&mut self, json_mode: bool) -> Result<i32, anyhow::Error> {
        install_sigpipe_handler();
        self.apply_custom_system_prompt();
        self.print_startup_info();

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                break; // EOF
            }

            let input = if json_mode {
                match self.parse_json_input(&line) {
                    Ok(msg) => msg,
                    Err(e) => {
                        eprintln!("error: invalid JSON input: {e}");
                        continue;
                    }
                }
            } else {
                line.trim().to_string()
            };

            if input.is_empty() {
                continue;
            }

            if is_quit_command(&input) {
                break;
            }

            self.process_input(&input, json_mode).await?;
        }

        Ok(0)
    }

    /// Single-shot mode: one input, one response, exit.
    pub async fn run_single(&mut self, json_mode: bool) -> Result<i32, anyhow::Error> {
        install_sigpipe_handler();
        self.apply_custom_system_prompt();

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let input = if json_mode {
            self.parse_json_input(&line)?
        } else {
            line.trim().to_string()
        };

        if input.is_empty() {
            return Ok(0);
        }

        self.process_input(&input, json_mode).await?;
        Ok(0)
    }

    // ── internal helpers ────────────────────────────────────────────────

    async fn process_input(&mut self, input: &str, json_mode: bool) -> Result<(), anyhow::Error> {
        self.update_memory_context(input);
        let snapshot = self.build_perception_snapshot(input);
        let llm = RouterLoopLlmProvider::new(&self.router, self.active_model.clone());
        let result = self
            .loop_engine
            .run_cycle(snapshot, &llm)
            .await
            .map_err(|e| anyhow::anyhow!("loop error: stage={} reason={}", e.stage, e.reason))?;

        if json_mode {
            self.emit_json_output(&result)?;
        } else {
            self.emit_text_output(&result)?;
        }

        let response = extract_response_text(&result);
        self.record_turn(input, &response);
        Ok(())
    }

    fn apply_custom_system_prompt(&mut self) {
        if self.custom_system_prompt.is_some() {
            // Initial memory context injection; update_memory_context()
            // will re-inject the custom prompt on each cycle.
            self.update_memory_context("");
        }
    }

    fn print_startup_info(&self) {
        eprintln!("fawx serve — headless mode");
        eprintln!("model: {}", self.active_model);
        if self.custom_system_prompt.is_some() {
            eprintln!("system prompt: ~/.fawx/system_prompt.md loaded");
        }
        eprintln!("ready (type /quit to exit)");
    }

    fn update_memory_context(&mut self, input: &str) {
        let mut context_parts: Vec<String> = Vec::new();

        if let Some(prompt) = &self.custom_system_prompt {
            context_parts.push(prompt.clone());
        }

        if let Some(mem) = self.relevant_memory_context(input) {
            context_parts.push(mem);
        }

        let combined = context_parts.join("\n\n");
        self.loop_engine.set_memory_context(combined);
    }

    fn relevant_memory_context(&self, input: &str) -> Option<String> {
        let entries = self.search_memory_entries(input)?;
        format_memory_for_prompt(&entries, self.config.memory.max_snapshot_chars)
    }

    fn search_memory_entries(&self, input: &str) -> Option<Vec<(String, String)>> {
        let memory = self.memory.as_ref()?;
        match memory.lock() {
            Ok(store) => {
                let max = self.config.memory.max_relevant_results;
                Some((*store).search_relevant(input, max))
            }
            Err(e) => {
                eprintln!("warning: failed to lock memory store: {e}");
                None
            }
        }
    }

    fn build_perception_snapshot(&self, input: &str) -> PerceptionSnapshot {
        let timestamp_ms = current_time_ms();
        PerceptionSnapshot {
            screen: ScreenState {
                current_app: "fawx.headless".to_string(),
                elements: Vec::new(),
                text_content: input.to_string(),
            },
            notifications: Vec::new(),
            active_app: "fawx.headless".to_string(),
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

    fn record_turn(&mut self, user_text: &str, assistant_text: &str) {
        self.conversation_history
            .push(Message::user(user_text.to_string()));
        self.conversation_history
            .push(Message::assistant(assistant_text.to_string()));
        trim_history(&mut self.conversation_history, self.max_history);
    }

    fn parse_json_input(&self, raw: &str) -> Result<String, serde_json::Error> {
        let parsed: JsonInput = serde_json::from_str(raw)?;
        Ok(parsed.message)
    }

    fn emit_json_output(&self, result: &LoopResult) -> Result<(), anyhow::Error> {
        let output = JsonOutput {
            response: extract_response_text(result),
            model: self.active_model.clone(),
            iterations: extract_iterations(result),
        };
        let json = serde_json::to_string(&output)?;
        println!("{json}");
        io::stdout().flush()?;
        Ok(())
    }

    fn emit_text_output(&self, result: &LoopResult) -> Result<(), anyhow::Error> {
        let text = extract_response_text(result);
        println!("{text}");
        io::stdout().flush()?;
        Ok(())
    }
}

// ── Free functions ──────────────────────────────────────────────────────────

fn is_quit_command(input: &str) -> bool {
    matches!(input, "/quit" | "/exit")
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Load a system prompt from an explicit path or the default location.
///
/// When `explicit_path` is `Some`, only that path is tried. Otherwise
/// the default `~/.fawx/system_prompt.md` is used.
fn load_system_prompt(explicit_path: Option<&std::path::Path>) -> Option<String> {
    let path = match explicit_path {
        Some(p) => p.to_path_buf(),
        None => fawx_data_dir().join("system_prompt.md"),
    };
    std::fs::read_to_string(&path).ok().and_then(
        |s| {
            if s.trim().is_empty() {
                None
            } else {
                Some(s)
            }
        },
    )
}

/// Reset SIGPIPE to default behavior on Unix so piped output
/// (`fawx serve | head -1`) terminates cleanly instead of producing
/// ugly error messages.
fn install_sigpipe_handler() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn extract_response_text(result: &LoopResult) -> String {
    match result {
        LoopResult::Complete { response, .. } => response.clone(),
        LoopResult::BudgetExhausted {
            partial_response, ..
        } => partial_response.clone().unwrap_or_default(),
        LoopResult::NeedsInput { prompt, .. } => prompt.clone(),
        LoopResult::UserStopped {
            partial_response, ..
        } => partial_response.clone().unwrap_or_default(),
        LoopResult::Error { message, .. } => format!("error: {message}"),
    }
}

fn extract_iterations(result: &LoopResult) -> u32 {
    match result {
        LoopResult::Complete { iterations, .. }
        | LoopResult::BudgetExhausted { iterations, .. }
        | LoopResult::NeedsInput { iterations, .. }
        | LoopResult::UserStopped { iterations, .. } => *iterations,
        LoopResult::Error { .. } => 0,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
    use fx_kernel::budget::{BudgetConfig, BudgetTracker};
    use fx_kernel::cancellation::CancellationToken;
    use fx_kernel::context_manager::ContextCompactor;
    use fx_kernel::loop_engine::LoopEngine;
    use std::sync::Arc;

    // ── Test helpers ────────────────────────────────────────────────────

    /// Stub tool executor that rejects all calls (no tools in headless tests).
    #[derive(Debug)]
    struct StubToolExecutor;

    #[async_trait]
    impl ToolExecutor for StubToolExecutor {
        async fn execute_tools(
            &self,
            _calls: &[fx_llm::ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(Vec::new())
        }
    }

    fn test_engine() -> LoopEngine {
        LoopEngine::builder()
            .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
            .context(ContextCompactor::new(2048, 256))
            .max_iterations(3)
            .tool_executor(Arc::new(StubToolExecutor))
            .synthesis_instruction("Summarize".to_string())
            .build()
            .expect("test engine")
    }

    fn test_app() -> HeadlessApp {
        HeadlessApp {
            loop_engine: test_engine(),
            router: ModelRouter::new(),
            config: FawxConfig::default(),
            memory: None,
            active_model: "mock-model".to_string(),
            conversation_history: Vec::new(),
            max_history: 20,
            custom_system_prompt: None,
        }
    }

    // ── Unit tests (10) ─────────────────────────────────────────────────

    #[test]
    fn quit_commands_recognized() {
        assert!(is_quit_command("/quit"));
        assert!(is_quit_command("/exit"));
        assert!(!is_quit_command("hello"));
        assert!(!is_quit_command("/help"));
    }

    #[test]
    fn empty_input_not_treated_as_quit() {
        assert!(!is_quit_command(""));
    }

    #[test]
    fn json_input_parses_message() {
        let app = test_app();
        let result = app.parse_json_input(r#"{"message": "hello world"}"#);
        assert_eq!(result.unwrap(), "hello world");
    }

    #[test]
    fn json_input_rejects_invalid() {
        let app = test_app();
        assert!(app.parse_json_input("not json").is_err());
    }

    #[test]
    fn json_output_serializes_correctly() {
        let output = JsonOutput {
            response: "hello".to_string(),
            model: "gpt-4".to_string(),
            iterations: 2,
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&output).unwrap()).unwrap();
        assert_eq!(json["response"], "hello");
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["iterations"], 2);
    }

    #[test]
    fn extract_response_from_complete() {
        let result = LoopResult::Complete {
            response: "done".to_string(),
            iterations: 1,
            tokens_used: fx_kernel::act::TokenUsage::default(),
            learnings: Vec::new(),
            signals: Vec::new(),
        };
        assert_eq!(extract_response_text(&result), "done");
        assert_eq!(extract_iterations(&result), 1);
    }

    #[test]
    fn extract_response_from_error() {
        let result = LoopResult::Error {
            message: "boom".to_string(),
            recoverable: false,
            signals: Vec::new(),
        };
        assert_eq!(extract_response_text(&result), "error: boom");
        assert_eq!(extract_iterations(&result), 0);
    }

    #[test]
    fn extract_response_from_budget_exhausted() {
        let result = LoopResult::BudgetExhausted {
            partial_response: Some("partial".to_string()),
            iterations: 3,
            signals: Vec::new(),
        };
        assert_eq!(extract_response_text(&result), "partial");
        assert_eq!(extract_iterations(&result), 3);
    }

    #[test]
    fn perception_snapshot_has_correct_app_id() {
        let app = test_app();
        let snap = app.build_perception_snapshot("hi");
        assert_eq!(snap.active_app, "fawx.headless");
        assert_eq!(snap.screen.current_app, "fawx.headless");
        assert_eq!(
            snap.user_input.as_ref().map(|u| u.text.as_str()),
            Some("hi")
        );
    }

    #[test]
    fn conversation_history_trimmed() {
        let mut app = test_app();
        app.max_history = 4;
        for i in 0..10 {
            app.record_turn(&format!("q{i}"), &format!("a{i}"));
        }
        // max_history = 4 means 4 messages retained (2 turns)
        assert_eq!(app.conversation_history.len(), 4);
    }

    #[test]
    fn system_prompt_missing_file_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nonexistent_prompt.md");
        assert!(load_system_prompt(Some(&missing)).is_none());
    }

    #[test]
    fn system_prompt_loads_from_explicit_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("custom_prompt.md");
        std::fs::write(&path, "You are a helpful assistant.").expect("write");
        let prompt = load_system_prompt(Some(&path));
        assert_eq!(prompt.as_deref(), Some("You are a helpful assistant."));
    }

    #[test]
    fn system_prompt_empty_file_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty_prompt.md");
        std::fs::write(&path, "   \n  ").expect("write");
        assert!(load_system_prompt(Some(&path)).is_none());
    }
}
