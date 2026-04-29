use super::{
    canonicalize_existing_or_parent, parse_args, tool_failure_from_io, validate_path, ToolFailure,
    ToolRegistry,
};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_core::command_text::{
    tokenize_non_shell_command as shared_tokenize_non_shell_command, CommandTokenizationError,
};
use fx_kernel::act::{
    FailureClass, JournalAction, RunCommandDiagnostics, ToolCacheability, ToolCallClassification,
    ToolExecutionDiagnostics, ToolResult,
};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::ToolAuthoritySurface;
use fx_llm::{ToolCall, ToolDefinition};
use fx_ripcord::git_guard::{check_push_allowed, extract_push_targets_from_tokens};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{iter::Peekable, str::CharIndices};
use tokio::process::Command;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(RunCommandTool::new(context));
}

struct RunCommandTool {
    context: Arc<ToolContext>,
    diagnostics: Arc<Mutex<HashMap<String, ToolExecutionDiagnostics>>>,
}

impl RunCommandTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn store_execution_diagnostics(
        &self,
        call_id: &str,
        diagnostics: Option<ToolExecutionDiagnostics>,
    ) {
        let mut guard = self
            .diagnostics
            .lock()
            .expect("run_command diagnostics lock");
        if let Some(diagnostics) = diagnostics {
            guard.insert(call_id.to_string(), diagnostics);
        } else {
            guard.remove(call_id);
        }
    }
}

#[async_trait]
impl Tool for RunCommandTool {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Run a local command and capture exit code, stdout, and stderr. Use argv for an exact non-shell program/argument invocation. Use command with shell=true to execute a shell string via /bin/sh -c. Use shell=false to force exact non-shell parsing of command. If shell is omitted, the runtime auto-detects whether command needs a shell: pipes, redirection, and similar shell operators run through /bin/sh -c, while plain commands are tokenized with strict quote-aware parsing. If both command and argv are supplied, the runtime chooses the safer unambiguous form instead of failing the turn. The tool succeeds only when the command exits with code 0; any non-zero exit returns a failed tool result with the captured output.".to_string(),
            // Anthropic rejects top-level allOf/oneOf/anyOf in tool schemas, so the
            // exact either-or contract is enforced by parse_run_command_invocation_from_args.
            parameters: serde_json::json!({
                "type": "object",
                "description": "Prefer either command or argv. When both are supplied, command is used for shell=true and argv is used otherwise. When shell is omitted, command auto-detects shell syntax while argv always stays exact non-shell.",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command text. With shell=true it runs via /bin/sh -c. With shell=false it is parsed as an exact non-shell command string. When shell is omitted, the runtime auto-detects whether shell syntax is required."
                    },
                    "argv": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "description": "Exact program and arguments for non-shell execution. argv[0] is the program name and argument boundaries are preserved exactly."
                    },
                    "working_dir": { "type": "string" },
                    "shell": {
                        "type": "boolean",
                        "description": "Optional execution mode override. true forces /bin/sh -c. false forces exact non-shell execution. When omitted, argv stays non-shell and command auto-detects whether shell syntax such as pipes or redirection is present."
                    }
                }
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        match self.context.execute_run_command(&call.arguments).await {
            Ok(execution) => {
                self.store_execution_diagnostics(&call.id, Some(execution.diagnostics));
                ToolResult::success(&call.id, self.name(), execution.output)
            }
            Err(error) => {
                self.store_execution_diagnostics(&call.id, error.diagnostics().cloned());
                ToolResult::failure(&call.id, self.name(), error.message, error.class)
            }
        }
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
        classify_run_command_call(&call.arguments)
    }

    fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
        let command = parse_run_command_invocation(&call.arguments)
            .ok()?
            .display_command()
            .to_string();
        Some(JournalAction::ShellCommand {
            command,
            exit_code: shell_exit_code(&result.output, result.success),
        })
    }

    fn action_category(&self) -> &'static str {
        "code_execute"
    }

    fn authority_surface(&self, _call: &ToolCall) -> ToolAuthoritySurface {
        ToolAuthoritySurface::Command
    }

    fn take_execution_diagnostics(&self, call_id: &str) -> Option<ToolExecutionDiagnostics> {
        self.diagnostics
            .lock()
            .expect("run_command diagnostics lock")
            .remove(call_id)
    }
}

