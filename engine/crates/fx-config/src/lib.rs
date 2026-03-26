pub mod manager;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use serde::{Deserialize, Serialize};
use std::str::FromStr;
use toml_edit::{value, DocumentMut, Item, Table};
use tracing_subscriber::filter::LevelFilter;

pub const MAX_SYNTHESIS_INSTRUCTION_LENGTH: usize = 500;
const MIN_MAX_READ_SIZE: u64 = 1024;
pub(crate) const VALID_LOG_LEVELS: &str = "error, warn, info, debug, trace";

pub fn validate_synthesis_instruction(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("synthesis_instruction must not be empty".to_string());
    }
    if trimmed.len() > MAX_SYNTHESIS_INSTRUCTION_LENGTH {
        return Err(format!(
            "synthesis_instruction exceeds {MAX_SYNTHESIS_INSTRUCTION_LENGTH} characters"
        ));
    }
    Ok(())
}
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FawxConfig {
    pub general: GeneralConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    pub model: ModelConfig,
    pub logging: LoggingConfig,
    pub tools: ToolsConfig,
    #[serde(default)]
    pub git: GitConfig,
    pub memory: MemoryConfig,
    pub security: SecurityConfig,
    pub self_modify: SelfModifyCliConfig,
    pub http: HttpConfig,
    pub improvement: ImprovementToolsConfig,
    pub preprocess: PreprocessDedup,
    pub fleet: FleetConfig,
    pub webhook: WebhookConfig,
    pub orchestrator: OrchestratorConfig,
    pub telegram: TelegramChannelConfig,
    pub workspace: WorkspaceConfig,
    pub permissions: PermissionsConfig,
    pub budget: BudgetConfig,
    pub sandbox: SandboxConfig,
    pub proposals: ProposalConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AgentConfig {
    pub name: String,
    pub personality: String,
    pub custom_personality: Option<String>,
    pub behavior: AgentBehaviorConfig,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AgentBehaviorConfig {
    pub custom_instructions: Option<String>,
    pub verbosity: String,
    pub proactive: bool,
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

/// Workspace configuration for filesystem boundaries and defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// Root directory for workspace operations. Resolved to cwd at startup if None.
    pub root: Option<PathBuf>,
}

/// Git policy configuration for protected branch enforcement.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GitConfig {
    #[serde(default)]
    pub protected_branches: Vec<String>,
}

/// Permission presets that define default agent autonomy levels.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityMode {
    /// Default: denied actions are silently blocked with structured error.
    #[default]
    Capability,
    /// Opt-in: denied actions trigger interactive prompts (legacy behavior).
    Prompt,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionPreset {
    Power,
    Cautious,
    Experimental,
    Custom,
}

impl PermissionPreset {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Power => "power",
            Self::Cautious => "cautious",
            Self::Experimental => "experimental",
            Self::Custom => "custom",
        }
    }
}

impl FromStr for PermissionPreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "power" | "standard" => Ok(Self::Power),
            "cautious" | "restricted" => Ok(Self::Cautious),
            "experimental" | "open" => Ok(Self::Experimental),
            "custom" => Ok(Self::Custom),
            other => Err(format!(
                "unknown permission preset '{other}'; expected power, cautious, experimental, custom, standard, restricted, open"
            )),
        }
    }
}

/// Permission actions that can be allowed outright or gated behind proposals.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAction {
    ReadAny,
    WebSearch,
    WebFetch,
    CodeExecute,
    FileWrite,
    Git,
    Shell,
    ToolCall,
    SelfModify,
    CredentialChange,
    SystemInstall,
    NetworkListen,
    OutboundMessage,
    FileDelete,
    OutsideWorkspace,
    KernelModify,
}

impl PermissionAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadAny => "read_any",
            Self::WebSearch => "web_search",
            Self::WebFetch => "web_fetch",
            Self::CodeExecute => "code_execute",
            Self::FileWrite => "file_write",
            Self::Git => "git",
            Self::Shell => "shell",
            Self::ToolCall => "tool_call",
            Self::SelfModify => "self_modify",
            Self::CredentialChange => "credential_change",
            Self::SystemInstall => "system_install",
            Self::NetworkListen => "network_listen",
            Self::OutboundMessage => "outbound_message",
            Self::FileDelete => "file_delete",
            Self::OutsideWorkspace => "outside_workspace",
            Self::KernelModify => "kernel_modify",
        }
    }
}

