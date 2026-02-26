use async_trait::async_trait;
use crossterm::style::Stylize;
use crossterm::{cursor, event, style, terminal, ExecutableCommand};
use ct_core::error::LlmError as CoreLlmError;
use ct_core::types::{InputSource, ScreenState, UserInput};
use ct_kernel::auth::{AuthManager, AuthMethod};
use ct_kernel::budget::{BudgetConfig, BudgetTracker};
use ct_kernel::context_manager::ContextCompactor;
use ct_kernel::loop_engine::{LlmProvider as LoopLlmProvider, LoopEngine, LoopResult};
use ct_kernel::oauth::{PkceFlow, TokenExchangeRequest, TokenResponse};
use ct_kernel::types::PerceptionSnapshot;
use ct_llm::{
    AnthropicProvider, CompletionRequest, ContentBlock, Message, ModelCatalog, ModelRouter,
    OpenAiProvider, OpenAiResponsesProvider,
};
use std::fmt;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_AUTH_FILE: &str = ".citros/auth.json";
const DEFAULT_OPENAI_TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const MAX_PROMPT_RETRIES: usize = 10;

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

/// The main TUI application loop.
pub struct TuiApp {
    router: ModelRouter,
    auth_manager: AuthManager,
    catalog: ModelCatalog,
    loop_engine: LoopEngine,
    running: bool,
}

impl TuiApp {
    /// Create a new TUI application.
    pub fn new(auth_manager: AuthManager, router: ModelRouter, loop_engine: LoopEngine) -> Self {
        Self {
            router,
            auth_manager,
            catalog: ModelCatalog::new(),
            loop_engine,
            running: true,
        }
    }

