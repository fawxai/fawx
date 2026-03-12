use crate::chain::JsonFileChainStorage;
use crate::error::ConsensusError;
use crate::evaluator::{BuildTestEvaluator, EvaluationWorkspace};
use crate::generator::{GenerationStrategy, LlmCandidateGenerator, PatchSource};
use crate::orchestrator::{CandidateEvaluator, CandidateGenerator, ExperimentOrchestrator};
use crate::protocol::{ConsensusProtocol, ExperimentConfig, LocalConsensusEngine};
use crate::types::{Candidate, ConsensusResult, Decision, NodeId};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct ExperimentRunner {
    engine: LocalConsensusEngine,
    storage_path: PathBuf,
    node_provider: NodeProvider,
}

pub struct NodeConfig {
    pub node_id: NodeId,
    pub strategy: GenerationStrategy,
    pub patch_source: Box<dyn PatchSource>,
    pub workspace: Box<dyn EvaluationWorkspace>,
}

pub struct NeutralEvaluatorConfig {
    pub node_id: NodeId,
    pub workspace: Box<dyn EvaluationWorkspace>,
}

pub struct RoundNodes {
    pub nodes: Vec<NodeConfig>,
    pub neutral_evaluator: Option<NeutralEvaluatorConfig>,
}

pub trait RoundNodesBuilder: Send + Sync {
    fn build_round_nodes(
        &self,
        chain_path: &Path,
        signal: &str,
    ) -> Result<RoundNodes, ConsensusError>;
}

#[derive(Debug)]
pub struct ExperimentReport {
    pub result: ConsensusResult,
    pub chain_entry_index: u64,
    pub candidates: Vec<CandidateReport>,
}

impl ExperimentReport {
    pub fn best_aggregate_score(&self) -> f64 {
        self.result
            .aggregate_scores
            .values()
            .copied()
            .fold(0.0_f64, f64::max)
    }

    fn should_stop_auto_chain(&self) -> bool {
        match self.result.decision {
            Decision::Accept | Decision::Inconclusive => true,
            Decision::Reject => self.best_aggregate_score() == 0.0,
        }
    }
}

#[derive(Debug)]
pub struct CandidateReport {
    pub node_id: NodeId,
    pub strategy: GenerationStrategy,
    pub approach: String,
    pub aggregate_score: f64,
    pub is_winner: bool,
}

#[derive(Debug)]
pub struct AutoChainResult {
    pub rounds_completed: u32,
    pub max_rounds: u32,
    pub reports: Vec<ExperimentReport>,
}

impl AutoChainResult {
    pub fn final_report(&self) -> Option<&ExperimentReport> {
        self.reports.last()
    }
}

pub fn validate_auto_chain_rounds(max_rounds: u32) -> Result<(), ConsensusError> {
    if max_rounds == 0 {
        return Err(ConsensusError::Protocol(
            "max_rounds must be at least 1".to_owned(),
        ));
    }
    Ok(())
}

pub fn format_auto_chain_result<F>(result: &AutoChainResult, mut format_report: F) -> String
where
    F: FnMut(&ExperimentReport) -> String,
{
    if result.max_rounds == 1 {
        return result
            .final_report()
            .map(&mut format_report)
            .unwrap_or_default();
    }
    let mut lines = Vec::new();
    for (index, report) in result.reports.iter().enumerate() {
        let round = index as u32 + 1;
        lines.push(format!(
            "═══ Experiment Round {}/{} ═══",
            round, result.max_rounds
        ));
        lines.push(format_report(report));
        if round < result.rounds_completed {
            lines.push(format!(
                "Continuing — promising result ({}), retrying with chain history...\n",
                report.result.decision.uppercase_label()
            ));
        }
    }
    lines.push(auto_chain_summary(result));
    lines.join("\n")
}

fn auto_chain_summary(result: &AutoChainResult) -> String {
    let final_decision = result
        .final_report()
        .map(|report| report.result.decision.uppercase_label())
        .unwrap_or("UNKNOWN");
    format!(
        "═══ Auto-chain complete: {} after {} round{} ═══",
        final_decision,
        result.rounds_completed,
        plural_suffix(result.rounds_completed),
    )
}