/// Permissions configuration for preset-based and custom autonomy policies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PermissionsConfig {
    /// Selected preset that produced these permission lists.
    pub preset: PermissionPreset,
    /// Whether restricted actions are denied or trigger prompts.
    #[serde(default)]
    pub mode: CapabilityMode,
    /// Actions Fawx can perform without asking.
    pub unrestricted: Vec<PermissionAction>,
    /// Actions that require human approval via proposal.
    pub proposal_required: Vec<PermissionAction>,
}

impl PermissionsConfig {
    /// 🔥 Power User — full workspace autonomy, proposals for external actions.
    pub fn power() -> Self {
        Self {
            preset: PermissionPreset::Power,
            mode: CapabilityMode::Capability,
            unrestricted: actions(&[
                PermissionAction::ReadAny,
                PermissionAction::WebSearch,
                PermissionAction::WebFetch,
                PermissionAction::CodeExecute,
                PermissionAction::FileWrite,
                PermissionAction::Git,
                PermissionAction::Shell,
                PermissionAction::ToolCall,
                PermissionAction::SelfModify,
            ]),
            proposal_required: actions(&[
                PermissionAction::CredentialChange,
                PermissionAction::SystemInstall,
                PermissionAction::NetworkListen,
                PermissionAction::OutboundMessage,
                PermissionAction::FileDelete,
                PermissionAction::OutsideWorkspace,
                PermissionAction::KernelModify,
            ]),
        }
    }

    /// 🔒 Cautious — proposals for writes too.
    pub fn cautious() -> Self {
        Self {
            preset: PermissionPreset::Cautious,
            mode: CapabilityMode::Capability,
            unrestricted: actions(&[
                PermissionAction::ReadAny,
                PermissionAction::WebSearch,
                PermissionAction::WebFetch,
                PermissionAction::ToolCall,
            ]),
            proposal_required: actions(&[
                PermissionAction::CodeExecute,
                PermissionAction::FileWrite,
                PermissionAction::Git,
                PermissionAction::Shell,
                PermissionAction::SelfModify,
                PermissionAction::CredentialChange,
                PermissionAction::SystemInstall,
                PermissionAction::NetworkListen,
                PermissionAction::OutboundMessage,
                PermissionAction::FileDelete,
                PermissionAction::OutsideWorkspace,
                PermissionAction::KernelModify,
            ]),
        }
    }

    /// 🧪 Experimental — maximum autonomy including kernel self-modification.
    pub fn experimental() -> Self {
        Self {
            preset: PermissionPreset::Experimental,
            mode: CapabilityMode::Capability,
            unrestricted: actions(&[
                PermissionAction::ReadAny,
                PermissionAction::WebSearch,
                PermissionAction::WebFetch,
                PermissionAction::CodeExecute,
                PermissionAction::FileWrite,
                PermissionAction::Git,
                PermissionAction::Shell,
                PermissionAction::ToolCall,
                PermissionAction::SelfModify,
                PermissionAction::KernelModify,
            ]),
            proposal_required: actions(&[
                PermissionAction::CredentialChange,
                PermissionAction::SystemInstall,
                PermissionAction::NetworkListen,
                PermissionAction::OutboundMessage,
                PermissionAction::FileDelete,
                PermissionAction::OutsideWorkspace,
            ]),
        }
    }

    /// Open — everything allowed except privilege escalation.
    pub fn open() -> Self {
        Self {
            preset: PermissionPreset::Experimental,
            mode: CapabilityMode::Capability,
            ..Self::experimental()
        }
    }

    /// Standard — developer workflow, credential/system changes blocked.
    pub fn standard() -> Self {
        Self {
            preset: PermissionPreset::Power,
            mode: CapabilityMode::Capability,
            ..Self::power()
        }
    }

    /// Restricted — read-heavy, most writes blocked.
    pub fn restricted() -> Self {
        Self {
            preset: PermissionPreset::Cautious,
            mode: CapabilityMode::Capability,
            ..Self::cautious()
        }
    }

    pub fn from_preset_name(name: &str) -> Result<Self, String> {
        match PermissionPreset::from_str(name)? {
            PermissionPreset::Power => Ok(Self::power()),
            PermissionPreset::Cautious => Ok(Self::cautious()),
            PermissionPreset::Experimental => Ok(Self::experimental()),
            PermissionPreset::Custom => Ok(Self {
                preset: PermissionPreset::Custom,
                ..Self::default()
            }),
        }
    }
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self::standard()
    }
}

fn actions(list: &[PermissionAction]) -> Vec<PermissionAction> {
    list.to_vec()
}

