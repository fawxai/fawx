//! # fx-loadable — Fawx Loadable Layer
//!
//! The hot-swappable intelligence layer: skill registry, plugin loading,
//! strategy management, configuration, prompt templates, and A/B slot lifecycle.
//! Everything that makes the agent smart (as opposed to safe — that’s the
//! kernel’s job).
//!
//! ## Modules
//!
//! - **skill** — `Skill` trait: tool definitions + execution
//! - **registry** — `SkillRegistry`: aggregates skills, implements `ToolExecutor`
//! - **loader** — `SkillLoader`: discovers skill manifests from directory
//! - **builtin** — `BuiltinSkill`: wraps existing tools for registry compatibility
//!
//! ## Stub modules (planned)
//!
//! - **strategies** — reasoning strategies, recovery strategies, compaction strategies
//! - **config** — runtime configuration management
//! - **templates** — prompt template management
//! - **ab_slots** — A/B slot lifecycle (pending → active → fallback)

pub mod builtin;
pub mod lifecycle;
pub mod loader;
pub mod notify_skill;
pub mod registry;
pub mod session_memory_skill;
pub mod skill;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod transaction_skill;
pub mod wasm_host;
pub mod wasm_skill;
pub mod watcher;

pub(crate) mod ab_slots;
pub(crate) mod config;
pub(crate) mod skills;
pub(crate) mod strategies;
pub(crate) mod templates;

pub use builtin::BuiltinSkill;
pub use lifecycle::{
    find_revision_snapshot_dir, read_activation_record, read_revision_source_metadata,
    read_statuses as read_skill_statuses, revision_snapshot_dir, write_source_metadata,
    SignatureStatus, SkillActivation, SkillLifecycleConfig, SkillLifecycleManager, SkillRevision,
    SkillSource, SkillStatusSummary, SOURCE_METADATA_FILE,
};
pub use loader::{SkillLoader, SkillManifest};
pub use notify_skill::{NotificationSender, NotifySkill};
pub use registry::SkillRegistry;
pub use session_memory_skill::SessionMemorySkill;
pub use skill::{Skill, SkillError};
pub use transaction_skill::TransactionSkill;
pub use wasm_skill::SignaturePolicy;
pub use watcher::{ReloadEvent, SkillWatcher};

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
        let expected = "https://github.com/fawxai/fawx/issues/860";

        assert!(include_str!("ab_slots.rs").contains(expected));
        assert!(include_str!("config.rs").contains(expected));
        assert!(include_str!("skills.rs").contains(expected));
        assert!(include_str!("strategies.rs").contains(expected));
        assert!(include_str!("templates.rs").contains(expected));
    }
}
