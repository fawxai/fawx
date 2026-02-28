use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_loadable::{Skill, SkillError};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
const DIFF_TIMEOUT: Duration = Duration::from_secs(15);
const CHECKPOINT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_DIFF_OUTPUT_CHARS: usize = 50_000;
const TRUNCATED_SUFFIX: &str = "\n(truncated)";

#[derive(Debug, Clone)]
pub struct GitSkill {
    working_dir: PathBuf,
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
    pub(crate) fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }

    fn run_git(&self, args: &[&str]) -> Result<String, String> {
        self.run_git_with_timeout(args, STATUS_TIMEOUT)
    }

    fn run_git_with_timeout(&self, args: &[&str], timeout: Duration) -> Result<String, String> {
        let output = spawn_and_wait(
            Command::new("git")
                .arg("-C")
                .arg(&self.working_dir)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
            timeout,
        )?;
        parse_git_output(output, &self.working_dir, args)
    }

    fn execute_status(&self) -> Result<String, String> {
        self.run_git(&["status", "--short", "--branch"])
            .map_err(|error| friendly_status_error(&error))
    }

    fn execute_diff(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitDiffArgs = parse_args(arguments)?;
        validate_diff_ref(parsed.reference.as_deref())?;
        let args = build_diff_args(&parsed);
        let output = self.run_git_with_timeout(&args, DIFF_TIMEOUT)?;
        Ok(truncate_diff_output(output))
    }

    fn execute_checkpoint(&self, arguments: &str) -> Result<String, String> {
        let parsed: GitCheckpointArgs = parse_args(arguments)?;
        if parsed.message.trim().is_empty() {
            return Err("missing required field: message".to_string());
        }
        self.run_git_with_timeout(&["add", "-A"], CHECKPOINT_TIMEOUT)?;
        match self.run_git_with_timeout(&["commit", "-m", &parsed.message], CHECKPOINT_TIMEOUT) {
            Ok(output) => Ok(output),
            Err(error) if error.contains("nothing to commit") => {
                Ok("nothing to commit, working tree clean".to_string())
            }
            Err(error) => Err(error),
        }
    }
}

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

    fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        let result: Result<String, SkillError> = match tool_name {
            "git_status" => self.execute_status(),
            "git_diff" => self.execute_diff(arguments),
            "git_checkpoint" => self.execute_checkpoint(arguments),
            _ => return None,
        };
        Some(result)
    }
}

fn git_status_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_status".to_string(),
        description:
            "Show the current git repository status including branch, staged, and unstaged changes"
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
        description: "Show git diff of changes in the working directory".to_string(),
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
        description: "Create a local git checkpoint by staging all changes and committing with a message. Does NOT push.".to_string(),
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

fn spawn_and_wait(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to spawn git: {error}"))?;
    let stdout_reader = spawn_pipe_reader(child.stdout.take());
    let stderr_reader = spawn_pipe_reader(child.stderr.take());
    wait_for_process(&mut child, timeout, stdout_reader, stderr_reader)
}

fn wait_for_process(
    child: &mut Child,
    timeout: Duration,
    stdout_reader: thread::JoinHandle<Result<Vec<u8>, String>>,
    stderr_reader: thread::JoinHandle<Result<Vec<u8>, String>>,
) -> Result<Output, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return collect_output(status, stdout_reader, stderr_reader),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err("git command timed out".to_string());
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(format!("failed while waiting for git: {error}")),
        }
    }
}

