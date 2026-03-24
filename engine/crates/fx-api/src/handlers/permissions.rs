use crate::engine::ConfigManagerHandle;
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Json, State};
use axum::http::StatusCode;
use fx_config::{
    CapabilityMode, FawxConfig, PermissionAction, PermissionPreset, PermissionsConfig,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path as FsPath, PathBuf};
use toml_edit::{value as edit_value, Array, DocumentMut, Item, Table};

use super::HandlerResult;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionEntry {
    pub action: String,
    pub level: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionsResponse {
    pub preset: String,
    pub mode: String,
    pub permissions: Vec<PermissionEntry>,
    pub available_presets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PermissionsPatchRequest {
    pub preset: Option<String>,
    pub mode: Option<String>,
    #[serde(default)]
    pub changes: Option<Vec<PermissionChange>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionChange {
    pub action: String,
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionsPatchResponse {
    pub updated: bool,
    pub preset: String,
    pub changed_actions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionLevel {
    Allow,
    Propose,
    Deny,
}

const ALL_ACTIONS: [PermissionAction; 16] = [
    PermissionAction::ReadAny,
    PermissionAction::WebSearch,
    PermissionAction::WebFetch,
    PermissionAction::CodeExecute,
    PermissionAction::FileWrite,
    PermissionAction::Git,
    PermissionAction::Shell,
    PermissionAction::ToolCall,
    PermissionAction::SelfModify,
    PermissionAction::CredentialChange,
    PermissionAction::SystemInstall,
    PermissionAction::NetworkListen,
    PermissionAction::OutboundMessage,
    PermissionAction::FileDelete,
    PermissionAction::OutsideWorkspace,
    PermissionAction::KernelModify,
];

pub async fn handle_get_permissions(
    State(state): State<HttpState>,
) -> HandlerResult<Json<PermissionsResponse>> {
    let permissions = normalized_permissions(&load_permissions_config(&state).await?);
    Ok(Json(build_permissions_response(&permissions)))
}

pub async fn handle_patch_permissions(
    State(state): State<HttpState>,
    Json(request): Json<PermissionsPatchRequest>,
) -> HandlerResult<Json<PermissionsPatchResponse>> {
    let current = load_permissions_config(&state).await?;
    let (permissions, changed_actions) = apply_patch_request(&current, request)?;
    write_permissions_config(&state.data_dir, &permissions).map_err(internal_error)?;
    reload_config_manager(&state).await?;

    Ok(Json(PermissionsPatchResponse {
        updated: true,
        preset: permissions.preset.as_str().to_string(),
        changed_actions,
    }))
}

fn build_permissions_response(config: &PermissionsConfig) -> PermissionsResponse {
    PermissionsResponse {
        preset: config.preset.as_str().to_string(),
        mode: capability_mode_name(config.mode).to_string(),
        permissions: permission_entries(config),
        available_presets: available_presets(),
    }
}

fn permission_entries(config: &PermissionsConfig) -> Vec<PermissionEntry> {
    all_actions()
        .iter()
        .map(|action| PermissionEntry {
            action: action.as_str().to_string(),
            level: action_level(*action, config).to_string(),
            title: action_title(*action).to_string(),
        })
        .collect()
}

fn available_presets() -> Vec<String> {
    vec![
        PermissionPreset::Power.as_str().to_string(),
        PermissionPreset::Cautious.as_str().to_string(),
        PermissionPreset::Experimental.as_str().to_string(),
        PermissionPreset::Custom.as_str().to_string(),
    ]
}

fn apply_patch_request(
    current: &PermissionsConfig,
    request: PermissionsPatchRequest,
) -> Result<(PermissionsConfig, Vec<String>), (StatusCode, Json<ErrorBody>)> {
    validate_patch_request(&request).map_err(validation_error)?;

    let mut permissions = resolve_base_permissions(current, request.preset.as_deref())?;
    apply_capability_mode(&mut permissions, request.mode.as_deref())?;
    let changed_actions = apply_permission_changes(&mut permissions, request.changes)?;
    if !changed_actions.is_empty() {
        permissions.preset = PermissionPreset::Custom;
    }

    Ok((normalized_permissions(&permissions), changed_actions))
}

fn validate_patch_request(request: &PermissionsPatchRequest) -> Result<(), String> {
    if has_preset(request) || has_mode(request) || has_changes(request) {
        return Ok(());
    }
    Err("permissions patch requires a preset, mode, or at least one change".to_string())
}

fn has_preset(request: &PermissionsPatchRequest) -> bool {
    request
        .preset
        .as_deref()
        .is_some_and(|preset| !preset.trim().is_empty())
}

fn has_mode(request: &PermissionsPatchRequest) -> bool {
    request
        .mode
        .as_deref()
        .is_some_and(|mode| !mode.trim().is_empty())
}

fn has_changes(request: &PermissionsPatchRequest) -> bool {
    request
        .changes
        .as_ref()
        .is_some_and(|changes| !changes.is_empty())
}

fn resolve_base_permissions(
    current: &PermissionsConfig,
    preset: Option<&str>,
) -> Result<PermissionsConfig, (StatusCode, Json<ErrorBody>)> {
    match preset.map(str::trim) {
        Some("custom") => Ok(PermissionsConfig {
            preset: PermissionPreset::Custom,
            ..normalized_permissions(current)
        }),
        Some(name) => PermissionsConfig::from_preset_name(name).map_err(validation_error),
        None => Ok(normalized_permissions(current)),
    }
}

fn apply_permission_changes(
    config: &mut PermissionsConfig,
    changes: Option<Vec<PermissionChange>>,
) -> Result<Vec<String>, (StatusCode, Json<ErrorBody>)> {
    let mut changed_actions = Vec::new();
    for change in changes.unwrap_or_default() {
        let action = parse_action(&change.action).map_err(validation_error)?;
        let level = parse_level(&change.level).map_err(validation_error)?;
        set_action_level(config, action, level);
        push_changed_action(&mut changed_actions, action);
    }
    Ok(changed_actions)
}

fn push_changed_action(changed_actions: &mut Vec<String>, action: PermissionAction) {
    let action_name = action.as_str().to_string();
    if !changed_actions.contains(&action_name) {
        changed_actions.push(action_name);
    }
}

async fn load_permissions_config(state: &HttpState) -> HandlerResult<PermissionsConfig> {
    let Some(manager) = config_manager_handle(state).await else {
        return FawxConfig::load(&state.data_dir)
            .map(|config| config.permissions)
            .map_err(internal_error);
    };

    let guard = manager.lock().map_err(config_lock_error)?;
    Ok(guard.config().permissions.clone())
}

async fn config_manager_handle(state: &HttpState) -> Option<ConfigManagerHandle> {
    let app = state.app.lock().await;
    app.config_manager()
}

fn normalized_permissions(config: &PermissionsConfig) -> PermissionsConfig {
    let mut normalized = PermissionsConfig {
        preset: config.preset,
        mode: config.mode,
        unrestricted: Vec::new(),
        proposal_required: Vec::new(),
    };

    for &action in all_actions() {
        match action_level(action, config) {
            "allow" => normalized.unrestricted.push(action),
            "ask" | "propose" | "denied" => normalized.proposal_required.push(action),
            _ => {}
        }
    }

    normalized
}

fn action_level(action: PermissionAction, config: &PermissionsConfig) -> &'static str {
    if config.unrestricted.contains(&action) {
        return "allow";
    }
    if config.proposal_required.contains(&action) {
        return proposal_level(config.mode);
    }
    "deny"
}

fn set_action_level(
    config: &mut PermissionsConfig,
    action: PermissionAction,
    level: PermissionLevel,
) {
    remove_action(config, action);
    match level {
        PermissionLevel::Allow => config.unrestricted.push(action),
        PermissionLevel::Propose => config.proposal_required.push(action),
        PermissionLevel::Deny => {}
    }
}

fn remove_action(config: &mut PermissionsConfig, action: PermissionAction) {
    config.unrestricted.retain(|candidate| *candidate != action);
    config
        .proposal_required
        .retain(|candidate| *candidate != action);
}

fn parse_level(level: &str) -> Result<PermissionLevel, String> {
    match level.trim().to_ascii_lowercase().as_str() {
        "allow" => Ok(PermissionLevel::Allow),
        "ask" | "propose" | "denied" => Ok(PermissionLevel::Propose),
        "deny" => Ok(PermissionLevel::Deny),
        other => Err(format!(
            "unknown permission level '{other}'; expected allow, propose, or deny"
        )),
    }
}

fn apply_capability_mode(
    config: &mut PermissionsConfig,
    mode: Option<&str>,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    if let Some(mode_name) = mode {
        config.mode = parse_capability_mode(mode_name).map_err(validation_error)?;
    }
    Ok(())
}

fn parse_capability_mode(mode: &str) -> Result<CapabilityMode, String> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "capability" => Ok(CapabilityMode::Capability),
        "prompt" => Ok(CapabilityMode::Prompt),
        other => Err(format!(
            "unknown capability mode '{other}'; expected capability or prompt"
        )),
    }
}

fn capability_mode_name(mode: CapabilityMode) -> &'static str {
    match mode {
        CapabilityMode::Capability => "capability",
        CapabilityMode::Prompt => "prompt",
    }
}

fn proposal_level(mode: CapabilityMode) -> &'static str {
    match mode {
        CapabilityMode::Capability => "denied",
        CapabilityMode::Prompt => "propose",
    }
}