fn plural_suffix(rounds_completed: u32) -> &'static str {
    if rounds_completed == 1 {
        ""
    } else {
        "s"
    }
}

impl ExperimentRunner {
    pub fn new(storage_path: PathBuf) -> Result<Self, ConsensusError> {
        Self::with_nodes(storage_path, Vec::new(), None)
    }

    pub fn with_nodes(
        storage_path: PathBuf,
        nodes: Vec<NodeConfig>,
        neutral_evaluator: Option<NeutralEvaluatorConfig>,
    ) -> Result<Self, ConsensusError> {
        let engine =
            LocalConsensusEngine::new(Box::new(JsonFileChainStorage::new(storage_path.clone())))?;
        Ok(Self {
            engine,
            storage_path,
            node_provider: NodeProvider::Fixed(build_nodes(nodes, neutral_evaluator)),
        })
    }

    pub fn with_round_nodes_builder<B>(
        storage_path: PathBuf,
        builder: B,
    ) -> Result<Self, ConsensusError>
    where
        B: RoundNodesBuilder + 'static,
    {
        let engine =
            LocalConsensusEngine::new(Box::new(JsonFileChainStorage::new(storage_path.clone())))?;
        Ok(Self {
            engine,
            storage_path,
            node_provider: NodeProvider::Builder(Box::new(builder)),
        })
    }

    pub async fn run(&self, config: ExperimentConfig) -> Result<ExperimentReport, ConsensusError> {
        let nodes = self.active_nodes(&config.signal.name)?;
        let orchestrator = ExperimentOrchestrator::new(&self.engine);
        let result = orchestrator
            .run_experiment(config, nodes.generators(), nodes.evaluators())
            .await?;
        build_report(&self.engine, nodes.strategies(), result).await
    }

    pub async fn run_loop(
        &self,
        config: ExperimentConfig,
        max_rounds: u32,
    ) -> Result<AutoChainResult, ConsensusError> {
        validate_auto_chain_rounds(max_rounds)?;
        let mut reports = Vec::new();
        for _ in 1..=max_rounds {
            let report = self.run(config.clone()).await?;
            let should_stop = report.should_stop_auto_chain();
            reports.push(report);
            if should_stop {
                break;
            }
        }
        Ok(AutoChainResult {
            rounds_completed: reports.len() as u32,
            max_rounds,
            reports,
        })
    }

    fn active_nodes(&self, signal: &str) -> Result<ActiveNodes<'_>, ConsensusError> {
        match &self.node_provider {
            NodeProvider::Fixed(nodes) => Ok(ActiveNodes::Fixed(nodes)),
            NodeProvider::Builder(builder) => {
                let round_nodes = builder.build_round_nodes(&self.storage_path, signal)?;
                Ok(ActiveNodes::Built(build_nodes(
                    round_nodes.nodes,
                    round_nodes.neutral_evaluator,
                )))
            }
        }
    }
}

enum NodeProvider {
    Fixed(BuiltNodes),
    Builder(Box<dyn RoundNodesBuilder>),
}

enum ActiveNodes<'a> {
    Fixed(&'a BuiltNodes),
    Built(BuiltNodes),
}

impl ActiveNodes<'_> {
    fn generators(&self) -> &[Box<dyn CandidateGenerator>] {
        match self {
            ActiveNodes::Fixed(nodes) => &nodes.generators,
            ActiveNodes::Built(nodes) => &nodes.generators,
        }
    }

    fn evaluators(&self) -> &[Box<dyn CandidateEvaluator>] {
        match self {
            ActiveNodes::Fixed(nodes) => &nodes.evaluators,
            ActiveNodes::Built(nodes) => &nodes.evaluators,
        }
    }

    fn strategies(&self) -> &BTreeMap<NodeId, GenerationStrategy> {
        match self {
            ActiveNodes::Fixed(nodes) => &nodes.strategies,
            ActiveNodes::Built(nodes) => &nodes.strategies,
        }
    }
}

struct BuiltNodes {
    generators: Vec<Box<dyn CandidateGenerator>>,
    evaluators: Vec<Box<dyn CandidateEvaluator>>,
    strategies: BTreeMap<NodeId, GenerationStrategy>,
}