fn collect_output(
    status: ExitStatus,
    stdout_reader: thread::JoinHandle<Result<Vec<u8>, String>>,
    stderr_reader: thread::JoinHandle<Result<Vec<u8>, String>>,
) -> Result<Output, String> {
    let stdout = join_pipe_reader(stdout_reader)?;
    let stderr = join_pipe_reader(stderr_reader)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn spawn_pipe_reader(
    pipe: Option<impl std::io::Read + Send + 'static>,
) -> thread::JoinHandle<Result<Vec<u8>, String>> {
    thread::spawn(move || drain_pipe(pipe))
}

fn join_pipe_reader(
    reader: thread::JoinHandle<Result<Vec<u8>, String>>,
) -> Result<Vec<u8>, String> {
    reader
        .join()
        .map_err(|_| "git output reader thread panicked".to_string())?
}

fn drain_pipe(pipe: Option<impl std::io::Read>) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    if let Some(mut reader) = pipe {
        std::io::Read::read_to_end(&mut reader, &mut bytes)
            .map_err(|error| format!("failed to read git output: {error}"))?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn init_test_repo() -> TempDir {
        let tmp = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        tmp
    }

    fn run_tool(
        skill: &GitSkill,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<String, String> {
        skill
            .execute(tool_name, &args.to_string(), None)
            .expect("known tool should return Some")
    }

    fn seed_initial_commit(repo: &TempDir, file: &str, content: &str) {
        fs::write(repo.path().join(file), content).unwrap();
        Command::new("git")
            .args(["add", file])
            .current_dir(repo.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(repo.path())
            .output()
            .unwrap();
    }

    #[test]
    fn git_skill_provides_three_tool_definitions() {
        let skill = GitSkill::new(PathBuf::from("."));
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
    fn git_skill_name_is_git() {
        let skill = GitSkill::new(PathBuf::from("."));
        assert_eq!(skill.name(), "git");
    }

    #[test]
    fn git_skill_returns_none_for_unknown_tool() {
        let skill = GitSkill::new(PathBuf::from("."));
        let result = skill.execute("unknown_tool", "{}", None);
        assert!(result.is_none());
    }

    #[test]
    fn git_status_in_git_repo() {
        let repo = init_test_repo();
        let skill = GitSkill::new(repo.path().to_path_buf());
        let output =
            run_tool(&skill, "git_status", serde_json::json!({})).expect("status should work");
        assert!(output.contains("##"));
    }

    #[test]
    fn git_status_outside_git_repo() {
        let dir = TempDir::new().unwrap();
        let skill = GitSkill::new(dir.path().to_path_buf());
        let error =
            run_tool(&skill, "git_status", serde_json::json!({})).expect_err("status should fail");
        assert!(error.contains("not a git repository"));
    }

    #[test]
    fn git_diff_shows_changes() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "notes.txt", "before\nafter\n");
        fs::write(repo.path().join("notes.txt"), "before\nchanged\n").unwrap();

        let skill = GitSkill::new(repo.path().to_path_buf());
        let output = run_tool(&skill, "git_diff", serde_json::json!({})).expect("diff should work");

        assert!(output.contains("diff --git"));
        assert!(output.contains("-after"));
        assert!(output.contains("+changed"));
    }

    #[test]
    fn git_diff_staged() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "file.txt", "one\n");
        fs::write(repo.path().join("file.txt"), "two\n").unwrap();
        Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(repo.path())
            .output()
            .unwrap();

        let skill = GitSkill::new(repo.path().to_path_buf());
        let output = run_tool(&skill, "git_diff", serde_json::json!({ "staged": true }))
            .expect("staged diff should work");

        assert!(output.contains("diff --git"));
        assert!(output.contains("-one"));
        assert!(output.contains("+two"));
    }

    #[test]
    fn git_checkpoint_creates_commit() {
        let repo = init_test_repo();
        fs::write(repo.path().join("checkpoint.txt"), "saved\n").unwrap();
        let skill = GitSkill::new(repo.path().to_path_buf());

        let output = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "checkpoint commit" }),
        )
        .expect("checkpoint should succeed");

        assert!(output.contains("checkpoint commit"));
        let log = Command::new("git")
            .args(["log", "--oneline", "-1"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        let log_text = String::from_utf8(log.stdout).unwrap();
        assert!(log_text.contains("checkpoint commit"));
    }

    #[test]
    fn git_checkpoint_nothing_to_commit() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "clean.txt", "clean\n");
        let skill = GitSkill::new(repo.path().to_path_buf());

        let output = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "no-op" }),
        )
        .expect("checkpoint should return a clean message");

        assert_eq!(output, "nothing to commit, working tree clean");
    }

    #[test]
    fn git_checkpoint_requires_message() {
        let repo = init_test_repo();
        let skill = GitSkill::new(repo.path().to_path_buf());
        let error = run_tool(&skill, "git_checkpoint", serde_json::json!({}))
            .expect_err("missing message should fail");
        assert!(error.contains("missing field `message`"));
    }

    #[test]
    fn git_checkpoint_rejects_whitespace_only_message() {
        let repo = init_test_repo();
        let skill = GitSkill::new(repo.path().to_path_buf());
        let error = run_tool(
            &skill,
            "git_checkpoint",
            serde_json::json!({ "message": "   " }),
        )
        .expect_err("whitespace-only message should fail");
        assert!(error.contains("missing required field: message"));
    }

    #[test]
    fn git_diff_ref_injection_prevented() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "safe.txt", "one\n");
        fs::write(repo.path().join("safe.txt"), "two\n").unwrap();

        let evil_output = repo.path().join("evil.patch");
        let reference = format!("--output={}", evil_output.display());
        let skill = GitSkill::new(repo.path().to_path_buf());
        let error = run_tool(&skill, "git_diff", serde_json::json!({ "ref": reference }))
            .expect_err("dash-prefixed ref should fail");

        assert!(error.contains("invalid ref: refs cannot start with '-'"));
        assert!(
            !evil_output.exists(),
            "git flag injection created output file"
        );
    }

    #[test]
    fn git_diff_with_valid_ref_compares_against_main() {
        let repo = init_test_repo();
        seed_initial_commit(&repo, "safe.txt", "one\n");
        Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        fs::write(repo.path().join("safe.txt"), "two\n").unwrap();

        let skill = GitSkill::new(repo.path().to_path_buf());
        let output = run_tool(&skill, "git_diff", serde_json::json!({ "ref": "main" }))
            .expect("valid ref diff should succeed");

        assert!(output.contains("diff --git"));
        assert!(output.contains("-one"));
        assert!(output.contains("+two"));
    }

    #[test]
    fn git_diff_truncates_large_output() {
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
        fs::write(repo.path().join("huge.txt"), &large_new).unwrap();

        let skill = GitSkill::new(repo.path().to_path_buf());
        let output = run_tool(&skill, "git_diff", serde_json::json!({})).expect("diff should work");

        assert!(output.ends_with("(truncated)"));
        assert!(output.chars().count() > MAX_DIFF_OUTPUT_CHARS);
    }
}
