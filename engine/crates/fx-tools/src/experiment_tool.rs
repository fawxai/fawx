use async_trait::async_trait;
use fx_config::FawxConfig;
use fx_consensus::{
    format_auto_chain_result, load_chain_history_for_signal, CargoWorkspace, ConsensusError,
    ExperimentConfig, ExperimentRunner, FitnessCriterion, GenerationStrategy, LlmPatchSource,
    MetricType, ModificationScope, NeutralEvaluatorConfig, NodeConfig, NodeId, PathPattern,
    ProgressCallback, ProposalTier, RoundNodes, RoundNodesBuilder, Severity, Signal,
    SubagentPatchSource,
};
use fx_llm::{ModelInfo, ModelRouter, ToolDefinition};
use fx_subagent::SubagentControl;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Callback for registering experiments in an external registry.
/// Implemented by the HTTP layer to bridge fx-tools → fx-api.
pub trait ExperimentRegistrar: Send + Sync {
    fn register_started(&self, signal: &str, hypothesis: &str) -> String;
    fn register_completed(&self, id: &str, success: bool, summary: &str);
    fn register_failed(&self, id: &str, error: &str);
}

#[derive(Clone)]
pub struct ExperimentToolState {
    pub chain_path: PathBuf,
    pub router: Arc<ModelRouter>,
    pub config: FawxConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RunExperimentArgs {
    pub signal: String,
    pub hypothesis: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_nodes")]
    pub nodes: u32,
    #[serde(default = "default_mode")]
    pub mode: ExperimentNodeMode,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    pub project: Option<PathBuf>,
    #[serde(default)]
    pub sequential: bool,
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExperimentNodeMode {
    Placeholder,
    Direct,
    Subagent,
}

pub fn run_experiment_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "run_experiment".to_string(),
        description: "Run a proof-of-fitness experiment. Spawns competing subagent nodes that generate patches, evaluates them against fitness criteria, and records the result to the consensus chain. Use this when asked to improve, research, or experiment with any aspect of the codebase.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "signal": {
                    "type": "string",
                    "description": "Signal or topic that triggered this experiment"
                },
                "hypothesis": {
                    "type": "string",
                    "description": "What improvement or research hypothesis to test"
                },
                "scope": {
                    "type": "string",
                    "description": "File patterns to modify (glob, comma-separated). Default: src/**/*.rs"
                },
                "nodes": {
                    "type": "integer",
                    "description": "Number of competing nodes (default: 3)"
                },
                "mode": {
                    "type": "string",
                    "enum": ["placeholder", "direct", "subagent"],
                    "description": "Experiment mode. Default: subagent"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout per node in seconds (default: 120)"
                },
                "project": {
                    "type": "string",
                    "description": "Cargo project directory. Defaults to the current working directory"
                },
                "sequential": {
                    "type": "boolean",
                    "description": "Run node generation and evaluation one at a time. Default: false"
                },
                "max_rounds": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum rounds for auto-chain loop. Default: 1 (single-shot)"
                }
            },
            "required": ["signal", "hypothesis"]
        }),
    }
}

pub fn parse_run_experiment_args(
    args: &serde_json::Value,
    working_dir: &Path,
) -> Result<RunExperimentArgs, String> {
    let mut parsed: RunExperimentArgs =
        serde_json::from_value(args.clone()).map_err(|error| error.to_string())?;
    if parsed.signal.trim().is_empty() {
        return Err("signal is required".to_string());
    }
    if parsed.hypothesis.trim().is_empty() {
        return Err("hypothesis is required".to_string());
    }
    if parsed.nodes == 0 {
        return Err("nodes must be at least 1".to_string());
    }
    if parsed.max_rounds == 0 {
        return Err("max_rounds must be at least 1".to_string());
    }
    parsed.project = Some(expand_tilde(
        &parsed
            .project
            .clone()
            .unwrap_or_else(|| working_dir.to_path_buf()),
    ));
    Ok(parsed)
}

