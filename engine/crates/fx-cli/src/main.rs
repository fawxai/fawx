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
mod prompts;
mod proposal_review;
// Phase 2: many rendering/history utilities are currently test-only while we
// wire ratatui. Phase 3 (polish) will re-connect markdown rendering, banner
// art, and history persistence. Suppress dead-code warnings until then.
#[allow(dead_code)] // TODO(#1148): Phase 3 reconnects history, banner art, and markdown
mod tui;
mod ui;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use fx_canary::{CanaryConfig, CanaryMonitor, RipcordTrigger, RollbackTrigger};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

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

    /// Launch the Fawx TUI (connects to a running server)
    #[command(trailing_var_arg = true)]
    Tui {
        /// Extra arguments passed through to fawx-tui
        #[arg(hide = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Launch the legacy inline TUI
    #[command(hide = true)]
    TuiLegacy,

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
        /// Start local HTTP API server with SSE streaming
        #[arg(long)]
        http: bool,
        /// HTTP server port (default: 8400)
        #[arg(long, default_value = "8400")]
        port: u16,
    },

    /// Run system diagnostics
    Doctor,

    /// Interactive first-run setup wizard
    Setup {
        /// Re-run setup even if already configured
        #[arg(long)]
        force: bool,
    },

    /// Manage authentication credentials
    Auth {
        #[command(subcommand)]
        command: commands::auth::AuthCommands,
    },

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

const FAWX_TUI_NOT_FOUND_MESSAGE: &str =
    "fawx-tui binary not found. Build it with: cargo build --release -p fawx-tui";

async fn run_tui() -> anyhow::Result<i32> {
    let auth_manager = tui::load_auth_manager()?;
    let router = tui::build_router(&auth_manager)?;
    let config = tui::load_config()?;
    let data_dir = configured_data_dir(&config);
    let improvement_provider = tui::build_improvement_provider(&auth_manager, &config);
    let config_manager = build_config_manager(&config);
    let subagent_router = Arc::new(tui::build_router(&auth_manager)?);
    let subagent_manager =
        build_subagent_manager(subagent_router, &config, improvement_provider.clone());
    let bundle = tui::build_loop_engine_from_config_with_options(
        &config,
        improvement_provider,
        parent_loop_build_options(&subagent_manager, Some(Arc::clone(&config_manager))),
    )?;
    let mut deps = bundle.into_tui_deps(auth_manager, router, config);
    deps.canary_monitor = Some(build_canary_monitor(&data_dir));
    let mut app = tui::TuiApp::new_with_deps(deps)?;
    app.run().await?;
    Ok(0)
}

fn build_config_manager(
    config: &fx_config::FawxConfig,
) -> Arc<std::sync::Mutex<fx_config::manager::ConfigManager>> {
    let data_dir = config
        .general
        .data_dir
        .clone()
        .unwrap_or_else(tui::fawx_data_dir);
    let config_path = data_dir.join("config.toml");
    let manager = fx_config::manager::ConfigManager::from_config(config.clone(), config_path);
    Arc::new(std::sync::Mutex::new(manager))
}

fn build_subagent_manager(
    router: Arc<fx_llm::ModelRouter>,
    config: &fx_config::FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
) -> Arc<fx_subagent::SubagentManager> {
    let factory = headless::HeadlessSubagentFactory::new(headless::HeadlessSubagentFactoryDeps {
        router,
        config: config.clone(),
        improvement_provider,
    });
    Arc::new(fx_subagent::SubagentManager::new(
        fx_subagent::SubagentManagerDeps {
            factory: Arc::new(factory),
            limits: fx_subagent::SubagentLimits::default(),
        },
    ))
}

fn parent_loop_build_options(
    subagent_manager: &Arc<fx_subagent::SubagentManager>,
    config_manager: Option<Arc<std::sync::Mutex<fx_config::manager::ConfigManager>>>,
) -> tui::HeadlessLoopBuildOptions {
    tui::HeadlessLoopBuildOptions {
        memory_enabled: true,
        subagent_control: Some(
            Arc::clone(subagent_manager) as Arc<dyn fx_subagent::SubagentControl>
        ),
        config_manager,
        ..tui::HeadlessLoopBuildOptions::default()
    }
}

fn launch_fawx_tui(args: &[String]) -> anyhow::Result<i32> {
    let tui_binary = find_fawx_tui_binary()?;
    let status = std::process::Command::new(&tui_binary)
        .args(args)
        .status()?;
    Ok(status.code().unwrap_or(1))
}

