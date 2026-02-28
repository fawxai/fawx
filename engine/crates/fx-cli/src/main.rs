//! Fawx CLI - Management interface for the Fawx agent.

mod commands;
mod confirmation;
mod conversation_store;
mod json_memory;
mod tools;
mod tui;

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

    /// Run OAuth bridge server for Android Codex sign-in
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
}

async fn run_tui() -> anyhow::Result<i32> {
    let auth_manager = tui::load_auth_manager().await?;
    let router = tui::build_router(&auth_manager)?;
    let loop_engine = tui::build_loop_engine();
    let mut app = tui::TuiApp::new(auth_manager, router, loop_engine)?;
    app.run().await?;
    Ok(0)
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
    }
}

async fn dispatch_command(command: Commands) -> anyhow::Result<i32> {
    match command {
        Commands::Tui => run_tui().await,
        Commands::Start => Ok(run_stub("Starting")),
        Commands::Stop => Ok(run_stub("Stopping")),
        Commands::Chat => Ok(commands::chat::run().await?),
        Commands::Doctor => Ok(commands::doctor::run().await?),
        Commands::Config => {
            commands::config::run().await?;
            Ok(0)
        }
        Commands::Audit { command } => dispatch_audit(command).await,
        Commands::Skill { command } => dispatch_skill(command).await,
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
