//! Cross-branch tournament scoring.
//!
//! Run the same experiment against multiple branches and compare scores.
//! Each branch gets a score; the highest-scoring branch wins.
//!
//! # Usage
//!
//! ```ignore
//! let tournament = Tournament::new(vec!["feat/approach-a", "feat/approach-b"]);
//! let results = tournament.run(&runner, experiment_config).await?;
//! println!("Winner: {}", results.winner().branch);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for a cross-branch tournament.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TournamentConfig {
    /// Branches to compete.
    pub branches: Vec<String>,
    /// Number of rounds per branch (averaging reduces noise).
    pub rounds_per_branch: u32,
    /// Maximum concurrent branch evaluations.
    pub max_parallel: usize,
}

impl TournamentConfig {
    pub fn new(branches: Vec<String>) -> Self {
        Self {
            branches,
            rounds_per_branch: 1,
            max_parallel: 1,
        }
    }

    pub fn with_rounds(mut self, rounds: u32) -> Self {
        self.rounds_per_branch = rounds.max(1);
        self
    }

    pub fn with_parallelism(mut self, max: usize) -> Self {
        self.max_parallel = max.max(1);
        self
    }
}

/// Result of a single branch evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchScore {
    pub branch: String,
    pub scores: Vec<f64>,
    pub average_score: f64,
    pub best_score: f64,
    pub rounds_completed: u32,
    pub rounds_failed: u32,
}

impl BranchScore {
    pub fn new(branch: String) -> Self {
        Self {
            branch,
            scores: Vec::new(),
            average_score: 0.0,
            best_score: 0.0,
            rounds_completed: 0,
            rounds_failed: 0,
        }
    }

    pub fn record_score(&mut self, score: f64) {
        self.scores.push(score);
        self.rounds_completed += 1;
        self.best_score = self
            .scores
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        self.average_score = if self.scores.is_empty() {
            0.0
        } else {
            self.scores.iter().sum::<f64>() / self.scores.len() as f64
        };
    }

    pub fn record_failure(&mut self) {
        self.rounds_failed += 1;
    }
}

/// Outcome of a tournament across branches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TournamentResult {
    pub config: TournamentConfig,
    pub branch_scores: Vec<BranchScore>,
    pub winner: Option<String>,
    pub completed: bool,
}

impl TournamentResult {
    pub fn new(config: TournamentConfig) -> Self {
        let branch_scores = config
            .branches
            .iter()
            .map(|b| BranchScore::new(b.clone()))
            .collect();
        Self {
            config,
            branch_scores,
            winner: None,
            completed: false,
        }
    }

    /// Get the branch score for a given branch name.
    pub fn get_branch(&self, branch: &str) -> Option<&BranchScore> {
        self.branch_scores.iter().find(|s| s.branch == branch)
    }

    /// Get mutable branch score.
    pub fn get_branch_mut(&mut self, branch: &str) -> Option<&mut BranchScore> {
        self.branch_scores.iter_mut().find(|s| s.branch == branch)
    }

