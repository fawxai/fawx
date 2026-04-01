use crate::{ConsensusError, EvaluationWorkspace, Experiment, Signal, TestResult};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use tokio::process::Command;

pub struct CargoWorkspace {
    project_dir: PathBuf,
    baseline_tests: TestResult,
    package: Option<String>,
    _workspace_root: Option<TempDir>,
}

impl CargoWorkspace {
    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    /// Derive a crate package name from a scope path like
    /// `engine/crates/fx-consensus/src/scoring.rs`.
    /// Looks for a `crates/<name>/` pattern in the path.
    pub fn package_from_scope(scope: &str) -> Option<String> {
        let parts: Vec<&str> = scope.split('/').collect();
        for window in parts.windows(2) {
            if window[0] == "crates" {
                return Some(window[1].to_owned());
            }
        }
        None
    }

    pub fn new(project_dir: PathBuf) -> crate::Result<Self> {
        Self::with_package(project_dir, None)
    }

    pub fn with_package(project_dir: PathBuf, package: Option<String>) -> crate::Result<Self> {
        validate_workspace_dir(&project_dir)?;
        let baseline_tests = collect_baseline_tests(&project_dir, package.as_deref())?;
        Ok(Self {
            project_dir,
            baseline_tests,
            package,
            _workspace_root: None,
        })
    }

    pub fn clone_from(source_dir: &Path, label: &str) -> crate::Result<Self> {
        Self::clone_from_with_package(source_dir, label, None)
    }

    pub fn clone_from_with_package(
        source_dir: &Path,
        label: &str,
        package: Option<String>,
    ) -> crate::Result<Self> {
        let workspace_root = tempfile::Builder::new()
            .prefix(&format!("fx-experiment-{label}-"))
            .tempdir()
            .map_err(|error| ConsensusError::WorkspaceError(error.to_string()))?;
        let project_dir = workspace_root.path().join("project");
        clone_project_dir(source_dir, &project_dir)?;
        let mut workspace = Self::with_package(project_dir, package)?;
        workspace._workspace_root = Some(workspace_root);
        Ok(workspace)
    }

    pub fn clone_node_workspaces(
        source_dir: &Path,
        node_label: &str,
        package: Option<String>,
    ) -> crate::Result<(Self, Self)> {
        let generator = Self::clone_node_workspace(source_dir, node_label, "gen", package.clone())?;
        let evaluator = Self::clone_node_workspace(source_dir, node_label, "eval", package)?;
        Ok((generator, evaluator))
    }

    fn clone_node_workspace(
        source_dir: &Path,
        node_label: &str,
        role: &str,
        package: Option<String>,
    ) -> crate::Result<Self> {
        Self::clone_from_with_package(source_dir, &format!("{node_label}-{role}"), package)
    }

