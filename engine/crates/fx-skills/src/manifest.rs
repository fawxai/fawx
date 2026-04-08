//! Skill manifest parsing and validation.

use fx_core::error::SkillError;
use fx_core::tool_routing::{RouteAuthMode, ToolRoutingMetadata};
use regex::Regex;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillSettingFieldType {
    Text,
    Secret,
    Boolean,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSettingsManifest {
    #[serde(default = "default_settings_version")]
    pub version: u32,
    #[serde(default)]
    pub fields: Vec<SkillSettingFieldManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSettingFieldManifest {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: SkillSettingFieldType,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub help_text: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub min_length: Option<usize>,
    #[serde(default)]
    pub max_length: Option<usize>,
    /// Optional server-side Rust `regex` pattern. The server is authoritative
    /// for pattern validation because client regex engines may differ.
    #[serde(default)]
    pub pattern: Option<String>,
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
    /// Optional skill-level intent hints for future routing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intent_hints: Vec<String>,
    /// Optional user-configurable settings exposed through shell UIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<SkillSettingsManifest>,
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
    pub routing: Option<ToolRoutingMetadata>,
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

const fn default_settings_version() -> u32 {
    1
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

/// Validate that every string entry in a manifest field is non-blank after trimming.
pub fn validate_nonblank_string_entries(
    field_name: &str,
    values: &[String],
) -> Result<(), SkillError> {
    if let Some((index, _)) = values
        .iter()
        .enumerate()
        .find(|(_, value)| value.trim().is_empty())
    {
        return Err(SkillError::InvalidManifest(format!(
            "{}[{}] cannot be blank",
            field_name, index
        )));
    }

    Ok(())
}

const MAX_INTENT_HINTS: usize = 64;
const MAX_INTENT_HINT_LENGTH: usize = 256;

pub type CompiledSettingPatterns = BTreeMap<String, Regex>;

/// Validate `intent_hints` for blank, duplicate, and unbounded entries.
pub fn validate_intent_hints(intent_hints: &[String]) -> Result<(), SkillError> {
    if intent_hints.len() > MAX_INTENT_HINTS {
        return Err(SkillError::InvalidManifest(format!(
            "intent_hints cannot contain more than {} entries",
            MAX_INTENT_HINTS
        )));
    }

    validate_nonblank_string_entries("intent_hints", intent_hints)?;

    let mut seen = std::collections::BTreeSet::new();
    for (index, intent_hint) in intent_hints.iter().enumerate() {
        if intent_hint.chars().count() > MAX_INTENT_HINT_LENGTH {
            return Err(SkillError::InvalidManifest(format!(
                "intent_hints[{index}] cannot exceed {} characters",
                MAX_INTENT_HINT_LENGTH
            )));
        }

        if !seen.insert(intent_hint.as_str()) {
            return Err(SkillError::InvalidManifest(format!(
                "intent_hints[{index}] duplicates a previous entry"
            )));
        }
    }

    Ok(())
}

pub fn compile_setting_patterns(
    fields: &[SkillSettingFieldManifest],
) -> Result<CompiledSettingPatterns, SkillError> {
    let mut compiled = BTreeMap::new();

    for field in fields {
        let Some(pattern) = field.pattern.as_deref() else {
            continue;
        };

        let regex = Regex::new(pattern).map_err(|error| {
            SkillError::InvalidManifest(format!(
                "settings field '{}' has invalid pattern: {}",
                field.key, error
            ))
        })?;
        compiled.insert(field.key.clone(), regex);
    }

    Ok(compiled)
}

pub fn validate_setting_value(
    field: &SkillSettingFieldManifest,
    value: Option<&str>,
    compiled_pattern: Option<&Regex>,
) -> Result<(), SkillError> {
    if field.required && value.is_none() {
        return Err(SkillError::InvalidManifest(format!(
            "{} is required.",
            field.label
        )));
    }

    let Some(value) = value else {
        return Ok(());
    };

    if matches!(field.field_type, SkillSettingFieldType::Boolean)
        && value != "true"
        && value != "false"
    {
        return Err(SkillError::InvalidManifest(format!(
            "{} must be either 'true' or 'false'",
            field.label
        )));
    }

    let length = value.chars().count();

    if let Some(min_length) = field.min_length {
        if length < min_length {
            return Err(SkillError::InvalidManifest(format!(
                "{} must be at least {} characters.",
                field.label, min_length
            )));
        }
    }

    if let Some(max_length) = field.max_length {
        if length > max_length {
            return Err(SkillError::InvalidManifest(format!(
                "{} must be at most {} characters.",
                field.label, max_length
            )));
        }
    }

    if let Some(regex) = compiled_pattern {
        if !regex.is_match(value) {
            return Err(SkillError::InvalidManifest(format!(
                "{} is invalid.",
                field.label
            )));
        }
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

    validate_intent_hints(&manifest.intent_hints)?;
    validate_settings(manifest.settings.as_ref())?;
    validate_tools(&manifest.tools)?;

    Ok(())
}

fn validate_settings(settings: Option<&SkillSettingsManifest>) -> Result<(), SkillError> {
    let Some(settings) = settings else {
        return Ok(());
    };

    if settings.version != default_settings_version() {
        return Err(SkillError::InvalidManifest(format!(
            "unsupported settings version '{}'",
            settings.version
        )));
    }

    let mut seen_field_keys = BTreeSet::new();
    for (index, field) in settings.fields.iter().enumerate() {
        if field.key.trim().is_empty() {
            return Err(SkillError::InvalidManifest(format!(
                "settings.fields[{index}].key cannot be empty"
            )));
        }

        if field.key.contains("..") || field.key.contains('/') || field.key.contains('\\') {
            return Err(SkillError::InvalidManifest(format!(
                "settings.fields[{index}].key must not contain path separators or '..'"
            )));
        }

        if !seen_field_keys.insert(field.key.as_str()) {
            return Err(SkillError::InvalidManifest(format!(
                "settings.fields[{index}] duplicates key '{}'",
                field.key
            )));
        }

        if field.label.trim().is_empty() {
            return Err(SkillError::InvalidManifest(format!(
                "settings.fields[{index}].label cannot be empty"
            )));
        }

        if matches!(field.field_type, SkillSettingFieldType::Boolean)
            && (field.min_length.is_some() || field.max_length.is_some() || field.pattern.is_some())
        {
            return Err(SkillError::InvalidManifest(format!(
                "settings field '{}' cannot use min_length, max_length, or pattern with boolean type",
                field.key
            )));
        }

        if let (Some(min_length), Some(max_length)) = (field.min_length, field.max_length) {
            if min_length > max_length {
                return Err(SkillError::InvalidManifest(format!(
                    "settings field '{}' has min_length greater than max_length",
                    field.key
                )));
            }
        }

        if let Some(pattern) = &field.pattern {
            if pattern.trim().is_empty() {
                return Err(SkillError::InvalidManifest(format!(
                    "settings field '{}' pattern cannot be empty",
                    field.key
                )));
            }
        }
    }

    let _ = compile_setting_patterns(&settings.fields)?;

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
        if let Some(routing) = &tool.routing {
            validate_tool_routing(&tool.name, routing)?;
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

fn validate_tool_routing(tool_name: &str, routing: &ToolRoutingMetadata) -> Result<(), SkillError> {
    if routing.resource_kinds.is_empty() {
        return Err(SkillError::InvalidManifest(format!(
            "tool '{}' routing.resource_kinds cannot be empty",
            tool_name
        )));
    }

    if routing.operations.is_empty() {
        return Err(SkillError::InvalidManifest(format!(
            "tool '{}' routing.operations cannot be empty",
            tool_name
        )));
    }

    if let RouteAuthMode::CredentialRequired { key } = &routing.auth_mode {
        if key.trim().is_empty() {
            return Err(SkillError::InvalidManifest(format!(
                "tool '{}' routing.auth_mode credential key cannot be blank",
                tool_name
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn brave_search_settings_manifest_toml(fields_toml: &str) -> String {
        format!(
            r#"
name = "brave-search"
version = "1.0.0"
description = "Search the web"
author = "Fawx Team"
api_version = "host_api_v1"

[settings]
version = 1

{fields_toml}
"#
        )
    }

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
        assert!(manifest.intent_hints.is_empty());
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
        assert_eq!(manifest.tools[0].routing, None);
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
    fn test_parse_manifest_with_settings() {
        let toml = brave_search_settings_manifest_toml(
            r#"
[[settings.fields]]
key = "api_key"
label = "API Key"
type = "secret"
required = true
min_length = 8
max_length = 128

[[settings.fields]]
key = "safesearch"
label = "Safe Search"
type = "boolean"
help_text = "Enable family-safe results."
"#,
        );

        let manifest = parse_manifest(&toml).expect("parse manifest");
        let settings = manifest.settings.expect("settings");
        assert_eq!(settings.version, 1);
        assert_eq!(settings.fields.len(), 2);
        assert_eq!(settings.fields[0].key, "api_key");
        assert_eq!(settings.fields[0].field_type, SkillSettingFieldType::Secret);
        assert!(settings.fields[0].required);
        assert_eq!(settings.fields[0].min_length, Some(8));
        assert_eq!(settings.fields[0].max_length, Some(128));
        assert_eq!(
            settings.fields[1].field_type,
            SkillSettingFieldType::Boolean
        );
    }

    #[test]
    fn test_parse_manifest_with_routing_metadata() {
        let toml = r#"
name = "browser"
version = "1.0.0"
description = "Browser skill"
author = "Fawx Team"
api_version = "host_api_v1"
entry_point = "run"

[[tools]]
name = "web_fetch"
description = "Fetch a web page"
routing = { resource_kinds = ["generic_url"], operations = ["fetch"], auth_mode = { kind = "none" }, artifact_strategy = "direct_fetch", fallback_rank = 100 }
"#;

        let manifest = parse_manifest(toml).expect("parse manifest");
        let routing = manifest.tools[0].routing.clone().expect("routing");

        assert_eq!(
            routing.resource_kinds,
            vec![fx_core::tool_routing::ResourceKind::GenericUrl]
        );
        assert_eq!(
            routing.operations,
            vec![fx_core::tool_routing::RouteOperation::Fetch]
        );
        assert_eq!(routing.auth_mode, RouteAuthMode::None);
        assert_eq!(
            routing.artifact_strategy,
            fx_core::tool_routing::ArtifactStrategy::DirectFetch
        );
        assert_eq!(routing.fallback_rank, 100);
    }

    #[test]
    fn test_validate_rejects_duplicate_settings_keys() {
        let manifest = SkillManifest {
            name: "brave-search".to_string(),
            version: "1.0.0".to_string(),
            description: "Search the web".to_string(),
            author: "Fawx Team".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            intent_hints: vec![],
            settings: Some(SkillSettingsManifest {
                version: 1,
                fields: vec![
                    SkillSettingFieldManifest {
                        key: "api_key".to_string(),
                        label: "API Key".to_string(),
                        field_type: SkillSettingFieldType::Secret,
                        placeholder: None,
                        help_text: None,
                        required: true,
                        min_length: Some(8),
                        max_length: None,
                        pattern: None,
                    },
                    SkillSettingFieldManifest {
                        key: "api_key".to_string(),
                        label: "Replacement API Key".to_string(),
                        field_type: SkillSettingFieldType::Secret,
                        placeholder: None,
                        help_text: None,
                        required: false,
                        min_length: None,
                        max_length: None,
                        pattern: None,
                    },
                ],
            }),
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("duplicates key"))
        );
    }

    #[test]
    fn test_validate_rejects_invalid_settings_pattern() {
        let manifest = SkillManifest {
            name: "brave-search".to_string(),
            version: "1.0.0".to_string(),
            description: "Search the web".to_string(),
            author: "Fawx Team".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            intent_hints: vec![],
            settings: Some(SkillSettingsManifest {
                version: 1,
                fields: vec![SkillSettingFieldManifest {
                    key: "region".to_string(),
                    label: "Region".to_string(),
                    field_type: SkillSettingFieldType::Text,
                    placeholder: None,
                    help_text: None,
                    required: false,
                    min_length: None,
                    max_length: None,
                    pattern: Some("[".to_string()),
                }],
            }),
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("invalid pattern"))
        );
    }

    #[test]
    fn test_validate_rejects_inverted_settings_length_bounds() {
        let manifest = SkillManifest {
            name: "brave-search".to_string(),
            version: "1.0.0".to_string(),
            description: "Search the web".to_string(),
            author: "Fawx Team".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            intent_hints: vec![],
            settings: Some(SkillSettingsManifest {
                version: 1,
                fields: vec![SkillSettingFieldManifest {
                    key: "region".to_string(),
                    label: "Region".to_string(),
                    field_type: SkillSettingFieldType::Text,
                    placeholder: None,
                    help_text: None,
                    required: false,
                    min_length: Some(8),
                    max_length: Some(4),
                    pattern: None,
                }],
            }),
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("min_length greater than max_length"))
        );
    }

    #[test]
    fn test_validate_setting_value_enforces_max_length() {
        let field = SkillSettingFieldManifest {
            key: "region".to_string(),
            label: "Region".to_string(),
            field_type: SkillSettingFieldType::Text,
            placeholder: None,
            help_text: None,
            required: false,
            min_length: None,
            max_length: Some(4),
            pattern: None,
        };

        let result = validate_setting_value(&field, Some("global"), None);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("at most 4 characters"))
        );
    }

    #[test]
    fn test_parse_manifest_with_intent_hints_round_trips() {
        let manifest = SkillManifest {
            name: "review-helper".to_string(),
            version: "1.0.0".to_string(),
            description: "Review helper".to_string(),
            author: "Fawx Team".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            intent_hints: vec![
                "review pr".to_string(),
                "github issue management".to_string(),
            ],
            settings: None,
            entry_point: "run".to_string(),
        };

        let toml = toml::to_string(&manifest).expect("serialize manifest");
        let parsed = parse_manifest(&toml).expect("parse manifest");

        assert_eq!(parsed.name, manifest.name);
        assert_eq!(parsed.version, manifest.version);
        assert_eq!(parsed.description, manifest.description);
        assert_eq!(parsed.author, manifest.author);
        assert_eq!(parsed.api_version, manifest.api_version);
        assert_eq!(parsed.intent_hints, manifest.intent_hints);
    }

    #[test]
    fn test_parse_manifest_defaults_intent_hints_to_empty() {
        let toml = r#"
name = "review-helper"
version = "1.0.0"
description = "Review helper"
author = "Fawx Team"
api_version = "host_api_v1"
        "#;

        let manifest = parse_manifest(toml).expect("parse manifest");
        assert!(manifest.intent_hints.is_empty());
    }

    #[test]
    fn test_validate_rejects_duplicate_intent_hints() {
        let manifest = SkillManifest {
            name: "review-helper".to_string(),
            version: "1.0.0".to_string(),
            description: "Review helper".to_string(),
            author: "Fawx Team".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            intent_hints: vec!["review pr".to_string(), "review pr".to_string()],
            settings: None,
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("duplicates a previous entry"))
        );
    }

    #[test]
    fn test_validate_rejects_too_many_intent_hints() {
        let manifest = SkillManifest {
            name: "review-helper".to_string(),
            version: "1.0.0".to_string(),
            description: "Review helper".to_string(),
            author: "Fawx Team".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            intent_hints: (0..=MAX_INTENT_HINTS)
                .map(|index| format!("hint-{index}"))
                .collect(),
            settings: None,
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("more than 64 entries"))
        );
    }

    #[test]
    fn test_validate_rejects_overlong_intent_hint() {
        let manifest = SkillManifest {
            name: "review-helper".to_string(),
            version: "1.0.0".to_string(),
            description: "Review helper".to_string(),
            author: "Fawx Team".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            intent_hints: vec![
                "review pr".to_string(),
                "a".repeat(MAX_INTENT_HINT_LENGTH + 1),
            ],
            settings: None,
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("cannot exceed 256 characters"))
        );
    }

    #[test]
    fn test_validate_rejects_blank_intent_hints() {
        let manifest = SkillManifest {
            name: "review-helper".to_string(),
            version: "1.0.0".to_string(),
            description: "Review helper".to_string(),
            author: "Fawx Team".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![],
            intent_hints: vec!["review pr".to_string(), "   ".to_string()],
            settings: None,
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("intent_hints[1]"))
        );
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
                routing: None,
                direct_utility: true,
                trigger_patterns: vec![],
                parameters: vec![SkillToolParameterManifest {
                    name: "location".to_string(),
                    kind: "string".to_string(),
                    description: "Location".to_string(),
                    required: true,
                }],
            }],
            intent_hints: vec![],
            settings: None,
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(result.is_err());
        assert!(matches!(result, Err(SkillError::InvalidManifest(_))));
    }

    #[test]
    fn test_validate_rejects_empty_tool_routing_resource_kinds() {
        let manifest = SkillManifest {
            name: "browser".to_string(),
            version: "1.0.0".to_string(),
            description: "Browser".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![SkillToolManifest {
                name: "web_fetch".to_string(),
                description: "Fetch".to_string(),
                authority_surface: Some(SkillToolAuthoritySurface::Network),
                routing: Some(ToolRoutingMetadata {
                    resource_kinds: Vec::new(),
                    operations: vec![fx_core::tool_routing::RouteOperation::Fetch],
                    auth_mode: RouteAuthMode::None,
                    artifact_strategy: fx_core::tool_routing::ArtifactStrategy::DirectFetch,
                    fallback_rank: 100,
                }),
                direct_utility: false,
                trigger_patterns: vec![],
                parameters: vec![],
            }],
            intent_hints: vec![],
            settings: None,
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("routing.resource_kinds cannot be empty"))
        );
    }

    #[test]
    fn test_validate_rejects_empty_tool_routing_operations() {
        let manifest = SkillManifest {
            name: "browser".to_string(),
            version: "1.0.0".to_string(),
            description: "Browser".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![SkillToolManifest {
                name: "web_fetch".to_string(),
                description: "Fetch".to_string(),
                authority_surface: Some(SkillToolAuthoritySurface::Network),
                routing: Some(ToolRoutingMetadata {
                    resource_kinds: vec![fx_core::tool_routing::ResourceKind::GenericUrl],
                    operations: Vec::new(),
                    auth_mode: RouteAuthMode::None,
                    artifact_strategy: fx_core::tool_routing::ArtifactStrategy::DirectFetch,
                    fallback_rank: 100,
                }),
                direct_utility: false,
                trigger_patterns: vec![],
                parameters: vec![],
            }],
            intent_hints: vec![],
            settings: None,
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("routing.operations cannot be empty"))
        );
    }

    #[test]
    fn test_validate_rejects_blank_tool_routing_credential_key() {
        let manifest = SkillManifest {
            name: "github".to_string(),
            version: "1.0.0".to_string(),
            description: "GitHub".to_string(),
            author: "Fawx".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            tools: vec![SkillToolManifest {
                name: "view_pr".to_string(),
                description: "View pull request".to_string(),
                authority_surface: Some(SkillToolAuthoritySurface::Network),
                routing: Some(ToolRoutingMetadata {
                    resource_kinds: vec![fx_core::tool_routing::ResourceKind::GitHubPullRequest],
                    operations: vec![fx_core::tool_routing::RouteOperation::Fetch],
                    auth_mode: RouteAuthMode::CredentialRequired {
                        key: "   ".to_string(),
                    },
                    artifact_strategy: fx_core::tool_routing::ArtifactStrategy::DirectFetch,
                    fallback_rank: 10,
                }),
                direct_utility: false,
                trigger_patterns: vec![],
                parameters: vec![],
            }],
            intent_hints: vec![],
            settings: None,
            entry_point: "run".to_string(),
        };

        let result = validate_manifest(&manifest);
        assert!(
            matches!(result, Err(SkillError::InvalidManifest(message)) if message.contains("credential key cannot be blank"))
        );
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
            intent_hints: vec![],
            settings: None,
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
            intent_hints: vec![],
            settings: None,
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
            intent_hints: vec![],
            settings: None,
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
            intent_hints: vec![],
            settings: None,
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
            intent_hints: vec![],
            settings: None,
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
            intent_hints: vec![],
            settings: None,
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
            intent_hints: vec![],
            settings: None,
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
            intent_hints: vec![],
            settings: None,
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
            intent_hints: vec![],
            settings: None,
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
                    routing: None,
                    direct_utility: false,
                    trigger_patterns: vec![],
                    parameters: vec![],
                },
                SkillToolManifest {
                    name: "web_search".to_string(),
                    description: "Duplicate".to_string(),
                    authority_surface: None,
                    routing: None,
                    direct_utility: false,
                    trigger_patterns: vec![],
                    parameters: vec![],
                },
            ],
            intent_hints: vec![],
            settings: None,
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
                routing: None,
                direct_utility: false,
                trigger_patterns: vec![],
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
            intent_hints: vec![],
            settings: None,
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
