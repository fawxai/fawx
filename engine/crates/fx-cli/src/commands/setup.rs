use crate::auth_store::{open_auth_store_with_recovery, AuthStore};
use crate::prompts::{
    open_browser, parse_auth_selection, prompt_api_key_provider_with_surface,
    prompt_choice_with_surface, prompt_line, prompt_non_empty_line_with_surface,
    prompt_non_empty_secret_with_surface, PromptSurface,
};
use crate::startup::{build_router, fawx_data_dir};
use anyhow::{anyhow, Context};
use fx_auth::auth::{AuthManager, AuthMethod};
use fx_auth::credential_store::{
    AuthProvider, CredentialStore as SkillCredentialStoreTrait, EncryptedFileCredentialStore,
};
use fx_auth::oauth::{extract_openai_account_id, PkceFlow, TokenExchangeRequest, TokenResponse};
use fx_config::{PermissionAction, PermissionPreset, PermissionsConfig, DEFAULT_CONFIG_TEMPLATE};
use fx_llm::{CompletionRequest, Message, ModelCatalog};
use serde::Deserialize;
use std::collections::BTreeSet;
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
    wizard.run_tailscale_phase();
    if !wizard.confirm_existing_config()? {
        println!("Setup cancelled.");
        return Ok(0);
    }

    wizard.run_auth_phase().await?;
    wizard.run_model_phase().await?;
    wizard.run_permissions_phase()?;
    let skill_state = wizard.run_skills_phase()?;
    wizard.run_skill_credentials_phase(&skill_state)?;
    wizard.run_http_phase()?;
    wizard.run_channels_phase().await?;
    wizard.run_validation_phase().await?;
    wizard.run_launchagent_phase();
    wizard.write_config()?;
    wizard.print_completion();
    Ok(0)
}

struct SetupWizard {
    data_dir: PathBuf,
    config_path: PathBuf,
    auth_store: AuthStore,
    auth_manager: AuthManager,
    skill_credential_store: Option<EncryptedFileCredentialStore>,
    config_document: DocumentMut,
    existing_config: bool,
    force: bool,
    store_recreated: bool,
    selected_provider: Option<String>,
    default_model: Option<String>,
    permissions_preset: Option<PermissionPreset>,
    selected_skills: Vec<&'static SetupSkill>,
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

#[derive(Clone, Copy)]
struct SetupSkill {
    name: &'static str,
    label: &'static str,
    default_selected: bool,
    auth: SkillAuth,
}

#[derive(Clone, Copy)]
enum SkillAuth {
    Free,
    OpenAiReuse,
    Credential {
        key: &'static str,
        label: &'static str,
    },
}

#[derive(Clone, Copy)]
struct SkillToggle {
    skill: &'static SetupSkill,
    selected: bool,
}

struct SkillWizardState {
    openai_configured: bool,
    installed_skills: BTreeSet<String>,
    stored_credentials: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CredentialPrompt {
    key: &'static str,
    label: &'static str,
}

static SETUP_SKILLS: [SetupSkill; 7] = [
    SetupSkill {
        name: "calculator",
        label: "Calculator",
        default_selected: true,
        auth: SkillAuth::Free,
    },
    SetupSkill {
        name: "weather",
        label: "Weather",
        default_selected: true,
        auth: SkillAuth::Free,
    },
    SetupSkill {
        name: "canvas",
        label: "Canvas",
        default_selected: true,
        auth: SkillAuth::Free,
    },
    SetupSkill {
        name: "tts",
        label: "TTS",
        default_selected: false,
        auth: SkillAuth::OpenAiReuse,
    },
    SetupSkill {
        name: "stt",
        label: "STT",
        default_selected: false,
        auth: SkillAuth::OpenAiReuse,
    },
    SetupSkill {
        name: "browser",
        label: "Browser",
        default_selected: false,
        auth: SkillAuth::Credential {
            key: "brave_api_key",
            label: "Brave API key",
        },
    },
    SetupSkill {
        name: "github",
        label: "GitHub",
        default_selected: false,
        auth: SkillAuth::Credential {
            key: "github_token",
            label: "GitHub Personal Access Token",
        },
    },
];

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
            skill_credential_store: None,
            config_document,
            existing_config,
            force,
            store_recreated,
            selected_provider: None,
            default_model: None,
            permissions_preset: None,
            selected_skills: Vec::new(),
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
        println!("Step 1: LLM Provider");
        println!("  How would you like to authenticate?");
        println!("    [1] Claude subscription (setup token)");
        println!("    [2] ChatGPT subscription (browser sign-in)");
        println!("    [3] API key (Anthropic, OpenAI, OpenRouter, etc.)");
        println!("    [4] Skip (configure later)");

