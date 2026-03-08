use crate::auth_store::{open_auth_store_with_recovery, AuthStore};
use crate::prompts::{
    open_browser, parse_auth_selection, prompt_api_key_provider_with_surface,
    prompt_choice_with_surface, prompt_line, prompt_non_empty_line_with_surface,
    prompt_non_empty_secret_with_surface, PromptSurface,
};
use crate::tui::{build_router, fawx_data_dir};
use anyhow::{anyhow, Context};
use fx_auth::auth::{AuthManager, AuthMethod};
use fx_auth::oauth::{extract_openai_account_id, PkceFlow, TokenExchangeRequest, TokenResponse};
use fx_config::DEFAULT_CONFIG_TEMPLATE;
use fx_llm::{CompletionRequest, Message, ModelCatalog};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table};

const DEFAULT_HTTP_PORT: u16 = 8400;
const OPENAI_TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const OPENAI_CLIENT_ID: &str = fx_auth::oauth::OPENAI_CLIENT_ID;
#[cfg(test)]
static TEST_EXIT_CODE: LazyLock<Mutex<Option<i32>>> = LazyLock::new(|| Mutex::new(None));

pub async fn run(force: bool) -> anyhow::Result<i32> {
    #[cfg(test)]
    if let Some(exit_code) = take_test_exit_code() {
        return Ok(exit_code + i32::from(force));
    }

    println!("🦊 Welcome to Fawx setup!\n");

    let mut wizard = SetupWizard::new(force)?;
    wizard.print_system_check();
    if !wizard.confirm_existing_config()? {
        println!("Setup cancelled.");
        return Ok(0);
    }

    wizard.run_auth_phase().await?;
    wizard.run_model_phase().await?;
    wizard.run_http_phase()?;
    wizard.run_channels_phase().await?;
    wizard.run_validation_phase().await?;
    wizard.write_config()?;
    wizard.print_completion();
    Ok(0)
}

struct SetupWizard {
    data_dir: PathBuf,
    config_path: PathBuf,
    auth_store: AuthStore,
    auth_manager: AuthManager,
    config_document: DocumentMut,
    existing_config: bool,
    force: bool,
    store_recreated: bool,
    selected_provider: Option<String>,
    default_model: Option<String>,
    http: HttpSetup,
    telegram: Option<TelegramSetup>,
    webhooks: Vec<GenericWebhookSetup>,
}

#[derive(Default)]
struct HttpSetup {
    enabled: bool,
    port: u16,
}

struct TelegramSetup {
    allowed_chat_ids: Vec<i64>,
    validation_label: String,
}

struct GenericWebhookSetup {
    id: String,
    name: String,
    callback_url: String,
}

