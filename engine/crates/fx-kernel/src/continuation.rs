//! Continue-step decisions.

use serde::{Deserialize, Serialize};

/// Decision describing whether the loop should continue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Continuation {
    /// Loop is complete.
    Complete,
    /// Continue loop execution with the provided sub-goal text.
    Continue(String),
    /// Pause and request user input.
    NeedsInput(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_variants_cover_all_paths() {
        assert!(matches!(Continuation::Complete, Continuation::Complete));
        assert!(matches!(
            Continuation::Continue("retry".to_string()),
            Continuation::Continue(_)
        ));
        assert!(matches!(
            Continuation::NeedsInput("clarify".to_string()),
            Continuation::NeedsInput(_)
        ));
    }
}