fn parse_action(action: &str) -> Result<PermissionAction, String> {
    let normalized = action.trim().to_ascii_lowercase();
    all_actions()
        .iter()
        .copied()
        .find(|candidate| candidate.as_str() == normalized)
        .ok_or_else(|| format!("unknown permission action '{action}'"))
}

fn action_title(action: PermissionAction) -> &'static str {
    match action {
        PermissionAction::ReadAny => "Read Any File",
        PermissionAction::WebSearch => "Web Search",
        PermissionAction::WebFetch => "Web Fetch",
        PermissionAction::CodeExecute => "Code Execute",
        PermissionAction::FileWrite => "File Write",
        PermissionAction::Git => "Git",
        PermissionAction::Shell => "Shell Commands",
        PermissionAction::ToolCall => "Tool Call",
        PermissionAction::SelfModify => "Self Modify",
        PermissionAction::CredentialChange => "Credential Change",
        PermissionAction::SystemInstall => "System Install",
        PermissionAction::NetworkListen => "Network Listen",
        PermissionAction::OutboundMessage => "Outbound Message",
        PermissionAction::FileDelete => "File Delete",
        PermissionAction::OutsideWorkspace => "Outside Workspace",
        PermissionAction::KernelModify => "Kernel Modify",
    }
}

fn all_actions() -> &'static [PermissionAction] {
    &ALL_ACTIONS
}

