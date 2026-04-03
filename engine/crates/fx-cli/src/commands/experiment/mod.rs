use anyhow::{anyhow, bail, Context};
use clap::Subcommand;
use fx_auth::auth::AuthManager;
use fx_config::FawxConfig;
use fx_consensus::{
    format_auto_chain_result, load_chain_history_for_signal, CargoWorkspace, Chain, ChainStorage,
    ConsensusError, EvaluationWorkspace, ExperimentConfig, ExperimentRunner, FitnessCriterion,
    JsonFileChainStorage, LlmPatchSource, MetricType, ModificationScope, NeutralEvaluatorConfig,
    PathPattern, ProgressCallback, ProposalTier, RemoteEvalTarget, RemoteEvaluationWorkspace,
    RoundNodes, RoundNodesBuilder, Signal, SubagentPatchSource,
};
use fx_llm::ModelRouter;
use fx_subagent::{SubagentControl, SubagentLimits, SubagentManager, SubagentManagerDeps};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

mod format;
mod placeholders;

use format::{
    format_chain_entries, format_chain_entry, format_chain_entry_detail, format_experiment_report,
    render_progress_event,
};
use placeholders::build_nodes;

const CHAIN_PATH_ENV: &str = "FAWX_CONSENSUS_CHAIN_PATH";

#[derive(Debug, Subcommand)]
pub enum ExperimentCommands {
    /// Create and run a new experiment
    Run {
        /// Signal name that triggered this experiment
        #[arg(long)]
        signal: String,

        /// Hypothesis to test
        #[arg(long)]
        hypothesis: String,

        /// Number of competing nodes (default: 3)
        #[arg(long, default_value = "3")]
        nodes: u32,

        /// Files allowed to be modified (glob patterns, comma-separated)
        #[arg(long, default_value = "src/**/*.rs")]
        scope: String,

        /// Timeout per node in seconds (default: 120)
        #[arg(long, default_value = "120")]
        timeout: u64,

        /// Experiment node mode: placeholder, direct, or subagent
        #[arg(long, default_value = "placeholder")]
        mode: ExperimentNodeMode,

        /// Cargo workspace project directory for direct/subagent evaluation
        #[arg(long)]
        project: Option<String>,

        /// Remote SSH evaluation target in user@host:/path format
        #[arg(long)]
        eval_node: Option<String>,

        /// Run node generation and evaluation one at a time
        #[arg(long)]
        sequential: bool,

        /// Maximum rounds for auto-chain loop (default: 1, preserves single-shot behavior)
        #[arg(
            long,
            default_value_t = 1,
            value_parser = clap::value_parser!(u32).range(1..)
        )]
        max_rounds: u32,

        /// Stream experiment progress to stderr
        #[arg(long)]
        verbose: bool,
    },

    /// View the consensus chain
    Chain {
        /// Number of recent entries to show (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Show details of a specific chain entry
    Show {
        /// Chain entry index
        index: u64,
        /// Show detailed evaluation breakdown
        #[arg(long)]
        detail: bool,
        /// Show raw JSON
        #[arg(long)]
        raw: bool,
    },

    /// Verify chain integrity
    Verify,
}

pub async fn run(command: ExperimentCommands) -> anyhow::Result<String> {
    match command {
        ExperimentCommands::Run {
            signal,
            hypothesis,
            nodes,
            scope,
            timeout,
            mode,
            project,
            eval_node,
            sequential,
            max_rounds,
            verbose,
        } => {
            run_experiment(
                RunExperimentArgs {
                    signal,
                    hypothesis,
                    nodes,
                    scope,
                    timeout,
                    mode,
                    project,
                    eval_node,
                    sequential,
                    verbose,
                },
                max_rounds,
            )
            .await
        }
        ExperimentCommands::Chain { limit } => show_chain(limit),
        ExperimentCommands::Show { index, detail, raw } => show_entry(index, detail, raw),
        ExperimentCommands::Verify => verify_chain(),
    }
}

#[derive(Clone, Copy, Debug, clap::ValueEnum, PartialEq, Eq)]
pub enum ExperimentNodeMode {
    Placeholder,
    Direct,
    Subagent,
}

