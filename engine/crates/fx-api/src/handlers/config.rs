use crate::engine::ConfigManagerHandle;
use crate::state::HttpState;
use crate::types::{
    ApplyConfigPresetRequest, ApplyConfigPresetResponse, ConfigPatchRequest, ConfigPatchResponse,
    ConfigPresetDiffEntry, ConfigPresetDiffResponse, ConfigPresetSummary, ConfigPresetsResponse,
    ErrorBody,
};
use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use fx_config::{FawxConfig, PermissionAction, PermissionsConfig};
use fx_tools::ConfigSetRequest;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path as FsPath, PathBuf};
use toml_edit::{value as edit_value, Array, DocumentMut, Item, Table};

use super::HandlerResult;

pub async fn handle_config_get(
    State(state): State<HttpState>,
    query: Query<HashMap<String, String>>,
) -> HandlerResult<Json<Value>> {
    let section = query.get("section").map(|s| s.as_str()).unwrap_or("all");
    let app = state.app.lock().await;
    let manager = app.config_manager().ok_or_else(config_manager_missing)?;
    let guard = manager.lock().map_err(config_lock_error)?;
    let value = guard.get(section).map_err(bad_request)?;
    Ok(Json(value))
}

pub async fn handle_config_set(
    State(state): State<HttpState>,
    Json(request): Json<ConfigSetRequest>,
) -> HandlerResult<Json<Value>> {
    let app = state.app.lock().await;
    let manager = app.config_manager().ok_or_else(config_manager_missing)?;
    let mut guard = manager.lock().map_err(config_lock_error)?;
    guard
        .set(&request.key, &request.value)
        .map_err(bad_request)?;
    Ok(Json(serde_json::json!({
        "updated": request.key,
        "value": request.value,
    })))
}

pub async fn handle_config_patch(
    State(state): State<HttpState>,
    Json(request): Json<ConfigPatchRequest>,
) -> HandlerResult<Json<ConfigPatchResponse>> {
    let changes = parse_patch_changes(request.changes).map_err(bad_request)?;
    apply_changes(&state.data_dir, &changes).map_err(internal_error)?;
    reload_config_manager(&state).await?;

    let changed_keys = changes
        .into_iter()
        .map(|change| change.key)
        .collect::<Vec<_>>();
    Ok(Json(ConfigPatchResponse {
        updated: true,
        restart_required: requires_restart(&changed_keys),
        changed_keys,
    }))
}

pub async fn handle_config_presets() -> Json<ConfigPresetsResponse> {
    let presets = preset_summaries();
    let total = presets.len();
    Json(ConfigPresetsResponse { presets, total })
}

pub async fn handle_apply_config_preset(
    State(state): State<HttpState>,
    Path(name): Path<String>,
    Json(request): Json<ApplyConfigPresetRequest>,
) -> HandlerResult<Json<ApplyConfigPresetResponse>> {
    if !request.confirm {
        return Err(bad_request(
            "preset application requires confirm=true".to_string(),
        ));
    }

    let changes =
        preset_changes(&name).ok_or_else(|| not_found(format!("unknown config preset: {name}")))?;
    let current = current_config_value(&state).await?;
    let diff = preset_diff_entries(&current, &changes);

    apply_changes(&state.data_dir, &changes).map_err(internal_error)?;
    reload_config_manager(&state).await?;

    let changed_keys = diff.into_iter().map(|entry| entry.key).collect::<Vec<_>>();
    Ok(Json(ApplyConfigPresetResponse {
        name,
        applied: true,
        restart_required: requires_restart(&changed_keys),
        changed_keys,
    }))
}

pub async fn handle_config_preset_diff(
    State(state): State<HttpState>,
    Path(name): Path<String>,
) -> HandlerResult<Json<ConfigPresetDiffResponse>> {
    let changes =
        preset_changes(&name).ok_or_else(|| not_found(format!("unknown config preset: {name}")))?;
    let current = current_config_value(&state).await?;
    Ok(Json(ConfigPresetDiffResponse {
        name,
        changes: preset_diff_entries(&current, &changes),
    }))
}

#[derive(Debug, Clone)]
struct ConfigValueChange {
    key: String,
    value: Value,
}

impl ConfigValueChange {
    fn new(key: impl Into<String>, value: Value) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }
}

