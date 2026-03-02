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
    pub tool_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSummary {
    pub max_iterations: u32,
    pub max_history: usize,
    pub memory_enabled: bool,
}
