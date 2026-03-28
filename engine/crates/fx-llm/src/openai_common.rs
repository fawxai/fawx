use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiModelsResponse {
    #[serde(default)]
    pub(crate) data: Vec<OpenAiModel>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiModel {
    pub(crate) id: String,
}

pub(crate) fn filter_model_ids(
    models: Vec<OpenAiModel>,
    supported_models: &[String],
    is_chat_capable: impl Fn(&str) -> bool,
) -> Vec<String> {
    models
        .into_iter()
        .filter_map(|model| filter_model_id(&model.id, supported_models, &is_chat_capable))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn filter_model_id(
    model_id: &str,
    supported_models: &[String],
    is_chat_capable: &impl Fn(&str) -> bool,
) -> Option<String> {
    if supported_models
        .iter()
        .any(|supported| supported == model_id)
    {
        return Some(model_id.to_string());
    }
    is_chat_capable(model_id).then(|| model_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::{filter_model_ids, OpenAiModel};

    #[test]
    fn filter_model_ids_keeps_supported_and_chat_models() {
        let models = vec![
            OpenAiModel {
                id: "gpt-4.1".to_string(),
            },
            OpenAiModel {
                id: "text-embedding-3-small".to_string(),
            },
            OpenAiModel {
                id: "custom-supported".to_string(),
            },
            OpenAiModel {
                id: "custom-supported".to_string(),
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
}
