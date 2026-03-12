use crate::chain::{Chain, ChainStorage};
use crate::error::{ConsensusError, Result};
use crate::scoring::{compute_aggregate_scores, determine_winner};
use crate::types::{
    Candidate, ConsensusResult, Evaluation, Experiment, FitnessCriterion, ModificationScope,
    NodeId, PathPattern, ProposalTier, Signal,
};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;
use std::time::Duration;
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentConfig {
    pub signal: Signal,
    pub hypothesis: String,
    pub fitness_criteria: Vec<FitnessCriterion>,
    pub scope: ModificationScope,
    #[serde(with = "duration_serde")]
    pub timeout: Duration,
    pub min_candidates: u32,
    #[serde(default)]
    pub sequential: bool,
}

#[async_trait]
pub trait ConsensusProtocol: Send + Sync {
    async fn create_experiment(&self, config: ExperimentConfig) -> Result<Experiment>;
    async fn submit_candidate(&self, candidate: Candidate) -> Result<()>;
    async fn candidates(&self, experiment_id: Uuid) -> Result<Vec<Candidate>>;
    async fn submit_evaluation(&self, evaluation: Evaluation) -> Result<()>;
    async fn finalize(&self, experiment_id: Uuid) -> Result<ConsensusResult>;
    fn chain(&self) -> Result<Chain>;
}

struct EngineState {
    experiments: HashMap<Uuid, Experiment>,
    candidates: HashMap<Uuid, Vec<Candidate>>,
    evaluations: HashMap<Uuid, Vec<Evaluation>>,
    finalized: HashMap<Uuid, ConsensusResult>,
    storage: Box<dyn ChainStorage>,
    chain: Chain,
}

pub struct LocalConsensusEngine {
    state: RwLock<EngineState>,
}

impl LocalConsensusEngine {
    pub fn new(storage: Box<dyn ChainStorage>) -> Result<Self> {
        let chain = storage.load()?;
        chain.verify()?;
        Ok(Self {
            state: RwLock::new(EngineState {
                experiments: HashMap::new(),
                candidates: HashMap::new(),
                evaluations: HashMap::new(),
                finalized: HashMap::new(),
                storage,
                chain,
            }),
        })
    }

    fn validate_config(config: &ExperimentConfig) -> Result<()> {
        if config.hypothesis.trim().is_empty() {
            return Err(ConsensusError::InvalidExperiment(
                "hypothesis must not be empty".into(),
            ));
        }
        if config.fitness_criteria.is_empty() {
            return Err(ConsensusError::InvalidExperiment(
                "fitness criteria must not be empty".into(),
            ));
        }
        if config.min_candidates == 0 {
            return Err(ConsensusError::InvalidExperiment(
                "min_candidates must be greater than zero".into(),
            ));
        }
        if config.timeout.is_zero() {
            return Err(ConsensusError::InvalidExperiment(
                "timeout must be greater than zero".into(),
            ));
        }
        validate_scope(&config.scope)
    }
}

#[async_trait]
impl ConsensusProtocol for LocalConsensusEngine {
    async fn create_experiment(&self, config: ExperimentConfig) -> Result<Experiment> {
        Self::validate_config(&config)?;
        let experiment = Experiment {
            id: Uuid::new_v4(),
            trigger: config.signal,
            hypothesis: config.hypothesis,
            fitness_criteria: config.fitness_criteria,
            scope: config.scope,
            timeout: config.timeout,
            min_candidates: config.min_candidates,
            created_at: Utc::now(),
        };
        let mut state = lock_write(&self.state)?;
        state.experiments.insert(experiment.id, experiment.clone());
        Ok(experiment)
    }

    async fn submit_candidate(&self, candidate: Candidate) -> Result<()> {
        let mut state = lock_write(&self.state)?;
        ensure_open_experiment(&state, candidate.experiment_id)?;
        state
            .candidates
            .entry(candidate.experiment_id)
            .or_default()
            .push(candidate);
        Ok(())
    }

