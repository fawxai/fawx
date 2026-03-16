use crate::context::Experiment;
use crate::error::DecomposeError;
use crate::{DecompositionPlan, SubGoalOutcome, SubGoalResult};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;

/// Aggregated result from executing a decomposition plan.
#[derive(Debug, Clone)]
pub struct AggregatedResult {
    pub combined_patch: String,
    pub approach: String,
    pub sub_goal_outcomes: Vec<(String, SubGoalOutcome)>,
    pub completion_rate: f64,
}

#[async_trait::async_trait]
pub trait ResultAggregator: Send + Sync {
    async fn aggregate(
        &self,
        plan: &DecompositionPlan,
        results: &[SubGoalResult],
        experiment: &Experiment,
    ) -> Result<AggregatedResult, DecomposeError>;
}

/// Simple aggregator that concatenates patches from completed sub-goals.
pub struct SimpleAggregator;

#[async_trait::async_trait]
impl ResultAggregator for SimpleAggregator {
    async fn aggregate(
        &self,
        _plan: &DecompositionPlan,
        results: &[SubGoalResult],
        _experiment: &Experiment,
    ) -> Result<AggregatedResult, DecomposeError> {
        let mut patches = Vec::new();
        let mut approaches = Vec::new();
        let mut outcomes = Vec::new();
        let mut completed = 0;

        for result in results {
            let description = result.goal.description.clone();
            if let SubGoalOutcome::Completed(patch) = &result.outcome {
                patches.push(patch.clone());
                approaches.push(format!("{description}: completed"));
                completed += 1;
            }
            outcomes.push((description, result.outcome.clone()));
        }

        let total = results.len();
        let completion_rate = if total == 0 {
            0.0
        } else {
            completed as f64 / total as f64
        };

        Ok(AggregatedResult {
            combined_patch: patches.join("\n"),
            approach: approaches.join("; "),
            sub_goal_outcomes: outcomes,
            completion_rate,
        })
    }
}

/// Result of merging sub-goal patches in a workspace.
#[derive(Debug, Clone)]
pub struct MergeResult {
    /// Combined patch of all successfully applied sub-goals.
    pub combined_patch: String,
    /// Indices of sub-goals successfully merged.
    pub merged: Vec<usize>,
    /// Indices of sub-goals that conflicted, with reason.
    pub conflicts: Vec<(usize, String)>,
    /// Whether cargo check passed on the merged result.
    pub build_ok: bool,
    /// Test results if build passed.
    pub test_result: Option<MergeTestResult>,
}

#[derive(Debug, Clone)]
pub struct MergeTestResult {
    pub passed: usize,
    pub failed: usize,
    pub total: usize,
}

/// Provides a temporary git workspace for merge verification.
#[async_trait::async_trait]
pub trait WorkspaceProvider: Send + Sync {
    async fn create(&self) -> Result<TempWorkspace, DecomposeError>;
}

/// A temporary git workspace for applying and verifying patches.
pub struct TempWorkspace {
    pub path: PathBuf,
    _temp_dir: TempDir,
}

impl TempWorkspace {
    /// Create an empty temp workspace with an initialized git repo.
    pub async fn empty() -> Result<Self, DecomposeError> {
        let temp_dir = TempDir::new()
            .map_err(|e| DecomposeError::AggregationFailed(format!("temp dir: {e}")))?;
        let path = temp_dir.path().to_path_buf();
        init_git_repo_async(path.clone()).await?;
        Ok(Self {
            path,
            _temp_dir: temp_dir,
        })
    }

    /// Create a temp workspace by copying an existing directory, then initializing git.
    pub async fn from_path(source: &Path) -> Result<Self, DecomposeError> {
        let temp_dir = TempDir::new()
            .map_err(|e| DecomposeError::AggregationFailed(format!("temp dir: {e}")))?;
        let path = temp_dir.path().to_path_buf();
        let src = source.to_path_buf();
        let dst = path.clone();
        tokio::task::spawn_blocking(move || copy_dir_recursive(&src, &dst))
            .await
            .map_err(|e| DecomposeError::AggregationFailed(format!("spawn_blocking: {e}")))??;
        init_git_repo_async(path.clone()).await?;
        Ok(Self {
            path,
            _temp_dir: temp_dir,
        })
    }