impl SetupWizard {
    fn new(force: bool) -> anyhow::Result<Self> {
        let data_dir = fawx_data_dir();
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("failed to create {}", data_dir.display()))?;
        let config_path = data_dir.join("config.toml");
        let existing_config = config_path.exists();
        let recovered = open_auth_store_with_recovery(&data_dir).map_err(|error| anyhow!(error))?;
        let store_recreated = recovered.recreated;
        let auth_store = recovered.store;
        let auth_manager = auth_store
            .load_auth_manager()
            .map_err(|error| anyhow!(error))?;
        let config_document = load_config_document(&config_path)?;
        Ok(Self {
            data_dir,
            config_path,
            auth_store,
            auth_manager,
            config_document,
            existing_config,
            force,
            store_recreated,
            selected_provider: None,
            default_model: None,
            http: HttpSetup {
                port: DEFAULT_HTTP_PORT,
                ..HttpSetup::default()
            },
            telegram: None,
            webhooks: Vec::new(),
        })
    }

    fn print_system_check(&self) {
        println!("Checking system...");
        println!("  ✓ Data directory: {}", self.data_dir.display());
        println!(
            "  {} Config file: {}",
            config_marker(self.existing_config),
            config_state(self.existing_config)
        );
        println!(
            "  {} Credential store: {}",
            credential_marker(self.store_recreated),
            credential_state(self.store_recreated)
        );
        println!();
    }

    fn confirm_existing_config(&self) -> anyhow::Result<bool> {
        if !self.existing_config || self.force {
            return Ok(true);
        }
        prompt_yes_no(
            "config.toml already exists. Continue and merge new settings? [y/N]: ",
            false,
        )
    }

    async fn run_auth_phase(&mut self) -> anyhow::Result<()> {
        println!("Step 1/5: LLM Provider");
        println!("  How would you like to authenticate?");
        println!("    [1] Claude subscription (setup token)");
        println!("    [2] ChatGPT subscription (browser sign-in)");
        println!("    [3] API key (Anthropic, OpenAI, OpenRouter, etc.)");

        let selection = prompt_choice_with_surface(
            PromptSurface::PlainTerminal,
            "  > ",
            "Please choose 1, 2, or 3.\n",
            "setup auth selection",
            parse_auth_selection,
        )?;

        match selection {
            crate::prompts::AuthSelection::ClaudeSubscription => self.store_claude_setup_token()?,
            crate::prompts::AuthSelection::ChatGptSubscription => self.store_openai_oauth().await?,
            crate::prompts::AuthSelection::ApiKey => self.store_api_key()?,
        }
        println!();
        Ok(())
    }

    fn store_claude_setup_token(&mut self) -> anyhow::Result<()> {
        let token = prompt_non_empty_secret_with_surface(
            PromptSurface::PlainTerminal,
            "  Claude setup token: ",
            "Token cannot be empty.\n",
            "Claude setup token",
        )?;
        self.auth_manager
            .store("anthropic", AuthMethod::SetupToken { token });
        self.persist_auth_manager()?;
        self.selected_provider = Some("anthropic".to_string());
        println!("  ✓ Claude setup token stored (encrypted)");
        Ok(())
    }

    async fn store_openai_oauth(&mut self) -> anyhow::Result<()> {
        let flow = PkceFlow::try_new().map_err(|error| anyhow!(error.to_string()))?;
        let auth_url = flow.authorization_url(OPENAI_CLIENT_ID);
        let auth_code = obtain_oauth_authorization_code(&flow, &auth_url).await?;
        let token_response = exchange_oauth_code_for_tokens(&flow, &auth_code).await?;
        let expires_at = now_ms().saturating_add(token_response.expires_in.saturating_mul(1_000));
        let account_id = extract_openai_account_id(&token_response.access_token);
        let method = AuthMethod::OAuth {
            provider: "openai".to_string(),
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_at,
            account_id,
        };
        self.auth_manager.store("openai", method);
        self.persist_auth_manager()?;
        self.selected_provider = Some("openai".to_string());
        println!("  ✓ ChatGPT subscription stored (encrypted)");
        Ok(())
    }

    fn store_api_key(&mut self) -> anyhow::Result<()> {
        let provider = prompt_api_key_provider_with_surface(PromptSurface::PlainTerminal)?;
        let key = prompt_non_empty_secret_with_surface(
            PromptSurface::PlainTerminal,
            &format!("  Enter your {provider} API key: "),
            "API key cannot be empty.\n",
            "API key",
        )?;
        let method = AuthMethod::ApiKey {
            provider: provider.clone(),
            key,
        };
        self.auth_manager.store(&provider, method);
        self.persist_auth_manager()?;
        self.selected_provider = Some(provider.clone());
        println!("  ✓ {provider} API key stored (encrypted)");
        Ok(())
    }

    async fn run_model_phase(&mut self) -> anyhow::Result<()> {
        println!("Step 2/5: Model Selection");
        println!("  Fetching available models...");
        let provider = self
            .selected_provider
            .clone()
            .context("provider not selected")?;
        let models = self.available_models(&provider).await?;
        print_models(&provider, &models);
        let selection = prompt_list_selection("  > ", models.len())?;
        let model = models[selection - 1].clone();
        self.default_model = Some(model.clone());
        println!("  ✓ Default model: {model}");
        println!();
        Ok(())
    }

    async fn available_models(&self, provider: &str) -> anyhow::Result<Vec<String>> {
        let dynamic = fetch_catalog_models(provider, self.auth_manager.get(provider)).await?;
        if !dynamic.is_empty() {
            return Ok(dynamic);
        }
        fallback_models(&self.auth_manager, provider)
    }

    fn run_http_phase(&mut self) -> anyhow::Result<()> {
        println!("Step 3/5: HTTP API");
        let enable = prompt_yes_no("  Enable HTTP API? [Y/n]: ", true)?;
        if !enable {
            println!("  ✓ HTTP API disabled");
            println!();
            return Ok(());
        }
        self.http.enabled = true;
        let bearer_token = random_hex(32)?;
        self.auth_store
            .store_provider_token("http_bearer", &bearer_token)
            .map_err(|error| anyhow!(error))?;
        println!("  ✓ Bearer token generated and stored (encrypted)");
        println!("  ✓ Port: {}", self.http.port);
        println!();
        Ok(())
    }

    async fn run_channels_phase(&mut self) -> anyhow::Result<()> {
        println!("Step 4/5: Channels");
        if !self.http.enabled {
            println!("  HTTP API is disabled; skipping channel setup.\n");
            return Ok(());
        }
        if !prompt_yes_no("  Set up a messaging channel? [y/N]: ", false)? {
            println!("  Skipping channels.\n");
            return Ok(());
        }
        loop {
            match prompt_channel_selection()? {
                ChannelSelection::Telegram => self.configure_telegram().await?,
                ChannelSelection::Webhook => self.configure_generic_webhook()?,
                ChannelSelection::Skip => break,
            }
            if !prompt_yes_no("  Add another channel? [y/N]: ", false)? {
                break;
            }
        }
        println!();
        Ok(())
    }

    async fn configure_telegram(&mut self) -> anyhow::Result<()> {
        let token = prompt_non_empty_secret_with_surface(
            PromptSurface::PlainTerminal,
            "  Telegram bot token: ",
            "Telegram bot token cannot be empty.\n",
            "Telegram bot token",
        )?;
        let allowed_chat_ids = prompt_telegram_chat_ids()?;
        let validation_label = validate_telegram_token(&token)
            .await
            .unwrap_or_else(|error| {
                println!("  ! Telegram validation failed: {error}");
                "validation skipped".to_string()
            });
        let webhook_secret = random_hex(32)?;
        store_telegram_credentials(&self.auth_store, &token, &webhook_secret)?;
        self.telegram = Some(TelegramSetup {
            allowed_chat_ids,
            validation_label,
        });
        println!("  ✓ Telegram channel configured (token encrypted)");
        println!("  ✓ Webhook secret generated for validation");
        Ok(())
    }

    fn configure_generic_webhook(&mut self) -> anyhow::Result<()> {
        let id = prompt_required("  Webhook channel ID: ")?;
        let name = prompt_required("  Webhook channel name: ")?;
        let callback_url = prompt_required("  Callback URL: ")?;
        self.webhooks.push(GenericWebhookSetup {
            id,
            name,
            callback_url,
        });
        println!("  ✓ Webhook channel added");
        Ok(())
    }

    async fn run_validation_phase(&mut self) -> anyhow::Result<()> {
        println!("Step 5/5: Validation");
        self.validate_model_connection().await?;
        self.validate_saved_telegram();
        println!("  ✓ Credential store: healthy");
        println!();
        Ok(())
    }

    async fn validate_model_connection(&self) -> anyhow::Result<()> {
        let model = self
            .default_model
            .as_deref()
            .context("model not selected")?;
        let mut router =
            build_router(&self.auth_manager).map_err(|error| anyhow!(error.to_string()))?;
        router
            .set_active(model)
            .map_err(|error| anyhow!(error.to_string()))?;
        let request = CompletionRequest {
            model: model.to_string(),
            messages: vec![Message::user("Reply with OK.")],
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(16),
            system_prompt: Some("Reply with OK.".to_string()),
            thinking: None,
        };
        router
            .complete(request)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        println!("  ✓ Model connection: {model}");
        Ok(())
    }

    fn validate_saved_telegram(&self) {
        if let Some(telegram) = &self.telegram {
            println!("  ✓ Telegram: {}", telegram.validation_label);
        }
    }

    fn write_config(&mut self) -> anyhow::Result<()> {
        if let Some(model) = self.default_model.clone() {
            set_string(
                &mut self.config_document,
                &["model"],
                "default_model",
                &model,
            )?;
        }
        if self.http.enabled {
            set_integer(
                &mut self.config_document,
                &["http"],
                "port",
                i64::from(self.http.port),
            )?;
        }
        if let Some(telegram) = &self.telegram {
            write_telegram_config(&mut self.config_document, telegram)?;
        }
        if !self.webhooks.is_empty() {
            write_webhook_config(&mut self.config_document, &self.webhooks)?;
        }
        fs::create_dir_all(&self.data_dir)?;
        fs::write(&self.config_path, self.config_document.to_string())?;
        println!("✓ Config written: {}", self.config_path.display());
        Ok(())
    }

    fn print_completion(&self) {
        for line in completion_lines() {
            println!("{line}");
        }
    }

    fn persist_auth_manager(&self) -> anyhow::Result<()> {
        self.auth_store
            .save_auth_manager(&self.auth_manager)
            .map_err(|error| anyhow!(error))
    }
}

