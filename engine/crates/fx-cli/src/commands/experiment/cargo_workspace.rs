use fx_consensus::{ConsensusError, EvaluationWorkspace, Experiment, Signal, TestResult};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use tokio::process::Command;

pub struct CargoWorkspace {
    project_dir: PathBuf,
    baseline_tests: TestResult,
    _workspace_root: Option<TempDir>,
}

impl CargoWorkspace {
    pub fn new(project_dir: PathBuf) -> fx_consensus::Result<Self> {
        validate_workspace_dir(&project_dir)?;
        let baseline_tests = collect_baseline_tests(&project_dir)?;
        Ok(Self {
            project_dir,
            baseline_tests,
            _workspace_root: None,
        })
    }

    pub fn clone_from(source_dir: &Path, label: &str) -> fx_consensus::Result<Self> {
        let workspace_root = tempfile::Builder::new()
            .prefix(&format!("fx-experiment-{label}-"))
            .tempdir()
            .map_err(|error| ConsensusError::WorkspaceError(error.to_string()))?;
        let project_dir = workspace_root.path().join("project");
        clone_project_dir(source_dir, &project_dir)?;
        let mut workspace = Self::new(project_dir)?;
        workspace._workspace_root = Some(workspace_root);
        Ok(workspace)
    }

    async fn run_cargo(&self, subcommand: &str) -> fx_consensus::Result<String> {
        let output = Command::new("cargo")
            .arg(subcommand)
            .current_dir(&self.project_dir)
            .output()
            .await
            .map_err(|error| ConsensusError::WorkspaceError(error.to_string()))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}{stderr}");
        if output.status.success() {
            Ok(combined)
        } else if subcommand == "test" {
            let result = parse_test_result(&combined);
            Err(ConsensusError::TestFailed {
                passed: result.passed,
                failed: result.failed,
                total: result.total,
            })
        } else {
            Err(ConsensusError::BuildFailed(combined))
        }
    }
}

#[async_trait::async_trait]
impl EvaluationWorkspace for CargoWorkspace {
    async fn apply_patch(&self, patch: &str) -> fx_consensus::Result<()> {
        let mut patch_file = NamedTempFile::new_in(&self.project_dir)
            .map_err(|error| ConsensusError::PatchFailed(error.to_string()))?;
        use std::io::Write as _;
        patch_file
            .write_all(patch.as_bytes())
            .map_err(|error| ConsensusError::PatchFailed(error.to_string()))?;
        let git_apply = Command::new("git")
            .arg("apply")
            .arg(patch_file.path())
            .current_dir(&self.project_dir)
            .output()
            .await
            .map_err(|error| ConsensusError::PatchFailed(error.to_string()))?;
        if git_apply.status.success() {
            return Ok(());
        }
        apply_patch_directly(&self.project_dir, patch)
    }

    async fn build(&self) -> fx_consensus::Result<()> {
        self.run_cargo("build").await.map(|_| ())
    }

    async fn test(&self) -> fx_consensus::Result<TestResult> {
        match self.run_cargo("test").await {
            Ok(output) => Ok(parse_test_result(&output)),
            Err(ConsensusError::TestFailed {
                passed,
                failed,
                total,
            }) => Err(ConsensusError::TestFailed {
                passed,
                failed,
                total,
            }),
            Err(error) => Err(error),
        }
    }

    async fn check_signal(&self, _signal: &Signal) -> fx_consensus::Result<bool> {
        Ok(false)
    }

    async fn check_regression(&self, _experiment: &Experiment) -> fx_consensus::Result<bool> {
        let candidate = self.test().await;
        self.reset().await?;
        match candidate {
            Ok(result) => Ok(result.passed < self.baseline_tests.passed),
            Err(ConsensusError::TestFailed { passed, .. }) => {
                Ok(passed < self.baseline_tests.passed)
            }
            Err(error) => Err(error),
        }
    }

    async fn reset(&self) -> fx_consensus::Result<()> {
        run_git_command(&self.project_dir, &["checkout", "--", "."]).await?;
        run_git_command(&self.project_dir, &["clean", "-fd"]).await
    }
}

fn validate_workspace_dir(project_dir: &Path) -> fx_consensus::Result<()> {
    let manifest = project_dir.join("Cargo.toml");
    if !manifest.exists() {
        return Err(ConsensusError::WorkspaceError(format!(
            "missing Cargo.toml in {}",
            project_dir.display()
        )));
    }
    verify_git_repo(project_dir)
}

