//! Validation helpers and invariants for parsed config values.

use crate::{FawxConfig, SelfModifyCliConfig};
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

pub(crate) fn validate_glob_patterns(self_modify: &SelfModifyCliConfig) -> Result<(), String> {
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

impl FawxConfig {
    pub(crate) fn validate(&self) -> Result<(), String> {
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
}
