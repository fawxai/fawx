//! Cooperative cancellation token for tool execution.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A lightweight, clone-safe token for cooperative cancellation.
///
/// The token is shared between the loop engine and the tool executor.
/// When cancelled, all clones observe the cancellation.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Create a new uncancelled token.
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal cancellation. All clones will observe this.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Reset the token so it can be reused for a new cycle.
    ///
    /// Called by `prepare_cycle()` before each `run_cycle()` so that a
    /// previous Ctrl+C does not permanently brick the engine.
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }

    /// Check whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Wait until cancellation has been requested.
    pub async fn cancelled(&self) {
        while !self.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_token_starts_uncancelled() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancellation_token_cancel_sets_flag() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancellation_token_is_shared() {
        let token = CancellationToken::new();
        let clone = token.clone();
        token.cancel();
        assert!(clone.is_cancelled());
    }

    #[test]
    fn cancellation_token_reset_clears_flag() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
        token.reset();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancellation_token_default_is_uncancelled() {
        let token = CancellationToken::default();
        assert!(!token.is_cancelled());
    }
}

#[cfg(test)]
mod async_tests {
    use super::*;
    use std::time::Duration;

    /// N1: Validates cross-task atomic visibility of cancellation.
    #[tokio::test]
    async fn cancellation_visible_across_tasks() {
        let token = CancellationToken::new();
        let clone = token.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            clone.cancel();
        });
        handle.await.unwrap();
        assert!(token.is_cancelled());
    }
}
