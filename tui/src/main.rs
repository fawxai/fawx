use clap::Parser;
use fawx_tui::DEFAULT_ENGINE_URL;

#[derive(Debug, Parser)]
struct Args {
    /// Run in embedded mode (start engine in-process, no server needed)
    #[arg(long)]
    embedded: bool,

    /// Server host URL (ignored in embedded mode)
    #[arg(long, default_value = DEFAULT_ENGINE_URL)]
    host: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    fawx_tui::run_tui(fawx_tui::RunOptions {
        embedded: args.embedded,
        host: args.host,
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_embedded_flag() {
        let args = Args::parse_from(["fawx-tui", "--embedded"]);
        assert!(args.embedded);
        assert_eq!(args.host, DEFAULT_ENGINE_URL);
    }

    #[test]
    fn parses_host_override() {
        let args = Args::parse_from(["fawx-tui", "--host", "http://example.com:1234"]);
        assert!(!args.embedded);
        assert_eq!(args.host, "http://example.com:1234");
    }
}