    /// Apply a unified diff patch to the workspace.
    pub async fn apply_patch(&self, patch: &str) -> Result<(), PatchApplyError> {
        if patch.trim().is_empty() {
            return Ok(());
        }

        let patch_file = self.path.join(".patch");
        tokio::fs::write(&patch_file, patch)
            .await
            .map_err(|e| PatchApplyError::IoError(e.to_string()))?;
        let output = tokio::process::Command::new("git")
            .args(["apply", "--3way", ".patch"])
            .current_dir(&self.path)
            .output()
            .await
            .map_err(|e| PatchApplyError::IoError(e.to_string()))?;
        let _ = tokio::fs::remove_file(&patch_file).await;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(PatchApplyError::Conflict {
                detail: stderr.to_string(),
            })
        }
    }

    /// Run cargo check with timeout.
    pub async fn cargo_check(&self, timeout: Duration) -> Result<bool, DecomposeError> {
        let result = tokio::time::timeout(timeout, self.run_cargo_check()).await;
        match result {
            Ok(Ok(output)) => Ok(output.status.success()),
            Ok(Err(e)) => Err(DecomposeError::AggregationFailed(format!(
                "cargo check: {e}"
            ))),
            Err(_) => Err(DecomposeError::AggregationFailed(format!(
                "cargo check timed out ({timeout:?})"
            ))),
        }
    }

    async fn run_cargo_check(&self) -> Result<std::process::Output, std::io::Error> {
        tokio::process::Command::new("cargo")
            .arg("check")
            .current_dir(&self.path)
            .output()
            .await
    }

    /// Get combined diff from initial commit.
    pub async fn combined_diff(&self) -> Result<String, DecomposeError> {
        let output = tokio::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(&self.path)
            .output()
            .await
            .map_err(|e| DecomposeError::AggregationFailed(format!("git diff: {e}")))?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Reset workspace to initial state.
    pub async fn reset(&self) -> Result<(), DecomposeError> {
        let output = tokio::process::Command::new("git")
            .args(["checkout", "."])
            .current_dir(&self.path)
            .output()
            .await
            .map_err(|e| DecomposeError::AggregationFailed(format!("git reset: {e}")))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(DecomposeError::AggregationFailed(
                "git checkout . failed".to_owned(),
            ))
        }
    }
}

#[derive(Debug)]
pub enum PatchApplyError {
    Conflict { detail: String },
    IoError(String),
}

impl std::fmt::Display for PatchApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict { detail } => write!(f, "patch conflict: {detail}"),
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl std::error::Error for PatchApplyError {}

async fn init_git_repo_async(path: PathBuf) -> Result<(), DecomposeError> {
    tokio::task::spawn_blocking(move || init_git_repo_sync(&path))
        .await
        .map_err(|e| DecomposeError::AggregationFailed(format!("spawn_blocking: {e}")))?
}

fn init_git_repo_sync(path: &Path) -> Result<(), DecomposeError> {
    let run = |args: &[&str]| -> Result<(), DecomposeError> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .map_err(|e| DecomposeError::AggregationFailed(format!("git {}: {e}", args[0])))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(DecomposeError::AggregationFailed(format!(
                "git {} failed: {stderr}",
                args[0]
            )))
        }
    };

    run(&["init"])?;
    run(&["config", "user.email", "fawx@localhost"])?;
    run(&["config", "user.name", "Fawx"])?;
    std::fs::write(path.join(".gitkeep"), "")
        .map_err(|e| DecomposeError::AggregationFailed(format!("write .gitkeep: {e}")))?;
    run(&["add", "."])?;
    run(&["commit", "-m", "initial"])?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), DecomposeError> {
    if !src.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(src)
        .map_err(|e| DecomposeError::AggregationFailed(format!("read dir: {e}")))?
    {
        let entry =
            entry.map_err(|e| DecomposeError::AggregationFailed(format!("dir entry: {e}")))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)
                .map_err(|e| DecomposeError::AggregationFailed(format!("mkdir: {e}")))?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| DecomposeError::AggregationFailed(format!("copy: {e}")))?;
        }
    }

    Ok(())
}

