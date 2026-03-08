//! Fawx CLI - Management interface for the Fawx agent.

mod ansi;
mod auth_store;
mod commands;
mod config_bridge;
mod confirmation;
mod headless;
#[cfg(feature = "http")]
mod http_serve;
#[allow(dead_code)] // TODO(#1148): Phase 3 will wire markdown rendering into ratatui
mod markdown;
// Phase 2: many rendering/history utilities are currently test-only while we
// wire ratatui. Phase 3 (polish) will re-connect markdown rendering, banner
// art, and history persistence. Suppress dead-code warnings until then.
#[allow(dead_code)] // TODO(#1148): Phase 3 reconnects history, banner art, and markdown
mod tui;
mod ui;

use clap::{Parser, Subcommand, ValueEnum};

pub use confirmation::ConfirmationUi;

#[derive(Parser)]
#[command(name = "fawx")]
#[command(about = "Fawx AI Agent CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the agent daemon
    Start,

    /// Stop the agent daemon
    Stop,

    /// Run the terminal shell interface (default)
    Tui,

    /// Interactive chat with the agent
    Chat,

    /// Run headless mode (stdin/stdout, no TUI)
    Serve {
        /// Process single input and exit
        #[arg(long)]
        single: bool,
        /// JSON input/output mode
        #[arg(long)]
        json: bool,
        /// Path to a custom system prompt file (default: ~/.fawx/system_prompt.md)
        #[arg(long)]
        system_prompt: Option<std::path::PathBuf>,
        /// Start HTTP API server (Tailscale-only)
        #[arg(long)]
        http: bool,
        /// HTTP server port (default: 8400)
        #[arg(long, default_value = "8400")]
        port: u16,
    },

    /// Run system diagnostics
    Doctor,

    /// Show current configuration
    Config,

    /// Manage audit logs
    Audit {
        #[command(subcommand)]
        command: AuditCommands,
    },

    /// Manage skills
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },

    /// Search the skill registry
    Search {
        /// Search query
        query: String,
    },

    /// Install a skill from the registry
    Install {
        /// Skill name to install
        name: String,
    },

    /// List installed skills (local)
    List,

    /// Run OAuth bridge server for Android Codex sign-in
    #[cfg(feature = "oauth-bridge")]
    OauthBridge {
        /// Listen address for the bridge HTTP server
        #[arg(long, default_value = "127.0.0.1:4318")]
        listen: String,

        /// OAuth authorize endpoint URL
        #[arg(long)]
        auth_url: Option<String>,

        /// OAuth token endpoint URL
        #[arg(long)]
        token_url: Option<String>,

        /// OAuth client ID
        #[arg(long)]
        client_id: Option<String>,

        /// OAuth client secret (optional for PKCE public clients)
        #[arg(long)]
        client_secret: Option<String>,

        /// OAuth scope string
        #[arg(long)]
        scope: Option<String>,
    },

    /// Run OAuth bridge server (requires --features oauth-bridge)
    #[cfg(not(feature = "oauth-bridge"))]
    OauthBridge,

    /// Run deterministic agent-loop eval harness and emit machine-readable metrics.
    EvalDeterminism {
        /// Eval mode: ci-lite (fast) or full (nightly/manual)
        #[arg(long, value_enum, default_value_t = EvalModeArg::CiLite)]
        mode: EvalModeArg,

        /// Output report path (JSON)
        #[arg(long, default_value = ".ci/determinism/latest-report.json")]
        output: String,

        /// Optional baseline report path for trend deltas
        #[arg(long)]
        baseline: Option<String>,

        /// Write current report as baseline snapshot
        #[arg(long, default_value_t = false)]
        update_baseline: bool,

        /// Exit non-zero when metrics regress against baseline
        #[arg(long, default_value_t = false)]
        fail_on_regression: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum EvalModeArg {
    CiLite,
    Full,
}

impl From<EvalModeArg> for commands::eval_harness::EvalMode {
    fn from(value: EvalModeArg) -> Self {
        match value {
            EvalModeArg::CiLite => Self::CiLite,
            EvalModeArg::Full => Self::Full,
        }
    }
}

#[derive(Subcommand)]
enum AuditCommands {
    /// Show recent audit entries
    Show {
        /// Maximum number of entries to display
        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// Verify audit log integrity
    Verify,
}

#[derive(Subcommand)]
enum SkillCommands {
    /// List installed skills
    List,

    /// Install a skill
    Install {
        /// Path to skill WASM file
        path: String,
    },

    /// Remove a skill
    Remove {
        /// Skill name
        name: String,
    },

    /// Build a skill from source (compile, sign, install)
    Build {
        /// Path to skill project directory
        path: String,
        /// Skip signing even if key exists
        #[arg(long)]
        no_sign: bool,
        /// Build only, don't install to ~/.fawx/skills/
        #[arg(long)]
        no_install: bool,
    },

    /// Scaffold a new skill project
    New {
        /// Name for the new skill
        name: String,
    },
}

async fn run_tui() -> anyhow::Result<i32> {
    let auth_manager = tui::load_auth_manager()?;
    let router = tui::build_router(&auth_manager)?;
    let config = tui::load_config()?;
    let improvement_provider = tui::build_improvement_provider(&auth_manager, &config);
    let bundle = tui::build_loop_engine_from_config(&config, improvement_provider)?;
    let deps = bundle.into_tui_deps(auth_manager, router, config);
    let mut app = tui::TuiApp::new_with_deps(deps)?;
    app.run().await?;
    Ok(0)
}

async fn run_headless(
    single: bool,
    json: bool,
    system_prompt: Option<std::path::PathBuf>,
) -> anyhow::Result<i32> {
    let auth_manager = tui::load_auth_manager()?;
    let router = tui::build_router(&auth_manager)?;
    let config = tui::load_config()?;
    let improvement_provider = tui::build_improvement_provider(&auth_manager, &config);
    let bundle = tui::build_loop_engine_from_config(&config, improvement_provider)?;

    let deps = headless::HeadlessAppDeps {
        loop_engine: bundle.engine,
        router,
        config,
        memory: bundle.memory,
        system_prompt_path: system_prompt,
    };

    let mut app = headless::HeadlessApp::new(deps)?;
    if single {
        app.run_single(json).await
    } else {
        app.run(json).await
    }
}

#[cfg(feature = "http")]
async fn run_http_server(
    system_prompt: Option<std::path::PathBuf>,
    port: u16,
) -> anyhow::Result<i32> {
    let auth_manager = tui::load_auth_manager()?;
    let router = tui::build_router(&auth_manager)?;
    let config = tui::load_config()?;
    let http_config = config.http.clone();
    let telegram_config = config.telegram.clone();
    let improvement_provider = tui::build_improvement_provider(&auth_manager, &config);
    let bundle = tui::build_loop_engine_from_config(&config, improvement_provider)?;

    let deps = headless::HeadlessAppDeps {
        loop_engine: bundle.engine,
        router,
        config,
        memory: bundle.memory,
        system_prompt_path: system_prompt,
    };

    let mut app = headless::HeadlessApp::new(deps)?;
    app.initialize();

    // Open credential store once; pass to channel builders for DI.
    let auth_store = {
        let data_dir = tui::fawx_data_dir();
        auth_store::AuthStore::open(&data_dir).ok()
    };

    // Build Telegram channel if configured.
    let telegram = build_telegram_channel(&telegram_config, auth_store.as_ref());

    http_serve::run(app, port, &http_config, telegram).await
}

#[cfg(feature = "http")]
fn build_telegram_channel(
    config: &fx_config::TelegramChannelConfig,
    auth_store: Option<&auth_store::AuthStore>,
) -> Option<std::sync::Arc<fx_channel_telegram::TelegramChannel>> {
    if !config.enabled {
        return None;
    }

    // Bot token priority: credential store → env var → config file.
    let bot_token = telegram_token_from_credential_store(auth_store)
        .or_else(|| std::env::var("FAWX_TELEGRAM_TOKEN").ok())
        .or_else(|| config.bot_token.clone())
        .filter(|t| !t.trim().is_empty());

    let bot_token = match bot_token {
        Some(t) => t,
        None => {
            eprintln!(
                "Warning: telegram.enabled = true but no bot token configured.\n\
                 Use /auth telegram set-token, set FAWX_TELEGRAM_TOKEN env var, \
                 or telegram.bot_token in config.toml."
            );
            return None;
        }
    };

    if config.allowed_chat_ids.is_empty() {
        eprintln!(
            "Warning: Telegram channel has no allowed_chat_ids configured.\n\
             All chats will be accepted. Set telegram.allowed_chat_ids for security."
        );
    }

    // Webhook secret: prefer env var, fall back to config file.
    let webhook_secret = std::env::var("FAWX_TELEGRAM_WEBHOOK_SECRET")
        .ok()
        .or_else(|| config.webhook_secret.clone())
        .filter(|s| !s.trim().is_empty());

    let tg_config = fx_channel_telegram::TelegramConfig {
        bot_token,
        allowed_chat_ids: config.allowed_chat_ids.clone(),
        webhook_secret,
    };

    Some(std::sync::Arc::new(
        fx_channel_telegram::TelegramChannel::new(tg_config),
    ))
}

/// Read the Telegram bot token from the encrypted credential store.
///
/// Returns `None` if no store is provided or it contains no token.
#[cfg(feature = "http")]
fn telegram_token_from_credential_store(store: Option<&auth_store::AuthStore>) -> Option<String> {
    store?
        .get_provider_token("telegram_bot_token")
        .ok()
        .flatten()
        .map(|token| token.to_string())
        .filter(|t| !t.trim().is_empty())
}

#[cfg(not(feature = "http"))]
async fn run_http_server(
    _system_prompt: Option<std::path::PathBuf>,
    _port: u16,
) -> anyhow::Result<i32> {
    eprintln!("Error: the http feature is not enabled in this build.");
    eprintln!("Rebuild with: cargo build -p fx-cli --features http");
    Ok(1)
}

fn run_stub(action: &str) -> i32 {
    println!("{action} Fawx agent daemon...");
    println!("(Implementation pending - Epic 9)");
    0
}

async fn dispatch_audit(command: AuditCommands) -> anyhow::Result<i32> {
    match command {
        AuditCommands::Show { limit } => {
            commands::audit::show(limit).await?;
            Ok(0)
        }
        AuditCommands::Verify => Ok(commands::audit::verify().await?),
    }
}

async fn dispatch_skill(command: SkillCommands) -> anyhow::Result<i32> {
    match command {
        SkillCommands::List => {
            commands::skills::list().await?;
            Ok(0)
        }
        SkillCommands::Install { path } => {
            commands::skills::install(&path).await?;
            Ok(0)
        }
        SkillCommands::Remove { name } => {
            commands::skills::remove(&name).await?;
            Ok(0)
        }
        SkillCommands::Build {
            path,
            no_sign,
            no_install,
        } => {
            commands::skills::build(&path, no_sign, no_install)?;
            Ok(0)
        }
        SkillCommands::New { name } => {
            commands::skills::scaffold(&name)?;
            Ok(0)
        }
    }
}

async fn dispatch_command(command: Commands) -> anyhow::Result<i32> {
    match command {
        Commands::Tui => run_tui().await,
        Commands::Start => Ok(run_stub("Starting")),
        Commands::Stop => Ok(run_stub("Stopping")),
        Commands::Chat => Ok(commands::chat::run().await?),
        Commands::Serve {
            single,
            json,
            system_prompt,
            http,
            port,
        } => {
            if http {
                run_http_server(system_prompt, port).await
            } else {
                run_headless(single, json, system_prompt).await
            }
        }
        Commands::Doctor => Ok(commands::doctor::run().await?),
        Commands::Config => {
            commands::config::run().await?;
            Ok(0)
        }
        Commands::Audit { command } => dispatch_audit(command).await,
        Commands::Skill { command } => dispatch_skill(command).await,
        Commands::Search { query } => {
            commands::marketplace::search_cmd(&query)?;
            Ok(0)
        }
        Commands::Install { name } => {
            commands::marketplace::install_cmd(&name)?;
            Ok(0)
        }
        Commands::List => {
            commands::marketplace::list_cmd()?;
            Ok(0)
        }
        #[cfg(not(feature = "oauth-bridge"))]
        Commands::OauthBridge => {
            eprintln!("Error: the oauth-bridge feature is not enabled in this build.");
            eprintln!("Rebuild with: cargo build -p fx-cli --features oauth-bridge");
            Ok(1)
        }
        #[cfg(feature = "oauth-bridge")]
        Commands::OauthBridge {
            listen,
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
        } => {
            dispatch_oauth_bridge(listen, auth_url, token_url, client_id, client_secret, scope)
                .await
        }
        Commands::EvalDeterminism {
            mode,
            output,
            baseline,
            update_baseline,
            fail_on_regression,
        } => dispatch_eval(mode, output, baseline, update_baseline, fail_on_regression),
    }
}

#[cfg(feature = "oauth-bridge")]
async fn dispatch_oauth_bridge(
    listen: String,
    auth_url: Option<String>,
    token_url: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    scope: Option<String>,
) -> anyhow::Result<i32> {
    commands::oauth_bridge::run(commands::oauth_bridge::Options {
        listen,
        auth_url,
        token_url,
        client_id,
        client_secret,
        scope,
    })
    .await
}

fn dispatch_eval(
    mode: EvalModeArg,
    output: String,
    baseline: Option<String>,
    update_baseline: bool,
    fail_on_regression: bool,
) -> anyhow::Result<i32> {
    commands::eval_harness::run(commands::eval_harness::Options {
        mode: mode.into(),
        output: output.into(),
        baseline: baseline.map(Into::into),
        update_baseline,
        fail_on_regression,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let exit_code = dispatch_command(cli.command.unwrap_or(Commands::Tui)).await?;
    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use crate::auth_store::AuthStore;

    fn test_auth_store() -> AuthStore {
        AuthStore::open_in_memory().expect("in-memory auth store")
    }

    #[test]
    fn telegram_token_credential_store_roundtrip() {
        let store = test_auth_store();
        let token = "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11";
        store
            .store_provider_token("telegram_bot_token", token)
            .expect("store");
        let retrieved = store
            .get_provider_token("telegram_bot_token")
            .expect("get")
            .expect("should have value");
        assert_eq!(*retrieved, token);
    }

    #[test]
    fn telegram_token_credential_store_empty_returns_none() {
        let store = test_auth_store();
        let result = store.get_provider_token("telegram_bot_token").expect("get");
        assert!(result.is_none());
    }

    #[test]
    fn telegram_token_credential_store_whitespace_only() {
        let store = test_auth_store();
        store
            .store_provider_token("telegram_bot_token", "   ")
            .expect("store");
        let retrieved = store
            .get_provider_token("telegram_bot_token")
            .expect("get")
            .expect("stored but whitespace");
        // The store returns the value; filtering is the caller's job.
        assert_eq!(retrieved.trim(), "");
    }

    #[test]
    fn telegram_token_does_not_collide_with_http_bearer() {
        let store = test_auth_store();
        store
            .store_provider_token("telegram_bot_token", "tg-token")
            .expect("store telegram");
        store
            .store_provider_token("http_bearer", "http-token")
            .expect("store http");

        let tg = store
            .get_provider_token("telegram_bot_token")
            .expect("get tg")
            .expect("should exist");
        let http = store
            .get_provider_token("http_bearer")
            .expect("get http")
            .expect("should exist");

        assert_eq!(*tg, "tg-token");
        assert_eq!(*http, "http-token");
    }
}