        let selection = prompt_choice_with_surface(
            PromptSurface::PlainTerminal,
            "  > ",
            "Please choose 1-4 (or press Enter to skip).\n",
            "setup auth selection",
            parse_auth_selection,
        )?;

        match selection {
            crate::prompts::AuthSelection::ClaudeSubscription => self.store_claude_setup_token()?,
            crate::prompts::AuthSelection::ChatGptSubscription => self.store_openai_oauth().await?,
            crate::prompts::AuthSelection::ApiKey => self.store_api_key()?,
            crate::prompts::AuthSelection::Skip => {
                println!("  ⏭ Skipped LLM provider setup (configure later with `fawx setup`)");
            }
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
        let Some(provider) = self.selected_provider.clone() else {
            println!("Step 2: Model Selection");
            println!("  ⏭ Skipped (no provider configured yet)");
            println!();
            return Ok(());
        };
        println!("Step 2: Model Selection");
        println!("  Fetching available models...");
        let models = self.available_models(&provider).await?;
        print_models(&provider, &models);
        println!("  (press Enter to skip)");
        loop {
            let input = prompt_line("  > ")?;
            if is_skip_input(&input) {
                let fallback =
                    default_model_for_provider(self.selected_provider.as_deref(), &models);
                self.default_model = Some(fallback.clone());
                println!("  ⏭ Skipped model selection (using {fallback})");
                println!();
                return Ok(());
            }
            if let Some(selection) = parse_list_selection(&input, models.len()) {
                let model = models[selection - 1].clone();
                self.default_model = Some(model.clone());
                println!("  ✓ Default model: {model}");
                println!();
                return Ok(());
            }
            println!("  Invalid choice, please enter a number from the list above.");
        }
    }

    fn run_permissions_phase(&mut self) -> anyhow::Result<()> {
        println!("Step 3: Permissions");
        println!("  Choose how much autonomy Fawx has:");
        println!(
            "    [1] 🔥 Power — full workspace autonomy, proposals for external actions (recommended)"
        );
        println!("    [2] 🔒 Cautious — proposals required for all writes and code execution");
        println!("    [3] 🧪 Experimental — maximum autonomy including kernel self-modification");
        println!("  (press Enter to skip, defaults to Power)");
        loop {
            let input = prompt_line("  > ")?;
            if is_skip_input(&input) {
                self.permissions_preset = Some(PermissionPreset::Power);
                println!("  ⏭ Using default: Power");
                println!();
                return Ok(());
            }
            if let Some(preset) = parse_permissions_selection(&input) {
                self.permissions_preset = Some(preset);
                println!(
                    "  ✓ Permissions: {} ({})",
                    permission_preset_label(preset),
                    permission_preset_summary(preset)
                );
                println!();
                return Ok(());
            }
            println!("  Invalid choice, please enter 1, 2, or 3.");
        }
    }

    async fn available_models(&self, provider: &str) -> anyhow::Result<Vec<String>> {
        let dynamic = fetch_catalog_models(provider, self.auth_manager.get(provider)).await?;
        if !dynamic.is_empty() {
            return Ok(dynamic);
        }
        fallback_models(&self.auth_manager, provider)
    }

    fn run_skills_phase(&mut self) -> anyhow::Result<SkillWizardState> {
        println!("Step 4: Skills");
        let state = self.skill_wizard_state()?;
        self.selected_skills = prompt_skill_selection(&state)?
            .into_iter()
            .filter(|toggle| toggle.selected)
            .map(|toggle| toggle.skill)
            .collect();
        print_selected_skill_status(&state, &self.selected_skills);
        println!();
        Ok(state)
    }

    fn skill_credential_store(&mut self) -> anyhow::Result<&EncryptedFileCredentialStore> {
        if self.skill_credential_store.is_none() {
            let store = EncryptedFileCredentialStore::open(&self.data_dir)
                .map_err(|error| anyhow!(error))?;
            self.skill_credential_store = Some(store);
        }
        self.skill_credential_store
            .as_ref()
            .ok_or_else(|| anyhow!("skill credential store unavailable"))
    }

    fn run_skill_credentials_phase(&mut self, state: &SkillWizardState) -> anyhow::Result<()> {
        println!("Step 5: Skill Credentials");
        let reuse_messages = provider_reuse_messages(&self.selected_skills, state);
        let requirement_messages = provider_requirement_messages(&self.selected_skills, state);
        let prompts = credential_prompts(&self.selected_skills, state);
        print_skill_messages(&reuse_messages);
        print_skill_messages(&requirement_messages);
        if prompts.is_empty() && reuse_messages.is_empty() && requirement_messages.is_empty() {
            println!("  No additional skill credentials needed.");
        }
        for prompt in prompts {
            self.prompt_for_skill_credential(prompt)?;
        }
        println!();
        Ok(())
    }

