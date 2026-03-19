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
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSummary {
    pub max_iterations: u32,
    pub max_history: usize,
    pub memory_enabled: bool,
}