/// Budget configuration for per-session and daily cost guardrails.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BudgetConfig {
    /// Max cost in cents per session (0 = unlimited). E.g., 500 = $5.00.
    pub max_session_cost_cents: u32,
    /// Max cost in cents per day (0 = unlimited).
    pub max_daily_cost_cents: u32,
    /// Alert threshold in cents.
    pub alert_threshold_cents: u32,
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

/// Sandbox configuration for process and network execution limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SandboxConfig {
    /// Allow network access from shell/skills.
    pub allow_network: bool,
    /// Allow subprocess spawning.
    pub allow_subprocess: bool,
    /// Kill processes after this many seconds (None = no limit).
    pub max_execution_seconds: Option<u64>,
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

/// Proposal configuration for approval timing, channels, and expiry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProposalConfig {
    /// Minutes before auto-approving proposals (None = never).
    pub auto_approve_timeout_minutes: Option<u32>,
    /// Where to send proposal notifications.
    pub notification_channels: Vec<String>,
    /// Hours before proposals expire unacted (None = never expires).
    pub expiry_hours: Option<u32>,
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

/// Fleet configuration for multi-node coordination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FleetConfig {
    /// Whether this node acts as a coordinator.
    pub coordinator: bool,
    /// Seconds before a node is considered stale.
    pub stale_timeout_seconds: u64,
    /// Nodes to auto-register (for coordinator).
    pub nodes: Vec<NodeConfig>,
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

/// Configuration for a known node in the fleet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeConfig {
    /// Unique node identifier (required by spec).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// HTTP API endpoint.
    pub endpoint: Option<String>,
    /// Bearer token for authentication.
    pub auth_token: Option<String>,
    /// Capability strings (e.g., "agentic_loop", "skill_build").
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// SSH address (IP or hostname) for SSH transport.
    pub address: Option<String>,
    /// SSH username.
    pub user: Option<String>,
    /// Path to SSH private key.
    pub ssh_key: Option<String>,
}

/// Webhook channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct WebhookConfig {
    /// Whether webhook channels are enabled.
    pub enabled: bool,
    /// Configured webhook channels.
    pub channels: Vec<WebhookChannelConfig>,
}

/// Configuration for a single webhook channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebhookChannelConfig {
    /// Unique channel identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Optional callback URL for response delivery.
    pub callback_url: Option<String>,
}

/// Orchestrator configuration for distributed task coordination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OrchestratorConfig {
    /// Whether the orchestrator is enabled.
    pub enabled: bool,
    /// Maximum number of pending tasks before rejecting new ones.
    pub max_pending_tasks: usize,
    /// Default task timeout in milliseconds (0 = no timeout).
    pub default_timeout_ms: u64,
    /// Default max retries for tasks (0 = no retry).
    pub default_max_retries: u32,
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

/// Telegram channel configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TelegramChannelConfig {
    /// Whether the Telegram channel is enabled.
    pub enabled: bool,
    /// Bot token (from BotFather). Can also be set via FAWX_TELEGRAM_TOKEN env var.
    pub bot_token: Option<String>,
    /// Restrict to specific Telegram chat IDs. Empty = accept all.
    pub allowed_chat_ids: Vec<i64>,
    /// Secret token for webhook validation. If set, the webhook handler
    /// validates the `X-Telegram-Bot-Api-Secret-Token` header on every
    /// incoming request. Can also be set via FAWX_TELEGRAM_WEBHOOK_SECRET.
    pub webhook_secret: Option<String>,
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

/// Thinking budget for extended thinking support.
///
/// Controls how much reasoning budget the model gets per request.
/// `None` is treated as `Adaptive` (the default).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum ThinkingBudget {
    #[default]
    #[serde(rename = "adaptive")]
    Adaptive,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "off")]
    Off,
    /// OpenAI "none" — reasoning disabled.
    #[serde(rename = "none")]
    None,
    /// OpenAI GPT-5 "minimal".
    #[serde(rename = "minimal")]
    Minimal,
    /// Anthropic Opus 4.6 "max".
    #[serde(rename = "max")]
    Max,
    /// OpenAI GPT-5.4 "xhigh".
    #[serde(rename = "xhigh")]
    Xhigh,
}

impl std::fmt::Display for ThinkingBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Adaptive => write!(f, "adaptive"),
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
            Self::Off => write!(f, "off"),
            Self::None => write!(f, "none"),
            Self::Minimal => write!(f, "minimal"),
            Self::Max => write!(f, "max"),
            Self::Xhigh => write!(f, "xhigh"),
        }
    }
}

