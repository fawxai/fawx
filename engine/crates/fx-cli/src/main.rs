//! Fawx CLI - Management interface for the Fawx agent.

mod auth_store;
mod commands;
mod config_bridge;
mod config_redaction;
mod confirmation;
mod context;
mod headless;
pub(crate) mod helpers;
#[cfg(feature = "http")]
mod http_serve;
#[cfg(test)]
mod markdown;
mod persisted_memory;
mod prompts;
mod proposal_review;
#[allow(dead_code)]
mod repo_root;
#[allow(dead_code)]
mod restart;
#[allow(dead_code)]
// TODO(#1282): narrow this once embedded/lib and CLI startup paths stop leaving target-specific helpers unused.
mod startup;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use fx_canary::{CanaryConfig, CanaryMonitor, RipcordTrigger, RollbackTrigger};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Once},
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

    /// Interactive chat with the agent
    Chat,

    /// Run headless mode (stdin/stdout, no TUI)
    Serve {
        /// Process single input and exit
        #[arg(long, conflicts_with = "fleet")]
        single: bool,
        /// JSON input/output mode
        #[arg(long, conflicts_with = "fleet")]
        json: bool,
        /// Path to a custom system prompt file (default: ~/.fawx/system_prompt.md)
        #[arg(long)]
        system_prompt: Option<std::path::PathBuf>,
        /// Start local HTTP API server with SSE streaming
        #[arg(long, conflicts_with = "fleet")]
        http: bool,
        /// HTTP server port (default: 8400)
        #[arg(long, default_value = "8400")]
        port: u16,
        /// Override data directory (default: ~/.fawx)
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,
        /// Run as a fleet worker using the saved fleet identity
        #[arg(long)]
        fleet: bool,
    },

    /// Restart the running agent daemon
    Restart(restart::RestartArgs),

    /// Pull latest code, rebuild, and restart
    Update(commands::update::UpdateArgs),

    /// Run system diagnostics
    Doctor,

    /// Show runtime status for a running Fawx instance
    Status,

    /// Generate a device pairing code for the local HTTP server
    Pair(commands::pair::PairArgs),

    /// List or revoke paired devices
    Devices(commands::devices::DevicesArgs),

    /// Show CLI build information
    Version,

    /// Inspect persistent log files
    Logs(commands::logs::LogsArgs),

    /// Inspect conversation sessions
    Sessions {
        #[command(subcommand)]
        command: SessionsCommands,
    },

    /// Run Fawx-specific security checks
    SecurityAudit(commands::security_audit::SecurityAuditArgs),

    /// Create a compressed backup of ~/.fawx
    Backup(commands::backup::BackupArgs),

    /// Non-interactive zero-to-one local setup for GUI/embedded use
    Bootstrap {
        /// Output JSON instead of human-readable text
        #[arg(long)]
        json: bool,
        /// Override default port (scans 8400-8410 if unset)
        #[arg(long)]
        port: Option<u16>,
        /// Override data directory (default: ~/.fawx)
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,
    },

    /// Import memory and context from another workspace
    Import(commands::import::ImportArgs),

    /// Interactive first-run setup wizard
    Setup {
        /// Re-run setup even if already configured
        #[arg(long)]
        force: bool,
    },

    /// Manage Tailscale integration
    Tailscale {
        #[command(subcommand)]
        command: TailscaleCommands,
    },

    /// Manage authentication credentials
    Auth {
        #[command(subcommand)]
        command: commands::auth::AuthCommands,
    },

    /// Show or update configuration
    Config {
        #[command(subcommand)]
        command: Option<commands::config::ConfigCommands>,
    },

    /// Reset managed Fawx runtime state while preserving credentials
    Reset(commands::reset::ResetArgs),

    /// Generate shell completions
    Completions {
        /// Shell to generate for (bash, zsh, fish)
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

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

    /// Run proof-of-fitness experiments
    Experiment {
        #[command(subcommand)]
        command: commands::experiment::ExperimentCommands,
    },

    /// Manage the distributed fleet
    Fleet {
        #[command(subcommand)]
        command: commands::fleet::FleetCommands,
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
    Create {
        /// Name for the new skill
        name: String,
        /// Comma-separated capabilities to pre-fill in the manifest
        #[arg(long)]
        capabilities: Option<String>,
        /// Primary tool name in the manifest
        #[arg(long)]
        tool_name: Option<String>,
        /// Directory to create the project in
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionsCommands {
    /// List all sessions
    List(commands::sessions::ListArgs),

    /// Export full conversation from a session
    Export(commands::sessions::ExportArgs),
}

#[derive(Subcommand)]
enum TailscaleCommands {
    /// Generate a TLS certificate for HTTPS
    Cert {
        /// Tailscale DNS name to request a certificate for
        #[arg(long)]
        hostname: Option<String>,
    },
}

const FAWX_TUI_NOT_FOUND_MESSAGE: &str =
    "fawx-tui binary not found. Build it with: cargo build --release -p fawx-tui";

fn build_config_manager(
    config: &fx_config::FawxConfig,
) -> Arc<std::sync::Mutex<fx_config::manager::ConfigManager>> {
    let data_dir = config
        .general
        .data_dir
        .clone()
        .unwrap_or_else(startup::fawx_data_dir);
    let config_path = data_dir.join("config.toml");
    let manager = fx_config::manager::ConfigManager::from_config(config.clone(), config_path);
    Arc::new(std::sync::Mutex::new(manager))
}

fn build_subagent_manager(
    router: Arc<std::sync::RwLock<fx_llm::ModelRouter>>,
    config: &fx_config::FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    session_bus: Option<fx_bus::SessionBus>,
    credential_store: Option<startup::SharedCredentialStore>,
) -> Arc<fx_subagent::SubagentManager> {
    let token_broker = startup::build_token_broker(config, credential_store.as_ref());
    let factory = headless::HeadlessSubagentFactory::new(headless::HeadlessSubagentFactoryDeps {
        router,
        config: config.clone(),
        improvement_provider,
        session_bus,
        token_broker,
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
    session_bus: Option<fx_bus::SessionBus>,
) -> startup::HeadlessLoopBuildOptions {
    startup::HeadlessLoopBuildOptions {
        memory_enabled: true,
        subagent_control: Some(
            Arc::clone(subagent_manager) as Arc<dyn fx_subagent::SubagentControl>
        ),
        config_manager,
        session_bus,
        ..startup::HeadlessLoopBuildOptions::default()
    }
}

fn launch_fawx_tui(args: &[String]) -> anyhow::Result<i32> {
    let tui_binary = find_fawx_tui_binary()?;
    let status = std::process::Command::new(&tui_binary)
        .args(args)
        .status()?;
    Ok(status.code().unwrap_or(1))
}

fn launch_embedded_tui() -> anyhow::Result<i32> {
    launch_fawx_tui(&embedded_tui_args())
}

fn embedded_tui_args() -> Vec<String> {
    vec!["--embedded".to_string()]
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
    _logging_guard: tracing_appender::non_blocking::WorkerGuard,
    #[cfg(feature = "http")]
    http_config: fx_config::HttpConfig,
    #[cfg(feature = "http")]
    telegram_config: fx_config::TelegramChannelConfig,
    #[cfg(feature = "http")]
    webhook_config: fx_config::WebhookConfig,
    #[cfg(feature = "http")]
    data_dir: std::path::PathBuf,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
}

fn build_headless_startup(
    system_prompt: Option<std::path::PathBuf>,
    skip_session_db: bool,
    #[cfg(feature = "http")] wire_experiment_registry: bool,
) -> anyhow::Result<HeadlessStartup> {
    let config = startup::load_config()?;
    let logging_guard = headless::init_serve_logging(&config)?;
    let auth_manager = startup::load_auth_manager()?;
    let mut router = startup::build_router(&auth_manager)?;
    headless::seed_headless_router_active_model(&mut router, &config);
    let router = Arc::new(std::sync::RwLock::new(router));
    #[cfg(feature = "http")]
    let http_config = config.http.clone();
    #[cfg(feature = "http")]
    let telegram_config = config.telegram.clone();
    #[cfg(feature = "http")]
    let webhook_config = config.webhook.clone();
    let data_dir = startup::fawx_data_dir();
    let config_manager = Some(build_config_manager(&config));
    let improvement_provider = startup::build_improvement_provider(&auth_manager, &config);
    let improvement_provider_for_http = improvement_provider.clone();
    #[cfg(feature = "http")]
    let experiment_registry = if wire_experiment_registry {
        let registry_data_dir = startup::configured_data_dir(&data_dir, &config);
        Some(startup::build_shared_experiment_registry(
            &registry_data_dir,
        )?)
    } else {
        None
    };
    let app = build_headless_app(
        router,
        config,
        improvement_provider,
        system_prompt,
        config_manager,
        data_dir.clone(),
        skip_session_db,
        #[cfg(feature = "http")]
        experiment_registry,
    )?;
    Ok(HeadlessStartup {
        app,
        _logging_guard: logging_guard,
        #[cfg(feature = "http")]
        http_config,
        #[cfg(feature = "http")]
        telegram_config,
        #[cfg(feature = "http")]
        webhook_config,
        #[cfg(feature = "http")]
        data_dir,
        improvement_provider: improvement_provider_for_http,
    })
}

#[allow(clippy::too_many_arguments)] // Pre-existing constructor shape; follow-up will bundle args into a config struct.
fn build_headless_app(
    router: Arc<std::sync::RwLock<fx_llm::ModelRouter>>,
    config: fx_config::FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    system_prompt: Option<std::path::PathBuf>,
    config_manager: Option<Arc<std::sync::Mutex<fx_config::manager::ConfigManager>>>,
    data_dir: PathBuf,
    skip_session_db: bool,
    #[cfg(feature = "http")] experiment_registry: Option<fx_api::SharedExperimentRegistry>,
) -> anyhow::Result<headless::HeadlessApp> {
    let session_bus = startup::build_session_bus_for_data_dir(&data_dir);
    let credential_store = startup::open_credential_store(&data_dir).ok();
    let subagent_manager = build_subagent_manager(
        Arc::clone(&router),
        &config,
        improvement_provider.clone(),
        session_bus.clone(),
        credential_store.clone(),
    );
    let session_registry = (!skip_session_db)
        .then(|| startup::open_session_registry(&data_dir))
        .flatten();
    let options = startup::HeadlessLoopBuildOptions {
        session_registry,
        credential_store: credential_store.clone(),
        #[cfg(feature = "http")]
        experiment_registry: experiment_registry.clone(),
        ..parent_loop_build_options(
            &subagent_manager,
            config_manager.clone(),
            session_bus.clone(),
        )
    };
    let bundle =
        startup::build_headless_loop_engine_bundle(&config, improvement_provider, options)?;
    headless::HeadlessApp::new(headless::HeadlessAppDeps {
        loop_engine: bundle.engine,
        router,
        runtime_info: bundle.runtime_info,
        config,
        memory: bundle.memory,
        embedding_index_persistence: bundle.embedding_index_persistence,
        system_prompt_path: system_prompt,
        config_manager,
        system_prompt_text: None,
        subagent_manager,
        canary_monitor: Some(build_canary_monitor(&data_dir)),
        session_bus,
        session_key: Some(headless::main_session_key()),
        cron_store: bundle.cron_store,
        startup_warnings: bundle.startup_warnings,
        stream_callback_slot: bundle.stream_callback_slot,
        ripcord_journal: bundle.ripcord_journal,
        #[cfg(feature = "http")]
        experiment_registry,
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

async fn run_headless(
    single: bool,
    json: bool,
    system_prompt: Option<std::path::PathBuf>,
) -> anyhow::Result<i32> {
    #[cfg(feature = "http")]
    let startup = build_headless_startup(system_prompt, false, false)?;
    #[cfg(not(feature = "http"))]
    let startup = build_headless_startup(system_prompt, false)?;
    let HeadlessStartup {
        mut app,
        _logging_guard,
        ..
    } = startup;
    ensure_headless_chat_model_available(app.active_model())?;
    if single {
        app.run_single(json).await
    } else {
        app.run(json).await
    }
}

fn ensure_headless_chat_model_available(active_model: &str) -> anyhow::Result<()> {
    if active_model.is_empty() {
        return Err(headless::no_headless_models_available());
    }
    Ok(())
}

#[cfg(feature = "http")]
async fn run_http_server(
    system_prompt: Option<std::path::PathBuf>,
    port: u16,
) -> anyhow::Result<i32> {
    let HeadlessStartup {
        mut app,
        _logging_guard,
        http_config,
        telegram_config,
        webhook_config,
        data_dir,
        improvement_provider,
    } = build_headless_startup(system_prompt, true, true)?;
    app.initialize();
    app.apply_http_defaults();

    // Install SIGHUP handler for graceful restart.
    install_sighup_handler();

    // Open credential store once; pass to channel builders for DI.
    let auth_store = { auth_store::AuthStore::open(&data_dir).ok() };

    // Build external channels if configured.
    let telegram = build_telegram_channel(&telegram_config, auth_store.as_ref());
    let webhooks = build_webhook_channels(&webhook_config);

    http_serve::run(
        app,
        port,
        &http_config,
        telegram,
        webhooks,
        improvement_provider,
    )
    .await
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
        SkillCommands::Create {
            name,
            capabilities,
            tool_name,
            path,
        } => {
            commands::skills::create(
                &name,
                capabilities.as_deref(),
                tool_name.as_deref(),
                path.as_deref(),
            )?;
            Ok(0)
        }
    }
}

fn dispatch_sessions(command: SessionsCommands) -> anyhow::Result<i32> {
    match command {
        SessionsCommands::List(args) => commands::sessions::run_list(&args),
        SessionsCommands::Export(args) => commands::sessions::run_export(&args),
    }
}

fn dispatch_tailscale(command: TailscaleCommands) -> anyhow::Result<i32> {
    match command {
        TailscaleCommands::Cert { hostname } => {
            let layout = commands::runtime_layout::RuntimeLayout::detect()?;
            commands::tailscale::run_cert(hostname, &layout.data_dir)?;
            Ok(0)
        }
    }
}

/// Best-effort stale PID cleanup for `fawx serve`.
///
/// This uses OS process-liveness checks, so the end-to-end behavior is best
/// covered with an integration-style dead-child regression test rather than a
/// pure unit test.
fn cleanup_stale_pid_file() {
    cleanup_stale_pid_file_at(&restart::pid_file_path());
}

fn cleanup_stale_pid_file_at(pid_path: &Path) {
    #[cfg(not(unix))]
    let _ = pid_path;

    #[cfg(unix)]
    {
        let Ok(Some(pid)) = restart::read_pid_file(pid_path) else {
            return;
        };
        use nix::sys::signal;

        let is_alive = i32::try_from(pid)
            .ok()
            .map(nix::unistd::Pid::from_raw)
            .map(|process| {
                matches!(
                    signal::kill(process, None),
                    Ok(()) | Err(nix::errno::Errno::EPERM)
                )
            })
            .unwrap_or(false);
        if !is_alive {
            let _ = std::fs::remove_file(pid_path);
            eprintln!("Removed stale PID file (process {pid} is dead)");
        }
    }
}

async fn dispatch_command(command: Commands) -> anyhow::Result<i32> {
    match command {
        Commands::Tui { args } => launch_fawx_tui(&args),
        Commands::Start => commands::start_stop::run_start(),
        Commands::Stop => commands::start_stop::run_stop(),
        Commands::Chat => launch_embedded_tui(),
        Commands::Serve {
            single,
            json,
            system_prompt,
            http,
            port,
            data_dir,
            fleet,
        } => {
            if let Some(ref dir) = data_dir {
                std::env::set_var("FAWX_DATA_DIR", dir);
            }
            cleanup_stale_pid_file();
            let _pid_guard = restart::create_serve_pid_file_guard()?;
            if fleet {
                commands::serve_fleet::run().await
            } else if http {
                run_http_server(system_prompt, port).await
            } else {
                run_headless(single, json, system_prompt).await
            }
        }
        Commands::Restart(args) => restart::run(args),
        Commands::Update(args) => commands::update::run(args),
        Commands::Doctor => Ok(commands::doctor::run().await?),
        Commands::Status => Ok(commands::status::run().await?),
        Commands::Pair(args) => Ok(commands::pair::run(&args).await?),
        Commands::Devices(args) => Ok(commands::devices::run(&args).await?),
        Commands::Version => Ok(commands::version::run()),
        Commands::Logs(args) => commands::logs::run(&args),
        Commands::Sessions { command } => dispatch_sessions(command),
        Commands::SecurityAudit(args) => commands::security_audit::run(&args).await,
        Commands::Backup(args) => commands::backup::run(&args),
        Commands::Bootstrap {
            json,
            port,
            data_dir,
        } => Ok(commands::bootstrap::run(json, port, data_dir).await?),
        Commands::Import(args) => commands::import::run(&args),
        Commands::Setup { force } => Ok(commands::setup::run(force).await?),
        Commands::Tailscale { command } => dispatch_tailscale(command),
        Commands::Auth { command } => Ok(commands::auth::run(command).await?),
        Commands::Config { command } => {
            commands::config::run(command).await?;
            Ok(0)
        }
        Commands::Reset(args) => commands::reset::run(&args),
        Commands::Completions { shell } => commands::completions::run(shell),
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
        Commands::Experiment { command } => {
            let result = commands::experiment::run(command).await?;
            println!("{result}");
            Ok(0)
        }
        Commands::Fleet { command } => {
            commands::fleet::handle_fleet_command(&command).await?;
            Ok(0)
        }
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

fn init_cli_logging(
    command: &Commands,
) -> anyhow::Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    if matches!(command, Commands::Serve { .. }) {
        return Ok(None);
    }
    let logging = startup::load_config()
        .map(|config| config.logging)
        .unwrap_or_else(|error| {
            eprintln!("warning: failed to load config for logging: {error}");
            fx_config::LoggingConfig::default()
        });
    startup::init_logging(&logging, startup::LoggingMode::Tui)
        .map(Some)
        .map_err(anyhow::Error::from)
}

#[cfg(feature = "http")]
fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: Once = Once::new();

    INSTALL_PROVIDER.call_once(|| {
        // Reqwest/websocket paths can touch rustls even without the HTTPS listener,
        // so make the process-wide provider explicit at startup.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(not(feature = "http"))]
fn install_rustls_crypto_provider() {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    install_rustls_crypto_provider();
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Commands::Tui { args: Vec::new() });
    let _logging_guard = init_cli_logging(&command)?;
    let exit_code = dispatch_command(command).await?;
    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "http")]
    use super::{build_telegram_channel, telegram_webhook_secret_from_credential_store};
    use super::{
        cleanup_stale_pid_file_at, dispatch_command, ensure_headless_chat_model_available,
        fawx_tui_binary_name, find_fawx_tui_binary_from, resolve_ripcord_path_with,
        ripcord_binary_name, Cli, Commands, SessionsCommands, SkillCommands,
        FAWX_TUI_NOT_FOUND_MESSAGE,
    };
    use crate::auth_store::AuthStore;
    use crate::restart;
    use clap::Parser;
    use clap_complete::Shell;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{
        ffi::OsString,
        fs,
        path::Path,
        sync::{Mutex, OnceLock},
    };

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
    fn cli_parses_bootstrap_command() {
        let cli = Cli::parse_from([
            "fawx",
            "bootstrap",
            "--json",
            "--port",
            "9500",
            "--data-dir",
            "/tmp/fawx",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Bootstrap {
                json: true,
                port: Some(9500),
                data_dir: Some(path),
            }) if path == *std::path::Path::new("/tmp/fawx")
        ));
    }

    #[test]
    fn cli_parses_pair_command() {
        let cli = Cli::parse_from(["fawx", "pair", "--ttl", "90", "--json"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Pair(crate::commands::pair::PairArgs {
                ttl: 90,
                json: true
            }))
        ));
    }

    #[test]
    fn cli_parses_devices_revoke_command_with_json_flag_after_subcommand() {
        let cli = Cli::parse_from(["fawx", "devices", "revoke", "dev-123", "--json"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Devices(crate::commands::devices::DevicesArgs {
                json: true,
                command: Some(crate::commands::devices::DevicesCommand::Revoke { device_id })
            })) if device_id == "dev-123"
        ));
    }

    #[test]
    fn cli_parses_skill_create_command() {
        let cli = Cli::parse_from([
            "fawx",
            "skill",
            "create",
            "weather-skill",
            "--capabilities",
            "network,storage",
            "--tool-name",
            "weather_tool",
            "--path",
            "/tmp/skills",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Skill {
                command: SkillCommands::Create {
                    name,
                    capabilities,
                    tool_name,
                    path,
                }
            }) if name == "weather-skill"
                && capabilities.as_deref() == Some("network,storage")
                && tool_name.as_deref() == Some("weather_tool")
                && path.as_deref() == Some("/tmp/skills")
        ));
    }

    #[test]
    fn cli_parses_completions_command() {
        let cli = Cli::parse_from(["fawx", "completions", "bash"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Completions { shell: Shell::Bash })
        ));
    }

    #[test]
    fn cli_parses_bare_config_command() {
        let cli = Cli::parse_from(["fawx", "config"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config { command: None })
        ));
    }

    #[test]
    fn cli_parses_config_get_command() {
        let cli = Cli::parse_from(["fawx", "config", "get", "model.default_model"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                command: Some(crate::commands::config::ConfigCommands::Get { key })
            }) if key == "model.default_model"
        ));
    }

    #[test]
    fn cli_parses_reset_command() {
        let cli = Cli::parse_from(["fawx", "reset", "--memory", "--force"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Reset(crate::commands::reset::ResetArgs {
                memory: true,
                conversations: false,
                config: false,
                all: false,
                force: true,
            }))
        ));
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
    fn cli_parses_serve_fleet_flag() {
        let cli = Cli::parse_from(["fawx", "serve", "--fleet"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Serve {
                single: false,
                json: false,
                system_prompt: None,
                http: false,
                port: 8400,
                data_dir: None,
                fleet: true,
            })
        ));
    }

    #[test]
    fn cli_rejects_serve_fleet_with_http() {
        match Cli::try_parse_from(["fawx", "serve", "--fleet", "--http"]) {
            Ok(_) => panic!("serve --fleet --http should be rejected"),
            Err(error) => {
                assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
            }
        }
    }

    #[test]
    fn cli_parses_restart_rebuild_flag() {
        let cli = Cli::parse_from(["fawx", "restart", "--rebuild"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Restart(restart::RestartArgs {
                rebuild: true,
                hard: false,
                no_skills: false,
            }))
        ));
    }

    #[test]
    fn cli_parses_restart_rebuild_no_skills_flag() {
        let cli = Cli::parse_from(["fawx", "restart", "--rebuild", "--no-skills"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Restart(restart::RestartArgs {
                rebuild: true,
                hard: false,
                no_skills: true,
            }))
        ));
    }

    #[test]
    fn cli_parses_restart_hard_flag() {
        let cli = Cli::parse_from(["fawx", "restart", "--hard"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Restart(restart::RestartArgs {
                rebuild: false,
                hard: true,
                no_skills: false,
            }))
        ));
    }

    #[test]
    fn cli_parses_update_command() {
        let cli = Cli::parse_from(["fawx", "update", "dev", "--no-skills", "--no-restart"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Update(crate::commands::update::UpdateArgs {
                branch,
                no_pull: false,
                no_skills: true,
                no_restart: true,
                force: false,
            })) if branch.as_deref() == Some("dev")
        ));
    }

    #[test]
    fn cli_parses_logs_command() {
        let cli = Cli::parse_from(["fawx", "logs", "--lines", "100"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Logs(crate::commands::logs::LogsArgs {
                lines: 100,
                list: false
            }))
        ));
    }

    #[test]
    fn cli_parses_sessions_list_command() {
        let cli = Cli::parse_from(["fawx", "sessions", "list", "--json", "--kind", "subagent"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Sessions {
                command: SessionsCommands::List(crate::commands::sessions::ListArgs { json: true, kind })
            }) if kind.as_deref() == Some("subagent")
        ));
    }

    #[test]
    fn cli_parses_sessions_export_command() {
        let cli = Cli::parse_from([
            "fawx", "sessions", "export", "sess-123", "--json", "--limit", "5",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Sessions {
                command: SessionsCommands::Export(crate::commands::sessions::ExportArgs { id, json: true, limit: Some(5) })
            }) if id == "sess-123"
        ));
    }

    #[test]
    fn cli_parses_security_audit_flag() {
        let cli = Cli::parse_from(["fawx", "security-audit", "--update-baseline"]);
        assert!(matches!(
            cli.command,
            Some(Commands::SecurityAudit(
                crate::commands::security_audit::SecurityAuditArgs {
                    update_baseline: true
                }
            ))
        ));
    }

    fn assert_completion_output(shell: Shell) {
        let output = crate::commands::completions::render(shell).expect("generate completions");
        assert!(!output.trim().is_empty());
        assert!(output.contains("fawx"));
    }

    #[test]
    fn bash_completions_are_generated() {
        assert_completion_output(Shell::Bash);
    }

    #[test]
    fn zsh_completions_are_generated() {
        assert_completion_output(Shell::Zsh);
    }

    #[test]
    fn fish_completions_are_generated() {
        assert_completion_output(Shell::Fish);
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

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct PathGuard {
        previous: Option<OsString>,
    }

    impl PathGuard {
        fn set(path: &Path) -> (std::sync::MutexGuard<'static, ()>, Self) {
            let guard = env_lock().lock().expect("PATH env lock");
            let previous = std::env::var_os("PATH");
            std::env::set_var("PATH", path);
            (guard, Self { previous })
        }
    }

    impl Drop for PathGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => std::env::set_var("PATH", previous),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    #[cfg(unix)]
    fn write_argument_recording_executable(path: &Path, args_log: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
        let script = format!(
            r#"#!/bin/sh
printf '%s\n' "$@" > "{}"
exit 0
"#,
            args_log.display()
        );
        fs::write(path, script).expect("write executable");
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("set permissions");
    }

    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // PathGuard must live across async dispatch in test
    async fn chat_command_launches_tui_in_embedded_mode() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path_dir = tempdir.path().join("path");
        let args_log = tempdir.path().join("chat-args.log");
        let tui = path_dir.join(fawx_tui_binary_name());
        write_argument_recording_executable(&tui, &args_log);
        let (_guard, _path_guard) = PathGuard::set(&path_dir);

        let exit_code = dispatch_command(Commands::Chat).await.expect("dispatch");
        let args = fs::read_to_string(&args_log).expect("read args log");

        assert_eq!(exit_code, 0);
        assert_eq!(args.lines().collect::<Vec<_>>(), vec!["--embedded"]);
    }

    #[tokio::test]
    async fn setup_command_dispatches_to_setup_runner() {
        crate::commands::setup::set_test_exit_code(240);
        let exit_code = dispatch_command(Commands::Setup { force: true })
            .await
            .expect("dispatch");
        assert_eq!(exit_code, 241);
    }

    #[tokio::test]
    async fn serve_fleet_dispatches_to_fleet_worker_runner() {
        crate::commands::serve_fleet::set_test_exit_code(73);
        let exit_code = dispatch_command(Commands::Serve {
            single: false,
            json: false,
            system_prompt: None,
            http: false,
            port: 8400,
            data_dir: None,
            fleet: true,
        })
        .await
        .expect("dispatch");

        assert_eq!(exit_code, 73);
    }

    #[test]
    fn run_headless_guard_rejects_missing_active_model() {
        let error =
            ensure_headless_chat_model_available("").expect_err("empty active model should fail");

        assert_eq!(
            error.to_string(),
            "no models available in router; configure a provider and authenticate it before starting headless mode"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_stale_pid_file_removes_dead_process_pid() {
        let temp_dir = tempfile::TempDir::new().expect("tempdir");
        let pid_path = temp_dir.path().join("fawx.pid");
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(":")
            .spawn()
            .expect("spawn child");
        let pid = child.id();
        child.wait().expect("wait for child");
        fs::write(&pid_path, format!("{pid}\n")).expect("write pid file");

        cleanup_stale_pid_file_at(&pid_path);

        assert!(!pid_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_stale_pid_file_keeps_live_process_pid() {
        let temp_dir = tempfile::TempDir::new().expect("tempdir");
        let pid_path = temp_dir.path().join("fawx.pid");
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("sleep 30")
            .spawn()
            .expect("spawn child");
        let pid = child.id();
        fs::write(&pid_path, format!("{pid}\n")).expect("write pid file");

        cleanup_stale_pid_file_at(&pid_path);

        let preserved_pid = restart::read_pid_file(&pid_path).expect("read pid file");
        let _ = child.kill();
        child.wait().expect("wait for child");

        assert_eq!(preserved_pid, Some(pid));
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
