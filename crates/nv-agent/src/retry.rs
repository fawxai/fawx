//! Retry logic with exponential backoff and jitter.

use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

/// Retry policy configuration.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Base delay in milliseconds.
    pub base_delay_ms: u64,
    /// Maximum delay in milliseconds (cap for exponential backoff).
    pub max_delay_ms: u64,
    /// Backoff factor (multiplier for each retry).
    pub backoff_factor: f64,
}

impl RetryPolicy {
    /// Create a new retry policy.
    pub fn new(
        max_retries: u32,
        base_delay_ms: u64,
        max_delay_ms: u64,
        backoff_factor: f64,
    ) -> Self {
        Self {
            max_retries,
            base_delay_ms,
            max_delay_ms,
            backoff_factor,
        }
    }

    /// Create a default retry policy (3 retries, 100ms base, 10s max, 2.0x backoff).
    pub fn default_policy() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 10_000,
            backoff_factor: 2.0,
        }
    }

    /// Create an aggressive retry policy for rate limits (5 retries, 1s base, 30s max).
    pub fn aggressive() -> Self {
        Self {
            max_retries: 5,
            base_delay_ms: 1_000,
            max_delay_ms: 30_000,
            backoff_factor: 2.0,
        }
    }
}

/// Execute a function with retry logic.
///
/// Retries on transient errors (rate limits, server errors, timeouts).
/// Does not retry on auth errors or bad requests.
pub async fn with_retry<F, Fut, T, E>(policy: &RetryPolicy, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;

    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(err) => {
                attempt += 1;

                if attempt > policy.max_retries {
                    tracing::warn!("Max retries ({}) exceeded, giving up", policy.max_retries);
                    return Err(err);
                }

                let delay = calculate_delay(attempt, policy);
                tracing::warn!(
                    "Attempt {} failed: {}. Retrying in {}ms...",
                    attempt,
                    err,
                    delay
                );

                sleep(Duration::from_millis(delay)).await;
            }
        }
    }
}

/// Determine if a status code should trigger a retry.
pub fn should_retry(status: u16) -> bool {
    matches!(
        status,
        429 |  // Rate limit
        500..=599 // Server errors
    )
}

/// Calculate delay for a given retry attempt with exponential backoff and jitter.
pub fn calculate_delay(attempt: u32, policy: &RetryPolicy) -> u64 {
    let exponential_delay =
        (policy.base_delay_ms as f64) * policy.backoff_factor.powi((attempt - 1) as i32);

    // Cap at max_delay_ms
    let capped_delay = exponential_delay.min(policy.max_delay_ms as f64);

    // Add jitter (±20%)
    let jitter_factor = 0.8 + (rand::random::<f64>() * 0.4);
    let delay_with_jitter = capped_delay * jitter_factor;

    delay_with_jitter as u64
}

// Simple random number generation for jitter
mod rand {
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};

    thread_local! {
        static SEED: Cell<u64> = Cell::new(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        );
    }

    pub fn random<T: FromRandom>() -> T {
        T::from_random()
    }

    pub trait FromRandom {
        fn from_random() -> Self;
    }

    impl FromRandom for f64 {
        fn from_random() -> Self {
            SEED.with(|seed| {
                let mut s = seed.get();
                // Simple LCG
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                seed.set(s);
                (s >> 32) as f64 / u32::MAX as f64
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_retry() {
        assert!(should_retry(429)); // Rate limit
        assert!(should_retry(500)); // Internal server error
        assert!(should_retry(503)); // Service unavailable
        assert!(should_retry(599)); // Any 5xx

        assert!(!should_retry(200)); // Success
        assert!(!should_retry(400)); // Bad request
        assert!(!should_retry(401)); // Unauthorized
        assert!(!should_retry(404)); // Not found
    }

    #[test]
    fn test_calculate_delay() {
        let policy = RetryPolicy::default_policy();

        // First retry should be around base_delay_ms (100ms ± jitter)
        let delay1 = calculate_delay(1, &policy);
        assert!((80..=120).contains(&delay1));

        // Second retry should be around base_delay_ms * backoff_factor (200ms ± jitter)
        let delay2 = calculate_delay(2, &policy);
        assert!((160..=240).contains(&delay2));

        // Third retry should be around 400ms ± jitter
        let delay3 = calculate_delay(3, &policy);
        assert!((320..=480).contains(&delay3));
    }

    #[test]
    fn test_calculate_delay_capping() {
        let policy = RetryPolicy::new(10, 100, 500, 2.0);

        // Early attempts should not be capped
        let delay1 = calculate_delay(1, &policy);
        assert!(delay1 < 500);

        // Later attempts should be capped at max_delay_ms
        let delay10 = calculate_delay(10, &policy);
        assert!(delay10 <= 500 * 12 / 10); // Allow for jitter (up to 1.2x)
    }

    #[test]
    fn test_retry_policy_creation() {
        let policy = RetryPolicy::default_policy();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.base_delay_ms, 100);
        assert_eq!(policy.max_delay_ms, 10_000);
        assert_eq!(policy.backoff_factor, 2.0);

        let aggressive = RetryPolicy::aggressive();
        assert_eq!(aggressive.max_retries, 5);
        assert_eq!(aggressive.base_delay_ms, 1_000);
        assert_eq!(aggressive.max_delay_ms, 30_000);
    }

    #[tokio::test]
    async fn test_with_retry_success() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let policy = RetryPolicy::new(3, 10, 100, 2.0);
        let attempt = Arc::new(AtomicU32::new(0));
        let attempt_clone = attempt.clone();

        let result = with_retry(&policy, || {
            let attempt = attempt_clone.clone();
            async move {
                let current = attempt.fetch_add(1, Ordering::SeqCst) + 1;
                if current < 2 {
                    Err("transient error")
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result, Ok(42));
        assert_eq!(attempt.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_with_retry_max_attempts() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let policy = RetryPolicy::new(2, 10, 100, 2.0);
        let attempt = Arc::new(AtomicU32::new(0));
        let attempt_clone = attempt.clone();

        let result = with_retry(&policy, || {
            let attempt = attempt_clone.clone();
            async move {
                attempt.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>("persistent error")
            }
        })
        .await;

        assert_eq!(result, Err("persistent error"));
        assert_eq!(attempt.load(Ordering::SeqCst), 3); // Initial + 2 retries
    }
}
