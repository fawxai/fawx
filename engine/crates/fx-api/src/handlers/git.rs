use crate::handlers::HandlerResult;
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tokio::process::Command;

const DEFAULT_LOG_LIMIT: usize = 20;

#[derive(Debug, Serialize)]
pub struct GitStatusResponse {
    pub branch: String,
    pub files: Vec<GitFileStatus>,
    pub clean: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GitFileStatus {
    pub path: String,
    pub status: String,
    pub staged: bool,
}

#[derive(Debug, Deserialize)]
pub struct GitLogQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct GitLogResponse {
    pub commits: Vec<GitCommit>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct GitDiffResponse {
    pub diff: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Deserialize, Default)]
pub struct GitStageRequest {
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GitStageResponse {
    pub staged: bool,
    pub paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct GitCommitRequest {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct GitCommitResponse {
    pub committed: bool,
    pub hash: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct GitPushResponse {
    pub pushed: bool,
    pub remote: String,
    pub branch: String,
}

#[derive(Debug, Default)]
struct GitDiffSummary {
    files_changed: usize,
    insertions: usize,
    deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitOutput {
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitError {
    message: String,
}

impl GitError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GitError {}

pub async fn handle_git_status(
    State(state): State<HttpState>,
) -> HandlerResult<Json<GitStatusResponse>> {
    let cwd = workspace_dir(&state).await;
    let output = run_git(["status", "--porcelain=v1", "--branch"], &cwd)
        .await
        .map_err(internal_error)?;
    let response = parse_status_response(&output).map_err(internal_error)?;
    Ok(Json(response))
}

pub async fn handle_git_log(
    State(state): State<HttpState>,
    Query(query): Query<GitLogQuery>,
) -> HandlerResult<Json<GitLogResponse>> {
    let limit = query.limit.unwrap_or(DEFAULT_LOG_LIMIT);
    if limit == 0 {
        return Ok(Json(GitLogResponse {
            commits: Vec::new(),
        }));
    }
    let cwd = workspace_dir(&state).await;
    let limit_arg = format!("-{limit}");
    let output = run_git(
        ["log", "--oneline", "--format=%H|%h|%s|%an|%aI", &limit_arg],
        &cwd,
    )
    .await
    .map_err(internal_error)?;
    let response = parse_log_response(&output).map_err(internal_error)?;
    Ok(Json(response))
}

pub async fn handle_git_diff(
    State(state): State<HttpState>,
) -> HandlerResult<Json<GitDiffResponse>> {
    let cwd = workspace_dir(&state).await;
    let diff = run_git(["diff"], &cwd).await.map_err(internal_error)?;
    let stat = run_git(["diff", "--stat"], &cwd)
        .await
        .map_err(internal_error)?;
    Ok(Json(parse_diff_response(diff, &stat)))
}

pub async fn handle_git_stage(
    State(state): State<HttpState>,
    Json(request): Json<GitStageRequest>,
) -> HandlerResult<Json<GitStageResponse>> {
    validate_paths(&request.paths).map_err(bad_request)?;
    let cwd = workspace_dir(&state).await;
    let paths = request.paths;
    if paths.is_empty() {
        run_git(["add", "-A"], &cwd).await.map_err(bad_request)?;
    } else {
        run_git(stage_args(&paths), &cwd)
            .await
            .map_err(bad_request)?;
    }
    Ok(Json(GitStageResponse {
        staged: true,
        paths,
    }))
}

// ── Unstage ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct GitUnstageRequest {
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GitUnstageResponse {
    pub unstaged: bool,
    pub paths: Vec<String>,
}

pub async fn handle_git_unstage(
    State(state): State<HttpState>,
    Json(request): Json<GitUnstageRequest>,
) -> HandlerResult<Json<GitUnstageResponse>> {
    validate_paths(&request.paths).map_err(bad_request)?;
    let cwd = workspace_dir(&state).await;
    let paths = request.paths;
    if paths.is_empty() {
        run_git(["reset", "HEAD"], &cwd)
            .await
            .map_err(bad_request)?;
    } else {
        run_git(unstage_args(&paths), &cwd)
            .await
            .map_err(bad_request)?;
    }
    Ok(Json(GitUnstageResponse {
        unstaged: true,
        paths,
    }))
}

fn unstage_args(paths: &[String]) -> Vec<String> {
    let mut args = vec!["reset".to_string(), "HEAD".to_string(), "--".to_string()];
    args.extend(paths.iter().cloned());
    args
}

// ── Pull ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct GitPullResponse {
    pub pulled: bool,
    pub summary: String,
    pub conflicts: bool,
}

pub async fn handle_git_pull(
    State(state): State<HttpState>,
) -> HandlerResult<Json<GitPullResponse>> {
    let cwd = workspace_dir(&state).await;
    match run_git_output(["pull"], &cwd).await {
        Ok(output) => Ok(Json(pull_response_from_output(&output))),
        Err(error) => {
            let msg = error.to_string();
            if msg.contains("CONFLICT") || msg.contains("Merge conflict") {
                Ok(Json(GitPullResponse {
                    pulled: false,
                    summary: msg,
                    conflicts: true,
                }))
            } else {
                Err(bad_request(error))
            }
        }
    }
}

fn pull_response_from_output(output: &GitOutput) -> GitPullResponse {
    let conflicts = output.stdout.contains("CONFLICT") || output.stderr.contains("CONFLICT");
    let summary = if output.stdout.trim().is_empty() {
        output.stderr.trim().to_string()
    } else {
        output.stdout.trim().to_string()
    };
    GitPullResponse {
        pulled: !conflicts,
        summary,
        conflicts,
    }
}

// ── Fetch ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct GitFetchResponse {
    pub fetched: bool,
    pub summary: String,
}

pub async fn handle_git_fetch(
    State(state): State<HttpState>,
) -> HandlerResult<Json<GitFetchResponse>> {
    let cwd = workspace_dir(&state).await;
    let output = run_git_output(["fetch"], &cwd).await.map_err(bad_request)?;
    let summary = if output.stderr.trim().is_empty() {
        "Already up to date.".to_string()
    } else {
        output.stderr.trim().to_string()
    };
    Ok(Json(GitFetchResponse {
        fetched: true,
        summary,
    }))
}

pub async fn handle_git_commit(
    State(state): State<HttpState>,
    Json(request): Json<GitCommitRequest>,
) -> HandlerResult<Json<GitCommitResponse>> {
    let message = request.message.trim().to_string();
    if message.is_empty() {
        return Err(bad_request(GitError::new(
            "commit message must not be empty",
        )));
    }
    let cwd = workspace_dir(&state).await;
    let output = run_git_output(["commit", "-m", message.as_str()], &cwd)
        .await
        .map_err(bad_request)?;
    let short_hash = parse_commit_hash(&output.stdout).map_err(internal_error)?;
    let hash = match resolve_head_hash(&cwd).await {
        Ok(hash) => hash,
        Err(_) => short_hash,
    };
    Ok(Json(GitCommitResponse {
        committed: true,
        hash,
        message,
    }))
}

pub async fn handle_git_push(
    State(state): State<HttpState>,
) -> HandlerResult<Json<GitPushResponse>> {
    let cwd = workspace_dir(&state).await;
    match run_git_output(["push"], &cwd).await {
        Ok(output) => {
            let (remote, branch) = push_target(&cwd, &output).await.map_err(internal_error)?;
            Ok(Json(GitPushResponse {
                pushed: true,
                remote,
                branch,
            }))
        }
        Err(error) => {
            let suggestion = classify_push_error(&error.to_string());
            Err(bad_request(GitError::new(format!(
                "{suggestion}\n\nDetails: {error}"
            ))))
        }
    }
}

fn classify_push_error(message: &str) -> &'static str {
    if message.contains("upstream branch") || message.contains("no upstream") {
        "Push failed: no upstream branch configured. Run `git push -u origin HEAD` from the terminal first."
    } else if message.contains("non-fast-forward") || message.contains("rejected") {
        "Push failed: remote has changes you don't have. Pull first, then push."
    } else if message.contains("permission denied") || message.contains("403") {
        "Push failed: permission denied. Check your Git credentials."
    } else if message.contains("Could not resolve host") {
        "Push failed: cannot reach the remote. Check your network connection."
    } else {
        "Push failed. See details below."
    }
}

async fn workspace_dir(state: &HttpState) -> PathBuf {
    if let Some(path) = configured_workspace_dir(state).await {
        return path;
    }
    if let Some(parent) = state.data_dir.parent().filter(|path| is_git_repo(path)) {
        return parent.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(dir) => dir,
        Err(_) => state.data_dir.clone(),
    }
}

async fn configured_workspace_dir(state: &HttpState) -> Option<PathBuf> {
    let manager = {
        let app = state.app.lock().await;
        app.config_manager()
    }?;
    let guard = manager.lock().ok()?;
    guard
        .config()
        .tools
        .working_dir
        .clone()
        .or_else(|| guard.config().workspace.root.clone())
}

fn is_git_repo(path: &Path) -> bool {
    path.join(".git").is_dir() || path.join(".git").is_file()
}

async fn run_git<I, S>(args: I, cwd: &Path) -> Result<String, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Ok(run_git_output(args, cwd).await?.stdout)
}

async fn run_git_output<I, S>(args: I, cwd: &Path) -> Result<GitOutput, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|error| GitError::new(format!("failed to run git: {error}")))?;
    if output.status.success() {
        return Ok(GitOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    Err(GitError::new(format!("git error: {message}")))
}

fn validate_paths(paths: &[String]) -> Result<(), GitError> {
    for path in paths {
        if path.trim().is_empty() {
            return Err(GitError::new("paths must not contain empty values"));
        }
        if path.contains("..") {
            return Err(GitError::new("paths must not contain path traversal (..)"));
        }
        if path.starts_with('/') {
            return Err(GitError::new("paths must be relative, not absolute"));
        }
    }
    Ok(())
}

fn stage_args(paths: &[String]) -> Vec<String> {
    let mut args = vec!["add".to_string(), "--".to_string()];
    args.extend(paths.iter().cloned());
    args
}

fn parse_status_response(output: &str) -> Result<GitStatusResponse, GitError> {
    let mut lines = output.lines();
    let branch = lines
        .next()
        .map(parse_branch)
        .ok_or_else(|| GitError::new("missing git status branch header"))?;
    let files = lines
        .filter(|line| !line.trim().is_empty())
        .map(parse_porcelain_status_line)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GitStatusResponse {
        clean: files.is_empty(),
        branch,
        files,
    })
}

fn parse_branch(line: &str) -> String {
    let summary = line.strip_prefix("## ").unwrap_or(line);
    if let Some(branch) = summary.strip_prefix("No commits yet on ") {
        return branch.to_string();
    }
    if let Some(branch) = summary.strip_prefix("Initial commit on ") {
        return branch.to_string();
    }
    summary
        .split_once("...")
        .map(|(branch, _)| branch)
        .unwrap_or(summary)
        .to_string()
}

fn parse_porcelain_status_line(line: &str) -> Result<GitFileStatus, GitError> {
    if line.len() < 4 {
        return Err(GitError::new(format!("invalid git status line: {line}")));
    }
    let mut chars = line.chars();
    let x = chars
        .next()
        .ok_or_else(|| GitError::new("missing index status"))?;
    let y = chars
        .next()
        .ok_or_else(|| GitError::new("missing worktree status"))?;
    let raw_path = line
        .get(3..)
        .ok_or_else(|| GitError::new(format!("missing path in git status line: {line}")))?;
    Ok(GitFileStatus {
        path: resolved_status_path(raw_path),
        status: status_label(x, y).to_string(),
        staged: x != ' ' && x != '?',
    })
}

fn resolved_status_path(raw_path: &str) -> String {
    raw_path
        .split_once(" -> ")
        .map(|(_, path)| path)
        .unwrap_or(raw_path)
        .to_string()
}

fn status_label(index_status: char, worktree_status: char) -> &'static str {
    if index_status == '?' || worktree_status == '?' {
        return "untracked";
    }
    if index_status == 'R' || worktree_status == 'R' {
        return "renamed";
    }
    if index_status == 'D' || worktree_status == 'D' {
        return "deleted";
    }
    if index_status == 'A' || worktree_status == 'A' || index_status == 'C' {
        return "added";
    }
    "modified"
}

fn parse_log_response(output: &str) -> Result<GitLogResponse, GitError> {
    let commits = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_log_line)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GitLogResponse { commits })
}

