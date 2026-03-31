//! Executes fix plans by writing proposals and recording fingerprints.

use crate::config::{ImprovementConfig, OutputMode};
use crate::detector::ImprovementDetector;
use crate::error::ImprovementError;
use crate::planner::FixPlan;
use fx_propose::{current_file_hash, Proposal, ProposalWriter};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Result of executing improvement plans.
#[derive(Debug)]
pub struct ExecutionResult {
    /// Proposal files written to disk.
    pub proposals_written: Vec<PathBuf>,
    /// Branches created in ProposalWithBranch mode.
    pub branches_created: Vec<String>,
    /// Candidates skipped during execution, with reasons.
    pub skipped: Vec<(String, String)>,
}

impl ExecutionResult {
    /// Create an empty result (no actions taken).
    pub fn empty() -> Self {
        Self {
            proposals_written: Vec::new(),
            branches_created: Vec::new(),
            skipped: Vec::new(),
        }
    }
}

struct MaterializedChange {
    path: PathBuf,
    content: String,
}

/// Turns fix plans into proposals based on the configured output mode.
pub struct ImprovementExecutor {
    config: ImprovementConfig,
    proposal_writer: ProposalWriter,
    repo_root: PathBuf,
}

impl ImprovementExecutor {
    pub fn new(config: ImprovementConfig, proposals_dir: PathBuf, repo_root: PathBuf) -> Self {
        Self {
            config,
            proposal_writer: ProposalWriter::new(proposals_dir),
            repo_root,
        }
    }

    /// Execute fix plans according to the configured output mode.
    pub fn execute(
        &self,
        plans: &[FixPlan],
        detector: &mut ImprovementDetector,
    ) -> Result<ExecutionResult, ImprovementError> {
        self.config.validate()?;
        let mut result = ExecutionResult::empty();

        for plan in plans {
            match self.config.output_mode {
                OutputMode::DryRun => {
                    result
                        .skipped
                        .push((plan.candidate.fingerprint.clone(), "dry run".to_string()));
                }
                OutputMode::ProposalOnly => {
                    let path = self.write_proposal(plan, None)?;
                    result.proposals_written.push(path);
                    detector.record_acted(&plan.candidate.fingerprint)?;
                }
                OutputMode::ProposalWithBranch => {
                    self.execute_proposal_with_branch(plan, detector, &mut result)?;
                }
            }
        }

        Ok(result)
    }

    fn execute_proposal_with_branch(
        &self,
        plan: &FixPlan,
        detector: &mut ImprovementDetector,
        result: &mut ExecutionResult,
    ) -> Result<(), ImprovementError> {
        let branch_name = self.create_branch_checkpoint(plan)?;
        let path = self.write_proposal(plan, branch_name.as_deref())?;
        result.proposals_written.push(path);

        if let Some(branch_name) = branch_name {
            result.branches_created.push(branch_name);
        } else {
            result.skipped.push((
                plan.candidate.fingerprint.clone(),
                "branch mode requires concrete code changes; wrote proposal only".to_string(),
            ));
        }

        detector.record_acted(&plan.candidate.fingerprint)?;
        Ok(())
    }

    fn write_proposal(
        &self,
        plan: &FixPlan,
        branch_name: Option<&str>,
    ) -> Result<PathBuf, ImprovementError> {
        let proposal = build_proposal(plan, branch_name, &self.repo_root)?;
        Ok(self.proposal_writer.write(&proposal)?)
    }

    fn create_branch_checkpoint(&self, plan: &FixPlan) -> Result<Option<String>, ImprovementError> {
        let Some(changes) = collect_materialized_changes(plan)? else {
            return Ok(None);
        };

        let branch_name = branch_name_for_plan(plan);
        let original_branch = git_current_branch(&self.repo_root)?;
        git_checkout_new_branch(&self.repo_root, &branch_name)?;

        let apply_result = apply_changes_and_commit(&self.repo_root, plan, &changes);
        let restore_result = git_checkout_branch(&self.repo_root, &original_branch);

        if let Err(error) = restore_result {
            return Err(ImprovementError::Git(format!(
                "failed to restore branch '{original_branch}' after creating '{branch_name}': {error}"
            )));
        }

        apply_result?;
        Ok(Some(branch_name))
    }
}

