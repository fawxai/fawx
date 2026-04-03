use super::*;

pub(super) fn handle_headless_keys_command(
    base_dir: &Path,
    subcommand: Option<&str>,
    value: Option<&str>,
    option: Option<&str>,
    has_extra_args: bool,
) -> anyhow::Result<String> {
    match subcommand {
        Some("list") if value.is_none() && option.is_none() && !has_extra_args => {
            crate::commands::keys::list_output(Some(base_dir))
        }
        Some("list") => Ok("Usage: /keys list".to_string()),
        Some(other) => Ok(keys_redirect_message(other)),
        None => Ok("Usage: /keys list".to_string()),
    }
}

fn keys_redirect_message(subcommand: &str) -> String {
    format!("Use `fawx keys {subcommand}` CLI for key management.")
}
