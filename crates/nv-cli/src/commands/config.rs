//! Configuration display command

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
struct Config {
    #[serde(default)]
    agent: AgentConfig,

    #[serde(default)]
    security: SecurityConfig,

    #[serde(default)]
    llm: LlmConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct AgentConfig {
    #[serde(default = "default_name")]
    name: String,

    #[serde(default = "default_workspace")]
    workspace: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SecurityConfig {
    #[serde(default = "default_bool_true")]
    require_confirmation: bool,

    #[serde(default = "default_bool_true")]
    audit_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct LlmConfig {
    #[serde(default = "default_model")]
    model: String,

    #[serde(default)]
    api_key: Option<String>,
}

fn default_name() -> String {
    "Nova".to_string()
}

fn default_workspace() -> String {
    "~/.nova".to_string()
}

fn default_bool_true() -> bool {
    true
}

fn default_model() -> String {
    "llama-3-8b".to_string()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: default_name(),
            workspace: default_workspace(),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            require_confirmation: true,
            audit_enabled: true,
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            api_key: None,
        }
    }
}

/// Show current configuration
pub fn run() -> anyhow::Result<()> {
    let config = load_config()?;
    let redacted_config = redact_sensitive(&config);

    let toml_str = toml::to_string_pretty(&redacted_config)?;
    println!("Current configuration:\n");
    println!("{}", toml_str);

    Ok(())
}

fn load_config() -> anyhow::Result<Config> {
    let config_path = get_config_path();

    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    } else {
        // Return default config
        Ok(Config::default())
    }
}

fn get_config_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".nova")
        .join("config.toml")
}

fn redact_sensitive(config: &Config) -> Config {
    let mut redacted = config.clone();

    if let Some(ref key) = redacted.llm.api_key {
        if !key.is_empty() {
            // Redact API keys: show first few chars and "...REDACTED"
            let prefix = if key.len() > 6 {
                &key[..6]
            } else {
                &key[..key.len().min(2)]
            };
            redacted.llm.api_key = Some(format!("{}...REDACTED", prefix));
        }
    }

    redacted
}

// Implement Clone manually since we need it for redaction
impl Clone for Config {
    fn clone(&self) -> Self {
        Self {
            agent: self.agent.clone(),
            security: self.security.clone(),
            llm: self.llm.clone(),
        }
    }
}

impl Clone for AgentConfig {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            workspace: self.workspace.clone(),
        }
    }
}

impl Clone for SecurityConfig {
    fn clone(&self) -> Self {
        Self {
            require_confirmation: self.require_confirmation,
            audit_enabled: self.audit_enabled,
        }
    }
}

impl Clone for LlmConfig {
    fn clone(&self) -> Self {
        Self {
            model: self.model.clone(),
            api_key: self.api_key.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_valid() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).expect("Failed to serialize");
        assert!(!toml_str.is_empty());
        assert!(toml_str.contains("Nova"));
    }

    #[test]
    fn test_redact_hides_api_keys() {
        let mut config = Config::default();
        config.llm.api_key = Some("sk-1234567890abcdef".to_string());

        let redacted = redact_sensitive(&config);

        assert!(redacted.llm.api_key.is_some());
        let redacted_key = redacted.llm.api_key.unwrap();
        assert!(redacted_key.contains("REDACTED"));
        assert!(!redacted_key.contains("1234567890abcdef"));
    }

    #[test]
    fn test_redact_preserves_non_sensitive() {
        let mut config = Config::default();
        config.agent.name = "TestAgent".to_string();
        config.llm.model = "test-model".to_string();
        config.llm.api_key = Some("sk-secret".to_string());

        let redacted = redact_sensitive(&config);

        assert_eq!(redacted.agent.name, "TestAgent");
        assert_eq!(redacted.llm.model, "test-model");
        assert!(redacted.llm.api_key.unwrap().contains("REDACTED"));
    }

    #[test]
    fn test_redact_empty_key() {
        let mut config = Config::default();
        config.llm.api_key = Some("".to_string());

        let redacted = redact_sensitive(&config);
        assert_eq!(redacted.llm.api_key, Some("".to_string()));
    }

    #[test]
    fn test_redact_none_key() {
        let config = Config::default();
        let redacted = redact_sensitive(&config);
        assert!(redacted.llm.api_key.is_none());
    }

    #[test]
    fn test_redact_short_key() {
        let mut config = Config::default();
        config.llm.api_key = Some("abc".to_string());

        let redacted = redact_sensitive(&config);
        let redacted_key = redacted.llm.api_key.unwrap();
        assert!(redacted_key.contains("REDACTED"));
    }
}