    async fn candidates(&self, experiment_id: Uuid) -> Result<Vec<Candidate>> {
        let state = lock_read(&self.state)?;
        ensure_experiment_exists(&state, experiment_id)?;
        Ok(state
            .candidates
            .get(&experiment_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn submit_evaluation(&self, evaluation: Evaluation) -> Result<()> {
        let mut state = lock_write(&self.state)?;
        let (experiment_id, candidate_node_id) =
            find_candidate_context(&state, evaluation.candidate_id)?;
        ensure_open_experiment(&state, experiment_id)?;
        if evaluation.evaluator_id == candidate_node_id {
            warn!(
                candidate_id = %evaluation.candidate_id,
                evaluator_id = %evaluation.evaluator_id.0,
                "skipping self-evaluation submission"
            );
            return Ok(());
        }
        state
            .evaluations
            .entry(experiment_id)
            .or_default()
            .push(evaluation);
        Ok(())
    }

    async fn finalize(&self, experiment_id: Uuid) -> Result<ConsensusResult> {
        let mut state = lock_write(&self.state)?;
        let experiment = get_open_experiment(&state, experiment_id)?.clone();
        let candidates = state
            .candidates
            .get(&experiment_id)
            .cloned()
            .unwrap_or_default();
        ensure_candidate_count(&experiment, &candidates)?;
        let evaluations = state
            .evaluations
            .get(&experiment_id)
            .cloned()
            .unwrap_or_default();
        let aggregate_scores =
            compute_aggregate_scores(&candidates, &evaluations, &experiment.fitness_criteria);
        let (decision, winner) = determine_winner(&aggregate_scores, &evaluations);
        let candidate_patches: BTreeMap<Uuid, String> = candidates
            .iter()
            .map(|candidate| (candidate.id, candidate.patch.clone()))
            .collect();
        let result = ConsensusResult {
            experiment_id,
            winner,
            candidates: candidates.iter().map(|candidate| candidate.id).collect(),
            candidate_nodes: candidates
                .iter()
                .map(|candidate| (candidate.id, candidate.node_id.clone()))
                .collect(),
            candidate_patches,
            evaluations,
            aggregate_scores,
            decision,
            timestamp: Utc::now(),
        };
        let winning_patch = winner.and_then(|winner_id| find_patch(&candidates, winner_id));
        state
            .chain
            .append(experiment, result.clone(), winning_patch, None)?;
        state.storage.save(&state.chain)?;
        state.finalized.insert(experiment_id, result.clone());
        Ok(result)
    }

    fn chain(&self) -> Result<Chain> {
        Ok(lock_read(&self.state)?.chain.clone())
    }
}

fn validate_scope(scope: &ModificationScope) -> Result<()> {
    if scope.allowed_files.is_empty() {
        return Err(ConsensusError::InvalidExperiment(
            "scope must include allowed files".into(),
        ));
    }
    validate_proposal_tier(&scope.proposal_tier)?;
    validate_paths(&scope.allowed_files)
}

fn validate_proposal_tier(tier: &ProposalTier) -> Result<()> {
    match tier {
        ProposalTier::Tier1 | ProposalTier::Tier2 => Ok(()),
    }
}

fn validate_paths(paths: &[PathPattern]) -> Result<()> {
    if paths.iter().any(|path| path.0.trim().is_empty()) {
        return Err(ConsensusError::InvalidExperiment(
            "scope paths must not be empty".into(),
        ));
    }
    Ok(())
}

fn lock_read<T>(lock: &RwLock<T>) -> Result<std::sync::RwLockReadGuard<'_, T>> {
    lock.read()
        .map_err(|_| ConsensusError::Protocol("engine read lock poisoned".into()))
}

fn lock_write<T>(lock: &RwLock<T>) -> Result<std::sync::RwLockWriteGuard<'_, T>> {
    lock.write()
        .map_err(|_| ConsensusError::Protocol("engine write lock poisoned".into()))
}

fn ensure_experiment_exists(state: &EngineState, experiment_id: Uuid) -> Result<()> {
    if state.experiments.contains_key(&experiment_id) {
        return Ok(());
    }
    Err(ConsensusError::ExperimentNotFound(experiment_id))
}

fn ensure_open_experiment(state: &EngineState, experiment_id: Uuid) -> Result<()> {
    ensure_experiment_exists(state, experiment_id)?;
    if state.finalized.contains_key(&experiment_id) {
        return Err(ConsensusError::ExperimentAlreadyFinalized(experiment_id));
    }
    Ok(())
}

fn get_open_experiment(state: &EngineState, experiment_id: Uuid) -> Result<&Experiment> {
    ensure_open_experiment(state, experiment_id)?;
    state
        .experiments
        .get(&experiment_id)
        .ok_or(ConsensusError::ExperimentNotFound(experiment_id))
}

fn find_candidate_context(state: &EngineState, candidate_id: Uuid) -> Result<(Uuid, NodeId)> {
    state
        .candidates
        .iter()
        .find_map(|(experiment_id, candidates)| {
            candidates
                .iter()
                .find(|candidate| candidate.id == candidate_id)
                .map(|candidate| (*experiment_id, candidate.node_id.clone()))
        })
        .ok_or(ConsensusError::CandidateNotFound(candidate_id))
}

fn ensure_candidate_count(experiment: &Experiment, candidates: &[Candidate]) -> Result<()> {
    let received = candidates.len() as u32;
    if received >= experiment.min_candidates {
        return Ok(());
    }
    Err(ConsensusError::InsufficientCandidates {
        required: experiment.min_candidates,
        received,
    })
}

fn find_patch(candidates: &[Candidate], winner_id: Uuid) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| candidate.id == winner_id)
        .map(|candidate| candidate.patch.clone())
}

mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    #[derive(Serialize, Deserialize)]
    struct DurationRepr {
        secs: u64,
        nanos: u32,
    }

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        DurationRepr {
            secs: duration.as_secs(),
            nanos: duration.subsec_nanos(),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let repr = DurationRepr::deserialize(deserializer)?;
        Ok(Duration::new(repr.secs, repr.nanos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::JsonFileChainStorage;
    use crate::types::tests::{sample_candidate, sample_evaluation, sample_experiment};

    #[tokio::test]
    async fn submit_evaluation_skips_self_evaluation() {
        let engine = LocalConsensusEngine::new(Box::new(JsonFileChainStorage::new(temp_path())))
            .expect("engine");
        let experiment = engine
            .create_experiment(sample_config(1))
            .await
            .expect("experiment");
        let candidate = sample_candidate(experiment.id, "node-a");
        engine
            .submit_candidate(candidate.clone())
            .await
            .expect("candidate");

        engine
            .submit_evaluation(sample_evaluation(candidate.id, "node-a", 0.8))
            .await
            .expect("self evaluation should be ignored");

        let result = engine.finalize(experiment.id).await.expect("finalize");
        assert!(result.evaluations.is_empty());
        assert_eq!(result.decision, crate::types::Decision::Inconclusive);
    }

    #[test]
    fn experiment_config_defaults_sequential_to_false() {
        let experiment = sample_experiment();
        let config: ExperimentConfig = serde_json::from_value(serde_json::json!({
            "signal": experiment.trigger,
            "hypothesis": experiment.hypothesis,
            "fitness_criteria": experiment.fitness_criteria,
            "scope": experiment.scope,
            "timeout": {"secs": experiment.timeout.as_secs(), "nanos": 0},
            "min_candidates": 1
        }))
        .expect("deserialize config");

        assert!(!config.sequential);
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
            sequential: false,
        }
    }

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("fx-consensus-protocol-{}.json", Uuid::new_v4()))
    }
}
