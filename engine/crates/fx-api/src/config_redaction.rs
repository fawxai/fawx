// TODO: This logic is duplicated in fx-cli/src/headless.rs. Extract to a shared crate (fx-config or fx-core) when adding more API endpoints.
use serde_json::{Map as JsonMap, Value as JsonValue};

pub(crate) const REDACTED_SECRET: &str = "[REDACTED]";

pub(crate) fn sanitize_json(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(map) => JsonValue::Object(sanitize_json_object(map)),
        JsonValue::Array(items) => JsonValue::Array(items.into_iter().map(sanitize_json).collect()),
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

fn secret_markers() -> &'static [&'static str] {
    &["key", "token", "secret", "password", "credential"]
}