    fn skill_wizard_state(&mut self) -> anyhow::Result<SkillWizardState> {
        let installed_skills = installed_skill_names(&self.data_dir)?;
        let store = self.skill_credential_store()?;
        let mut stored_credentials = store
            .list_generic_names()
            .map_err(|error| anyhow!(error))?
            .into_iter()
            .collect::<BTreeSet<_>>();
        if github_token_configured(store)? {
            stored_credentials.insert("github_token".to_string());
        }
        Ok(SkillWizardState {
            openai_configured: self.auth_manager.get("openai").is_some(),
            installed_skills,
            stored_credentials,
        })
    }

    fn prompt_for_skill_credential(&mut self, prompt: CredentialPrompt) -> anyhow::Result<()> {
        let value = prompt_line(&format!(
            "  {} (optional, press enter to skip): ",
            prompt.label
        ))?;
        if value.is_empty() {
            println!(
                "  - {} not stored; the skill will prompt later.",
                prompt.label
            );
            return Ok(());
        }
        self.store_skill_credential(prompt.key, &value)?;
        println!("  ✓ Stored securely");
        Ok(())
    }

    fn store_skill_credential(&mut self, key: &str, value: &str) -> anyhow::Result<()> {
        Ok(self.skill_credential_store()?.set_generic(key, value)?)
    }

