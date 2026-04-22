use std::ffi::OsStr;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitOutput {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitErrorKind {
    Invocation,
    CommandFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitError {
    kind: GitErrorKind,
    message: String,
}

impl GitError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self::command_failed(message)
    }

    pub(crate) fn invocation(message: impl Into<String>) -> Self {
        Self {
            kind: GitErrorKind::Invocation,
            message: message.into(),
        }
    }

    pub(crate) fn command_failed(message: impl Into<String>) -> Self {
        Self {
            kind: GitErrorKind::CommandFailed,
            message: message.into(),
        }
    }

    pub(crate) fn kind(&self) -> &GitErrorKind {
        &self.kind
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GitError {}

pub(crate) async fn run_git<I, S>(args: I, cwd: &Path) -> Result<String, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Ok(run_git_output(args, cwd).await?.stdout)
}

pub(crate) async fn run_git_output<I, S>(args: I, cwd: &Path) -> Result<GitOutput, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|error| GitError::invocation(format!("failed to run git: {error}")))?;
    if output.status.success() {
        return Ok(GitOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    Err(GitError::command_failed(format!("git error: {message}")))
}
