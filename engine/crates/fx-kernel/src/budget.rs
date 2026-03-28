//! Budget accounting and guardrails for the Decide step budget gate.

use crate::types::*;
use fx_decompose::{ComplexityHint, SubGoal};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[cfg(test)]
use fx_decompose::SubGoalContract;

/// Budget state for soft-ceiling awareness.
///
/// Only two states. `Exhausted` is already handled by the existing
/// `BudgetExceeded` / `LoopResult::BudgetExhausted` path — no need
/// to duplicate that in the enum (YAGNI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BudgetState {
    /// Below soft-ceiling. Full capabilities.
    Normal,
    /// Soft-ceiling crossed. Wrap-up mode: no tools, no decompose, synthesize only.
    Low,
}

const DEFAULT_SOFT_CEILING_PERCENT: u8 = 80;
const DEFAULT_MAX_FAN_OUT: usize = 4;
const DEFAULT_MAX_TOOL_RESULT_BYTES: usize = 16_384;
const DEFAULT_MAX_AGGREGATE_RESULT_BYTES: usize = 400_000;
const DEFAULT_MAX_SYNTHESIS_TOKENS: usize = 50_000;
const DEFAULT_LLM_CALL_TOKENS: u64 = 1_000;
pub(crate) const DEFAULT_LLM_CALL_COST_CENTS: u64 = 2;
pub(crate) const DEFAULT_TOOL_INVOCATION_COST_CENTS: u64 = 1;
const DEFAULT_MAX_CONSECUTIVE_FAILURES: u16 = 3;
const DEFAULT_MAX_CYCLE_FAILURES: u16 = 15;
const DEFAULT_MAX_NO_PROGRESS: u16 = 3;
#[cfg(test)]
const DEFAULT_MAX_TOOL_RETRIES: u8 = 2;
const COMPLEXITY_KEYWORDS: [&str; 6] = [
    "analyze",
    "refactor",
    "implement",
    "redesign",
    "migrate",
    "rewrite",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum DepthMode {
    Static,
    #[default]
    Adaptive,
}

/// Retry policy configuration for tool execution within a single cycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryPolicyConfig {
    /// Max consecutive failures on the same (tool, args) before blocking.
    #[serde(default = "default_max_consecutive_failures")]
    pub max_consecutive_failures: u16,
    /// Max total failures across all tools before circuit-breaking the cycle.
    #[serde(default = "default_max_cycle_failures")]
    pub max_cycle_failures: u16,
    /// Max times the same (tool, args) can return the same result before blocking.
    #[serde(default = "default_max_no_progress")]
    pub max_no_progress: u16,
}

impl RetryPolicyConfig {
    pub fn conservative() -> Self {
        Self {
            max_consecutive_failures: 2,
            max_cycle_failures: 8,
            max_no_progress: 2,
        }
    }

    pub fn permissive() -> Self {
        Self {
            max_consecutive_failures: 10,
            max_cycle_failures: 50,
            max_no_progress: 5,
        }
    }

    pub fn unlimited() -> Self {
        Self {
            max_consecutive_failures: u16::MAX,
            max_cycle_failures: u16::MAX,
            max_no_progress: u16::MAX,
        }
    }
}

impl Default for RetryPolicyConfig {
    fn default() -> Self {
        Self {
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
        }
    }
}

/// Budget configuration for a single loop invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(from = "BudgetConfigSerde")]
pub struct BudgetConfig {
    /// Maximum number of LLM calls allowed.
    pub max_llm_calls: u32,
    /// Maximum number of tool invocations allowed.
    pub max_tool_invocations: u32,
    /// Maximum number of tokens allowed.
    pub max_tokens: u64,
    /// Maximum cost in cents (integer to avoid floating-point precision issues).
    pub max_cost_cents: u64,
    /// Maximum wall-clock time in milliseconds.
    pub max_wall_time_ms: u64,
    /// Kernel-enforced maximum recursion depth.
    pub max_recursion_depth: u32,
    /// Runtime mode for decomposition depth limiting.
    #[serde(default)]
    pub decompose_depth_mode: DepthMode,
    /// Percentage of cost/LLM-call budget at which soft-ceiling triggers (0–100).
    /// Stored as integer to preserve `Eq` derivation; converted to f64 only in
    /// [`BudgetTracker::state()`] computation.
    #[serde(default = "default_soft_ceiling_percent")]
    pub soft_ceiling_percent: u8,
    /// Maximum tool calls executed per LLM response (fan-out cap).
    #[serde(default = "default_max_fan_out")]
    pub max_fan_out: usize,
    /// Maximum bytes per individual tool result before truncation.
    #[serde(default = "default_max_tool_result_bytes")]
    pub max_tool_result_bytes: usize,
    /// Maximum aggregate tool result bytes before triggering `BudgetState::Low`.
    ///
    /// NTH2: Currently a static default. When per-provider model context window
    /// detection is added, this should be derived as a fraction of the model's
    /// reported context limit (e.g., 50–75%) to automatically scale with the
    /// available context budget.
    #[serde(default = "default_max_aggregate_result_bytes")]
    pub max_aggregate_result_bytes: usize,
    /// Maximum tokens to include in the synthesis prompt (Layer 2 eviction limit).
    #[serde(default = "default_max_synthesis_tokens")]
    pub max_synthesis_tokens: usize,
    /// Max consecutive failures on the same (tool, args) before blocking.
    #[serde(default = "default_max_consecutive_failures")]
    pub max_consecutive_failures: u16,
    /// Max total failures across all tools before circuit-breaking the cycle.
    #[serde(default = "default_max_cycle_failures")]
    pub max_cycle_failures: u16,
    /// Max times the same (tool, args) can return the same result before blocking.
    #[serde(default = "default_max_no_progress")]
    pub max_no_progress: u16,
    /// Legacy compatibility field for configs that still set `max_tool_retries`.
    /// Total attempts in the old model = `max_tool_retries + 1`.
    #[serde(default = "default_max_tool_retries")]
    pub max_tool_retries: u8,
    /// Controls graceful termination behavior when budget limits fire and
    /// how tool-turn runs are handled.
    #[serde(default)]
    pub termination: TerminationConfig,
}

/// Controls how the loop exits when a budget limit fires and how tool-use
/// runs are managed across cycles and within continuation rounds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminationConfig {
    /// When true, make one final LLM call with tools stripped to force a
    /// text response before returning `BudgetExhausted`.
    #[serde(default = "default_synthesize_on_exhaustion")]
    pub synthesize_on_exhaustion: bool,

    /// Consecutive tool turns before injecting a nudge message telling the
    /// agent to respond to the user. 0 disables the nudge.
    #[serde(default = "default_nudge_after_tool_turns")]
    pub nudge_after_tool_turns: u16,

    /// Additional consecutive tool turns *after the nudge fires* before tools
    /// are stripped entirely, forcing a text response. 0 means strip
    /// immediately when the nudge threshold is reached. Set to `u16::MAX`
    /// to disable stripping while keeping the nudge.
    #[serde(default = "default_strip_tools_after_nudge")]
    pub strip_tools_after_nudge: u16,

    /// Tool continuation rounds before injecting a progress nudge. 0 disables
    /// both the nudge and the follow-up strip enforcement.
    #[serde(default = "default_tool_round_nudge_after")]
    pub tool_round_nudge_after: u16,

    /// Additional continuation rounds after the nudge before tools are
    /// stripped, forcing a text response.
    #[serde(default = "default_tool_round_strip_after_nudge")]
    pub tool_round_strip_after_nudge: u16,

    /// Consecutive observation-only tool rounds before injecting a targeted
    /// nudge telling the agent to stop researching and either implement or
    /// return an incomplete response.
    #[serde(default = "default_observation_only_round_nudge_after")]
    pub observation_only_round_nudge_after: u16,

    /// Additional observation-only rounds after the targeted nudge before the
    /// loop strips observation-only tools, leaving only side-effecting tools.
    #[serde(default = "default_observation_only_round_strip_after_nudge")]
    pub observation_only_round_strip_after_nudge: u16,
}

fn default_synthesize_on_exhaustion() -> bool {
    true
}
fn default_nudge_after_tool_turns() -> u16 {
    6
}
fn default_strip_tools_after_nudge() -> u16 {
    3
}
fn default_tool_round_nudge_after() -> u16 {
    4
}
fn default_tool_round_strip_after_nudge() -> u16 {
    2
}
fn default_observation_only_round_nudge_after() -> u16 {
    2
}
fn default_observation_only_round_strip_after_nudge() -> u16 {
    1
}

