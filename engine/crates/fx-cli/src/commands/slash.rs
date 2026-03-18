use anyhow::Result;
use fx_config::manager::ConfigManager;
use fx_config::{save_default_model, save_thinking_budget, FawxConfig, ThinkingBudget};
use fx_kernel::loop_engine::{LoopEngine, LoopStatus};
use fx_kernel::signals::{Signal, SignalCollector};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::helpers::thinking_config_for_active_model;

pub(crate) const DEFAULT_SYNTHESIS_INSTRUCTION: &str =
    "Use the tool output to directly answer the user's question. Be natural and specific — \
 don't dump raw tool output, but don't hide data either. Match your response format to what \
 the user asked for: if they asked for a specific format (e.g., a count, a timestamp, a \
 raw value), use exactly that format — do not reformat into a 'friendlier' version unless \
 explicitly asked. If they asked a simple question, give a simple answer. If they asked \
 for a listing or search results, present it cleanly formatted.";
pub(crate) const MAX_SYNTHESIS_INSTRUCTION_LENGTH: usize = 500;

pub trait CommandHost {
    fn supports_embedded_slash_commands(&self) -> bool {
        false
    }
    fn list_models(&self) -> String;
    fn set_active_model(&mut self, selector: &str) -> Result<String>;
    fn proposals(&self, selector: Option<&str>) -> Result<String>;
    fn approve(&self, selector: &str, force: bool) -> Result<String>;
    fn reject(&self, selector: &str) -> Result<String>;
    fn show_config(&self) -> Result<String>;
    fn init_config(&mut self) -> Result<String>;
    fn reload_config(&mut self) -> Result<String>;
    fn show_status(&self) -> String;
    fn show_budget_status(&self) -> String;
    fn show_signals_summary(&self) -> String;
    fn handle_thinking(&mut self, level: Option<&str>) -> Result<String>;
    fn show_history(&self) -> Result<String> {
        Ok("Conversation history is not available in this mode.".to_string())
    }
    fn new_conversation(&mut self) -> Result<String> {
        Ok("Starting a new conversation is not available in this mode.".to_string())
    }
    fn show_loop_status(&self) -> Result<String> {
        Ok("Loop status is not available in this mode.".to_string())
    }
    fn show_debug(&self) -> Result<String> {
        Ok("Signal debug output is not available in this mode.".to_string())
    }
    fn handle_synthesis(&mut self, _instruction: Option<&str>) -> Result<String> {
        Ok("Synthesis configuration is not available in this mode.".to_string())
    }
    fn handle_auth(
        &self,
        _subcommand: Option<&str>,
        _action: Option<&str>,
        _value: Option<&str>,
        _has_extra_args: bool,
    ) -> Result<String> {
        Ok("Authentication status is not available in this mode.".to_string())
    }
    fn handle_keys(
        &self,
        _subcommand: Option<&str>,
        _value: Option<&str>,
        _option: Option<&str>,
        _has_extra_args: bool,
    ) -> Result<String> {
        Ok("Signing key management is not available in this mode.".to_string())
    }
    fn handle_sign(&self, _target: Option<&str>, _has_extra_args: bool) -> Result<String> {
        Ok("WASM signing is not available in this mode.".to_string())
    }
}

pub struct CommandContext<'a, H: CommandHost> {
    pub app: &'a mut H,
}

