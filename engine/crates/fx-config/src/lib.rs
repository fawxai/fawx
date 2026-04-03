pub mod manager;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

mod defaults;
mod display;
mod env;
mod presets;
mod toml_io;
mod types;
mod validation;

pub use defaults::{DEFAULT_CONFIG_TEMPLATE, DEFAULT_DENY_PATHS};
pub use toml_io::{save_default_model, save_thinking_budget};
pub use types::{
    AgentBehaviorConfig, AgentConfig, BorrowScope, BudgetConfig, CapabilityMode, FawxConfig,
    FleetConfig, GeneralConfig, GitConfig, HttpConfig, ImprovementToolsConfig, LoggingConfig,
    MemoryConfig, ModelConfig, NodeConfig, OrchestratorConfig, PermissionAction, PermissionPreset,
    PermissionsConfig, PreprocessDedup, ProposalConfig, SandboxConfig, SecurityConfig,
    SelfModifyCliConfig, SelfModifyPathsCliConfig, TelegramChannelConfig, ThinkingBudget,
    ToolsConfig, WebhookChannelConfig, WebhookConfig, WorkspaceConfig,
};
pub use validation::{
    parse_log_level, validate_synthesis_instruction, MAX_SYNTHESIS_INSTRUCTION_LENGTH,
};

pub(crate) use toml_io::{parse_config_document, set_typed_field, write_config_file};
pub(crate) use validation::VALID_LOG_LEVELS;

#[cfg(test)]
use env::expand_tilde;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use tracing_subscriber::filter::LevelFilter;

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
embeddings_enabled = false
"#;
        write_config(&temp, content);
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        assert_eq!(loaded.general.max_iterations, 15);
        assert_eq!(loaded.general.max_history, 30);
        assert_eq!(loaded.model.default_model.as_deref(), Some("gpt-4.1"));
        assert_eq!(loaded.tools.max_read_size, 4096);
        assert_eq!(loaded.memory.max_snapshot_chars, 777);
        assert_eq!(loaded.memory.max_relevant_results, 9);
        assert!(!loaded.memory.embeddings_enabled);
    }

    #[test]
    fn load_parses_agent_config() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"
[agent]
name = "Rivet"
personality = "custom"
custom_personality = "Be sharply technical."

[agent.behavior]
custom_instructions = "Lead with the answer."
verbosity = "terse"
proactive = true
"#;
        write_config(&temp, content);
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        assert_eq!(loaded.agent.name, "Rivet");
        assert_eq!(loaded.agent.personality, "custom");
        assert_eq!(
            loaded.agent.custom_personality.as_deref(),
            Some("Be sharply technical.")
        );
        assert_eq!(
            loaded.agent.behavior.custom_instructions.as_deref(),
            Some("Lead with the answer.")
        );
        assert_eq!(loaded.agent.behavior.verbosity, "terse");
        assert!(loaded.agent.behavior.proactive);
    }

    #[test]
    fn load_parses_logging_config() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"
