//! User input channel for controlling loop execution.
//!
//! The TUI sends [`LoopCommand`] values through [`LoopInputSender`]; the
//! loop engine drains them via [`LoopInputChannel`] between steps.

use tokio::sync::mpsc;

/// Commands the user can issue during loop execution.
///
/// NOTE: Loop V2 spec §4 also defines `Steer(String)` and `AbortNegative`
/// commands, and per-step cancellation semantics (REASON/DECIDE/ACT/SYNTHESIZE).
/// These are deferred to a follow-up PR as the current Stop/Abort/Wait/Resume
/// set covers the primary use cases.
/// See: docs/architecture/loop-v2-spec.md Phase 4
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopCommand {
    /// Graceful stop after the current step completes.
    Stop,
    /// Immediate abort — cancel in-flight work.
    Abort,
    /// Pause execution until Resume.
    Wait,
    /// Resume after a Wait.
    Resume,
}

/// Receiving end of the user-input channel (held by the loop engine).
#[derive(Debug)]
pub struct LoopInputChannel {
    receiver: mpsc::Receiver<LoopCommand>,
}

impl LoopInputChannel {
    /// Try to receive a command without blocking.
    ///
    /// Returns `None` when no command is pending.
    pub fn try_recv(&mut self) -> Option<LoopCommand> {
        self.receiver.try_recv().ok()
    }
}

/// Sending end of the user-input channel (held by the TUI).
#[derive(Debug, Clone)]
pub struct LoopInputSender {
    sender: mpsc::Sender<LoopCommand>,
}

impl LoopInputSender {
    /// Send a command to the loop engine.
    ///
    /// Returns `Err` only if the receiver has been dropped.
    pub fn send(&self, cmd: LoopCommand) -> Result<(), LoopCommand> {
        self.sender.try_send(cmd).map_err(|e| e.into_inner())
    }
}

/// Create a paired (sender, receiver) channel for user commands.
pub fn loop_input_channel() -> (LoopInputSender, LoopInputChannel) {
    let (sender, receiver) = mpsc::channel(16);
    (LoopInputSender { sender }, LoopInputChannel { receiver })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn channel_delivers_commands_in_order() {
        let (sender, mut receiver) = loop_input_channel();
        sender.send(LoopCommand::Stop).expect("send Stop");
        sender.send(LoopCommand::Abort).expect("send Abort");

        assert_eq!(receiver.try_recv(), Some(LoopCommand::Stop));
        assert_eq!(receiver.try_recv(), Some(LoopCommand::Abort));
        assert_eq!(receiver.try_recv(), None);
    }

    #[tokio::test]
    async fn sender_is_cloneable() {
        let (sender, mut receiver) = loop_input_channel();
        let clone = sender.clone();
        clone.send(LoopCommand::Wait).expect("send Wait");
        assert_eq!(receiver.try_recv(), Some(LoopCommand::Wait));
    }
}