pub struct CommandResult {
    pub response: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
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
    Proposals {
        id: Option<String>,
        has_extra_args: bool,
    },
    Approve {
        id: Option<String>,
        force: bool,
        has_extra_args: bool,
    },
    Reject {
        id: Option<String>,
        has_extra_args: bool,
    },
    Synthesis(Option<String>),
    Clear,
    New,
    History,
    Thinking(Option<String>),
    Config(Option<String>),
    Help,
    Quit,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImproveFlags {
    pub dry_run: bool,
    pub has_unknown_flag: Option<String>,
}

pub fn is_command_input(input: &str) -> bool {
    input.trim_start().starts_with('/')
}

pub fn parse_command(value: &str) -> ParsedCommand {
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
        "auth" => parse_auth_command(&mut parts),
        "keys" => parse_keys_command(&mut parts),
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
        "proposals" => parse_proposals_command(&mut parts),
        "approve" => parse_approve_command(&mut parts),
        "reject" => parse_reject_command(&mut parts),
        "synthesis" => parse_synthesis_command(input, command),
        "clear" | "cls" => ParsedCommand::Clear,
        "new" => ParsedCommand::New,
        "history" => ParsedCommand::History,
        "thinking" => ParsedCommand::Thinking(parts.next().map(ToString::to_string)),
        "config" => ParsedCommand::Config(parts.next().map(ToString::to_string)),
        "help" => ParsedCommand::Help,
        "quit" | "exit" => ParsedCommand::Quit,
        other => ParsedCommand::Unknown(other.to_string()),
    }
}

pub fn execute_command<H: CommandHost>(
    ctx: &mut CommandContext<'_, H>,
    command: &ParsedCommand,
) -> Option<Result<CommandResult>> {
    match command {
        ParsedCommand::Model(None) => Some(Ok(response(ctx.app.list_models()))),
        ParsedCommand::Model(Some(model)) => {
            Some(ctx.app.set_active_model(model).map(model_set_response))
        }
        ParsedCommand::Auth {
            subcommand,
            action,
            value,
            has_extra_args,
        } => execute_embedded_only(ctx.app, |app| {
            execute_auth(
                app,
                subcommand.as_deref(),
                action.as_deref(),
                value.as_deref(),
                *has_extra_args,
            )
        }),
        ParsedCommand::Keys {
            subcommand,
            value,
            option,
            has_extra_args,
        } => execute_embedded_only(ctx.app, |app| {
            execute_keys(
                app,
                subcommand.as_deref(),
                value.as_deref(),
                option.as_deref(),
                *has_extra_args,
            )
        }),
        ParsedCommand::Sign {
            target,
            has_extra_args,
        } => execute_embedded_only(ctx.app, |app| {
            app.handle_sign(target.as_deref(), *has_extra_args)
                .map(response)
        }),
        ParsedCommand::Budget => Some(Ok(response(ctx.app.show_budget_status()))),
        ParsedCommand::Loop => {
            execute_embedded_only(ctx.app, |app| app.show_loop_status().map(response))
        }
        ParsedCommand::Status => Some(Ok(response(ctx.app.show_status()))),
        ParsedCommand::Signals => Some(Ok(response(ctx.app.show_signals_summary()))),
        ParsedCommand::Debug => {
            execute_embedded_only(ctx.app, |app| app.show_debug().map(response))
        }
        ParsedCommand::Analyze => None,
        ParsedCommand::Improve(_) => None,
        ParsedCommand::Proposals { id, has_extra_args } => {
            Some(execute_proposals(ctx.app, id.as_deref(), *has_extra_args))
        }
        ParsedCommand::Approve {
            id,
            force,
            has_extra_args,
        } => Some(execute_approve(
            ctx.app,
            id.as_deref(),
            *force,
            *has_extra_args,
        )),
        ParsedCommand::Reject { id, has_extra_args } => {
            Some(execute_reject(ctx.app, id.as_deref(), *has_extra_args))
        }
        ParsedCommand::Synthesis(instruction) => execute_embedded_only(ctx.app, |app| {
            app.handle_synthesis(instruction.as_deref()).map(response)
        }),
        ParsedCommand::Clear => None,
        ParsedCommand::New => {
            execute_embedded_only(ctx.app, |app| app.new_conversation().map(response))
        }
        ParsedCommand::History => {
            execute_embedded_only(ctx.app, |app| app.show_history().map(response))
        }
        ParsedCommand::Thinking(level) => {
            Some(ctx.app.handle_thinking(level.as_deref()).map(response))
        }
        ParsedCommand::Config(action) => Some(execute_config(ctx.app, action.as_deref())),
        ParsedCommand::Help => Some(Ok(response(help_text().to_string()))),
        ParsedCommand::Quit => None,
        ParsedCommand::Unknown(command) => Some(Ok(response(unknown_command_message(command)))),
    }
}

