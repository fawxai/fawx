use crate::error::Result;
use crate::protocol::{ConsensusProtocol, ExperimentConfig};
use crate::types::{Candidate, ConsensusResult, Evaluation, Experiment, NodeId};
use async_trait::async_trait;
use futures::future::try_join_all;

pub struct ExperimentOrchestrator<'a, E: ConsensusProtocol> {
    engine: &'a E,
}

impl<'a, E: ConsensusProtocol> ExperimentOrchestrator<'a, E> {
    pub fn new(engine: &'a E) -> Self {
        Self { engine }
    }

    pub async fn run_experiment(
        &self,
        config: ExperimentConfig,
        generators: &[Box<dyn CandidateGenerator>],
        evaluators: &[Box<dyn CandidateEvaluator>],
    ) -> Result<ConsensusResult> {
        let experiment = self.engine.create_experiment(config).await?;
        let candidates = generate_candidates(self.engine, &experiment, generators).await?;
        evaluate_candidates(self.engine, &experiment, &candidates, evaluators).await?;
        self.engine.finalize(experiment.id).await
    }
}

#[async_trait]
pub trait CandidateGenerator: Send + Sync {
    async fn generate(&self, experiment: &Experiment) -> Result<Candidate>;
    fn node_id(&self) -> &NodeId;
}

#[async_trait]
pub trait CandidateEvaluator: Send + Sync {
    async fn evaluate(&self, experiment: &Experiment, candidate: &Candidate) -> Result<Evaluation>;
    fn node_id(&self) -> &NodeId;
}

async fn generate_candidates<E: ConsensusProtocol>(
    engine: &E,
    experiment: &Experiment,
    generators: &[Box<dyn CandidateGenerator>],
) -> Result<Vec<Candidate>> {
    let candidates = try_join_all(
        generators
            .iter()
            .map(|generator| async move { generator.generate(experiment).await }),
    )
    .await?;
    submit_candidates(engine, &candidates).await?;
    Ok(candidates)
}

async fn evaluate_candidates<E: ConsensusProtocol>(
    engine: &E,
    experiment: &Experiment,
    candidates: &[Candidate],
    evaluators: &[Box<dyn CandidateEvaluator>],
) -> Result<()> {
    for candidate in candidates {
        let evaluations = generate_evaluations(experiment, candidate, evaluators).await?;
        submit_evaluations(engine, &evaluations).await?;
    }
    Ok(())
}

async fn generate_evaluations(
    experiment: &Experiment,
    candidate: &Candidate,
    evaluators: &[Box<dyn CandidateEvaluator>],
) -> Result<Vec<Evaluation>> {
    try_join_all(
        evaluators
            .iter()
            .filter(|evaluator| evaluator.node_id() != &candidate.node_id)
            .map(|evaluator| async move { evaluator.evaluate(experiment, candidate).await }),
    )
    .await
}

async fn submit_candidates<E: ConsensusProtocol>(
    engine: &E,
    candidates: &[Candidate],
) -> Result<()> {
    for candidate in candidates {
        engine.submit_candidate(candidate.clone()).await?;
    }
    Ok(())
}

async fn submit_evaluations<E: ConsensusProtocol>(
    engine: &E,
    evaluations: &[Evaluation],
) -> Result<()> {
    for evaluation in evaluations {
        engine.submit_evaluation(evaluation.clone()).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::JsonFileChainStorage;
    use crate::protocol::LocalConsensusEngine;
    use crate::types::tests::{sample_candidate, sample_evaluation, sample_experiment};
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    struct StaticEvaluator {
        node_id: NodeId,
        calls: Arc<Mutex<Vec<NodeId>>>,
    }

    #[async_trait]
    impl CandidateEvaluator for StaticEvaluator {
        async fn evaluate(
            &self,
            _experiment: &Experiment,
            candidate: &Candidate,
        ) -> Result<Evaluation> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(candidate.node_id.clone());
            Ok(sample_evaluation(candidate.id, &self.node_id.0, 1.0))
        }

        fn node_id(&self) -> &NodeId {
            &self.node_id
        }
    }

    struct StaticGenerator {
        node_id: NodeId,
    }

    #[async_trait]
    impl CandidateGenerator for StaticGenerator {
        async fn generate(&self, experiment: &Experiment) -> Result<Candidate> {
            Ok(Candidate {
                id: Uuid::new_v4(),
                experiment_id: experiment.id,
                node_id: self.node_id.clone(),
                patch: format!("patch-{}", self.node_id.0),
                approach: "approach".into(),
                self_metrics: BTreeMap::new(),
                created_at: Utc::now(),
            })
        }

        fn node_id(&self) -> &NodeId {
            &self.node_id
        }
    }

    #[tokio::test]
    async fn generate_evaluations_skips_self_eval() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let evaluators: Vec<Box<dyn CandidateEvaluator>> = vec![
            Box::new(StaticEvaluator {
                node_id: NodeId::from("node-a"),
                calls: Arc::clone(&calls),
            }),
            Box::new(StaticEvaluator {
                node_id: NodeId::from("node-b"),
                calls,
            }),
        ];

        let evaluations = generate_evaluations(&experiment, &candidate, &evaluators)
            .await
            .expect("evaluations");

        assert_eq!(evaluations.len(), 1);
        assert_eq!(evaluations[0].evaluator_id, NodeId::from("node-b"));
    }

    #[tokio::test]
    async fn cross_evaluation_works_with_multiple_nodes() {
        let engine = LocalConsensusEngine::new(Box::new(JsonFileChainStorage::new(temp_path())))
            .expect("engine");
        let orchestrator = ExperimentOrchestrator::new(&engine);
        let generators: Vec<Box<dyn CandidateGenerator>> = vec![
            Box::new(StaticGenerator {
                node_id: NodeId::from("node-a"),
            }),
            Box::new(StaticGenerator {
                node_id: NodeId::from("node-b"),
            }),
        ];
        let calls = Arc::new(Mutex::new(Vec::new()));
        let evaluators: Vec<Box<dyn CandidateEvaluator>> = vec![
            Box::new(StaticEvaluator {
                node_id: NodeId::from("node-a"),
                calls: Arc::clone(&calls),
            }),
            Box::new(StaticEvaluator {
                node_id: NodeId::from("node-b"),
                calls,
            }),
        ];

        let result = orchestrator
            .run_experiment(sample_config(2), &generators, &evaluators)
            .await
            .expect("run experiment");

        assert_eq!(result.candidates.len(), 2);
        assert_eq!(result.evaluations.len(), 2);
        for evaluation in &result.evaluations {
            let candidate_node = result
                .candidate_nodes
                .get(&evaluation.candidate_id)
                .expect("candidate node");
            assert_ne!(candidate_node, &evaluation.evaluator_id);
        }
    }

    fn sample_config(min_candidates: u32) -> ExperimentConfig {
        let experiment = sample_experiment();
        ExperimentConfig {
            signal: experiment.trigger,
            hypothesis: experiment.hypothesis,
            fitness_criteria: experiment.fitness_criteria,
            scope: experiment.scope,
            timeout: experiment.timeout,
            min_candidates,
        }
    }

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("fx-consensus-orchestrator-{}.json", Uuid::new_v4()))
    }
}
