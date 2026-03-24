//! Kernel yield primitive — unified park/wake for the agentic loop.
//!
//! Provides [`YieldRequest`] and [`WakeCondition`] types that allow the loop
//! to park in a low-resource state until an external event fires. This unifies
//! three patterns: fleet worker parking, permission prompt waiting, and human
//! interrupt handling.
//!
//! # Usage
//!
//! ```ignore
//! let (request, handle) = YieldRequest::new(vec![
//!     WakeCondition::user_message(),
//!     WakeCondition::timer(Duration::from_secs(60)),
//! ]);
//! // Park the loop...
//! let reason = handle.wait().await;
//! // Loop resumes with the wake reason
//! ```

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::oneshot;

/// Reason the loop was woken from a yield.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeReason {
    /// A user message arrived on the input channel.
    UserMessage,
    /// A permission prompt was resolved.
    PermissionResolved,
    /// A fleet task was dispatched to this worker.
    TaskDispatched,
    /// A timer fired.
    TimerFired,
    /// The cancellation token was triggered.
    Cancelled,
    /// The yield timed out.
    Timeout,
    /// An external signal woke the loop.
    Signal(String),
}

/// A condition that can wake the loop from a yield.
#[derive(Debug, Clone)]
pub enum WakeCondition {
    /// Wake when a user message arrives.
    UserMessage,
    /// Wake after a duration.
    Timer(Duration),
    /// Wake when a specific named event fires.
    Event(String),
    /// Wake when the cancellation token triggers.
    Cancellation,
}

impl WakeCondition {
    pub fn user_message() -> Self {
        Self::UserMessage
    }

    pub fn timer(duration: Duration) -> Self {
        Self::Timer(duration)
    }

    pub fn event(name: impl Into<String>) -> Self {
        Self::Event(name.into())
    }

    pub fn cancellation() -> Self {
        Self::Cancellation
    }
}

/// A request to yield (park) the loop until a wake condition fires.
#[derive(Debug)]
pub struct YieldRequest {
    /// Conditions that can wake the loop.
    pub conditions: Vec<WakeCondition>,
    /// Maximum time to stay parked before auto-waking with Timeout.
    pub timeout: Option<Duration>,
}

/// Handle returned from creating a yield request. The loop awaits this
/// to park until a wake condition fires.
#[derive(Debug)]
pub struct YieldHandle {
    receiver: oneshot::Receiver<WakeReason>,
}

/// Sender side — held by the wake source to trigger the resume.
#[derive(Debug)]
pub struct YieldWaker {
    sender: Option<oneshot::Sender<WakeReason>>,
}

impl YieldRequest {
    /// Create a yield request and its handle.
    /// Returns (request for the loop engine, handle the loop awaits).
    pub fn new(conditions: Vec<WakeCondition>) -> (Self, YieldHandle, YieldWaker) {
        let (sender, receiver) = oneshot::channel();
        let request = Self {
            conditions,
            timeout: None,
        };
        let handle = YieldHandle { receiver };
        let waker = YieldWaker {
            sender: Some(sender),
        };
        (request, handle, waker)
    }

    /// Create a yield request with a timeout.
    pub fn with_timeout(
        conditions: Vec<WakeCondition>,
        timeout: Duration,
    ) -> (Self, YieldHandle, YieldWaker) {
        let (sender, receiver) = oneshot::channel();
        let request = Self {
            conditions,
            timeout: Some(timeout),
        };
        let handle = YieldHandle { receiver };
        let waker = YieldWaker {
            sender: Some(sender),
        };
        (request, handle, waker)
    }
}

impl YieldHandle {
    /// Wait for the yield to be resolved. Returns the wake reason.
    /// If the waker is dropped without sending, returns Cancelled.
    pub async fn wait(self) -> WakeReason {
        match self.receiver.await {
            Ok(reason) => reason,
            Err(_) => WakeReason::Cancelled,
        }
    }

    /// Wait with a timeout. Returns Timeout if the deadline passes.
    pub async fn wait_with_timeout(self, timeout: Duration) -> WakeReason {
        match tokio::time::timeout(timeout, self.receiver).await {
            Ok(Ok(reason)) => reason,
            Ok(Err(_)) => WakeReason::Cancelled,
            Err(_) => WakeReason::Timeout,
        }
    }
}

