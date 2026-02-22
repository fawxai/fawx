# H2.11 Cost Tracking & Budget Limits Spec

Per-task token tracking with user-configurable spending caps.

Objective: The core agent MUST track token/cost totals per task and enforce configured budget limits to prevent runaway spend.

Source: `docs/agentic-loop-v2.md §12.8`

---

## Scope

This spec defines the behavior contract for cost tracking and budget enforcement in `:core`.

This spec is normative for:
- `TaskTokenAccumulator`
- `CostEstimator`
- `BudgetGuard`
- `BudgetStore`
- `PhoneAgentApi` budget integration points

This spec is intentionally non-prescriptive about storage technology, UI surfaces, and provider-specific transport details.

---

## Normative Requirements

1. Per-task accumulation
- The system MUST accumulate token usage across all provider calls in a single task/tool loop.
- The accumulator MUST support repeated updates and resets across tasks.
- Totals MUST be reported in a form that avoids overflow for realistic task lengths.

2. Cost estimation
- The system MUST estimate USD cost from token usage and model pricing.
- Input, output, cache-read, and cache-write token classes MUST be priced independently.
- Unknown or null model IDs MUST fall back to a default pricing entry.

3. Pricing catalog source of truth
- Pricing MUST be loaded from the packaged pricing catalog resource when available.
- If catalog loading fails or is incomplete, the estimator MUST fall back to an internal map that includes a `default` entry.
- The implementation MAY warn when catalog data appears stale; stale warnings MUST NOT block execution.

4. Model ID matching
- Pricing lookup MUST normalize incoming model IDs before lookup.
- Normalization MUST handle common provider prefixes and delimiter variants.
- Lookup MUST attempt progressively less specific candidates before using `default`.

5. Budget enforcement
- Budget checks MUST support daily and monthly limits, plus optional per-task limits.
- Budget checks MUST provide both:
  - Pre-call read-only gating (no spend mutation)
  - Post-estimate spend recording with typed decisions
- Pre-call gating MUST block when current spend is already at or above a configured daily/monthly cap.

6. Concurrency and atomicity
- Spend mutation and limit evaluation MUST be atomic with respect to shared budget state.
- Concurrent task flows MUST NOT bypass configured hard caps due to check/spend races.

7. Missing usage metadata
- If provider usage metadata is absent, the system MUST apply a conservative fallback estimate and progress budget accounting.
- Missing-usage fallback handling MUST still enforce configured limits.

8. Reporting surface
- `PhoneAgentApi` MUST expose the latest per-task cost summary for UI consumption.
- Budget violations MUST surface as typed errors suitable for user-facing handling.

---

## Component Contracts

### TaskTokenAccumulator

Contract:
- Records token usage events for the active task.
- Exposes cumulative totals and call count.
- Supports reset between tasks.
- Is safe for concurrent updates from agent call paths.

Non-contractual details (counter layout, internal storage shape) may change.

### CostEstimator

Contract:
- Estimates per-call and per-task cost from token usage + model pricing.
- Loads pricing from the catalog resource first, then falls back when needed.
- Uses normalized model ID candidate matching before default fallback.

The spec does not mandate specific regex expressions, map literals, or candidate ordering internals beyond the required outcomes above.

### BudgetGuard

Contract:
- Uses `BudgetStore` as the persistence boundary.
- Supports read-only pre-call gating via `checkWouldExceedBudgetWithoutSpendingDecision`.
- Supports atomic spend recording + decision via `trySpendDecision`.
- Supports per-task cap validation against cumulative task spend.
- Emits typed budget outcomes to allow deterministic handling in callers.

The spec does not couple behavior to any concrete storage backend.

### BudgetStore

Contract:
- Provides budget config and persisted spend state in microdollars.
- Provides pending fractional carry support needed by budget math.
- Supports daily/monthly reset operations.

Implementations MAY use any backing store (in-memory, preferences, database, etc.) if they satisfy this contract.

### PhoneAgentApi Integration

Contract:
- Before provider calls, the API MUST:
  1. check per-task cap against task spend-so-far
  2. run read-only daily/monthly precheck via `checkWouldExceedBudgetWithoutSpendingDecision`
- After each call outcome, the API MUST:
  - record usage-derived spend when metadata is present, or
  - record conservative fallback spend when metadata is missing
- Any over-limit decision MUST terminate the call path via budget exception handling.
- `lastTaskCostSummary` MUST reflect the latest accumulated task totals.

---

## Out of Scope

- UI settings and UX copy for budget controls
- Subscription/paywall policy
- Live remote pricing fetch/update workflows
- Selection of concrete persistence technology for `BudgetStore`

---

## Acceptance Criteria

This spec is satisfied when:
- Budget checks cannot be bypassed under concurrent call paths.
- Unknown models consistently use default pricing fallback.
- Catalog resource load/fallback behavior remains deterministic.
- Missing usage metadata still advances spend and enforces limits.
- `PhoneAgentApi` exposes per-task summary and typed budget failures.
