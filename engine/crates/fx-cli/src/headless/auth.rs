use super::*;

pub(super) fn handle_headless_auth_command(
    router: &ModelRouter,
    subcommand: Option<&str>,
    action: Option<&str>,
    value: Option<&str>,
    has_extra_args: bool,
) -> anyhow::Result<String> {
    if is_auth_write_action(action) {
        return Ok("Use `fawx setup` to manage credentials.".to_string());
    }

    match (subcommand, action, value, has_extra_args) {
        (None, None, None, false) | (Some("list-providers"), None, None, false) => {
            Ok(render_auth_overview(router))
        }
        (Some(provider), Some("show-status"), None, false) => {
            Ok(render_auth_provider_status(router, provider))
        }
        _ => Ok(auth_usage_message()),
    }
}

pub(super) fn auth_provider_statuses(
    models: Vec<ModelInfo>,
    stored_auth_entries: Vec<StoredAuthProviderEntry>,
) -> Vec<AuthProviderStatus> {
    let mut statuses = BTreeMap::new();
    for entry in stored_auth_entries {
        update_saved_auth_provider_status(&mut statuses, entry);
    }
    for model in models {
        update_auth_provider_status(&mut statuses, model);
    }
    statuses.into_values().collect()
}

#[cfg(feature = "http")]
pub(super) fn auth_provider_dto(status: AuthProviderStatus) -> AuthProviderDto {
    AuthProviderDto {
        provider: status.provider,
        auth_methods: status.auth_methods.into_iter().collect(),
        model_count: status.model_count,
        status: status.status,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoredAuthProviderEntry {
    pub provider: String,
    pub auth_method: String,
}

pub(super) fn stored_auth_provider_entries(data_dir: &Path) -> Vec<StoredAuthProviderEntry> {
    let store = match AuthStore::open(data_dir) {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(error = %error, "failed to open auth store while building auth statuses");
            return Vec::new();
        }
    };
    let auth_manager = match store.load_auth_manager() {
        Ok(auth_manager) => auth_manager,
        Err(error) => {
            tracing::warn!(error = %error, "failed to load auth manager while building auth statuses");
            return Vec::new();
        }
    };

    auth_manager
        .providers()
        .into_iter()
        .filter_map(|provider| {
            let auth_method = auth_manager
                .get(&provider)
                .map(stored_auth_method_label)?
                .to_string();
            Some(StoredAuthProviderEntry {
                provider: normalize_provider_name(&provider),
                auth_method,
            })
        })
        .collect()
}

fn is_auth_write_action(action: Option<&str>) -> bool {
    matches!(action, Some("set-token") | Some("clear-token"))
}

fn auth_usage_message() -> String {
    "Usage: /auth {provider} <set-token|show-status|clear-token> [TOKEN]".to_string()
}

fn render_auth_overview(router: &ModelRouter) -> String {
    let statuses = auth_provider_statuses(router.available_models(), Vec::new());
    if statuses.is_empty() {
        return "No credentials configured.".to_string();
    }

    let mut lines = vec!["Configured credentials:".to_string()];
    lines.extend(statuses.iter().map(render_auth_status_line));
    lines.join("\n")
}

fn render_auth_status_line(status: &AuthProviderStatus) -> String {
    let state_label = match status.status.as_str() {
        "saved" => "saved",
        _ => "configured",
    };

    format!(
        "  ✓ {}: {} ({}) — {}",
        status.provider,
        state_label,
        format_auth_methods(&status.auth_methods),
        model_count_label(status.model_count)
    )
}

fn render_auth_provider_status(router: &ModelRouter, provider: &str) -> String {
    let provider = normalize_provider_name(provider);
    match auth_provider_statuses(router.available_models(), Vec::new())
        .into_iter()
        .find(|status| status.provider == provider)
    {
        Some(status) => format!(
            "{} auth status:\n  Status: {} ({})\n  Models available: {}",
            status.provider,
            status.status,
            format_auth_methods(&status.auth_methods),
            status.model_count
        ),
        None => format!("{provider} auth status:\n  Status: not configured"),
    }
}

fn stored_auth_method_label(auth_method: &fx_auth::auth::AuthMethod) -> &'static str {
    match auth_method {
        fx_auth::auth::AuthMethod::ApiKey { .. } => "api_key",
        fx_auth::auth::AuthMethod::SetupToken { .. } => "setup_token",
        fx_auth::auth::AuthMethod::OAuth { .. } => "oauth",
    }
}

fn update_saved_auth_provider_status(
    statuses: &mut BTreeMap<String, AuthProviderStatus>,
    entry: StoredAuthProviderEntry,
) {
    let status = statuses
        .entry(entry.provider.clone())
        .or_insert_with(|| AuthProviderStatus {
            provider: entry.provider,
            auth_methods: BTreeSet::new(),
            model_count: 0,
            status: "saved".to_string(),
        });
    status.auth_methods.insert(entry.auth_method);
    if status.model_count == 0 {
        status.status = "saved".to_string();
    }
}

fn update_auth_provider_status(
    statuses: &mut BTreeMap<String, AuthProviderStatus>,
    model: ModelInfo,
) {
    let provider = normalize_provider_name(&model.provider_name);
    let status = statuses
        .entry(provider.clone())
        .or_insert_with(|| AuthProviderStatus {
            provider,
            auth_methods: BTreeSet::new(),
            model_count: 0,
            status: "registered".to_string(),
        });
    status.auth_methods.insert(model.auth_method);
    status.model_count += 1;
    if status.provider == "github" && status.status == "saved" {
        return;
    }
    status.status = "registered".to_string();
}

fn format_auth_methods(auth_methods: &BTreeSet<String>) -> String {
    auth_methods
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn model_count_label(model_count: usize) -> String {
    match model_count {
        1 => "1 model".to_string(),
        count => format!("{count} models"),
    }
}

fn normalize_provider_name(value: &str) -> String {
    let lower = value.trim().to_ascii_lowercase();
    match lower.as_str() {
        "gh" => "github".to_string(),
        other => other.to_string(),
    }
}
