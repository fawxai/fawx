use crate::chain::JsonFileChainStorage;
use crate::error::ConsensusError;
use crate::evaluator::{BuildTestEvaluator, EvaluationWorkspace};
use crate::generator::{GenerationStrategy, LlmCandidateGenerator, PatchSource};
use crate::orchestrator::{
    evaluate_candidates, generate_candidates, node_strategy, CandidateEvaluator,
    CandidateGenerator, OrchestrationProgress,
};
use crate::protocol::{ConsensusProtocol, ExperimentConfig, LocalConsensusEngine};
use crate::types::{Candidate, ConsensusResult, Decision, Evaluation, NodeId};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type ProgressCallback = Arc<dyn Fn(&ProgressEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    RoundStarted {
        round: u32,
        max_rounds: u32,
        signal: String,
    },
    BaselineCollected {
        round: u32,
        max_rounds: u32,
        node_count: usize,
    },
    NodeStarted {
        round: u32,
        max_rounds: u32,
        node_id: NodeId,
        strategy: GenerationStrategy,
    },
    PatchGenerated {
        round: u32,
        max_rounds: u32,
        node_id: NodeId,
    },
    BuildVerifying {
        round: u32,
        max_rounds: u32,
        node_id: NodeId,
    },
    BuildResult {
        round: u32,
        max_rounds: u32,
        node_id: NodeId,
        passed: usize,
        total: usize,
    },
    EvaluationStarted {
        round: u32,
        max_rounds: u32,
        node_id: NodeId,
    },
    EvaluationComplete {
        round: u32,
        max_rounds: u32,
        node_id: NodeId,
        evaluated: usize,
    },
    ScoringComplete {
        round: u32,
        max_rounds: u32,
        decision: Decision,
        winner: Option<NodeId>,
    },
    RoundComplete {
        round: u32,
        max_rounds: u32,
        decision: Decision,
        continuing: bool,
    },
    ChainRecorded {
        round: u32,
        max_rounds: u32,
        entry_index: u64,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct RoundProgress {
    round: u32,
    max_rounds: u32,
}

impl RoundProgress {
    pub(crate) fn new(round: u32, max_rounds: u32) -> Self {
        Self { round, max_rounds }
    }

    pub(crate) fn single() -> Self {
        Self::new(1, 1)
    }

    pub(crate) fn round_started(self, signal: &str) -> ProgressEvent {
        ProgressEvent::RoundStarted {
            round: self.round,
            max_rounds: self.max_rounds,
            signal: signal.to_owned(),
        }
    }

    pub(crate) fn baseline_collected(self, node_count: usize) -> ProgressEvent {
        ProgressEvent::BaselineCollected {
            round: self.round,
            max_rounds: self.max_rounds,
            node_count,
        }
    }

    pub(crate) fn node_started(
        self,
        node_id: NodeId,
        strategy: GenerationStrategy,
    ) -> ProgressEvent {
        ProgressEvent::NodeStarted {
            round: self.round,
            max_rounds: self.max_rounds,
            node_id,
            strategy,
        }
    }

    pub(crate) fn patch_generated(self, node_id: NodeId) -> ProgressEvent {
        ProgressEvent::PatchGenerated {
            round: self.round,
            max_rounds: self.max_rounds,
            node_id,
        }
    }

    pub(crate) fn build_verifying(self, node_id: NodeId) -> ProgressEvent {
        ProgressEvent::BuildVerifying {
            round: self.round,
            max_rounds: self.max_rounds,
            node_id,
        }
    }

    pub(crate) fn build_result(
        self,
        node_id: NodeId,
        passed: usize,
        total: usize,
    ) -> ProgressEvent {
        ProgressEvent::BuildResult {
            round: self.round,
            max_rounds: self.max_rounds,
            node_id,
            passed,
            total,
        }
    }

    pub(crate) fn evaluation_started(self, node_id: NodeId) -> ProgressEvent {
        ProgressEvent::EvaluationStarted {
            round: self.round,
            max_rounds: self.max_rounds,
            node_id,
        }
    }

    pub(crate) fn evaluation_complete(self, node_id: NodeId, evaluated: usize) -> ProgressEvent {
        ProgressEvent::EvaluationComplete {
            round: self.round,
            max_rounds: self.max_rounds,
            node_id,
            evaluated,
        }
    }

    pub(crate) fn scoring_complete(self, result: &ConsensusResult) -> ProgressEvent {
        ProgressEvent::ScoringComplete {
            round: self.round,
            max_rounds: self.max_rounds,
            decision: result.decision.clone(),
            winner: winner_node(result),
        }
    }

    pub(crate) fn round_complete(self, decision: Decision, continuing: bool) -> ProgressEvent {
        ProgressEvent::RoundComplete {
            round: self.round,
            max_rounds: self.max_rounds,
            decision,
            continuing,
        }
    }

    pub(crate) fn chain_recorded(self, entry_index: u64) -> ProgressEvent {
        ProgressEvent::ChainRecorded {
            round: self.round,
            max_rounds: self.max_rounds,
            entry_index,
        }
    }
}

pub struct ExperimentRunner {
    engine: LocalConsensusEngine,
    storage_path: PathBuf,
    node_provider: NodeProvider,
    progress: Option<ProgressCallback>,
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
        plural_suffix(u64::from(result.rounds_completed)),
    )
}

pub fn plural_suffix(count: u64) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildSummary {
    pub passed: usize,
    pub total: usize,
}

impl BuildSummary {
    pub fn from_counts(passed: usize, total: usize) -> Self {
        Self { passed, total }
    }

    pub fn outcome(self) -> BuildOutcome {
        if self.total == 0 {
            return BuildOutcome::Skipped;
        }
        if self.passed == self.total {
            return BuildOutcome::Passed;
        }
        BuildOutcome::Failed {
            failed: self.total.saturating_sub(self.passed),
            total: self.total,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOutcome {
    Skipped,
    Passed,
    Failed { failed: usize, total: usize },
}

pub fn build_summary(evaluations: &[Evaluation]) -> BuildSummary {
    let passed = evaluations
        .iter()
        .filter(|evaluation| evaluation_build_success(evaluation))
        .count();
    BuildSummary::from_counts(passed, evaluations.len())
}

pub fn evaluation_build_success(evaluation: &Evaluation) -> bool {
    build_ok_note(&evaluation.notes).unwrap_or_else(|| {
        evaluation
            .fitness_scores
            .get("build_success")
            .copied()
            .unwrap_or(0.0)
            > 0.0
    })
}

fn build_ok_note(notes: &str) -> Option<bool> {
    note_value(notes, "build_ok").and_then(|value| match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

/// Extract a value from semicolon-separated `key=value` notes.
pub fn note_value<'a>(notes: &'a str, key: &str) -> Option<&'a str> {
    notes.split(';').map(str::trim).find_map(|segment| {
        let (name, value) = segment.split_once('=')?;
        if name.trim() == key {
            Some(value.trim())
        } else {
            None
        }
    })
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
            progress: None,
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
            progress: None,
        })
    }

    pub fn with_progress(mut self, progress: Option<ProgressCallback>) -> Self {
        self.progress = progress;
        self
    }

    pub async fn run(&self, config: ExperimentConfig) -> Result<ExperimentReport, ConsensusError> {
        let round = RoundProgress::single();
        self.emit(round.round_started(&config.signal.name));
        let report = self.run_round(config, round).await?;
        self.emit(round.round_complete(report.result.decision.clone(), false));
        Ok(report)
    }

    pub async fn run_loop(
        &self,
        config: ExperimentConfig,
        max_rounds: u32,
    ) -> Result<AutoChainResult, ConsensusError> {
        validate_auto_chain_rounds(max_rounds)?;
        let mut reports = Vec::new();
        for round_index in 1..=max_rounds {
            let round = RoundProgress::new(round_index, max_rounds);
            self.emit(round.round_started(&config.signal.name));
            let report = self.run_round(config.clone(), round).await?;
            let should_stop = report.should_stop_auto_chain();
            let continuing = !should_stop && round_index < max_rounds;
            self.emit(round.round_complete(report.result.decision.clone(), continuing));
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

    async fn run_round(
        &self,
        config: ExperimentConfig,
        round: RoundProgress,
    ) -> Result<ExperimentReport, ConsensusError> {
        let nodes = self.active_nodes(&config.signal.name)?;
        let progress = self
            .progress
            .as_ref()
            .map(|callback| OrchestrationProgress::new(round, nodes.strategies(), callback));
        self.emit(round.baseline_collected(nodes.generator_count()));
        let sequential = config.sequential;
        let experiment = self.engine.create_experiment(config).await?;
        let candidates = generate_candidates(
            &self.engine,
            &experiment,
            nodes.generators(),
            sequential,
            progress,
        )
        .await?;
        evaluate_candidates(
            &self.engine,
            &experiment,
            &candidates,
            nodes.evaluators(),
            sequential,
            progress,
        )
        .await?;
        let result = self.engine.finalize(experiment.id).await?;
        self.emit(round.scoring_complete(&result));
        let chain_entry_index = latest_chain_entry_index(&self.engine)?;
        self.emit(round.chain_recorded(chain_entry_index));
        build_report(&self.engine, nodes.strategies(), result, chain_entry_index).await
    }

    fn emit(&self, event: ProgressEvent) {
        emit_progress(self.progress.as_ref(), event);
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

    fn generator_count(&self) -> usize {
        self.generators().len()
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

fn emit_progress(progress: Option<&ProgressCallback>, event: ProgressEvent) {
    if let Some(callback) = progress {
        callback(&event);
    }
}

fn latest_chain_entry_index(engine: &LocalConsensusEngine) -> Result<u64, ConsensusError> {
    engine
        .chain()?
        .head()
        .map(|entry| entry.index)
        .ok_or_else(|| ConsensusError::Protocol("missing chain entry after experiment".into()))
}

fn winner_node(result: &ConsensusResult) -> Option<NodeId> {
    result
        .winner
        .and_then(|winner| result.candidate_nodes.get(&winner).cloned())
}

async fn build_report(
    engine: &LocalConsensusEngine,
    strategies: &BTreeMap<NodeId, GenerationStrategy>,
    result: ConsensusResult,
    chain_entry_index: u64,
) -> Result<ExperimentReport, ConsensusError> {
    let candidates = engine.candidates(result.experiment_id).await?;
    Ok(ExperimentReport {
        chain_entry_index,
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
            strategy: node_strategy(strategies, &candidate.node_id),
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
    use chrono::Utc;
    use std::sync::Mutex;
    use std::time::Duration;
    use uuid::Uuid;

    fn sample_progress_result(decision: Decision, winner: Option<&str>) -> ConsensusResult {
        let candidate_id = Uuid::new_v4();
        ConsensusResult {
            experiment_id: Uuid::new_v4(),
            winner: winner.map(|_| candidate_id),
            candidates: vec![candidate_id],
            candidate_nodes: BTreeMap::from([(candidate_id, NodeId::from("node-a"))]),
            candidate_patches: BTreeMap::new(),
            evaluations: Vec::new(),
            aggregate_scores: BTreeMap::from([(candidate_id, 1.0)]),
            decision,
            timestamp: Utc::now(),
        }
    }

    fn sample_build_evaluation(notes: &str, build_success: Option<f64>) -> Evaluation {
        let mut fitness_scores = BTreeMap::new();
        if let Some(score) = build_success {
            fitness_scores.insert("build_success".into(), score);
        }
        Evaluation {
            candidate_id: Uuid::new_v4(),
            evaluator_id: NodeId::from("node-b"),
            fitness_scores,
            safety_pass: true,
            signal_resolved: false,
            regression_detected: false,
            notes: notes.to_owned(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn round_progress_builders_cover_round_setup_events() {
        let round = RoundProgress::new(2, 4);

        assert_eq!(
            round.round_started("signal"),
            ProgressEvent::RoundStarted {
                round: 2,
                max_rounds: 4,
                signal: "signal".into(),
            }
        );
        assert_eq!(
            round.baseline_collected(3),
            ProgressEvent::BaselineCollected {
                round: 2,
                max_rounds: 4,
                node_count: 3,
            }
        );
    }

    #[test]
    fn round_progress_builders_cover_node_workflow_events() {
        let round = RoundProgress::new(2, 4);
        let node_id = NodeId::from("node-a");

        assert_eq!(
            round.node_started(node_id.clone(), GenerationStrategy::Creative),
            ProgressEvent::NodeStarted {
                round: 2,
                max_rounds: 4,
                node_id: node_id.clone(),
                strategy: GenerationStrategy::Creative,
            }
        );
        assert_eq!(
            round.patch_generated(node_id.clone()),
            ProgressEvent::PatchGenerated {
                round: 2,
                max_rounds: 4,
                node_id: node_id.clone(),
            }
        );
        assert_eq!(
            round.build_verifying(node_id.clone()),
            ProgressEvent::BuildVerifying {
                round: 2,
                max_rounds: 4,
                node_id: node_id.clone(),
            }
        );
        assert_eq!(
            round.build_result(node_id, 1, 2),
            ProgressEvent::BuildResult {
                round: 2,
                max_rounds: 4,
                node_id: NodeId::from("node-a"),
                passed: 1,
                total: 2,
            }
        );
    }

    #[test]
    fn round_progress_builders_cover_completion_events() {
        let round = RoundProgress::new(2, 4);
        let node_id = NodeId::from("node-a");
        let result = sample_progress_result(Decision::Accept, Some("node-a"));

        assert_eq!(
            round.evaluation_started(node_id.clone()),
            ProgressEvent::EvaluationStarted {
                round: 2,
                max_rounds: 4,
                node_id: node_id.clone(),
            }
        );
        assert_eq!(
            round.evaluation_complete(node_id.clone(), 1),
            ProgressEvent::EvaluationComplete {
                round: 2,
                max_rounds: 4,
                node_id,
                evaluated: 1,
            }
        );
        assert_eq!(
            round.scoring_complete(&result),
            ProgressEvent::ScoringComplete {
                round: 2,
                max_rounds: 4,
                decision: Decision::Accept,
                winner: Some(NodeId::from("node-a")),
            }
        );
        assert_eq!(
            round.round_complete(Decision::Reject, true),
            ProgressEvent::RoundComplete {
                round: 2,
                max_rounds: 4,
                decision: Decision::Reject,
                continuing: true,
            }
        );
        assert_eq!(
            round.chain_recorded(9),
            ProgressEvent::ChainRecorded {
                round: 2,
                max_rounds: 4,
                entry_index: 9,
            }
        );
    }

    #[test]
    fn build_summary_returns_zero_counts_when_evaluations_are_empty() {
        assert_eq!(
            build_summary(&[]),
            BuildSummary {
                passed: 0,
                total: 0,
            }
        );
    }

    #[test]
    fn evaluation_build_success_prefers_build_ok_note_over_metric() {
        let evaluation = sample_build_evaluation("build_ok=false", Some(1.0));

        assert!(!evaluation_build_success(&evaluation));
    }

    #[test]
    fn evaluation_build_success_falls_back_to_metric_for_missing_or_invalid_notes() {
        let from_metric = sample_build_evaluation("tests=3/3", Some(1.0));
        let malformed_note = sample_build_evaluation("build_ok=maybe", Some(0.0));
        let missing_metric = sample_build_evaluation("tests=0/0", None);

        assert!(evaluation_build_success(&from_metric));
        assert!(!evaluation_build_success(&malformed_note));
        assert!(!evaluation_build_success(&missing_metric));
    }

    #[test]
    fn build_summary_counts_build_success_from_notes_and_metrics() {
        let evaluations = vec![
            sample_build_evaluation("build_ok=true", None),
            sample_build_evaluation("tests=1/1", Some(1.0)),
            sample_build_evaluation("build_ok=false", Some(1.0)),
        ];

        assert_eq!(
            build_summary(&evaluations),
            BuildSummary {
                passed: 2,
                total: 3,
            }
        );
    }

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