impl Default for TerminationConfig {
    fn default() -> Self {
        Self {
            synthesize_on_exhaustion: default_synthesize_on_exhaustion(),
            nudge_after_tool_turns: default_nudge_after_tool_turns(),
            strip_tools_after_nudge: default_strip_tools_after_nudge(),
            tool_round_nudge_after: default_tool_round_nudge_after(),
            tool_round_strip_after_nudge: default_tool_round_strip_after_nudge(),
            observation_only_round_nudge_after: default_observation_only_round_nudge_after(),
            observation_only_round_strip_after_nudge:
                default_observation_only_round_strip_after_nudge(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct BudgetConfigSerde {
    max_llm_calls: u32,
    max_tool_invocations: u32,
    max_tokens: u64,
    max_cost_cents: u64,
    max_wall_time_ms: u64,
    max_recursion_depth: u32,
    #[serde(default)]
    decompose_depth_mode: DepthMode,
    #[serde(default = "default_soft_ceiling_percent")]
    soft_ceiling_percent: u8,
    #[serde(default = "default_max_fan_out")]
    max_fan_out: usize,
    #[serde(default = "default_max_tool_result_bytes")]
    max_tool_result_bytes: usize,
    #[serde(default = "default_max_aggregate_result_bytes")]
    max_aggregate_result_bytes: usize,
    #[serde(default = "default_max_synthesis_tokens")]
    max_synthesis_tokens: usize,
    #[serde(default = "default_max_consecutive_failures")]
    max_consecutive_failures: u16,
    #[serde(default = "default_max_cycle_failures")]
    max_cycle_failures: u16,
    #[serde(default = "default_max_no_progress")]
    max_no_progress: u16,
    #[serde(default)]
    max_tool_retries: Option<u8>,
    #[serde(default)]
    termination: TerminationConfig,
}

impl From<BudgetConfigSerde> for BudgetConfig {
    fn from(value: BudgetConfigSerde) -> Self {
        let max_consecutive_failures = value
            .max_tool_retries
            .map(max_consecutive_failures_from_legacy_retries)
            .unwrap_or(value.max_consecutive_failures);
        let max_tool_retries = value
            .max_tool_retries
            .unwrap_or_else(|| legacy_retries_from_consecutive_failures(max_consecutive_failures));

        Self {
            max_llm_calls: value.max_llm_calls,
            max_tool_invocations: value.max_tool_invocations,
            max_tokens: value.max_tokens,
            max_cost_cents: value.max_cost_cents,
            max_wall_time_ms: value.max_wall_time_ms,
            max_recursion_depth: value.max_recursion_depth,
            decompose_depth_mode: value.decompose_depth_mode,
            soft_ceiling_percent: value.soft_ceiling_percent,
            max_fan_out: value.max_fan_out,
            max_tool_result_bytes: value.max_tool_result_bytes,
            max_aggregate_result_bytes: value.max_aggregate_result_bytes,
            max_synthesis_tokens: value.max_synthesis_tokens,
            max_consecutive_failures,
            max_cycle_failures: value.max_cycle_failures,
            max_no_progress: value.max_no_progress,
            max_tool_retries,
            termination: value.termination,
        }
    }
}

impl From<&BudgetConfig> for RetryPolicyConfig {
    fn from(value: &BudgetConfig) -> Self {
        Self {
            max_consecutive_failures: value.max_consecutive_failures,
            max_cycle_failures: value.max_cycle_failures,
            max_no_progress: value.max_no_progress,
        }
    }
}

fn default_soft_ceiling_percent() -> u8 {
    DEFAULT_SOFT_CEILING_PERCENT
}

fn default_max_fan_out() -> usize {
    DEFAULT_MAX_FAN_OUT
}

fn default_max_tool_result_bytes() -> usize {
    DEFAULT_MAX_TOOL_RESULT_BYTES
}

fn default_max_aggregate_result_bytes() -> usize {
    DEFAULT_MAX_AGGREGATE_RESULT_BYTES
}

fn default_max_synthesis_tokens() -> usize {
    DEFAULT_MAX_SYNTHESIS_TOKENS
}

fn default_max_consecutive_failures() -> u16 {
    DEFAULT_MAX_CONSECUTIVE_FAILURES
}

fn default_max_cycle_failures() -> u16 {
    DEFAULT_MAX_CYCLE_FAILURES
}

fn default_max_no_progress() -> u16 {
    DEFAULT_MAX_NO_PROGRESS
}

fn default_max_tool_retries() -> u8 {
    legacy_retries_from_consecutive_failures(DEFAULT_MAX_CONSECUTIVE_FAILURES)
}

fn max_consecutive_failures_from_legacy_retries(max_tool_retries: u8) -> u16 {
    u16::from(max_tool_retries).saturating_add(1)
}

fn legacy_retries_from_consecutive_failures(max_consecutive_failures: u16) -> u8 {
    max_consecutive_failures
        .saturating_sub(1)
        .min(u16::from(u8::MAX)) as u8
}

impl BudgetConfig {
    /// Return a conservative configuration for background/proactive actions.
    pub fn conservative() -> Self {
        let retry_policy = RetryPolicyConfig::conservative();
        Self {
            max_llm_calls: 8,
            max_tool_invocations: 16,
            max_tokens: 25_000,
            max_cost_cents: 100,
            max_wall_time_ms: 2 * 60 * 1000,
            max_recursion_depth: 3,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: retry_policy.max_consecutive_failures,
            max_cycle_failures: retry_policy.max_cycle_failures,
            max_no_progress: retry_policy.max_no_progress,
            max_tool_retries: legacy_retries_from_consecutive_failures(
                retry_policy.max_consecutive_failures,
            ),
            termination: TerminationConfig::default(),
        }
    }

    /// Return a more permissive retry policy for exploratory loops.
    pub fn permissive() -> Self {
        let retry_policy = RetryPolicyConfig::permissive();
        Self {
            max_consecutive_failures: retry_policy.max_consecutive_failures,
            max_cycle_failures: retry_policy.max_cycle_failures,
            max_no_progress: retry_policy.max_no_progress,
            max_tool_retries: legacy_retries_from_consecutive_failures(
                retry_policy.max_consecutive_failures,
            ),
            ..Self::default()
        }
    }

    /// Return effectively unlimited limits for testing.
    ///
    /// This should not be used in production runtime paths.
    pub fn unlimited() -> Self {
        let retry_policy = RetryPolicyConfig::unlimited();
        Self {
            max_llm_calls: u32::MAX,
            max_tool_invocations: u32::MAX,
            max_tokens: u64::MAX,
            max_cost_cents: u64::MAX,
            max_wall_time_ms: u64::MAX,
            max_recursion_depth: u32::MAX,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: 100,
            max_fan_out: usize::MAX,
            max_tool_result_bytes: usize::MAX,
            max_aggregate_result_bytes: usize::MAX,
            max_synthesis_tokens: usize::MAX,
            max_consecutive_failures: retry_policy.max_consecutive_failures,
            max_cycle_failures: retry_policy.max_cycle_failures,
            max_no_progress: retry_policy.max_no_progress,
            max_tool_retries: legacy_retries_from_consecutive_failures(
                retry_policy.max_consecutive_failures,
            ),
            termination: TerminationConfig::default(),
        }
    }

    pub fn retry_policy(&self) -> RetryPolicyConfig {
        RetryPolicyConfig::from(self)
    }
}

impl Default for BudgetConfig {
    /// Return a generous default for normal user-initiated loops.
    fn default() -> Self {
        let retry_policy = RetryPolicyConfig::default();
        let max_tool_retries =
            legacy_retries_from_consecutive_failures(DEFAULT_MAX_CONSECUTIVE_FAILURES);
        debug_assert_eq!(max_tool_retries, default_max_tool_retries());
        Self {
            max_llm_calls: 64,
            max_tool_invocations: 128,
            max_tokens: 250_000,
            max_cost_cents: 500,
            max_wall_time_ms: 15 * 60 * 1000,
            max_recursion_depth: 8,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: retry_policy.max_consecutive_failures,
            max_cycle_failures: retry_policy.max_cycle_failures,
            max_no_progress: retry_policy.max_no_progress,
            max_tool_retries,
            termination: TerminationConfig::default(),
        }
    }
}

/// Allocation policy selector.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AllocationMode {
    Sequential,
    Concurrent,
}

/// Minimum viable budget for executing a sub-goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BudgetFloor {
    pub min_llm_calls: u32,
    pub min_tool_invocations: u32,
    pub min_tokens: u64,
    pub min_cost_cents: u64,
    pub min_wall_time_ms: u64,
}

impl Default for BudgetFloor {
    fn default() -> Self {
        Self {
            min_llm_calls: 2,
            min_tool_invocations: 2,
            min_tokens: 1_000,
            min_cost_cents: 4,
            min_wall_time_ms: 5_000,
        }
    }
}

/// Result of decomposition budget planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationPlan {
    /// Budget config for each sub-goal, in original order.
    pub sub_goal_budgets: Vec<BudgetConfig>,
    /// Budget intentionally reserved for parent continuation.
    pub parent_continuation_budget: BudgetConfig,
    /// Indices that should not execute because they are below floor.
    pub skipped_indices: Vec<usize>,
}

/// Infallible allocator for decomposition budgets.
#[derive(Debug, Clone)]
pub struct BudgetAllocator {
    /// Fraction of remaining resources reserved for parent continuation.
    pub parent_continuation_fraction: f32,
    /// Minimum viable budget floor.
    pub floor: BudgetFloor,
}

impl Default for BudgetAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl BudgetAllocator {
    pub fn new() -> Self {
        Self {
            parent_continuation_fraction: 0.10,
            floor: BudgetFloor::default(),
        }
    }

    pub fn allocate(
        &self,
        parent: &BudgetTracker,
        sub_goals: &[SubGoal],
        mode: AllocationMode,
        current_time_ms: u64,
    ) -> AllocationPlan {
        // V1: both modes use snapshot-based allocation; match kept for exhaustiveness.
        match mode {
            AllocationMode::Sequential | AllocationMode::Concurrent => {}
        }

        let remaining = parent.remaining(current_time_ms);
        let reserved = self.reserve_parent_continuation(&remaining);
        let parent_budget = budget_from_remaining(parent.config(), &reserved);
        if sub_goals.is_empty() {
            return AllocationPlan {
                sub_goal_budgets: Vec::new(),
                parent_continuation_budget: parent_budget,
                skipped_indices: Vec::new(),
            };
        }

        let weights = self.compute_weights(sub_goals);
        let distributable = subtract_remaining(&remaining, &reserved);
        let mut sub_goal_budgets =
            self.distribute_by_weight(parent.config(), &distributable, &weights);
        let (skipped_indices, reclaimed) = self.enforce_floor(&mut sub_goal_budgets);
        self.redistribute_skipped(
            &mut sub_goal_budgets,
            &weights,
            &skipped_indices,
            &reclaimed,
        );

        AllocationPlan {
            sub_goal_budgets,
            parent_continuation_budget: parent_budget,
            skipped_indices,
        }
    }

    fn compute_weights(&self, sub_goals: &[SubGoal]) -> Vec<u32> {
        sub_goals
            .iter()
            .map(|sub_goal| {
                sub_goal
                    .complexity_hint
                    .unwrap_or_else(|| estimate_complexity(sub_goal))
                    .weight()
            })
            .collect()
    }

    fn distribute_by_weight(
        &self,
        template: &BudgetConfig,
        distributable: &BudgetRemaining,
        weights: &[u32],
    ) -> Vec<BudgetConfig> {
        let llm_calls = distribute_u32(distributable.llm_calls, weights);
        let tool_invocations = distribute_u32(distributable.tool_invocations, weights);
        let tokens = distribute_u64(distributable.tokens, weights);
        let cost_cents = distribute_u64(distributable.cost_cents, weights);
        let wall_time_ms = distribute_u64(distributable.wall_time_ms, weights);

        let mut budgets = Vec::with_capacity(weights.len());
        for index in 0..weights.len() {
            budgets.push(BudgetConfig {
                max_llm_calls: llm_calls.get(index).copied().unwrap_or_default(),
                max_tool_invocations: tool_invocations.get(index).copied().unwrap_or_default(),
                max_tokens: tokens.get(index).copied().unwrap_or_default(),
                max_cost_cents: cost_cents.get(index).copied().unwrap_or_default(),
                max_wall_time_ms: wall_time_ms.get(index).copied().unwrap_or_default(),
                max_recursion_depth: template.max_recursion_depth,
                decompose_depth_mode: template.decompose_depth_mode,
                soft_ceiling_percent: template.soft_ceiling_percent,
                max_fan_out: template.max_fan_out,
                max_tool_result_bytes: template.max_tool_result_bytes,
                max_aggregate_result_bytes: template.max_aggregate_result_bytes,
                max_synthesis_tokens: template.max_synthesis_tokens,
                max_consecutive_failures: template.max_consecutive_failures,
                max_cycle_failures: template.max_cycle_failures,
                max_no_progress: template.max_no_progress,
                max_tool_retries: template.max_tool_retries,
                termination: template.termination.clone(),
            });
        }

        budgets
    }

    fn enforce_floor(&self, budgets: &mut [BudgetConfig]) -> (Vec<usize>, BudgetRemaining) {
        let mut skipped_indices = Vec::new();
        let mut reclaimed = BudgetRemaining::default();

        for (index, budget) in budgets.iter_mut().enumerate() {
            if !self.below_floor(budget) {
                continue;
            }

            skipped_indices.push(index);
            reclaimed.llm_calls = reclaimed.llm_calls.saturating_add(budget.max_llm_calls);
            reclaimed.tool_invocations = reclaimed
                .tool_invocations
                .saturating_add(budget.max_tool_invocations);
            reclaimed.tokens = reclaimed.tokens.saturating_add(budget.max_tokens);
            reclaimed.cost_cents = reclaimed.cost_cents.saturating_add(budget.max_cost_cents);
            reclaimed.wall_time_ms = reclaimed
                .wall_time_ms
                .saturating_add(budget.max_wall_time_ms);

            *budget = zeroed_config_like(budget);
        }

        (skipped_indices, reclaimed)
    }