fn parse_patch_changes(changes: Value) -> Result<Vec<ConfigValueChange>, String> {
    if !changes.is_object() {
        return Err("changes must be a JSON object".to_string());
    }

    let mut collected = Vec::new();
    collect_patch_changes("", &changes, &mut collected)?;
    if collected.is_empty() {
        return Err("changes must include at least one leaf value".to_string());
    }

    for change in &collected {
        json_to_item(&change.value)?;
    }

    Ok(collected)
}

fn collect_patch_changes(
    path: &str,
    value: &Value,
    collected: &mut Vec<ConfigValueChange>,
) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                if key.is_empty() || key.contains('.') {
                    return Err(format!("invalid config key segment: '{key}'"));
                }
                let next = join_config_path(path, &key);
                let nested = map
                    .get(&key)
                    .ok_or_else(|| format!("missing config key: {key}"))?;
                collect_patch_changes(&next, nested, collected)?;
            }
            Ok(())
        }
        _ if path.is_empty() => Err("changes must be a JSON object".to_string()),
        _ => {
            collected.push(ConfigValueChange::new(path, value.clone()));
            Ok(())
        }
    }
}

fn join_config_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}.{segment}")
    }
}

async fn current_config_value(state: &HttpState) -> HandlerResult<Value> {
    if let Some(manager) = config_manager_handle(state).await {
        let guard = manager.lock().map_err(config_lock_error)?;
        return serde_json::to_value(guard.config())
            .map_err(|error| internal_error(format!("failed to serialize config: {error}")));
    }

    let config = FawxConfig::load(&state.data_dir).map_err(internal_error)?;
    serde_json::to_value(config)
        .map_err(|error| internal_error(format!("failed to serialize config: {error}")))
}

async fn reload_config_manager(state: &HttpState) -> HandlerResult<()> {
    let Some(manager) = config_manager_handle(state).await else {
        return Ok(());
    };

    let mut guard = manager.lock().map_err(config_lock_error)?;
    guard.reload().map_err(internal_error)
}

async fn config_manager_handle(state: &HttpState) -> Option<ConfigManagerHandle> {
    let app = state.app.lock().await;
    app.config_manager()
}

fn apply_changes(data_dir: &FsPath, changes: &[ConfigValueChange]) -> Result<(), String> {
    let mut document = read_config_document(data_dir)?;
    for change in changes {
        apply_change(&mut document, change)?;
    }
    write_config_document(data_dir, document)
}

fn apply_change(document: &mut DocumentMut, change: &ConfigValueChange) -> Result<(), String> {
    let (sections, key) = parse_key_path(&change.key)?;
    set_document_value(document, &sections, key, &change.value)
}

fn parse_key_path(key: &str) -> Result<(Vec<&str>, &str), String> {
    let parts = key.split('.').collect::<Vec<_>>();
    if parts.len() < 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(format!("key must be dot-separated, got '{key}'"));
    }

    let field = parts
        .last()
        .copied()
        .ok_or_else(|| "empty key path".to_string())?;
    Ok((parts[..parts.len() - 1].to_vec(), field))
}

fn read_config_document(data_dir: &FsPath) -> Result<DocumentMut, String> {
    let path = config_path(data_dir);
    let content = if path.exists() {
        fs::read_to_string(&path).map_err(|error| format!("failed to read config: {error}"))?
    } else {
        String::new()
    };

    content
        .parse::<DocumentMut>()
        .map_err(|error| format!("invalid config: {error}"))
}

fn write_config_document(data_dir: &FsPath, document: DocumentMut) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
    fs::write(config_path(data_dir), document.to_string())
        .map_err(|error| format!("failed to write config: {error}"))
}

fn config_path(data_dir: &FsPath) -> PathBuf {
    data_dir.join("config.toml")
}

fn set_document_value(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    json: &Value,
) -> Result<(), String> {
    let table = table_mut(document, sections)?;
    replace_item(table, key, json_to_item(json)?)
}

fn replace_item(table: &mut Table, key: &str, new_item: Item) -> Result<(), String> {
    let decor = table
        .get(key)
        .and_then(Item::as_value)
        .map(|value| value.decor().clone());
    table[key] = new_item;

    if let Some(decor) = decor {
        table[key]
            .as_value_mut()
            .ok_or_else(|| format!("config field '{key}' must be a value"))?
            .decor_mut()
            .clone_from(&decor);
    }
    Ok(())
}