pub async fn from_path(source: &Path) -> Result<TempWorkspace, DecomposeError> {
    TempWorkspace::from_path(source).await
}

/// Aggregator that applies patches to a workspace, verifies build, and handles conflicts.
pub struct BuildVerifyAggregator {
    workspace_provider: Box<dyn WorkspaceProvider>,
    build_timeout: Duration,
}

impl BuildVerifyAggregator {
    pub fn new(workspace_provider: Box<dyn WorkspaceProvider>, build_timeout: Duration) -> Self {
        Self {
            workspace_provider,
            build_timeout,
        }
    }

    async fn merge_patches(
        &self,
        workspace: &TempWorkspace,
        results: &[SubGoalResult],
    ) -> Result<MergeResult, DecomposeError> {
        let mut merged = Vec::new();
        let mut conflicts = Vec::new();

        for (index, result) in results.iter().enumerate() {
            if let SubGoalOutcome::Completed(patch) = &result.outcome {
                match workspace.apply_patch(patch).await {
                    Ok(()) => merged.push(index),
                    Err(e) => conflicts.push((index, e.to_string())),
                }
            }
        }

        let combined_patch = workspace.combined_diff().await?;
        let build_ok = if !merged.is_empty() {
            workspace.cargo_check(self.build_timeout).await?
        } else {
            true
        };

        Ok(MergeResult {
            combined_patch,
            merged,
            conflicts,
            build_ok,
            test_result: None,
        })
    }
}

#[async_trait::async_trait]
impl ResultAggregator for BuildVerifyAggregator {
    async fn aggregate(
        &self,
        _plan: &DecompositionPlan,
        results: &[SubGoalResult],
        _experiment: &Experiment,
    ) -> Result<AggregatedResult, DecomposeError> {
        let workspace = self.workspace_provider.create().await?;
        let merge = self.merge_patches(&workspace, results).await?;
        let total = results.len().max(1);
        let rate = merge.merged.len() as f64 / total as f64;
        let outcomes = results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                let description = result.goal.description.clone();
                if merge
                    .conflicts
                    .iter()
                    .any(|(conflict_index, _)| *conflict_index == index)
                {
                    (
                        description,
                        SubGoalOutcome::Failed("merge conflict".to_owned()),
                    )
                } else {
                    (description, result.outcome.clone())
                }
            })
            .collect();

        Ok(AggregatedResult {
            combined_patch: merge.combined_patch,
            approach: format!(
                "Merged {}/{} sub-goals, build={}",
                merge.merged.len(),
                results.len(),
                if merge.build_ok { "pass" } else { "fail" }
            ),
            sub_goal_outcomes: outcomes,
            completion_rate: rate,
        })
    }
}

/// Simple workspace provider that creates a fresh git repo.
pub struct DefaultWorkspaceProvider;

#[async_trait::async_trait]
impl WorkspaceProvider for DefaultWorkspaceProvider {
    async fn create(&self) -> Result<TempWorkspace, DecomposeError> {
        TempWorkspace::empty().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComplexityHint, SubGoal};

    fn sample_experiment() -> Experiment {
        Experiment {
            hypothesis: "test".to_owned(),
        }
    }

    fn goal(description: &str) -> SubGoal {
        SubGoal {
            description: description.to_owned(),
            required_tools: vec![],
            expected_output: None,
            complexity_hint: Some(ComplexityHint::Trivial),
        }
    }

    fn completed(description: &str, patch: &str) -> SubGoalResult {
        SubGoalResult {
            goal: goal(description),
            outcome: SubGoalOutcome::Completed(patch.to_owned()),
            signals: Vec::new(),
        }
    }

    fn failed_result(description: &str) -> SubGoalResult {
        SubGoalResult {
            goal: goal(description),
            outcome: SubGoalOutcome::Failed("error".to_owned()),
            signals: Vec::new(),
        }
    }

