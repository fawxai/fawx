use crate::handlers::entity_ids::stable_entity_id;
use crate::handlers::git_exec::run_git;
use crate::types::{WorktreeStatus, WorktreeSummary};
use anyhow::{anyhow, Context, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct RepositoryInspection {
    pub repo_root: PathBuf,
    pub current_branch: String,
    pub default_branch: Option<String>,
    pub origin: Option<String>,
    pub clean: bool,
    pub worktrees: Vec<WorktreeSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreeCandidate {
    path: PathBuf,
    branch_ref: Option<String>,
    detached: bool,
}

pub(crate) fn is_git_repo(path: &Path) -> bool {
    path.join(".git").is_dir() || path.join(".git").is_file()
}

pub(crate) async fn resolve_repo_root(path: &Path) -> Result<Option<PathBuf>> {
    match run_git(["rev-parse", "--show-toplevel"], path).await {
        Ok(root) => Ok(Some(PathBuf::from(root.trim()))),
        Err(error) if error.to_string().contains("not a git repository") => Ok(None),
        Err(error) => Err(anyhow::Error::new(error)),
    }
}

pub(crate) async fn resolve_repository_identity_root(path: &Path) -> Result<Option<PathBuf>> {
    let Some(worktree_root) = resolve_repo_root(path).await? else {
        return Ok(None);
    };

    let common_dir = run_git(["rev-parse", "--git-common-dir"], &worktree_root)
        .await
        .map_err(anyhow::Error::new)?;
    let common_dir = normalize_git_path(&worktree_root, common_dir.trim());
    let common_dir = std::fs::canonicalize(&common_dir).unwrap_or(common_dir);

    Ok(Some(
        common_dir
            .file_name()
            .filter(|name| *name == OsStr::new(".git"))
            .and_then(|_| common_dir.parent().map(Path::to_path_buf))
            .unwrap_or(worktree_root),
    ))
}

pub(crate) async fn inspect_repository(
    repo_root: &Path,
    active_worktree_root: &Path,
    workspace_id: &str,
) -> Result<RepositoryInspection> {
    let worktrees = list_worktrees(repo_root, active_worktree_root, workspace_id).await?;
    let active_worktree = worktrees
        .iter()
        .find(|worktree| worktree.status == WorktreeStatus::Active)
        .or_else(|| worktrees.first())
        .ok_or_else(|| anyhow!("repository inspection requires at least one worktree"))?;

    Ok(RepositoryInspection {
        repo_root: repo_root.to_path_buf(),
        current_branch: active_worktree.branch.clone(),
        default_branch: remote_default_branch(repo_root).await,
        origin: optional_git_stdout(["remote", "get-url", "origin"], repo_root).await?,
        clean: active_worktree.clean,
        worktrees,
    })
}

async fn list_worktrees(
    repo_root: &Path,
    active_worktree_root: &Path,
    workspace_id: &str,
) -> Result<Vec<WorktreeSummary>> {
    let candidates =
        parse_worktree_list(&run_git(["worktree", "list", "--porcelain"], repo_root).await?)?;
    let candidates = if candidates.is_empty() {
        vec![WorktreeCandidate {
            path: repo_root.to_path_buf(),
            branch_ref: None,
            detached: false,
        }]
    } else {
        candidates
    };

    let mut worktrees = Vec::new();
    for candidate in candidates {
        if !candidate.path.exists() {
            continue;
        }
        worktrees
            .push(build_worktree_summary(workspace_id, active_worktree_root, candidate).await?);
    }

    worktrees.sort_by(|left, right| {
        sort_rank(&left.status)
            .cmp(&sort_rank(&right.status))
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.path.cmp(&right.path))
    });

    Ok(worktrees)
}

async fn build_worktree_summary(
    workspace_id: &str,
    active_worktree_root: &Path,
    candidate: WorktreeCandidate,
) -> Result<WorktreeSummary> {
    let branch = worktree_branch(&candidate).await?;
    let base_ref = optional_git_stdout(
        ["rev-parse", "--abbrev-ref", "HEAD@{upstream}"],
        &candidate.path,
    )
    .await?;
    let (ahead_count, behind_count) = match base_ref.as_deref() {
        Some(upstream) => ahead_behind_counts(&candidate.path, upstream).await?,
        None => (0, 0),
    };
    let clean = run_git(["status", "--porcelain"], &candidate.path)
        .await
        .map(|output| output.trim().is_empty())
        .map_err(anyhow::Error::new)?;
    let status = if candidate.path == active_worktree_root {
        WorktreeStatus::Active
    } else if candidate.detached {
        WorktreeStatus::Detached
    } else {
        WorktreeStatus::Available
    };
    let path_string = candidate.path.to_string_lossy().into_owned();

    Ok(WorktreeSummary {
        id: stable_entity_id("worktree", &path_string),
        workspace_id: workspace_id.to_string(),
        label: worktree_label(&candidate.path, &branch),
        path: path_string,
        branch,
        base_ref,
        status,
        clean,
        ahead_count,
        behind_count,
    })
}

fn sort_rank(status: &WorktreeStatus) -> u8 {
    match status {
        WorktreeStatus::Active => 0,
        WorktreeStatus::Available => 1,
        WorktreeStatus::Detached => 2,
    }
}

fn normalize_git_path(base: &Path, value: &str) -> PathBuf {
    let candidate = PathBuf::from(value);
    if candidate.is_absolute() {
        candidate
    } else {
        base.join(candidate)
    }
}

async fn worktree_branch(candidate: &WorktreeCandidate) -> Result<String> {
    if let Some(branch) = optional_git_stdout(["branch", "--show-current"], &candidate.path).await?
    {
        if !branch.is_empty() {
            return Ok(branch);
        }
    }
    if let Some(branch_ref) = &candidate.branch_ref {
        return Ok(branch_name_from_ref(branch_ref));
    }
    if candidate.detached {
        let head = run_git(["rev-parse", "--short", "HEAD"], &candidate.path)
            .await
            .map_err(anyhow::Error::new)?;
        return Ok(format!("detached@{}", head.trim()));
    }
    Ok("HEAD".to_string())
}

fn worktree_label(path: &Path, branch: &str) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| branch.to_string())
}