    /// Run the TUI main loop.
    pub async fn run(&mut self) -> Result<(), TuiError> {
        self.show_welcome();

        if !self.auth_manager.has_any() {
            self.auth_wizard().await?;
        } else {
            self.refresh_router_models().await?;
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
                if let Err(error) = self.handle_command(input).await {
                    println!("[31mError: {error}[0m\n");
                }
            } else {
                match self.handle_message(input).await {
                    Ok(response) => self.display_response(&response),
                    Err(error) => {
                        println!("[31mError: {error}[0m\n");
                    }
                }
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
        println!(
            "{:padding$}{}",
            "",
            "Terminal shell".with(style::Color::DarkGrey)
        );
        println!();
    }

    /// Run the first-time auth wizard if no credentials exist.
    async fn auth_wizard(&mut self) -> Result<(), TuiError> {
        println!("Welcome to Citros.\n");
        if self.auth_manager.providers().is_empty() {
            println!("No credentials found. Let's set up authentication.\n");
        } else {
            println!("Add another provider.\n");
        }

        println!("How would you like to authenticate?");
        println!("  [1] Claude subscription (paste setup-token)");
        println!("  [2] ChatGPT subscription (browser sign-in)");
        println!("  [3] API key (any provider)");
        println!();

        let selection = prompt_choice(
            "> ",
            "Please choose 1, 2, or 3.\n",
            "authentication selection",
            parse_auth_selection,
        )?;

        let preferred_provider = match selection {
            AuthSelection::ClaudeSubscription => {
                let token = prompt_non_empty_secret(
                    "Paste your Claude setup token: ",
                    "Setup token cannot be empty.\n",
                    "Claude setup token",
                )?;

                self.auth_manager
                    .store("anthropic", AuthMethod::SetupToken { token });

                println!("✓ Authenticated. Token stored.\n");
                "anthropic".to_string()
            }
            AuthSelection::ChatGptSubscription => {
                let flow = PkceFlow::new();
                let client_id = std::env::var("CITROS_OPENAI_CLIENT_ID")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| ct_kernel::oauth::OPENAI_CLIENT_ID.to_string());
                let auth_url = flow.authorization_url(&client_id);

                // Start local callback server before opening browser
                let auth_code = match start_oauth_callback_server(flow.state()).await {
                    Ok(server) => {
                        println!("Opening browser for ChatGPT sign-in...");
                        if let Err(error) = open_browser(&auth_url) {
                            println!(
                                "Couldn't open browser automatically ({error}). Open this URL manually:\n{auth_url}"
                            );
                        }
                        println!("Waiting for callback on http://127.0.0.1:1455/auth/callback...");
                        println!("(Or paste the redirect URL/code below if browser didn't work)\n");

                        // Race: wait for callback server OR manual paste
                        let code = tokio::select! {
                            result = server => {
                                match result {
                                    Ok(code) => code,
                                    Err(_) => {
                                        // Server failed, fall back to manual paste
                                        let input = prompt_non_empty_line(
                                            "Paste the redirect URL or authorization code: ",
                                            "Input cannot be empty.\n",
                                            "OAuth redirect URL or code",
                                        )?;
                                        flow.parse_callback(&input).or_else(|_| {
                                            // Maybe they pasted just the code
                                            Ok::<String, ct_kernel::oauth::AuthError>(input.trim().to_string())
                                        }).map_err(|e| TuiError::Auth(format!("{e}")))?
                                    }
                                }
                            }
                        };
                        code
                    }
                    Err(_) => {
                        // Couldn't bind port 1455, fall back to manual flow
                        println!("Couldn't start local server. Open this URL in your browser:\n");
                        println!("  {auth_url}\n");
                        let input = prompt_non_empty_line(
                            "Paste the redirect URL or authorization code: ",
                            "Input cannot be empty.\n",
                            "OAuth redirect URL or code",
                        )?;
                        flow.parse_callback(&input)
                            .or_else(|_| {
                                Ok::<String, ct_kernel::oauth::AuthError>(input.trim().to_string())
                            })
                            .map_err(|e| TuiError::Auth(format!("{e}")))?
                    }
                };

                println!("Exchanging authorization code for tokens...");
                let token_response =
                    exchange_oauth_code_for_tokens(&flow, &client_id, &auth_code).await?;

                let account_id =
                    ct_kernel::oauth::extract_openai_account_id(&token_response.access_token);
                let expires_at = current_time_ms()
                    .saturating_add(token_response.expires_in.saturating_mul(1_000));

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

                println!("✓ Authenticated. Tokens stored.\n");
                "openai".to_string()
            }
            AuthSelection::ApiKey => {
                println!("Which provider?");
                println!("  [1] Anthropic");
                println!("  [2] OpenAI");
                println!("  [3] OpenRouter");
                println!("  [4] Other (OpenAI-compatible)");
                println!();

                let provider_choice = prompt_choice(
                    "> ",
                    "Please choose 1, 2, 3, or 4.",
                    "API key provider selection",
                    parse_api_key_provider_selection,
                )?;

                let provider = match provider_choice {
                    ApiKeyProvider::Anthropic => "anthropic".to_string(),
                    ApiKeyProvider::OpenAi => "openai".to_string(),
                    ApiKeyProvider::OpenRouter => "openrouter".to_string(),
                    ApiKeyProvider::Other => prompt_non_empty_line(
                        "Provider name: ",
                        "Provider name cannot be empty.",
                        "API key provider name",
                    )?,
                };

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
                provider
            }
        };

        persist_auth_manager(&self.auth_manager).await?;

        let preferred_models = self.get_models_for_provider(&preferred_provider).await;
        self.refresh_router_models().await?;
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
            ParsedCommand::Model(None) => {
                self.refresh_router_models().await?;
                self.show_model_menu();
            }
            ParsedCommand::Model(Some(model)) => {
                self.refresh_router_models().await?;
                match self.router.set_active(&model) {
                    Ok(()) => println!("Active model set to: {model}"),
                    Err(error) => {
                        println!("Couldn't select model: {error}");
                        self.show_model_menu();
                    }
                }
            }
            ParsedCommand::Auth => {
                self.show_auth_status();
                let add_more = prompt_line("Run auth wizard to add/update credentials? [y/N]: ")?;
                if is_yes(&add_more) {
                    self.auth_wizard().await?;
                }
            }
            ParsedCommand::Budget => self.show_budget_status(),
            ParsedCommand::Loop => self.show_loop_status(),
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
        let active_model = self
            .router
            .active_model()
            .ok_or_else(|| TuiError::Router("no active model selected".to_string()))?
            .to_string();

        let snapshot = self.build_perception_snapshot(input);
        let loop_result = {
            let router = &self.router;
            let llm = RouterLoopLlmProvider::new(router, active_model);
            let loop_engine = &mut self.loop_engine;
            loop_engine
                .run_cycle(snapshot, &llm)
                .await
                .map_err(|error| TuiError::Loop(error.reason))?
        };

        Ok(render_loop_result(loop_result))
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
                    let _ = refreshed.set_active(&first_model);
                }
            }
        }

        self.router = refreshed;
        Ok(())
    }

    fn show_help(&self) {
        println!("Available commands:");
        println!("  /model         List models and show active model");
        println!("  /model <name>  Switch active model");
        println!("  /auth          Show auth status and run auth wizard");
        println!("  /budget        Show current budget usage");
        println!("  /loop          Show loop status (iterations, budget, tokens)");
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

    fn build_perception_snapshot(&self, input: &str) -> PerceptionSnapshot {
        let timestamp_ms = current_time_ms();

        PerceptionSnapshot {
            screen: ScreenState {
                current_app: "citros.tui".to_string(),
                elements: Vec::new(),
                text_content: input.to_string(),
            },
            notifications: Vec::new(),
            active_app: "citros.tui".to_string(),
            timestamp_ms,
            sensor_data: None,
            user_input: Some(UserInput {
                text: input.to_string(),
                source: InputSource::Text,
                timestamp: timestamp_ms,
                context_id: None,
            }),
        }
    }
}

