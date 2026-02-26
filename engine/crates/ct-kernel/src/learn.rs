//! Learn-step output types.

use serde::{Deserialize, Serialize};

/// Learning artifact captured from a completed verification episode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Learning {
    /// Episodic memory summary of what happened.
    pub episode: String,
    /// Optional pattern detected across attempts.
    pub pattern: Option<String>,
    /// Optional behavioral adjustment for future loops.
    pub adjustment: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_struct_holds_episode_and_adjustments() {
        let learning = Learning {
            episode: "responded with partial confidence".to_string(),
            pattern: Some("ambiguous user prompts".to_string()),
            adjustment: Some("ask for clarifying details earlier".to_string()),
        };

        assert_eq!(learning.pattern.as_deref(), Some("ambiguous user prompts"));
        assert!(learning.adjustment.is_some());
    }
}
