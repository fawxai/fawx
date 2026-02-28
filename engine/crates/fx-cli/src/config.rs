use serde::{Deserialize, Serialize};

const MAX_SYNTHESIS_INSTRUCTION_LENGTH: usize = 500;
const MIN_MAX_READ_SIZE: u64 = 1024;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG_TEMPLATE: &str = r#"# Fawx Configuration
# Location: ~/.fawx/config.toml

[general]
# data_dir = "~/.fawx"
# max_iterations = 10
# max_history = 20

[model]
# default_model = "anthropic/claude-sonnet-4-20250514"
# synthesis_instruction = "Be concise and direct."

[tools]
# working_dir = "/home/user/projects"
# search_exclude = ["vendor", "dist"]
# max_read_size = 1048576

[memory]
# max_entries = 1000
# max_value_size = 10240
# max_snapshot_chars = 2000
"#;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FawxConfig {
    pub general: GeneralConfig,
    pub model: ModelConfig,
    pub tools: ToolsConfig,
    pub memory: MemoryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GeneralConfig {
    pub data_dir: Option<PathBuf>,
    pub max_iterations: u32,
    pub max_history: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ModelConfig {
    pub default_model: Option<String>,
    pub synthesis_instruction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ToolsConfig {
    pub working_dir: Option<PathBuf>,
    pub search_exclude: Vec<String>,
    pub max_read_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MemoryConfig {
    pub max_entries: usize,
    pub max_value_size: usize,
    pub max_snapshot_chars: usize,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            data_dir: None,
            max_iterations: 10,
            max_history: 20,
        }
    }
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            working_dir: None,
            search_exclude: Vec::new(),
            max_read_size: 1024 * 1024,
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            max_value_size: 10240,
            max_snapshot_chars: 2000,
        }
    }
}