    fn run_http_phase(&mut self) -> anyhow::Result<()> {
        println!("Step 6: HTTP API");
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
        println!("Step 7: Channels");
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
        println!("Step 8: Validation");
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
        match router.complete(request).await {
            Ok(_) => {
                println!("  ✓ Model connection: {model}");
                Ok(())
            }
            Err(error) => {
                eprintln!("  ✗ Validation failed for {model}: {error}");
                eprintln!(
                    "    Auth method: {:?}",
                    self.auth_manager
                        .get(model.split('/').next().unwrap_or("unknown"))
                );
                Err(anyhow!(error.to_string()))
            }
        }
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
        if let Some(preset) = self.permissions_preset {
            write_permissions_preset(&mut self.config_document, preset)?;
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

    fn run_tailscale_phase(&self) {
        println!("\n── Tailscale ──\n");
        match std::process::Command::new("tailscale")
            .args(["status", "--json"])
            .output()
        {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                println!("  Tailscale not found.");
                println!("  Install from https://tailscale.com/download for secure remote access.");
                println!("  (Optional — Fawx works without it for local use.)");
            }
            Err(_) => {
                println!("  Tailscale is installed but status unavailable.");
            }
            Ok(output) if !output.status.success() => {
                println!("  Tailscale is installed but not responding.");
            }
            Ok(output) => report_tailscale_status(self, &output.stdout),
        }
    }

    #[cfg(all(target_os = "macos", feature = "http"))]
    fn run_launchagent_phase(&self) {
        println!("\n── Auto-Start ──\n");
        let answer = launchagent_answer_from_prompt(prompt_line(
            "  Start Fawx automatically when you log in? [Y/n] ",
        ));
        if should_install_launchagent(&answer) {
            let binary_path = match std::env::current_exe() {
                Ok(path) => path,
                Err(e) => {
                    println!("  ⚠ Could not determine binary path: {e}");
                    println!("  Skipping LaunchAgent install.");
                    return;
                }
            };
            let config = fx_api::launchagent::LaunchAgentConfig {
                server_binary_path: binary_path,
                port: if self.http.enabled {
                    self.http.port
                } else {
                    DEFAULT_HTTP_PORT
                },
                data_dir: self.data_dir.clone(),
                log_path: self.data_dir.join("server.log"),
                auto_start: true,
                keep_alive: true,
            };
            match fx_api::launchagent::install(&config) {
                Ok(()) => println!("  ✅ LaunchAgent installed — Fawx will start on login"),
                Err(e) => println!("  ⚠ Could not install LaunchAgent: {e}"),
            }
        } else {
            println!("  Skipped auto-start.");
        }
    }

    #[cfg(all(target_os = "macos", not(feature = "http")))]
    fn run_launchagent_phase(&self) {
        println!("\n── Auto-Start ──\n");
        println!("  Auto-start setup is unavailable in this build.");
    }

    #[cfg(not(target_os = "macos"))]
    fn run_launchagent_phase(&self) {
        // LaunchAgent is macOS-only; skip silently on other platforms
    }
}

fn report_tailscale_status(wizard: &SetupWizard, status_stdout: &[u8]) {
    let json: serde_json::Value = match serde_json::from_slice(status_stdout) {
        Ok(json) => json,
        Err(_) => {
            println!("  Tailscale is installed but status unreadable.");
            return;
        }
    };
    let backend = json
        .get("BackendState")
        .and_then(|value| value.as_str())
        .unwrap_or("Unknown");
    if backend == "Running" {
        println!("  ✅ Tailscale is running");
        let hostname = json
            .pointer("/Self/DNSName")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim_end_matches('.');
        run_tailscale_cert(wizard, hostname);
    } else {
        println!("  Tailscale is installed but not logged in.");
        println!("  Run 'tailscale login' to enable secure remote access.");
    }
}

fn run_tailscale_cert(wizard: &SetupWizard, hostname: &str) {
    if hostname.is_empty() {
        return;
    }

    println!("  Running tailscale cert...");
    match super::tailscale::generate_cert_files(hostname, &wizard.data_dir) {
        Ok((cert_path, _)) => {
            println!("  ✅ HTTPS certificate ready at {}", cert_path.display());
        }
        Err(_) => {
            println!("  ⚠ Could not generate certificate. You can set this up later.");
        }
    }
}

fn launchagent_answer_from_prompt<E>(result: Result<String, E>) -> String {
    result.unwrap_or_else(|_| "n".to_string())
}

fn should_install_launchagent(answer: &str) -> bool {
    answer.is_empty() || answer.starts_with('y') || answer.starts_with('Y')
}

fn installed_skill_names(data_dir: &Path) -> anyhow::Result<BTreeSet<String>> {
    let skills_dir = data_dir.join("skills");
    if !skills_dir.exists() {
        return Ok(BTreeSet::new());
    }
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(skills_dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            names.insert(entry.file_name().to_string_lossy().to_string());
        }
    }
    Ok(names)
}

fn github_token_configured(store: &EncryptedFileCredentialStore) -> anyhow::Result<bool> {
    store
        .get(
            AuthProvider::GitHub,
            fx_auth::credential_store::CredentialMethod::Pat,
        )
        .map(|value| value.is_some())
        .map_err(|error| anyhow!(error))
}

fn prompt_skill_selection(state: &SkillWizardState) -> anyhow::Result<Vec<SkillToggle>> {
    let mut toggles = SETUP_SKILLS
        .iter()
        .map(|skill| SkillToggle {
            skill,
            selected: skill.default_selected,
        })
        .collect::<Vec<_>>();
    let mut has_toggled = false;
    loop {
        print_skill_selection_prompt(&toggles, state);
        println!("  (press Enter to confirm, or type \"skip\" to skip)");
        let input = prompt_line("  > ")?;
        if is_skip_input(&input) {
            if has_toggled {
                println!("  ✓ Skills configured");
            } else {
                println!("  ⏭ Using default skills selection");
            }
            return Ok(toggles);
        }
        let indexes = parse_toggle_input(&input, toggles.len());
        if !indexes.is_empty() {
            has_toggled = true;
        }
        apply_skill_toggles(&mut toggles, &indexes);
    }
}

fn print_skill_selection_prompt(toggles: &[SkillToggle], state: &SkillWizardState) {
    println!("  Toggle skills with numbers, press enter to confirm:");
    for line in skill_display_lines(toggles, state) {
        println!("  {line}");
    }
}

fn skill_display_lines(toggles: &[SkillToggle], state: &SkillWizardState) -> Vec<String> {
    toggles
        .iter()
        .enumerate()
        .map(|(index, toggle)| format_skill_line(index + 1, toggle, state))
        .collect()
}

fn format_skill_line(index: usize, toggle: &SkillToggle, state: &SkillWizardState) -> String {
    format!(
        "{}. [{}] {:<16} {} [{}]",
        index,
        if toggle.selected { "x" } else { " " },
        toggle.skill.label,
        skill_requirement_text(toggle.skill, state),
        skill_install_state(toggle.skill, state)
    )
}

fn skill_requirement_text(skill: &SetupSkill, state: &SkillWizardState) -> String {
    match skill.auth {
        SkillAuth::Free => "free, no key needed".to_string(),
        SkillAuth::OpenAiReuse if state.openai_configured => "uses your OpenAI key".to_string(),
        SkillAuth::OpenAiReuse => "needs OpenAI auth".to_string(),
        SkillAuth::Credential { label, .. } => format!("needs {label}"),
    }
}

fn skill_install_state(skill: &SetupSkill, state: &SkillWizardState) -> &'static str {
    if state.installed_skills.contains(skill.name) {
        "installed"
    } else {
        "not installed"
    }
}

