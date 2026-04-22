use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use fx_auth::credential_store::EncryptedFileCredentialStore;
use fx_skills::manifest::{
    compile_setting_patterns, parse_manifest, validate_manifest,
    validate_setting_value as validate_manifest_setting_value,
    validate_skill_name as validate_manifest_skill_name, SkillManifest, SkillSettingFieldManifest,
    SkillSettingFieldType, SkillSettingsManifest,
};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkillSettingValue {
    pub key: String,
    pub value: Option<String>,
    pub is_secret: bool,
    pub is_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkillSettings {
    pub skill_name: String,
    pub schema: SkillSettingsManifest,
    pub values: Vec<ResolvedSkillSettingValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSettingUpdate {
    pub key: String,
    pub value: Option<String>,
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

pub fn load_skill_settings(
    data_dir: &Path,
    skill_name: &str,
) -> Result<Option<ResolvedSkillSettings>, SkillManifestError> {
    validate_skill_name(skill_name)?;
    let manifest_path = manifest_path_for_skill_name(&data_dir.join("skills"), skill_name)?;
    let manifest = read_skill_manifest(&manifest_path)?;
    let Some(schema) = manifest.settings.clone() else {
        return Ok(None);
    };

    let store = open_skill_store(data_dir)?;
    let values = schema
        .fields
        .iter()
        .map(|field| {
            let stored = read_skill_setting(&store, skill_name, &field.key)?;
            let is_secret = matches!(field.field_type, SkillSettingFieldType::Secret);
            Ok(ResolvedSkillSettingValue {
                key: field.key.clone(),
                value: if is_secret { None } else { stored.clone() },
                is_secret,
                is_configured: stored.is_some(),
            })
        })
        .collect::<Result<Vec<_>, SkillManifestError>>()?;

    Ok(Some(ResolvedSkillSettings {
        skill_name: skill_name.to_string(),
        schema,
        values,
    }))
}

pub fn update_skill_settings(
    data_dir: &Path,
    skill_name: &str,
    updates: &[SkillSettingUpdate],
) -> Result<ResolvedSkillSettings, SkillManifestError> {
    validate_skill_name(skill_name)?;
    let manifest_path = manifest_path_for_skill_name(&data_dir.join("skills"), skill_name)?;
    let manifest = read_skill_manifest(&manifest_path)?;
    let schema = manifest.settings.clone().ok_or_else(|| {
        SkillManifestError::Invalid(format!(
            "Skill '{skill_name}' does not declare any settings"
        ))
    })?;
    let store = open_skill_store(data_dir)?;
    let existing_values = load_existing_setting_values(&store, skill_name, &schema.fields)?;
    let normalized_updates = normalize_updates(&schema.fields, updates)?;
    validate_merged_settings(&schema.fields, &existing_values, &normalized_updates)?;

    for field in &schema.fields {
        let Some(value) = normalized_updates.get(field.key.as_str()) else {
            continue;
        };

        if let Some(value) = value {
            store_skill_setting(&store, skill_name, &field.key, value)?;
        } else {
            clear_skill_setting(&store, skill_name, &field.key)?;
        }
    }

    drop(store);

    load_skill_settings(data_dir, skill_name)?.ok_or_else(|| {
        SkillManifestError::Internal(format!(
            "Skill '{skill_name}' settings disappeared after update"
        ))
    })
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

fn read_skill_manifest(manifest_path: &Path) -> Result<SkillManifest, SkillManifestError> {
    let contents = fs::read_to_string(manifest_path).map_err(|error| {
        SkillManifestError::Internal(format!("failed to read manifest: {error}"))
    })?;
    let manifest = parse_manifest(&contents)
        .map_err(|error| SkillManifestError::Invalid(error.to_string()))?;
    validate_manifest(&manifest).map_err(|error| SkillManifestError::Invalid(error.to_string()))?;
    Ok(manifest)
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

fn open_skill_store(data_dir: &Path) -> Result<EncryptedFileCredentialStore, SkillManifestError> {
    EncryptedFileCredentialStore::open(data_dir).map_err(|error| {
        SkillManifestError::Internal(format!("failed to open skill settings store: {error}"))
    })
}

fn load_existing_setting_values(
    store: &EncryptedFileCredentialStore,
    skill_name: &str,
    fields: &[SkillSettingFieldManifest],
) -> Result<HashMap<String, Option<String>>, SkillManifestError> {
    fields
        .iter()
        .map(|field| {
            read_skill_setting(store, skill_name, &field.key)
                .map(|value| (field.key.clone(), value))
        })
        .collect()
}

fn normalize_updates(
    fields: &[SkillSettingFieldManifest],
    updates: &[SkillSettingUpdate],
) -> Result<HashMap<String, Option<String>>, SkillManifestError> {
    let known_keys: HashSet<&str> = fields.iter().map(|field| field.key.as_str()).collect();
    let mut normalized = HashMap::new();

    for update in updates {
        if !known_keys.contains(update.key.as_str()) {
            return Err(SkillManifestError::Invalid(format!(
                "unknown skill setting '{}'",
                update.key
            )));
        }

        if normalized.contains_key(update.key.as_str()) {
            return Err(SkillManifestError::Invalid(format!(
                "duplicate skill setting '{}'",
                update.key
            )));
        }

        let field = fields
            .iter()
            .find(|field| field.key == update.key)
            .ok_or_else(|| {
                SkillManifestError::Invalid(format!("unknown skill setting '{}'", update.key))
            })?;
        normalized.insert(
            update.key.clone(),
            normalize_field_value(field, update.value.as_deref())?,
        );
    }

    Ok(normalized)
}

fn normalize_field_value(
    field: &SkillSettingFieldManifest,
    raw_value: Option<&str>,
) -> Result<Option<String>, SkillManifestError> {
    match field.field_type {
        SkillSettingFieldType::Boolean => match raw_value {
            None => Ok(None),
            Some(value) => {
                let normalized = value.trim().to_ascii_lowercase();
                match normalized.as_str() {
                    "true" | "false" => Ok(Some(normalized)),
                    _ => Err(SkillManifestError::Invalid(format!(
                        "{} must be either 'true' or 'false'",
                        field.label
                    ))),
                }
            }
        },
        SkillSettingFieldType::Text | SkillSettingFieldType::Secret => Ok(raw_value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)),
    }
}

fn validate_merged_settings(
    fields: &[SkillSettingFieldManifest],
    existing_values: &HashMap<String, Option<String>>,
    updates: &HashMap<String, Option<String>>,
) -> Result<(), SkillManifestError> {
    let compiled_patterns = compile_setting_patterns(fields)
        .map_err(|error| SkillManifestError::Invalid(error.to_string()))?;

    for field in fields {
        let merged = if let Some(value) = updates.get(field.key.as_str()) {
            value.clone()
        } else {
            existing_values.get(field.key.as_str()).cloned().flatten()
        };
        validate_manifest_setting_value(
            field,
            merged.as_deref(),
            compiled_patterns.get(field.key.as_str()),
        )
        .map_err(|error| SkillManifestError::Invalid(error.to_string()))?;
    }

    Ok(())
}

fn read_skill_setting(
    store: &EncryptedFileCredentialStore,
    skill_name: &str,
    key: &str,
) -> Result<Option<String>, SkillManifestError> {
    store
        .get_generic(&skill_setting_store_key(skill_name, key))
        .map(|value| value.map(|value| value.to_string()))
        .map_err(|error| {
            SkillManifestError::Internal(format!("failed to read stored skill setting: {error}"))
        })
}

fn store_skill_setting(
    store: &EncryptedFileCredentialStore,
    skill_name: &str,
    key: &str,
    value: &str,
) -> Result<(), SkillManifestError> {
    store
        .set_generic(&skill_setting_store_key(skill_name, key), value)
        .map_err(|error| {
            SkillManifestError::Internal(format!("failed to store skill setting: {error}"))
        })
}

fn clear_skill_setting(
    store: &EncryptedFileCredentialStore,
    skill_name: &str,
    key: &str,
) -> Result<(), SkillManifestError> {
    store
        .clear_generic(&skill_setting_store_key(skill_name, key))
        .map(|_| ())
        .map_err(|error| {
            SkillManifestError::Internal(format!("failed to clear skill setting: {error}"))
        })
}

pub fn skill_setting_store_key(skill_name: &str, key: &str) -> String {
    format!("skill:{skill_name}:{key}")
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
    validate_manifest_skill_name(skill_name)
        .map_err(|error| SkillManifestError::Invalid(error.to_string()))
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

    fn brave_search_settings_manifest_toml(fields_toml: &str) -> String {
        format!(
            r#"
name = "brave-search"
version = "1.0.0"
description = "Search the web"
author = "Fawx"
api_version = "host_api_v1"

[settings]
version = 1

{fields_toml}
"#
        )
    }

    fn write_manifest(temp_dir: &TempDir, skill_name: &str, contents: &str) {
        let skill_dir = temp_dir.path().join(skill_name);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(skill_dir.join("manifest.toml"), contents).expect("write manifest");
    }

    fn write_manifest_in_data_dir(temp_dir: &TempDir, skill_dir_name: &str, contents: &str) {
        let skill_dir = temp_dir.path().join("skills").join(skill_dir_name);
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

    #[test]
    fn load_skill_settings_redacts_secret_values() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_manifest_in_data_dir(
            &temp_dir,
            "brave-search",
            &brave_search_settings_manifest_toml(
                r#"
[[settings.fields]]
key = "api_key"
label = "API Key"
type = "secret"
required = true

[[settings.fields]]
key = "safesearch"
label = "Safe Search"
type = "boolean"
"#,
            ),
        );

        {
            let store = EncryptedFileCredentialStore::open(temp_dir.path()).expect("open store");
            store
                .set_generic(
                    &skill_setting_store_key("brave-search", "api_key"),
                    "brv_secret_123",
                )
                .expect("store api key");
            store
                .set_generic(
                    &skill_setting_store_key("brave-search", "safesearch"),
                    "true",
                )
                .expect("store boolean");
        }

        let settings = load_skill_settings(temp_dir.path(), "brave-search")
            .expect("load settings")
            .expect("settings");

        assert_eq!(settings.schema.fields.len(), 2);
        assert_eq!(settings.values[0].key, "api_key");
        assert!(settings.values[0].is_secret);
        assert!(settings.values[0].is_configured);
        assert!(settings.values[0].value.is_none());
        assert_eq!(settings.values[1].value.as_deref(), Some("true"));
    }

    #[test]
    fn update_skill_settings_stores_values_under_skill_scoped_keys() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_manifest_in_data_dir(
            &temp_dir,
            "brave-search",
            &brave_search_settings_manifest_toml(
                r#"
[[settings.fields]]
key = "api_key"
label = "API Key"
type = "secret"
required = true
min_length = 8
max_length = 128

[[settings.fields]]
key = "region"
label = "Region"
type = "text"
pattern = "^[a-z-]+$"
"#,
            ),
        );

        let settings = update_skill_settings(
            temp_dir.path(),
            "brave-search",
            &[
                SkillSettingUpdate {
                    key: "api_key".to_string(),
                    value: Some("brv_secret_123".to_string()),
                },
                SkillSettingUpdate {
                    key: "region".to_string(),
                    value: Some("us-en".to_string()),
                },
            ],
        )
        .expect("update settings");

        assert!(settings.values.iter().any(|value| {
            value.key == "api_key"
                && value.is_secret
                && value.is_configured
                && value.value.is_none()
        }));
        assert_eq!(
            settings
                .values
                .iter()
                .find(|value| value.key == "region")
                .and_then(|value| value.value.as_deref()),
            Some("us-en")
        );

        let store = EncryptedFileCredentialStore::open(temp_dir.path()).expect("open store");
        assert_eq!(
            store
                .get_generic(&skill_setting_store_key("brave-search", "api_key"))
                .expect("read secret")
                .as_ref()
                .map(|value| value.as_str()),
            Some("brv_secret_123")
        );
        assert_eq!(
            store
                .get_generic(&skill_setting_store_key("brave-search", "region"))
                .expect("read region")
                .as_ref()
                .map(|value| value.as_str()),
            Some("us-en")
        );
    }

    #[test]
    fn update_skill_settings_allows_partial_secret_updates_and_clear() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_manifest_in_data_dir(
            &temp_dir,
            "brave-search",
            &brave_search_settings_manifest_toml(
                r#"
[[settings.fields]]
key = "api_key"
label = "API Key"
type = "secret"
required = true

[[settings.fields]]
key = "region"
label = "Region"
type = "text"
"#,
            ),
        );

        {
            let store = EncryptedFileCredentialStore::open(temp_dir.path()).expect("open store");
            store
                .set_generic(
                    &skill_setting_store_key("brave-search", "api_key"),
                    "brv_secret_123",
                )
                .expect("store api key");
            store
                .set_generic(&skill_setting_store_key("brave-search", "region"), "us-en")
                .expect("store region");
        }

        update_skill_settings(
            temp_dir.path(),
            "brave-search",
            &[SkillSettingUpdate {
                key: "region".to_string(),
                value: Some("global".to_string()),
            }],
        )
        .expect("partial update");

        {
            let store = EncryptedFileCredentialStore::open(temp_dir.path()).expect("open store");
            assert_eq!(
                store
                    .get_generic(&skill_setting_store_key("brave-search", "api_key"))
                    .expect("read api key")
                    .as_ref()
                    .map(|value| value.as_str()),
                Some("brv_secret_123")
            );
        }

        let error = update_skill_settings(
            temp_dir.path(),
            "brave-search",
            &[SkillSettingUpdate {
                key: "api_key".to_string(),
                value: None,
            }],
        )
        .expect_err("required secret should not clear");

        assert_eq!(
            error.message(),
            "Invalid skill manifest: API Key is required."
        );
    }
}
