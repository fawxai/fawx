//! # ct-loadable — Citros Loadable Layer
//!
//! The hot-swappable intelligence layer: WASM skill runtime, strategy management,
//! configuration, prompt templates, and A/B slot lifecycle. Everything that makes
//! the agent smart (as opposed to safe — that's the kernel's job).
//!
//! ## Modules (planned)
//!
//! - **skills** — WASM skill loading, sandboxing, capability enforcement
//! - **strategies** — reasoning strategies, recovery strategies, compaction strategies
//! - **config** — runtime configuration management
//! - **templates** — prompt template management
//! - **ab_slots** — A/B slot lifecycle (pending → active → fallback)

pub mod ab_slots;
pub mod config;
pub mod skills;
pub mod strategies;
pub mod templates;

/// A strategy identifier for A/B slot management.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StrategyId(pub String);

/// A/B slot state for any loadable component.
#[derive(Debug, Clone)]
pub enum SlotState {
    /// Only one version exists (no A/B test active).
    Single,
    /// Two versions being compared.
    Testing {
        active: StrategyId,
        pending: StrategyId,
        tasks_evaluated: u32,
    },
}