pub fn client_only_command_message(command: &ParsedCommand) -> Option<String> {
    client_only_command_name(command)
        .map(|name| format!("/{name} is a client-side command (only available in the TUI)"))
}

pub(crate) fn persist_default_model(
    config: &mut FawxConfig,
    config_manager: Option<&Arc<Mutex<ConfigManager>>>,
    data_dir: &Path,
    resolved: &str,
) -> Result<()> {
    config.model.default_model = Some(resolved.to_string());
    if let Some(manager) = config_manager {
        let mut guard = manager
            .lock()
            .map_err(|error| anyhow::anyhow!("config manager lock poisoned: {error}"))?;
        return guard
            .set("model.default_model", resolved)
            .map_err(anyhow::Error::msg);
    }
    save_default_model(data_dir, resolved).map_err(anyhow::Error::msg)
}

pub(crate) fn persist_thinking_budget(
    config_manager: Option<&Arc<Mutex<ConfigManager>>>,
    data_dir: &Path,
    budget: ThinkingBudget,
) -> Result<()> {
    if let Some(manager) = config_manager {
        let mut guard = manager
            .lock()
            .map_err(|error| anyhow::anyhow!("config manager lock poisoned: {error}"))?;
        return guard
            .set("general.thinking", &budget.to_string())
            .map_err(anyhow::Error::msg);
    }
    save_thinking_budget(data_dir, budget).map_err(anyhow::Error::msg)
}

pub(crate) fn reload_runtime_config(
    config_manager: Option<&Arc<Mutex<ConfigManager>>>,
    config_path: &Path,
) -> Result<FawxConfig> {
    if let Some(manager) = config_manager {
        let mut guard = manager
            .lock()
            .map_err(|error| anyhow::anyhow!("config manager lock poisoned: {error}"))?;
        guard.reload().map_err(anyhow::Error::msg)?;
        return Ok(guard.config().clone());
    }

    let data_dir = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path has no parent directory"))?;
    FawxConfig::load(data_dir).map_err(anyhow::Error::msg)
}

pub(crate) fn init_default_config(data_dir: &Path) -> Result<String> {
    let created = FawxConfig::write_default(data_dir).map_err(anyhow::Error::msg)?;
    Ok(format!("Created default config at {}", created.display()))
}

pub(crate) fn config_reload_success_message(config_path: &Path) -> String {
    format!(
        "Configuration reloaded from {}",
        display_path_for_user(config_path)
    )
}

pub(crate) fn display_path_for_user(path: &Path) -> String {
    let Some(home_dir) = dirs::home_dir() else {
        return path.display().to_string();
    };
    match path.strip_prefix(home_dir) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".to_string(),
        Ok(relative) => format!("~/{}", relative.display()),
        Err(_) => path.display().to_string(),
    }
}

pub(crate) fn render_budget_text(status: LoopStatus) -> String {
    [
        "Budget usage:".to_string(),
        format!("  - LLM calls used: {}", status.llm_calls_used),
        format!("  - Tool calls used: {}", status.tool_invocations_used),
        format!("  - Tokens used: {}", status.tokens_used),
        format!("  - Cost used (cents): {}", status.cost_cents_used),
        format!("  - Tokens remaining: {}", status.remaining.tokens),
        format!("  - LLM calls remaining: {}", status.remaining.llm_calls),
    ]
    .join("\n")
}

