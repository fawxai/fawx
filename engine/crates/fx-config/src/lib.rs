use serde::{Deserialize, Serialize};
use toml_edit::{value, DocumentMut, Item, Table};

const MAX_SYNTHESIS_INSTRUCTION_LENGTH: usize = 500;
const MIN_MAX_READ_SIZE: u64 = 1024;
use std::fs;
use std::path::{Path, PathBuf};

/// Default deny patterns for self-modification path enforcement.
///
/// These patterns are duplicated from `fx_core::self_modify::DEFAULT_DENY_PATHS`
/// to keep fx-config independent of fx-core. If these defaults change, update
/// both locations.
pub(crate) const DEFAULT_DENY_PATHS: &[&str] = &[".git/**", "*.key", "*.pem", "credentials.*"];

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
# max_relevant_results = 5

# [security]
# require_signatures = false

# [self_modify]
# enabled = false
# branch_prefix = "fawx/improve"
# require_tests = true
# [self_modify.paths]
# allow = []
# propose = []
# deny = [".git/**", "*.key", "*.pem", "credentials.*"]
# proposals_dir = "~/.fawx/proposals"

# [http]
# bearer_token = "your-secret-token"

# [improvement]
# enabled = false
# max_analyses_per_hour = 10
# max_proposals_per_day = 3
# auto_branch_prefix = "fawx/improve"
"#;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FawxConfig {
    pub general: GeneralConfig,
    pub model: ModelConfig,
    pub tools: ToolsConfig,
    pub memory: MemoryConfig,
    pub security: SecurityConfig,
    pub self_modify: SelfModifyCliConfig,
    pub http: HttpConfig,
    pub improvement: ImprovementToolsConfig,
    pub preprocess: PreprocessDedup,
}

/// Preprocessing deduplication settings.
///
/// Controls cross-turn conversation deduplication. Disabled by default —
/// requires explicit opt-in via `dedup_enabled = true`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PreprocessDedup {
    /// Enable cross-turn deduplication (default: false).
    pub dedup_enabled: bool,
    /// Minimum content length in characters to consider for dedup (default: 100).
    pub dedup_min_length: usize,
    /// Number of recent turns to always preserve intact (default: 2).
    pub dedup_preserve_recent: usize,
}

impl Default for PreprocessDedup {
    fn default() -> Self {
        Self {
            dedup_enabled: false,
            dedup_min_length: 100,
            dedup_preserve_recent: 2,
        }
    }
}

/// HTTP API settings for headless mode (`fawx serve --http`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HttpConfig {
    /// Bearer token for HTTP API authentication. Required when using `--http`.
    pub bearer_token: Option<String>,
}

/// Security settings for WASM skill signature verification.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SecurityConfig {
    /// When true, reject any WASM skill without a valid signature.
    /// When false (default), unsigned skills load with a warning.
    /// Invalid signatures are ALWAYS rejected regardless of this setting.
    pub require_signatures: bool,
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
    pub max_relevant_results: usize,
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
            max_relevant_results: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SelfModifyCliConfig {
    pub enabled: bool,
    pub branch_prefix: String,
    pub require_tests: bool,
    pub paths: SelfModifyPathsCliConfig,
    pub proposals_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SelfModifyPathsCliConfig {
    pub allow: Vec<String>,
    pub propose: Vec<String>,
    pub deny: Vec<String>,
}

/// Configuration for the self-improvement tool interfaces.
///
/// Controls whether Fawx can analyze its own runtime signals and propose
/// improvements. Disabled by default — requires explicit opt-in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ImprovementToolsConfig {
    /// Whether improvement tools appear in the tool definitions.
    pub enabled: bool,
    /// Maximum analysis calls per hour per session.
    pub max_analyses_per_hour: u32,
    /// Maximum improvement proposals per day.
    pub max_proposals_per_day: u32,
    /// Branch prefix for improvement proposals.
    pub auto_branch_prefix: String,
}

impl Default for ImprovementToolsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_analyses_per_hour: 10,
            max_proposals_per_day: 3,
            auto_branch_prefix: "fawx/improve".to_string(),
        }
    }
}

impl Default for SelfModifyCliConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            branch_prefix: "fawx/improve".to_string(),
            require_tests: true,
            paths: SelfModifyPathsCliConfig::default(),
            proposals_dir: None,
        }
    }
}