fn json_to_item(json: &Value) -> Result<Item, String> {
    match json {
        Value::Null => Err("null config values are not supported".to_string()),
        Value::Bool(boolean) => Ok(edit_value(*boolean)),
        Value::String(string) => Ok(edit_value(string.as_str())),
        Value::Number(number) => json_number_to_item(number),
        Value::Array(values) => json_array_to_item(values),
        Value::Object(_) => Err("config values must be scalars or arrays".to_string()),
    }
}

fn json_number_to_item(number: &serde_json::Number) -> Result<Item, String> {
    if let Some(integer) = number.as_i64() {
        return Ok(edit_value(integer));
    }
    if let Some(integer) = number.as_u64() {
        let signed = i64::try_from(integer)
            .map_err(|_| format!("integer config value is too large: {integer}"))?;
        return Ok(edit_value(signed));
    }
    if let Some(float) = number.as_f64() {
        return Ok(edit_value(float));
    }
    Err(format!("unsupported numeric config value: {number}"))
}

fn json_array_to_item(values: &[Value]) -> Result<Item, String> {
    let mut array = Array::new();
    for value in values {
        let item = json_to_item(value)?;
        let edit_value = item
            .into_value()
            .map_err(|_| "config arrays cannot contain tables".to_string())?;
        array.push(edit_value);
    }
    Ok(Item::Value(array.into()))
}

fn table_mut<'a>(
    document: &'a mut DocumentMut,
    sections: &[&str],
) -> Result<&'a mut Table, String> {
    table_mut_in(document.as_table_mut(), sections)
}

fn table_mut_in<'a>(table: &'a mut Table, sections: &[&str]) -> Result<&'a mut Table, String> {
    let Some((section, rest)) = sections.split_first() else {
        return Ok(table);
    };

    if !table.contains_key(section) {
        table[*section] = Item::Table(Table::new());
    }

    let child = table[*section]
        .as_table_mut()
        .ok_or_else(|| format!("config section '{section}' must be a table"))?;
    table_mut_in(child, rest)
}

fn preset_summaries() -> Vec<ConfigPresetSummary> {
    vec![
        ConfigPresetSummary {
            name: "safe".to_string(),
            title: "Safe".to_string(),
            description: "Conservative defaults for cautious use.".to_string(),
        },
        ConfigPresetSummary {
            name: "power-user".to_string(),
            title: "Power User".to_string(),
            description: "Fewer confirmations, higher autonomy.".to_string(),
        },
    ]
}

fn preset_changes(name: &str) -> Option<Vec<ConfigValueChange>> {
    match name {
        "safe" => Some(permission_preset_changes(PermissionsConfig::cautious())),
        "power-user" => Some(permission_preset_changes(PermissionsConfig::power())),
        _ => None,
    }
}

fn permission_preset_changes(permissions: PermissionsConfig) -> Vec<ConfigValueChange> {
    vec![
        ConfigValueChange::new(
            "permissions.preset",
            Value::String(permissions.preset.as_str().to_string()),
        ),
        ConfigValueChange::new(
            "permissions.unrestricted",
            permission_action_values(&permissions.unrestricted),
        ),
        ConfigValueChange::new(
            "permissions.proposal_required",
            permission_action_values(&permissions.proposal_required),
        ),
    ]
}

fn permission_action_values(actions: &[PermissionAction]) -> Value {
    Value::Array(
        actions
            .iter()
            .copied()
            .map(|action| Value::String(action.as_str().to_string()))
            .collect(),
    )
}

fn preset_diff_entries(
    current: &Value,
    changes: &[ConfigValueChange],
) -> Vec<ConfigPresetDiffEntry> {
    changes
        .iter()
        .filter_map(|change| {
            let old = json_path_value(current, &change.key)
                .cloned()
                .unwrap_or(Value::Null);
            if old == change.value {
                return None;
            }
            Some(ConfigPresetDiffEntry {
                key: change.key.clone(),
                old,
                r#new: change.value.clone(),
            })
        })
        .collect()
}

fn json_path_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.')
        .try_fold(value, |current, segment| match current {
            Value::Object(map) => map.get(segment),
            _ => None,
        })
}