fn config_marker(existing_config: bool) -> &'static str {
    if existing_config {
        "!"
    } else {
        "✓"
    }
}

fn config_state(existing_config: bool) -> &'static str {
    if existing_config {
        "found (will merge)"
    } else {
        "not found (will create)"
    }
}

fn credential_marker(recreated: bool) -> &'static str {
    if recreated {
        "!"
    } else {
        "✓"
    }
}

fn credential_state(recreated: bool) -> &'static str {
    if recreated {
        "Credential store from a different installation detected. Recreated."
    } else {
        "healthy"
    }
}

fn completion_lines() -> [&'static str; 3] {
    [
        "Setup complete! Next steps:",
        "  fawx serve --http    — start the engine",
        "  fawx-tui             — connect the terminal UI (requires engine running)",
    ]
}

fn load_config_document(config_path: &Path) -> anyhow::Result<DocumentMut> {
    let content = if config_path.exists() {
        fs::read_to_string(config_path)?
    } else {
        DEFAULT_CONFIG_TEMPLATE.to_string()
    };
    content
        .parse::<DocumentMut>()
        .map_err(|error| anyhow!("invalid config: {error}"))
}

fn prompt_yes_no(prompt: &str, default_yes: bool) -> anyhow::Result<bool> {
    let value = prompt_line(prompt)?;
    if value.is_empty() {
        return Ok(default_yes);
    }
    let normalized = value.trim().to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
}