pub(crate) fn render_loop_status(status: LoopStatus) -> String {
    [
        "Loop status:".to_string(),
        format!(
            "  - Iterations (last cycle): {}/{}",
            status.iteration_count, status.max_iterations
        ),
        format!("  - Tokens used (tracker): {}", status.tokens_used),
        format!("  - Tokens remaining: {}", status.remaining.tokens),
        format!("  - LLM calls remaining: {}", status.remaining.llm_calls),
        format!(
            "  - Tool calls remaining: {}",
            status.remaining.tool_invocations
        ),
        format!(
            "  - Wall time remaining (ms): {}",
            status.remaining.wall_time_ms
        ),
    ]
    .join("\n")
}

pub(crate) fn render_debug_dump(signals: &[Signal]) -> String {
    if signals.is_empty() {
        return "No signals from last turn.".to_string();
    }
    SignalCollector::from_signals(signals.to_vec()).debug_dump()
}

pub(crate) fn render_signals_summary(signals: &[Signal]) -> String {
    if signals.is_empty() {
        return "No signals from last turn.".to_string();
    }
    SignalCollector::from_signals(signals.to_vec())
        .summary()
        .to_string()
}

pub(crate) fn apply_thinking_budget(
    config: &mut FawxConfig,
    loop_engine: &mut LoopEngine,
    config_manager: Option<&Arc<Mutex<ConfigManager>>>,
    data_dir: &Path,
    model_id: &str,
    level: Option<&str>,
) -> Result<String> {
    let current = config.general.thinking.unwrap_or_default();
    let Some(level_str) = level else {
        return Ok(format!("Current thinking budget: {current}"));
    };
    let budget: ThinkingBudget = level_str
        .parse()
        .map_err(|error: String| anyhow::anyhow!(error))?;
    config.general.thinking = Some(budget);
    loop_engine.set_thinking_config(thinking_config_for_active_model(&budget, model_id));
    persist_thinking_budget(config_manager, data_dir, budget)?;
    Ok(format!("Thinking budget set to: {budget}"))
}

pub fn help_text() -> &'static str {
    concat!(
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
        "  /sign <skill>  Sign one WASM skill\n",
        "  /sign --all    Sign all installed WASM skills\n",
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
    )
}

