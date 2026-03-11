use async_trait::async_trait;
use fx_config::FawxConfig;
use fx_consensus::{
    CargoWorkspace, ExperimentConfig, ExperimentRunner, FitnessCriterion, GenerationStrategy,
    LlmPatchSource, MetricType, ModificationScope, NeutralEvaluatorConfig, NodeConfig, NodeId,
    PathPattern, ProposalTier, Severity, Signal, SubagentPatchSource,
};
use fx_llm::{ModelInfo, ModelRouter, ToolDefinition};
use fx_subagent::SubagentControl;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct ExperimentToolState {
    pub chain_path: PathBuf,
    pub router: Arc<ModelRouter>,
    pub config: FawxConfig,
}

#[derive(Debug, Deserialize)]
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
    parsed.project = Some(
        parsed
            .project
            .clone()
            .unwrap_or_else(|| working_dir.to_path_buf()),
    );
    Ok(parsed)
}

pub async fn handle_run_experiment(
    state: &ExperimentToolState,
    subagent_control: Option<&Arc<dyn SubagentControl>>,
    working_dir: &Path,
    args: &serde_json::Value,
) -> Result<String, String> {
    let parsed = parse_run_experiment_args(args, working_dir)?;
    let nodes = build_nodes(&parsed, state, subagent_control)?;
    let neutral_evaluator = build_neutral_evaluator(&parsed)?;
    let runner = ExperimentRunner::with_nodes(state.chain_path.clone(), nodes, neutral_evaluator)
        .map_err(|error| error.to_string())?;
    let report = runner
        .run(build_config(&parsed))
        .await
        .map_err(|error| error.to_string())?;
    Ok(format_experiment_report(&parsed, &report))
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
            let workspace = CargoWorkspace::clone_from(&project_dir, "neutral-evaluator")
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
) -> Result<Vec<NodeConfig>, String> {
    match args.mode {
        ExperimentNodeMode::Placeholder => Ok(build_placeholder_nodes(args.nodes)),
        ExperimentNodeMode::Direct => build_direct_nodes(args, state),
        ExperimentNodeMode::Subagent => build_subagent_nodes(args, state, subagent_control),
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
) -> Result<Vec<NodeConfig>, String> {
    let model = resolve_model(&state.router, &state.config)?;
    let project_dir = validate_project_dir(required_project(args)?)?;
    (0..args.nodes)
        .map(|index| {
            let node_id = NodeId(format!("node-{index}"));
            let strategy = strategy_for(index);
            let workspace = CargoWorkspace::clone_from(&project_dir, &node_id.0)
                .map_err(|error| error.to_string())?;
            Ok(NodeConfig {
                node_id,
                strategy,
                patch_source: Box::new(LlmPatchSource::new(
                    Arc::clone(&state.router),
                    model.clone(),
                )),
                workspace: Box::new(workspace),
            })
        })
        .collect()
}

fn build_subagent_nodes(
    args: &RunExperimentArgs,
    _state: &ExperimentToolState,
    subagent_control: Option<&Arc<dyn SubagentControl>>,
) -> Result<Vec<NodeConfig>, String> {
    let control = subagent_control
        .cloned()
        .ok_or_else(|| "subagent control not configured".to_string())?;
    let project_dir = validate_project_dir(required_project(args)?)?;
    (0..args.nodes)
        .map(|index| {
            let node_id = NodeId(format!("node-{index}"));
            let strategy = strategy_for(index);
            let workspace = CargoWorkspace::clone_from(&project_dir, &node_id.0)
                .map_err(|error| error.to_string())?;
            Ok(NodeConfig {
                node_id,
                strategy: strategy.clone(),
                patch_source: Box::new(SubagentPatchSource::new(
                    Arc::clone(&control),
                    strategy,
                    project_dir.clone(),
                )),
                workspace: Box::new(workspace),
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
        format!(
            "Decision:      {}",
            match report.result.decision {
                fx_consensus::Decision::Accept => "✅ ACCEPT",
                fx_consensus::Decision::Reject => "❌ REJECT",
                fx_consensus::Decision::Inconclusive => "➖ INCONCLUSIVE",
            }
        ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
}
