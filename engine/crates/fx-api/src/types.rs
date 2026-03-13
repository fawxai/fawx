use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct MessageRequest {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub response: String,
    pub model: String,
    pub iterations: u32,
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

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedImage {
    pub media_type: String,
    pub base64_data: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImagePayload {
    pub data: String,
    pub media_type: String,
}