fn parse_log_line(line: &str) -> Result<GitCommit, GitError> {
    let mut head = line.splitn(3, '|');
    let hash = next_field(&mut head, "hash")?;
    let short_hash = next_field(&mut head, "short hash")?;
    let remainder = next_field(&mut head, "commit payload")?;
    let mut tail = remainder.rsplitn(3, '|');
    let timestamp = next_field(&mut tail, "timestamp")?;
    let author = next_field(&mut tail, "author")?;
    let message = next_field(&mut tail, "message")?;
    Ok(GitCommit {
        hash: hash.to_string(),
        short_hash: short_hash.to_string(),
        message: message.to_string(),
        author: author.to_string(),
        timestamp: timestamp.to_string(),
    })
}

fn next_field<'a, I>(parts: &mut I, name: &str) -> Result<&'a str, GitError>
where
    I: Iterator<Item = &'a str>,
{
    parts
        .next()
        .ok_or_else(|| GitError::new(format!("missing git {name}")))
}

fn parse_diff_response(diff: String, stat: &str) -> GitDiffResponse {
    let summary = parse_diff_summary(stat);
    GitDiffResponse {
        diff,
        files_changed: summary.files_changed,
        insertions: summary.insertions,
        deletions: summary.deletions,
    }
}

fn parse_diff_summary(stat: &str) -> GitDiffSummary {
    let Some(summary_line) = stat.lines().rev().find(|line| line.contains("changed")) else {
        return GitDiffSummary::default();
    };
    let mut summary = GitDiffSummary::default();
    for segment in summary_line.split(',') {
        apply_diff_segment(&mut summary, segment.trim());
    }
    summary
}

