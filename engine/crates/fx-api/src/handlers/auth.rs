use crate::auth_store::AuthStore;
use crate::state::HttpState;
use crate::types::{
    ApiKeyRequest, ApiKeyResponse, DeleteProviderResponse, ErrorBody, SetupTokenRequest,
    SetupTokenResponse, VerifyRequest, VerifyResponse,
};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use fx_auth::auth::AuthMethod;
use fx_auth::github::validate_github_pat;
use fx_llm::{CompletionProvider, ModelCatalog, OpenAiResponsesProvider};
use std::time::Duration;
use tokio::time;
use zeroize::Zeroizing;

use super::HandlerResult;

pub async fn handle_setup_token(
    State(state): State<HttpState>,
    Json(request): Json<SetupTokenRequest>,
) -> HandlerResult<Json<SetupTokenResponse>> {
    // TODO: exchange setup token via Anthropic API
    // For now, stub — store the token and return success
    save_auth_method(
        &state,
        "anthropic",
        fx_auth::auth::AuthMethod::SetupToken {
            token: request.setup_token,
        },
    )
    .await?;

    Ok(Json(SetupTokenResponse {
        provider: "anthropic".to_string(),
        status: "authenticated".to_string(),
        auth_method: "setup_token".to_string(),
        model_count: 0,
        verified: false,
    }))
}

pub async fn handle_store_api_key(
    State(state): State<HttpState>,
    Path(provider): Path<String>,
    Json(request): Json<ApiKeyRequest>,
) -> HandlerResult<Json<ApiKeyResponse>> {
    // TODO: store key in credential store
    save_auth_method(
        &state,
        &provider,
        fx_auth::auth::AuthMethod::ApiKey {
            provider: provider.clone(),
            key: request.api_key,
        },
    )
    .await?;

    // NEVER echo the api_key back
    Ok(Json(ApiKeyResponse {
        provider,
        status: "authenticated".to_string(),
        auth_method: "api_key".to_string(),
        model_count: 0,
        verified: false,
    }))
}

pub async fn handle_delete_provider(
    State(state): State<HttpState>,
    Path(provider): Path<String>,
) -> HandlerResult<Json<DeleteProviderResponse>> {
    // TODO: remove from credential store
    delete_provider_auth(&state, &provider).await?;

    Ok(Json(DeleteProviderResponse {
        provider,
        removed: true,
    }))
}

pub async fn handle_verify_provider(
    State(state): State<HttpState>,
    Path(provider): Path<String>,
    Json(request): Json<VerifyRequest>,
) -> HandlerResult<Json<VerifyResponse>> {
    let checked_at = current_unix_timestamp_secs();
    let store = AuthStore::open(&state.data_dir).map_err(internal_error)?;
    let auth_manager = store.load_auth_manager().map_err(internal_error)?;

    let Some(auth_method) = auth_manager.get(&provider).cloned() else {
        return Ok(Json(VerifyResponse {
            provider,
            verified: false,
            status: "not_configured".to_string(),
            message: "No saved credentials found for this provider.".to_string(),
            checked_at,
        }));
    };

    let timeout_seconds = request.timeout_seconds.clamp(1, 30);
    let timeout = Duration::from_secs(timeout_seconds);

    match verify_auth_method(&provider, &auth_method, timeout).await {
        Ok(verification) => {
            if !verification.discovered_models.is_empty() {
                fx_config::update_provider_model_cache(
                    &state.data_dir,
                    &provider,
                    &verification.discovered_models,
                )
                .map_err(internal_error)?;
                reload_app_providers(&state).await;
            }

            Ok(Json(VerifyResponse {
                provider,
                verified: true,
                status: "authenticated".to_string(),
                message: verification.message,
                checked_at,
            }))
        }
        Err(message) => Ok(Json(VerifyResponse {
            provider,
            verified: false,
            status: "invalid".to_string(),
            message,
            checked_at,
        })),
    }
}