fn shell_exit_code(output: &str, success: bool) -> i32 {
    match output
        .lines()
        .find_map(|line| line.strip_prefix("exit_code: "))
    {
        Some(value) => value.trim().parse().unwrap_or(if success { 0 } else { -1 }),
        None => {
            if success {
                0
            } else {
                -1
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum RunCommandShellMode {
    #[default]
    Auto,
    Enabled,
    Disabled,
}

impl<'de> Deserialize<'de> for RunCommandShellMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let enabled = bool::deserialize(deserializer)?;
        Ok(if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        })
    }
}

impl RunCommandShellMode {
    fn diagnostics_shell_flag(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Clone, Deserialize)]
struct RunCommandArgs {
    command: Option<String>,
    argv: Option<Vec<String>>,
    working_dir: Option<String>,
    #[serde(default)]
    shell: RunCommandShellMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RunCommandInvocation {
    Shell {
        command: String,
    },
    NonShell {
        display_command: String,
        argv: Vec<String>,
    },
}

impl RunCommandInvocation {
    fn shell(&self) -> bool {
        matches!(self, Self::Shell { .. })
    }

    fn display_command(&self) -> &str {
        match self {
            Self::Shell { command } => command,
            Self::NonShell {
                display_command, ..
            } => display_command,
        }
    }
}

struct RunCommandExecution {
    output: String,
    diagnostics: ToolExecutionDiagnostics,
}

impl ToolContext {
    async fn execute_run_command(
        &self,
        args: &serde_json::Value,
    ) -> Result<RunCommandExecution, ToolFailure> {
        let started_at = Instant::now();
        let parsed: RunCommandArgs = parse_args(args).map_err(|error| {
            attach_run_command_diagnostics(
                ToolFailure::permanent(error),
                started_at,
                RunCommandShellMode::Auto.diagnostics_shell_flag(),
                None,
                false,
            )
        })?;
        let shell = parsed.shell.diagnostics_shell_flag();
        let requested_working_dir = parsed.working_dir.clone();
        let invocation =
            parse_run_command_invocation_from_args(parsed.clone()).map_err(|error| {
                attach_run_command_diagnostics(error, started_at, shell, None, false)
            })?;
        let working_dir = self
            .resolve_command_dir(requested_working_dir.as_deref())
            .map_err(|error| {
                attach_run_command_diagnostics(error, started_at, invocation.shell(), None, false)
            })?;
        self.guard_push_invocation(&invocation).map_err(|error| {
            attach_run_command_diagnostics(error, started_at, invocation.shell(), None, false)
        })?;
        let child = build_command(&invocation, &working_dir)
            .map_err(|error| {
                attach_run_command_diagnostics(error, started_at, invocation.shell(), None, false)
            })?
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                attach_run_command_diagnostics(
                    tool_failure_from_io(error),
                    started_at,
                    invocation.shell(),
                    None,
                    false,
                )
            })?;
        let output = wait_with_timeout(child, self.config.command_timeout)
            .await
            .map_err(|error| {
                attach_run_command_diagnostics(error, started_at, invocation.shell(), None, true)
            })?;
        let invocation_note = discarded_run_command_field_note(&parsed, &invocation);
        let formatted = format_command_output(&output, invocation.shell());
        let failure_output = format_command_output_with_invocation_note(
            &output,
            invocation.shell(),
            invocation_note.as_deref(),
        );
        if output.status.success() {
            Ok(RunCommandExecution {
                diagnostics: run_command_success_diagnostics(
                    started_at,
                    invocation.shell(),
                    invocation.display_command(),
                    &formatted,
                    &output,
                ),
                output: formatted,
            })
        } else {
            Err(
                ToolFailure::new(failure_output, classify_command_exit(&output)).with_diagnostics(
                    run_command_failure_diagnostics(
                        started_at,
                        invocation.shell(),
                        Some(&output),
                        false,
                        None,
                    ),
                ),
            )
        }
    }

    pub(crate) fn guard_push_command(&self, command: &str) -> Result<(), ToolFailure> {
        let targets = extract_push_targets_from_shell_command(command);
        if targets.is_empty() {
            return Ok(());
        }
        check_push_allowed(&targets, &self.protected_branches).map_err(ToolFailure::permanent)
    }

    fn guard_push_invocation(&self, invocation: &RunCommandInvocation) -> Result<(), ToolFailure> {
        let targets = match invocation {
            RunCommandInvocation::Shell { command } => {
                extract_push_targets_from_shell_command(command)
            }
            RunCommandInvocation::NonShell { argv, .. } => extract_push_targets_from_tokens(argv),
        };
        if targets.is_empty() {
            return Ok(());
        }
        check_push_allowed(&targets, &self.protected_branches).map_err(ToolFailure::permanent)
    }

    pub(crate) fn resolve_command_dir(
        &self,
        requested: Option<&str>,
    ) -> Result<PathBuf, ToolFailure> {
        let working_dir = self.working_dir();
        let desired = requested.unwrap_or_else(|| working_dir.to_str().unwrap_or("."));
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(desired));
        }
        validate_path(&working_dir, desired)
    }
}