fn apply_diff_segment(summary: &mut GitDiffSummary, segment: &str) {
    let count = segment
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if segment.contains("file changed") || segment.contains("files changed") {
        summary.files_changed = count;
    }
    if segment.contains("insertion") {
        summary.insertions = count;
    }
    if segment.contains("deletion") {
        summary.deletions = count;
    }
}

fn parse_commit_hash(output: &str) -> Result<String, GitError> {
    let line = output
        .lines()
        .find(|line| line.starts_with('['))
        .ok_or_else(|| GitError::new("missing commit summary from git output"))?;
    let summary = line
        .strip_prefix('[')
        .and_then(|value| value.split(']').next())
        .ok_or_else(|| GitError::new("invalid commit summary from git output"))?;
    summary
        .split_whitespace()
        .last()
        .map(ToString::to_string)
        .ok_or_else(|| GitError::new("missing commit hash from git output"))
}

async fn resolve_head_hash(cwd: &Path) -> Result<String, GitError> {
    let hash = run_git(["rev-parse", "HEAD"], cwd).await?;
    Ok(hash.trim().to_string())
}

async fn push_target(cwd: &Path, output: &GitOutput) -> Result<(String, String), GitError> {
    let upstream = upstream_parts(cwd).await.ok();
    let remote = upstream
        .as_ref()
        .map(|(remote, _)| remote.clone())
        .or_else(|| parse_push_remote(output))
        .ok_or_else(|| GitError::new("unable to determine push remote"))?;
    let branch = if let Some(branch) = parse_push_branch(output) {
        branch
    } else if let Some((_, branch)) = upstream.as_ref() {
        branch.clone()
    } else if let Ok(branch) = current_branch(cwd).await {
        branch
    } else {
        return Err(GitError::new("unable to determine push branch"));
    };
    Ok((remote, branch))
}