pub struct RunExperimentArgs {
    pub signal: String,
    pub hypothesis: String,
    pub nodes: u32,
    pub scope: String,
    pub timeout: u64,
    pub mode: ExperimentNodeMode,
    pub project: Option<String>,
    pub eval_node: Option<String>,
    pub sequential: bool,
    pub verbose: bool,
}

pub async fn run_experiment(args: RunExperimentArgs, max_rounds: u32) -> anyhow::Result<String> {
    run_experiment_with_path(args, consensus_chain_path(), max_rounds).await
}

async fn run_experiment_with_path(
    args: RunExperimentArgs,
    chain_path: PathBuf,
    max_rounds: u32,
) -> anyhow::Result<String> {
    ensure_chain_parent_dir(&chain_path)?;
    let runner = build_runner(&args, chain_path)?.with_progress(build_progress_callback(&args));
    let chain_result = runner.run_loop(build_config(&args)?, max_rounds).await?;
    Ok(format_auto_chain_result(&chain_result, |report| {
        format_experiment_report(&args, report)
    }))
}

fn build_progress_callback(args: &RunExperimentArgs) -> Option<ProgressCallback> {
    if !args.verbose {
        return None;
    }
    Some(Arc::new(render_progress_event))
}

fn build_runner(args: &RunExperimentArgs, chain_path: PathBuf) -> anyhow::Result<ExperimentRunner> {
    match args.mode {
        ExperimentNodeMode::Placeholder => build_placeholder_runner(args, chain_path),
        ExperimentNodeMode::Direct => build_direct_runner(args, chain_path),
        ExperimentNodeMode::Subagent => build_subagent_runner(args, chain_path),
    }
}

fn build_placeholder_runner(
    args: &RunExperimentArgs,
    chain_path: PathBuf,
) -> anyhow::Result<ExperimentRunner> {
    let neutral_evaluator = build_neutral_evaluator_from_args(args)?;
    ExperimentRunner::with_nodes(chain_path, build_nodes(args.nodes), neutral_evaluator)
        .map_err(anyhow::Error::from)
}

fn build_direct_runner(
    args: &RunExperimentArgs,
    chain_path: PathBuf,
) -> anyhow::Result<ExperimentRunner> {
    let auth_manager = crate::startup::load_auth_manager()?;
    let config = crate::startup::load_config()?;
    let (router, model) = build_active_router(&auth_manager, &config)?;
    let project_dir = resolve_project_dir(args)?;
    let builder = DirectRoundNodesBuilder {
        count: args.nodes,
        scope: args.scope.clone(),
        router,
        model,
        project_dir,
        eval_target: parse_eval_target(args)?,
    };
    ExperimentRunner::with_round_nodes_builder(chain_path, builder).map_err(anyhow::Error::from)
}

fn build_subagent_runner(
    args: &RunExperimentArgs,
    chain_path: PathBuf,
) -> anyhow::Result<ExperimentRunner> {
    let auth_manager = crate::startup::load_auth_manager()?;
    let config = crate::startup::load_config()?;
    let improvement_provider = crate::startup::build_improvement_provider(&auth_manager, &config);
    let subagent_router = crate::startup::build_router(&auth_manager)?;
    let locked_router = Arc::new(std::sync::RwLock::new(subagent_router));
    let factory = crate::headless::HeadlessSubagentFactory::new(
        crate::headless::HeadlessSubagentFactoryDeps {
            router: locked_router,
            config: config.clone(),
            improvement_provider,
            session_bus: None,
            credential_store: None,
            token_broker: None,
        },
    );
    let builder = SubagentRoundNodesBuilder {
        count: args.nodes,
        scope: args.scope.clone(),
        manager: Arc::new(SubagentManager::new(SubagentManagerDeps {
            factory: Arc::new(factory),
            limits: SubagentLimits::default(),
        })),
        project_dir: resolve_project_dir(args)?,
        eval_target: parse_eval_target(args)?,
    };
    ExperimentRunner::with_round_nodes_builder(chain_path, builder).map_err(anyhow::Error::from)
}

