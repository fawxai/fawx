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

pub(crate) mod ab_slots;
pub(crate) mod config;
pub(crate) mod skills;
pub(crate) mod strategies;
pub(crate) mod templates;

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

#[cfg(test)]
mod tests {
    #[test]
    fn stub_modules_track_shared_implementation_issue() {
        let expected = "https://github.com/abbudjoe/citros/issues/860";

        assert!(include_str!("ab_slots.rs").contains(expected));
        assert!(include_str!("config.rs").contains(expected));
        assert!(include_str!("skills.rs").contains(expected));
        assert!(include_str!("strategies.rs").contains(expected));
        assert!(include_str!("templates.rs").contains(expected));
    }
}
