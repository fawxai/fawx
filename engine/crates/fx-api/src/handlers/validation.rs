use crate::handlers::workspaces::bad_request;
use crate::types::ErrorBody;
use axum::http::StatusCode;
use axum::Json;

pub(crate) fn normalized_required_field(
    value: String,
    label: &str,
) -> Result<String, (StatusCode, Json<ErrorBody>)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(bad_request(format!("{label} must not be empty")))
    } else {
        Ok(trimmed.to_string())
    }
}

pub(crate) fn normalized_optional_field(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(crate) fn normalized_optional_nonempty_field(
    value: Option<String>,
    label: &str,
) -> Result<Option<String>, (StatusCode, Json<ErrorBody>)> {
    match value {
        Some(value) => normalized_required_field(value, label).map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_optional_nonempty_field_rejects_blank_values() {
        let error = normalized_optional_nonempty_field(Some("   ".to_string()), "thread title")
            .expect_err("blank value should fail");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(error.1 .0.error, "thread title must not be empty");
    }

    #[test]
    fn normalized_optional_field_trims_nonempty_values() {
        assert_eq!(
            normalized_optional_field(Some("  origin/main  ".to_string())),
            Some("origin/main".to_string())
        );
    }
}
