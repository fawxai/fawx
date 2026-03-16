use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use toml_edit::{Array, DocumentMut, Item, Value};

const EDITABLE_CAPABILITIES: [&str; 5] = [
    "network",
    "storage",
    "notifications",
    "sensors",
    "phone_actions",
];

#[derive(Debug)]
pub enum SkillManifestError {
    NotFound(String),
    Invalid(String),
    Internal(String),
}

impl SkillManifestError {
    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(message) | Self::Invalid(message) | Self::Internal(message) => message,
        }
    }
}

pub fn installed_skill_capabilities(
    skills_dir: &Path,
) -> Result<HashMap<String, Vec<String>>, String> {
    if !skills_dir.exists() {
        return Ok(HashMap::new());
    }

    let mut summaries = HashMap::new();
    let entries = fs::read_dir(skills_dir)
        .map_err(|error| format!("failed to read skills directory: {error}"))?;

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to read skill directory entry: {error}"))?;
        if !entry.path().is_dir() {
            continue;
        }

        let manifest_path = entry.path().join("manifest.toml");
        if !manifest_path.is_file() {
            continue;
        }

        let (name, capabilities) = read_manifest_summary(&manifest_path)
            .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
        summaries.insert(name, capabilities);
    }

    Ok(summaries)
}

pub fn update_skill_capabilities(
    skills_dir: &Path,
    skill_name: &str,
    requested_capabilities: &[String],
) -> Result<Vec<String>, SkillManifestError> {
    validate_skill_name(skill_name)?;
    let manifest_path = manifest_path_for_skill_name(skills_dir, skill_name)?;

    let mut document = read_document(&manifest_path)?;
    let existing = read_capabilities(&document).map_err(SkillManifestError::Invalid)?;
    if existing.contains(&"network_restricted".to_string()) {
        return Err(SkillManifestError::Invalid(
            "This skill uses advanced network restrictions and can't be edited in-app yet."
                .to_string(),
        ));
    }

    let normalized = normalize_capabilities(requested_capabilities)?;
    let mut array = Array::new();
    for capability in &normalized {
        array.push(capability.as_str());
    }
    document["capabilities"] = Item::Value(Value::Array(array));

    fs::write(&manifest_path, document.to_string()).map_err(|error| {
        SkillManifestError::Internal(format!("failed to write manifest: {error}"))
    })?;

    Ok(normalized)
}

fn read_manifest_summary(manifest_path: &Path) -> Result<(String, Vec<String>), String> {
    let document = read_document(manifest_path).map_err(|error| match error {
        SkillManifestError::NotFound(message)
        | SkillManifestError::Invalid(message)
        | SkillManifestError::Internal(message) => message,
    })?;

    let name = document["name"]
        .as_str()
        .ok_or_else(|| "manifest missing string field 'name'".to_string())?
        .to_string();
    let capabilities = read_capabilities(&document)?;
    Ok((name, capabilities))
}

fn read_document(path: &Path) -> Result<DocumentMut, SkillManifestError> {
    let contents = fs::read_to_string(path).map_err(|error| {
        SkillManifestError::Internal(format!("failed to read manifest: {error}"))
    })?;
    contents
        .parse::<DocumentMut>()
        .map_err(|error| SkillManifestError::Invalid(format!("invalid manifest TOML: {error}")))
}

fn read_capabilities(document: &DocumentMut) -> Result<Vec<String>, String> {
    let Some(item) = document.as_table().get("capabilities") else {
        return Ok(Vec::new());
    };

    let Some(array) = item.as_array() else {
        return Err("manifest field 'capabilities' must be an array".to_string());
    };

    let mut capabilities = Vec::new();
    for value in array.iter() {
        let Some(raw) = value.as_str() else {
            return Err(
                "manifest contains advanced capability entries that are not editable in-app yet"
                    .to_string(),
            );
        };
        capabilities.push(raw.to_string());
    }

    Ok(capabilities)
}