fn build_neutral_evaluator_from_args(
    args: &RunExperimentArgs,
) -> anyhow::Result<Option<NeutralEvaluatorConfig>> {
    if args.nodes != 1 {
        return Ok(None);
    }
    match args.mode {
        ExperimentNodeMode::Placeholder => Ok(Some(placeholders::build_neutral_evaluator())),
        ExperimentNodeMode::Direct | ExperimentNodeMode::Subagent => {
            let project_dir = resolve_project_dir(args)?;
            let eval_target = parse_eval_target(args)?;
            build_round_neutral_evaluator(
                args.nodes,
                &project_dir,
                &args.scope,
                eval_target.as_ref(),
            )
            .map_err(anyhow::Error::from)
        }
    }
}

struct DirectRoundNodesBuilder {
    count: u32,
    scope: String,
    router: Arc<ModelRouter>,
    model: String,
    project_dir: PathBuf,
    eval_target: Option<RemoteEvalTarget>,
}

impl RoundNodesBuilder for DirectRoundNodesBuilder {
    fn build_round_nodes(
        &self,
        chain_path: &Path,
        signal: &str,
    ) -> Result<RoundNodes, ConsensusError> {
        let chain_history = load_round_chain_history(chain_path, signal)?;
        let nodes = build_direct_nodes(DirectNodesBuildConfig {
            count: self.count,
            scope: self.scope.clone(),
            router: Arc::clone(&self.router),
            model: self.model.clone(),
            project_dir: self.project_dir.clone(),
            eval_target: self.eval_target.clone(),
            chain_history,
        })
        .map_err(protocol_error)?;
        Ok(RoundNodes {
            nodes,
            neutral_evaluator: build_round_neutral_evaluator(
                self.count,
                &self.project_dir,
                &self.scope,
                self.eval_target.as_ref(),
            )?,
        })
    }
}

struct SubagentRoundNodesBuilder {
    count: u32,
    scope: String,
    manager: Arc<SubagentManager>,
    project_dir: PathBuf,
    eval_target: Option<RemoteEvalTarget>,
}

impl RoundNodesBuilder for SubagentRoundNodesBuilder {
    fn build_round_nodes(
        &self,
        chain_path: &Path,
        signal: &str,
    ) -> Result<RoundNodes, ConsensusError> {
        let chain_history = load_round_chain_history(chain_path, signal)?;
        let nodes = build_subagent_nodes(SubagentNodesBuildConfig {
            count: self.count,
            scope: self.scope.clone(),
            manager: Arc::clone(&self.manager),
            project_dir: self.project_dir.clone(),
            eval_target: self.eval_target.clone(),
            chain_history,
        })
        .map_err(protocol_error)?;
        Ok(RoundNodes {
            nodes,
            neutral_evaluator: build_round_neutral_evaluator(
                self.count,
                &self.project_dir,
                &self.scope,
                self.eval_target.as_ref(),
            )?,
        })
    }
}

fn load_round_chain_history(chain_path: &Path, signal: &str) -> Result<String, ConsensusError> {
    load_chain_history_for_signal(chain_path, signal)
}

fn build_round_neutral_evaluator(
    count: u32,
    project_dir: &Path,
    scope: &str,
    eval_target: Option<&RemoteEvalTarget>,
) -> Result<Option<NeutralEvaluatorConfig>, ConsensusError> {
    if count != 1 {
        return Ok(None);
    }
    Ok(Some(NeutralEvaluatorConfig {
        node_id: fx_consensus::NodeId("neutral-evaluator".to_owned()),
        workspace: build_evaluation_workspace(
            project_dir,
            "neutral-evaluator",
            scope,
            eval_target,
        )?,
    }))
}

fn parse_eval_target(args: &RunExperimentArgs) -> anyhow::Result<Option<RemoteEvalTarget>> {
    args.eval_node
        .as_deref()
        .map(|target| target.parse().map_err(anyhow::Error::from))
        .transpose()
}

fn build_evaluation_workspace(
    project_dir: &Path,
    label: &str,
    scope: &str,
    eval_target: Option<&RemoteEvalTarget>,
) -> Result<Box<dyn EvaluationWorkspace>, ConsensusError> {
    let package = CargoWorkspace::package_from_scope(scope);
    match eval_target {
        Some(target) => Ok(Box::new(RemoteEvaluationWorkspace::new(
            target.clone(),
            package,
        )?)),
        None => Ok(Box::new(CargoWorkspace::clone_from_with_package(
            project_dir,
            label,
            package,
        )?)),
    }
}

