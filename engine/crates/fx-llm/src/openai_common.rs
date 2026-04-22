use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::types::LlmError;

const OPENAI_MODEL_READ_SCOPE: &str = "api.model.read";
const OPENAI_MODEL_CATALOG_SCOPE_ERROR: &str =
    "OpenAI model catalog unavailable; missing api.model.read scope";
const OPENAI_MODEL_CATALOG_SCOPE_ERROR_LOWER: &str =
    "openai model catalog unavailable; missing api.model.read scope";
static OPENAI_MODEL_CATALOG_RESTRICTED_KEY_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiModelsResponse {
    #[serde(default)]
    pub(crate) data: Vec<OpenAiModel>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiModel {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) architecture: Option<OpenAiModelArchitecture>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct OpenAiModelArchitecture {
    #[serde(default)]
    pub(crate) modality: Option<String>,
    #[serde(default)]
    pub(crate) input_modalities: Vec<String>,
    #[serde(default)]
    pub(crate) output_modalities: Vec<String>,
}

pub(crate) fn filter_model_ids(
    models: Vec<OpenAiModel>,
    supported_models: &[String],
    is_chat_capable: impl Fn(&str) -> bool,
) -> Vec<String> {
    models
        .into_iter()
        .filter_map(|model| filter_model_id(&model, supported_models, &is_chat_capable))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn is_missing_model_read_scope_response(
    status: reqwest::StatusCode,
    body: &str,
) -> bool {
    status == reqwest::StatusCode::FORBIDDEN
        && body.to_ascii_lowercase().contains(OPENAI_MODEL_READ_SCOPE)
}

pub(crate) fn missing_model_read_scope_error() -> LlmError {
    LlmError::Provider(OPENAI_MODEL_CATALOG_SCOPE_ERROR.to_string())
}

pub(crate) fn is_missing_model_read_scope_error(error: &LlmError) -> bool {
    match error {
        LlmError::Provider(message) | LlmError::Authentication(message) => {
            let message = message.to_ascii_lowercase();
            message.contains(OPENAI_MODEL_CATALOG_SCOPE_ERROR_LOWER)
                || message.contains(OPENAI_MODEL_READ_SCOPE)
        }
        _ => false,
    }
}

pub(crate) fn log_restricted_model_catalog_fallback_once(provider: &str) {
    if OPENAI_MODEL_CATALOG_RESTRICTED_KEY_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::debug!(
            provider = %provider,
            scope = OPENAI_MODEL_READ_SCOPE,
            "OpenAI model catalog unavailable; using fallback models"
        );
        return;
    }

    tracing::info!(
        provider = %provider,
        scope = OPENAI_MODEL_READ_SCOPE,
        "OpenAI model catalog unavailable for restricted key; using fallback models"
    );
}

fn filter_model_id(
    model: &OpenAiModel,
    supported_models: &[String],
    is_chat_capable: &impl Fn(&str) -> bool,
) -> Option<String> {
    let model_id = model.id.as_str();
    if supported_models
        .iter()
        .any(|supported| supported == model_id)
    {
        return Some(model_id.to_string());
    }

    model_architecture_supports_text_chat(model.architecture.as_ref())
        .unwrap_or_else(|| is_chat_capable(model_id))
        .then(|| model_id.to_string())
}

pub(crate) fn model_architecture_supports_text_chat(
    architecture: Option<&OpenAiModelArchitecture>,
) -> Option<bool> {
    let architecture = architecture?;
    let (input_modalities, output_modalities) = architecture_modalities(architecture);

    if input_modalities.is_empty() && output_modalities.is_empty() {
        return None;
    }

    let input_has_text = input_modalities.iter().any(|modality| modality == "text");
    let output_has_text = output_modalities.iter().any(|modality| modality == "text");

    if !input_modalities.is_empty() && !output_modalities.is_empty() {
        return Some(input_has_text && output_has_text);
    }

    if !input_modalities.is_empty() && !input_has_text {
        return Some(false);
    }

    if !output_modalities.is_empty() && !output_has_text {
        return Some(false);
    }

    None
}

fn architecture_modalities(architecture: &OpenAiModelArchitecture) -> (Vec<String>, Vec<String>) {
    let mut input_modalities = normalize_modalities(&architecture.input_modalities);
    let mut output_modalities = normalize_modalities(&architecture.output_modalities);

    if let Some((parsed_input, parsed_output)) =
        parse_modality_signature(architecture.modality.as_deref())
    {
        if input_modalities.is_empty() {
            input_modalities = parsed_input;
        }
        if output_modalities.is_empty() {
            output_modalities = parsed_output;
        }
    }

    (input_modalities, output_modalities)
}

fn normalize_modalities(modalities: &[String]) -> Vec<String> {
    modalities
        .iter()
        .map(|modality| modality.trim().to_ascii_lowercase())
        .filter(|modality| !modality.is_empty())
        .collect()
}

fn parse_modality_signature(signature: Option<&str>) -> Option<(Vec<String>, Vec<String>)> {
    let signature = signature?.trim();
    let (input, output) = signature.split_once("->")?;
    Some((split_modality_side(input), split_modality_side(output)))
}

fn split_modality_side(side: &str) -> Vec<String> {
    side.split('+')
        .map(|modality| modality.trim().to_ascii_lowercase())
        .filter(|modality| !modality.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        filter_model_ids, is_missing_model_read_scope_error, is_missing_model_read_scope_response,
        missing_model_read_scope_error, model_architecture_supports_text_chat, OpenAiModel,
        OpenAiModelArchitecture,
    };

    #[test]
    fn filter_model_ids_keeps_supported_and_chat_models() {
        let models = vec![
            OpenAiModel {
                id: "gpt-4.1".to_string(),
                architecture: None,
            },
            OpenAiModel {
                id: "text-embedding-3-small".to_string(),
                architecture: None,
            },
            OpenAiModel {
                id: "custom-supported".to_string(),
                architecture: None,
            },
            OpenAiModel {
                id: "custom-supported".to_string(),
                architecture: None,
            },
        ];

        let filtered = filter_model_ids(models, &["custom-supported".to_string()], |model_id| {
            model_id.starts_with("gpt-")
        });

        assert_eq!(
            filtered,
            vec!["custom-supported".to_string(), "gpt-4.1".to_string()]
        );
    }

    #[test]
    fn missing_model_read_scope_catalog_error_is_detected_without_raw_body() {
        let body = r#"{"error":"Missing scopes: api.model.read"}"#;

        assert!(is_missing_model_read_scope_response(
            reqwest::StatusCode::FORBIDDEN,
            body
        ));
        assert!(!is_missing_model_read_scope_response(
            reqwest::StatusCode::UNAUTHORIZED,
            body
        ));

        let error = missing_model_read_scope_error();
        assert!(is_missing_model_read_scope_error(&error));
        assert!(!error.to_string().contains("Missing scopes"));
    }

    #[test]
    fn filter_model_ids_prefers_architecture_metadata_over_name_heuristics() {
        let models = vec![
            OpenAiModel {
                id: "z-ai/glm-4.5-air:free".to_string(),
                architecture: Some(OpenAiModelArchitecture {
                    modality: Some("text->text".to_string()),
                    input_modalities: vec!["text".to_string()],
                    output_modalities: vec!["text".to_string()],
                }),
            },
            OpenAiModel {
                id: "openai/text-embedding-3-large".to_string(),
                architecture: Some(OpenAiModelArchitecture {
                    modality: Some("text->embedding".to_string()),
                    input_modalities: vec!["text".to_string()],
                    output_modalities: vec!["embedding".to_string()],
                }),
            },
        ];

        let filtered = filter_model_ids(models, &[], |_| false);

        assert_eq!(filtered, vec!["z-ai/glm-4.5-air:free".to_string()]);
    }

    #[test]
    fn model_architecture_supports_text_chat_requires_text_input_and_output() {
        let chat = OpenAiModelArchitecture {
            modality: Some("text+image->text".to_string()),
            input_modalities: vec!["text".to_string(), "image".to_string()],
            output_modalities: vec!["text".to_string()],
        };
        assert_eq!(
            model_architecture_supports_text_chat(Some(&chat)),
            Some(true)
        );

        let embedding = OpenAiModelArchitecture {
            modality: Some("text->embedding".to_string()),
            input_modalities: vec!["text".to_string()],
            output_modalities: vec!["embedding".to_string()],
        };
        assert_eq!(
            model_architecture_supports_text_chat(Some(&embedding)),
            Some(false)
        );

        let audio_only = OpenAiModelArchitecture {
            modality: Some("audio->text".to_string()),
            input_modalities: vec!["audio".to_string()],
            output_modalities: vec!["text".to_string()],
        };
        assert_eq!(
            model_architecture_supports_text_chat(Some(&audio_only)),
            Some(false)
        );

        let modality_only = OpenAiModelArchitecture {
            modality: Some("text->text".to_string()),
            input_modalities: Vec::new(),
            output_modalities: Vec::new(),
        };
        assert_eq!(
            model_architecture_supports_text_chat(Some(&modality_only)),
            Some(true)
        );
    }
}