impl FawxConfig {
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        let config_path = data_dir.join("config.toml");
        if !config_path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&config_path)
            .map_err(|error| format!("failed to read config: {error}"))?;
        let config: Self =
            toml::from_str(&content).map_err(|error| format!("invalid config: {error}"))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.general.max_iterations == 0 {
            return Err("general.max_iterations must be >= 1".to_string());
        }
        if self.general.max_history == 0 {
            return Err("general.max_history must be >= 1".to_string());
        }
        if self.tools.max_read_size < MIN_MAX_READ_SIZE {
            return Err(format!(
                "tools.max_read_size must be >= {MIN_MAX_READ_SIZE}"
            ));
        }
        if self.memory.max_entries == 0 {
            return Err("memory.max_entries must be >= 1".to_string());
        }
        if let Some(instruction) = &self.model.synthesis_instruction {
            if instruction.len() > MAX_SYNTHESIS_INSTRUCTION_LENGTH {
                return Err(format!(
                    "model.synthesis_instruction exceeds {} characters",
                    MAX_SYNTHESIS_INSTRUCTION_LENGTH
                ));
            }
        }
        Ok(())
    }

    pub fn write_default(data_dir: &Path) -> Result<PathBuf, String> {
        let config_path = data_dir.join("config.toml");
        if config_path.exists() {
            return Err("config.toml already exists".to_string());
        }
        fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
        fs::write(&config_path, DEFAULT_CONFIG_TEMPLATE)
            .map_err(|error| format!("failed to write config: {error}"))?;
        Ok(config_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_default_when_no_file() {
        let temp = TempDir::new().expect("tempdir");
        let loaded = FawxConfig::load(temp.path()).expect("load defaults");
        assert_eq!(loaded, FawxConfig::default());
    }

    #[test]
    fn load_parses_valid_toml() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"
[general]
max_iterations = 15
max_history = 30

[model]
default_model = "gpt-4.1"
synthesis_instruction = "Stay concise"

[tools]
working_dir = "/tmp/work"
search_exclude = ["vendor", "dist"]
max_read_size = 4096

[memory]
max_entries = 200
max_value_size = 555
max_snapshot_chars = 777
"#;
        fs::write(temp.path().join("config.toml"), content).expect("write config");
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        assert_eq!(loaded.general.max_iterations, 15);
        assert_eq!(loaded.general.max_history, 30);
        assert_eq!(loaded.model.default_model.as_deref(), Some("gpt-4.1"));
        assert_eq!(loaded.tools.max_read_size, 4096);
        assert_eq!(loaded.memory.max_snapshot_chars, 777);
    }

    #[test]
    fn load_partial_config_uses_defaults() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[general]\nmax_iterations = 42\n";
        fs::write(temp.path().join("config.toml"), content).expect("write config");
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        assert_eq!(loaded.general.max_iterations, 42);
        assert_eq!(loaded.general.max_history, 20);
        assert_eq!(loaded.tools.max_read_size, 1024 * 1024);
        assert_eq!(loaded.memory.max_entries, 1000);
    }

    #[test]
    fn load_invalid_toml_returns_error() {
        let temp = TempDir::new().expect("tempdir");
        fs::write(
            temp.path().join("config.toml"),
            "[general\nmax_iterations = 5",
        )
        .expect("write config");
        let error = FawxConfig::load(temp.path()).expect_err("should fail");
        assert!(error.contains("invalid config"));
    }

    #[test]
    fn write_default_creates_file() {
        let temp = TempDir::new().expect("tempdir");
        let path = FawxConfig::write_default(temp.path()).expect("create default config");
        assert!(path.exists());
        let content = fs::read_to_string(path).expect("read config");
        assert!(content.contains("# Fawx Configuration"));
    }

    #[test]
    fn write_default_refuses_overwrite() {
        let temp = TempDir::new().expect("tempdir");
        fs::write(temp.path().join("config.toml"), "[general]\n").expect("write config");
        let error = FawxConfig::write_default(temp.path()).expect_err("should refuse overwrite");
        assert!(error.contains("already exists"));
    }

    #[test]
    fn default_values_are_sensible() {
        let defaults = FawxConfig::default();
        assert_eq!(defaults.general.max_iterations, 10);
        assert_eq!(defaults.general.max_history, 20);
        assert_eq!(defaults.tools.max_read_size, 1024 * 1024);
        assert_eq!(defaults.memory.max_entries, 1000);
        assert_eq!(defaults.memory.max_value_size, 10240);
        assert_eq!(defaults.memory.max_snapshot_chars, 2000);
    }

    #[test]
    fn config_fields_roundtrip() {
        let original = FawxConfig {
            general: GeneralConfig {
                data_dir: Some(PathBuf::from("/tmp/data")),
                max_iterations: 9,
                max_history: 99,
            },
            model: ModelConfig {
                default_model: Some("claude-sonnet".to_string()),
                synthesis_instruction: Some("short answers".to_string()),
            },
            tools: ToolsConfig {
                working_dir: Some(PathBuf::from("/tmp/work")),
                search_exclude: vec!["vendor".to_string()],
                max_read_size: 2048,
            },
            memory: MemoryConfig {
                max_entries: 4,
                max_value_size: 5,
                max_snapshot_chars: 6,
            },
        };

        let encoded = toml::to_string(&original).expect("serialize");
        let decoded: FawxConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, original);
    }

    #[test]
    fn load_rejects_zero_max_iterations() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[general]\nmax_iterations = 0\n";
        fs::write(temp.path().join("config.toml"), content).expect("write config");
        let error = FawxConfig::load(temp.path()).expect_err("should reject zero");
        assert!(error.contains("max_iterations must be >= 1"));
    }

    #[test]
    fn load_rejects_zero_max_history() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[general]\nmax_history = 0\n";
        fs::write(temp.path().join("config.toml"), content).expect("write config");
        let error = FawxConfig::load(temp.path()).expect_err("should reject zero");
        assert!(error.contains("max_history must be >= 1"));
    }

    #[test]
    fn load_rejects_tiny_max_read_size() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[tools]\nmax_read_size = 100\n";
        fs::write(temp.path().join("config.toml"), content).expect("write config");
        let error = FawxConfig::load(temp.path()).expect_err("should reject small value");
        assert!(error.contains("max_read_size must be >= 1024"));
    }

    #[test]
    fn load_rejects_zero_max_entries() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[memory]\nmax_entries = 0\n";
        fs::write(temp.path().join("config.toml"), content).expect("write config");
        let error = FawxConfig::load(temp.path()).expect_err("should reject zero");
        assert!(error.contains("max_entries must be >= 1"));
    }

    #[test]
    fn load_rejects_oversized_synthesis_instruction() {
        let temp = TempDir::new().expect("tempdir");
        let long_value = "x".repeat(501);
        let content = format!("[model]\nsynthesis_instruction = \"{}\"\n", long_value);
        fs::write(temp.path().join("config.toml"), content).expect("write config");
        let error = FawxConfig::load(temp.path()).expect_err("should reject long instruction");
        assert!(error.contains("synthesis_instruction exceeds 500 characters"));
    }

    #[test]
    fn load_accepts_max_length_synthesis_instruction() {
        let temp = TempDir::new().expect("tempdir");
        let value = "x".repeat(500);
        let content = format!("[model]\nsynthesis_instruction = \"{}\"\n", value);
        fs::write(temp.path().join("config.toml"), content).expect("write config");
        let config = FawxConfig::load(temp.path()).expect("should accept 500 chars");
        assert_eq!(config.model.synthesis_instruction.unwrap().len(), 500);
    }
}
