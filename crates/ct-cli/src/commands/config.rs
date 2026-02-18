//! Configuration display command

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct Config {
    #[serde(default)]
    agent: AgentConfig,

    #[serde(default)]
    security: SecurityConfig,

    #[serde(default)]
    llm: LlmConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AgentConfig {
    #[serde(default = "default_name")]
    name: String,

    #[serde(default = "default_workspace")]
    workspace: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SecurityConfig {
    #[serde(default = "default_bool_true")]
    require_confirmation: bool,

    #[serde(default = "default_bool_true")]
    audit_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LlmConfig {
    #[serde(default = "default_model")]
    model: String,

    #[serde(default)]
    api_key: Option<String>,

    #[serde(default)]
    claude_setup_token: Option<String>,

    #[serde(default)]
    claude_sdk_url: Option<String>,

    #[serde(default)]
    openai_oauth_token: Option<String>,
}

fn default_name() -> String {
    "Citros".to_string()
}

fn default_workspace() -> String {
    "~/.citros".to_string()
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
            claude_setup_token: None,
            claude_sdk_url: None,
            openai_oauth_token: None,
        }
    }
}

/// Show current configuration
pub async fn run() -> anyhow::Result<()> {
    let config = load_config().await?;
    let redacted_config = redact_sensitive(&config);

    let toml_str = toml::to_string_pretty(&redacted_config)?;
    println!("Current configuration:\n");
    println!("{}", toml_str);

    Ok(())
}

async fn load_config() -> anyhow::Result<Config> {
    let config_path = get_config_path()?;

    if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    } else {
        // Return default config
        Ok(Config::default())
    }
}

fn get_config_path() -> anyhow::Result<std::path::PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home.join(".citros").join("config.toml"))
}

fn redact_sensitive(config: &Config) -> Config {
    let mut redacted = config.clone();

    redacted.llm.api_key = redact_secret(redacted.llm.api_key.as_deref());
    redacted.llm.claude_setup_token = redact_secret(redacted.llm.claude_setup_token.as_deref());
    redacted.llm.openai_oauth_token = redact_secret(redacted.llm.openai_oauth_token.as_deref());
    redacted.llm.claude_sdk_url = redact_secret(redacted.llm.claude_sdk_url.as_deref());

    redacted
}

fn redact_secret(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.is_empty() {
        return Some(String::new());
    }

    // Redact tokens/keys while preserving a tiny prefix for debugging.
    let prefix = if value.len() > 6 {
        &value[..6]
    } else {
        &value[..value.len().min(2)]
    };
    Some(format!("{}...REDACTED", prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_valid() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).expect("Failed to serialize");
        assert!(!toml_str.is_empty());
        assert!(toml_str.contains("Citros"));
    }

    #[test]
    fn test_redact_hides_api_keys() {
        let mut config = Config::default();
        config.llm.api_key = Some("sk-1234567890abcdef".to_string());
        config.llm.claude_setup_token = Some("claude-sub-token-abc123".to_string());
        config.llm.openai_oauth_token = Some("openai-oauth-token-xyz999".to_string());

        let redacted = redact_sensitive(&config);

        assert!(redacted.llm.api_key.is_some());
        let redacted_key = redacted.llm.api_key.unwrap();
        assert!(redacted_key.contains("REDACTED"));
        assert!(!redacted_key.contains("1234567890abcdef"));

        let redacted_claude_token = redacted.llm.claude_setup_token.unwrap();
        assert!(redacted_claude_token.contains("REDACTED"));
        assert!(!redacted_claude_token.contains("abc123"));

        let redacted_openai_token = redacted.llm.openai_oauth_token.unwrap();
        assert!(redacted_openai_token.contains("REDACTED"));
        assert!(!redacted_openai_token.contains("xyz999"));
    }

    #[test]
    fn test_redact_preserves_non_sensitive() {
        let mut config = Config::default();
        config.agent.name = "TestAgent".to_string();
        config.llm.model = "test-model".to_string();
        config.llm.api_key = Some("sk-secret".to_string());
        config.llm.claude_setup_token = Some("claude-secret".to_string());
        config.llm.openai_oauth_token = Some("openai-secret".to_string());

        let redacted = redact_sensitive(&config);

        assert_eq!(redacted.agent.name, "TestAgent");
        assert_eq!(redacted.llm.model, "test-model");
        assert!(redacted.llm.api_key.unwrap().contains("REDACTED"));
        assert!(redacted
            .llm
            .claude_setup_token
            .unwrap()
            .contains("REDACTED"));
        assert!(redacted
            .llm
            .openai_oauth_token
            .unwrap()
            .contains("REDACTED"));
    }

    #[test]
    fn test_redact_empty_key() {
        let mut config = Config::default();
        config.llm.api_key = Some("".to_string());

        let redacted = redact_sensitive(&config);
        assert_eq!(redacted.llm.api_key, Some("".to_string()));
        assert_eq!(redacted.llm.claude_setup_token, None);
        assert_eq!(redacted.llm.openai_oauth_token, None);
    }

    #[test]
    fn test_redact_none_key() {
        let config = Config::default();
        let redacted = redact_sensitive(&config);
        assert!(redacted.llm.api_key.is_none());
        assert!(redacted.llm.claude_setup_token.is_none());
        assert!(redacted.llm.openai_oauth_token.is_none());
    }

    #[test]
    fn test_redact_short_key() {
        let mut config = Config::default();
        config.llm.api_key = Some("abc".to_string());

        let redacted = redact_sensitive(&config);
        let redacted_key = redacted.llm.api_key.unwrap();
        assert!(redacted_key.contains("REDACTED"));
    }

    #[test]
    fn test_redact_secret_empty_and_none() {
        assert_eq!(redact_secret(None), None);
        assert_eq!(redact_secret(Some("")), Some("".to_string()));
    }
}
