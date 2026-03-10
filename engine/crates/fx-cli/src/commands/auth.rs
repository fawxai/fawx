use crate::auth_store::open_auth_store_with_recovery;
use crate::startup::fawx_data_dir;
use anyhow::anyhow;
use clap::Subcommand;
use fx_auth::auth::{AuthManager, AuthMethod};
use fx_auth::credential_store::EncryptedFileCredentialStore;

#[derive(Debug, Clone, Subcommand)]
pub enum AuthCommands {
    /// Store an LLM provider token or Telegram bot token
    SetToken { provider: String, key: String },
    /// Store a credential for WASM skills (e.g., brave_api_key, openai_tts_key)
    SetCredential { name: String, value: String },
    /// List stored skill credentials (names only, not values)
    ListCredentials,
    /// Store the HTTP bearer token used by `fawx serve --http`
    SetBearer { token: String },
    /// Show configured auth providers
    Status,
}

pub async fn run(command: AuthCommands) -> anyhow::Result<i32> {
    match command {
        AuthCommands::SetToken { provider, key } => {
            eprintln!("{}", cli_argument_warning());
            set_token(&provider, &key)?;
            Ok(0)
        }
        AuthCommands::SetCredential { name, value } => {
            eprintln!("{}", cli_argument_warning());
            set_credential(&name, &value)?;
            Ok(0)
        }
        AuthCommands::ListCredentials => {
            print_credentials()?;
            Ok(0)
        }
        AuthCommands::SetBearer { token } => {
            set_bearer(&token)?;
            Ok(0)
        }
        AuthCommands::Status => {
            print_status()?;
            Ok(0)
        }
    }
}

fn cli_argument_warning() -> &'static str {
    "⚠ API keys passed as CLI arguments may be visible in shell history. Consider using `fawx setup` instead."
}

fn set_token(provider: &str, key: &str) -> anyhow::Result<()> {
    let provider = normalize_provider_name(provider);
    let data_dir = fawx_data_dir();
    let recovered = open_auth_store_with_recovery(&data_dir).map_err(|error| anyhow!(error))?;
    if is_telegram_provider(&provider) {
        recovered
            .store
            .store_provider_token("telegram_bot_token", key)
            .map_err(|error| anyhow!(error))
    } else {
        let mut manager = recovered
            .store
            .load_auth_manager()
            .map_err(|error| anyhow!(error))?;
        manager.store(
            &provider,
            AuthMethod::ApiKey {
                provider: provider.clone(),
                key: key.to_string(),
            },
        );
        recovered
            .store
            .save_auth_manager(&manager)
            .map_err(|error| anyhow!(error))
    }
}

pub(crate) fn set_credential(name: &str, value: &str) -> anyhow::Result<()> {
    let store = open_skill_credential_store()?;
    store
        .set_generic(name, value)
        .map_err(|error| anyhow!(error))
}

fn set_bearer(token: &str) -> anyhow::Result<()> {
    let data_dir = fawx_data_dir();
    let recovered = open_auth_store_with_recovery(&data_dir).map_err(|error| anyhow!(error))?;
    recovered
        .store
        .store_provider_token("http_bearer", token)
        .map_err(|error| anyhow!(error))
}

fn print_status() -> anyhow::Result<()> {
    let data_dir = fawx_data_dir();
    let recovered = open_auth_store_with_recovery(&data_dir).map_err(|error| anyhow!(error))?;
    let manager = recovered
        .store
        .load_auth_manager()
        .map_err(|error| anyhow!(error))?;
    let skill_credentials = open_skill_credential_store()?
        .list_generic_names()
        .map_err(|error| anyhow!(error))?;
    for line in status_lines(&manager, &recovered.store, &skill_credentials)? {
        println!("{line}");
    }
    Ok(())
}

fn print_credentials() -> anyhow::Result<()> {
    let names = open_skill_credential_store()?
        .list_generic_names()
        .map_err(|error| anyhow!(error))?;
    for line in credential_listing_lines(&names) {
        println!("{line}");
    }
    Ok(())
}

pub(crate) fn open_skill_credential_store() -> anyhow::Result<EncryptedFileCredentialStore> {
    EncryptedFileCredentialStore::open(&fawx_data_dir()).map_err(|error| anyhow!(error))
}

fn status_lines(
    manager: &AuthManager,
    store: &crate::auth_store::AuthStore,
    skill_credentials: &[String],
) -> anyhow::Result<Vec<String>> {
    let mut lines = manager_status_lines(manager);
    lines.extend(provider_token_status_lines(store)?);
    lines.extend(skill_credential_status_lines(skill_credentials));
    Ok(lines)
}

fn manager_status_lines(manager: &AuthManager) -> Vec<String> {
    let mut providers = manager.providers();
    providers.sort();
    providers
        .into_iter()
        .map(|provider| format!("{provider}: {}", status_label(manager.get(&provider))))
        .collect()
}

fn provider_token_status_lines(
    store: &crate::auth_store::AuthStore,
) -> anyhow::Result<Vec<String>> {
    let mut lines = Vec::new();
    if provider_token_present(store, "http_bearer")? {
        lines.push("http_bearer: configured".to_string());
    }
    if provider_token_present(store, "telegram_bot_token")? {
        lines.push("telegram: configured".to_string());
    }
    Ok(lines)
}

fn skill_credential_status_lines(names: &[String]) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }

    let mut lines = vec!["Skill credentials:".to_string()];
    lines.extend(names.iter().map(|name| format!("  ✓ {name}")));
    lines
}

fn credential_listing_lines(names: &[String]) -> Vec<String> {
    let mut lines = vec!["Skill credentials:".to_string()];
    if names.is_empty() {
        lines.push("  (none)".to_string());
        return lines;
    }

    lines.extend(names.iter().map(|name| format!("  {name}")));
    lines
}

