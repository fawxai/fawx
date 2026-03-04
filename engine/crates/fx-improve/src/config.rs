use crate::error::ImprovementError;
use fx_analysis::Confidence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImprovementConfig {
    pub min_confidence: Confidence,
    pub min_evidence_count: usize,
    pub output_mode: OutputMode,
    pub cooldown_hours: u32,
    pub max_improvements_per_run: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OutputMode {
    ProposalOnly,
    ProposalWithBranch,
    DryRun,
}

impl ImprovementConfig {
    pub fn validate(&self) -> Result<(), ImprovementError> {
        validate_non_zero("min_evidence_count", self.min_evidence_count)?;
        validate_non_zero("max_improvements_per_run", self.max_improvements_per_run)?;
        validate_non_zero("cooldown_hours", self.cooldown_hours as usize)?;
        Ok(())
    }
}

fn validate_non_zero(name: &str, value: usize) -> Result<(), ImprovementError> {
    if value == 0 {
        return Err(ImprovementError::Config(format!(
            "{name} must be greater than 0"
        )));
    }
    Ok(())
}

impl Default for ImprovementConfig {
    fn default() -> Self {
        Self {
            min_confidence: Confidence::High,
            min_evidence_count: 3,
            output_mode: OutputMode::ProposalOnly,
            cooldown_hours: 24,
            max_improvements_per_run: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = ImprovementConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_min_evidence_count() {
        let config = ImprovementConfig {
            min_evidence_count: 0,
            ..ImprovementConfig::default()
        };

        let error = config.validate().unwrap_err();
        assert!(
            matches!(error, ImprovementError::Config(message) if message.contains("min_evidence_count"))
        );
    }

    #[test]
    fn validate_rejects_zero_max_improvements_per_run() {
        let config = ImprovementConfig {
            max_improvements_per_run: 0,
            ..ImprovementConfig::default()
        };

        let error = config.validate().unwrap_err();
        assert!(
            matches!(error, ImprovementError::Config(message) if message.contains("max_improvements_per_run"))
        );
    }

    #[test]
    fn validate_rejects_zero_cooldown_hours() {
        let config = ImprovementConfig {
            cooldown_hours: 0,
            ..ImprovementConfig::default()
        };

        let error = config.validate().unwrap_err();
        assert!(
            matches!(error, ImprovementError::Config(message) if message.contains("cooldown_hours"))
        );
    }
}
