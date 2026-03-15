use super::runtime_layout::RuntimeLayout;
use crate::config_redaction::sanitize_toml;
use anyhow::Context;
use clap::Subcommand;
use fx_config::{manager::ConfigManager, FawxConfig};
use toml::Value as TomlValue;

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommands {
    /// Show the full redacted configuration
    Show,
    /// Read a config key or section
    Get {
        /// Dot-path key or section name
        key: String,
    },
    /// Update a config value
    Set {
        /// Dot-path key
        key: String,
        /// New value
        value: String,
    },
}

pub async fn run(command: Option<ConfigCommands>) -> anyhow::Result<()> {
    let mut manager = load_config_manager()?;
    let output = execute(command, &mut manager)?;
    println!("{output}");
    Ok(())
}

pub(crate) fn execute(
    command: Option<ConfigCommands>,
    manager: &mut ConfigManager,
) -> anyhow::Result<String> {
    match command.unwrap_or(ConfigCommands::Show) {
        ConfigCommands::Show => show_config(manager.config()),
        ConfigCommands::Get { key } => get_config(manager.config(), &key),
        ConfigCommands::Set { key, value } => set_config(manager, &key, &value),
    }
}

fn load_config_manager() -> anyhow::Result<ConfigManager> {
    let layout = RuntimeLayout::detect()?;
    Ok(ConfigManager::from_config(
        layout.config,
        layout.config_path,
    ))
}

fn show_config(config: &FawxConfig) -> anyhow::Result<String> {
    let value = config_toml_value(config)?;
    let redacted = sanitize_toml(value);
    toml::to_string_pretty(&redacted).context("failed to render config")
}

fn get_config(config: &FawxConfig, key: &str) -> anyhow::Result<String> {
    if key == "all" {
        return show_config(config);
    }
    let selected = select_redacted_value(config, key)?;
    render_selected_value(key, &selected)
}

fn select_redacted_value(config: &FawxConfig, key: &str) -> anyhow::Result<TomlValue> {
    let selected = select_config_value(config, key)?;
    redact_selected_value(key, selected)
}

fn redact_selected_value(key: &str, value: TomlValue) -> anyhow::Result<TomlValue> {
    let wrapped = wrap_selection(key, value);
    let redacted = sanitize_toml(wrapped);
    lookup_value(&redacted, key).ok_or_else(|| anyhow::anyhow!("failed to render config selection"))
}

fn set_config(manager: &mut ConfigManager, key: &str, value: &str) -> anyhow::Result<String> {
    manager.set(key, value).map_err(anyhow::Error::msg)?;
    let rendered = get_config(manager.config(), key)?;
    Ok(format!("Updated {key}\n{rendered}"))
}

fn select_config_value(config: &FawxConfig, key: &str) -> anyhow::Result<TomlValue> {
    let value = config_toml_value(config)?;
    lookup_value(&value, key).ok_or_else(|| unknown_key_error(key))
}

fn config_toml_value(config: &FawxConfig) -> anyhow::Result<TomlValue> {
    TomlValue::try_from(config.clone()).context("failed to serialize config")
}

fn lookup_value(value: &TomlValue, key: &str) -> Option<TomlValue> {
    key.split('.')
        .try_fold(value, |current, segment| match current {
            TomlValue::Table(table) => table.get(segment),
            _ => None,
        })
        .cloned()
}

fn unknown_key_error(key: &str) -> anyhow::Error {
    anyhow::anyhow!("unknown config key or section: '{key}'")
}

fn render_selected_value(key: &str, value: &TomlValue) -> anyhow::Result<String> {
    match value {
        TomlValue::String(text) => Ok(text.clone()),
        TomlValue::Integer(number) => Ok(number.to_string()),
        TomlValue::Float(number) => Ok(number.to_string()),
        TomlValue::Boolean(flag) => Ok(flag.to_string()),
        TomlValue::Datetime(datetime) => Ok(datetime.to_string()),
        other => render_wrapped_value(key, other),
    }
}

fn render_wrapped_value(key: &str, value: &TomlValue) -> anyhow::Result<String> {
    let wrapped = wrap_selection(key, value.clone());
    toml::to_string_pretty(&wrapped)
        .context("failed to render config selection")
        .map(trim_trailing_newline)
}

fn wrap_selection(key: &str, value: TomlValue) -> TomlValue {
    key.split('.').rev().fold(value, wrap_segment)
}

fn wrap_segment(value: TomlValue, segment: &str) -> TomlValue {
    let mut table = toml::map::Map::new();
    table.insert(segment.to_string(), value);
    TomlValue::Table(table)
}