fn prompt_required(prompt: &str) -> anyhow::Result<String> {
    prompt_non_empty_line_with_surface(
        PromptSurface::PlainTerminal,
        prompt,
        "Please enter a value.\n",
        "required input",
    )
    .map_err(Into::into)
}

fn prompt_list_selection(prompt: &str, len: usize) -> anyhow::Result<usize> {
    prompt_choice_with_surface(
        PromptSurface::PlainTerminal,
        prompt,
        "Please choose one of the numbered options.\n",
        "list selection",
        |value| parse_list_selection(value, len),
    )
    .map_err(Into::into)
}

fn parse_list_selection(value: &str, len: usize) -> Option<usize> {
    let parsed = value.trim().parse::<usize>().ok()?;
    (1..=len).contains(&parsed).then_some(parsed)
}

fn prompt_channel_selection() -> anyhow::Result<ChannelSelection> {
    println!("    [1] Telegram");
    println!("    [2] Webhook (generic HTTP)");
    println!("    [3] Skip");
    prompt_choice_with_surface(
        PromptSurface::PlainTerminal,
        "  > ",
        "Please choose 1, 2, or 3.\n",
        "channel selection",
        parse_channel_selection,
    )
    .map_err(Into::into)
}

pub(crate) fn parse_channel_selection(value: &str) -> Option<ChannelSelection> {
    match value.trim() {
        "1" => Some(ChannelSelection::Telegram),
        "2" => Some(ChannelSelection::Webhook),
        "3" => Some(ChannelSelection::Skip),
        _ => None,
    }
}

