use crate::error::{ConsensusError, Result};
use crate::types::{ConsensusResult, Experiment};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const GENESIS_HASH: &str = "genesis";

struct HashInput<'a> {
    index: u64,
    previous_hash: &'a str,
    experiment: &'a Experiment,
    result: &'a ConsensusResult,
    winning_patch: &'a Option<String>,
    applied_at: &'a Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainEntry {
    pub index: u64,
    pub previous_hash: String,
    pub experiment: Experiment,
    pub result: ConsensusResult,
    pub winning_patch: Option<String>,
    pub applied_at: Option<DateTime<Utc>>,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    entries: Vec<ChainEntry>,
    head_hash: String,
}

pub trait ChainStorage: Send + Sync {
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
        match serde_json::from_str(&json) {
            Ok(chain) => Ok(chain),
            Err(error) => {
                tracing::warn!(
                    path = %self.path.display(),
                    %error,
                    "corrupt chain file, starting fresh"
                );
                Ok(Chain::new())
            }
        }
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
        applied_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let entry = self.build_entry(experiment, result, winning_patch, applied_at)?;
        self.head_hash = entry.hash.clone();
        self.entries.push(entry);
        Ok(())
    }

    #[must_use = "chain verification must be checked to detect tampering"]
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

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[ChainEntry] {
        &self.entries
    }

    pub fn head(&self) -> Option<&ChainEntry> {
        self.entries.last()
    }

    pub fn recent_entries_for_signal(&self, signal_name: &str, limit: usize) -> Vec<ChainEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|entry| signal_matches(&entry.experiment.trigger.name, signal_name))
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    fn build_entry(
        &self,
        experiment: Experiment,
        result: ConsensusResult,
        winning_patch: Option<String>,
        applied_at: Option<DateTime<Utc>>,
    ) -> Result<ChainEntry> {
        let index = self.entries.len() as u64;
        let hash = compute_hash(&HashInput {
            index,
            previous_hash: &self.head_hash,
            experiment: &experiment,
            result: &result,
            winning_patch: &winning_patch,
            applied_at: &applied_at,
        })?;
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

fn signal_matches(entry_signal: &str, requested_signal: &str) -> bool {
    entry_signal
        .trim()
        .eq_ignore_ascii_case(requested_signal.trim())
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
    let input = HashInput {
        index: entry.index,
        previous_hash: &entry.previous_hash,
        experiment: &entry.experiment,
        result: &entry.result,
        winning_patch: &entry.winning_patch,
        applied_at: &entry.applied_at,
    };
    let expected = compute_hash(&input)?;
    if entry.hash == expected {
        return Ok(());
    }
    // Accept entries hashed with the pre-canonical serializer
    let legacy = compute_hash_legacy(&input)?;
    if entry.hash == legacy {
        return Ok(());
    }
    Err(ConsensusError::ChainIntegrity {
        index,
        message: "entry hash mismatch".into(),
    })
}

fn compute_hash(input: &HashInput<'_>) -> Result<String> {
    let payload = hash_payload(input);
    let canonical = canonicalize_value(&payload);
    let encoded = serde_json::to_vec(&canonical)
        .map_err(|error| ConsensusError::Storage(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

/// Accepts entries written before canonical serialization was introduced.
fn compute_hash_legacy(input: &HashInput<'_>) -> Result<String> {
    let payload = hash_payload(input);
    let encoded =
        serde_json::to_vec(&payload).map_err(|error| ConsensusError::Storage(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn hash_payload(input: &HashInput<'_>) -> serde_json::Value {
    serde_json::json!({
        "index": input.index,
        "previous_hash": input.previous_hash,
        "experiment": input.experiment,
        "result": input.result,
        "winning_patch": input.winning_patch,
        "applied_at": input.applied_at,
    })
}

/// Recursively sort all object keys so serialization is deterministic
/// regardless of struct field declaration order or serde version.
fn canonicalize_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), canonicalize_value(v)))
                .collect();
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(canonicalize_value).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tests::{sample_candidate, sample_evaluation, sample_experiment};
    use crate::types::{ConsensusResult, Decision, Experiment};
    use chrono::Utc;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[test]
    fn creates_empty_chain_with_genesis_hash() {
        let chain = Chain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn appends_entry_and_verifies_chain() {
        let mut chain = Chain::new();
        let experiment = sample_experiment();
        let result = sample_result(experiment.id);
        let applied_at = Some(Utc::now());

        chain
            .append(experiment, result, Some("diff --git".into()), applied_at)
            .expect("append succeeds");

        assert_eq!(chain.len(), 1);
        assert!(chain.head().is_some());
        assert_eq!(chain.head().and_then(|entry| entry.applied_at), applied_at);
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn verifies_multi_entry_chain_and_detects_middle_entry_tampering() {
        let mut chain = Chain::new();
        for _ in 0..3 {
            let experiment = sample_experiment();
            let result = sample_result(experiment.id);
            chain
                .append(experiment, result, None, None)
                .expect("append succeeds");
        }

        assert!(chain.verify().is_ok());
        chain.entries[1].winning_patch = Some("tampered".into());

        let error = chain.verify().expect_err("verification should fail");

        assert!(matches!(
            error,
            ConsensusError::ChainIntegrity { index: 1, .. }
        ));
    }

    #[test]
    fn fails_verification_when_entry_is_tampered() {
        let mut chain = Chain::new();
        let experiment = sample_experiment();
        let result = sample_result(experiment.id);
        chain
            .append(experiment, result, None, None)
            .expect("append succeeds");
        chain.entries[0].winning_patch = Some("tampered".into());

        let error = chain.verify().expect_err("verification should fail");

        assert!(matches!(
            error,
            ConsensusError::ChainIntegrity { index: 0, .. }
        ));
    }

    #[test]
    fn loading_nonexistent_storage_path_returns_empty_chain() {
        let dir = tempdir().expect("create tempdir");
        let storage = JsonFileChainStorage::new(dir.path().join("missing-chain.json"));

        let chain = storage.load().expect("load succeeds");

        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn saves_and_loads_chain_round_trip() {
        let dir = tempdir().expect("create tempdir");
        let storage = JsonFileChainStorage::new(dir.path().join("chain.json"));
        let mut chain = Chain::new();
        let experiment = sample_experiment();
        let result = sample_result(experiment.id);
        chain
            .append(experiment, result, None, None)
            .expect("append succeeds");

        storage.save(&chain).expect("save succeeds");
        let loaded = storage.load().expect("load succeeds");

        assert_eq!(loaded.len(), 1);
        assert!(loaded.verify().is_ok());
    }

    #[test]
    fn recent_entries_for_signal_filters_matches_and_preserves_order() {
        let mut chain = Chain::new();
        chain
            .append(
                experiment_with_signal("latency"),
                sample_result(uuid::Uuid::new_v4()),
                None,
                None,
            )
            .expect("append succeeds");
        chain
            .append(
                experiment_with_signal("throughput"),
                sample_result(uuid::Uuid::new_v4()),
                None,
                None,
            )
            .expect("append succeeds");
        chain
            .append(
                experiment_with_signal("Latency"),
                sample_result(uuid::Uuid::new_v4()),
                None,
                None,
            )
            .expect("append succeeds");

        let entries = chain.recent_entries_for_signal(" latency ", 2);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 0);
        assert_eq!(entries[1].index, 2);
        assert!(entries.iter().all(|entry| entry
            .experiment
            .trigger
            .name
            .eq_ignore_ascii_case("latency")));
    }

    #[test]
    fn recent_entries_for_signal_respects_limit() {
        let mut chain = Chain::new();
        for _ in 0..3 {
            chain
                .append(
                    experiment_with_signal("latency"),
                    sample_result(uuid::Uuid::new_v4()),
                    None,
                    None,
                )
                .expect("append succeeds");
        }

        let entries = chain.recent_entries_for_signal("latency", 1);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].index, 2);
    }

    #[test]
    fn deserializes_legacy_chain_entry_without_candidate_patches() {
        let experiment_id = uuid::Uuid::nil();
        let candidate_id = uuid::Uuid::from_u128(1);
        let signal_id = uuid::Uuid::from_u128(2);
        let candidate_node = "node-a";
        let evaluator_node = "node-b";
        let timestamp = "2026-03-11T00:00:00Z";
        let candidate_nodes = BTreeMap::from([(candidate_id.to_string(), candidate_node)]);
        let aggregate_scores = BTreeMap::from([(candidate_id.to_string(), 0.7)]);
        let legacy_entry = serde_json::json!({
            "index": 0,
            "previous_hash": "genesis",
            "experiment": {
                "id": experiment_id,
                "trigger": {
                    "id": signal_id,
                    "name": "latency",
                    "description": "High latency detected",
                    "severity": "high"
                },
                "hypothesis": "parallelism helps",
                "fitness_criteria": [{
                    "name": "latency",
                    "metric_type": "Lower",
                    "weight": 1.0
                }],
                "scope": {
                    "allowed_files": ["src/**/*.rs"],
                    "proposal_tier": "Tier1"
                },
                "timeout": {"secs": 300, "nanos": 0},
                "min_candidates": 2,
                "created_at": timestamp
            },
            "result": {
                "experiment_id": experiment_id,
                "winner": candidate_id,
                "candidates": [candidate_id],
                "candidate_nodes": candidate_nodes,
                "evaluations": [{
                    "candidate_id": candidate_id,
                    "evaluator_id": evaluator_node,
                    "fitness_scores": {"latency": 0.7},
                    "safety_pass": true,
                    "signal_resolved": true,
                    "regression_detected": false,
                    "notes": "Looks good",
                    "created_at": timestamp
                }],
                "aggregate_scores": aggregate_scores,
                "decision": "Accept",
                "timestamp": timestamp
            },
            "winning_patch": null,
            "applied_at": null,
            "hash": "legacy-hash"
        });
        let legacy_chain = serde_json::json!({
            "entries": [legacy_entry],
            "head_hash": "legacy-hash"
        });

        let chain: Chain =
            serde_json::from_value(legacy_chain).expect("legacy chain should deserialize");

        assert_eq!(chain.len(), 1);
        assert!(chain.entries()[0].result.candidate_patches.is_empty());
    }

    fn experiment_with_signal(signal: &str) -> Experiment {
        let mut experiment = sample_experiment();
        experiment.trigger.name = signal.to_owned();
        experiment
    }

    fn sample_result(experiment_id: uuid::Uuid) -> ConsensusResult {
        let candidate = sample_candidate(experiment_id, "node-a");
        ConsensusResult {
            experiment_id,
            winner: Some(candidate.id),
            candidates: vec![candidate.id],
            candidate_nodes: BTreeMap::from([(candidate.id, candidate.node_id.clone())]),
            candidate_patches: BTreeMap::from([(candidate.id, candidate.patch.clone())]),
            evaluations: vec![sample_evaluation(candidate.id, "node-b", 0.7)],
            aggregate_scores: BTreeMap::from([(candidate.id, 0.7)]),
            decision: Decision::Accept,
            timestamp: Utc::now(),
        }
    }
}