impl Default for SelfModifyPathsCliConfig {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            propose: Vec::new(),
            deny: DEFAULT_DENY_PATHS.iter().map(|s| s.to_string()).collect(),
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
        validate_glob_patterns(&self.self_modify)
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), String> {
        let config_path = data_dir.join("config.toml");
        fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
        if config_path.exists() {
            return Err("config.toml already exists; use targeted update helpers".to_string());
        }
        let content = toml::to_string_pretty(self)
            .map_err(|error| format!("failed to serialize config: {error}"))?;
        write_config_file(&config_path, content)
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

pub fn save_default_model(data_dir: &Path, default_model: &str) -> Result<(), String> {
    let config_path = data_dir.join("config.toml");
    fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
    if config_path.exists() {
        return update_default_model(&config_path, default_model);
    }
    create_model_config(data_dir, default_model)
}

fn create_model_config(data_dir: &Path, default_model: &str) -> Result<(), String> {
    let mut config = FawxConfig::default();
    config.model.default_model = Some(default_model.to_string());
    config.save(data_dir)
}

fn update_default_model(config_path: &Path, default_model: &str) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|error| format!("failed to read config: {error}"))?;
    let mut document = parse_config_document(&content)?;
    set_string_field(&mut document, &["model"], "default_model", default_model)?;
    write_config_file(config_path, document.to_string())
}

fn parse_config_document(content: &str) -> Result<DocumentMut, String> {
    content
        .parse::<DocumentMut>()
        .map_err(|error| format!("invalid config: {error}"))
}

fn set_string_field(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: &str,
) -> Result<(), String> {
    let table = get_or_insert_table(document, sections)?;
    if let Some(item) = table.get_mut(key) {
        return update_string_item(item, key, field_value);
    }
    table[key] = value(field_value);
    Ok(())
}

fn update_string_item(item: &mut Item, key: &str, field_value: &str) -> Result<(), String> {
    let decor = item
        .as_value()
        .ok_or_else(|| format!("config field '{key}' must be a value"))?
        .decor()
        .clone();
    *item = value(field_value);
    item.as_value_mut()
        .ok_or_else(|| format!("config field '{key}' must be a value"))?
        .decor_mut()
        .clone_from(&decor);
    Ok(())
}

fn get_or_insert_table<'a>(
    document: &'a mut DocumentMut,
    sections: &[&str],
) -> Result<&'a mut Table, String> {
    get_or_insert_table_in(document.as_table_mut(), sections)
}

fn get_or_insert_table_in<'a>(
    table: &'a mut Table,
    sections: &[&str],
) -> Result<&'a mut Table, String> {
    let Some((section, rest)) = sections.split_first() else {
        return Ok(table);
    };
    if !table.contains_key(section) {
        table[*section] = Item::Table(Table::new());
    }
    let child = table[*section]
        .as_table_mut()
        .ok_or_else(|| format!("config section '{section}' must be a table"))?;
    get_or_insert_table_in(child, rest)
}

fn write_config_file(config_path: &Path, content: String) -> Result<(), String> {
    fs::write(config_path, content).map_err(|error| format!("failed to write config: {error}"))
}