/// Expand `~` or `~/...` to the user's home directory.
fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s == "~" {
        dirs::home_dir().unwrap_or_else(|| path.to_path_buf())
    } else if let Some(rest) = s.strip_prefix("~/") {
        match dirs::home_dir() {
            Some(home) => home.join(rest),
            None => path.to_path_buf(),
        }
    } else {
        path.to_path_buf()
    }
}

pub async fn handle_run_experiment(
    state: &ExperimentToolState,
    subagent_control: Option<&Arc<dyn SubagentControl>>,
    working_dir: &Path,
    args: &serde_json::Value,
    progress: Option<ProgressCallback>,
) -> Result<String, String> {
    let parsed = parse_run_experiment_args(args, working_dir)?;
    let runner = build_runner(&parsed, state, subagent_control)?.with_progress(progress);
    let chain_result = runner
        .run_loop(build_config(&parsed), parsed.max_rounds)
        .await
        .map_err(|error| error.to_string())?;
    Ok(format_auto_chain_result(&chain_result, |report| {
        format_experiment_report(&parsed, report)
    }))
}

/// Result delivered to the completion callback when a background experiment finishes.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields read by callback consumers (experiment registry integration)
pub struct BackgroundExperimentResult {
    pub signal: String,
    pub success: bool,
    pub summary: String,
    pub error: Option<String>,
}

/// Run an experiment in the background. Returns immediately with a status message.
/// The experiment runs in a `tokio::spawn` task and calls `on_complete` when done.
pub fn spawn_background_experiment(
    state: &ExperimentToolState,
    subagent_control: Option<Arc<dyn SubagentControl>>,
    working_dir: &Path,
    args: &serde_json::Value,
    progress: Option<ProgressCallback>,
    on_complete: Option<Arc<dyn Fn(BackgroundExperimentResult) + Send + Sync>>,
    registrar: Option<Arc<dyn ExperimentRegistrar>>,
) -> Result<String, String> {
    let parsed = parse_run_experiment_args(args, working_dir)?;
    let signal = parsed.signal.clone();
    let hypothesis = parsed.hypothesis.clone();
    let max_rounds = parsed.max_rounds;
    let runner = build_runner(&parsed, state, subagent_control.as_ref())?.with_progress(progress);
    let config = build_config(&parsed);
    let experiment_id = register_experiment_start(registrar.as_ref(), &signal, &hypothesis);
    let spawn_experiment_id = experiment_id.clone();

    let spawn_signal = signal.clone();
    tokio::spawn(async move {
        let completion = match runner.run_loop(config, max_rounds).await {
            Ok(chain_result) => BackgroundExperimentResult {
                signal: spawn_signal.clone(),
                success: true,
                summary: format_auto_chain_result(&chain_result, |report| {
                    format_experiment_report(&parsed, report)
                }),
                error: None,
            },
            Err(error) => {
                tracing::error!(
                    signal = %spawn_signal,
                    %error,
                    "background experiment failed"
                );
                BackgroundExperimentResult {
                    signal: spawn_signal.clone(),
                    success: false,
                    summary: String::new(),
                    error: Some(error.to_string()),
                }
            }
        };
        notify_registrar(
            registrar.as_ref(),
            spawn_experiment_id.as_deref(),
            &completion,
        );
        if let Some(callback) = on_complete {
            callback(completion);
        }
    });

    let id_msg = experiment_id.as_deref().unwrap_or("(unregistered)");
    Ok(format!(
        "Experiment started in background (ID: {id_msg}).\n\
         Signal: {signal}\n\
         Hypothesis: {hypothesis}\n\
         Max rounds: {max_rounds}\n\n\
         The experiment is running asynchronously. Check the Experiment Monitor for progress."
    ))
}

fn register_experiment_start(
    registrar: Option<&Arc<dyn ExperimentRegistrar>>,
    signal: &str,
    hypothesis: &str,
) -> Option<String> {
    registrar.and_then(|callback| {
        let id = callback.register_started(signal, hypothesis);
        (!id.is_empty()).then_some(id)
    })
}