fn collect_materialized_changes(
    plan: &FixPlan,
) -> Result<Option<Vec<MaterializedChange>>, ImprovementError> {
    let Some(code_changes) = plan.code_changes.as_ref() else {
        return Ok(None);
    };
    if code_changes.is_empty() {
        return Ok(None);
    }

    let mut changes = Vec::with_capacity(code_changes.len());
    for change in code_changes {
        validate_change_path(&change.path)?;
        let Some(content) = change.content.clone() else {
            return Ok(None);
        };
        changes.push(MaterializedChange {
            path: change.path.clone(),
            content,
        });
    }
    Ok(Some(changes))
}

fn validate_change_path(path: &Path) -> Result<(), ImprovementError> {
    if path.is_absolute() {
        return Err(ImprovementError::Git(format!(
            "absolute paths are not allowed in branch changes: {}",
            path.display()
        )));
    }

    let escapes_repo = path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if escapes_repo {
        return Err(ImprovementError::Git(format!(
            "path escapes repository root: {}",
            path.display()
        )));
    }
    Ok(())
}

fn apply_changes_and_commit(
    repo_root: &Path,
    plan: &FixPlan,
    changes: &[MaterializedChange],
) -> Result<(), ImprovementError> {
    write_materialized_changes(repo_root, changes)?;
    git_add_paths(repo_root, changes)?;
    let message = format!(
        "chore(improve): apply plan for {}",
        plan.candidate.finding.pattern_name
    );
    git_commit(repo_root, &message)
}

fn write_materialized_changes(
    repo_root: &Path,
    changes: &[MaterializedChange],
) -> Result<(), ImprovementError> {
    for change in changes {
        let full_path = repo_root.join(&change.path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(full_path, &change.content)?;
    }
    Ok(())
}

fn git_current_branch(repo_root: &Path) -> Result<String, ImprovementError> {
    run_git(
        repo_root,
        ["rev-parse", "--abbrev-ref", "HEAD"],
        "read current branch",
    )
}

fn git_checkout_new_branch(repo_root: &Path, branch_name: &str) -> Result<(), ImprovementError> {
    run_git(
        repo_root,
        ["checkout", "-b", branch_name],
        "create and checkout branch",
    )
    .map(|_| ())
}

fn git_checkout_branch(repo_root: &Path, branch_name: &str) -> Result<(), ImprovementError> {
    run_git(repo_root, ["checkout", branch_name], "checkout branch").map(|_| ())
}

fn git_add_paths(repo_root: &Path, changes: &[MaterializedChange]) -> Result<(), ImprovementError> {
    let mut command = Command::new("git");
    command.arg("add").arg("--");
    for change in changes {
        command.arg(&change.path);
    }
    run_git_command(repo_root, command, "add changes").map(|_| ())
}

fn git_commit(repo_root: &Path, message: &str) -> Result<(), ImprovementError> {
    run_git(repo_root, ["commit", "-m", message], "commit checkpoint").map(|_| ())
}

fn run_git<I, S>(repo_root: &Path, args: I, context: &str) -> Result<String, ImprovementError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut command = Command::new("git");
    command.args(args);
    run_git_command(repo_root, command, context)
}

