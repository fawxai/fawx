use anyhow::{anyhow, Context};
use clap::Subcommand;
use fx_consensus::{
    Chain, ChainStorage, ExperimentConfig, ExperimentRunner, FitnessCriterion,
    JsonFileChainStorage, MetricType, ModificationScope, PathPattern, ProposalTier, Signal,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

mod format;
mod placeholders;

use format::{format_chain_entries, format_chain_entry, format_experiment_report};
use placeholders::build_nodes;

const CHAIN_PATH_ENV: &str = "FAWX_CONSENSUS_CHAIN_PATH";

#[derive(Subcommand)]
pub enum ExperimentCommands {
    /// Create and run a new experiment
    Run {
        /// Signal name that triggered this experiment
        #[arg(long)]
        signal: String,

        /// Hypothesis to test
        #[arg(long)]
        hypothesis: String,

        /// Number of competing nodes (default: 3)
        #[arg(long, default_value = "3")]
        nodes: u32,

        /// Files allowed to be modified (glob patterns, comma-separated)
        #[arg(long, default_value = "src/**/*.rs")]
        scope: String,

        /// Timeout per node in seconds (default: 120)
        #[arg(long, default_value = "120")]
        timeout: u64,
    },

    /// View the consensus chain
    Chain {
        /// Number of recent entries to show (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Show details of a specific chain entry
    Show {
        /// Chain entry index
        index: u64,
    },

    /// Verify chain integrity
    Verify,
}

pub async fn run(command: ExperimentCommands) -> anyhow::Result<String> {
    match command {
        ExperimentCommands::Run {
            signal,
            hypothesis,
            nodes,
            scope,
            timeout,
        } => {
            run_experiment(RunExperimentArgs {
                signal,
                hypothesis,
                nodes,
                scope,
                timeout,
            })
            .await
        }
        ExperimentCommands::Chain { limit } => show_chain(limit),
        ExperimentCommands::Show { index } => show_entry(index),
        ExperimentCommands::Verify => verify_chain(),
    }
}

pub struct RunExperimentArgs {
    pub signal: String,
    pub hypothesis: String,
    pub nodes: u32,
    pub scope: String,
    pub timeout: u64,
}

pub async fn run_experiment(args: RunExperimentArgs) -> anyhow::Result<String> {
    run_experiment_with_path(args, consensus_chain_path()).await
}

async fn run_experiment_with_path(
    args: RunExperimentArgs,
    chain_path: PathBuf,
) -> anyhow::Result<String> {
    ensure_chain_parent_dir(&chain_path)?;
    let runner = ExperimentRunner::with_nodes(chain_path, build_nodes(args.nodes))?;
    let report = runner.run(build_config(&args)?).await?;
    Ok(format_experiment_report(&args, &report))
}

fn build_config(args: &RunExperimentArgs) -> anyhow::Result<ExperimentConfig> {
    Ok(ExperimentConfig {
        signal: build_signal(&args.signal),
        hypothesis: args.hypothesis.clone(),
        fitness_criteria: default_fitness_criteria(),
        scope: build_scope(&args.scope),
        timeout: Duration::from_secs(args.timeout),
        min_candidates: args.nodes,
    })
}

fn build_signal(name: &str) -> Signal {
    Signal {
        id: Uuid::new_v4(),
        name: name.to_owned(),
        description: format!("Experiment triggered by signal '{name}'"),
        severity: fx_consensus::Severity::Medium,
    }
}

fn default_fitness_criteria() -> Vec<FitnessCriterion> {
    vec![
        criterion("build_success", MetricType::Higher, 0.2),
        criterion("test_pass_rate", MetricType::Higher, 0.5),
        criterion("signal_resolution", MetricType::Higher, 0.3),
    ]
}

fn criterion(name: &str, metric_type: MetricType, weight: f64) -> FitnessCriterion {
    FitnessCriterion {
        name: name.to_owned(),
        metric_type,
        weight,
    }
}

fn build_scope(scope: &str) -> ModificationScope {
    ModificationScope {
        allowed_files: parse_scope(scope),
        proposal_tier: ProposalTier::Tier1,
    }
}

fn parse_scope(scope: &str) -> Vec<PathPattern> {
    scope
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathPattern::from)
        .collect()
}

fn show_chain(limit: usize) -> anyhow::Result<String> {
    show_chain_at(consensus_chain_path(), limit)
}

fn show_chain_at(path: PathBuf, limit: usize) -> anyhow::Result<String> {
    let chain = load_chain(path)?;
    if chain.is_empty() {
        return Ok("No experiments recorded yet".to_owned());
    }
    Ok(format_chain_entries(&chain, limit))
}

fn show_entry(index: u64) -> anyhow::Result<String> {
    show_entry_at(consensus_chain_path(), index)
}

fn show_entry_at(path: PathBuf, index: u64) -> anyhow::Result<String> {
    let chain = load_chain(path)?;
    let entry = chain
        .entries()
        .iter()
        .find(|entry| entry.index == index)
        .ok_or_else(|| anyhow!("Chain entry #{index} not found"))?;
    Ok(format_chain_entry(entry))
}

fn verify_chain() -> anyhow::Result<String> {
    verify_chain_at(consensus_chain_path())
}

fn verify_chain_at(path: PathBuf) -> anyhow::Result<String> {
    let chain = load_chain(path)?;
    chain.verify().map_err(anyhow::Error::from)?;
    Ok(format!(
        "Chain integrity verified: {} entries, all hashes valid",
        chain.len()
    ))
}

fn load_chain(path: PathBuf) -> anyhow::Result<Chain> {
    let storage = JsonFileChainStorage::new(path);
    storage.load().map_err(anyhow::Error::from)
}

pub(crate) fn consensus_chain_path() -> PathBuf {
    if let Ok(path) = std::env::var(CHAIN_PATH_ENV) {
        return PathBuf::from(path);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".fawx")
        .join("consensus")
        .join("chain.json")
}

fn ensure_chain_parent_dir(path: &Path) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("invalid consensus chain path: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fx_consensus::{ChainEntry, Decision, ExperimentReport, GenerationStrategy};
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    #[test]
    fn chain_with_empty_storage_reports_no_experiments() {
        let temp = TempDir::new().expect("temp dir");
        let output = show_chain_at(temp.path().join("chain.json"), 10).expect("show chain");
        assert_eq!(output, "No experiments recorded yet");
    }

    #[test]
    fn verify_passes_for_empty_chain() {
        let temp = TempDir::new().expect("temp dir");
        let output = verify_chain_at(temp.path().join("chain.json")).expect("verify chain");
        assert_eq!(
            output,
            "Chain integrity verified: 0 entries, all hashes valid"
        );
    }

    #[tokio::test]
    async fn run_creates_chain_entry_and_verification_passes() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");

        let output = run_experiment_with_path(
            RunExperimentArgs {
                signal: "sequential-tool-calls".to_owned(),
                hypothesis: "Parallelizing tool calls reduces token waste".to_owned(),
                nodes: 3,
                scope: "src/**/*.rs".to_owned(),
                timeout: 120,
            },
            chain_path.clone(),
        )
        .await
        .expect("run experiment");
        let verify = verify_chain_at(chain_path).expect("verify chain");

        assert!(output.contains("═══ Experiment Complete ═══"));
        assert!(output.contains("Chain entry #0 recorded"));
        assert_eq!(
            verify,
            "Chain integrity verified: 1 entries, all hashes valid"
        );
    }

    #[test]
    fn report_formatting_shows_winner_and_decision() {
        let report = ExperimentReport {
            result: fx_consensus::ConsensusResult {
                experiment_id: Uuid::nil(),
                winner: Some(Uuid::from_u128(1)),
                candidates: vec![Uuid::from_u128(1), Uuid::from_u128(2)],
                candidate_nodes: BTreeMap::from([
                    (
                        Uuid::from_u128(1),
                        fx_consensus::NodeId("node-0".to_owned()),
                    ),
                    (
                        Uuid::from_u128(2),
                        fx_consensus::NodeId("node-1".to_owned()),
                    ),
                ]),
                evaluations: vec![],
                aggregate_scores: BTreeMap::from([
                    (Uuid::from_u128(1), 8.73),
                    (Uuid::from_u128(2), 6.21),
                ]),
                decision: Decision::Accept,
                timestamp: Utc::now(),
            },
            chain_entry_index: 4,
            candidates: vec![
                fx_consensus::CandidateReport {
                    node_id: fx_consensus::NodeId("node-0".to_owned()),
                    strategy: GenerationStrategy::Conservative,
                    approach: "steady".to_owned(),
                    aggregate_score: 8.73,
                    is_winner: true,
                },
                fx_consensus::CandidateReport {
                    node_id: fx_consensus::NodeId("node-1".to_owned()),
                    strategy: GenerationStrategy::Aggressive,
                    approach: "fast".to_owned(),
                    aggregate_score: 6.21,
                    is_winner: false,
                },
            ],
        };
        let args = RunExperimentArgs {
            signal: "sequential-tool-calls".to_owned(),
            hypothesis: "Parallelizing tool calls reduces token waste".to_owned(),
            nodes: 2,
            scope: "src/**/*.rs".to_owned(),
            timeout: 120,
        };

        let output = format_experiment_report(&args, &report);

        assert!(output.contains("Experiment ID: 00000000-0000-0000-0000-000000000000"));
        assert!(output.contains("Decision:      ✅ ACCEPT"));
        assert!(output.contains("🏆 node-0 (Conservative)  score: 8.73  ← WINNER"));
        assert!(output.contains("Chain entry #4 recorded"));
    }

    #[test]
    fn show_entry_with_valid_index_formats_human_readable_output() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_sample_chain(&chain_path);

        let output = show_entry_at(chain_path, 0).expect("show entry");

        assert!(output.contains("Chain entry #0"));
        assert!(output.contains("Decision: ✅ ACCEPT"));
        assert!(output.contains("Winner: node-0"));
        assert!(output.contains("Scores:"));
        assert!(output.contains("  - node-0: 8.73  ← WINNER"));
        assert!(output.contains("Evaluations: 1 total"));
        assert!(!output.contains("aggregate_scores"));
        assert!(!output.contains("evaluations:"));
    }

    #[test]
    fn show_entry_with_invalid_index_returns_error() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_sample_chain(&chain_path);

        let error = show_entry_at(chain_path, 99).expect_err("missing entry should fail");

        assert_eq!(error.to_string(), "Chain entry #99 not found");
    }

    #[test]
    fn show_chain_with_entries_formats_recent_output() {
        let temp = TempDir::new().expect("temp dir");
        let chain_path = temp.path().join("chain.json");
        write_sample_chain(&chain_path);

        let output = show_chain_at(chain_path, 10).expect("show chain");

        assert!(output.contains("Recent experiments:"));
        assert!(output.contains(
            "#0 | Parallelizing tool calls reduces token waste | ✅ ACCEPT | winner: node-0"
        ));
    }

    #[test]
    fn parse_scope_splits_comma_separated_patterns() {
        assert_eq!(
            parse_scope(" src/**/*.rs, tests/**/*.rs ,docs/*.md "),
            vec![
                PathPattern::from("src/**/*.rs"),
                PathPattern::from("tests/**/*.rs"),
                PathPattern::from("docs/*.md"),
            ]
        );
    }

    fn write_sample_chain(path: &Path) {
        let mut chain = Chain::new();
        let candidate_id = Uuid::from_u128(1);
        let experiment_id = Uuid::from_u128(2);
        let timestamp = Utc::now();
        let experiment = fx_consensus::Experiment {
            id: experiment_id,
            trigger: Signal {
                id: Uuid::from_u128(3),
                name: "sequential-tool-calls".to_owned(),
                description: "signal".to_owned(),
                severity: fx_consensus::Severity::Medium,
            },
            hypothesis: "Parallelizing tool calls reduces token waste".to_owned(),
            fitness_criteria: default_fitness_criteria(),
            scope: build_scope("src/**/*.rs"),
            timeout: Duration::from_secs(120),
            min_candidates: 1,
            created_at: timestamp,
        };
        let result = fx_consensus::ConsensusResult {
            experiment_id,
            winner: Some(candidate_id),
            candidates: vec![candidate_id],
            candidate_nodes: BTreeMap::from([(
                candidate_id,
                fx_consensus::NodeId("node-0".to_owned()),
            )]),
            evaluations: vec![fx_consensus::Evaluation {
                candidate_id,
                evaluator_id: fx_consensus::NodeId("node-1".to_owned()),
                fitness_scores: BTreeMap::from([("build_success".to_owned(), 1.0)]),
                safety_pass: true,
                signal_resolved: true,
                regression_detected: false,
                notes: "Looks good".to_owned(),
                created_at: timestamp,
            }],
            aggregate_scores: BTreeMap::from([(candidate_id, 8.73)]),
            decision: Decision::Accept,
            timestamp,
        };
        chain
            .append(experiment, result, Some("diff --git".to_owned()), None)
            .expect("append chain entry");
        ensure_chain_parent_dir(path).expect("chain dir");
        JsonFileChainStorage::new(path)
            .save(&chain)
            .expect("save chain");
    }
}
