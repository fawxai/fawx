//! Verify-step output types.

use serde::{Deserialize, Serialize};

/// Verification result comparing expected and actual outcomes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Verification {
    /// True when the execution outcome is satisfactory.
    pub outcome_satisfactory: bool,
    /// Confidence score in the verification judgment.
    pub confidence: f64,
    /// Mismatches observed between expectation and execution.
    pub discrepancies: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_is_constructible() {
        let verification = Verification {
            outcome_satisfactory: true,
            confidence: 0.91,
            discrepancies: Vec::new(),
        };

        assert!(verification.outcome_satisfactory);
        assert!(verification.discrepancies.is_empty());
    }
}