fn build_generator_workspace(
    project_dir: &Path,
    label: &str,
    scope: &str,
) -> Result<CargoWorkspace, ConsensusError> {
    let package = CargoWorkspace::package_from_scope(scope);
    CargoWorkspace::clone_from_with_package(project_dir, label, package)
}

fn protocol_error(error: anyhow::Error) -> ConsensusError {
    ConsensusError::Protocol(error.to_string())
}

fn build_active_router(
    auth_manager: &AuthManager,
    config: &FawxConfig,
) -> anyhow::Result<(Arc<ModelRouter>, String)> {
    let mut router = crate::startup::build_router(auth_manager)?;
    let model = crate::headless::resolve_active_model(&router, config)?;
    router
        .set_active(&model)
        .map_err(|error| anyhow!("failed to activate model '{model}': {error}"))?;
    Ok((Arc::new(router), model))
}

fn resolve_project_dir(args: &RunExperimentArgs) -> anyhow::Result<PathBuf> {
    let project_dir = args
        .project
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    validate_project_dir(&project_dir)
}

fn validate_project_dir(project_dir: &Path) -> anyhow::Result<PathBuf> {
    let canonical = fs::canonicalize(project_dir)
        .with_context(|| format!("failed to access project path {}", project_dir.display()))?;
    if !canonical.exists() {
        bail!("project path does not exist: {}", canonical.display());
    }
    if !canonical.is_dir() {
        bail!("project path is not a directory: {}", canonical.display());
    }
    let manifest = canonical.join("Cargo.toml");
    if !manifest.is_file() {
        bail!("project is missing Cargo.toml: {}", canonical.display());
    }
    verify_git_repo(&canonical)?;
    ensure_clean_git_status(&canonical)?;
    Ok(canonical)
}

fn verify_git_repo(project_dir: &Path) -> anyhow::Result<()> {
    run_git_check(project_dir, &["rev-parse", "--git-dir"]).map(|_| ())
}

fn ensure_clean_git_status(project_dir: &Path) -> anyhow::Result<()> {
    let status = run_git_check(project_dir, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        Ok(())
    } else {
        bail!(
            "refusing to run experiment on a dirty git repository: {}",
            project_dir.display()
        )
    }
}

