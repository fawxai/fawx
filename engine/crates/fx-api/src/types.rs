use fx_kernel::ErrorCategory;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MessageRequest {
    pub message: String,
    #[serde(default)]
    pub images: Vec<ImagePayload>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub response: String,
    pub model: String,
    pub iterations: u32,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SetupAuthStatus {
    pub bearer_token_present: bool,
    pub providers_configured: Vec<String>,
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
}

impl From<fx_llm::ModelInfo> for ModelInfoDto {
    fn from(m: fx_llm::ModelInfo) -> Self {
        Self {
            model_id: m.model_id,
            provider: m.provider_name,
            auth_method: m.auth_method,
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
}

impl From<(String, String, Vec<String>)> for SkillSummaryDto {
    fn from((name, description, tools): (String, String, Vec<String>)) -> Self {
        Self {
            name,
            description,
            tools,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImagePayload {
    pub data: String,
    pub media_type: String,
}