pub(super) fn classify_run_command_call(args: &serde_json::Value) -> ToolCallClassification {
    let Ok(invocation) = parse_run_command_invocation(args) else {
        return ToolCallClassification::Mutation;
    };
    match invocation {
        RunCommandInvocation::Shell { command } => {
            if is_observational_command(&command, true) {
                ToolCallClassification::Observation
            } else {
                ToolCallClassification::Mutation
            }
        }
        RunCommandInvocation::NonShell { argv, .. } => {
            if is_observational_program_and_args(&argv) {
                ToolCallClassification::Observation
            } else {
                ToolCallClassification::Mutation
            }
        }
    }
}

fn build_command(
    invocation: &RunCommandInvocation,
    working_dir: &Path,
) -> Result<Command, ToolFailure> {
    match invocation {
        RunCommandInvocation::Shell { command } => {
            let mut built = Command::new("/bin/sh");
            built.kill_on_drop(true);
            built.arg("-c").arg(command).current_dir(working_dir);
            Ok(built)
        }
        RunCommandInvocation::NonShell { argv, .. } => {
            let (program, args) = argv
                .split_first()
                .expect("non-shell argv validated during invocation parsing");
            let mut built = Command::new(program);
            built.kill_on_drop(true);
            built.args(args).current_dir(working_dir);
            Ok(built)
        }
    }
}

async fn wait_with_timeout(
    child: tokio::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, ToolFailure> {
    let waited = tokio::time::timeout(timeout, child.wait_with_output()).await;
    match waited {
        Ok(result) => result.map_err(tool_failure_from_io),
        Err(_) => Err(ToolFailure::transient("command timed out")),
    }
}

fn run_command_failure_diagnostics(
    started_at: Instant,
    shell: bool,
    output: Option<&std::process::Output>,
    timed_out: bool,
    fallback_stderr: Option<&str>,
) -> ToolExecutionDiagnostics {
    ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
        exit_code: output.and_then(|process_output| process_output.status.code()),
        stderr_snippet: output
            .and_then(|process_output| stderr_snippet(&process_output.stderr))
            .or_else(|| fallback_stderr.and_then(stderr_snippet_from_text)),
        duration_ms: elapsed_ms(started_at),
        shell,
        timed_out,
        external_actions: Vec::new(),
    })
}

fn run_command_success_diagnostics(
    started_at: Instant,
    shell: bool,
    command: &str,
    formatted_output: &str,
    output: &std::process::Output,
) -> ToolExecutionDiagnostics {
    ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
        exit_code: output.status.code(),
        stderr_snippet: stderr_snippet(&output.stderr),
        duration_ms: elapsed_ms(started_at),
        shell,
        timed_out: false,
        external_actions: fx_kernel::act::external_actions_from_run_command(
            command,
            formatted_output,
        ),
    })
}