impl ThinkingBudget {
    /// Map a budget level to its token count, or `None` for disabled variants.
    pub fn budget_tokens(&self) -> Option<u32> {
        match self {
            Self::Xhigh | Self::Max => Some(32_000),
            Self::High => Some(10_000),
            Self::Adaptive | Self::Medium => Some(5_000),
            Self::Low | Self::Minimal => Some(1_024),
            Self::Off | Self::None => Option::None,
        }
    }
}

impl std::str::FromStr for ThinkingBudget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "adaptive" => Ok(Self::Adaptive),
            "high" => Ok(Self::High),
            "medium" => Ok(Self::Medium),
            "low" => Ok(Self::Low),
            "off" => Ok(Self::Off),
            "none" => Ok(Self::None),
            "minimal" => Ok(Self::Minimal),
            "max" => Ok(Self::Max),
            "xhigh" => Ok(Self::Xhigh),
            other => Err(format!(
                "unknown thinking level '{other}'; expected off, none, minimal, low, medium, high, xhigh, max, or adaptive"
            )),
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

/// Scope for borrowed GitHub credentials.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorrowScope {
    #[default]
    ReadOnly,
    Contribution,
}

impl std::fmt::Display for BorrowScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadOnly => write!(f, "read_only"),
            Self::Contribution => write!(f, "contribution"),
        }
    }
}

/// Security settings for WASM skill signature verification.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SecurityConfig {
    /// When true, reject any WASM skill without a valid signature.
    /// When false (default), unsigned skills load with a warning.
    /// Invalid signatures are ALWAYS rejected regardless of this setting.
    pub require_signatures: bool,
    /// Maximum GitHub PAT borrow scope for subagents/workers.
    /// Defaults to read-only for safety. Set to "contribution" to allow
    /// subagents to push branches and create PRs.
    #[serde(default)]
    pub github_borrow_scope: BorrowScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GeneralConfig {
    pub data_dir: Option<PathBuf>,
    pub max_iterations: u32,
    pub max_history: usize,
    /// Extended thinking budget. `None` is treated as `Adaptive`.
    pub thinking: Option<ThinkingBudget>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ModelConfig {
    pub default_model: Option<String>,
    pub synthesis_instruction: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LoggingConfig {
    pub file_logging: Option<bool>,
    pub file_level: Option<String>,
    pub stderr_level: Option<String>,
    pub max_files: Option<usize>,
    pub log_dir: Option<String>,
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
    pub embeddings_enabled: bool,
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

/// Expand a leading `~` in a path to the user's home directory.
///
/// Only expands `~` at the very start of the path (i.e., `~/.fawx` becomes
/// `/home/user/.fawx`). Paths like `foo/~/bar` or absolute paths are returned
/// unchanged. Returns the original path if the home directory cannot be
/// determined.
fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    // ~user paths and everything else: return as-is
    path.to_path_buf()
}

/// Apply tilde expansion to an optional path field.
fn expand_tilde_opt(path: &mut Option<PathBuf>) {
    if let Some(p) = path.as_mut() {
        let original = p.clone();
        *p = expand_tilde(&original);
        if *p != original {
            tracing::debug!(
                "config path expanded: {} -> {}",
                original.display(),
                p.display()
            );
        }
    }
}

fn expand_tilde_string_opt(path: &mut Option<String>) {
    if let Some(path_str) = path.as_mut() {
        let original = path_str.clone();
        let expanded = expand_tilde(Path::new(&original));
        let expanded_str = expanded.to_string_lossy().into_owned();
        if expanded_str != original {
            tracing::debug!("config path expanded: {} -> {}", original, expanded_str);
            *path_str = expanded_str;
        }
    }
}

pub fn parse_log_level(value: &str) -> Option<LevelFilter> {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => Some(LevelFilter::ERROR),
        "warn" => Some(LevelFilter::WARN),
        "info" => Some(LevelFilter::INFO),
        "debug" => Some(LevelFilter::DEBUG),
        "trace" => Some(LevelFilter::TRACE),
        _ => None,
    }
}

fn validate_log_level(field: &str, value: &Option<String>) -> Result<(), String> {
    let Some(level) = value.as_ref() else {
        return Ok(());
    };
    if parse_log_level(level).is_some() {
        return Ok(());
    }
    Err(format!("{field} must be one of: {VALID_LOG_LEVELS}"))
}

impl FawxConfig {
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        let config_path = data_dir.join("config.toml");
        if !config_path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&config_path)
            .map_err(|error| format!("failed to read config: {error}"))?;
        let mut config: Self =
            toml::from_str(&content).map_err(|error| format!("invalid config: {error}"))?;
        config.validate()?;
        config.expand_paths();
        Ok(config)
    }

    /// Expand `~` to the user's home directory in all user-facing path configs.
    fn expand_paths(&mut self) {
        expand_tilde_opt(&mut self.general.data_dir);
        expand_tilde_string_opt(&mut self.logging.log_dir);
        expand_tilde_opt(&mut self.tools.working_dir);
        expand_tilde_opt(&mut self.self_modify.proposals_dir);
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
            validate_synthesis_instruction(instruction)?;
        }
        if let Some(max_files) = self.logging.max_files {
            if max_files == 0 {
                return Err("logging.max_files must be >= 1".to_string());
            }
        }
        validate_log_level("logging.file_level", &self.logging.file_level)?;
        validate_log_level("logging.stderr_level", &self.logging.stderr_level)?;
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

