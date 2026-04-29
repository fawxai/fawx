use crate::engine::ResultKind;
use fx_kernel::ErrorCategory;
use fx_session::SessionThreadBinding;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct MessageRequest {
    pub message: String,
    #[serde(default)]
    pub images: Vec<ImagePayload>,
    #[serde(default)]
    pub documents: Vec<DocumentPayload>,
    #[serde(default)]
    pub steering: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub response: String,
    pub model: String,
    pub iterations: u32,
    pub result_kind: ResultKind,
}

#[derive(Debug, Deserialize)]
pub struct SendToSessionRequest {
    pub text: Option<String>,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SendToSessionResponse {
    pub envelope_id: String,
    pub delivered: bool,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub model: String,
    pub uptime_seconds: u64,
    pub skills_loaded: usize,
    pub https_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub model: String,
    pub skills: Vec<String>,
    pub memory_entries: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tailscale_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    General,
    Repository,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositorySummary {
    pub root: String,
    pub vcs: String,
    pub current_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub clean: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSummary {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: WorkspaceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepositorySummary>,
    pub last_opened_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacesResponse {
    pub workspaces: Vec<WorkspaceSummary>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenWorkspaceRequest {
    pub path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct WorkspaceScope(Option<String>);

impl WorkspaceScope {
    #[cfg(test)]
    pub(crate) fn explicit(path: impl Into<String>) -> Self {
        Self(Some(path.into()))
    }

    pub fn requested_path(&self) -> Option<&str> {
        self.0.as_deref()
    }

    pub fn is_default(&self) -> bool {
        self.0.is_none()
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct WorkspaceRouteQuery {
    #[serde(rename = "workspace_path", default)]
    pub workspace_scope: WorkspaceScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeStatus {
    Active,
    Available,
    Detached,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeSummary {
    pub id: String,
    pub workspace_id: String,
    pub label: String,
    pub path: String,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    pub status: WorktreeStatus,
    pub clean: bool,
    pub ahead_count: u64,
    pub behind_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreesResponse {
    pub worktrees: Vec<WorktreeSummary>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadKind {
    General,
    Coding,
    Automation,
    Subagent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    Active,
    Idle,
    Completed,
    Failed,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadSummary {
    pub id: String,
    pub title: String,
    pub kind: ThreadKind,
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    pub active_session_id: String,
    pub status: ThreadStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadsResponse {
    pub threads: Vec<ThreadSummary>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateThreadRequest {
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(
        rename = "workspace_path",
        default,
        skip_serializing_if = "WorkspaceScope::is_default"
    )]
    pub workspace_scope: WorkspaceScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateWorktreeRequest {
    pub workspace_id: String,
    pub branch: String,
    #[serde(
        rename = "workspace_path",
        default,
        skip_serializing_if = "WorkspaceScope::is_default"
    )]
    pub workspace_scope: WorkspaceScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachWorktreeThreadRequest {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachWorktreeThreadResponse {
    pub worktree_id: String,
    pub thread_id: String,
    pub active_session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveWorktreeResponse {
    pub worktree_id: String,
    pub archived_thread_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeleteWorktreeResponse {
    pub deleted: bool,
    pub worktree_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SetupAuthStatus {
    pub bearer_token_present: bool,
    pub providers_configured: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionThreadBindingDto {
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
}

impl From<SessionThreadBinding> for SessionThreadBindingDto {
    fn from(value: SessionThreadBinding) -> Self {
        Self {
            workspace_id: value.workspace_id,
            execution_root: value.execution_root,
            worktree_path: value.worktree_path,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct SetupLaunchAgentStatus {
    pub installed: bool,
    pub loaded: bool,
    pub auto_start_enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SetupLocalServerStatus {
    pub host: String,
    pub port: u16,
    pub https_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct SetupTailscaleStatus {
    pub installed: bool,
    pub running: bool,
    pub logged_in: bool,
    pub hostname: Option<String>,
    pub cert_ready: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SetupStatusResponse {
    pub mode: String,
    pub setup_complete: bool,
    pub has_valid_config: bool,
    pub server_running: bool,
    pub launchagent: SetupLaunchAgentStatus,
    pub local_server: SetupLocalServerStatus,
    pub auth: SetupAuthStatus,
    pub tailscale: SetupTailscaleStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerStatusResponse {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub pid: u32,
    pub host: String,
    pub port: u16,
    pub https_enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerRestartResponse {
    pub accepted: bool,
    pub restart_via: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServerStopResponse {
    pub stopped: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SetupTokenRequest {
    pub setup_token: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SetupTokenResponse {
    pub provider: String,
    pub status: String,
    pub auth_method: String,
    pub model_count: usize,
    pub verified: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ApiKeyRequest {
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ApiKeyResponse {
    pub provider: String,
    pub status: String,
    pub auth_method: String,
    pub model_count: usize,
    pub verified: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct DeleteProviderResponse {
    pub provider: String,
    pub removed: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct VerifyRequest {
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct VerifyResponse {
    pub provider: String,
    pub verified: bool,
    pub status: String,
    pub message: String,
    pub checked_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ConfigPatchRequest {
    pub changes: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConfigPatchResponse {
    pub updated: bool,
    pub restart_required: bool,
    pub changed_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConfigPresetSummary {
    pub name: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConfigPresetsResponse {
    pub presets: Vec<ConfigPresetSummary>,
    pub total: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ApplyConfigPresetRequest {
    pub confirm: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ApplyConfigPresetResponse {
    pub name: String,
    pub applied: bool,
    pub restart_required: bool,
    pub changed_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ConfigPresetDiffEntry {
    pub key: String,
    pub old: Value,
    pub r#new: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ConfigPresetDiffResponse {
    pub name: String,
    pub changes: Vec<ConfigPresetDiffEntry>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ErrorRecordDto {
    pub timestamp: String,
    pub category: ErrorCategory,
    pub message: String,
    pub recoverable: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RecentErrorsResponse {
    pub errors: Vec<ErrorRecordDto>,
}

#[derive(Debug, Deserialize)]
pub struct SetModelRequest {
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct SetThinkingRequest {
    pub level: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfoDto {
    pub model_id: String,
    pub provider: String,
    pub auth_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub recommended: bool,
    pub thinking_levels: Vec<String>,
}

impl From<fx_llm::ModelInfo> for ModelInfoDto {
    fn from(m: fx_llm::ModelInfo) -> Self {
        Self {
            model_id: m.model_id,
            provider: m.provider_name,
            auth_method: m.auth_method,
            display_name: m.display_name,
            recommended: m.recommended,
            thinking_levels: m.thinking_levels,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ThinkingLevelDto {
    pub level: String,
    pub budget_tokens: Option<u32>,
    pub available: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ThinkingAdjustedDto {
    pub from: String,
    pub to: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelSwitchDto {
    pub previous_model: String,
    pub active_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_adjusted: Option<ThinkingAdjustedDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextInfoDto {
    pub used_tokens: usize,
    pub max_tokens: usize,
    pub percentage: f32,
    pub compaction_threshold: f32,
}

pub trait ContextInfoSnapshotLike {
    fn used_tokens(&self) -> usize;
    fn max_tokens(&self) -> usize;
    fn percentage(&self) -> f32;
    fn compaction_threshold(&self) -> f32;
}

impl ContextInfoDto {
    pub fn from_snapshot(snapshot: &impl ContextInfoSnapshotLike) -> Self {
        Self {
            used_tokens: snapshot.used_tokens(),
            max_tokens: snapshot.max_tokens(),
            percentage: snapshot.percentage(),
            compaction_threshold: snapshot.compaction_threshold(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillSummaryDto {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_status: Option<String>,
    /// Opaque drift detail for installed skills whose source manifest no longer
    /// matches the active loaded revision. Non-nil means "update available";
    /// clients should not parse the string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_source: Option<String>,
}

impl From<(String, String, Vec<String>, Vec<String>)> for SkillSummaryDto {
    fn from(
        (name, description, tools, capabilities): (String, String, Vec<String>, Vec<String>),
    ) -> Self {
        Self {
            name,
            description,
            tools,
            capabilities,
            version: None,
            source: None,
            revision_hash: None,
            activated_at_ms: None,
            signature_status: None,
            stale_source: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthProviderDto {
    pub provider: String,
    pub auth_methods: Vec<String>,
    pub model_count: usize,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedImage {
    pub media_type: String,
    pub base64_data: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedDocument {
    pub media_type: String,
    pub base64_data: String,
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImagePayload {
    pub data: String,
    pub media_type: String,
}

#[derive(Debug, Deserialize)]
pub struct DocumentPayload {
    pub data: String,
    pub media_type: String,
    #[serde(default)]
    pub filename: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QrPairingResponse {
    pub scheme_url: String,
    pub display_host: String,
    pub port: u16,
    pub transport: String,
    pub same_network_only: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct TailscaleCertRequest {
    pub hostname: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TailscaleCertResponse {
    pub success: bool,
    pub hostname: String,
    pub cert_path: String,
    pub key_path: String,
    pub https_enabled: bool,
}