struct VerificationSuccess {
    message: String,
    discovered_models: Vec<String>,
}

async fn verify_auth_method(
    provider: &str,
    auth_method: &AuthMethod,
    timeout: Duration,
) -> Result<VerificationSuccess, String> {
    match auth_method {
        AuthMethod::ApiKey { key, .. } if provider == "github" => verify_github_token(key)
            .await
            .map(|message| VerificationSuccess {
                message,
                discovered_models: Vec::new(),
            }),
        AuthMethod::OAuth {
            provider: stored_provider,
            access_token,
            account_id,
            ..
        } => {
            if stored_provider != provider {
                return Err(format!(
                    "Stored OAuth credentials belong to '{stored_provider}', not '{provider}'."
                ));
            }

            if let Some(account_id) = account_id {
                if provider != "openai" {
                    return Err(format!(
                        "Stored OAuth credentials don't support verification for provider '{provider}'."
                    ));
                }

                let provider_client =
                    OpenAiResponsesProvider::new(access_token.clone(), account_id.clone())
                        .map_err(|error| error.to_string())?;
                let verification = time::timeout(timeout, provider_client.verify_credentials())
                    .await
                    .map_err(|_| {
                        format!(
                            "Timed out while contacting {}.",
                            provider_display_name(provider)
                        )
                    })?;

                verification
                    .map_err(|error| verification_error_message(provider, error.to_string()))?;

                let discovered_models = CompletionProvider::list_models(&provider_client)
                    .await
                    .unwrap_or_default();
                Ok(VerificationSuccess {
                    message: "Credentials verified successfully.".to_string(),
                    discovered_models,
                })
            } else {
                verify_with_catalog(provider, access_token, "oauth", timeout).await
            }
        }
        _ => {
            let (provider_name, token, auth_mode) = verification_request(provider, auth_method)?;
            verify_with_catalog(provider_name, &token, auth_mode, timeout).await
        }
    }
}

async fn verify_with_catalog(
    provider: &str,
    token: &str,
    auth_mode: &str,
    timeout: Duration,
) -> Result<VerificationSuccess, String> {
    let catalog = ModelCatalog::with_timeout(timeout);
    let models = catalog
        .fetch_live_models(provider, token, auth_mode)
        .await
        .map(unique_catalog_model_ids)
        .map_err(|error| verification_error_message(provider, error))?;
    Ok(VerificationSuccess {
        message: "Credentials verified successfully.".to_string(),
        discovered_models: models,
    })
}

async fn verify_github_token(token: &str) -> Result<String, String> {
    let token = Zeroizing::new(token.to_string());
    let info = validate_github_pat(&token)
        .await
        .map_err(|error| verification_error_message("github", error.to_string()))?;

    if info.missing_scopes.is_empty() {
        return Ok(format!("GitHub token verified for @{}.", info.login));
    }

    if token.as_str().starts_with("github_pat_") {
        return Ok(format!(
            "GitHub token verified for @{}. Fine-grained PAT scopes couldn't be confirmed from GitHub headers.",
            info.login
        ));
    }

    Err(format!(
        "GitHub token is valid for @{}, but it's missing required scopes: {}.",
        info.login,
        info.missing_scopes.join(", ")
    ))
}

fn verification_request<'a>(
    provider: &'a str,
    auth_method: &'a AuthMethod,
) -> Result<(&'a str, String, &'static str), String> {
    match auth_method {
        AuthMethod::ApiKey { key, .. } => {
            let auth_mode = if provider == "anthropic" {
                "api_key"
            } else {
                "bearer"
            };
            Ok((provider, key.clone(), auth_mode))
        }
        AuthMethod::SetupToken { token } => {
            if provider != "anthropic" {
                return Err(format!(
                    "Stored credentials don't support verification for provider '{provider}'."
                ));
            }
            Ok((provider, token.clone(), "setup_token"))
        }
        AuthMethod::OAuth { .. } => Err(format!(
            "Stored OAuth credentials require provider-specific verification for '{provider}'."
        )),
    }
}