fn notify_registrar(
    registrar: Option<&Arc<dyn ExperimentRegistrar>>,
    experiment_id: Option<&str>,
    completion: &BackgroundExperimentResult,
) {
    let (Some(callback), Some(id)) = (registrar, experiment_id) else {
        return;
    };
    if completion.success {
        callback.register_completed(id, true, &completion.summary);
        return;
    }
    let error = completion.error.as_deref().unwrap_or("unknown");
    callback.register_failed(id, error);
}

fn build_runner(
    args: &RunExperimentArgs,
    state: &ExperimentToolState,
    subagent_control: Option<&Arc<dyn SubagentControl>>,
) -> Result<ExperimentRunner, String> {
    match args.mode {
        ExperimentNodeMode::Placeholder => build_placeholder_runner(args, state, subagent_control),
        ExperimentNodeMode::Direct => build_direct_runner(args, state),
        ExperimentNodeMode::Subagent => build_subagent_runner(args, state, subagent_control),
    }
}

fn build_placeholder_runner(
    args: &RunExperimentArgs,
    state: &ExperimentToolState,
    subagent_control: Option<&Arc<dyn SubagentControl>>,
) -> Result<ExperimentRunner, String> {
    let nodes = build_nodes(args, state, subagent_control, "")?;
    let neutral_evaluator = build_neutral_evaluator(args)?;
    ExperimentRunner::with_nodes(state.chain_path.clone(), nodes, neutral_evaluator)
        .map_err(|error| error.to_string())
}

fn build_direct_runner(
    args: &RunExperimentArgs,
    state: &ExperimentToolState,
) -> Result<ExperimentRunner, String> {
    let builder = DirectRoundNodesBuilder {
        args: args.clone(),
        state: state.clone(),
    };
    ExperimentRunner::with_round_nodes_builder(state.chain_path.clone(), builder)
        .map_err(|error| error.to_string())
}

fn build_subagent_runner(
    args: &RunExperimentArgs,
    state: &ExperimentToolState,
    subagent_control: Option<&Arc<dyn SubagentControl>>,
) -> Result<ExperimentRunner, String> {
    let control = subagent_control
        .cloned()
        .ok_or_else(|| "subagent control not configured".to_string())?;
    let builder = SubagentRoundNodesBuilder {
        args: args.clone(),
        state: state.clone(),
        control,
    };
    ExperimentRunner::with_round_nodes_builder(state.chain_path.clone(), builder)
        .map_err(|error| error.to_string())
}

#[derive(Clone)]
struct DirectRoundNodesBuilder {
    args: RunExperimentArgs,
    state: ExperimentToolState,
}

impl RoundNodesBuilder for DirectRoundNodesBuilder {
    fn build_round_nodes(
        &self,
        chain_path: &Path,
        signal: &str,
    ) -> Result<RoundNodes, ConsensusError> {
        let chain_history = load_chain_history_for_signal(chain_path, signal)?;
        Ok(RoundNodes {
            nodes: build_direct_nodes(&self.args, &self.state, &chain_history)
                .map_err(protocol_error)?,
            neutral_evaluator: build_neutral_evaluator(&self.args).map_err(protocol_error)?,
        })
    }
}

#[derive(Clone)]
struct SubagentRoundNodesBuilder {
    args: RunExperimentArgs,
    state: ExperimentToolState,
    control: Arc<dyn SubagentControl>,
}

impl RoundNodesBuilder for SubagentRoundNodesBuilder {
    fn build_round_nodes(
        &self,
        chain_path: &Path,
        signal: &str,
    ) -> Result<RoundNodes, ConsensusError> {
        let chain_history = load_chain_history_for_signal(chain_path, signal)?;
        Ok(RoundNodes {
            nodes: build_subagent_nodes(
                &self.args,
                &self.state,
                Some(&self.control),
                &chain_history,
            )
            .map_err(protocol_error)?,
            neutral_evaluator: build_neutral_evaluator(&self.args).map_err(protocol_error)?,
        })
    }
}

