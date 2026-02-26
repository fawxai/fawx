//! Budget accounting and guardrails for the Decide step budget gate.

use crate::types::*;
use serde::{Deserialize, Serialize};

const DEFAULT_LLM_CALL_TOKENS: u64 = 1_000;
const DEFAULT_LLM_CALL_COST_CENTS: u64 = 2;
const DEFAULT_TOOL_INVOCATION_COST_CENTS: u64 = 1;

/// Budget configuration for a single loop invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
}

impl BudgetConfig {
    /// Return a conservative configuration for background/proactive actions.
    pub fn conservative() -> Self {
        Self {
            max_llm_calls: 8,
            max_tool_invocations: 16,
            max_tokens: 25_000,
            max_cost_cents: 100,
            max_wall_time_ms: 2 * 60 * 1000,
            max_recursion_depth: 3,
        }
    }

    /// Return effectively unlimited limits for testing.
    ///
    /// This should not be used in production runtime paths.
    pub fn unlimited() -> Self {
        Self {
            max_llm_calls: u32::MAX,
            max_tool_invocations: u32::MAX,
            max_tokens: u64::MAX,
            max_cost_cents: u64::MAX,
            max_wall_time_ms: u64::MAX,
            max_recursion_depth: u32::MAX,
        }
    }
}

impl Default for BudgetConfig {
    /// Return a generous default for normal user-initiated loops.
    fn default() -> Self {
        Self {
            max_llm_calls: 64,
            max_tool_invocations: 128,
            max_tokens: 250_000,
            max_cost_cents: 500,
            max_wall_time_ms: 15 * 60 * 1000,
            max_recursion_depth: 8,
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
        }
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

    /// Reset consumed budget counters for a fresh cycle starting at `start_time_ms`.
    pub fn reset(&mut self, start_time_ms: u64) {
        self.llm_calls = 0;
        self.tool_invocations = 0;
        self.tokens_used = 0;
        self.cost_cents = 0;
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

    /// Create a child budget config by partitioning remaining resources.
    ///
    /// `fraction` is clamped into `[0.0, 1.0]` and partitioning uses *remaining*
    /// wall-time at `current_time_ms`, not the original configured wall-time.
    pub fn partition_child(&self, fraction: f32, current_time_ms: u64) -> BudgetConfig {
        let bounded_fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };

        let remaining = self.remaining(current_time_ms);

        BudgetConfig {
            max_llm_calls: partition_u32(remaining.llm_calls, bounded_fraction),
            max_tool_invocations: partition_u32(remaining.tool_invocations, bounded_fraction),
            max_tokens: partition_u64(remaining.tokens, bounded_fraction),
            max_cost_cents: partition_u64(remaining.cost_cents, bounded_fraction),
            max_wall_time_ms: partition_u64(remaining.wall_time_ms, bounded_fraction),
            max_recursion_depth: self.config.max_recursion_depth,
        }
    }

    /// Depth to assign to a child tracker spawned from this tracker.
    pub fn child_depth(&self) -> u32 {
        self.depth.saturating_add(1)
    }

    /// Create a child tracker with partitioned config and incremented depth.
    pub fn child_tracker(&self, fraction: f32, current_time_ms: u64) -> Self {
        Self::new(
            self.partition_child(fraction, current_time_ms),
            current_time_ms,
            self.child_depth(),
        )
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

fn partition_u64(value: u64, fraction: f32) -> u64 {
    if value == 0 {
        return 0;
    }

    let scaled = (value as f64 * f64::from(fraction)).floor() as u64;
    scaled.max(1)
}

fn partition_u32(value: u32, fraction: f32) -> u32 {
    if value == 0 {
        return 0;
    }

    let scaled = (f64::from(value) * f64::from(fraction)).floor() as u32;
    scaled.max(1)
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
    fn partition_child_apportions_remaining_budget() {
        let mut tracker = BudgetTracker::new(test_config(), 1_000, 0);
        tracker.record(&ActionCost {
            llm_calls: 2,
            tool_invocations: 4,
            tokens: 2_000,
            cost_cents: 20,
        });

        let child = tracker.partition_child(0.5, 6_000);

        assert_eq!(
            child,
            BudgetConfig {
                max_llm_calls: 4,
                max_tool_invocations: 8,
                max_tokens: 4_000,
                max_cost_cents: 40,
                max_wall_time_ms: 2_500,
                max_recursion_depth: 4,
            }
        );
    }

    #[test]
    fn partition_child_with_zero_fraction_produces_minimum_viable_config() {
        let tracker = BudgetTracker::new(test_config(), 0, 0);

        let child = tracker.partition_child(0.0, 0);

        assert_eq!(child.max_llm_calls, 1);
        assert_eq!(child.max_tool_invocations, 1);
        assert_eq!(child.max_tokens, 1);
        assert_eq!(child.max_cost_cents, 1);
        assert_eq!(child.max_wall_time_ms, 1);
        assert_eq!(child.max_recursion_depth, 4);
    }

    #[test]
    fn partition_child_with_one_fraction_uses_full_parent_remaining() {
        let mut tracker = BudgetTracker::new(test_config(), 1_000, 1);
        tracker.record(&ActionCost {
            llm_calls: 3,
            tool_invocations: 5,
            tokens: 2_500,
            cost_cents: 25,
        });

        let child = tracker.partition_child(1.0, 6_000);

        assert_eq!(child.max_llm_calls, 7);
        assert_eq!(child.max_tool_invocations, 15);
        assert_eq!(child.max_tokens, 7_500);
        assert_eq!(child.max_cost_cents, 75);
        assert_eq!(child.max_wall_time_ms, 5_000);
        assert_eq!(child.max_recursion_depth, 4);
    }

    #[test]
    fn partition_child_clamps_fraction_greater_than_one() {
        let mut tracker = BudgetTracker::new(test_config(), 1_000, 1);
        tracker.record(&ActionCost {
            llm_calls: 3,
            tool_invocations: 5,
            tokens: 2_500,
            cost_cents: 25,
        });

        let child = tracker.partition_child(1.5, 6_000);

        assert_eq!(child.max_llm_calls, 7);
        assert_eq!(child.max_tool_invocations, 15);
        assert_eq!(child.max_tokens, 7_500);
        assert_eq!(child.max_cost_cents, 75);
        assert_eq!(child.max_wall_time_ms, 5_000);
    }

    #[test]
    fn child_tracker_increments_depth() {
        let tracker = BudgetTracker::new(test_config(), 1_000, 2);

        assert_eq!(tracker.child_depth(), 3);

        let child = tracker.child_tracker(0.5, 6_000);
        assert_eq!(child.depth, 3);
        assert_eq!(child.start_time_ms, 6_000);
        assert_eq!(child.config.max_recursion_depth, 4);
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

        assert_eq!(unlimited.max_llm_calls, u32::MAX);
        assert_eq!(unlimited.max_tool_invocations, u32::MAX);
        assert_eq!(unlimited.max_tokens, u64::MAX);
        assert_eq!(unlimited.max_cost_cents, u64::MAX);
        assert_eq!(unlimited.max_wall_time_ms, u64::MAX);
        assert_eq!(unlimited.max_recursion_depth, u32::MAX);
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
}