fn validate_glob_patterns(self_modify: &SelfModifyCliConfig) -> Result<(), String> {
    let all_fields = [
        ("paths.allow", &self_modify.paths.allow),
        ("paths.propose", &self_modify.paths.propose),
        ("paths.deny", &self_modify.paths.deny),
    ];
    for (field, patterns) in all_fields {
        for pattern in patterns {
            glob::Pattern::new(pattern).map_err(|error| {
                format!("invalid glob in self_modify.{field}: '{pattern}': {error}")
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_config(temp: &TempDir, content: &str) {
        fs::write(temp.path().join("config.toml"), content).expect("write config");
    }

    fn read_config(temp: &TempDir) -> String {
        fs::read_to_string(temp.path().join("config.toml")).expect("read config")
    }

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
max_relevant_results = 9
"#;
        write_config(&temp, content);
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        assert_eq!(loaded.general.max_iterations, 15);
        assert_eq!(loaded.general.max_history, 30);
        assert_eq!(loaded.model.default_model.as_deref(), Some("gpt-4.1"));
        assert_eq!(loaded.tools.max_read_size, 4096);
        assert_eq!(loaded.memory.max_snapshot_chars, 777);
        assert_eq!(loaded.memory.max_relevant_results, 9);
    }

    #[test]
    fn load_partial_config_uses_defaults() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[general]\nmax_iterations = 42\n";
        write_config(&temp, content);
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        assert_eq!(loaded.general.max_iterations, 42);
        assert_eq!(loaded.general.max_history, 20);
        assert_eq!(loaded.tools.max_read_size, 1024 * 1024);
        assert_eq!(loaded.memory.max_entries, 1000);
        assert_eq!(loaded.memory.max_relevant_results, 5);
    }

    #[test]
    fn load_invalid_toml_returns_error() {
        let temp = TempDir::new().expect("tempdir");
        write_config(&temp, "[general\nmax_iterations = 5");
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
    fn default_template_uses_nested_self_modify_paths_section() {
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("[self_modify.paths]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("allow = []"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("propose = []"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("deny = ["));
    }

    #[test]
    fn write_default_refuses_overwrite() {
        let temp = TempDir::new().expect("tempdir");
        write_config(&temp, "[general]\n");
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
        assert_eq!(defaults.memory.max_relevant_results, 5);
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
                max_relevant_results: 7,
            },
            self_modify: SelfModifyCliConfig {
                enabled: true,
                branch_prefix: "custom/prefix".to_string(),
                require_tests: false,
                paths: SelfModifyPathsCliConfig {
                    allow: vec!["src/**".to_string()],
                    propose: vec![],
                    deny: vec!["*.key".to_string()],
                },
                proposals_dir: Some(PathBuf::from("/tmp/proposals")),
            },
            security: SecurityConfig {
                require_signatures: true,
            },
            http: HttpConfig {
                bearer_token: Some("test-token".to_string()),
            },
            improvement: ImprovementToolsConfig {
                enabled: true,
                max_analyses_per_hour: 5,
                max_proposals_per_day: 2,
                auto_branch_prefix: "test/improve".to_string(),
            },
            preprocess: PreprocessDedup {
                dedup_enabled: true,
                dedup_min_length: 200,
                dedup_preserve_recent: 3,
            },
        };

        let encoded = toml::to_string(&original).expect("serialize");
        let decoded: FawxConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, original);
    }

    #[test]
    fn config_save_and_reload_preserves_model() {
        let temp = TempDir::new().expect("tempdir");
        let mut config = FawxConfig::default();
        config.model.default_model = Some("claude-sonnet-4-6-20250929".to_string());

        config.save(temp.path()).expect("save config");

        let loaded = FawxConfig::load(temp.path()).expect("reload config");
        assert_eq!(
            loaded.model.default_model.as_deref(),
            Some("claude-sonnet-4-6-20250929")
        );
    }

    #[test]
    fn config_save_refuses_existing_config() {
        let temp = TempDir::new().expect("tempdir");
        write_config(&temp, "[general]\nmax_iterations = 12\n");

        let error = FawxConfig::default()
            .save(temp.path())
            .expect_err("save should refuse overwrite");

        assert!(error.contains("targeted update helpers"));
    }

    #[test]
    fn save_default_model_preserves_comments() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"# keep header

[model]
# keep comment
default_model = "old-model"
"#;
        write_config(&temp, content);

        save_default_model(temp.path(), "new-model").expect("save model");

        let saved = read_config(&temp);
        assert!(saved.contains("# keep header"));
        assert!(saved.contains("# keep comment"));
        assert!(saved.contains("default_model = \"new-model\""));
    }

    #[test]
    fn save_default_model_preserves_inline_comment() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[model]\ndefault_model = \"old-model\" # keep me\n";
        write_config(&temp, content);

        save_default_model(temp.path(), "new-model").expect("save model");

        let saved = read_config(&temp);
        assert!(saved.contains("default_model = \"new-model\" # keep me"));
    }

    #[test]
    fn save_default_model_preserves_manual_http_section() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"[model]
default_model = "old-model"

[http]
bearer_token = "manual-token"
"#;
        write_config(&temp, content);

        save_default_model(temp.path(), "new-model").expect("save model");

        let saved = read_config(&temp);
        assert!(saved.contains("[http]"));
        assert!(saved.contains("bearer_token = \"manual-token\""));
    }

    #[test]
    fn save_default_model_creates_file_when_missing() {
        let temp = TempDir::new().expect("tempdir");

        save_default_model(temp.path(), "claude-opus-4-6").expect("save model");

        let saved = read_config(&temp);
        assert!(saved.contains("[model]"));
        assert!(saved.contains("default_model = \"claude-opus-4-6\""));
    }

    #[test]
    fn save_default_model_creates_model_section_when_missing() {
        let temp = TempDir::new().expect("tempdir");
        write_config(&temp, "[general]\nmax_iterations = 12\n");

        save_default_model(temp.path(), "claude-opus-4-6").expect("save model");

        let saved = read_config(&temp);
        assert!(saved.contains("[model]"));
        assert!(saved.contains("default_model = \"claude-opus-4-6\""));
    }

    #[test]
    fn save_default_model_creates_model_key_when_missing() {
        let temp = TempDir::new().expect("tempdir");
        write_config(&temp, "[model]\n# keep section comment\n");

        save_default_model(temp.path(), "claude-opus-4-6").expect("save model");

        let saved = read_config(&temp);
        assert!(saved.contains("# keep section comment"));
        assert!(saved.contains("default_model = \"claude-opus-4-6\""));
    }

    #[test]
    fn save_default_model_multiple_times_preserves_formatting() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"# keep this header

