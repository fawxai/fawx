use crate::error::{ConsensusError, Result};
use crate::types::{ConsensusResult, Experiment};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const GENESIS_HASH: &str = "genesis";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEntry {
    pub index: u64,
    pub previous_hash: String,
    pub experiment: Experiment,
    pub result: ConsensusResult,
    pub winning_patch: Option<String>,
    pub applied_at: Option<DateTime<Utc>>,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chain {
    entries: Vec<ChainEntry>,
    head_hash: String,
}

pub trait ChainStorage {
    fn load(&self) -> Result<Chain>;
    fn save(&self, chain: &Chain) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct JsonFileChainStorage {
    path: PathBuf,
}

impl JsonFileChainStorage {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl ChainStorage for JsonFileChainStorage {
    fn load(&self) -> Result<Chain> {
        if !self.path.exists() {
            return Ok(Chain::new());
        }
        let json = fs::read_to_string(&self.path)
            .map_err(|error| ConsensusError::Storage(error.to_string()))?;
        serde_json::from_str(&json).map_err(|error| ConsensusError::Storage(error.to_string()))
    }

    fn save(&self, chain: &Chain) -> Result<()> {
        let json = serde_json::to_string_pretty(chain)
            .map_err(|error| ConsensusError::Storage(error.to_string()))?;
        fs::write(&self.path, json).map_err(|error| ConsensusError::Storage(error.to_string()))
    }
}

impl Default for Chain {
    fn default() -> Self {
        Self::new()
    }
}

impl Chain {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            head_hash: GENESIS_HASH.into(),
        }
    }

    pub fn append(
        &mut self,
        experiment: Experiment,
        result: ConsensusResult,
        winning_patch: Option<String>,
    ) -> Result<()> {
        let entry = self.build_entry(experiment, result, winning_patch)?;
        self.head_hash = entry.hash.clone();
        self.entries.push(entry);
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        let mut previous_hash = GENESIS_HASH.to_string();
        for (index, entry) in self.entries.iter().enumerate() {
            verify_entry_link(entry, index, &previous_hash)?;
            verify_entry_hash(entry, index)?;
            previous_hash = entry.hash.clone();
        }
        if self.head_hash != previous_hash {
            return Err(ConsensusError::ChainIntegrity {
                index: self.entries.len(),
                message: "head hash mismatch".into(),
            });
        }
        Ok(())
    }

    pub fn entries(&self) -> &[ChainEntry] {
        &self.entries
    }

    pub fn head(&self) -> Option<&ChainEntry> {
        self.entries.last()
    }

    fn build_entry(
        &self,
        experiment: Experiment,
        result: ConsensusResult,
        winning_patch: Option<String>,
    ) -> Result<ChainEntry> {
        let index = self.entries.len() as u64;
        let applied_at = winning_patch.as_ref().map(|_| Utc::now());
        let hash = compute_hash(
            index,
            &self.head_hash,
            &experiment,
            &result,
            &winning_patch,
            &applied_at,
        )?;
        Ok(ChainEntry {
            index,
            previous_hash: self.head_hash.clone(),
            experiment,
            result,
            winning_patch,
            applied_at,
            hash,
        })
    }
}

fn verify_entry_link(entry: &ChainEntry, index: usize, previous_hash: &str) -> Result<()> {
    if entry.index != index as u64 || entry.previous_hash != previous_hash {
        return Err(ConsensusError::ChainIntegrity {
            index,
            message: "previous hash link mismatch".into(),
        });
    }
    Ok(())
}

fn verify_entry_hash(entry: &ChainEntry, index: usize) -> Result<()> {
    let expected = compute_hash(
        entry.index,
        &entry.previous_hash,
        &entry.experiment,
        &entry.result,
        &entry.winning_patch,
        &entry.applied_at,
    )?;
    if entry.hash != expected {
        return Err(ConsensusError::ChainIntegrity {
            index,
            message: "entry hash mismatch".into(),
        });
    }
    Ok(())
}

fn compute_hash(
    index: u64,
    previous_hash: &str,
    experiment: &Experiment,
    result: &ConsensusResult,
    winning_patch: &Option<String>,
    applied_at: &Option<DateTime<Utc>>,
) -> Result<String> {
    let payload = serde_json::json!({
        "index": index,
        "previous_hash": previous_hash,
        "experiment": experiment,
        "result": result,
        "winning_patch": winning_patch,
        "applied_at": applied_at,
    });
    let encoded =
        serde_json::to_vec(&payload).map_err(|error| ConsensusError::Storage(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tests::{sample_candidate, sample_evaluation, sample_experiment};
    use crate::types::{ConsensusResult, Decision};
    use chrono::Utc;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[test]
    fn creates_empty_chain_with_genesis_hash() {
        let chain = Chain::new();
        assert!(chain.entries().is_empty());
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn appends_entry_and_verifies_chain() {
        let mut chain = Chain::new();
        let experiment = sample_experiment();
        let result = sample_result(experiment.id);

        chain
            .append(experiment, result, Some("diff --git".into()))
            .expect("append succeeds");

        assert_eq!(chain.entries().len(), 1);
        assert!(chain.head().is_some());
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn fails_verification_when_entry_is_tampered() {
        let mut chain = Chain::new();
        let experiment = sample_experiment();
        let result = sample_result(experiment.id);
        chain
            .append(experiment, result, None)
            .expect("append succeeds");
        chain.entries[0].winning_patch = Some("tampered".into());

        let error = chain.verify().expect_err("verification should fail");

        assert!(matches!(
            error,
            ConsensusError::ChainIntegrity { index: 0, .. }
        ));
    }

    #[test]
    fn saves_and_loads_chain_round_trip() {
        let dir = tempdir().expect("create tempdir");
        let storage = JsonFileChainStorage::new(dir.path().join("chain.json"));
        let mut chain = Chain::new();
        let experiment = sample_experiment();
        let result = sample_result(experiment.id);
        chain
            .append(experiment, result, None)
            .expect("append succeeds");

        storage.save(&chain).expect("save succeeds");
        let loaded = storage.load().expect("load succeeds");

        assert_eq!(loaded.entries().len(), 1);
        assert!(loaded.verify().is_ok());
    }

    fn sample_result(experiment_id: uuid::Uuid) -> ConsensusResult {
        let candidate = sample_candidate(experiment_id, "node-a");
        ConsensusResult {
            experiment_id,
            winner: Some(candidate.id),
            candidates: vec![candidate.id],
            evaluations: vec![sample_evaluation(candidate.id, "node-b", 0.7)],
            aggregate_scores: BTreeMap::from([(candidate.id, 0.7)]),
            decision: Decision::Accept,
            timestamp: Utc::now(),
        }
    }
}