fn prompt_telegram_chat_ids() -> anyhow::Result<Vec<i64>> {
    let value =
        prompt_line("  Restrict to specific chat IDs? (comma-separated, or Enter to allow all): ")?;
    parse_chat_ids(&value)
}

fn parse_chat_ids(value: &str) -> anyhow::Result<Vec<i64>> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|entry| {
            entry
                .trim()
                .parse::<i64>()
                .map_err(|_| anyhow!("invalid chat id: {entry}"))
        })
        .collect()
}

async fn fetch_catalog_models(
    provider: &str,
    method: Option<&AuthMethod>,
) -> anyhow::Result<Vec<String>> {
    let Some(method) = method else {
        return Ok(Vec::new());
    };
    let mut catalog = ModelCatalog::new();
    let auth_mode = auth_mode_for_method(method, provider)?;
    let credential = auth_credential_for_method(method)?;
    let models = catalog.get_models(provider, &credential, auth_mode).await;
    Ok(unique_catalog_model_ids(models))
}

fn auth_mode_for_method(method: &AuthMethod, provider: &str) -> anyhow::Result<&'static str> {
    match method {
        AuthMethod::SetupToken { .. } => Ok("setup_token"),
        AuthMethod::OAuth { .. } => Ok("oauth"),
        AuthMethod::ApiKey { .. } if provider == "anthropic" => Ok("api_key"),
        AuthMethod::ApiKey { .. } => Ok("bearer"),
    }
}

fn auth_credential_for_method(method: &AuthMethod) -> anyhow::Result<String> {
    match method {
        AuthMethod::SetupToken { token } => Ok(token.clone()),
        AuthMethod::ApiKey { key, .. } => Ok(key.clone()),
        AuthMethod::OAuth { access_token, .. } => Ok(access_token.clone()),
    }
}

fn unique_catalog_model_ids(models: Vec<fx_llm::CatalogModel>) -> Vec<String> {
    let mut ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

fn fallback_models(auth_manager: &AuthManager, provider: &str) -> anyhow::Result<Vec<String>> {
    let router = build_router(auth_manager).map_err(|error| anyhow!(error.to_string()))?;
    let models = router
        .available_models()
        .into_iter()
        .filter(|model| model.provider_name == provider)
        .map(|model| model.model_id)
        .collect::<Vec<_>>();
    if models.is_empty() {
        Err(anyhow!("no models available for {provider}"))
    } else {
        Ok(models)
    }
}

fn print_models(provider: &str, models: &[String]) {
    println!("  Available models for {provider}:");
    for (index, model) in models.iter().enumerate() {
        println!("    [{}] {}", index + 1, model);
    }
}

async fn obtain_oauth_authorization_code(
    flow: &PkceFlow,
    auth_url: &str,
) -> anyhow::Result<String> {
    println!("  Opening browser for ChatGPT sign-in...");
    if let Err(error) = open_browser(auth_url) {
        println!("  Couldn't open a browser automatically: {error}");
        println!("  Open this URL manually:\n  {auth_url}");
    }
    let input = prompt_required("  Paste the redirect URL or authorization code: ")?;
    match flow.parse_callback(&input) {
        Ok(code) => Ok(code),
        Err(_) => Ok(input.trim().to_string()),
    }
}

async fn exchange_oauth_code_for_tokens(
    flow: &PkceFlow,
    authorization_code: &str,
) -> anyhow::Result<TokenResponse> {
    let request = TokenExchangeRequest {
        grant_type: "authorization_code".to_string(),
        code: authorization_code.to_string(),
        redirect_uri: flow.redirect_uri().to_string(),
        code_verifier: flow.code_verifier().to_string(),
        client_id: OPENAI_CLIENT_ID.to_string(),
    };
    let response = reqwest::Client::new()
        .post(OPENAI_TOKEN_ENDPOINT)
        .form(&request)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(anyhow!("oauth token exchange failed ({status}): {body}"));
    }
    serde_json::from_str(&body).map_err(Into::into)
}