fn collect_baseline_tests(project_dir: &Path) -> fx_consensus::Result<TestResult> {
    let output = std::process::Command::new("cargo")
        .arg("test")
        .current_dir(project_dir)
        .output()
        .map_err(|error| ConsensusError::WorkspaceError(error.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    let result = parse_test_result(&combined);
    if output.status.success() || result.total > 0 {
        Ok(result)
    } else {
        Err(ConsensusError::WorkspaceError(format!(
            "failed to collect baseline tests: {combined}"
        )))
    }
}

fn verify_git_repo(project_dir: &Path) -> fx_consensus::Result<()> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(project_dir)
        .output()
        .map_err(|error| ConsensusError::WorkspaceError(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ConsensusError::WorkspaceError(format!(
            "{} is not a git repository",
            project_dir.display()
        )))
    }
}

fn clone_project_dir(source_dir: &Path, target_dir: &Path) -> fx_consensus::Result<()> {
    let output = std::process::Command::new("git")
        .args(["clone", "--local"])
        .arg(source_dir)
        .arg(target_dir)
        .output()
        .map_err(|error| ConsensusError::WorkspaceError(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ConsensusError::WorkspaceError(format!(
            "failed to clone {} into {}: {}",
            source_dir.display(),
            target_dir.display(),
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

async fn run_git_command(project_dir: &Path, args: &[&str]) -> fx_consensus::Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(project_dir)
        .output()
        .await
        .map_err(|error| ConsensusError::WorkspaceError(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ConsensusError::WorkspaceError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

fn apply_patch_directly(project_dir: &Path, patch: &str) -> fx_consensus::Result<()> {
    let file_patch = parse_single_file_patch(patch)?;
    let path = project_dir.join(file_patch.path);
    let original = fs::read_to_string(&path)
        .map_err(|error| ConsensusError::PatchFailed(error.to_string()))?;
    let updated = apply_hunks(&original, &file_patch.hunks)?;
    fs::write(path, updated).map_err(|error| ConsensusError::PatchFailed(error.to_string()))
}

struct FilePatch {
    path: PathBuf,
    hunks: Vec<Hunk>,
}

struct Hunk {
    lines: Vec<HunkLine>,
}

enum HunkLine {
    Context(String),
    Remove(String),
    Add(String),
}

fn parse_single_file_patch(patch: &str) -> fx_consensus::Result<FilePatch> {
    let file_count = patch
        .lines()
        .filter(|line| line.starts_with("diff --git "))
        .count();
    if file_count > 1 {
        return Err(ConsensusError::PatchFailed(
            "direct patch fallback only supports single-file patches".to_owned(),
        ));
    }

    let mut path = None;
    let mut hunks = Vec::new();
    let mut current = Vec::new();
    for line in patch.lines() {
        if let Some(stripped) = line.strip_prefix("+++ b/") {
            path = Some(PathBuf::from(stripped));
            continue;
        }
        if line.starts_with("@@") {
            if !current.is_empty() {
                hunks.push(Hunk { lines: current });
                current = Vec::new();
            }
            continue;
        }
        if let Some(content) = line.strip_prefix(' ') {
            current.push(HunkLine::Context(content.to_owned()));
            continue;
        }
        if let Some(content) = line.strip_prefix('-') {
            current.push(HunkLine::Remove(content.to_owned()));
            continue;
        }
        if let Some(content) = line.strip_prefix('+') {
            current.push(HunkLine::Add(content.to_owned()));
        }
    }
    if !current.is_empty() {
        hunks.push(Hunk { lines: current });
    }
    let Some(path) = path else {
        return Err(ConsensusError::PatchFailed(
            "patch missing target file".to_owned(),
        ));
    };
    Ok(FilePatch { path, hunks })
}

fn apply_hunks(original: &str, hunks: &[Hunk]) -> fx_consensus::Result<String> {
    let mut current = original.to_owned();
    for hunk in hunks {
        let before = hunk
            .lines
            .iter()
            .filter_map(|line| match line {
                HunkLine::Context(text) | HunkLine::Remove(text) => Some(text.as_str()),
                HunkLine::Add(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let after = hunk
            .lines
            .iter()
            .filter_map(|line| match line {
                HunkLine::Context(text) | HunkLine::Add(text) => Some(text.as_str()),
                HunkLine::Remove(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !current.contains(&before) {
            return Err(ConsensusError::PatchFailed(
                "failed to map patch hunk onto file contents".to_owned(),
            ));
        }
        current = current.replacen(&before, &after, 1);
    }
    Ok(current)
}

fn parse_test_result(output: &str) -> TestResult {
    let mut passed = 0;
    let mut failed = 0;
    let mut total = 0;
    for line in output.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("test result:") {
            continue;
        }
        let line_passed = parse_count(trimmed, "passed");
        let line_failed = parse_count(trimmed, "failed");
        passed += line_passed;
        failed += line_failed;
        total += line_passed + line_failed;
    }
    TestResult {
        passed,
        failed,
        total,
    }
}

fn parse_count(line: &str, label: &str) -> u32 {
    line.replace(';', ",")
        .split(',')
        .find_map(|segment| {
            let trimmed = segment.trim();
            if !trimmed.ends_with(label) {
                return None;
            }
            trimmed
                .split_whitespace()
                .find_map(|token| token.parse::<u32>().ok())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn apply_patch_build_and_test_work_for_temp_project() {
        let temp = TempDir::new().expect("temp dir");
        init_git_project(temp.path());
        write_project_files(
            temp.path(),
            "pub fn value() -> i32 { 1 }\n",
            "assert_eq!(demo::value(), 1);",
        );

        let workspace = CargoWorkspace::new(temp.path().to_path_buf()).expect("workspace");
        let patch = concat!(
            "diff --git a/src/lib.rs b/src/lib.rs\n",
            "--- a/src/lib.rs\n",
            "+++ b/src/lib.rs\n",
            "@@ -1 +1 @@\n",
            "-pub fn value() -> i32 { 1 }\n",
            "+pub fn value() -> i32 { 2 }\n"
        );

        workspace.apply_patch(patch).await.expect("apply patch");
        workspace.build().await.expect("build");
        let failure = workspace.test().await.expect_err("tests should fail");
        match failure {
            ConsensusError::TestFailed {
                passed,
                failed,
                total,
            } => {
                assert_eq!(passed, 0);
                assert_eq!(failed, 1);
                assert_eq!(total, 1);
            }
            other => panic!("unexpected error: {other}"),
        }

        workspace.reset().await.expect("reset");
        let result = workspace.test().await.expect("tests after reset");
        assert_eq!(result.passed, 1);
        assert_eq!(result.failed, 0);
    }

    #[tokio::test]
    async fn check_regression_compares_candidate_against_baseline() {
        let temp = TempDir::new().expect("temp dir");
        init_git_project(temp.path());
        write_project_files(
            temp.path(),
            "pub fn value() -> i32 { 1 }\n",
            "assert_eq!(demo::value(), 1);",
        );

        let workspace = CargoWorkspace::new(temp.path().to_path_buf()).expect("workspace");
        let patch = concat!(
            "diff --git a/src/lib.rs b/src/lib.rs\n",
            "--- a/src/lib.rs\n",
            "+++ b/src/lib.rs\n",
            "@@ -1 +1 @@\n",
            "-pub fn value() -> i32 { 1 }\n",
            "+pub fn value() -> i32 { 2 }\n"
        );

        workspace.apply_patch(patch).await.expect("apply patch");

        assert!(workspace
            .check_regression(&sample_experiment())
            .await
            .expect("regression result"));

        let result = workspace.test().await.expect("tests after reset");
        assert_eq!(result.passed, 1);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn direct_patch_fallback_rejects_multi_file_patch() {
        let result = parse_single_file_patch(concat!(
            "diff --git a/src/lib.rs b/src/lib.rs\n",
            "--- a/src/lib.rs\n",
            "+++ b/src/lib.rs\n",
            "@@ -1 +1 @@\n",
            "-old\n",
            "+new\n",
            "diff --git a/tests/basic.rs b/tests/basic.rs\n",
            "--- a/tests/basic.rs\n",
            "+++ b/tests/basic.rs\n",
            "@@ -1 +1 @@\n",
            "-old\n",
            "+new\n"
        ));

        match result {
            Err(error) => assert!(error
                .to_string()
                .contains("direct patch fallback only supports single-file patches")),
            Ok(_) => panic!("multi-file patch should fail"),
        }
    }

    #[test]
    fn new_rejects_non_git_directory() {
        let temp = TempDir::new().expect("temp dir");
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");

        let result = CargoWorkspace::new(temp.path().to_path_buf());

        match result {
            Err(error) => assert!(error.to_string().contains("is not a git repository")),
            Ok(_) => panic!("non-git repo should fail"),
        }
    }

    fn sample_experiment() -> Experiment {
        Experiment {
            id: uuid::Uuid::nil(),
            trigger: Signal {
                id: uuid::Uuid::nil(),
                name: "signal".to_owned(),
                description: "signal".to_owned(),
                severity: fx_consensus::Severity::Low,
            },
            hypothesis: "hypothesis".to_owned(),
            fitness_criteria: Vec::new(),
            scope: fx_consensus::ModificationScope {
                allowed_files: Vec::new(),
                proposal_tier: fx_consensus::ProposalTier::Tier1,
            },
            timeout: std::time::Duration::from_secs(1),
            min_candidates: 1,
            created_at: chrono::Utc::now(),
        }
    }

    fn init_git_project(path: &Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .status()
            .expect("git init");
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .status()
            .expect("git email");
        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .status()
            .expect("git name");
    }

    fn write_project_files(path: &Path, library: &str, assertion: &str) {
        fs::create_dir_all(path.join("src")).expect("src dir");
        fs::create_dir_all(path.join("tests")).expect("tests dir");
        fs::write(
            path.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");
        fs::write(path.join("src/lib.rs"), library).expect("lib");
        fs::write(
            path.join("tests/basic.rs"),
            format!("#[test]\nfn works() {{ {assertion} }}\n"),
        )
        .expect("test");
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .status()
            .expect("git add");
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(path)
            .status()
            .expect("git commit");
    }
}