impl YieldWaker {
    /// Wake the yielded loop with a reason.
    /// Returns false if the handle was already dropped (loop resumed elsewhere).
    pub fn wake(&mut self, reason: WakeReason) -> bool {
        if let Some(sender) = self.sender.take() {
            sender.send(reason).is_ok()
        } else {
            false
        }
    }

    /// Check if this waker can still wake the loop.
    pub fn is_active(&self) -> bool {
        self.sender.is_some()
    }
}

impl Drop for YieldWaker {
    fn drop(&mut self) {
        // If the waker is dropped without sending, the receiver gets RecvError
        // which YieldHandle::wait maps to Cancelled.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn yield_and_wake_with_reason() {
        let (_request, handle, mut waker) = YieldRequest::new(vec![WakeCondition::UserMessage]);

        assert!(waker.wake(WakeReason::UserMessage));
        assert_eq!(handle.wait().await, WakeReason::UserMessage);
    }

    #[tokio::test]
    async fn dropped_waker_returns_cancelled() {
        let (_request, handle, waker) = YieldRequest::new(vec![WakeCondition::UserMessage]);

        drop(waker);
        assert_eq!(handle.wait().await, WakeReason::Cancelled);
    }

    #[tokio::test]
    async fn wake_after_handle_dropped_returns_false() {
        let (_request, handle, mut waker) = YieldRequest::new(vec![WakeCondition::UserMessage]);

        drop(handle);
        assert!(!waker.wake(WakeReason::UserMessage));
    }

    #[tokio::test]
    async fn double_wake_returns_false() {
        let (_request, handle, mut waker) = YieldRequest::new(vec![WakeCondition::UserMessage]);

        assert!(waker.wake(WakeReason::UserMessage));
        assert!(!waker.wake(WakeReason::TimerFired));
        assert_eq!(handle.wait().await, WakeReason::UserMessage);
    }

    #[tokio::test]
    async fn wait_with_timeout_returns_timeout() {
        let (_request, handle, _waker) =
            YieldRequest::new(vec![WakeCondition::Timer(Duration::from_millis(10))]);

        let reason = handle.wait_with_timeout(Duration::from_millis(1)).await;
        assert_eq!(reason, WakeReason::Timeout);
    }

    #[tokio::test]
    async fn wait_with_timeout_returns_reason_before_deadline() {
        let (_request, handle, mut waker) = YieldRequest::new(vec![WakeCondition::UserMessage]);

        waker.wake(WakeReason::TaskDispatched);
        let reason = handle.wait_with_timeout(Duration::from_secs(10)).await;
        assert_eq!(reason, WakeReason::TaskDispatched);
    }

    #[test]
    fn wake_reason_serializes() {
        let json = serde_json::to_value(WakeReason::PermissionResolved).unwrap();
        assert_eq!(json, "permission_resolved");
    }

    #[test]
    fn wake_reason_round_trips() {
        for reason in [
            WakeReason::UserMessage,
            WakeReason::PermissionResolved,
            WakeReason::TaskDispatched,
            WakeReason::TimerFired,
            WakeReason::Cancelled,
            WakeReason::Timeout,
            WakeReason::Signal("custom".into()),
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let decoded: WakeReason = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, reason);
        }
    }

    #[test]
    fn yield_request_with_timeout() {
        let (request, _handle, _waker) =
            YieldRequest::with_timeout(vec![WakeCondition::UserMessage], Duration::from_secs(60));
        assert_eq!(request.timeout, Some(Duration::from_secs(60)));
        assert_eq!(request.conditions.len(), 1);
    }

    #[test]
    fn waker_is_active_before_wake() {
        let (_request, _handle, waker) = YieldRequest::new(vec![]);
        assert!(waker.is_active());
    }

    #[test]
    fn waker_is_inactive_after_wake() {
        let (_request, _handle, mut waker) = YieldRequest::new(vec![]);
        waker.wake(WakeReason::TimerFired);
        assert!(!waker.is_active());
    }
}