fn branch_name_from_ref(branch_ref: &str) -> String {
    branch_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(branch_ref)
        .to_string()
}

async fn ahead_behind_counts(path: &Path, upstream: &str) -> Result<(u64, u64)> {
    let output = run_git(
        [
            "rev-list",
            "--left-right",
            "--count",
            &format!("HEAD...{upstream}"),
        ],
        path,
    )
    .await
    .map_err(anyhow::Error::new)?;
    parse_ahead_behind_counts(&output)
}

fn parse_ahead_behind_counts(output: &str) -> Result<(u64, u64)> {
    let mut parts = output.split_whitespace();
    let ahead = parts
        .next()
        .ok_or_else(|| anyhow!("missing ahead count"))?
        .parse::<u64>()
        .context("invalid ahead count")?;
    let behind = parts
        .next()
        .ok_or_else(|| anyhow!("missing behind count"))?
        .parse::<u64>()
        .context("invalid behind count")?;
    Ok((ahead, behind))
}

async fn remote_default_branch(repo_root: &Path) -> Option<String> {
    optional_git_stdout(
        [
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
        repo_root,
    )
    .await
    .ok()
    .flatten()
    .map(|branch| branch.trim_start_matches("origin/").to_string())
}

fn parse_worktree_list(output: &str) -> Result<Vec<WorktreeCandidate>> {
    let mut worktrees = Vec::new();
    let mut current: Option<WorktreeCandidate> = None;

    for line in output.lines() {
        if line.trim().is_empty() {
            if let Some(candidate) = current.take() {
                worktrees.push(candidate);
            }
            continue;
        }

        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(candidate) = current.take() {
                worktrees.push(candidate);
            }
            current = Some(WorktreeCandidate {
                path: PathBuf::from(path),
                branch_ref: None,
                detached: false,
            });
            continue;
        }

        let candidate = current
            .as_mut()
            .ok_or_else(|| anyhow!("missing worktree path before metadata line: {line}"))?;
        if let Some(branch_ref) = line.strip_prefix("branch ") {
            candidate.branch_ref = Some(branch_ref.to_string());
        } else if line == "detached" {
            candidate.detached = true;
        }
    }

    if let Some(candidate) = current.take() {
        worktrees.push(candidate);
    }

    Ok(worktrees)
}

async fn optional_git_stdout<I, S>(args: I, cwd: &Path) -> Result<Option<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    match run_git(args, cwd).await {
        Ok(output) => {
            let output = output.trim().to_string();
            if output.is_empty() {
                Ok(None)
            } else {
                Ok(Some(output))
            }
        }
        Err(error) if error.to_string().contains("no upstream configured") => Ok(None),
        Err(error) if error.to_string().contains("no such ref was fetched") => Ok(None),
        Err(error) if error.to_string().contains("ambiguous argument") => Ok(None),
        Err(error)
            if error
                .to_string()
                .contains("HEAD does not point to a branch") =>
        {
            Ok(None)
        }
        Err(error) if error.to_string().contains("No such remote") => Ok(None),
        Err(error) => Err(anyhow::Error::new(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_worktree_list_reads_branch_and_detached_entries() {
        let output = "\
worktree /tmp/main
HEAD abcdef0123456789
branch refs/heads/main

worktree /tmp/detached
HEAD fedcba9876543210
detached
";

        let worktrees = parse_worktree_list(output).expect("parse worktrees");

        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[0].path, PathBuf::from("/tmp/main"));
        assert_eq!(worktrees[0].branch_ref.as_deref(), Some("refs/heads/main"));
        assert!(!worktrees[0].detached);
        assert_eq!(worktrees[1].path, PathBuf::from("/tmp/detached"));
        assert!(worktrees[1].detached);
        assert!(worktrees[1].branch_ref.is_none());
    }

    #[test]
    fn parse_ahead_behind_counts_reads_git_rev_list_output() {
        let counts = parse_ahead_behind_counts("3\t5\n").expect("parse counts");
        assert_eq!(counts, (3, 5));
    }
}
