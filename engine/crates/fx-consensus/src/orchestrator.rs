use crate::error::Result;
use crate::protocol::{ConsensusProtocol, ExperimentConfig};
use crate::types::{Candidate, ConsensusResult, Evaluation, Experiment, NodeId};
use async_trait::async_trait;
use futures::future::try_join_all;

pub struct ExperimentOrchestrator<E: ConsensusProtocol> {
    engine: E,
}

impl<E: ConsensusProtocol> ExperimentOrchestrator<E> {
    pub fn new(engine: E) -> Self {
        Self { engine }
    }

    pub async fn run_experiment(
        &self,
        config: ExperimentConfig,
        generators: Vec<Box<dyn CandidateGenerator>>,
        evaluators: Vec<Box<dyn CandidateEvaluator>>,
    ) -> Result<ConsensusResult> {
        let experiment = self.engine.create_experiment(config).await?;
        let candidates = generate_candidates(&self.engine, &experiment, generators).await?;
        evaluate_candidates(&self.engine, &experiment, &candidates, evaluators).await?;
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
    generators: Vec<Box<dyn CandidateGenerator>>,
) -> Result<Vec<Candidate>> {
    let candidates = try_join_all(
        generators
            .into_iter()
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
    evaluators: Vec<Box<dyn CandidateEvaluator>>,
) -> Result<()> {
    for candidate in candidates {
        let evaluations = generate_evaluations(experiment, candidate, &evaluators).await?;
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
