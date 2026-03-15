use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 600;
const DEFAULT_MAX_CONCURRENT: usize = 5;

/// Configuration for spawning a subagent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnConfig {
    pub label: Option<String>,
    pub task: String,
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub mode: SpawnMode,
    pub timeout: Duration,
    pub max_tokens: Option<u64>,
    pub cwd: Option<PathBuf>,
    pub system_prompt: Option<String>,
}

impl SpawnConfig {
    /// Build a one-shot config with default limits.
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            label: None,
            task: task.into(),
            model: None,
            thinking: None,
            mode: SpawnMode::Run,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_tokens: None,
            cwd: None,
            system_prompt: None,
        }
    }
}

/// Execution mode for the subagent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnMode {
    Run,
    Session,
}

/// Global limits applied by the manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentLimits {
    pub max_concurrent: usize,
    pub default_timeout: Duration,
    pub total_token_budget: Option<u64>,
}

impl Default for SubagentLimits {
    fn default() -> Self {
        Self {
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            default_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            total_token_budget: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_config_new_uses_default_run_settings() {
        let config = SpawnConfig::new("review this diff");

        assert_eq!(config.task, "review this diff");
        assert_eq!(config.mode, SpawnMode::Run);
        assert_eq!(config.timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        assert!(config.label.is_none());
        assert!(config.model.is_none());
    }

    #[test]
    fn subagent_limits_default_matches_spec_defaults() {
        let limits = SubagentLimits::default();

        assert_eq!(limits.max_concurrent, DEFAULT_MAX_CONCURRENT);
        assert_eq!(
            limits.default_timeout,
            Duration::from_secs(DEFAULT_TIMEOUT_SECS)
        );
        assert!(limits.total_token_budget.is_none());
    }
}