fn run_git_command(
    repo_root: &Path,
    mut command: Command,
    context: &str,
) -> Result<String, ImprovementError> {
    command.current_dir(repo_root);
    let output = command
        .output()
        .map_err(|error| ImprovementError::Git(format!("git {context}: {error}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(ImprovementError::Git(format!(
            "git {context} failed: {detail}"
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn build_proposal(
    plan: &FixPlan,
    branch_name: Option<&str>,
    repo_root: &Path,
) -> Result<Proposal, ImprovementError> {
    let target_path = plan
        .target_files
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("(unspecified)"));
    let file_hash = proposal_file_hash(repo_root, &target_path)?;

    Ok(Proposal {
        action: "improvement_proposal".to_string(),
        title: format!("Improvement: {}", plan.candidate.finding.pattern_name),
        description: format_proposal_description(plan, branch_name),
        target_path,
        proposed_content: format_proposed_content(plan),
        risk: plan.risk.to_string(),
        timestamp: proposal_timestamp(),
        file_hash,
    })
}

fn proposal_timestamp() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => {
            tracing::warn!("system clock before UNIX epoch, using timestamp 0");
            0
        }
    }
}

fn proposal_file_hash(
    repo_root: &Path,
    target_path: &Path,
) -> Result<Option<String>, ImprovementError> {
    if target_path == Path::new("(unspecified)") {
        return Ok(None);
    }
    current_file_hash(repo_root, target_path).map_err(ImprovementError::Io)
}

fn format_proposal_description(plan: &FixPlan, branch_name: Option<&str>) -> String {
    match branch_name {
        Some(name) => format!("{}\n\nBranch: {name}", plan.fix_description),
        None => plan.fix_description.clone(),
    }
}

fn format_proposed_content(plan: &FixPlan) -> String {
    match &plan.code_changes {
        Some(changes) if !changes.is_empty() => changes
            .iter()
            .map(|change| {
                let content = change
                    .content
                    .as_deref()
                    .unwrap_or("(manual implementation required)");
                format!(
                    "## {}\n{}\n{content}",
                    change.path.display(),
                    change.description,
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => plan.fix_description.clone(),
    }
}

fn branch_name_for_plan(plan: &FixPlan) -> String {
    let short_fingerprint = plan
        .candidate
        .fingerprint
        .chars()
        .take(12)
        .collect::<String>();
    format!("improve/{short_fingerprint}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ImprovementConfig;
    use crate::detector::{compute_fingerprint, ImprovementCandidate, ImprovementDetector};
    use crate::planner::{FileChange, RiskLevel};
    use fx_analysis::{AnalysisFinding, Confidence, SignalEvidence};
    use fx_core::signals::SignalKind;
    use tempfile::TempDir;

    fn mk_candidate(name: &str) -> ImprovementCandidate {
        let finding = AnalysisFinding {
            pattern_name: name.to_string(),
            description: format!("Description for {name}"),
            confidence: Confidence::High,
            evidence: vec![SignalEvidence {
                session_id: "s1".to_string(),
                signal_kind: SignalKind::Friction,
                message: "test".to_string(),
                timestamp_ms: 1,
            }],
            suggested_action: Some("fix it".to_string()),
        };
        ImprovementCandidate {
            fingerprint: compute_fingerprint(&finding.pattern_name, &finding.description),
            finding,
        }
    }

    fn mk_plan(name: &str) -> FixPlan {
        FixPlan {
            candidate: mk_candidate(name),
            target_files: vec![PathBuf::from("src/main.rs")],
            fix_description: "Fix the thing".to_string(),
            code_changes: None,
            risk: RiskLevel::Low,
        }
    }

    fn mk_plan_with_change(name: &str) -> FixPlan {
        FixPlan {
            candidate: mk_candidate(name),
            target_files: vec![PathBuf::from("src/main.rs")],
            fix_description: "Fix the thing".to_string(),
            code_changes: Some(vec![FileChange {
                path: PathBuf::from("src/main.rs"),
                description: "Update implementation".to_string(),
                content: Some("fn main() { println!(\"improved\"); }\n".to_string()),
            }]),
            risk: RiskLevel::Low,
        }
    }

    fn init_git_repo(repo_root: &Path) -> String {
        run_git_test(repo_root, ["init"]);
        run_git_test(repo_root, ["config", "user.email", "test@example.com"]);
        run_git_test(repo_root, ["config", "user.name", "Test User"]);

        let src_dir = repo_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(
            src_dir.join("main.rs"),
            "fn main() { println!(\"before\"); }\n",
        )
        .unwrap();

        run_git_test(repo_root, ["add", "src/main.rs"]);
        run_git_test(repo_root, ["commit", "-m", "initial"]);
        git_output_test(repo_root, ["rev-parse", "--abbrev-ref", "HEAD"])
    }

    fn run_git_test<I, S>(repo_root: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed");
    }

    fn git_output_test<I, S>(repo_root: &Path, args: I) -> String
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .output()
            .unwrap();
        assert!(output.status.success(), "git output command failed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn dry_run_writes_nothing() {
        let tmp = TempDir::new().unwrap();
        let mut config = ImprovementConfig::default();
        config.output_mode = OutputMode::DryRun;
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let executor = ImprovementExecutor::new(config, tmp.path().join("proposals"), repo_root);

        let mut detector =
            ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();
        let result = executor.execute(&[mk_plan("dry")], &mut detector).unwrap();

        assert!(result.proposals_written.is_empty());
        assert!(result.branches_created.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].1, "dry run");
    }

    #[test]
    fn proposal_only_writes_proposal_no_branch() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();

        let executor = ImprovementExecutor::new(
            ImprovementConfig::default(),
            tmp.path().join("proposals"),
            repo_root,
        );
        let mut detector =
            ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();

        let result = executor
            .execute(&[mk_plan("proposal")], &mut detector)
            .unwrap();

        assert_eq!(result.proposals_written.len(), 1);
        assert!(result.proposals_written[0].exists());
        assert!(result.branches_created.is_empty());
    }

    #[test]
    fn proposal_only_records_target_hash_when_file_exists() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(repo_root.join("src/main.rs"), "fn main() {}\n").unwrap();

        let executor = ImprovementExecutor::new(
            ImprovementConfig::default(),
            tmp.path().join("proposals"),
            repo_root,
        );
        let mut detector =
            ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();

        let result = executor
            .execute(&[mk_plan("proposal-hash")], &mut detector)
            .unwrap();
        let sidecar =
            std::fs::read_to_string(result.proposals_written[0].with_extension("json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&sidecar).unwrap();

        assert_eq!(
            value["file_hash_at_creation"],
            serde_json::Value::String(format!(
                "sha256:{}",
                fx_propose::sha256_hex(b"fn main() {}\n")
            ))
        );
    }

    #[test]
    fn proposal_with_branch_creates_branch_and_references_it() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let original_branch = init_git_repo(&repo_root);

        let mut config = ImprovementConfig::default();
        config.output_mode = OutputMode::ProposalWithBranch;
        let executor =
            ImprovementExecutor::new(config, tmp.path().join("proposals"), repo_root.clone());
        let mut detector =
            ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();

        let result = executor
            .execute(&[mk_plan_with_change("branch")], &mut detector)
            .unwrap();

        assert_eq!(result.proposals_written.len(), 1);
        assert_eq!(result.branches_created.len(), 1);

        let branch_name = &result.branches_created[0];
        let listed = git_output_test(&repo_root, ["branch", "--list", branch_name]);
        assert_eq!(listed.trim(), branch_name);

        let current_branch = git_output_test(&repo_root, ["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(current_branch, original_branch);

        let proposal = std::fs::read_to_string(&result.proposals_written[0]).unwrap();
        assert!(proposal.contains(&format!("Branch: {branch_name}")));
    }

    #[test]
    fn proposal_with_branch_without_code_changes_writes_proposal_only() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        init_git_repo(&repo_root);

        let mut config = ImprovementConfig::default();
        config.output_mode = OutputMode::ProposalWithBranch;
        let executor = ImprovementExecutor::new(config, tmp.path().join("proposals"), repo_root);
        let mut detector =
            ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();

        let result = executor
            .execute(&[mk_plan("branch-no-code")], &mut detector)
            .unwrap();

        assert_eq!(result.proposals_written.len(), 1);
        assert!(result.branches_created.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].1.contains("concrete code changes"));
    }

    #[test]
    fn records_fingerprints_after_execution() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();

        let executor = ImprovementExecutor::new(
            ImprovementConfig::default(),
            tmp.path().join("proposals"),
            repo_root,
        );
        let mut detector =
            ImprovementDetector::new(ImprovementConfig::default(), tmp.path()).unwrap();
        let plan = mk_plan("recorded");
        let fingerprint = plan.candidate.fingerprint.clone();
        executor.execute(&[plan], &mut detector).unwrap();

        let history = tmp.path().join("improvements").join("history.jsonl");
        let content = std::fs::read_to_string(&history).unwrap();
        assert!(content.contains(&fingerprint));
    }
}
