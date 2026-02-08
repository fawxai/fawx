//! Skill manifest parsing and validation.

use nv_core::error::SkillError;
use serde::{Deserialize, Serialize};

/// Capability a skill can request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// HTTP requests
    Network,
    /// Persistent key-value storage
    Storage,
    /// Send notifications
    Notifications,
    /// Read sensor data
    Sensors,
    /// Control phone (high privilege)
    PhoneActions,
}

/// Skill manifest metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    /// Skill name (unique identifier)
    pub name: String,
    /// Semantic version
    pub version: String,
    /// Human-readable description
    pub description: String,
    /// Author/publisher
    pub author: String,
    /// Host API version contract (e.g., "host_api_v1")
    pub api_version: String,
    /// Required capabilities
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// Entry point function name
    #[serde(default = "default_entry_point")]
    pub entry_point: String,
}

fn default_entry_point() -> String {
    "run".to_string()
}

/// Parse a skill manifest from TOML.
pub fn parse_manifest(toml_str: &str) -> Result<SkillManifest, SkillError> {
    toml::from_str(toml_str)
        .map_err(|e| SkillError::InvalidManifest(format!("Failed to parse TOML: {}", e)))
}

/// Validate a skill manifest.
pub fn validate_manifest(manifest: &SkillManifest) -> Result<(), SkillError> {
    // Check required fields are non-empty
    if manifest.name.trim().is_empty() {
        return Err(SkillError::InvalidManifest(
            "name cannot be empty".to_string(),
        ));
    }

    if manifest.version.trim().is_empty() {
        return Err(SkillError::InvalidManifest(
            "version cannot be empty".to_string(),
        ));
    }

    if manifest.description.trim().is_empty() {
        return Err(SkillError::InvalidManifest(
            "description cannot be empty".to_string(),
        ));
    }

    if manifest.author.trim().is_empty() {
        return Err(SkillError::InvalidManifest(
            "author cannot be empty".to_string(),
        ));
    }

    // Validate api_version
    if manifest.api_version != "host_api_v1" {
        return Err(SkillError::InvalidManifest(format!(
            "Unsupported api_version '{}', expected 'host_api_v1'",
            manifest.api_version
        )));
    }

    if manifest.entry_point.trim().is_empty() {
        return Err(SkillError::InvalidManifest(
            "entry_point cannot be empty".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_manifest() {
        let toml = r#"
name = "weather"
version = "1.0.0"
description = "Weather lookup skill"
author = "Nova Team"
api_version = "host_api_v1"
capabilities = ["network"]
entry_point = "run"
        "#;

        let manifest = parse_manifest(toml).expect("Should parse valid manifest");
        assert_eq!(manifest.name, "weather");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.api_version, "host_api_v1");
        assert_eq!(manifest.capabilities, vec![Capability::Network]);
        assert_eq!(manifest.entry_point, "run");
    }

    #[test]
    fn test_parse_invalid_toml() {
        let toml = r#"
name = "broken
        "#;

        let result = parse_manifest(toml);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::InvalidManifest(_))));
    }

    #[test]
    fn test_validate_missing_name() {
        let manifest = SkillManifest {
            name: "".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "Nova".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::InvalidManifest(_))));
    }

    #[test]
    fn test_validate_invalid_api_version() {
        let manifest = SkillManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "Nova".to_string(),
            api_version: "v2".to_string(),
            capabilities: vec![],
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(result.is_err());
        if let Err(SkillError::InvalidManifest(msg)) = result {
            assert!(msg.contains("Unsupported api_version"));
        }
    }

    #[test]
    fn test_default_entry_point() {
        let toml = r#"
name = "test"
version = "1.0.0"
description = "Test skill"
author = "Nova"
api_version = "host_api_v1"
        "#;

        let manifest = parse_manifest(toml).expect("Should parse");
        assert_eq!(manifest.entry_point, "run");
    }

    #[test]
    fn test_capabilities_deserialization() {
        let toml = r#"
name = "test"
version = "1.0.0"
description = "Test skill"
author = "Nova"
api_version = "host_api_v1"
capabilities = ["network", "storage", "notifications", "sensors", "phone_actions"]
        "#;

        let manifest = parse_manifest(toml).expect("Should parse");
        assert_eq!(manifest.capabilities.len(), 5);
        assert!(manifest.capabilities.contains(&Capability::Network));
        assert!(manifest.capabilities.contains(&Capability::Storage));
        assert!(manifest.capabilities.contains(&Capability::Notifications));
        assert!(manifest.capabilities.contains(&Capability::Sensors));
        assert!(manifest.capabilities.contains(&Capability::PhoneActions));
    }

    #[test]
    fn test_validate_empty_description() {
        let manifest = SkillManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "   ".to_string(),
            author: "Nova".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            entry_point: "run".to_string(),
        };

        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn test_validate_empty_author() {
        let manifest = SkillManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            entry_point: "run".to_string(),
        };

        assert!(validate_manifest(&manifest).is_err());
    }
}
