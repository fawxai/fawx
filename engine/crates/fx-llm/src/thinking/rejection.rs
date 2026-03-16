use std::collections::HashSet;
use std::sync::RwLock;

/// Session-scoped cache of runtime-rejected (model, level) pairs.
/// Resets on process restart — no disk persistence.
pub struct RejectionCache {
    rejected: RwLock<HashSet<(String, String)>>,
}

impl RejectionCache {
    pub fn new() -> Self {
        Self {
            rejected: RwLock::new(HashSet::new()),
        }
    }

    /// Record that a level was rejected for a model.
    pub fn record(&self, model_id: &str, level: &str) {
        let mut set = match self.rejected.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        set.insert((model_id.to_owned(), level.to_owned()));
    }

    /// Check if a level has been rejected for a model.
    pub fn is_rejected(&self, model_id: &str, level: &str) -> bool {
        let set = match self.rejected.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        set.contains(&(model_id.to_owned(), level.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cache_has_no_rejections() {
        let cache = RejectionCache::new();
        assert!(!cache.is_rejected("model", "high"));
    }

    #[test]
    fn recorded_rejection_is_found() {
        let cache = RejectionCache::new();
        cache.record("claude-opus-4-6", "max");
        assert!(cache.is_rejected("claude-opus-4-6", "max"));
    }

    #[test]
    fn rejection_is_model_specific() {
        let cache = RejectionCache::new();
        cache.record("claude-opus-4-6", "max");
        assert!(!cache.is_rejected("claude-sonnet-4-6", "max"));
    }

    #[test]
    fn rejection_is_level_specific() {
        let cache = RejectionCache::new();
        cache.record("claude-opus-4-6", "max");
        assert!(!cache.is_rejected("claude-opus-4-6", "high"));
    }

    #[test]
    fn multiple_rejections_tracked() {
        let cache = RejectionCache::new();
        cache.record("model-a", "high");
        cache.record("model-b", "max");
        assert!(cache.is_rejected("model-a", "high"));
        assert!(cache.is_rejected("model-b", "max"));
        assert!(!cache.is_rejected("model-a", "max"));
    }
}
