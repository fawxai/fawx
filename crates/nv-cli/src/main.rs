//! Nova CLI - Management interface for the Nova agent.

mod commands;
mod confirmation;

use clap::{Parser, Subcommand};

pub use confirmation::ConfirmationUi;

#[derive(Parser)]
#[command(name = "nova")]
#[command(about = "Nova AI Agent CLI", long_about = None)]
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
            println!("Starting Nova agent daemon...");
            println!("(Implementation pending - Epic 9)");
            0
        }
        Commands::Stop => {
            println!("Stopping Nova agent daemon...");
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
                println!("Installed skills:");
                println!("(Skill management pending - Epic 8)");
                0
            }
            SkillCommands::Install { path } => {
                println!("Installing skill from: {}", path);
                println!("(Skill installation pending - Epic 8)");
                0
            }
            SkillCommands::Remove { name } => {
                println!("Removing skill: {}", name);
                println!("(Skill removal pending - Epic 8)");
                0
            }
        },
    };

    std::process::exit(exit_code);
}
