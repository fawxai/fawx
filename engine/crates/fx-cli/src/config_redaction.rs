use serde_json::{Map as JsonMap, Value as JsonValue};
use toml::{map::Map as TomlMap, Value as TomlValue};

pub(crate) const REDACTED_SECRET: &str = "[REDACTED]";

pub(crate) fn sanitize_json(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(map) => JsonValue::Object(sanitize_json_object(map)),
        JsonValue::Array(items) => JsonValue::Array(items.into_iter().map(sanitize_json).collect()),
        other => other,
    }
}

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
        .any(|marker| normalized.contains(marker))
}

fn sanitize_json_object(map: JsonMap<String, JsonValue>) -> JsonMap<String, JsonValue> {
    map.into_iter()
        .map(|(key, value)| (key.clone(), sanitize_json_entry(&key, value)))
        .collect()
}

fn sanitize_json_entry(key: &str, value: JsonValue) -> JsonValue {
    if is_secret_key(key) {
        JsonValue::String(REDACTED_SECRET.to_string())
    } else {
        sanitize_json(value)
    }
}

fn sanitize_toml_table(table: TomlMap<String, TomlValue>) -> TomlMap<String, TomlValue> {
    table
        .into_iter()
        .map(|(key, value)| (key.clone(), sanitize_toml_entry(&key, value)))
        .collect()
}

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
    fn secret_key_detection_uses_contains_heuristic() {
        assert!(is_secret_key("bot_token"));
        assert!(is_secret_key("service_private_key"));
        assert!(is_secret_key("aws_access_key"));
        assert!(is_secret_key("customCredentialName"));
        assert!(!is_secret_key("default_model"));
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
