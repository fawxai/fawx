use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use fx_kernel::{PermissionDecision, PromptError, ResolveResult};
use serde::{Deserialize, Serialize};

use super::HandlerResult;

#[derive(Debug, Deserialize)]
pub struct PromptRespondRequest {
    pub decision: String,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptRespondResponse {
    pub id: String,
    pub resolved: bool,
    pub decision: String,
    pub scope: String,
    pub status: String,
    pub message: String,
}

pub async fn handle_respond(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Json(request): Json<PromptRespondRequest>,
) -> HandlerResult<Json<PromptRespondResponse>> {
    let decision = parse_decision(&request)?;
    let result = state
        .permission_prompts
        .resolve(&id, decision)
        .map_err(|error| prompt_error_response(&id, error))?;

    Ok(Json(build_respond_response(id, &result)))
}

fn build_respond_response(id: String, result: &ResolveResult) -> PromptRespondResponse {
    let (decision, scope) = match result.decision {
        PermissionDecision::Allow => ("allow", "once"),
        PermissionDecision::AllowSession => ("allow", "session"),
        PermissionDecision::Deny => ("deny", "once"),
    };
    let status = if matches!(
        result.decision,
        PermissionDecision::Allow | PermissionDecision::AllowSession
    ) {
        "approved"
    } else {
        "denied"
    };

    PromptRespondResponse {
        id,
        resolved: true,
        decision: decision.to_string(),
        scope: scope.to_string(),
        status: status.to_string(),
        message: resolve_message(result),
    }
}

fn parse_decision(
    request: &PromptRespondRequest,
) -> Result<PermissionDecision, (StatusCode, Json<ErrorBody>)> {
    let scope_is_session = request.scope.as_deref() == Some("session");
    match request.decision.as_str() {
        "allow" if scope_is_session => Ok(PermissionDecision::AllowSession),
        "allow" => Ok(PermissionDecision::Allow),
        "deny" => Ok(PermissionDecision::Deny),
        other => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorBody {
                error: format!("Unknown decision '{other}'; expected 'allow' or 'deny'"),
            }),
        )),
    }
}

fn resolve_message(result: &ResolveResult) -> String {
    match result.decision {
        PermissionDecision::Allow => "Permission granted.".to_string(),
        PermissionDecision::AllowSession => {
            format!("Permission granted for '{}' for this session.", result.tool)
        }
        PermissionDecision::Deny => "Permission denied.".to_string(),
    }
}

fn prompt_error_response(id: &str, error: PromptError) -> (StatusCode, Json<ErrorBody>) {
    match error {
        PromptError::NotFound => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: format!("Prompt '{id}' not found"),
            }),
        ),
        PromptError::Expired => (
            StatusCode::CONFLICT,
            Json(ErrorBody {
                error: format!("Prompt '{id}' expired — treated as denied"),
            }),
        ),
        PromptError::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "Internal error".to_string(),
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respond_request_deserializes() {
        let request: PromptRespondRequest =
            serde_json::from_str(r#"{"decision":"allow","scope":"session"}"#)
                .expect("deserialize request");

        assert_eq!(request.decision, "allow");
        assert_eq!(request.scope.as_deref(), Some("session"));
    }

    #[test]
    fn respond_response_serializes() {
        let response = PromptRespondResponse {
            id: "prompt-1".to_string(),
            resolved: true,
            decision: "allow".to_string(),
            scope: "session".to_string(),
            status: "approved".to_string(),
            message: "Permission granted for 'shell' for this session.".to_string(),
        };

        let json = serde_json::to_value(&response).expect("serialize response");
        assert_eq!(json["id"], "prompt-1");
        assert_eq!(json["resolved"], true);
        assert_eq!(json["decision"], "allow");
        assert_eq!(json["scope"], "session");
        assert_eq!(json["status"], "approved");
        assert_eq!(
            json["message"],
            "Permission granted for 'shell' for this session."
        );
    }

    #[test]
    fn parse_decision_allow() {
        let request = PromptRespondRequest {
            decision: "allow".to_string(),
            scope: Some("once".to_string()),
        };

        let decision = parse_decision(&request).expect("parse allow");
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn parse_decision_deny() {
        let request = PromptRespondRequest {
            decision: "deny".to_string(),
            scope: Some("session".to_string()),
        };

        let decision = parse_decision(&request).expect("parse deny");
        assert_eq!(decision, PermissionDecision::Deny);
    }

    #[test]
    fn parse_decision_allow_session() {
        let request = PromptRespondRequest {
            decision: "allow".to_string(),
            scope: Some("session".to_string()),
        };

        let decision = parse_decision(&request).expect("parse allow session");
        assert_eq!(decision, PermissionDecision::AllowSession);
    }

    #[test]
    fn parse_decision_invalid() {
        let request = PromptRespondRequest {
            decision: "maybe".to_string(),
            scope: None,
        };

        let error = parse_decision(&request).expect_err("invalid decision should fail");
        assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            error.1 .0.error,
            "Unknown decision 'maybe'; expected 'allow' or 'deny'"
        );
    }

    #[test]
    fn prompt_error_response_expired_maps_to_conflict() {
        let error = prompt_error_response("prompt-1", PromptError::Expired);
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(
            error.1 .0.error,
            "Prompt 'prompt-1' expired — treated as denied"
        );
    }
}
