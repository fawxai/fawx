use crate::handlers::git_inspect::resolve_repository_identity_root;
use crate::handlers::workspace_catalog::{
    load_workspace_catalog, repository_workspace_from_paths, resolve_requested_workspace_root,
};
use crate::state::HttpState;
use crate::types::{ErrorBody, OpenWorkspaceRequest, WorkspaceSummary, WorkspacesResponse};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use super::HandlerResult;

pub async fn handle_list_workspaces(
    State(state): State<HttpState>,
) -> HandlerResult<Json<WorkspacesResponse>> {
    let catalog = load_workspace_catalog(&state)
        .await
        .map_err(internal_error)?;
    let workspaces = catalog.summaries();
    let total = workspaces.len();
    Ok(Json(WorkspacesResponse { workspaces, total }))
}

pub async fn handle_open_workspace(
    State(_state): State<HttpState>,
    Json(request): Json<OpenWorkspaceRequest>,
) -> HandlerResult<(StatusCode, Json<WorkspaceSummary>)> {
    let canonical_path = resolve_requested_workspace_root(Some(&request.path))
        .await
        .map_err(internal_error)?;
    let canonical_path =
        canonical_path.ok_or_else(|| bad_request("workspace path must not be empty"))?;

    let Some(repo_root) = resolve_repository_identity_root(&canonical_path)
        .await
        .map_err(internal_error)?
    else {
        return Err(bad_request(
            "workspace path must be inside a git repository",
        ));
    };

    let repository =
        repository_workspace_from_paths(repo_root, canonical_path, current_timestamp())
            .await
            .map_err(internal_error)?;

    Ok((StatusCode::CREATED, Json(repository.summary)))
}

pub(crate) fn workspace_not_found(id: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("workspace not found: {id}"),
        }),
    )
}

pub(crate) fn bad_request(message: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            error: message.into(),
        }),
    )
}

pub(crate) fn conflict(message: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::CONFLICT,
        Json(ErrorBody {
            error: message.into(),
        }),
    )
}

pub(crate) fn internal_error(error: anyhow::Error) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            error: error.to_string(),
        }),
    )
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