/// Persist the thinking budget to `config.toml`, preserving comments.
pub fn save_thinking_budget(data_dir: &Path, budget: ThinkingBudget) -> Result<(), String> {
    let config_path = data_dir.join("config.toml");
    fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
    if config_path.exists() {
        return update_thinking_budget(&config_path, budget);
    }
    let mut config = FawxConfig::default();
    config.general.thinking = Some(budget);
    config.save(data_dir)
}

fn update_thinking_budget(config_path: &Path, budget: ThinkingBudget) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|error| format!("failed to read config: {error}"))?;
    let mut document = parse_config_document(&content)?;
    set_string_field(&mut document, &["general"], "thinking", &budget.to_string())?;
    write_config_file(config_path, document.to_string())
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

pub(crate) fn parse_config_document(content: &str) -> Result<DocumentMut, String> {
    content
        .parse::<DocumentMut>()
        .map_err(|error| format!("invalid config: {error}"))
}

/// Set a field in a TOML document, inferring the correct value type.
///
/// Attempts to parse `field_value` as an integer, float, or boolean before
/// falling back to a string. When updating an existing key the original
/// value's type is preferred (e.g. an existing integer stays integer even
/// if the new value could be read as a string). Inline comments/decor on
/// the original value are preserved.
pub(crate) fn set_typed_field(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: &str,
) -> Result<(), String> {
    let table = get_or_insert_table(document, sections)?;
    if let Some(item) = table.get_mut(key) {
        return update_typed_item(item, key, field_value);
    }
    // New key — infer type from the raw string.
    table[key] = infer_typed_value(field_value);
    Ok(())
}

/// Infer a `toml_edit::Value` from a raw string, trying integer → bool → string.
fn infer_typed_value(raw: &str) -> Item {
    if let Ok(n) = raw.parse::<i64>() {
        return value(n);
    }
    match raw {
        "true" => return value(true),
        "false" => return value(false),
        _ => {}
    }
    value(raw)
}

fn update_typed_item(item: &mut Item, key: &str, field_value: &str) -> Result<(), String> {
    let existing = item
        .as_value()
        .ok_or_else(|| format!("config field '{key}' must be a value"))?;
    let decor = existing.decor().clone();

    // Match the existing value's type when possible.
    let new_item = if existing.is_integer() {
        if let Ok(n) = field_value.parse::<i64>() {
            value(n)
        } else {
            // Fall back to string if the new value isn't numeric.
            value(field_value)
        }
    } else if existing.is_bool() {
        match field_value {
            "true" => value(true),
            "false" => value(false),
            _ => value(field_value),
        }
    } else {
        value(field_value)
    };

    *item = new_item;
    item.as_value_mut()
        .ok_or_else(|| format!("config field '{key}' must be a value"))?
        .decor_mut()
        .clone_from(&decor);
    Ok(())
}

// Keep the old name as a thin wrapper for callers that always want strings.
pub(crate) fn set_string_field(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: &str,
) -> Result<(), String> {
    let table = get_or_insert_table(document, sections)?;
    if let Some(item) = table.get_mut(key) {
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
        return Ok(());
    }
    table[key] = value(field_value);
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

pub(crate) fn write_config_file(config_path: &Path, content: String) -> Result<(), String> {
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
        let path = PathBuf::from("~user/.config");
        let expanded = expand_tilde(&path);
        assert_eq!(expanded, PathBuf::from("~user/.config"));
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
