use crate::chain::{Chain, ChainStorage};
use crate::error::{ConsensusError, Result};
use crate::scoring::{compute_aggregate_scores, determine_winner};
use crate::types::{
    Candidate, ConsensusResult, Evaluation, Experiment, FitnessCriterion, ModificationScope,
    PathPattern, ProposalTier, Signal,
};
use async_trait::async_trait;
use chrono::Utc;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ExperimentConfig {
    pub signal: Signal,
    pub hypothesis: String,
    pub fitness_criteria: Vec<FitnessCriterion>,
    pub scope: ModificationScope,
    pub timeout: Duration,
    pub min_candidates: u32,
}

#[async_trait]
pub trait ConsensusProtocol: Send + Sync {
    async fn create_experiment(&self, config: ExperimentConfig) -> Result<Experiment>;
    async fn submit_candidate(&self, candidate: Candidate) -> Result<()>;
    async fn candidates(&self, experiment_id: Uuid) -> Result<Vec<Candidate>>;
    async fn submit_evaluation(&self, evaluation: Evaluation) -> Result<()>;
    async fn finalize(&self, experiment_id: Uuid) -> Result<ConsensusResult>;
    fn chain(&self) -> &Chain;
}

struct EngineState {
    experiments: HashMap<Uuid, Experiment>,
    candidates: HashMap<Uuid, Vec<Candidate>>,
    evaluations: HashMap<Uuid, Vec<Evaluation>>,
    finalized: HashMap<Uuid, ConsensusResult>,
    storage: Box<dyn ChainStorage>,
}

pub struct LocalConsensusEngine {
    state: RwLock<EngineState>,
    chain: UnsafeCell<Chain>,
}

// Safety: all mutable access to `chain` and the other state maps happens while holding
// `state`'s write lock.
unsafe impl Send for LocalConsensusEngine {}
unsafe impl Sync for LocalConsensusEngine {}

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
            }),
            chain: UnsafeCell::new(chain),
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

    fn chain_mut(&self) -> &mut Chain {
        // Safety: all mutation is serialized through `state` write locks.
        unsafe { &mut *self.chain.get() }
    }

    fn chain_ref(&self) -> &Chain {
        // Safety: `chain` is only updated under the engine lock and then exposed as a shared ref.
        unsafe { &*self.chain.get() }
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
        let experiment_id = find_experiment_id_for_candidate(&state, evaluation.candidate_id)?;
        ensure_open_experiment(&state, experiment_id)?;
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
        let result = ConsensusResult {
            experiment_id,
            winner,
            candidates: candidates.iter().map(|candidate| candidate.id).collect(),
            evaluations,
            aggregate_scores,
            decision,
            timestamp: Utc::now(),
        };
        let winning_patch = winner.and_then(|winner_id| find_patch(&candidates, winner_id));
        self.chain_mut()
            .append(experiment, result.clone(), winning_patch, None)?;
        state.storage.save(self.chain_ref())?;
        state.finalized.insert(experiment_id, result.clone());
        Ok(result)
    }

    fn chain(&self) -> &Chain {
        self.chain_ref()
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

fn find_experiment_id_for_candidate(state: &EngineState, candidate_id: Uuid) -> Result<Uuid> {
    state
        .candidates
        .iter()
        .find_map(|(experiment_id, candidates)| {
            candidates
                .iter()
                .any(|candidate| candidate.id == candidate_id)
                .then_some(*experiment_id)
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