fn protocol_error(error: String) -> ConsensusError {
    ConsensusError::Protocol(error)
}

fn build_neutral_evaluator(
    args: &RunExperimentArgs,
) -> Result<Option<NeutralEvaluatorConfig>, String> {
    if args.nodes != 1 {
        return Ok(None);
    }
    match args.mode {
        ExperimentNodeMode::Placeholder => Ok(Some(build_placeholder_neutral_evaluator())),
        ExperimentNodeMode::Direct | ExperimentNodeMode::Subagent => {
            let project_dir = validate_project_dir(required_project(args)?)?;
            let package = CargoWorkspace::package_from_scope(&args.scope);
            let workspace =
                CargoWorkspace::clone_from_with_package(&project_dir, "neutral-evaluator", package)
                    .map_err(|error| error.to_string())?;
            Ok(Some(NeutralEvaluatorConfig {
                node_id: NodeId("neutral-evaluator".to_owned()),
                workspace: Box::new(workspace),
            }))
        }
    }
}

fn build_nodes(
    args: &RunExperimentArgs,
    state: &ExperimentToolState,
    subagent_control: Option<&Arc<dyn SubagentControl>>,
    chain_history: &str,
) -> Result<Vec<NodeConfig>, String> {
    match args.mode {
        ExperimentNodeMode::Placeholder => Ok(build_placeholder_nodes(args.nodes)),
        ExperimentNodeMode::Direct => build_direct_nodes(args, state, chain_history),
        ExperimentNodeMode::Subagent => {
            build_subagent_nodes(args, state, subagent_control, chain_history)
        }
    }
}

fn build_placeholder_nodes(count: u32) -> Vec<NodeConfig> {
    (0..count)
        .map(|index| NodeConfig {
            node_id: NodeId(format!("node-{index}")),
            strategy: strategy_for(index),
            patch_source: Box::new(PlaceholderPatchSource { index }),
            workspace: Box::new(PlaceholderWorkspace),
        })
        .collect()
}

fn build_placeholder_neutral_evaluator() -> NeutralEvaluatorConfig {
    NeutralEvaluatorConfig {
        node_id: NodeId("neutral-evaluator".to_owned()),
        workspace: Box::new(PlaceholderWorkspace),
    }
}

fn build_direct_nodes(
    args: &RunExperimentArgs,
    state: &ExperimentToolState,
    chain_history: &str,
) -> Result<Vec<NodeConfig>, String> {
    let model = resolve_model(&state.router, &state.config)?;
    let project_dir = validate_project_dir(required_project(args)?)?;
    let package = CargoWorkspace::package_from_scope(&args.scope);
    (0..args.nodes)
        .map(|index| {
            let node_id = NodeId(format!("node-{index}"));
            let strategy = strategy_for(index);
            let workspace =
                CargoWorkspace::clone_from_with_package(&project_dir, &node_id.0, package.clone())
                    .map_err(|error| error.to_string())?;
            Ok(NodeConfig {
                node_id,
                strategy,
                patch_source: Box::new(
                    LlmPatchSource::new(Arc::clone(&state.router), model.clone())
                        .with_chain_history(chain_history.to_owned()),
                ),
                workspace: Box::new(workspace),
            })
        })
        .collect()
}

fn build_subagent_nodes(
    args: &RunExperimentArgs,
    _state: &ExperimentToolState,
    subagent_control: Option<&Arc<dyn SubagentControl>>,
    chain_history: &str,
) -> Result<Vec<NodeConfig>, String> {
    let control = subagent_control
        .cloned()
        .ok_or_else(|| "subagent control not configured".to_string())?;
    let project_dir = validate_project_dir(required_project(args)?)?;
    let package = CargoWorkspace::package_from_scope(&args.scope);
    (0..args.nodes)
        .map(|index| {
            let node_id = NodeId(format!("node-{index}"));
            let strategy = strategy_for(index);
            let (generator_workspace, evaluator_workspace) =
                CargoWorkspace::clone_node_workspaces(&project_dir, &node_id.0, package.clone())
                    .map_err(|error| error.to_string())?;
            Ok(NodeConfig {
                node_id,
                strategy: strategy.clone(),
                patch_source: Box::new(
                    SubagentPatchSource::with_workspace(
                        Arc::clone(&control),
                        strategy,
                        generator_workspace,
                    )
                    .with_chain_history(chain_history.to_owned()),
                ),
                workspace: Box::new(evaluator_workspace),
            })
        })
        .collect()
}

