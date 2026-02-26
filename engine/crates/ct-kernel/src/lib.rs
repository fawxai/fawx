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

use serde::{Deserialize, Serialize};

pub mod act;
pub mod budget;
pub mod checkpoint;
pub mod continuation;
pub mod decide;
pub mod learn;
pub mod loop_engine;
pub mod perceive;
pub mod permissions;
pub mod policy;
pub mod reason;
pub mod rollback;
pub mod types;
pub mod verify;
pub mod watchdog;

pub use types::{ContinuationDecision, EscalationContext, LoopError, LoopEvidence};

/// The core loop result type.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Stable checkpoint identifier.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CheckpointId(pub u64);
