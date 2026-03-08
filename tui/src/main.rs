#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fawx_tui::run_tui().await
}