fn normalize_capabilities(
    requested_capabilities: &[String],
) -> Result<Vec<String>, SkillManifestError> {
    let normalized: Vec<String> = requested_capabilities
        .iter()
        .map(|capability| capability.trim().to_string())
        .filter(|capability| !capability.is_empty())
        .collect();

    let requested_set: HashSet<&str> = normalized.iter().map(String::as_str).collect();
    if let Some(unknown) = requested_set
        .iter()
        .find(|capability| !EDITABLE_CAPABILITIES.contains(capability))
    {
        return Err(SkillManifestError::Invalid(format!(
            "unknown skill capability '{}'",
            unknown
        )));
    }

    Ok(EDITABLE_CAPABILITIES
        .iter()
        .filter(|capability| requested_set.contains(**capability))
        .map(|capability| capability.to_string())
        .collect())
}

fn validate_skill_name(skill_name: &str) -> Result<(), SkillManifestError> {
    if skill_name.trim().is_empty()
        || skill_name.contains('/')
        || skill_name.contains('\\')
        || skill_name.contains("..")
    {
        return Err(SkillManifestError::Invalid(
            "invalid skill name".to_string(),
        ));
    }
    Ok(())
}

fn manifest_path_for_skill_name(
    skills_dir: &Path,
    skill_name: &str,
) -> Result<std::path::PathBuf, SkillManifestError> {
    if !skills_dir.exists() {
        return Err(SkillManifestError::NotFound(format!(
            "Skill '{skill_name}' not found"
        )));
    }

    let entries = fs::read_dir(skills_dir).map_err(|error| {
        SkillManifestError::Internal(format!("failed to read skills directory: {error}"))
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            SkillManifestError::Internal(format!("failed to read skill directory entry: {error}"))
        })?;
        if !entry.path().is_dir() {
            continue;
        }

        let manifest_path = entry.path().join("manifest.toml");
        if !manifest_path.is_file() {
            continue;
        }

        let Ok((manifest_name, _)) = read_manifest_summary(&manifest_path) else {
            continue;
        };

        if manifest_name == skill_name {
            return Ok(manifest_path);
        }
    }

    Err(SkillManifestError::NotFound(format!(
        "Skill '{skill_name}' not found"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_manifest(temp_dir: &TempDir, skill_name: &str, contents: &str) {
        let skill_dir = temp_dir.path().join(skill_name);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(skill_dir.join("manifest.toml"), contents).expect("write manifest");
    }

    #[test]
    fn installed_skill_capabilities_reads_string_arrays() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_manifest(
            &temp_dir,
            "weather",
            r#"
name = "weather"
version = "1.0.0"
description = "Weather"
author = "Fawx"
api_version = "host_api_v1"
capabilities = ["network", "notifications"]
"#,
        );

        let capabilities = installed_skill_capabilities(temp_dir.path()).expect("read skills");
        assert_eq!(
            capabilities.get("weather"),
            Some(&vec!["network".to_string(), "notifications".to_string()])
        );
    }

    #[test]
    fn update_skill_capabilities_rewrites_manifest_array_in_canonical_order() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_manifest(
            &temp_dir,
            "weather-skill",
            r#"
name = "weather"
version = "1.0.0"
description = "Weather"
author = "Fawx"
api_version = "host_api_v1"
capabilities = ["notifications"]
"#,
        );

        let updated = update_skill_capabilities(
            temp_dir.path(),
            "weather",
            &[
                "notifications".to_string(),
                "network".to_string(),
                "network".to_string(),
            ],
        )
        .expect("update permissions");

        assert_eq!(
            updated,
            vec!["network".to_string(), "notifications".to_string()]
        );

        let manifest =
            fs::read_to_string(temp_dir.path().join("weather-skill").join("manifest.toml"))
                .expect("manifest");
        assert!(manifest.contains(r#"capabilities = ["network", "notifications"]"#));
    }

    #[test]
    fn update_skill_capabilities_rejects_unknown_capabilities() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_manifest(
            &temp_dir,
            "weather",
            r#"
name = "weather"
version = "1.0.0"
description = "Weather"
author = "Fawx"
api_version = "host_api_v1"
capabilities = []
"#,
        );

        let error =
            update_skill_capabilities(temp_dir.path(), "weather", &["telepathy".to_string()])
                .expect_err("should reject unknown capability");

        assert_eq!(error.message(), "unknown skill capability 'telepathy'");
    }
}
