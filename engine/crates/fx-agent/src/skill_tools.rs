//! Convert WASM skills into Claude tool definitions.

use crate::claude::types::Tool;
use fx_skills::{SkillManifest, SkillRegistry};
use serde_json::json;

/// Get all available skill tools from the registry.
pub fn skill_tools() -> Vec<Tool> {
    let registry = match SkillRegistry::new() {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to create skill registry: {}", e);
            return vec![];
        }
    };

    let manifests = match registry.list_manifests() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Failed to list skills: {}", e);
            return vec![];
        }
    };

    manifests.iter().map(skill_to_tool).collect()
}

/// Convert a skill manifest into a Claude Tool definition.
fn skill_to_tool(manifest: &SkillManifest) -> Tool {
    // Build description with capabilities info
    let mut description = manifest.description.clone();

    if !manifest.capabilities.is_empty() {
        let caps: Vec<String> = manifest
            .capabilities
            .iter()
            .map(|c| c.to_string())
            .collect();
        description.push_str(&format!(" (requires: {})", caps.join(", ")));
    }

    // Skills accept a JSON input parameter
    let schema = json!({
        "type": "object",
        "properties": {
            "input": {
                "type": "string",
                "description": "JSON input for the skill (skill-specific format)"
            }
        },
        "required": ["input"]
    });

    Tool::new(format!("skill_{}", manifest.name), &description, schema)
}

/// Format skill description for planning context.
pub fn format_skills_context() -> String {
    let registry = match SkillRegistry::new() {
        Ok(r) => r,
        Err(_) => return String::new(),
    };

    let manifests = match registry.list_manifests() {
        Ok(m) => m,
        Err(_) => return String::new(),
    };

    if manifests.is_empty() {
        return String::new();
    }

    let mut context = String::from("\n\nAvailable Skills:\n");

    for manifest in manifests {
        context.push_str(&format!("- {}: {}\n", manifest.name, manifest.description));

        if !manifest.capabilities.is_empty() {
            let caps: Vec<String> = manifest
                .capabilities
                .iter()
                .map(|c| c.to_string())
                .collect();
            context.push_str(&format!("  Capabilities: {}\n", caps.join(", ")));
        }
    }

    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_skills::manifest::Capability;

    fn create_test_manifest(name: &str, description: &str) -> SkillManifest {
        SkillManifest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: description.to_string(),
            author: "Test".to_string(),
            api_version: "host_api_v1".to_string(),
            capabilities: vec![],
            entry_point: "run".to_string(),
        }
    }

    #[test]
    fn test_skill_to_tool() {
        let manifest = create_test_manifest("weather", "Get weather for a location");
        let tool = skill_to_tool(&manifest);

        assert_eq!(tool.name, "skill_weather");
        assert!(tool.description.contains("Get weather"));

        let schema = tool.input_schema.as_object().expect("Should be object");
        assert_eq!(schema["type"], "object");

        let props = schema["properties"]
            .as_object()
            .expect("Should have properties");
        assert!(props.contains_key("input"));
    }

    #[test]
    fn test_skill_to_tool_with_capabilities() {
        let mut manifest = create_test_manifest("weather", "Get weather info");
        manifest.capabilities = vec![Capability::Network];

        let tool = skill_to_tool(&manifest);
        assert!(tool.description.contains("requires: network"));
    }

    #[test]
    fn test_skill_to_tool_multiple_capabilities() {
        let mut manifest = create_test_manifest("complex", "Complex skill");
        manifest.capabilities = vec![
            Capability::Network,
            Capability::Storage,
            Capability::Notifications,
        ];

        let tool = skill_to_tool(&manifest);
        assert!(tool.description.contains("network"));
        assert!(tool.description.contains("storage"));
        assert!(tool.description.contains("notifications"));
    }

    #[test]
    fn test_format_empty_skills_context() {
        // Without any installed skills, should return empty
        // This test assumes no skills are installed in test environment
        // In practice, we'd need to mock the registry
        let context = format_skills_context();
        // Context will either be empty or contain skills if any are installed
        // Just verify it doesn't panic
        assert!(context.is_empty() || context.contains("Available Skills"));
    }
}