fn parse_auth_command(parts: &mut std::str::SplitWhitespace<'_>) -> ParsedCommand {
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

fn parse_keys_command(parts: &mut std::str::SplitWhitespace<'_>) -> ParsedCommand {
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

fn parse_synthesis_command(input: &str, command: &str) -> ParsedCommand {
    let remainder = input[command.len()..].strip_prefix(' ');
    match remainder {
        None => ParsedCommand::Synthesis(None),
        Some(raw) if raw.trim().is_empty() => ParsedCommand::Synthesis(Some(String::new())),
        Some(raw) => ParsedCommand::Synthesis(Some(raw.trim().to_string())),
    }
}

fn response(text: String) -> CommandResult {
    CommandResult { response: text }
}

fn model_set_response(model: String) -> CommandResult {
    response(format!("Active model set to: {model}"))
}

fn execute_proposals<H: CommandHost>(
    app: &mut H,
    id: Option<&str>,
    has_extra_args: bool,
) -> Result<CommandResult> {
    match has_extra_args {
        false => app.proposals(id).map(response),
        true => Ok(response("Usage: /proposals [id]".to_string())),
    }
}

fn execute_approve<H: CommandHost>(
    app: &mut H,
    id: Option<&str>,
    force: bool,
    has_extra_args: bool,
) -> Result<CommandResult> {
    match (id, has_extra_args) {
        (Some(selector), false) => app.approve(selector, force).map(response),
        _ => Ok(response("Usage: /approve <id> [--force]".to_string())),
    }
}

fn execute_reject<H: CommandHost>(
    app: &mut H,
    id: Option<&str>,
    has_extra_args: bool,
) -> Result<CommandResult> {
    match (id, has_extra_args) {
        (Some(selector), false) => app.reject(selector).map(response),
        _ => Ok(response("Usage: /reject <id>".to_string())),
    }
}

fn execute_embedded_only<H, F>(app: &mut H, handler: F) -> Option<Result<CommandResult>>
where
    H: CommandHost,
    F: FnOnce(&mut H) -> Result<CommandResult>,
{
    app.supports_embedded_slash_commands().then(|| handler(app))
}

fn execute_auth<H: CommandHost>(
    app: &mut H,
    subcommand: Option<&str>,
    action: Option<&str>,
    value: Option<&str>,
    has_extra_args: bool,
) -> Result<CommandResult> {
    app.handle_auth(subcommand, action, value, has_extra_args)
        .map(response)
}

fn execute_keys<H: CommandHost>(
    app: &mut H,
    subcommand: Option<&str>,
    value: Option<&str>,
    option: Option<&str>,
    has_extra_args: bool,
) -> Result<CommandResult> {
    app.handle_keys(subcommand, value, option, has_extra_args)
        .map(response)
}

fn execute_config<H: CommandHost>(app: &mut H, action: Option<&str>) -> Result<CommandResult> {
    match action {
        None => app.show_config().map(response),
        Some("init") => app.init_config().map(response),
        Some("reload") => app.reload_config().map(response),
        Some(other) => Ok(response(format!(
            "Unknown /config action: {other}. Use /config, /config init, or /config reload."
        ))),
    }
}

fn unknown_command_message(command: &str) -> String {
    format!("Unknown command: /{command}\nType /help for available commands.")
}

fn client_only_command_name(command: &ParsedCommand) -> Option<&'static str> {
    match command {
        ParsedCommand::Clear => Some("clear"),
        ParsedCommand::Quit => Some("quit"),
        _ => None,
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

fn parse_proposals_command(parts: &mut std::str::SplitWhitespace<'_>) -> ParsedCommand {
    ParsedCommand::Proposals {
        id: parts.next().map(ToString::to_string),
        has_extra_args: parts.next().is_some(),
    }
}

fn parse_approve_command(parts: &mut std::str::SplitWhitespace<'_>) -> ParsedCommand {
    let first = parts.next();
    let (id, mut force) = match first {
        Some("--force") => (None, true),
        Some(value) => (Some(value.to_string()), false),
        None => (None, false),
    };
    let mut has_extra_args = false;

    for arg in parts {
        match arg {
            "--force" if !force => force = true,
            _ => {
                has_extra_args = true;
                break;
            }
        }
    }

    ParsedCommand::Approve {
        id,
        force,
        has_extra_args,
    }
}

fn parse_reject_command(parts: &mut std::str::SplitWhitespace<'_>) -> ParsedCommand {
    let id = parts.next().map(ToString::to_string);
    let has_extra_args = parts.next().is_some();
    ParsedCommand::Reject { id, has_extra_args }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::{build_headless_loop_engine_bundle, HeadlessLoopBuildOptions};
    use anyhow::anyhow;
    use fx_config::manager::ConfigManager;
    use fx_config::{FawxConfig, ThinkingBudget};
    use fx_core::signals::{LoopStep, Signal, SignalKind};
    use fx_kernel::budget::BudgetRemaining;
    use fx_kernel::loop_engine::LoopStatus;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[derive(Default)]
    struct StubHost {
        models: String,
        status: String,
        budget: String,
        signals: String,
        config: String,
        proposals: String,
        thinking: String,
        init: String,
        reload: String,
        last_model: Option<String>,
        thinking_level: Option<String>,
    }

    impl CommandHost for StubHost {
        fn supports_embedded_slash_commands(&self) -> bool {
            true
        }

        fn list_models(&self) -> String {
            self.models.clone()
        }

        fn set_active_model(&mut self, selector: &str) -> Result<String> {
            if selector == "bad-model" {
                return Err(anyhow!("model not found"));
            }
            self.last_model = Some(selector.to_string());
            Ok(selector.to_string())
        }

        fn proposals(&self, selector: Option<&str>) -> Result<String> {
            Ok(match selector {
                Some(value) => format!("{}:{value}", self.proposals),
                None => self.proposals.clone(),
            })
        }

        fn approve(&self, selector: &str, force: bool) -> Result<String> {
            Ok(format!("approved:{selector}:{force}"))
        }

        fn reject(&self, selector: &str) -> Result<String> {
            Ok(format!("rejected:{selector}"))
        }

        fn show_config(&self) -> Result<String> {
            Ok(self.config.clone())
        }

        fn init_config(&mut self) -> Result<String> {
            Ok(self.init.clone())
        }

        fn reload_config(&mut self) -> Result<String> {
            Ok(self.reload.clone())
        }

        fn show_status(&self) -> String {
            self.status.clone()
        }

        fn show_budget_status(&self) -> String {
            self.budget.clone()
        }

        fn show_signals_summary(&self) -> String {
            self.signals.clone()
        }

        fn handle_thinking(&mut self, level: Option<&str>) -> Result<String> {
            self.thinking_level = level.map(ToString::to_string);
            Ok(self.thinking.clone())
        }
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
    fn command_parsing_recognizes_config_and_reload() {
        assert_eq!(parse_command("/config"), ParsedCommand::Config(None));
        assert_eq!(
            parse_command("/config init"),
            ParsedCommand::Config(Some("init".to_string()))
        );
        assert_eq!(
            parse_command("/config reload"),
            ParsedCommand::Config(Some("reload".to_string()))
        );
    }

    #[test]
    fn parse_proposals_accepts_optional_id() {
        assert_eq!(
            parse_command("/proposals"),
            ParsedCommand::Proposals {
                id: None,
                has_extra_args: false,
            }
        );
        assert_eq!(
            parse_command("/proposals abc123"),
            ParsedCommand::Proposals {
                id: Some("abc123".to_string()),
                has_extra_args: false,
            }
        );
    }

    #[test]
    fn execute_command_routes_server_side_commands() {
        let mut host = StubHost {
            models: "Available models".to_string(),
            status: "status".to_string(),
            budget: "budget".to_string(),
            signals: "signals".to_string(),
            config: "config".to_string(),
            proposals: "proposals".to_string(),
            thinking: "thinking".to_string(),
            init: "init".to_string(),
            reload: "reload".to_string(),
            ..StubHost::default()
        };

        let mut context = CommandContext { app: &mut host };
        let result = execute_command(&mut context, &ParsedCommand::Status)
            .expect("server-side")
            .expect("ok");
        assert_eq!(result.response, "status");
    }

    #[test]
    fn execute_command_returns_none_for_remaining_client_only_commands() {
        let mut host = StubHost::default();
        let mut context = CommandContext { app: &mut host };
        assert!(execute_command(&mut context, &ParsedCommand::Quit).is_none());
        assert!(execute_command(&mut context, &ParsedCommand::Clear).is_none());
    }

    #[test]
    fn execute_command_routes_new_server_side_commands() {
        let mut host = StubHost::default();
        let mut context = CommandContext { app: &mut host };

        let history = execute_command(&mut context, &ParsedCommand::History)
            .expect("server-side")
            .expect("ok");
        assert_eq!(
            history.response,
            "Conversation history is not available in this mode."
        );

        let auth = execute_command(
            &mut context,
            &ParsedCommand::Auth {
                subcommand: None,
                action: None,
                value: None,
                has_extra_args: false,
            },
        )
        .expect("server-side")
        .expect("ok");
        assert_eq!(
            auth.response,
            "Authentication status is not available in this mode."
        );

        let loop_status = execute_command(&mut context, &ParsedCommand::Loop)
            .expect("server-side")
            .expect("ok");
        assert_eq!(
            loop_status.response,
            "Loop status is not available in this mode."
        );
    }

    #[test]
    fn execute_command_formats_model_switch_response() {
        let mut host = StubHost::default();
        let mut context = CommandContext { app: &mut host };
        let result = execute_command(
            &mut context,
            &ParsedCommand::Model(Some("claude-opus-4-6".to_string())),
        )
        .expect("server-side")
        .expect("ok");

        assert_eq!(result.response, "Active model set to: claude-opus-4-6");
        assert_eq!(host.last_model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn execute_command_routes_proposals_detail_requests() {
        let mut host = StubHost {
            proposals: "proposals".to_string(),
            ..StubHost::default()
        };
        let mut context = CommandContext { app: &mut host };
        let result = execute_command(
            &mut context,
            &ParsedCommand::Proposals {
                id: Some("abc123".to_string()),
                has_extra_args: false,
            },
        )
        .expect("server-side")
        .expect("ok");

        assert_eq!(result.response, "proposals:abc123");
    }

    #[test]
    fn execute_command_validates_approve_usage() {
        let mut host = StubHost::default();
        let mut context = CommandContext { app: &mut host };
        let result = execute_command(
            &mut context,
            &ParsedCommand::Approve {
                id: None,
                force: false,
                has_extra_args: false,
            },
        )
        .expect("server-side")
        .expect("ok");

        assert_eq!(result.response, "Usage: /approve <id> [--force]");
    }

    #[test]
    fn execute_command_runs_config_reload() {
        let mut host = StubHost {
            reload: "Configuration reloaded".to_string(),
            ..StubHost::default()
        };
        let mut context = CommandContext { app: &mut host };
        let result = execute_command(
            &mut context,
            &ParsedCommand::Config(Some("reload".to_string())),
        )
        .expect("server-side")
        .expect("ok");

        assert_eq!(result.response, "Configuration reloaded");
    }

    #[test]
    fn client_only_message_mentions_tui() {
        let message = client_only_command_message(&ParsedCommand::Quit).expect("client-only");
        assert_eq!(
            message,
            "/quit is a client-side command (only available in the TUI)"
        );
    }

    #[test]
    fn server_side_commands_are_not_marked_client_only() {
        assert!(client_only_command_message(&ParsedCommand::Analyze).is_none());
        assert!(
            client_only_command_message(&ParsedCommand::Improve(ImproveFlags {
                dry_run: false,
                has_unknown_flag: None,
            }))
            .is_none()
        );
        assert!(client_only_command_message(&ParsedCommand::Loop).is_none());
        assert!(client_only_command_message(&ParsedCommand::History).is_none());
        assert!(client_only_command_message(&ParsedCommand::Auth {
            subcommand: None,
            action: None,
            value: None,
            has_extra_args: false,
        })
        .is_none());
    }

    #[test]
    fn execute_command_returns_unknown_command_response() {
        let mut host = StubHost::default();
        let mut context = CommandContext { app: &mut host };
        let result = execute_command(&mut context, &ParsedCommand::Unknown("wat".to_string()))
            .expect("server-side")
            .expect("ok");

        assert_eq!(
            result.response,
            "Unknown command: /wat\nType /help for available commands."
        );
    }

    #[test]
    fn persist_default_model_updates_config_manager_and_memory() {
        let temp = tempdir().expect("tempdir");
        let mut config = FawxConfig::default();
        let manager = Arc::new(Mutex::new(
            ConfigManager::new(temp.path()).expect("config manager"),
        ));

        persist_default_model(&mut config, Some(&manager), temp.path(), "claude-opus-4-6")
            .expect("persist model");

        assert_eq!(
            config.model.default_model.as_deref(),
            Some("claude-opus-4-6")
        );
        let stored = manager.lock().expect("manager lock").config().clone();
        assert_eq!(
            stored.model.default_model.as_deref(),
            Some("claude-opus-4-6")
        );
    }

    #[test]
    fn persist_thinking_budget_writes_config_without_manager() {
        let temp = tempdir().expect("tempdir");

        persist_thinking_budget(None, temp.path(), ThinkingBudget::Low).expect("persist thinking");

        let stored = FawxConfig::load(temp.path()).expect("load config");
        assert_eq!(stored.general.thinking, Some(ThinkingBudget::Low));
    }

    #[test]
    fn reload_runtime_config_refreshes_manager_state() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.toml");
        let manager = Arc::new(Mutex::new(
            ConfigManager::new(temp.path()).expect("config manager"),
        ));

        {
            let mut guard = manager.lock().expect("manager lock");
            guard
                .set("model.default_model", "gpt-4.1")
                .expect("set model");
        }

        let reloaded = reload_runtime_config(Some(&manager), &config_path).expect("reload config");

        assert_eq!(reloaded.model.default_model.as_deref(), Some("gpt-4.1"));
    }

    #[test]
    fn init_default_config_creates_config_file() {
        let temp = tempdir().expect("tempdir");

        let response = init_default_config(temp.path()).expect("init config");
        let config_path = temp.path().join("config.toml");

        assert_eq!(
            response,
            format!("Created default config at {}", config_path.display())
        );
        assert!(config_path.exists());
    }

    #[test]
    fn config_reload_success_message_uses_tilde_for_home_paths() {
        let home = dirs::home_dir().expect("home directory");
        let config_path = home.join(".fawx").join("config.toml");

        assert_eq!(
            config_reload_success_message(&config_path),
            "Configuration reloaded from ~/.fawx/config.toml"
        );
    }

    #[test]
    fn config_reload_success_message_keeps_non_home_paths_absolute() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.toml");

        assert_eq!(
            config_reload_success_message(&config_path),
            format!("Configuration reloaded from {}", config_path.display())
        );
    }

    #[test]
    fn render_budget_text_formats_loop_status() {
        let status = LoopStatus {
            iteration_count: 2,
            max_iterations: 5,
            llm_calls_used: 3,
            tool_invocations_used: 4,
            tokens_used: 123,
            cost_cents_used: 9,
            remaining: BudgetRemaining {
                llm_calls: 7,
                tool_invocations: 8,
                tokens: 456,
                cost_cents: 11,
                wall_time_ms: 12,
            },
        };

        assert_eq!(
            render_budget_text(status),
            "Budget usage:\n  - LLM calls used: 3\n  - Tool calls used: 4\n  - Tokens used: 123\n  - Cost used (cents): 9\n  - Tokens remaining: 456\n  - LLM calls remaining: 7"
        );
    }

    #[test]
    fn render_signals_summary_uses_shared_signal_collector_format() {
        let signals = vec![Signal {
            step: LoopStep::Act,
            kind: SignalKind::Friction,
            message: "tool timed out".to_string(),
            metadata: serde_json::Value::Null,
            timestamp_ms: 42,
        }];

        assert_eq!(
            render_signals_summary(&signals),
            "1 signals · 1 friction · 1 tool action · last friction: tool timed out"
        );
    }

    #[test]
    fn apply_thinking_budget_updates_config_disk_and_wire_format() {
        let temp = tempdir().expect("tempdir");
        let mut config = FawxConfig::default();
        config.general.data_dir = Some(temp.path().to_path_buf());
        let mut bundle =
            build_headless_loop_engine_bundle(&config, None, HeadlessLoopBuildOptions::default())
                .expect("build loop engine");

        let response = apply_thinking_budget(
            &mut config,
            &mut bundle.engine,
            None,
            temp.path(),
            "gpt-5.4",
            Some("high"),
        )
        .expect("apply thinking budget");

        assert_eq!(response, "Thinking budget set to: high");
        assert_eq!(config.general.thinking, Some(ThinkingBudget::High));
        let stored = FawxConfig::load(temp.path()).expect("load config");
        assert_eq!(stored.general.thinking, Some(ThinkingBudget::High));
    }
}