fn run_git_check(project_dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(project_dir)
        .output()
        .with_context(|| format!("failed to run git in {}", project_dir.display()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        bail!(
            "git {} failed for {}: {}",
            args.join(" "),
            project_dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

struct DirectNodesBuildConfig {
    count: u32,
    scope: String,
    router: Arc<ModelRouter>,
    model: String,
    project_dir: PathBuf,
    eval_target: Option<RemoteEvalTarget>,
    chain_history: String,
}

fn build_direct_nodes(
    config: DirectNodesBuildConfig,
) -> anyhow::Result<Vec<fx_consensus::NodeConfig>> {
    (0..config.count)
        .map(|index| {
            let strategy = placeholders::strategy_for(index);
            let node_id = fx_consensus::NodeId(format!("node-{index}"));
            Ok(fx_consensus::NodeConfig {
                node_id: node_id.clone(),
                strategy,
                patch_source: Box::new(
                    LlmPatchSource::new(Arc::clone(&config.router), config.model.clone())
                        .with_chain_history(config.chain_history.clone()),
                ),
                workspace: build_evaluation_workspace(
                    &config.project_dir,
                    &node_id.0,
                    &config.scope,
                    config.eval_target.as_ref(),
                )
                .map_err(anyhow::Error::from)?,
            })
        })
        .collect()
}

struct SubagentNodesBuildConfig {
    count: u32,
    scope: String,
    manager: Arc<SubagentManager>,
    project_dir: PathBuf,
    eval_target: Option<RemoteEvalTarget>,
    chain_history: String,
}

fn build_subagent_nodes(
    config: SubagentNodesBuildConfig,
) -> anyhow::Result<Vec<fx_consensus::NodeConfig>> {
    (0..config.count)
        .map(|index| {
            let strategy = placeholders::strategy_for(index);
            let node_id = fx_consensus::NodeId(format!("node-{index}"));
            let generator_label = format!("{}-gen", node_id.0);
            let evaluator_label = format!("{}-eval", node_id.0);
            let generator_workspace =
                build_generator_workspace(&config.project_dir, &generator_label, &config.scope)
                    .map_err(anyhow::Error::from)?;
            Ok(fx_consensus::NodeConfig {
                node_id: node_id.clone(),
                strategy: strategy.clone(),
                patch_source: Box::new(
                    SubagentPatchSource::with_workspace(
                        Arc::clone(&config.manager) as Arc<dyn SubagentControl>,
                        strategy,
                        generator_workspace,
                    )
                    .with_chain_history(config.chain_history.clone()),
                ),
                workspace: build_evaluation_workspace(
                    &config.project_dir,
                    &evaluator_label,
                    &config.scope,
                    config.eval_target.as_ref(),
                )
                .map_err(anyhow::Error::from)?,
            })
        })
        .collect()
}

fn build_config(args: &RunExperimentArgs) -> anyhow::Result<ExperimentConfig> {
    Ok(ExperimentConfig {
        signal: build_signal(&args.signal),
        hypothesis: args.hypothesis.clone(),
        fitness_criteria: default_fitness_criteria(),
        scope: build_scope(&args.scope),
        timeout: Duration::from_secs(args.timeout),
        min_candidates: args.nodes,
        sequential: args.sequential,
    })
}

fn build_signal(name: &str) -> Signal {
    Signal {
        id: Uuid::new_v4(),
        name: name.to_owned(),
        description: format!("Experiment triggered by signal '{name}'"),
        severity: fx_consensus::Severity::Medium,
    }
}

fn default_fitness_criteria() -> Vec<FitnessCriterion> {
    vec![
        criterion("build_success", MetricType::Higher, 0.2),
        criterion("test_pass_rate", MetricType::Higher, 0.5),
        criterion("signal_resolution", MetricType::Higher, 0.3),
    ]
}

fn criterion(name: &str, metric_type: MetricType, weight: f64) -> FitnessCriterion {
    FitnessCriterion {
        name: name.to_owned(),
        metric_type,
        weight,
    }
}

fn build_scope(scope: &str) -> ModificationScope {
    ModificationScope {
        allowed_files: parse_scope(scope),
        proposal_tier: ProposalTier::Tier1,
    }
}

fn parse_scope(scope: &str) -> Vec<PathPattern> {
    scope
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathPattern::from)
        .collect()
}

fn show_chain(limit: usize) -> anyhow::Result<String> {
    show_chain_at(consensus_chain_path(), limit)
}

fn show_chain_at(path: PathBuf, limit: usize) -> anyhow::Result<String> {
    let chain = load_chain(path)?;
    if chain.is_empty() {
        return Ok("No experiments recorded yet".to_owned());
    }
    Ok(format_chain_entries(&chain, limit))
}

fn show_entry(index: u64, detail: bool, raw: bool) -> anyhow::Result<String> {
    show_entry_at(consensus_chain_path(), index, detail, raw)
}

fn show_entry_at(path: PathBuf, index: u64, detail: bool, raw: bool) -> anyhow::Result<String> {
    let chain = load_chain(path)?;
    let entry = chain
        .entries()
        .iter()
        .find(|entry| entry.index == index)
        .ok_or_else(|| anyhow!("Chain entry #{index} not found"))?;
    if raw {
        return Ok(serde_json::to_string_pretty(&entry)?);
    }
    if detail {
        return Ok(format_chain_entry_detail(entry));
    }
    Ok(format_chain_entry(entry))
}

fn verify_chain() -> anyhow::Result<String> {
    verify_chain_at(consensus_chain_path())
}

fn verify_chain_at(path: PathBuf) -> anyhow::Result<String> {
    let chain = load_chain(path)?;
    chain.verify().map_err(anyhow::Error::from)?;
    Ok(format!(
        "Chain integrity verified: {} entries, all hashes valid",
        chain.len()
    ))
}

fn load_chain(path: PathBuf) -> anyhow::Result<Chain> {
    let storage = JsonFileChainStorage::new(path);
    storage.load().map_err(anyhow::Error::from)
}

pub(crate) fn consensus_chain_path() -> PathBuf {
    if let Ok(path) = std::env::var(CHAIN_PATH_ENV) {
        return PathBuf::from(path);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".fawx")
        .join("consensus")
        .join("chain.json")
}

fn ensure_chain_parent_dir(path: &Path) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("invalid consensus chain path: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use clap::Parser;
    use fx_consensus::test_fixtures::write_chain_with_signals;
    use fx_consensus::{Decision, ExperimentReport, GenerationStrategy};
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    #[test]
    fn chain_with_empty_storage_reports_no_experiments() {
        let temp = TempDir::new().expect("temp dir");
        let output = show_chain_at(temp.path().join("chain.json"), 10).expect("show chain");
        assert_eq!(output, "No experiments recorded yet");
    }

    #[test]
    fn verify_passes_for_empty_chain() {
        let temp = TempDir::new().expect("temp dir");
        let output = verify_chain_at(temp.path().join("chain.json")).expect("verify chain");
        assert_eq!(
            output,
            "Chain integrity verified: 0 entries, all hashes valid"
        );
    }

    #[tokio::test]
    async fn run_creates_chain_entry_and_verification_passes() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");

        let output = run_experiment_with_path(
            RunExperimentArgs {
                signal: "sequential-tool-calls".to_owned(),
                hypothesis: "Parallelizing tool calls reduces token waste".to_owned(),
                nodes: 3,
                scope: "src/**/*.rs".to_owned(),
                timeout: 120,
                mode: ExperimentNodeMode::Placeholder,
                project: None,
                sequential: false,
                eval_node: None,
                verbose: false,
            },
            chain_path.clone(),
            1,
        )
        .await
        .expect("run experiment");
        let verify = verify_chain_at(chain_path).expect("verify chain");

        assert!(output.contains("═══ Experiment Complete ═══"));
        assert!(output.contains("Chain entry #0 recorded"));
        assert_eq!(
            verify,
            "Chain integrity verified: 1 entries, all hashes valid"
        );
    }

    #[test]
    fn report_formatting_shows_winner_and_decision() {
        let report = ExperimentReport {
            result: fx_consensus::ConsensusResult {
                experiment_id: Uuid::nil(),
                winner: Some(Uuid::from_u128(1)),
                candidates: vec![Uuid::from_u128(1), Uuid::from_u128(2)],
                candidate_nodes: BTreeMap::from([
                    (
                        Uuid::from_u128(1),
                        fx_consensus::NodeId("node-0".to_owned()),
                    ),
                    (
                        Uuid::from_u128(2),
                        fx_consensus::NodeId("node-1".to_owned()),
                    ),
                ]),
                evaluations: vec![],
                aggregate_scores: BTreeMap::from([
                    (Uuid::from_u128(1), 8.73),
                    (Uuid::from_u128(2), 6.21),
                ]),
                decision: Decision::Accept,
                timestamp: Utc::now(),
                candidate_patches: BTreeMap::new(),
            },
            chain_entry_index: 4,
            candidates: vec![
                fx_consensus::CandidateReport {
                    node_id: fx_consensus::NodeId("node-0".to_owned()),
                    strategy: GenerationStrategy::Conservative,
                    approach: "steady".to_owned(),
                    aggregate_score: 8.73,
                    is_winner: true,
                },
                fx_consensus::CandidateReport {
                    node_id: fx_consensus::NodeId("node-1".to_owned()),
                    strategy: GenerationStrategy::Aggressive,
                    approach: "fast".to_owned(),
                    aggregate_score: 6.21,
                    is_winner: false,
                },
            ],
        };
        let args = RunExperimentArgs {
            signal: "sequential-tool-calls".to_owned(),
            hypothesis: "Parallelizing tool calls reduces token waste".to_owned(),
            nodes: 2,
            scope: "src/**/*.rs".to_owned(),
            timeout: 120,
            mode: ExperimentNodeMode::Placeholder,
            project: None,
            sequential: false,
            eval_node: None,
            verbose: false,
        };

        let output = format_experiment_report(&args, &report);

        assert!(output.contains("Experiment ID: 00000000-0000-0000-0000-000000000000"));
        assert!(output.contains("Decision:      ✅ ACCEPT"));
        assert!(output.contains("🏆 node-0 (Conservative)  score: 8.73  ← WINNER"));
        assert!(output.contains("Chain entry #4 recorded"));
    }

    #[test]
    fn show_entry_with_valid_index_formats_human_readable_output() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_sample_chain(&chain_path);

        let output = show_entry_at(chain_path, 0, false, false).expect("show entry");

        assert!(output.contains("Chain entry #0"));
        assert!(output.contains("Decision: ✅ ACCEPT"));
        assert!(output.contains("Winner: node-0"));
        assert!(output.contains("Scores:"));
        assert!(output.contains("  - node-0: 8.73  ← WINNER"));
        assert!(output.contains("Evaluations: 1 total"));
        assert!(!output.contains("aggregate_scores"));
        assert!(!output.contains("evaluations:"));
    }

    #[test]
    fn show_entry_default_is_summary() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_sample_chain(&chain_path);

        let output = show_entry_at(chain_path, 0, false, false).expect("show entry");

        assert!(output.contains("Chain entry #0"));
        assert!(output.contains("Decision: ✅ ACCEPT"));
        assert!(output.contains("Winner: node-0"));
        assert!(output.contains("Scores:"));
        assert!(!output.contains("Fitness scores:"));
        assert!(!output.contains("Scope:"));
    }

    #[test]
    fn show_entry_detail_formats_evaluations() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_sample_chain(&chain_path);

        let output = show_entry_at(chain_path, 0, true, false).expect("show entry detail");

        assert!(output.contains("Chain entry #0"));
        assert!(output.contains("Scope: src/**/*.rs"));
        assert!(output.contains("Timeout: 120s"));
        assert!(output.contains("Candidates:"));
        assert!(output.contains("node-0 (Conservative)"));
        assert!(output.contains("Approach: (not stored in chain entry)"));
        assert!(output.contains("Patch:"));
        assert!(output.contains("diff --git"));
        assert!(output.contains("Evaluations:"));
        assert!(output.contains("[1] Evaluator: node-1"));
        assert!(output.contains("Build: ✅ PASSED"));
        assert!(output.contains("Tests: 0 passed / 0 failed / 0 total"));
        assert!(output.contains("Signal resolved: yes"));
        assert!(output.contains("Regression detected: no"));
        assert!(output.contains("Safety pass: yes"));
        assert!(output.contains("Fitness scores:"));
        assert!(output.contains("build_success: 1.00 (weight: 0.20)"));
        assert!(output.contains("test_pass_rate: 0.00 (weight: 0.50)"));
        assert!(output.contains("signal_resolution: 0.00 (weight: 0.30)"));
        assert!(
            output.contains("Notes: build_ok=true; tests=0/0, failed=0; placeholder evaluation")
        );
        assert!(output.contains("Decision: ✅ ACCEPT"));
        assert!(output.contains("Winner: node-0"));
        assert!(output.contains("Chain hash:"));
    }

    #[test]
    fn show_entry_raw_outputs_json() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_sample_chain(&chain_path);

        let output = show_entry_at(chain_path.clone(), 0, false, true).expect("show raw entry");
        let from_raw: fx_consensus::ChainEntry =
            serde_json::from_str(&output).expect("deserialize chain entry");
        let chain = load_chain(chain_path).expect("load chain");

        assert_eq!(from_raw, chain.entries()[0]);
    }

    #[test]
    fn show_entry_with_invalid_index_returns_error() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_sample_chain(&chain_path);

        let error =
            show_entry_at(chain_path, 99, false, false).expect_err("missing entry should fail");

        assert_eq!(error.to_string(), "Chain entry #99 not found");
    }

    #[test]
    fn show_chain_with_entries_formats_recent_output() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_sample_chain(&chain_path);

        let output = show_chain_at(chain_path, 10).expect("show chain");

        assert!(output.contains("Recent experiments:"));
        assert!(output.contains(
            "#0 | Parallelizing tool calls reduces token waste | ✅ ACCEPT | winner: node-0"
        ));
    }

    #[tokio::test]
    async fn placeholder_mode_skips_corrupt_chain_loading() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        fs::write(&chain_path, "{not-json").expect("invalid chain");

        let output = run_experiment_with_path(
            RunExperimentArgs {
                signal: "latency".to_owned(),
                hypothesis: "placeholder should ignore chain history".to_owned(),
                nodes: 1,
                scope: "src/**/*.rs".to_owned(),
                timeout: 120,
                mode: ExperimentNodeMode::Placeholder,
                project: None,
                sequential: false,
                eval_node: None,
                verbose: false,
            },
            chain_path,
            1,
        )
        .await
        .expect("placeholder run");

        assert!(output.contains("═══ Experiment Complete ═══"));
    }

    #[test]
    fn parse_scope_splits_comma_separated_patterns() {
        assert_eq!(
            parse_scope(" src/**/*.rs, tests/**/*.rs ,docs/*.md "),
            vec![
                PathPattern::from("src/**/*.rs"),
                PathPattern::from("tests/**/*.rs"),
                PathPattern::from("docs/*.md"),
            ]
        );
    }

    #[test]
    fn validate_project_dir_requires_git_repo() {
        let temp = TempDir::new().expect("temp dir");
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");

        let error = validate_project_dir(temp.path()).expect_err("missing git repo");

        assert!(error.to_string().contains("git rev-parse --git-dir failed"));
    }

    #[test]
    fn validate_project_dir_rejects_dirty_repo() {
        let temp = TempDir::new().expect("temp dir");
        init_git_project(temp.path());
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");
        std::process::Command::new("git")
            .args(["add", "Cargo.toml"])
            .current_dir(temp.path())
            .status()
            .expect("git add");
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(temp.path())
            .status()
            .expect("git commit");
        fs::write(temp.path().join("scratch.txt"), "dirty\n").expect("scratch file");

        let error = validate_project_dir(temp.path()).expect_err("dirty repo");

        assert!(error
            .to_string()
            .contains("refusing to run experiment on a dirty git repository"));
    }

    #[test]
    fn cli_parser_accepts_direct_mode() {
        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: ExperimentCommands,
        }

        let cli = TestCli::try_parse_from([
            "experiment",
            "run",
            "--signal",
            "latency",
            "--hypothesis",
            "parallelism helps",
            "--mode",
            "direct",
        ])
        .expect("parse experiment cli");

        match cli.command {
            ExperimentCommands::Run { mode, .. } => {
                assert_eq!(mode, ExperimentNodeMode::Direct);
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn cli_parser_accepts_placeholder_mode() {
        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: ExperimentCommands,
        }

        let cli = TestCli::try_parse_from([
            "experiment",
            "run",
            "--signal",
            "latency",
            "--hypothesis",
            "parallelism helps",
            "--mode",
            "placeholder",
        ])
        .expect("parse experiment cli");

        match cli.command {
            ExperimentCommands::Run { mode, .. } => {
                assert_eq!(mode, ExperimentNodeMode::Placeholder);
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn cli_parser_accepts_subagent_mode_and_project() {
        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: ExperimentCommands,
        }

        let cli = TestCli::try_parse_from([
            "experiment",
            "run",
            "--signal",
            "latency",
            "--hypothesis",
            "parallelism helps",
            "--mode",
            "subagent",
            "--project",
            "/tmp/demo",
        ])
        .expect("parse experiment cli");

        match cli.command {
            ExperimentCommands::Run { mode, project, .. } => {
                assert_eq!(mode, ExperimentNodeMode::Subagent);
                assert_eq!(project.as_deref(), Some("/tmp/demo"));
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn cli_parser_accepts_sequential_flag() {
        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: ExperimentCommands,
        }

        let cli = TestCli::try_parse_from([
            "experiment",
            "run",
            "--signal",
            "latency",
            "--hypothesis",
            "parallelism helps",
            "--sequential",
        ])
        .expect("parse experiment cli");

        match cli.command {
            ExperimentCommands::Run { sequential, .. } => {
                assert!(sequential);
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn cli_parser_rejects_zero_max_rounds() {
        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: ExperimentCommands,
        }

        let error = TestCli::try_parse_from([
            "experiment",
            "run",
            "--signal",
            "latency",
            "--hypothesis",
            "parallelism helps",
            "--max-rounds",
            "0",
        ])
        .expect_err("zero max rounds should fail");

        assert!(error.to_string().contains("1.."));
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

    fn write_sample_chain(path: &Path) {
        write_chain_with_signals(
            path,
            [(
                "sequential-tool-calls",
                "Parallelizing tool calls reduces token waste",
                "build_ok=true; tests=0/0, failed=0; placeholder evaluation",
            )],
        );
    }
}
