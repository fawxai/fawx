use serde_json::{Map as JsonMap, Value as JsonValue};
use toml::{map::Map as TomlMap, Value as TomlValue};

pub(crate) const REDACTED_SECRET: &str = "[REDACTED]";

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn sanitize_json(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(map) => JsonValue::Object(sanitize_json_object(map)),
        JsonValue::Array(items) => JsonValue::Array(items.into_iter().map(sanitize_json).collect()),
        other => other,
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn sanitize_toml(value: TomlValue) -> TomlValue {
    match value {
        TomlValue::Table(table) => TomlValue::Table(sanitize_toml_table(table)),
        TomlValue::Array(items) => TomlValue::Array(items.into_iter().map(sanitize_toml).collect()),
        other => other,
    }
}

pub(crate) fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    secret_markers()
        .iter()
        .any(|marker| has_secret_suffix(&normalized, marker))
}

fn has_secret_suffix(key: &str, marker: &str) -> bool {
    key == marker
        || key
            .strip_suffix(marker)
            .is_some_and(|prefix| prefix.ends_with('_'))
}

#[cfg_attr(not(test), allow(dead_code))]
fn sanitize_json_object(map: JsonMap<String, JsonValue>) -> JsonMap<String, JsonValue> {
    map.into_iter()
        .map(|(key, value)| (key.clone(), sanitize_json_entry(&key, value)))
        .collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn sanitize_json_entry(key: &str, value: JsonValue) -> JsonValue {
    if is_secret_key(key) {
        JsonValue::String(REDACTED_SECRET.to_string())
    } else {
        sanitize_json(value)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn sanitize_toml_table(table: TomlMap<String, TomlValue>) -> TomlMap<String, TomlValue> {
    table
        .into_iter()
        .map(|(key, value)| (key.clone(), sanitize_toml_entry(&key, value)))
        .collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn sanitize_toml_entry(key: &str, value: TomlValue) -> TomlValue {
    if is_secret_key(key) {
        TomlValue::String(REDACTED_SECRET.to_string())
    } else {
        sanitize_toml(value)
    }
}

fn secret_markers() -> &'static [&'static str] {
    &["key", "token", "secret", "password", "credential"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_key_detection_matches_secret_suffixes() {
        assert!(is_secret_key("bot_token"));
        assert!(is_secret_key("service_private_key"));
        assert!(is_secret_key("aws_access_key"));
        assert!(is_secret_key("credential"));
        assert!(!is_secret_key("default_model"));
    }

    #[test]
    fn secret_key_detection_avoids_non_secret_marker_substrings() {
        assert!(!is_secret_key("max_tokens"));
        assert!(!is_secret_key("api_key_id"));
        assert!(!is_secret_key("tokenizer_model"));
    }

    #[test]
    fn sanitize_json_redacts_nested_secret_values() {
        let sanitized = sanitize_json(serde_json::json!({
            "model": { "default_model": "test-model" },
            "telegram": { "bot_token": "secret-token" },
            "nested": { "api_key": "secret-key" }
        }));

        assert_eq!(sanitized["model"]["default_model"], "test-model");
        assert_eq!(sanitized["telegram"]["bot_token"], REDACTED_SECRET);
        assert_eq!(sanitized["nested"]["api_key"], REDACTED_SECRET);
    }

    #[test]
    fn sanitize_toml_redacts_secret_values_without_touching_safe_fields() {
        let sanitized = sanitize_toml(
            toml::Value::try_from(serde_json::json!({
                "http": { "bearer_token": "secret-bearer" },
                "model": { "default_model": "test-model" }
            }))
            .expect("toml value"),
        );

        assert_eq!(
            sanitized["http"]["bearer_token"].as_str(),
            Some(REDACTED_SECRET)
        );
        assert_eq!(
            sanitized["model"]["default_model"].as_str(),
            Some("test-model")
        );
    }
}
