use crate::error::Result;
use crate::generator::GenerationStrategy;
use crate::protocol::{ConsensusProtocol, ExperimentConfig};
use crate::runner::{build_summary, ProgressCallback, ProgressEvent, RoundProgress};
use crate::types::{Candidate, ConsensusResult, Evaluation, Experiment, NodeId};
use async_trait::async_trait;
use futures::future::try_join_all;
use std::collections::BTreeMap;
use tracing::warn;

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
        let sequential = config.sequential;
        let experiment = self.engine.create_experiment(config).await?;
        let candidates =
            generate_candidates(self.engine, &experiment, generators, sequential, None).await?;
        evaluate_candidates(
            self.engine,
            &experiment,
            &candidates,
            evaluators,
            sequential,
            None,
        )
        .await?;
        self.engine.finalize(experiment.id).await
    }
}

#[derive(Clone, Copy)]
pub(crate) struct OrchestrationProgress<'a> {
    round: RoundProgress,
    strategies: &'a BTreeMap<NodeId, GenerationStrategy>,
    callback: &'a ProgressCallback,
}

impl<'a> OrchestrationProgress<'a> {
    pub(crate) fn new(
        round: RoundProgress,
        strategies: &'a BTreeMap<NodeId, GenerationStrategy>,
        callback: &'a ProgressCallback,
    ) -> Self {
        Self {
            round,
            strategies,
            callback,
        }
    }

    fn emit(self, event: ProgressEvent) {
        (self.callback)(&event);
    }

    fn node_started(self, node_id: &NodeId) {
        let strategy = node_strategy(self.strategies, node_id);
        self.emit(self.round.node_started(node_id.clone(), strategy));
    }

    fn patch_generated(self, node_id: &NodeId) {
        self.emit(self.round.patch_generated(node_id.clone()));
    }

    fn build_verifying(self, node_id: &NodeId) {
        self.emit(self.round.build_verifying(node_id.clone()));
    }

    fn build_result(self, node_id: &NodeId, evaluations: &[Evaluation]) {
        let build = build_summary(evaluations);
        self.emit(
            self.round
                .build_result(node_id.clone(), build.passed, build.total),
        );
    }

    fn evaluation_started(self, node_id: &NodeId) {
        self.emit(self.round.evaluation_started(node_id.clone()));
    }