fn attach_run_command_diagnostics(
    error: ToolFailure,
    started_at: Instant,
    shell: bool,
    output: Option<&std::process::Output>,
    timed_out: bool,
) -> ToolFailure {
    let fallback_stderr = if output.is_none() {
        Some(error.message.clone())
    } else {
        None
    };
    error.with_diagnostics(run_command_failure_diagnostics(
        started_at,
        shell,
        output,
        timed_out,
        fallback_stderr.as_deref(),
    ))
}

fn stderr_snippet(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    stderr_snippet_from_text(text.as_ref())
}

fn stderr_snippet_from_text(text: &str) -> Option<String> {
    let snippet = text.trim();
    if snippet.is_empty() {
        None
    } else {
        Some(truncate_snippet(snippet, 240))
    }
}

fn truncate_snippet(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    let prefix = text.chars().take(max_chars).collect::<String>();
    format!("{prefix}...")
}

fn elapsed_ms(started_at: Instant) -> u64 {
    let millis = started_at.elapsed().as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

fn format_command_output(output: &std::process::Output, shell: bool) -> String {
    let mut lines = vec![format!("exit_code: {}", output.status.code().unwrap_or(-1))];
    if shell {
        lines.push("warning: command executed via shell=true".to_string());
    }
    lines.push(format!(
        "stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    ));
    lines.push(format!(
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    ));
    lines.join("\n")
}

fn format_command_output_with_invocation_note(
    output: &std::process::Output,
    shell: bool,
    invocation_note: Option<&str>,
) -> String {
    let formatted = format_command_output(output, shell);
    match invocation_note {
        Some(note) => format!("[run_command note: {note}]\n{formatted}"),
        None => formatted,
    }
}

fn classify_command_exit(output: &std::process::Output) -> FailureClass {
    match output.status.code() {
        Some(126 | 127) => FailureClass::Permanent,
        Some(_) => FailureClass::Unknown,
        None => FailureClass::Transient,
    }
}

fn is_observational_command(command: &str, shell: bool) -> bool {
    if command.is_empty() {
        return false;
    }
    if shell {
        if contains_mutating_shell_syntax(command) {
            return false;
        }
        return shell_segments(command)
            .into_iter()
            .all(is_observational_shell_segment);
    }
    match tokenize_non_shell_command(command) {
        Ok(tokens) => is_observational_program_and_args(&tokens),
        Err(_) => false,
    }
}

fn contains_mutating_shell_syntax(command: &str) -> bool {
    let normalized = strip_quoted_shell_strings(command).replace("\\>", "");
    normalized.contains(">>")
        || normalized.contains('>')
        || normalized.contains("<<")
        || normalized.contains("| tee")
        || normalized.contains("|tee")
}

fn strip_quoted_shell_strings(command: &str) -> String {
    let mut stripped = String::with_capacity(command.len());
    let mut chars = command.chars().peekable();
    let mut active_quote = None;
    while let Some(ch) = chars.next() {
        match active_quote {
            Some('\'') => {
                if ch == '\'' {
                    active_quote = None;
                }
            }
            Some('"') => {
                if ch == '\\' {
                    let _ = chars.next();
                } else if ch == '"' {
                    active_quote = None;
                }
            }
            Some('`') => {
                if ch == '`' {
                    active_quote = None;
                }
            }
            Some(_) => {}
            None => match ch {
                '\'' | '"' | '`' => active_quote = Some(ch),
                _ => stripped.push(ch),
            },
        }
    }
    stripped
}

type IndexedChars<'a> = Peekable<CharIndices<'a>>;

fn shell_segments(command: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut chars = command.char_indices().peekable();
    let mut active_quote = None;
    while let Some((index, ch)) = chars.next() {
        if advance_quote_state(&mut chars, ch, &mut active_quote) {
            continue;
        }
        if matches!(ch, '\'' | '"' | '`') {
            active_quote = Some(ch);
            continue;
        }
        let Some(separator_len) = separator_len(&mut chars, ch) else {
            continue;
        };
        push_shell_segment(&mut segments, command, start, index);
        start = index + separator_len;
    }
    push_shell_segment(&mut segments, command, start, command.len());
    segments
}

fn advance_quote_state(
    chars: &mut IndexedChars<'_>,
    ch: char,
    active_quote: &mut Option<char>,
) -> bool {
    match active_quote {
        Some('\'') => {
            if ch == '\'' {
                *active_quote = None;
            }
            true
        }
        Some('"') => {
            if ch == '\\' {
                let _ = chars.next();
            } else if ch == '"' {
                *active_quote = None;
            }
            true
        }
        Some('`') => {
            if ch == '`' {
                *active_quote = None;
            }
            true
        }
        Some(_) => true,
        None => false,
    }
}

fn separator_len(chars: &mut IndexedChars<'_>, ch: char) -> Option<usize> {
    match ch {
        '\n' | ';' => Some(1),
        '|' => {
            if matches!(chars.peek(), Some((_, '|'))) {
                let _ = chars.next();
                Some(2)
            } else {
                Some(1)
            }
        }
        '&' if matches!(chars.peek(), Some((_, '&'))) => {
            let _ = chars.next();
            Some(2)
        }
        _ => None,
    }
}

fn push_shell_segment<'a>(segments: &mut Vec<&'a str>, command: &'a str, start: usize, end: usize) {
    let segment = command[start..end].trim();
    if !segment.is_empty() {
        segments.push(segment);
    }
}

fn command_requires_shell(command: &str) -> bool {
    shell_segments(command).len() > 1 || contains_shell_redirection(command)
}

fn contains_shell_redirection(command: &str) -> bool {
    let normalized = strip_quoted_shell_strings(command).replace("\\>", "");
    normalized.contains(">>")
        || normalized.contains('>')
        || normalized.contains("<<")
        || normalized.contains('<')
}

fn is_observational_shell_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return true;
    }
    // Shell segments may contain expansions or subshell syntax. For classification
    // we intentionally ignore expansion semantics and only recover the literal
    // program/argument shape well enough to decide whether the command is read-only.
    let Ok(tokens) = tokenize_non_shell_command(segment) else {
        return false;
    };
    if tokens.is_empty() {
        return true;
    }
    if tokens[0] == "cd" {
        return tokens.len() <= 2;
    }
    is_observational_program_and_args(&tokens)
}