fn parse_toggle_input(input: &str, len: usize) -> Vec<usize> {
    let mut indexes = BTreeSet::new();
    for token in input.split([',', ' ']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let Ok(index) = token.parse::<usize>() else {
            continue;
        };
        if (1..=len).contains(&index) {
            indexes.insert(index - 1);
        }
    }
    indexes.into_iter().collect()
}

fn apply_skill_toggles(toggles: &mut [SkillToggle], indexes: &[usize]) {
    for index in indexes {
        toggles[*index].selected = !toggles[*index].selected;
    }
}

fn print_selected_skill_status(state: &SkillWizardState, selected: &[&SetupSkill]) {
    let installed = installed_skill_labels(selected, state);
    let missing = missing_skill_labels(selected, state);
    if selected.is_empty() {
        println!("  ✓ No skills selected.");
        return;
    }
    if !installed.is_empty() {
        println!("  ✓ Installed: {}", installed.join(", "));
    }
    if !missing.is_empty() {
        println!("  ! Not yet installed: {}", missing.join(", "));
        println!("  Run `skills/build.sh --install` to build and install skills.");
    }
}

fn installed_skill_labels(selected: &[&SetupSkill], state: &SkillWizardState) -> Vec<&'static str> {
    selected
        .iter()
        .copied()
        .filter(|skill| state.installed_skills.contains(skill.name))
        .map(|skill| skill.label)
        .collect()
}

fn missing_skill_labels(selected: &[&SetupSkill], state: &SkillWizardState) -> Vec<&'static str> {
    selected
        .iter()
        .copied()
        .filter(|skill| !state.installed_skills.contains(skill.name))
        .map(|skill| skill.label)
        .collect()
}

fn provider_reuse_messages(selected: &[&SetupSkill], state: &SkillWizardState) -> Vec<String> {
    let labels = openai_skill_labels(selected);
    if !state.openai_configured || labels.is_empty() {
        return Vec::new();
    }
    vec![format!(
        "  {} will use your OpenAI key automatically.",
        labels.join("/")
    )]
}

fn provider_requirement_messages(
    selected: &[&SetupSkill],
    state: &SkillWizardState,
) -> Vec<String> {
    let labels = openai_skill_labels(selected);
    if state.openai_configured || labels.is_empty() {
        return Vec::new();
    }
    vec![format!(
        "  ! {} {} OpenAI auth. Configure OpenAI to use {}.",
        labels.join("/"),
        if labels.len() == 1 { "needs" } else { "need" },
        labels.join("/")
    )]
}

fn openai_skill_labels(selected: &[&SetupSkill]) -> Vec<&'static str> {
    selected
        .iter()
        .copied()
        .filter(|skill| matches!(skill.auth, SkillAuth::OpenAiReuse))
        .map(|skill| skill.label)
        .collect()
}

fn credential_prompts(selected: &[&SetupSkill], state: &SkillWizardState) -> Vec<CredentialPrompt> {
    let mut seen = BTreeSet::new();
    let mut prompts = Vec::new();
    for skill in selected {
        if let SkillAuth::Credential { key, label } = skill.auth {
            if seen.insert(key) && !state.stored_credentials.contains(key) {
                prompts.push(CredentialPrompt { key, label });
            }
        }
    }
    prompts
}