    fn evaluation_complete(self, node_id: &NodeId, evaluated: usize) {
        self.emit(self.round.evaluation_complete(node_id.clone(), evaluated));
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

pub(crate) async fn generate_candidates<E>(
    engine: &E,
    experiment: &Experiment,
    generators: &[Box<dyn CandidateGenerator>],
    sequential: bool,
    progress: Option<OrchestrationProgress<'_>>,
) -> Result<Vec<Candidate>>
where
    E: ConsensusProtocol,
{
    let candidates = if sequential {
        generate_candidates_sequential(experiment, generators, progress).await?
    } else {
        generate_candidates_parallel(experiment, generators, progress).await?
    };
    submit_candidates(engine, &candidates).await?;
    Ok(candidates)
}

async fn generate_candidates_parallel(
    experiment: &Experiment,
    generators: &[Box<dyn CandidateGenerator>],
    progress: Option<OrchestrationProgress<'_>>,
) -> Result<Vec<Candidate>> {
    try_join_all(generators.iter().map(|generator| async move {
        generate_candidate(generator.as_ref(), experiment, progress).await
    }))
    .await
}

async fn generate_candidates_sequential(
    experiment: &Experiment,
    generators: &[Box<dyn CandidateGenerator>],
    progress: Option<OrchestrationProgress<'_>>,
) -> Result<Vec<Candidate>> {
    let mut candidates = Vec::with_capacity(generators.len());
    for generator in generators {
        candidates.push(generate_candidate(generator.as_ref(), experiment, progress).await?);
    }
    Ok(candidates)
}

async fn generate_candidate(
    generator: &dyn CandidateGenerator,
    experiment: &Experiment,
    progress: Option<OrchestrationProgress<'_>>,
) -> Result<Candidate> {
    let node_id = generator.node_id().clone();
    if let Some(progress) = progress {
        progress.node_started(&node_id);
    }
    let candidate = generator.generate(experiment).await?;
    if let Some(progress) = progress {
        progress.patch_generated(&node_id);
    }
    Ok(candidate)
}

pub(crate) async fn evaluate_candidates<E>(
    engine: &E,
    experiment: &Experiment,
    candidates: &[Candidate],
    evaluators: &[Box<dyn CandidateEvaluator>],
    sequential: bool,
    progress: Option<OrchestrationProgress<'_>>,
) -> Result<()>
where
    E: ConsensusProtocol,
{
    for candidate in candidates {
        let node_id = candidate.node_id.clone();
        if let Some(progress) = progress {
            progress.build_verifying(&node_id);
        }
        let evaluations =
            generate_evaluations(experiment, candidate, evaluators, sequential).await?;
        if let Some(progress) = progress {
            progress.build_result(&node_id, &evaluations);
            progress.evaluation_started(&node_id);
        }
        submit_evaluations(engine, &evaluations).await?;
        if let Some(progress) = progress {
            progress.evaluation_complete(&node_id, evaluations.len());
        }
    }
    Ok(())
}

async fn generate_evaluations(
    experiment: &Experiment,
    candidate: &Candidate,
    evaluators: &[Box<dyn CandidateEvaluator>],
    sequential: bool,
) -> Result<Vec<Evaluation>> {
    if sequential {
        generate_evaluations_sequential(experiment, candidate, evaluators).await
    } else {
        generate_evaluations_parallel(experiment, candidate, evaluators).await
    }
}

async fn generate_evaluations_parallel(
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

async fn generate_evaluations_sequential(
    experiment: &Experiment,
    candidate: &Candidate,
    evaluators: &[Box<dyn CandidateEvaluator>],
) -> Result<Vec<Evaluation>> {
    let mut evaluations = Vec::with_capacity(evaluators.len());
    for evaluator in evaluators {
        if evaluator.node_id() == &candidate.node_id {
            continue;
        }
        evaluations.push(evaluator.evaluate(experiment, candidate).await?);
    }
    Ok(evaluations)
}

async fn submit_candidates<E>(engine: &E, candidates: &[Candidate]) -> Result<()>
where
    E: ConsensusProtocol,
{
    for candidate in candidates {
        engine.submit_candidate(candidate.clone()).await?;
    }
    Ok(())
}

async fn submit_evaluations<E>(engine: &E, evaluations: &[Evaluation]) -> Result<()>
where
    E: ConsensusProtocol,
{
    for evaluation in evaluations {
        engine.submit_evaluation(evaluation.clone()).await?;
    }
    Ok(())
}

pub(crate) fn node_strategy(
    strategies: &BTreeMap<NodeId, GenerationStrategy>,
    node_id: &NodeId,
) -> GenerationStrategy {
    strategies.get(node_id).cloned().unwrap_or_else(|| {
        warn!(
            "Unknown node_id {:?} not found in strategies map, defaulting to Conservative",
            node_id
        );
        GenerationStrategy::Conservative
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_strategy_returns_default_for_unknown_node() {
        let strategies = BTreeMap::new();
        let node_id = NodeId::from("unknown-node");
        let strategy = node_strategy(&strategies, &node_id);
        assert_eq!(strategy, GenerationStrategy::Conservative);
    }

    #[test]
    fn node_strategy_returns_mapped_strategy() {
        let mut strategies = BTreeMap::new();
        let node_id = NodeId::from("node-0");
        strategies.insert(node_id.clone(), GenerationStrategy::Aggressive);
        assert_eq!(
            node_strategy(&strategies, &node_id),
            GenerationStrategy::Aggressive
        );
    }
}