[model]
# keep model comment
default_model = "old-model"
"#;
        write_config(&temp, content);

        save_default_model(temp.path(), "mid-model").expect("save model");
        save_default_model(temp.path(), "final-model").expect("save model");

        let saved = read_config(&temp);
        assert!(saved.contains("# keep this header"));
        assert!(saved.contains("# keep model comment"));
        assert_eq!(saved.matches("[model]").count(), 1);
        assert!(saved.contains("default_model = \"final-model\""));
    }

    #[test]
    fn save_default_model_preserves_unrelated_known_sections() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"[tools]
max_read_size = 4096

[model]
default_model = "old-model"
"#;
        write_config(&temp, content);

        save_default_model(temp.path(), "new-model").expect("save model");

        let saved = read_config(&temp);
        assert!(saved.contains("max_read_size = 4096"));
        assert!(saved.contains("default_model = \"new-model\""));
    }

    #[test]
    fn load_rejects_zero_max_iterations() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[general]\nmax_iterations = 0\n";
        write_config(&temp, content);
        let error = FawxConfig::load(temp.path()).expect_err("should reject zero");
        assert!(error.contains("max_iterations must be >= 1"));
    }

    #[test]
    fn load_rejects_zero_max_history() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[general]\nmax_history = 0\n";
        write_config(&temp, content);
        let error = FawxConfig::load(temp.path()).expect_err("should reject zero");
        assert!(error.contains("max_history must be >= 1"));
    }

    #[test]
    fn load_rejects_tiny_max_read_size() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[tools]\nmax_read_size = 100\n";
        write_config(&temp, content);
        let error = FawxConfig::load(temp.path()).expect_err("should reject small value");
        assert!(error.contains("max_read_size must be >= 1024"));
    }

    #[test]
    fn load_rejects_zero_max_entries() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[memory]\nmax_entries = 0\n";
        write_config(&temp, content);
        let error = FawxConfig::load(temp.path()).expect_err("should reject zero");
        assert!(error.contains("max_entries must be >= 1"));
    }

    #[test]
    fn load_rejects_oversized_synthesis_instruction() {
        let temp = TempDir::new().expect("tempdir");
        let long_value = "x".repeat(501);
        let content = format!("[model]\nsynthesis_instruction = \"{}\"\n", long_value);
        write_config(&temp, &content);
        let error = FawxConfig::load(temp.path()).expect_err("should reject long instruction");
        assert!(error.contains("synthesis_instruction exceeds 500 characters"));
    }

    #[test]
    fn load_accepts_max_length_synthesis_instruction() {
        let temp = TempDir::new().expect("tempdir");
        let value = "x".repeat(500);
        let content = format!("[model]\nsynthesis_instruction = \"{}\"\n", value);
        write_config(&temp, &content);
        let config = FawxConfig::load(temp.path()).expect("should accept 500 chars");
        assert_eq!(config.model.synthesis_instruction.unwrap().len(), 500);
    }

    #[test]
    fn load_config_with_self_modify_section() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"
[self_modify]
enabled = true
branch_prefix = "custom/prefix"
require_tests = false

[self_modify.paths]
allow = ["src/**"]
propose = ["kernel/**"]
deny = [".git/**", "*.key"]
"#;
        write_config(&temp, content);
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        assert!(loaded.self_modify.enabled);
        assert_eq!(loaded.self_modify.branch_prefix, "custom/prefix");
        assert!(!loaded.self_modify.require_tests);
        assert_eq!(loaded.self_modify.paths.allow, vec!["src/**"]);
        assert_eq!(loaded.self_modify.paths.propose, vec!["kernel/**"]);
        assert_eq!(loaded.self_modify.paths.deny, vec![".git/**", "*.key"]);
    }

    #[test]
    fn load_rejects_invalid_glob_pattern() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"
[self_modify.paths]
deny = ["[invalid"]
"#;
        write_config(&temp, content);
        let error = FawxConfig::load(temp.path()).expect_err("should reject invalid glob");
        assert!(
            error.contains("invalid glob"),
            "error should mention invalid glob, got: {error}"
        );
    }

    #[test]
    fn security_config_defaults_and_roundtrip() {
        let defaults = SecurityConfig::default();
        assert!(!defaults.require_signatures);

        let config = SecurityConfig {
            require_signatures: true,
        };
        let encoded = toml::to_string(&config).expect("serialize");
        let decoded: SecurityConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, config);
    }

    #[test]
    fn config_template_includes_security_section() {
        assert!(
            DEFAULT_CONFIG_TEMPLATE.contains("[security]"),
            "template should contain [security] section"
        );
        assert!(
            DEFAULT_CONFIG_TEMPLATE.contains("require_signatures"),
            "template should mention require_signatures"
        );
    }
}
