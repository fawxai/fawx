use async_trait::async_trait;
use fx_core::self_modify::{classify_path, format_tier_violation, SelfModifyConfig};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_loadable::{Skill, SkillError};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::timeout;

const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
const DIFF_TIMEOUT: Duration = Duration::from_secs(15);
const CHECKPOINT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_DIFF_OUTPUT_CHARS: usize = 50_000;
const TRUNCATED_SUFFIX: &str = "\n(truncated)";

#[derive(Debug, Clone)]
pub struct GitSkill {
    working_dir: PathBuf,
    self_modify: Option<SelfModifyConfig>,
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

impl GitSkill {
    pub(crate) fn new(working_dir: PathBuf, self_modify: Option<SelfModifyConfig>) -> Self {
        Self {
            working_dir,
            self_modify,
        }
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
        if let Err(error) = self.check_staged_paths().await {
            if let Err(reset_err) = self
                .run_git_with_timeout(&["reset"], CHECKPOINT_TIMEOUT)
                .await
            {
                tracing::warn!("failed to reset index after blocked checkpoint: {reset_err}");
            }
            return Err(error);
        }
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

    async fn check_staged_paths(&self) -> Result<(), String> {
        let Some(ref config) = self.self_modify else {
            return Ok(());
        };
        let output = self.run_git(&["diff", "--cached", "--name-only"]).await?;
        let mut violations = Vec::new();
        for line in output.lines() {
            let file_path = line.trim();
            if file_path.is_empty() {
                continue;
            }
            let full = self.working_dir.join(file_path);
            let tier = classify_path(&full, &self.working_dir, config);
            if let Some(message) = format_tier_violation(Path::new(file_path), tier) {
                violations.push(message);
            }
        }
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations.join(
                "
",
            ))
        }
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
        ]
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
        StdCommand::new("git")
            .args(["add", file])
            .current_dir(repo.path())
            .output()
            .expect("git command should run in test setup");
        StdCommand::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(repo.path())
            .output()
            .expect("git command should run in test setup");
    }

    #[test]
    fn git_skill_provides_three_tool_definitions() {
        let skill = GitSkill::new(PathBuf::from("."), None);
        let defs = skill.tool_definitions();
        let names: Vec<_> = defs
            .iter()
            .map(|definition| definition.name.as_str())
            .collect();
        assert_eq!(defs.len(), 3);
        assert!(names.contains(&"git_status"));
        assert!(names.contains(&"git_diff"));
        assert!(names.contains(&"git_checkpoint"));
    }

    #[test]
    fn git_tool_descriptions_include_when_to_use_guidance() {
        let skill = GitSkill::new(PathBuf::from("."), None);
        let definitions = skill.tool_definitions();
        for tool_name in ["git_status", "git_diff", "git_checkpoint"] {
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
    fn git_skill_name_is_git() {
        let skill = GitSkill::new(PathBuf::from("."), None);
        assert_eq!(skill.name(), "git");
    }

    #[tokio::test]
    async fn git_skill_returns_none_for_unknown_tool() {
        let skill = GitSkill::new(PathBuf::from("."), None);
        let result = skill.execute("unknown_tool", "{}", None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn git_status_in_git_repo() {
        let repo = init_test_repo();
        let skill = GitSkill::new(repo.path().to_path_buf(), None);
        let output = run_tool(&skill, "git_status", serde_json::json!({}))
            .await
            .expect("status should work");
        assert!(output.contains("##"));
    }

    #[tokio::test]
    async fn git_status_outside_git_repo() {
        let dir = TempDir::new().expect("tempdir should be created");
        let skill = GitSkill::new(dir.path().to_path_buf(), None);
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

        let skill = GitSkill::new(repo.path().to_path_buf(), None);
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

        let skill = GitSkill::new(repo.path().to_path_buf(), None);
        let output = run_tool(&skill, "git_diff", serde_json::json!({ "staged": true }))
            .await
            .expect("staged diff should work");

        assert!(output.contains("diff --git"));
        assert!(output.contains("-one"));
        assert!(output.contains("+two"));
    }

    #[tokio::test]
    async fn git_checkpoint_creates_commit() {
        let repo = init_test_repo();
        fs::write(repo.path().join("checkpoint.txt"), "saved\n")
            .expect("checkpoint file should be written");
        let skill = GitSkill::new(repo.path().to_path_buf(), None);

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
        let skill = GitSkill::new(repo.path().to_path_buf(), None);

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
        let skill = GitSkill::new(repo.path().to_path_buf(), None);
        let error = run_tool(&skill, "git_checkpoint", serde_json::json!({}))
            .await
            .expect_err("missing message should fail");
        assert!(error.contains("missing field `message`"));
    }

    #[tokio::test]
    async fn git_checkpoint_rejects_whitespace_only_message() {
        let repo = init_test_repo();
        let skill = GitSkill::new(repo.path().to_path_buf(), None);
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
        let skill = GitSkill::new(repo.path().to_path_buf(), None);
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

        let skill = GitSkill::new(repo.path().to_path_buf(), None);
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

        let skill = GitSkill::new(repo.path().to_path_buf(), None);
        let output = run_tool(&skill, "git_diff", serde_json::json!({}))
            .await
            .expect("diff should work");

        assert!(output.ends_with("(truncated)"));
        assert!(output.chars().count() > MAX_DIFF_OUTPUT_CHARS);
    }

    #[tokio::test]
    async fn git_checkpoint_blocks_denied_path() {
        let repo = init_test_repo();
        let config = SelfModifyConfig {
            enabled: true,
            deny_paths: vec!["*.key".to_string()],
            ..SelfModifyConfig::default()
        };
        fs::write(repo.path().join("secret.key"), "private").expect("write key file");
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config));
        let error = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "should fail" }),
        )
        .await
        .expect_err("checkpoint with denied file should fail");
        assert!(error.contains("Self-modify policy violation [deny]"));
    }

    #[tokio::test]
    async fn git_checkpoint_propose_tier_requires_proposal_system() {
        let repo = init_test_repo();
        let config = SelfModifyConfig {
            enabled: true,
            propose_paths: vec!["kernel/**".to_string()],
            ..SelfModifyConfig::default()
        };
        fs::create_dir_all(repo.path().join("kernel")).expect("create kernel dir");
        fs::write(repo.path().join("kernel/loop.rs"), "pub fn tick() {}")
            .expect("write kernel file");
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config));
        let error = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "should fail" }),
        )
        .await
        .expect_err("checkpoint with propose path should fail");
        assert!(error.contains("Self-modify policy violation [propose]"));
        assert!(error.contains("proposal system"));
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
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config));
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
        let skill = GitSkill::new(repo.path().to_path_buf(), None);
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
    async fn git_checkpoint_resets_index_on_deny() {
        let repo = init_test_repo();
        let config = SelfModifyConfig {
            enabled: true,
            deny_paths: vec!["*.key".to_string()],
            ..SelfModifyConfig::default()
        };
        fs::write(repo.path().join("secret.key"), "private").expect("write key file");
        let skill = GitSkill::new(repo.path().to_path_buf(), Some(config));
        let _error = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "should fail" }),
        )
        .await
        .expect_err("checkpoint with denied file should fail");

        // After denial, the index should be reset (file should be unstaged)
        let status = StdCommand::new("git")
            .args(["status", "--porcelain"])
            .current_dir(repo.path())
            .output()
            .expect("git status should work");
        let status_text =
            String::from_utf8(status.stdout).expect("git status output should be valid UTF-8");
        assert!(
            status_text.contains("?? secret.key"),
            "secret.key should be unstaged after denied checkpoint, got: {status_text}"
        );
    }
}