    /// Determine the winner based on average score.
    pub fn finalize(&mut self) {
        self.completed = true;
        self.winner = self
            .branch_scores
            .iter()
            .filter(|s| s.rounds_completed > 0)
            .max_by(|a, b| {
                a.average_score
                    .partial_cmp(&b.average_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|s| s.branch.clone());
    }

    /// Get a leaderboard sorted by average score (descending).
    pub fn leaderboard(&self) -> Vec<&BranchScore> {
        let mut sorted: Vec<&BranchScore> = self.branch_scores.iter().collect();
        sorted.sort_by(|a, b| {
            b.average_score
                .partial_cmp(&a.average_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted
    }

    /// Summary string for display.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        for (rank, score) in self.leaderboard().iter().enumerate() {
            lines.push(format!(
                "{}. {} — avg: {:.2}, best: {:.2} ({}/{} rounds)",
                rank + 1,
                score.branch,
                score.average_score,
                score.best_score,
                score.rounds_completed,
                score.rounds_completed + score.rounds_failed,
            ));
        }
        if let Some(winner) = &self.winner {
            lines.push(format!("\nWinner: {winner}"));
        }
        lines.join("\n")
    }
}

/// Serializable tournament progress for HTTP endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TournamentProgress {
    pub total_branches: usize,
    pub completed_branches: usize,
    pub current_branch: Option<String>,
    pub current_round: u32,
    pub leaderboard: HashMap<String, f64>,
}

impl TournamentProgress {
    pub fn from_result(result: &TournamentResult, current: Option<(&str, u32)>) -> Self {
        let completed = result
            .branch_scores
            .iter()
            .filter(|s| s.rounds_completed + s.rounds_failed > 0)
            .count();
        let leaderboard = result
            .branch_scores
            .iter()
            .filter(|s| s.rounds_completed > 0)
            .map(|s| (s.branch.clone(), s.average_score))
            .collect();
        Self {
            total_branches: result.config.branches.len(),
            completed_branches: completed,
            current_branch: current.map(|(b, _)| b.to_string()),
            current_round: current.map(|(_, r)| r).unwrap_or(0),
            leaderboard,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_score_records_and_averages() {
        let mut score = BranchScore::new("main".into());
        score.record_score(0.8);
        score.record_score(0.6);

        assert_eq!(score.rounds_completed, 2);
        assert_eq!(score.best_score, 0.8);
        assert!((score.average_score - 0.7).abs() < 0.001);
    }

    #[test]
    fn branch_score_handles_failures() {
        let mut score = BranchScore::new("main".into());
        score.record_failure();
        score.record_score(0.5);

        assert_eq!(score.rounds_completed, 1);
        assert_eq!(score.rounds_failed, 1);
        assert_eq!(score.average_score, 0.5);
    }

    #[test]
    fn tournament_result_finds_winner() {
        let config = TournamentConfig::new(vec!["a".into(), "b".into(), "c".into()]);
        let mut result = TournamentResult::new(config);

        result.get_branch_mut("a").unwrap().record_score(0.5);
        result.get_branch_mut("b").unwrap().record_score(0.9);
        result.get_branch_mut("c").unwrap().record_score(0.7);
        result.finalize();

        assert_eq!(result.winner.as_deref(), Some("b"));
    }

    #[test]
    fn tournament_result_no_winner_when_all_failed() {
        let config = TournamentConfig::new(vec!["a".into(), "b".into()]);
        let mut result = TournamentResult::new(config);
        result.get_branch_mut("a").unwrap().record_failure();
        result.get_branch_mut("b").unwrap().record_failure();
        result.finalize();

        assert!(result.winner.is_none());
    }

    #[test]
    fn leaderboard_is_sorted_descending() {
        let config = TournamentConfig::new(vec!["a".into(), "b".into(), "c".into()]);
        let mut result = TournamentResult::new(config);
        result.get_branch_mut("a").unwrap().record_score(0.3);
        result.get_branch_mut("b").unwrap().record_score(0.9);
        result.get_branch_mut("c").unwrap().record_score(0.6);

        let board = result.leaderboard();
        assert_eq!(board[0].branch, "b");
        assert_eq!(board[1].branch, "c");
        assert_eq!(board[2].branch, "a");
    }

    #[test]
    fn tournament_config_builder() {
        let config = TournamentConfig::new(vec!["main".into()])
            .with_rounds(3)
            .with_parallelism(2);

        assert_eq!(config.rounds_per_branch, 3);
        assert_eq!(config.max_parallel, 2);
    }

    #[test]
    fn tournament_progress_from_result() {
        let config = TournamentConfig::new(vec!["a".into(), "b".into()]);
        let mut result = TournamentResult::new(config);
        result.get_branch_mut("a").unwrap().record_score(0.8);

        let progress = TournamentProgress::from_result(&result, Some(("b", 1)));

        assert_eq!(progress.total_branches, 2);
        assert_eq!(progress.completed_branches, 1);
        assert_eq!(progress.current_branch.as_deref(), Some("b"));
        assert_eq!(*progress.leaderboard.get("a").unwrap(), 0.8);
    }

    #[test]
    fn summary_formats_nicely() {
        let config = TournamentConfig::new(vec!["a".into(), "b".into()]);
        let mut result = TournamentResult::new(config);
        result.get_branch_mut("a").unwrap().record_score(0.5);
        result.get_branch_mut("b").unwrap().record_score(0.9);
        result.finalize();

        let summary = result.summary();
        assert!(summary.contains("b — avg: 0.90"));
        assert!(summary.contains("Winner: b"));
    }

    #[test]
    fn tournament_result_serializes() {
        let config = TournamentConfig::new(vec!["main".into()]);
        let result = TournamentResult::new(config);
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["completed"], false);
    }
}