async fn reload_config_manager(state: &HttpState) -> HandlerResult<()> {
    let Some(manager) = config_manager_handle(state).await else {
        return Ok(());
    };

    let mut guard = manager.lock().map_err(config_lock_error)?;
    guard.reload().map_err(internal_error)
}

fn write_permissions_config(
    data_dir: &FsPath,
    permissions: &PermissionsConfig,
) -> Result<(), String> {
    let mut document = read_config_document(data_dir)?;
    write_permissions_table(&mut document, permissions)?;
    write_config_document(data_dir, document)
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

fn write_permissions_table(
    document: &mut DocumentMut,
    permissions: &PermissionsConfig,
) -> Result<(), String> {
    let table = permissions_table(document)?;
    replace_item(table, "preset", edit_value(permissions.preset.as_str()))?;
    replace_item(
        table,
        "mode",
        edit_value(capability_mode_name(permissions.mode)),
    )?;
    replace_item(
        table,
        "unrestricted",
        permission_array_item(&permissions.unrestricted),
    )?;
    replace_item(
        table,
        "proposal_required",
        permission_array_item(&permissions.proposal_required),
    )
}

fn permissions_table(document: &mut DocumentMut) -> Result<&mut Table, String> {
    if !document.as_table().contains_key("permissions") {
        document["permissions"] = Item::Table(Table::new());
    }

    document["permissions"]
        .as_table_mut()
        .ok_or_else(|| "config section 'permissions' must be a table".to_string())
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
            .ok_or_else(|| format!("permissions field '{key}' must be a value"))?
            .decor_mut()
            .clone_from(&decor);
    }
    Ok(())
}

fn permission_array_item(actions: &[PermissionAction]) -> Item {
    let mut array = Array::new();
    for action in actions {
        array.push(action.as_str());
    }
    Item::Value(array.into())
}

fn write_config_document(data_dir: &FsPath, document: DocumentMut) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
    fs::write(config_path(data_dir), document.to_string())
        .map_err(|error| format!("failed to write config: {error}"))
}

fn config_path(data_dir: &FsPath) -> PathBuf {
    data_dir.join("config.toml")
}

