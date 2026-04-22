use crate::handlers::entity_ids::stable_entity_id;
use crate::handlers::git_exec::{run_git, GitError, GitErrorKind};
use crate::handlers::sessions::require_session_registry;
use crate::handlers::validation::{normalized_optional_field, normalized_required_field};
use crate::handlers::workspace_catalog::{load_targeted_workspace_catalog, WorkspaceSelection};
use crate::handlers::workspaces::{bad_request, conflict, internal_error, workspace_not_found};
use crate::state::HttpState;
use crate::types::{
    ArchiveWorktreeResponse, AttachWorktreeThreadRequest, AttachWorktreeThreadResponse,
    CreateWorktreeRequest, DeleteWorktreeResponse, WorkspaceRouteQuery, WorkspaceScope,
    WorktreeStatus, WorktreeSummary, WorktreesResponse,
};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use fx_session::{
    SessionArchiveFilter, SessionInfo, SessionKey, SessionRegistry, SessionThreadBinding,
};
use std::path::{Path as FsPath, PathBuf};

use super::HandlerResult;

pub async fn handle_list_workspace_worktrees(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Query(query): Query<WorkspaceRouteQuery>,
) -> HandlerResult<Json<WorktreesResponse>> {
    let targeted_catalog =
        load_targeted_workspace_catalog(&state, query.workspace_scope.requested_path())
            .await
            .map_err(internal_error)?;
    let worktrees = match targeted_catalog.catalog.selection(&id) {
        Some(WorkspaceSelection::General(_)) => Vec::new(),
        Some(WorkspaceSelection::Repository(workspace)) => workspace.worktrees.clone(),
        None => return Err(workspace_not_found(&id)),
    };
    let total = worktrees.len();
    Ok(Json(WorktreesResponse { worktrees, total }))
}