    fn redistribute_skipped(
        &self,
        budgets: &mut [BudgetConfig],
        weights: &[u32],
        skipped_indices: &[usize],
        reclaimed: &BudgetRemaining,
    ) {
        if skipped_indices.is_empty() {
            return;
        }

        let skip_mask = build_skip_mask(budgets.len(), skipped_indices);
        let recipients = recipient_indices(&skip_mask);
        if recipients.is_empty() {
            return;
        }

        let recipient_weights = recipients
            .iter()
            .map(|&index| weights.get(index).copied().unwrap_or(1))
            .collect::<Vec<_>>();

        let llm_calls = distribute_u32(reclaimed.llm_calls, &recipient_weights);
        let tool_invocations = distribute_u32(reclaimed.tool_invocations, &recipient_weights);
        let tokens = distribute_u64(reclaimed.tokens, &recipient_weights);
        let cost_cents = distribute_u64(reclaimed.cost_cents, &recipient_weights);
        let wall_time_ms = distribute_u64(reclaimed.wall_time_ms, &recipient_weights);

        for (recipient_position, &goal_index) in recipients.iter().enumerate() {
            if let Some(goal_budget) = budgets.get_mut(goal_index) {
                goal_budget.max_llm_calls = goal_budget.max_llm_calls.saturating_add(
                    llm_calls
                        .get(recipient_position)
                        .copied()
                        .unwrap_or_default(),
                );
                goal_budget.max_tool_invocations = goal_budget.max_tool_invocations.saturating_add(
                    tool_invocations
                        .get(recipient_position)
                        .copied()
                        .unwrap_or_default(),
                );
                goal_budget.max_tokens = goal_budget
                    .max_tokens
                    .saturating_add(tokens.get(recipient_position).copied().unwrap_or_default());
                goal_budget.max_cost_cents = goal_budget.max_cost_cents.saturating_add(
                    cost_cents
                        .get(recipient_position)
                        .copied()
                        .unwrap_or_default(),
                );
                goal_budget.max_wall_time_ms = goal_budget.max_wall_time_ms.saturating_add(
                    wall_time_ms
                        .get(recipient_position)
                        .copied()
                        .unwrap_or_default(),
                );
            }
        }
    }

    fn below_floor(&self, budget: &BudgetConfig) -> bool {
        budget.max_llm_calls < self.floor.min_llm_calls
            || budget.max_tool_invocations < self.floor.min_tool_invocations
            || budget.max_tokens < self.floor.min_tokens
            || budget.max_cost_cents < self.floor.min_cost_cents
            || budget.max_wall_time_ms < self.floor.min_wall_time_ms
    }

    fn reserve_parent_continuation(&self, remaining: &BudgetRemaining) -> BudgetRemaining {
        let fraction = bounded_fraction(self.parent_continuation_fraction);

        BudgetRemaining {
            llm_calls: to_u32(share_u64(u64::from(remaining.llm_calls), fraction)),
            tool_invocations: to_u32(share_u64(u64::from(remaining.tool_invocations), fraction)),
            tokens: share_u64(remaining.tokens, fraction),
            cost_cents: share_u64(remaining.cost_cents, fraction),
            wall_time_ms: share_u64(remaining.wall_time_ms, fraction),
        }
    }
}

/// Tracks budget consumption during a loop execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BudgetTracker {
    config: BudgetConfig,
    llm_calls: u32,
    tool_invocations: u32,
    tokens_used: u64,
    cost_cents: u64,
    start_time_ms: u64,
    depth: u32,
    /// Accumulated tool result bytes across all rounds in this cycle.
    /// Triggers `BudgetState::Low` when exceeding `config.max_aggregate_result_bytes`.
    #[serde(default)]
    accumulated_result_bytes: usize,
}

impl BudgetTracker {
    /// Create a new tracker with zero consumption at the given start timestamp.
    ///
    /// `depth` represents the recursion depth of this tracker in the loop tree.
    pub fn new(config: BudgetConfig, start_time_ms: u64, depth: u32) -> Self {
        Self {
            config,
            llm_calls: 0,
            tool_invocations: 0,
            tokens_used: 0,
            cost_cents: 0,
            start_time_ms,
            depth,
            accumulated_result_bytes: 0,
        }
    }

    /// Compute the current budget state based on cost and LLM call consumption.
    ///
    /// Returns `BudgetState::Low` when either `cost_cents` or `llm_calls`
    /// exceeds the soft-ceiling fraction of their respective maximum.
    /// Wall time is intentionally excluded — hitting 80% wall time while
    /// at 10% cost shouldn't force wrap-up.
    pub fn state(&self) -> BudgetState {
        let fraction = f64::from(self.config.soft_ceiling_percent) / 100.0;
        if self.exceeds_fraction(
            u64::from(self.llm_calls),
            u64::from(self.config.max_llm_calls),
            fraction,
        ) {
            return BudgetState::Low;
        }
        if self.exceeds_fraction(self.cost_cents, self.config.max_cost_cents, fraction) {
            return BudgetState::Low;
        }
        if self.accumulated_result_bytes > self.config.max_aggregate_result_bytes {
            return BudgetState::Low;
        }
        BudgetState::Normal
    }

    fn exceeds_fraction(&self, current: u64, max: u64, fraction: f64) -> bool {
        if max == 0 {
            return false;
        }
        let threshold = (max as f64 * fraction) as u64;
        current > threshold
    }

    /// Check whether the proposed action cost can be admitted under current limits.
    ///
    /// This method validates resource limits (LLM calls, tools, tokens, cost, depth)
    /// but does not evaluate wall-time. Use [`BudgetTracker::check_at`] to enforce
    /// wall-time and resource limits together.
    pub fn check(&self, cost: &ActionCost) -> Result<(), BudgetExceeded> {
        self.check_resources(cost)
    }

    /// Check whether the proposed action cost is admissible at a specific time.
    ///
    /// Unlike [`BudgetTracker::check`], this method validates both resource budgets
    /// and wall-time budget in one call.
    pub fn check_at(&self, current_time_ms: u64, cost: &ActionCost) -> Result<(), BudgetExceeded> {
        let elapsed = current_time_ms.saturating_sub(self.start_time_ms);
        if elapsed > self.config.max_wall_time_ms {
            return Err(BudgetExceeded {
                resource: BudgetResource::WallTime,
                limit: self.config.max_wall_time_ms,
                current: elapsed,
                requested: 0,
            });
        }

        self.check_resources(cost)
    }

    /// Record an action cost as consumed budget.
    pub fn record(&mut self, cost: &ActionCost) {
        self.llm_calls = self.llm_calls.saturating_add(cost.llm_calls);
        self.tool_invocations = self.tool_invocations.saturating_add(cost.tool_invocations);
        self.tokens_used = self.tokens_used.saturating_add(cost.tokens);
        self.cost_cents = self.cost_cents.saturating_add(cost.cost_cents);
    }

    /// Fold a child tracker's consumed resources into this tracker.
    pub fn absorb_child_usage(&mut self, child: &BudgetTracker) {
        self.record(&ActionCost {
            llm_calls: child.llm_calls_used(),
            tool_invocations: child.tool_invocations_used(),
            tokens: child.tokens_used(),
            cost_cents: child.cost_cents_used(),
        });
    }

    /// Snapshot remaining budget for each tracked resource at `current_time_ms`.
    pub fn remaining(&self, current_time_ms: u64) -> BudgetRemaining {
        BudgetRemaining {
            llm_calls: self.config.max_llm_calls.saturating_sub(self.llm_calls),
            tool_invocations: self
                .config
                .max_tool_invocations
                .saturating_sub(self.tool_invocations),
            tokens: self.config.max_tokens.saturating_sub(self.tokens_used),
            cost_cents: self.config.max_cost_cents.saturating_sub(self.cost_cents),
            wall_time_ms: self
                .config
                .max_wall_time_ms
                .saturating_sub(current_time_ms.saturating_sub(self.start_time_ms)),
        }
    }

    /// Return true when elapsed wall time is greater than the configured maximum.
    pub fn wall_time_exceeded(&self, current_time_ms: u64) -> bool {
        current_time_ms.saturating_sub(self.start_time_ms) > self.config.max_wall_time_ms
    }

    /// Record tool result bytes for aggregate tracking (Layer 3).
    pub fn record_result_bytes(&mut self, bytes: usize) {
        self.accumulated_result_bytes = self.accumulated_result_bytes.saturating_add(bytes);
    }

    /// Accumulated tool result bytes tracked this cycle.
    pub fn accumulated_result_bytes(&self) -> usize {
        self.accumulated_result_bytes
    }

    /// Reset consumed budget counters for a fresh cycle starting at `start_time_ms`.
    pub fn reset(&mut self, start_time_ms: u64) {
        self.llm_calls = 0;
        self.tool_invocations = 0;
        self.tokens_used = 0;
        self.cost_cents = 0;
        self.accumulated_result_bytes = 0;
        self.start_time_ms = start_time_ms;
    }

    /// Number of LLM calls consumed so far.
    pub fn llm_calls_used(&self) -> u32 {
        self.llm_calls
    }

    /// Number of tool invocations consumed so far.
    pub fn tool_invocations_used(&self) -> u32 {
        self.tool_invocations
    }

    /// Tokens consumed so far.
    pub fn tokens_used(&self) -> u64 {
        self.tokens_used
    }

    /// Cost consumed so far, in cents.
    pub fn cost_cents_used(&self) -> u64 {
        self.cost_cents
    }

    /// Depth to assign to a child tracker spawned from this tracker.
    pub fn child_depth(&self) -> u32 {
        self.depth.saturating_add(1)
    }

    pub(crate) fn config(&self) -> &BudgetConfig {
        &self.config
    }

    pub(crate) fn depth(&self) -> u32 {
        self.depth
    }

    fn check_resources(&self, cost: &ActionCost) -> Result<(), BudgetExceeded> {
        if self.depth >= self.config.max_recursion_depth {
            return Err(BudgetExceeded {
                resource: BudgetResource::RecursionDepth,
                limit: u64::from(self.config.max_recursion_depth),
                current: u64::from(self.depth),
                requested: 1,
            });
        }

        Self::check_limit(
            BudgetResource::LlmCalls,
            u64::from(self.config.max_llm_calls),
            u64::from(self.llm_calls),
            u64::from(cost.llm_calls),
        )?;

        Self::check_limit(
            BudgetResource::ToolInvocations,
            u64::from(self.config.max_tool_invocations),
            u64::from(self.tool_invocations),
            u64::from(cost.tool_invocations),
        )?;

        Self::check_limit(
            BudgetResource::Tokens,
            self.config.max_tokens,
            self.tokens_used,
            cost.tokens,
        )?;

        Self::check_limit(
            BudgetResource::Cost,
            self.config.max_cost_cents,
            self.cost_cents,
            cost.cost_cents,
        )?;

        Ok(())
    }

