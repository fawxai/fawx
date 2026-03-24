use async_trait::async_trait;
use fx_consensus::{
    ConsensusError, EvaluationWorkspace, ExperimentConfig, ExperimentRunner, FitnessCriterion,
    GenerationStrategy, MetricType, ModificationScope, NeutralEvaluatorConfig, NodeConfig, NodeId,
    PatchResponse, PatchSource, PathPattern, ProgressEvent, ProposalTier, Severity, Signal,
    TestResult,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

struct StaticPatchSource;

#[async_trait]
impl PatchSource for StaticPatchSource {
    async fn generate_patch(
        &self,
        _system_prompt: &str,
        _experiment: &fx_consensus::Experiment,
    ) -> Result<PatchResponse, ConsensusError> {
        Ok(PatchResponse {
            patch: "diff --git a/src/lib.rs b/src/lib.rs".into(),
            approach: "accepting approach".into(),
            self_metrics: BTreeMap::from([
                ("build_success".into(), 1.0),
                ("test_pass_rate".into(), 1.0),
                ("signal_resolution".into(), 1.0),
            ]),
        })
    }
}

struct AcceptingWorkspace;

#[async_trait]
impl EvaluationWorkspace for AcceptingWorkspace {
    async fn apply_patch(&self, _patch: &str) -> Result<(), ConsensusError> {
        Ok(())
    }

    async fn build(&self) -> Result<(), ConsensusError> {
        Ok(())
    }

    async fn test(&self) -> Result<TestResult, ConsensusError> {
        Ok(TestResult {
            passed: 1,
            failed: 0,
            total: 1,
        })
    }

    async fn check_signal(&self, _signal: &Signal) -> Result<bool, ConsensusError> {
        Ok(false)
    }

    async fn check_regression(
        &self,
        _experiment: &fx_consensus::Experiment,
    ) -> Result<bool, ConsensusError> {
        Ok(false)
    }

    async fn reset(&self) -> Result<(), ConsensusError> {
        Ok(())
    }
}

#[tokio::test]
async fn experiment_runner_emits_progress_events_in_execution_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&events);
    let runner = ExperimentRunner::with_nodes(
        temp_path(),
        vec![node("node-a")],
        Some(neutral_evaluator("neutral")),
    )
    .expect("runner")
    .with_progress(Some(Arc::new(move |event: &ProgressEvent| {
        recorded.lock().expect("progress lock").push(event.clone());
    })));

    let report = runner.run(sample_config()).await.expect("run");
    let events = events.lock().expect("progress lock").clone();

    assert_eq!(report.chain_entry_index, 0);
    assert_eq!(
        events,
        vec![
            ProgressEvent::RoundStarted {
                round: 1,
                max_rounds: 1,
                signal: "signal".into(),
            },
            ProgressEvent::BaselineCollected {
                round: 1,
                max_rounds: 1,
                node_count: 1,
            },
            ProgressEvent::NodeStarted {
                round: 1,
                max_rounds: 1,
                node_id: NodeId::from("node-a"),
                strategy: GenerationStrategy::Conservative,
            },
            ProgressEvent::PatchGenerated {
                round: 1,
                max_rounds: 1,
                node_id: NodeId::from("node-a"),
            },
            ProgressEvent::BuildVerifying {
                round: 1,
                max_rounds: 1,
                node_id: NodeId::from("node-a"),
            },
            ProgressEvent::BuildResult {
                round: 1,
                max_rounds: 1,
                node_id: NodeId::from("node-a"),
                passed: 1,
                total: 1,
            },
            ProgressEvent::EvaluationStarted {
                round: 1,
                max_rounds: 1,
                node_id: NodeId::from("node-a"),
            },
            ProgressEvent::EvaluationComplete {
                round: 1,
                max_rounds: 1,
                node_id: NodeId::from("node-a"),
                evaluated: 1,
            },
            ProgressEvent::ScoringComplete {
                round: 1,
                max_rounds: 1,
                decision: fx_consensus::Decision::Accept,
                winner: Some(NodeId::from("node-a")),
            },
            ProgressEvent::ChainRecorded {
                round: 1,
                max_rounds: 1,
                entry_index: 0,
            },
            ProgressEvent::RoundComplete {
                round: 1,
                max_rounds: 1,
                decision: fx_consensus::Decision::Accept,
                continuing: false,
            },
        ]
    );
}

fn node(node_id: &str) -> NodeConfig {
    NodeConfig {
        node_id: NodeId::from(node_id),
        strategy: GenerationStrategy::Conservative,
        patch_source: Box::new(StaticPatchSource),
        workspace: Box::new(AcceptingWorkspace),
    }
}

fn neutral_evaluator(node_id: &str) -> NeutralEvaluatorConfig {
    NeutralEvaluatorConfig {
        node_id: NodeId::from(node_id),
        workspace: Box::new(AcceptingWorkspace),
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
            criterion("build_success", 0.2),
            criterion("test_pass_rate", 0.5),
            criterion("signal_resolution", 0.3),
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

fn criterion(name: &str, weight: f64) -> FitnessCriterion {
    FitnessCriterion {
        name: name.into(),
        metric_type: MetricType::Higher,
        weight,
    }
}

fn temp_path() -> PathBuf {
    std::env::temp_dir().join(format!("fx-consensus-progress-{}.json", Uuid::new_v4()))
}
