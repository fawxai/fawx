//! Interactive chat REPL

use std::io::{self, Write};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Run the interactive chat REPL
pub async fn run() -> anyhow::Result<i32> {
    println!("Nova v0.1.0");
    println!("Type /help for available commands, /quit to exit");
    println!();

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        print!("> ");
        io::stdout().flush()?;

        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            // EOF
            break;
        }

        let input = line.trim();

        if input.is_empty() {
            continue;
        }

        match input {
            "/quit" | "/exit" => {
                println!("Goodbye!");
                break;
            }
            "/help" => {
                show_help();
            }
            _ => {
                // Placeholder response - actual agent integration comes later
                println!("Echo: {}", input);
                println!("(Actual agent integration pending - Epic 9)");
            }
        }
    }

    Ok(0)
}

fn show_help() {
    println!("Available commands:");
    println!("  /help   - Show this help message");
    println!("  /quit   - Exit the chat");
    println!("  /exit   - Exit the chat");
    println!();
    println!("Type any message to chat with Nova (integration pending)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_produces_text() {
        // Capture stdout to verify help text is produced
        // For now, just ensure the function doesn't panic
        show_help();
    }
}
