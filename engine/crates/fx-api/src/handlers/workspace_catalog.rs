use crate::handlers::entity_ids::stable_entity_id;
use crate::handlers::git_inspect::{
    inspect_repository, is_git_repo, resolve_repository_identity_root,
};
use crate::state::HttpState;
use crate::types::{RepositorySummary, WorkspaceKind, WorkspaceSummary, WorktreeSummary};
use anyhow::{anyhow, Context, Result};
use fx_session::{SessionArchiveFilter, SessionInfo, SessionRegistry, SessionThreadBinding};
use std::path::{Path, PathBuf};

pub(crate) const GENERAL_WORKSPACE_ID: &str = "workspace-general";

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceCatalog {
    pub general: WorkspaceSummary,
    pub repository: Option<RepositoryWorkspace>,
}

#[derive(Debug, Clone)]
pub(crate) struct TargetedWorkspaceCatalog {
    pub catalog: WorkspaceCatalog,
    pub requested_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryWorkspace {
    pub summary: WorkspaceSummary,
    pub worktrees: Vec<WorktreeSummary>,
}

#[derive(Debug, Clone)]
pub(crate) enum WorkspaceSelection<'a> {
    General(&'a WorkspaceSummary),
    Repository(&'a RepositoryWorkspace),
}

impl WorkspaceCatalog {
    pub(crate) fn summaries(&self) -> Vec<WorkspaceSummary> {
        let mut workspaces = vec![self.general.clone()];
        if let Some(repository) = &self.repository {
            workspaces.push(repository.summary.clone());
        }
        workspaces
    }

    pub(crate) fn selection(&self, workspace_id: &str) -> Option<WorkspaceSelection<'_>> {
        if workspace_id == self.general.id {
            return Some(WorkspaceSelection::General(&self.general));
        }
        self.repository
            .as_ref()
            .filter(|repository| repository.summary.id == workspace_id)
            .map(WorkspaceSelection::Repository)
    }
}

pub(crate) async fn resolve_workspace_dir(state: &HttpState) -> Result<PathBuf> {
    if let Some(path) = configured_workspace_dir(state).await? {
        return Ok(path);
    }
    if let Some(parent) = state.data_dir.parent().filter(|path| is_git_repo(path)) {
        return Ok(parent.to_path_buf());
    }
    Ok(std::env::current_dir().unwrap_or_else(|_| state.data_dir.clone()))
}

pub(crate) async fn resolve_session_execution_root(
    state: &HttpState,
    session: &SessionInfo,
) -> Result<PathBuf> {
    if let Some(binding) = &session.thread_binding {
        if let Some(path) = binding
            .execution_root
            .as_deref()
            .or(binding.worktree_path.as_deref())
        {
            return Ok(PathBuf::from(path));
        }
        if let Some(path) = resolve_bound_workspace_root(state, binding).await? {
            return Ok(path);
        }
    }
    resolve_workspace_dir(state).await
}

pub(crate) async fn resolve_selection_execution_root(
    state: &HttpState,
    selection: &WorkspaceSelection<'_>,
    worktree: Option<&WorktreeSummary>,
    requested_root: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(worktree) = worktree {
        return Ok(PathBuf::from(&worktree.path));
    }
    if let Some(path) = selection_workspace_root(selection) {
        return Ok(path);
    }
    if let Some(path) = requested_root {
        return Ok(path.to_path_buf());
    }
    resolve_workspace_dir(state).await
}

pub(crate) async fn load_workspace_catalog(state: &HttpState) -> Result<WorkspaceCatalog> {
    Ok(load_targeted_workspace_catalog(state, None).await?.catalog)
}

pub(crate) async fn load_targeted_workspace_catalog(
    state: &HttpState,
    requested_path: Option<&str>,
) -> Result<TargetedWorkspaceCatalog> {
    let session_last_opened_at = active_session_last_opened_at(state.session_registry.clone())?;
    let requested_root = resolve_requested_workspace_root(requested_path).await?;
    let repository = match requested_root.as_ref() {
        Some(root) => {
            discover_repository_workspace_from_root(root.clone(), session_last_opened_at).await?
        }
        None => discover_repository_workspace(state, session_last_opened_at).await?,
    };
    let general = WorkspaceSummary {
        id: GENERAL_WORKSPACE_ID.to_string(),
        name: "General".to_string(),
        path: String::new(),
        kind: WorkspaceKind::General,
        repo: None,
        last_opened_at: if repository.is_some() {
            0
        } else {
            session_last_opened_at
        },
    };
    Ok(TargetedWorkspaceCatalog {
        catalog: WorkspaceCatalog {
            general,
            repository,
        },
        requested_root,
    })
}

async fn configured_workspace_dir(state: &HttpState) -> Result<Option<PathBuf>> {
    let manager = {
        let app = state.app.lock().await;
        app.config_manager()
    };
    let Some(manager) = manager else {
        return Ok(None);
    };
    let guard = manager
        .lock()
        .map_err(|_| anyhow!("config manager lock poisoned"))?;
    Ok(guard
        .config()
        .tools
        .working_dir
        .clone()
        .or_else(|| guard.config().workspace.root.clone()))
}

fn active_session_last_opened_at(registry: Option<SessionRegistry>) -> Result<u64> {
    let Some(registry) = registry else {
        return Ok(0);
    };
    let sessions = registry.list_with_archive_filter(None, SessionArchiveFilter::ActiveOnly)?;
    Ok(sessions
        .into_iter()
        .map(|session| session.updated_at)
        .max()
        .unwrap_or(0))
}

async fn discover_repository_workspace(
    state: &HttpState,
    last_opened_at: u64,
) -> Result<Option<RepositoryWorkspace>> {
    let workspace_dir = resolve_workspace_dir(state).await?;
    discover_repository_workspace_from_root(workspace_dir, last_opened_at).await
}

async fn discover_repository_workspace_from_root(
    workspace_dir: PathBuf,
    last_opened_at: u64,
) -> Result<Option<RepositoryWorkspace>> {
    let Some(repo_root) = resolve_repository_identity_root(&workspace_dir).await? else {
        return Ok(None);
    };

    Ok(Some(
        repository_workspace_from_paths(repo_root, workspace_dir, last_opened_at).await?,
    ))
}

pub(crate) async fn resolve_requested_workspace_root(
    requested_path: Option<&str>,
) -> Result<Option<PathBuf>> {
    let Some(requested_path) = requested_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
    else {
        return Ok(None);
    };

    Ok(Some(canonicalize_workspace_root(requested_path).await?))
}

async fn canonicalize_workspace_root(requested_path: String) -> Result<PathBuf> {
    let canonical_path = tokio::fs::canonicalize(&requested_path)
        .await
        .with_context(|| format!("failed to resolve workspace path: {requested_path}"))?;
    let metadata = tokio::fs::metadata(&canonical_path)
        .await
        .with_context(|| {
            format!(
                "failed to inspect workspace path: {}",
                canonical_path.display()
            )
        })?;
    if !metadata.is_dir() {
        return Err(anyhow!("workspace path is not a directory"));
    }

    Ok(canonical_path)
}

pub(crate) async fn repository_workspace_from_paths(
    repo_root: PathBuf,
    active_worktree_root: PathBuf,
    last_opened_at: u64,
) -> Result<RepositoryWorkspace> {
    let workspace_id = stable_entity_id("workspace", &repo_root.to_string_lossy());
    let repository = inspect_repository(&repo_root, &active_worktree_root, &workspace_id).await?;
    let summary = WorkspaceSummary {
        id: workspace_id,
        name: workspace_name(&repository.repo_root),
        path: repository.repo_root.to_string_lossy().into_owned(),
        kind: WorkspaceKind::Repository,
        repo: Some(RepositorySummary {
            root: repository.repo_root.to_string_lossy().into_owned(),
            vcs: "git".to_string(),
            current_branch: repository.current_branch,
            default_branch: repository.default_branch,
            origin: repository.origin,
            clean: repository.clean,
        }),
        last_opened_at,
    };

    Ok(RepositoryWorkspace {
        summary,
        worktrees: repository.worktrees,
    })
}

pub(crate) fn workspace_name(repo_root: &std::path::Path) -> String {
    repo_root
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| repo_root.to_string_lossy().into_owned())
}

