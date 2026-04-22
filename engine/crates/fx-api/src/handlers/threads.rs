use crate::handlers::entity_ids::stable_entity_id;
use crate::handlers::sessions::{
    create_session, require_session_registry, resolve_session_model, resolve_session_thinking,
};
use crate::handlers::validation::{normalized_optional_field, normalized_optional_nonempty_field};
use crate::handlers::workspace_catalog::{
    load_targeted_workspace_catalog, resolve_selection_execution_root, RepositoryWorkspace,
    WorkspaceSelection, GENERAL_WORKSPACE_ID,
};
use crate::handlers::workspaces::{bad_request, internal_error, workspace_not_found};
use crate::state::HttpState;
use crate::types::{
    CreateThreadRequest, ThreadKind, ThreadStatus, ThreadSummary, ThreadsResponse,
    WorkspaceRouteQuery,
};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use fx_session::{
    SessionArchiveFilter, SessionConfig, SessionInfo, SessionKind, SessionStatus,
    SessionThreadBinding,
};

use super::HandlerResult;

pub async fn handle_create_thread(
    State(state): State<HttpState>,
    Json(request): Json<CreateThreadRequest>,
) -> HandlerResult<(StatusCode, Json<ThreadSummary>)> {
    let targeted_catalog =
        load_targeted_workspace_catalog(&state, request.workspace_scope.requested_path())
            .await
            .map_err(internal_error)?;
    let selection = targeted_catalog
        .catalog
        .selection(&request.workspace_id)
        .ok_or_else(|| workspace_not_found(&request.workspace_id))?;
    let registry = require_session_registry(&state)?;
    let title = normalized_optional_nonempty_field(request.title, "thread title")?;
    let model = match normalized_optional_nonempty_field(request.model, "thread model")? {
        Some(model) => resolve_session_model(&state, &model).await?.model,
        None => {
            let app = state.app.lock().await;
            app.active_model().to_string()
        }
    };
    let thinking = match normalized_optional_nonempty_field(request.thinking, "thread thinking")? {
        Some(level) => Some(resolve_session_thinking(&state, &model, &level).await?),
        None => None,
    };
    let selected_worktree = requested_worktree(
        &selection,
        normalized_optional_field(request.worktree_id).as_deref(),
    )?;
    let info = create_session(
        &registry,
        SessionConfig {
            label: title,
            model,
            thinking,
        },
    )
    .map_err(internal_error)?;
    let thread_binding = SessionThreadBinding {
        workspace_id: selection_workspace_id(&selection).to_string(),
        execution_root: Some(
            resolve_selection_execution_root(
                &state,
                &selection,
                selected_worktree,
                targeted_catalog.requested_root.as_deref(),
            )
            .await
            .map_err(internal_error)?
            .to_string_lossy()
            .into_owned(),
        ),
        worktree_path: selected_worktree.map(|worktree| worktree.path.clone()),
    };
    registry
        .set_thread_binding(&info.key, Some(thread_binding))
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;
    let updated_info = registry
        .get_info(&info.key)
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;

    Ok((
        StatusCode::CREATED,
        Json(session_to_thread_summary(updated_info, &selection)),
    ))
}

pub async fn handle_list_workspace_threads(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    Query(query): Query<WorkspaceRouteQuery>,
) -> HandlerResult<Json<ThreadsResponse>> {
    let targeted_catalog =
        load_targeted_workspace_catalog(&state, query.workspace_scope.requested_path())
            .await
            .map_err(internal_error)?;
    let selection = targeted_catalog
        .catalog
        .selection(&id)
        .ok_or_else(|| workspace_not_found(&id))?;
    let registry = require_session_registry(&state)?;
    let mut sessions = registry
        .list_with_archive_filter(None, SessionArchiveFilter::ActiveOnly)
        .map_err(|error| internal_error(anyhow::Error::new(error)))?;
    sessions.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.key.as_str().cmp(right.key.as_str()))
    });

    let threads: Vec<_> = sessions
        .into_iter()
        .filter_map(|session| {
            if session_workspace_id(
                &session,
                targeted_catalog.catalog.repository.as_ref(),
                targeted_catalog.requested_root.is_none(),
            ) != selection_workspace_id(&selection)
            {
                return None;
            }

            Some(session_to_thread_summary(session, &selection))
        })
        .collect();
    let total = threads.len();
    Ok(Json(ThreadsResponse { threads, total }))
}

