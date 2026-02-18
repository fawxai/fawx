//! Citros CLI - Management interface for the Citros agent.

mod commands;
mod confirmation;

use clap::{Parser, Subcommand};

pub use confirmation::ConfirmationUi;

#[derive(Parser)]
#[command(name = "citros")]
#[command(about = "Citros AI Agent CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the agent daemon
    Start,

    /// Stop the agent daemon
    Stop,

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

    let exit_code = match cli.command {
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
    };

    std::process::exit(exit_code);
}
