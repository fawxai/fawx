use async_trait::async_trait;
use fx_core::self_modify::SelfModifyConfig;
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::ToolAuthoritySurface;
use fx_llm::{ToolCall, ToolDefinition};
use fx_loadable::{Skill, SkillError};
use fx_ripcord::git_guard::check_push_allowed;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::timeout;
use zeroize::Zeroizing;

const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
const DIFF_TIMEOUT: Duration = Duration::from_secs(15);
const CHECKPOINT_TIMEOUT: Duration = Duration::from_secs(10);
const BRANCH_TIMEOUT: Duration = Duration::from_secs(5);
const MERGE_TIMEOUT: Duration = Duration::from_secs(10);
const REVERT_TIMEOUT: Duration = Duration::from_secs(10);
const PUSH_TIMEOUT: Duration = Duration::from_secs(30);
const PR_CREATE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_DIFF_OUTPUT_CHARS: usize = 50_000;
const TRUNCATED_SUFFIX: &str = "\n(truncated)";

/// Provider for the GitHub PAT used in remote git operations.
pub type GitHubTokenProvider = Arc<dyn Fn() -> Option<Zeroizing<String>> + Send + Sync>;

#[derive(Clone)]
pub struct GitSkill {
    working_dir: PathBuf,
    self_modify: Option<SelfModifyConfig>,
    github_token: Option<GitHubTokenProvider>,
    protected_branches: Vec<String>,
}

impl std::fmt::Debug for GitSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitSkill")
            .field("working_dir", &self.working_dir)
            .field("self_modify", &self.self_modify)
            .field("github_token", &self.github_token.is_some())
            .field("protected_branches", &self.protected_branches)
            .finish()
    }
}

