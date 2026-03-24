use crate::error::HttpError;
use fx_config::HttpConfig;

pub trait BearerTokenStore {
    fn get_provider_token(&self, provider: &str) -> Result<Option<String>, String>;
}

pub fn validate_bearer_token(
    config: &HttpConfig,
    auth_store: Option<&dyn BearerTokenStore>,
) -> Result<String, HttpError> {
    if let Some(store) = auth_store {
        if let Ok(Some(token)) = store.get_provider_token("http_bearer") {
            let trimmed = token.trim().to_string();
            if !trimmed.is_empty() {
                return Ok(trimmed);
            }
        }
    }

    match &config.bearer_token {
        Some(token) => {
            let trimmed = token.trim().to_string();
            if trimmed.is_empty() {
                Err(HttpError::MissingBearerToken)
            } else {
                Ok(trimmed)
            }
        }
        _ => Err(HttpError::MissingBearerToken),
    }
}
