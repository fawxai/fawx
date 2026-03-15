use crate::auth_store::AuthStore;
use crate::state::HttpState;
use crate::types::{
    ApiKeyRequest, ApiKeyResponse, DeleteProviderResponse, ErrorBody, SetupTokenRequest,
    SetupTokenResponse, VerifyRequest, VerifyResponse,
};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

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
    )?;

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
    )?;

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
    delete_provider_auth(&state, &provider)?;

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
    let _ = (state, request);

    // TODO: make lightweight API call to verify credentials
    let checked_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(Json(VerifyResponse {
        provider,
        verified: false,
        status: "unverified".to_string(),
        message: "Verification not yet implemented.".to_string(),
        checked_at,
    }))
}

pub(super) fn save_auth_method(
    state: &HttpState,
    provider: &str,
    auth_method: fx_auth::auth::AuthMethod,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    let store = AuthStore::open(&state.data_dir).map_err(internal_error)?;
    let mut auth_manager = store.load_auth_manager().map_err(internal_error)?;
    auth_manager.store(provider, auth_method);
    store
        .save_auth_manager(&auth_manager)
        .map_err(internal_error)
}

fn delete_provider_auth(
    state: &HttpState,
    provider: &str,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    let store = AuthStore::open(&state.data_dir).map_err(internal_error)?;
    let mut auth_manager = store.load_auth_manager().map_err(internal_error)?;
    auth_manager.remove(provider);
    store
        .save_auth_manager(&auth_manager)
        .map_err(internal_error)
}

fn internal_error(error: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorBody { error }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            verified: false,
            status: "unverified".to_string(),
            message: "Verification not yet implemented.".to_string(),
            checked_at: 1_742_000_000,
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["provider"], "anthropic");
        assert_eq!(json["verified"], false);
        assert_eq!(json["status"], "unverified");
        assert_eq!(json["message"], "Verification not yet implemented.");
        assert_eq!(json["checked_at"], 1_742_000_000);
    }
}