#[derive(Deserialize)]
#[serde(default)]
struct TelegramGetMeResponse {
    ok: bool,
    result: Option<TelegramUser>,
    description: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TelegramUser {
    username: Option<String>,
    first_name: Option<String>,
}

impl Default for TelegramGetMeResponse {
    fn default() -> Self {
        Self {
            ok: false,
            result: None,
            description: None,
        }
    }
}

async fn validate_telegram_token(token: &str) -> anyhow::Result<String> {
    let url = format!("https://api.telegram.org/bot{token}/getMe");
    let response: TelegramGetMeResponse = reqwest::get(&url).await?.json().await?;
    if !response.ok {
        let description = response
            .description
            .unwrap_or_else(|| "unknown error".to_string());
        return Err(anyhow!(description));
    }
    let label = response
        .result
        .and_then(|user| user.username.or(user.first_name))
        .unwrap_or_else(|| "bot connected".to_string());
    Ok(label)
}

fn store_telegram_credentials(
    auth_store: &AuthStore,
    bot_token: &str,
    webhook_secret: &str,
) -> anyhow::Result<()> {
    auth_store
        .store_provider_token("telegram_bot_token", bot_token)
        .map_err(|error| anyhow!(error))?;
    auth_store
        .store_provider_token("telegram_webhook_secret", webhook_secret)
        .map_err(|error| anyhow!(error))?;
    Ok(())
}

fn write_telegram_config(
    document: &mut DocumentMut,
    telegram: &TelegramSetup,
) -> anyhow::Result<()> {
    set_bool(document, &["telegram"], "enabled", true)?;
    set_integer_array(
        document,
        &["telegram"],
        "allowed_chat_ids",
        &telegram.allowed_chat_ids,
    )?;
    Ok(())
}

fn write_webhook_config(
    document: &mut DocumentMut,
    webhooks: &[GenericWebhookSetup],
) -> anyhow::Result<()> {
    set_bool(document, &["webhook"], "enabled", true)?;
    let table = table_mut(document, &["webhook"])?;
    let mut channels = ArrayOfTables::new();
    for webhook in webhooks {
        let mut channel = Table::new();
        channel["id"] = value(&webhook.id);
        channel["name"] = value(&webhook.name);
        channel["callback_url"] = value(&webhook.callback_url);
        channels.push(channel);
    }
    table["channels"] = Item::ArrayOfTables(channels);
    Ok(())
}

fn set_string(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: &str,
) -> anyhow::Result<()> {
    let table = table_mut(document, sections)?;
    table[key] = value(field_value);
    Ok(())
}

fn set_bool(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: bool,
) -> anyhow::Result<()> {
    let table = table_mut(document, sections)?;
    table[key] = value(field_value);
    Ok(())
}

fn set_integer(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: i64,
) -> anyhow::Result<()> {
    let table = table_mut(document, sections)?;
    table[key] = value(field_value);
    Ok(())
}

fn set_integer_array(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_values: &[i64],
) -> anyhow::Result<()> {
    let table = table_mut(document, sections)?;
    let mut array = Array::new();
    for value in field_values {
        array.push(*value);
    }
    table[key] = Item::Value(array.into());
    Ok(())
}

fn table_mut<'a>(
    document: &'a mut DocumentMut,
    sections: &[&str],
) -> anyhow::Result<&'a mut Table> {
    table_mut_in(document.as_table_mut(), sections)
}

fn table_mut_in<'a>(table: &'a mut Table, sections: &[&str]) -> anyhow::Result<&'a mut Table> {
    let Some((section, rest)) = sections.split_first() else {
        return Ok(table);
    };
    if !table.contains_key(section) {
        table[*section] = Item::Table(Table::new());
    }
    let child = table[*section]
        .as_table_mut()
        .ok_or_else(|| anyhow!("config section '{section}' must be a table"))?;
    table_mut_in(child, rest)
}

fn random_hex(bytes: usize) -> anyhow::Result<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut buffer = vec![0u8; bytes];
    rng.fill(&mut buffer)
        .map_err(|_| anyhow!("failed to generate secure random bytes"))?;
    Ok(buffer.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn now_ms() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration_millis_u64(elapsed)
}

fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
pub(crate) fn set_test_exit_code(exit_code: i32) {
    *TEST_EXIT_CODE.lock().expect("setup test exit code lock") = Some(exit_code);
}

#[cfg(test)]
fn take_test_exit_code() -> Option<i32> {
    TEST_EXIT_CODE
        .lock()
        .expect("setup test exit code lock")
        .take()
}

#[derive(Clone, Copy)]
pub(crate) enum ChannelSelection {
    Telegram,
    Webhook,
    Skip,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chat_ids_accepts_blank_input() {
        assert!(parse_chat_ids("  ").expect("blank input").is_empty());
    }

    #[test]
    fn parse_chat_ids_parses_commas() {
        let ids = parse_chat_ids("123, -456, 789").expect("chat ids");
        assert_eq!(ids, vec![123, -456, 789]);
    }

    #[test]
    fn parse_chat_ids_rejects_invalid_values() {
        assert!(parse_chat_ids("123, nope").is_err());
    }

    #[test]
    fn parse_list_selection_honors_bounds() {
        assert_eq!(parse_list_selection("1", 3), Some(1));
        assert_eq!(parse_list_selection("3", 3), Some(3));
        assert_eq!(parse_list_selection("4", 3), None);
        assert_eq!(parse_list_selection("x", 3), None);
    }

    #[test]
    fn completion_lines_match_headless_engine_workflow() {
        assert_eq!(
            completion_lines(),
            [
                "Setup complete! Next steps:",
                "  fawx serve --http    — start the engine",
                "  fawx-tui             — connect the terminal UI (requires engine running)",
            ]
        );
    }

    #[test]
    fn write_config_keeps_telegram_webhook_secret_out_of_toml() {
        let mut document = load_config_document(Path::new("config.toml")).expect("document");
        set_string(
            &mut document,
            &["model"],
            "default_model",
            "claude-sonnet-4",
        )
        .expect("model");
        set_integer(&mut document, &["http"], "port", 8400).expect("port");
        write_telegram_config(
            &mut document,
            &TelegramSetup {
                allowed_chat_ids: vec![1, 2],
                validation_label: "bot".to_string(),
            },
        )
        .expect("telegram");
        write_webhook_config(
            &mut document,
            &[GenericWebhookSetup {
                id: "alerts".to_string(),
                name: "Alerts".to_string(),
                callback_url: "https://example.com/hook".to_string(),
            }],
        )
        .expect("webhook");

        let rendered = document.to_string();
        assert!(rendered.contains("default_model = \"claude-sonnet-4\""));
        assert!(rendered.contains("port = 8400"));
        assert!(rendered.contains("enabled = true"));
        assert!(rendered.contains("allowed_chat_ids = [1, 2]"));
        assert!(!rendered.contains("webhook_secret"));
        assert!(rendered.contains("[[webhook.channels]]"));
        assert!(rendered.contains("callback_url = \"https://example.com/hook\""));
    }

    #[test]
    fn store_telegram_credentials_persists_bot_token_and_webhook_secret() {
        let store = AuthStore::open_for_testing().expect("test auth store");

        store_telegram_credentials(&store, "bot-token", "secret-token").expect("store secrets");

        let bot_token = store
            .get_provider_token("telegram_bot_token")
            .expect("load bot token")
            .expect("bot token present");
        let webhook_secret = store
            .get_provider_token("telegram_webhook_secret")
            .expect("load webhook secret")
            .expect("webhook secret present");
        assert_eq!(*bot_token, "bot-token");
        assert_eq!(*webhook_secret, "secret-token");
    }

    #[test]
    fn duration_millis_u64_clamps_on_overflow() {
        let duration = Duration::from_secs(u64::MAX);
        assert_eq!(duration_millis_u64(duration), u64::MAX);
    }

    #[test]
    fn telegram_get_me_response_defaults_missing_optional_fields() {
        let response: TelegramGetMeResponse =
            serde_json::from_str(r#"{"ok":true}"#).expect("deserialize response");

        assert!(response.ok);
        assert!(response.result.is_none());
        assert!(response.description.is_none());
    }
}