fn provider_token_present(
    store: &crate::auth_store::AuthStore,
    provider: &str,
) -> anyhow::Result<bool> {
    store
        .get_provider_token(provider)
        .map(|value| value.is_some())
        .map_err(|error| anyhow!(error))
}

fn status_label(method: Option<&AuthMethod>) -> &'static str {
    match method {
        Some(AuthMethod::ApiKey { .. }) => "configured (API key)",
        Some(AuthMethod::SetupToken { .. }) => "configured (setup token)",
        Some(AuthMethod::OAuth { .. }) => "configured (OAuth)",
        None => "not configured",
    }
}

fn normalize_provider_name(value: &str) -> String {
    let lower = value.trim().to_ascii_lowercase();
    match lower.as_str() {
        "gh" => "github".to_string(),
        "telegram_bot" => "telegram".to_string(),
        other => other.to_string(),
    }
}

fn is_telegram_provider(provider: &str) -> bool {
    provider == "telegram"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cli, Commands};
    use clap::Parser;
    use std::sync::LazyLock;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

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
    fn normalize_provider_name_handles_aliases() {
        assert_eq!(normalize_provider_name("GH"), "github");
        assert_eq!(normalize_provider_name(" telegram_bot "), "telegram");
        assert_eq!(normalize_provider_name("OpenAI"), "openai");
    }

    #[test]
    fn status_label_matches_auth_method() {
        assert_eq!(status_label(None), "not configured");
        assert_eq!(
            status_label(Some(&AuthMethod::SetupToken {
                token: "secret".to_string(),
            })),
            "configured (setup token)"
        );
    }

    #[test]
    fn cli_argument_warning_matches_security_guidance() {
        assert_eq!(
            cli_argument_warning(),
            "⚠ API keys passed as CLI arguments may be visible in shell history. Consider using `fawx setup` instead."
        );
    }

    #[test]
    fn credential_listing_lines_show_names_only() {
        let lines = credential_listing_lines(&[
            "brave_api_key".to_string(),
            "custom_webhook_token".to_string(),
        ]);

        assert_eq!(lines[0], "Skill credentials:");
        assert!(lines.iter().any(|line| line == "  brave_api_key"));
        assert!(lines.iter().any(|line| line == "  custom_webhook_token"));
        assert!(!lines.iter().any(|line| line.contains("sk-test-secret")));
    }

    #[test]
    fn status_lines_include_skill_credential_names() {
        let store = crate::auth_store::AuthStore::open_for_testing().expect("test auth store");
        let manager = AuthManager::new();
        let lines = status_lines(
            &manager,
            &store,
            &["brave_api_key".to_string(), "openai_tts_key".to_string()],
        )
        .expect("status lines");

        assert!(lines.iter().any(|line| line == "Skill credentials:"));
        assert!(lines.iter().any(|line| line == "  ✓ brave_api_key"));
        assert!(lines.iter().any(|line| line == "  ✓ openai_tts_key"));
    }

    #[test]
    fn auth_cli_parses_set_token_command() {
        let cli = Cli::try_parse_from(["fawx", "auth", "set-token", "anthropic", "sk-test"])
            .expect("parse auth command");

        assert!(matches!(
            cli.command,
            Some(Commands::Auth {
                command: AuthCommands::SetToken { provider, key }
            }) if provider == "anthropic" && key == "sk-test"
        ));
    }

    #[test]
    fn auth_cli_parses_set_credential_command() {
        let cli =
            Cli::try_parse_from(["fawx", "auth", "set-credential", "brave_api_key", "sk-test"])
                .expect("parse auth set-credential command");

        assert!(matches!(
            cli.command,
            Some(Commands::Auth {
                command: AuthCommands::SetCredential { name, value }
            }) if name == "brave_api_key" && value == "sk-test"
        ));
    }

    #[test]
    fn auth_cli_rejects_missing_set_token_key() {
        let error = Cli::try_parse_from(["fawx", "auth", "set-token", "anthropic"])
            .err()
            .expect("missing key should fail to parse");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[tokio::test]
    async fn run_set_token_persists_provider_and_reports_configured_status() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_home = TempDir::new().expect("temp home");
        let _home = HomeGuard::set(&temp_home);

        run(AuthCommands::SetToken {
            provider: "anthropic".to_string(),
            key: "sk-ant-test".to_string(),
        })
        .await
        .expect("set token");

        let data_dir = temp_home.path().join(".fawx");
        let recovered =
            open_auth_store_with_recovery(&data_dir).expect("auth store should open after run");
        let manager = recovered
            .store
            .load_auth_manager()
            .expect("auth manager should load");
        let lines = status_lines(&manager, &recovered.store, &[]).expect("status lines");

        assert!(lines
            .iter()
            .any(|line| line == "anthropic: configured (API key)"));
    }

    #[tokio::test]
    async fn run_set_credential_persists_generic_skill_credential() {
        let _env_lock = ENV_LOCK.lock().await;
        let temp_home = TempDir::new().expect("temp home");
        let _home = HomeGuard::set(&temp_home);

        run(AuthCommands::SetCredential {
            name: "brave_api_key".to_string(),
            value: "sk-brave-test".to_string(),
        })
        .await
        .expect("set credential");

        let store = EncryptedFileCredentialStore::open(&temp_home.path().join(".fawx"))
            .expect("credential store should open");
        let stored = store
            .get_generic("brave_api_key")
            .expect("read generic credential")
            .expect("generic credential should exist");

        assert_eq!(*stored, "sk-brave-test");
    }
}