fn is_observational_program_and_args(tokens: &[String]) -> bool {
    let mut index = 0;
    while index < tokens.len() && looks_like_env_assignment(&tokens[index]) {
        index += 1;
    }
    if index >= tokens.len() {
        return false;
    }
    let program = tokens[index].as_str();
    let args = &tokens[index + 1..];
    match program {
        "cat" | "grep" | "rg" | "head" | "tail" | "ls" | "find" | "pwd" | "wc" | "which"
        | "stat" | "file" | "cut" | "sort" | "uniq" | "jq" | "awk" | "realpath" | "dirname"
        | "basename" | "printenv" | "env" | "uname" | "date" | "tree" | "df" | "du" | "id"
        | "whoami" | "hostname" | "lsof" | "ps" => true,
        "top" => args
            .iter()
            .any(|arg| arg == "-b" || arg == "-l" || arg.starts_with("-l")),
        "echo" => true,
        "sed" => !args.iter().any(|arg| arg == "-i" || arg.starts_with("-i")),
        "git" => is_observational_git_command(args),
        "gh" => is_observational_gh_command(args),
        "cargo" => is_observational_cargo_command(args),
        _ => false,
    }
}

fn looks_like_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn is_observational_git_command(args: &[String]) -> bool {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return false;
    };
    match subcommand {
        "status" | "diff" | "show" | "log" | "rev-parse" | "ls-files" | "grep" | "describe" => true,
        "branch" => args.len() == 1 || args.iter().skip(1).all(|arg| arg == "--list"),
        "remote" => args.len() == 1 || args.iter().skip(1).all(|arg| arg == "-v"),
        "config" => args
            .iter()
            .skip(1)
            .any(|arg| arg == "--get" || arg == "--get-all"),
        _ => false,
    }
}

