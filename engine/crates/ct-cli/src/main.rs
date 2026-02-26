//! Citros CLI - Management interface for the Citros agent.

mod commands;
mod confirmation;
mod tui;

use clap::{Parser, Subcommand, ValueEnum};

pub use confirmation::ConfirmationUi;

#[derive(Parser)]
#[command(name = "citros")]
#[command(about = "Citros AI Agent CLI", long_about = None)]
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let exit_code = match cli.command.unwrap_or(Commands::Tui) {
        Commands::Tui => {
            let auth_manager = tui::load_auth_manager().await?;
            let router = tui::build_router(&auth_manager)?;
            let mut app = tui::TuiApp::new(auth_manager, router);
            app.run().await?;
            0
        }
        Commands::Start => {
            println!("Starting Citros agent daemon...");
            println!("(Implementation pending - Epic 9)");
            0
        }
        Commands::Stop => {
            println!("Stopping Citros agent daemon...");
            println!("(Implementation pending - Epic 9)");
            0
        }
        Commands::Chat => commands::chat::run().await?,
        Commands::Doctor => commands::doctor::run().await?,
        Commands::Config => {
            commands::config::run().await?;
            0
        }
        Commands::Audit { command } => match command {
            AuditCommands::Show { limit } => {
                commands::audit::show(limit).await?;
                0
            }
            AuditCommands::Verify => commands::audit::verify().await?,
        },
        Commands::Skill { command } => match command {
            SkillCommands::List => {
                commands::skills::list().await?;
                0
            }
            SkillCommands::Install { path } => {
                commands::skills::install(&path).await?;
                0
            }
            SkillCommands::Remove { name } => {
                commands::skills::remove(&name).await?;
                0
            }
        },
        Commands::OauthBridge {
            listen,
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
        } => {
            let options = commands::oauth_bridge::Options {
                listen,
                auth_url,
                token_url,
                client_id,
                client_secret,
                scope,
            };
            commands::oauth_bridge::run(options).await?
        }
        Commands::EvalDeterminism {
            mode,
            output,
            baseline,
            update_baseline,
            fail_on_regression,
        } => {
            let options = commands::eval_harness::Options {
                mode: mode.into(),
                output: output.into(),
                baseline: baseline.map(Into::into),
                update_baseline,
                fail_on_regression,
            };
            commands::eval_harness::run(options)?
        }
    };

    std::process::exit(exit_code);
}