    async fn run_cargo(&self, subcommand: &str) -> crate::Result<String> {
        let mut cmd = async_cargo_command(&self.project_dir, self.package.as_deref(), subcommand);
        let output = cmd
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
    async fn apply_patch(&self, patch: &str) -> crate::Result<()> {
        // git apply requires a trailing newline; ensure it's present
        // (various extraction paths trim() the patch text)
        let patch = if patch.ends_with('\n') {
            patch.to_owned()
        } else {
            format!("{patch}\n")
        };
        let mut patch_file = NamedTempFile::new_in(&self.project_dir)
            .map_err(|error| ConsensusError::PatchFailed(error.to_string()))?;
        use std::io::Write as _;
        patch_file
            .write_all(patch.as_bytes())
            .map_err(|error| ConsensusError::PatchFailed(error.to_string()))?;
        let output = Command::new("git")
            .arg("apply")
            .arg(patch_file.path())
            .current_dir(&self.project_dir)
            .output()
            .await
            .map_err(|error| ConsensusError::PatchFailed(error.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(ConsensusError::PatchFailed(format!(
                "git apply failed: {stderr}"
            )))
        }
    }

    async fn build(&self) -> crate::Result<()> {
        self.run_cargo("build").await.map(|_| ())
    }

    async fn test(&self) -> crate::Result<TestResult> {
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

    async fn check_signal(&self, _signal: &Signal) -> crate::Result<bool> {
        Ok(false)
    }

    async fn check_regression(&self, _experiment: &Experiment) -> crate::Result<bool> {
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

    async fn reset(&self) -> crate::Result<()> {
        run_git_command(&self.project_dir, &["checkout", "--", "."]).await?;
        run_git_command(&self.project_dir, &["clean", "-fd"]).await
    }
}

fn validate_workspace_dir(project_dir: &Path) -> crate::Result<()> {
    let manifest = project_dir.join("Cargo.toml");
    if !manifest.exists() {
        return Err(ConsensusError::WorkspaceError(format!(
            "missing Cargo.toml in {}",
            project_dir.display()
        )));
    }
    verify_git_repo(project_dir)
}

fn collect_baseline_tests(project_dir: &Path, package: Option<&str>) -> crate::Result<TestResult> {
    let mut cmd = blocking_cargo_command(project_dir, package, "test");
    let output = cmd
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

fn async_cargo_command(project_dir: &Path, package: Option<&str>, subcommand: &str) -> Command {
    let mut cmd = Command::new("cargo");
    configure_cargo_command(cmd.as_std_mut(), project_dir, package, subcommand);
    cmd
}

fn blocking_cargo_command(
    project_dir: &Path,
    package: Option<&str>,
    subcommand: &str,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("cargo");
    configure_cargo_command(&mut cmd, project_dir, package, subcommand);
    cmd
}

fn configure_cargo_command(
    cmd: &mut std::process::Command,
    project_dir: &Path,
    package: Option<&str>,
    subcommand: &str,
) {
    cmd.arg(subcommand)
        .current_dir(project_dir)
        .env_remove("CARGO_TARGET_DIR");
    if let Some(pkg) = package {
        cmd.args(["-p", pkg]);
    }
}

fn verify_git_repo(project_dir: &Path) -> crate::Result<()> {
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

fn clone_project_dir(source_dir: &Path, target_dir: &Path) -> crate::Result<()> {
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

async fn run_git_command(project_dir: &Path, args: &[&str]) -> crate::Result<()> {
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

pub(crate) fn parse_test_result(output: &str) -> TestResult {
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
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn project_dir_returns_configured_path() {
        let temp = TempDir::new().expect("temp dir");
        init_git_project(temp.path());
        write_project_files(
            temp.path(),
            "pub fn value() -> i32 { 1 }\n",
            "assert_eq!(demo::value(), 1);",
        );
        let workspace = CargoWorkspace::new(temp.path().to_path_buf()).expect("workspace");
        assert_eq!(workspace.project_dir(), temp.path());
    }

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

    #[tokio::test]
    async fn apply_patch_handles_indented_code() {
        let temp = TempDir::new().expect("temp dir");
        init_git_project(temp.path());
        write_project_files(
            temp.path(),
            concat!(
                "pub fn value() -> i32 { 1 }\n",
                "\n",
                "#[cfg(test)]\n",
                "mod tests {\n",
                "    use super::*;\n",
                "\n",
                "    #[test]\n",
                "    fn it_works() {\n",
                "        assert_eq!(value(), 1);\n",
                "    }\n",
                "}\n",
            ),
            "assert_eq!(demo::value(), 1);",
        );
        let workspace = CargoWorkspace::new(temp.path().to_path_buf()).expect("workspace");

        // Patch adds an indented test inside the existing mod tests block
        let patch = concat!(
            "diff --git a/src/lib.rs b/src/lib.rs\n",
            "--- a/src/lib.rs\n",
            "+++ b/src/lib.rs\n",
            "@@ -9,4 +9,9 @@ mod tests {\n",
            "     fn it_works() {\n",
            "         assert_eq!(value(), 1);\n",
            "     }\n",
            "+\n",
            "+    #[test]\n",
            "+    fn it_also_works() {\n",
            "+        assert_eq!(value() + 1, 2);\n",
            "+    }\n",
            " }\n",
        );

        workspace
            .apply_patch(patch)
            .await
            .expect("patch should apply to indented code");

        let content = fs::read_to_string(temp.path().join("src/lib.rs")).expect("read");
        assert!(content.contains("fn it_also_works()"));
        assert!(content.contains("        assert_eq!(value() + 1, 2);"));
    }

    #[test]
    fn clone_node_workspaces_creates_isolated_generator_and_evaluator_clones() {
        let temp = TempDir::new().expect("temp dir");
        init_git_project(temp.path());
        write_project_files(
            temp.path(),
            "pub fn value() -> i32 { 1 }\n",
            "assert_eq!(demo::value(), 1);",
        );

        let (generator, evaluator) =
            CargoWorkspace::clone_node_workspaces(temp.path(), "node-0", None)
                .expect("clone workspaces");

        assert_ne!(generator.project_dir(), evaluator.project_dir());
        fs::write(
            generator.project_dir().join("src/lib.rs"),
            "pub fn value() -> i32 { 2 }\n",
        )
        .expect("write generator clone");

        let evaluator_lib = fs::read_to_string(evaluator.project_dir().join("src/lib.rs"))
            .expect("read evaluator clone");

        assert_eq!(evaluator_lib, "pub fn value() -> i32 { 1 }\n");
    }

    #[test]
    fn cargo_commands_remove_inherited_target_dir() {
        let async_command = async_cargo_command(Path::new("/tmp/project"), None, "test");
        let blocking_command = blocking_cargo_command(Path::new("/tmp/project"), None, "test");
        let async_removed = async_command
            .as_std()
            .get_envs()
            .any(|(key, value)| key == "CARGO_TARGET_DIR" && value.is_none());
        let blocking_removed = blocking_command
            .get_envs()
            .any(|(key, value)| key == "CARGO_TARGET_DIR" && value.is_none());

        assert!(
            async_removed,
            "async cargo commands must ignore shared target dirs"
        );
        assert!(
            blocking_removed,
            "blocking cargo commands must ignore shared target dirs"
        );
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
                severity: crate::Severity::Low,
            },
            hypothesis: "hypothesis".to_owned(),
            fitness_criteria: Vec::new(),
            scope: crate::ModificationScope {
                allowed_files: Vec::new(),
                proposal_tier: crate::ProposalTier::Tier1,
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