pub async fn handle_create_worktree(
    State(state): State<HttpState>,
    Json(request): Json<CreateWorktreeRequest>,
) -> HandlerResult<(StatusCode, Json<WorktreeSummary>)> {
    let branch = normalized_required_field(request.branch, "branch")?;
    let base_ref = normalized_optional_field(request.base_ref);
    let repo_root =
        resolve_repo_root_for_workspace(&state, &request.workspace_id, &request.workspace_scope)
            .await?;
    let destination = create_git_worktree(&repo_root, &branch, base_ref.as_deref()).await?;

    let refreshed_catalog =
        load_targeted_workspace_catalog(&state, request.workspace_scope.requested_path())
            .await
            .map_err(internal_error)?;
    let refreshed_workspace = match refreshed_catalog.catalog.selection(&request.workspace_id) {
        Some(WorkspaceSelection::Repository(workspace)) => workspace,
        Some(WorkspaceSelection::General(_)) => {
            return Err(internal_error(anyhow::anyhow!(
                "workspace selection changed while creating worktree"
            )));
        }
        None => return Err(workspace_not_found(&request.workspace_id)),
    };
    let created = refreshed_workspace
        .worktrees
        .iter()
        .find(|worktree| worktree.path == destination.to_string_lossy())
        .cloned()
        .ok_or_else(|| {
            internal_error(anyhow::anyhow!(
                "created worktree was not visible in refreshed catalog"
            ))
        })?;

    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn handle_attach_worktree_thread(
    State(state): State<HttpState>,
    Path(worktree_id): Path<String>,
    Query(query): Query<WorkspaceRouteQuery>,
    Json(request): Json<AttachWorktreeThreadRequest>,
) -> HandlerResult<Json<AttachWorktreeThreadResponse>> {
    let registry = require_session_registry(&state)?;
    let (workspace_id, worktree) =
        resolve_worktree_target(&state, &worktree_id, &query.workspace_scope).await?;
    let thread_id = normalized_required_field(request.thread_id, "thread id")?;
    let session = resolve_session_for_thread_id(&registry, &thread_id)?;
    let binding = SessionThreadBinding {
        workspace_id,
        execution_root: Some(worktree.path.clone()),
        worktree_path: Some(worktree.path.clone()),
    };
    registry
        .set_thread_binding(&session.key, Some(binding))
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;

    Ok(Json(AttachWorktreeThreadResponse {
        worktree_id,
        thread_id,
        active_session_id: session.key.to_string(),
    }))
}

pub async fn handle_archive_worktree(
    State(state): State<HttpState>,
    Path(worktree_id): Path<String>,
    Query(query): Query<WorkspaceRouteQuery>,
) -> HandlerResult<Json<ArchiveWorktreeResponse>> {
    let registry = require_session_registry(&state)?;
    let (_, worktree) =
        resolve_worktree_target(&state, &worktree_id, &query.workspace_scope).await?;
    let session_keys = attached_session_keys(&registry, &worktree.path)?;

    for session_key in &session_keys {
        registry
            .archive(session_key)
            .map_err(|error| internal_error(anyhow::Error::new(error)))?;
    }

    Ok(Json(ArchiveWorktreeResponse {
        worktree_id,
        archived_thread_count: session_keys.len(),
    }))
}

pub async fn handle_delete_worktree(
    State(state): State<HttpState>,
    Path(worktree_id): Path<String>,
    Query(query): Query<WorkspaceRouteQuery>,
) -> HandlerResult<Json<DeleteWorktreeResponse>> {
    let registry = require_session_registry(&state)?;
    let (workspace_id, worktree) =
        resolve_worktree_target(&state, &worktree_id, &query.workspace_scope).await?;
    if worktree.status == WorktreeStatus::Active {
        return Err(conflict("active worktree cannot be removed"));
    }

    for session_key in attached_session_keys(&registry, &worktree.path)? {
        registry
            .archive(&session_key)
            .map_err(|error| internal_error(anyhow::Error::new(error)))?;
    }

    let repo_root =
        resolve_repo_root_for_workspace(&state, &workspace_id, &query.workspace_scope).await?;
    run_git(
        ["worktree", "remove", "--force", worktree.path.as_str()],
        &repo_root,
    )
    .await
    .map_err(git_request_error)?;

    Ok(Json(DeleteWorktreeResponse {
        deleted: true,
        worktree_id,
    }))
}

async fn resolve_worktree_target(
    state: &HttpState,
    worktree_id: &str,
    workspace_scope: &WorkspaceScope,
) -> HandlerResult<(String, WorktreeSummary)> {
    let targeted_catalog = load_targeted_workspace_catalog(state, workspace_scope.requested_path())
        .await
        .map_err(internal_error)?;
    let Some(workspace) = targeted_catalog.catalog.repository.as_ref() else {
        return Err(workspace_not_found(worktree_id));
    };
    let worktree = workspace
        .worktrees
        .iter()
        .find(|candidate| candidate.id == worktree_id)
        .cloned()
        .ok_or_else(|| workspace_not_found(worktree_id))?;
    Ok((workspace.summary.id.clone(), worktree))
}

async fn resolve_repo_root_for_workspace(
    state: &HttpState,
    workspace_id: &str,
    workspace_scope: &WorkspaceScope,
) -> HandlerResult<PathBuf> {
    let targeted_catalog = load_targeted_workspace_catalog(state, workspace_scope.requested_path())
        .await
        .map_err(internal_error)?;
    match targeted_catalog.catalog.selection(workspace_id) {
        Some(WorkspaceSelection::Repository(workspace)) => {
            Ok(PathBuf::from(&workspace.summary.path))
        }
        Some(WorkspaceSelection::General(_)) => Err(bad_request(
            "general workspace does not have a git worktree root",
        )),
        None => Err(workspace_not_found(workspace_id)),
    }
}

async fn create_git_worktree(
    repo_root: &FsPath,
    branch: &str,
    base_ref: Option<&str>,
) -> HandlerResult<PathBuf> {
    let destination = next_worktree_destination(repo_root, branch);
    prepare_worktree_parent_directory(&destination)?;
    let args = worktree_add_args(&destination, branch, base_ref);
    run_git(args, repo_root).await.map_err(git_request_error)?;
    configure_worktree_upstream(&destination, branch, base_ref).await;
    Ok(destination)
}

fn resolve_session_for_thread_id(
    registry: &SessionRegistry,
    thread_id: &str,
) -> HandlerResult<SessionInfo> {
    let sessions = registry
        .list_with_archive_filter(None, SessionArchiveFilter::ActiveOnly)
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;
    sessions
        .into_iter()
        .find(|session| stable_entity_id("thread", session.key.as_str()) == thread_id)
        .ok_or_else(|| bad_request(format!("thread not found: {thread_id}")))
}

fn attached_session_keys(
    registry: &SessionRegistry,
    worktree_path: &str,
) -> HandlerResult<Vec<SessionKey>> {
    let sessions = registry
        .list_with_archive_filter(None, SessionArchiveFilter::ActiveOnly)
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;
    Ok(sessions
        .into_iter()
        .filter(|session| {
            session
                .thread_binding
                .as_ref()
                .and_then(|binding| binding.worktree_path.as_deref())
                == Some(worktree_path)
        })
        .map(|session| session.key)
        .collect())
}

fn prepare_worktree_parent_directory(destination: &FsPath) -> HandlerResult<()> {
    let Some(parent) = destination.parent() else {
        return Ok(());
    };

    std::fs::create_dir_all(parent).map_err(|error| {
        internal_error(anyhow::anyhow!(
            "failed to prepare worktree directory {}: {error}",
            parent.display()
        ))
    })
}

fn worktree_add_args(destination: &FsPath, branch: &str, base_ref: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "worktree".to_string(),
        "add".to_string(),
        "-b".to_string(),
        branch.to_string(),
        destination.to_string_lossy().into_owned(),
    ];
    if let Some(base_ref) = base_ref {
        args.push(base_ref.to_string());
    }
    args
}