fn find_fawx_tui_binary() -> anyhow::Result<PathBuf> {
    let current_exe =
        std::env::current_exe().context("failed to locate current fawx executable")?;
    find_fawx_tui_binary_from(&current_exe, std::env::var_os("PATH").as_deref())
}

fn find_fawx_tui_binary_from(
    current_exe: &Path,
    path_env: Option<&OsStr>,
) -> anyhow::Result<PathBuf> {
    current_exe
        .parent()
        .and_then(find_fawx_tui_in_directory)
        .or_else(|| find_fawx_tui_on_path(path_env))
        .or_else(|| find_fawx_tui_in_cargo_release_dir(current_exe))
        .ok_or_else(|| anyhow::anyhow!(FAWX_TUI_NOT_FOUND_MESSAGE))
}

fn find_fawx_tui_in_directory(directory: &Path) -> Option<PathBuf> {
    let candidate = directory.join(fawx_tui_binary_name());
    candidate.is_file().then_some(candidate)
}

fn find_fawx_tui_on_path(path_env: Option<&OsStr>) -> Option<PathBuf> {
    let current_dir = std::env::current_dir().ok()?;
    which::which_in(fawx_tui_binary_name(), path_env, current_dir).ok()
}

fn find_fawx_tui_in_cargo_release_dir(current_exe: &Path) -> Option<PathBuf> {
    let target_dir = current_exe
        .ancestors()
        .find(|path| path.file_name() == Some(OsStr::new("target")))?;
    find_fawx_tui_in_directory(&target_dir.join("release"))
}

fn fawx_tui_binary_name() -> &'static str {
    if cfg!(windows) {
        "fawx-tui.exe"
    } else {
        "fawx-tui"
    }
}

struct HeadlessStartup {
    app: headless::HeadlessApp,
    #[cfg(feature = "http")]
    http_config: fx_config::HttpConfig,
    #[cfg(feature = "http")]
    telegram_config: fx_config::TelegramChannelConfig,
    #[cfg(feature = "http")]
    webhook_config: fx_config::WebhookConfig,
    #[cfg(feature = "http")]
    data_dir: std::path::PathBuf,
}

fn build_headless_startup(
    system_prompt: Option<std::path::PathBuf>,
) -> anyhow::Result<HeadlessStartup> {
    let auth_manager = tui::load_auth_manager()?;
    let router = Arc::new(tui::build_router(&auth_manager)?);
    let config = tui::load_config()?;
    #[cfg(feature = "http")]
    let http_config = config.http.clone();
    #[cfg(feature = "http")]
    let telegram_config = config.telegram.clone();
    #[cfg(feature = "http")]
    let webhook_config = config.webhook.clone();
    let data_dir = tui::fawx_data_dir();
    let config_manager = Some(build_config_manager(&config));
    let improvement_provider = tui::build_improvement_provider(&auth_manager, &config);
    let app = build_headless_app(
        router,
        config,
        improvement_provider,
        system_prompt,
        config_manager,
        data_dir.clone(),
    )?;
    Ok(HeadlessStartup {
        app,
        #[cfg(feature = "http")]
        http_config,
        #[cfg(feature = "http")]
        telegram_config,
        #[cfg(feature = "http")]
        webhook_config,
        #[cfg(feature = "http")]
        data_dir,
    })
}

fn build_headless_app(
    router: Arc<fx_llm::ModelRouter>,
    config: fx_config::FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    system_prompt: Option<std::path::PathBuf>,
    config_manager: Option<Arc<std::sync::Mutex<fx_config::manager::ConfigManager>>>,
    data_dir: PathBuf,
) -> anyhow::Result<headless::HeadlessApp> {
    let subagent_manager =
        build_subagent_manager(Arc::clone(&router), &config, improvement_provider.clone());
    let bundle = tui::build_headless_loop_engine_bundle(
        &config,
        improvement_provider,
        parent_loop_build_options(&subagent_manager, config_manager.clone()),
    )?;
    headless::HeadlessApp::new(headless::HeadlessAppDeps {
        loop_engine: bundle.engine,
        router,
        config,
        memory: bundle.memory,
        system_prompt_path: system_prompt,
        config_manager,
        system_prompt_text: None,
        subagent_manager,
        canary_monitor: Some(build_canary_monitor(&data_dir)),
    })
}