#[derive(Debug, Deserialize, Default)]
struct GitDiffArgs {
    staged: Option<bool>,
    #[serde(rename = "ref")]
    reference: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitCheckpointArgs {
    message: String,
}

#[derive(Debug, Deserialize)]
struct GitBranchCreateArgs {
    name: String,
    from: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitBranchSwitchArgs {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GitBranchDeleteArgs {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GitMergeArgs {
    branch: String,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitRevertArgs {
    commit_sha: String,
}

#[derive(Debug, Deserialize, Default)]
struct GitPushArgs {
    remote: Option<String>,
    branch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubPrCreateArgs {
    title: String,
    body: Option<String>,
    base: Option<String>,
    head: Option<String>,
    draft: Option<bool>,
}

impl GitSkill {
    pub fn new(
        working_dir: PathBuf,
        self_modify: Option<SelfModifyConfig>,
        github_token: Option<GitHubTokenProvider>,
    ) -> Self {
        Self {
            working_dir,
            self_modify,
            github_token,
            protected_branches: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_protected_branches(mut self, protected_branches: Vec<String>) -> Self {
        self.protected_branches = protected_branches;
        self
    }

    async fn run_git(&self, args: &[&str]) -> Result<String, String> {
        self.run_git_with_timeout(args, STATUS_TIMEOUT).await
    }

    async fn run_git_with_timeout(
        &self,
        args: &[&str],
        timeout_duration: Duration,
    ) -> Result<String, String> {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.working_dir)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = command
            .spawn()
            .map_err(|error| format!("failed to spawn git: {error}"))?;
        let output = wait_for_git_output(child, timeout_duration).await?;
        parse_git_output(output, &self.working_dir, args)
    }

    async fn execute_status(&self) -> Result<String, String> {
        self.run_git(&["status", "--short", "--branch"])
            .await
            .map_err(|error| friendly_status_error(&error))
    }

    async fn execute_diff(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitDiffArgs = parse_args(arguments)?;
        validate_diff_ref(parsed.reference.as_deref())?;
        let args = build_diff_args(&parsed);
        let output = self.run_git_with_timeout(&args, DIFF_TIMEOUT).await?;
        Ok(truncate_diff_output(output))
    }

    async fn execute_checkpoint(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitCheckpointArgs = parse_args(arguments)?;
        if parsed.message.trim().is_empty() {
            return Err("missing required field: message".to_string());
        }
        self.run_git_with_timeout(&["add", "-A"], CHECKPOINT_TIMEOUT)
            .await?;
        match self
            .run_git_with_timeout(&["commit", "-m", &parsed.message], CHECKPOINT_TIMEOUT)
            .await
        {
            Ok(output) => Ok(output),
            Err(error) if error.contains("nothing to commit") => {
                Ok("nothing to commit, working tree clean".to_string())
            }
            Err(error) => Err(error),
        }
    }

    async fn execute_branch_create(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitBranchCreateArgs = parse_args(arguments)?;
        validate_branch_name(&parsed.name)?;
        if let Some(ref base) = parsed.from {
            validate_branch_name(base)?;
        }
        let mut args = vec!["checkout", "-b", parsed.name.as_str()];
        if let Some(base) = parsed.from.as_deref() {
            args.push(base);
        }
        self.run_git_with_timeout(&args, BRANCH_TIMEOUT).await?;
        Ok(format!("Created and switched to branch '{}'", parsed.name))
    }

    async fn execute_branch_switch(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitBranchSwitchArgs = parse_args(arguments)?;
        validate_branch_name(&parsed.name)?;
        self.run_git_with_timeout(&["rev-parse", "--verify", &parsed.name], BRANCH_TIMEOUT)
            .await
            .map_err(|_| format!("branch '{}' does not exist", parsed.name))?;
        self.run_git_with_timeout(&["checkout", &parsed.name], BRANCH_TIMEOUT)
            .await?;
        Ok(format!("Switched to branch '{}'", parsed.name))
    }

    async fn execute_branch_delete(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitBranchDeleteArgs = parse_args(arguments)?;
        validate_branch_name(&parsed.name)?;
        let current = self
            .run_git_with_timeout(&["branch", "--show-current"], BRANCH_TIMEOUT)
            .await?;
        if current.trim() == parsed.name {
            return Err(format!(
                "cannot delete the currently checked-out branch '{}'",
                parsed.name
            ));
        }
        self.run_git_with_timeout(&["branch", "-d", &parsed.name], BRANCH_TIMEOUT)
            .await?;
        Ok(format!("Deleted branch '{}'", parsed.name))
    }

    async fn execute_merge(&self, arguments: &str) -> Result<String, String> {
        match &self.self_modify {
            Some(config) if config.enabled => {}
            _ => {
                return Err("git_merge requires self-modification to be enabled".to_string());
            }
        }
        let parsed: GitMergeArgs = parse_args(arguments)?;
        validate_branch_name(&parsed.branch)?;
        let mut args = vec!["merge", parsed.branch.as_str()];
        if let Some(message) = parsed.message.as_deref() {
            args.push("-m");
            args.push(message);
        }
        self.run_git_with_timeout(&args, MERGE_TIMEOUT).await
    }

    async fn execute_revert(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitRevertArgs = parse_args(arguments)?;
        validate_sha(&parsed.commit_sha)?;
        self.run_git_with_timeout(&["revert", &parsed.commit_sha, "--no-edit"], REVERT_TIMEOUT)
            .await
    }

    async fn execute_push(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitPushArgs = parse_args(arguments)?;
        let remote = parsed.remote.as_deref().unwrap_or("origin");
        let branch = match &parsed.branch {
            Some(branch) => branch.clone(),
            None => self.current_branch().await?,
        };
        validate_remote_name(remote)?;
        validate_branch_name(&branch)?;
        self.ensure_push_allowed(&branch)?;
        let token = self.require_github_token()?;
        self.run_git_with_token_auth(&["push", remote, &branch], &token, PUSH_TIMEOUT)
            .await
    }

    async fn execute_pr_create(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitHubPrCreateArgs = parse_args(arguments)?;
        if parsed.title.trim().is_empty() {
            return Err("missing required field: title".to_string());
        }
        let token = self.require_github_token()?;
        let head = match &parsed.head {
            Some(head) => {
                validate_branch_name(head)?;
                head.clone()
            }
            None => self.current_branch().await?,
        };
        if let Some(ref base) = parsed.base {
            validate_branch_name(base)?;
        }
        let remote_url = self.run_git(&["remote", "get-url", "origin"]).await?;
        let (owner, repo) = parse_github_remote(remote_url.trim())?;
        let base = parsed.base.as_deref().unwrap_or("main");
        let request = PullRequestRequest {
            title: &parsed.title,
            body: parsed.body.as_deref(),
            base,
            head: &head,
            draft: parsed.draft.unwrap_or(false),
        };
        create_pull_request(&token, &owner, &repo, &request).await
    }

    async fn current_branch(&self) -> Result<String, String> {
        let output = self.run_git(&["branch", "--show-current"]).await?;
        let branch = output.trim().to_string();
        if branch.is_empty() {
            return Err("not on a branch (detached HEAD)".to_string());
        }
        Ok(branch)
    }

    fn ensure_push_allowed(&self, branch: &str) -> Result<(), String> {
        let target = branch.to_string();
        check_push_allowed(std::slice::from_ref(&target), &self.protected_branches)
    }

    fn require_github_token(&self) -> Result<Zeroizing<String>, String> {
        let provider = self.github_token.as_ref().ok_or_else(|| {
            "GitHub token not configured. Set up GitHub auth via `fawx setup` or configure a PAT."
                .to_string()
        })?;
        provider().ok_or_else(|| {
            "GitHub token not available. Configure a PAT via `fawx setup`.".to_string()
        })
    }

    async fn run_git_with_token_auth(
        &self,
        args: &[&str],
        token: &str,
        timeout_duration: Duration,
    ) -> Result<String, String> {
        let config_value = Zeroizing::new(format!(
            "url.https://x-access-token:{}@github.com/.insteadOf=https://github.com/",
            token
        ));
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.working_dir)
            .arg("-c")
            .arg(config_value.as_str())
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = command
            .spawn()
            .map_err(|error| format!("failed to spawn git: {error}"))?;
        let output = wait_for_git_output(child, timeout_duration).await?;
        parse_git_output(output, &self.working_dir, args)
    }
}

#[async_trait]
impl Skill for GitSkill {
    fn name(&self) -> &str {
        "git"
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            git_status_definition(),
            git_diff_definition(),
            git_checkpoint_definition(),
            git_branch_create_definition(),
            git_branch_switch_definition(),
            git_branch_delete_definition(),
            git_merge_definition(),
            git_revert_definition(),
            git_push_definition(),
            github_pr_create_definition(),
        ]
    }

    fn authority_surface(&self, call: &ToolCall) -> ToolAuthoritySurface {
        match call.name.as_str() {
            "git_checkpoint" => ToolAuthoritySurface::GitCheckpoint,
            _ => ToolAuthoritySurface::Other,
        }
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        let result: Result<String, SkillError> = match tool_name {
            "git_status" => self.execute_status().await,
            "git_diff" => self.execute_diff(arguments).await,
            "git_checkpoint" => self.execute_checkpoint(arguments).await,
            "git_branch_create" => self.execute_branch_create(arguments).await,
            "git_branch_switch" => self.execute_branch_switch(arguments).await,
            "git_branch_delete" => self.execute_branch_delete(arguments).await,
            "git_merge" => self.execute_merge(arguments).await,
            "git_revert" => self.execute_revert(arguments).await,
            "git_push" => self.execute_push(arguments).await,
            "github_pr_create" => self.execute_pr_create(arguments).await,
            _ => return None,
        };
        Some(result)
    }
}

fn git_status_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_status".to_string(),
        description: "Show the current git repository status including branch, staged, and unstaged changes. Use this when the user asks about git status, the current branch, what files changed, or what is staged for commit."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    }
}

fn git_diff_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_diff".to_string(),
        description: "Show git diff of changes in the working directory. Use this when the user asks to see what changed, review modifications, or compare file versions."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "staged": {
                    "type": "boolean",
                    "description": "Show only staged changes"
                },
                "ref": {
                    "type": "string",
                    "description": "Compare against a specific ref (branch, tag, commit)"
                }
            },
            "required": []
        }),
    }
}