[logging]
file_logging = true
file_level = "trace"
stderr_level = "error"
max_files = 14
log_dir = "~/.fawx/custom-logs"
"#;
        write_config(&temp, content);
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        let home = dirs::home_dir().expect("home dir should exist in test");
        assert_eq!(loaded.logging.file_logging, Some(true));
        assert_eq!(loaded.logging.file_level.as_deref(), Some("trace"));
        assert_eq!(loaded.logging.stderr_level.as_deref(), Some("error"));
        assert_eq!(loaded.logging.max_files, Some(14));
        assert_eq!(
            loaded.logging.log_dir.as_deref(),
            Some(home.join(".fawx/custom-logs").to_string_lossy().as_ref())
        );
    }

    #[test]
    fn load_partial_config_uses_defaults() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[general]\nmax_iterations = 42\n";
        write_config(&temp, content);
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        assert_eq!(loaded.general.max_iterations, 42);
        assert_eq!(loaded.general.max_history, 20);
        assert_eq!(loaded.agent, AgentConfig::default());
        assert_eq!(loaded.logging, LoggingConfig::default());
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
    fn default_template_includes_power_user_sections() {
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("[agent]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("# name = \"Fawx\""));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("# [agent.behavior]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("# verbosity = \"normal\""));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("[workspace]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("# root = \".\""));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("[git]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("# protected_branches = [\"main\", \"staging\"]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("[permissions]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("# preset = \"power\""));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("[budget]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("# max_session_cost_cents = 500"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("[sandbox]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("# allow_network = true"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("[proposals]"));
        assert!(DEFAULT_CONFIG_TEMPLATE.contains("# notification_channels = [\"tui\"]"));
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
        assert_eq!(defaults.agent.name, "Fawx");
        assert_eq!(defaults.agent.personality, "casual");
        assert_eq!(defaults.agent.behavior.verbosity, "normal");
        assert_eq!(defaults.logging, LoggingConfig::default());
        assert_eq!(defaults.tools.max_read_size, 1024 * 1024);
        assert!(defaults.git.protected_branches.is_empty());
        assert_eq!(defaults.memory.max_entries, 1000);
        assert_eq!(defaults.memory.max_value_size, 10240);
        assert_eq!(defaults.memory.max_snapshot_chars, 2000);
        assert_eq!(defaults.memory.max_relevant_results, 5);
        assert!(defaults.memory.embeddings_enabled);
    }

    #[test]
    fn config_parses_git_protected_branches() {
        let config: FawxConfig = toml::from_str(
            r#"[git]
protected_branches = ["main", "staging"]
"#,
        )
        .expect("deserialize git config");

        assert_eq!(config.git.protected_branches, vec!["main", "staging"]);
    }

    #[test]
    fn config_parses_empty_git_protected_branches() {
        let config: FawxConfig = toml::from_str(
            r#"[git]
protected_branches = []
"#,
        )
        .expect("deserialize empty git config");

        assert!(config.git.protected_branches.is_empty());
    }

    #[test]
    fn git_config_serde_round_trip() {
        let original = GitConfig {
            protected_branches: vec!["main".to_string(), "staging".to_string()],
        };

        let encoded = toml::to_string(&original).expect("serialize git config");
        let decoded: GitConfig = toml::from_str(&encoded).expect("deserialize git config");

        assert_eq!(decoded, original);
    }

    #[test]
    fn config_fields_roundtrip() {
        let original = FawxConfig {
            general: GeneralConfig {
                data_dir: Some(PathBuf::from("/tmp/data")),
                max_iterations: 9,
                max_history: 99,
                thinking: None,
            },
            agent: AgentConfig {
                name: "Rivet".to_string(),
                personality: "technical".to_string(),
                custom_personality: Some("Explain tradeoffs plainly.".to_string()),
                behavior: AgentBehaviorConfig {
                    custom_instructions: Some("Prefer concrete next steps.".to_string()),
                    verbosity: "thorough".to_string(),
                    proactive: true,
                },
            },
            model: ModelConfig {
                default_model: Some("claude-sonnet".to_string()),
                synthesis_instruction: Some("short answers".to_string()),
            },
            logging: LoggingConfig {
                file_logging: Some(true),
                file_level: Some("debug".to_string()),
                stderr_level: Some("error".to_string()),
                max_files: Some(14),
                log_dir: Some("~/.fawx/custom-logs".to_string()),
            },
            tools: ToolsConfig {
                working_dir: Some(PathBuf::from("/tmp/work")),
                search_exclude: vec!["vendor".to_string()],
                max_read_size: 2048,
            },
            git: GitConfig {
                protected_branches: vec!["main".to_string(), "staging".to_string()],
            },
            memory: MemoryConfig {
                max_entries: 4,
                max_value_size: 5,
                max_snapshot_chars: 6,
                max_relevant_results: 7,
                embeddings_enabled: false,
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
                github_borrow_scope: BorrowScope::Contribution,
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
            fleet: FleetConfig {
                coordinator: true,
                stale_timeout_seconds: 120,
                nodes: vec![NodeConfig {
                    id: "test-node".to_string(),
                    name: "test-node".to_string(),
                    endpoint: Some("https://10.0.0.1:8400".to_string()),
                    auth_token: Some("token123".to_string()),
                    capabilities: vec!["agentic_loop".to_string()],
                    address: Some("10.0.0.1".to_string()),
                    user: Some("deploy".to_string()),
                    ssh_key: Some("~/.ssh/id_ed25519".to_string()),
                }],
            },
            webhook: WebhookConfig {
                enabled: true,
                channels: vec![WebhookChannelConfig {
                    id: "wh-test".to_string(),
                    name: "Test Webhook".to_string(),
                    callback_url: Some("https://example.com/cb".to_string()),
                }],
            },
            orchestrator: OrchestratorConfig {
                enabled: true,
                max_pending_tasks: 50,
                default_timeout_ms: 15_000,
                default_max_retries: 3,
            },
            telegram: TelegramChannelConfig {
                enabled: true,
                bot_token: Some("123456:ABC-DEF".to_string()),
                allowed_chat_ids: vec![100, 200],
                webhook_secret: Some("test-webhook-secret".to_string()),
            },
            workspace: WorkspaceConfig {
                root: Some(PathBuf::from("/tmp/workspace")),
            },
            permissions: PermissionsConfig {
                preset: PermissionPreset::Custom,
                mode: CapabilityMode::Prompt,
                unrestricted: vec![PermissionAction::ReadAny, PermissionAction::ToolCall],
                proposal_required: vec![PermissionAction::FileDelete],
            },
            budget: BudgetConfig {
                max_session_cost_cents: 750,
                max_daily_cost_cents: 4_200,
                alert_threshold_cents: 350,
            },
            sandbox: SandboxConfig {
                allow_network: false,
                allow_subprocess: false,
                max_execution_seconds: Some(45),
            },
            proposals: ProposalConfig {
                auto_approve_timeout_minutes: Some(5),
                notification_channels: vec!["tui".to_string(), "telegram".to_string()],
                expiry_hours: Some(48),
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
    fn load_rejects_invalid_logging_level() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[logging]\nfile_level = \"verbose\"\n";
        write_config(&temp, content);
        let error = FawxConfig::load(temp.path()).expect_err("should reject invalid level");
        assert!(error.contains("logging.file_level must be one of"));
    }

    #[test]
    fn parse_log_level_accepts_supported_values_case_insensitively() {
        assert_eq!(parse_log_level("error"), Some(LevelFilter::ERROR));
        assert_eq!(parse_log_level(" Warn "), Some(LevelFilter::WARN));
        assert_eq!(parse_log_level("INFO"), Some(LevelFilter::INFO));
        assert_eq!(parse_log_level("Debug"), Some(LevelFilter::DEBUG));
        assert_eq!(parse_log_level("trace"), Some(LevelFilter::TRACE));
    }

    #[test]
    fn parse_log_level_rejects_unknown_values() {
        assert_eq!(parse_log_level("verbose"), None);
    }

    #[test]
    fn load_rejects_zero_max_log_files() {
        let temp = TempDir::new().expect("tempdir");
        let content = "[logging]\nmax_files = 0\n";
        write_config(&temp, content);
        let error = FawxConfig::load(temp.path()).expect_err("should reject zero");
        assert!(error.contains("logging.max_files must be >= 1"));
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
    fn load_rejects_empty_synthesis_instruction() {
        let temp = TempDir::new().expect("tempdir");
        write_config(&temp, "[model]\nsynthesis_instruction = \"\"\n");
        let error = FawxConfig::load(temp.path()).expect_err("should reject empty instruction");
        assert!(error.contains("synthesis_instruction must not be empty"));
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
        assert_eq!(defaults.github_borrow_scope, BorrowScope::ReadOnly);

        let config = SecurityConfig {
            require_signatures: true,
            github_borrow_scope: BorrowScope::Contribution,
        };
        let encoded = toml::to_string(&config).expect("serialize");
        let decoded: SecurityConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, config);
    }

    #[test]
    fn security_config_default_borrow_scope_is_read_only() {
        let config = SecurityConfig::default();
        assert_eq!(config.github_borrow_scope, BorrowScope::ReadOnly);
    }

    #[test]
    fn security_config_deserializes_contribution_scope() {
        let config: SecurityConfig = toml::from_str("github_borrow_scope = \"contribution\"")
            .expect("deserialize security config");
        assert_eq!(config.github_borrow_scope, BorrowScope::Contribution);
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
        assert!(
            DEFAULT_CONFIG_TEMPLATE.contains("github_borrow_scope"),
            "template should mention github_borrow_scope"
        );
    }

    #[test]
    fn config_template_includes_logging_section() {
        assert!(
            DEFAULT_CONFIG_TEMPLATE.contains("[logging]"),
            "template should contain [logging] section"
        );
        assert!(
            DEFAULT_CONFIG_TEMPLATE.contains("file_level"),
            "template should mention file_level"
        );
    }

    #[test]
    fn thinking_budget_serialization() {
        let config = GeneralConfig {
            thinking: Some(ThinkingBudget::High),
            ..GeneralConfig::default()
        };
        let encoded = toml::to_string(&config).expect("serialize");
        assert!(encoded.contains(r#"thinking = "high""#));
        let decoded: GeneralConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded.thinking, Some(ThinkingBudget::High));

        // Round-trip all variants
        for variant in [
            ThinkingBudget::Adaptive,
            ThinkingBudget::Low,
            ThinkingBudget::Off,
        ] {
            let cfg = GeneralConfig {
                thinking: Some(variant),
                ..GeneralConfig::default()
            };
            let enc = toml::to_string(&cfg).expect("serialize");
            let dec: GeneralConfig = toml::from_str(&enc).expect("deserialize");
            assert_eq!(dec.thinking, Some(variant));
        }
    }

    #[test]
    fn thinking_budget_default_is_adaptive() {
        let config = GeneralConfig::default();
        assert_eq!(config.thinking, None);
        // None should be treated as Adaptive
        let effective = config.thinking.unwrap_or_default();
        assert_eq!(effective, ThinkingBudget::Adaptive);
    }

    #[test]
    fn thinking_command_persists() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"[general]
# keep comment
max_iterations = 10
"#;
        write_config(&temp, content);

        save_thinking_budget(temp.path(), ThinkingBudget::High).expect("save thinking");

        let saved = read_config(&temp);
        assert!(saved.contains("# keep comment"));
        assert!(saved.contains(r#"thinking = "high""#));

        // Update again
        save_thinking_budget(temp.path(), ThinkingBudget::Low).expect("save thinking");
        let saved = read_config(&temp);
        assert!(saved.contains(r#"thinking = "low""#));
        assert!(!saved.contains(r#"thinking = "high""#));
    }

    #[test]
    fn thinking_budget_tokens_maps_correctly() {
        assert_eq!(ThinkingBudget::High.budget_tokens(), Some(10_000));
        assert_eq!(ThinkingBudget::Adaptive.budget_tokens(), Some(5_000));
        assert_eq!(ThinkingBudget::Low.budget_tokens(), Some(1_024));
        assert_eq!(ThinkingBudget::Off.budget_tokens(), None);
    }

    #[test]
    fn tilde_expansion_resolves_home() {
        let path = PathBuf::from("~/.fawx");
        let expanded = expand_tilde(&path);
        let home = dirs::home_dir().expect("home dir should exist in test");
        assert_eq!(expanded, home.join(".fawx"));
    }

    #[test]
    fn tilde_expansion_preserves_absolute() {
        let path = PathBuf::from("/absolute/path");
        let expanded = expand_tilde(&path);
        assert_eq!(expanded, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn tilde_expansion_preserves_relative() {
        let path = PathBuf::from("relative/path");
        let expanded = expand_tilde(&path);
        assert_eq!(expanded, PathBuf::from("relative/path"));
    }

    #[test]
    fn tilde_expansion_preserves_tilde_in_middle() {
        let path = PathBuf::from("foo/~/bar");
        let expanded = expand_tilde(&path);
        assert_eq!(expanded, PathBuf::from("foo/~/bar"));
    }

    #[test]
    fn tilde_expansion_does_not_expand_tilde_user() {
        let path = PathBuf::from("~joe/.config");
        let expanded = expand_tilde(&path);
        assert_eq!(expanded, PathBuf::from("~joe/.config"));
    }

    #[test]
    fn tilde_expansion_bare_tilde_resolves_to_home() {
        let path = PathBuf::from("~");
        let expanded = expand_tilde(&path);
        let home = dirs::home_dir().expect("home dir should exist in test");
        assert_eq!(expanded, home);
    }

    #[test]
    fn load_expands_tilde_in_config_paths() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"
[general]
data_dir = "~/.fawx"

[logging]
log_dir = "~/.fawx/logs"

[tools]
working_dir = "~/projects"

[self_modify]
proposals_dir = "~/.fawx/proposals"
"#;
        write_config(&temp, content);
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        let home = dirs::home_dir().expect("home dir should exist in test");
        assert_eq!(loaded.general.data_dir, Some(home.join(".fawx")),);
        assert_eq!(
            loaded.logging.log_dir.as_deref(),
            Some(home.join(".fawx/logs").to_string_lossy().as_ref())
        );
        assert_eq!(loaded.tools.working_dir, Some(home.join("projects")),);
        assert_eq!(
            loaded.self_modify.proposals_dir,
            Some(home.join(".fawx/proposals")),
        );
    }

    #[test]
    fn load_preserves_absolute_config_paths() {
        let temp = TempDir::new().expect("tempdir");
        let content = r#"
[general]
data_dir = "/tmp/fawx-data"

[tools]
working_dir = "/tmp/work"
"#;
        write_config(&temp, content);
        let loaded = FawxConfig::load(temp.path()).expect("load config");

        assert_eq!(
            loaded.general.data_dir,
            Some(PathBuf::from("/tmp/fawx-data")),
        );
        assert_eq!(loaded.tools.working_dir, Some(PathBuf::from("/tmp/work")),);
    }

    #[test]
    fn power_preset_has_correct_unrestricted() {
        let config = PermissionsConfig::power();
        assert_eq!(config.unrestricted.len(), 9);
        assert_eq!(
            config.unrestricted,
            vec![
                PermissionAction::ReadAny,
                PermissionAction::WebSearch,
                PermissionAction::WebFetch,
                PermissionAction::CodeExecute,
                PermissionAction::FileWrite,
                PermissionAction::Git,
                PermissionAction::Shell,
                PermissionAction::ToolCall,
                PermissionAction::SelfModify,
            ]
        );
    }

    #[test]
    fn power_preset_has_correct_proposals() {
        let config = PermissionsConfig::power();
        assert_eq!(config.proposal_required.len(), 7);
        assert_eq!(
            config.proposal_required,
            vec![
                PermissionAction::CredentialChange,
                PermissionAction::SystemInstall,
                PermissionAction::NetworkListen,
                PermissionAction::OutboundMessage,
                PermissionAction::FileDelete,
                PermissionAction::OutsideWorkspace,
                PermissionAction::KernelModify,
            ]
        );
    }

    #[test]
    fn cautious_preset_restricts_writes() {
        let config = PermissionsConfig::cautious();
        assert!(!config.unrestricted.contains(&PermissionAction::FileWrite));
        assert!(config
            .proposal_required
            .contains(&PermissionAction::FileWrite));
    }

    #[test]
    fn experimental_preset_allows_kernel_modify() {
        let config = PermissionsConfig::experimental();
        assert!(config
            .unrestricted
            .contains(&PermissionAction::KernelModify));
        assert!(!config
            .proposal_required
            .contains(&PermissionAction::KernelModify));
    }

    #[test]
    fn permissions_config_serde_round_trip() {
        let config = PermissionsConfig {
            preset: PermissionPreset::Custom,
            mode: CapabilityMode::Prompt,
            unrestricted: vec![PermissionAction::ReadAny, PermissionAction::ToolCall],
            proposal_required: vec![PermissionAction::FileDelete, PermissionAction::KernelModify],
        };
        let encoded = toml::to_string(&config).expect("serialize");
        let decoded: PermissionsConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, config);
    }

    #[test]
    fn permission_preset_as_str_matches_serde_name() {
        let presets = [
            PermissionPreset::Power,
            PermissionPreset::Cautious,
            PermissionPreset::Experimental,
            PermissionPreset::Custom,
        ];

        for preset in presets {
            let encoded = serde_json::to_string(&preset).expect("serialize preset");
            assert_eq!(encoded, format!("\"{}\"", preset.as_str()));
        }
    }

    #[test]
    fn permission_action_as_str_matches_serde_name() {
        let actions = [
            PermissionAction::ReadAny,
            PermissionAction::WebSearch,
            PermissionAction::WebFetch,
            PermissionAction::CodeExecute,
            PermissionAction::FileWrite,
            PermissionAction::Git,
            PermissionAction::Shell,
            PermissionAction::ToolCall,
            PermissionAction::SelfModify,
            PermissionAction::CredentialChange,
            PermissionAction::SystemInstall,
            PermissionAction::NetworkListen,
            PermissionAction::OutboundMessage,
            PermissionAction::FileDelete,
            PermissionAction::OutsideWorkspace,
            PermissionAction::KernelModify,
        ];

        for action in actions {
            let encoded = serde_json::to_string(&action).expect("serialize action");
            assert_eq!(encoded, format!("\"{}\"", action.as_str()));
        }
    }

    #[test]
    fn budget_config_defaults() {
        let config = BudgetConfig::default();
        assert_eq!(config.max_session_cost_cents, 500);
        assert_eq!(config.max_daily_cost_cents, 2_000);
        assert_eq!(config.alert_threshold_cents, 200);
    }

    #[test]
    fn budget_config_serde_round_trip() {
        let config = BudgetConfig {
            max_session_cost_cents: 750,
            max_daily_cost_cents: 4_200,
            alert_threshold_cents: 350,
        };
        let encoded = toml::to_string(&config).expect("serialize");
        let decoded: BudgetConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, config);
    }

    #[test]
    fn sandbox_config_defaults() {
        let config = SandboxConfig::default();
        assert!(config.allow_network);
        assert!(config.allow_subprocess);
        assert_eq!(config.max_execution_seconds, Some(300));
    }

    #[test]
    fn sandbox_config_serde_round_trip() {
        let config = SandboxConfig {
            allow_network: false,
            allow_subprocess: true,
            max_execution_seconds: Some(120),
        };
        let encoded = toml::to_string(&config).expect("serialize");
        let decoded: SandboxConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, config);
    }

    #[test]
    fn proposal_config_serde_round_trip() {
        let config = ProposalConfig {
            auto_approve_timeout_minutes: Some(15),
            notification_channels: vec!["tui".to_string(), "telegram".to_string()],
            expiry_hours: Some(72),
        };
        let encoded = toml::to_string(&config).expect("serialize");
        let decoded: ProposalConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, config);
    }

    #[test]
    fn fawx_config_with_new_sections_round_trips() {
        let config = FawxConfig {
            workspace: WorkspaceConfig {
                root: Some(PathBuf::from("/tmp/workspace")),
            },
            permissions: PermissionsConfig::experimental(),
            budget: BudgetConfig {
                max_session_cost_cents: 800,
                max_daily_cost_cents: 3_000,
                alert_threshold_cents: 400,
            },
            sandbox: SandboxConfig {
                allow_network: false,
                allow_subprocess: true,
                max_execution_seconds: Some(120),
            },
            proposals: ProposalConfig {
                auto_approve_timeout_minutes: Some(15),
                notification_channels: vec!["tui".to_string(), "telegram".to_string()],
                expiry_hours: Some(72),
            },
            ..FawxConfig::default()
        };
        let encoded = toml::to_string(&config).expect("serialize");
        let decoded: FawxConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, config);
    }

    #[test]
    fn preset_from_name() {
        assert_eq!(
            PermissionsConfig::from_preset_name("power").expect("power preset"),
            PermissionsConfig::power()
        );
        assert_eq!(
            PermissionsConfig::from_preset_name("cautious").expect("cautious preset"),
            PermissionsConfig::cautious()
        );
        assert_eq!(
            PermissionsConfig::from_preset_name("experimental").expect("experimental preset"),
            PermissionsConfig::experimental()
        );
    }

    #[test]
    fn preset_from_name_supports_custom() {
        assert_eq!(
            PermissionsConfig::from_preset_name("custom").expect("custom preset"),
            PermissionsConfig {
                preset: PermissionPreset::Custom,
                mode: CapabilityMode::Capability,
                ..PermissionsConfig::default()
            }
        );
    }

    #[test]
    fn permissions_default_is_standard_capability() {
        let config = PermissionsConfig::default();

        assert_eq!(config, PermissionsConfig::standard());
        assert_eq!(config.mode, CapabilityMode::Capability);
    }

    #[test]
    fn preset_from_name_accepts_aliases() {
        assert_eq!(
            PermissionsConfig::from_preset_name("standard").expect("standard preset"),
            PermissionsConfig::power()
        );
        assert_eq!(
            PermissionsConfig::from_preset_name("restricted").expect("restricted preset"),
            PermissionsConfig::cautious()
        );
        assert_eq!(
            PermissionsConfig::from_preset_name("open").expect("open preset"),
            PermissionsConfig::experimental()
        );
    }

    #[test]
    fn preset_from_name_rejects_unknown_value() {
        let error = PermissionsConfig::from_preset_name("nope").expect_err("should fail fast");
        assert_eq!(
            error,
            "unknown permission preset 'nope'; expected power, cautious, experimental, custom, standard, restricted, open"
        );
    }

    #[test]
    fn old_configs_deserialize_with_new_sections_defaulted() {
        let config: FawxConfig =
            toml::from_str("[general]\nmax_iterations = 12\n").expect("deserialize old config");
        assert_eq!(config.workspace, WorkspaceConfig::default());
        assert_eq!(config.git, GitConfig::default());
        assert_eq!(config.budget, BudgetConfig::default());
        assert_eq!(config.sandbox, SandboxConfig::default());
        assert_eq!(config.proposals, ProposalConfig::default());
    }

    #[test]
    fn permissions_without_mode_defaults_to_capability() {
        let config: PermissionsConfig = toml::from_str(
            r#"
preset = "power"
unrestricted = ["read_any"]
proposal_required = ["shell"]
"#,
        )
        .expect("deserialize permissions without mode");
        assert_eq!(config.mode, CapabilityMode::Capability);
    }
}