fn build_nodes(
    nodes: Vec<NodeConfig>,
    neutral_evaluator: Option<NeutralEvaluatorConfig>,
) -> BuiltNodes {
    let mut generators: Vec<Box<dyn CandidateGenerator>> = Vec::new();
    let mut evaluators: Vec<Box<dyn CandidateEvaluator>> = Vec::new();
    let mut strategies = BTreeMap::new();
    for node in nodes {
        strategies.insert(node.node_id.clone(), node.strategy.clone());
        generators.push(Box::new(LlmCandidateGenerator::new(
            node.node_id.clone(),
            node.strategy,
            node.patch_source,
        )));
        evaluators.push(Box::new(BuildTestEvaluator::new(
            node.node_id,
            node.workspace,
        )));
    }
    if generators.len() == 1 {
        if let Some(neutral_evaluator) = neutral_evaluator {
            evaluators.push(Box::new(BuildTestEvaluator::new(
                neutral_evaluator.node_id,
                neutral_evaluator.workspace,
            )));
        }
    }
    BuiltNodes {
        generators,
        evaluators,
        strategies,
    }
}

async fn build_report(
    engine: &LocalConsensusEngine,
    strategies: &BTreeMap<NodeId, GenerationStrategy>,
    result: ConsensusResult,
) -> Result<ExperimentReport, ConsensusError> {
    let candidates = engine.candidates(result.experiment_id).await?;
    let chain = engine.chain()?;
    let entry = chain
        .head()
        .ok_or_else(|| ConsensusError::Protocol("missing chain entry after experiment".into()))?;
    Ok(ExperimentReport {
        chain_entry_index: entry.index,
        candidates: candidate_reports(&candidates, strategies, &result),
        result,
    })
}

