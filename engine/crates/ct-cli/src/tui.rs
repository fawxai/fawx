use crossterm::style::Stylize;
use crossterm::{cursor, event, style, terminal, ExecutableCommand};
use ct_kernel::auth::{AuthManager, AuthMethod};
use ct_kernel::oauth::PkceFlow;
use ct_llm::{
    AnthropicProvider, CompletionRequest, ContentBlock, Message, ModelRouter, OpenAiProvider,
};
use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_AUTH_FILE: &str = ".citros/auth.json";

const DEFAULT_ANTHROPIC_MODELS: &[&str] =
    &["claude-opus-4", "claude-sonnet-4", "claude-3-7-sonnet-latest"];
const DEFAULT_OPENAI_MODELS: &[&str] = &["gpt-4.1", "gpt-4o", "gpt-4o-mini"];
const DEFAULT_OPENROUTER_MODELS: &[&str] = &[
    "openai/gpt-4o-mini",
    "anthropic/claude-3.5-sonnet",
    "google/gemini-2.0-flash-001",
];

/// The main TUI application loop.
pub struct TuiApp {
    router: ModelRouter,
    auth_manager: AuthManager,
    running: bool,
}

impl TuiApp {
    /// Create a new TUI application.
    pub fn new(auth_manager: AuthManager, router: ModelRouter) -> Self {
        Self {
            router,
            auth_manager,
            running: true,
        }
    }

    /// Run the TUI main loop.
    pub async fn run(&mut self) -> Result<(), TuiError> {
        self.show_welcome();

        if !self.auth_manager.has_any() {
            self.auth_wizard().await?;
        } else {
            self.select_first_available_model();
        }

        println!("Type /help for commands.\n");

        let mut line = String::new();

        while self.running {
            print!("{}", "> ".with(style::Color::DarkGrey));
            io::stdout().flush().map_err(TuiError::Io)?;

            line.clear();
            let bytes = io::stdin().read_line(&mut line).map_err(TuiError::Io)?;
            if bytes == 0 {
                break;
            }

            let input = line.trim();
            if input.is_empty() {
                continue;
            }

            if input.starts_with('/') {
                self.handle_command(input).await?;
            } else {
                let response = self.handle_message(input).await?;
                self.display_response(&response);
            }
        }

        Ok(())
    }

    /// Display the welcome banner.
    fn show_welcome(&self) {
        let mut stdout = io::stdout();
        let _ = stdout.execute(terminal::Clear(terminal::ClearType::CurrentLine));

        let width = terminal::size().map(|(w, _)| w).unwrap_or(80);
        let banner = "Citros";
        let padding = usize::from(width.saturating_sub(banner.len() as u16) / 2);

        println!();
        println!("{:padding$}{}", "", banner.bold().with(style::Color::Cyan));
        println!("{:padding$}{}", "", "Terminal shell".with(style::Color::DarkGrey));
        println!();
    }

