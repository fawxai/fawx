//! Default config values and templates for `fx-config`.

use crate::{
    AgentBehaviorConfig, AgentConfig, BudgetConfig, FleetConfig, GeneralConfig,
    ImprovementToolsConfig, MemoryConfig, OrchestratorConfig, PermissionsConfig, PreprocessDedup,
    ProposalConfig, SandboxConfig, SelfModifyCliConfig, SelfModifyPathsCliConfig, ToolsConfig,
};

/// Canonical default deny patterns for self-modification path enforcement.
pub const DEFAULT_DENY_PATHS: &[&str] = &[".git/**", "*.key", "*.pem", "credentials.*"];

pub const DEFAULT_CONFIG_TEMPLATE: &str = r#"# Fawx Configuration
# Location: ~/.fawx/config.toml

[general]
# data_dir = "~/.fawx"
# max_iterations = 10
# max_history = 20
# thinking = "adaptive"  # "high" | "low" | "adaptive" | "off"

[agent]
# name = "Fawx"
# personality = "casual"  # "casual" | "professional" | "technical" | "minimal" | "custom"
# custom_personality = ""
# [agent.behavior]
# custom_instructions = "Be concise and direct."
# verbosity = "normal"  # "terse" | "normal" | "thorough"
# proactive = false

[model]
# default_model = "anthropic/claude-sonnet-4-20250514"
# synthesis_instruction = "Be concise and direct."

[logging]
# file_logging = true
# file_level = "info"
# stderr_level = "warn"
# max_files = 7
# log_dir = "~/.fawx/logs"

[tools]
# working_dir = "/home/user/projects"
# search_exclude = ["vendor", "dist"]
# max_read_size = 1048576

[git]
# protected_branches = ["main", "staging"]

[memory]
# max_entries = 1000
# max_value_size = 10240
# max_snapshot_chars = 2000
# max_relevant_results = 5
# embeddings_enabled = true

[workspace]
# Workspace root. Defaults to the current directory.
# root = "."

[permissions]
# Default preset for new configs. Use "custom" to manage lists manually.
# preset = "power"
# unrestricted = ["read_any", "web_search", "web_fetch", "code_execute", "file_write", "git", "shell", "tool_call", "self_modify"]
# proposal_required = ["credential_change", "system_install", "network_listen", "outbound_message", "file_delete", "outside_workspace", "kernel_modify"]

[budget]
# Default cost guardrails in cents. Set to 0 for unlimited.
# max_session_cost_cents = 500
# max_daily_cost_cents = 2000
# alert_threshold_cents = 200

[sandbox]
# Default sandbox preset for shell and skill execution.
# allow_network = true
# allow_subprocess = true
# max_execution_seconds = 300

[proposals]
# Proposal defaults; leave auto_approve_timeout_minutes unset to keep approval manual.
# notification_channels = ["tui"]
# expiry_hours = 24

# [security]
# require_signatures = false
# github_borrow_scope = "read_only"  # "read_only" | "contribution"

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

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "Fawx".to_string(),
            personality: "casual".to_string(),
            custom_personality: None,
            behavior: AgentBehaviorConfig::default(),
        }
    }
}

impl Default for AgentBehaviorConfig {
    fn default() -> Self {
        Self {
            custom_instructions: None,
            verbosity: "normal".to_string(),
            proactive: false,
        }
    }
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self::standard()
    }
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_session_cost_cents: 500,
            max_daily_cost_cents: 2_000,
            alert_threshold_cents: 200,
        }
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            allow_network: true,
            allow_subprocess: true,
            max_execution_seconds: Some(300),
        }
    }
}

impl Default for ProposalConfig {
    fn default() -> Self {
        Self {
            auto_approve_timeout_minutes: None,
            notification_channels: vec!["tui".to_string()],
            expiry_hours: Some(24),
        }
    }
}

impl Default for FleetConfig {
    fn default() -> Self {
        Self {
            coordinator: false,
            stale_timeout_seconds: 60,
            nodes: Vec::new(),
        }
    }
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_pending_tasks: 100,
            default_timeout_ms: 30_000,
            default_max_retries: 1,
        }
    }
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

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            data_dir: None,
            max_iterations: 10,
            max_history: 20,
            thinking: None,
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
            embeddings_enabled: true,
        }
    }
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
