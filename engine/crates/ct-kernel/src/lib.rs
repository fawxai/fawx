//! # ct-kernel — Citros Kernel
//!
//! The kernel layer: loop orchestration, policy engine, permissions, budget
//! enforcement, verification, rollback, and watchdog. Immutable at runtime.
//!
//! ## Architecture
//!
//! The kernel implements `Loop(goal, context, depth) → LoopResult` — the
//! recursive seven-step agentic loop:
//!
//! 1. **Perceive** — assemble perception snapshot (screen, sensors, memory retrieval)
//! 2. **Reason** — LLM planning given perception + identity + procedural memory
//! 3. **Decide** — three-gate check: policy → budget → permission
//! 4. **Act** — execute approved intent, checkpoint state first
//! 5. **Verify** — artifact contract check + prediction comparison
//! 6. **Learn** — extract episodic memories, propose semantic/procedural updates
//! 7. **Continue** — evaluate completion, loop or return result
//!
//! ## Kernel Invariants
//!
//! - Permission registry is user-only (agent cannot escalate)
//! - Policies are user-only (agent cannot modify allow/deny/confirm rules)
//! - Rollback cannot be disabled (A/B slots + watchdog always active)
//! - Audit trail is append-only (no edits, no deletes)
//! - Capability dropping is one-way (skills cannot gain capabilities)
//! - Kernel is immutable at runtime (compiled Rust, no hot-patching)
//! - Consolidation requires validated checkpoint before mutations
//! - Three-gate decision: policy → budget → permission, no gate skippable

pub mod loop_engine;
pub mod perceive;
pub mod reason;
pub mod decide;
pub mod act;
pub mod verify;
pub mod learn;
pub mod continuation;
pub mod policy;
pub mod budget;
pub mod permissions;
pub mod checkpoint;
pub mod watchdog;

/// The core loop result type.
#[derive(Debug)]
pub enum LoopResult {
    /// Goal achieved with verification evidence.
    Completed(LoopEvidence),
    /// Unrecoverable failure.
    Failed(LoopError),
    /// User input needed to proceed.
    NeedsUser(EscalationContext),
    /// Hit maximum recursion depth.
    DepthExceeded,
    /// Budget exhausted for this loop invocation.
    BudgetExhausted,
    /// User cancelled; state checkpointed for potential resume.
    Interrupted(CheckpointId),
}

// Placeholder types — these will be fleshed out as modules are implemented.

#[derive(Debug)]
pub struct LoopEvidence {
    pub summary: String,
}

#[derive(Debug)]
pub struct LoopError {
    pub reason: String,
}

#[derive(Debug)]
pub struct EscalationContext {
    pub question: String,
}

#[derive(Debug, Clone, Copy)]
pub struct CheckpointId(pub u64);
