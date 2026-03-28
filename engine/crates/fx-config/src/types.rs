//! Core configuration data types for `fx-config`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AgentBehaviorConfig {
    pub custom_instructions: Option<String>,
    pub verbosity: String,
    pub proactive: bool,
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
/// Controls cross-turn conversation deduplication. Disabled by default -
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
    /// OpenAI "none" - reasoning disabled.
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
/// improvements. Disabled by default - requires explicit opt-in.
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
