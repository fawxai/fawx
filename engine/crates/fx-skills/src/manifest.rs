//! Skill manifest parsing and validation.

use fx_core::error::SkillError;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Capability a skill can request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// HTTP requests
    Network,
    /// HTTP requests restricted to specific domains
    NetworkRestricted { allowed_domains: Vec<String> },
    /// Persistent key-value storage
    Storage,
    /// Execute shell commands
    Shell,
    /// Read and write local files
    Filesystem,
    /// Send notifications
    Notifications,
    /// Read sensor data
    Sensors,
    /// Control phone (high privilege)
    PhoneActions,
}

pub const ALL_CAPABILITIES: [Capability; 7] = [
    Capability::Network,
    Capability::Storage,
    Capability::Shell,
    Capability::Filesystem,
    Capability::Notifications,
    Capability::Sensors,
    Capability::PhoneActions,
];

impl Capability {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Capability::Network => "network",
            Capability::NetworkRestricted { .. } => "network_restricted",
            Capability::Storage => "storage",
            Capability::Shell => "shell",
            Capability::Filesystem => "filesystem",
            Capability::Notifications => "notifications",
            Capability::Sensors => "sensors",
            Capability::PhoneActions => "phone_actions",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.parse().ok()
    }
}

impl FromStr for Capability {
    type Err = SkillError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        ALL_CAPABILITIES
            .into_iter()
            .find(|capability| capability.as_str() == value)
            .ok_or_else(|| SkillError::InvalidManifest(format!("Unknown capability '{}'", value)))
    }
}

/// Authority-relevant tool surface declared by a manifest-defined tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillToolAuthoritySurface {
    PathRead,
    PathWrite,
    PathDelete,
    GitCheckpoint,
    Command,
    Network,
    Other,
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
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
    /// Optional tool definitions declared by the skill.
    #[serde(default)]
    pub tools: Vec<SkillToolManifest>,
    /// Entry point function name
    #[serde(default = "default_entry_point")]
    pub entry_point: String,
}

/// Tool metadata declared by a skill manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillToolManifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub authority_surface: Option<SkillToolAuthoritySurface>,
    #[serde(default)]
    pub direct_utility: bool,
    #[serde(default)]
    pub trigger_patterns: Vec<String>,
    #[serde(default)]
    pub parameters: Vec<SkillToolParameterManifest>,
}

/// Parameter metadata declared by a manifest-defined tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillToolParameterManifest {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
}

fn default_entry_point() -> String {
    "run".to_string()
}

/// Parse a skill manifest from TOML.
pub fn parse_manifest(toml_str: &str) -> Result<SkillManifest, SkillError> {
    toml::from_str(toml_str)
        .map_err(|e| SkillError::InvalidManifest(format!("Failed to parse TOML: {}", e)))
}

pub fn validate_skill_name(name: &str) -> Result<(), SkillError> {
    if name.trim().is_empty() {
        return Err(SkillError::InvalidManifest(
            "name cannot be empty".to_string(),
        ));
    }

    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(SkillError::InvalidManifest(
            "name must not contain path separators or '..'".to_string(),
        ));
    }

    Ok(())
}