fn validation_error(error: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::UNPROCESSABLE_ENTITY, Json(ErrorBody { error }))
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
    use crate::devices::DeviceStore;
    use crate::engine::{AppEngine, CycleResult};
    use crate::experiment_registry::ExperimentRegistry;
    use crate::pairing::PairingState;
    use crate::server_runtime::ServerRuntime;
    use crate::state::{build_channel_runtime, in_memory_telemetry, HttpState};
    use crate::types::{
        AuthProviderDto, ContextInfoDto, ErrorRecordDto, ModelInfoDto, ModelSwitchDto,
        SkillSummaryDto, ThinkingLevelDto,
    };
    use async_trait::async_trait;
    use axum::extract::State;
    use fx_bus::SessionBus;
    use fx_config::manager::ConfigManager;
    use fx_core::types::InputSource;
    use fx_kernel::StreamCallback;
    use fx_llm::{DocumentAttachment, ImageAttachment, Message};
    use std::path::Path;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Instant;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct TestApp {
        config_manager: Option<ConfigManagerHandle>,
    }

    #[async_trait]
    impl AppEngine for TestApp {
        async fn process_message(
            &mut self,
            _input: &str,
            _images: Vec<ImageAttachment>,
            _documents: Vec<DocumentAttachment>,
            _source: InputSource,
            _callback: Option<StreamCallback>,
        ) -> Result<CycleResult, anyhow::Error> {
            unreachable!("not used in permissions tests")
        }

        async fn process_message_with_context(
            &mut self,
            _input: &str,
            _images: Vec<ImageAttachment>,
            _documents: Vec<DocumentAttachment>,
            _context: Vec<Message>,
            _source: InputSource,
            _callback: Option<StreamCallback>,
        ) -> Result<(CycleResult, Vec<Message>), anyhow::Error> {
            unreachable!("not used in permissions tests")
        }

        fn active_model(&self) -> &str {
            "test-model"
        }

        fn available_models(&self) -> Vec<ModelInfoDto> {
            Vec::new()
        }

        fn set_active_model(&mut self, _selector: &str) -> Result<ModelSwitchDto, anyhow::Error> {
            Err(anyhow::anyhow!("not used in permissions tests"))
        }

        fn thinking_level(&self) -> ThinkingLevelDto {
            ThinkingLevelDto {
                level: "adaptive".to_string(),
                budget_tokens: None,
                available: vec!["adaptive".to_string()],
            }
        }

        fn context_info(&self) -> ContextInfoDto {
            ContextInfoDto {
                used_tokens: 0,
                max_tokens: 0,
                percentage: 0.0,
                compaction_threshold: 0.8,
            }
        }

        fn context_info_for_messages(&self, _messages: &[Message]) -> ContextInfoDto {
            self.context_info()
        }

        fn set_thinking_level(&mut self, _level: &str) -> Result<ThinkingLevelDto, anyhow::Error> {
            Err(anyhow::anyhow!("not used in permissions tests"))
        }

        fn skill_summaries(&self) -> Vec<SkillSummaryDto> {
            Vec::new()
        }

        fn auth_provider_statuses(&self) -> Vec<AuthProviderDto> {
            Vec::new()
        }

        fn config_manager(&self) -> Option<ConfigManagerHandle> {
            self.config_manager.clone()
        }

        fn session_bus(&self) -> Option<&SessionBus> {
            None
        }

        fn recent_errors(&self, _limit: usize) -> Vec<ErrorRecordDto> {
            Vec::new()
        }
    }

    #[tokio::test]
    async fn get_returns_all_sixteen_actions() {
        let (_temp, state) = test_state(PermissionsConfig::power());
        let Json(response) = handle_get_permissions(State(state))
            .await
            .expect("get permissions");

        assert_eq!(response.permissions.len(), 16);
    }

    #[tokio::test]
    async fn get_reflects_capability_mode_defaults() {
        let (_temp, state) = test_state(PermissionsConfig::power());
        let Json(response) = handle_get_permissions(State(state))
            .await
            .expect("get permissions");

        assert_eq!(response.mode, "capability");
        assert_eq!(
            permission_level(&response, "credential_change"),
            Some("denied")
        );
    }

    #[tokio::test]
    async fn get_reflects_power_preset_defaults() {
        let (_temp, state) = test_state(PermissionsConfig::power());
        let Json(response) = handle_get_permissions(State(state))
            .await
            .expect("get permissions");

        assert_eq!(response.preset, "power");
        assert_eq!(permission_level(&response, "shell"), Some("allow"));
        assert_eq!(
            permission_level(&response, "credential_change"),
            Some("denied")
        );
        assert_eq!(permission_level(&response, "kernel_modify"), Some("denied"));
    }

    #[tokio::test]
    async fn get_reflects_cautious_preset_defaults() {
        let (_temp, state) = test_state(PermissionsConfig::cautious());
        let Json(response) = handle_get_permissions(State(state))
            .await
            .expect("get permissions");

        assert_eq!(response.preset, "cautious");
        assert_eq!(permission_level(&response, "read_any"), Some("allow"));
        assert_eq!(permission_level(&response, "file_write"), Some("denied"));
        assert_eq!(permission_level(&response, "kernel_modify"), Some("denied"));
    }

    #[tokio::test]
    async fn patch_rejects_invalid_action() {
        let (_temp, state) = test_state(PermissionsConfig::power());
        let request = PermissionsPatchRequest {
            preset: None,
            mode: None,
            changes: Some(vec![PermissionChange {
                action: "nonexistent".to_string(),
                level: "allow".to_string(),
            }]),
        };

        let error = handle_patch_permissions(State(state), Json(request))
            .await
            .expect_err("invalid action should fail");

        assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(error.1 .0.error.contains("nonexistent"));
    }

    #[tokio::test]
    async fn patch_rejects_invalid_level() {
        let (_temp, state) = test_state(PermissionsConfig::power());
        let request = PermissionsPatchRequest {
            preset: None,
            mode: None,
            changes: Some(vec![PermissionChange {
                action: "shell".to_string(),
                level: "yolo".to_string(),
            }]),
        };

        let error = handle_patch_permissions(State(state), Json(request))
            .await
            .expect_err("invalid level should fail");

        assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(error.1 .0.error.contains("yolo"));
    }

    #[tokio::test]
    async fn patch_persists_changes_and_reloads_manager() {
        let (temp, state) = test_state(PermissionsConfig::power());
        let request = PermissionsPatchRequest {
            preset: Some("cautious".to_string()),
            mode: None,
            changes: Some(vec![PermissionChange {
                action: "shell".to_string(),
                level: "allow".to_string(),
            }]),
        };

        let Json(response) = handle_patch_permissions(State(state.clone()), Json(request))
            .await
            .expect("patch permissions");
        let Json(get_response) = handle_get_permissions(State(state))
            .await
            .expect("get updated");
        let saved = FawxConfig::load(temp.path()).expect("load saved config");

        assert!(response.updated);
        assert_eq!(response.preset, "custom");
        assert_eq!(response.changed_actions, vec!["shell"]);
        assert_eq!(permission_level(&get_response, "shell"), Some("allow"));
        assert_eq!(
            permission_level(&get_response, "file_write"),
            Some("denied")
        );
        assert_eq!(saved.permissions.preset, PermissionPreset::Custom);
        assert!(saved
            .permissions
            .unrestricted
            .contains(&PermissionAction::Shell));
    }

    #[tokio::test]
    async fn patch_applies_preset_only() {
        let (_temp, state) = test_state(PermissionsConfig::power());
        let request = PermissionsPatchRequest {
            preset: Some("cautious".to_string()),
            mode: None,
            changes: None,
        };

        let Json(response) = handle_patch_permissions(State(state.clone()), Json(request))
            .await
            .expect("patch permissions");

        assert!(response.updated);
        assert_eq!(response.preset, "cautious");
        assert!(response.changed_actions.is_empty());

        let Json(get_response) = handle_get_permissions(State(state))
            .await
            .expect("get updated");
        assert_eq!(get_response.preset, "cautious");
        assert_eq!(
            permission_level(&get_response, "file_write"),
            Some("denied")
        );
    }

    #[tokio::test]
    async fn patch_mode_only_updates_response_and_display_levels() {
        let (_temp, state) = test_state(PermissionsConfig::power());
        let request = PermissionsPatchRequest {
            preset: None,
            mode: Some("prompt".to_string()),
            changes: None,
        };

        let Json(response) = handle_patch_permissions(State(state.clone()), Json(request))
            .await
            .expect("patch permissions");
        assert!(response.updated);

        let Json(get_response) = handle_get_permissions(State(state))
            .await
            .expect("get updated");
        assert_eq!(get_response.mode, "prompt");
        assert_eq!(
            permission_level(&get_response, "credential_change"),
            Some("propose")
        );
    }

    #[tokio::test]
    async fn patch_changes_only_sets_custom() {
        let (_temp, state) = test_state(PermissionsConfig::power());
        let request = PermissionsPatchRequest {
            preset: None,
            mode: None,
            changes: Some(vec![PermissionChange {
                action: "shell".to_string(),
                level: "deny".to_string(),
            }]),
        };

        let Json(response) = handle_patch_permissions(State(state.clone()), Json(request))
            .await
            .expect("patch permissions");

        assert!(response.updated);
        assert_eq!(response.preset, "custom");
        assert_eq!(response.changed_actions, vec!["shell"]);

        let Json(get_response) = handle_get_permissions(State(state))
            .await
            .expect("get updated");
        assert_eq!(permission_level(&get_response, "shell"), Some("deny"));
    }

    #[test]
    fn patch_preset_response_serializes() {
        let response = PermissionsPatchResponse {
            updated: true,
            preset: "custom".to_string(),
            changed_actions: vec!["shell".to_string(), "file_write".to_string()],
        };

        let json = serde_json::to_string(&response).expect("serialize");
        let decoded: PermissionsPatchResponse = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(decoded, response);
    }

    #[test]
    fn permissions_response_round_trips() {
        let response = build_permissions_response(&PermissionsConfig::power());
        let json = serde_json::to_string(&response).expect("serialize");
        let decoded: PermissionsResponse = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(decoded, response);
    }

    #[test]
    fn all_actions_exhaustive_with_titles() {
        // If a new PermissionAction is added, action_title() will fail to compile
        // (non-exhaustive match), and this test ensures all_actions covers them all.
        let titles: Vec<&str> = all_actions().iter().map(|a| action_title(*a)).collect();
        assert_eq!(titles.len(), ALL_ACTIONS.len());

        let mut seen = std::collections::HashSet::new();
        for action in all_actions() {
            assert!(
                seen.insert(action.as_str()),
                "duplicate action: {}",
                action.as_str()
            );
        }
    }

    #[test]
    fn action_level_maps_correctly() {
        let config = PermissionsConfig {
            preset: PermissionPreset::Custom,
            mode: CapabilityMode::Prompt,
            unrestricted: vec![PermissionAction::ReadAny],
            proposal_required: vec![PermissionAction::FileDelete],
        };

        assert_eq!(action_level(PermissionAction::ReadAny, &config), "allow");
        assert_eq!(
            action_level(PermissionAction::FileDelete, &config),
            "propose"
        );
        assert_eq!(action_level(PermissionAction::Shell, &config), "deny");
    }

    fn permission_level<'a>(response: &'a PermissionsResponse, action: &str) -> Option<&'a str> {
        response
            .permissions
            .iter()
            .find(|entry| entry.action == action)
            .map(|entry| entry.level.as_str())
    }

    fn test_registry(data_dir: &Path) -> Arc<Mutex<ExperimentRegistry>> {
        let registry = ExperimentRegistry::new(data_dir).expect("registry");
        Arc::new(Mutex::new(registry))
    }

    fn test_state(permissions: PermissionsConfig) -> (TempDir, HttpState) {
        let temp = TempDir::new().expect("tempdir");
        write_test_config(temp.path(), permissions);

        let manager = Arc::new(StdMutex::new(
            ConfigManager::new(temp.path()).expect("config manager"),
        ));
        let app = TestApp {
            config_manager: Some(Arc::clone(&manager)),
        };
        let state = HttpState {
            app: Arc::new(Mutex::new(app)),
            shared: Arc::new(crate::state::SharedReadState::from_app(&TestApp {
                config_manager: Some(manager),
            })),
            config_manager: None,
            session_registry: None,
            start_time: Instant::now(),
            server_runtime: ServerRuntime::local(8400),
            tailscale_ip: None,
            bearer_token: "test-token".to_string(),
            pairing: Arc::new(Mutex::new(PairingState::new())),
            devices: Arc::new(Mutex::new(DeviceStore::new())),
            devices_path: None,
            channels: build_channel_runtime(None, vec![]),
            data_dir: temp.path().to_path_buf(),
            synthesis: Arc::new(crate::handlers::synthesis::SynthesisState::new(false)),
            oauth_flows: Arc::new(crate::handlers::oauth::OAuthFlowStore::new()),
            permission_prompts: Arc::new(fx_kernel::PermissionPromptState::new()),
            ripcord: None,
            fleet_manager: None,
            cron_store: None,
            experiment_registry: test_registry(temp.path()),
            improvement_provider: None,
            telemetry: in_memory_telemetry(),
        };

        (temp, state)
    }

    fn write_test_config(data_dir: &Path, permissions: PermissionsConfig) {
        let mut config = FawxConfig::default();
        config.general.data_dir = Some(data_dir.to_path_buf());
        config.permissions = permissions;

        let content = toml_edit::ser::to_string_pretty(&config).expect("serialize config");
        fs::write(data_dir.join("config.toml"), content).expect("write config");
    }
}
