/// A mapping from model ID pattern to profile name.
#[derive(Debug, Clone)]
pub struct ModelMapping {
    pattern: String,
    pub(crate) profile_name: String,
    priority: MatchPriority,
}

/// Match priority: exact > prefix > wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchPriority {
    Exact = 0,
    Prefix = 1,
    Wildcard = 2,
}

impl ModelMapping {
    pub fn exact(model_id: &str, profile: &str) -> Self {
        Self {
            pattern: model_id.to_owned(),
            profile_name: profile.to_owned(),
            priority: MatchPriority::Exact,
        }
    }

    pub fn prefix(prefix: &str, profile: &str) -> Self {
        Self {
            pattern: prefix.to_owned(),
            profile_name: profile.to_owned(),
            priority: MatchPriority::Prefix,
        }
    }

    pub fn wildcard(prefix: &str, profile: &str) -> Self {
        Self {
            pattern: prefix.to_owned(),
            profile_name: profile.to_owned(),
            priority: MatchPriority::Wildcard,
        }
    }

    /// Check if this mapping matches a model ID.
    pub fn matches(&self, model_id: &str) -> bool {
        match self.priority {
            MatchPriority::Exact => model_id == self.pattern,
            MatchPriority::Prefix | MatchPriority::Wildcard => model_id.starts_with(&self.pattern),
        }
    }
}

/// Find the best matching profile name for a model ID.
/// Returns None if no mapping matches.
pub fn find_best_match<'a>(mappings: &'a [ModelMapping], model_id: &str) -> Option<&'a str> {
    mappings
        .iter()
        .filter(|m| m.matches(model_id))
        .min_by_key(|m| m.priority)
        .map(|m| m.profile_name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_wins_over_prefix() {
        let mappings = vec![
            ModelMapping::prefix("claude-opus-", "prefix-profile"),
            ModelMapping::exact("claude-opus-4-6", "exact-profile"),
        ];
        assert_eq!(
            find_best_match(&mappings, "claude-opus-4-6"),
            Some("exact-profile")
        );
    }

    #[test]
    fn prefix_wins_over_wildcard() {
        let mappings = vec![
            ModelMapping::wildcard("claude-", "wild-profile"),
            ModelMapping::prefix("claude-opus-4-5", "prefix-profile"),
        ];
        assert_eq!(
            find_best_match(&mappings, "claude-opus-4-5-20260301"),
            Some("prefix-profile")
        );
    }

    #[test]
    fn wildcard_matches_when_no_better() {
        let mappings = vec![
            ModelMapping::wildcard("claude-", "wild-profile"),
            ModelMapping::exact("gpt-5.4", "gpt-profile"),
        ];
        assert_eq!(
            find_best_match(&mappings, "claude-sonnet-99"),
            Some("wild-profile")
        );
    }

    #[test]
    fn no_match_returns_none() {
        let mappings = vec![ModelMapping::exact("claude-opus-4-6", "opus-profile")];
        assert_eq!(find_best_match(&mappings, "totally-unknown"), None);
    }

    #[test]
    fn exact_does_not_match_prefix() {
        let mapping = ModelMapping::exact("claude-opus-4-6", "profile");
        assert!(mapping.matches("claude-opus-4-6"));
        assert!(!mapping.matches("claude-opus-4-6-extended"));
    }

    #[test]
    fn prefix_matches_extensions() {
        let mapping = ModelMapping::prefix("claude-opus-4-5", "profile");
        assert!(mapping.matches("claude-opus-4-5"));
        assert!(mapping.matches("claude-opus-4-5-20260301"));
        assert!(!mapping.matches("claude-opus-4-6"));
    }
}
