#![allow(clippy::field_reassign_with_default)]
//! # fx-kernel — Fawx Kernel
//!
//! The kernel layer: loop orchestration, policy engine, permissions, budget
//! enforcement, rollback, and watchdog. Immutable at runtime.
//!
//! ## Architecture
//!
//! The kernel implements `Loop(goal, context, depth) → LoopResult` — the
//! single-pass agentic loop:
//!
//! 1. **Perceive** — assemble perception snapshot (screen, sensors, memory retrieval)
//! 2. **Reason** — LLM planning given perception + identity + procedural memory
//! 3. **Decide** — three-gate check: policy → budget → permission
//! 4. **Act** — execute approved intent with tool chaining inside act_with_tools
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
pub mod authority;
pub mod budget;
pub mod caching_executor;
pub mod cancellation;
pub mod channels;
pub mod checkpoint;
pub mod context_manager;
pub mod conversation_compactor;
pub mod decide;
pub mod event_bus;
pub mod input;
mod kernel_blind;
pub mod loop_engine;
pub mod perceive;
pub mod permission_gate;
pub mod permission_prompt;
pub mod permissions;
pub mod policy;
pub mod process_registry;
pub mod proposal_gate;
pub mod reason;
pub mod rollback;
pub mod scoped_tool_executor;
pub mod signals;
pub mod streaming;
pub mod system_prompt;
pub mod types;
pub mod user_facing_error;
pub mod watchdog;
pub mod yield_primitive;

pub use act::{
    cancelled_result, is_cancelled, timed_out_result, ActionResult, ConcurrencyPolicy, TokenUsage,
    ToolCacheStats, ToolCacheability, ToolResult,
};
pub use authority::{
    ApprovalScope, AuthorityCoordinator, AuthorityDecision, AuthorityEffect,
    AuthorityStatusSnapshot, AuthorityTargetKind, AuthorityVerdict, ToolAuthoritySurface,
};
pub use caching_executor::CachingExecutor;
pub use cancellation::CancellationToken;
pub use channels::{ChannelRegistry, HttpChannel, ResponseRouter, TuiChannel};
pub use decide::Decision;
pub use event_bus::{CompletionEvent, EventBus, Observer, TaskResult};
pub use fx_decompose::{
    AggregationStrategy, DecompositionPlan, SubGoal, SubGoalOutcome, SubGoalResult,
};
pub use input::{loop_input_channel, LoopCommand, LoopInputChannel, LoopInputSender};
pub use loop_engine::{LoopEngine, LoopEngineBuilder, LoopResult, LoopStatus, ScratchpadProvider};
pub use perceive::ProcessedPerception;
pub use permission_gate::{PermissionGateExecutor, PermissionPolicy};
pub use permission_prompt::{
    PermissionDecision, PermissionPrompt, PermissionPromptState, PromptError, ResolveResult,
    PROMPT_TTL,
};
pub use process_registry::{
    ListEntry, ProcessConfig, ProcessRegistry, ProcessStatus, SpawnResult, StatusResult,
};
pub use proposal_gate::{is_tier3_path, ProposalGateExecutor, ProposalGateState};
pub use signals::{LoopStep, Signal, SignalCollector, SignalKind};
pub use streaming::{ErrorCategory, Phase, StreamCallback, StreamEvent};
pub use types::LoopError;
pub use yield_primitive::{WakeCondition, WakeReason, YieldHandle, YieldRequest, YieldWaker};