    fn check_limit(
        resource: BudgetResource,
        limit: u64,
        current: u64,
        requested: u64,
    ) -> Result<(), BudgetExceeded> {
        if current.saturating_add(requested) > limit {
            Err(BudgetExceeded {
                resource,
                limit,
                current,
                requested,
            })
        } else {
            Ok(())
        }
    }
}

/// Cost of a single action (estimated before execution, measured after execution).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ActionCost {
    /// Number of LLM calls.
    pub llm_calls: u32,
    /// Number of tool invocations.
    pub tool_invocations: u32,
    /// Number of tokens consumed.
    pub tokens: u64,
    /// Cost in cents.
    pub cost_cents: u64,
}

/// Remaining budget snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BudgetRemaining {
    /// Remaining LLM calls.
    pub llm_calls: u32,
    /// Remaining tool invocations.
    pub tool_invocations: u32,
    /// Remaining tokens.
    pub tokens: u64,
    /// Remaining cost in cents.
    pub cost_cents: u64,
    /// Remaining wall time in milliseconds.
    pub wall_time_ms: u64,
}

/// Alias used by the loop engine perception pipeline.
pub type BudgetSnapshot = BudgetRemaining;

/// Adaptive decomposition depth cap based on remaining LLM calls.
pub fn effective_max_depth(remaining: &BudgetRemaining) -> u32 {
    match remaining.llm_calls {
        calls if calls > 32 => 3,
        calls if calls > 16 => 2,
        calls if calls > 6 => 1,
        _ => 0,
    }
}

/// Estimate complexity from description and required tools.
pub(crate) fn estimate_complexity(sub_goal: &SubGoal) -> ComplexityHint {
    let description_len = sub_goal.description.chars().count();
    let tools_count = sub_goal.required_tools.len();

    if description_len > 200
        || tools_count >= 3
        || contains_complexity_keyword(&sub_goal.description)
    {
        return ComplexityHint::Complex;
    }

    if description_len < 50 && tools_count == 0 {
        return ComplexityHint::Trivial;
    }

    ComplexityHint::Moderate
}

/// Error returned when a requested action would exceed a budget limit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BudgetExceeded {
    /// Resource that was exceeded.
    pub resource: BudgetResource,
    /// Configured limit.
    pub limit: u64,
    /// Current consumed value.
    pub current: u64,
    /// Requested additional value.
    pub requested: u64,
}

/// Budgeted resource category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BudgetResource {
    /// LLM call count.
    LlmCalls,
    /// Tool invocation count.
    ToolInvocations,
    /// Token consumption.
    Tokens,
    /// Monetary cost.
    Cost,
    /// Wall-clock time.
    WallTime,
    /// Recursive depth.
    RecursionDepth,
}

/// Estimate the budget cost of an intended action using conservative defaults.
pub fn estimate_cost(action: &IntendedAction) -> ActionCost {
    match action {
        IntendedAction::Tap { .. }
        | IntendedAction::Type { .. }
        | IntendedAction::Swipe { .. }
        | IntendedAction::LaunchApp { .. }
        | IntendedAction::Navigate { .. }
        | IntendedAction::Wait { .. } => ActionCost {
            llm_calls: 0,
            tool_invocations: 1,
            tokens: 0,
            cost_cents: DEFAULT_TOOL_INVOCATION_COST_CENTS,
        },
        IntendedAction::Respond { .. } => ActionCost {
            llm_calls: 1,
            tool_invocations: 0,
            tokens: DEFAULT_LLM_CALL_TOKENS,
            cost_cents: DEFAULT_LLM_CALL_COST_CENTS,
        },
        IntendedAction::Delegate { .. } => ActionCost {
            llm_calls: 1,
            tool_invocations: 1,
            tokens: DEFAULT_LLM_CALL_TOKENS,
            cost_cents: DEFAULT_LLM_CALL_COST_CENTS + DEFAULT_TOOL_INVOCATION_COST_CENTS,
        },
        IntendedAction::Composite(actions) => {
            actions.iter().fold(ActionCost::default(), |mut acc, item| {
                let estimate = estimate_cost(item);
                acc.llm_calls = acc.llm_calls.saturating_add(estimate.llm_calls);
                acc.tool_invocations = acc
                    .tool_invocations
                    .saturating_add(estimate.tool_invocations);
                acc.tokens = acc.tokens.saturating_add(estimate.tokens);
                acc.cost_cents = acc.cost_cents.saturating_add(estimate.cost_cents);
                acc
            })
        }
    }
}

fn bounded_fraction(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn share_u64(value: u64, fraction: f32) -> u64 {
    ((value as f64) * f64::from(fraction)).floor() as u64
}

fn distribute_u32(total: u32, weights: &[u32]) -> Vec<u32> {
    distribute_u64(u64::from(total), weights)
        .into_iter()
        .map(to_u32)
        .collect()
}

fn distribute_u64(total: u64, weights: &[u32]) -> Vec<u64> {
    if weights.is_empty() || total == 0 {
        return vec![0; weights.len()];
    }

    let total_weight = weights
        .iter()
        .fold(0_u64, |acc, weight| acc.saturating_add(u64::from(*weight)));
    if total_weight == 0 {
        return vec![0; weights.len()];
    }

    let (allocations, mut ranking, allocated) =
        base_weighted_allocations(total, weights, total_weight);
    let leftover = total.saturating_sub(allocated);
    distribute_remainders(allocations, &mut ranking, leftover)
}

fn base_weighted_allocations(
    total: u64,
    weights: &[u32],
    total_weight: u64,
) -> (Vec<u64>, Vec<(usize, u32, u64)>, u64) {
    let mut allocations = vec![0_u64; weights.len()];
    let mut ranking = Vec::with_capacity(weights.len());
    let mut allocated = 0_u64;

    for (index, weight) in weights.iter().enumerate() {
        let numerator = total.saturating_mul(u64::from(*weight));
        let base = numerator / total_weight;
        let remainder = numerator % total_weight;
        allocations[index] = base;
        allocated = allocated.saturating_add(base);
        ranking.push((index, *weight, remainder));
    }

    (allocations, ranking, allocated)
}

fn distribute_remainders(
    mut allocations: Vec<u64>,
    ranking: &mut [(usize, u32, u64)],
    mut leftover: u64,
) -> Vec<u64> {
    ranking.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });

    if ranking.is_empty() {
        return allocations;
    }

    let recipient_count = ranking.len() as u64;
    if leftover > recipient_count {
        let even_share = leftover / recipient_count;
        for (index, _, _) in ranking.iter() {
            if let Some(value) = allocations.get_mut(*index) {
                *value = value.saturating_add(even_share);
            }
        }
        leftover %= recipient_count;
    }

    let mut cursor = 0_usize;
    while leftover > 0 {
        let index = ranking[cursor].0;
        if let Some(value) = allocations.get_mut(index) {
            *value = value.saturating_add(1);
        }
        leftover = leftover.saturating_sub(1);
        cursor = (cursor + 1) % ranking.len();
    }

    allocations
}

fn to_u32(value: u64) -> u32 {
    if value > u64::from(u32::MAX) {
        u32::MAX
    } else {
        value as u32
    }
}

fn contains_complexity_keyword(description: &str) -> bool {
    let normalized = description.to_ascii_lowercase();
    normalized
        .split(|ch: char| !ch.is_ascii_lowercase() && !ch.is_ascii_digit())
        .filter(|token| !token.is_empty())
        .any(is_complexity_keyword)
}

fn is_complexity_keyword(token: &str) -> bool {
    COMPLEXITY_KEYWORDS.contains(&token)
}

fn subtract_remaining(total: &BudgetRemaining, reserved: &BudgetRemaining) -> BudgetRemaining {
    BudgetRemaining {
        llm_calls: total.llm_calls.saturating_sub(reserved.llm_calls),
        tool_invocations: total
            .tool_invocations
            .saturating_sub(reserved.tool_invocations),
        tokens: total.tokens.saturating_sub(reserved.tokens),
        cost_cents: total.cost_cents.saturating_sub(reserved.cost_cents),
        wall_time_ms: total.wall_time_ms.saturating_sub(reserved.wall_time_ms),
    }
}

fn budget_from_remaining(template: &BudgetConfig, remaining: &BudgetRemaining) -> BudgetConfig {
    BudgetConfig {
        max_llm_calls: remaining.llm_calls,
        max_tool_invocations: remaining.tool_invocations,
        max_tokens: remaining.tokens,
        max_cost_cents: remaining.cost_cents,
        max_wall_time_ms: remaining.wall_time_ms,
        max_recursion_depth: template.max_recursion_depth,
        decompose_depth_mode: template.decompose_depth_mode,
        soft_ceiling_percent: template.soft_ceiling_percent,
        max_fan_out: template.max_fan_out,
        max_tool_result_bytes: template.max_tool_result_bytes,
        max_aggregate_result_bytes: template.max_aggregate_result_bytes,
        max_synthesis_tokens: template.max_synthesis_tokens,
        max_consecutive_failures: template.max_consecutive_failures,
        max_cycle_failures: template.max_cycle_failures,
        max_no_progress: template.max_no_progress,
        max_tool_retries: template.max_tool_retries,
        termination: template.termination.clone(),
    }
}

fn zeroed_config_like(template: &BudgetConfig) -> BudgetConfig {
    BudgetConfig {
        max_llm_calls: 0,
        max_tool_invocations: 0,
        max_tokens: 0,
        max_cost_cents: 0,
        max_wall_time_ms: 0,
        max_recursion_depth: template.max_recursion_depth,
        decompose_depth_mode: template.decompose_depth_mode,
        soft_ceiling_percent: template.soft_ceiling_percent,
        max_fan_out: template.max_fan_out,
        max_tool_result_bytes: template.max_tool_result_bytes,
        max_aggregate_result_bytes: template.max_aggregate_result_bytes,
        max_synthesis_tokens: template.max_synthesis_tokens,
        max_consecutive_failures: template.max_consecutive_failures,
        max_cycle_failures: template.max_cycle_failures,
        max_no_progress: template.max_no_progress,
        max_tool_retries: template.max_tool_retries,
        termination: template.termination.clone(),
    }
}

/// Truncate a tool result string to `max_bytes`, appending a marker.
///
/// If the result is within the limit, returns it unchanged.
/// If empty, returns it unchanged.
pub fn truncate_tool_result<'a>(output: &'a str, max_bytes: usize) -> Cow<'a, str> {
    if output.is_empty() || output.len() <= max_bytes {
        return Cow::Borrowed(output);
    }
    let total_bytes = output.len();
    let remaining_bytes = total_bytes.saturating_sub(max_bytes);
    // Truncate at a char boundary at or before max_bytes.
    let truncated = truncate_at_char_boundary(output, max_bytes);
    Cow::Owned(format!(
        "{truncated}\n[truncated — {remaining_bytes} bytes omitted, {total_bytes} total]"
    ))
}

fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if max_bytes >= s.len() {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub(crate) fn build_skip_mask(total: usize, skipped_indices: &[usize]) -> Vec<bool> {
    let mut mask = vec![false; total];
    for &index in skipped_indices {
        if let Some(entry) = mask.get_mut(index) {
            *entry = true;
        }
    }
    mask
}

fn recipient_indices(skip_mask: &[bool]) -> Vec<usize> {
    skip_mask
        .iter()
        .enumerate()
        .filter_map(|(index, is_skipped)| if *is_skipped { None } else { Some(index) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_config() -> BudgetConfig {
        BudgetConfig {
            max_llm_calls: 10,
            max_tool_invocations: 20,
            max_tokens: 10_000,
            max_cost_cents: 100,
            max_wall_time_ms: 10_000,
            max_recursion_depth: 4,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        }
    }

    fn zero_floor() -> BudgetFloor {
        BudgetFloor {
            min_llm_calls: 0,
            min_tool_invocations: 0,
            min_tokens: 0,
            min_cost_cents: 0,
            min_wall_time_ms: 0,
        }
    }

    fn allocation_config() -> BudgetConfig {
        BudgetConfig {
            max_llm_calls: 100,
            max_tool_invocations: 100,
            max_tokens: 20_000,
            max_cost_cents: 200,
            max_wall_time_ms: 100_000,
            max_recursion_depth: 6,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        }
    }

    fn sub_goal(
        description: &str,
        required_tools: &[&str],
        hint: Option<ComplexityHint>,
    ) -> SubGoal {
        SubGoal {
            description: description.to_string(),
            required_tools: required_tools
                .iter()
                .map(|tool| (*tool).to_string())
                .collect(),
            completion_contract: SubGoalContract::from_definition_of_done(None),
            complexity_hint: hint,
        }
    }

    fn sum_llm_calls(budgets: &[BudgetConfig]) -> u32 {
        budgets.iter().fold(0_u32, |acc, budget| {
            acc.saturating_add(budget.max_llm_calls)
        })
    }

    fn sum_tool_calls(budgets: &[BudgetConfig]) -> u32 {
        budgets.iter().fold(0_u32, |acc, budget| {
            acc.saturating_add(budget.max_tool_invocations)
        })
    }

    fn sum_tokens(budgets: &[BudgetConfig]) -> u64 {
        budgets
            .iter()
            .fold(0_u64, |acc, budget| acc.saturating_add(budget.max_tokens))
    }

    fn sum_cost_cents(budgets: &[BudgetConfig]) -> u64 {
        budgets.iter().fold(0_u64, |acc, budget| {
            acc.saturating_add(budget.max_cost_cents)
        })
    }

    fn sum_wall_time_ms(budgets: &[BudgetConfig]) -> u64 {
        budgets.iter().fold(0_u64, |acc, budget| {
            acc.saturating_add(budget.max_wall_time_ms)
        })
    }

    fn resolved_depth_cap(config: &BudgetConfig, remaining: &BudgetRemaining) -> u32 {
        match config.decompose_depth_mode {
            DepthMode::Static => config.max_recursion_depth,
            DepthMode::Adaptive => config
                .max_recursion_depth
                .min(effective_max_depth(remaining)),
        }
    }

    #[test]
    fn tracker_creation_initializes_zero_consumption() {
        let config = test_config();
        let tracker = BudgetTracker::new(config.clone(), 1_000, 2);

        assert_eq!(tracker.config, config);
        assert_eq!(tracker.llm_calls, 0);
        assert_eq!(tracker.tool_invocations, 0);
        assert_eq!(tracker.tokens_used, 0);
        assert_eq!(tracker.cost_cents, 0);
        assert_eq!(tracker.start_time_ms, 1_000);
        assert_eq!(tracker.depth, 2);

        assert_eq!(
            tracker.remaining(1_000),
            BudgetRemaining {
                llm_calls: 10,
                tool_invocations: 20,
                tokens: 10_000,
                cost_cents: 100,
                wall_time_ms: 10_000,
            }
        );
    }

    #[test]
    fn check_passes_when_cost_is_within_budget() {
        let tracker = BudgetTracker::new(test_config(), 0, 0);

        let cost = ActionCost {
            llm_calls: 1,
            tool_invocations: 2,
            tokens: 500,
            cost_cents: 5,
        };

        assert!(tracker.check(&cost).is_ok());
    }

    #[test]
    fn check_passes_at_exact_limit_boundary() {
        let mut tracker = BudgetTracker::new(test_config(), 0, 0);
        tracker.llm_calls = 9;
        tracker.tool_invocations = 18;
        tracker.tokens_used = 9_500;
        tracker.cost_cents = 95;

        let cost = ActionCost {
            llm_calls: 1,
            tool_invocations: 2,
            tokens: 500,
            cost_cents: 5,
        };

        assert!(tracker.check(&cost).is_ok());
    }

    #[test]
    fn check_fails_when_llm_calls_exceed_limit() {
        let mut tracker = BudgetTracker::new(test_config(), 0, 0);
        tracker.llm_calls = 10;

        let result = tracker.check(&ActionCost {
            llm_calls: 1,
            tool_invocations: 0,
            tokens: 0,
            cost_cents: 0,
        });

        assert_eq!(
            result,
            Err(BudgetExceeded {
                resource: BudgetResource::LlmCalls,
                limit: 10,
                current: 10,
                requested: 1,
            })
        );
    }

    #[test]
    fn check_fails_when_tool_invocations_exceed_limit() {
        let mut tracker = BudgetTracker::new(test_config(), 0, 0);
        tracker.tool_invocations = 20;

        let result = tracker.check(&ActionCost {
            llm_calls: 0,
            tool_invocations: 1,
            tokens: 0,
            cost_cents: 0,
        });

        assert_eq!(
            result,
            Err(BudgetExceeded {
                resource: BudgetResource::ToolInvocations,
                limit: 20,
                current: 20,
                requested: 1,
            })
        );
    }

    #[test]
    fn check_fails_when_tokens_exceed_limit() {
        let mut tracker = BudgetTracker::new(test_config(), 0, 0);
        tracker.tokens_used = 9_500;

        let result = tracker.check(&ActionCost {
            llm_calls: 0,
            tool_invocations: 0,
            tokens: 600,
            cost_cents: 0,
        });

        assert_eq!(
            result,
            Err(BudgetExceeded {
                resource: BudgetResource::Tokens,
                limit: 10_000,
                current: 9_500,
                requested: 600,
            })
        );
    }

    #[test]
    fn check_fails_when_cost_exceeds_limit() {
        let mut tracker = BudgetTracker::new(test_config(), 0, 0);
        tracker.cost_cents = 95;

        let result = tracker.check(&ActionCost {
            llm_calls: 0,
            tool_invocations: 0,
            tokens: 0,
            cost_cents: 6,
        });

        assert_eq!(
            result,
            Err(BudgetExceeded {
                resource: BudgetResource::Cost,
                limit: 100,
                current: 95,
                requested: 6,
            })
        );
    }

    #[test]
    fn check_fails_when_recursion_depth_reaches_limit() {
        let tracker = BudgetTracker::new(test_config(), 0, 4);

        let result = tracker.check(&ActionCost::default());

        assert_eq!(
            result,
            Err(BudgetExceeded {
                resource: BudgetResource::RecursionDepth,
                limit: 4,
                current: 4,
                requested: 1,
            })
        );
    }

    #[test]
    fn record_updates_consumption() {
        let mut tracker = BudgetTracker::new(test_config(), 0, 0);

        tracker.record(&ActionCost {
            llm_calls: 1,
            tool_invocations: 2,
            tokens: 300,
            cost_cents: 3,
        });

        tracker.record(&ActionCost {
            llm_calls: 2,
            tool_invocations: 1,
            tokens: 200,
            cost_cents: 2,
        });

        assert_eq!(tracker.llm_calls, 3);
        assert_eq!(tracker.tool_invocations, 3);
        assert_eq!(tracker.tokens_used, 500);
        assert_eq!(tracker.cost_cents, 5);
    }

    #[test]
    fn absorb_child_usage_rolls_up_consumption() {
        let mut parent = BudgetTracker::new(test_config(), 0, 0);
        let mut child = BudgetTracker::new(test_config(), 0, 1);
        child.record(&ActionCost {
            llm_calls: 2,
            tool_invocations: 3,
            tokens: 450,
            cost_cents: 8,
        });

        parent.absorb_child_usage(&child);

        assert_eq!(parent.llm_calls_used(), 2);
        assert_eq!(parent.tool_invocations_used(), 3);
        assert_eq!(parent.tokens_used(), 450);
        assert_eq!(parent.cost_cents_used(), 8);
    }

    #[test]
    fn reset_clears_usage_and_updates_start_time() {
        let mut tracker = BudgetTracker::new(test_config(), 1_000, 2);
        tracker.record(&ActionCost {
            llm_calls: 3,
            tool_invocations: 4,
            tokens: 1_200,
            cost_cents: 9,
        });

        tracker.reset(9_999);

        assert_eq!(tracker.llm_calls_used(), 0);
        assert_eq!(tracker.tool_invocations_used(), 0);
        assert_eq!(tracker.tokens_used(), 0);
        assert_eq!(tracker.cost_cents_used(), 0);
        assert_eq!(tracker.start_time_ms, 9_999);
        assert_eq!(tracker.depth, 2);
    }

    #[test]
    fn usage_accessors_return_recorded_values() {
        let mut tracker = BudgetTracker::new(test_config(), 0, 0);
        tracker.record(&ActionCost {
            llm_calls: 5,
            tool_invocations: 7,
            tokens: 3_000,
            cost_cents: 42,
        });

        assert_eq!(tracker.llm_calls_used(), 5);
        assert_eq!(tracker.tool_invocations_used(), 7);
        assert_eq!(tracker.cost_cents_used(), 42);
    }

    #[test]
    fn remaining_reflects_consumed_resources() {
        let mut tracker = BudgetTracker::new(test_config(), 0, 0);
        tracker.record(&ActionCost {
            llm_calls: 4,
            tool_invocations: 7,
            tokens: 2_500,
            cost_cents: 55,
        });

        assert_eq!(
            tracker.remaining(0),
            BudgetRemaining {
                llm_calls: 6,
                tool_invocations: 13,
                tokens: 7_500,
                cost_cents: 45,
                wall_time_ms: 10_000,
            }
        );
    }

    #[test]
    fn remaining_wall_time_reflects_elapsed_time() {
        let tracker = BudgetTracker::new(test_config(), 1_000, 0);

        assert_eq!(tracker.remaining(6_000).wall_time_ms, 5_000);
        assert_eq!(tracker.remaining(20_000).wall_time_ms, 0);

        // Saturating subtraction protects against clock skew/backward values.
        assert_eq!(tracker.remaining(999).wall_time_ms, 10_000);
    }

    #[test]
    fn wall_time_exceeded_uses_elapsed_time() {
        let tracker = BudgetTracker::new(test_config(), 1_000, 0);

        assert!(!tracker.wall_time_exceeded(10_999));
        assert!(!tracker.wall_time_exceeded(11_000));
        assert!(tracker.wall_time_exceeded(11_001));

        // Saturating subtraction protects against clock skew/backward values.
        assert!(!tracker.wall_time_exceeded(999));
    }

    #[test]
    fn check_at_enforces_wall_time_and_resources() {
        let mut tracker = BudgetTracker::new(test_config(), 1_000, 0);
        tracker.llm_calls = 10;

        let resource_result = tracker.check_at(
            1_500,
            &ActionCost {
                llm_calls: 1,
                ..ActionCost::default()
            },
        );
        assert_eq!(
            resource_result,
            Err(BudgetExceeded {
                resource: BudgetResource::LlmCalls,
                limit: 10,
                current: 10,
                requested: 1,
            })
        );

        let wall_time_result = tracker.check_at(11_001, &ActionCost::default());
        assert_eq!(
            wall_time_result,
            Err(BudgetExceeded {
                resource: BudgetResource::WallTime,
                limit: 10_000,
                current: 10_001,
                requested: 0,
            })
        );
    }

    #[test]
    fn child_depth_increments() {
        let tracker = BudgetTracker::new(test_config(), 1_000, 2);
        assert_eq!(tracker.child_depth(), 3);
    }

    #[test]
    fn record_uses_saturating_add_for_all_resources() {
        let mut tracker = BudgetTracker::new(test_config(), 0, 0);
        tracker.llm_calls = u32::MAX - 1;
        tracker.tool_invocations = u32::MAX - 1;
        tracker.tokens_used = u64::MAX - 1;
        tracker.cost_cents = u64::MAX - 1;

        tracker.record(&ActionCost {
            llm_calls: 10,
            tool_invocations: 10,
            tokens: 10,
            cost_cents: 10,
        });

        assert_eq!(tracker.llm_calls, u32::MAX);
        assert_eq!(tracker.tool_invocations, u32::MAX);
        assert_eq!(tracker.tokens_used, u64::MAX);
        assert_eq!(tracker.cost_cents, u64::MAX);
    }

    #[test]
    fn config_constructors_behave_as_expected() {
        let default = BudgetConfig::default();
        let conservative = BudgetConfig::conservative();
        let unlimited = BudgetConfig::unlimited();

        assert!(default.max_llm_calls > conservative.max_llm_calls);
        assert!(default.max_tool_invocations > conservative.max_tool_invocations);
        assert!(default.max_tokens > conservative.max_tokens);
        assert!(default.max_cost_cents > conservative.max_cost_cents);
        assert!(default.max_wall_time_ms > conservative.max_wall_time_ms);
        assert!(default.max_recursion_depth > conservative.max_recursion_depth);

        assert_eq!(default.decompose_depth_mode, DepthMode::Adaptive);
        assert_eq!(conservative.decompose_depth_mode, DepthMode::Adaptive);
        assert_eq!(unlimited.decompose_depth_mode, DepthMode::Adaptive);
        assert_eq!(
            default.max_tool_retries,
            legacy_retries_from_consecutive_failures(default.max_consecutive_failures)
        );

        assert_eq!(unlimited.max_llm_calls, u32::MAX);
        assert_eq!(unlimited.max_tool_invocations, u32::MAX);
        assert_eq!(unlimited.max_tokens, u64::MAX);
        assert_eq!(unlimited.max_cost_cents, u64::MAX);
        assert_eq!(unlimited.max_wall_time_ms, u64::MAX);
        assert_eq!(unlimited.max_recursion_depth, u32::MAX);
    }

    #[test]
    fn budget_config_permissive_uses_default_limits_with_permissive_retry_policy() {
        let retry_policy = RetryPolicyConfig::permissive();
        let expected = BudgetConfig {
            max_consecutive_failures: retry_policy.max_consecutive_failures,
            max_cycle_failures: retry_policy.max_cycle_failures,
            max_no_progress: retry_policy.max_no_progress,
            max_tool_retries: legacy_retries_from_consecutive_failures(
                retry_policy.max_consecutive_failures,
            ),
            ..BudgetConfig::default()
        };

        assert_eq!(BudgetConfig::permissive(), expected);
    }

    #[test]
    fn budget_config_deserialization_prefers_max_tool_retries_over_consecutive_failures() {
        let json = r#"{
            "max_llm_calls": 7,
            "max_tool_invocations": 9,
            "max_tokens": 1234,
            "max_cost_cents": 55,
            "max_wall_time_ms": 123456,
            "max_recursion_depth": 6,
            "max_consecutive_failures": 99,
            "max_tool_retries": 4
        }"#;
        let config: BudgetConfig = serde_json::from_str(json).unwrap();
        let expected = BudgetConfig {
            max_llm_calls: 7,
            max_tool_invocations: 9,
            max_tokens: 1_234,
            max_cost_cents: 55,
            max_wall_time_ms: 123_456,
            max_recursion_depth: 6,
            max_consecutive_failures: max_consecutive_failures_from_legacy_retries(4),
            max_tool_retries: 4,
            ..BudgetConfig::default()
        };

        assert_eq!(config, expected);
    }

    #[test]
    fn estimate_cost_covers_intended_action_variants() {
        let tap = IntendedAction::Tap {
            target: "button-send".to_owned(),
            fallback: None,
        };
        assert_eq!(
            estimate_cost(&tap),
            ActionCost {
                llm_calls: 0,
                tool_invocations: 1,
                tokens: 0,
                cost_cents: 1,
            }
        );

        let respond = IntendedAction::Respond {
            text: "Done!".to_owned(),
        };
        assert_eq!(
            estimate_cost(&respond),
            ActionCost {
                llm_calls: 1,
                tool_invocations: 0,
                tokens: 1_000,
                cost_cents: 2,
            }
        );

        let mut params = HashMap::new();
        params.insert("query".to_owned(), "weather".to_owned());
        let delegate = IntendedAction::Delegate {
            skill_id: "weather".to_owned(),
            params,
        };
        assert_eq!(
            estimate_cost(&delegate),
            ActionCost {
                llm_calls: 1,
                tool_invocations: 1,
                tokens: 1_000,
                cost_cents: 3,
            }
        );

        let composite = IntendedAction::Composite(vec![tap, respond, delegate]);
        assert_eq!(
            estimate_cost(&composite),
            ActionCost {
                llm_calls: 2,
                tool_invocations: 2,
                tokens: 2_000,
                cost_cents: 6,
            }
        );
    }

    // ----- BudgetAllocator unit tests (13) -----

    #[test]
    fn allocate_reserves_parent_continuation_budget() {
        let tracker = BudgetTracker::new(allocation_config(), 0, 0);
        let allocator = BudgetAllocator::new();
        let sub_goals = vec![sub_goal("one", &[], Some(ComplexityHint::Moderate))];

        let plan = allocator.allocate(&tracker, &sub_goals, AllocationMode::Sequential, 0);

        assert_eq!(plan.parent_continuation_budget.max_llm_calls, 10);
        assert_eq!(plan.parent_continuation_budget.max_tool_invocations, 10);
        assert_eq!(plan.parent_continuation_budget.max_tokens, 2_000);
        assert_eq!(plan.parent_continuation_budget.max_cost_cents, 20);
        assert_eq!(plan.parent_continuation_budget.max_wall_time_ms, 10_000);
        assert_eq!(plan.sub_goal_budgets[0].max_llm_calls, 90);
    }

    #[test]
    fn allocate_distributes_by_complexity_weight() {
        let mut config = allocation_config();
        config.max_llm_calls = 50;
        config.max_tool_invocations = 50;
        let tracker = BudgetTracker::new(config, 0, 0);
        let allocator = BudgetAllocator {
            parent_continuation_fraction: 0.0,
            floor: zero_floor(),
        };
        let sub_goals = vec![
            sub_goal("tiny", &[], Some(ComplexityHint::Trivial)),
            sub_goal("big", &[], Some(ComplexityHint::Complex)),
        ];

        let plan = allocator.allocate(&tracker, &sub_goals, AllocationMode::Concurrent, 0);

        assert_eq!(plan.sub_goal_budgets[0].max_llm_calls, 10);
        assert_eq!(plan.sub_goal_budgets[1].max_llm_calls, 40);
        assert_eq!(plan.sub_goal_budgets[0].max_tool_invocations, 10);
        assert_eq!(plan.sub_goal_budgets[1].max_tool_invocations, 40);
    }

    #[test]
    fn allocate_integer_rounding_conserves_resource_totals() {
        let config = BudgetConfig {
            max_llm_calls: 11,
            max_tool_invocations: 13,
            max_tokens: 10_003,
            max_cost_cents: 17,
            max_wall_time_ms: 9_999,
            max_recursion_depth: 5,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        };
        let tracker = BudgetTracker::new(config.clone(), 0, 0);
        let allocator = BudgetAllocator {
            parent_continuation_fraction: 0.0,
            floor: zero_floor(),
        };
        let sub_goals = vec![
            sub_goal("a", &[], Some(ComplexityHint::Trivial)),
            sub_goal("b", &[], Some(ComplexityHint::Moderate)),
            sub_goal("c", &[], Some(ComplexityHint::Complex)),
        ];

        let plan = allocator.allocate(&tracker, &sub_goals, AllocationMode::Sequential, 0);
        let remaining = tracker.remaining(0);

        assert_eq!(sum_llm_calls(&plan.sub_goal_budgets), remaining.llm_calls);
        assert_eq!(
            sum_tool_calls(&plan.sub_goal_budgets),
            remaining.tool_invocations
        );
        assert_eq!(sum_tokens(&plan.sub_goal_budgets), remaining.tokens);
        assert_eq!(sum_cost_cents(&plan.sub_goal_budgets), remaining.cost_cents);
        assert_eq!(
            sum_wall_time_ms(&plan.sub_goal_budgets),
            remaining.wall_time_ms
        );
    }

    #[test]
    fn allocate_integer_rounding_is_deterministic_for_ties() {
        let config = BudgetConfig {
            max_llm_calls: 5,
            max_tool_invocations: 5,
            max_tokens: 5_000,
            max_cost_cents: 5,
            max_wall_time_ms: 5_000,
            max_recursion_depth: 5,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        };
        let tracker = BudgetTracker::new(config, 0, 0);
        let allocator = BudgetAllocator {
            parent_continuation_fraction: 0.0,
            floor: zero_floor(),
        };
        let sub_goals = vec![
            sub_goal("one", &[], Some(ComplexityHint::Moderate)),
            sub_goal("two", &[], Some(ComplexityHint::Moderate)),
            sub_goal("three", &[], Some(ComplexityHint::Moderate)),
        ];

        let first = allocator.allocate(&tracker, &sub_goals, AllocationMode::Concurrent, 0);
        let second = allocator.allocate(&tracker, &sub_goals, AllocationMode::Concurrent, 0);

        assert_eq!(first.sub_goal_budgets[0].max_llm_calls, 2);
        assert_eq!(first.sub_goal_budgets[1].max_llm_calls, 2);
        assert_eq!(first.sub_goal_budgets[2].max_llm_calls, 1);
        assert_eq!(first, second);
    }

    #[test]
    fn distribute_remainders_handles_large_leftover_without_linear_work() {
        let mut ranking = vec![(0, 1, 0), (1, 1, 0), (2, 1, 0)];

        let allocations = distribute_remainders(vec![0, 0, 0], &mut ranking, 1_000_000_001);

        assert_eq!(allocations, vec![333_333_334, 333_333_334, 333_333_333]);
        assert_eq!(allocations.iter().sum::<u64>(), 1_000_000_001);
    }

    #[test]
    fn build_skip_mask_marks_only_in_range_indices() {
        let mask = build_skip_mask(3, &[0, 2, 99]);

        assert_eq!(mask, vec![true, false, true]);
    }

    #[test]
    fn allocate_skips_sub_goals_below_floor() {
        let config = BudgetConfig {
            max_llm_calls: 8,
            max_tool_invocations: 8,
            max_tokens: 3_000,
            max_cost_cents: 12,
            max_wall_time_ms: 12_000,
            max_recursion_depth: 5,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        };
        let tracker = BudgetTracker::new(config, 0, 0);
        let allocator = BudgetAllocator::new();
        let sub_goals = vec![
            sub_goal("short", &[], Some(ComplexityHint::Trivial)),
            sub_goal("also short", &[], Some(ComplexityHint::Trivial)),
            sub_goal("heavy", &[], Some(ComplexityHint::Complex)),
        ];

        let plan = allocator.allocate(&tracker, &sub_goals, AllocationMode::Sequential, 0);

        assert!(plan.skipped_indices.contains(&0));
        assert!(plan.skipped_indices.contains(&1));
        assert!(!plan.skipped_indices.contains(&2));
    }

    #[test]
    fn allocate_redistributes_skipped_budget() {
        let config = BudgetConfig {
            max_llm_calls: 8,
            max_tool_invocations: 8,
            max_tokens: 3_000,
            max_cost_cents: 12,
            max_wall_time_ms: 12_000,
            max_recursion_depth: 5,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        };
        let tracker = BudgetTracker::new(config, 0, 0);
        let allocator = BudgetAllocator {
            parent_continuation_fraction: 0.0,
            floor: BudgetFloor::default(),
        };
        let sub_goals = vec![
            sub_goal("short", &[], Some(ComplexityHint::Trivial)),
            sub_goal("also short", &[], Some(ComplexityHint::Trivial)),
            sub_goal("heavy", &[], Some(ComplexityHint::Complex)),
        ];

        let plan = allocator.allocate(&tracker, &sub_goals, AllocationMode::Concurrent, 0);

        assert_eq!(plan.sub_goal_budgets[0].max_llm_calls, 0);
        assert_eq!(plan.sub_goal_budgets[1].max_llm_calls, 0);
        assert_eq!(plan.sub_goal_budgets[2].max_llm_calls, 8);
        assert_eq!(plan.sub_goal_budgets[2].max_tokens, 3_000);
        assert_eq!(plan.sub_goal_budgets[2].max_cost_cents, 12);
    }

    #[test]
    fn allocate_single_sub_goal_gets_full_distributable() {
        let tracker = BudgetTracker::new(allocation_config(), 0, 0);
        let allocator = BudgetAllocator::new();
        let sub_goals = vec![sub_goal("only", &[], None)];

        let plan = allocator.allocate(&tracker, &sub_goals, AllocationMode::Sequential, 0);

        assert_eq!(plan.sub_goal_budgets.len(), 1);
        assert_eq!(plan.sub_goal_budgets[0].max_llm_calls, 90);
        assert_eq!(plan.sub_goal_budgets[0].max_tool_invocations, 90);
        assert_eq!(plan.sub_goal_budgets[0].max_tokens, 18_000);
    }

    #[test]
    fn allocate_all_sub_goals_below_floor_returns_all_skipped() {
        let config = BudgetConfig {
            max_llm_calls: 2,
            max_tool_invocations: 2,
            max_tokens: 500,
            max_cost_cents: 2,
            max_wall_time_ms: 1_000,
            max_recursion_depth: 5,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        };
        let tracker = BudgetTracker::new(config, 0, 0);
        let allocator = BudgetAllocator::new();
        let sub_goals = vec![sub_goal("a", &[], None), sub_goal("b", &[], None)];

        let plan = allocator.allocate(&tracker, &sub_goals, AllocationMode::Sequential, 0);

        assert_eq!(plan.skipped_indices, vec![0, 1]);
        assert!(plan
            .sub_goal_budgets
            .iter()
            .all(|budget| budget.max_llm_calls == 0));
    }

    #[test]
    fn allocate_mode_sequential_and_concurrent_match_in_v1() {
        let tracker = BudgetTracker::new(allocation_config(), 0, 0);
        let allocator = BudgetAllocator {
            parent_continuation_fraction: 0.0,
            floor: zero_floor(),
        };
        let sub_goals = vec![
            sub_goal("a", &[], Some(ComplexityHint::Trivial)),
            sub_goal("b", &[], Some(ComplexityHint::Moderate)),
            sub_goal("c", &[], Some(ComplexityHint::Complex)),
        ];

        let sequential = allocator.allocate(&tracker, &sub_goals, AllocationMode::Sequential, 0);
        let concurrent = allocator.allocate(&tracker, &sub_goals, AllocationMode::Concurrent, 0);

        assert_eq!(sequential, concurrent);
    }

    #[test]
    fn allocate_zero_sub_goals_returns_empty_plan() {
        let tracker = BudgetTracker::new(allocation_config(), 0, 0);
        let allocator = BudgetAllocator::new();

        let plan = allocator.allocate(&tracker, &[], AllocationMode::Sequential, 0);

        assert!(plan.sub_goal_budgets.is_empty());
        assert!(plan.skipped_indices.is_empty());
    }

    #[test]
    fn allocate_zero_remaining_is_infallible() {
        let config = BudgetConfig {
            max_llm_calls: 0,
            max_tool_invocations: 0,
            max_tokens: 0,
            max_cost_cents: 0,
            max_wall_time_ms: 0,
            max_recursion_depth: 5,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        };
        let tracker = BudgetTracker::new(config, 0, 0);
        let allocator = BudgetAllocator::new();
        let goals = vec![sub_goal("a", &[], None), sub_goal("b", &[], None)];

        let plan = allocator.allocate(&tracker, &goals, AllocationMode::Concurrent, 0);

        assert_eq!(plan.sub_goal_budgets.len(), 2);
        assert_eq!(plan.skipped_indices, vec![0, 1]);
        assert_eq!(sum_llm_calls(&plan.sub_goal_budgets), 0);
    }

    #[test]
    fn parent_continuation_budget_clamps_to_remaining() {
        let config = BudgetConfig {
            max_llm_calls: 3,
            max_tool_invocations: 3,
            max_tokens: 30,
            max_cost_cents: 3,
            max_wall_time_ms: 30,
            max_recursion_depth: 5,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        };
        let tracker = BudgetTracker::new(config, 0, 0);
        let allocator = BudgetAllocator {
            parent_continuation_fraction: 5.0,
            floor: zero_floor(),
        };
        let goals = vec![sub_goal("a", &[], None)];

        let plan = allocator.allocate(&tracker, &goals, AllocationMode::Sequential, 0);

        assert_eq!(plan.parent_continuation_budget.max_llm_calls, 3);
        assert_eq!(plan.parent_continuation_budget.max_tool_invocations, 3);
        assert_eq!(plan.parent_continuation_budget.max_tokens, 30);
        assert_eq!(plan.sub_goal_budgets[0].max_llm_calls, 0);
    }

    #[test]
    fn complexity_hint_overrides_heuristic() {
        let config = BudgetConfig {
            max_llm_calls: 10,
            max_tool_invocations: 10,
            max_tokens: 10_000,
            max_cost_cents: 20,
            max_wall_time_ms: 10_000,
            max_recursion_depth: 5,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            termination: TerminationConfig::default(),
        };
        let tracker = BudgetTracker::new(config, 0, 0);
        let allocator = BudgetAllocator {
            parent_continuation_fraction: 0.0,
            floor: zero_floor(),
        };
        let goals = vec![
            sub_goal("tiny task", &[], Some(ComplexityHint::Complex)),
            sub_goal("tiny task", &[], None),
        ];

        let plan = allocator.allocate(&tracker, &goals, AllocationMode::Sequential, 0);

        assert!(plan.sub_goal_budgets[0].max_llm_calls > plan.sub_goal_budgets[1].max_llm_calls);
    }

    // ----- estimate_complexity unit tests (10) -----

    #[test]
    fn trivial_for_short_description_no_tools() {
        let goal = sub_goal("quick status check", &[], None);
        assert_eq!(estimate_complexity(&goal), ComplexityHint::Trivial);
    }

    #[test]
    fn complex_keyword_preempts_trivial_when_conditions_overlap() {
        let goal = sub_goal("refactor", &[], None);
        assert_eq!(estimate_complexity(&goal), ComplexityHint::Complex);
    }

    #[test]
    fn boundary_50_chars_is_moderate_not_trivial() {
        let goal = sub_goal(&"a".repeat(50), &[], None);
        assert_eq!(estimate_complexity(&goal), ComplexityHint::Moderate);
    }

    #[test]
    fn exactly_2_tools_is_moderate() {
        let goal = sub_goal("inspect", &["one", "two"], None);
        assert_eq!(estimate_complexity(&goal), ComplexityHint::Moderate);
    }

    #[test]
    fn exactly_3_tools_is_complex() {
        let goal = sub_goal("inspect", &["one", "two", "three"], None);
        assert_eq!(estimate_complexity(&goal), ComplexityHint::Complex);
    }

    #[test]
    fn moderate_for_medium_description_few_tools() {
        let goal = sub_goal(&"m".repeat(120), &["tool-a"], None);
        assert_eq!(estimate_complexity(&goal), ComplexityHint::Moderate);
    }

    #[test]
    fn complex_for_long_description() {
        let goal = sub_goal(&"c".repeat(201), &[], None);
        assert_eq!(estimate_complexity(&goal), ComplexityHint::Complex);
    }

    #[test]
    fn keyword_matching_is_case_insensitive() {
        let goal = sub_goal("Please ReFaCtOr this module", &[], None);
        assert_eq!(estimate_complexity(&goal), ComplexityHint::Complex);
    }

    #[test]
    fn keyword_matching_uses_word_boundaries() {
        let no_match = sub_goal("implementation details", &["tool"], None);
        let match_word = sub_goal("implement api", &[], None);

        assert_eq!(estimate_complexity(&no_match), ComplexityHint::Moderate);
        assert_eq!(estimate_complexity(&match_word), ComplexityHint::Complex);
    }

    #[test]
    fn complex_for_exhaustive_keyword_list() {
        for keyword in COMPLEXITY_KEYWORDS {
            let goal = sub_goal(keyword, &[], None);
            assert_eq!(estimate_complexity(&goal), ComplexityHint::Complex);
        }
    }

    // ----- Dynamic depth cap tests (budget-side) -----

    #[test]
    fn effective_max_depth_threshold_mapping_is_stable() {
        assert_eq!(
            effective_max_depth(&BudgetRemaining {
                llm_calls: 33,
                ..BudgetRemaining::default()
            }),
            3
        );
        assert_eq!(
            effective_max_depth(&BudgetRemaining {
                llm_calls: 17,
                ..BudgetRemaining::default()
            }),
            2
        );
        assert_eq!(
            effective_max_depth(&BudgetRemaining {
                llm_calls: 7,
                ..BudgetRemaining::default()
            }),
            1
        );
        assert_eq!(
            effective_max_depth(&BudgetRemaining {
                llm_calls: 6,
                ..BudgetRemaining::default()
            }),
            0
        );
    }

    #[test]
    fn depth_mode_default_is_adaptive() {
        assert_eq!(DepthMode::default(), DepthMode::Adaptive);
        assert_eq!(
            BudgetConfig::default().decompose_depth_mode,
            DepthMode::Adaptive
        );
    }

    #[test]
    fn depth_mode_static_ignores_budget_derived_depth() {
        let config = BudgetConfig {
            max_recursion_depth: 8,
            decompose_depth_mode: DepthMode::Static,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            ..test_config()
        };
        let remaining = BudgetRemaining {
            llm_calls: 1,
            ..BudgetRemaining::default()
        };

        assert_eq!(effective_max_depth(&remaining), 0);
        assert_eq!(resolved_depth_cap(&config, &remaining), 8);
    }

    #[test]
    fn depth_mode_adaptive_uses_min_of_static_and_effective_cap() {
        let config = BudgetConfig {
            max_recursion_depth: 2,
            decompose_depth_mode: DepthMode::Adaptive,
            soft_ceiling_percent: DEFAULT_SOFT_CEILING_PERCENT,
            max_fan_out: DEFAULT_MAX_FAN_OUT,
            max_tool_result_bytes: DEFAULT_MAX_TOOL_RESULT_BYTES,
            max_aggregate_result_bytes: DEFAULT_MAX_AGGREGATE_RESULT_BYTES,
            max_synthesis_tokens: DEFAULT_MAX_SYNTHESIS_TOKENS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            max_cycle_failures: DEFAULT_MAX_CYCLE_FAILURES,
            max_no_progress: DEFAULT_MAX_NO_PROGRESS,
            max_tool_retries: DEFAULT_MAX_TOOL_RETRIES,
            ..test_config()
        };

        let high_budget = BudgetRemaining {
            llm_calls: 40,
            ..BudgetRemaining::default()
        };
        let low_budget = BudgetRemaining {
            llm_calls: 8,
            ..BudgetRemaining::default()
        };

        assert_eq!(effective_max_depth(&high_budget), 3);
        assert_eq!(resolved_depth_cap(&config, &high_budget), 2);
        assert_eq!(effective_max_depth(&low_budget), 1);
        assert_eq!(resolved_depth_cap(&config, &low_budget), 1);
    }

    // --- Loop resilience: budget soft-ceiling tests ---

    /// Test 1: Agent at 79% cost budget → `state()` returns `Normal`.
    #[test]
    fn budget_state_normal_at_79_percent_cost() {
        let config = BudgetConfig {
            max_cost_cents: 100,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);
        // Record 79 cents out of 100
        tracker.record(&ActionCost {
            cost_cents: 79,
            ..ActionCost::default()
        });
        assert_eq!(tracker.state(), BudgetState::Normal);
    }

    /// Test 2: Agent at 81% cost budget → `state()` returns `Low`.
    #[test]
    fn budget_state_low_at_81_percent_cost() {
        let config = BudgetConfig {
            max_cost_cents: 100,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);
        tracker.record(&ActionCost {
            cost_cents: 81,
            ..ActionCost::default()
        });
        assert_eq!(tracker.state(), BudgetState::Low);
    }

    /// Test 3: Agent at 81% LLM calls (cost still Normal) → `state()` returns `Low`.
    #[test]
    fn budget_state_low_at_81_percent_llm_calls() {
        let config = BudgetConfig {
            max_llm_calls: 100,
            max_cost_cents: 10_000, // Cost well below ceiling
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);
        // Record 81 LLM calls out of 100
        for _ in 0..81 {
            tracker.record(&ActionCost {
                llm_calls: 1,
                cost_cents: 1,
                ..ActionCost::default()
            });
        }
        assert_eq!(tracker.state(), BudgetState::Low);
    }

    /// Test 6 (partial — state monotonicity check): Low stays Low even if we
    /// don't record more cost. (Monotonicity within run_cycle is enforced by
    /// the fact that record() only adds and reset() is only called in
    /// prepare_cycle().)
    #[test]
    fn budget_state_stays_low_once_crossed() {
        let config = BudgetConfig {
            max_cost_cents: 100,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);
        tracker.record(&ActionCost {
            cost_cents: 81,
            ..ActionCost::default()
        });
        assert_eq!(tracker.state(), BudgetState::Low);
        // No additional recording — still Low
        assert_eq!(tracker.state(), BudgetState::Low);
    }

    /// `state()` returns Normal after reset().
    #[test]
    fn budget_state_normal_after_reset() {
        let config = BudgetConfig {
            max_cost_cents: 100,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);
        tracker.record(&ActionCost {
            cost_cents: 81,
            ..ActionCost::default()
        });
        assert_eq!(tracker.state(), BudgetState::Low);
        tracker.reset(1);
        assert_eq!(tracker.state(), BudgetState::Normal);
    }

    /// Stronger monotonicity test: records cost through multiple thresholds.
    /// 50% → Normal, 81% → Low, stays Low without additional recording.
    #[test]
    fn budget_state_monotonicity_through_thresholds() {
        let config = BudgetConfig {
            max_cost_cents: 100,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);

        // At 50% → Normal
        tracker.record(&ActionCost {
            cost_cents: 50,
            ..ActionCost::default()
        });
        assert_eq!(
            tracker.state(),
            BudgetState::Normal,
            "50% cost should be Normal"
        );

        // Push to 81% → Low
        tracker.record(&ActionCost {
            cost_cents: 31,
            ..ActionCost::default()
        });
        assert_eq!(tracker.state(), BudgetState::Low, "81% cost should be Low");

        // No more recording → still Low
        assert_eq!(
            tracker.state(),
            BudgetState::Low,
            "should stay Low without additional recording"
        );
    }

    /// Boundary test: exactly 80/100 = Normal (threshold uses `>` not `>=`),
    /// and 81/100 = Low (just over).
    #[test]
    fn budget_state_boundary_at_exactly_80_percent() {
        let config = BudgetConfig {
            max_cost_cents: 100,
            ..BudgetConfig::default()
        };

        // Exactly at threshold → Normal (exceeds_fraction uses `>`)
        let mut tracker = BudgetTracker::new(config.clone(), 0, 0);
        tracker.record(&ActionCost {
            cost_cents: 80,
            ..ActionCost::default()
        });
        assert_eq!(
            tracker.state(),
            BudgetState::Normal,
            "exactly 80% (at threshold, not over) should be Normal"
        );

        // One cent over → Low
        let mut tracker = BudgetTracker::new(config, 0, 0);
        tracker.record(&ActionCost {
            cost_cents: 81,
            ..ActionCost::default()
        });
        assert_eq!(
            tracker.state(),
            BudgetState::Low,
            "81% (just over threshold) should be Low"
        );
    }

    /// Custom soft_ceiling_percent works.
    #[test]
    fn budget_state_custom_fraction() {
        let config = BudgetConfig {
            max_cost_cents: 100,
            soft_ceiling_percent: 50,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);
        tracker.record(&ActionCost {
            cost_cents: 51,
            ..ActionCost::default()
        });
        assert_eq!(tracker.state(), BudgetState::Low);
    }

    // --- Loop resilience: tool result truncation tests ---

    /// Test 13: 8KB result with 16KB limit → no truncation.
    #[test]
    fn truncate_tool_result_no_op_within_limit() {
        let input = "x".repeat(8_000);
        let result = truncate_tool_result(&input, 16_384);
        assert_eq!(result, input);
    }

    /// Test 14: 32KB result with 16KB limit → truncated to 16KB + marker.
    #[test]
    fn truncate_tool_result_truncates_over_limit() {
        let input = "x".repeat(32_000);
        let result = truncate_tool_result(&input, 16_384);
        assert!(result.len() < input.len());
        assert!(result.starts_with(&"x".repeat(16_384)));
        assert!(result.contains("[truncated"));
    }

    /// Test 15: Marker includes correct byte counts.
    #[test]
    fn truncate_tool_result_marker_has_correct_counts() {
        let input = "a".repeat(20_000);
        let result = truncate_tool_result(&input, 10_000);
        // Remaining = 20000 - 10000 = 10000
        assert!(
            result.contains("10000 bytes omitted"),
            "marker should list remaining bytes: {result}"
        );
        assert!(
            result.contains("20000 total"),
            "marker should list total bytes: {result}"
        );
    }

    /// Test 16: Empty result → no truncation, no marker.
    #[test]
    fn truncate_tool_result_empty_is_noop() {
        let result = truncate_tool_result("", 16_384);
        assert_eq!(result, "");
    }

    /// Cow::Borrowed returned when no truncation needed (avoids allocation).
    #[test]
    fn truncate_tool_result_returns_borrowed_within_limit() {
        let input = "short output";
        let result = truncate_tool_result(input, 16_384);
        assert!(
            matches!(result, Cow::Borrowed(_)),
            "should return Cow::Borrowed when within limit"
        );
    }

    /// Cow::Owned returned when truncation occurs.
    #[test]
    fn truncate_tool_result_returns_owned_when_truncated() {
        let input = "x".repeat(32_000);
        let result = truncate_tool_result(&input, 16_384);
        assert!(
            matches!(result, Cow::Owned(_)),
            "should return Cow::Owned when truncated"
        );
    }

    /// Multi-byte char boundary safety.
    #[test]
    fn truncate_tool_result_respects_char_boundaries() {
        // '€' is 3 bytes in UTF-8
        let input = "€".repeat(10_000);
        let result = truncate_tool_result(&input, 100);
        // Must not panic and must be valid UTF-8
        assert!(!result.is_empty());
        assert!(result.contains("[truncated"));
    }
}

#[cfg(test)]
mod synthesis_context_guard_budget_tests {
    use super::*;

    #[test]
    fn accumulated_result_bytes_triggers_low_state() {
        let config = BudgetConfig {
            max_aggregate_result_bytes: 1_000,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);

        assert_eq!(tracker.state(), BudgetState::Normal);

        tracker.record_result_bytes(500);
        assert_eq!(tracker.state(), BudgetState::Normal);

        tracker.record_result_bytes(501);
        assert_eq!(
            tracker.state(),
            BudgetState::Low,
            "exceeding max_aggregate_result_bytes should trigger Low"
        );
    }

    #[test]
    fn accumulated_result_bytes_reset_on_cycle_reset() {
        let config = BudgetConfig {
            max_aggregate_result_bytes: 1_000,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);

        tracker.record_result_bytes(2_000);
        assert_eq!(tracker.state(), BudgetState::Low);

        tracker.reset(100);
        assert_eq!(tracker.accumulated_result_bytes(), 0);
        assert_eq!(tracker.state(), BudgetState::Normal);
    }

    #[test]
    fn accumulated_result_bytes_uses_saturating_add() {
        let config = BudgetConfig::default();
        let mut tracker = BudgetTracker::new(config, 0, 0);

        tracker.record_result_bytes(usize::MAX);
        tracker.record_result_bytes(1);
        assert_eq!(tracker.accumulated_result_bytes(), usize::MAX);
    }

    #[test]
    fn normal_state_when_under_aggregate_limit() {
        let config = BudgetConfig {
            max_aggregate_result_bytes: 100_000,
            ..BudgetConfig::default()
        };
        let mut tracker = BudgetTracker::new(config, 0, 0);
        tracker.record_result_bytes(50_000);
        assert_eq!(tracker.state(), BudgetState::Normal);
    }
}
