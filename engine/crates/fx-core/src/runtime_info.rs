use serde::Serialize;

/// Runtime state snapshot for self-introspection.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    pub active_model: String,
    pub provider: String,
    pub skills: Vec<SkillInfo>,
    pub config_summary: ConfigSummary,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub tool_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSummary {
    pub max_iterations: u32,
    pub max_history: usize,
    pub memory_enabled: bool,
}