fn candidate_reports(
    candidates: &[Candidate],
    strategies: &BTreeMap<NodeId, GenerationStrategy>,
    result: &ConsensusResult,
) -> Vec<CandidateReport> {
    candidates
        .iter()
        .map(|candidate| CandidateReport {
            node_id: candidate.node_id.clone(),
            strategy: strategies
                .get(&candidate.node_id)
                .cloned()
                .unwrap_or(GenerationStrategy::Conservative),
            approach: candidate.approach.clone(),
            aggregate_score: *result.aggregate_scores.get(&candidate.id).unwrap_or(&0.0),
            is_winner: result.winner == Some(candidate.id),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::ChainStorage;
    use crate::evaluator::TestResult;
    use crate::generator::PatchResponse;
    use crate::types::{
        Decision, FitnessCriterion, MetricType, ModificationScope, PathPattern, ProposalTier,
        Severity, Signal,
    };
    use async_trait::async_trait;
    use std::sync::Mutex;
    use std::time::Duration;
    use uuid::Uuid;

    struct StaticPatchSource {
        patch: String,
        approach: String,
        metrics: BTreeMap<String, f64>,
    }

    struct PatchAwareWorkspace {
        current_patch: Mutex<String>,
    }

    struct FailingWorkspace;

    #[async_trait]
    impl EvaluationWorkspace for FailingWorkspace {
        async fn apply_patch(&self, _patch: &str) -> crate::error::Result<()> {
            Ok(())
        }

        async fn build(&self) -> crate::error::Result<()> {
            Err(ConsensusError::BuildFailed("build failed".into()))
        }

        async fn test(&self) -> crate::error::Result<TestResult> {
            Err(ConsensusError::TestFailed {
                passed: 0,
                failed: 1,
                total: 1,
            })
        }

        async fn check_signal(&self, _signal: &Signal) -> crate::error::Result<bool> {
            Ok(false)
        }

        async fn check_regression(
            &self,
            _experiment: &crate::types::Experiment,
        ) -> crate::error::Result<bool> {
            Ok(false)
        }

        async fn reset(&self) -> crate::error::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl PatchSource for StaticPatchSource {
        async fn generate_patch(
            &self,
            _system_prompt: &str,
            _experiment: &crate::types::Experiment,
        ) -> crate::error::Result<PatchResponse> {
            Ok(PatchResponse {
                patch: self.patch.clone(),
                approach: self.approach.clone(),
                self_metrics: self.metrics.clone(),
            })
        }
    }

    #[async_trait]
    impl EvaluationWorkspace for PatchAwareWorkspace {
        async fn apply_patch(&self, patch: &str) -> crate::error::Result<()> {
            *self.current_patch.lock().expect("patch lock") = patch.to_owned();
            Ok(())
        }

        async fn build(&self) -> crate::error::Result<()> {
            Ok(())
        }

        async fn test(&self) -> crate::error::Result<TestResult> {
            let patch = self.current_patch.lock().expect("patch lock").clone();
            match patch.as_str() {
                value if value.contains("node-a") => Ok(TestResult {
                    passed: 5,
                    failed: 0,
                    total: 5,
                }),
                value if value.contains("node-b") => Err(ConsensusError::TestFailed {
                    passed: 4,
                    failed: 1,
                    total: 5,
                }),
                value if value.contains("node-c") => Ok(TestResult {
                    passed: 5,
                    failed: 0,
                    total: 5,
                }),
                _ => Ok(TestResult {
                    passed: 0,
                    failed: 1,
                    total: 1,
                }),
            }
        }

        async fn check_signal(&self, _signal: &Signal) -> crate::error::Result<bool> {
            let patch = self.current_patch.lock().expect("patch lock").clone();
            Ok(patch.contains("node-c"))
        }

        async fn check_regression(
            &self,
            _experiment: &crate::types::Experiment,
        ) -> crate::error::Result<bool> {
            Ok(false)
        }

        async fn reset(&self) -> crate::error::Result<()> {
            *self.current_patch.lock().expect("patch lock") = String::new();
            Ok(())
        }
    }

    #[tokio::test]
    async fn runner_selects_best_candidate_and_records_chain_entry() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![
                node("node-a", GenerationStrategy::Conservative),
                node("node-b", GenerationStrategy::Aggressive),
                node("node-c", GenerationStrategy::Creative),
            ],
            None,
        )
        .expect("runner");

        let report = runner.run(sample_config()).await.expect("run");

        assert_eq!(report.result.decision, crate::types::Decision::Accept);
        assert_eq!(report.chain_entry_index, 0);
        assert_eq!(report.candidates.len(), 3);
        assert!(report
            .candidates
            .iter()
            .any(|candidate| candidate.is_winner));
        assert!(report
            .candidates
            .iter()
            .find(|candidate| candidate.node_id == NodeId::from("node-c"))
            .map(|candidate| matches!(candidate.strategy, GenerationStrategy::Creative))
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn runner_rejects_when_all_candidates_fail() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![
                failing_node("node-a", GenerationStrategy::Conservative),
                failing_node("node-b", GenerationStrategy::Aggressive),
                failing_node("node-c", GenerationStrategy::Creative),
            ],
            None,
        )
        .expect("runner");

        let report = runner.run(sample_config()).await.expect("run");

        assert_eq!(report.result.decision, crate::types::Decision::Reject);
        assert!(report.result.winner.is_none());
    }

    #[tokio::test]
    async fn single_node_runner_uses_neutral_evaluator() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![node("node-c", GenerationStrategy::Creative)],
            Some(neutral_evaluator("neutral-evaluator")),
        )
        .expect("runner");

        let mut config = sample_config();
        config.min_candidates = 1;
        let report = runner.run(config).await.expect("run");

        assert_eq!(report.result.evaluations.len(), 1);
        assert!(report
            .result
            .aggregate_scores
            .values()
            .any(|score| *score > 0.0));
    }

    #[tokio::test]
    async fn multi_node_runner_cross_evaluates() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![
                node("node-a", GenerationStrategy::Conservative),
                node("node-c", GenerationStrategy::Creative),
            ],
            Some(neutral_evaluator("neutral-evaluator")),
        )
        .expect("runner");

        let mut config = sample_config();
        config.min_candidates = 2;
        let report = runner.run(config).await.expect("run");

        assert_eq!(report.result.candidates.len(), 2);
        assert_eq!(report.result.evaluations.len(), 2);
        assert!(report
            .result
            .evaluations
            .iter()
            .all(|evaluation| evaluation.evaluator_id != NodeId::from("neutral-evaluator")));
    }

    #[tokio::test]
    async fn sequential_mode_matches_parallel_placeholder_results() {
        let parallel_report = multi_node_runner(temp_path())
            .run(sample_config())
            .await
            .expect("parallel run");
        let mut sequential_config = sample_config();
        sequential_config.sequential = true;
        let sequential_report = multi_node_runner(temp_path())
            .run(sequential_config)
            .await
            .expect("sequential run");

        assert_eq!(
            parallel_report.result.decision,
            sequential_report.result.decision
        );
        assert_eq!(parallel_report.result.evaluations.len(), 6);
        assert_eq!(
            parallel_report.result.evaluations.len(),
            sequential_report.result.evaluations.len()
        );
        assert_eq!(
            candidate_outcomes(&parallel_report),
            candidate_outcomes(&sequential_report)
        );
    }

    fn candidate_outcomes(
        report: &ExperimentReport,
    ) -> Vec<(NodeId, GenerationStrategy, String, f64, bool)> {
        report
            .candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.node_id.clone(),
                    candidate.strategy.clone(),
                    candidate.approach.clone(),
                    candidate.aggregate_score,
                    candidate.is_winner,
                )
            })
            .collect()
    }

    fn multi_node_runner(path: PathBuf) -> ExperimentRunner {
        ExperimentRunner::with_nodes(
            path,
            vec![
                node("node-a", GenerationStrategy::Conservative),
                node("node-b", GenerationStrategy::Aggressive),
                node("node-c", GenerationStrategy::Creative),
            ],
            None,
        )
        .expect("runner")
    }

    fn node(node_id: &str, strategy: GenerationStrategy) -> NodeConfig {
        NodeConfig {
            node_id: NodeId::from(node_id),
            strategy: strategy.clone(),
            patch_source: Box::new(StaticPatchSource {
                patch: format!("diff --git a/{node_id} b/{node_id}"),
                approach: format!("{strategy:?} approach"),
                metrics: BTreeMap::from([
                    ("build_success".into(), 1.0),
                    ("test_pass_rate".into(), 1.0),
                    ("signal_resolution".into(), 1.0),
                ]),
            }),
            workspace: Box::new(PatchAwareWorkspace {
                current_patch: Mutex::new(String::new()),
            }),
        }
    }

    fn neutral_evaluator(node_id: &str) -> NeutralEvaluatorConfig {
        NeutralEvaluatorConfig {
            node_id: NodeId::from(node_id),
            workspace: Box::new(PatchAwareWorkspace {
                current_patch: Mutex::new(String::new()),
            }),
        }
    }

    fn failing_node(node_id: &str, strategy: GenerationStrategy) -> NodeConfig {
        NodeConfig {
            node_id: NodeId::from(node_id),
            strategy: strategy.clone(),
            patch_source: Box::new(StaticPatchSource {
                patch: format!("diff --git a/{node_id} b/{node_id}"),
                approach: format!("{strategy:?} approach"),
                metrics: BTreeMap::from([
                    ("build_success".into(), 1.0),
                    ("test_pass_rate".into(), 1.0),
                    ("signal_resolution".into(), 1.0),
                ]),
            }),
            workspace: Box::new(FailingWorkspace),
        }
    }

    fn sample_config() -> ExperimentConfig {
        ExperimentConfig {
            signal: Signal {
                id: Uuid::new_v4(),
                name: "signal".into(),
                description: "something is wrong".into(),
                severity: Severity::Medium,
            },
            hypothesis: "best candidate wins".into(),
            fitness_criteria: vec![
                FitnessCriterion {
                    name: "build_success".into(),
                    metric_type: MetricType::Higher,
                    weight: 0.2,
                },
                FitnessCriterion {
                    name: "test_pass_rate".into(),
                    metric_type: MetricType::Higher,
                    weight: 0.5,
                },
                FitnessCriterion {
                    name: "signal_resolution".into(),
                    metric_type: MetricType::Higher,
                    weight: 0.3,
                },
            ],
            scope: ModificationScope {
                allowed_files: vec![PathPattern::from("src/**/*.rs")],
                proposal_tier: ProposalTier::Tier1,
            },
            timeout: Duration::from_secs(30),
            min_candidates: 3,
            sequential: false,
        }
    }

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!("fx-consensus-runner-{}.json", Uuid::new_v4()))
    }

    // --- Auto-chain (run_loop) tests ---

    /// Workspace that produces an ACCEPT decision (all tests pass, signal resolved).
    /// check_signal returns false meaning "signal is no longer present" = resolved.
    struct AcceptingWorkspace;

    #[async_trait]
    impl EvaluationWorkspace for AcceptingWorkspace {
        async fn apply_patch(&self, _patch: &str) -> crate::error::Result<()> {
            Ok(())
        }
        async fn build(&self) -> crate::error::Result<()> {
            Ok(())
        }
        async fn test(&self) -> crate::error::Result<TestResult> {
            Ok(TestResult {
                passed: 5,
                failed: 0,
                total: 5,
            })
        }
        async fn check_signal(&self, _signal: &Signal) -> crate::error::Result<bool> {
            Ok(false) // signal gone = resolved
        }
        async fn check_regression(
            &self,
            _experiment: &crate::types::Experiment,
        ) -> crate::error::Result<bool> {
            Ok(false)
        }
        async fn reset(&self) -> crate::error::Result<()> {
            Ok(())
        }
    }

    /// Workspace where build fails (score 0.0 → REJECT).
    struct ZeroScoreWorkspace;

    #[async_trait]
    impl EvaluationWorkspace for ZeroScoreWorkspace {
        async fn apply_patch(&self, _patch: &str) -> crate::error::Result<()> {
            Ok(())
        }
        async fn build(&self) -> crate::error::Result<()> {
            Err(ConsensusError::BuildFailed("build failed".into()))
        }
        async fn test(&self) -> crate::error::Result<TestResult> {
            Err(ConsensusError::TestFailed {
                passed: 0,
                failed: 1,
                total: 1,
            })
        }
        async fn check_signal(&self, _signal: &Signal) -> crate::error::Result<bool> {
            Ok(false)
        }
        async fn check_regression(
            &self,
            _experiment: &crate::types::Experiment,
        ) -> crate::error::Result<bool> {
            Ok(false)
        }
        async fn reset(&self) -> crate::error::Result<()> {
            Ok(())
        }
    }

    /// Workspace that produces a partial score (tests pass, but signal not resolved → REJECT > 0).
    struct PartialScoreWorkspace;

    #[async_trait]
    impl EvaluationWorkspace for PartialScoreWorkspace {
        async fn apply_patch(&self, _patch: &str) -> crate::error::Result<()> {
            Ok(())
        }
        async fn build(&self) -> crate::error::Result<()> {
            Ok(())
        }
        async fn test(&self) -> crate::error::Result<TestResult> {
            Ok(TestResult {
                passed: 4,
                failed: 1,
                total: 5,
            })
        }
        async fn check_signal(&self, _signal: &Signal) -> crate::error::Result<bool> {
            Ok(false)
        }
        async fn check_regression(
            &self,
            _experiment: &crate::types::Experiment,
        ) -> crate::error::Result<bool> {
            Ok(false)
        }
        async fn reset(&self) -> crate::error::Result<()> {
            Ok(())
        }
    }

    fn accepting_node(node_id: &str) -> NodeConfig {
        NodeConfig {
            node_id: NodeId::from(node_id),
            strategy: GenerationStrategy::Conservative,
            patch_source: Box::new(StaticPatchSource {
                patch: format!("diff --git a/{node_id} b/{node_id}"),
                approach: "accepting approach".into(),
                metrics: BTreeMap::from([
                    ("build_success".into(), 1.0),
                    ("test_pass_rate".into(), 1.0),
                    ("signal_resolution".into(), 1.0),
                ]),
            }),
            workspace: Box::new(AcceptingWorkspace),
        }
    }

    fn zero_score_node(node_id: &str) -> NodeConfig {
        NodeConfig {
            node_id: NodeId::from(node_id),
            strategy: GenerationStrategy::Conservative,
            patch_source: Box::new(StaticPatchSource {
                patch: format!("diff --git a/{node_id} b/{node_id}"),
                approach: "zero score approach".into(),
                metrics: BTreeMap::from([
                    ("build_success".into(), 0.0),
                    ("test_pass_rate".into(), 0.0),
                    ("signal_resolution".into(), 0.0),
                ]),
            }),
            workspace: Box::new(ZeroScoreWorkspace),
        }
    }

    fn partial_score_node(node_id: &str) -> NodeConfig {
        NodeConfig {
            node_id: NodeId::from(node_id),
            strategy: GenerationStrategy::Conservative,
            patch_source: Box::new(StaticPatchSource {
                patch: format!("diff --git a/{node_id} b/{node_id}"),
                approach: "partial approach".into(),
                metrics: BTreeMap::from([
                    ("build_success".into(), 1.0),
                    ("test_pass_rate".into(), 0.8),
                    ("signal_resolution".into(), 0.0),
                ]),
            }),
            workspace: Box::new(PartialScoreWorkspace),
        }
    }

    fn single_node_config() -> ExperimentConfig {
        ExperimentConfig {
            signal: Signal {
                id: Uuid::new_v4(),
                name: "signal".into(),
                description: "something is wrong".into(),
                severity: Severity::Medium,
            },
            hypothesis: "best candidate wins".into(),
            fitness_criteria: vec![
                FitnessCriterion {
                    name: "build_success".into(),
                    metric_type: MetricType::Higher,
                    weight: 0.2,
                },
                FitnessCriterion {
                    name: "test_pass_rate".into(),
                    metric_type: MetricType::Higher,
                    weight: 0.5,
                },
                FitnessCriterion {
                    name: "signal_resolution".into(),
                    metric_type: MetricType::Higher,
                    weight: 0.3,
                },
            ],
            scope: ModificationScope {
                allowed_files: vec![PathPattern::from("src/**/*.rs")],
                proposal_tier: ProposalTier::Tier1,
            },
            timeout: Duration::from_secs(30),
            min_candidates: 1,
            sequential: false,
        }
    }

    fn accepting_neutral_evaluator() -> NeutralEvaluatorConfig {
        NeutralEvaluatorConfig {
            node_id: NodeId::from("neutral"),
            workspace: Box::new(AcceptingWorkspace),
        }
    }

    fn zero_score_neutral_evaluator() -> NeutralEvaluatorConfig {
        NeutralEvaluatorConfig {
            node_id: NodeId::from("neutral"),
            workspace: Box::new(ZeroScoreWorkspace),
        }
    }

    fn partial_score_neutral_evaluator() -> NeutralEvaluatorConfig {
        NeutralEvaluatorConfig {
            node_id: NodeId::from("neutral"),
            workspace: Box::new(PartialScoreWorkspace),
        }
    }

    struct ChainHistoryRoundBuilder;

    impl RoundNodesBuilder for ChainHistoryRoundBuilder {
        fn build_round_nodes(
            &self,
            chain_path: &Path,
            signal: &str,
        ) -> Result<RoundNodes, ConsensusError> {
            let chain_history = crate::load_chain_history_for_signal(chain_path, signal)?;
            Ok(RoundNodes {
                nodes: vec![NodeConfig {
                    node_id: NodeId::from("node-a"),
                    strategy: GenerationStrategy::Conservative,
                    patch_source: Box::new(StaticPatchSource {
                        patch: "diff --git a/node-a b/node-a".to_owned(),
                        approach: chain_history,
                        metrics: BTreeMap::from([
                            ("build_success".into(), 1.0),
                            ("test_pass_rate".into(), 0.8),
                            ("signal_resolution".into(), 0.0),
                        ]),
                    }),
                    workspace: Box::new(PartialScoreWorkspace),
                }],
                neutral_evaluator: Some(partial_score_neutral_evaluator()),
            })
        }
    }

    #[tokio::test]
    async fn run_loop_reloads_chain_history_between_rounds() {
        let path = temp_path();
        let runner = ExperimentRunner::with_round_nodes_builder(path, ChainHistoryRoundBuilder)
            .expect("runner");

        let result = runner
            .run_loop(single_node_config(), 2)
            .await
            .expect("run_loop");

        assert_eq!(result.rounds_completed, 2);
        assert!(result.reports[0].candidates[0]
            .approach
            .contains("No previous experiments recorded"));
        assert!(result.reports[1].candidates[0]
            .approach
            .contains("Entry #0"));
    }

    #[tokio::test]
    async fn run_loop_preserves_signal_id_across_rounds() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path.clone(),
            vec![partial_score_node("node-a")],
            Some(partial_score_neutral_evaluator()),
        )
        .expect("runner");
        let config = single_node_config();
        let signal_id = config.signal.id;

        runner.run_loop(config, 2).await.expect("run_loop");
        let chain = JsonFileChainStorage::new(&path).load().expect("load chain");

        assert_eq!(chain.entries().len(), 2);
        assert!(chain
            .entries()
            .iter()
            .all(|entry| entry.experiment.trigger.id == signal_id));
    }

    #[tokio::test]
    async fn run_loop_rejects_zero_rounds() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![accepting_node("node-a")],
            Some(accepting_neutral_evaluator()),
        )
        .expect("runner");

        let error = runner
            .run_loop(single_node_config(), 0)
            .await
            .expect_err("zero rounds should fail");

        assert_eq!(
            error.to_string(),
            "protocol error: max_rounds must be at least 1"
        );
    }

    #[tokio::test]
    async fn run_loop_stops_on_inconclusive_after_round_1() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(path, vec![accepting_node("node-a")], None)
            .expect("runner");

        let result = runner
            .run_loop(single_node_config(), 5)
            .await
            .expect("run_loop");

        assert_eq!(result.rounds_completed, 1);
        assert_eq!(result.reports[0].result.decision, Decision::Inconclusive);
    }

    #[tokio::test]
    async fn run_loop_stops_on_accept_after_round_1() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![accepting_node("node-a")],
            Some(accepting_neutral_evaluator()),
        )
        .expect("runner");

        let result = runner
            .run_loop(single_node_config(), 5)
            .await
            .expect("run_loop");

        assert_eq!(result.rounds_completed, 1);
        assert_eq!(result.max_rounds, 5);
        assert_eq!(result.reports.len(), 1);
        assert_eq!(
            result.reports[0].result.decision,
            crate::types::Decision::Accept
        );
    }

    #[tokio::test]
    async fn run_loop_stops_on_zero_score() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![zero_score_node("node-a")],
            Some(zero_score_neutral_evaluator()),
        )
        .expect("runner");

        let result = runner
            .run_loop(single_node_config(), 5)
            .await
            .expect("run_loop");

        assert_eq!(result.rounds_completed, 1);
        assert_eq!(
            result.reports[0].result.decision,
            crate::types::Decision::Reject
        );
    }

    #[tokio::test]
    async fn run_loop_continues_on_reject_with_positive_score() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![partial_score_node("node-a")],
            Some(partial_score_neutral_evaluator()),
        )
        .expect("runner");

        let result = runner
            .run_loop(single_node_config(), 3)
            .await
            .expect("run_loop");

        assert_eq!(result.rounds_completed, 3);
        assert_eq!(result.max_rounds, 3);
        assert_eq!(result.reports.len(), 3);
        for report in &result.reports {
            assert_eq!(report.result.decision, crate::types::Decision::Reject);
        }
    }

    #[tokio::test]
    async fn run_loop_stops_at_max_rounds() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![partial_score_node("node-a")],
            Some(partial_score_neutral_evaluator()),
        )
        .expect("runner");

        let result = runner
            .run_loop(single_node_config(), 2)
            .await
            .expect("run_loop");

        assert_eq!(result.rounds_completed, 2);
        assert_eq!(result.reports.len(), 2);
    }

    #[tokio::test]
    async fn run_loop_default_max_rounds_preserves_single_shot() {
        let path = temp_path();
        let runner = ExperimentRunner::with_nodes(
            path,
            vec![accepting_node("node-a")],
            Some(accepting_neutral_evaluator()),
        )
        .expect("runner");

        let result = runner
            .run_loop(single_node_config(), 1)
            .await
            .expect("run_loop");

        assert_eq!(result.rounds_completed, 1);
        assert_eq!(result.max_rounds, 1);
        assert_eq!(result.reports.len(), 1);
    }
}
