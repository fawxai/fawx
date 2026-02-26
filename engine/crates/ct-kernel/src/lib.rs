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

pub mod act;
pub mod auth;
pub mod budget;
pub mod checkpoint;
pub mod context_manager;
pub mod continuation;
pub mod decide;
pub mod learn;
pub mod loop_engine;
pub mod oauth;
pub mod perceive;
pub mod permissions;
pub mod policy;
pub mod reason;
pub mod rollback;
pub mod types;
pub mod verify;
pub mod watchdog;

pub use act::{ActionResult, TokenUsage, ToolResult};
pub use continuation::Continuation;
pub use decide::Decision;
pub use learn::Learning;
pub use loop_engine::{LoopEngine, LoopResult, LoopStatus};
pub use perceive::ProcessedPerception;
pub use types::{ContinuationDecision, EscalationContext, LoopError, LoopEvidence};
pub use verify::Verification;