fn parse_push_remote(output: &GitOutput) -> Option<String> {
    git_output_lines(output)
        .find_map(|line| line.strip_prefix("To "))
        .map(ToString::to_string)
}

fn parse_push_branch(output: &GitOutput) -> Option<String> {
    git_output_lines(output)
        .filter_map(|line| line.split_once(" -> ").map(|(_, branch)| branch.trim()))
        .find_map(|branch| branch.split_whitespace().next().map(ToString::to_string))
}

fn git_output_lines(output: &GitOutput) -> impl Iterator<Item = &str> {
    output.stdout.lines().chain(output.stderr.lines())
}

async fn upstream_parts(cwd: &Path) -> Result<(String, String), GitError> {
    let upstream = run_git(
        [
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        cwd,
    )
    .await?;
    split_upstream_ref(upstream.trim())
}

fn split_upstream_ref(upstream: &str) -> Result<(String, String), GitError> {
    upstream
        .split_once('/')
        .map(|(remote, branch)| (remote.to_string(), branch.to_string()))
        .ok_or_else(|| GitError::new(format!("invalid upstream ref: {upstream}")))
}

async fn current_branch(cwd: &Path) -> Result<String, GitError> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|error| GitError::new(format!("failed to read current branch: {error}")))?;
    if !output.status.success() {
        return Err(GitError::new("failed to read current branch"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn bad_request(error: GitError) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            error: error.to_string(),
        }),
    )
}