fn requires_restart(changed_keys: &[String]) -> bool {
    changed_keys.iter().any(|key| {
        matches!(
            key.as_str(),
            "http" | "ui" | "telegram" | "webhook" | "fleet" | "orchestrator"
        ) || [
            "http.",
            "ui.",
            "telegram.",
            "webhook.",
            "fleet.",
            "orchestrator.",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix))
    })
}

fn config_manager_missing() -> (StatusCode, Json<ErrorBody>) {
    not_found("config manager not available".to_string())
}

fn bad_request(error: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::BAD_REQUEST, Json(ErrorBody { error }))
}

fn not_found(error: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::NOT_FOUND, Json(ErrorBody { error }))
}

fn internal_error(error: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorBody { error }))
}

fn config_lock_error(error: impl std::fmt::Display) -> (StatusCode, Json<ErrorBody>) {
    tracing::error!(error = %error, "config manager lock failed");
    internal_error("internal_error".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_scalar_values_convert_to_toml_items() {
        assert_eq!(
            json_to_item(&serde_json::json!(true))
                .expect("bool")
                .to_string(),
            "true"
        );
        assert_eq!(
            json_to_item(&serde_json::json!("safe"))
                .expect("string")
                .to_string(),
            "\"safe\""
        );
        assert_eq!(
            json_to_item(&serde_json::json!(8401))
                .expect("integer")
                .to_string(),
            "8401"
        );
        assert_eq!(
            json_to_item(&serde_json::json!(0.5))
                .expect("float")
                .to_string(),
            "0.5"
        );
    }

    #[test]
    fn config_patch_request_deserializes_nested_changes() {
        let request: ConfigPatchRequest = serde_json::from_value(serde_json::json!({
            "changes": {
                "http": { "port": 8401 },
                "ui": { "auto_start": true }
            }
        }))
        .expect("deserialize");

        let changes = parse_patch_changes(request.changes).expect("flatten changes");
        let keys = changes
            .into_iter()
            .map(|change| change.key)
            .collect::<Vec<_>>();

        assert_eq!(keys, vec!["http.port", "ui.auto_start"]);
    }

    #[test]
    fn config_patch_response_serializes_expected_shape() {
        let response = ConfigPatchResponse {
            updated: true,
            restart_required: true,
            changed_keys: vec!["http.port".to_string(), "ui.auto_start".to_string()],
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["updated"], true);
        assert_eq!(json["restart_required"], true);
        assert_eq!(
            json["changed_keys"],
            serde_json::json!(["http.port", "ui.auto_start"])
        );
    }

    #[test]
    fn config_presets_response_serializes_expected_shape() {
        let response = ConfigPresetsResponse {
            presets: preset_summaries(),
            total: 2,
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["total"], 2);
        assert_eq!(json["presets"][0]["name"], "safe");
        assert_eq!(
            json["presets"][0]["description"],
            "Conservative defaults for cautious use."
        );
        assert_eq!(json["presets"][1]["name"], "power-user");
        assert_eq!(
            json["presets"][1]["description"],
            "Fewer confirmations, higher autonomy."
        );
    }

    #[test]
    fn apply_config_preset_request_deserializes_expected_shape() {
        let request: ApplyConfigPresetRequest =
            serde_json::from_value(serde_json::json!({ "confirm": true })).expect("deserialize");

        assert!(request.confirm);
    }

    #[test]
    fn apply_config_preset_response_serializes_expected_shape() {
        let response = ApplyConfigPresetResponse {
            name: "safe".to_string(),
            applied: true,
            restart_required: false,
            changed_keys: vec!["permissions.preset".to_string()],
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["name"], "safe");
        assert_eq!(json["applied"], true);
        assert_eq!(json["restart_required"], false);
        assert_eq!(
            json["changed_keys"],
            serde_json::json!(["permissions.preset"])
        );
    }

    #[test]
    fn config_preset_diff_response_serializes_expected_shape() {
        let response = ConfigPresetDiffResponse {
            name: "safe".to_string(),
            changes: vec![ConfigPresetDiffEntry {
                key: "permissions.preset".to_string(),
                old: Value::String("power".to_string()),
                r#new: Value::String("cautious".to_string()),
            }],
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["name"], "safe");
        assert_eq!(json["changes"][0]["key"], "permissions.preset");
        assert_eq!(json["changes"][0]["old"], "power");
        assert_eq!(json["changes"][0]["new"], "cautious");
    }
}