async fn resolve_bound_workspace_root(
    state: &HttpState,
    binding: &SessionThreadBinding,
) -> Result<Option<PathBuf>> {
    if binding.workspace_id == GENERAL_WORKSPACE_ID {
        return Ok(None);
    }
    let catalog = load_workspace_catalog(state).await?;
    Ok(catalog
        .selection(&binding.workspace_id)
        .and_then(|selection| selection_workspace_root(&selection)))
}

fn selection_workspace_root(selection: &WorkspaceSelection<'_>) -> Option<PathBuf> {
    match selection {
        WorkspaceSelection::General(workspace) => {
            (!workspace.path.is_empty()).then(|| PathBuf::from(&workspace.path))
        }
        WorkspaceSelection::Repository(workspace) => Some(PathBuf::from(&workspace.summary.path)),
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_requested_workspace_root;
    use tempfile::tempdir;

    #[tokio::test]
    async fn resolve_requested_workspace_root_ignores_blank_input() {
        assert_eq!(
            resolve_requested_workspace_root(Some("   "))
                .await
                .expect("blank input"),
            None
        );
        assert_eq!(
            resolve_requested_workspace_root(None)
                .await
                .expect("missing input"),
            None
        );
    }

    #[tokio::test]
    async fn resolve_requested_workspace_root_canonicalizes_directories() {
        let workspace = tempdir().expect("tempdir");

        let resolved = resolve_requested_workspace_root(Some(
            workspace.path().to_str().expect("workspace path utf8"),
        ))
        .await
        .expect("resolve workspace root");

        assert_eq!(
            resolved,
            Some(std::fs::canonicalize(workspace.path()).expect("canonical workspace path"))
        );
    }

    #[tokio::test]
    async fn resolve_requested_workspace_root_rejects_files() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("not-a-directory.txt");
        std::fs::write(&file_path, "hello").expect("write file");

        let error =
            resolve_requested_workspace_root(Some(file_path.to_str().expect("file path utf8")))
                .await
                .expect_err("file path should be rejected");

        assert_eq!(error.to_string(), "workspace path is not a directory");
    }
}
