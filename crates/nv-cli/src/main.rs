//! Nova CLI - Management interface for the Nova agent.

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

    /// Manage skills
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
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

    match cli.command {
        Commands::Start => {
            println!("Starting Nova agent daemon...");
            println!("(Implementation pending - Epic 9)");
        }
        Commands::Stop => {
            println!("Stopping Nova agent daemon...");
            println!("(Implementation pending - Epic 9)");
        }
        Commands::Chat => {
            println!("Nova interactive chat");
            println!("(Implementation pending - Epic 9)");
        }
        Commands::Doctor => {
            println!("Running diagnostics...");
            println!("✓ Workspace compiled successfully");
            println!("(Full diagnostics pending - Epic 9)");
        }
        Commands::Config => {
            println!("Current configuration:");
            println!("(Configuration display pending - Epic 9)");
        }
        Commands::Skill { command } => match command {
            SkillCommands::List => {
                println!("Installed skills:");
                println!("(Skill management pending - Epic 8)");
            }
            SkillCommands::Install { path } => {
                println!("Installing skill from: {}", path);
                println!("(Skill installation pending - Epic 8)");
            }
            SkillCommands::Remove { name } => {
                println!("Removing skill: {}", name);
                println!("(Skill removal pending - Epic 8)");
            }
        },
    }

    Ok(())
}
