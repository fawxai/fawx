use serde::{Deserialize, Serialize};

/// A typed operation in the Graph of Thoughts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GraphOperation {
    /// Generate N thought branches from each active thought.
    Generate {
        /// Number of branches to create per input thought.
        num_branches: usize,
        /// Optional prompt template override for generation.
        prompt_override: Option<String>,
    },
    /// Score each active thought using a scoring strategy.
    Score { strategy: ScoringStrategy },
    /// Keep only the top-N scored thoughts, pruning the rest.
    KeepBest { n: usize },
    /// Merge all active thoughts into a single combined thought.
    Merge { strategy: MergeStrategy },
    /// Refine active thoughts through iterative score-then-improve cycles.
    Refine {
        max_iterations: usize,
        target_score: f64,
        scoring: ScoringStrategy,
    },
    /// Validate active thoughts against a ground-truth strategy.
    Validate { strategy: ValidationStrategy },
}

/// How to score a thought.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScoringStrategy {
    /// Ask the LLM to rate the thought on a 0.0-1.0 scale.
    LlmRating { criteria: String },
    /// Use a regex or substring match to compute a heuristic score.
    Heuristic { pattern: String },
    /// Use an external scoring function provided at runtime.
    External,
}

/// How to merge multiple thoughts into one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MergeStrategy {
    /// Ask the LLM to synthesize all active thoughts into one.
    LlmSynthesis { instruction: Option<String> },
    /// Concatenate all thought contents with a separator.
    Concatenate { separator: String },
}

/// How to validate a thought against ground truth.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ValidationStrategy {
    /// Exact string match against expected output.
    ExactMatch { expected: String },
    /// Substring containment check.
    Contains { expected: String },
    /// LLM-based validation against explicit criteria.
    LlmJudge { criteria: String },
    /// Always passes.
    AlwaysPass,
}