fn build_canary_monitor(data_dir: &Path) -> CanaryMonitor {
    let trigger = resolve_ripcord_path(data_dir).map(|path| {
        Arc::new(RipcordTrigger::new(path, data_dir.to_path_buf())) as Arc<dyn RollbackTrigger>
    });
    if trigger.is_none() {
        tracing::warn!(
            data_dir = %data_dir.display(),
            "fawx-ripcord not found; automatic rollback is disabled"
        );
    }
    CanaryMonitor::new(CanaryConfig::default(), trigger)
}

fn resolve_ripcord_path(data_dir: &Path) -> Option<PathBuf> {
    resolve_ripcord_path_with(
        ripcord_current_exe_candidate(),
        data_dir,
        std::env::var_os("PATH"),
    )
}

fn resolve_ripcord_path_with(
    current_exe_candidate: Option<PathBuf>,
    data_dir: &Path,
    path_env: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    current_exe_candidate
        .into_iter()
        .chain(std::iter::once(
            data_dir.join("bin").join(ripcord_binary_name()),
        ))
        .chain(path_candidates_from(path_env))
        .find(|path| path.is_file())
}

fn ripcord_current_exe_candidate() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(ripcord_binary_name()))
}

fn path_candidates_from(path_env: Option<std::ffi::OsString>) -> Vec<PathBuf> {
    let Some(paths) = path_env else {
        return Vec::new();
    };
    std::env::split_paths(&paths)
        .map(|dir| dir.join(ripcord_binary_name()))
        .collect()
}

fn ripcord_binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "fawx-ripcord.exe"
    }
    #[cfg(not(windows))]
    {
        "fawx-ripcord"
    }
}

fn configured_data_dir(config: &fx_config::FawxConfig) -> PathBuf {
    config
        .general
        .data_dir
        .clone()
        .unwrap_or_else(tui::fawx_data_dir)
}

async fn run_headless(
    single: bool,
    json: bool,
    system_prompt: Option<std::path::PathBuf>,
) -> anyhow::Result<i32> {
    let HeadlessStartup { mut app, .. } = build_headless_startup(system_prompt)?;
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
    let HeadlessStartup {
        mut app,
        http_config,
        telegram_config,
        webhook_config,
        data_dir,
    } = build_headless_startup(system_prompt)?;
    app.initialize();
    app.apply_http_defaults();

    // Install SIGHUP handler for graceful restart.
    install_sighup_handler();

    // Open credential store once; pass to channel builders for DI.
    let auth_store = { auth_store::AuthStore::open(&data_dir).ok() };

    // Build external channels if configured.
    let telegram = build_telegram_channel(&telegram_config, auth_store.as_ref());
    let webhooks = build_webhook_channels(&webhook_config);

    http_serve::run(app, port, &http_config, telegram, webhooks).await
}

/// Install a SIGHUP handler that logs and triggers a graceful process restart.
///
/// On non-Unix platforms this is a no-op.
#[cfg(feature = "http")]
fn install_sighup_handler() {
    #[cfg(unix)]
    {
        tokio::spawn(async {
            use tokio::signal::unix::{signal, SignalKind};
            let mut stream = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to install SIGHUP handler");
                    return;
                }
            };
            loop {
                stream.recv().await;
                tracing::info!("received SIGHUP — initiating graceful restart");
                eprintln!("SIGHUP received — restarting...");
                // Re-exec ourselves with the same arguments.
                let args: Vec<String> = std::env::args().collect();
                if let Some(exe) = args.first() {
                    let err = exec_replace(exe, &args);
                    tracing::error!(error = %err, "exec failed");
                }
            }
        });
    }
}

/// Replace the current process with a new instance (Unix only).
///
/// Uses `nix::unistd::execvp` (safe wrapper) to replace the process image.
/// Only returns on error.
#[cfg(all(feature = "http", unix))]
fn exec_replace(exe: &str, args: &[String]) -> std::io::Error {
    use std::ffi::CString;
    let c_exe = match CString::new(exe.as_bytes()) {
        Ok(s) => s,
        Err(e) => return std::io::Error::new(std::io::ErrorKind::InvalidInput, e),
    };
    let c_args: Result<Vec<CString>, _> = args.iter().map(|a| CString::new(a.as_bytes())).collect();
    let c_args = match c_args {
        Ok(a) => a,
        Err(e) => return std::io::Error::new(std::io::ErrorKind::InvalidInput, e),
    };
    // nix::unistd::execvp is a safe wrapper around libc::execvp.
    // On success it never returns; on failure we convert the nix::Errno.
    match nix::unistd::execvp(&c_exe, &c_args) {
        Ok(_infallible) => unreachable!("execvp does not return on success"),
        Err(errno) => std::io::Error::from_raw_os_error(errno as i32),
    }
}