/// Build a loop engine with sensible defaults for the TUI shell.
pub fn build_loop_engine() -> LoopEngine {
    let budget = BudgetTracker::new(BudgetConfig::default(), current_time_ms(), 0);
    let context = ContextCompactor::new(8_000, 6_000);
    LoopEngine::new(budget, context, 10)
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
            temperature: Some(0.2),
            max_tokens: Some(max_tokens),
            system_prompt: None,
        };

        let response = self
            .router
            .complete(request)
            .await
            .map_err(|error| CoreLlmError::Inference(error.to_string()))?;

        let rendered = render_completion_blocks(&response.content);
        if rendered.trim().is_empty() {
            Err(CoreLlmError::InvalidResponse(
                "provider returned an empty completion".to_string(),
            ))
        } else {
            Ok(rendered)
        }
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, CoreLlmError> {
        let rendered = self.generate(prompt, max_tokens).await?;
        callback(rendered.clone());
        Ok(rendered)
    }

    fn model_name(&self) -> &str {
        &self.active_model
    }
}

fn render_completion_blocks(content: &[ContentBlock]) -> String {
    content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::ToolUse { name, .. } => format!("[tool requested: {name}]"),
            ContentBlock::ToolResult { tool_use_id, .. } => {
                format!("[tool result: {tool_use_id}]")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_loop_result(result: LoopResult) -> String {
    match result {
        LoopResult::Complete {
            response,
            iterations,
            tokens_used,
            ..
        } => {
            format!(
                "{response}\n\n[loop complete in {iterations} iteration(s); tokens in/out: {}/{}]",
                tokens_used.input_tokens, tokens_used.output_tokens
            )
        }
        LoopResult::BudgetExhausted {
            partial_response,
            iterations,
        } => {
            if let Some(partial) = partial_response {
                format!(
                    "{partial}\n\n[loop stopped: budget exhausted after {iterations} iteration(s)]"
                )
            } else {
                format!("[loop stopped: budget exhausted after {iterations} iteration(s)]")
            }
        }
        LoopResult::NeedsInput { prompt, iterations } => {
            format!("{prompt}\n\n[loop needs input after {iterations} iteration(s)]")
        }
        LoopResult::Error {
            message,
            recoverable,
        } => {
            if recoverable {
                format!("{message}\n\n[loop error is recoverable — try again]")
            } else {
                format!("{message}\n\n[loop error is not recoverable]")
            }
        }
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

    let raw = tokio::fs::read_to_string(&auth_path)
        .await
        .map_err(TuiError::Io)?;
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
        .map_err(TuiError::Io)?;

    #[cfg(unix)]
    {
        tokio::fs::set_permissions(&auth_path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(TuiError::Io)?;
    }

    Ok(())
}

const OAUTH_SUCCESS_HTML: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Authentication successful</title></head>
<body><p>Authentication successful. Return to your terminal to continue.</p></body></html>"#;

/// Start a local HTTP server on port 1455 to capture the OAuth callback.
/// Returns a future that resolves with the authorization code when received.
async fn start_oauth_callback_server(
    expected_state: &str,
) -> Result<impl std::future::Future<Output = Result<String, TuiError>>, TuiError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:1455")
        .await
        .map_err(|e| TuiError::Auth(format!("failed to bind port 1455: {e}")))?;

    let state = expected_state.to_string();
    Ok(async move {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);

        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Err(TuiError::Auth("OAuth callback timed out (60s)".to_string()));
            }

            let accept = tokio::time::timeout(deadline - now, listener.accept())
                .await
                .map_err(|_| TuiError::Auth("OAuth callback timed out (60s)".to_string()))?
                .map_err(|e| TuiError::Auth(format!("failed to accept connection: {e}")))?;

            let (mut stream, _addr) = accept;
            let mut buf = vec![0u8; 4096];
            let n = stream
                .read(&mut buf)
                .await
                .map_err(|e| TuiError::Auth(format!("failed to read request: {e}")))?;
            let request = String::from_utf8_lossy(&buf[..n]);

            // Extract the path from "GET /auth/callback?code=...&state=... HTTP/1.1"
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .ok_or_else(|| TuiError::Auth("malformed HTTP request".to_string()))?;

            let (path_only, query) = path.split_once('?').map_or((path, ""), |(p, q)| (p, q));
            if path_only != "/auth/callback" {
                let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot found";
                let _ = stream.write_all(response.as_bytes()).await;
                continue;
            }

            // Parse query params from the path

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
                    .map_err(|e| {
                        TuiError::Auth(format!(
                            "callback {key} was not valid percent-encoding: {e}"
                        ))
                    })
            };

            // Validate state (after percent-decoding)
            let returned_state = decode_param("state")?;
            if returned_state != state {
                let response =
                    "HTTP/1.1 400 Bad Request\r\nContent-Length: 14\r\n\r\nState mismatch";
                let _ = stream.write_all(response.as_bytes()).await;
                return Err(TuiError::Auth("OAuth state mismatch".to_string()));
            }

            // Extract code (after percent-decoding)
            let code = decode_param("code")?;

            // Send success page
            let body = OAUTH_SUCCESS_HTML;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;

            return Ok(code);
        }
    })
}