fn internal_error(error: GitError) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            error: error.to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_serializes() {
        let response = GitStatusResponse {
            branch: "main".to_string(),
            files: vec![GitFileStatus {
                path: "src/lib.rs".to_string(),
                status: "modified".to_string(),
                staged: false,
            }],
            clean: false,
        };

        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["branch"], "main");
        assert_eq!(json["files"][0]["path"], "src/lib.rs");
        assert_eq!(json["clean"], false);
    }

    #[test]
    fn log_response_serializes() {
        let response = GitLogResponse {
            commits: vec![GitCommit {
                hash: "abcdef123456".to_string(),
                short_hash: "abcdef1".to_string(),
                message: "feat: add git api".to_string(),
                author: "Alice".to_string(),
                timestamp: "2026-03-15T20:00:00Z".to_string(),
            }],
        };

        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["commits"][0]["hash"], "abcdef123456");
        assert_eq!(json["commits"][0]["author"], "Alice");
    }

    #[test]
    fn diff_response_serializes() {
        let response = GitDiffResponse {
            diff: "diff --git a/a b/a".to_string(),
            files_changed: 1,
            insertions: 2,
            deletions: 3,
        };

        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["files_changed"], 1);
        assert_eq!(json["insertions"], 2);
        assert_eq!(json["deletions"], 3);
    }

    #[test]
    fn commit_request_deserializes() {
        let request: GitCommitRequest =
            serde_json::from_str(r#"{"message":"feat: commit changes"}"#).unwrap();

        assert_eq!(request.message, "feat: commit changes");
    }

    #[test]
    fn stage_request_deserializes_empty_paths() {
        let request: GitStageRequest = serde_json::from_str("{}").unwrap();

        assert!(request.paths.is_empty());
    }

    #[test]
    fn parse_porcelain_status_line() {
        let status = super::parse_porcelain_status_line("R  src/old.rs -> src/new.rs").unwrap();

        assert_eq!(status.path, "src/new.rs");
        assert_eq!(status.status, "renamed");
        assert!(status.staged);
    }

    #[test]
    fn parse_log_line() {
        let commit = super::parse_log_line(
            "abcdef123456|abcdef1|feat: support pipes | in messages|Alice|2026-03-15T20:00:00Z",
        )
        .unwrap();

        assert_eq!(commit.hash, "abcdef123456");
        assert_eq!(commit.short_hash, "abcdef1");
        assert_eq!(commit.message, "feat: support pipes | in messages");
        assert_eq!(commit.author, "Alice");
    }

    #[test]
    fn unstage_response_serializes() {
        let response = GitUnstageResponse {
            unstaged: true,
            paths: vec!["src/lib.rs".to_string()],
        };
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["unstaged"], true);
        assert_eq!(json["paths"][0], "src/lib.rs");
    }

    #[test]
    fn pull_response_serializes() {
        let response = GitPullResponse {
            pulled: true,
            summary: "Already up to date.".to_string(),
            conflicts: false,
        };
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["pulled"], true);
        assert_eq!(json["conflicts"], false);
    }

    #[test]
    fn fetch_response_serializes() {
        let response = GitFetchResponse {
            fetched: true,
            summary: "Already up to date.".to_string(),
        };
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["fetched"], true);
    }

    #[test]
    fn pull_response_detects_conflicts_in_output() {
        let output = GitOutput {
            stdout: "CONFLICT (content): Merge conflict in src/lib.rs".to_string(),
            stderr: String::new(),
        };
        let response = pull_response_from_output(&output);
        assert!(response.conflicts);
        assert!(!response.pulled);
    }

    #[test]
    fn pull_response_clean_merge() {
        let output = GitOutput {
            stdout: "Updating abc123..def456\nFast-forward".to_string(),
            stderr: String::new(),
        };
        let response = pull_response_from_output(&output);
        assert!(!response.conflicts);
        assert!(response.pulled);
    }

    #[test]
    fn validate_paths_rejects_traversal() {
        assert!(validate_paths(&["../../etc/passwd".to_string()]).is_err());
        assert!(validate_paths(&["src/../../../etc".to_string()]).is_err());
    }

    #[test]
    fn validate_paths_rejects_absolute() {
        assert!(validate_paths(&["/etc/passwd".to_string()]).is_err());
    }

    #[test]
    fn validate_paths_accepts_normal() {
        assert!(validate_paths(&["src/lib.rs".to_string()]).is_ok());
        assert!(validate_paths(&["engine/crates/fx-api/src/lib.rs".to_string()]).is_ok());
    }

    #[test]
    fn unstage_args_builds_correct_command() {
        let args = unstage_args(&["src/lib.rs".to_string()]);
        assert_eq!(args, vec!["reset", "HEAD", "--", "src/lib.rs"]);
    }
}