async fn configure_worktree_upstream(destination: &FsPath, branch: &str, base_ref: Option<&str>) {
    let Some(base_ref) = base_ref else {
        return;
    };

    let _ = run_git(
        ["branch", "--set-upstream-to", base_ref, branch],
        destination,
    )
    .await;
}

fn next_worktree_destination(repo_root: &FsPath, branch: &str) -> PathBuf {
    let repo_name = repo_root
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("workspace");
    let slug = branch_path_slug(branch);
    let parent = repo_root.parent().unwrap_or_else(|| FsPath::new("/tmp"));
    let base_name = format!("{repo_name}-{slug}");
    let mut candidate = parent.join(&base_name);
    let mut suffix = 2_u32;

    while candidate.exists() {
        candidate = parent.join(format!("{base_name}-{suffix}"));
        suffix += 1;
    }

    candidate
}

fn branch_path_slug(branch: &str) -> String {
    let mut slug = String::with_capacity(branch.len());
    for character in branch.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else {
            slug.push('-');
        }
    }

    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "worktree".to_string()
    } else {
        slug.to_string()
    }
}

fn git_request_error(error: GitError) -> (StatusCode, Json<crate::types::ErrorBody>) {
    if matches!(error.kind(), GitErrorKind::Invocation) {
        return internal_error(anyhow::anyhow!(error.to_string()));
    }

    let message = error.to_string();
    let normalized_message = message.to_ascii_lowercase();

    if git_conflict_error(&normalized_message) {
        return conflict(message);
    }

    if git_bad_request_error(&normalized_message) {
        return bad_request(message);
    }

    internal_error(anyhow::anyhow!(message))
}

fn git_conflict_error(message: &str) -> bool {
    [
        "already exists",
        "already checked out",
        "already used by worktree",
        "branch named",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
}

fn git_bad_request_error(message: &str) -> bool {
    [
        "not a git repository",
        "invalid reference",
        "not a valid object name",
        "unknown revision",
        "bad revision",
        "ambiguous argument",
        "needed a single revision",
        "could not resolve",
        "worktree not found",
        "is not a working tree",
    ]
    .iter()
    .any(|pattern| message.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_request_error_returns_400_for_invalid_git_input() {
        let error = GitError::command_failed("git error: invalid reference: missing-base");
        let (status, body) = git_request_error(error);

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.0.error, "git error: invalid reference: missing-base");
    }

    #[test]
    fn git_request_error_returns_500_for_server_side_git_failures() {
        let error = GitError::command_failed("git error: permission denied");
        let (status, body) = git_request_error(error);

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.0.error, "git error: permission denied");
    }
}