fn session_to_thread_summary(
    session: SessionInfo,
    selection: &WorkspaceSelection<'_>,
) -> ThreadSummary {
    let workspace_id = selection_workspace_id(selection).to_string();
    let thread_id = stable_entity_id("thread", session.key.as_str());
    let title = thread_title(&session);
    let kind = thread_kind(session.kind, workspace_id.as_str() != GENERAL_WORKSPACE_ID);
    let status = thread_status(session.status);
    let active_session_id = session.key.to_string();
    let worktree_id = match selection {
        WorkspaceSelection::General(_) => None,
        WorkspaceSelection::Repository(workspace) => session
            .thread_binding
            .as_ref()
            .and_then(|binding| binding.worktree_path.as_deref())
            .and_then(|worktree_path| {
                workspace
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.path == worktree_path)
                    .map(|worktree| worktree.id.clone())
            }),
    };
    ThreadSummary {
        id: thread_id,
        title,
        kind,
        workspace_id,
        worktree_id,
        active_session_id,
        status,
        preview: session.preview,
        model: session.model,
        thinking: session.thinking,
        created_at: session.created_at,
        updated_at: session.updated_at,
    }
}

fn session_workspace_id<'a>(
    session: &'a SessionInfo,
    repository: Option<&'a RepositoryWorkspace>,
    allow_legacy_repository_fallback: bool,
) -> &'a str {
    // Legacy sessions predate explicit thread binding, so we keep routing them to the
    // current repository workspace when one exists rather than orphaning them outright.
    // Explicitly scoped, off-catalog workspace routes must not claim every legacy
    // session, or the same active session appears in multiple workspace snapshots.
    session
        .thread_binding
        .as_ref()
        .map(|binding| binding.workspace_id.as_str())
        .or_else(|| {
            allow_legacy_repository_fallback
                .then(|| repository.map(|workspace| workspace.summary.id.as_str()))
                .flatten()
        })
        .unwrap_or(GENERAL_WORKSPACE_ID)
}

fn selection_workspace_id<'a>(selection: &'a WorkspaceSelection<'a>) -> &'a str {
    match selection {
        WorkspaceSelection::General(workspace) => workspace.id.as_str(),
        WorkspaceSelection::Repository(workspace) => workspace.summary.id.as_str(),
    }
}

fn thread_title(session: &SessionInfo) -> String {
    session
        .title
        .clone()
        .or_else(|| session.label.clone())
        .unwrap_or_else(|| session.key.to_string())
}

fn thread_kind(kind: SessionKind, repo_backed: bool) -> ThreadKind {
    match kind {
        SessionKind::Main => {
            if repo_backed {
                ThreadKind::Coding
            } else {
                ThreadKind::General
            }
        }
        SessionKind::Subagent => ThreadKind::Subagent,
        SessionKind::Cron => ThreadKind::Automation,
        SessionKind::Channel => ThreadKind::General,
    }
}

fn thread_status(status: SessionStatus) -> ThreadStatus {
    match status {
        SessionStatus::Active => ThreadStatus::Active,
        SessionStatus::Idle => ThreadStatus::Idle,
        SessionStatus::Completed => ThreadStatus::Completed,
        SessionStatus::Failed => ThreadStatus::Failed,
        SessionStatus::Paused => ThreadStatus::Paused,
    }
}

fn requested_worktree<'a>(
    selection: &WorkspaceSelection<'a>,
    worktree_id: Option<&str>,
) -> Result<Option<&'a crate::types::WorktreeSummary>, (StatusCode, Json<crate::types::ErrorBody>)>
{
    let Some(worktree_id) = worktree_id.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };

    match selection {
        WorkspaceSelection::General(_) => Err(bad_request(
            "general threads cannot attach to a git worktree",
        )),
        WorkspaceSelection::Repository(workspace) => workspace
            .worktrees
            .iter()
            .find(|worktree| worktree.id == worktree_id)
            .map(Some)
            .ok_or_else(|| bad_request(format!("worktree not found: {worktree_id}"))),
    }
}