#[cfg(feature = "http")]
fn build_webhook_channels(
    config: &fx_config::WebhookConfig,
) -> Vec<std::sync::Arc<fx_channel_webhook::WebhookChannel>> {
    if !config.enabled {
        return Vec::new();
    }

    config
        .channels
        .iter()
        .map(|channel| {
            std::sync::Arc::new(fx_channel_webhook::WebhookChannel::new(
                channel.id.clone(),
                channel.name.clone(),
                channel.callback_url.clone(),
            ))
        })
        .collect()
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

    // Webhook secret priority: credential store → env var → config file.
    let webhook_secret = telegram_webhook_secret_from_credential_store(auth_store)
        .or_else(|| std::env::var("FAWX_TELEGRAM_WEBHOOK_SECRET").ok())
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
    provider_token_from_credential_store(store, "telegram_bot_token")
}

#[cfg(feature = "http")]
fn telegram_webhook_secret_from_credential_store(
    store: Option<&auth_store::AuthStore>,
) -> Option<String> {
    provider_token_from_credential_store(store, "telegram_webhook_secret")
}

#[cfg(feature = "http")]
fn provider_token_from_credential_store(
    store: Option<&auth_store::AuthStore>,
    provider: &str,
) -> Option<String> {
    store?
        .get_provider_token(provider)
        .ok()
        .flatten()
        .map(|token| token.to_string())
        .filter(|token| !token.trim().is_empty())
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
        Commands::Tui { args } => launch_fawx_tui(&args),
        Commands::TuiLegacy => run_tui().await,
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
        Commands::Setup { force } => Ok(commands::setup::run(force).await?),
        Commands::Auth { command } => Ok(commands::auth::run(command).await?),
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
    let exit_code =
        dispatch_command(cli.command.unwrap_or(Commands::Tui { args: Vec::new() })).await?;
    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "http")]
    use super::{build_telegram_channel, telegram_webhook_secret_from_credential_store};
    use super::{
        dispatch_command, fawx_tui_binary_name, find_fawx_tui_binary_from,
        resolve_ripcord_path_with, ripcord_binary_name, Cli, Commands, FAWX_TUI_NOT_FOUND_MESSAGE,
    };
    use crate::auth_store::AuthStore;
    use clap::Parser;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{fs, path::Path};

    fn test_auth_store() -> AuthStore {
        AuthStore::open_for_testing().expect("test auth store")
    }

    fn touch(path: &std::path::Path) {
        std::fs::create_dir_all(path.parent().expect("parent path")).expect("create parent");
        std::fs::write(path, "").expect("write file");
    }

    #[test]
    fn resolve_ripcord_path_prefers_current_exe_sibling() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let current = temp_dir.path().join("current").join(ripcord_binary_name());
        let data = temp_dir
            .path()
            .join("data")
            .join("bin")
            .join(ripcord_binary_name());
        let path = temp_dir.path().join("path").join(ripcord_binary_name());
        touch(&current);
        touch(&data);
        touch(&path);

        let resolved = resolve_ripcord_path_with(
            Some(current.clone()),
            &temp_dir.path().join("data"),
            Some(std::env::join_paths([temp_dir.path().join("path")]).expect("join PATH")),
        );

        assert_eq!(resolved, Some(current));
    }

    #[test]
    fn resolve_ripcord_path_falls_back_to_data_dir_bin_before_path() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let data = temp_dir
            .path()
            .join("data")
            .join("bin")
            .join(ripcord_binary_name());
        let path = temp_dir.path().join("path").join(ripcord_binary_name());
        touch(&data);
        touch(&path);

        let resolved = resolve_ripcord_path_with(
            None,
            &temp_dir.path().join("data"),
            Some(std::env::join_paths([temp_dir.path().join("path")]).expect("join PATH")),
        );

        assert_eq!(resolved, Some(data));
    }

    #[test]
    fn resolve_ripcord_path_uses_path_when_other_locations_are_missing() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("path").join(ripcord_binary_name());
        touch(&path);

        let resolved = resolve_ripcord_path_with(
            None,
            &temp_dir.path().join("data"),
            Some(std::env::join_paths([temp_dir.path().join("path")]).expect("join PATH")),
        );

        assert_eq!(resolved, Some(path));
    }

    #[test]
    fn resolve_ripcord_path_returns_none_when_binary_is_missing() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");

        let resolved = resolve_ripcord_path_with(
            None,
            &temp_dir.path().join("data"),
            Some(std::env::join_paths([temp_dir.path().join("path")]).expect("join PATH")),
        );

        assert!(resolved.is_none());
    }

    #[test]
    fn cli_parses_setup_command() {
        let cli = Cli::parse_from(["fawx", "setup", "--force"]);
        assert!(matches!(cli.command, Some(Commands::Setup { force: true })));
    }

    #[test]
    fn cli_parses_tui_passthrough_args() {
        let cli = Cli::parse_from(["fawx", "tui", "--host", "http://127.0.0.1:8400"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Tui { args }) if args == vec!["--host", "http://127.0.0.1:8400"]
        ));
    }

    #[test]
    fn cli_parses_hidden_tui_legacy_command() {
        let cli = Cli::parse_from(["fawx", "tui-legacy"]);
        assert!(matches!(cli.command, Some(Commands::TuiLegacy)));
    }

    #[test]
    fn find_fawx_tui_binary_prefers_current_exe_directory() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let current_exe = tempdir.path().join("bin").join("fawx");
        let same_dir_tui = tempdir.path().join("bin").join(fawx_tui_binary_name());
        let path_dir = tempdir.path().join("path");

        write_fake_executable(&same_dir_tui);
        write_fake_executable(&path_dir.join(fawx_tui_binary_name()));

        let found =
            find_fawx_tui_binary_from(&current_exe, Some(path_dir.as_os_str())).expect("found");

        assert_eq!(found, same_dir_tui);
    }

    #[test]
    fn find_fawx_tui_binary_falls_back_to_path() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let current_exe = tempdir.path().join("bin").join("fawx");
        let path_dir = tempdir.path().join("path");
        let path_tui = path_dir.join(fawx_tui_binary_name());

        write_fake_executable(&path_tui);

        let found =
            find_fawx_tui_binary_from(&current_exe, Some(path_dir.as_os_str())).expect("found");

        assert_eq!(found, path_tui);
    }

    #[test]
    fn find_fawx_tui_binary_falls_back_to_cargo_release_dir() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let current_exe = tempdir.path().join("target").join("debug").join("fawx");
        let release_tui = tempdir
            .path()
            .join("target")
            .join("release")
            .join(fawx_tui_binary_name());

        write_fake_executable(&release_tui);

        let found = find_fawx_tui_binary_from(&current_exe, None).expect("found");

        assert_eq!(found, release_tui);
    }

    #[test]
    fn find_fawx_tui_binary_reports_missing_binary() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let current_exe = tempdir.path().join("bin").join("fawx");

        let error = find_fawx_tui_binary_from(&current_exe, None).expect_err("missing binary");

        assert!(error.to_string().contains(FAWX_TUI_NOT_FOUND_MESSAGE));
    }

    fn write_fake_executable(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
        fs::write(
            path,
            b"#!/bin/sh
exit 0
",
        )
        .expect("write executable");
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("set permissions");
        }
    }

    #[tokio::test]
    async fn setup_command_dispatches_to_setup_runner() {
        crate::commands::setup::set_test_exit_code(240);
        let exit_code = dispatch_command(Commands::Setup { force: true })
            .await
            .expect("dispatch");
        assert_eq!(exit_code, 241);
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

    #[cfg(feature = "http")]
    #[test]
    fn telegram_webhook_secret_credential_store_roundtrip() {
        let store = test_auth_store();
        store
            .store_provider_token("telegram_webhook_secret", "webhook-secret")
            .expect("store webhook secret");

        let retrieved = telegram_webhook_secret_from_credential_store(Some(&store));
        assert_eq!(retrieved.as_deref(), Some("webhook-secret"));
    }

    #[cfg(feature = "http")]
    #[test]
    fn build_telegram_channel_prefers_stored_webhook_secret() {
        let store = test_auth_store();
        store
            .store_provider_token("telegram_bot_token", "tg-token")
            .expect("store telegram token");
        store
            .store_provider_token("telegram_webhook_secret", "stored-secret")
            .expect("store webhook secret");
        let config = fx_config::TelegramChannelConfig {
            enabled: true,
            bot_token: Some("config-token".to_string()),
            allowed_chat_ids: vec![123],
            webhook_secret: Some("config-secret".to_string()),
        };

        let channel = build_telegram_channel(&config, Some(&store)).expect("channel should build");

        assert_eq!(channel.webhook_secret(), Some("stored-secret"));
    }
}
