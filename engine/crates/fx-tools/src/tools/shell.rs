use super::{
    canonicalize_existing_or_parent, parse_args, to_tool_result, validate_path, ToolRegistry,
};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_kernel::act::{JournalAction, ToolCacheability, ToolCallClassification, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::ToolAuthoritySurface;
use fx_llm::{ToolCall, ToolDefinition};
use fx_ripcord::git_guard::{check_push_allowed, extract_push_targets};
use serde::Deserialize;
use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::CharIndices;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(RunCommandTool::new(context));
}

struct RunCommandTool {
    context: Arc<ToolContext>,
}

impl RunCommandTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
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
            description: "Run a command and capture exit code, stdout, and stderr".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "working_dir": { "type": "string" },
                    "shell": { "type": "boolean" }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_run_command(&call.arguments).await,
        )
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
        classify_run_command_call(&call.arguments)
    }

    fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
        let command = call.arguments.get("command")?.as_str()?.to_string();
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

#[derive(Deserialize)]
struct RunCommandArgs {
    command: String,
    working_dir: Option<String>,
    shell: Option<bool>,
}

impl ToolContext {
    pub(crate) async fn handle_run_command(
        &self,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        let parsed: RunCommandArgs = parse_args(args)?;
        let command = parsed.command.trim();
        if command.is_empty() {
            return Err("command cannot be empty".to_string());
        }
        let working_dir = self.resolve_command_dir(parsed.working_dir.as_deref())?;
        self.guard_push_command(command)?;
        let child = build_command(command, parsed.shell.unwrap_or(false), &working_dir)?
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        let output = wait_with_timeout(child, self.config.command_timeout).await?;
        Ok(format_command_output(output, parsed.shell.unwrap_or(false)))
    }

    pub(crate) fn guard_push_command(&self, command: &str) -> Result<(), String> {
        let targets = extract_push_targets(command);
        if targets.is_empty() {
            return Ok(());
        }
        check_push_allowed(&targets, &self.protected_branches)
    }

    pub(crate) fn resolve_command_dir(&self, requested: Option<&str>) -> Result<PathBuf, String> {
        let desired = requested.unwrap_or_else(|| self.working_dir.to_str().unwrap_or("."));
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(desired));
        }
        validate_path(&self.working_dir, desired)
    }
}

pub(super) fn classify_run_command_call(args: &serde_json::Value) -> ToolCallClassification {
    let Ok(parsed): Result<RunCommandArgs, _> = parse_args(args) else {
        return ToolCallClassification::Mutation;
    };
    if is_observational_command(parsed.command.trim(), parsed.shell.unwrap_or(false)) {
        ToolCallClassification::Observation
    } else {
        ToolCallClassification::Mutation
    }
}

fn build_command(command: &str, shell: bool, working_dir: &Path) -> Result<Command, String> {
    if shell {
        let mut built = Command::new("/bin/sh");
        built.kill_on_drop(true);
        built.arg("-c").arg(command).current_dir(working_dir);
        return Ok(built);
    }
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| "command cannot be empty".to_string())?;
    let mut built = Command::new(program);
    built.kill_on_drop(true);
    built.args(parts).current_dir(working_dir);
    Ok(built)
}

async fn wait_with_timeout(
    child: tokio::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let waited = tokio::time::timeout(timeout, child.wait_with_output()).await;
    match waited {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(_) => Err("command timed out".to_string()),
    }
}

fn format_command_output(output: std::process::Output, shell: bool) -> String {
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

fn is_observational_command(command: &str, shell: bool) -> bool {
    if command.is_empty() {
        return false;
    }
    if contains_mutating_shell_syntax(command) {
        return false;
    }
    if shell {
        return shell_segments(command)
            .into_iter()
            .all(is_observational_shell_segment);
    }
    is_observational_program_and_args(
        &command
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>(),
    )
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

fn is_observational_shell_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return true;
    }
    let tokens: Vec<String> = segment.split_whitespace().map(str::to_string).collect();
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

fn is_observational_cargo_command(args: &[String]) -> bool {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return false;
    };
    matches!(
        subcommand,
        "metadata" | "tree" | "locate-project" | "help" | "search" | "version"
    )
}