    /// Run the first-time auth wizard if no credentials exist.
    async fn auth_wizard(&mut self) -> Result<(), TuiError> {
        println!("Welcome to Citros.\n");
        println!("No credentials found. Let's set up authentication.\n");

        let preferred_models = loop {
            println!("How would you like to authenticate?");
            println!("  [1] Claude subscription (paste setup-token)");
            println!("  [2] ChatGPT subscription (browser sign-in)");
            println!("  [3] API key (any provider)");
            println!();

            let selection = prompt_line("> ")?;
            match parse_auth_selection(&selection) {
                Some(AuthSelection::ClaudeSubscription) => {
                    let token = prompt_secret("Paste your Claude setup token: ")?;
                    if token.is_empty() {
                        println!("Setup token cannot be empty.\n");
                        continue;
                    }

                    self.auth_manager
                        .store("anthropic", AuthMethod::SetupToken { token });

                    println!("✓ Authenticated. Token stored.\n");
                    break to_strings(DEFAULT_ANTHROPIC_MODELS);
                }
                Some(AuthSelection::ChatGptSubscription) => {
                    let flow = PkceFlow::new();
                    let client_id = std::env::var("CITROS_OPENAI_CLIENT_ID")
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| "citros-cli".to_string());
                    let auth_url = flow.authorization_url(&client_id);

                    println!("Opening browser for ChatGPT sign-in...");
                    if let Err(error) = open_browser(&auth_url) {
                        println!(
                            "Couldn't open browser automatically ({error}). Open this URL manually:\n{auth_url}"
                        );
                    }
                    println!("Waiting for callback on {}...", flow.redirect_uri());

                    let callback_url = prompt_line("Paste callback URL (optional): ")?;
                    if !callback_url.is_empty() {
                        flow.parse_callback(&callback_url).map_err(|error| {
                            TuiError::Auth(format!("invalid OAuth callback URL: {error}"))
                        })?;
                    }

                    let access_token = prompt_secret("Enter OpenAI access token: ")?;
                    if access_token.is_empty() {
                        println!("Access token cannot be empty.\n");
                        continue;
                    }

                    let refresh_token = prompt_secret("Enter OpenAI refresh token (optional): ")?;
                    let account_id = prompt_line("OpenAI account id (optional): ")?;

                    self.auth_manager.store(
                        "openai",
                        AuthMethod::OAuth {
                            provider: "openai".to_string(),
                            access_token,
                            refresh_token,
                            expires_at: current_time_ms() + 3_600_000,
                            account_id: empty_to_none(account_id),
                        },
                    );

                    println!("✓ Authenticated. Tokens stored.\n");
                    break to_strings(DEFAULT_OPENAI_MODELS);
                }
                Some(AuthSelection::ApiKey) => {
                    println!("Which provider?");
                    println!("  [1] Anthropic");
                    println!("  [2] OpenAI");
                    println!("  [3] OpenRouter");
                    println!("  [4] Other (OpenAI-compatible)");
                    println!();

                    let provider = loop {
                        let provider_choice = prompt_line("> ")?;
                        match parse_api_key_provider_selection(&provider_choice) {
                            Some(ApiKeyProvider::Anthropic) => {
                                break "anthropic".to_string();
                            }
                            Some(ApiKeyProvider::OpenAi) => {
                                break "openai".to_string();
                            }
                            Some(ApiKeyProvider::OpenRouter) => {
                                break "openrouter".to_string();
                            }
                            Some(ApiKeyProvider::Other) => {
                                let name = prompt_line("Provider name: ")?;
                                if name.is_empty() {
                                    println!("Provider name cannot be empty.");
                                    continue;
                                }
                                break name;
                            }
                            None => {
                                println!("Please choose 1, 2, 3, or 4.");
                            }
                        }
                    };

                    let key = prompt_secret(&format!("Enter your {provider} API key: "))?;
                    if key.is_empty() {
                        println!("API key cannot be empty.\n");
                        continue;
                    }

                    self.auth_manager.store(
                        &provider,
                        AuthMethod::ApiKey {
                            provider: provider.clone(),
                            key,
                        },
                    );

                    println!("✓ API key stored.\n");
                    break models_for_provider(&provider);
                }
                None => {
                    println!("Please choose 1, 2, or 3.\n");
                }
            }
        };

        persist_auth_manager(&self.auth_manager).await?;

        self.router = build_router(&self.auth_manager)?;
        self.set_preferred_model(&preferred_models);

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