fn git_checkpoint_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_checkpoint".to_string(),
        description: "Create a local git checkpoint by staging all changes and committing with a message. Does NOT push. Use this when the user asks to save progress, create a commit, checkpoint work, or snapshot changes."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Commit message for the checkpoint"
                }
            },
            "required": ["message"]
        }),
    }
}

fn git_branch_create_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_branch_create".to_string(),
        description: "Create a new git branch and switch to it. Use this when the user wants to start a new feature branch, create a branch for changes, or branch from a specific ref.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name for the new branch"
                },
                "from": {
                    "type": "string",
                    "description": "Base ref to branch from (branch, tag, or commit). Defaults to current HEAD."
                }
            },
            "required": ["name"]
        }),
    }
}

fn git_branch_switch_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_branch_switch".to_string(),
        description: "Switch to an existing git branch. Use this when the user wants to change branches, checkout a different branch, or move to another branch.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the branch to switch to"
                }
            },
            "required": ["name"]
        }),
    }
}

fn git_branch_delete_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_branch_delete".to_string(),
        description: "Delete a git branch. Use this when the user wants to remove a branch that is no longer needed. Cannot delete the currently checked-out branch.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the branch to delete"
                }
            },
            "required": ["name"]
        }),
    }
}

fn git_merge_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_merge".to_string(),
        description: "Merge a branch into the current branch. Use this when the user wants to merge changes from another branch. Requires self-modification to be enabled.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "branch": {
                    "type": "string",
                    "description": "Name of the branch to merge into the current branch"
                },
                "message": {
                    "type": "string",
                    "description": "Custom merge commit message"
                }
            },
            "required": ["branch"]
        }),
    }
}

fn git_revert_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_revert".to_string(),
        description: "Revert a specific commit by creating a new commit that undoes its changes. Use this when the user wants to undo a specific commit without rewriting history.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "commit_sha": {
                    "type": "string",
                    "description": "SHA of the commit to revert (7-40 hex characters)"
                }
            },
            "required": ["commit_sha"]
        }),
    }
}

fn build_diff_args(parsed: &GitDiffArgs) -> Vec<&str> {
    let mut args = vec!["diff"];
    if parsed.staged.unwrap_or(false) {
        args.push("--staged");
    }
    if let Some(reference) = parsed.reference.as_deref() {
        args.push(reference);
        args.push("--");
    }
    args
}

fn validate_diff_ref(reference: Option<&str>) -> Result<(), String> {
    if reference.is_some_and(|value| value.starts_with('-')) {
        return Err("invalid ref: refs cannot start with '-'".to_string());
    }
    Ok(())
}

fn parse_git_output(output: Output, working_dir: &Path, args: &[&str]) -> Result<String, String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return Err(stderr);
    }
    if !stdout.is_empty() {
        return Err(stdout);
    }
    let joined = args.join(" ");
    Err(format!(
        "git command failed: git -C {} {joined}",
        working_dir.display()
    ))
}

fn parse_args<T>(arguments: &str) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(arguments).map_err(|error| format!("malformed tool arguments: {error}"))
}

fn friendly_status_error(error: &str) -> String {
    if error.contains("not a git repository") {
        return "not a git repository (or any of the parent directories)".to_string();
    }
    error.to_string()
}

fn truncate_diff_output(mut output: String) -> String {
    let Some(truncate_index) = output
        .char_indices()
        .map(|(index, _)| index)
        .nth(MAX_DIFF_OUTPUT_CHARS)
    else {
        return output;
    };
    output.truncate(truncate_index);
    output.push_str(TRUNCATED_SUFFIX);
    output
}

