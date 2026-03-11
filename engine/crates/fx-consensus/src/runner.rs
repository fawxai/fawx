use crate::chain::JsonFileChainStorage;
use crate::error::ConsensusError;
use crate::evaluator::{BuildTestEvaluator, EvaluationWorkspace};
use crate::generator::{GenerationStrategy, LlmCandidateGenerator, PatchSource};
use crate::orchestrator::{CandidateEvaluator, CandidateGenerator, ExperimentOrchestrator};
use crate::protocol::{ConsensusProtocol, ExperimentConfig, LocalConsensusEngine};
use crate::types::{Candidate, ConsensusResult, NodeId};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct ExperimentRunner {
    engine: LocalConsensusEngine,
    generators: Vec<Box<dyn CandidateGenerator>>,
    evaluators: Vec<Box<dyn CandidateEvaluator>>,
    strategies: BTreeMap<NodeId, GenerationStrategy>,
}

pub struct NodeConfig {
    pub node_id: NodeId,
    pub strategy: GenerationStrategy,
    pub patch_source: Box<dyn PatchSource>,
    pub workspace: Box<dyn EvaluationWorkspace>,
}

pub struct ExperimentReport {
    pub result: ConsensusResult,
    pub chain_entry_index: u64,
    pub candidates: Vec<CandidateReport>,
}

pub struct CandidateReport {
    pub node_id: NodeId,
    pub strategy: GenerationStrategy,
    pub approach: String,
    pub aggregate_score: f64,
    pub is_winner: bool,
}

impl ExperimentRunner {
    pub fn new(storage_path: PathBuf) -> Result<Self, ConsensusError> {
        Self::with_nodes(storage_path, Vec::new())
    }

    pub fn with_nodes(
        storage_path: PathBuf,
        nodes: Vec<NodeConfig>,
    ) -> Result<Self, ConsensusError> {
        let engine = LocalConsensusEngine::new(Box::new(JsonFileChainStorage::new(storage_path)))?;
        let BuiltNodes {
            generators,
            evaluators,
            strategies,
        } = build_nodes(nodes);
        Ok(Self {
            engine,
            generators,
            evaluators,
            strategies,
        })
    }

    pub async fn run(&self, config: ExperimentConfig) -> Result<ExperimentReport, ConsensusError> {
        let orchestrator = ExperimentOrchestrator::new(&self.engine);
        let result = orchestrator
            .run_experiment(config, &self.generators, &self.evaluators)
            .await?;
        build_report(&self.engine, &self.strategies, result).await
    }
}

struct BuiltNodes {
    generators: Vec<Box<dyn CandidateGenerator>>,
    evaluators: Vec<Box<dyn CandidateEvaluator>>,
    strategies: BTreeMap<NodeId, GenerationStrategy>,
}

fn build_nodes(nodes: Vec<NodeConfig>) -> BuiltNodes {
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
    use crate::evaluator::TestResult;
    use crate::generator::PatchResponse;
    use crate::types::{
        FitnessCriterion, MetricType, ModificationScope, PathPattern, ProposalTier, Severity,
        Signal,
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
        )
        .expect("runner");

        let report = runner.run(sample_config()).await.expect("run");

        assert_eq!(report.result.decision, crate::types::Decision::Reject);
        assert!(report.result.winner.is_none());
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
        }
    }

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!("fx-consensus-runner-{}.json", Uuid::new_v4()))
    }
}