    #[tokio::test]
    async fn aggregates_all_completed() {
        let plan = DecompositionPlan {
            sub_goals: vec![goal("A"), goal("B")],
            strategy: crate::AggregationStrategy::Sequential,
            truncated_from: None,
        };
        let results = vec![completed("A", "diff-a"), completed("B", "diff-b")];
        let aggregator = SimpleAggregator;
        let result = aggregator
            .aggregate(&plan, &results, &sample_experiment())
            .await
            .unwrap();

        assert_eq!(result.completion_rate, 1.0);
        assert!(result.combined_patch.contains("diff-a"));
        assert!(result.combined_patch.contains("diff-b"));
    }

    #[tokio::test]
    async fn partial_failure_reduces_rate() {
        let plan = DecompositionPlan {
            sub_goals: vec![goal("A"), goal("B")],
            strategy: crate::AggregationStrategy::Sequential,
            truncated_from: None,
        };
        let results = vec![completed("A", "diff-a"), failed_result("B")];
        let aggregator = SimpleAggregator;
        let result = aggregator
            .aggregate(&plan, &results, &sample_experiment())
            .await
            .unwrap();

        assert_eq!(result.completion_rate, 0.5);
        assert!(result.combined_patch.contains("diff-a"));
        assert!(!result.combined_patch.contains("diff-b"));
    }

    #[tokio::test]
    async fn empty_results() {
        let plan = DecompositionPlan {
            sub_goals: vec![],
            strategy: crate::AggregationStrategy::Sequential,
            truncated_from: None,
        };
        let aggregator = SimpleAggregator;
        let result = aggregator
            .aggregate(&plan, &[], &sample_experiment())
            .await
            .unwrap();

        assert_eq!(result.completion_rate, 0.0);
        assert!(result.combined_patch.is_empty());
    }

    #[tokio::test]
    async fn build_verify_aggregator_merges_clean_patches() {
        let provider = Box::new(DefaultWorkspaceProvider);
        let aggregator = BuildVerifyAggregator::new(provider, Duration::from_secs(30));
        let plan = DecompositionPlan {
            sub_goals: vec![goal("A"), goal("B")],
            strategy: crate::AggregationStrategy::Sequential,
            truncated_from: None,
        };
        let results = vec![completed("A", ""), completed("B", "")];
        let result = aggregator
            .aggregate(&plan, &results, &sample_experiment())
            .await
            .unwrap();
        assert_eq!(result.completion_rate, 1.0);
    }

    #[tokio::test]
    async fn temp_workspace_initializes_git_repo() {
        let workspace = TempWorkspace::empty().await.unwrap();
        let output = tokio::process::Command::new("git")
            .args(["status"])
            .current_dir(&workspace.path)
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
    }

    #[tokio::test]
    async fn temp_workspace_applies_empty_patch() {
        let workspace = TempWorkspace::empty().await.unwrap();
        workspace.apply_patch("").await.unwrap();
    }

    #[tokio::test]
    async fn temp_workspace_from_path_copies_source_files() {
        let source = tempfile::TempDir::new().unwrap();
        tokio::fs::create_dir(source.path().join("src"))
            .await
            .unwrap();
        tokio::fs::write(
            source.path().join("src/lib.rs"),
            "pub fn value() -> u32 { 1 }\n",
        )
        .await
        .unwrap();

        let workspace = TempWorkspace::from_path(source.path()).await.unwrap();
        let copied = tokio::fs::read_to_string(workspace.path.join("src/lib.rs"))
            .await
            .unwrap();

        assert_eq!(copied, "pub fn value() -> u32 { 1 }\n");

        let output = tokio::process::Command::new("git")
            .args(["status"])
            .current_dir(&workspace.path)
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
    }

    #[tokio::test]
    async fn temp_workspace_combined_diff_empty_on_clean() {
        let workspace = TempWorkspace::empty().await.unwrap();
        let diff = workspace.combined_diff().await.unwrap();
        assert!(diff.trim().is_empty());
    }
}