fn trim_trailing_newline(text: String) -> String {
    text.trim_end_matches('\n').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct TestManager {
        _temp: TempDir,
        manager: ConfigManager,
    }

    fn manager_from(content: &str) -> TestManager {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(temp.path().join("config.toml"), content).expect("write config");
        let config = FawxConfig::load(temp.path()).expect("load config");
        let manager = ConfigManager::from_config(config, temp.path().join("config.toml"));
        TestManager {
            _temp: temp,
            manager,
        }
    }

    #[test]
    fn bare_config_behaves_like_show() {
        let mut test_manager = manager_from("[http]\nbearer_token = \"secret\"\n");
        let bare = execute(None, &mut test_manager.manager).expect("bare config");
        let show = execute(Some(ConfigCommands::Show), &mut test_manager.manager).expect("show");
        assert_eq!(bare, show);
    }

    #[test]
    fn get_returns_scalar_value_cleanly() {
        let mut test_manager = manager_from("[model]\ndefault_model = \"opus\"\n");
        let output = execute(
            Some(ConfigCommands::Get {
                key: "model.default_model".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect("get config");
        assert_eq!(output, "opus");
    }

    #[test]
    fn get_returns_section_as_toml() {
        let mut test_manager = manager_from("[model]\ndefault_model = \"opus\"\n");
        let output = execute(
            Some(ConfigCommands::Get {
                key: "model".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect("get section");
        assert!(output.contains("[model]"));
        assert!(output.contains("default_model = \"opus\""));
    }

    #[test]
    fn get_redacts_secret_scalar_values() {
        let mut test_manager = manager_from("[http]\nbearer_token = \"super-secret\"\n");
        let output = execute(
            Some(ConfigCommands::Get {
                key: "http.bearer_token".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect("get secret value");
        assert_eq!(output, "[REDACTED]");
    }

    #[test]
    fn get_redacts_secret_values_inside_sections() {
        let mut test_manager =
            manager_from("[telegram]\nbot_token = \"bot-secret\"\nallowed_chat_ids = [1]\n");
        let output = execute(
            Some(ConfigCommands::Get {
                key: "telegram".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect("get redacted section");
        assert!(output.contains("[REDACTED]"));
        assert!(output.contains("allowed_chat_ids = [1]"));
        assert!(!output.contains("bot-secret"));
    }

    #[test]
    fn set_updates_persisted_config_through_manager() {
        let mut test_manager = manager_from("[model]\ndefault_model = \"old\"\n");
        execute(
            Some(ConfigCommands::Set {
                key: "model.default_model".to_string(),
                value: "new".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect("set config");

        let stored = select_config_value(test_manager.manager.config(), "model.default_model")
            .expect("stored model");
        assert_eq!(stored.as_str(), Some("new"));

        let persisted =
            std::fs::read_to_string(test_manager.manager.config_path()).expect("persisted config");
        assert!(persisted.contains("default_model = \"new\""));
    }

    #[test]
    fn set_redacts_secret_values_after_update() {
        let mut test_manager = manager_from("[telegram]\nallowed_chat_ids = [1]\n");
        let output = execute(
            Some(ConfigCommands::Set {
                key: "telegram.bot_token".to_string(),
                value: "new-secret".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect("set secret config");
        assert!(output.contains("Updated telegram.bot_token"));
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("new-secret"));
        assert_eq!(
            select_config_value(test_manager.manager.config(), "telegram.bot_token")
                .expect("stored secret")
                .as_str(),
            Some("new-secret")
        );
    }

    #[test]
    fn invalid_set_value_surfaces_validation_error() {
        let mut test_manager = manager_from("[general]\nmax_iterations = 10\n");
        let error = execute(
            Some(ConfigCommands::Set {
                key: "general.max_iterations".to_string(),
                value: "0".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect_err("invalid set should fail");
        assert!(error.to_string().contains("must be >= 1"));
    }

    #[test]
    fn show_keeps_full_config_redacted() {
        let mut test_manager = manager_from(
            "[http]\nbearer_token = \"super-secret\"\n\n[telegram]\nbot_token = \"bot-secret\"\n",
        );
        let output =
            execute(Some(ConfigCommands::Show), &mut test_manager.manager).expect("show config");
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("super-secret"));
        assert!(!output.contains("bot-secret"));
    }

    #[test]
    fn show_uses_default_config_when_config_file_is_missing() {
        let temp = TempDir::new().expect("tempdir");
        let manager = ConfigManager::new(temp.path()).expect("manager");
        let output = show_config(manager.config()).expect("show defaults");
        assert!(output.contains("[general]"));
        assert!(output.contains("max_iterations = 10"));
    }

    #[test]
    fn set_then_get_round_trips_fawx_config_keys() {
        let mut test_manager = manager_from("");
        execute(
            Some(ConfigCommands::Set {
                key: "model.default_model".to_string(),
                value: "openai/gpt-5".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect("set config");
        let output = execute(
            Some(ConfigCommands::Get {
                key: "model.default_model".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect("get config");
        assert_eq!(output, "openai/gpt-5");
    }

    #[test]
    fn set_invalid_key_path_errors_cleanly() {
        let mut test_manager = manager_from("");
        let error = execute(
            Some(ConfigCommands::Set {
                key: "model.missing_field".to_string(),
                value: "openai/gpt-5".to_string(),
            }),
            &mut test_manager.manager,
        )
        .expect_err("invalid key should fail");
        assert!(error.to_string().contains("missing_field"));
    }
}