fn print_skill_messages(messages: &[String]) {
    for message in messages {
        println!("{message}");
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

fn completion_lines() -> [&'static str; 7] {
    [
        "Setup complete! Start chatting:",
        "",
        "  fawx chat              — all-in-one (recommended)",
        "",
        "Or run as a server:",
        "  fawx serve --http      — start the engine",
        "  fawx tui               — connect the TUI (separate terminal)",
    ]
}

pub(crate) fn load_config_document(config_path: &Path) -> anyhow::Result<DocumentMut> {
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

fn is_skip_input(value: &str) -> bool {
    value.is_empty() || value.eq_ignore_ascii_case("skip")
}

fn parse_list_selection(value: &str, len: usize) -> Option<usize> {
    let parsed = value.trim().parse::<usize>().ok()?;
    (1..=len).contains(&parsed).then_some(parsed)
}

fn parse_permissions_selection(value: &str) -> Option<PermissionPreset> {
    match value.trim() {
        "1" => Some(PermissionPreset::Power),
        "2" => Some(PermissionPreset::Cautious),
        "3" => Some(PermissionPreset::Experimental),
        _ => None,
    }
}

fn permission_preset_label(preset: PermissionPreset) -> &'static str {
    match preset {
        PermissionPreset::Power => "Power",
        PermissionPreset::Cautious => "Cautious",
        PermissionPreset::Experimental => "Experimental",
        PermissionPreset::Custom => "Custom",
    }
}

fn permission_preset_summary(preset: PermissionPreset) -> &'static str {
    match preset {
        PermissionPreset::Power => "full workspace autonomy",
        PermissionPreset::Cautious => "proposals required for all writes and code execution",
        PermissionPreset::Experimental => "maximum autonomy including kernel self-modification",
        PermissionPreset::Custom => "custom permission policy",
    }
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

fn default_model_for_provider(provider: Option<&str>, models: &[String]) -> String {
    if let Some(first) = models.first() {
        return first.clone();
    }
    match provider {
        Some("anthropic") => "claude-sonnet-4-6".to_string(),
        Some("openai") => "gpt-4o".to_string(),
        _ => "gpt-4o".to_string(),
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

#[derive(Deserialize, Default)]
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

fn write_permissions_preset(
    document: &mut DocumentMut,
    preset: PermissionPreset,
) -> anyhow::Result<()> {
    let permissions = match preset {
        PermissionPreset::Power => PermissionsConfig::power(),
        PermissionPreset::Cautious => PermissionsConfig::cautious(),
        PermissionPreset::Experimental => PermissionsConfig::experimental(),
        PermissionPreset::Custom => PermissionsConfig {
            preset,
            ..PermissionsConfig::default()
        },
    };
    write_permissions_config(document, &permissions)
}

fn write_permissions_config(
    document: &mut DocumentMut,
    permissions: &PermissionsConfig,
) -> anyhow::Result<()> {
    let unrestricted = permission_action_names(&permissions.unrestricted);
    let proposal_required = permission_action_names(&permissions.proposal_required);
    set_string(
        document,
        &["permissions"],
        "preset",
        permissions.preset.as_str(),
    )?;
    set_string(
        document,
        &["permissions"],
        "mode",
        match permissions.mode {
            fx_config::CapabilityMode::Capability => "capability",
            fx_config::CapabilityMode::Prompt => "prompt",
        },
    )?;
    set_string_array(document, &["permissions"], "unrestricted", &unrestricted)?;
    set_string_array(
        document,
        &["permissions"],
        "proposal_required",
        &proposal_required,
    )?;
    Ok(())
}

fn permission_action_names(actions: &[PermissionAction]) -> Vec<&'static str> {
    actions
        .iter()
        .copied()
        .map(PermissionAction::as_str)
        .collect()
}

pub(crate) fn set_string(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: &str,
) -> anyhow::Result<()> {
    let table = table_mut(document, sections)?;
    table[key] = value(field_value);
    Ok(())
}

pub(crate) fn set_bool(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: bool,
) -> anyhow::Result<()> {
    let table = table_mut(document, sections)?;
    table[key] = value(field_value);
    Ok(())
}

pub(crate) fn set_integer(
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

fn set_string_array(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_values: &[&str],
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

pub(crate) fn random_hex(bytes: usize) -> anyhow::Result<String> {
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
    use tempfile::TempDir;

    static TEST_HOME_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct HomeGuard {
        original_home: Option<String>,
    }

    impl HomeGuard {
        fn set(temp_home: &TempDir) -> Self {
            let original_home = std::env::var("HOME").ok();
            unsafe {
                std::env::set_var("HOME", temp_home.path());
            }
            Self { original_home }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            if let Some(home) = &self.original_home {
                unsafe {
                    std::env::set_var("HOME", home);
                }
            } else {
                unsafe {
                    std::env::remove_var("HOME");
                }
            }
        }
    }

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
    fn parse_permissions_selection() {
        assert_eq!(
            super::parse_permissions_selection("1"),
            Some(PermissionPreset::Power)
        );
        assert_eq!(
            super::parse_permissions_selection("2"),
            Some(PermissionPreset::Cautious)
        );
        assert_eq!(
            super::parse_permissions_selection("3"),
            Some(PermissionPreset::Experimental)
        );
        assert_eq!(super::parse_permissions_selection("0"), None);
        assert_eq!(super::parse_permissions_selection("4"), None);
        assert_eq!(super::parse_permissions_selection("abc"), None);
    }

    #[test]
    fn parse_toggle_input_accepts_single_number() {
        assert_eq!(parse_toggle_input("3", SETUP_SKILLS.len()), vec![2]);
    }

    #[test]
    fn parse_toggle_input_accepts_comma_and_space_separated_numbers() {
        assert_eq!(
            parse_toggle_input("1,3,5", SETUP_SKILLS.len()),
            vec![0, 2, 4]
        );
        assert_eq!(
            parse_toggle_input("1 3 5", SETUP_SKILLS.len()),
            vec![0, 2, 4]
        );
        assert_eq!(
            parse_toggle_input("1, 3 5", SETUP_SKILLS.len()),
            vec![0, 2, 4]
        );
    }

    #[test]
    fn parse_toggle_input_ignores_invalid_and_duplicate_entries() {
        assert_eq!(
            parse_toggle_input("0", SETUP_SKILLS.len()),
            Vec::<usize>::new()
        );
        assert_eq!(
            parse_toggle_input("99", SETUP_SKILLS.len()),
            Vec::<usize>::new()
        );
        assert_eq!(
            parse_toggle_input("abc", SETUP_SKILLS.len()),
            Vec::<usize>::new()
        );
        assert_eq!(parse_toggle_input("3,3", SETUP_SKILLS.len()), vec![2]);
        assert_eq!(
            parse_toggle_input("", SETUP_SKILLS.len()),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn parse_toggle_input_keeps_valid_entries_when_mixed_with_invalid_ones() {
        assert_eq!(parse_toggle_input("1,abc", SETUP_SKILLS.len()), vec![0]);
    }

    #[test]
    fn completion_lines_match_headless_engine_workflow() {
        assert_eq!(
            completion_lines(),
            [
                "Setup complete! Start chatting:",
                "",
                "  fawx chat              — all-in-one (recommended)",
                "",
                "Or run as a server:",
                "  fawx serve --http      — start the engine",
                "  fawx tui               — connect the TUI (separate terminal)",
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
    fn permissions_config_writes_preset_to_document() {
        let mut document = load_config_document(Path::new("config.toml")).expect("document");
        let permissions = PermissionsConfig::power();

        write_permissions_config(&mut document, &permissions).expect("permissions config");

        let rendered = document.to_string();
        assert!(rendered.contains("[permissions]"));
        assert!(rendered.contains("preset = \"power\""));
        assert!(rendered.contains(
            "unrestricted = [\"read_any\", \"web_search\", \"web_fetch\", \"code_execute\", \"file_write\", \"git\", \"shell\", \"tool_call\", \"self_modify\"]"
        ));
        assert!(rendered.contains(
            "proposal_required = [\"credential_change\", \"system_install\", \"network_listen\", \"outbound_message\", \"file_delete\", \"outside_workspace\", \"kernel_modify\"]"
        ));
    }

    #[test]
    fn write_permissions_preset_writes_each_selectable_preset() {
        let cases = [
            PermissionPreset::Power,
            PermissionPreset::Cautious,
            PermissionPreset::Experimental,
        ];

        for preset in cases {
            let mut document = load_config_document(Path::new("config.toml")).expect("document");
            write_permissions_preset(&mut document, preset).expect("write preset");
            assert!(document
                .to_string()
                .contains(&format!("preset = \"{}\"", preset.as_str())));
        }
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
    fn setup_wizard_stores_skill_credentials_without_reopening_database() {
        let _home_lock = TEST_HOME_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_home = TempDir::new().expect("temp home");
        let _home = HomeGuard::set(&temp_home);
        let mut wizard = SetupWizard::new(false).expect("setup wizard");
        let data_dir = temp_home.path().join(".fawx");

        {
            // Verify the wizard did not eagerly open credentials.db during construction.
            // The handle is scoped so the redb file lock is released before the wizard
            // lazily opens the same database below.
            EncryptedFileCredentialStore::open(&data_dir)
                .expect("wizard should not lock credential store during construction");
        }

        wizard
            .store_skill_credential("brave_api_key", "brv-test")
            .expect("store skill credential");

        let stored = wizard
            .skill_credential_store()
            .expect("skill credential store")
            .get_generic("brave_api_key")
            .expect("read skill credential")
            .expect("skill credential present");
        assert_eq!(*stored, "brv-test");
    }

    fn test_state() -> SkillWizardState {
        SkillWizardState {
            openai_configured: false,
            installed_skills: BTreeSet::new(),
            stored_credentials: BTreeSet::new(),
        }
    }

    fn test_skill(name: &str) -> &'static SetupSkill {
        SETUP_SKILLS
            .iter()
            .find(|skill| skill.name == name)
            .expect("setup skill")
    }

    #[test]
    fn setup_skills_exclude_vision() {
        assert_eq!(SETUP_SKILLS.len(), 7);
        assert!(SETUP_SKILLS.iter().all(|skill| skill.name != "vision"));
    }

    #[test]
    fn skill_display_lines_show_mixed_install_state() {
        let mut state = test_state();
        state.installed_skills.insert("calculator".to_string());
        let toggles = SETUP_SKILLS
            .iter()
            .map(|skill| SkillToggle {
                skill,
                selected: skill.default_selected,
            })
            .collect::<Vec<_>>();

        let lines = skill_display_lines(&toggles, &state);

        assert!(lines[0].contains("Calculator"));
        assert!(lines[0].contains("[installed]"));
        assert!(lines[1].contains("Weather"));
        assert!(lines[1].contains("[not installed]"));
    }

    #[test]
    fn credential_prompts_only_include_selected_missing_keys() {
        let mut state = test_state();
        state.stored_credentials.insert("github_token".to_string());

        let prompts = credential_prompts(&[test_skill("browser"), test_skill("github")], &state);

        assert_eq!(
            prompts,
            vec![CredentialPrompt {
                key: "brave_api_key",
                label: "Brave API key",
            }]
        );
    }

    #[test]
    fn credential_prompts_skip_existing_keys() {
        let mut state = test_state();
        state.stored_credentials.insert("brave_api_key".to_string());

        let prompts = credential_prompts(&[test_skill("browser")], &state);

        assert!(prompts.is_empty());
    }

    #[test]
    fn provider_reuse_message_groups_selected_openai_skills() {
        let mut state = test_state();
        state.openai_configured = true;

        let messages = provider_reuse_messages(&[test_skill("tts"), test_skill("stt")], &state);

        assert_eq!(
            messages,
            vec!["  TTS/STT will use your OpenAI key automatically.".to_string()]
        );
    }

    #[test]
    fn provider_requirement_messages_handle_singular_plural_and_empty_cases() {
        let state = test_state();

        assert_eq!(
            provider_requirement_messages(&[test_skill("tts")], &state),
            vec!["  ! TTS needs OpenAI auth. Configure OpenAI to use TTS.".to_string()]
        );
        assert_eq!(
            provider_requirement_messages(&[test_skill("tts"), test_skill("stt")], &state),
            vec!["  ! TTS/STT need OpenAI auth. Configure OpenAI to use TTS/STT.".to_string()]
        );
        assert!(provider_requirement_messages(&[test_skill("browser")], &state).is_empty());
    }

    #[test]
    fn default_model_for_provider_uses_first_available_model() {
        let models = vec!["claude-opus-4".to_string(), "claude-sonnet-4".to_string()];
        assert_eq!(
            default_model_for_provider(Some("anthropic"), &models),
            "claude-opus-4"
        );
    }

    #[test]
    fn default_model_for_provider_falls_back_to_hardcoded_when_list_empty() {
        let empty: Vec<String> = Vec::new();
        assert_eq!(
            default_model_for_provider(Some("anthropic"), &empty),
            "claude-sonnet-4-6"
        );
        assert_eq!(default_model_for_provider(Some("openai"), &empty), "gpt-4o");
        assert_eq!(
            default_model_for_provider(Some("openrouter"), &empty),
            "gpt-4o"
        );
        assert_eq!(default_model_for_provider(None, &empty), "gpt-4o");
    }

    #[test]
    fn is_skip_input_accepts_empty_and_skip_variants() {
        assert!(is_skip_input(""));
        assert!(is_skip_input("skip"));
        assert!(is_skip_input("Skip"));
        assert!(is_skip_input("SKIP"));
        assert!(!is_skip_input("1"));
        assert!(!is_skip_input("abc"));
    }

    #[test]
    fn launchagent_answer_defaults_to_no_when_prompt_fails() {
        let answer = launchagent_answer_from_prompt::<&str>(Err("stdin closed"));

        assert_eq!(answer, "n");
    }

    #[test]
    fn should_install_launchagent_accepts_empty_and_yes_answers() {
        assert!(should_install_launchagent(""));
        assert!(should_install_launchagent("y"));
        assert!(should_install_launchagent("Yes"));
        assert!(!should_install_launchagent("n"));
    }

    #[test]
    fn tailscale_cert_paths_use_tls_directory() {
        let data_dir = Path::new("/tmp/fawx");
        let (cert_path, key_path) = crate::commands::tailscale::cert_paths(data_dir);

        assert_eq!(cert_path, data_dir.join("tls").join("cert.pem"));
        assert_eq!(key_path, data_dir.join("tls").join("key.pem"));
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