fn is_observational_gh_command(args: &[String]) -> bool {
    let Some(resource) = args.first().map(String::as_str) else {
        return false;
    };
    let Some(subcommand) = args.get(1).map(String::as_str) else {
        return false;
    };
    match resource {
        "pr" => matches!(subcommand, "view" | "diff" | "list" | "status" | "checks"),
        "issue" => matches!(subcommand, "view" | "list" | "status"),
        "repo" => matches!(subcommand, "view" | "list"),
        "search" => matches!(subcommand, "prs" | "issues" | "repos" | "commits" | "code"),
        "auth" => subcommand == "status",
        _ => false,
    }
}

fn is_observational_cargo_command(args: &[String]) -> bool {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return false;
    };
    matches!(
        subcommand,
        "metadata" | "tree" | "locate-project" | "help" | "search" | "version"
    )
}

fn parse_run_command_invocation(
    args: &serde_json::Value,
) -> Result<RunCommandInvocation, ToolFailure> {
    let parsed: RunCommandArgs = parse_args(args).map_err(ToolFailure::permanent)?;
    parse_run_command_invocation_from_args(parsed)
}

fn parse_run_command_invocation_from_args(
    parsed: RunCommandArgs,
) -> Result<RunCommandInvocation, ToolFailure> {
    match (parsed.shell, parsed.command, parsed.argv) {
        (RunCommandShellMode::Enabled, Some(command), _argv) => {
            let command = command.trim();
            if command.is_empty() {
                return Err(ToolFailure::permanent("command cannot be empty"));
            }
            Ok(RunCommandInvocation::Shell {
                command: command.to_string(),
            })
        }
        (RunCommandShellMode::Enabled, None, Some(argv)) => {
            validate_argv(&argv)?;
            Ok(RunCommandInvocation::NonShell {
                display_command: format_argv_for_display(&argv),
                argv,
            })
        }
        (RunCommandShellMode::Enabled, None, None) => {
            Err(ToolFailure::permanent("command or argv is required"))
        }
        (RunCommandShellMode::Disabled, Some(_command), Some(argv))
        | (RunCommandShellMode::Auto, Some(_command), Some(argv)) => {
            validate_argv(&argv)?;
            Ok(RunCommandInvocation::NonShell {
                display_command: format_argv_for_display(&argv),
                argv,
            })
        }
        (RunCommandShellMode::Disabled, Some(command), None) => {
            let command = command.trim();
            if command.is_empty() {
                return Err(ToolFailure::permanent("command cannot be empty"));
            }
            let argv = tokenize_non_shell_command(command)?;
            validate_argv(&argv)?;
            Ok(RunCommandInvocation::NonShell {
                display_command: command.to_string(),
                argv,
            })
        }
        (RunCommandShellMode::Auto, Some(command), None) => {
            let command = command.trim();
            if command.is_empty() {
                return Err(ToolFailure::permanent("command cannot be empty"));
            }
            if command_requires_shell(command) {
                Ok(RunCommandInvocation::Shell {
                    command: command.to_string(),
                })
            } else {
                let argv = tokenize_non_shell_command(command)?;
                validate_argv(&argv)?;
                Ok(RunCommandInvocation::NonShell {
                    display_command: command.to_string(),
                    argv,
                })
            }
        }
        (RunCommandShellMode::Disabled, None, Some(argv))
        | (RunCommandShellMode::Auto, None, Some(argv)) => {
            validate_argv(&argv)?;
            Ok(RunCommandInvocation::NonShell {
                display_command: format_argv_for_display(&argv),
                argv,
            })
        }
        (RunCommandShellMode::Disabled, None, None) | (RunCommandShellMode::Auto, None, None) => {
            Err(ToolFailure::permanent("command or argv is required"))
        }
    }
}

fn discarded_run_command_field_note(
    args: &RunCommandArgs,
    invocation: &RunCommandInvocation,
) -> Option<String> {
    let command = args.command.as_deref()?.trim();
    let argv = args.argv.as_ref()?;
    if command.is_empty() || argv.is_empty() {
        return None;
    }
    let argv_display = format_argv_for_display(argv);

    match (args.shell, invocation) {
        (RunCommandShellMode::Enabled, RunCommandInvocation::Shell { .. }) => {
            (command != argv_display).then(|| {
                format!("argv field was ignored because shell=true uses command: {argv_display}")
            })
        }
        (
            RunCommandShellMode::Disabled | RunCommandShellMode::Auto,
            RunCommandInvocation::NonShell { .. },
        ) => (command != argv_display).then(|| {
            format!("command field was ignored because argv was also supplied: {command}")
        }),
        _ => None,
    }
}