fn required_project(args: &RunExperimentArgs) -> Result<&Path, String> {
    args.project
        .as_deref()
        .ok_or_else(|| "project is required".to_string())
}

fn validate_project_dir(project_dir: &Path) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(project_dir).map_err(|error| {
        format!(
            "failed to access project path {}: {error}",
            project_dir.display()
        )
    })?;
    if !canonical.is_dir() {
        return Err(format!(
            "project path is not a directory: {}",
            canonical.display()
        ));
    }
    let manifest = canonical.join("Cargo.toml");
    if !manifest.is_file() {
        return Err(format!(
            "project is missing Cargo.toml: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn resolve_model(router: &ModelRouter, config: &FawxConfig) -> Result<String, String> {
    if let Some(active) = router.active_model() {
        return Ok(active.to_string());
    }
    if let Some(default) = config.model.default_model.clone() {
        return Ok(default);
    }
    router
        .available_models()
        .first()
        .map(model_name)
        .ok_or_else(|| "no model available for experiment".to_string())
}

fn model_name(model: &ModelInfo) -> String {
    model.model_id.clone()
}

fn build_config(args: &RunExperimentArgs) -> ExperimentConfig {
    ExperimentConfig {
        signal: Signal {
            id: Uuid::new_v4(),
            name: args.signal.clone(),
            description: format!("Experiment triggered by signal '{}'", args.signal),
            severity: Severity::Medium,
        },
        hypothesis: args.hypothesis.clone(),
        fitness_criteria: default_fitness_criteria(),
        scope: ModificationScope {
            allowed_files: parse_scope(&args.scope),
            proposal_tier: ProposalTier::Tier1,
        },
        timeout: Duration::from_secs(args.timeout),
        min_candidates: args.nodes,
        sequential: args.sequential,
    }
}

fn default_fitness_criteria() -> Vec<FitnessCriterion> {
    vec![
        FitnessCriterion {
            name: "build_success".to_string(),
            metric_type: MetricType::Higher,
            weight: 0.2,
        },
        FitnessCriterion {
            name: "test_pass_rate".to_string(),
            metric_type: MetricType::Higher,
            weight: 0.5,
        },
        FitnessCriterion {
            name: "signal_resolution".to_string(),
            metric_type: MetricType::Higher,
            weight: 0.3,
        },
    ]
}

fn parse_scope(scope: &str) -> Vec<PathPattern> {
    scope
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathPattern::from)
        .collect()
}

fn strategy_for(index: u32) -> GenerationStrategy {
    match index % 3 {
        0 => GenerationStrategy::Conservative,
        1 => GenerationStrategy::Aggressive,
        _ => GenerationStrategy::Creative,
    }
}

fn format_experiment_report(
    args: &RunExperimentArgs,
    report: &fx_consensus::ExperimentReport,
) -> String {
    let mut lines = vec![
        "═══ Experiment Complete ═══".to_string(),
        format!("Signal:        {}", args.signal),
        format!("Hypothesis:    {}", args.hypothesis),
        format!("Mode:          {:?}", args.mode),
        format!("Nodes:         {}", args.nodes),
        format!("Experiment ID: {}", report.result.experiment_id),
        format!("Decision:      {}", report.result.decision.emoji_label()),
    ];
    for candidate in &report.candidates {
        lines.push(format!(
            "{} {:?} score: {:.2}{}",
            candidate.node_id.0,
            candidate.strategy,
            candidate.aggregate_score,
            if candidate.is_winner {
                " ← WINNER"
            } else {
                ""
            }
        ));
    }
    lines.push(format!(
        "Chain entry #{} recorded",
        report.chain_entry_index
    ));
    lines.join("\n")
}

struct PlaceholderPatchSource {
    index: u32,
}

#[async_trait]
impl fx_consensus::PatchSource for PlaceholderPatchSource {
    async fn generate_patch(
        &self,
        _system_prompt: &str,
        _experiment: &fx_consensus::Experiment,
    ) -> Result<fx_consensus::PatchResponse, fx_consensus::ConsensusError> {
        Ok(fx_consensus::PatchResponse {
            patch: format!("diff --git a/src/node_{0}.rs b/src/node_{0}.rs", self.index),
            approach: format!("placeholder candidate {}", self.index),
            self_metrics: std::collections::BTreeMap::from([
                ("build_success".to_string(), 1.0),
                ("test_pass_rate".to_string(), 1.0),
                (
                    "signal_resolution".to_string(),
                    1.0 - (self.index as f64 * 0.1),
                ),
            ]),
        })
    }
}

struct PlaceholderWorkspace;

#[async_trait]
impl fx_consensus::EvaluationWorkspace for PlaceholderWorkspace {
    async fn apply_patch(&self, _patch: &str) -> Result<(), fx_consensus::ConsensusError> {
        Ok(())
    }

    async fn build(&self) -> Result<(), fx_consensus::ConsensusError> {
        Ok(())
    }

    async fn test(&self) -> Result<fx_consensus::TestResult, fx_consensus::ConsensusError> {
        Ok(fx_consensus::TestResult {
            passed: 1,
            failed: 0,
            total: 1,
        })
    }

    async fn check_signal(
        &self,
        _signal: &fx_consensus::Signal,
    ) -> Result<bool, fx_consensus::ConsensusError> {
        Ok(true)
    }

    async fn check_regression(
        &self,
        _experiment: &fx_consensus::Experiment,
    ) -> Result<bool, fx_consensus::ConsensusError> {
        Ok(false)
    }

    async fn reset(&self) -> Result<(), fx_consensus::ConsensusError> {
        Ok(())
    }
}

fn default_scope() -> String {
    "src/**/*.rs".to_string()
}

fn default_nodes() -> u32 {
    3
}

fn default_timeout() -> u64 {
    120
}

fn default_mode() -> ExperimentNodeMode {
    ExperimentNodeMode::Subagent
}

fn default_max_rounds() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::FawxConfig;
    use fx_llm::ModelRouter;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;
    use tempfile::TempDir;

    struct RecordingRegistrar {
        started: Mutex<Vec<(String, String)>>,
        completed: Mutex<Vec<(String, bool, String)>>,
        failed: Mutex<Vec<(String, String)>>,
    }

    impl RecordingRegistrar {
        fn new() -> Self {
            Self {
                started: Mutex::new(Vec::new()),
                completed: Mutex::new(Vec::new()),
                failed: Mutex::new(Vec::new()),
            }
        }
    }

    impl ExperimentRegistrar for RecordingRegistrar {
        fn register_started(&self, signal: &str, hypothesis: &str) -> String {
            self.started
                .lock()
                .expect("started lock")
                .push((signal.to_string(), hypothesis.to_string()));
            "exp-123".to_string()
        }

        fn register_completed(&self, id: &str, success: bool, summary: &str) {
            self.completed.lock().expect("completed lock").push((
                id.to_string(),
                success,
                summary.to_string(),
            ));
        }

        fn register_failed(&self, id: &str, error: &str) {
            self.failed
                .lock()
                .expect("failed lock")
                .push((id.to_string(), error.to_string()));
        }
    }

    fn experiment_state(root: &std::path::Path) -> ExperimentToolState {
        std::fs::create_dir_all(root.join("consensus")).expect("consensus dir");
        ExperimentToolState {
            chain_path: root.join("consensus").join("chain.json"),
            router: Arc::new(ModelRouter::new()),
            config: FawxConfig::default(),
        }
    }

    #[test]
    fn tool_definition_is_present() {
        let definition = run_experiment_tool_definition();
        assert_eq!(definition.name, "run_experiment");
        assert_eq!(
            definition.parameters["required"],
            serde_json::json!(["signal", "hypothesis"])
        );
    }

    #[test]
    fn parse_args_defaults_mode_and_project() {
        let temp = TempDir::new().expect("tempdir");
        let parsed = parse_run_experiment_args(
            &serde_json::json!({"signal": "latency", "hypothesis": "parallelism helps"}),
            temp.path(),
        )
        .expect("parse args");
        assert_eq!(parsed.mode, ExperimentNodeMode::Subagent);
        assert_eq!(parsed.project.as_deref(), Some(temp.path()));
        assert_eq!(parsed.nodes, 3);
        assert!(!parsed.sequential);
    }

    #[test]
    fn parse_args_placeholder_mode_survives_corrupt_chain() {
        // Regression test: Placeholder mode must not attempt to load chain.
        // With a corrupt chain file, Subagent mode would fail but Placeholder
        // should succeed because the guard skips the load entirely.
        let temp = TempDir::new().expect("tempdir");
        let parsed = parse_run_experiment_args(
            &serde_json::json!({
                "signal": "test-signal",
                "hypothesis": "test",
                "mode": "placeholder"
            }),
            temp.path(),
        )
        .expect("parse");
        assert_eq!(parsed.mode, ExperimentNodeMode::Placeholder);
    }

    #[test]
    fn parse_args_accepts_sequential_mode() {
        let temp = TempDir::new().expect("tempdir");
        let parsed = parse_run_experiment_args(
            &serde_json::json!({
                "signal": "latency",
                "hypothesis": "parallelism helps",
                "sequential": true
            }),
            temp.path(),
        )
        .expect("parse args");

        assert!(parsed.sequential);
    }

    #[test]
    fn parse_args_rejects_missing_required_fields() {
        let temp = TempDir::new().expect("tempdir");
        let error = parse_run_experiment_args(
            &serde_json::json!({"signal": "", "hypothesis": "parallelism helps"}),
            temp.path(),
        )
        .expect_err("empty signal should fail");
        assert!(error.contains("signal is required"));
    }

    #[test]
    fn parse_args_rejects_zero_max_rounds() {
        let temp = TempDir::new().expect("tempdir");
        let error = parse_run_experiment_args(
            &serde_json::json!({
                "signal": "latency",
                "hypothesis": "parallelism helps",
                "max_rounds": 0
            }),
            temp.path(),
        )
        .expect_err("zero max_rounds should fail");
        assert!(error.contains("max_rounds must be at least 1"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn background_experiment_reports_registration_lifecycle() {
        let temp = TempDir::new().expect("tempdir");
        let registrar = Arc::new(RecordingRegistrar::new());
        let (done_tx, done_rx) = mpsc::channel();
        let callback: Arc<dyn Fn(BackgroundExperimentResult) + Send + Sync> = Arc::new(move |_| {
            let _ = done_tx.send(());
        });

        let message = spawn_background_experiment(
            &experiment_state(temp.path()),
            None,
            temp.path(),
            &serde_json::json!({
                "signal": "latency",
                "hypothesis": "parallelism helps",
                "mode": "placeholder",
                "nodes": 1,
                "project": temp.path().display().to_string(),
            }),
            None,
            Some(callback),
            Some(registrar.clone()),
        )
        .expect("background experiment should start");

        assert!(message.contains("ID: exp-123"), "{message}");
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("background experiment should finish");

        let started = registrar.started.lock().expect("started lock").clone();
        let completed = registrar.completed.lock().expect("completed lock").clone();
        let failed = registrar.failed.lock().expect("failed lock").clone();

        assert_eq!(
            started,
            vec![("latency".to_string(), "parallelism helps".to_string())]
        );
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].0, "exp-123");
        assert!(completed[0].1);
        assert!(completed[0].2.contains("Signal:"));
        assert!(completed[0].2.contains("latency"));
        assert!(failed.is_empty());
    }
}
