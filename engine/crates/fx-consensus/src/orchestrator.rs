use crate::error::Result;
use crate::protocol::{ConsensusProtocol, ExperimentConfig};
use crate::types::{Candidate, ConsensusResult, Evaluation, Experiment, NodeId};
use async_trait::async_trait;

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
    let mut candidates = Vec::with_capacity(generators.len());
    for generator in generators {
        let candidate = generator.generate(experiment).await?;
        engine.submit_candidate(candidate.clone()).await?;
        candidates.push(candidate);
    }
    Ok(candidates)
}

async fn evaluate_candidates<E: ConsensusProtocol>(
    engine: &E,
    experiment: &Experiment,
    candidates: &[Candidate],
    evaluators: Vec<Box<dyn CandidateEvaluator>>,
) -> Result<()> {
    for candidate in candidates {
        submit_candidate_evaluations(engine, experiment, candidate, &evaluators).await?;
    }
    Ok(())
}

async fn submit_candidate_evaluations<E: ConsensusProtocol>(
    engine: &E,
    experiment: &Experiment,
    candidate: &Candidate,
    evaluators: &[Box<dyn CandidateEvaluator>],
) -> Result<()> {
    for evaluator in evaluators {
        if evaluator.node_id() == &candidate.node_id {
            continue;
        }
        let evaluation = evaluator.evaluate(experiment, candidate).await?;
        engine.submit_evaluation(evaluation).await?;
    }
    Ok(())
}