fn validate_argv(argv: &[String]) -> Result<(), ToolFailure> {
    if argv.is_empty() {
        return Err(ToolFailure::permanent("argv cannot be empty"));
    }
    if argv[0].trim().is_empty() {
        return Err(ToolFailure::permanent("argv[0] cannot be empty"));
    }
    Ok(())
}

fn extract_push_targets_from_shell_command(command: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for segment in shell_segments(command) {
        let Ok(tokens) = tokenize_non_shell_command(segment) else {
            continue;
        };
        for target in extract_push_targets_from_tokens(&tokens) {
            if !targets.iter().any(|existing| existing == &target) {
                targets.push(target);
            }
        }
    }
    targets
}

fn format_argv_for_display(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| format_display_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format argv for journaling/display only.
/// This uses JSON-style escaping for readability and must not be treated as a
/// shell-escaped command line for re-execution or copy-paste.
fn format_display_arg(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':'))
    {
        arg.to_string()
    } else {
        serde_json::to_string(arg).unwrap_or_else(|_| "\"<invalid utf8>\"".to_string())
    }
}

fn tokenize_non_shell_command(command: &str) -> Result<Vec<String>, ToolFailure> {
    shared_tokenize_non_shell_command(command).map_err(command_tokenization_failure)
}

fn command_tokenization_failure(error: CommandTokenizationError) -> ToolFailure {
    ToolFailure::permanent(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_non_shell_command_preserves_quoted_arguments() {
        let tokens = tokenize_non_shell_command(r#"open -a "Google Chrome" --new"#)
            .expect("quoted command should parse");

        assert_eq!(
            tokens,
            vec![
                "open".to_string(),
                "-a".to_string(),
                "Google Chrome".to_string(),
                "--new".to_string(),
            ]
        );
    }

    #[test]
    fn tokenize_non_shell_command_rejects_unmatched_quotes() {
        let error = tokenize_non_shell_command(r#"open -a "Google Chrome"#)
            .expect_err("unterminated quote should fail");

        assert_eq!(error.class, FailureClass::Permanent);
        assert!(error.message.contains("unmatched double quote"));
    }

    #[test]
    fn tokenize_non_shell_command_preserves_single_quoted_arguments() {
        let tokens = tokenize_non_shell_command("open -a 'Google Chrome' --new")
            .expect("single-quoted command should parse");

        assert_eq!(
            tokens,
            vec![
                "open".to_string(),
                "-a".to_string(),
                "Google Chrome".to_string(),
                "--new".to_string(),
            ]
        );
    }

    #[test]
    fn tokenize_non_shell_command_allows_single_quotes_inside_double_quotes() {
        let tokens = tokenize_non_shell_command(r#"open -a "it's here" --new"#)
            .expect("mixed quotes should parse");

        assert_eq!(
            tokens,
            vec![
                "open".to_string(),
                "-a".to_string(),
                "it's here".to_string(),
                "--new".to_string(),
            ]
        );
    }

    #[test]
    fn tokenize_non_shell_command_handles_posix_single_quote_splicing() {
        let tokens = tokenize_non_shell_command(r#"open -a 'it'\''s here' --new"#)
            .expect("single-quote splice should parse");

        assert_eq!(
            tokens,
            vec![
                "open".to_string(),
                "-a".to_string(),
                "it's here".to_string(),
                "--new".to_string(),
            ]
        );
    }

    #[test]
    fn classify_non_shell_command_ignores_shell_redirect_literals() {
        let classification =
            classify_run_command_call(&serde_json::json!({"command": "echo '>' notes.txt"}));

        assert_eq!(classification, ToolCallClassification::Observation);
    }

    #[test]
    fn parse_run_command_invocation_prefers_argv_for_ambiguous_non_shell_shape() {
        let invocation = parse_run_command_invocation(&serde_json::json!({
            "command": "echo hi",
            "argv": ["echo", "hi"]
        }))
        .expect("ambiguous non-shell shape should use argv");

        assert_eq!(
            invocation,
            RunCommandInvocation::NonShell {
                display_command: "echo hi".to_string(),
                argv: vec!["echo".to_string(), "hi".to_string()],
            }
        );
    }

    #[test]
    fn extract_push_targets_from_shell_command_preserves_quoted_repo_arguments() {
        let targets =
            extract_push_targets_from_shell_command(r#"git push --repo "/tmp/remote path" main"#);

        assert_eq!(targets, vec!["main".to_string()]);
    }

    #[test]
    fn parse_run_command_invocation_infers_shell_for_pipeline_commands() {
        let invocation = parse_run_command_invocation(&serde_json::json!({
            "command": "gh pr diff 1872 --repo fawxai/fawx 2>&1 | head -3000"
        }))
        .expect("pipeline command should infer shell");

        assert!(matches!(
            invocation,
            RunCommandInvocation::Shell { ref command }
            if command == "gh pr diff 1872 --repo fawxai/fawx 2>&1 | head -3000"
        ));
    }

    #[test]
    fn parse_run_command_invocation_infers_shell_for_redirection_commands() {
        let invocation = parse_run_command_invocation(&serde_json::json!({
            "command": "gh pr diff 1872 --repo fawxai/fawx > /tmp/pr1872.diff"
        }))
        .expect("redirect command should infer shell");

        assert!(matches!(
            invocation,
            RunCommandInvocation::Shell { ref command }
            if command == "gh pr diff 1872 --repo fawxai/fawx > /tmp/pr1872.diff"
        ));
    }

    #[test]
    fn parse_run_command_invocation_keeps_explicit_non_shell_pipeline_literal() {
        let invocation = parse_run_command_invocation(&serde_json::json!({
            "command": "gh pr diff 1872 --repo fawxai/fawx | head -3000",
            "shell": false
        }))
        .expect("explicit non-shell command should not infer shell");

        assert!(matches!(
            invocation,
            RunCommandInvocation::NonShell { ref argv, .. }
            if argv.iter().any(|arg| arg == "|")
        ));
    }

    #[test]
    fn ambiguous_invocation_note_formats_only_failed_output() {
        let args = RunCommandArgs {
            command: Some("echo ignored".to_string()),
            argv: Some(vec!["false".to_string()]),
            working_dir: None,
            shell: RunCommandShellMode::Auto,
        };
        let invocation = parse_run_command_invocation_from_args(args.clone())
            .expect("ambiguous invocation should parse");
        let invocation_note = discarded_run_command_field_note(&args, &invocation);
        let output = std::process::Command::new("false")
            .output()
            .expect("run false");

        let success_output = format_command_output(&output, invocation.shell());
        let failure_output = format_command_output_with_invocation_note(
            &output,
            invocation.shell(),
            invocation_note.as_deref(),
        );

        assert!(!success_output.contains("[run_command note:"));
        assert!(failure_output.contains(
            "[run_command note: command field was ignored because argv was also supplied: echo ignored]"
        ));
    }

    #[test]
    fn ambiguous_invocation_note_suppressed_when_command_and_argv_match() {
        let args = RunCommandArgs {
            command: Some("echo hi".to_string()),
            argv: Some(vec!["echo".to_string(), "hi".to_string()]),
            working_dir: None,
            shell: RunCommandShellMode::Auto,
        };
        let invocation = parse_run_command_invocation_from_args(args.clone())
            .expect("ambiguous invocation should parse");

        assert_eq!(discarded_run_command_field_note(&args, &invocation), None);
    }

    #[test]
    fn command_requires_shell_ignores_quoted_metacharacters() {
        assert!(!command_requires_shell(r#"echo "a | b" '>'"#));
    }

    #[test]
    fn run_command_shell_mode_defaults_to_auto_when_omitted() {
        let parsed: RunCommandArgs =
            serde_json::from_value(serde_json::json!({ "command": "pwd" })).expect("parse");

        assert_eq!(parsed.shell, RunCommandShellMode::Auto);
    }
}