fn verification_error_message(provider: &str, error: String) -> String {
    let provider_label = provider_display_name(provider);

    if error.contains("401") || error.contains("403") {
        return format!("{provider_label} rejected these credentials.");
    }

    if error.contains("invalid or expired") {
        return format!("{provider_label} rejected these credentials.");
    }

    if error.contains("timed out") || error.contains("deadline has elapsed") {
        return format!("Timed out while contacting {provider_label}.");
    }

    if error.contains("unsupported provider") || error.contains("unsupported auth mode") {
        return error;
    }

    if error.contains("request failed") {
        return format!("Couldn't reach {provider_label} to verify credentials.");
    }

    format!("{provider_label} verification failed: {error}")
}

fn unique_catalog_model_ids(models: Vec<fx_llm::CatalogModel>) -> Vec<String> {
    let mut ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

fn provider_display_name(provider: &str) -> &str {
    match provider {
        "anthropic" => "Anthropic",
        "fireworks" => "Fireworks",
        "github" => "GitHub",
        "openai" => "OpenAI",
        "openrouter" => "OpenRouter",
        _ => provider,
    }
}

fn current_unix_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) async fn save_auth_method(
    state: &HttpState,
    provider: &str,
    auth_method: fx_auth::auth::AuthMethod,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    let store = AuthStore::open(&state.data_dir).map_err(internal_error)?;
    let mut auth_manager = store.load_auth_manager().map_err(internal_error)?;
    auth_manager.store(provider, auth_method);
    store
        .save_auth_manager(&auth_manager)
        .map_err(internal_error)?;
    reload_app_providers(state).await;
    Ok(())
}

async fn delete_provider_auth(
    state: &HttpState,
    provider: &str,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    let store = AuthStore::open(&state.data_dir).map_err(internal_error)?;
    let mut auth_manager = store.load_auth_manager().map_err(internal_error)?;
    auth_manager.remove(provider);
    store
        .save_auth_manager(&auth_manager)
        .map_err(internal_error)?;
    fx_config::clear_provider_model_cache(&state.data_dir, provider).map_err(internal_error)?;
    reload_app_providers(state).await;
    Ok(())
}

async fn reload_app_providers(state: &HttpState) {
    let snapshot = {
        let mut app = state.app.lock().await;
        match app.reload_providers() {
            Ok(()) => Some((
                app.active_model().to_owned(),
                app.thinking_level(),
                app.available_models(),
                app.max_history(),
            )),
            Err(error) => {
                tracing::warn!(error = %error, "failed to reload providers after auth change");
                None
            }
        }
    };

    if let Some((active_model, thinking, models, max_history)) = snapshot {
        state
            .shared
            .update_model(&active_model, &thinking, models, max_history)
            .await;
    }
}

