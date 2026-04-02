use crate::restart::{
    self, BuildOutcome, LiveRestartSystem, RestartSignal, RestartSystem, SkillBuildResult,
};
use anyhow::{anyhow, Context};
use clap::Args;
use std::{
    fmt,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(5);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const HTTP_PORT: u16 = 8400;
const SKILL_WASM_TARGET: &str = "wasm32-wasip1";

#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateArgs {
    /// Git branch to pull from
    pub(crate) branch: Option<String>,

    /// Skip git pull and rebuild the current working tree
    #[arg(long)]
    pub(crate) no_pull: bool,

    /// Skip WASM skill rebuild and install
    #[arg(long)]
    pub(crate) no_skills: bool,

    /// Build only; do not restart the running instance
    #[arg(long)]
    pub(crate) no_restart: bool,

    /// Continue even if the working tree has uncommitted changes
    #[arg(long)]
    pub(crate) force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateConfig {
    pid_file: PathBuf,
    current_exe: PathBuf,
    repo_root: PathBuf,
    stop_timeout: Duration,
    ready_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateSummary {
    git_result: GitResult,
    skill_result: SkillBuildResult,
    server_result: ServerResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GitResult {
    Skipped,
    Updated {
        branch: String,
        previous_sha: String,
        current_sha: String,
    },
    AlreadyCurrent {
        branch: String,
        sha: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ServerResult {
    Skipped,
    Restarted { pid: u32 },
    StartedFresh { pid: u32 },
}

#[derive(Debug)]
enum UpdateError {
    Preflight(anyhow::Error),
    Git(anyhow::Error),
    Build(anyhow::Error),
    Restart(anyhow::Error),
}

impl UpdateError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Preflight(_) => 1,
            Self::Git(_) => 2,
            Self::Build(_) => 3,
            Self::Restart(_) => 4,
        }
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight(error)
            | Self::Git(error)
            | Self::Build(error)
            | Self::Restart(error) => write!(formatter, "{error}"),
        }
    }
}

trait UpdateSystem: RestartSystem {
    fn check_cargo(&self) -> anyhow::Result<()>;
    fn check_wasm_target(&self) -> anyhow::Result<()>;
    fn git_status_porcelain(&self, repo_root: &Path) -> anyhow::Result<String>;
    fn git_current_branch(&self, repo_root: &Path) -> anyhow::Result<String>;
    fn git_head_sha(&self, repo_root: &Path) -> anyhow::Result<String>;
    fn git_fetch_origin(&self, repo_root: &Path) -> anyhow::Result<()>;
    fn git_checkout(&self, repo_root: &Path, branch: &str) -> anyhow::Result<()>;
    fn git_pull_ff_only(&self, repo_root: &Path, branch: &str) -> anyhow::Result<()>;
    fn verify_server_ready(&self, pid_file: &Path, timeout: Duration) -> anyhow::Result<()>;
}

struct LiveUpdateSystem {
    restart: LiveRestartSystem,
}

impl RestartSystem for LiveUpdateSystem {
    fn process_exists(&self, pid: u32) -> anyhow::Result<bool> {
        self.restart.process_exists(pid)
    }

    fn find_fawx_process(&self, exclude_pid: u32) -> anyhow::Result<Option<u32>> {
        self.restart.find_fawx_process(exclude_pid)
    }

    fn send_signal(&self, pid: u32, signal: RestartSignal) -> anyhow::Result<()> {
        self.restart.send_signal(pid, signal)
    }

    fn build_all(&self, repo_root: &Path, skip_skills: bool) -> anyhow::Result<BuildOutcome> {
        self.restart.build_all(repo_root, skip_skills)
    }

    fn spawn_serve(&self, executable: &Path) -> anyhow::Result<u32> {
        self.restart.spawn_serve(executable)
    }
}

impl UpdateSystem for LiveUpdateSystem {
    fn check_cargo(&self) -> anyhow::Result<()> {
        let _ = restart::cargo_binary()?;
        Ok(())
    }

    fn check_wasm_target(&self) -> anyhow::Result<()> {
        let output = run_command_output(
            "rustup",
            &["target", "list", "--installed"],
            None,
            "failed to run rustup while checking skill target",
        )?;
        if output.lines().any(|line| line.trim() == SKILL_WASM_TARGET) {
            return Ok(());
        }
        Err(anyhow!(
            "Missing {SKILL_WASM_TARGET} target. Run: rustup target add {SKILL_WASM_TARGET}"
        ))
    }

    fn git_status_porcelain(&self, repo_root: &Path) -> anyhow::Result<String> {
        run_command_output(
            "git",
            &["status", "--porcelain"],
            Some(repo_root),
            "failed to inspect git status",
        )
    }

    fn git_current_branch(&self, repo_root: &Path) -> anyhow::Result<String> {
        let branch = run_command_output(
            "git",
            &["rev-parse", "--abbrev-ref", "HEAD"],
            Some(repo_root),
            "failed to determine current branch",
        )?;
        Ok(branch.trim().to_string())
    }

    fn git_head_sha(&self, repo_root: &Path) -> anyhow::Result<String> {
        let sha = run_command_output(
            "git",
            &["rev-parse", "--short", "HEAD"],
            Some(repo_root),
            "failed to determine current revision",
        )?;
        Ok(sha.trim().to_string())
    }

    fn git_fetch_origin(&self, repo_root: &Path) -> anyhow::Result<()> {
        run_command_status(
            "git",
            &["fetch", "origin"],
            Some(repo_root),
            "failed to fetch origin",
        )
    }

    fn git_checkout(&self, repo_root: &Path, branch: &str) -> anyhow::Result<()> {
        let status = Command::new("git")
            .current_dir(repo_root)
            .args(["checkout", branch])
            .status()
            .with_context(|| format!("failed to checkout branch {branch}"))?;
        if status.success() {
            return Ok(());
        }
        Err(anyhow!("Branch '{branch}' not found on remote."))
    }

    fn git_pull_ff_only(&self, repo_root: &Path, branch: &str) -> anyhow::Result<()> {
        let output = Command::new("git")
            .current_dir(repo_root)
            .args(["pull", "origin", branch, "--ff-only"])
            .output()
            .with_context(|| format!("failed to pull origin/{branch}"))?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Not possible to fast-forward")
            || stderr.contains("divergent branches")
            || stderr.contains("fast-forward")
        {
            return Err(anyhow!(
                "Cannot fast-forward. Resolve manually: git pull --rebase origin {branch}"
            ));
        }
        let detail = stderr.trim();
        if detail.is_empty() {
            Err(anyhow!("git pull origin {branch} --ff-only failed"))
        } else {
            Err(anyhow!(detail.to_string()))
        }
    }

    fn verify_server_ready(&self, pid_file: &Path, timeout: Duration) -> anyhow::Result<()> {
        wait_for_pid_file(pid_file, timeout)?;
        wait_for_http_port(timeout)
    }
}

pub(crate) fn run(args: UpdateArgs) -> anyhow::Result<i32> {
    let system = LiveUpdateSystem {
        restart: LiveRestartSystem,
    };
    match update_config()
        .map_err(UpdateError::Preflight)
        .and_then(|config| execute_update(&system, &config, &args))
    {
        Ok(summary) => {
            print_summary(&summary);
            Ok(0)
        }
        Err(error) => {
            eprintln!("Error: {error}");
            Ok(error.exit_code())
        }
    }
}

fn update_config() -> anyhow::Result<UpdateConfig> {
    let current_exe = std::env::current_exe().context("failed to locate current executable")?;
    let current_dir = std::env::current_dir().context("failed to read current directory")?;
    let repo_root = crate::repo_root::resolve_repo_root(&current_dir, &current_exe)
        .map_err(|_| anyhow!("Not a git repository. Run from the fawx source directory."))?;
    Ok(UpdateConfig {
        pid_file: restart::pid_file_path(),
        current_exe,
        repo_root,
        stop_timeout: restart::DEFAULT_STOP_TIMEOUT,
        ready_timeout: DEFAULT_READY_TIMEOUT,
    })
}

fn execute_update(
    system: &impl UpdateSystem,
    config: &UpdateConfig,
    args: &UpdateArgs,
) -> Result<UpdateSummary, UpdateError> {
    let branch = run_preflight(system, &config.repo_root, args).map_err(UpdateError::Preflight)?;
    let git_result =
        maybe_pull_branch(system, &config.repo_root, args, &branch).map_err(UpdateError::Git)?;
    let build_outcome = system
        .build_all(&config.repo_root, args.no_skills)
        .map_err(UpdateError::Build)?;
    let server_result =
        maybe_restart(system, config, args.no_restart).map_err(UpdateError::Restart)?;
    Ok(UpdateSummary {
        git_result,
        skill_result: build_outcome.skill_result,
        server_result,
    })
}

fn run_preflight(
    system: &impl UpdateSystem,
    repo_root: &Path,
    args: &UpdateArgs,
) -> anyhow::Result<String> {
    system.check_cargo()?;
    if !args.no_skills {
        system.check_wasm_target()?;
    }
    ensure_clean_worktree(system, repo_root, args.force)?;
    println!("Pre-flight checks... OK");
    match &args.branch {
        Some(branch) => Ok(branch.clone()),
        None => system.git_current_branch(repo_root),
    }
}

fn ensure_clean_worktree(
    system: &impl UpdateSystem,
    repo_root: &Path,
    force: bool,
) -> anyhow::Result<()> {
    let status = system.git_status_porcelain(repo_root)?;
    if status.trim().is_empty() || force {
        return Ok(());
    }
    Err(anyhow!(
        "Working tree has uncommitted changes. Use --force to update anyway."
    ))
}

fn maybe_pull_branch(
    system: &impl UpdateSystem,
    repo_root: &Path,
    args: &UpdateArgs,
    branch: &str,
) -> anyhow::Result<GitResult> {
    if args.no_pull {
        return Ok(GitResult::Skipped);
    }
    println!("Fetching origin...");
    system.git_fetch_origin(repo_root)?;
    let previous_sha = prepare_target_branch(system, repo_root, branch)?;
    system.git_pull_ff_only(repo_root, branch)?;
    let current_sha = system.git_head_sha(repo_root)?;
    if current_sha == previous_sha {
        return Ok(GitResult::AlreadyCurrent {
            branch: branch.to_string(),
            sha: current_sha,
        });
    }
    Ok(GitResult::Updated {
        branch: branch.to_string(),
        previous_sha,
        current_sha,
    })
}

fn prepare_target_branch(
    system: &impl UpdateSystem,
    repo_root: &Path,
    branch: &str,
) -> anyhow::Result<String> {
    let current_branch = system.git_current_branch(repo_root)?;
    if current_branch != branch {
        system.git_checkout(repo_root, branch)?;
    }
    system.git_head_sha(repo_root)
}

fn maybe_restart(
    system: &impl UpdateSystem,
    config: &UpdateConfig,
    no_restart: bool,
) -> anyhow::Result<ServerResult> {
    if no_restart {
        return Ok(ServerResult::Skipped);
    }
    let had_running_instance = stop_existing_instance(system, config)?;
    let executable =
        restart::release_binary_path(&Some(config.repo_root.clone()), &config.current_exe);
    let pid = system.spawn_serve(&executable)?;
    system.verify_server_ready(&config.pid_file, config.ready_timeout)?;
    if had_running_instance {
        Ok(ServerResult::Restarted { pid })
    } else {
        Ok(ServerResult::StartedFresh { pid })
    }
}

fn stop_existing_instance(
    system: &impl UpdateSystem,
    config: &UpdateConfig,
) -> anyhow::Result<bool> {
    let Some(pid) = restart::resolve_target_pid(system, &config.pid_file)? else {
        println!("No running instance found, starting fresh.");
        return Ok(false);
    };
    system.send_signal(pid, RestartSignal::Terminate)?;
    restart::wait_for_exit(system, pid, config.stop_timeout)?;
    Ok(true)
}

fn print_summary(summary: &UpdateSummary) {
    print_git_summary(&summary.git_result);
    print_skill_summary(&summary.skill_result);
    print_server_summary(&summary.server_result);
    println!("\n✓ Update complete");
}

fn print_git_summary(result: &GitResult) {
    match result {
        GitResult::Skipped => println!("Git: skipped pull"),
        GitResult::Updated {
            branch,
            previous_sha,
            current_sha,
        } => println!("Git: updated {branch} ({previous_sha}..{current_sha})"),
        GitResult::AlreadyCurrent { branch, sha } => {
            println!("Git: already up to date on {branch} ({sha})")
        }
    }
}

fn print_skill_summary(result: &SkillBuildResult) {
    match result {
        SkillBuildResult::Installed => println!("Skills: built and installed"),
        SkillBuildResult::Skipped => println!("Skills: skipped"),
        SkillBuildResult::Failed(error) => println!("Skills: build failed ({error})"),
    }
}

fn print_server_summary(result: &ServerResult) {
    match result {
        ServerResult::Skipped => println!("Server: restart skipped"),
        ServerResult::Restarted { pid } => println!("Server: restarted (pid {pid})"),
        ServerResult::StartedFresh { pid } => println!("Server: started fresh (pid {pid})"),
    }
}

fn run_command_output(
    program: &str,
    args: &[&str],
    current_dir: Option<&Path>,
    context: &str,
) -> anyhow::Result<String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    let output = command.output().with_context(|| context.to_string())?;
    if !output.status.success() {
        return Err(anyhow!(format!("{program} exited with a non-zero status")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_command_status(
    program: &str,
    args: &[&str],
    current_dir: Option<&Path>,
    context: &str,
) -> anyhow::Result<()> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    let status = command.status().with_context(|| context.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(format!("{program} exited with a non-zero status")))
    }
}

fn wait_for_pid_file(pid_file: &Path, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pid_file.is_file() {
            return Ok(());
        }
        thread::sleep(READY_POLL_INTERVAL);
    }
    Err(anyhow!(
        "Timed out waiting for pid file {}",
        pid_file.display()
    ))
}

fn wait_for_http_port(timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    let address = SocketAddr::from(([127, 0, 0, 1], HTTP_PORT));
    while Instant::now() < deadline {
        if TcpStream::connect(address).is_ok() {
            return Ok(());
        }
        thread::sleep(READY_POLL_INTERVAL);
    }
    Err(anyhow!(http_port_timeout_message()))
}

fn http_port_timeout_message() -> String {
    format!(
        "Server not ready: port {HTTP_PORT} not responding after restart. Check logs with: fawx logs"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque, fs};

    struct MockUpdateSystem {
        actions: RefCell<Vec<String>>,
        cargo_ok: bool,
        wasm_ok: bool,
        dirty_tree: bool,
        current_branch: String,
        head_shas: RefCell<VecDeque<String>>,
        fetch_error: RefCell<Option<String>>,
        checkout_error: RefCell<Option<String>>,
        pull_error: RefCell<Option<String>>,
        build_result: RefCell<Result<BuildOutcome, String>>,
        process_exists_responses: RefCell<VecDeque<bool>>,
        search_result: Option<u32>,
        sent_signals: RefCell<Vec<(u32, RestartSignal)>>,
        spawned_paths: RefCell<Vec<PathBuf>>,
        spawn_pid: u32,
        ready_error: RefCell<Option<String>>,
        build_skips: RefCell<Vec<bool>>,
    }

    impl MockUpdateSystem {
        fn new() -> Self {
            Self {
                actions: RefCell::new(Vec::new()),
                cargo_ok: true,
                wasm_ok: true,
                dirty_tree: false,
                current_branch: "dev".to_string(),
                head_shas: RefCell::new(VecDeque::from([
                    "abc1234".to_string(),
                    "def5678".to_string(),
                ])),
                fetch_error: RefCell::new(None),
                checkout_error: RefCell::new(None),
                pull_error: RefCell::new(None),
                build_result: RefCell::new(Ok(BuildOutcome {
                    skill_result: SkillBuildResult::Installed,
                })),
                process_exists_responses: RefCell::new(VecDeque::new()),
                search_result: None,
                sent_signals: RefCell::new(Vec::new()),
                spawned_paths: RefCell::new(Vec::new()),
                spawn_pid: 51_515,
                ready_error: RefCell::new(None),
                build_skips: RefCell::new(Vec::new()),
            }
        }

        fn record(&self, action: impl Into<String>) {
            self.actions.borrow_mut().push(action.into());
        }
    }

    impl RestartSystem for MockUpdateSystem {
        fn process_exists(&self, _pid: u32) -> anyhow::Result<bool> {
            let next = self.process_exists_responses.borrow_mut().pop_front();
            Ok(next.unwrap_or(false))
        }

        fn find_fawx_process(&self, _exclude_pid: u32) -> anyhow::Result<Option<u32>> {
            Ok(self.search_result)
        }

        fn send_signal(&self, pid: u32, signal: RestartSignal) -> anyhow::Result<()> {
            self.record(format!("signal:{pid}:{signal:?}"));
            self.sent_signals.borrow_mut().push((pid, signal));
            Ok(())
        }

        fn build_all(&self, _repo_root: &Path, skip_skills: bool) -> anyhow::Result<BuildOutcome> {
            self.record(format!("build:{skip_skills}"));
            self.build_skips.borrow_mut().push(skip_skills);
            match &*self.build_result.borrow() {
                Ok(result) => Ok(result.clone()),
                Err(message) => Err(anyhow!(message.clone())),
            }
        }

        fn spawn_serve(&self, executable: &Path) -> anyhow::Result<u32> {
            self.record(format!("spawn:{}", executable.display()));
            self.spawned_paths
                .borrow_mut()
                .push(executable.to_path_buf());
            Ok(self.spawn_pid)
        }
    }

    impl UpdateSystem for MockUpdateSystem {
        fn check_cargo(&self) -> anyhow::Result<()> {
            self.record("check-cargo");
            if self.cargo_ok {
                Ok(())
            } else {
                Err(anyhow!("cargo missing"))
            }
        }

        fn check_wasm_target(&self) -> anyhow::Result<()> {
            self.record("check-wasm");
            if self.wasm_ok {
                Ok(())
            } else {
                Err(anyhow!("missing wasm target"))
            }
        }

        fn git_status_porcelain(&self, _repo_root: &Path) -> anyhow::Result<String> {
            self.record("git-status");
            if self.dirty_tree {
                Ok(" M src/main.rs\n".to_string())
            } else {
                Ok(String::new())
            }
        }

        fn git_current_branch(&self, _repo_root: &Path) -> anyhow::Result<String> {
            self.record("git-branch");
            Ok(self.current_branch.clone())
        }

        fn git_head_sha(&self, _repo_root: &Path) -> anyhow::Result<String> {
            self.record("git-head");
            let next = self.head_shas.borrow_mut().pop_front();
            next.ok_or_else(|| anyhow!("missing head sha"))
        }

        fn git_fetch_origin(&self, _repo_root: &Path) -> anyhow::Result<()> {
            self.record("git-fetch");
            match self.fetch_error.borrow_mut().take() {
                Some(message) => Err(anyhow!(message)),
                None => Ok(()),
            }
        }

        fn git_checkout(&self, _repo_root: &Path, branch: &str) -> anyhow::Result<()> {
            self.record(format!("git-checkout:{branch}"));
            match self.checkout_error.borrow_mut().take() {
                Some(message) => Err(anyhow!(message)),
                None => Ok(()),
            }
        }

        fn git_pull_ff_only(&self, _repo_root: &Path, branch: &str) -> anyhow::Result<()> {
            self.record(format!("git-pull:{branch}"));
            match self.pull_error.borrow_mut().take() {
                Some(message) => Err(anyhow!(message)),
                None => Ok(()),
            }
        }

        fn verify_server_ready(&self, _pid_file: &Path, _timeout: Duration) -> anyhow::Result<()> {
            self.record("verify-ready");
            match self.ready_error.borrow_mut().take() {
                Some(message) => Err(anyhow!(message)),
                None => Ok(()),
            }
        }
    }

    fn test_update_config(temp_dir: &tempfile::TempDir) -> UpdateConfig {
        let repo_root = temp_dir.path().join("repo");
        let release_binary = repo_root.join("target").join("release").join("fawx");
        fs::create_dir_all(release_binary.parent().expect("release dir")).expect("release dir");
        fs::write(&release_binary, "binary").expect("release binary");
        UpdateConfig {
            pid_file: temp_dir.path().join("fawx.pid"),
            current_exe: temp_dir.path().join("target").join("debug").join("fawx"),
            repo_root,
            stop_timeout: Duration::from_millis(1),
            ready_timeout: Duration::from_millis(1),
        }
    }

    #[test]
    fn dirty_tree_requires_force() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let mut system = MockUpdateSystem::new();
        system.dirty_tree = true;
        let args = UpdateArgs {
            branch: None,
            no_pull: false,
            no_skills: false,
            no_restart: false,
            force: false,
        };

        let error = execute_update(&system, &config, &args).expect_err("dirty tree should fail");

        assert!(error
            .to_string()
            .contains("Working tree has uncommitted changes"));
    }

    #[test]
    fn dirty_tree_can_continue_with_force() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let mut system = MockUpdateSystem::new();
        system.dirty_tree = true;
        let args = UpdateArgs {
            branch: None,
            no_pull: true,
            no_skills: true,
            no_restart: true,
            force: true,
        };

        let summary = execute_update(&system, &config, &args).expect("forced update should pass");

        assert_eq!(summary.git_result, GitResult::Skipped);
        assert_eq!(*system.build_skips.borrow(), vec![true]);
    }

    #[test]
    fn git_pull_fast_forward_success_reports_sha_range() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let system = MockUpdateSystem::new();
        let args = UpdateArgs {
            branch: Some("dev".to_string()),
            no_pull: false,
            no_skills: true,
            no_restart: true,
            force: false,
        };

        let summary = execute_update(&system, &config, &args).expect("update should pass");

        assert_eq!(
            summary.git_result,
            GitResult::Updated {
                branch: "dev".to_string(),
                previous_sha: "abc1234".to_string(),
                current_sha: "def5678".to_string(),
            }
        );
    }

    #[test]
    fn git_pull_already_current_reports_no_new_commits() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let system = MockUpdateSystem::new();
        *system.head_shas.borrow_mut() =
            VecDeque::from(["abc1234".to_string(), "abc1234".to_string()]);
        let args = UpdateArgs {
            branch: Some("dev".to_string()),
            no_pull: false,
            no_skills: true,
            no_restart: true,
            force: false,
        };

        let summary = execute_update(&system, &config, &args).expect("update should pass");

        assert_eq!(
            summary.git_result,
            GitResult::AlreadyCurrent {
                branch: "dev".to_string(),
                sha: "abc1234".to_string(),
            }
        );
    }

    #[test]
    fn git_pull_failure_bubbles_up() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let system = MockUpdateSystem::new();
        *system.pull_error.borrow_mut() =
            Some("Cannot fast-forward. Resolve manually: git pull --rebase origin dev".to_string());
        let args = UpdateArgs {
            branch: Some("dev".to_string()),
            no_pull: false,
            no_skills: true,
            no_restart: true,
            force: false,
        };

        let error = execute_update(&system, &config, &args).expect_err("pull should fail");

        assert!(error.to_string().contains("Cannot fast-forward"));
    }

    #[test]
    fn build_completes_before_existing_process_is_stopped() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        fs::write(&config.pid_file, "4242\n").expect("pid file");
        let system = MockUpdateSystem::new();
        *system.process_exists_responses.borrow_mut() = VecDeque::from([true, false]);
        let args = UpdateArgs {
            branch: None,
            no_pull: true,
            no_skills: false,
            no_restart: false,
            force: false,
        };

        execute_update(&system, &config, &args).expect("update should restart");

        let actions = system.actions.borrow();
        let build_index = actions
            .iter()
            .position(|action| action == "build:false")
            .expect("build action");
        let signal_index = actions
            .iter()
            .position(|action| action == "signal:4242:Terminate")
            .expect("signal action");
        assert!(build_index < signal_index);
    }

    #[test]
    fn starts_fresh_when_no_running_instance_exists() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let system = MockUpdateSystem::new();
        let args = UpdateArgs {
            branch: None,
            no_pull: true,
            no_skills: true,
            no_restart: false,
            force: false,
        };

        let summary = execute_update(&system, &config, &args).expect("fresh start should work");

        assert_eq!(
            summary.server_result,
            ServerResult::StartedFresh { pid: 51_515 }
        );
        assert!(system.sent_signals.borrow().is_empty());
    }

    #[test]
    fn skill_build_failure_does_not_block_restart() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let system = MockUpdateSystem::new();
        *system.build_result.borrow_mut() = Ok(BuildOutcome {
            skill_result: SkillBuildResult::Failed("skill build exploded".to_string()),
        });
        let args = UpdateArgs {
            branch: None,
            no_pull: true,
            no_skills: false,
            no_restart: false,
            force: false,
        };

        let summary = execute_update(&system, &config, &args).expect("restart should continue");

        assert_eq!(
            summary.skill_result,
            SkillBuildResult::Failed("skill build exploded".to_string())
        );
        assert_eq!(
            summary.server_result,
            ServerResult::StartedFresh { pid: 51_515 }
        );
    }

    #[test]
    fn no_pull_skips_git_commands() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let system = MockUpdateSystem::new();
        let args = UpdateArgs {
            branch: None,
            no_pull: true,
            no_skills: true,
            no_restart: true,
            force: false,
        };

        execute_update(&system, &config, &args).expect("update should pass");

        let actions = system.actions.borrow();
        assert!(!actions.iter().any(|action| action == "git-fetch"));
        assert!(!actions.iter().any(|action| action.starts_with("git-pull:")));
        assert!(!actions
            .iter()
            .any(|action| action.starts_with("git-checkout:")));
    }

    #[test]
    fn no_skills_forwards_skip_flag_to_builds() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let system = MockUpdateSystem::new();
        let args = UpdateArgs {
            branch: None,
            no_pull: true,
            no_skills: true,
            no_restart: true,
            force: false,
        };

        execute_update(&system, &config, &args).expect("update should pass");

        assert_eq!(*system.build_skips.borrow(), vec![true]);
    }

    #[test]
    fn no_restart_skips_restart_flow() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let system = MockUpdateSystem::new();
        let args = UpdateArgs {
            branch: None,
            no_pull: true,
            no_skills: true,
            no_restart: true,
            force: false,
        };

        let summary = execute_update(&system, &config, &args).expect("update should pass");

        assert_eq!(summary.server_result, ServerResult::Skipped);
        let actions = system.actions.borrow();
        assert!(!actions.iter().any(|action| action.starts_with("signal:")));
        assert!(!actions.iter().any(|action| action.starts_with("spawn:")));
        assert!(!actions.iter().any(|action| action == "verify-ready"));
    }

    #[test]
    fn update_errors_use_spec_exit_codes() {
        assert_eq!(UpdateError::Preflight(anyhow!("preflight")).exit_code(), 1);
        assert_eq!(UpdateError::Git(anyhow!("git")).exit_code(), 2);
        assert_eq!(UpdateError::Build(anyhow!("build")).exit_code(), 3);
        assert_eq!(UpdateError::Restart(anyhow!("restart")).exit_code(), 4);
    }

    #[test]
    fn http_port_timeout_message_describes_server_not_ready() {
        let message = http_port_timeout_message();

        assert!(message.contains("Server not ready"));
        assert!(message.contains("not responding"));
        assert!(message.contains("fawx logs"));
        assert!(!message.contains("still in use"));
    }

    #[test]
    fn checks_out_target_branch_when_it_differs() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_update_config(&temp_dir);
        let mut system = MockUpdateSystem::new();
        system.current_branch = "main".to_string();
        let args = UpdateArgs {
            branch: Some("dev".to_string()),
            no_pull: false,
            no_skills: true,
            no_restart: true,
            force: false,
        };

        execute_update(&system, &config, &args).expect("update should pass");

        assert!(system
            .actions
            .borrow()
            .iter()
            .any(|action| action == "git-checkout:dev"));
    }
}