        Ok(())
    }

    /// Process a user command (starts with `/`).
    async fn handle_command(&mut self, input: &str) -> Result<(), TuiError> {
        match parse_command(input) {
            ParsedCommand::Model(None) => self.show_model_menu(),
            ParsedCommand::Model(Some(model)) => match self.router.set_active(&model) {
                Ok(()) => println!("Active model set to: {model}"),
                Err(error) => {
                    println!("Couldn't select model: {error}");
                    self.show_model_menu();
                }
            },
            ParsedCommand::Auth => {
                self.show_auth_status();
                let add_more = prompt_line("Run auth wizard to add/update credentials? [y/N]: ")?;
                if is_yes(&add_more) {
                    self.auth_wizard().await?;
                }
            }
            ParsedCommand::Budget => self.show_budget_status(),
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

    /// Process a user message (sent to the model router).
    async fn handle_message(&mut self, input: &str) -> Result<String, TuiError> {
        let active_model = self
            .router
            .active_model()
            .ok_or_else(|| TuiError::Router("no active model selected".to_string()))?
            .to_string();

        let request = CompletionRequest {
            model: active_model,
            messages: vec![Message::user(input)],
            tools: Vec::new(),
            temperature: Some(0.2),
            max_tokens: Some(1024),
            system_prompt: None,
        };

        let response = self
            .router
            .complete(request)
            .await
            .map_err(|error| TuiError::Loop(error.to_string()))?;

        let rendered = response
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => text.clone(),
                ContentBlock::ToolUse { name, .. } => {
                    format!("[tool requested: {name}]")
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    format!("[tool result: {tool_use_id}]")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        if rendered.is_empty() {
            Ok("(No text response returned.)".to_string())
        } else {
            Ok(rendered)
        }
    }

    /// Display formatted output to the terminal.
    fn display_response(&self, response: &str) {
        let mut stdout = io::stdout();
        let _ = stdout.execute(cursor::MoveToColumn(0));

        println!();
        println!("{}", "Assistant".bold().with(style::Color::Cyan));
        println!("{response}");
        println!();
    }

    /// Display the model selection menu.
    fn show_model_menu(&self) {
        let active = self.router.active_model();
        let models = self.router.available_models();

        if models.is_empty() {
            println!("No models available. Use /auth to configure credentials.");
            return;
        }

        println!("Available models:");
        for model in models {
            let marker = if Some(model.model_id.as_str()) == active {
                "*"
            } else {
                " "
            };

            println!(
                "  {marker} {} ({}, {})",
                model.model_id, model.provider_name, model.auth_method
            );
        }
    }

    fn select_first_available_model(&mut self) {
        if self.router.active_model().is_some() {
            return;
        }

        if let Some(model) = self
            .router
            .available_models()
            .into_iter()
            .next()
            .map(|model| model.model_id)
        {
            let _ = self.router.set_active(&model);
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

    fn show_help(&self) {
        println!("Available commands:");
        println!("  /model         List models and show active model");
        println!("  /model <name>  Switch active model");
        println!("  /auth          Show auth status and run auth wizard");
        println!("  /budget        Show current budget usage");
        println!("  /help          Show this help message");
        println!("  /quit          Exit Citros");
        println!("  /exit          Exit Citros");
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

    fn show_budget_status(&self) {
        println!("Budget integration lands in PR 9.");
        println!("Current mode: direct model chat (no loop budget accounting yet).");
    }
}

/// User-facing TUI errors.
#[derive(Debug)]
pub enum TuiError {
    /// Terminal or filesystem IO failure.
    Io(io::Error),
    /// Authentication flow error.
    Auth(String),
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

/// Load the persisted auth manager from disk, if present.
pub async fn load_auth_manager() -> Result<AuthManager, TuiError> {
    let auth_path = auth_file_path()?;

    if !auth_path.exists() {
        return Ok(AuthManager::new());
    }

    let raw = tokio::fs::read_to_string(&auth_path).await.map_err(TuiError::Io)?;
    AuthManager::from_json(&raw).map_err(|error| {
        TuiError::Auth(format!(
            "failed to parse auth file {}: {error}",
            auth_path.display()
        ))
    })
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
        let _ = router.set_active(&first_model);
    }

    Ok(router)
}

async fn persist_auth_manager(auth_manager: &AuthManager) -> Result<(), TuiError> {
    let auth_path = auth_file_path()?;

    if let Some(parent) = auth_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(TuiError::Io)?;
    }

    let payload = auth_manager
        .to_json()
        .map_err(|error| TuiError::Auth(format!("failed to serialize auth manager: {error}")))?;

    tokio::fs::write(&auth_path, payload)
        .await
        .map_err(TuiError::Io)
}

fn register_auth_provider(router: &mut ModelRouter, auth_method: &AuthMethod) -> Result<(), TuiError> {
    match auth_method {
        AuthMethod::SetupToken { token } => {
            let provider = AnthropicProvider::new(base_url_for_provider("anthropic"), token.clone())
                .map_err(|error| TuiError::Router(format!("failed to configure Anthropic provider: {error}")))?
                .with_supported_models(to_strings(DEFAULT_ANTHROPIC_MODELS));

            router.register_provider_with_auth(Box::new(provider), "subscription");
        }
        AuthMethod::ApiKey { provider, key } => {
            if provider == "anthropic" {
                let anthropic_provider =
                    AnthropicProvider::new(base_url_for_provider("anthropic"), key.clone())
                        .map_err(|error| {
                            TuiError::Router(format!(
                                "failed to configure Anthropic provider: {error}"
                            ))
                        })?
                        .with_supported_models(models_for_provider("anthropic"));

                router.register_provider_with_auth(Box::new(anthropic_provider), "api_key");
            } else {
                let provider_client =
                    OpenAiProvider::new(base_url_for_provider(provider), key.clone())
                        .map_err(|error| {
                            TuiError::Router(format!(
                                "failed to configure {provider} provider: {error}"
                            ))
                        })?
                        .with_name(provider.clone())
                        .with_supported_models(models_for_provider(provider));

                router.register_provider_with_auth(Box::new(provider_client), "api_key");
            }
        }
        AuthMethod::OAuth {
            provider,
            access_token,
            ..
        } => {
            let provider_client = OpenAiProvider::new(base_url_for_provider(provider), access_token.clone())
                .map_err(|error| {
                    TuiError::Router(format!("failed to configure {provider} provider: {error}"))
                })?
                .with_name(provider.clone())
                .with_supported_models(models_for_provider(provider));

            router.register_provider_with_auth(Box::new(provider_client), "subscription");
        }
    }

    Ok(())
}

fn auth_file_path() -> Result<PathBuf, TuiError> {
    if let Ok(path) = std::env::var("CITROS_AUTH_FILE") {
        let path = path.trim();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    let home = dirs::home_dir()
        .ok_or_else(|| TuiError::Auth("unable to determine home directory".to_string()))?;

    Ok(home.join(DEFAULT_AUTH_FILE))
}

fn base_url_for_provider(provider: &str) -> String {
    let env_key = format!(
        "CITROS_{}_BASE_URL",
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
        _ => std::env::var("CITROS_OPENAI_COMPAT_BASE_URL")
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
    print!("{prompt}");
    io::stdout().flush().map_err(TuiError::Io)?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(TuiError::Io)?;

    Ok(input.trim().to_string())
}

fn prompt_secret(prompt: &str) -> Result<String, TuiError> {
    print!("{prompt}");
    io::stdout().flush().map_err(TuiError::Io)?;

    let _guard = RawModeGuard::new()?;
    let mut value = String::new();

    loop {
        let event = event::read().map_err(TuiError::Io)?;
        if let event::Event::Key(key_event) = event {
            match key_event.code {
                event::KeyCode::Enter => break,
                event::KeyCode::Char(ch) => value.push(ch),
                event::KeyCode::Backspace => {
                    value.pop();
                }
                _ => {}
            }
        }
    }

    println!();
    Ok(value.trim().to_string())
}

fn open_browser(url: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open").arg(url).status()?;
        if status.success() {
            return Ok(());
        }
        return Err(io::Error::other("open command failed"));
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

fn empty_to_none(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn is_yes(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "y" | "yes")
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
    Help,
    Quit,
    Unknown(String),
}

fn parse_command(value: &str) -> ParsedCommand {
    let input = value.trim();
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
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_test_app() -> TuiApp {
        TuiApp::new(AuthManager::new(), ModelRouter::new())
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
    fn command_parsing_recognizes_model_help_and_quit() {
        assert_eq!(parse_command("/model"), ParsedCommand::Model(None));
        assert_eq!(
            parse_command("/model claude-opus-4"),
            ParsedCommand::Model(Some("claude-opus-4".to_string()))
        );
        assert_eq!(parse_command("/help"), ParsedCommand::Help);
        assert_eq!(parse_command("/quit"), ParsedCommand::Quit);
        assert_eq!(parse_command("/exit"), ParsedCommand::Quit);
    }

    #[tokio::test]
    async fn handle_command_dispatches_help_without_stopping() {
        let mut app = new_test_app();

        app.handle_command("/help").await.unwrap();

        assert!(app.running);
    }

    #[tokio::test]
    async fn handle_command_dispatches_quit_and_stops() {
        let mut app = new_test_app();

        app.handle_command("/quit").await.unwrap();

        assert!(!app.running);
    }
}