async fn wait_for_git_output(child: Child, timeout_duration: Duration) -> Result<Output, String> {
    match timeout(timeout_duration, child.wait_with_output()).await {
        Ok(result) => result.map_err(|error| format!("failed while waiting for git: {error}")),
        Err(_) => Err("git command timed out".to_string()),
    }
}

fn validate_branch_name(name: &str) -> Result<(), String> {
    if name.starts_with('-') {
        return Err("invalid branch name: cannot start with '-'".to_string());
    }
    if name.contains(' ') {
        return Err("invalid branch name: cannot contain spaces".to_string());
    }
    if name.contains("..") {
        return Err("invalid branch name: cannot contain '..'".to_string());
    }
    Ok(())
}

fn validate_sha(sha: &str) -> Result<(), String> {
    if sha.len() < 7 || sha.len() > 40 {
        return Err(format!(
            "invalid commit SHA: must be 7-40 characters, got {}",
            sha.len()
        ));
    }
    if !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid commit SHA: must contain only hex characters".to_string());
    }
    Ok(())
}

fn git_push_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_push".to_string(),
        description: "Push commits to a remote repository. Use this when the user asks to push changes, upload commits, or sync a branch with the remote. Requires GitHub authentication."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "remote": { "type": "string", "description": "Remote name. Defaults to 'origin'." },
                "branch": { "type": "string", "description": "Branch to push. Defaults to current branch." }
            },
            "required": []
        }),
    }
}

fn github_pr_create_definition() -> ToolDefinition {
    ToolDefinition {
        name: "github_pr_create".to_string(),
        description: "Create a GitHub pull request. Use this when the user asks to open a PR or submit changes for review. Requires GitHub authentication."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "PR title" },
                "body": { "type": "string", "description": "PR description" },
                "base": { "type": "string", "description": "Base branch. Defaults to 'main'." },
                "head": { "type": "string", "description": "Head branch. Defaults to current." },
                "draft": { "type": "boolean", "description": "Create as draft. Defaults to false." }
            },
            "required": ["title"]
        }),
    }
}

fn validate_remote_name(name: &str) -> Result<(), String> {
    if name.starts_with('-') {
        return Err("invalid remote name: cannot start with '-'".to_string());
    }
    if name.contains(' ') {
        return Err("invalid remote name: cannot contain spaces".to_string());
    }
    if name.contains("..") {
        return Err("invalid remote name: cannot contain '..'".to_string());
    }
    Ok(())
}

fn parse_github_remote(url: &str) -> Result<(String, String), String> {
    let url = url.trim_end_matches(".git");
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Ok((parts[0].to_string(), parts[1].to_string()));
        }
    }
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Ok((parts[0].to_string(), parts[1].to_string()));
        }
    }
    Err(format!(
        "could not parse GitHub owner/repo from remote URL: {url}"
    ))
}

struct PullRequestRequest<'a> {
    title: &'a str,
    body: Option<&'a str>,
    base: &'a str,
    head: &'a str,
    draft: bool,
}