fn internal_error(error: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorBody { error }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::DeviceStore;
    use crate::pairing::PairingState;
    use crate::server_runtime::ServerRuntime;
    use crate::state::{build_channel_runtime, in_memory_telemetry, HttpState, SharedReadState};
    use crate::test_support::StubAppEngine;
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::sync::Mutex;

    fn test_state(data_dir: std::path::PathBuf) -> HttpState {
        let app = StubAppEngine::default();
        let shared = Arc::new(SharedReadState::from_app(&app));

        HttpState {
            app: Arc::new(Mutex::new(app)),
            shared,
            config_manager: None,
            session_registry: None,
            session_runs: crate::state::SessionRunRegistry::default(),
            session_engines: crate::state::SessionEnginePool::default(),
            start_time: Instant::now(),
            server_runtime: ServerRuntime::local(8400),
            tailscale_ip: None,
            bearer_token: "test-token".to_string(),
            pairing: Arc::new(Mutex::new(PairingState::new())),
            devices: Arc::new(Mutex::new(DeviceStore::new())),
            devices_path: None,
            channels: build_channel_runtime(None, Vec::new()),
            data_dir: data_dir.clone(),
            synthesis: Arc::new(crate::handlers::synthesis::SynthesisState::new(false)),
            oauth_flows: Arc::new(crate::handlers::oauth::OAuthFlowStore::new()),
            permission_prompts: Arc::new(fx_kernel::PermissionPromptState::new()),
            ripcord: None,
            fleet_manager: None,
            cron_store: None,
            credential_store: None,
            experiment_registry: Arc::new(tokio::sync::Mutex::new(
                crate::experiment_registry::ExperimentRegistry::new(data_dir.as_path())
                    .expect("experiment registry"),
            )),
            improvement_provider: None,
            telemetry: in_memory_telemetry(),
        }
    }

    #[test]
    fn setup_token_response_serializes_expected_shape() {
        let response = SetupTokenResponse {
            provider: "anthropic".to_string(),
            status: "authenticated".to_string(),
            auth_method: "setup_token".to_string(),
            model_count: 0,
            verified: false,
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["provider"], "anthropic");
        assert_eq!(json["status"], "authenticated");
        assert_eq!(json["auth_method"], "setup_token");
        assert_eq!(json["model_count"], 0);
        assert_eq!(json["verified"], false);
    }

    #[test]
    fn api_key_response_serializes_expected_shape() {
        let response = ApiKeyResponse {
            provider: "openai".to_string(),
            status: "authenticated".to_string(),
            auth_method: "api_key".to_string(),
            model_count: 0,
            verified: false,
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["provider"], "openai");
        assert_eq!(json["status"], "authenticated");
        assert_eq!(json["auth_method"], "api_key");
        assert_eq!(json["model_count"], 0);
        assert_eq!(json["verified"], false);
    }

    #[test]
    fn delete_provider_response_serializes_expected_shape() {
        let response = DeleteProviderResponse {
            provider: "anthropic".to_string(),
            removed: true,
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["provider"], "anthropic");
        assert_eq!(json["removed"], true);
    }

    #[test]
    fn verify_response_serializes_expected_shape() {
        let response = VerifyResponse {
            provider: "anthropic".to_string(),
            verified: true,
            status: "authenticated".to_string(),
            message: "Credentials verified successfully.".to_string(),
            checked_at: 1_742_000_000,
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["provider"], "anthropic");
        assert_eq!(json["verified"], true);
        assert_eq!(json["status"], "authenticated");
        assert_eq!(json["message"], "Credentials verified successfully.");
        assert_eq!(json["checked_at"], 1_742_000_000);
    }

    #[test]
    fn verification_request_maps_anthropic_setup_tokens() {
        let auth_method = AuthMethod::SetupToken {
            token: "setup-token-123".to_string(),
        };

        let request = verification_request("anthropic", &auth_method).expect("verify request");

        assert_eq!(request.0, "anthropic");
        assert_eq!(request.1, "setup-token-123");
        assert_eq!(request.2, "setup_token");
    }

    #[tokio::test]
    async fn save_auth_method_preserves_provider_model_cache() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        fx_config::update_provider_model_cache(
            temp.path(),
            "openrouter",
            &[
                "anthropic/claude-sonnet-4.6".to_string(),
                "openai/gpt-5.4".to_string(),
            ],
        )
        .expect("seed provider model cache");
        let state = test_state(temp.path().to_path_buf());

        save_auth_method(
            &state,
            "openrouter",
            AuthMethod::ApiKey {
                provider: "openrouter".to_string(),
                key: "or-test-key".to_string(),
            },
        )
        .await
        .expect("save auth");

        let cache =
            fx_config::load_provider_model_cache(temp.path()).expect("reload provider model cache");
        assert_eq!(
            cache.models_for("openrouter"),
            Some(vec![
                "anthropic/claude-sonnet-4.6".to_string(),
                "openai/gpt-5.4".to_string(),
            ])
        );
    }
}
