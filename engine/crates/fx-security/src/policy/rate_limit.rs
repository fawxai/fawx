//! Rate limiting for action executions.

use super::types::PolicyDecision;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Rate limiter using sliding window counters.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Rate limits: action -> (max_count, window_ms)
    limits: HashMap<String, (u32, u64)>,
    /// Timestamps of recent actions: action -> [timestamps]
    history: HashMap<String, Vec<u64>>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new() -> Self {
        Self {
            limits: HashMap::new(),
            history: HashMap::new(),
        }
    }

    /// Add a rate limit for an action.
    ///
    /// # Arguments
    /// * `action` - Action name or pattern
    /// * `max_count` - Maximum number of occurrences
    /// * `window_ms` - Time window in milliseconds
    pub fn add_limit(&mut self, action: String, max_count: u32, window_ms: u64) {
        self.limits.insert(action, (max_count, window_ms));
    }

    /// Check if an action is within rate limits.
    ///
    /// # Arguments
    /// * `action` - Action to check
    ///
    /// # Returns
    /// `PolicyDecision::Allow` if within limits, `PolicyDecision::RateLimit` otherwise
    pub fn check(&mut self, action: &str) -> PolicyDecision {
        // Get limit for this action
        let (max_count, window_ms) = match self.limits.get(action) {
            Some(&limits) => limits,
            None => return PolicyDecision::Allow, // No limit set
        };

        let now = current_timestamp_ms();
        let window_start = now.saturating_sub(window_ms);

        // Get or create history for this action
        let timestamps = self.history.entry(action.to_string()).or_default();

        // Remove timestamps outside the window
        timestamps.retain(|&ts| ts > window_start);

        // Check if we're over the limit
        if timestamps.len() >= max_count as usize {
            // Calculate how long to wait
            // Note: For max_count=0, timestamps will be empty. Use 'now' as fallback.
            // For max_count>0, timestamps is guaranteed non-empty by the check above.
            let oldest_in_window = timestamps.first().copied().unwrap_or(now);
            let wait_ms = (oldest_in_window + window_ms).saturating_sub(now);
            return PolicyDecision::RateLimit { wait_ms };
        }

        // Record this action
        timestamps.push(now);

        PolicyDecision::Allow
    }

    /// Reset all rate limit state (useful for testing).
    pub fn reset(&mut self) {
        self.history.clear();
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current timestamp in milliseconds since UNIX epoch.
fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0) // Fallback to 0 if system time is before UNIX epoch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_limit() {
        let mut limiter = RateLimiter::new();
        assert!(matches!(
            limiter.check("test_action"),
            PolicyDecision::Allow
        ));
    }

    #[test]
    fn test_under_limit() {
        let mut limiter = RateLimiter::new();
        limiter.add_limit("test_action".to_string(), 3, 1000);

        assert!(matches!(
            limiter.check("test_action"),
            PolicyDecision::Allow
        ));
        assert!(matches!(
            limiter.check("test_action"),
            PolicyDecision::Allow
        ));
        assert!(matches!(
            limiter.check("test_action"),
            PolicyDecision::Allow
        ));
    }

    #[test]
    fn test_over_limit() {
        let mut limiter = RateLimiter::new();
        limiter.add_limit("test_action".to_string(), 2, 1000);

        assert!(matches!(
            limiter.check("test_action"),
            PolicyDecision::Allow
        ));
        assert!(matches!(
            limiter.check("test_action"),
            PolicyDecision::Allow
        ));

        match limiter.check("test_action") {
            PolicyDecision::RateLimit { wait_ms } => {
                assert!(wait_ms > 0);
            }
            _ => panic!("Expected RateLimit decision"),
        }
    }

    #[test]
    fn test_reset() {
        let mut limiter = RateLimiter::new();
        limiter.add_limit("test_action".to_string(), 2, 1000);

        limiter.check("test_action");
        limiter.check("test_action");

        limiter.reset();

        assert!(matches!(
            limiter.check("test_action"),
            PolicyDecision::Allow
        ));
    }
}