fn openai_oauth_token_endpoint() -> String {
    std::env::var("CITROS_OPENAI_TOKEN_URL")
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
        let reason = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|json| {
                json.get("error_description")
                    .or_else(|| json.get("error"))
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| "token endpoint request failed".to_string());

        return Err(TuiError::Auth(format!(
            "oauth token exchange failed ({}): {reason}",
            status.as_u16()
        )));
    }

    serde_json::from_str::<TokenResponse>(&body)
        .map_err(|error| TuiError::Auth(format!("oauth token response was invalid JSON: {error}")))
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
    let supported_models = if supported_models.is_empty() {
        default_supported_models(auth_method)
    } else {
        supported_models
    };

    match auth_method {
        AuthMethod::SetupToken { token } => {
            let provider =
                AnthropicProvider::new(base_url_for_provider("anthropic"), token.clone())
                    .map_err(|error| {
                        TuiError::Router(format!("failed to configure Anthropic provider: {error}"))
                    })?
                    .with_supported_models(supported_models);

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
                        .with_supported_models(supported_models);

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
                        .with_supported_models(supported_models);

                router.register_provider_with_auth(Box::new(provider_client), "api_key");
            }
        }
        AuthMethod::OAuth {
            provider,
            access_token,
            account_id,
            ..
        } => {
            if let Some(acct_id) = account_id {
                let provider_client =
                    OpenAiResponsesProvider::new(access_token.clone(), acct_id.clone())
                        .map_err(|error| {
                            TuiError::Router(format!(
                                "failed to configure {provider} Responses provider: {error}"
                            ))
                        })?
                        .with_supported_models(supported_models);

                router.register_provider_with_auth(Box::new(provider_client), "subscription");
            } else {
                let provider_client =
                    OpenAiProvider::new(base_url_for_provider(provider), access_token.clone())
                        .map_err(|error| {
                            TuiError::Router(format!(
                                "failed to configure {provider} provider: {error}"
                            ))
                        })?
                        .with_name(provider.clone())
                        .with_supported_models(supported_models);

                router.register_provider_with_auth(Box::new(provider_client), "subscription");
            }
        }
    }

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
    let bytes = io::stdin().read_line(&mut input).map_err(TuiError::Io)?;
    if bytes == 0 {
        return Err(TuiError::Auth("stdin closed unexpectedly".to_string()));
    }

    Ok(input.trim().to_string())
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
    let mut display_len: usize = 0;

    loop {
        let event = event::read().map_err(TuiError::Io)?;
        if let event::Event::Key(key_event) = event {
            match key_event.code {
                event::KeyCode::Enter => break,
                event::KeyCode::Char(ch) => {
                    value.push(ch);
                    display_len += 1;
                    // Show a dot for each character so user knows paste worked
                    print!("•");
                    io::stdout().flush().map_err(TuiError::Io)?;
                }
                event::KeyCode::Backspace => {
                    if value.pop().is_some() && display_len > 0 {
                        display_len -= 1;
                        // Erase the last dot
                        print!("\x08 \x08");
                        io::stdout().flush().map_err(TuiError::Io)?;
                    }
                }
                _ => {}
            }
        }
    }

    // Show confirmation with character count
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        println!();
    } else {
        println!(" ({} chars)", trimmed.len());
    }
    Ok(trimmed)
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
    Loop,
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
        "loop" => ParsedCommand::Loop,
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
    use async_trait::async_trait;
    use std::ffi::OsString;
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
        TuiApp::new(AuthManager::new(), ModelRouter::new(), build_loop_engine())
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
    impl ct_llm::CompletionProvider for StaticCompletionProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<ct_llm::CompletionResponse, ct_llm::ProviderError> {
            Ok(ct_llm::CompletionResponse {
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
        ) -> Result<ct_llm::CompletionStream, ct_llm::ProviderError> {
            Err(ct_llm::ProviderError::Streaming(
                "streaming not implemented in test provider".to_string(),
            ))
        }

        fn name(&self) -> &str {
            &self.provider_name
        }

        fn supported_models(&self) -> Vec<String> {
            vec![self.model.clone()]
        }
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

        TuiApp::new(AuthManager::new(), router, build_loop_engine())
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
            parse_command("/model claude-sonnet-4-20250514"),
            ParsedCommand::Model(Some("claude-sonnet-4-20250514".to_string()))
        );
        assert_eq!(parse_command("/help"), ParsedCommand::Help);
        assert_eq!(parse_command("/loop"), ParsedCommand::Loop);
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
        assert!(rendered.contains("[loop complete"));
    }

    #[tokio::test]
    async fn load_auth_manager_loads_expected_auth_manager_from_temp_file() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_dir = TempDir::new().unwrap();
        let auth_path = temp_dir.path().join("auth.json");
        let expected = test_auth_manager();

        tokio::fs::write(&auth_path, expected.to_json().unwrap())
            .await
            .unwrap();
        let _auth_path_env = ScopedEnvVar::set("CITROS_AUTH_FILE", auth_path.to_str().unwrap());

        let loaded = load_auth_manager().await.unwrap();

        assert_eq!(loaded, expected);
    }

    #[tokio::test]
    async fn persist_auth_manager_writes_expected_json_to_temp_file() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_dir = TempDir::new().unwrap();
        let auth_path = temp_dir.path().join("nested").join("auth.json");
        let auth_manager = test_auth_manager();
        let _auth_path_env = ScopedEnvVar::set("CITROS_AUTH_FILE", auth_path.to_str().unwrap());

        persist_auth_manager(&auth_manager).await.unwrap();

        assert!(auth_path.exists());

        let persisted = tokio::fs::read_to_string(&auth_path).await.unwrap();
        let persisted_json: serde_json::Value = serde_json::from_str(&persisted).unwrap();
        let expected_json: serde_json::Value =
            serde_json::from_str(&auth_manager.to_json().unwrap()).unwrap();

        assert_eq!(persisted_json, expected_json);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn persist_auth_manager_sets_0600_permissions_on_unix() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_dir = TempDir::new().unwrap();
        let auth_path = temp_dir.path().join("auth.json");
        let auth_manager = test_auth_manager();
        let _auth_path_env = ScopedEnvVar::set("CITROS_AUTH_FILE", auth_path.to_str().unwrap());

        persist_auth_manager(&auth_manager).await.unwrap();

        let mode = tokio::fs::metadata(&auth_path)
            .await
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn persist_then_load_round_trip_returns_equivalent_auth_manager() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_dir = TempDir::new().unwrap();
        let auth_path = temp_dir.path().join("auth.json");

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

        let _auth_path_env = ScopedEnvVar::set("CITROS_AUTH_FILE", auth_path.to_str().unwrap());

        persist_auth_manager(&expected).await.unwrap();
        let loaded = load_auth_manager().await.unwrap();

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
}