async fn create_pull_request(
    token: &str,
    owner: &str,
    repo: &str,
    request: &PullRequestRequest<'_>,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(PR_CREATE_TIMEOUT)
        .build()
        .map_err(|error| format!("failed to build HTTP client: {error}"))?;
    let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls");
    let mut payload = serde_json::json!({
        "title": request.title,
        "head": request.head,
        "base": request.base,
        "draft": request.draft,
    });
    if let Some(body) = request.body {
        payload["body"] = serde_json::Value::String(body.to_string());
    }
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "fawx-cli")
        .header("Accept", "application/vnd.github+json")
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("GitHub API request failed: {error}"))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("failed to parse GitHub response: {error}"))?;
    if !status.is_success() {
        let message = body["message"].as_str().unwrap_or("unknown error");
        return Err(format!("GitHub API error (HTTP {status}): {message}"));
    }
    let pr_number = body["number"].as_u64().unwrap_or(0);
    let pr_url = body["html_url"].as_str().unwrap_or("(no URL)");
    Ok(format!("Pull request #{pr_number} created: {pr_url}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    fn init_test_repo() -> TempDir {
        let tmp = TempDir::new().expect("tempdir should be created");
        StdCommand::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .expect("git command should run in test setup");
        StdCommand::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .expect("git command should run in test setup");
        StdCommand::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .expect("git command should run in test setup");
        tmp
    }

    async fn run_tool(
        skill: &GitSkill,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<String, String> {
        skill
            .execute(tool_name, &args.to_string(), None)
            .await
            .expect("known tool should return Some")
    }

    fn seed_initial_commit(repo: &TempDir, file: &str, content: &str) {
        fs::write(repo.path().join(file), content).expect("seed file should be written");
        run_git_ok(repo, &["add", file]);
        run_git_ok(repo, &["commit", "-m", "initial"]);
    }

    fn run_git_ok(repo: &TempDir, args: &[&str]) {
        let output = StdCommand::new("git")
            .args(args)
            .current_dir(repo.path())
            .output()
            .expect("git command should run in tests");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn fake_token_provider() -> Option<GitHubTokenProvider> {
        Some(Arc::new(|| Some(Zeroizing::new("ghp_fake".to_string()))))
    }

    fn init_push_remote(repo: &TempDir) -> TempDir {
        let remote = TempDir::new().expect("remote tempdir");
        let remote_path = remote.path().to_str().expect("utf8 remote path");
        let output = StdCommand::new("git")
            .args(["init", "--bare", remote_path])
            .output()
            .expect("init bare remote");
        assert!(output.status.success(), "bare remote init should succeed");
        run_git_ok(repo, &["remote", "add", "origin", remote_path]);
        remote
    }

    #[test]
    fn git_skill_provides_ten_tool_definitions() {
        let skill = GitSkill::new(PathBuf::from("."), None, None);
        let defs = skill.tool_definitions();
        let names: Vec<_> = defs
            .iter()
            .map(|definition| definition.name.as_str())
            .collect();
        assert_eq!(defs.len(), 10);
        assert!(names.contains(&"git_status"));
        assert!(names.contains(&"git_diff"));
        assert!(names.contains(&"git_checkpoint"));
        assert!(names.contains(&"git_branch_create"));
        assert!(names.contains(&"git_branch_switch"));
        assert!(names.contains(&"git_branch_delete"));
        assert!(names.contains(&"git_merge"));
        assert!(names.contains(&"git_revert"));
        assert!(names.contains(&"git_push"));
        assert!(names.contains(&"github_pr_create"));
    }

    #[test]
    fn git_tool_descriptions_include_when_to_use_guidance() {
        let skill = GitSkill::new(PathBuf::from("."), None, None);
        let definitions = skill.tool_definitions();
        for tool_name in [
            "git_status",
            "git_diff",
            "git_checkpoint",
            "git_branch_create",
            "git_branch_switch",
            "git_branch_delete",
            "git_merge",
            "git_revert",
            "git_push",
            "github_pr_create",
        ] {
            let definition = definitions
                .iter()
                .find(|definition| definition.name == tool_name)
                .expect("tool definition should exist");
            assert!(
                definition.description.contains("Use this when"),
                "{tool_name} description should include actionable usage guidance"
            );
        }
    }

    #[test]
    fn git_skill_reports_checkpoint_authority_surface() {
        let skill = GitSkill::new(PathBuf::from("."), None, None);
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "git_checkpoint".to_string(),
            arguments: serde_json::json!({}),
        };

        assert_eq!(
            skill.authority_surface(&call),
            ToolAuthoritySurface::GitCheckpoint
        );
    }

    #[test]
    fn git_skill_name_is_git() {
        let skill = GitSkill::new(PathBuf::from("."), None, None);
        assert_eq!(skill.name(), "git");
    }

    #[tokio::test]
    async fn git_skill_returns_none_for_unknown_tool() {
        let skill = GitSkill::new(PathBuf::from("."), None, None);
        let result = skill.execute("unknown_tool", "{}", None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn git_status_in_git_repo() {
        let repo = init_test_repo();
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(&skill, "git_status", serde_json::json!({}))
            .await
            .expect("status should work");
        assert!(output.contains("##"));
    }

    #[tokio::test]
    async fn git_status_outside_git_repo() {
        let dir = TempDir::new().expect("tempdir should be created");
        let skill = GitSkill::new(dir.path().to_path_buf(), None, None);
        let error = run_tool(&skill, "git_status", serde_json::json!({}))
            .await
            .expect_err("status should fail");
        assert!(error.contains("not a git repository"));
    }

    #[tokio::test]
    async fn git_diff_shows_changes() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "notes.txt", "before\nafter\n");
        fs::write(repo.path().join("notes.txt"), "before\nchanged\n")
            .expect("notes file should be updated");

        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(&skill, "git_diff", serde_json::json!({}))
            .await
            .expect("diff should work");

        assert!(output.contains("diff --git"));
        assert!(output.contains("-after"));
        assert!(output.contains("+changed"));
    }

    #[tokio::test]
    async fn git_diff_staged() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "one\n");
        fs::write(repo.path().join("file.txt"), "two\n").expect("file should be updated");
        StdCommand::new("git")
            .args(["add", "file.txt"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run in test setup");

        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(&skill, "git_diff", serde_json::json!({ "staged": true }))
            .await
            .expect("staged diff should work");

        assert!(output.contains("diff --git"));
        assert!(output.contains("-one"));
        assert!(output.contains("+two"));
    }

    #[tokio::test]
    async fn git_branch_create_creates_branch() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(
            &skill,
            "git_branch_create",
            serde_json::json!({ "name": "feature-x" }),
        )
        .await
        .expect("branch create should succeed");
        assert!(output.contains("feature-x"));
        let branch = StdCommand::new("git")
            .args(["branch", "--show-current"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        let branch_name = String::from_utf8(branch.stdout).expect("valid utf8");
        assert_eq!(branch_name.trim(), "feature-x");
    }

    #[tokio::test]
    async fn git_branch_create_from_base_ref() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        StdCommand::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(
            &skill,
            "git_branch_create",
            serde_json::json!({ "name": "from-main", "from": "main" }),
        )
        .await
        .expect("branch create from base should succeed");
        assert!(output.contains("from-main"));
    }

    #[tokio::test]
    async fn git_branch_create_rejects_invalid_name() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);

        let err1 = run_tool(
            &skill,
            "git_branch_create",
            serde_json::json!({ "name": "has space" }),
        )
        .await
        .expect_err("spaces should be rejected");
        assert!(err1.contains("spaces"));

        let err2 = run_tool(
            &skill,
            "git_branch_create",
            serde_json::json!({ "name": "bad..name" }),
        )
        .await
        .expect_err("double dots should be rejected");
        assert!(err2.contains(".."));

        let err3 = run_tool(
            &skill,
            "git_branch_create",
            serde_json::json!({ "name": "-leading" }),
        )
        .await
        .expect_err("leading dash should be rejected");
        assert!(err3.contains("-"));
    }

    #[tokio::test]
    async fn git_branch_create_rejects_invalid_from_ref() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let error = run_tool(
            &skill,
            "git_branch_create",
            serde_json::json!({ "name": "valid-name", "from": "--exec=bad" }),
        )
        .await
        .expect_err("dash-prefixed from ref should be rejected");
        assert!(error.contains("invalid branch name"));
    }

    #[tokio::test]
    async fn git_branch_switch_switches_to_existing_branch() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        StdCommand::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        StdCommand::new("git")
            .args(["checkout", "-b", "other"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        StdCommand::new("git")
            .args(["checkout", "main"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(
            &skill,
            "git_branch_switch",
            serde_json::json!({ "name": "other" }),
        )
        .await
        .expect("switch should succeed");
        assert!(output.contains("other"));
    }

    #[tokio::test]
    async fn git_branch_switch_errors_on_nonexistent() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let error = run_tool(
            &skill,
            "git_branch_switch",
            serde_json::json!({ "name": "no-such-branch" }),
        )
        .await
        .expect_err("switch to nonexistent should fail");
        assert!(error.contains("does not exist"));
    }

    #[tokio::test]
    async fn git_branch_delete_deletes_merged_branch() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        StdCommand::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        StdCommand::new("git")
            .args(["checkout", "-b", "to-delete"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        StdCommand::new("git")
            .args(["checkout", "main"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(
            &skill,
            "git_branch_delete",
            serde_json::json!({ "name": "to-delete" }),
        )
        .await
        .expect("delete should succeed");
        assert!(output.contains("to-delete"));
    }

    #[tokio::test]
    async fn git_branch_delete_refuses_current_branch() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        StdCommand::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let error = run_tool(
            &skill,
            "git_branch_delete",
            serde_json::json!({ "name": "main" }),
        )
        .await
        .expect_err("deleting current branch should fail");
        assert!(error.contains("currently checked-out"));
    }

    #[tokio::test]
    async fn git_merge_succeeds_when_self_modify_enabled() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        run_git_ok(&repo, &["branch", "-M", "main"]);
        run_git_ok(&repo, &["checkout", "-b", "feature"]);
        fs::write(repo.path().join("feature.txt"), "new\n").expect("write feature file");
        run_git_ok(&repo, &["add", "-A"]);
        run_git_ok(&repo, &["commit", "-m", "feature commit"]);
        run_git_ok(&repo, &["checkout", "main"]);

        let config = SelfModifyConfig {
            enabled: true,
            ..SelfModifyConfig::default()
        };
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config), None);
        let output = run_tool(
            &skill,
            "git_merge",
            serde_json::json!({ "branch": "feature" }),
        )
        .await
        .expect("merge should succeed");

        assert!(!output.is_empty());
        let merged = fs::read_to_string(repo.path().join("feature.txt")).expect("read merged file");
        assert_eq!(merged, "new\n");
    }

    #[tokio::test]
    async fn git_merge_blocked_when_disabled() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        let config = SelfModifyConfig {
            enabled: false,
            ..SelfModifyConfig::default()
        };
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config), None);
        let error = run_tool(
            &skill,
            "git_merge",
            serde_json::json!({ "branch": "feature" }),
        )
        .await
        .expect_err("merge should be blocked");
        assert!(error.contains("self-modification"));
    }

    #[tokio::test]
    async fn git_merge_blocked_when_none() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let error = run_tool(
            &skill,
            "git_merge",
            serde_json::json!({ "branch": "feature" }),
        )
        .await
        .expect_err("merge with no config should be blocked");
        assert!(error.contains("self-modification"));
    }

    #[tokio::test]
    async fn git_revert_reverts_commit() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "original\n");
        std::fs::write(repo.path().join("file.txt"), "changed\n").expect("write file");
        StdCommand::new("git")
            .args(["add", "-A"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        StdCommand::new("git")
            .args(["commit", "-m", "change to revert"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        let log = StdCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run");
        let sha = String::from_utf8(log.stdout)
            .expect("valid utf8")
            .trim()
            .to_string();
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(
            &skill,
            "git_revert",
            serde_json::json!({ "commit_sha": sha }),
        )
        .await
        .expect("revert should succeed");
        assert!(!output.is_empty());
        let reverted =
            fs::read_to_string(repo.path().join("file.txt")).expect("read reverted file");
        assert_eq!(reverted, "original\n");
    }

    #[tokio::test]
    async fn git_revert_rejects_invalid_sha() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "content\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);

        let invalid_values = [
            "zzzzzzz",
            "abc",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ];
        for sha in invalid_values {
            let error = run_tool(
                &skill,
                "git_revert",
                serde_json::json!({ "commit_sha": sha }),
            )
            .await
            .expect_err("invalid sha should fail");
            assert!(error.contains("invalid commit SHA"));
        }
    }

    #[tokio::test]
    async fn git_checkpoint_creates_commit() {
        let repo = init_test_repo();
        fs::write(repo.path().join("checkpoint.txt"), "saved\n")
            .expect("checkpoint file should be written");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);

        let output = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "checkpoint commit" }),
        )
        .await
        .expect("checkpoint should succeed");

        assert!(output.contains("checkpoint commit"));
        let log = StdCommand::new("git")
            .args(["log", "--oneline", "-1"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run in test setup");
        let log_text = String::from_utf8(log.stdout).expect("git log output should be valid UTF-8");
        assert!(log_text.contains("checkpoint commit"));
    }

    #[tokio::test]
    async fn git_checkpoint_nothing_to_commit() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "clean.txt", "clean\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);

        let output = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "no-op" }),
        )
        .await
        .expect("checkpoint should return a clean message");

        assert_eq!(output, "nothing to commit, working tree clean");
    }

    #[tokio::test]
    async fn git_checkpoint_requires_message() {
        let repo = init_test_repo();
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let error = run_tool(&skill, "git_checkpoint", serde_json::json!({}))
            .await
            .expect_err("missing message should fail");
        assert!(error.contains("missing field `message`"));
    }

    #[tokio::test]
    async fn git_checkpoint_rejects_whitespace_only_message() {
        let repo = init_test_repo();
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let error = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "   " }),
        )
        .await
        .expect_err("whitespace-only message should fail");
        assert!(error.contains("missing required field: message"));
    }

    #[tokio::test]
    async fn git_diff_ref_injection_prevented() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "safe.txt", "one\n");
        fs::write(repo.path().join("safe.txt"), "two\n").expect("safe file should be updated");

        let evil_output = repo.path().join("evil.patch");
        let reference = format!("--output={}", evil_output.display());
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let error = run_tool(&skill, "git_diff", serde_json::json!({ "ref": reference }))
            .await
            .expect_err("dash-prefixed ref should fail");

        assert!(error.contains("invalid ref: refs cannot start with '-'"));
        assert!(
            !evil_output.exists(),
            "git flag injection created output file"
        );
    }

    #[tokio::test]
    async fn git_diff_with_valid_ref_compares_against_main() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "safe.txt", "one\n");
        StdCommand::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run in test setup");
        StdCommand::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run in test setup");
        fs::write(repo.path().join("safe.txt"), "two\n").expect("safe file should be updated");

        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(&skill, "git_diff", serde_json::json!({ "ref": "main" }))
            .await
            .expect("valid ref diff should succeed");

        assert!(output.contains("diff --git"));
        assert!(output.contains("-one"));
        assert!(output.contains("+two"));
    }

    #[tokio::test]
    async fn git_diff_truncates_large_output() {
        let repo = init_test_repo();
        let large_old = (0..7000)
            .map(|idx| format!("old-line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let large_new = (0..7000)
            .map(|idx| format!("new-line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        seed_initial_commit(&repo, "huge.txt", &large_old);
        fs::write(repo.path().join("huge.txt"), &large_new).expect("large file should be updated");

        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(&skill, "git_diff", serde_json::json!({}))
            .await
            .expect("diff should work");

        assert!(output.ends_with("(truncated)"));
        assert!(output.chars().count() > MAX_DIFF_OUTPUT_CHARS);
    }

    #[tokio::test]
    async fn git_checkpoint_does_not_self_enforce_denied_path_policy() {
        let repo = init_test_repo();
        let config = SelfModifyConfig {
            enabled: true,
            deny_paths: vec!["*.txt".to_string()],
            ..SelfModifyConfig::default()
        };
        fs::write(repo.path().join("secret.txt"), "private").expect("write text file");
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config), None);
        let output = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "should fail" }),
        )
        .await
        .expect("checkpoint should succeed without local authority enforcement");
        assert!(output.contains("should fail"));
    }

    #[tokio::test]
    async fn git_checkpoint_does_not_require_local_proposal_system() {
        let repo = init_test_repo();
        let config = SelfModifyConfig {
            enabled: true,
            propose_paths: vec!["kernel/**".to_string()],
            ..SelfModifyConfig::default()
        };
        fs::create_dir_all(repo.path().join("kernel")).expect("create kernel dir");
        fs::write(repo.path().join("kernel/loop.rs"), "pub fn tick() {}")
            .expect("write kernel file");
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config), None);
        let output = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "should fail" }),
        )
        .await
        .expect("checkpoint should succeed without local proposal enforcement");
        assert!(output.contains("should fail"));
    }

    #[tokio::test]
    async fn git_checkpoint_allows_permitted_path() {
        let repo = init_test_repo();
        let config = SelfModifyConfig {
            enabled: true,
            allow_paths: vec!["*.txt".to_string()],
            ..SelfModifyConfig::default()
        };
        fs::write(repo.path().join("notes.txt"), "hello").expect("write txt file");
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config), None);
        let output = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "allowed commit" }),
        )
        .await
        .expect("checkpoint with allowed file should succeed");
        assert!(output.contains("allowed commit"));
    }

    #[tokio::test]
    async fn git_checkpoint_no_enforcement_when_disabled() {
        let repo = init_test_repo();
        fs::write(repo.path().join("secret.key"), "private").expect("write key file");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let output = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "no enforcement" }),
        )
        .await
        .expect("checkpoint without enforcement should succeed");
        assert!(output.contains("no enforcement"));
    }
    #[tokio::test]
    async fn git_checkpoint_leaves_clean_index_after_commit() {
        let repo = init_test_repo();
        let config = SelfModifyConfig {
            enabled: true,
            deny_paths: vec!["*.key".to_string()],
            ..SelfModifyConfig::default()
        };
        fs::write(repo.path().join("secret.key"), "private").expect("write key file");
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config), None);
        let _output = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "should fail" }),
        )
        .await
        .expect("checkpoint should succeed");

        let status = StdCommand::new("git")
            .args(["status", "--porcelain"])
            .current_dir(repo.path())
            .output()
            .expect("git status should work");
        let status_text =
            String::from_utf8(status.stdout).expect("git status output should be valid UTF-8");
        assert!(
            status_text.trim().is_empty(),
            "working tree should be clean after checkpoint, got: {status_text}"
        );
    }

    #[test]
    fn parse_github_remote_https() {
        let (owner, repo) = parse_github_remote("https://github.com/fawxai/fawx.git").unwrap();
        assert_eq!(owner, "fawxai");
        assert_eq!(repo, "fawx");
    }

    #[test]
    fn parse_github_remote_https_no_suffix() {
        let (owner, repo) = parse_github_remote("https://github.com/fawxai/fawx").unwrap();
        assert_eq!(owner, "fawxai");
        assert_eq!(repo, "fawx");
    }

    #[test]
    fn parse_github_remote_ssh() {
        let (owner, repo) = parse_github_remote("git@github.com:fawxai/fawx.git").unwrap();
        assert_eq!(owner, "fawxai");
        assert_eq!(repo, "fawx");
    }

    #[test]
    fn parse_github_remote_invalid() {
        assert!(parse_github_remote("https://gitlab.com/foo/bar").is_err());
    }

    #[test]
    fn validate_remote_name_rejects_dash() {
        assert!(validate_remote_name("-evil").is_err());
    }

    #[test]
    fn validate_remote_name_accepts_origin() {
        assert!(validate_remote_name("origin").is_ok());
    }

    #[tokio::test]
    async fn git_push_blocks_protected_branch() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "f.txt", "data\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None)
            .with_protected_branches(vec!["main".to_string()]);

        let error = run_tool(
            &skill,
            "git_push",
            serde_json::json!({"remote": "origin", "branch": "main"}),
        )
        .await
        .expect_err("push to protected branch should fail");

        assert!(error.contains("protected branch(es) 'main'"));
    }

    #[tokio::test]
    async fn git_push_allows_unprotected_branch() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "f.txt", "data\n");
        run_git_ok(&repo, &["checkout", "-b", "dev"]);
        let remote = init_push_remote(&repo);
        let skill = GitSkill::new(repo.path().to_path_buf(), None, fake_token_provider())
            .with_protected_branches(vec!["main".to_string()]);

        run_tool(
            &skill,
            "git_push",
            serde_json::json!({"remote": "origin", "branch": "dev"}),
        )
        .await
        .expect("push to unprotected branch should succeed");

        let remote_path = remote.path().to_str().expect("utf8 remote path");
        let output = StdCommand::new("git")
            .args([
                "--git-dir",
                remote_path,
                "show-ref",
                "--verify",
                "refs/heads/dev",
            ])
            .output()
            .expect("verify remote ref");
        assert!(output.status.success(), "remote dev branch should exist");
    }

    #[tokio::test]
    async fn git_push_requires_github_token() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "f.txt", "data\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let error = run_tool(&skill, "git_push", serde_json::json!({}))
            .await
            .expect_err("push without token should fail");
        assert!(error.contains("GitHub token not configured"));
    }

    #[tokio::test]
    async fn github_pr_create_requires_github_token() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "f.txt", "data\n");
        let skill = GitSkill::new(repo.path().to_path_buf(), None, None);
        let error = run_tool(
            &skill,
            "github_pr_create",
            serde_json::json!({"title": "test"}),
        )
        .await
        .expect_err("PR create without token should fail");
        assert!(error.contains("GitHub token not configured"));
    }

    #[tokio::test]
    async fn github_pr_create_rejects_empty_title() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "f.txt", "data\n");
        let token_fn: Option<GitHubTokenProvider> =
            Some(Arc::new(|| Some(Zeroizing::new("ghp_fake".to_string()))));
        let skill = GitSkill::new(repo.path().to_path_buf(), None, token_fn);
        let error = run_tool(
            &skill,
            "github_pr_create",
            serde_json::json!({"title": "  "}),
        )
        .await
        .expect_err("empty title should fail");
        assert!(error.contains("title"));
    }
}