/// Validate a skill manifest.
pub fn validate_manifest(manifest: &SkillManifest) -> Result<(), SkillError> {
    validate_skill_name(&manifest.name)?;

    if manifest.version.trim().is_empty() {
        return Err(SkillError::InvalidManifest(
            "version cannot be empty".to_string(),
        ));
    }

    // Validate version is valid semver
    if Version::parse(&manifest.version).is_err() {
        return Err(SkillError::InvalidManifest(format!(
            "version '{}' is not a valid semantic version",
            manifest.version
        )));
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
    if manifest.api_version != "host_api_v1" && manifest.api_version != "host_api_v2" {
        return Err(SkillError::InvalidManifest(format!(
            "Unsupported api_version '{}', expected 'host_api_v1' or 'host_api_v2'",
            manifest.api_version
        )));
    }

    if manifest.entry_point.trim().is_empty() {
        return Err(SkillError::InvalidManifest(
            "entry_point cannot be empty".to_string(),
        ));
    }

    validate_tools(&manifest.tools)?;

    Ok(())
}

fn validate_tools(tools: &[SkillToolManifest]) -> Result<(), SkillError> {
    let mut seen_tool_names = std::collections::BTreeSet::new();
    for tool in tools {
        if tool.name.trim().is_empty() {
            return Err(SkillError::InvalidManifest(
                "tool name cannot be empty".to_string(),
            ));
        }
        if tool.description.trim().is_empty() {
            return Err(SkillError::InvalidManifest(format!(
                "tool '{}' description cannot be empty",
                tool.name
            )));
        }
        if tool.direct_utility && tool.trigger_patterns.is_empty() {
            return Err(SkillError::InvalidManifest(format!(
                "direct utility tool '{}' must declare at least one trigger pattern",
                tool.name
            )));
        }
        if !seen_tool_names.insert(tool.name.clone()) {
            return Err(SkillError::InvalidManifest(format!(
                "duplicate tool name '{}'",
                tool.name
            )));
        }

        let mut seen_parameter_names = std::collections::BTreeSet::new();
        for parameter in &tool.parameters {
            if parameter.name.trim().is_empty() {
                return Err(SkillError::InvalidManifest(format!(
                    "tool '{}' parameter name cannot be empty",
                    tool.name
                )));
            }
            if parameter.kind.trim().is_empty() {
                return Err(SkillError::InvalidManifest(format!(
                    "tool '{}' parameter '{}' type cannot be empty",
                    tool.name, parameter.name
                )));
            }
            if parameter.description.trim().is_empty() {
                return Err(SkillError::InvalidManifest(format!(
                    "tool '{}' parameter '{}' description cannot be empty",
                    tool.name, parameter.name
                )));
            }
            if !seen_parameter_names.insert(parameter.name.clone()) {
                return Err(SkillError::InvalidManifest(format!(
                    "duplicate parameter '{}' in tool '{}'",
                    parameter.name, tool.name
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_parse_valid_manifest() {
        let toml = r#"
name = "weather"
version = "1.0.0"
description = "Weather lookup skill"
author = "Fawx Team"
api_version = "host_api_v1"
capabilities = ["network"]
entry_point = "run"
        "#;

        let manifest = parse_manifest(toml).expect("Should parse valid manifest");
        assert_eq!(manifest.name, "weather");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.api_version, "host_api_v1");
        assert_eq!(manifest.capabilities, vec![Capability::Network]);
        assert!(manifest.tools.is_empty());
        assert_eq!(manifest.entry_point, "run");
    }

    #[test]
    fn test_parse_manifest_with_tools() {
        let toml = r#"
name = "browser"
version = "1.0.0"
description = "Browser skill"
author = "Fawx Team"
api_version = "host_api_v1"
capabilities = ["network", "storage"]
entry_point = "run"

[[tools]]
name = "web_search"
description = "Search the web"
authority_surface = "network"
direct_utility = true
trigger_patterns = ["search the web"]

[[tools.parameters]]
name = "query"
type = "string"
description = "Search query"
required = true
        "#;

        let manifest = parse_manifest(toml).expect("Should parse manifest with tools");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "web_search");
        assert_eq!(
            manifest.tools[0].authority_surface,
            Some(SkillToolAuthoritySurface::Network)
        );
        assert!(manifest.tools[0].direct_utility);
        assert_eq!(
            manifest.tools[0].trigger_patterns,
            vec!["search the web".to_string()]
        );
        assert_eq!(manifest.tools[0].parameters.len(), 1);
        assert_eq!(manifest.tools[0].parameters[0].name, "query");
        assert_eq!(manifest.tools[0].parameters[0].kind, "string");
        assert!(manifest.tools[0].parameters[0].required);
    }

    #[test]
    fn test_validate_direct_utility_requires_trigger_patterns() {
        let manifest = SkillManifest {
            name: "weather".to_string(),
            version: "1.0.0".to_string(),
            description: "Weather".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![SkillToolManifest {
                name: "weather".to_string(),
                description: "Weather".to_string(),
                authority_surface: None,
                direct_utility: true,
                trigger_patterns: Vec::new(),
                parameters: vec![SkillToolParameterManifest {
                    name: "location".to_string(),
                    kind: "string".to_string(),
                    description: "Location".to_string(),
                    required: true,
                }],
            }],
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::InvalidManifest(_))));
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
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::InvalidManifest(_))));
    }

    #[test]
    fn v2_manifest_accepted() {
        let manifest = SkillManifest {
            name: "v2_skill".to_string(),
            version: "1.0.0".to_string(),
            description: "A v2 skill".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v2".to_string(),
            capabilities: vec![],
            tools: vec![],
            entry_point: "run".to_string(),
        };
        assert!(validate_manifest(&manifest).is_ok());
    }

    #[test]
    fn test_validate_invalid_api_version() {
        let manifest = SkillManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "Fawx".to_string(),
            api_version: "v2".to_string(),
            capabilities: vec![],
            tools: vec![],
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
author = "Fawx"
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
author = "Fawx"
api_version = "host_api_v1"
capabilities = ["network", "storage", "shell", "filesystem", "notifications", "sensors", "phone_actions"]
        "#;

        let manifest = parse_manifest(toml).expect("Should parse");
        assert_eq!(manifest.capabilities.len(), 7);
        assert!(manifest.capabilities.contains(&Capability::Network));
        assert!(manifest.capabilities.contains(&Capability::Storage));
        assert!(manifest.capabilities.contains(&Capability::Shell));
        assert!(manifest.capabilities.contains(&Capability::Filesystem));
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
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
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
            tools: vec![],
            entry_point: "run".to_string(),
        };

        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn test_validate_name_rejects_path_traversal() {
        let result = validate_skill_name("../evil");

        assert!(result.is_err());
    }

    #[test]
    fn test_capability_parse_uses_shared_capability_list() {
        for capability in ALL_CAPABILITIES {
            assert_eq!(Capability::parse(capability.as_str()), Some(capability));
        }
    }

    #[test]
    fn test_capability_display() {
        assert_eq!(format!("{}", Capability::Network), "network");
        assert_eq!(
            format!(
                "{}",
                Capability::NetworkRestricted {
                    allowed_domains: vec!["api.weather.gov".to_string()],
                }
            ),
            "network_restricted"
        );
        assert_eq!(format!("{}", Capability::Storage), "storage");
        assert_eq!(format!("{}", Capability::Shell), "shell");
        assert_eq!(format!("{}", Capability::Filesystem), "filesystem");
        assert_eq!(format!("{}", Capability::Notifications), "notifications");
        assert_eq!(format!("{}", Capability::Sensors), "sensors");
        assert_eq!(format!("{}", Capability::PhoneActions), "phone_actions");
    }

    #[test]
    fn shell_capability_serializes() {
        let json = serde_json::to_string(&Capability::Shell).expect("serialize shell");
        assert_eq!(json, "\"shell\"");
        let parsed: Capability = serde_json::from_str(&json).expect("deserialize shell");
        assert_eq!(parsed, Capability::Shell);
    }

    #[test]
    fn filesystem_capability_serializes() {
        let json = serde_json::to_string(&Capability::Filesystem).expect("serialize filesystem");
        assert_eq!(json, "\"filesystem\"");
        let parsed: Capability = serde_json::from_str(&json).expect("deserialize filesystem");
        assert_eq!(parsed, Capability::Filesystem);
    }

    #[test]
    fn test_capability_display_in_error_message() {
        let cap = Capability::Network;
        let error_msg = format!("Capability denied: {}", cap);
        assert_eq!(error_msg, "Capability denied: network");
    }

    #[test]
    fn test_validate_valid_semver() {
        let manifest = SkillManifest {
            name: "test".to_string(),
            version: "1.2.3".to_string(),
            description: "Test".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            entry_point: "run".to_string(),
        };

        assert!(validate_manifest(&manifest).is_ok());
    }

    #[test]
    fn test_validate_semver_with_prerelease() {
        let manifest = SkillManifest {
            name: "test".to_string(),
            version: "1.0.0-alpha.1".to_string(),
            description: "Test".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            entry_point: "run".to_string(),
        };

        assert!(validate_manifest(&manifest).is_ok());
    }

    #[test]
    fn test_validate_invalid_semver() {
        let manifest = SkillManifest {
            name: "test".to_string(),
            version: "not-a-version".to_string(),
            description: "Test".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(result.is_err());
        if let Err(SkillError::InvalidManifest(msg)) = result {
            assert!(msg.contains("not a valid semantic version"));
        }
    }

    #[test]
    fn test_validate_incomplete_semver() {
        let manifest = SkillManifest {
            name: "test".to_string(),
            version: "1.0".to_string(),
            description: "Test".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_duplicate_tool_name_rejected() {
        let manifest = SkillManifest {
            name: "browser".to_string(),
            version: "1.0.0".to_string(),
            description: "Browser".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![
                SkillToolManifest {
                    name: "web_search".to_string(),
                    description: "Search".to_string(),
                    authority_surface: None,
                    direct_utility: false,
                    trigger_patterns: Vec::new(),
                    parameters: vec![],
                },
                SkillToolManifest {
                    name: "web_search".to_string(),
                    description: "Duplicate".to_string(),
                    authority_surface: None,
                    direct_utility: false,
                    trigger_patterns: Vec::new(),
                    parameters: vec![],
                },
            ],
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("duplicate tool name"))
        );
    }

    #[test]
    fn test_validate_duplicate_tool_parameter_rejected() {
        let manifest = SkillManifest {
            name: "browser".to_string(),
            version: "1.0.0".to_string(),
            description: "Browser".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![SkillToolManifest {
                name: "web_search".to_string(),
                description: "Search".to_string(),
                authority_surface: None,
                direct_utility: false,
                trigger_patterns: Vec::new(),
                parameters: vec![
                    SkillToolParameterManifest {
                        name: "query".to_string(),
                        kind: "string".to_string(),
                        description: "Search query".to_string(),
                        required: true,
                    },
                    SkillToolParameterManifest {
                        name: "query".to_string(),
                        kind: "string".to_string(),
                        description: "Duplicate".to_string(),
                        required: false,
                    },
                ],
            }],
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("duplicate parameter"))
        );
    }

    #[test]
    #[ignore] // requires skills/ directory present in repo root
    fn migrated_skill_manifests_expose_visible_structured_tools() {
        for skill_dir in ["calculator-skill", "github-skill", "canvas-skill"] {
            let manifest_path = repo_root()
                .join("skills")
                .join(skill_dir)
                .join("manifest.toml");
            let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
            let manifest = parse_manifest(&manifest_text).expect("parse manifest");

            validate_manifest(&manifest).expect("validate manifest");
            assert!(
                !manifest.tools.is_empty(),
                "{skill_dir} should expose manifest tools"
            );
            for tool in &manifest.tools {
                assert!(
                    tool.parameters
                        .iter()
                        .all(|parameter| parameter.name != "input"),
                    "{skill_dir} should expose real structured parameters"
                );
            }
        }
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repo root")
    }
}
